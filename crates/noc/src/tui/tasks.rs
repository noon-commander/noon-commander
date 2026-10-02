//! Background work for the app: listings, new directories, jobs, and one task per connected
//! host that owns its ssh session and SFTP channel.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt as _;
use futures_util::stream::FuturesUnordered;
use noc_ops::{Conflict, CopyOptions, Decision, Endpoint, Event, Outcome, Reporter};
use noc_ssh::askpass::{AskpassEnv, AskpassEvent, AskpassServer};
use noc_ssh::resolve::resolve;
use noc_ssh::version::check_version;
use noc_ssh::{CachedHost, ChannelProcess, Session, SftpChannel, SshError, cleanup_stale};
use noc_vfs::{FileReader as _, LocalFs, Location, Metadata, RemotePath, SftpFs, Vfs, VfsError};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::app::{Effect, Side};
use super::describe;
use super::dialog::{Ask, Reply};
use super::panel::{ListRequest, Listed, Listing};
use super::root;
use crate::context::Context;
use crate::i18n::fl;

/// Bytes of a file the viewer reads.
pub(crate) const VIEW_LIMIT: usize = 16 * 1024 * 1024;

/// How long quitting waits for connections to shut down.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// How long closing an SFTP session may take.
const SFTP_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a failed channel gets to exit and finish its stderr, which says why it failed.
const CHANNEL_EXIT_WAIT: Duration = Duration::from_secs(1);

/// A finished piece of background work.
#[derive(Debug)]
pub(crate) enum Done {
    Listed {
        side: Side,
        generation: u64,
        result: Result<Listed, String>,
    },
    /// A report from the job `id` of [`Effect::Delete`] or [`Effect::Copy`].
    Job { id: u64, event: JobEvent },
    /// The start of the file of [`Effect::Read`], and whether there is more, or why not.
    Read {
        id: u64,
        result: Result<(Vec<u8>, bool), String>,
    },
    /// The directory of [`Effect::CreateDir`] was made, or why not.
    Created {
        side: Side,
        location: Location,
        result: Result<(), String>,
    },
    /// The host of [`Effect::Connect`] is connected; its listings go through `handle`.
    Connected {
        host: String,
        connection: u64,
        handle: HostHandle,
    },
    /// ssh asks something.
    Ask(Ask),
    /// ssh tells something that needs no answer, until [`Done::PromptClosed`].
    Notice {
        id: u64,
        context: String,
        message: String,
    },
    /// ssh no longer waits for the prompt or notice `id`.
    PromptClosed { id: u64 },
    /// `ssh -G` gave the address of a host that [`Effect::Connect`] connects to.
    Resolved { host: String, address: String },
    /// The attempt or connection of [`Effect::Connect`] ended; `reason` is `None` if it was
    /// asked to stop.
    Closed {
        host: String,
        connection: u64,
        reason: Option<String>,
    },
}

/// A report from a job, with its paths as locations and its errors in words.
#[derive(Debug)]
pub(crate) enum JobEvent {
    /// Counting what to do: `items` found so far.
    Scanning { items: u64 },
    /// At `current`, with `done` of `total` entries and `bytes_done` of `bytes_total` bytes
    /// behind it; jobs that move no data count no bytes.
    Progress {
        current: Location,
        done: u64,
        total: u64,
        bytes_done: u64,
        bytes_total: u64,
    },
    /// Something failed at `path`; the job waits for `reply`.
    Failed {
        path: Location,
        error: String,
        reply: oneshot::Sender<Decision>,
    },
    /// The name `target` of a copy is taken; the job waits for `reply`.
    Exists {
        target: Location,
        source_metadata: Metadata,
        target_metadata: Metadata,
        reply: oneshot::Sender<Conflict>,
    },
    /// The job is over; `complete` if it did all it was asked, without skipping or aborting.
    Finished { complete: bool },
}

/// Work for the task of a connected host, for the panel on a side.
#[derive(Debug)]
pub(crate) enum HostRequest {
    List(Side, ListRequest),
    /// Makes the directory at `path`.
    CreateDir(Side, RemotePath),
    /// Runs the delete job `id` on `paths` until `cancel`.
    Delete {
        id: u64,
        paths: Vec<RemotePath>,
        cancel: CancellationToken,
    },
    /// Shares the host's SFTP session, for a job that runs elsewhere, such as a copy between
    /// two hosts.
    Share(oneshot::Sender<Arc<SftpFs>>),
}

