//! Background work for the app: listings, new directories, jobs, and one task per connected
//! host that owns its ssh session and SFTP channel.

use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::future::{BoxFuture, FutureExt as _};
use noc_config::{
    Config, ConfigError, HistoryEntry, HostConfig, Hosts, Recent, Run, Workspaces, save_host,
};
use noc_locale::Formats;
use noc_ops::{Algorithm, Conflict, CopyOptions, Outcome, Resolution, Step, Sum};
use noc_ssh::askpass::AskpassServer;
use noc_tools::opener::Opener;
use noc_tools::zoxide::Scored;
use noc_vfs::{LocalFs, Location, Metadata, RemotePath, Space, Vfs, VfsError, VfsPath as _};
use noc_viewer::{Part, Progress, Want, read_part};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use super::app::{Effect, FindRequest, HistoryChange, ReadRequest, RecentChange, WorkspaceChange};
use super::describe::{self, Reason};
use super::dialog::Ask;
use super::find::{FindQuery, FindSummary, Finding};
use super::panel::{ListError, ListRequest, Listed, Listing};
use super::places::{self, Places};
use super::progress::Counts;
use super::tabs::PanelId;
use crate::about::{self, Detected, Tool};
use crate::context::Context;
use crate::i18n::fl;

mod batching;
mod host;
mod jobs;
mod saving;
mod searching;

#[cfg(test)]
mod tests;

use batching::forward;
pub(crate) use host::{HostHandle, HostLink, HostRequest, Relink};
use host::{HostTask, Rejoin, prepare_ssh, resolve_address, share};
use jobs::{CopyJob, rename, run_checksum, run_delete, write_file};
use saving::{change_history, change_recent, change_workspaces, discard, save_configs};
use searching::{Batches, FIND_REPORTS, Named, Searched};

/// How long quitting waits for connections to shut down.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
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
        result: Result<Part, Reason>,
    },
    /// How far [`Effect::Read`] is with its part.
    ReadProgress { id: u64, progress: Progress },
    /// The directory of [`Effect::CreateDir`] was made, or why not.
    Created {
        panel: PanelId,
        location: Location,
        result: Result<(), Unmade>,
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
    /// The entry of [`Effect::Rename`] is renamed, or why not.
    Renamed {
        panel: PanelId,
        from: Location,
        to: Location,
        result: Result<(), Refused>,
    },
    /// The file of [`Effect::WriteFile`] is written, or why not.
    Written {
        location: Location,
        result: Result<(), Refused>,
    },
    /// The settings of [`Effect::SaveHost`] are saved, and these are all the host settings
    /// now; or why not.
    HostSaved {
        name: String,
        host: Option<HostConfig>,
        result: Result<Arc<Hosts>, Reason>,
    },
    /// The settings of [`Effect::SaveConfig`] are written, or why not.
    ConfigSaved {
        old: Box<Config>,
        new: Box<Config>,
        result: Result<(), Reason>,
    },
    /// The workspaces as `workspaces.toml` holds them after an [`Effect::Workspaces`], or why
    /// it could not be read, or changed if `changed`.
    Workspaces {
        change: WorkspaceChange,
        result: Result<Workspaces, Reason>,
    },
    /// The commands of `history.toml`, oldest first, after a change, or why it could not be
    /// read or written.
    History {
        change: HistoryChange,
        result: Result<Vec<HistoryEntry>, Reason>,
    },
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
    OpenFailed { path: PathBuf, reason: Reason },
}

/// A report from a job, with its paths as locations and its errors in words. Reports of how
/// far it is and questions come with `at`, when the job made them: the UI may take them
/// later, as a report that waits for its batch, or a question asked while the terminal was
/// handed over to another program.
#[derive(Debug)]
pub(crate) enum JobEvent {
    /// Counting what to do: `items` found so far, as the last of the reports it stands for
    /// said; `at` the first of them, made as the job started to count or went on counting.
    Scanning { at: Instant, items: u64 },
    /// How far the job is: what it had behind it in reports it made one after another, each
    /// with `at`, the last at `current`. A busy job sends a batch at most every `BATCH`.
    Progress {
        current: Location,
        reports: Vec<(Instant, Counts)>,
    },
    /// `step` failed at `path`, for `reason`; the job waits for `reply`. Retry would fail
    /// again unless `retry`, as when the target is the source itself.
    Failed {
        at: Instant,
        path: Location,
        step: Step,
        reason: Reason,
        retry: bool,
        reply: oneshot::Sender<Resolution>,
    },
    /// The name of a copy is taken; the job waits for the answer.
    Exists { at: Instant, taken: Box<TakenName> },
    /// The checksums of a checksum job that was not aborted, just before it is over.
    Sums(Vec<Sum<Location>>),
    /// The job is over; `complete` if it did all it was asked, without skipping or aborting.
    Finished { complete: bool },
}

