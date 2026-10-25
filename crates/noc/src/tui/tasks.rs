//! Background work for the app: listings, new directories, jobs, and one task per connected
//! host that owns its ssh session and SFTP channel.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::ops::ControlFlow;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt as _;
use futures_util::future::{BoxFuture, FutureExt as _};
use futures_util::stream::FuturesUnordered;
use noc_config::{
    Config, ConfigError, History, HistoryEntry, HostConfig, Hosts, Recent, Workspaces, add_command,
    remove_command, remove_workspace, rename_workspace, save_config, save_host, save_workspace,
};
use noc_locale::Formats;
use noc_ops::{
    Algorithm, Checksum, Conflict, CopyOptions, Decision, Endpoint, Event, Files, Outcome,
    RenameError, Reporter, Sum,
};
use noc_ssh::askpass::{AskpassEnv, AskpassEvent, AskpassServer};
use noc_ssh::resolve::resolve;
use noc_ssh::version::check_version;
use noc_ssh::{CachedHost, ChannelProcess, Session, SftpChannel, SshError, Target, cleanup_stale};
use noc_tools::ToolError;
use noc_tools::fd::{self, Entries, Fd, Names};
use noc_tools::opener::Opener;
use noc_tools::ripgrep::{self, Hits, Query, Ripgrep};
use noc_tools::zoxide::Scored;
use noc_vfs::{
    FileKind, FileWriter as _, LocalFs, Location, Metadata, RemotePath, SftpFs, Space, Vfs,
    VfsError, VfsPath as _,
};
use noc_viewer::{Part, Progress, Want, read_part};
use tokio::io::AsyncReadExt as _;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use super::app::{Effect, FindRequest, HistoryChange, ReadRequest, RecentChange, WorkspaceChange};
use super::describe;
use super::dialog::{Ask, Reply};
use super::find::{FindQuery, FindSummary, Finding};
use super::panel::{ListError, ListRequest, Listed, Listing};
use super::places::{self, Places};
use super::tabs::PanelId;
use crate::about::{self, Detected, Tool};
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
        panel: PanelId,
        generation: u64,
        result: Result<Listed, ListError>,
    },
    /// The volumes and hosts of [`Effect::ListPlaces`], for the location menu.
    Places {
        generation: u64,
        result: Result<Places, String>,
    },
    /// A report from the job `id` of [`Effect::Delete`] or [`Effect::Copy`].
    Job { id: u64, event: JobEvent },
    /// The part of the file of [`Effect::Read`], or why not.
    Read {
        id: u64,
        result: Result<Part, String>,
    },
    /// How far [`Effect::Read`] is with its part.
    ReadProgress { id: u64, progress: Progress },
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
    /// `ssh -G` gave the address and the user of a host that [`Effect::Connect`] connects to.
    Resolved {
        host: String,
        address: String,
        user: String,
    },
    /// The attempt or connection of [`Effect::Connect`] ended; `reason` is `None` if it was
    /// asked to stop.
    Closed {
        host: String,
        connection: u64,
        reason: Option<String>,
    },
    /// The entry of [`Effect::Rename`] is renamed, or why not: `None` if a file has the name.
    Renamed {
        panel: PanelId,
        from: Location,
        to: Location,
        result: Result<(), Option<String>>,
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
    /// The workspaces as `workspaces.toml` holds them after an [`Effect::Workspaces`], or why
    /// it could not be read, or changed if `changed`.
    Workspaces {
        changed: bool,
        result: Result<Workspaces, String>,
    },
    /// The commands of `history.toml`, oldest first, after a change, or why it could not be
    /// read or written.
    History(Result<Vec<HistoryEntry>, String>),
    /// The hosts and volumes of `recent.toml`, of [`RecentChange::Load`], or why they could not
    /// be read.
    Recent(Result<Recent, String>),
    /// The names of the [`Effect::ListNames`] `generation`: each with whether it is a
    /// directory, or why there are none; and the aliases of the hosts.
    Names {
        generation: u64,
        entries: Result<Vec<(Vec<u8>, bool)>, String>,
        hosts: Vec<String>,
    },
    /// What a tool said it is, after an [`Effect::Detect`].
    Detected(Tool, Detected),
    /// The formats of [`Effect::Formats`], made.
    Formats(Box<Formats>),
    /// zoxide's answer to the [`Effect::ZoxideQuery`] `generation`, or why there is none.
    Jumps {
        generation: u64,
        result: Result<Vec<Scored>, String>,
    },
    /// What the search `generation` of [`Effect::Find`] found since its last report.
    Found {
        generation: u64,
        found: Vec<Finding>,
    },
    /// How the search `generation` of [`Effect::Find`] ended, after what it found, or why it
    /// could not search.
    FindEnded {
        generation: u64,
        result: Result<FindSummary, String>,
    },
    /// The path of [`Effect::Open`] that did not open, and why; the ones after it were not
    /// tried.
    OpenFailed { path: PathBuf, reason: String },
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
    /// The ssh command that runs `command` in `dir` on the host, for the event loop to start
    /// with the terminal handed over, or why there is none.
    Command {
        dir: RemotePath,
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
    /// Reads and changes of `workspaces.toml`, which one task makes in turn.
    workspace_changes: mpsc::UnboundedSender<WorkspaceChange>,
    /// Reads and changes of `history.toml`, which one task makes in turn.
    history_changes: mpsc::UnboundedSender<HistoryChange>,
    /// Reads and writes of `recent.toml`, which one task makes in turn.
    recent_changes: mpsc::UnboundedSender<RecentChange>,
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
        let workspace_changes = change_workspaces(context.paths.workspaces_file(), done.clone());
        let history_changes = change_history(context.paths.history_file(), done.clone());
        let recent_changes = change_recent(context.paths.recent_file(), done.clone());
        let zoxide_adds = add_to_zoxide(Arc::clone(&context));
        Self {
            context,
            done,
            askpass,
            hosts: JoinSet::new(),
            jobs: JoinSet::new(),
            cache: Arc::new(Mutex::new(())),
            config_saves,
            workspace_changes,
            history_changes,
            recent_changes,
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
                Effect::Rename {
                    panel,
                    from,
                    to,
                    replace,
                    host,
                } => self.rename(panel, from, to, replace, host),
                Effect::Discard(path) => discard(path),
                Effect::Read(request) => self.read(request),
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
                } => self.copy(id, sources, target, hosts, options, cancel),
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
                Effect::Open(paths) => self.open(paths),
                Effect::ListNames {
                    generation,
                    dir,
                    host,
                    hosts,
                } => self.list_names(generation, dir, host, hosts),
                Effect::ListCommands {
                    generation,
                    dirs,
                    prefix,
                    with_dirs,
                } => self.list_commands(generation, dirs, prefix, with_dirs),
                Effect::Detect(tool) => self.detect(tool),
                Effect::Formats(setting) => self.formats(setting),
                Effect::ZoxideQuery {
                    generation,
                    keywords,
                    exclude,
                } => self.query_zoxide(generation, keywords, exclude),
                Effect::Find(request) => self.find(request),
                Effect::SaveConfig { old, new, config } => self.save_config(*old, *new, *config),
                Effect::Workspaces(change) => self.change_workspaces(change),
                Effect::History(change) => {
                    let _ = self.history_changes.send(change);
                }
                Effect::Recent(change) => {
                    let _ = self.recent_changes.send(change);
                }
            }
        }
    }

    /// Runs the copy or move job `id` in a task of its own.
    fn copy(
        &mut self,
        id: u64,
        sources: Vec<Location>,
        target: Location,
        hosts: (Option<HostHandle>, Option<HostHandle>),
        options: CopyOptions,
        cancel: CancellationToken,
    ) {
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

    /// Renames `from` to `to` in a task of its own, through the session of `host` if they are
    /// remote.
    fn rename(
        &self,
        panel: PanelId,
        from: Location,
        to: Location,
        replace: bool,
        host: Option<HostHandle>,
    ) {
        let done = self.done.clone();
        tokio::spawn(async move {
            let result = rename(&from, &to, replace, host).await;
            let _ = done.send(Done::Renamed {
                panel,
                from,
                to,
                result,
            });
        });
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

    /// Uses `config` from now on, and writes to the config file the settings that differ
    /// between `old` and `new`, after the changes before.
    fn save_config(&self, old: Config, new: Config, config: Config) {
        self.context.set_config(config);
        let _ = self.config_saves.send((old, new));
    }

    /// Reads or changes `workspaces.toml`, after the changes before.
    fn change_workspaces(&self, change: WorkspaceChange) {
        let _ = self.workspace_changes.send(change);
    }

    /// Lists the names in `dir` and, with `hosts`, the hosts of the ssh config, for completion.
    /// A remote directory is read through the session of its host; with no host to read it
    /// through, it holds nothing.
    fn list_names(
        &self,
        generation: u64,
        dir: Option<Location>,
        host: Option<HostHandle>,
        hosts: bool,
    ) {
        let context = Arc::clone(&self.context);
        let done = self.done.clone();
        tokio::spawn(async move {
            let entries = names_in(dir, host);
            let aliases = async {
                if !hosts {
                    return Vec::new();
                }
                tokio::task::spawn_blocking(move || places::read_hosts(&context))
                    .await
                    .map(|hosts| hosts.into_iter().map(|host| host.alias).collect())
                    .unwrap_or_default()
            };
            let (entries, hosts) = tokio::join!(entries, aliases);
            let _ = done.send(Done::Names {
                generation,
                entries,
                hosts,
            });
        });
    }

    /// Lists the commands in `dirs` whose names start with `prefix` regardless of case, for
    /// completion.
    fn list_commands(&self, generation: u64, dirs: Vec<PathBuf>, prefix: String, with_dirs: bool) {
        let done = self.done.clone();
        tokio::spawn(async move {
            let entries =
                tokio::task::spawn_blocking(move || commands_in(&dirs, &prefix, with_dirs))
                    .await
                    .map_err(|error| error.to_string());
            let _ = done.send(Done::Names {
                generation,
                entries,
                hosts: Vec::new(),
            });
        });
    }

    /// Asks `tool` what it is.
    fn detect(&self, tool: Tool) {
        let done = self.done.clone();
        let detection: BoxFuture<'static, Detected> = match tool {
            Tool::Zoxide => {
                let zoxide = self.context.zoxide();
                async move { about::detect_zoxide(&zoxide).await }.boxed()
            }
            Tool::Ripgrep => {
                let ripgrep = self.context.ripgrep();
                async move { about::detect_ripgrep(&ripgrep).await }.boxed()
            }
            Tool::Fd => {
                let fd = self.context.fd();
                async move { about::detect_fd(&fd).await }.boxed()
            }
        };
        tokio::spawn(async move {
            let _ = done.send(Done::Detected(tool, detection.await));
        });
    }

    /// Makes the formats `setting` names, which reads the environment and asks the system.
    fn formats(&self, setting: String) {
        let done = self.done.clone();
        tokio::spawn(async move {
            if let Ok(formats) =
                tokio::task::spawn_blocking(move || Formats::detect(&setting)).await
            {
                let _ = done.send(Done::Formats(Box::new(formats)));
            }
        });
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

    /// Searches the files under `location` with ripgrep, or their names with fd, after `delay`,
    /// in a task of its own, through the session of `host` if it is remote, until `cancel`.
    fn find(&self, request: FindRequest) {
        let FindRequest {
            generation,
            location,
            host,
            query,
            max_results,
            delay,
            cancel,
        } = request;
        let (ripgrep, fd) = (self.context.ripgrep(), self.context.fd());
        let done = self.done.clone();
        tokio::spawn(async move {
            let search = async {
                tokio::time::sleep(delay).await;
                let mut batches = Batches {
                    generation,
                    found: Vec::new(),
                    sent: None,
                    done: done.clone(),
                };
                let result = match &query {
                    FindQuery::Lines(query) => {
                        let searched = Searched { query, max_results };
                        searched
                            .search(&ripgrep, location, host, &mut batches)
                            .await
                    }
                    FindQuery::Names(query) => {
                        let named = Named { query, max_results };
                        named.search(&fd, location, host, &mut batches).await
                    }
                };
                batches.flush();
                result
            };
            let result = tokio::select! {
                () = cancel.cancelled() => return,
                result = search => result,
            };
            let _ = done.send(Done::FindEnded { generation, result });
        });
    }

    /// Opens `paths` with the programs of the desktop, one after another, until one fails.
    fn open(&self, paths: Vec<PathBuf>) {
        let done = self.done.clone();
        tokio::spawn(async move {
            let opener = Opener::default();
            for path in paths {
                if let Err(error) = opener.open(&path).await {
                    let reason = describe::open_error(&error);
                    let _ = done.send(Done::OpenFailed { path, reason });
                    return;
                }
            }
        });
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

    /// Reads the volumes and the hosts for the location menu.
    fn list_places(&self, generation: u64) {
        let context = Arc::clone(&self.context);
        let done = self.done.clone();
        tokio::spawn(async move {
            let result = places::read_places(context).await;
            let _ = done.send(Done::Places { generation, result });
        });
    }

    /// Lists a local directory here.
    fn list(&self, panel: PanelId, request: ListRequest) {
        let done = self.done.clone();
        tokio::spawn(async move {
            let generation = request.generation;
            let result = list(request.location).await;
            let _ = done.send(Done::Listed {
                panel,
                generation,
                result,
            });
        });
    }

    /// Reads a part of a file for the viewer, reporting how far it is at most every
    /// [`FIND_REPORTS`].
    fn read(&self, request: ReadRequest) {
        let ReadRequest {
            id,
            location,
            host,
            want,
            stop,
            cancel,
        } = request;
        let done = self.done.clone();
        tokio::spawn(async move {
            let mut sent: Option<Instant> = None;
            let report = |progress: Progress| {
                if stop.is_cancelled() {
                    return ControlFlow::Break(());
                }
                // The last report always goes, so that the gauge ends full.
                let last = progress.read >= progress.total;
                if last || sent.is_none_or(|sent| sent.elapsed() >= FIND_REPORTS) {
                    sent = Some(Instant::now());
                    let _ = done.send(Done::ReadProgress { id, progress });
                }
                ControlFlow::Continue(())
            };
            let result = tokio::select! {
                result = read(location, host, want, report) => result,
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
            (Location::Remote { .. }, None) => {
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

/// Starts the task that reads and changes `workspaces.toml` at `path`, in the order the
/// changes come, and reports each to `done`.
fn change_workspaces(
    path: PathBuf,
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<WorkspaceChange> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<WorkspaceChange>();
    tokio::spawn(async move {
        while let Some(change) = receiver.recv().await {
            let path = path.clone();
            let changed = change != WorkspaceChange::Load;
            let result = tokio::task::spawn_blocking(move || match change {
                WorkspaceChange::Load => Workspaces::load(&path),
                WorkspaceChange::Save(workspace) => save_workspace(&path, &workspace),
                WorkspaceChange::Remove(name) => remove_workspace(&path, &name),
                WorkspaceChange::Rename { from, to } => rename_workspace(&path, &from, &to),
            })
            .await;
            let result = match result {
                Ok(result) => result.map_err(|error| describe::chain(&error)),
                Err(error) => Err(error.to_string()),
            };
            if done.send(Done::Workspaces { changed, result }).is_err() {
                break;
            }
        }
    });
    sender
}

/// Starts the task that reads and changes `history.toml` at `path`, in the order the changes
/// come, and reports each to `done`.
fn change_history(
    path: PathBuf,
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<HistoryChange> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<HistoryChange>();
    tokio::spawn(async move {
        while let Some(change) = receiver.recv().await {
            let path = path.clone();
            let result = tokio::task::spawn_blocking(move || match change {
                HistoryChange::Load => History::load(&path),
                HistoryChange::Add { entry, size } => add_command(&path, &entry, size),
                HistoryChange::Remove { host, command } => {
                    remove_command(&path, host.as_deref(), &command)
                }
            })
            .await;
            let result = match result {
                Ok(result) => result
                    .map(|history| history.commands)
                    .map_err(|error| describe::chain(&error)),
                Err(error) => Err(error.to_string()),
            };
            if done.send(Done::History(result)).is_err() {
                break;
            }
        }
    });
    sender
}

/// Starts the task that reads and writes `recent.toml` at `path`, in the order the changes
/// come, and reports what it read to `done`; a failed write only goes to the log.
fn change_recent(
    path: PathBuf,
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<RecentChange> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<RecentChange>();
    tokio::spawn(async move {
        while let Some(change) = receiver.recv().await {
            let path = path.clone();
            let result = tokio::task::spawn_blocking(move || match change {
                RecentChange::Load => Recent::load(&path).map(Some),
                RecentChange::Save(recent) => recent.save(&path).map(|()| None),
            })
            .await;
            let result = match result {
                Ok(Ok(None)) => continue,
                Ok(Ok(Some(recent))) => Ok(recent),
                Ok(Err(error)) => Err(describe::chain(&error)),
                Err(error) => Err(error.to_string()),
            };
            if let Err(reason) = &result {
                tracing::warn!(%reason, "cannot read or write the recent places");
            }
            if done.send(Done::Recent(result)).is_err() {
                break;
            }
        }
    });
    sender
}

/// The names in `dir`, each with whether it is a directory, read through `host` if it is
/// remote; nothing without a host to read it through.
async fn names_in(
    dir: Option<Location>,
    host: Option<HostHandle>,
) -> Result<Vec<(Vec<u8>, bool)>, String> {
    let entries = match dir {
        Some(Location::Local(path)) => LocalFs.list_dir(&path).await,
        Some(Location::Remote { path, .. }) => {
            let Some(Some(fs)) = share(host).await else {
                return Ok(Vec::new());
            };
            // The empty path is the home directory, where the session starts.
            let path = if path.as_bytes().is_empty() {
                RemotePath::from(".")
            } else {
                path
            };
            fs.list_dir(&path).await
        }
        None => return Ok(Vec::new()),
    };
    entries
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| {
                    let dir = entry.is_dir_like();
                    (entry.name, dir)
                })
                .collect()
        })
        .map_err(|error| describe::vfs_error(&error))
}

/// Removes a temporary file, if it is there, in a task of its own.
fn discard(path: PathBuf) {
    tokio::spawn(async move {
        if let Err(error) = tokio::fs::remove_file(&path).await {
            tracing::debug!(path = %path.display(), %error, "cannot remove");
        }
    });
}

/// The commands in `dirs` whose names start with `prefix` regardless of case: the regular files,
/// and links to them, with an execute bit, each name once; `with_dirs`, the directories too, as
/// vim's `:!` offers them for a path. Directories that are not absolute, or cannot be read,
/// hold none. Blocks.
fn commands_in(dirs: &[PathBuf], prefix: &str, with_dirs: bool) -> Vec<(Vec<u8>, bool)> {
    let prefix = prefix.to_lowercase();
    let mut names = BTreeMap::new();
    for dir in dirs.iter().filter(|dir| dir.is_absolute()) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            // Names that are not UTF-8 cannot be typed.
            let Some(name) = name.to_str() else {
                continue;
            };
            if names.contains_key(name) || !name.to_lowercase().starts_with(&prefix) {
                continue;
            }
            // Follows links, as the shell does.
            let Ok(metadata) = std::fs::metadata(entry.path()) else {
                continue;
            };
            let runs = metadata.is_file() && metadata.permissions().mode() & 0o111 != 0;
            let dir = with_dirs && metadata.is_dir();
            if runs || dir {
                names.insert(name.to_owned(), dir);
            }
        }
    }
    names
        .into_iter()
        .map(|(name, dir)| (name.into_bytes(), dir))
        .collect()
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

/// Runs `ssh -G` for a host that is being connected to, as the location menu shows effective
/// addresses only from there: reports the address and stores it in the cache. Failures only go to the
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
                            Location::Remote { .. } => None,
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
                            Location::Local(_) => None,
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

/// Renames `from` to `to`, in the same directory; `Err(None)` if a file has the name and not
/// `replace`.
async fn rename(
    from: &Location,
    to: &Location,
    replace: bool,
    host: Option<HostHandle>,
) -> Result<(), Option<String>> {
    let result = match (from, to, share(host).await) {
        (Location::Local(from), Location::Local(to), _) => {
            noc_ops::rename(&LocalFs, from, to, replace).await
        }
        (
            Location::Remote { path: from, .. },
            Location::Remote { path: to, .. },
            Some(Some(fs)),
        ) => noc_ops::rename(&*fs, from, to, replace).await,
        _ => return Err(Some(fl!("error-connection-closed"))),
    };
    match result {
        Ok(()) => Ok(()),
        Err(RenameError::Exists(FileKind::Dir)) => Err(Some(fl!("rename-dir-exists"))),
        Err(RenameError::Exists(_)) => Err(None),
        Err(RenameError::Vfs(error)) => Err(Some(describe::vfs_error(&error))),
    }
}

/// The ssh command that runs `command` in `dir` on the host of `handle`, over its connection,
/// or why there is none.
pub(crate) async fn remote_command(
    handle: &HostHandle,
    dir: RemotePath,
    command: String,
) -> Result<tokio::process::Command, String> {
    let (reply, answer) = oneshot::channel();
    let request = HostRequest::Command {
        dir,
        command,
        reply,
    };
    let closed = || fl!("error-connection-closed");
    handle.0.send(request).map_err(|_| closed())?;
    answer.await.map_err(|_| closed())?
}

/// The part `want` of the file at `location`; `report` hears how far it is.
async fn read(
    location: Location,
    host: Option<HostHandle>,
    want: Want,
    report: impl FnMut(Progress) -> ControlFlow<()>,
) -> Result<Part, String> {
    let result = match (location, share(host).await) {
        (Location::Local(path), _) => read_part(&LocalFs, &path, want, report).await,
        (Location::Remote { path, .. }, Some(Some(fs))) => {
            read_part(&*fs, &path, want, report).await
        }
        _ => return Err(fl!("error-connection-closed")),
    };
    result.map_err(|error| describe::vfs_error(&error))
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

/// How often a search of a tree reports what it found, at most: the window takes its rows
/// then, not after each line.
const FIND_REPORTS: Duration = Duration::from_millis(50);
/// ripgrep on a host, as the host's `PATH` finds it.
const REMOTE_RIPGREP: &str = "rg";
/// fd on a host, as the host's `PATH` finds it; Debian and Ubuntu name it `fdfind`.
const REMOTE_FD: [&str; 2] = ["fd", "fdfind"];
/// The most of the stderr of ripgrep or fd that is kept, for why it failed.
const SEARCH_STDERR: u64 = 64 * 1024;

/// What ripgrep or fd found and has not been reported yet.
struct Batches {
    generation: u64,
    found: Vec<Finding>,
    /// When it reported last.
    sent: Option<Instant>,
    done: mpsc::UnboundedSender<Done>,
}

impl Batches {
    /// Takes what was found, and reports it with what came before if the last report was long
    /// enough ago.
    fn take(&mut self, finding: Finding) {
        self.found.push(finding);
        if self.sent.is_none_or(|sent| sent.elapsed() >= FIND_REPORTS) {
            self.flush();
        }
    }

    /// Reports what was found and not reported yet, if anything.
    fn flush(&mut self) {
        if self.found.is_empty() {
            return;
        }
        self.sent = Some(Instant::now());
        let found = std::mem::take(&mut self.found);
        let _ = self.done.send(Done::Found {
            generation: self.generation,
            found,
        });
    }
}

/// A search of a tree with ripgrep: what it looks for, and the most results it takes.
struct Searched<'a> {
    query: &'a Query,
    max_results: usize,
}

impl Searched<'_> {
    /// Runs ripgrep in `location`, locally as `ripgrep`, or on its host through `host`, and
    /// passes what it finds to `batches`; then how it ended, or why it could not search.
    async fn search(
        &self,
        ripgrep: &Ripgrep,
        location: Location,
        host: Option<HostHandle>,
        batches: &mut Batches,
    ) -> Result<FindSummary, String> {
        let (mut child, program) = match location {
            Location::Local(path) => {
                let child = ripgrep
                    .spawn(&path, self.query)
                    .map_err(|error| describe::ripgrep_error(&error))?;
                (child, ripgrep.program().to_path_buf())
            }
            Location::Remote { path, .. } => {
                let handle = host.ok_or_else(|| fl!("error-connection-closed"))?;
                let args = self.query.args();
                let child = remote_spawn(&handle, path, REMOTE_RIPGREP, args).await?;
                (child, PathBuf::from(REMOTE_RIPGREP))
            }
        };
        let closed = || fl!("error-connection-closed");
        let stdout = child.stdout.take().ok_or_else(closed)?;
        let stderr = child.stderr.take().ok_or_else(closed)?;
        // ripgrep stops once a pipe is full: its stderr is read while its stdout is.
        let stderr = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(SEARCH_STDERR).read_to_end(&mut bytes).await;
            bytes
        });
        let limit = self.read(stdout, batches).await?;
        if limit {
            // What is left is never read: ripgrep goes.
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Ok(FindSummary {
                limit: true,
                ..FindSummary::default()
            });
        }
        let status = child.wait().await.map_err(|error| error.to_string())?;
        let stderr = stderr.await.unwrap_or_default();
        let ended = ripgrep::ended(&program, status, &stderr)
            .map_err(|error| describe::ripgrep_error(&error))?;
        Ok(FindSummary {
            limit: false,
            errors: ended.errors,
            last_error: ended.last_error,
        })
    }

    /// Reads what ripgrep prints into `batches`, reporting them at least every
    /// [`FIND_REPORTS`] while it is slow; `true` once it found `max_results`.
    async fn read(
        &self,
        stdout: impl tokio::io::AsyncRead + Unpin,
        batches: &mut Batches,
    ) -> Result<bool, String> {
        let files = self.query.lists_files();
        let mut hits = Hits::new(stdout, files);
        let mut results = 0;
        loop {
            // Reading is cancel-safe: what a timeout cut off is read on the next turn.
            let Ok(next) = tokio::time::timeout(FIND_REPORTS, hits.next()).await else {
                batches.flush();
                continue;
            };
            let Some(hit) = next.map_err(|error| describe::chain(&error))? else {
                return Ok(false);
            };
            let counts = files || matches!(hit, ripgrep::Hit::Line { .. });
            batches.take(Finding::Hit(hit));
            if counts {
                results += 1;
                if results >= self.max_results {
                    return Ok(true);
                }
            }
        }
    }
}

/// A search of a tree for names with fd: what it looks for, and the most it takes.
struct Named<'a> {
    query: &'a fd::Query,
    max_results: usize,
}