/// Passes requests to the task of a connected host.
#[derive(Debug, Clone)]
pub(crate) struct HostHandle(mpsc::UnboundedSender<HostRequest>);

#[cfg(test)]
impl HostHandle {
    /// A handle and what it receives.
    pub(crate) fn channel() -> (Self, mpsc::UnboundedReceiver<HostRequest>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self(sender), receiver)
    }
}

/// Runs [`Effect`]s; their results come back as [`Done`].
pub(crate) struct Tasks {
    context: Arc<Context>,
    done: mpsc::UnboundedSender<Done>,
    /// The askpass bridge, or why there is none.
    askpass: Result<AskpassServer, String>,
    hosts: JoinSet<()>,
    /// Jobs that run here, not in the task of a host.
    jobs: JoinSet<()>,
    /// Taken while the `ssh -G` cache is read, changed, and written.
    cache: Arc<Mutex<()>>,
}

impl Tasks {
    /// Prepares the runtime directory and the askpass bridge, and cleans up after crashed
    /// instances in the background. A failure here only affects connections.
    pub(crate) async fn start(context: Arc<Context>, done: mpsc::UnboundedSender<Done>) -> Self {
        let askpass = prepare_ssh(&context, done.clone()).await;
        if let Err(reason) = &askpass {
            tracing::warn!(%reason, "cannot prepare for ssh connections");
        }
        Self {
            context,
            done,
            askpass,
            hosts: JoinSet::new(),
            jobs: JoinSet::new(),
            cache: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn run(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::List {
                    side,
                    request,
                    host: Some(handle),
                } => {
                    // If the host's task has ended, its `Closed` report is on its way.
                    let _ = handle.0.send(HostRequest::List(side, request));
                }
                Effect::List {
                    side,
                    request,
                    host: None,
                } => self.list(side, request),
                Effect::CreateDir {
                    side,
                    location,
                    host,
                } => self.create_dir(side, location, host),
                Effect::Discard(path) => {
                    tokio::spawn(async move {
                        if let Err(error) = tokio::fs::remove_file(&path).await {
                            tracing::debug!(path = %path.display(), %error, "cannot remove");
                        }
                    });
                }
                Effect::Read {
                    id,
                    location,
                    host,
                    cancel,
                } => self.read(id, location, host, cancel),
                Effect::Delete {
                    id,
                    targets,
                    host,
                    cancel,
                } => self.delete(id, targets, host, cancel),
                Effect::Copy {
                    id,
                    sources,
                    target,
                    hosts,
                    options,
                    cancel,
                } => {
                    let done = self.done.clone();
                    self.reap_jobs();
                    self.jobs.spawn(async move {
                        let job = CopyJob {
                            id,
                            options,
                            cancel,
                            done: &done,
                        };
                        let finished = job.run(sources, target, hosts).await;
                        let _ = done.send(finished);
                    });
                }
                Effect::Connect {
                    host,
                    connection,
                    stop,
                } => {
                    let askpass = match &self.askpass {
                        Ok(server) => Ok(server.env(&host)),
                        Err(reason) => Err(reason.clone()),
                    };
                    self.hosts.spawn(resolve_address(
                        Arc::clone(&self.context),
                        host.clone(),
                        stop.clone(),
                        Arc::clone(&self.cache),
                        self.done.clone(),
                    ));
                    let task = HostTask {
                        context: Arc::clone(&self.context),
                        host,
                        connection,
                        stop,
                        done: self.done.clone(),
                    };
                    self.hosts.spawn(task.run(askpass));
                }
            }
        }
    }

    /// Lists the virtual root or a local directory here.
    fn list(&self, side: Side, request: ListRequest) {
        let context = Arc::clone(&self.context);
        let done = self.done.clone();
        tokio::spawn(async move {
            let generation = request.generation;
            let result = list(context, request.location).await;
            let _ = done.send(Done::Listed {
                side,
                generation,
                result,
            });
        });
    }

