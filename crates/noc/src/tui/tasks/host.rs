//! The task of each host: connecting, serving requests, and the link jobs keep to it.

use std::ffi::OsString;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures_util::StreamExt as _;
use futures_util::stream::FuturesUnordered;
use noc_config::Run;
use noc_ssh::askpass::{AskpassEnv, AskpassEvent, AskpassServer};
use noc_ssh::resolve::resolve;
use noc_ssh::version::check_version;
use noc_ssh::{CachedHost, ChannelProcess, Session, SftpChannel, SshError, Target, cleanup_stale};
use noc_vfs::{
    DirEntry, Location, Metadata, RemotePath, SftpFs, SftpReader, SftpWriter, Space, Vfs, VfsError,
};
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::context::Context;
use crate::i18n::fl;
use crate::tui::describe;
use crate::tui::dialog::{Ask, Reply};
use crate::tui::panel::{ListError, ListRequest, Listed, Listing};
use crate::tui::places;
use crate::tui::tabs::PanelId;

use super::{Done, Unmade, space};

/// How long closing an SFTP session may take: long enough, over a slow link, for what a job
/// sent before it was cancelled and let go, as the removal of a file it did not finish.
pub(super) const SFTP_CLOSE_TIMEOUT: Duration = Duration::from_secs(4);

/// How long a failed channel gets to exit and finish its stderr, which says why it failed.
pub(super) const CHANNEL_EXIT_WAIT: Duration = Duration::from_secs(1);

/// Work for the task of a connected host, for a panel.
#[derive(Debug)]
pub(crate) enum HostRequest {
    List(PanelId, ListRequest),
    /// Makes the directory at `path`.
    CreateDir(PanelId, RemotePath),
    /// Shares the host's SFTP session, for a job that runs elsewhere, such as a copy between
    /// two hosts.
    Share(oneshot::Sender<Arc<SftpFs>>),
    /// The ssh command that runs `command` with `shell` in `dir` on the host, for the event
    /// loop to start with the terminal handed over, or why there is none.
    Command {
        dir: RemotePath,
        shell: Run,
        command: String,
        reply: oneshot::Sender<Result<tokio::process::Command, String>>,
    },
    /// Starts `program` with `args` in `dir` on the host without a terminal, its stdout and
    /// stderr piped, or says why it cannot (ADR 0028).
    Spawn {
        dir: RemotePath,
        program: &'static str,
        args: Vec<OsString>,
        reply: oneshot::Sender<Result<tokio::process::Child, String>>,
    },
}

/// Passes requests to the task of a connected host.
#[derive(Debug, Clone)]
pub(crate) struct HostHandle(pub(super) mpsc::UnboundedSender<HostRequest>);

#[cfg(test)]
impl HostHandle {
    /// A handle and what it receives.
    pub(crate) fn channel() -> (Self, mpsc::UnboundedReceiver<HostRequest>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self(sender), receiver)
    }
}

/// How the jobs on a host reach it: the connection it has, and once it connects again after
/// losing one, the next (ADR 0064).
#[derive(Debug, Clone)]
pub(crate) struct HostLink(watch::Receiver<HostHandle>);

/// Where the app gives the jobs on a host each connection of the host in turn.
#[derive(Debug)]
pub(crate) struct Relink(watch::Sender<HostHandle>);

impl Relink {
    pub(crate) fn new(handle: HostHandle) -> Self {
        Self(watch::channel(handle).0)
    }

    /// The host is connected again, through `handle`.
    pub(crate) fn connected(&self, handle: HostHandle) {
        self.0.send_replace(handle);
    }

    /// A link for a job, to the connection the host has now, and to those that follow.
    pub(crate) fn link(&self) -> HostLink {
        HostLink(self.0.subscribe())
    }
}

/// The SFTP session of a host for a job, through its [`HostLink`]: it takes the session of
/// the connection the host has when it first needs one, and that of the next connection once
/// the host connects again. An operation that failed as the connection was lost works when the
/// job tries it again then; a reader or a writer opened before stays on the old session.
#[derive(Debug)]
pub(super) struct Rejoin {
    link: HostLink,
    /// The connection whose session it has, and that session.
    joined: std::sync::Mutex<Option<(HostHandle, Arc<SftpFs>)>>,
}

impl Rejoin {
    pub(super) fn new(link: HostLink) -> Self {
        Self {
            link,
            joined: std::sync::Mutex::default(),
        }
    }