impl Named<'_> {
    /// Runs fd in `location`, locally as `fd`, or on its host through `host`, as `fd` or, where
    /// the host has none, as `fdfind`, and passes what it finds to `batches`; then how it
    /// ended, or why it could not search. A glob or a pattern that is not one says so in place
    /// of the rows, as ripgrep's error does.
    async fn search(
        &self,
        fd: &Fd,
        location: Location,
        host: Option<HostHandle>,
        batches: &mut Batches,
    ) -> Result<FindSummary, String> {
        let names = match self.query.names() {
            Ok(names) => names,
            Err(error) => {
                return Ok(FindSummary {
                    limit: false,
                    errors: 1,
                    last_error: Some(error.to_string()),
                });
            }
        };
        let ended = match location {
            Location::Local(path) => {
                let child = fd
                    .spawn(&path, self.query)
                    .map_err(|error| describe::fd_error(&error))?;
                self.run(child, fd.program(), &names, batches).await?
            }
            Location::Remote { path, .. } => {
                let handle = host.ok_or_else(|| fl!("error-connection-closed"))?;
                let mut ended = Ok(FindSummary::default());
                for program in REMOTE_FD {
                    let args = self.query.args();
                    let child = remote_spawn(&handle, path.clone(), program, args).await?;
                    ended = self.run(child, Path::new(program), &names, batches).await?;
                    // The host's shell says 127 for a program it does not find.
                    let missing = matches!(&ended, Err(ToolError::Failed { status, .. })
                        if status.code() == Some(127));
                    if !missing {
                        break;
                    }
                }
                ended
            }
        };
        ended.map_err(|error| describe::fd_error(&error))
    }

    /// Reads what `child`, fd as `program`, prints, its stderr at the same time, and how it
    /// ended: why reading failed, or else how fd ended, or that it stopped at the limit.
    async fn run(
        &self,
        mut child: tokio::process::Child,
        program: &Path,
        names: &Names,
        batches: &mut Batches,
    ) -> Result<Result<FindSummary, ToolError>, String> {
        let closed = || fl!("error-connection-closed");
        let stdout = child.stdout.take().ok_or_else(closed)?;
        let stderr = child.stderr.take().ok_or_else(closed)?;
        let stderr = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(SEARCH_STDERR).read_to_end(&mut bytes).await;
            bytes
        });
        if self.read(stdout, names, batches).await? {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Ok(Ok(FindSummary {
                limit: true,
                ..FindSummary::default()
            }));
        }
        let status = child.wait().await.map_err(|error| error.to_string())?;
        let stderr = stderr.await.unwrap_or_default();
        Ok(
            fd::ended(program, status, &stderr).map(|ended| FindSummary {
                limit: false,
                errors: ended.errors,
                last_error: ended.last_error,
            }),
        )
    }

    /// Reads what fd prints into `batches`, the entries that `names` keeps, with where their
    /// names match, reporting them at least every [`FIND_REPORTS`] while it is slow; `true`
    /// once it found `max_results`.
    async fn read(
        &self,
        stdout: impl tokio::io::AsyncRead + Unpin,
        names: &Names,
        batches: &mut Batches,
    ) -> Result<bool, String> {
        let mut entries = Entries::new(stdout);
        let mut results = 0;
        loop {
            // Reading is cancel-safe: what a timeout cut off is read on the next turn.
            let Ok(next) = tokio::time::timeout(FIND_REPORTS, entries.next()).await else {
                batches.flush();
                continue;
            };
            let Some(entry) = next.map_err(|error| describe::chain(&error))? else {
                return Ok(false);
            };
            if !names.keeps(&entry.path) {
                continue;
            }
            let name = entry
                .path
                .iter()
                .rposition(|&byte| byte == b'/')
                .map_or(0, |slash| slash + 1);
            let matches = names
                .found(&entry.path[name..])
                .into_iter()
                .map(|found| found.start + name..found.end + name)
                .collect();
            batches.take(Finding::Entry {
                path: entry.path,
                dir: entry.dir,
                matches,
            });
            results += 1;
            if results >= self.max_results {
                return Ok(true);
            }
        }
    }
}