    /// Reads the start of a file for the viewer `id`, until `cancel`.
    fn read(
        &self,
        id: u64,
        location: Location,
        host: Option<HostHandle>,
        cancel: CancellationToken,
    ) {
        let done = self.done.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                result = read(location, host) => result,
                () = cancel.cancelled() => return,
            };
            let _ = done.send(Done::Read { id, result });
        });
    }

    /// Makes a local directory here, or passes a remote one to the task of its host.
    fn create_dir(&self, side: Side, location: Location, host: Option<HostHandle>) {
        match (&location, host) {
            (Location::Remote { path, .. }, Some(handle)) => {
                let _ = handle.0.send(HostRequest::CreateDir(side, path.clone()));
            }
            (Location::Local(path), _) => {
                let path = path.clone();
                let done = self.done.clone();
                tokio::spawn(async move {
                    let result = LocalFs
                        .create_dir(&path)
                        .await
                        .map_err(|error| describe::vfs_error(&error));
                    let _ = done.send(Done::Created {
                        side,
                        location,
                        result,
                    });
                });
            }
            (Location::Root | Location::Remote { .. }, _) => {
                let result = Err(fl!("error-connection-closed"));
                let _ = self.done.send(Done::Created {
                    side,
                    location,
                    result,
                });
            }
        }
    }

    /// Runs the delete job `id` on `targets`, all local or all on one host, here or in the task
    /// of their host.
    fn delete(
        &mut self,
        id: u64,
        targets: Vec<Location>,
        host: Option<HostHandle>,
        cancel: CancellationToken,
    ) {
        let mut local = Vec::new();
        let mut remote = Vec::new();
        for target in targets {
            match target {
                Location::Local(path) => local.push(path),
                Location::Remote { path, .. } => remote.push(path),
                Location::Root => {}
            }
        }
        if let Some(handle) = host {
            let request = HostRequest::Delete {
                id,
                paths: remote,
                cancel,
            };
            if handle.0.send(request).is_err() {
                let _ = self.done.send(finished(id, None));
            }
        } else {
            let done = self.done.clone();
            self.reap_jobs();
            self.jobs.spawn(async move {
                let finished = run_delete(&LocalFs, local, cancel, id, &done, Location::Local);
                let _ = done.send(finished.await);
            });
        }
    }

    /// Forgets the jobs that are over.
    fn reap_jobs(&mut self) {
        while self.jobs.try_join_next().is_some() {}
    }

    /// Waits a while for the jobs, which the app has told to stop, to clean up: a copy removes
    /// its unfinished file, through a session that is still there.
    pub(crate) async fn finish_jobs(&mut self) {
        let all = async { while self.jobs.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_TIMEOUT, all).await.is_err() {
            tracing::warn!("some jobs did not stop in time");
        }
    }

    /// Waits a while for the host tasks, which the app has told to stop, to shut down.
    pub(crate) async fn shutdown(mut self) {
        let all = async { while self.hosts.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_TIMEOUT, all).await.is_err() {
            tracing::warn!("some connections did not shut down in time");
        }
    }
}

/// The runtime directory and the askpass bridge, whose prompts go to the app as [`Done`]s.
async fn prepare_ssh(
    context: &Context,
    done: mpsc::UnboundedSender<Done>,
) -> Result<AskpassServer, String> {
    let paths = context.paths.clone();
    tokio::task::spawn_blocking(move || paths.ensure_runtime_dir())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| describe::chain(&error))?;
    let runtime_dir = context.paths.runtime_dir.clone();
    let settings = context.settings.clone();
    tokio::spawn(async move { cleanup_stale(&runtime_dir, &settings).await });
    let program = std::env::current_exe().map_err(|error| describe::chain(&error))?;
    let (server, mut events) = AskpassServer::bind(&context.paths.runtime_dir, program)
        .map_err(|error| describe::chain(&error))?;
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let done_event = match event {
                AskpassEvent::Prompt(prompt) => Done::Ask(Ask {
                    id: prompt.id,
                    context: prompt.context.clone(),
                    message: prompt.message.clone(),
                    kind: prompt.kind,
                    reply: Reply::new(move |answer| match answer {
                        Some(text) => prompt.answer(text),
                        None => prompt.cancel(),
                    }),
                }),
                AskpassEvent::Notice {
                    id,
                    context,
                    message,
                } => Done::Notice {
                    id,
                    context,
                    message,
                },
                AskpassEvent::Closed { id } => Done::PromptClosed { id },
            };
            if done.send(done_event).is_err() {
                break;
            }
        }
    });
    Ok(server)
}