    /// The session of the connection the host has now, from its task the first time.
    async fn fs(&self) -> Result<Arc<SftpFs>, VfsError> {
        let handle = self.link.0.borrow().clone();
        {
            let joined = self
                .joined
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((used, fs)) = &*joined
                && used.0.same_channel(&handle.0)
            {
                return Ok(Arc::clone(fs));
            }
        }
        let Some(Some(fs)) = share(Some(handle.clone())).await else {
            return Err(VfsError::Io(std::io::ErrorKind::NotConnected.into()));
        };
        *self
            .joined
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((handle, Arc::clone(&fs)));
        Ok(fs)
    }
}

impl Vfs for Rejoin {
    type Path = RemotePath;
    type Reader = SftpReader;
    type Writer = SftpWriter;

    async fn list_dir(&self, path: &RemotePath) -> Result<Vec<DirEntry>, VfsError> {
        self.fs().await?.list_dir(path).await
    }

    async fn metadata(&self, path: &RemotePath) -> Result<Metadata, VfsError> {
        self.fs().await?.metadata(path).await
    }

    async fn symlink_metadata(&self, path: &RemotePath) -> Result<Metadata, VfsError> {
        self.fs().await?.symlink_metadata(path).await
    }

    async fn space(&self, path: &RemotePath) -> Result<Option<Space>, VfsError> {
        self.fs().await?.space(path).await
    }

    async fn canonicalize(&self, path: &RemotePath) -> Result<RemotePath, VfsError> {
        self.fs().await?.canonicalize(path).await
    }

    async fn create_dir(&self, path: &RemotePath) -> Result<(), VfsError> {
        self.fs().await?.create_dir(path).await
    }

    async fn remove_file(&self, path: &RemotePath) -> Result<(), VfsError> {
        self.fs().await?.remove_file(path).await
    }

    async fn remove_dir(&self, path: &RemotePath) -> Result<(), VfsError> {
        self.fs().await?.remove_dir(path).await
    }

    async fn rename(&self, from: &RemotePath, to: &RemotePath) -> Result<(), VfsError> {
        self.fs().await?.rename(from, to).await
    }

    async fn read_link(&self, path: &RemotePath) -> Result<Vec<u8>, VfsError> {
        self.fs().await?.read_link(path).await
    }

    async fn create_symlink(&self, target: &[u8], path: &RemotePath) -> Result<(), VfsError> {
        self.fs().await?.create_symlink(target, path).await
    }

    async fn set_permissions(&self, path: &RemotePath, mode: u32) -> Result<(), VfsError> {
        self.fs().await?.set_permissions(path, mode).await
    }

    async fn open_file_at(&self, path: &RemotePath, offset: u64) -> Result<SftpReader, VfsError> {
        self.fs().await?.open_file_at(path, offset).await
    }

    async fn create_file(&self, path: &RemotePath, replace: bool) -> Result<SftpWriter, VfsError> {
        self.fs().await?.create_file(path, replace).await
    }

    async fn set_modified(&self, path: &RemotePath, time: SystemTime) -> Result<(), VfsError> {
        self.fs().await?.set_modified(path, time).await
    }
}

