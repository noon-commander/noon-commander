//! Background work for the app: listings, new directories, jobs, and one task per connected
//! host that owns its ssh session and SFTP channel.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt as _;
use futures_util::stream::FuturesUnordered;
use noc_config::{Config, ConfigError, HostConfig, Hosts, save_config, save_host};
use noc_ops::{
    Algorithm, Checksum, Conflict, CopyOptions, Decision, Endpoint, Event, Files, Outcome,
    Reporter, Sum,
};
use noc_ssh::askpass::{AskpassEnv, AskpassEvent, AskpassServer};
use noc_ssh::resolve::resolve;
use noc_ssh::version::check_version;
use noc_ssh::{CachedHost, ChannelProcess, Session, SftpChannel, SshError, Target, cleanup_stale};
use noc_tools::zoxide::Scored;
use noc_vfs::{
    FileReader as _, FileWriter as _, LocalFs, Location, Metadata, RemotePath, SftpFs, Space, Vfs,
    VfsError, VfsPath as _,
};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use super::app::Effect;
use super::describe;
use super::dialog::{Ask, Reply};
use super::panel::{ListRequest, Listed, Listing};
use super::root;
use super::tabs::PanelId;
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
        panel: PanelId,
        generation: u64,
        result: Result<Listed, String>,
    },
    /// The listing of the virtual root for the location menu, of [`Effect::ListPlaces`].
    Places {
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
        panel: PanelId,
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
    /// The file of [`Effect::WriteFile`] is written, or why not: `None` if the name is taken.
    Written {
        location: Location,
        result: Result<(), Option<String>>,
    },
    /// The settings of [`Effect::SaveHost`] are saved, and these are all the host settings
    /// now; or why not.
    HostSaved(Result<Arc<Hosts>, String>),
    /// The settings of [`Effect::SaveConfig`] are written, or why not.
    ConfigSaved(Result<(), String>),
    /// zoxide's answer to the [`Effect::ZoxideQuery`] `generation`, or why there is none.
    Jumps {
        generation: u64,
        result: Result<Vec<Scored>, String>,
    },
}

/// A report from a job, with its paths as locations and its errors in words.
#[derive(Debug)]
pub(crate) enum JobEvent {
    /// Counting what to do: `items` found so far.
    Scanning { items: u64 },
    /// At `current`, with `done` of `total` entries and `bytes_done` of `bytes_total` bytes
    /// behind it, `bytes_copied` of them read and written; jobs that move no data count no
    /// bytes.
    Progress {
        current: Location,
        done: u64,
        total: u64,
        bytes_done: u64,
        bytes_total: u64,
        bytes_copied: u64,
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
    /// The checksums of a checksum job that was not aborted, just before it is over.
    Sums(Vec<Sum<Location>>),
    /// The job is over; `complete` if it did all it was asked, without skipping or aborting.
    Finished { complete: bool },
}

/// Work for the task of a connected host, for a panel.
#[derive(Debug)]
pub(crate) enum HostRequest {
    List(PanelId, ListRequest),
    /// Makes the directory at `path`.
    CreateDir(PanelId, RemotePath),
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
    /// Changes to the config file, which one task writes in turn, so that a change never
    /// overtakes the one before it.
    config_saves: mpsc::UnboundedSender<(Config, Config)>,
    /// Directories for zoxide, which one task adds in turn.
    zoxide_adds: mpsc::UnboundedSender<PathBuf>,
    /// The zoxide query in flight; a newer one replaces it.
    zoxide_query: Option<AbortHandle>,
    /// New working directories for the process, which one task changes to in turn.
    work_dirs: mpsc::UnboundedSender<PathBuf>,
}

impl Tasks {
    /// Prepares the runtime directory and the askpass bridge, and cleans up after crashed
    /// instances in the background. A failure here only affects connections.
    pub(crate) async fn start(context: Arc<Context>, done: mpsc::UnboundedSender<Done>) -> Self {
        let askpass = prepare_ssh(&context, done.clone()).await;
        if let Err(reason) = &askpass {
            tracing::warn!(%reason, "cannot prepare for ssh connections");
        }
        let config_saves = save_configs(context.config_file.clone(), done.clone());
        let zoxide_adds = add_to_zoxide(Arc::clone(&context));
        Self {
            context,
            done,
            askpass,
            hosts: JoinSet::new(),
            jobs: JoinSet::new(),
            cache: Arc::new(Mutex::new(())),
            config_saves,
            zoxide_adds,
            zoxide_query: None,
            work_dirs: change_work_dirs(),
        }
    }