/// A taken name that a copy job asks about: `source` was to go to `target`, where a file is,
/// both with their metadata, and `free` is a name free beside it; the answer goes to `reply`.
#[derive(Debug)]
pub(crate) struct TakenName {
    pub(crate) source: Location,
    pub(crate) target: Location,
    pub(crate) source_metadata: Metadata,
    pub(crate) target_metadata: Metadata,
    pub(crate) free: Vec<u8>,
    pub(crate) reply: oneshot::Sender<Conflict>,
}

/// Why a directory was not made.
#[derive(Debug)]
pub(crate) enum Unmade {
    /// Something has the name.
    Taken(Reason),
    Failed(Reason),
}

impl Unmade {
    fn of(error: &VfsError) -> Self {
        let reason = describe::vfs_reason(error);
        match error {
            VfsError::AlreadyExists(..) => Self::Taken(reason),
            _ => Self::Failed(reason),
        }
    }
}

/// Why a rename or a write of a file left everything as it was.
#[derive(Debug)]
pub(crate) enum Refused {
    /// A file has the name.
    Taken(Box<TakenFile>),
    /// Anything else.
    Failed(Reason),
}

/// A file that has the name a rename or a write was to give: what the entry renamed is, if
/// anything is, what the file there is, and `free`, a name free beside it.
#[derive(Debug)]
pub(crate) struct TakenFile {
    pub(crate) source: Option<Metadata>,
    pub(crate) target: Metadata,
    pub(crate) free: Vec<u8>,
}

impl JobEvent {
    /// Whether it tells only how far the job is, which changes nothing but how its window, its
    /// row, and the corner look.
    pub(crate) fn is_progress(&self) -> bool {
        matches!(self, Self::Scanning { .. } | Self::Progress { .. })
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
        hosts: (Option<HostLink>, Option<HostLink>),
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
            let (saving, config) = (name.clone(), host.clone());
            let saved = tokio::task::spawn_blocking(move || {
                save_host(&context.hosts_file, &saving, config.as_ref())?;
                context.reload_hosts()?;
                Ok::<_, ConfigError>(context.hosts())
            })
            .await;
            let result = match saved {
                Ok(result) => result.map_err(|error| describe::reason(&error)),
                Err(error) => Err(error.to_string().into()),
            };
            let _ = done.send(Done::HostSaved { name, host, result });
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
                        .map_err(|error| Unmade::of(&error));
                    let _ = done.send(Done::Created {
                        panel,
                        location,
                        result,
                    });
                });
            }
            (Location::Remote { .. }, None) => {
                let result = Err(Unmade::Failed(fl!("error-connection-closed").into()));
                let _ = self.done.send(Done::Created {
                    panel,
                    location,
                    result,
                });
            }
        }
    }

    /// Runs the delete job `id` on `targets`, all local or all on the host of `host`, in a
    /// task of its own.
    fn delete(
        &mut self,
        id: u64,
        targets: Vec<Location>,
        host: Option<HostLink>,
        cancel: CancellationToken,
    ) {
        let mut local = Vec::new();
        let mut remote = Vec::new();
        let mut on = String::new();
        for target in targets {
            match target {
                Location::Local(path) => local.push(path),
                Location::Remote { host, path } => {
                    on = host;
                    remote.push(path);
                }
            }
        }
        let done = self.done.clone();
        self.reap_jobs();
        self.jobs.spawn(async move {
            let finished = if let Some(link) = host {
                let location = move |path| Location::Remote {
                    host: on.clone(),
                    path,
                };
                run_delete(&Rejoin::new(link), remote, cancel, id, &done, location).await
            } else {
                run_delete(&LocalFs, local, cancel, id, &done, Location::Local).await
            };
            let _ = done.send(finished);
        });
    }

    /// Runs the checksum job `id` here.
    fn checksum(
        &mut self,
        id: u64,
        targets: Vec<(Vec<Location>, Option<HostLink>)>,
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

/// The end of the job `id`, with its outcome if it ran.
fn finished(id: u64, outcome: Option<Outcome>) -> Done {
    let complete = outcome.is_some_and(|outcome| !outcome.aborted && outcome.skipped == 0);
    Done::Job {
        id,
        event: JobEvent::Finished { complete },
    }
}

/// The ssh command that runs `command` with `shell` in `dir` on the host of `handle`, over its
/// connection, or why there is none.
pub(crate) async fn remote_command(
    handle: &HostHandle,
    dir: RemotePath,
    shell: Run,
    command: String,
) -> Result<tokio::process::Command, String> {
    let (reply, answer) = oneshot::channel();
    let request = HostRequest::Command {
        dir,
        shell,
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
) -> Result<Part, Reason> {
    let result = match (location, share(host).await) {
        (Location::Local(path), _) => read_part(&LocalFs, &path, want, report).await,
        (Location::Remote { path, .. }, Some(Some(fs))) => {
            read_part(&*fs, &path, want, report).await
        }
        _ => return Err(fl!("error-connection-closed").into()),
    };
    result.map_err(|error| describe::vfs_reason(&error))
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