/// Runs `ssh -G` for a host that is being connected to, as the root shows effective addresses
/// only from there: reports the address and stores it in the cache. Failures only go to the
/// log; the connection reports its own.
async fn resolve_address(
    context: Arc<Context>,
    host: String,
    stop: CancellationToken,
    cache: Arc<Mutex<()>>,
    done: mpsc::UnboundedSender<Done>,
) {
    let target = context.target(&host);
    let resolved = tokio::select! {
        resolved = resolve(&context.settings, &target) => resolved,
        () = stop.cancelled() => return,
    };
    let cached = match resolved {
        Ok(resolved) => CachedHost::from(&resolved),
        Err(error) => {
            tracing::debug!(%host, error = %describe::chain(&error), "ssh -G failed");
            return;
        }
    };
    let address = cached.address();
    let _ = done.send(Done::Resolved {
        host: host.clone(),
        address,
    });
    let _guard = cache.lock().await;
    let saved = tokio::task::spawn_blocking(move || root::remember(&context, &host, cached)).await;
    if let Ok(Err(error)) = saved {
        tracing::warn!(%error, "cannot save the ssh -G cache");
    }
}

/// Runs the delete job `id` on `targets` with `vfs`, passing its reports on to `done` with
/// paths made locations by `location`, and returns its end.
async fn run_delete<V: Vfs>(
    vfs: &V,
    targets: Vec<V::Path>,
    cancel: CancellationToken,
    id: u64,
    done: &mpsc::UnboundedSender<Done>,
    location: impl Fn(V::Path) -> Location,
) -> Done {
    let (events, incoming) = mpsc::unbounded_channel();
    let reporter = Reporter::new(events, cancel);
    let work = async move {
        let mut reporter = reporter;
        noc_ops::delete(vfs, targets, &mut reporter).await
    };
    let (outcome, ()) = tokio::join!(work, forward(incoming, id, done, location));
    finished(id, Some(outcome))
}

/// The end of the job `id`, with its outcome if it ran.
fn finished(id: u64, outcome: Option<Outcome>) -> Done {
    let complete = outcome.is_some_and(|outcome| !outcome.aborted && outcome.skipped == 0);
    Done::Job {
        id,
        event: JobEvent::Finished { complete },
    }
}

/// Passes the reports of the job `id` on to `done`, with paths made locations by `location`
/// and errors in words.
async fn forward<P>(
    mut incoming: mpsc::UnboundedReceiver<Event<P>>,
    id: u64,
    done: &mpsc::UnboundedSender<Done>,
    location: impl Fn(P) -> Location,
) {
    while let Some(event) = incoming.recv().await {
        let event = match event {
            Event::Scanning { items } => JobEvent::Scanning { items },
            Event::Progress(progress) => JobEvent::Progress {
                current: location(progress.current),
                done: progress.items_done,
                total: progress.items_total,
                bytes_done: progress.bytes_done,
                bytes_total: progress.bytes_total,
            },
            Event::Failed { path, error, reply } => JobEvent::Failed {
                path: location(path),
                error: describe::vfs_error(&error),
                reply,
            },
            Event::Exists {
                target,
                source_metadata,
                target_metadata,
                reply,
                ..
            } => JobEvent::Exists {
                target: location(target),
                source_metadata,
                target_metadata,
                reply,
            },
        };
        let _ = done.send(Done::Job { id, event });
    }
}

/// The start of the file at `location`, up to [`VIEW_LIMIT`], and whether there is more.
async fn read(location: Location, host: Option<HostHandle>) -> Result<(Vec<u8>, bool), String> {
    let result = match (location, share(host).await) {
        (Location::Local(path), _) => read_start(&LocalFs, &path).await,
        (Location::Remote { path, .. }, Some(Some(fs))) => read_start(&*fs, &path).await,
        _ => return Err(fl!("error-connection-closed")),
    };
    result.map_err(|error| describe::vfs_error(&error))
}