    /// Makes `dir` the working directory of the process, after the changes before.
    pub(crate) fn change_work_dir(&self, dir: PathBuf) {
        let _ = self.work_dirs.send(dir);
    }

    pub(crate) fn run(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::List {
                    panel,
                    request,
                    host: Some(handle),
                } => {
                    // If the host's task has ended, its `Closed` report is on its way.
                    let _ = handle.0.send(HostRequest::List(panel, request));
                }
                Effect::List {
                    panel,
                    request,
                    host: None,
                } => self.list(panel, request),
                Effect::ListPlaces { generation } => self.list_places(generation),
                Effect::CreateDir {
                    panel,
                    location,
                    host,
                } => self.create_dir(panel, location, host),
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
                Effect::Checksum {
                    id,
                    targets,
                    algorithm,
                    cancel,
                } => self.checksum(id, targets, algorithm, cancel),
                Effect::WriteFile {
                    location,
                    bytes,
                    replace,
                    host,
                } => self.write_file(location, bytes, replace, host),
                Effect::Connect {
                    host,
                    connection,
                    stop,
                } => self.connect(host, connection, stop),
                Effect::SaveHost { name, host } => self.save_host(name, host),
                Effect::ZoxideAdd(dir) => {
                    let _ = self.zoxide_adds.send(dir);
                }
                Effect::ZoxideQuery {
                    generation,
                    keywords,
                    exclude,
                } => self.query_zoxide(generation, keywords, exclude),
                Effect::SaveConfig { old, new, config } => {
                    self.context.set_config(*config);
                    self.save_config(*old, *new);
                }
            }
        }
    }

    /// Starts the task of `host`, which connects to it, and finds its address.
    fn connect(&mut self, host: String, connection: u64, stop: CancellationToken) {
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

    /// Writes to the config file the settings that differ between `old` and `new`, after
    /// the changes before.
    fn save_config(&self, old: Config, new: Config) {
        let _ = self.config_saves.send((old, new));
    }

    /// Asks zoxide for the directories that match `keywords`, without `exclude`; a query that
    /// is still running is dropped, which stops zoxide.
    fn query_zoxide(&mut self, generation: u64, keywords: Vec<String>, exclude: Option<PathBuf>) {
        if let Some(previous) = self.zoxide_query.take() {
            previous.abort();
        }
        let zoxide = self.context.zoxide();
        let done = self.done.clone();
        let task = tokio::spawn(async move {
            let result = zoxide
                .query(&keywords, exclude.as_deref())
                .await
                .map_err(|error| describe::zoxide_error(&error));
            let _ = done.send(Done::Jumps { generation, result });
        });
        self.zoxide_query = Some(task.abort_handle());
    }

    /// Writes the settings of the host `name` to `hosts.toml` and reads them all again.
    fn save_host(&self, name: String, host: Option<HostConfig>) {
        let context = Arc::clone(&self.context);
        let done = self.done.clone();
        tokio::spawn(async move {
            let saved = tokio::task::spawn_blocking(move || {
                save_host(&context.hosts_file, &name, host.as_ref())?;
                context.reload_hosts()?;
                Ok::<_, ConfigError>(context.hosts())
            })
            .await;
            let result = match saved {
                Ok(result) => result.map_err(|error| describe::chain(&error)),
                Err(error) => Err(error.to_string()),
            };
            let _ = done.send(Done::HostSaved(result));
        });
    }

    /// Lists the virtual root for the location menu.
    fn list_places(&self, generation: u64) {
        let context = Arc::clone(&self.context);
        let done = self.done.clone();
        tokio::spawn(async move {
            let result = list(context, Location::Root).await;
            let _ = done.send(Done::Places { generation, result });
        });
    }

    /// Lists the virtual root, the hosts, or a local directory here.
    fn list(&self, panel: PanelId, request: ListRequest) {
        let context = Arc::clone(&self.context);
        let done = self.done.clone();
        tokio::spawn(async move {
            let generation = request.generation;
            let result = list(context, request.location).await;
            let _ = done.send(Done::Listed {
                panel,
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
    fn create_dir(&self, panel: PanelId, location: Location, host: Option<HostHandle>) {
        match (&location, host) {
            (Location::Remote { path, .. }, Some(handle)) => {
                let _ = handle.0.send(HostRequest::CreateDir(panel, path.clone()));
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
                        panel,
                        location,
                        result,
                    });
                });
            }
            (Location::Root | Location::Sftp | Location::Remote { .. }, _) => {
                let result = Err(fl!("error-connection-closed"));
                let _ = self.done.send(Done::Created {
                    panel,
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
                Location::Root | Location::Sftp => {}
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

    /// Runs the checksum job `id` here.
    fn checksum(
        &mut self,
        id: u64,
        targets: Vec<(Vec<Location>, Option<HostHandle>)>,
        algorithm: Algorithm,
        cancel: CancellationToken,
    ) {
        let done = self.done.clone();
        self.reap_jobs();
        self.jobs.spawn(async move {
            let finished = run_checksum(id, targets, algorithm, cancel, &done).await;
            let _ = done.send(finished);
        });
    }

    /// Writes a small file, here or through the session of its host.
    fn write_file(
        &self,
        location: Location,
        bytes: Vec<u8>,
        replace: bool,
        host: Option<HostHandle>,
    ) {
        let done = self.done.clone();
        tokio::spawn(async move {
            let result = write_file(&location, bytes, replace, host).await;
            let _ = done.send(Done::Written { location, result });
        });
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
    let settings = context.settings();
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

/// Starts the task that writes changes to the config file at `path`, one after another, and
/// reports each; returns where to send them.
fn save_configs(
    path: PathBuf,
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<(Config, Config)> {
    let (sender, mut changes) = mpsc::unbounded_channel::<(Config, Config)>();
    tokio::spawn(async move {
        while let Some((old, new)) = changes.recv().await {
            let path = path.clone();
            let saved = tokio::task::spawn_blocking(move || save_config(&path, &old, &new)).await;
            let result = match saved {
                Ok(result) => result.map_err(|error| describe::chain(&error)),
                Err(error) => Err(error.to_string()),
            };
            if done.send(Done::ConfigSaved(result)).is_err() {
                break;
            }
        }
    });
    sender
}

/// Starts the task that adds directories to zoxide, one after another, so that no two zoxide
/// processes write its database at once; returns where to send them. Failures only go to the
/// log; a missing zoxide goes there once, and is not tried again until `zoxide.program`
/// changes.
fn add_to_zoxide(context: Arc<Context>) -> mpsc::UnboundedSender<PathBuf> {
    let (sender, mut dirs) = mpsc::unbounded_channel::<PathBuf>();
    tokio::spawn(async move {
        let mut missing: Option<PathBuf> = None;
        while let Some(dir) = dirs.recv().await {
            let zoxide = context.zoxide();
            if missing.as_deref() == Some(zoxide.program()) {
                continue;
            }
            match zoxide.add(&dir).await {
                Ok(()) => {}
                Err(error) if error.is_not_found() => {
                    let program = zoxide.program().display().to_string();
                    tracing::info!(%program, "zoxide is not installed; directories are not recorded");
                    missing = Some(zoxide.program().to_path_buf());
                }
                Err(error) => {
                    let error = describe::chain(&error);
                    tracing::debug!(dir = %dir.display(), %error, "cannot add to zoxide");
                }
            }
        }
    });
    sender
}

/// Starts the task that changes the working directory of the process, one change after
/// another, so that the last one wins; returns where to send them. `chdir` can hang on a
/// network volume, so it runs off the event loop. Failures only go to the log: the directory
/// may be gone by now.
fn change_work_dirs() -> mpsc::UnboundedSender<PathBuf> {
    let (sender, mut dirs) = mpsc::unbounded_channel::<PathBuf>();
    tokio::spawn(async move {
        while let Some(mut dir) = dirs.recv().await {
            // Only the latest of the waiting ones matters.
            while let Ok(newer) = dirs.try_recv() {
                dir = newer;
            }
            let changed = tokio::task::spawn_blocking(move || {
                std::env::set_current_dir(&dir).map_err(|error| (dir, error))
            })
            .await;
            if let Ok(Err((dir, error))) = changed {
                tracing::debug!(dir = %dir.display(), %error, "cannot change the working directory");
            }
        }
    });
    sender
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
    let target = Target::new(host.as_str());
    let settings = context.settings();
    let resolved = tokio::select! {
        resolved = resolve(&settings, &target) => resolved,
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
                bytes_copied: progress.bytes_copied,
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

/// Files found for a checksum job, on the file system they are on.
enum Found {
    Local(Files<PathBuf>),
    Remote(Arc<SftpFs>, String, Files<RemotePath>),
}

/// Runs the checksum job `id` on `targets`, each group local or on the host of its handle:
/// counts the files of every group first, so that progress has one total, then hashes them.
async fn run_checksum(
    id: u64,
    targets: Vec<(Vec<Location>, Option<HostHandle>)>,
    algorithm: Algorithm,
    cancel: CancellationToken,
    done: &mpsc::UnboundedSender<Done>,
) -> Done {
    let mut sessions = Vec::new();
    for (group, host) in targets {
        let Some(fs) = share(host).await else {
            return finished(id, None);
        };
        sessions.push((group, fs));
    }
    let (events, incoming) = mpsc::unbounded_channel();
    let reporter = Reporter::new(events, cancel);
    let work = async move {
        let mut reporter = reporter;
        let mut job = Checksum::new(algorithm, &mut reporter);
        let here = |path: &PathBuf| Location::Local(path.clone());
        let mut found = Vec::new();
        for (group, fs) in sessions {
            match fs {
                None => {
                    let paths = group
                        .into_iter()
                        .filter_map(|location| match location {
                            Location::Local(path) => Some(path),
                            _ => None,
                        })
                        .collect();
                    let side = Endpoint {
                        vfs: &LocalFs,
                        report: &here,
                    };
                    found.push(Found::Local(job.scan(&side, paths).await));
                }
                Some(fs) => {
                    let mut host = String::new();
                    let paths = group
                        .into_iter()
                        .filter_map(|location| match location {
                            Location::Remote { host: on, path } => {
                                host = on;
                                Some(path)
                            }
                            _ => None,
                        })
                        .collect();
                    let there = remote_location(host.clone());
                    let side = Endpoint {
                        vfs: &*fs,
                        report: &there,
                    };
                    let files = job.scan(&side, paths).await;
                    found.push(Found::Remote(fs, host, files));
                }
            }
        }
        for files in found {
            match files {
                Found::Local(files) => {
                    let side = Endpoint {
                        vfs: &LocalFs,
                        report: &here,
                    };
                    job.hash(&side, files).await;
                }
                Found::Remote(fs, host, files) => {
                    let there = remote_location(host);
                    let side = Endpoint {
                        vfs: &*fs,
                        report: &there,
                    };
                    job.hash(&side, files).await;
                }
            }
        }
        job.finish()
    };
    let ((outcome, sums), ()) = tokio::join!(work, forward(incoming, id, done, |path| path));
    if !outcome.aborted {
        let _ = done.send(Done::Job {
            id,
            event: JobEvent::Sums(sums),
        });
    }
    finished(id, Some(outcome))
}

/// How a remote path on `host` is reported.
fn remote_location(host: String) -> impl Fn(&RemotePath) -> Location + Send + Sync {
    move |path| Location::Remote {
        host: host.clone(),
        path: path.clone(),
    }
}

/// Writes `bytes` to the file at `location`; `Err(None)` if the name is taken and not
/// `replace`.
async fn write_file(
    location: &Location,
    bytes: Vec<u8>,
    replace: bool,
    host: Option<HostHandle>,
) -> Result<(), Option<String>> {
    let result = match (location, share(host).await) {
        (Location::Local(path), _) => write_all(&LocalFs, path, bytes, replace).await,
        (Location::Remote { path, .. }, Some(Some(fs))) => {
            write_all(&*fs, path, bytes, replace).await
        }
        _ => return Err(Some(fl!("error-connection-closed"))),
    };
    match result {
        Ok(()) => Ok(()),
        Err(VfsError::AlreadyExists(_)) => Err(None),
        Err(error) => Err(Some(describe::vfs_error(&error))),
    }
}

async fn write_all<V: Vfs>(
    vfs: &V,
    path: &V::Path,
    bytes: Vec<u8>,
    replace: bool,
) -> Result<(), VfsError> {
    let mut writer = vfs.create_file(path, replace).await?;
    writer.write(bytes).await?;
    writer.finish().await
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
                Location::Root | Location::Sftp => {}
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

/// Lists the virtual root, the hosts, or a local directory. The root reads the volumes and the
/// ssh config each time, so a reload shows what changed.
async fn list(context: Arc<Context>, location: Location) -> Result<Listed, String> {
    let (listing, space) = match &location {
        Location::Root => {
            let hide = context.config().volumes.hide.clone();
            let hosts = tokio::task::spawn_blocking(move || root::read_hosts(&context));
            let (volumes, hosts) = tokio::join!(root::read_volumes(&hide), hosts);
            let listing = Listing::Root {
                volumes,
                hosts: hosts.map_err(|error| error.to_string())?,
            };
            (listing, None)
        }
        Location::Sftp => {
            let hosts = tokio::task::spawn_blocking(move || root::read_hosts(&context))
                .await
                .map_err(|error| error.to_string())?;
            (Listing::Hosts(hosts), None)
        }
        Location::Local(path) => {
            // The listing does not wait for a file system that is slow to tell its space.
            let space = tokio::time::timeout(root::VOLUME_TIMEOUT, space(&LocalFs, path));
            let (entries, space) = tokio::join!(LocalFs.list_dir(path), space);
            let entries = entries.map_err(|error| describe::vfs_error(&error))?;
            (Listing::Dir(entries), space.ok().flatten())
        }
        // The app sends these to the host's task.
        Location::Remote { .. } => return Err(fl!("error-connection-closed")),
    };
    Ok(Listed {
        location,
        listing,
        space,
    })
}

/// The space of the file system that holds `path`, or `None` if it cannot be told.
async fn space<V: Vfs>(vfs: &V, path: &V::Path) -> Option<Space> {
    match vfs.space(path).await {
        Ok(space) => space,
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "no file system space");
            None
        }
    }
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
        let settings = &self.context.settings();
        let failed = |error: SshError| describe::ssh_error(&error);
        tokio::select! {
            result = check_version(settings) => result.map_err(failed)?,
            () = self.stop.cancelled() => return Err(None),
        };
        let target = Target::new(self.host.as_str());
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
                    HostRequest::List(panel, request) => {
                        running.push(Box::pin(self.list(&fs, panel, request)));
                    }
                    HostRequest::CreateDir(panel, path) => {
                        running.push(Box::pin(self.create_dir(&fs, panel, path)));
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

    async fn create_dir(&self, fs: &SftpFs, panel: PanelId, path: RemotePath) -> Done {
        let result = fs
            .create_dir(&path)
            .await
            .map_err(|error| describe::vfs_error(&error));
        let location = Location::Remote {
            host: self.host.clone(),
            path,
        };
        Done::Created {
            panel,
            location,
            result,
        }
    }

    /// Lists a remote directory. The empty path is where the host opens: the directory to
    /// resume, if it is still there, else its `start_dir`, else the remote home directory; the
    /// reply names it as an absolute path.
    async fn list(&self, fs: &SftpFs, panel: PanelId, request: ListRequest) -> Done {
        let result = async {
            let Location::Remote { path, .. } = &request.location else {
                return Err(fl!("error-connection-closed"));
            };
            if path.as_bytes().is_empty()
                && let Some(dir) = &request.resume
                && let Ok(listed) = self.list_dir(fs, dir.clone()).await
            {
                return Ok(listed);
            }
            let path = if path.as_bytes().is_empty() {
                let hosts = self.context.hosts();
                match hosts.get(&self.host).and_then(|host| host.start_dir()) {
                    Some(dir) => fs.canonicalize(&RemotePath::from(dir)).await,
                    None => fs.home().await,
                }
                .map_err(|error: VfsError| describe::vfs_error(&error))?
            } else {
                path.clone()
            };
            self.list_dir(fs, path)
                .await
                .map_err(|error| describe::vfs_error(&error))
        }
        .await;
        Done::Listed {
            panel,
            generation: request.generation,
            result,
        }
    }

    async fn list_dir(&self, fs: &SftpFs, path: RemotePath) -> Result<Listed, VfsError> {
        // No timeout, unlike locally: the server serves requests in order, so a statvfs that
        // hangs there would hold up the listing anyway.
        let (space, entries) = tokio::join!(space(fs, &path), fs.list_dir(&path));
        let location = Location::Remote {
            host: self.host.clone(),
            path,
        };
        Ok(Listed {
            location,
            listing: Listing::Dir(entries?),
            space,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Stdio;

    use noc_config::{Config, HostConfig, Hosts, Paths, SftpHost};
    use noc_vfs::RemotePath;
    use tokio::process::Command;

    use super::*;

    /// A session with a local `sftp-server` that starts in `dir`; `None` without one.
    async fn local_sftp(dir: &Path) -> Option<(tokio::process::Child, SftpFs)> {
        let program = std::env::var_os("SFTP_SERVER")
            .filter(|program| !program.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server"]
                    .into_iter()
                    .map(PathBuf::from)
                    .find(|path| path.exists())
            })?;
        let mut child = Command::new(program)
            .arg("-e")
            .arg("-d")
            .arg(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let fs = SftpFs::from_pipes(stdin, stdout).await.unwrap();
        Some((child, fs))
    }

    fn task(dir: &Path, start_dir: Option<&str>) -> HostTask {
        let mut hosts = Hosts::default();
        let host = SftpHost {
            start_dir: start_dir.map(str::to_owned),
            ..SftpHost::default()
        };
        hosts.hosts.insert("web".to_owned(), HostConfig::Sftp(host));
        let paths = Paths::resolve(dir, 501, &|_| None);
        let context = Context::new(paths, Config::default(), dir.join("config.toml"), hosts);
        HostTask {
            context: Arc::new(context),
            host: "web".to_owned(),
            connection: 1,
            stop: CancellationToken::new(),
            done: mpsc::unbounded_channel().0,
        }
    }

    /// Where opening `web` lands, resuming `resume`.
    async fn opened(task: &HostTask, fs: &SftpFs, resume: Option<&Path>) -> Result<String, String> {
        let request = ListRequest {
            generation: 1,
            location: Location::Remote {
                host: "web".to_owned(),
                path: RemotePath::from(""),
            },
            resume: resume.map(|path| RemotePath::from(path.to_str().unwrap())),
        };
        let panel = PanelId {
            side: super::super::app::Side::Left,
            tab: 1,
        };
        let Done::Listed { result, .. } = task.list(fs, panel, request).await else {
            panic!("expected a listing");
        };
        result.map(|listed| match listed.location {
            Location::Remote { path, .. } => String::from_utf8_lossy(path.as_bytes()).into_owned(),
            other => panic!("expected a remote location, got {other:?}"),
        })
    }

    #[tokio::test]
    async fn a_host_opens_where_it_left_off_else_at_its_start_dir_else_at_home() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::create_dir_all(root.join("www/app")).unwrap();
        let Some((_child, fs)) = local_sftp(&root).await else {
            return;
        };
        let text = |path: &Path| path.to_str().unwrap().to_owned();
        let with_start = task(&root, Some("www"));
        let app = root.join("www/app");
        assert_eq!(opened(&with_start, &fs, Some(&app)).await, Ok(text(&app)));
        let gone = root.join("gone");
        assert_eq!(
            opened(&with_start, &fs, Some(&gone)).await,
            Ok(text(&root.join("www"))),
            "a directory that is gone falls back to start_dir"
        );
        let plain = task(&root, None);
        assert_eq!(opened(&plain, &fs, Some(&gone)).await, Ok(text(&root)));
        let missing = task(&root, Some("missing"));
        assert!(opened(&missing, &fs, None).await.is_err());
    }

    #[tokio::test]
    async fn a_checksum_job_reports_its_sums_then_its_end() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("abc");
        std::fs::write(&file, "abc").unwrap();
        let (done, mut reports) = mpsc::unbounded_channel();
        let targets = vec![(vec![Location::Local(file.clone())], None)];
        let cancel = CancellationToken::new();
        let end = run_checksum(7, targets, Algorithm::Sha256, cancel, &done).await;
        assert!(matches!(
            end,
            Done::Job {
                id: 7,
                event: JobEvent::Finished { complete: true }
            }
        ));
        let mut sums = None;
        while let Ok(report) = reports.try_recv() {
            if let Done::Job {
                event: JobEvent::Sums(found),
                ..
            } = report
            {
                sums = Some(found);
            }
        }
        let sums = sums.unwrap();
        assert_eq!(sums.len(), 1);
        assert_eq!(sums[0].path, Location::Local(file));
        assert_eq!(sums[0].name, b"abc");
        assert_eq!(
            sums[0].digest.as_ref().unwrap()[..4],
            [0xba, 0x78, 0x16, 0xbf]
        );

        let cancel = CancellationToken::new();
        cancel.cancel();
        let targets = vec![(vec![Location::Local(tmp.path().join("abc"))], None)];
        run_checksum(8, targets, Algorithm::Sha256, cancel, &done).await;
        while let Ok(report) = reports.try_recv() {
            assert!(
                !matches!(
                    report,
                    Done::Job {
                        event: JobEvent::Sums(_),
                        ..
                    }
                ),
                "an aborted job has no sums"
            );
        }
    }
}
