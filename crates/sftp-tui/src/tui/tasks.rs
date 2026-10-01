//! Background work for the app: listings, and one task per connected host that owns its ssh
//! session and SFTP channel.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt as _;
use futures_util::stream::FuturesUnordered;
use sftp_tui_ssh::askpass::{AskpassEnv, AskpassEvent, AskpassServer};
use sftp_tui_ssh::version::check_version;
use sftp_tui_ssh::{ChannelProcess, Session, SftpChannel, SshError, cleanup_stale};
use sftp_tui_vfs::{LocalFs, Location, RemotePath, SftpFs, Vfs as _, VfsError};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::app::{Effect, Side};
use super::describe;
use super::panel::{ListRequest, Listed, Listing};
use super::root;
use crate::context::Context;
use crate::i18n::fl;

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
    /// The host of [`Effect::Connect`] is connected; its listings go through `handle`.
    Connected {
        host: String,
        connection: u64,
        handle: HostHandle,
    },
    /// The attempt or connection of [`Effect::Connect`] ended; `reason` is `None` if it was
    /// asked to stop.
    Closed {
        host: String,
        connection: u64,
        reason: Option<String>,
    },
}

/// Passes listing requests to the task of a connected host.
#[derive(Debug, Clone)]
pub(crate) struct HostHandle(mpsc::UnboundedSender<(Side, ListRequest)>);

#[cfg(test)]
impl HostHandle {
    /// A handle and what it receives.
    pub(crate) fn channel() -> (Self, mpsc::UnboundedReceiver<(Side, ListRequest)>) {
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
}

impl Tasks {
    /// Prepares the runtime directory and the askpass bridge, and cleans up after crashed
    /// instances in the background. A failure here only affects connections.
    pub(crate) async fn start(context: Arc<Context>, done: mpsc::UnboundedSender<Done>) -> Self {
        let askpass = prepare_ssh(&context).await;
        if let Err(reason) = &askpass {
            tracing::warn!(%reason, "cannot prepare for ssh connections");
        }
        Self {
            context,
            done,
            askpass,
            hosts: JoinSet::new(),
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
                    let _ = handle.0.send((side, request));
                }
                Effect::List {
                    side,
                    request,
                    host: None,
                } => {
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
                Effect::Connect {
                    host,
                    connection,
                    stop,
                } => {
                    let askpass = match &self.askpass {
                        Ok(server) => Ok(server.env(&host)),
                        Err(reason) => Err(reason.clone()),
                    };
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

    /// Waits a while for the host tasks, which the app has told to stop, to shut down.
    pub(crate) async fn shutdown(mut self) {
        let all = async { while self.hosts.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_TIMEOUT, all).await.is_err() {
            tracing::warn!("some connections did not shut down in time");
        }
    }
}

/// The runtime directory and the askpass bridge. Until there are dialogs for them, prompts are
/// declined, so ssh fails as it would without a way to ask.
async fn prepare_ssh(context: &Context) -> Result<AskpassServer, String> {
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
            if let AskpassEvent::Prompt(prompt) = event {
                tracing::info!(host = %prompt.context, "declining an ssh prompt");
                prompt.cancel();
            }
        }
    });
    Ok(server)
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

    /// Serves listings until the connection is lost or asked to stop, then shuts it down.
    /// Returns why it ended: `None` if asked to stop.
    async fn serve(&self, connection: Connection) -> Option<String> {
        let Connection {
            session,
            fs,
            mut process,
        } = connection;
        let (requests, mut incoming) = mpsc::unbounded_channel();
        let _ = self.done.send(Done::Connected {
            host: self.host.clone(),
            connection: self.connection,
            handle: HostHandle(requests),
        });
        let mut running = FuturesUnordered::new();
        let reason = loop {
            tokio::select! {
                Some((side, request)) = incoming.recv() => {
                    running.push(self.list(&fs, side, request));
                }
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
        // Listings in flight are abandoned; dropping them is safe.
        drop(running);
        drop(incoming);
        let _ = tokio::time::timeout(SFTP_CLOSE_TIMEOUT, fs.close()).await;
        session.close().await;
        process.finish().await;
        reason
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