async fn read_start<V: Vfs>(vfs: &V, path: &V::Path) -> Result<(Vec<u8>, bool), VfsError> {
    let mut reader = vfs.open_file(path).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.read().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() >= VIEW_LIMIT {
            let more = bytes.len() > VIEW_LIMIT || reader.read().await?.is_some();
            bytes.truncate(VIEW_LIMIT);
            return Ok((bytes, more));
        }
    }
    Ok((bytes, false))
}

/// The session of a connected host, from its task; `None` if the host is gone.
async fn share(handle: Option<HostHandle>) -> Option<Option<Arc<SftpFs>>> {
    let Some(handle) = handle else {
        return Some(None);
    };
    let (reply, shared) = oneshot::channel();
    handle.0.send(HostRequest::Share(reply)).ok()?;
    shared.await.ok().map(Some)
}

/// How a side of a copy reports its paths.
type Report<'a, P> = &'a (dyn Fn(&P) -> Location + Send + Sync);

/// A copy job, which runs in a task of its own with the sessions of the hosts at its ends.
struct CopyJob<'a> {
    id: u64,
    options: CopyOptions,
    cancel: CancellationToken,
    done: &'a mpsc::UnboundedSender<Done>,
}

impl CopyJob<'_> {
    /// Copies `sources`, all local or all on one host, to `target`, or moves them if the
    /// options remove sources; `hosts` lead to the hosts of the sources and of the target, if
    /// they are remote.
    async fn run(
        self,
        sources: Vec<Location>,
        target: Location,
        (from, to): (Option<HostHandle>, Option<HostHandle>),
    ) -> Done {
        let (Some(from), Some(to)) = (share(from).await, share(to).await) else {
            return finished(self.id, None);
        };
        let mut local: Vec<PathBuf> = Vec::new();
        let mut remote = Vec::new();
        let mut source_host = String::new();
        for source in sources {
            match source {
                Location::Local(path) => local.push(path),
                Location::Remote { host, path } => {
                    source_host = host;
                    remote.push(path);
                }
                Location::Root => {}
            }
        }
        let on_host = |host: String| {
            move |path: &RemotePath| Location::Remote {
                host: host.clone(),
                path: path.clone(),
            }
        };
        let here = |path: &PathBuf| Location::Local(path.clone());
        let moving = self.options.remove_sources;
        match (from, to, target) {
            // Within one file system, a move renames.
            (None, None, Location::Local(target)) if moving => {
                self.move_within(&LocalFs, local, target, &here).await
            }
            (Some(from), Some(_), Location::Remote { host, path })
                if moving && host == source_host =>
            {
                let there = on_host(host);
                self.move_within(&*from, remote, path, &there).await
            }
            (None, None, Location::Local(target)) => {
                self.copy((&LocalFs, local, &here), (&LocalFs, target, &here))
                    .await
            }
            (None, Some(to), Location::Remote { host, path }) => {
                let there = on_host(host);
                self.copy((&LocalFs, local, &here), (&*to, path, &there))
                    .await
            }
            (Some(from), None, Location::Local(target)) => {
                let there = on_host(source_host);
                self.copy((&*from, remote, &there), (&LocalFs, target, &here))
                    .await
            }
            (Some(from), Some(to), Location::Remote { host, path }) => {
                let (source, target) = (on_host(source_host), on_host(host));
                self.copy((&*from, remote, &source), (&*to, path, &target))
                    .await
            }
            _ => finished(self.id, None),
        }
    }

    async fn move_within<V: Vfs>(
        &self,
        vfs: &V,
        sources: Vec<V::Path>,
        target: V::Path,
        report: Report<'_, V::Path>,
    ) -> Done {
        let (events, incoming) = mpsc::unbounded_channel();
        let reporter = Reporter::new(events, self.cancel.clone());
        let options = self.options;
        let work = async move {
            let mut reporter = reporter;
            let side = Endpoint { vfs, report };
            noc_ops::move_within(side, sources, target, options, &mut reporter).await
        };
        let (outcome, ()) = tokio::join!(work, forward(incoming, self.id, self.done, |path| path));
        finished(self.id, Some(outcome))
    }

    async fn copy<A: Vfs, B: Vfs>(
        &self,
        (from, sources, from_report): (&A, Vec<A::Path>, Report<'_, A::Path>),
        (to, target, to_report): (&B, B::Path, Report<'_, B::Path>),
    ) -> Done {
        let (events, incoming) = mpsc::unbounded_channel();
        let reporter = Reporter::new(events, self.cancel.clone());
        let options = self.options;
        let work = async move {
            let mut reporter = reporter;
            let from = Endpoint {
                vfs: from,
                report: from_report,
            };
            let to = Endpoint {
                vfs: to,
                report: to_report,
            };
            noc_ops::copy(from, sources, to, target, options, &mut reporter).await
        };
        let (outcome, ()) = tokio::join!(work, forward(incoming, self.id, self.done, |path| path));
        finished(self.id, Some(outcome))
    }
}

/// Lists the virtual root or a local directory. The root reads the ssh config each time, so a
/// reload shows new hosts.
async fn list(context: Arc<Context>, location: Location) -> Result<Listed, String> {
    let listing = match &location {
        Location::Root => tokio::task::spawn_blocking(move || root::read_hosts(&context))
            .await
            .map(Listing::Root)
            .map_err(|error| error.to_string())?,
        Location::Local(path) => LocalFs
            .list_dir(path)
            .await
            .map(Listing::Dir)
            .map_err(|error| describe::vfs_error(&error))?,
        // The app sends these to the host's task.
        Location::Remote { .. } => return Err(fl!("error-connection-closed")),
    };
    Ok(Listed { location, listing })
}

/// A connection to a host: the session, an SFTP channel, and the ssh process behind it.
struct Connection {
    session: Session,
    fs: SftpFs,
    process: ChannelProcess,
}

/// Connects to one host, serves its listings, and shuts the connection down.
struct HostTask {
    context: Arc<Context>,
    host: String,
    connection: u64,
    stop: CancellationToken,
    done: mpsc::UnboundedSender<Done>,
}

impl HostTask {
    async fn run(self, askpass: Result<AskpassEnv, String>) {
        let reason = match self.connect(askpass).await {
            Ok(connection) => self.serve(connection).await,
            Err(reason) => reason,
        };
        let _ = self.done.send(Done::Closed {
            host: self.host.clone(),
            connection: self.connection,
            reason,
        });
    }

    /// Connects; the error is `None` if the attempt was stopped.
    async fn connect(
        &self,
        askpass: Result<AskpassEnv, String>,
    ) -> Result<Connection, Option<String>> {
        let askpass = askpass.map_err(Some)?;
        let settings = &self.context.settings;
        let failed = |error: SshError| describe::ssh_error(&error);
        tokio::select! {
            result = check_version(settings) => result.map_err(failed)?,
            () = self.stop.cancelled() => return Err(None),
        };
        let target = self.context.target(&self.host);
        let runtime_dir = &self.context.paths.runtime_dir;
        let session = Session::connect(settings, &target, runtime_dir, Some(askpass), &self.stop)
            .await
            .map_err(failed)?;
        let SftpChannel {
            stdin,
            stdout,
            mut process,
        } = match session.open_sftp() {
            Ok(channel) => channel,
            Err(error) => {
                session.close().await;
                return Err(failed(error));
            }
        };
        let fs = tokio::select! {
            fs = SftpFs::from_pipes(stdin, stdout) => fs,
            () = self.stop.cancelled() => {
                process.finish().await;
                session.close().await;
                return Err(None);
            }
        };
        match fs {
            Ok(fs) => Ok(Connection {
                session,
                fs,
                process,
            }),
            Err(error) => {
                // Without multiplexing, authentication happens here, and ssh says what failed.
                let _ = tokio::time::timeout(CHANNEL_EXIT_WAIT, process.wait()).await;
                let stderr = process.stderr().text();
                process.finish().await;
                session.close().await;
                Err(Some(
                    describe::last_line(&stderr)
                        .map_or_else(|| describe::vfs_error(&error), str::to_owned),
                ))
            }
        }
    }

    /// Serves requests until the connection is lost or asked to stop, then shuts it down.
    /// Returns why it ended: `None` if asked to stop.
    async fn serve(&self, connection: Connection) -> Option<String> {
        let Connection {
            session,
            fs,
            mut process,
        } = connection;
        let fs = Arc::new(fs);
        let (requests, mut incoming) = mpsc::unbounded_channel();
        let _ = self.done.send(Done::Connected {
            host: self.host.clone(),
            connection: self.connection,
            handle: HostHandle(requests),
        });
        let mut running: FuturesUnordered<Pin<Box<dyn Future<Output = Done> + Send + '_>>> =
            FuturesUnordered::new();
        let reason = loop {
            tokio::select! {
                Some(request) = incoming.recv() => match request {
                    HostRequest::List(side, request) => {
                        running.push(Box::pin(self.list(&fs, side, request)));
                    }
                    HostRequest::CreateDir(side, path) => {
                        running.push(Box::pin(self.create_dir(&fs, side, path)));
                    }
                    HostRequest::Delete { id, paths, cancel } => {
                        let host = self.host.clone();
                        let location = move |path| Location::Remote {
                            host: host.clone(),
                            path,
                        };
                        let job = run_delete(&*fs, paths, cancel, id, &self.done, location);
                        running.push(Box::pin(job));
                    }
                    HostRequest::Share(reply) => {
                        let _ = reply.send(Arc::clone(&fs));
                    }
                },
                Some(done) = running.next() => {
                    let _ = self.done.send(done);
                }
                () = self.stop.cancelled() => break None,
                () = session.closed() => {
                    let error = SshError::Disconnected { stderr: session.stderr() };
                    break describe::ssh_error(&error);
                }
                // Without multiplexing the channel is the connection.
                _ = process.wait() => {
                    let stderr = process.stderr().text();
                    break Some(describe::last_line(&stderr)
                        .map_or_else(|| fl!("error-connection-closed"), str::to_owned));
                }
            }
        };
        // Requests in flight are abandoned; dropping them is safe.
        drop(running);
        drop(incoming);
        // A job that shares the session still holds it; the session goes with that job, which,
        // with the channel gone, soon fails.
        if let Ok(fs) = Arc::try_unwrap(fs) {
            let _ = tokio::time::timeout(SFTP_CLOSE_TIMEOUT, fs.close()).await;
        }
        session.close().await;
        process.finish().await;
        reason
    }

    async fn create_dir(&self, fs: &SftpFs, side: Side, path: RemotePath) -> Done {
        let result = fs
            .create_dir(&path)
            .await
            .map_err(|error| describe::vfs_error(&error));
        let location = Location::Remote {
            host: self.host.clone(),
            path,
        };
        Done::Created {
            side,
            location,
            result,
        }
    }

    /// Lists a remote directory. The empty path is where the host opens: its `start_dir`, or
    /// the remote home directory; the reply names it as an absolute path.
    async fn list(&self, fs: &SftpFs, side: Side, request: ListRequest) -> Done {
        let result = async {
            let Location::Remote { path, .. } = &request.location else {
                return Err(fl!("error-connection-closed"));
            };
            let path = if path.as_bytes().is_empty() {
                let start_dir = self
                    .context
                    .config
                    .hosts
                    .get(&self.host)
                    .and_then(|host| host.start_dir.as_deref());
                match start_dir {
                    Some(dir) => fs.canonicalize(&RemotePath::from(dir)).await,
                    None => fs.home().await,
                }
                .map_err(|error: VfsError| describe::vfs_error(&error))?
            } else {
                path.clone()
            };
            let entries = fs
                .list_dir(&path)
                .await
                .map_err(|error| describe::vfs_error(&error))?;
            let location = Location::Remote {
                host: self.host.clone(),
                path,
            };
            Ok(Listed {
                location,
                listing: Listing::Dir(entries),
            })
        }
        .await;
        Done::Listed {
            side,
            generation: request.generation,
            result,
        }
    }
}