/// The runtime directory and the askpass bridge, whose prompts go to the app as [`Done`]s.
pub(super) async fn prepare_ssh(
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

/// Runs `ssh -G` for a host that is being connected to, as the location menu shows effective
/// addresses only from there: reports the address and stores it in the cache. Failures only go to the
/// log; the connection reports its own.
pub(super) async fn resolve_address(
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
    let _ = done.send(Done::Resolved {
        host: host.clone(),
        address: cached.address(),
        user: cached.user.clone(),
    });
    let _guard = cache.lock().await;
    let saved =
        tokio::task::spawn_blocking(move || places::remember(&context, &host, cached)).await;
    if let Ok(Err(error)) = saved {
        tracing::warn!(%error, "cannot save the ssh -G cache");
    }
}

/// The session of a connected host, from its task; `None` if the host is gone.
pub(super) async fn share(handle: Option<HostHandle>) -> Option<Option<Arc<SftpFs>>> {
    let Some(handle) = handle else {
        return Some(None);
    };
    let (reply, shared) = oneshot::channel();
    handle.0.send(HostRequest::Share(reply)).ok()?;
    shared.await.ok().map(Some)
}

/// A connection to a host: the session, an SFTP channel, and the ssh process behind it.
pub(super) struct Connection {
    session: Session,
    fs: SftpFs,
    process: ChannelProcess,
}

/// Connects to one host, serves its listings, and shuts the connection down.
pub(super) struct HostTask {
    pub(super) context: Arc<Context>,
    pub(super) host: String,
    pub(super) connection: u64,
    pub(super) stop: CancellationToken,
    pub(super) done: mpsc::UnboundedSender<Done>,
}

impl HostTask {
    pub(super) async fn run(self, askpass: Result<AskpassEnv, String>) {
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
            Ok(fs) => {
                // Without multiplexing the channel connected; with it, the master did.
                process.stderr().forget();
                Ok(Connection {
                    session,
                    fs,
                    process,
                })
            }
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
                    HostRequest::Share(reply) => {
                        let _ = reply.send(Arc::clone(&fs));
                    }
                    HostRequest::Command { dir, shell, command, reply } => {
                        let built = session.command_in(dir.as_bytes(), shell.words(), &command).map_err(|error| {
                            describe::ssh_error(&error)
                                .unwrap_or_else(|| fl!("error-connection-closed"))
                        });
                        let _ = reply.send(built);
                    }
                    HostRequest::Spawn { dir, program, args, reply } => {
                        let started = session.spawn_in(dir.as_bytes(), program, &args).map_err(|error| {
                            describe::ssh_error(&error)
                                .unwrap_or_else(|| fl!("error-connection-closed"))
                        });
                        let _ = reply.send(started);
                    }
                },
                Some(done) = running.next() => {
                    let _ = self.done.send(done);
                }
                () = self.stop.cancelled() => break None,
                () = session.closed() => {
                    break Some(describe::lost(&session.stderr(), session.exit_status()));
                }
                // Without multiplexing the channel is the connection.
                status = process.wait() => {
                    break Some(describe::lost(&process.stderr().text(), status));
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
            .map_err(|error| Unmade::of(&error));
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
    /// reply, or the failure, names it as an absolute path.
    pub(super) async fn list(&self, fs: &SftpFs, panel: PanelId, request: ListRequest) -> Done {
        let result = async {
            let Location::Remote { path, .. } = &request.location else {
                return Err(ListError::other(fl!("error-connection-closed")));
            };
            if !path.as_bytes().is_empty() {
                return self
                    .list_dir(fs, path.clone())
                    .await
                    .map_err(|error| ListError::vfs(&error));
            }
            if let Some(dir) = &request.resume
                && let Ok(listed) = self.list_dir(fs, dir.clone()).await
            {
                return Ok(listed);
            }
            let path = self.start_dir(fs).await?;
            self.list_dir(fs, path.clone())
                .await
                .map_err(|error| ListError::vfs(&error).at(self.remote(path)))
        }
        .await;
        Done::Listed {
            panel,
            generation: request.generation,
            result,
        }
    }

    /// The directory the host opens in, with no directory to resume: its `start_dir`, else the
    /// remote home directory. A `start_dir` that cannot be resolved fails at its path, which
    /// goes from the home directory if it is relative.
    async fn start_dir(&self, fs: &SftpFs) -> Result<RemotePath, ListError> {
        let hosts = self.context.hosts();
        let Some(dir) = hosts.get(&self.host).and_then(|host| host.start_dir()) else {
            return fs.home().await.map_err(|error| ListError::vfs(&error));
        };
        let dir = RemotePath::from(dir);
        let error = match fs.canonicalize(&dir).await {
            Ok(path) => return Ok(path),
            Err(error) => ListError::vfs(&error),
        };
        let path = if dir.is_absolute() {
            Some(dir)
        } else {
            fs.home().await.ok().map(|home| home.join(dir.as_bytes()))
        };
        Err(match path {
            Some(path) => error.at(self.remote(path)),
            None => error,
        })
    }

    async fn list_dir(&self, fs: &SftpFs, path: RemotePath) -> Result<Listed, VfsError> {
        // No timeout, unlike locally: the server serves requests in order, so a statvfs that
        // hangs there would hold up the listing anyway.
        let (space, entries) = tokio::join!(space(fs, &path), fs.list_dir(&path));
        Ok(Listed {
            location: self.remote(path),
            listing: Listing::Dir(entries?),
            space,
        })
    }

    /// `path` on this host.
    fn remote(&self, path: RemotePath) -> Location {
        Location::Remote {
            host: self.host.clone(),
            path,
        }
    }
}