/// Starts `program` with `args` in `dir` on the host of `handle`, over its connection, or why
/// it cannot.
async fn remote_spawn(
    handle: &HostHandle,
    dir: RemotePath,
    program: &'static str,
    args: Vec<OsString>,
) -> Result<tokio::process::Child, String> {
    let (reply, answer) = oneshot::channel();
    let request = HostRequest::Spawn {
        dir,
        program,
        args,
        reply,
    };
    let closed = || fl!("error-connection-closed");
    handle.0.send(request).map_err(|_| closed())?;
    answer.await.map_err(|_| closed())?
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
            }
        }
        let on_host = |host: String| {
            move |path: &RemotePath| Location::Remote {
                host: host.clone(),
                path: path.clone(),
            }
        };
        let here = |path: &PathBuf| Location::Local(path.clone());
        match (from, to, target) {
            // Within one file system, a move renames, and neither goes over its source under a
            // name spelled otherwise.
            (None, None, Location::Local(target)) => {
                self.within(&LocalFs, local, target, &here).await
            }
            (Some(from), Some(_), Location::Remote { host, path }) if host == source_host => {
                let there = on_host(host);
                self.within(&*from, remote, path, &there).await
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

    /// Copies, or moves if the options remove sources, within one file system.
    async fn within<V: Vfs>(
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
            if options.remove_sources {
                noc_ops::move_within(side, sources, target, options, &mut reporter).await
            } else {
                noc_ops::copy_within(side, sources, target, options, &mut reporter).await
            }
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

/// Lists a local directory.
async fn list(location: Location) -> Result<Listed, ListError> {
    let Location::Local(path) = &location else {
        // The app sends remote ones to the host's task.
        return Err(ListError::other(fl!("error-connection-closed")));
    };
    // The listing does not wait for a file system that is slow to tell its space.
    let space = tokio::time::timeout(places::VOLUME_TIMEOUT, space(&LocalFs, path));
    let (entries, space) = tokio::join!(LocalFs.list_dir(path), space);
    let entries = entries.map_err(|error| ListError::vfs(&error))?;
    Ok(Listed {
        location,
        listing: Listing::Dir(entries),
        space: space.ok().flatten(),
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
                    HostRequest::Command { dir, command, reply } => {
                        let built = session.command_in(dir.as_bytes(), &command).map_err(|error| {
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
    /// reply, or the failure, names it as an absolute path.
    async fn list(&self, fs: &SftpFs, panel: PanelId, request: ListRequest) -> Done {
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Stdio;

    use noc_config::{Config, HostConfig, Hosts, Paths, SftpHost};
    use noc_tools::ripgrep::Hit;
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
            .stderr(Stdio::null())
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

    /// Where opening `web` lands, resuming `resume`; else where it failed, if the failure says.
    async fn opened(
        task: &HostTask,
        fs: &SftpFs,
        resume: Option<&Path>,
    ) -> Result<String, Option<String>> {
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
        let path = |location: Location| match location {
            Location::Remote { path, .. } => String::from_utf8_lossy(path.as_bytes()).into_owned(),
            other @ Location::Local(_) => panic!("expected a remote location, got {other:?}"),
        };
        result
            .map(|listed| path(listed.location))
            .map_err(|error| error.location.map(path))
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
        // A start_dir that is not there fails at its path, for Up and Go to.
        let absolute = text(&root.join("gone/deeper"));
        for (start_dir, path) in [
            ("missing", root.join("missing")),
            ("missing/deeper", root.join("missing/deeper")),
            (absolute.as_str(), root.join("gone/deeper")),
        ] {
            let missing = task(&root, Some(start_dir));
            assert_eq!(
                opened(&missing, &fs, None).await,
                Err(Some(text(&path))),
                "{start_dir}"
            );
        }
    }

    #[tokio::test]
    async fn names_to_complete_tell_directories_and_links_to_them() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("dir")).unwrap();
        std::fs::write(tmp.path().join("file"), "").unwrap();
        std::os::unix::fs::symlink("dir", tmp.path().join("link")).unwrap();
        let dir = Some(Location::Local(tmp.path().to_path_buf()));
        let mut names = names_in(dir, None).await.unwrap();
        names.sort();
        assert_eq!(
            names,
            [
                (b"dir".to_vec(), true),
                (b"file".to_vec(), false),
                (b"link".to_vec(), true)
            ]
        );
        let missing = Some(Location::Local(tmp.path().join("gone")));
        assert!(names_in(missing, None).await.is_err());
        let remote = Location::Remote {
            host: "web".to_owned(),
            path: RemotePath::from("/srv"),
        };
        assert_eq!(names_in(Some(remote), None).await, Ok(Vec::new()));
    }

    #[test]
    fn commands_are_the_executable_files_of_path_each_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, more) = (tmp.path().join("bin"), tmp.path().join("more"));
        for dir in [&bin, &more] {
            std::fs::create_dir(dir).unwrap();
        }
        let file = |path: PathBuf, mode: u32| {
            std::fs::write(&path, "").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        file(bin.join("mytool"), 0o755);
        file(bin.join("mynotes"), 0o644);
        file(bin.join("other"), 0o755);
        file(more.join("mytool"), 0o700);
        file(more.join("MyCase"), 0o100);
        std::fs::create_dir(bin.join("mydir")).unwrap();
        std::os::unix::fs::symlink("mytool", bin.join("mylink")).unwrap();
        std::os::unix::fs::symlink("mynotes", bin.join("mynotelink")).unwrap();
        std::os::unix::fs::symlink("gone", bin.join("mygone")).unwrap();
        let names = |dirs: &[PathBuf], prefix: &str| -> Vec<String> {
            commands_in(dirs, prefix, false)
                .into_iter()
                .map(|(name, dir)| {
                    assert!(!dir);
                    String::from_utf8(name).unwrap()
                })
                .collect()
        };
        let path = [
            bin.clone(),
            PathBuf::from("relative"),
            tmp.path().join("missing"),
            more,
        ];
        assert_eq!(names(&path, "my"), ["MyCase", "mylink", "mytool"]);
        assert_eq!(names(&path, "MYT"), ["mytool"], "regardless of case");
        assert_eq!(
            names(std::slice::from_ref(&bin), ""),
            ["mylink", "mytool", "other"]
        );
        // A path to a program offers the directories too, as vim's `:!` does.
        let with_dirs: Vec<(String, bool)> = commands_in(&[bin], "my", true)
            .into_iter()
            .map(|(name, dir)| (String::from_utf8(name).unwrap(), dir))
            .collect();
        let expected = [("mydir", true), ("mylink", false), ("mytool", false)];
        assert_eq!(
            with_dirs,
            expected.map(|(name, dir)| (name.to_owned(), dir))
        );
    }

    /// Searches `dir` for `pattern` with the rg in `PATH`, taking `max_results`: what it
    /// found, and how it ended; `None` without rg.
    async fn rg(
        dir: &std::path::Path,
        pattern: &str,
        max_results: usize,
    ) -> Option<(Vec<Hit>, Result<FindSummary, String>)> {
        let ripgrep = Ripgrep::new(PathBuf::from("rg"));
        let query = Query {
            pattern: pattern.to_owned(),
            globs: Vec::new(),
            regex: true,
            case: noc_pattern::Case::Smart,
            hidden: true,
            max_per_file: 1000,
        };
        let (done, mut reports) = mpsc::unbounded_channel();
        let mut batches = Batches {
            generation: 1,
            found: Vec::new(),
            sent: None,
            done,
        };
        let searched = Searched {
            query: &query,
            max_results,
        };
        let location = Location::Local(dir.to_path_buf());
        let result = searched
            .search(&ripgrep, location, None, &mut batches)
            .await;
        if result
            .as_ref()
            .is_err_and(|reason| reason.contains("not installed"))
        {
            return None;
        }
        batches.flush();
        Some((hits_of(&mut reports), result))
    }

    /// What ripgrep found, from the reports of a search.
    fn hits_of(reports: &mut mpsc::UnboundedReceiver<Done>) -> Vec<Hit> {
        let mut hits = Vec::new();
        while let Ok(report) = reports.try_recv() {
            if let Done::Found { found, .. } = report {
                hits.extend(found.into_iter().map(|finding| match finding {
                    Finding::Hit(hit) => hit,
                    entry @ Finding::Entry { .. } => panic!("expected a hit, got {entry:?}"),
                }));
            }
        }
        hits
    }

    #[tokio::test]
    async fn ripgrep_finds_lines_stops_at_the_limit_and_says_what_is_wrong() {
        let tmp = tempfile::tempdir().unwrap();
        let lines: String = (0..100)
            .map(|n| format!("needle {n}\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(tmp.path().join("many"), lines).unwrap();
        std::fs::write(tmp.path().join("one"), "a needle\n").unwrap();
        let Some((hits, result)) = rg(tmp.path(), "needle", 1000).await else {
            return;
        };
        assert_eq!(result.unwrap(), FindSummary::default());
        let lines = hits
            .iter()
            .filter(|hit| matches!(hit, Hit::Line { .. }))
            .count();
        assert_eq!(lines, 101);
        assert!(hits.contains(&Hit::File(b"one".to_vec())));
        let (hits, result) = rg(tmp.path(), "needle", 10).await.unwrap();
        assert!(result.unwrap().limit);
        let lines = hits
            .iter()
            .filter(|hit| matches!(hit, Hit::Line { .. }))
            .count();
        assert_eq!(lines, 10);
        let (hits, result) = rg(tmp.path(), "needle (", 10).await.unwrap();
        assert_eq!(hits, []);
        let summary = result.unwrap();
        assert!(
            summary
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("unclosed")),
            "{summary:?}"
        );
    }

    #[tokio::test]
    async fn on_a_host_ripgrep_runs_there_through_its_task() {
        let (handle, mut requests) = HostHandle::channel();
        let host = tokio::spawn(async move {
            let Some(HostRequest::Spawn {
                dir,
                program,
                args,
                reply,
            }) = requests.recv().await
            else {
                panic!("expected a program to start");
            };
            assert_eq!(dir.as_bytes(), b"/srv");
            assert_eq!(program, "rg");
            assert!(args.iter().any(|arg| arg == "--json"), "{args:?}");
            let json = r#"{"type":"begin","data":{"path":{"text":"./a"}}}"#;
            let child = tokio::process::Command::new("/bin/sh")
                .args(["-c", "printf '%s\n' \"$0\"", json])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|error| error.to_string());
            let _ = reply.send(child);
        });
        let query = Query {
            pattern: "x".to_owned(),
            globs: Vec::new(),
            regex: true,
            case: noc_pattern::Case::Smart,
            hidden: false,
            max_per_file: 10,
        };
        let (done, mut reports) = mpsc::unbounded_channel();
        let mut batches = Batches {
            generation: 1,
            found: Vec::new(),
            sent: None,
            done,
        };
        let searched = Searched {
            query: &query,
            max_results: 10,
        };
        let location = Location::Remote {
            host: "web".to_owned(),
            path: RemotePath::from("/srv"),
        };
        let ripgrep = Ripgrep::new(PathBuf::from("/nonexistent/rg"));
        let result = searched
            .search(&ripgrep, location, Some(handle), &mut batches)
            .await;
        host.await.unwrap();
        assert_eq!(result.unwrap(), FindSummary::default());
        batches.flush();
        assert_eq!(hits_of(&mut reports), [Hit::File(b"a".to_vec())]);
    }

    /// Looks for `query` in `location` with the fd of `fd`, through `host` if remote, taking
    /// `max_results`: what it found, and how it ended.
    async fn fd_in(
        (fd, location, host): (&Fd, Location, Option<HostHandle>),
        query: &fd::Query,
        max_results: usize,
    ) -> (Vec<Finding>, Result<FindSummary, String>) {
        let (done, mut reports) = mpsc::unbounded_channel();
        let mut batches = Batches {
            generation: 1,
            found: Vec::new(),
            sent: None,
            done,
        };
        let named = Named { query, max_results };
        let result = named.search(fd, location, host, &mut batches).await;
        batches.flush();
        let mut found = Vec::new();
        while let Ok(report) = reports.try_recv() {
            if let Done::Found { found: more, .. } = report {
                found.extend(more);
            }
        }
        (found, result)
    }

    fn names(pattern: &str) -> fd::Query {
        fd::Query {
            pattern: pattern.to_owned(),
            globs: Vec::new(),
            regex: false,
            case: noc_pattern::Case::Smart,
            hidden: false,
        }
    }

    fn entry(path: &str, dir: bool, matches: &[std::ops::Range<usize>]) -> Finding {
        Finding::Entry {
            path: path.as_bytes().to_vec(),
            dir,
            matches: matches.to_vec(),
        }
    }

    /// The paths of what fd found, in their order.
    fn sorted(mut found: Vec<Finding>) -> Vec<Finding> {
        found.sort_by_key(|finding| match finding {
            Finding::Entry { path, .. } => path.clone(),
            Finding::Hit(_) => Vec::new(),
        });
        found
    }

    #[tokio::test]
    #[allow(clippy::single_range_in_vec_init)]
    async fn fd_finds_names_keeps_what_the_globs_let_through_and_stops_at_the_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/find")).unwrap();
        std::fs::write(root.join("src/find.rs"), "").unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();
        std::fs::write(root.join("Find.md"), "").unwrap();
        let fd = Fd::new(PathBuf::from("fd"));
        let local = || Location::Local(root.to_path_buf());
        let (found, result) = fd_in((&fd, local(), None), &names("find"), 100).await;
        if result
            .as_ref()
            .is_err_and(|reason| reason.contains("not installed"))
        {
            return;
        }
        assert_eq!(result.unwrap(), FindSummary::default());
        assert_eq!(
            sorted(found),
            [
                entry("Find.md", false, &[0..4]),
                entry("src/find", true, &[4..8]),
                entry("src/find.rs", false, &[4..8]),
            ]
        );
        let files = fd::Query {
            globs: vec!["*.rs".to_owned()],
            ..names("")
        };
        let (found, _) = fd_in((&fd, local(), None), &files, 100).await;
        assert_eq!(
            sorted(found),
            [
                entry("src/find.rs", false, &[]),
                entry("src/main.rs", false, &[])
            ]
        );
        let (found, result) = fd_in((&fd, local(), None), &names(""), 2).await;
        assert_eq!(found.len(), 2);
        assert!(result.unwrap().limit);
        let bad = fd::Query {
            regex: true,
            ..names("(")
        };
        let (found, result) = fd_in((&fd, local(), None), &bad, 10).await;
        assert_eq!(found, []);
        let result = result.unwrap();
        assert_eq!(result.last_error.as_deref(), Some("error: unclosed group"));
        let missing = Fd::new(PathBuf::from("/nonexistent/fd"));
        let (_, result) = fd_in((&missing, local(), None), &names("x"), 10).await;
        assert!(result.unwrap_err().contains("fd is not installed"));
    }

    #[tokio::test]
    async fn on_a_host_without_fd_fdfind_runs_in_its_place() {
        let (handle, mut requests) = HostHandle::channel();
        let host = tokio::spawn(async move {
            let mut programs = Vec::new();
            while let Some(HostRequest::Spawn { program, reply, .. }) = requests.recv().await {
                programs.push(program);
                // The host has fdfind only: its shell says 127 for fd.
                let script = if program == "fd" {
                    "echo 'sh: fd: not found' >&2; exit 127"
                } else {
                    "printf 'src/\\0src/a.rs\\0'"
                };
                let child = tokio::process::Command::new("/bin/sh")
                    .args(["-c", script])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|error| error.to_string());
                let _ = reply.send(child);
            }
            programs
        });
        let location = Location::Remote {
            host: "web".to_owned(),
            path: RemotePath::from("/srv"),
        };
        let fd = Fd::new(PathBuf::from("/nonexistent/fd"));
        let (found, result) = fd_in((&fd, location, Some(handle)), &names(""), 10).await;
        assert_eq!(result.unwrap(), FindSummary::default());
        assert_eq!(
            found,
            [entry("src", true, &[]), entry("src/a.rs", false, &[])]
        );
        assert_eq!(host.await.unwrap(), ["fd", "fdfind"]);
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
