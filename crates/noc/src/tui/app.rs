//! State and drawing of the whole screen.

use std::collections::{HashMap, HashSet, VecDeque};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use jiff::tz::TimeZone;
use noc_config::{
    Config, HistoryEntry, HostConfig, Hosts, MenuBar, PanelSide, SftpHost, TabBar, UiConfig, Wheel,
    Workspace, Workspaces,
};
use noc_ops::{Algorithm, Conflict, CopyOptions, Decision, Sum};
use noc_tools::zoxide::Scored;
use noc_vfs::{FileKind, Location, Metadata, RemotePath};
use noc_viewer::{Command as ViewerCommand, Styles as ViewerStyles, Viewer};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::cd;
use super::cells::{self, Align};
use super::command::{self, CommandEvent, CommandLine, Place, Run};
use super::complete::{self, Candidate, Choices, ChoicesEvent, Kind, Offer, Outcome};
use super::configuration::{ConfigEvent, Configuration};
use super::decor::Decor;
use super::dialog::{Ask, Button, Dialog, DialogEvent, Reply};
use super::help::Help;
use super::history::{HistoryEvent, HistoryWindow};
use super::jobs::{JobsEvent, JobsList, Row};
use super::jump::{JumpEvent, JumpMenu};
use super::keymap::{Action, Context, Keymap, Resolved};
use super::menu::{LocationMenu, MenuEvent};
use super::mouse::{Pointer, Press};
use super::panel::{
    Destination, HostState, HostStatus, ListRequest, Listed, Panel, View, child, location_text,
};
use super::pattern::Pattern;
use super::progress::{Counts, JobButton, JobView};
use super::pulldown::{self, Command, PullDown, PullDownEvent, Status};
use super::sums::{Mark, SumRow, SumsButton, SumsEvent, SumsWindow, Verdict};
use super::tabs::{self, Bar, PanelId, Tab, Tabs};
use super::tasks::{HostHandle, JobEvent};
use super::theme::{ColorDepth, Theme};
use super::workspaces::{self, WorkspacesEvent, WorkspacesWindow};
use crate::i18n::fl;

/// One of the two panels, named by the side it starts on. Ctrl-U swaps where the panels are
/// drawn, not who they are, so replies to requests in flight still reach the panel that asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

impl Side {
    fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

impl Side {
    const BOTH: [Self; 2] = [Self::Left, Self::Right];
}

/// Work the app asks the event loop to do.
#[derive(Debug)]
pub(crate) enum Effect {
    /// List a location and pass the result to [`App::listed`]. Remote locations go to the task
    /// of their host.
    List {
        panel: PanelId,
        request: ListRequest,
        host: Option<HostHandle>,
    },
    /// List the virtual root for the location menu and pass the result to [`App::places`].
    ListPlaces { generation: u64 },
    /// Read the start of the file at `location` for the viewer `id`, through `host` if it is
    /// remote, and report to [`App::read`]; `cancel` stops it.
    Read {
        id: u64,
        location: Location,
        host: Option<HostHandle>,
        cancel: CancellationToken,
    },
    /// Remove a temporary file, if it is there.
    Discard(PathBuf),
    /// Make the directory at `location` and report to [`App::created`]. Remote ones go to the
    /// task of their host.
    CreateDir {
        panel: PanelId,
        location: Location,
        host: Option<HostHandle>,
    },
    /// Rename `from` to `to`, a new name in the same directory, over a file there if
    /// `replace`, through `host` if it is remote, and report to [`App::entry_renamed`].
    Rename {
        panel: PanelId,
        from: Location,
        to: Location,
        replace: bool,
        host: Option<HostHandle>,
    },
    /// Run the delete job `id` on `targets`, which are all local or all on the host of `host`,
    /// and report to [`App::job_event`]; `cancel` stops it.
    Delete {
        id: u64,
        targets: Vec<Location>,
        host: Option<HostHandle>,
        cancel: CancellationToken,
    },
    /// Run the copy job `id` of `sources`, all local or all on one host, to `target`, or the
    /// move job if the options remove sources, and report to [`App::job_event`]; `hosts` lead to the hosts of the sources and of the target,
    /// if they are remote, and `cancel` stops it.
    Copy {
        id: u64,
        sources: Vec<Location>,
        target: Location,
        hosts: (Option<HostHandle>, Option<HostHandle>),
        options: CopyOptions,
        cancel: CancellationToken,
    },
    /// Run the checksum job `id` on `targets`, groups of locations that are all local or all on
    /// the host of their handle, and report to [`App::job_event`]; `cancel` stops it.
    Checksum {
        id: u64,
        targets: Vec<(Vec<Location>, Option<HostHandle>)>,
        algorithm: Algorithm,
        cancel: CancellationToken,
    },
    /// Write `bytes` to a new file at `location`, or with `replace` over the one there, through
    /// `host` if it is remote, and report to [`App::written`].
    WriteFile {
        location: Location,
        bytes: Vec<u8>,
        replace: bool,
        host: Option<HostHandle>,
    },
    /// Connect to a host and report to [`App::connected`] and [`App::closed`]; `stop` ends the
    /// attempt or the connection.
    Connect {
        host: String,
        connection: u64,
        stop: CancellationToken,
    },
    /// Write the settings of the host `name` to `hosts.toml`, or remove them if `None`, and
    /// report to [`App::host_saved`].
    SaveHost {
        name: String,
        host: Option<HostConfig>,
    },
    /// Add the local directory to zoxide, after the ones before; failures only go to the log.
    ZoxideAdd(PathBuf),
    /// Ask zoxide for the directories that match `keywords`, without `exclude`, and pass them
    /// to [`App::jumps`].
    ZoxideQuery {
        generation: u64,
        keywords: Vec<String>,
        exclude: Option<PathBuf>,
    },
    /// List the names in `dir`, through `host` if it is remote, and, with `hosts`, the hosts of
    /// the ssh config, for completion, and pass them to [`App::names`]. Without `dir`, only the
    /// hosts.
    ListNames {
        generation: u64,
        dir: Option<Location>,
        host: Option<HostHandle>,
        hosts: bool,
    },
    /// Use `config` for new connections and listings, write to the config file the settings
    /// that differ between `old`, what the dialog showed, and `new`, both as the file writes
    /// them, and report to [`App::config_saved`].
    SaveConfig {
        old: Box<Config>,
        new: Box<Config>,
        config: Box<Config>,
    },
    /// Read or change `workspaces.toml`, after the changes before, and report to
    /// [`App::workspaces`].
    Workspaces(WorkspaceChange),
    /// Read or change `history.toml`, after the changes before, and report to
    /// [`App::history_changed`].
    History(HistoryChange),
}

/// What to do with `history.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryChange {
    /// Read it.
    Load,
    /// Add a command as the newest, keeping the newest `size`.
    Add { entry: HistoryEntry, size: usize },
    /// Remove `command` on `host`.
    Remove {
        host: Option<String>,
        command: String,
    },
}

/// What to do with `workspaces.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceChange {
    /// Read it.
    Load,
    /// Save a workspace, in place of the one with its name if there is one.
    Save(Workspace),
    Remove(String),
    /// Rename `from`, in its place, replacing another workspace named `to`.
    Rename {
        from: String,
        to: String,
    },
}

/// A host that is connected or on its way. `connection` tells attempts apart, so that reports
/// about an earlier one are ignored.
#[derive(Debug)]
enum Host {
    Connecting {
        connection: u64,
        stop: CancellationToken,
    },
    Connected {
        connection: u64,
        stop: CancellationToken,
        handle: HostHandle,
    },
}

impl Host {
    fn connection(&self) -> u64 {
        match self {
            Self::Connecting { connection, .. } | Self::Connected { connection, .. } => *connection,
        }
    }

    fn stop(&self) {
        match self {
            Self::Connecting { stop, .. } | Self::Connected { stop, .. } => stop.cancel(),
        }
    }
}

/// Width of the dialogs of `+` and `-`, as in mc.
const PATTERN_DIALOG_WIDTH: u16 = 50;
/// Width of the dialogs of F7 and F5.
const MKDIR_DIALOG_WIDTH: u16 = 60;
const COPY_DIALOG_WIDTH: u16 = 70;
const HOST_DIALOG_WIDTH: u16 = 70;
const CHECKSUM_DIALOG_WIDTH: u16 = 70;
const TAB_DIALOG_WIDTH: u16 = 70;
const WORKSPACE_DIALOG_WIDTH: u16 = 60;
/// The fields of the dialog of F4 on a host.
const HOST_LABEL: usize = 0;
const HOST_START_DIR: usize = 1;
const HOST_OTHER_DIR: usize = 2;

/// A dialog on screen or waiting for its turn, and what it is for.
#[derive(Debug)]
struct Open {
    dialog: Dialog,
    purpose: Purpose,
}

#[derive(Debug)]
enum Purpose {
    /// A prompt from ssh, or a notice, which has no `reply`.
    Ssh { id: u64, reply: Option<Reply> },
    /// `+` (`mark`) or `-` in `panel`.
    Pattern { panel: PanelId, mark: bool },
    /// F7 in `panel`.
    Mkdir { panel: PanelId },
    /// F8 in a panel on `dir`, for the entries `names`.
    Delete { dir: Location, names: Vec<Vec<u8>> },
    /// The new name of `from`, which `panel` renamed in its row, is `to`, a file's: Yes
    /// renames it, and the file goes.
    RenameOver {
        panel: PanelId,
        from: Location,
        to: Location,
    },
    /// F5 or F6 (by `kind`) in `panel`, which shows `dir`, for the entries `names`. The field
    /// opened with `offered`, the text for the other panel's location, if it shows one.
    Transfer {
        kind: JobKind,
        panel: PanelId,
        dir: Location,
        names: Vec<Vec<u8>>,
        offered: Option<(String, Location)>,
    },
    /// A failure in the job `job`, which waits for `reply`.
    Failure {
        job: u64,
        reply: oneshot::Sender<Decision>,
    },
    /// A taken name in the copy job `job`, which waits for `reply`.
    Conflict {
        job: u64,
        reply: oneshot::Sender<Conflict>,
    },
    /// F4 on the host `name`, whose settings were `old`. Use Current fills in `current`, the
    /// directory a panel shows on the host, if one does.
    EditHost {
        name: String,
        old: Option<HostConfig>,
        current: Option<String>,
    },
    /// The checksum dialog in the panel on `side`, which shows `dir`, for the entries
    /// `names`; with a check box to compare the one file with `other`, the file under the
    /// cursor of the other panel, and a field for the expected checksum if `expect`.
    Checksum {
        dir: Location,
        names: Vec<Vec<u8>>,
        other: Option<Location>,
        expect: bool,
    },
    /// Save the checksums of the window `window` in `dir`, under the name typed.
    SaveSums {
        window: u64,
        dir: Location,
        bytes: Vec<u8>,
    },
    /// `location` is taken: Yes writes the checksums of the window `window` over it.
    OverwriteSums {
        window: u64,
        location: Location,
        bytes: Vec<u8>,
    },
    /// The list of the tabs on `side`: OK shows the one chosen.
    TabList { side: Side },
    /// Quick cd in `panel`: OK opens the path typed there.
    QuickCd { panel: PanelId },
    /// A question about workspaces.
    Workspace(WorkspaceQuestion),
    /// F10 while jobs run: Yes quits and stops them.
    Quit,
    /// Something to read, such as an error.
    Info,
}

/// What a dialog about workspaces is for.
#[derive(Debug)]
enum WorkspaceQuestion {
    /// Alt-Shift-W, or Insert in the window: OK saves the tabs of both panels under the name
    /// typed.
    Save,
    /// The name typed for `workspace` is taken: Yes saves it in place of that one.
    Replace { workspace: Workspace },
    /// F6 in the window of the workspaces: OK renames `from` to the name typed.
    Rename { from: String },
    /// The new name of `from` is taken: Yes renames it, and the other one goes.
    RenameOver { from: String, to: String },
    /// F8 in the window of the workspaces: Yes deletes `name`.
    Delete { name: String },
}

/// Which hosts a path field takes before a `:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldHosts {
    /// Every host of the ssh config, which Quick cd connects to.
    All,
    /// The hosts that are connected, as the targets of F5 and F6.
    Connected,
    /// None: F7 makes local or remote directories by name only.
    None,
}

/// A Tab in the path field of the dialog in front: what it waits for, then the list it shows.
#[derive(Debug)]
struct Completing {
    generation: u64,
    /// The text before the cursor at the Tab; a reply for other text is dropped.
    before: String,
    split: complete::Split,
    offer: Offer,
    hosts: FieldHosts,
    /// The hosts that were connected at the Tab.
    connected: Vec<String>,
    choices: Option<Choices>,
}

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Delete,
    Copy,
    Move,
    Checksum,
}

/// What follows a job of F4 on a host.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Then {
    /// The copy for the editor is there: open it.
    Edit,
    /// The edited `copy` went back to `remote`: remove it.
    Discard { copy: PathBuf, remote: Location },
}

/// What a checksum job was asked for, and the checksums it has found.
#[derive(Debug)]
struct Hashing {
    algorithm: Algorithm,
    /// In lowercase hex.
    expected: Option<String>,
    /// Two files, one in each panel, to compare.
    compare: bool,
    /// Where the files are, for saving their checksums; compared files have none.
    dir: Option<Location>,
    /// `None` until the job reports them, which an aborted one never does.
    sums: Option<Vec<Sum<Location>>>,
}

/// The window of a finished checksum job, with what it shows.
#[derive(Debug)]
struct Results {
    /// The job's id.
    id: u64,
    window: SumsWindow,
    algorithm: Algorithm,
    dir: Option<Location>,
    /// The names, as files of checksums list them, and the checksums in hex; `None` for
    /// skipped files.
    lines: Vec<(Vec<u8>, Option<String>)>,
}

/// A file in the editor of F4.
#[derive(Debug)]
struct Editing {
    /// The local file the editor works on.
    file: PathBuf,
    /// Where it came from, if it is a copy of a remote file; it goes back there if it changes.
    remote: Option<Location>,
    /// Panels on this directory read it again afterwards.
    dir: Location,
}

/// A running job: in front, in its window over the panels, or in the background.
#[derive(Debug)]
struct Job {
    id: u64,
    kind: JobKind,
    background: bool,
    /// What starts the job when its turn comes, while it waits.
    queued: Option<Effect>,
    then: Option<Then>,
    hashing: Option<Hashing>,
    /// Directories it changes, which panels read again when it ends.
    changes: Vec<Location>,
    /// Hosts it works on; it ends with their connections.
    hosts: Vec<String>,
    cancel: CancellationToken,
    view: JobView,
}

impl Job {
    /// What follows the job, which then stays in front: a job of F4.
    fn then(mut self, then: Then) -> Self {
        self.then = Some(then);
        self.view.keep_in_front();
        self
    }

    fn new(id: u64, kind: JobKind, changes: Vec<Location>, cancel: CancellationToken) -> Self {
        let hosts = changes
            .iter()
            .filter_map(|location| match location {
                Location::Remote { host, .. } => Some(host.clone()),
                Location::Root | Location::Sftp | Location::Local(_) => None,
            })
            .collect();
        let view = match kind {
            JobKind::Delete => JobView::new(fl!("delete-title"), fl!("delete-deleting")),
            JobKind::Copy => JobView::new(fl!("copy-title"), fl!("copy-copying")),
            JobKind::Move => JobView::new(fl!("move-title"), fl!("move-moving")),
            JobKind::Checksum => JobView::new(fl!("checksum-title"), fl!("checksum-hashing")),
        };
        Self {
            id,
            kind,
            background: false,
            queued: None,
            then: None,
            hashing: None,
            changes,
            hosts,
            cancel,
            view,
        }
    }
}

/// The viewer on screen, the read it waits for, and what stops it loading.
#[derive(Debug)]
struct Viewing {
    id: u64,
    viewer: Viewer,
    location: Location,
    cancel: CancellationToken,
}

/// How F5 copies: what its dialog asked for last, which it starts with, and the setting.
#[derive(Debug, Clone, Copy)]
struct CopyChoices {
    preserve: bool,
    /// Through temporary names: `transfer.atomic_upload`.
    atomic: bool,
}

/// What `+` and `-` asked for last; their dialogs start with it.
#[derive(Debug, Clone)]
struct PatternOptions {
    pattern: String,
    /// Leaves directories alone.
    files_only: bool,
    case_sensitive: bool,
}

impl Default for PatternOptions {
    /// As in mc: case counts, and directories match too.
    fn default() -> Self {
        Self {
            pattern: "*".to_owned(),
            files_only: false,
            case_sensitive: true,
        }
    }
}

/// What the TUI shows and whether it keeps running.
#[derive(Debug)]
pub(crate) struct App {
    left: Tabs,
    right: Tabs,
    /// The number of the last tab opened.
    last_tab: u64,
    active: Side,
    /// The right panel is drawn on the left.
    swapped: bool,
    hosts: HashMap<String, Host>,
    connections: u64,
    /// The settings, with `~` expanded; `ui.show_hidden` follows Alt-.
    config: Config,
    decor: Decor,
    theme: Theme,
    /// What the terminal can show, for themes in RGB.
    color_depth: ColorDepth,
    /// Counts the frames of spinners.
    tick: u64,
    /// Hosts whose last attempt failed or whose connection was lost.
    failed: HashSet<String>,
    /// Addresses from `ssh -G` in this session.
    addresses: HashMap<String, String>,
    /// The settings from `hosts.toml`.
    host_settings: Arc<Hosts>,
    /// The last directory shown on each host in this session, for `remember_dir`.
    last_dirs: HashMap<String, RemotePath>,
    /// The first one is on screen and gets the keys; the others wait, so that a new prompt
    /// never takes the keys from a dialog in use.
    dialogs: VecDeque<Open>,
    pattern_options: PatternOptions,
    copy_choices: CopyChoices,
    /// The algorithm the checksum dialog chose last.
    algorithm: Algorithm,
    /// Windows of finished checksum jobs: the first one is on screen, over the panels and the
    /// jobs, under the dialogs.
    results: VecDeque<Results>,
    /// Text for the event loop to put on the clipboard.
    clipboard: Option<String>,
    /// Files of checksums being written: the window they came from, and what goes in them.
    saving: HashMap<Location, (u64, Vec<u8>)>,
    /// For the times in questions.
    tz: TimeZone,
    /// Running jobs: at most one in front, over the panels and the help and under the
    /// dialogs, and any number in the background.
    jobs: Vec<Job>,
    /// Instead of the panels.
    viewing: Option<Viewing>,
    editing: Option<Editing>,
    /// A file for the event loop to open in the editor.
    edit_now: Option<PathBuf>,
    /// The command line of `!` and `:`, above the F-key bar.
    command_line: Option<CommandLine>,
    /// A shell command for the event loop to run.
    run_now: Option<Run>,
    /// The commands of the command line, oldest first, as `history.toml` held them last.
    history: Vec<HistoryEntry>,
    /// The window of the command history, over the panels and under the dialogs.
    history_window: Option<HistoryWindow>,
    /// A file for the event loop to open in the editor with the command line's text, which
    /// comes back to the line.
    command_edit: Option<(PathBuf, String)>,
    /// The working directory last handed to the event loop.
    work_dir: Option<PathBuf>,
    /// Where copies of remote files for the editor go.
    runtime_dir: PathBuf,
    /// The id of the last job started.
    last_job: u64,
    /// How many jobs run at once: `transfer.parallel_jobs`.
    parallel_jobs: usize,
    /// The list of jobs, over the panels and the help.
    jobs_list: Option<JobsList>,
    /// Over the panels, under the dialogs.
    help: Option<Help>,
    /// The location menu of Alt-F1 or Alt-F2, over the panels and under the dialogs.
    menu: Option<LocationMenu>,
    /// The generation of the last listing for the menu.
    menu_listings: u64,
    /// Tab completion in the dialog in front.
    completion: Option<Completing>,
    /// The generation of the last listing for completion.
    completions: u64,
    /// The zoxide window of Alt-Z, over the panels and under the dialogs.
    jump: Option<JumpMenu>,
    /// The generation of the last query for the zoxide window.
    jump_queries: u64,
    /// The pull-down menu of F9, over the panels and under the windows and dialogs.
    pulldown: Option<PullDown>,
    /// Where the pull-down menu was when it closed, to open it there again.
    pulldown_place: Option<pulldown::Place>,
    /// The Configuration dialog, over the panels and the menus, under the other windows and
    /// the dialogs.
    configuration: Option<Configuration>,
    /// The saved workspaces, as `workspaces.toml` held them last.
    workspaces: Workspaces,
    /// The workspace restored or saved last, which the dialog of Alt-Shift-W offers.
    workspace: Option<String>,
    /// The window of the saved workspaces, over the panels and under the dialogs.
    workspaces_window: Option<WorkspacesWindow>,
    /// The home directory, the first row of the location menu.
    home: PathBuf,
    /// The title of the virtual root: the name of this machine.
    root_title: String,
    keymap: Keymap,
    /// Where the last render drew what the mouse can press.
    spots: Spots,
    /// What was in front after the last click, if that click left it there: a double click
    /// counts only then.
    clicked: Option<Front>,
    quit: bool,
    redraw: bool,
}

/// What the mouse can press, where the last render drew it.
#[derive(Debug, Default)]
struct Spots {
    /// The slots of the F-key bar, F1 first.
    fkeys: Vec<Rect>,
    /// The panel on each side with the line of its tabs, if it has one; none while the viewer
    /// shows.
    sides: Vec<(Side, Rect)>,
    /// The tabs shown, by side and index.
    tabs: Vec<(Side, usize, Rect)>,
    /// The titles of the menu bar that `ui.menu_bar` keeps.
    menu_bar: Vec<Rect>,
}

/// What is in front, and takes the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    /// The dialog in front, by its id.
    Dialog(u64),
    Menu,
    Jump,
    Workspaces,
    History,
    Results(usize),
    Job,
    JobsList,
    Help,
    Configuration,
    PullDown,
    Viewer,
    Panels,
}

impl App {
    /// Both panels on the local directory `start`, and the listings to request for them. The
    /// virtual root and the location menu open `home`, which `~` in dialogs stands for too.
    pub(crate) fn new(start: &Path, home: &Path, config: &Config) -> (Self, Vec<Effect>) {
        let (ui, transfer) = (&config.ui, &config.transfer);
        let show_hidden = ui.show_hidden;
        let panel = || {
            let start = Location::Local(start.to_path_buf());
            Panel::new(start, home.to_path_buf(), show_hidden)
        };
        let (left, left_request) = panel();
        let (right, right_request) = panel();
        let left_id = PanelId {
            side: Side::Left,
            tab: 1,
        };
        let right_id = PanelId {
            side: Side::Right,
            tab: 2,
        };
        let mut app = Self {
            left: Tabs::new(Tab::new(left_id, left)),
            right: Tabs::new(Tab::new(right_id, right)),
            last_tab: 2,
            active: Side::Left,
            swapped: false,
            hosts: HashMap::new(),
            connections: 0,
            config: config.clone(),
            decor: Decor::new(ui.icons),
            // `ui.theme` was checked when the config was loaded.
            theme: theme_of(ui, ColorDepth::TrueColor),
            color_depth: ColorDepth::TrueColor,
            tick: 0,
            failed: HashSet::new(),
            addresses: HashMap::new(),
            host_settings: Arc::default(),
            last_dirs: HashMap::new(),
            dialogs: VecDeque::new(),
            pattern_options: PatternOptions::default(),
            copy_choices: CopyChoices {
                preserve: true,
                atomic: transfer.atomic_upload,
            },
            algorithm: Algorithm::Sha256,
            results: VecDeque::new(),
            clipboard: None,
            saving: HashMap::new(),
            tz: TimeZone::UTC,
            jobs: Vec::new(),
            viewing: None,
            editing: None,
            edit_now: None,
            command_line: None,
            run_now: None,
            history: Vec::new(),
            history_window: None,
            command_edit: None,
            work_dir: None,
            runtime_dir: std::env::temp_dir(),
            last_job: 0,
            parallel_jobs: transfer.parallel_jobs.get(),
            jobs_list: None,
            help: None,
            menu: None,
            menu_listings: 0,
            completion: None,
            completions: 0,
            jump: None,
            jump_queries: 0,
            pulldown: None,
            pulldown_place: None,
            configuration: None,
            workspaces: Workspaces::default(),
            workspace: None,
            workspaces_window: None,
            home: home.to_path_buf(),
            root_title: fl!("root-title"),
            keymap: Keymap::mc(),
            spots: Spots::default(),
            clicked: None,
            quit: false,
            redraw: false,
        };
        let mut effects = app.route(left_id, left_request);
        effects.extend(app.route(right_id, right_request));
        (app, effects)
    }

    /// Whether the user asked to quit.
    pub(crate) fn quits(&self) -> bool {
        self.quit
    }

    /// Whether the whole screen must be drawn again; resets the request.
    pub(crate) fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// Text to put on the clipboard now; resets the request.
    pub(crate) fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    /// Puts copies of remote files for the editor in `dir`, which is private.
    pub(crate) fn set_runtime_dir(&mut self, dir: PathBuf) {
        self.runtime_dir = dir;
    }

    /// A file to open in the editor now, with the screen handed over; resets the request.
    pub(crate) fn take_edit(&mut self) -> Option<PathBuf> {
        self.edit_now.take()
    }

    /// A file to write the command line's text to and open in the editor now, with the screen
    /// handed over, and the text; resets the request.
    pub(crate) fn take_command_edit(&mut self) -> Option<(PathBuf, String)> {
        self.command_edit.take()
    }

    /// Takes what the editor left of the command, or why it could not run.
    pub(crate) fn command_edited(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => {
                if let Some(line) = &mut self.command_line {
                    line.set_text(&text);
                }
            }
            Err(reason) => self.show_error(&cells::sanitize(reason.as_bytes())),
        }
    }

    /// Takes pasted text: the command line gets all of it, line breaks included, and runs
    /// nothing; other places that take text get its characters but the line breaks; panels and
    /// menus whose keys are commands take none of it.
    pub(crate) fn paste(&mut self, text: &str) -> Vec<Effect> {
        match self.context() {
            Context::CommandLine => {
                if let Some(line) = &mut self.command_line {
                    line.paste(text);
                }
                Vec::new()
            }
            Context::Panel
            | Context::Root
            | Context::PullDown
            | Context::Dialog
            | Context::Viewer => Vec::new(),
            _ => text
                .chars()
                .filter(|c| !c.is_control())
                .flat_map(|c| self.handle(Resolved::Insert(c)))
                .collect(),
        }
    }

    /// A shell command to run now, with the screen handed over; resets the request.
    pub(crate) fn take_run(&mut self) -> Option<Run> {
        self.run_now.take()
    }

    /// The directory of the active panel if it is local and new since the last call: the
    /// working directory of the process follows it. A remote panel or the root leaves the
    /// working directory where it was.
    pub(crate) fn take_work_dir(&mut self) -> Option<PathBuf> {
        let Location::Local(dir) = self.panel(self.active).location() else {
            return None;
        };
        if self.work_dir.as_ref() == Some(dir) {
            return None;
        }
        self.work_dir = Some(dir.clone());
        self.work_dir.clone()
    }

    /// Uses `hosts`, the settings from `hosts.toml`.
    pub(crate) fn set_hosts(&mut self, hosts: Arc<Hosts>) {
        self.host_settings = hosts;
    }

    /// Titles the virtual root with `name`, the name of this machine.
    pub(crate) fn set_root_title(&mut self, name: String) {
        self.root_title = name;
    }

    /// Draws in the colors that the terminal can show, `depth`.
    pub(crate) fn set_color_depth(&mut self, depth: ColorDepth) {
        self.color_depth = depth;
        self.theme = theme_of(&self.config.ui, depth);
    }

    /// Shows times in `tz`.
    pub(crate) fn set_time_zone(&mut self, tz: TimeZone) {
        self.tz = tz;
    }

    /// What turns keys into actions.
    pub(crate) fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Whether the mouse is on: `ui.mouse`.
    pub(crate) fn mouse(&self) -> bool {
        self.config.ui.mouse
    }

    /// Takes a press of the mouse at a cell, as the last render drew the screen. What is in
    /// front takes it, as it takes keys.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Vec<Effect> {
        if !self.config.ui.mouse {
            return Vec::new();
        }
        let front = self.front();
        // A first click that closed a menu or a dialog, or opened one, leaves the second click
        // of a double click nothing to do.
        if pointer.press == Press::DoubleClick && self.clicked != Some(front) {
            return Vec::new();
        }
        let effects = self.route_pointer(pointer);
        self.clicked = (pointer.press == Press::Click && self.front() == front).then_some(front);
        effects
    }

    /// What is in front, in the order [`handle_over`](Self::handle_over) gives keys.
    fn front(&self) -> Front {
        if let Some(open) = self.dialogs.front() {
            Front::Dialog(open.dialog.id())
        } else if self.menu.is_some() {
            Front::Menu
        } else if self.jump.is_some() {
            Front::Jump
        } else if self.workspaces_window.is_some() {
            Front::Workspaces
        } else if self.history_window.is_some() {
            Front::History
        } else if !self.results.is_empty() {
            Front::Results(self.results.len())
        } else if self.in_front().is_some() {
            Front::Job
        } else if self.jobs_list.is_some() {
            Front::JobsList
        } else if self.help.is_some() {
            Front::Help
        } else if self.configuration.is_some() {
            Front::Configuration
        } else if self.pulldown.is_some() {
            Front::PullDown
        } else if self.viewing.is_some() {
            Front::Viewer
        } else {
            Front::Panels
        }
    }

    /// The F-key bar presses its keys; then what is in front takes the press: a dialog, the
    /// pull-down menu, the viewer, the menu bar that `ui.menu_bar` keeps, or the panels, where
    /// a click moves the cursor, a double click opens, a right click marks, and the wheel
    /// scrolls.
    fn route_pointer(&mut self, pointer: Pointer) -> Vec<Effect> {
        let Pointer { press, at } = pointer;
        if let Some(slot) = self.spots.fkeys.iter().position(|slot| slot.contains(at)) {
            // The second click of a double click would press the key again.
            let action = self.fkey_actions().into_iter().nth(slot).flatten();
            return match action {
                Some((action, _)) if press == Press::Click => self.handle(Resolved::Action(action)),
                _ => Vec::new(),
            };
        }
        match self.front() {
            Front::Dialog(_) => return self.pointer_dialog(pointer),
            Front::PullDown => return self.pointer_pulldown(pointer),
            Front::Viewer => {
                self.wheel_viewer(press);
                return Vec::new();
            }
            Front::Panels => {}
            _ => return self.pointer_window(pointer),
        }
        if self.panel(self.active).renaming() || self.command_line.is_some() {
            return Vec::new();
        }
        if let Some(index) = self
            .spots
            .menu_bar
            .iter()
            .position(|title| title.contains(at))
        {
            if press == Press::Click {
                self.open_pulldown();
                if let Some(mut pulldown) = self.pulldown.take() {
                    pulldown.open_menu(index, &|command| self.command_status(command));
                    self.pulldown = Some(pulldown);
                }
            }
            return Vec::new();
        }
        let Some(side) = self
            .spots
            .sides
            .iter()
            .find(|(_, area)| area.contains(at))
            .map(|(side, _)| *side)
        else {
            return Vec::new();
        };
        match press {
            Press::WheelUp | Press::WheelDown => {
                let step = self.config.ui.wheel;
                self.panel_mut(side).wheel(step, press == Press::WheelDown);
                Vec::new()
            }
            Press::Click | Press::DoubleClick | Press::RightClick => {
                self.click_panel(side, press, at)
            }
        }
    }

    /// The key that a step of the wheel stands for in windows and lists, and how many times:
    /// Up or Down by `ui.wheel` lines, or a page up or down.
    fn wheel_keys(&self, press: Press) -> Option<(Action, u8)> {
        let down = match press {
            Press::WheelUp => false,
            Press::WheelDown => true,
            Press::Click | Press::DoubleClick | Press::RightClick => return None,
        };
        Some(match (self.config.ui.wheel, down) {
            (Wheel::Lines(count), false) => (Action::Up, count),
            (Wheel::Lines(count), true) => (Action::Down, count),
            (Wheel::Page, false) => (Action::PageUp, 1),
            (Wheel::Page, true) => (Action::PageDown, 1),
        })
    }

    /// Gives the keys of a step of the wheel to what is in front.
    fn wheel(&mut self, (action, count): (Action, u8)) -> Vec<Effect> {
        let mut effects = Vec::new();
        for _ in 0..count {
            effects.extend(self.handle(Resolved::Action(action)));
        }
        effects
    }

    /// Gives a press of the mouse to the window or menu in front. The wheel moves its cursor
    /// as Up and Down do, or scrolls the help; but not in a job's window, where they move
    /// between the buttons. A click does what the window says, often by the key it stands
    /// for.
    fn pointer_window(&mut self, pointer: Pointer) -> Vec<Effect> {
        let front = self.front();
        if let Some(keys) = self.wheel_keys(pointer.press) {
            return if front == Front::Job {
                Vec::new()
            } else {
                self.wheel(keys)
            };
        }
        let action = match front {
            Front::Menu => self.menu.as_mut().and_then(|menu| menu.pointer(pointer)),
            Front::Jump => self.jump.as_mut().and_then(|jump| jump.pointer(pointer)),
            Front::Workspaces => self
                .workspaces_window
                .as_mut()
                .and_then(|window| window.pointer(pointer)),
            Front::History => self
                .history_window
                .as_mut()
                .and_then(|window| window.pointer(pointer)),
            Front::Results(_) => self
                .results
                .front_mut()
                .and_then(|results| results.window.pointer(pointer)),
            Front::Job => self
                .jobs
                .iter_mut()
                .find(|job| !job.background)
                .and_then(|job| job.view.pointer(pointer)),
            Front::JobsList => {
                let rows = self.job_rows();
                let list = self.jobs_list.as_mut();
                list.and_then(|list| list.pointer(pointer, &rows))
            }
            Front::Configuration => {
                let Some(configuration) = &mut self.configuration else {
                    return Vec::new();
                };
                let event = configuration.pointer(pointer);
                return self.configuration_event(event);
            }
            Front::Dialog(_) | Front::Help | Front::PullDown | Front::Viewer | Front::Panels => {
                None
            }
        };
        action.map_or_else(Vec::new, |action| self.handle(Resolved::Action(action)))
    }

    /// Scrolls the viewer by a step of the wheel, if `press` is one.
    fn wheel_viewer(&mut self, press: Press) {
        let Some(viewing) = &mut self.viewing else {
            return;
        };
        let (lines, page) = match press {
            Press::WheelUp => (ViewerCommand::Up, ViewerCommand::PageUp),
            Press::WheelDown => (ViewerCommand::Down, ViewerCommand::PageDown),
            Press::Click | Press::DoubleClick | Press::RightClick => return,
        };
        match self.config.ui.wheel {
            Wheel::Lines(count) => {
                for _ in 0..count {
                    viewing.viewer.handle(lines);
                }
            }
            Wheel::Page => viewing.viewer.handle(page),
        }
    }

    /// Gives a press of the mouse to the dialog in front, and does what it was for if it
    /// closes; or to the list of completions under its field, which goes at a press outside
    /// it.
    fn pointer_dialog(&mut self, pointer: Pointer) -> Vec<Effect> {
        let wheel = self.wheel_keys(pointer.press);
        if let Some(completing) = &mut self.completion
            && let Some(choices) = &mut completing.choices
            && choices.contains(pointer.at)
        {
            if let Some(keys) = wheel {
                return self.wheel(keys);
            }
            return match choices.pointer(pointer) {
                Some(action) => self.handle(Resolved::Action(action)),
                None => Vec::new(),
            };
        }
        self.completion = None;
        let Some(open) = self.dialogs.front_mut() else {
            return Vec::new();
        };
        let event = open.dialog.pointer(pointer);
        self.dialog_event(event)
    }

    /// Gives a press of the mouse to the pull-down menu; a command closes it, then runs.
    fn pointer_pulldown(&mut self, pointer: Pointer) -> Vec<Effect> {
        let Some(mut pulldown) = self.pulldown.take() else {
            return Vec::new();
        };
        let event = pulldown.pointer(pointer, &|command| self.command_status(command));
        self.pulldown_event(pulldown, event)
    }

    /// A click on the panel on `side`, at `at`: on a tab, it shows that tab; on a row, it puts
    /// the cursor there, and opens the row on a double click or marks it on a right click.
    /// Either way the side gets the keys, and quick search ends, as at a key of its own.
    fn click_panel(&mut self, side: Side, press: Press, at: Position) -> Vec<Effect> {
        self.panel_mut(self.active).end_search();
        self.active = side;
        let tab = self
            .spots
            .tabs
            .iter()
            .find(|(tab_side, _, area)| *tab_side == side && area.contains(at))
            .map(|(_, index, _)| *index);
        if let Some(index) = tab {
            if press == Press::Click && self.tabs_mut(side).select(index) {
                return self.revealed(side);
            }
            return Vec::new();
        }
        let panel = self.panel_mut(side);
        match press {
            Press::RightClick => panel.mark_at(at),
            Press::DoubleClick if panel.click(at) => {
                return self.handle(Resolved::Action(Action::Enter));
            }
            _ => {
                panel.click(at);
            }
        }
        Vec::new()
    }

    /// Where keys go now.
    pub(crate) fn context(&self) -> Context {
        if let Some(open) = self.dialogs.front() {
            let listing = self
                .completion
                .as_ref()
                .is_some_and(|completing| completing.choices.is_some());
            if listing && open.dialog.completes() {
                Context::Completion
            } else {
                open.dialog.context()
            }
        } else if self.menu.is_some() {
            Context::Menu
        } else if self.jump.is_some() {
            Context::Jump
        } else if self.workspaces_window.is_some() {
            Context::Workspaces
        } else if self.history_window.is_some() {
            Context::History
        } else if !self.results.is_empty()
            || self.in_front().is_some()
            || self.jobs_list.is_some()
            || self.help.is_some()
        {
            Context::Dialog
        } else if let Some(configuration) = &self.configuration {
            configuration.context()
        } else if self.pulldown.is_some() {
            Context::PullDown
        } else if self.viewing.is_some() {
            Context::Viewer
        } else if self.command_line.is_some() {
            Context::CommandLine
        } else if self.panel(self.active).renaming() {
            Context::Rename
        } else if self.panel(self.active).searching() {
            Context::QuickSearch
        } else {
            self.panel_context()
        }
    }

    /// Where keys go in the active panel, out of quick search: the commands of the pull-down
    /// menu show the keys of that context.
    fn panel_context(&self) -> Context {
        if self.panel(self.active).shows_root() {
            Context::Root
        } else {
            Context::Panel
        }
    }

    /// Whether the app does something for `action` now; the F-key bar shows only those.
    fn supports(&self, action: Action) -> bool {
        if self.context() == Context::Workspaces {
            // F6 and F8 rename and delete the workspace under the cursor.
            let chosen = self
                .workspaces_window
                .as_ref()
                .is_some_and(WorkspacesWindow::has_chosen);
            return action == Action::Cancel || chosen;
        }
        match action {
            Action::Help
            | Action::Quit
            | Action::Redraw
            | Action::Disconnect
            | Action::Cancel
            | Action::PullDown => true,
            Action::Mkdir
            | Action::Delete
            | Action::Copy
            | Action::Move
            | Action::Rename
            | Action::View
            | Action::Edit => !self.panel(self.active).shows_root(),
            Action::ToggleWrap => self.viewing.is_some(),
            Action::EditHost => self.panel(self.active).host_under_cursor().is_some(),
            _ => false,
        }
    }

    /// Gives a key to the job in front, if there is one: Abort stops it, Background sends it
    /// behind the panels.
    fn handle_job(&mut self, input: Resolved) -> bool {
        let Some(job) = self.jobs.iter_mut().find(|job| !job.background) else {
            return false;
        };
        match job.view.handle(input) {
            Some(JobButton::Abort) => {
                let id = job.id;
                self.abort_job(id);
            }
            Some(JobButton::Background) => job.background = true,
            None => {}
        }
        true
    }

    /// Gives a key to the list of jobs, if it is open: Show brings a job to the front, in place
    /// of the list.
    fn handle_jobs_list(&mut self, input: Resolved) -> bool {
        let rows = self.job_rows();
        let Some(list) = &mut self.jobs_list else {
            return false;
        };
        match list.handle(input, &rows) {
            JobsEvent::Pending => {}
            JobsEvent::Show(id) => {
                if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                    job.background = false;
                }
                self.jobs_list = None;
            }
            JobsEvent::Abort(id) => self.abort_job(id),
            JobsEvent::Closed => self.jobs_list = None,
        }
        true
    }

    /// Stops the job `id`; one that waits goes at once, as it has not started.
    fn abort_job(&mut self, id: u64) {
        let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return;
        };
        if job.queued.is_some() {
            self.end_job(id);
        } else {
            job.cancel.cancel();
            job.view.abort();
        }
    }

    /// The jobs, as the list shows them.
    fn job_rows(&self) -> Vec<Row> {
        let now = Instant::now();
        self.jobs
            .iter()
            .map(|job| {
                let summary = job.view.summary(now);
                Row {
                    id: job.id,
                    title: job.view.title().to_owned(),
                    state: summary.state,
                    left: summary.left,
                    current: summary.current,
                }
            })
            .collect()
    }

    /// The job `id` has the answer to its question and goes on, with its clock.
    fn answered(&mut self, id: u64) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
            job.view.answered(Instant::now());
        }
    }

    /// The job in front, in its window.
    fn in_front(&self) -> Option<&Job> {
        self.jobs.iter().find(|job| !job.background)
    }

    /// The help screen, for the keymap and the settings.
    fn help_screen(&self) -> Help {
        Help::new(&self.keymap, self.config.ui.fuzzy_search)
    }

    /// Quits, after asking if jobs would stop.
    fn ask_quit(&mut self) {
        if self.jobs.is_empty() {
            self.quit = true;
            return;
        }
        let message = fl!("quit-jobs", count = self.jobs.len());
        let buttons = vec![Button::Yes, Button::No];
        let dialog = Dialog::question(&fl!("quit-title"), &message, buttons, 1, false);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Quit,
        });
    }

    fn tabs(&self, side: Side) -> &Tabs {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn tabs_mut(&mut self, side: Side) -> &mut Tabs {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    /// The panel of the tab that shows on `side`.
    fn panel(&self, side: Side) -> &Panel {
        &self.tabs(side).active().panel
    }

    fn panel_mut(&mut self, side: Side) -> &mut Panel {
        &mut self.tabs_mut(side).active_mut().panel
    }

    /// The panel that shows on `side`.
    fn shown(&self, side: Side) -> PanelId {
        self.tabs(side).active().id
    }

    fn is_shown(&self, id: PanelId) -> bool {
        self.shown(id.side) == id
    }

    fn tab_mut(&mut self, id: PanelId) -> Option<&mut Tab> {
        self.tabs_mut(id.side).get_mut(id.tab)
    }

    /// The panel `id`, unless its tab was closed.
    fn panel_of(&self, id: PanelId) -> Option<&Panel> {
        self.tabs(id.side).get(id.tab).map(|tab| &tab.panel)
    }

    fn panel_of_mut(&mut self, id: PanelId) -> Option<&mut Panel> {
        self.tab_mut(id).map(|tab| &mut tab.panel)
    }

    /// Every panel, shown or not.
    fn panel_ids(&self) -> Vec<PanelId> {
        Side::BOTH
            .iter()
            .flat_map(|&side| self.tabs(side).iter().map(|tab| tab.id))
            .collect()
    }

    fn panels_mut(&mut self) -> impl Iterator<Item = &mut Panel> {
        self.left
            .iter_mut()
            .chain(self.right.iter_mut())
            .map(|tab| &mut tab.panel)
    }

    /// Does what a tab action does on `side`.
    fn tab_action(&mut self, side: Side, action: Action) -> Vec<Effect> {
        match action {
            Action::NewTab => return self.new_tab(side),
            Action::CloseTab => {
                if self.tabs_mut(side).close().is_some() {
                    return self.revealed(side);
                }
            }
            Action::NextTab | Action::PrevTab => {
                self.panel_mut(side).end_search();
                if self.tabs_mut(side).step(action == Action::NextTab) {
                    return self.revealed(side);
                }
            }
            Action::TabList => self.ask_tab(side),
            _ => {}
        }
        Vec::new()
    }

    /// Opens a tab on `side` after the one that shows, on the same location, and shows it.
    fn new_tab(&mut self, side: Side) -> Vec<Effect> {
        self.panel_mut(side).end_search();
        let (panel, request) = self.panel(side).duplicate();
        self.last_tab += 1;
        let id = PanelId {
            side,
            tab: self.last_tab,
        };
        self.tabs_mut(side).push(Tab::new(id, panel));
        request.map_or_else(Vec::new, |request| self.route(id, request))
    }

    /// The tab that shows on `side` now asks for its first listing if a workspace restored it
    /// hidden, or reads its location again if it changed while hidden.
    fn revealed(&mut self, side: Side) -> Vec<Effect> {
        let tab = self.tabs_mut(side).active_mut();
        let id = tab.id;
        if let Some(request) = tab.deferred.take()
            && tab
                .panel
                .pending_request()
                .is_some_and(|pending| pending.generation == request.generation)
        {
            tab.stale = false;
            return self.route(id, request);
        }
        if !std::mem::take(&mut tab.stale) {
            return Vec::new();
        }
        let here = tab.panel.here();
        let request = tab.panel.go(here);
        self.route(id, request)
    }

    /// Lists the tabs on `side`, where they are, to choose one.
    fn ask_tab(&mut self, side: Side) {
        let tabs = self.tabs(side);
        let choices = tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let title = tabs::title(tab.panel.location(), &self.root_title);
                format!("{} {title}", index + 1)
            })
            .collect();
        let buttons = vec![Button::Ok, Button::Cancel];
        let title = fl!("tabs-title");
        let dialog = Dialog::fields(&title, &[], &[], buttons, TAB_DIALOG_WIDTH)
            .with_choices(choices, tabs.index());
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::TabList { side },
        });
    }

    /// Gives a key to what is over the panels, front first, if anything is.
    fn handle_over(&mut self, input: Resolved) -> Option<Vec<Effect>> {
        if !self.dialogs.is_empty() {
            return Some(self.handle_dialog(input));
        }
        if self.menu.is_some() {
            return Some(self.handle_menu(input));
        }
        if self.jump.is_some() {
            return Some(self.handle_jump(input));
        }
        if self.workspaces_window.is_some() {
            return Some(self.handle_workspaces(input));
        }
        if self.history_window.is_some() {
            return Some(self.handle_history(input));
        }
        if self.handle_results(input) || self.handle_job(input) || self.handle_jobs_list(input) {
            return Some(Vec::new());
        }
        if let Some(help) = &mut self.help {
            if help.handle(input) {
                self.help = None;
            }
            return Some(Vec::new());
        }
        if self.configuration.is_some() {
            return Some(self.handle_configuration(input));
        }
        if self.pulldown.is_some() {
            return Some(self.handle_pulldown(input));
        }
        if self.viewing.is_some() {
            self.handle_viewer(input);
            return Some(Vec::new());
        }
        None
    }

    pub(crate) fn handle(&mut self, input: Resolved) -> Vec<Effect> {
        self.sync_connected();
        if let Some(effects) = self.handle_over(input) {
            return effects;
        }
        if self.command_line.is_some() {
            return self.handle_command_line(input);
        }
        if self.panel(self.active).renaming() {
            return self.handle_rename(input);
        }
        let Some(action) = self.search_key(input) else {
            return Vec::new();
        };
        match action {
            Action::QuickSearch => {
                let fuzzy_search = self.config.ui.fuzzy_search;
                self.panel_mut(self.active).search_next(fuzzy_search);
            }
            Action::Shell | Action::Command | Action::CommandHistory => {
                self.open_command_line(action);
            }
            Action::Quit => self.ask_quit(),
            Action::Jobs => self.jobs_list = Some(JobsList::default()),
            Action::Redraw => self.redraw = true,
            Action::Help => {
                self.help = Some(self.help_screen());
            }
            Action::SwitchPanel => self.active = self.active.other(),
            Action::NewTab
            | Action::CloseTab
            | Action::NextTab
            | Action::PrevTab
            | Action::TabList => return self.tab_action(self.active, action),
            // The active panel moves to the other side and stays active, as in mc.
            Action::SwapPanels => self.swapped = !self.swapped,
            Action::OtherPanelOpen => {
                if let Some(destination) = self.panel_mut(self.active).for_other_panel() {
                    return self.go(self.shown(self.active.other()), destination);
                }
            }
            Action::OtherPanelSync => {
                let here = self.panel(self.active).here();
                return self.go(self.shown(self.active.other()), here);
            }
            // As in mc, for both panels, and in every tab.
            Action::ToggleHidden => {
                self.config.ui.show_hidden = !self.config.ui.show_hidden;
                let show = self.config.ui.show_hidden;
                for panel in self.panels_mut() {
                    panel.set_show_hidden(show);
                }
            }
            Action::Mkdir => self.ask_mkdir(),
            Action::Checksum => self.ask_checksum(),
            Action::Delete => self.ask_delete(),
            Action::View => return self.view(),
            Action::Edit => return self.edit(),
            Action::Copy => self.ask_transfer(JobKind::Copy),
            Action::Move => self.ask_transfer(JobKind::Move),
            Action::Rename => self.start_rename(),
            Action::Select => self.ask_pattern(true),
            Action::Unselect => self.ask_pattern(false),
            Action::Cancel => self.cancel(self.active),
            Action::Disconnect => return self.disconnect_under_cursor(),
            Action::EditHost => self.ask_edit_host(),
            Action::LocationMenuLeft => return self.open_menu(Side::Left),
            Action::LocationMenuRight => return self.open_menu(Side::Right),
            Action::Jump => return self.open_jump(),
            Action::QuickCd => self.ask_cd(),
            Action::SaveWorkspace => self.ask_save_workspace(true),
            Action::Workspaces => self.open_workspaces(),
            Action::PullDown => self.open_pulldown(),
            _ => {
                let id = self.shown(self.active);
                if let Some(request) = self.panel_mut(id.side).handle(action) {
                    return self.open(id, request);
                }
            }
        }
        Vec::new()
    }

    /// Gives a key to quick search in the active panel, if it runs; the action that the panel
    /// does next, if any. Characters go nowhere else: typing in a panel does nothing.
    fn search_key(&mut self, input: Resolved) -> Option<Action> {
        let fuzzy_search = self.config.ui.fuzzy_search;
        let panel = self.panel_mut(self.active);
        match input {
            Resolved::Insert(c) => {
                if panel.searching() {
                    panel.search_type(c, fuzzy_search);
                }
                None
            }
            Resolved::Action(action) if panel.searching() => match action {
                Action::Backspace => {
                    panel.search_back();
                    None
                }
                Action::QuickSearch => {
                    panel.search_next(fuzzy_search);
                    None
                }
                Action::Cancel => {
                    panel.end_search();
                    None
                }
                // Any other key ends the search, then does what it does.
                _ => {
                    panel.end_search();
                    Some(action)
                }
            },
            Resolved::Action(action) => Some(action),
        }
    }

    /// Gives a key to the dialog in front, and does what it was for once it closes. In a path
    /// field, Tab completes, and the list of completions takes the keys while it shows.
    fn handle_dialog(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(completing) = &mut self.completion
            && let Some(choices) = &mut completing.choices
        {
            let event = match input {
                Resolved::Action(action) => choices.handle(action),
                Resolved::Insert(_) => ChoicesEvent::Passed,
            };
            match event {
                ChoicesEvent::Pending => return Vec::new(),
                ChoicesEvent::Closed => {
                    self.completion = None;
                    return Vec::new();
                }
                ChoicesEvent::Chosen(text) => {
                    let before = format!("{}{text}", completing.split.dir);
                    self.completion = None;
                    if let Some(open) = self.dialogs.front_mut() {
                        open.dialog.replace_before_cursor(&before);
                    }
                    return Vec::new();
                }
                ChoicesEvent::Passed => {}
            }
        }
        if input == Resolved::Action(Action::Complete) {
            return self.complete();
        }
        // Any other key makes a completion on its way too late.
        self.completion = None;
        let Some(open) = self.dialogs.front_mut() else {
            return Vec::new();
        };
        let event = open.dialog.handle(input);
        self.dialog_event(event)
    }

    /// Does what the dialog in front was for, if `event` closes it.
    fn dialog_event(&mut self, event: DialogEvent) -> Vec<Effect> {
        if event == DialogEvent::Pending || self.keeps_open(event) {
            return Vec::new();
        }
        match self.dialogs.pop_front() {
            Some(Open { dialog, purpose }) => self.dialog_closed(&dialog, purpose, event),
            None => Vec::new(),
        }
    }

    /// Opens the location menu for the panel on `side`, and lists what it offers.
    fn open_menu(&mut self, side: Side) -> Vec<Effect> {
        self.menu_listings += 1;
        let generation = self.menu_listings;
        let current = self.panel(side).location().clone();
        let menu = LocationMenu::new(side, current, self.home.clone(), generation)
            .fuzzy(self.config.ui.fuzzy_search);
        self.menu = Some(menu);
        vec![Effect::ListPlaces { generation }]
    }

    /// Gives a key to the location menu: what it opens goes to its panel, which becomes active.
    fn handle_menu(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(menu) = &mut self.menu else {
            return Vec::new();
        };
        match menu.handle(input) {
            MenuEvent::Pending => Vec::new(),
            MenuEvent::Closed => {
                self.menu = None;
                Vec::new()
            }
            MenuEvent::Open(location) => {
                let side = menu.side();
                self.menu = None;
                self.active = side;
                self.go(self.shown(side), Destination::to(location))
            }
            MenuEvent::Disconnect(host) => self.disconnect(&host),
            MenuEvent::Reload => {
                self.menu_listings += 1;
                let generation = self.menu_listings;
                menu.reload(generation);
                vec![Effect::ListPlaces { generation }]
            }
        }
    }

    /// Asks for a name, and saves the tabs of both panels under it; it offers the name of the
    /// workspace restored or saved last if `again`, else none, for a new one.
    fn ask_save_workspace(&mut self, again: bool) {
        let name = match &self.workspace {
            Some(name) if again => name.clone(),
            _ => String::new(),
        };
        let (title, prompt) = (fl!("workspaces-save-title"), fl!("workspaces-save-prompt"));
        let dialog = Dialog::form(&title, &prompt, &name, &[], WORKSPACE_DIALOG_WIDTH);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Workspace(WorkspaceQuestion::Save),
        });
    }

    /// Saves the tabs of both panels under `name`, the text of the dialog of Alt-Shift-W. A name
    /// that another workspace has asks first; that of the workspace restored or saved last
    /// does not, as saving it again is what the dialog offers.
    fn save_workspace_as(&mut self, name: &str) -> Vec<Effect> {
        let name = name.trim();
        if name.is_empty() {
            return Vec::new();
        }
        let workspace = self.workspace_named(name.to_owned());
        if self.workspaces.get(name).is_some() && self.workspace.as_deref() != Some(name) {
            let message = fl!(
                "workspaces-replace",
                name = cells::sanitize(name.as_bytes())
            );
            let buttons = vec![Button::Yes, Button::No];
            let title = fl!("workspaces-replace-title");
            let dialog = Dialog::question(&title, &message, buttons, 0, false);
            self.dialogs.push_back(Open {
                dialog,
                purpose: Purpose::Workspace(WorkspaceQuestion::Replace { workspace }),
            });
            return Vec::new();
        }
        self.save_workspace(workspace)
    }

    fn save_workspace(&mut self, workspace: Workspace) -> Vec<Effect> {
        self.workspace = Some(workspace.name.clone());
        if let Some(window) = &mut self.workspaces_window {
            window.focus(&workspace.name);
        }
        vec![Effect::Workspaces(WorkspaceChange::Save(workspace))]
    }

    /// The tabs of both panels, as a workspace named `name`.
    fn workspace_named(&self, name: String) -> Workspace {
        let saved = |side: Side| {
            let tabs = self.tabs(side);
            // One tab needs no mark of the one that shows.
            let several = tabs.len() > 1;
            tabs.iter()
                .enumerate()
                .map(|(index, tab)| {
                    let current = several && index == tabs.index();
                    workspaces::saved_tab(&tab.panel, current, &self.home)
                })
                .collect()
        };
        Workspace {
            name,
            active: match self.active {
                Side::Left => PanelSide::Left,
                Side::Right => PanelSide::Right,
            },
            left: saved(Side::Left),
            right: saved(Side::Right),
        }
    }

    /// Replaces the tabs of both panels with those of the workspace `name`. The tabs that show
    /// list their locations at once, the others once they show; the tabs that close take
    /// their marks with them.
    fn restore_workspace(&mut self, name: &str) -> Vec<Effect> {
        let Some(workspace) = self.workspaces.get(name).cloned() else {
            let name = cells::sanitize(name.as_bytes());
            self.show_error(&fl!("workspaces-gone", name = name));
            return Vec::new();
        };
        let show_hidden = self.config.ui.show_hidden;
        let mut effects = Vec::new();
        for (side, saved) in [
            (Side::Left, &workspace.left),
            (Side::Right, &workspace.right),
        ] {
            let current = workspaces::current_of(saved);
            let mut tabs = Vec::with_capacity(saved.len());
            let mut first = None;
            for (index, saved) in saved.iter().enumerate() {
                let (panel, request) = workspaces::restored_panel(saved, &self.home, show_hidden);
                self.last_tab += 1;
                let id = PanelId {
                    side,
                    tab: self.last_tab,
                };
                let mut tab = Tab::new(id, panel);
                if index == current {
                    first = Some((id, request));
                } else {
                    // Should the request be lost on the way, the tab reads its location then.
                    tab.stale = true;
                    tab.deferred = Some(request);
                }
                tabs.push(tab);
            }
            // `workspaces.toml` holds no side without tabs.
            if let Some(tabs) = Tabs::of(tabs, current) {
                *self.tabs_mut(side) = tabs;
            }
            if let Some((id, request)) = first {
                effects.extend(self.route(id, request));
            }
        }
        self.active = match workspace.active {
            PanelSide::Left => Side::Left,
            PanelSide::Right => Side::Right,
        };
        self.workspace = Some(workspace.name);
        self.sync_connected();
        effects
    }

    /// Opens the window of the saved workspaces.
    fn open_workspaces(&mut self) {
        let rows = workspaces::Row::all(&self.workspaces);
        let window = WorkspacesWindow::new(rows, self.config.ui.fuzzy_search);
        self.workspaces_window = Some(window);
    }

    /// Gives a key to the window of the saved workspaces: a restored one closes it; saving,
    /// renaming, and deleting ask in a dialog over it.
    fn handle_workspaces(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.workspaces_window else {
            return Vec::new();
        };
        match window.handle(input) {
            WorkspacesEvent::Pending => {}
            WorkspacesEvent::Closed => self.workspaces_window = None,
            WorkspacesEvent::Save => self.ask_save_workspace(false),
            WorkspacesEvent::Restore(name) => {
                self.workspaces_window = None;
                return self.restore_workspace(&name);
            }
            WorkspacesEvent::Rename(from) => {
                let shown = cells::sanitize(from.as_bytes());
                let title = fl!("workspaces-rename-title");
                let prompt = fl!("workspaces-rename-prompt", name = shown);
                let dialog = Dialog::form(&title, &prompt, &from, &[], WORKSPACE_DIALOG_WIDTH);
                self.dialogs.push_back(Open {
                    dialog,
                    purpose: Purpose::Workspace(WorkspaceQuestion::Rename { from }),
                });
            }
            WorkspacesEvent::Delete(name) => {
                let shown = cells::sanitize(name.as_bytes());
                let message = fl!("workspaces-delete", name = shown);
                let buttons = vec![Button::Yes, Button::No];
                let title = fl!("workspaces-delete-title");
                let dialog = Dialog::question(&title, &message, buttons, 0, true);
                self.dialogs.push_back(Open {
                    dialog,
                    purpose: Purpose::Workspace(WorkspaceQuestion::Delete { name }),
                });
            }
        }
        Vec::new()
    }

    /// Renames the workspace `from` to `to`, the text of the dialog of F6; a name that another
    /// workspace has asks first.
    fn rename_workspace(&mut self, from: String, to: &str) -> Vec<Effect> {
        let to = to.trim();
        if to.is_empty() || to == from {
            return Vec::new();
        }
        if self.workspaces.get(to).is_some() {
            let message = fl!(
                "workspaces-rename-replace",
                name = cells::sanitize(to.as_bytes())
            );
            let buttons = vec![Button::Yes, Button::No];
            let title = fl!("workspaces-rename-title");
            let dialog = Dialog::question(&title, &message, buttons, 1, false);
            let to = to.to_owned();
            self.dialogs.push_back(Open {
                dialog,
                purpose: Purpose::Workspace(WorkspaceQuestion::RenameOver { from, to }),
            });
            return Vec::new();
        }
        self.renamed(from, to.to_owned())
    }

    fn renamed(&mut self, from: String, to: String) -> Vec<Effect> {
        if self.workspace.as_ref() == Some(&from) {
            self.workspace = Some(to.clone());
        } else if self.workspace.as_ref() == Some(&to) {
            self.workspace = None;
        }
        vec![Effect::Workspaces(WorkspaceChange::Rename { from, to })]
    }

    fn delete_workspace(&mut self, name: String) -> Vec<Effect> {
        if self.workspace.as_ref() == Some(&name) {
            self.workspace = None;
        }
        vec![Effect::Workspaces(WorkspaceChange::Remove(name))]
    }

    /// Does what a dialog about workspaces was for, once `event` closed it with `text` in its
    /// field.
    fn workspace_dialog_closed(
        &mut self,
        text: &str,
        question: WorkspaceQuestion,
        event: DialogEvent,
    ) -> Vec<Effect> {
        let ok = event == DialogEvent::Pressed(Button::Ok);
        let yes = event == DialogEvent::Pressed(Button::Yes);
        match question {
            WorkspaceQuestion::Save if ok => self.save_workspace_as(text),
            WorkspaceQuestion::Replace { workspace } if yes => self.save_workspace(workspace),
            WorkspaceQuestion::Rename { from } if ok => self.rename_workspace(from, text),
            WorkspaceQuestion::RenameOver { from, to } if yes => self.renamed(from, to),
            WorkspaceQuestion::Delete { name } if yes => self.delete_workspace(name),
            _ => Vec::new(),
        }
    }

    /// Takes the end of an [`Effect::Workspaces`]: the workspaces as `workspaces.toml` holds
    /// them now, or why it could not be read, or changed if `changed`.
    pub(crate) fn workspaces_changed(&mut self, changed: bool, result: Result<Workspaces, String>) {
        match result {
            Ok(workspaces) => {
                self.workspaces = workspaces;
                if let Some(window) = &mut self.workspaces_window {
                    window.set_rows(workspaces::Row::all(&self.workspaces));
                }
            }
            Err(reason) => {
                let reason = cells::sanitize(reason.as_bytes());
                let message = if changed {
                    fl!("workspaces-save-error", reason = reason)
                } else {
                    fl!("workspaces-load-error", reason = reason)
                };
                self.show_error(&message);
            }
        }
    }

    /// What the path field of a dialog for `purpose` completes: in which panel, what it takes,
    /// and which hosts.
    fn completion_of(purpose: &Purpose) -> Option<(PanelId, Offer, FieldHosts)> {
        match purpose {
            Purpose::QuickCd { panel } => Some((*panel, Offer::Dirs, FieldHosts::All)),
            Purpose::Transfer { panel, .. } => Some((*panel, Offer::Any, FieldHosts::Connected)),
            Purpose::Mkdir { panel } => Some((*panel, Offer::Dirs, FieldHosts::None)),
            _ => None,
        }
    }

    /// Completes the path before the cursor in the field of the dialog in front: lists the
    /// directory it is in, read as that dialog reads its text, and the hosts it may name.
    fn complete(&mut self) -> Vec<Effect> {
        let Some(open) = self.dialogs.front_mut() else {
            return Vec::new();
        };
        if !open.dialog.completes() {
            return Vec::new();
        }
        let Some((panel, offer, hosts)) = Self::completion_of(&open.purpose) else {
            return Vec::new();
        };
        let before = open.dialog.around_cursor().0.to_owned();
        if before == "~" {
            open.dialog.replace_before_cursor("~/");
            return Vec::new();
        }
        let connected: Vec<String> = self
            .hosts
            .iter()
            .filter(|(_, state)| matches!(state, Host::Connected { .. }))
            .map(|(host, _)| host.clone())
            .collect();
        let is_host = |host: &str| match hosts {
            FieldHosts::All => true,
            FieldHosts::Connected => connected.iter().any(|known| known == host),
            FieldHosts::None => false,
        };
        let split = complete::split(&before, is_host);
        let Some(shown) = self.panel_of(panel) else {
            return Vec::new();
        };
        let dir = cd::directory(&split.dir, shown.location(), &self.home, is_host);
        // A host that is not connected has nothing to list.
        let host = self.handle_for(&dir).unwrap_or(None);
        let list_hosts = split.hosts && hosts == FieldHosts::All;
        self.completions += 1;
        let generation = self.completions;
        self.completion = Some(Completing {
            generation,
            before,
            split,
            offer,
            hosts,
            connected,
            choices: None,
        });
        vec![Effect::ListNames {
            generation,
            dir: Some(dir),
            host,
            hosts: list_hosts,
        }]
    }

    /// Takes the result of an [`Effect::ListNames`]: one match goes in the field, several go
    /// in as far as they agree, or show in a list when they agree no further.
    pub(crate) fn names(
        &mut self,
        generation: u64,
        entries: Result<Vec<(Vec<u8>, bool)>, String>,
        hosts: &[String],
    ) {
        let Some(completing) = &mut self.completion else {
            return;
        };
        if completing.generation != generation || completing.choices.is_some() {
            return;
        }
        let Some(open) = self
            .dialogs
            .front_mut()
            .filter(|open| open.dialog.around_cursor().0 == completing.before)
        else {
            self.completion = None;
            return;
        };
        let entries = entries.unwrap_or_else(|reason| {
            tracing::debug!(%reason, "cannot list names to complete");
            Vec::new()
        });
        let mut candidates = complete::from_names(entries, completing.offer);
        if completing.split.hosts {
            let aliases = match completing.hosts {
                FieldHosts::All => hosts,
                FieldHosts::Connected => &completing.connected[..],
                FieldHosts::None => &[],
            };
            candidates.extend(aliases.iter().map(|alias| Candidate {
                name: alias.clone(),
                kind: Kind::Host,
            }));
        }
        match complete::complete(&completing.split.prefix, candidates) {
            Outcome::Nothing => self.completion = None,
            Outcome::Insert(text) => {
                let before = format!("{}{text}", completing.split.dir);
                self.completion = None;
                open.dialog.replace_before_cursor(&before);
            }
            Outcome::List(items) => completing.choices = Some(Choices::new(items)),
        }
    }

    /// Opens the zoxide window for the active panel, and asks zoxide for its best directories;
    /// with `ui.fuzzy_search`, for all of them, which the window filters itself.
    fn open_jump(&mut self) -> Vec<Effect> {
        let exclude = match self.panel(self.active).location() {
            Location::Local(path) => Some(path.clone()),
            Location::Root | Location::Sftp | Location::Remote { .. } => None,
        };
        let jump = JumpMenu::new(self.home.clone(), exclude, 0).fuzzy(self.config.ui.fuzzy_search);
        self.jump = Some(jump);
        self.query_jump()
    }

    /// Asks zoxide for the directories that match the keywords of the zoxide window.
    fn query_jump(&mut self) -> Vec<Effect> {
        let Some(jump) = &mut self.jump else {
            return Vec::new();
        };
        self.jump_queries += 1;
        let generation = self.jump_queries;
        jump.wait(generation);
        vec![Effect::ZoxideQuery {
            generation,
            keywords: jump.keywords(),
            exclude: jump.exclude().map(Path::to_path_buf),
        }]
    }

    /// Gives a key to the zoxide window: the directory it opens goes to the active panel, and
    /// counts in zoxide, as `z` counts it.
    fn handle_jump(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(jump) = &mut self.jump else {
            return Vec::new();
        };
        match jump.handle(input) {
            JumpEvent::Pending => Vec::new(),
            JumpEvent::Closed => {
                self.jump = None;
                Vec::new()
            }
            JumpEvent::Query => self.query_jump(),
            JumpEvent::Open(path) => {
                self.jump = None;
                let id = self.shown(self.active);
                let record = self.config.zoxide.record;
                if let Some(tab) = self.tab_mut(id) {
                    tab.arriving = record.then(|| path.clone());
                }
                let mut effects = self.go(id, Destination::to(Location::Local(path.clone())));
                if record {
                    effects.push(Effect::ZoxideAdd(path));
                }
                effects
            }
        }
    }

    /// Takes the result of an [`Effect::ZoxideQuery`].
    pub(crate) fn jumps(&mut self, generation: u64, result: Result<Vec<Scored>, String>) {
        if let Some(jump) = &mut self.jump {
            jump.found(generation, result);
        }
    }

    /// The user did something in `dir`, such as copy from it: a local directory that a panel
    /// shows goes to zoxide, once a visit (`zoxide.record`). Directories that no panel shows,
    /// such as a target typed in a dialog, and passing through, do not count.
    fn note(&mut self, dir: &Location) -> Vec<Effect> {
        let Location::Local(path) = dir else {
            return Vec::new();
        };
        if !self.config.zoxide.record {
            return Vec::new();
        }
        let mut counted = false;
        for side in Side::BOTH {
            let tab = self.tabs_mut(side).active_mut();
            if tab.panel.location() == dir && !tab.noted {
                tab.noted = true;
                counted = true;
            }
        }
        if !counted {
            return Vec::new();
        }
        vec![Effect::ZoxideAdd(path.clone())]
    }

    /// Opens the pull-down menu where it was when it closed; the first time, on the bar at the
    /// menu of the active panel.
    fn open_pulldown(&mut self) {
        let pulldown = PullDown::new(
            self.active,
            self.swapped,
            self.pulldown_place.as_ref(),
            &workspaces::names(&self.workspaces),
            &|command| self.command_status(command),
        );
        self.pulldown = Some(pulldown);
    }

    /// Gives a key to the pull-down menu; a command closes it, then runs.
    fn handle_pulldown(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(mut pulldown) = self.pulldown.take() else {
            return Vec::new();
        };
        let event = pulldown.handle(input, &|command| self.command_status(command));
        self.pulldown_event(pulldown, event)
    }

    /// Keeps the pull-down menu open after `event`, or closes it and runs its command.
    fn pulldown_event(&mut self, pulldown: PullDown, event: PullDownEvent) -> Vec<Effect> {
        match event {
            PullDownEvent::Pending => {
                self.pulldown = Some(pulldown);
                Vec::new()
            }
            PullDownEvent::Closed => {
                self.pulldown_place = Some(pulldown.place().clone());
                Vec::new()
            }
            PullDownEvent::Run(command) => {
                self.pulldown_place = Some(pulldown.place().clone());
                self.run(command)
            }
        }
    }

    /// Runs a command of the pull-down menu.
    fn run(&mut self, command: Command) -> Vec<Effect> {
        match command {
            Command::Do(action) => self.handle(Resolved::Action(action)),
            Command::On(
                side,
                action @ (Action::NewTab
                | Action::CloseTab
                | Action::NextTab
                | Action::PrevTab
                | Action::TabList),
            ) => self.tab_action(side, action),
            Command::On(side, action) => match self.panel_mut(side).handle(action) {
                Some(request) => self.open(self.shown(side), request),
                None => Vec::new(),
            },
            Command::Location(side) => self.open_menu(side),
            Command::DisconnectPanel(side) => match self.host_of(side) {
                Some(host) => self.disconnect(&host),
                None => Vec::new(),
            },
            Command::Configuration => {
                let icons = self.decor.icons();
                let configuration =
                    Configuration::new(&self.config, &self.home, Theme::NAMES, icons);
                self.configuration = Some(configuration);
                Vec::new()
            }
            Command::Workspace(index) => match self.workspaces.workspaces.get(index) {
                Some(workspace) => {
                    let name = workspace.name.clone();
                    self.restore_workspace(&name)
                }
                None => Vec::new(),
            },
        }
    }

    /// Gives a key to the Configuration dialog, which applies each change as it is made.
    fn handle_configuration(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(configuration) = &mut self.configuration else {
            return Vec::new();
        };
        let event = configuration.handle(input);
        self.configuration_event(event)
    }

    /// Closes the Configuration dialog if `event` closes it, and applies the change it made.
    fn configuration_event(&mut self, event: ConfigEvent) -> Vec<Effect> {
        if event.closed {
            self.configuration = None;
        }
        match event.change {
            Some((old, new)) => self.apply_config(old, new),
            None => Vec::new(),
        }
    }

    /// Uses the settings `new`, as `config.toml` writes them, from now on, but the language,
    /// which the next start reads; and writes to the config file those that differ from `old`.
    fn apply_config(&mut self, old: Config, new: Config) -> Vec<Effect> {
        if old == new {
            return Vec::new();
        }
        let mut config = new.clone();
        config.expand_tilde(&self.home);
        let ui = &config.ui;
        // The dialog offers only the built-in themes and those `ui.theme` names.
        self.theme = theme_of(ui, self.color_depth);
        self.decor = Decor::new(ui.icons);
        if ui.show_hidden != self.config.ui.show_hidden {
            let show = ui.show_hidden;
            for panel in self.panels_mut() {
                panel.set_show_hidden(show);
            }
        }
        self.copy_choices.atomic = config.transfer.atomic_upload;
        self.parallel_jobs = config.transfer.parallel_jobs.get();
        // What the root and the list of hosts show may change with the ssh settings and the
        // hidden hosts and volumes.
        let lists = config.ssh != self.config.ssh
            || config.discovery != self.config.discovery
            || config.volumes != self.config.volumes;
        self.config = config.clone();
        let mut effects = vec![Effect::SaveConfig {
            old: Box::new(old),
            new: Box::new(new),
            config: Box::new(config),
        }];
        effects.extend(self.start_queued());
        if lists {
            effects.extend(self.reload(&Location::Root));
            effects.extend(self.reload(&Location::Sftp));
        }
        effects
    }

    /// Takes the result of an [`Effect::SaveConfig`].
    pub(crate) fn config_saved(&mut self, result: Result<(), String>) {
        if let Err(reason) = result {
            let reason = cells::sanitize(reason.as_bytes());
            self.show_error(&fl!("config-save-error", reason = reason));
        }
    }

    /// The host that the panel on `side` shows, if it is connected or connecting.
    fn host_of(&self, side: Side) -> Option<String> {
        match self.panel(side).location() {
            Location::Remote { host, .. } if self.hosts.contains_key(host) => Some(host.clone()),
            _ => None,
        }
    }

    /// Whether a command of the pull-down menu runs now, its mark, and its key.
    fn command_status(&self, command: Command) -> Status {
        let context = self.panel_context();
        let active = self.panel(self.active);
        let files = !active.shows_root();
        match command {
            Command::Do(action) => Status {
                enabled: match action {
                    Action::Select | Action::Unselect | Action::InvertMarks => files,
                    Action::Checksum => !active.chosen().is_empty(),
                    Action::Disconnect => active
                        .host_under_cursor()
                        .is_some_and(|host| self.hosts.contains_key(host)),
                    Action::QuickSearch
                    | Action::Jump
                    | Action::QuickCd
                    | Action::SwapPanels
                    | Action::OtherPanelOpen
                    | Action::OtherPanelSync
                    | Action::Jobs
                    | Action::SaveWorkspace
                    | Action::Workspaces
                    | Action::ToggleHidden => true,
                    _ => self.supports(action),
                },
                checked: action == Action::ToggleHidden && self.config.ui.show_hidden,
                key: self.keymap.key(context, action),
            },
            Command::On(side, action) => Status {
                enabled: match action {
                    Action::CloseTab | Action::NextTab | Action::PrevTab => {
                        self.tabs(side).len() > 1
                    }
                    _ => true,
                },
                checked: self.panel(side).sort_action() == action,
                // Keys act on the active panel only.
                key: (side == self.active)
                    .then(|| self.keymap.key(context, action))
                    .flatten(),
            },
            Command::Location(side) => Status {
                enabled: true,
                checked: false,
                key: self.keymap.key(
                    context,
                    match side {
                        Side::Left => Action::LocationMenuLeft,
                        Side::Right => Action::LocationMenuRight,
                    },
                ),
            },
            Command::DisconnectPanel(side) => Status {
                enabled: self.host_of(side).is_some(),
                ..Status::default()
            },
            Command::Configuration => Status {
                enabled: true,
                ..Status::default()
            },
            Command::Workspace(index) => Status {
                enabled: true,
                checked: self
                    .workspaces
                    .workspaces
                    .get(index)
                    .is_some_and(|workspace| self.workspace.as_ref() == Some(&workspace.name)),
                key: None,
            },
        }
    }

    /// Takes the result of an [`Effect::ListPlaces`].
    pub(crate) fn places(&mut self, generation: u64, result: Result<Listed, String>) {
        if let Some(menu) = &mut self.menu {
            menu.listed(generation, result);
        }
    }

    /// Hosts that are connected or connecting, which the virtual root shows again.
    fn sync_connected(&mut self) {
        let connected: HashSet<String> = self.hosts.keys().cloned().collect();
        for panel in self.panels_mut() {
            panel.set_connected(connected.clone());
        }
    }

    /// Does what a dialog was for, once `event` closed it.
    fn dialog_closed(
        &mut self,
        dialog: &Dialog,
        purpose: Purpose,
        event: DialogEvent,
    ) -> Vec<Effect> {
        let ok = event == DialogEvent::Pressed(Button::Ok);
        match purpose {
            Purpose::Ssh { reply, .. } => {
                if let Some(reply) = reply {
                    reply.send(dialog.answer(event));
                }
            }
            Purpose::Pattern { panel, mark } if ok => self.mark_matching(panel, mark, dialog),
            Purpose::Mkdir { panel } if ok && !dialog.text().is_empty() => {
                let resolved = self.panel_of(panel).and_then(|p| p.resolve(dialog.text()));
                if let Some(location) = resolved {
                    return self.create_dir(panel, location);
                }
            }
            Purpose::Delete { dir, names } if event == DialogEvent::Pressed(Button::Yes) => {
                return self.start_delete(dir, &names);
            }
            Purpose::Transfer {
                kind,
                panel,
                dir,
                names,
                offered,
            } if ok => {
                if kind == JobKind::Copy {
                    self.copy_choices.preserve = dialog.checked(0);
                }
                let target = match offered {
                    Some((text, location)) if text == dialog.text() => Some(location),
                    _ => self.resolve_target(panel, dialog.text()),
                };
                if let Some(target) = target {
                    return self.start_transfer(kind, dir, &names, target);
                }
            }
            Purpose::Failure { job, reply } => {
                self.answered(job);
                let _ = reply.send(decision_of(event));
            }
            Purpose::Conflict { job, reply } => {
                self.answered(job);
                let _ = reply.send(conflict_of(event));
            }
            Purpose::EditHost { name, old, .. } if ok => {
                return save_host(name, old.as_ref(), dialog);
            }
            Purpose::Checksum {
                dir,
                names,
                other,
                expect,
            } if ok => return self.start_checksum(dialog, dir, &names, other, expect),
            Purpose::SaveSums { window, dir, bytes } if ok && !dialog.text().is_empty() => {
                if let Some(location) = child(&dir, dialog.text().as_bytes()) {
                    return self.write_sums(window, location, bytes, false);
                }
            }
            Purpose::OverwriteSums {
                window,
                location,
                bytes,
            } if event == DialogEvent::Pressed(Button::Yes) => {
                return self.write_sums(window, location, bytes, true);
            }
            Purpose::RenameOver { panel, from, to }
                if event == DialogEvent::Pressed(Button::Yes) =>
            {
                return self.rename_entry(panel, from, to, true);
            }
            Purpose::TabList { side } if ok => {
                self.panel_mut(side).end_search();
                if self.tabs_mut(side).select(dialog.chosen()) {
                    return self.revealed(side);
                }
            }
            Purpose::QuickCd { panel } if ok => return self.cd(panel, dialog.text()),
            Purpose::Workspace(question) => {
                return self.workspace_dialog_closed(dialog.text(), question, event);
            }
            Purpose::Quit => self.quit = event == DialogEvent::Pressed(Button::Yes),
            Purpose::EditHost { .. }
            | Purpose::TabList { .. }
            | Purpose::QuickCd { .. }
            | Purpose::Checksum { .. }
            | Purpose::SaveSums { .. }
            | Purpose::OverwriteSums { .. }
            | Purpose::RenameOver { .. }
            | Purpose::Pattern { .. }
            | Purpose::Mkdir { .. }
            | Purpose::Delete { .. }
            | Purpose::Transfer { .. }
            | Purpose::Info => {}
        }
        Vec::new()
    }

    /// Views the file under the cursor of the active panel, as F3 does in mc; a directory, or
    /// a row of the virtual root, opens as with Enter.
    fn view(&mut self) -> Vec<Effect> {
        let side = self.active;
        let panel = self.panel(side);
        let file = panel
            .entry_under_cursor()
            .filter(|entry| !entry.is_dir_like())
            .and_then(|entry| child(panel.location(), &entry.name));
        let Some(location) = file else {
            if let Some(request) = self.panel_mut(side).handle(Action::Enter) {
                return self.open(self.shown(side), request);
            }
            return Vec::new();
        };
        let host = match &location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Some(handle.clone()),
                _ => None,
            },
            Location::Root | Location::Sftp | Location::Local(_) => None,
        };
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        self.viewing = Some(Viewing {
            id,
            viewer: Viewer::new(location_text(&location)),
            location: location.clone(),
            cancel: cancel.clone(),
        });
        let dir = self.panel(side).location().clone();
        let mut effects = vec![Effect::Read {
            id,
            location,
            host,
            cancel,
        }];
        effects.extend(self.note(&dir));
        effects
    }

    /// Edits the file under the cursor of the active panel with F4: a local one where it is, a
    /// remote one as a local copy, which goes back if it changes.
    fn edit(&mut self) -> Vec<Effect> {
        let panel = self.panel(self.active);
        let dir = panel.location().clone();
        let Some((name, location)) = panel
            .entry_under_cursor()
            .filter(|entry| !entry.is_dir_like())
            .and_then(|entry| Some((entry.name.clone(), child(&dir, &entry.name)?)))
        else {
            return Vec::new();
        };
        let Location::Remote { host, .. } = &location else {
            if let Location::Local(path) = location {
                let noted = self.note(&dir);
                self.editing = Some(Editing {
                    file: path.clone(),
                    remote: None,
                    dir,
                });
                self.edit_now = Some(path);
                return noted;
            }
            return Vec::new();
        };
        let Some(Host::Connected { handle, .. }) = self.hosts.get(host) else {
            let reason = fl!("error-connection-closed");
            self.show_error(&fl!(
                "edit-error",
                path = location_text(&location),
                reason = reason
            ));
            return Vec::new();
        };
        let (handle, on) = (handle.clone(), host.clone());
        self.last_job += 1;
        let id = self.last_job;
        // Its own name last, so that the editor knows what kind of file it is.
        let mut temporary = format!("edit-{}-{id}-", std::process::id()).into_bytes();
        temporary.extend_from_slice(&name);
        let file = self
            .runtime_dir
            .join(std::ffi::OsStr::from_bytes(&temporary));
        let cancel = CancellationToken::new();
        let mut job = Job::new(id, JobKind::Copy, Vec::new(), cancel.clone()).then(Then::Edit);
        job.hosts.push(on);
        self.jobs.push(job);
        self.editing = Some(Editing {
            file: file.clone(),
            remote: Some(location.clone()),
            dir,
        });
        vec![Effect::Copy {
            id,
            sources: vec![location],
            target: Location::Local(file),
            hosts: (Some(handle), None),
            options: CopyOptions {
                preserve: true,
                atomic: false,
                remove_sources: false,
                overwrite: true,
            },
            cancel,
        }]
    }

    /// Takes the end of the editor: whether the file changed, or why it could not run. A
    /// changed copy of a remote file goes back.
    pub(crate) fn edited(&mut self, result: Result<bool, String>) -> Vec<Effect> {
        let Some(editing) = self.editing.take() else {
            return Vec::new();
        };
        let path = editing.remote.as_ref().map_or_else(
            || location_text(&Location::Local(editing.file.clone())),
            location_text,
        );
        let changed = match result {
            Ok(changed) => changed,
            Err(reason) => {
                let reason = cells::sanitize(reason.as_bytes());
                self.show_error(&fl!("edit-error", path = path.clone(), reason = reason));
                false
            }
        };
        let Some(remote) = editing.remote else {
            return self.reload(&editing.dir);
        };
        if !changed {
            return vec![Effect::Discard(editing.file)];
        }
        let handle = match &remote {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Some(handle.clone()),
                _ => None,
            },
            Location::Root | Location::Sftp | Location::Local(_) => None,
        };
        let Some(handle) = handle else {
            self.keep_edit(&path, &editing.file);
            return Vec::new();
        };
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        let then = Then::Discard {
            copy: editing.file.clone(),
            remote: remote.clone(),
        };
        let job = Job::new(id, JobKind::Copy, vec![editing.dir], cancel.clone()).then(then);
        self.jobs.push(job);
        vec![Effect::Copy {
            id,
            sources: vec![Location::Local(editing.file)],
            target: remote,
            hosts: (None, Some(handle)),
            options: CopyOptions {
                preserve: true,
                atomic: self.copy_choices.atomic,
                remove_sources: false,
                overwrite: true,
            },
            cancel,
        }]
    }

    /// Opens the command line for `action`: `!` for a shell command, `:` for commands of Noon
    /// Commander, Alt-H with the window of the history; in a panel on a local directory or on
    /// one of a connected host.
    fn open_command_line(&mut self, action: Action) {
        let kind = if action == Action::Command {
            command::Kind::Noc
        } else {
            command::Kind::Shell
        };
        let opens = match self.panel(self.active).location() {
            Location::Local(_) => true,
            Location::Remote { host, .. } => {
                matches!(self.hosts.get(host), Some(Host::Connected { .. }))
            }
            Location::Root | Location::Sftp => false,
        };
        if opens {
            self.command_line = Some(CommandLine::new(kind));
            if action == Action::CommandHistory {
                self.open_history();
            }
        }
    }

    /// Gives a key to the command line: Enter hands its command to the event loop, to run in
    /// the active panel's directory.
    fn handle_command_line(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(line) = &mut self.command_line else {
            return Vec::new();
        };
        let event = line.handle(input);
        match event {
            CommandEvent::None => Vec::new(),
            CommandEvent::Close => {
                self.command_line = None;
                Vec::new()
            }
            CommandEvent::Older | CommandEvent::Newer => {
                let host = match self.panel(self.active).location() {
                    Location::Remote { host, .. } => Some(host.as_str()),
                    _ => None,
                };
                let commands: Vec<&str> = self
                    .history
                    .iter()
                    .rev()
                    .filter(|entry| entry.host.as_deref() == host)
                    .map(|entry| entry.command.as_str())
                    .collect();
                if let Some(line) = &mut self.command_line {
                    line.browse(event == CommandEvent::Older, &commands);
                }
                Vec::new()
            }
            CommandEvent::History => {
                self.open_history();
                Vec::new()
            }
            CommandEvent::Edit => {
                let name = format!("command-{}.sh", std::process::id());
                let text = line.text().to_owned();
                self.command_edit = Some((self.runtime_dir.join(name), text));
                Vec::new()
            }
            CommandEvent::Unknown(text) => {
                let text = cells::sanitize(text.as_bytes());
                self.show_error(&fl!("command-unknown", command = text));
                Vec::new()
            }
            CommandEvent::Run(command) => {
                self.command_line = None;
                let dir = self.panel(self.active).location().clone();
                let place = match &dir {
                    Location::Local(path) => Place::Local(path.clone()),
                    Location::Remote { host, path } => {
                        let Some(Host::Connected { handle, .. }) = self.hosts.get(host) else {
                            self.show_error(&fl!("error-connection-closed"));
                            return Vec::new();
                        };
                        Place::Remote {
                            handle: handle.clone(),
                            dir: path.clone(),
                        }
                    }
                    Location::Root | Location::Sftp => return Vec::new(),
                };
                let mut effects = self.remember(&dir, &command);
                self.run_now = Some(Run { place, command });
                effects.extend(self.note(&dir));
                effects
            }
        }
    }

    /// Opens the window of the command history over the command line, if it is open.
    fn open_history(&mut self) {
        if self.command_line.is_none() {
            return;
        }
        let here = match self.panel(self.active).location() {
            Location::Remote { host, .. } => Some(host.clone()),
            _ => None,
        };
        let labels = self
            .history
            .iter()
            .filter_map(|entry| entry.host.as_deref())
            .filter_map(|host| {
                let label = self.host_settings.get(host).and_then(HostConfig::label)?;
                Some((host.to_owned(), label.to_owned()))
            })
            .collect();
        let fuzzy = self.config.ui.fuzzy_search;
        self.history_window = Some(HistoryWindow::new(&self.history, here, labels, fuzzy));
    }

    /// Gives a key to the window of the command history: a command it takes goes on the command
    /// line, which shows whether it ran on another host; Delete removes one.
    fn handle_history(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.history_window else {
            return Vec::new();
        };
        match window.handle(input) {
            HistoryEvent::Pending => Vec::new(),
            HistoryEvent::Closed => {
                self.history_window = None;
                Vec::new()
            }
            HistoryEvent::Take { command, host } => {
                self.history_window = None;
                let here = match self.panel(self.active).location() {
                    Location::Remote { host, .. } => Some(host.as_str()),
                    _ => None,
                };
                let foreign = host.as_deref() != here;
                if let Some(line) = &mut self.command_line {
                    line.take(&command, foreign);
                }
                Vec::new()
            }
            HistoryEvent::Delete { command, host } => {
                self.history
                    .retain(|entry| !entry.is(host.as_deref(), &command));
                window.set_history(&self.history);
                vec![Effect::History(HistoryChange::Remove { host, command })]
            }
        }
    }

    /// Keeps `command`, run in `dir`, as the newest in the history, here and in `history.toml`;
    /// not if it starts with a space, as bash's `HISTCONTROL=ignorespace`, or while
    /// `shell.history_size` is 0.
    fn remember(&mut self, dir: &Location, command: &str) -> Vec<Effect> {
        let size = self.config.shell.history_size;
        if size == 0 || command.starts_with(' ') {
            return Vec::new();
        }
        let (host, dir) = match dir {
            Location::Local(path) => (None, path.to_string_lossy().into_owned()),
            Location::Remote { host, path } => (
                Some(host.clone()),
                String::from_utf8_lossy(path.as_bytes()).into_owned(),
            ),
            Location::Root | Location::Sftp => return Vec::new(),
        };
        let time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
            });
        let entry = HistoryEntry {
            command: command.to_owned(),
            host,
            dir,
            time,
        };
        self.history
            .retain(|kept| !kept.is(entry.host.as_deref(), &entry.command));
        self.history.push(entry.clone());
        let excess = self.history.len().saturating_sub(size);
        self.history.drain(..excess);
        vec![Effect::History(HistoryChange::Add { entry, size })]
    }

    /// Takes the commands as `history.toml` holds them after a change, or why it could not
    /// be read or written.
    pub(crate) fn history_changed(&mut self, result: Result<Vec<HistoryEntry>, String>) {
        match result {
            Ok(history) => {
                self.history = history;
                if let Some(window) = &mut self.history_window {
                    window.set_history(&self.history);
                }
            }
            Err(reason) => {
                let reason = cells::sanitize(reason.as_bytes());
                self.show_error(&fl!("history-error", reason = reason));
            }
        }
    }

    /// Takes the end of a shell command, or why the shell could not run; both panels read
    /// their directories again, as in mc, since the command may have changed anything.
    pub(crate) fn ran(&mut self, result: Result<(), String>) -> Vec<Effect> {
        if let Err(reason) = result {
            self.show_error(&cells::sanitize(reason.as_bytes()));
        }
        let mut dirs: Vec<Location> = Vec::new();
        for side in [self.active, self.active.other()] {
            let dir = self.panel(side).location().clone();
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        dirs.iter().flat_map(|dir| self.reload(dir)).collect()
    }

    /// What starts the command line: `:` for commands of Noon Commander; for a shell command,
    /// the panel's directory, with `~` for the home directory, or `host:path` with the host's
    /// label if it has one, its middle cut if it would take more than a third of `width`, and
    /// `$`.
    fn command_prompt(&self, width: u16) -> String {
        let Some(line) = &self.command_line else {
            return String::new();
        };
        if line.kind() == command::Kind::Noc {
            return ":".to_owned();
        }
        let dir = match self.panel(self.active).location() {
            Location::Local(path) => match path.strip_prefix(&self.home) {
                Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
                Ok(rest) => format!("~/{}", cells::sanitize(rest.as_os_str().as_bytes())),
                Err(_) => cells::sanitize(path.as_os_str().as_bytes()),
            },
            Location::Remote { host, path } => {
                let label = self
                    .host_settings
                    .get(host)
                    .and_then(HostConfig::label)
                    .unwrap_or(host);
                location_text(&Location::Remote {
                    host: label.to_owned(),
                    path: path.clone(),
                })
            }
            other => location_text(other),
        };
        let room = usize::from(width / 3).max(1);
        let dir = if cells::width(&dir) > room {
            cells::fit(&dir, room, Align::Left)
        } else {
            dir
        };
        format!("{dir} $ ")
    }

    /// Says that the edited copy of `path` could not go back, and where it stays.
    fn keep_edit(&mut self, path: &str, file: &Path) {
        let copy = cells::sanitize(file.as_os_str().as_bytes());
        self.show_error(&fl!("edit-kept", path = path, copy = copy));
    }

    /// Takes what [`Effect::Read`] read for the viewer `id`; an error closes the viewer and
    /// shows why.
    pub(crate) fn read(&mut self, id: u64, result: Result<(Vec<u8>, bool), String>) {
        let Some(viewing) = self.viewing.as_mut().filter(|viewing| viewing.id == id) else {
            return;
        };
        match result {
            Ok((bytes, truncated)) => viewing.viewer.show(&bytes, truncated),
            Err(reason) => {
                let path = location_text(&viewing.location);
                self.close_viewer();
                let reason = cells::sanitize(reason.as_bytes());
                self.show_error(&fl!("viewer-error", path = path, reason = reason));
            }
        }
    }

    /// Takes a key for the viewer: F1 shows the help over it.
    fn handle_viewer(&mut self, input: Resolved) {
        let Resolved::Action(action) = input else {
            return;
        };
        match action {
            Action::Help => {
                self.help = Some(self.help_screen());
            }
            Action::Redraw => self.redraw = true,
            Action::Quit | Action::Cancel => self.close_viewer(),
            action => {
                if let Some(viewing) = &mut self.viewing
                    && let Some(command) = viewer_command(action)
                {
                    viewing.viewer.handle(command);
                }
            }
        }
    }

    fn close_viewer(&mut self) {
        if let Some(viewing) = self.viewing.take() {
            viewing.cancel.cancel();
        }
    }

    /// Asks before F8 deletes the marked entries of the active panel, or the one under the
    /// cursor, in red with Yes as the default, as mc does.
    fn ask_delete(&mut self) {
        let panel = self.panel(self.active);
        let chosen = panel.chosen();
        let message = match chosen.as_slice() {
            [] => return,
            [entry] => {
                let name = cells::sanitize(&entry.name);
                if entry.metadata.kind == FileKind::Dir {
                    fl!("delete-directory", name = name)
                } else {
                    fl!("delete-file", name = name)
                }
            }
            many => fl!("delete-many", count = many.len()),
        };
        let names = chosen.iter().map(|entry| entry.name.clone()).collect();
        let dir = panel.location().clone();
        let buttons = vec![Button::Yes, Button::No];
        let dialog = Dialog::question(&fl!("delete-title"), &message, buttons, 0, true);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Delete { dir, names },
        });
    }

    /// Starts deleting `names` in `dir`, and shows its progress.
    fn start_delete(&mut self, dir: Location, names: &[Vec<u8>]) -> Vec<Effect> {
        let host = match &dir {
            Location::Remote { host, .. } => {
                if let Some(Host::Connected { handle, .. }) = self.hosts.get(host) {
                    Some(handle.clone())
                } else {
                    let reason = fl!("error-connection-closed");
                    let path = location_text(&dir);
                    self.show_error(&fl!("delete-error", path = path, reason = reason));
                    return Vec::new();
                }
            }
            Location::Root | Location::Sftp | Location::Local(_) => None,
        };
        let targets = names.iter().filter_map(|name| child(&dir, name)).collect();
        let noted = self.note(&dir);
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        let job = Job::new(id, JobKind::Delete, vec![dir], cancel.clone());
        let effect = Effect::Delete {
            id,
            targets,
            host,
            cancel,
        };
        let mut effects = self.launch(job, effect);
        effects.extend(noted);
        effects
    }

    /// Starts `job` with `effect`, or lets it wait while as many jobs run as
    /// `transfer.parallel_jobs` allows. The jobs of F4 never wait.
    fn launch(&mut self, mut job: Job, effect: Effect) -> Vec<Effect> {
        let running = self.jobs.iter().filter(|job| job.queued.is_none()).count();
        if job.then.is_some() || running < self.parallel_jobs {
            self.jobs.push(job);
            return vec![effect];
        }
        job.view.wait();
        job.queued = Some(effect);
        self.jobs.push(job);
        Vec::new()
    }

    /// Starts the jobs that wait, the oldest first, as far as `transfer.parallel_jobs` allows.
    fn start_queued(&mut self) -> Vec<Effect> {
        let mut running = self.jobs.iter().filter(|job| job.queued.is_none()).count();
        let mut effects = Vec::new();
        for job in &mut self.jobs {
            if running >= self.parallel_jobs {
                break;
            }
            if let Some(effect) = job.queued.take() {
                job.view.scanning(0);
                effects.push(effect);
                running += 1;
            }
        }
        effects
    }

    /// Asks where F5 copies, or F6 moves, the marked entries of the active panel, or the one
    /// under the cursor, as mc does: the field opens with the other panel's location. Copies
    /// may preserve attributes; moves always do.
    fn ask_transfer(&mut self, kind: JobKind) {
        let panel = self.panel(self.active);
        let chosen = panel.chosen();
        let message = match (chosen.as_slice(), kind) {
            ([], _) => return,
            ([entry], JobKind::Move) => fl!("move-one", name = cells::sanitize(&entry.name)),
            ([entry], _) => fl!("copy-one", name = cells::sanitize(&entry.name)),
            (many, JobKind::Move) => fl!("move-many", count = many.len()),
            (many, _) => fl!("copy-many", count = many.len()),
        };
        let names = chosen.iter().map(|entry| entry.name.clone()).collect();
        let dir = panel.location().clone();
        let other = self.panel(self.active.other()).location().clone();
        let offered = (!other.is_virtual()).then(|| (location_text(&other), other));
        let text = offered
            .as_ref()
            .map(|(text, _)| text.clone())
            .unwrap_or_default();
        let (title, checks) = if kind == JobKind::Move {
            (fl!("move-title"), Vec::new())
        } else {
            let preserve = (fl!("copy-preserve"), self.copy_choices.preserve);
            (fl!("copy-title"), vec![preserve])
        };
        let dialog =
            Dialog::form(&title, &message, &text, &checks, COPY_DIALOG_WIDTH).with_completion();
        let panel = self.shown(self.active);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Transfer {
                kind,
                panel,
                dir,
                names,
                offered,
            },
        });
    }

    /// Where a typed target points: `host:path` on a host the app knows, or a path from the
    /// directory of `panel`.
    fn resolve_target(&self, panel: PanelId, text: &str) -> Option<Location> {
        if text.is_empty() {
            return None;
        }
        if let Location::Remote { host, path } = Location::parse(text)
            && self.hosts.contains_key(&host)
        {
            return Some(Location::Remote { host, path });
        }
        self.panel_of(panel)?.resolve(text)
    }

    /// Starts copying or moving `names` from `dir` to `target`, and shows its progress; a
    /// target that is one of the sources, or in one, is an error.
    fn start_transfer(
        &mut self,
        kind: JobKind,
        dir: Location,
        names: &[Vec<u8>],
        target: Location,
    ) -> Vec<Effect> {
        let sources: Vec<Location> = names.iter().filter_map(|name| child(&dir, name)).collect();
        let error = |reason: String| {
            let path = location_text(&target);
            if kind == JobKind::Move {
                fl!("move-error", path = path, reason = reason)
            } else {
                fl!("copy-error", path = path, reason = reason)
            }
        };
        if target == dir {
            self.show_error(&error(fl!("transfer-same")));
            return Vec::new();
        }
        if let Some(source) = sources.iter().find(|source| within(source, &target)) {
            let reason = fl!("transfer-into-itself", path = location_text(source));
            self.show_error(&error(reason));
            return Vec::new();
        }
        let handle = |location: &Location| match location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Ok(Some(handle.clone())),
                _ => Err(()),
            },
            Location::Root | Location::Sftp | Location::Local(_) => Ok(None),
        };
        let (Ok(from), Ok(to)) = (handle(&dir), handle(&target)) else {
            self.show_error(&error(fl!("error-connection-closed")));
            return Vec::new();
        };
        // The source, and the target if a panel shows it.
        let mut noted = self.note(&dir);
        noted.extend(self.note(&target));
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        // Into the target, or to it as a new name in its parent; a move empties the source.
        let changes = vec![target.clone(), target.parent(), dir];
        let job = Job::new(id, kind, changes, cancel.clone());
        let moving = kind == JobKind::Move;
        let options = CopyOptions {
            preserve: self.copy_choices.preserve || moving,
            atomic: self.copy_choices.atomic,
            remove_sources: moving,
            overwrite: false,
        };
        let effect = Effect::Copy {
            id,
            sources,
            target,
            hosts: (from, to),
            options,
            cancel,
        };
        let mut effects = self.launch(job, effect);
        effects.extend(noted);
        effects
    }

    /// Asks what to do about a taken name in the copy job `id`, as mc does: in red, with the
    /// sizes and times of both, and No as the default.
    fn ask_conflict(
        &mut self,
        id: u64,
        target: &Location,
        (new, old): (&Metadata, &Metadata),
        reply: oneshot::Sender<Conflict>,
    ) {
        let now = SystemTime::now();
        let size = |metadata: &Metadata| metadata.size.map_or_else(String::new, cells::grouped);
        let time = |metadata: &Metadata| {
            cells::mtime(metadata.modified, now, &self.tz)
                .trim()
                .to_owned()
        };
        let message = fl!(
            "copy-exists",
            path = location_text(target),
            new_size = size(new),
            new_time = time(new),
            old_size = size(old),
            old_time = time(old)
        );
        let buttons = vec![
            Button::Yes,
            Button::No,
            Button::All,
            Button::KeepAll,
            Button::Older,
            Button::Abort,
        ];
        let title = fl!("copy-exists-title");
        let dialog = Dialog::question(&title, &message, buttons, 1, true);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Conflict { job: id, reply },
        });
    }

    /// Takes a report from the job `id`: progress for its window, a failure for a dialog
    /// that waits for an answer, or its end.
    pub(crate) fn job_event(&mut self, id: u64, event: JobEvent) -> Vec<Effect> {
        let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return Vec::new();
        };
        let now = Instant::now();
        match event {
            JobEvent::Scanning { items } => job.view.scanning(items),
            JobEvent::Progress {
                current,
                done,
                total,
                bytes_done,
                bytes_total,
                bytes_copied,
            } => {
                let counts = Counts {
                    done,
                    total,
                    bytes_done,
                    bytes_total,
                    bytes_copied,
                };
                job.view.working(location_text(&current), counts, now);
            }
            JobEvent::Exists {
                target,
                source_metadata,
                target_metadata,
                reply,
            } => {
                job.view.ask(now);
                let metadata = (&source_metadata, &target_metadata);
                self.ask_conflict(id, &target, metadata, reply);
            }
            JobEvent::Failed { path, error, reply } => {
                job.view.ask(now);
                let path = location_text(&path);
                let reason = cells::sanitize(error.as_bytes());
                let message = match job.kind {
                    JobKind::Delete => fl!("delete-error", path = path, reason = reason),
                    JobKind::Copy => fl!("copy-error", path = path, reason = reason),
                    JobKind::Move => fl!("move-error", path = path, reason = reason),
                    JobKind::Checksum => fl!("checksum-error", path = path, reason = reason),
                };
                let buttons = vec![Button::Skip, Button::SkipAll, Button::Retry, Button::Abort];
                let dialog = Dialog::question(&fl!("dialog-error"), &message, buttons, 0, true);
                self.dialogs.push_back(Open {
                    dialog,
                    purpose: Purpose::Failure { job: id, reply },
                });
            }
            JobEvent::Sums(sums) => {
                if let Some(hashing) = &mut job.hashing {
                    hashing.sums = Some(sums);
                }
            }
            JobEvent::Finished { complete } => {
                if let Some(mut job) = self.end_job(id) {
                    if let Some(hashing) = job.hashing.take() {
                        self.show_sums(id, hashing);
                    }
                    let mut effects = job
                        .then
                        .map_or_else(Vec::new, |then| self.follow_up(then, complete));
                    for (index, dir) in job.changes.iter().enumerate() {
                        if !job.changes[..index].contains(dir) {
                            effects.extend(self.reload(dir));
                        }
                    }
                    effects.extend(self.start_queued());
                    return effects;
                }
            }
        }
        Vec::new()
    }

    /// Takes the job `id` away, with the questions it asked.
    fn end_job(&mut self, id: u64) -> Option<Job> {
        let index = self.jobs.iter().position(|job| job.id == id)?;
        let job = self.jobs.remove(index);
        self.dialogs.retain(|open| match open.purpose {
            Purpose::Failure { job: asked, .. } | Purpose::Conflict { job: asked, .. } => {
                asked != job.id
            }
            _ => true,
        });
        Some(job)
    }

    /// Does what follows a job of F4 once it ends, `complete` if it did all it was asked: opens
    /// the editor on a copy that came, or removes a copy that went back. A partial copy goes;
    /// an edited one that did not go back stays, and the user hears where.
    fn follow_up(&mut self, then: Then, complete: bool) -> Vec<Effect> {
        match (then, complete) {
            (Then::Edit, true) => {
                self.edit_now = self.editing.as_ref().map(|editing| editing.file.clone());
                Vec::new()
            }
            (Then::Edit, false) => self
                .editing
                .take()
                .map(|editing| Effect::Discard(editing.file))
                .into_iter()
                .collect(),
            (Then::Discard { copy, .. }, true) => vec![Effect::Discard(copy)],
            (Then::Discard { copy, remote }, false) => {
                self.keep_edit(&location_text(&remote), &copy);
                Vec::new()
            }
        }
    }

    /// Reads `dir` again in the panels that show it, with their cursors where they were;
    /// hidden tabs read it once they show.
    fn reload(&mut self, dir: &Location) -> Vec<Effect> {
        let mut effects = Vec::new();
        for id in self.panel_ids() {
            let shown = self.is_shown(id);
            let Some(tab) = self.tab_mut(id) else {
                continue;
            };
            if tab.panel.location() != dir {
                continue;
            }
            if shown {
                let here = tab.panel.here();
                let request = tab.panel.go(here);
                effects.extend(self.route(id, request));
            } else {
                tab.stale = true;
            }
        }
        effects
    }

    /// How to reach `location`: `None` here, the handle of its host if that is connected,
    /// and an error if it is not.
    fn handle_for(&self, location: &Location) -> Result<Option<HostHandle>, ()> {
        match location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Ok(Some(handle.clone())),
                _ => Err(()),
            },
            Location::Root | Location::Sftp | Location::Local(_) => Ok(None),
        }
    }

    /// Opens the checksum dialog for the marked entries of the active panel, or the one under
    /// the cursor: the algorithm, last chosen first; for one file, a field for the checksum
    /// it should have, and, if the other panel's cursor is on a file, a check box to compare
    /// the two, checked if they have the same name.
    fn ask_checksum(&mut self) {
        let panel = self.panel(self.active);
        let chosen = panel.chosen();
        let single_file = match chosen.as_slice() {
            [entry] => !entry.is_dir_like(),
            _ => false,
        };
        let message = match chosen.as_slice() {
            [] => return,
            [entry] if single_file => fl!("checksum-one", name = cells::sanitize(&entry.name)),
            [entry] => fl!("checksum-directory", name = cells::sanitize(&entry.name)),
            many => fl!("checksum-many", count = many.len()),
        };
        let names: Vec<Vec<u8>> = chosen.iter().map(|entry| entry.name.clone()).collect();
        let dir = panel.location().clone();
        let other_panel = self.panel(self.active.other());
        let other = other_panel
            .entry_under_cursor()
            .filter(|entry| single_file && !entry.is_dir_like())
            .and_then(|entry| {
                let location = child(other_panel.location(), &entry.name)?;
                Some((location, names.first() == Some(&entry.name)))
            })
            .filter(|(location, _)| child(&dir, &names[0]).as_ref() != Some(location));
        let fields = if single_file {
            vec![(fl!("checksum-expected"), String::new())]
        } else {
            Vec::new()
        };
        let checks: Vec<(String, bool)> = other
            .iter()
            .map(|(location, same)| {
                let path = location_text(location);
                (fl!("checksum-compare", path = path), *same)
            })
            .collect();
        let choices = Algorithm::ALL.iter().map(|&a| algorithm_name(a)).collect();
        let chosen = Algorithm::ALL
            .iter()
            .position(|&a| a == self.algorithm)
            .unwrap_or(0);
        let buttons = vec![Button::Ok, Button::Cancel];
        let title = fl!("checksum-title");
        let dialog = Dialog::fields(&title, &fields, &checks, buttons, CHECKSUM_DIALOG_WIDTH)
            .with_message(&message)
            .with_choices(choices, chosen);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Checksum {
                dir,
                names,
                other: other.map(|(location, _)| location),
                expect: single_file,
            },
        });
    }

    /// Starts the checksum job that the dialog of [`Self::ask_checksum`] asked for. An expected
    /// checksum of another length picks the algorithm that has it.
    fn start_checksum(
        &mut self,
        dialog: &Dialog,
        dir: Location,
        names: &[Vec<u8>],
        other: Option<Location>,
        expect: bool,
    ) -> Vec<Effect> {
        let mut algorithm = Algorithm::ALL
            .get(dialog.chosen())
            .copied()
            .unwrap_or(Algorithm::Sha256);
        let typed = if expect { dialog.text().trim() } else { "" };
        let expected = if typed.is_empty() {
            None
        } else if let Some((hex, by_length)) = parse_expected(typed, algorithm) {
            algorithm = by_length;
            Some(hex)
        } else {
            let text = cells::sanitize(typed.as_bytes());
            self.show_error(&fl!("checksum-expected-invalid", text = text));
            return Vec::new();
        };
        self.algorithm = algorithm;
        let compare = other.is_some() && dialog.checked(0);
        let mut groups = vec![names.iter().filter_map(|name| child(&dir, name)).collect()];
        if compare {
            groups.extend(other.map(|location| vec![location]));
        }
        let mut targets = Vec::new();
        let mut hosts = Vec::new();
        for group in groups {
            let Some(first) = group.first() else { continue };
            let Ok(handle) = self.handle_for(first) else {
                let reason = fl!("error-connection-closed");
                let path = location_text(first);
                self.show_error(&fl!("checksum-error", path = path, reason = reason));
                return Vec::new();
            };
            if let Location::Remote { host, .. } = first {
                hosts.push(host.clone());
            }
            targets.push((group, handle));
        }
        let noted = self.note(&dir);
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        let mut job = Job::new(id, JobKind::Checksum, Vec::new(), cancel.clone());
        job.hosts = hosts;
        job.hashing = Some(Hashing {
            algorithm,
            expected,
            compare,
            dir: (!compare).then_some(dir),
            sums: None,
        });
        let effect = Effect::Checksum {
            id,
            targets,
            algorithm,
            cancel,
        };
        let mut effects = self.launch(job, effect);
        effects.extend(noted);
        effects
    }

    /// Opens the window with the checksums of the job `id`; an aborted job has none to show,
    /// and a job that found no files says so.
    fn show_sums(&mut self, id: u64, hashing: Hashing) {
        let Some(sums) = hashing.sums else {
            return;
        };
        if sums.is_empty() {
            self.dialogs.push_back(Open {
                dialog: Dialog::notice(&fl!("checksum-title"), &fl!("checksum-no-files")),
                purpose: Purpose::Info,
            });
            return;
        }
        let hexes: Vec<Option<String>> = sums
            .iter()
            .map(|sum| sum.digest.as_deref().map(hex))
            .collect();
        let compared = hashing.compare && sums.len() == 2;
        let same = compared && hexes[0].is_some() && hexes[0] == hexes[1];
        let rows: Vec<SumRow> = sums
            .iter()
            .zip(&hexes)
            .map(|(sum, hex)| {
                let mark = match (hex, &hashing.expected) {
                    (None, _) => Mark::Skipped,
                    (Some(hex), Some(expected)) if hex == expected => Mark::Match,
                    (Some(_), Some(_)) => Mark::Mismatch,
                    (Some(_), None) if compared && same => Mark::Match,
                    (Some(_), None) if compared => Mark::Mismatch,
                    (Some(_), None) => Mark::None,
                };
                let name = if compared {
                    location_text(&sum.path)
                } else {
                    cells::sanitize(&sum.name)
                };
                SumRow {
                    name,
                    hex: hex.clone(),
                    mark,
                }
            })
            .collect();
        let verdict = if compared {
            Some(if same {
                Verdict {
                    text: fl!("checksum-same"),
                    good: true,
                }
            } else {
                Verdict {
                    text: fl!("checksum-different"),
                    good: false,
                }
            })
        } else {
            hashing.expected.as_ref().map(|expected| {
                if hexes[0].as_ref() == Some(expected) {
                    Verdict {
                        text: fl!("checksum-matches"),
                        good: true,
                    }
                } else {
                    Verdict {
                        text: fl!("checksum-differs"),
                        good: false,
                    }
                }
            })
        };
        let mut buttons = vec![SumsButton::Copy];
        if rows.len() > 1 {
            buttons.push(SumsButton::CopyAll);
        }
        if hashing.dir.is_some() {
            buttons.push(SumsButton::Save);
        }
        buttons.push(SumsButton::Close);
        let lines = sums
            .into_iter()
            .zip(hexes)
            .map(|(sum, hex)| {
                let name = if compared {
                    location_text(&sum.path).into_bytes()
                } else {
                    sum.name
                };
                (name, hex)
            })
            .collect();
        let title = algorithm_name(hashing.algorithm);
        self.results.push_back(Results {
            id,
            window: SumsWindow::new(title, rows, verdict, buttons),
            algorithm: hashing.algorithm,
            dir: hashing.dir,
            lines,
        });
    }

    /// Gives a key to the window of checksums in front, if there is one: Copy puts a checksum
    /// on the clipboard, Save asks where to save them all.
    fn handle_results(&mut self, input: Resolved) -> bool {
        let Some(results) = self.results.front_mut() else {
            return false;
        };
        match results.window.handle(input) {
            SumsEvent::Pending => {}
            SumsEvent::Copy(row) => {
                if let Some((_, Some(hex))) = results.lines.get(row) {
                    self.clipboard = Some(hex.clone());
                    results.window.set_status(fl!("checksum-copied"));
                }
            }
            SumsEvent::CopyAll => {
                let text = sums_file(&results.lines);
                self.clipboard = Some(String::from_utf8_lossy(&text).into_owned());
                results.window.set_status(fl!("checksum-copied"));
            }
            SumsEvent::Save => {
                if let Some(dir) = results.dir.clone() {
                    let name = match results.lines.as_slice() {
                        [(name, _)] => {
                            let name = String::from_utf8_lossy(name).into_owned();
                            format!("{name}.{}", extension(results.algorithm))
                        }
                        _ => format!("{}SUMS", extension(results.algorithm).to_uppercase()),
                    };
                    let bytes = sums_file(&results.lines);
                    let window = results.id;
                    let (title, prompt) = (fl!("checksum-save-title"), fl!("checksum-save-prompt"));
                    let dialog = Dialog::form(&title, &prompt, &name, &[], MKDIR_DIALOG_WIDTH);
                    self.dialogs.push_back(Open {
                        dialog,
                        purpose: Purpose::SaveSums { window, dir, bytes },
                    });
                }
            }
            SumsEvent::Closed => {
                self.results.pop_front();
            }
        }
        true
    }

    /// Writes the checksums of the window `window` to `location`, over what is there if
    /// `replace`.
    fn write_sums(
        &mut self,
        window: u64,
        location: Location,
        bytes: Vec<u8>,
        replace: bool,
    ) -> Vec<Effect> {
        let Ok(host) = self.handle_for(&location) else {
            let reason = fl!("error-connection-closed");
            let path = location_text(&location);
            self.show_error(&fl!("checksum-save-error", path = path, reason = reason));
            return Vec::new();
        };
        self.saving
            .insert(location.clone(), (window, bytes.clone()));
        vec![Effect::WriteFile {
            location,
            bytes,
            replace,
            host,
        }]
    }

    /// Takes the end of an [`Effect::WriteFile`]: panels on its directory read it again, and
    /// the window it came from says where it went; a taken name asks whether to replace it.
    /// `Err(None)` is a taken name.
    pub(crate) fn written(
        &mut self,
        location: &Location,
        result: Result<(), Option<String>>,
    ) -> Vec<Effect> {
        let Some((window, bytes)) = self.saving.remove(location) else {
            return Vec::new();
        };
        let path = location_text(location);
        match result {
            Ok(()) => {
                if let Some(results) = self.results.iter_mut().find(|r| r.id == window) {
                    results
                        .window
                        .set_status(fl!("checksum-saved", path = path));
                }
                self.reload(&location.parent())
            }
            Err(None) => {
                let message = fl!("checksum-exists", path = path);
                let buttons = vec![Button::Yes, Button::No];
                let title = fl!("copy-exists-title");
                let dialog = Dialog::question(&title, &message, buttons, 1, true);
                self.dialogs.push_back(Open {
                    dialog,
                    purpose: Purpose::OverwriteSums {
                        window,
                        location: location.clone(),
                        bytes,
                    },
                });
                Vec::new()
            }
            Err(Some(reason)) => {
                let reason = cells::sanitize(reason.as_bytes());
                self.show_error(&fl!("checksum-save-error", path = path, reason = reason));
                Vec::new()
            }
        }
    }

    /// Opens the dialog of Quick cd, Alt-C, for the active panel, as in mc.
    fn ask_cd(&mut self) {
        let (title, prompt) = (fl!("cd-title"), fl!("cd-prompt"));
        let dialog = Dialog::form(&title, &prompt, "", &[], MKDIR_DIALOG_WIDTH).with_completion();
        let panel = self.shown(self.active);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::QuickCd { panel },
        });
    }

    /// Sends `panel` where `cd <text>` leads from its directory. A path that is not there
    /// leaves the panel where it is and says why, as any listing does.
    fn cd(&mut self, panel: PanelId, text: &str) -> Vec<Effect> {
        let Some(tab) = self.tabs(panel.side).get(panel.tab) else {
            return Vec::new();
        };
        let here = tab.panel.location();
        let Some(location) = cd::target(text, here, &self.home, tab.previous.as_ref()) else {
            return Vec::new();
        };
        if location == *here {
            return Vec::new();
        }
        let destination = tab.panel.destination(location);
        self.go(panel, destination)
    }

    /// Opens the dialog of F7 for the active panel, with the name under the cursor, as in mc.
    fn ask_mkdir(&mut self) {
        let panel = self.panel(self.active);
        if panel.shows_root() {
            return;
        }
        let name = panel
            .name_under_cursor()
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .unwrap_or_default();
        let (title, prompt) = (fl!("mkdir-title"), fl!("mkdir-prompt"));
        let dialog =
            Dialog::form(&title, &prompt, &name, &[], MKDIR_DIALOG_WIDTH).with_completion();
        let panel = self.shown(self.active);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Mkdir { panel },
        });
    }

    /// Makes the directory at `location` for `panel`, in the background; that counts as work
    /// in the panel's directory.
    fn create_dir(&mut self, panel: PanelId, location: Location) -> Vec<Effect> {
        let noted = match self.panel_of(panel) {
            Some(shown) => {
                let dir = shown.location().clone();
                self.note(&dir)
            }
            None => Vec::new(),
        };
        let host = match &location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Some(handle.clone()),
                _ => None,
            },
            Location::Root | Location::Sftp | Location::Local(_) => None,
        };
        let mut effects = vec![Effect::CreateDir {
            panel,
            location,
            host,
        }];
        effects.extend(noted);
        effects
    }

    /// Takes the result of an [`Effect::CreateDir`]: panels on the directory it is in read it
    /// again, the one that asked with the cursor on it, and hidden tabs once they show; an
    /// error shows in a dialog.
    pub(crate) fn created(
        &mut self,
        panel: PanelId,
        location: &Location,
        result: Result<(), String>,
    ) -> Vec<Effect> {
        if let Err(reason) = result {
            let path = location_text(location);
            let reason = cells::sanitize(reason.as_bytes());
            self.show_error(&fl!("mkdir-error", path = path, reason = reason));
            return Vec::new();
        }
        self.show_new(panel, location)
    }

    /// Panels on the directory that holds the new `location` read it again, `panel` with the
    /// cursor on it, and hidden tabs once they show.
    fn show_new(&mut self, panel: PanelId, location: &Location) -> Vec<Effect> {
        let parent = location.parent();
        let name = file_name(location);
        let mut effects = Vec::new();
        for id in self.panel_ids() {
            let shown = self.is_shown(id);
            let Some(tab) = self.tab_mut(id) else {
                continue;
            };
            if *tab.panel.location() != parent {
                continue;
            }
            let request = match &name {
                Some(name) if id == panel => tab.panel.reload_onto(name.clone()),
                _ if !shown => {
                    tab.stale = true;
                    continue;
                }
                _ => {
                    let here = tab.panel.here();
                    tab.panel.go(here)
                }
            };
            effects.extend(self.route(id, request));
        }
        effects
    }

    /// Starts renaming the entry under the cursor of the active panel in its row, as Shift-F6;
    /// a name that is not UTF-8 cannot be edited, which an error says.
    fn start_rename(&mut self) {
        let panel = self.panel_mut(self.active);
        if panel.start_rename() {
            return;
        }
        let invalid = panel
            .name_under_cursor()
            .filter(|name| std::str::from_utf8(name).is_err())
            .map(cells::sanitize);
        if let Some(name) = invalid {
            self.show_error(&fl!("rename-not-utf8", name = name));
        }
    }

    /// Gives a key to the field of the entry renamed in the active panel: Enter renames it,
    /// Esc leaves it as it was, and other keys edit the name.
    fn handle_rename(&mut self, input: Resolved) -> Vec<Effect> {
        let panel = self.panel_mut(self.active);
        match input {
            Resolved::Action(Action::Confirm) => return self.confirm_rename(),
            Resolved::Action(Action::Cancel) => {
                panel.end_rename();
            }
            Resolved::Insert(c) => {
                if let Some(field) = panel.rename_field() {
                    field.insert(c);
                }
            }
            Resolved::Action(action) => {
                if let Some(field) = panel.rename_field() {
                    field.edit(action);
                }
            }
        }
        Vec::new()
    }

    /// Renames the entry renamed in the active panel to the name in its field. An empty or
    /// unchanged name leaves it as it was; one that cannot name an entry in the directory
    /// shows an error, and the field stays for another try.
    fn confirm_rename(&mut self) -> Vec<Effect> {
        let id = self.shown(self.active);
        let Some(text) = self
            .panel_mut(self.active)
            .rename_field()
            .map(|field| field.text().to_owned())
        else {
            return Vec::new();
        };
        if text == "." || text == ".." || text.contains(['/', '\0']) {
            let name = cells::sanitize(text.as_bytes());
            self.show_error(&fl!("rename-invalid", name = name));
            return Vec::new();
        }
        let panel = self.panel_mut(self.active);
        let Some((name, _)) = panel.end_rename() else {
            return Vec::new();
        };
        if text.is_empty() || text.as_bytes() == name {
            return Vec::new();
        }
        let dir = panel.location().clone();
        match (child(&dir, &name), child(&dir, text.as_bytes())) {
            (Some(from), Some(to)) => self.rename_entry(id, from, to, false),
            _ => Vec::new(),
        }
    }

    /// Renames `from` to `to` for `panel`, in the background, over a file there if `replace`;
    /// that counts as work in the directory.
    fn rename_entry(
        &mut self,
        panel: PanelId,
        from: Location,
        to: Location,
        replace: bool,
    ) -> Vec<Effect> {
        let Ok(host) = self.handle_for(&from) else {
            let path = location_text(&from);
            let reason = fl!("error-connection-closed");
            self.show_error(&fl!("rename-error", path = path, reason = reason));
            return Vec::new();
        };
        let noted = self.note(&from.parent());
        let mut effects = vec![Effect::Rename {
            panel,
            from,
            to,
            replace,
            host,
        }];
        effects.extend(noted);
        effects
    }

    /// Takes the result of an [`Effect::Rename`]: panels on the directory read it again, the
    /// one that renamed with the cursor on the new name. A file with that name asks whether to
    /// rename over it; an error shows in a dialog.
    pub(crate) fn entry_renamed(
        &mut self,
        panel: PanelId,
        from: &Location,
        to: &Location,
        result: Result<(), Option<String>>,
    ) -> Vec<Effect> {
        match result {
            Ok(()) => self.show_new(panel, to),
            Err(None) => {
                let name = file_name(to).unwrap_or_default();
                let message = fl!("rename-exists", name = cells::sanitize(&name));
                let buttons = vec![Button::Yes, Button::No];
                let title = fl!("copy-exists-title");
                let dialog = Dialog::question(&title, &message, buttons, 1, true);
                self.dialogs.push_back(Open {
                    dialog,
                    purpose: Purpose::RenameOver {
                        panel,
                        from: from.clone(),
                        to: to.clone(),
                    },
                });
                Vec::new()
            }
            Err(Some(reason)) => {
                let path = location_text(from);
                let reason = cells::sanitize(reason.as_bytes());
                self.show_error(&fl!("rename-error", path = path, reason = reason));
                Vec::new()
            }
        }
    }

    /// Shows `message` in an error dialog.
    fn show_error(&mut self, message: &str) {
        self.dialogs.push_back(Open {
            dialog: Dialog::error(&fl!("dialog-error"), message),
            purpose: Purpose::Info,
        });
    }

    /// Opens the dialog of `+` (`mark`) or `-` for the active panel, as in mc: a pattern, and
    /// whether it applies to files only and whether case counts.
    fn ask_pattern(&mut self, mark: bool) {
        if self.panel(self.active).shows_root() {
            return;
        }
        let title = if mark {
            fl!("pattern-select")
        } else {
            fl!("pattern-unselect")
        };
        let options = &self.pattern_options;
        let checks = [
            (fl!("pattern-files-only"), options.files_only),
            (fl!("pattern-case-sensitive"), options.case_sensitive),
        ];
        let dialog = Dialog::form(&title, "", &options.pattern, &checks, PATTERN_DIALOG_WIDTH);
        let panel = self.shown(self.active);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Pattern { panel, mark },
        });
    }

    /// Marks, or unmarks, what the dialog of `+` or `-` asked for. An empty pattern does
    /// nothing.
    fn mark_matching(&mut self, panel: PanelId, mark: bool, dialog: &Dialog) {
        if dialog.text().is_empty() {
            return;
        }
        let options = PatternOptions {
            pattern: dialog.text().to_owned(),
            files_only: dialog.checked(0),
            case_sensitive: dialog.checked(1),
        };
        let fold = |text: &str| {
            if options.case_sensitive {
                text.to_owned()
            } else {
                text.to_lowercase()
            }
        };
        let pattern = Pattern::new(&fold(&options.pattern));
        if let Some(panel) = self.panel_of_mut(panel) {
            panel.mark_where(mark, |entry| {
                !(options.files_only && entry.is_dir_like())
                    && pattern.matches(&fold(&entry.display_name()))
            });
        }
        self.pattern_options = options;
    }

    /// Opens the dialog of F4 on the host under the cursor of the active panel: its settings
    /// from `hosts.toml`.
    fn ask_edit_host(&mut self) {
        let Some(name) = self
            .panel(self.active)
            .host_under_cursor()
            .map(str::to_owned)
        else {
            return;
        };
        let old = self.host_settings.get(&name).cloned();
        let sftp = match &old {
            Some(HostConfig::Sftp(sftp)) => sftp.clone(),
            None => SftpHost::default(),
        };
        // The active panel shows the hosts, so the other one is more likely on this one.
        let current = [self.active.other(), self.active]
            .into_iter()
            .find_map(|side| match self.panel(side).location() {
                Location::Remote { host, path } if *host == name && !path.as_bytes().is_empty() => {
                    Some(String::from_utf8_lossy(path.as_bytes()).into_owned())
                }
                _ => None,
            });
        let text = |value: &Option<String>| value.clone().unwrap_or_default();
        let fields = [
            (fl!("host-label"), text(&sftp.label)),
            (fl!("host-start-dir"), text(&sftp.start_dir)),
            (fl!("host-other-dir"), text(&sftp.other_dir)),
        ];
        let checks = [(fl!("host-remember-dir"), sftp.remember_dir)];
        let mut buttons = vec![Button::Ok, Button::Cancel];
        if current.is_some() {
            buttons.insert(1, Button::UseCurrent);
        }
        let title = fl!("host-edit-title", host = cells::sanitize(name.as_bytes()));
        let dialog = Dialog::fields(&title, &fields, &checks, buttons, HOST_DIALOG_WIDTH);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::EditHost { name, old, current },
        });
    }

    /// Whether the dialog in front stays open after `event`: Use Current fills in a field, and
    /// OK on host settings that are not valid shows why over it.
    fn keeps_open(&mut self, event: DialogEvent) -> bool {
        let Some(Open { dialog, purpose }) = self.dialogs.front_mut() else {
            return false;
        };
        let Purpose::EditHost { current, .. } = purpose else {
            return false;
        };
        match event {
            DialogEvent::Pressed(Button::UseCurrent) => {
                if let Some(current) = current {
                    dialog.set_text(HOST_START_DIR, current);
                }
                true
            }
            DialogEvent::Pressed(Button::Ok) => {
                let other_dir = dialog.text_of(HOST_OTHER_DIR).trim();
                if other_dir.is_empty() || noc_config::local_dir(other_dir, &self.home).is_some() {
                    return false;
                }
                let error = Dialog::error(&fl!("dialog-error"), &fl!("host-other-dir-invalid"));
                self.dialogs.push_front(Open {
                    dialog: error,
                    purpose: Purpose::Info,
                });
                true
            }
            _ => false,
        }
    }

    /// Takes the end of an [`Effect::SaveHost`]: the host settings now, which the lists of
    /// hosts show at once, or why they could not be saved.
    pub(crate) fn host_saved(&mut self, result: Result<Arc<Hosts>, String>) -> Vec<Effect> {
        match result {
            Ok(hosts) => {
                self.host_settings = hosts;
                let mut effects = self.reload(&Location::Root);
                effects.extend(self.reload(&Location::Sftp));
                effects
            }
            Err(reason) => {
                let reason = cells::sanitize(reason.as_bytes());
                self.show_error(&fl!("host-save-error", reason = reason));
                Vec::new()
            }
        }
    }

    /// Sends `panel` to `destination`.
    fn go(&mut self, panel: PanelId, destination: Destination) -> Vec<Effect> {
        let Some(shown) = self.panel_of_mut(panel) else {
            return Vec::new();
        };
        let request = shown.go(destination);
        self.open(panel, request)
    }

    /// Sends a new request of `panel`. Opening a host sends the panel that shows on the other
    /// side to the host's `other_dir`, if it has one.
    fn open(&mut self, panel: PanelId, request: ListRequest) -> Vec<Effect> {
        let other_dir = match &request.location {
            Location::Remote { host, path } if path.as_bytes().is_empty() => self
                .host_settings
                .get(host)
                .and_then(HostConfig::other_dir)
                .and_then(|dir| noc_config::local_dir(dir, &self.home)),
            _ => None,
        };
        let mut effects = self.route(panel, request);
        if let Some(dir) = other_dir {
            let other = panel.side.other();
            let request = self
                .panel_mut(other)
                .go(Destination::to(Location::Local(dir)));
            effects.extend(self.route(self.shown(other), request));
        }
        effects
    }

    /// Sends a panel's request where it can be answered. A host that is not connected gets
    /// connected first; its panels' requests go out once it is. Opening a host with
    /// `remember_dir` resumes its last directory.
    fn route(&mut self, panel: PanelId, mut request: ListRequest) -> Vec<Effect> {
        if let Location::Remote { host, path } = &request.location
            && path.as_bytes().is_empty()
            && self
                .host_settings
                .get(host)
                .is_some_and(HostConfig::remember_dir)
        {
            request.resume = self.last_dirs.get(host).cloned();
        }
        let Location::Remote { host, .. } = &request.location else {
            return vec![Effect::List {
                panel,
                request,
                host: None,
            }];
        };
        match self.hosts.get(host) {
            Some(Host::Connected { handle, .. }) => {
                let host = Some(handle.clone());
                vec![Effect::List {
                    panel,
                    request,
                    host,
                }]
            }
            Some(Host::Connecting { .. }) => Vec::new(),
            None => {
                self.failed.remove(host);
                self.connections += 1;
                let connection = self.connections;
                let stop = CancellationToken::new();
                let host = host.clone();
                let state = Host::Connecting {
                    connection,
                    stop: stop.clone(),
                };
                self.hosts.insert(host.clone(), state);
                vec![Effect::Connect {
                    host,
                    connection,
                    stop,
                }]
            }
        }
    }

    /// Stops what the panel on `side` waits for. A connection attempt stops for every panel.
    fn cancel(&mut self, side: Side) {
        let Some(request) = self.panel(side).pending_request() else {
            return;
        };
        if let Location::Remote { host, .. } = &request.location
            && let Some(state @ Host::Connecting { .. }) = self.hosts.get(host)
        {
            state.stop();
            let host = host.clone();
            self.hosts.remove(&host);
            for panel in self.panels_mut() {
                if waits_for(panel, &host) {
                    panel.cancel();
                }
            }
        }
        self.panel_mut(side).cancel();
    }

    /// Closes the connection to the host under the cursor of the active panel, or stops
    /// connecting to it.
    fn disconnect_under_cursor(&mut self) -> Vec<Effect> {
        match self.panel(self.active).host_under_cursor() {
            Some(host) => {
                let host = host.to_owned();
                self.disconnect(&host)
            }
            None => Vec::new(),
        }
    }

    /// Closes the connection to `host`, or stops connecting to it. Panels on that host go back
    /// to the list of hosts.
    fn disconnect(&mut self, host: &str) -> Vec<Effect> {
        let Some(state) = self.hosts.remove(host) else {
            return Vec::new();
        };
        state.stop();
        let mut effects = Vec::new();
        for id in self.panel_ids() {
            let Some(panel) = self.panel_of_mut(id) else {
                continue;
            };
            if matches!(state, Host::Connecting { .. }) {
                if waits_for(panel, host) {
                    panel.cancel();
                }
            } else if let Some(request) = panel.leave_host(host, None) {
                effects.extend(self.route(id, request));
            }
        }
        self.sync_connected();
        effects
    }

    /// Takes the address `ssh -G` gave for a host.
    pub(crate) fn resolved(&mut self, host: String, address: String) {
        self.addresses.insert(host, address);
    }

    /// What the root shows for a host.
    fn host_state(&self, host: &str) -> HostState {
        let status = match self.hosts.get(host) {
            Some(Host::Connecting { .. }) => HostStatus::Connecting,
            Some(Host::Connected { .. }) => HostStatus::Connected,
            None if self.failed.contains(host) => HostStatus::Failed,
            None => HostStatus::Idle,
        };
        HostState {
            status,
            address: self.addresses.get(host).cloned(),
        }
    }

    /// Takes the result of an [`Effect::List`].
    pub(crate) fn listed(
        &mut self,
        panel: PanelId,
        generation: u64,
        result: Result<Listed, String>,
    ) {
        self.sync_connected();
        let Some(tab) = self.tab_mut(panel) else {
            return;
        };
        let before = tab.panel.location().clone();
        tab.panel.listed(generation, result);
        let location = tab.panel.location().clone();
        if location != before {
            tab.previous = Some(before);
            // A new visit, which counts in zoxide again; a jump counted already.
            let arriving = tab.arriving.take();
            tab.noted =
                matches!(&location, Location::Local(path) if arriving.as_ref() == Some(path));
        }
        if let Location::Remote { host, path } = location
            && !path.as_bytes().is_empty()
        {
            self.last_dirs.insert(host, path);
        }
    }

    /// Takes the handle of a host that [`Effect::Connect`] connected, and sends the requests
    /// that waited for it.
    pub(crate) fn connected(
        &mut self,
        host: &str,
        connection: u64,
        handle: HostHandle,
    ) -> Vec<Effect> {
        let Some(state) = self.hosts.get_mut(host) else {
            return Vec::new();
        };
        if !matches!(state, Host::Connecting { connection: current, .. } if *current == connection)
        {
            return Vec::new();
        }
        let stop = match state {
            Host::Connecting { stop, .. } | Host::Connected { stop, .. } => stop.clone(),
        };
        *state = Host::Connected {
            connection,
            stop,
            handle,
        };
        let mut effects = Vec::new();
        for id in self.panel_ids() {
            // A tab that a workspace restored hidden asks once it shows.
            if let Some(tab) = self.tabs(id.side).get(id.tab)
                && tab.deferred.is_none()
                && let Some(request) = tab.panel.pending_request()
                && waits_for(&tab.panel, host)
            {
                effects.extend(self.route(id, request));
            }
        }
        effects
    }

    /// Takes the end of an [`Effect::Connect`]: a failed attempt, or a closed connection.
    /// `reason` is `None` if it was asked to stop.
    pub(crate) fn closed(
        &mut self,
        host: &str,
        connection: u64,
        reason: Option<&str>,
    ) -> Vec<Effect> {
        if self.hosts.get(host).map(Host::connection) != Some(connection) {
            return Vec::new();
        }
        let Some(state) = self.hosts.remove(host) else {
            return Vec::new();
        };
        if reason.is_some() {
            self.failed.insert(host.to_owned());
        }
        let mut effects = Vec::new();
        // Their tasks dropped the jobs on the host; its panels leave it, so there is nothing
        // to read.
        let lost: Vec<u64> = self
            .jobs
            .iter()
            .filter(|job| job.hosts.iter().any(|on| on == host))
            .map(|job| job.id)
            .collect();
        for id in lost {
            if let Some(Job {
                then: Some(then), ..
            }) = self.end_job(id)
            {
                effects.extend(self.follow_up(then, false));
            }
        }
        effects.extend(self.start_queued());
        for id in self.panel_ids() {
            let Some(panel) = self.panel_of_mut(id) else {
                continue;
            };
            match (&state, reason) {
                (Host::Connecting { .. }, Some(reason)) => {
                    if let Some(request) = panel.pending_request()
                        && waits_for(panel, host)
                    {
                        panel.listed(request.generation, Err(reason.to_owned()));
                    }
                }
                (Host::Connecting { .. }, None) => {
                    if waits_for(panel, host) {
                        panel.cancel();
                    }
                }
                (Host::Connected { .. }, _) => {
                    if let Some(request) = panel.leave_host(host, reason) {
                        effects.extend(self.route(id, request));
                    }
                }
            }
        }
        effects
    }

    /// Queues a dialog for a question from ssh.
    pub(crate) fn ask(&mut self, ask: Ask) {
        let dialog = Dialog::prompt(&ask.context, &ask.message, ask.kind);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Ssh {
                id: ask.id,
                reply: Some(ask.reply),
            },
        });
    }

    /// Queues a dialog for information from ssh.
    pub(crate) fn notice(&mut self, id: u64, context: &str, message: &str) {
        let dialog = Dialog::notice(context, message);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Ssh { id, reply: None },
        });
    }

    /// Closes the dialog of a prompt or notice that ssh no longer waits for.
    pub(crate) fn prompt_closed(&mut self, id: u64) {
        self.dialogs
            .retain(|open| !matches!(open.purpose, Purpose::Ssh { id: shown, .. } if shown == id));
    }

    /// Whether something on screen moves while time passes: a host that is connecting, or the
    /// time of a working job in its window or in the list of jobs.
    pub(crate) fn animates(&self) -> bool {
        let connecting = self
            .hosts
            .values()
            .any(|host| matches!(host, Host::Connecting { .. }));
        let timed = self
            .jobs
            .iter()
            .any(|job| job.view.ticking() && (!job.background || self.jobs_list.is_some()));
        connecting || timed
    }

    /// Moves spinners on by a frame.
    pub(crate) fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// Stops every job, for quitting; they clean up before the connections go.
    pub(crate) fn stop_jobs(&self) {
        for job in &self.jobs {
            job.cancel.cancel();
        }
    }

    /// Stops every connection and connection attempt, for quitting.
    pub(crate) fn disconnect_all(&mut self) {
        for (_, state) in self.hosts.drain() {
            state.stop();
        }
        self.close_viewer();
    }

    /// Two panels side by side above the F-key bar; the menu bar of F9 above them with
    /// `ui.menu_bar`, else over their top line while a menu is open.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, now: SystemTime, tz: &TimeZone) {
        self.spots = Spots::default();
        let always = self.config.ui.menu_bar == MenuBar::Always;
        let bar_height = u16::from(always);
        let screen = frame.area();
        let prompt = self.command_prompt(screen.width);
        let line_height = self
            .command_line
            .as_ref()
            .map_or(0, |line| line.height(&prompt, screen.width, screen.height));
        let [menu_bar, panels, command_line, key_bar] = Layout::vertical([
            Constraint::Length(bar_height),
            Constraint::Fill(1),
            Constraint::Length(line_height),
            Constraint::Length(1),
        ])
        .areas(screen);
        let menu_bar = Rect {
            height: 1.min(frame.area().height),
            ..menu_bar
        };
        let [(left_bar, left), (right_bar, right)] = self.panel_areas(panels);
        self.sync_connected();
        let active = self.active;
        let states: HashMap<String, HostState> = self
            .hosts
            .keys()
            .chain(&self.failed)
            .chain(self.addresses.keys())
            .map(|host| (host.clone(), self.host_state(host)))
            .collect();
        let hosts = |host: &str| states.get(host).cloned().unwrap_or_default();
        let view = View {
            hosts: &hosts,
            decor: self.decor,
            theme: &self.theme,
            root_title: &self.root_title,
            tick: self.tick,
            now,
            tz,
        };
        if let Some(viewing) = &mut self.viewing {
            let styles = ViewerStyles {
                header: self.theme.cursor,
                text: self.theme.panel,
            };
            viewing.viewer.render(frame, panels, &styles);
        } else {
            let sides = [
                (Side::Left, &mut self.left, left_bar, left),
                (Side::Right, &mut self.right, right_bar, right),
            ];
            for (side, tabs, line, area) in sides {
                let focused = active == side;
                let spots = render_side(frame, tabs, (line, area), focused, &view, &self.home);
                let whole = line.map_or(area, |line| line.union(area));
                self.spots.sides.push((side, whole));
                let tabs = spots.into_iter().map(|(index, spot)| (side, index, spot));
                self.spots.tabs.extend(tabs);
            }
        }
        self.spots.fkeys = self.render_fkeys(frame, key_bar);
        let focused = self.context() == Context::CommandLine;
        if let Some(line) = &mut self.command_line {
            line.render(frame, command_line, &prompt, Style::default(), focused);
        }
        if always {
            self.spots.menu_bar = pulldown::render_idle(frame, menu_bar, &self.theme);
        }
        if let Some(pulldown) = &self.pulldown {
            let screen = Rect {
                height: frame.area().height.saturating_sub(1),
                ..frame.area()
            };
            let icons = self.decor.icons();
            pulldown.render(frame, (menu_bar, screen), &self.theme, icons, &|command| {
                self.command_status(command)
            });
        }
        if self.viewing.is_none() {
            // On the menu bar, or on the top line of the panels without it.
            self.render_jobs(frame, menu_bar);
            if let Some(menu) = &mut self.menu {
                let area = match menu.side() {
                    Side::Left => left,
                    Side::Right => right,
                };
                menu.render(frame, area, &self.theme, self.decor, &hosts, self.tick);
            }
            if let Some(jump) = &mut self.jump {
                jump.render(frame, panels, &self.theme);
            }
        }
        self.render_windows(frame, panels);
    }

    /// The windows over `panels`, those in front last: the workspaces, the Configuration
    /// dialog, the help, the jobs, the checksums, and the dialogs.
    fn render_windows(&mut self, frame: &mut Frame<'_>, panels: Rect) {
        if let Some(window) = &mut self.workspaces_window {
            window.render(frame, panels, &self.theme);
        }
        if let Some(window) = &mut self.history_window {
            window.render(frame, panels, &self.theme);
        }
        if let Some(configuration) = &mut self.configuration {
            configuration.render(frame, panels, &self.theme);
        }
        if let Some(help) = &mut self.help {
            help.render(frame, panels, &self.theme);
        }
        if let Some(list) = &self.jobs_list {
            list.render(frame, panels, &self.theme, &self.job_rows());
        }
        if let Some(job) = self.in_front() {
            job.view.render(frame, panels, &self.theme, Instant::now());
        }
        if let Some(results) = self.results.front() {
            results.window.render(frame, panels, &self.theme);
        }
        if let Some(open) = self.dialogs.front() {
            open.dialog.render(frame, panels, &self.theme);
            if let Some(completing) = &mut self.completion
                && let Some(choices) = &mut completing.choices
                && let Some(field) = open.dialog.field_area()
            {
                choices.render(frame, field, panels, &self.theme);
            }
        }
    }

    /// Where the panels go in `area`, left side first, each with the line of its tabs if
    /// `ui.tab_bar` puts them on one. Both sides keep their panels level: a line of tabs above
    /// one goes above the other too.
    fn panel_areas(&self, area: Rect) -> [(Option<Rect>, Rect); 2] {
        let [mut left, mut right] = Layout::horizontal([Constraint::Fill(1); 2]).areas(area);
        if self.swapped {
            std::mem::swap(&mut left, &mut right);
        }
        let lines =
            self.config.ui.tab_bar == TabBar::Line && (self.left.len() > 1 || self.right.len() > 1);
        [left, right].map(|area| {
            if !lines || area.height < 2 {
                return (None, area);
            }
            let line = Rect { height: 1, ..area };
            let below = Rect {
                y: area.y + 1,
                height: area.height - 1,
                ..area
            };
            (Some(line), below)
        })
    }

    /// The jobs in the background at the top right of `area`, where Far has its clock: how
    /// many, and how far they are together.
    fn render_jobs(&self, frame: &mut Frame<'_>, area: Rect) {
        let behind: Vec<&Job> = self.jobs.iter().filter(|job| job.background).collect();
        if behind.is_empty() {
            return;
        }
        let ratio = behind.iter().map(|job| job.view.ratio()).sum::<f64>();
        // A few jobs, each at 0 … 1.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let percent = (ratio / behind.len() as f64 * 100.0).round() as u64;
        let text = format!(
            " {} ",
            fl!(
                "jobs-running",
                count = behind.len(),
                percent = percent.to_string()
            )
        );
        // Inside the corner of the right panel's frame.
        let room = usize::from(area.width.saturating_sub(2));
        let text = cells::fit(&text, cells::width(&text).min(room), cells::Align::Left);
        let width = u16::try_from(cells::width(&text)).unwrap_or(0);
        let x = area.right().saturating_sub(width + 1);
        let row = Rect::new(x, area.y, width, 1.min(area.height));
        frame.render_widget(Line::styled(text, self.theme.panel_title_active), row);
    }

    /// What F1 … F10 do now, each with its label on the F-key bar; `None` for a key that does
    /// nothing.
    fn fkey_actions(&self) -> [Option<(Action, String)>; 10] {
        let context = self.context();
        self.keymap.fkeys(context).map(|action| {
            let action = action.filter(|action| self.supports(*action))?;
            let label = match (context, action) {
                (Context::Workspaces, Action::Move) => Some(fl!("fkey-rename")),
                _ => fkey_label(action),
            }?;
            Some((action, label))
        })
    }

    /// The F-key bar: ten equal slots, each the key number and the label of its action.
    /// Returns the slots.
    fn render_fkeys(&self, frame: &mut Frame<'_>, area: Rect) -> Vec<Rect> {
        let slots = Layout::horizontal([Constraint::Fill(1); 10]).split(area);
        let actions = self.fkey_actions();
        for (number, (slot, action)) in (1..).zip(slots.iter().zip(actions)) {
            let label = action.map(|(_, label)| label).unwrap_or_default();
            let number = number.to_string();
            // The label's color fills its slot, as in mc.
            let room = usize::from(slot.width).saturating_sub(number.len());
            let line = Line::from(vec![
                Span::styled(number, self.theme.fkey_number),
                Span::styled(cells::fit(&label, room, Align::Left), self.theme.fkey_label),
            ]);
            frame.render_widget(line, *slot);
        }
        slots.to_vec()
    }
}

/// Draws the tab that shows of `tabs` in `area`, and its tabs on `line` if there is one, its
/// frame joined to it, else in its frame if it has more than one; `focused` if the side has the keys.
/// Returns where each tab shown is, by its index.
fn render_side(
    frame: &mut Frame<'_>,
    tabs: &mut Tabs,
    (line, area): (Option<Rect>, Rect),
    focused: bool,
    view: &View<'_>,
    home: &Path,
) -> Vec<(usize, Rect)> {
    tabs.active_mut().panel.render(frame, area, focused, view);
    let names = tabs.names(view.root_title, home, line.is_none());
    let bar = Bar {
        names: &names,
        active: tabs.index(),
        focused,
        theme: view.theme,
    };
    match line {
        Some(line) => {
            let spots = bar.render_line(frame, line);
            tabs::join_frame(frame, area, view.theme);
            spots
        }
        None if tabs.len() > 1 => {
            let spots = bar.render_frame(frame, area);
            tabs.active().panel.join_columns(frame, area, view.theme);
            spots
        }
        None => Vec::new(),
    }
}

/// What the viewer does for `action`, if anything.
fn viewer_command(action: Action) -> Option<ViewerCommand> {
    Some(match action {
        Action::Up => ViewerCommand::Up,
        Action::Down => ViewerCommand::Down,
        Action::PageUp => ViewerCommand::PageUp,
        Action::PageDown => ViewerCommand::PageDown,
        Action::Home => ViewerCommand::Home,
        Action::End => ViewerCommand::End,
        Action::Left => ViewerCommand::Left,
        Action::Right => ViewerCommand::Right,
        Action::ToggleWrap => ViewerCommand::ToggleWrap,
        _ => return None,
    })
}

/// The theme that `ui` picks, in the colors of `depth`. `ui.theme` was checked when the config
/// was loaded, and the dialog offers only the built-in themes.
fn theme_of(ui: &UiConfig, depth: ColorDepth) -> Theme {
    Theme::by_name(&ui.theme, depth)
        .unwrap_or_else(Theme::mc_classic)
        .with_borders(ui.borders)
}

/// What a job does after a failure, by the button `event` pressed.
fn decision_of(event: DialogEvent) -> Decision {
    match event {
        DialogEvent::Pressed(Button::Skip) => Decision::Skip,
        DialogEvent::Pressed(Button::SkipAll) => Decision::SkipAll,
        DialogEvent::Pressed(Button::Retry) => Decision::Retry,
        _ => Decision::Abort,
    }
}

/// What a copy does with a taken name, by the button `event` pressed.
fn conflict_of(event: DialogEvent) -> Conflict {
    match event {
        DialogEvent::Pressed(Button::Yes) => Conflict::Overwrite,
        DialogEvent::Pressed(Button::No) => Conflict::Skip,
        DialogEvent::Pressed(Button::All) => Conflict::OverwriteAll,
        DialogEvent::Pressed(Button::KeepAll) => Conflict::SkipAll,
        DialogEvent::Pressed(Button::Older) => Conflict::OverwriteOlder,
        _ => Conflict::Abort,
    }
}

/// Saves what the dialog of F4 on the host `name` holds, if it changed anything.
fn save_host(name: String, old: Option<&HostConfig>, dialog: &Dialog) -> Vec<Effect> {
    let text = |index: usize| {
        let text = dialog.text_of(index).trim();
        (!text.is_empty()).then(|| text.to_owned())
    };
    let host = HostConfig::Sftp(SftpHost {
        label: text(HOST_LABEL),
        start_dir: text(HOST_START_DIR),
        other_dir: text(HOST_OTHER_DIR),
        remember_dir: dialog.checked(0),
    });
    let unchanged = match old {
        Some(old) => *old == host,
        None => host.is_default(),
    };
    if unchanged {
        return Vec::new();
    }
    let host = (!host.is_default()).then_some(host);
    vec![Effect::SaveHost { name, host }]
}

/// The name of `algorithm`, as the checksum dialog and window show it.
fn algorithm_name(algorithm: Algorithm) -> String {
    match algorithm {
        Algorithm::Sha256 => fl!("checksum-sha256"),
        Algorithm::Sha512 => fl!("checksum-sha512"),
        Algorithm::Sha1 => fl!("checksum-sha1"),
        Algorithm::Md5 => fl!("checksum-md5"),
        Algorithm::Blake3 => fl!("checksum-blake3"),
    }
}

/// What files of checksums of `algorithm` end in, as `sha256sum` and `b3sum` users name them.
fn extension(algorithm: Algorithm) -> &'static str {
    match algorithm {
        Algorithm::Sha256 => "sha256",
        Algorithm::Sha512 => "sha512",
        Algorithm::Sha1 => "sha1",
        Algorithm::Md5 => "md5",
        Algorithm::Blake3 => "b3",
    }
}

/// `bytes` in lowercase hex.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// An expected checksum as typed or pasted, in lowercase hex, with the algorithm it is for:
/// `chosen` if its length fits, else the one whose length it has. Only the first word counts,
/// so a line that `sha256sum` printed works too. `None` if it is not hex of a known length.
fn parse_expected(text: &str, chosen: Algorithm) -> Option<(String, Algorithm)> {
    let word = text.split_whitespace().next()?.to_ascii_lowercase();
    if !word.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let length = word.len();
    let algorithm = if chosen.digest_len() * 2 == length {
        chosen
    } else {
        *Algorithm::ALL
            .iter()
            .find(|algorithm| algorithm.digest_len() * 2 == length)?
    };
    Some((word, algorithm))
}

/// The lines of a file of checksums, as `sha256sum` writes them: the checksum, two spaces, and
/// the name. A name with a backslash or a line break is escaped, and its line starts with a
/// backslash, as GNU coreutils does. Skipped files are left out.
fn sums_file(lines: &[(Vec<u8>, Option<String>)]) -> Vec<u8> {
    let mut text = Vec::new();
    for (name, hex) in lines {
        let Some(hex) = hex else { continue };
        let escape = name
            .iter()
            .any(|byte| matches!(byte, b'\\' | b'\n' | b'\r'));
        if escape {
            text.push(b'\\');
        }
        text.extend_from_slice(hex.as_bytes());
        text.extend_from_slice(b"  ");
        for &byte in name {
            match byte {
                b'\\' => text.extend_from_slice(b"\\\\"),
                b'\n' => text.extend_from_slice(b"\\n"),
                b'\r' => text.extend_from_slice(b"\\r"),
                byte => text.push(byte),
            }
        }
        text.push(b'\n');
    }
    text
}

/// Whether `inner` is `outer` or in it, on the same file system.
fn within(outer: &Location, inner: &Location) -> bool {
    match (outer, inner) {
        (Location::Local(outer), Location::Local(inner)) => inner.starts_with(outer),
        (
            Location::Remote {
                host: outer_host,
                path: outer,
            },
            Location::Remote {
                host: inner_host,
                path: inner,
            },
        ) => outer_host == inner_host && remote_within(outer, inner),
        _ => false,
    }
}

/// Whether the remote path `inner` is `outer` or in it, by their components.
fn remote_within(outer: &RemotePath, inner: &RemotePath) -> bool {
    let components = |path: &RemotePath| {
        path.as_bytes()
            .split(|&byte| byte == b'/')
            .filter(|part| !part.is_empty())
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>()
    };
    let absolute = |path: &RemotePath| path.as_bytes().starts_with(b"/");
    absolute(outer) == absolute(inner) && components(inner).starts_with(&components(outer))
}

/// The last component of a local or remote path.
fn file_name(location: &Location) -> Option<Vec<u8>> {
    match location {
        Location::Root | Location::Sftp => None,
        Location::Local(path) => path.file_name().map(|name| name.as_bytes().to_vec()),
        Location::Remote { path, .. } => path.file_name().map(<[u8]>::to_vec),
    }
}

/// Whether `panel` waits for a listing on `host`.
fn waits_for(panel: &Panel, host: &str) -> bool {
    panel.pending_request().is_some_and(
        |request| matches!(&request.location, Location::Remote { host: wanted, .. } if wanted == host),
    )
}

/// The label of an action in the F-key bar.
fn fkey_label(action: Action) -> Option<String> {
    match action {
        Action::Help => Some(fl!("fkey-help")),
        Action::Quit => Some(fl!("fkey-quit")),
        Action::Cancel => Some(fl!("fkey-cancel")),
        Action::Disconnect => Some(fl!("fkey-disconnect")),
        Action::EditHost => Some(fl!("fkey-edit-host")),
        Action::Mkdir => Some(fl!("fkey-mkdir")),
        Action::Delete => Some(fl!("fkey-delete")),
        Action::View => Some(fl!("fkey-view")),
        Action::Edit => Some(fl!("fkey-edit")),
        Action::ToggleWrap => Some(fl!("fkey-wrap")),
        Action::Copy => Some(fl!("fkey-copy")),
        Action::Move => Some(fl!("fkey-move")),
        Action::PullDown => Some(fl!("fkey-pulldown")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    use std::sync::mpsc;

    use noc_config::{TransferConfig, ZoxideConfig};
    use noc_ssh::askpass::PromptKind;
    use noc_vfs::{DirEntry, FileKind, Metadata, RemotePath};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use secrecy::ExposeSecret as _;

    use super::super::panel::Listing;
    use super::super::root::RootHost;
    use super::*;

    fn dir(name: &str) -> DirEntry {
        DirEntry {
            name: name.as_bytes().to_vec(),
            metadata: Metadata {
                kind: FileKind::Dir,
                size: Some(4096),
                permissions: Some(0o755),
                modified: Some(UNIX_EPOCH + Duration::from_secs(1_699_990_000)),
                uid: None,
                gid: None,
            },
            target_kind: None,
        }
    }

    fn file(name: &str, size: u64) -> DirEntry {
        let mut entry = dir(name);
        entry.metadata.kind = FileKind::File;
        entry.metadata.size = Some(size);
        entry
    }

    /// Settings with mc's markers in the interface, and without zoxide, which tests of its
    /// own turn on.
    fn config() -> Config {
        Config {
            ui: ui(),
            zoxide: ZoxideConfig {
                record: false,
                ..ZoxideConfig::default()
            },
            ..Config::default()
        }
    }

    /// Settings that record directories in zoxide.
    fn with_zoxide() -> Config {
        Config {
            zoxide: ZoxideConfig::default(),
            ..config()
        }
    }

    /// The directories `effects` add to zoxide.
    fn zoxide_adds(effects: &[Effect]) -> Vec<PathBuf> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::ZoxideAdd(dir) => Some(dir.clone()),
                _ => None,
            })
            .collect()
    }

    /// The effects but those for zoxide.
    fn without_zoxide(effects: Vec<Effect>) -> Vec<Effect> {
        effects
            .into_iter()
            .filter(|effect| !matches!(effect, Effect::ZoxideAdd(_)))
            .collect()
    }

    /// Settings with mc's markers, which read better in tests than icons.
    fn ui() -> UiConfig {
        UiConfig {
            icons: false,
            ..UiConfig::default()
        }
    }

    fn local(path: &str) -> Location {
        Location::Local(PathBuf::from(path))
    }

    fn remote(host: &str, path: &str) -> Location {
        Location::Remote {
            host: host.to_owned(),
            path: RemotePath::from(path),
        }
    }

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    /// The only effect.
    fn one(effects: Vec<Effect>) -> Effect {
        let [effect] = <[Effect; 1]>::try_from(effects).unwrap();
        effect
    }

    /// Answers every listing in `effects` with `listing`, from where it was asked for.
    fn answer(app: &mut App, effects: Vec<Effect>, listing: &Listing) {
        for effect in effects {
            let Effect::List { panel, request, .. } = effect else {
                panic!("expected a listing, got {effect:?}");
            };
            let location = request.location;
            let listing = listing.clone();
            app.listed(
                panel,
                request.generation,
                Ok(Listed {
                    location,
                    listing,
                    space: None,
                }),
            );
        }
    }

    /// An app on `/srv` whose first listings arrived.
    fn loaded() -> App {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("left"), dir("right")]),
        );
        app
    }

    /// An app with both panels on the list of hosts, `web` and `db`, which they reached from the
    /// virtual root.
    fn at_root() -> App {
        let (mut app, effects) = App::new(Path::new("/"), Path::new("/home/me"), &config());
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        let hosts = ["web", "db"].map(|alias| RootHost {
            alias: alias.to_owned(),
            label: None,
            address: None,
        });
        for side in Side::BOTH {
            app.active = side;
            let effects = app.handle(action(Action::Parent));
            let root = Listing::Root {
                volumes: Vec::new(),
                hosts: hosts.to_vec(),
            };
            answer(&mut app, effects, &root);
            app.handle(action(Action::End));
            let effects = app.handle(action(Action::Enter));
            answer(&mut app, effects, &Listing::Hosts(hosts.to_vec()));
        }
        app.active = Side::Left;
        app
    }

    /// Opens the host `rows` rows below `..` in the list of hosts in the panel on `side`.
    fn enter_host(app: &mut App, side: Side, rows: usize) -> Vec<Effect> {
        app.active = side;
        for _ in 0..rows {
            app.handle(action(Action::Down));
        }
        app.handle(action(Action::Enter))
    }

    /// Wide enough for whole status lines.
    fn screen(app: &mut App) -> String {
        screen_of(app, 8)
    }

    fn screen_of(app: &mut App, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(160, height)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, now, &TimeZone::UTC))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn lists_both_panels_at_start() {
        let (_, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let sides: Vec<Side> = effects
            .iter()
            .map(|effect| match effect {
                Effect::List {
                    panel, host: None, ..
                } => panel.side,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(sides, [Side::Left, Side::Right]);
    }

    #[test]
    fn keys_go_to_the_active_panel_and_tab_switches_it() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let Effect::List {
            panel: PanelId { side, .. },
            request,
            ..
        } = one(app.handle(action(Action::Enter)))
        else {
            panic!("expected a listing");
        };
        assert_eq!((side, request.location), (Side::Left, local("/srv/left")));

        assert!(app.handle(action(Action::SwitchPanel)).is_empty());
        app.handle(action(Action::End));
        let Effect::List {
            panel: PanelId { side, .. },
            request,
            ..
        } = one(app.handle(action(Action::Enter)))
        else {
            panic!("expected a listing");
        };
        assert_eq!((side, request.location), (Side::Right, local("/srv/right")));

        app.handle(action(Action::SwitchPanel));
        assert_eq!(app.active, Side::Left);
    }

    #[test]
    fn the_work_dir_follows_the_local_directory_of_the_active_panel() {
        let mut app = loaded();
        assert_eq!(app.take_work_dir(), Some(PathBuf::from("/srv")));
        assert_eq!(app.take_work_dir(), None, "only when it changes");

        app.handle(action(Action::Down));
        let effects = app.handle(action(Action::Enter));
        assert_eq!(app.take_work_dir(), None, "not before the listing arrives");
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        assert_eq!(app.take_work_dir(), Some(PathBuf::from("/srv/left")));

        app.handle(action(Action::SwitchPanel));
        assert_eq!(app.take_work_dir(), Some(PathBuf::from("/srv")));
        app.handle(action(Action::SwitchPanel));
        assert_eq!(app.take_work_dir(), Some(PathBuf::from("/srv/left")));

        let mut app = at_root();
        app.take_work_dir();
        let effects = enter_host(&mut app, Side::Left, 1);
        let Effect::Connect { connection, .. } = one(effects) else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        assert!(matches!(
            app.panel(Side::Left).location(),
            Location::Remote { .. }
        ));
        assert_eq!(
            app.take_work_dir(),
            None,
            "a remote panel keeps the last one"
        );
    }

    /// Presses the mouse at column `x` and row `y` of the screen last drawn.
    fn press(app: &mut App, press: Press, x: u16, y: u16) -> Vec<Effect> {
        let at = Position::new(x, y);
        app.pointer(Pointer { press, at })
    }

    fn cursor_name(app: &App, side: Side) -> String {
        let name = app.panel(side).name_under_cursor().unwrap_or_default();
        String::from_utf8_lossy(name).into_owned()
    }

    fn chosen_names(app: &App, side: Side) -> Vec<String> {
        let chosen = app.panel(side).chosen();
        let names = chosen.iter().map(|entry| &entry.name);
        names
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect()
    }

    // On a screen 160 wide and 12 high, the left panel takes columns 0 … 79 and the right one
    // 80 … 159; their rows start on line 2, under the frame and the header, and six fit.
    // The F-key bar is the last line, in slots 16 wide.

    #[test]
    fn a_click_moves_the_cursor_and_a_double_click_opens() {
        let mut app = loaded();
        screen_of(&mut app, 12);
        assert!(press(&mut app, Press::Click, 90, 3).is_empty());
        assert_eq!(app.active, Side::Right, "the clicked panel gets the keys");
        assert_eq!(cursor_name(&app, Side::Right), "left");
        let Effect::List { panel, request, .. } = one(press(&mut app, Press::DoubleClick, 90, 3))
        else {
            panic!("expected a listing");
        };
        assert_eq!(
            (panel.side, request.location),
            (Side::Right, local("/srv/left"))
        );

        // Below the last row the panel only gets the keys.
        app.active = Side::Left;
        press(&mut app, Press::Click, 90, 8);
        assert_eq!(app.active, Side::Right);
        assert_eq!(cursor_name(&app, Side::Right), "left");
        assert!(press(&mut app, Press::DoubleClick, 90, 8).is_empty());
    }

    #[test]
    fn a_right_click_marks_the_row_and_leaves_the_cursor_on_it() {
        let mut app = loaded();
        screen_of(&mut app, 12);
        press(&mut app, Press::RightClick, 10, 4);
        press(&mut app, Press::RightClick, 10, 3);
        assert_eq!(cursor_name(&app, Side::Left), "left");
        assert_eq!(chosen_names(&app, Side::Left), ["left", "right"]);
        press(&mut app, Press::RightClick, 10, 3);
        assert_eq!(cursor_name(&app, Side::Left), "left");
        assert_eq!(chosen_names(&app, Side::Left), ["right"]);
        press(&mut app, Press::RightClick, 10, 2);
        assert_eq!(
            chosen_names(&app, Side::Left),
            ["right"],
            "`..` takes no mark"
        );
    }

    #[test]
    fn the_wheel_scrolls_the_panel_under_it() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let files = (0..20).map(|n| file(&format!("f{n:02}"), 1)).collect();
        answer(&mut app, effects, &Listing::Dir(files));
        screen_of(&mut app, 12);
        press(&mut app, Press::WheelDown, 90, 5);
        assert_eq!(app.active, Side::Left, "the wheel does not take the keys");
        assert_eq!(
            cursor_name(&app, Side::Right),
            "f02",
            "on the first row shown"
        );
        assert_eq!(cursor_name(&app, Side::Left), "");
        screen_of(&mut app, 12);
        press(&mut app, Press::WheelUp, 90, 5);
        assert_eq!(cursor_name(&app, Side::Right), "f02", "still on screen");
        for _ in 0..10 {
            press(&mut app, Press::WheelDown, 90, 5);
        }
        assert_eq!(
            cursor_name(&app, Side::Right),
            "f14",
            "on the first row of the last page"
        );

        app.config.ui.wheel = Wheel::Page;
        screen_of(&mut app, 12);
        press(&mut app, Press::WheelUp, 90, 5);
        screen_of(&mut app, 12);
        assert_eq!(
            cursor_name(&app, Side::Right),
            "f13",
            "on the last row a page up"
        );
    }

    #[test]
    fn the_f_key_bar_presses_its_keys_where_they_do_something() {
        let mut app = loaded();
        screen_of(&mut app, 12);
        press(&mut app, Press::DoubleClick, 100, 11);
        assert!(app.dialogs.is_empty(), "a double click is no second press");
        press(&mut app, Press::Click, 100, 11);
        assert_eq!(app.context(), Context::PathInput, "F7 asks for a name");
        press(&mut app, Press::Click, 90, 3);
        assert_eq!(
            app.active,
            Side::Left,
            "a dialog keeps the panels from the mouse"
        );
        screen_of(&mut app, 12);
        // F10 cancels in a dialog.
        press(&mut app, Press::Click, 150, 11);
        assert!(app.dialogs.is_empty());

        app.config.ui.mouse = false;
        press(&mut app, Press::Click, 100, 11);
        press(&mut app, Press::Click, 90, 3);
        assert!(app.dialogs.is_empty());
        assert_eq!(app.active, Side::Left, "the mouse is off");
    }

    #[test]
    fn a_click_on_a_tab_shows_it() {
        for tab_bar in [TabBar::Line, TabBar::Frame] {
            let mut app = loaded();
            app.config.ui.tab_bar = tab_bar;
            app.handle(action(Action::NewTab));
            app.handle(action(Action::SwitchPanel));
            assert_eq!(app.tabs(Side::Left).index(), 1);
            screen_of(&mut app, 12);
            // ` 1 srv ` from column 1 of the line, or after the frame's corner.
            press(&mut app, Press::Click, 3, 0);
            assert_eq!(app.tabs(Side::Left).index(), 0, "{tab_bar:?}");
            assert_eq!(app.active, Side::Left);
        }
    }

    /// Draws `app` 160 by 12 and finds where `text` starts on the screen.
    fn find(app: &mut App, text: &str) -> (u16, u16) {
        let mut terminal = Terminal::new(TestBackend::new(160, 12)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, now, &TimeZone::UTC))
            .unwrap();
        let at = super::super::mouse::find(terminal.backend().buffer(), text);
        (at.x, at.y)
    }

    #[test]
    fn the_mouse_runs_commands_of_the_pull_down_menu() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let dirs = (0..20).map(|n| dir(&format!("d{n:02}"))).collect();
        answer(&mut app, effects, &Listing::Dir(dirs));
        let (x, y) = find(&mut app, "PullDn");
        press(&mut app, Press::Click, x, y);
        assert_eq!(app.context(), Context::PullDown);
        let (x, y) = find(&mut app, "Command");
        press(&mut app, Press::Click, x, y);
        let (x, y) = find(&mut app, "Swap panels");
        assert!(press(&mut app, Press::Click, x, y).is_empty());
        assert!(app.swapped);
        assert!(app.pulldown.is_none());
        // The second click of a double click lands on a directory, which it does not open.
        assert!(press(&mut app, Press::DoubleClick, x, y).is_empty());
        assert_eq!(cursor_name(&app, Side::Left), "");

        // A menu bar that stays opens a menu at a click on its title.
        app.config.ui.menu_bar = MenuBar::Always;
        let (x, y) = find(&mut app, "Options");
        press(&mut app, Press::Click, x, y);
        let (x, y) = find(&mut app, "Configuration");
        press(&mut app, Press::Click, x, y);
        assert!(app.configuration.is_some());
    }

    #[test]
    fn the_mouse_chooses_in_menus_and_presses_the_buttons_of_windows() {
        let mut app = loaded();
        open_menu(&mut app, Action::LocationMenuRight);
        let (x, y) = find(&mut app, "USB");
        press(&mut app, Press::Click, x, y);
        assert!(app.menu.is_some(), "a click puts the cursor on the row");
        let Effect::List { panel, request, .. } = one(press(&mut app, Press::DoubleClick, x, y))
        else {
            panic!("expected a listing");
        };
        assert_eq!(
            (panel.side, request.location),
            (Side::Right, local("/Volumes/USB"))
        );
        assert!(app.menu.is_none());
        open_menu(&mut app, Action::LocationMenuRight);
        screen_of(&mut app, 12);
        press(&mut app, Press::Click, 10, 3);
        assert!(app.menu.is_none(), "a click outside closes the menu");
        assert_eq!(app.active, Side::Right, "and does nothing else");

        // A job in front goes behind the panels, then the list of jobs aborts it.
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (_, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
        let (x, y) = find(&mut app, "Background");
        press(&mut app, Press::WheelDown, x, y);
        press(&mut app, Press::Click, x, y);
        assert!(app.in_front().is_none());
        app.handle(action(Action::Jobs));
        let (x, y) = find(&mut app, "Abort");
        press(&mut app, Press::Click, x, y);
        assert!(cancel.is_cancelled());

        // The wheel scrolls the help.
        app.handle(action(Action::Cancel));
        app.handle(action(Action::Help));
        let before = screen_of(&mut app, 12);
        press(&mut app, Press::WheelDown, 80, 5);
        assert_ne!(screen_of(&mut app, 12), before);
    }

    #[test]
    fn the_mouse_answers_dialogs() {
        let mut app = loaded();
        let (x, y) = find(&mut app, "Mkdir");
        press(&mut app, Press::Click, x, y);
        assert_eq!(app.dialogs.len(), 1);
        let (x, y) = find(&mut app, "Cancel");
        let (panel_x, panel_y) = (90, 3);
        press(&mut app, Press::Click, panel_x, panel_y);
        assert_eq!(
            app.active,
            Side::Left,
            "the dialog keeps the panels from the mouse"
        );
        press(&mut app, Press::Click, x, y);
        assert!(app.dialogs.is_empty());
    }

    /// The titles of the panels drawn on the left and on the right.
    fn titles(app: &mut App) -> (String, String) {
        let text = screen(app);
        let top = text.lines().next().unwrap();
        let (left, right) = top.split_once("╗╔").unwrap();
        (left.to_owned(), right.to_owned())
    }

    #[test]
    fn ctrl_u_swaps_where_the_panels_are_and_replies_follow_them() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let Effect::List {
            panel: PanelId { side, .. },
            request,
            ..
        } = one(app.handle(action(Action::Enter)))
        else {
            panic!("expected a listing");
        };
        assert!(app.handle(action(Action::SwapPanels)).is_empty());
        assert_eq!(app.active, Side::Left, "the active panel stays active");

        let location = request.location.clone();
        let listing = Listing::Dir(vec![dir("inner")]);
        app.listed(
            app.shown(side),
            request.generation,
            Ok(Listed {
                location,
                listing,
                space: None,
            }),
        );
        let (left, right) = titles(&mut app);
        assert!(
            right.contains("/srv/left"),
            "the panel that asked, now on the right"
        );
        assert!(!left.contains("/srv/left"), "{left}");

        app.handle(action(Action::SwapPanels));
        let (left, _) = titles(&mut app);
        assert!(left.contains("/srv/left"), "{left}");
    }

    #[test]
    fn alt_o_and_alt_i_send_the_other_panel_here() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let Effect::List { panel, request, .. } = one(app.handle(action(Action::OtherPanelOpen)))
        else {
            panic!("expected a listing");
        };
        assert_eq!(
            (panel.side, &request.location),
            (Side::Right, &local("/srv/left"))
        );
        assert_eq!(app.active, Side::Left);
        answer(
            &mut app,
            vec![Effect::List {
                panel,
                request,
                host: None,
            }],
            &Listing::Dir(Vec::new()),
        );

        let effects = app.handle(action(Action::OtherPanelSync));
        let [
            Effect::List {
                panel: PanelId { side, .. },
                request,
                ..
            },
        ] = &effects[..]
        else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!((*side, &request.location), (Side::Right, &local("/srv")));
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("left"), dir("right")]),
        );
        assert_eq!(
            app.panel(Side::Right).here(),
            app.panel(Side::Left).here(),
            "on the row Alt-O moved on to"
        );

        // A host opens in the other panel once it is connected.
        let mut app = at_root();
        app.handle(action(Action::Down));
        let Effect::Connect {
            host, connection, ..
        } = one(app.handle(action(Action::OtherPanelOpen)))
        else {
            panic!("expected a connection");
        };
        assert_eq!(host, "web");
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        assert!(matches!(
            &effects[..],
            [Effect::List {
                panel: PanelId {
                    side: Side::Right,
                    ..
                },
                ..
            }]
        ));
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            app.handle(Resolved::Insert(c));
        }
    }

    #[test]
    fn plus_and_minus_mark_and_unmark_by_pattern() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let entries = vec![
            file("a.md", 10),
            file("B.MD", 20),
            file("c.txt", 30),
            dir("docs.md"),
        ];
        answer(&mut app, effects, &Listing::Dir(entries));

        app.handle(action(Action::Select));
        assert_eq!(app.context(), Context::DialogInput);
        assert!(screen(&mut app).contains("Select"));
        type_text(&mut app, "*.md");
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::Panel);
        let text = screen(&mut app);
        assert!(text.contains(" 10 B in 2 files "), "case counts: {text}");

        // The dialog opens with the last pattern; Case sensitive is the second box.
        app.handle(action(Action::Select));
        for step in [
            Action::NextField,
            Action::Down,
            Action::Toggle,
            Action::Confirm,
        ] {
            app.handle(action(step));
        }
        assert!(screen(&mut app).contains(" 30 B in 3 files "));

        // `-` with Files only leaves the directory marked.
        app.handle(action(Action::Unselect));
        assert!(screen(&mut app).contains("Unselect"));
        type_text(&mut app, "*");
        for step in [Action::NextField, Action::Toggle, Action::Confirm] {
            app.handle(action(step));
        }
        assert!(screen(&mut app).contains(" 0 B in 1 file "));

        // Esc and an empty pattern change nothing.
        app.handle(action(Action::Select));
        type_text(&mut app, "c*");
        app.handle(action(Action::Cancel));
        app.handle(action(Action::Select));
        app.handle(action(Action::DeleteToStart));
        app.handle(action(Action::Confirm));
        assert!(screen(&mut app).contains(" 0 B in 1 file "));
        assert_eq!(app.pattern_options.pattern, "*");
    }

    /// The only effect, which must make a directory: where, and through which host.
    fn create_dir(effects: Vec<Effect>) -> (PanelId, Location, Option<HostHandle>) {
        match one(effects) {
            Effect::CreateDir {
                panel,
                location,
                host,
            } => (panel, location, host),
            other => panic!("expected a new directory, got {other:?}"),
        }
    }

    #[test]
    fn f7_makes_a_directory_and_puts_the_cursor_on_it() {
        let mut app = loaded();
        app.active = Side::Right;
        app.handle(action(Action::End));
        app.active = Side::Left;

        app.handle(action(Action::Mkdir));
        assert_eq!(app.context(), Context::PathInput);
        let text = screen(&mut app);
        assert!(text.contains("Create a new directory"), "{text}");
        assert!(text.contains("Enter directory name:"), "{text}");
        type_text(&mut app, "new");
        let (panel, location, host) = create_dir(app.handle(action(Action::Confirm)));
        assert_eq!(
            (panel.side, &location, host.is_none()),
            (Side::Left, &local("/srv/new"), true)
        );

        // Both panels show /srv and read it again; the one that asked lands on the directory.
        let effects = app.created(panel, &location, Ok(()));
        assert_eq!(effects.len(), 2);
        let listing = Listing::Dir(vec![dir("left"), dir("new"), dir("right")]);
        answer(&mut app, effects, &listing);
        assert_eq!(app.panel(Side::Left).name_under_cursor(), Some(&b"new"[..]));
        assert_eq!(
            app.panel(Side::Right).name_under_cursor(),
            Some(&b"right"[..]),
            "stays"
        );

        // The name under the cursor is filled in, and typing replaces it.
        app.handle(action(Action::Mkdir));
        type_text(&mut app, "x");
        let (_, location, _) = create_dir(app.handle(action(Action::Confirm)));
        assert_eq!(location, local("/srv/x"));
        app.handle(action(Action::Mkdir));
        app.handle(action(Action::End));
        type_text(&mut app, "2");
        let (_, location, _) = create_dir(app.handle(action(Action::Confirm)));
        assert_eq!(location, local("/srv/new2"));

        // Esc and an empty name make nothing.
        app.handle(action(Action::Mkdir));
        assert!(app.handle(action(Action::Cancel)).is_empty());
        app.handle(action(Action::Mkdir));
        app.handle(action(Action::DeleteToStart));
        assert!(app.handle(action(Action::Confirm)).is_empty());
    }

    #[test]
    fn a_directory_that_cannot_be_made_is_an_error_dialog() {
        let mut app = loaded();
        let effects = app.created(
            app.shown(Side::Left),
            &local("/srv/left"),
            Err("already exists".into()),
        );
        assert!(effects.is_empty());
        assert_eq!(app.context(), Context::Dialog);
        let text = screen(&mut app);
        assert!(text.contains("Error"), "{text}");
        assert!(
            text.contains("Cannot create directory /srv/left: already exists"),
            "{text}"
        );
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::Panel);

        // A directory elsewhere reads no panel again.
        assert!(
            app.created(app.shown(Side::Left), &local("/tmp/x"), Ok(()))
                .is_empty()
        );
    }

    #[test]
    fn f7_works_on_hosts_but_not_in_the_root() {
        let mut app = at_root();
        assert!(!screen(&mut app).contains("7Mkdir"));
        app.handle(action(Action::Mkdir));
        assert_eq!(app.context(), Context::Root, "nowhere to make it");

        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        let generation = request.generation;
        let location = remote("web", "/home/deploy");
        let listing = Listing::Dir(Vec::new());
        app.listed(
            app.shown(Side::Left),
            generation,
            Ok(Listed {
                location,
                listing,
                space: None,
            }),
        );
        assert!(screen(&mut app).contains("7Mkdir"));
        app.handle(action(Action::Mkdir));
        type_text(&mut app, "www");
        let (_, location, host) = create_dir(app.handle(action(Action::Confirm)));
        assert_eq!(location, remote("web", "/home/deploy/www"));
        assert!(host.is_some(), "through the host's task");
    }

    /// The only effect, which must start deleting: the job, its targets, and its host.
    fn delete_job(effects: Vec<Effect>) -> (u64, Vec<Location>, bool, CancellationToken) {
        match one(effects) {
            Effect::Delete {
                id,
                targets,
                host,
                cancel,
            } => (id, targets, host.is_some(), cancel),
            other => panic!("expected a delete job, got {other:?}"),
        }
    }

    #[test]
    fn f8_asks_deletes_and_reads_the_directory_again() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        assert_eq!(app.context(), Context::Dialog);
        let text = screen(&mut app);
        assert!(
            text.contains("Delete directory \"left\" and everything in it?"),
            "{text}"
        );
        assert!(
            text.contains("[< Yes >]"),
            "Yes is the default, as in mc: {text}"
        );
        let (id, targets, remote, _) = delete_job(app.handle(action(Action::Confirm)));
        assert_eq!((targets, remote), (vec![local("/srv/left")], false));

        assert_eq!(
            app.context(),
            Context::Dialog,
            "the job's window takes the keys"
        );
        app.job_event(id, JobEvent::Scanning { items: 3 });
        assert!(screen(&mut app).contains("3 found"));
        let current = local("/srv/left/a");
        let progress = JobEvent::Progress {
            current,
            done: 1,
            total: 3,
            bytes_done: 0,
            bytes_total: 0,
            bytes_copied: 0,
        };
        app.job_event(id, progress);
        let text = screen(&mut app);
        assert!(
            text.contains("/srv/left/a") && text.contains("1 of 3"),
            "{text}"
        );

        // A failure waits for an answer: Ignore all is next to Ignore.
        let (reply, mut decision) = oneshot::channel();
        let failed = JobEvent::Failed {
            path: local("/srv/left/b"),
            error: "permission denied".to_owned(),
            reply,
        };
        app.job_event(id, failed);
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot delete /srv/left/b: permission denied"),
            "{text}"
        );
        assert!(text.contains("[< Ignore >]"), "{text}");
        app.handle(action(Action::Right));
        app.handle(action(Action::Confirm));
        assert_eq!(decision.try_recv(), Ok(Decision::SkipAll));

        // Reports of other jobs change nothing; the end reads /srv again in both panels.
        assert!(
            app.job_event(id + 1, JobEvent::Finished { complete: true })
                .is_empty()
        );
        let effects = app.job_event(id, JobEvent::Finished { complete: true });
        assert_eq!(effects.len(), 2);
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn f8_names_what_it_deletes_and_no_keeps_it() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let entries = vec![file("a.txt", 1), file("b.txt", 2), dir("c")];
        answer(&mut app, effects, &Listing::Dir(entries));
        app.handle(action(Action::Delete));
        assert!(app.dialogs.is_empty(), "nothing to delete on `..`");

        app.handle(action(Action::End));
        app.handle(action(Action::Delete));
        assert!(screen(&mut app).contains("Delete file \"b.txt\"?"));
        app.handle(action(Action::Right));
        assert!(app.handle(action(Action::Confirm)).is_empty(), "No");

        app.handle(action(Action::InvertMarks));
        app.handle(action(Action::Delete));
        assert!(screen(&mut app).contains("Delete 2 files and directories?"));
        let (_, targets, _, _) = delete_job(app.handle(action(Action::Confirm)));
        assert_eq!(targets, [local("/srv/a.txt"), local("/srv/b.txt")]);
    }

    #[test]
    fn esc_aborts_a_job_and_its_questions_go_with_it() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (id, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
        app.handle(action(Action::Cancel));
        assert!(cancel.is_cancelled());
        assert!(screen(&mut app).contains("Aborting"));

        let (reply, mut decision) = oneshot::channel();
        let path = local("/srv/left/x");
        let error = "busy".to_owned();
        app.job_event(id, JobEvent::Failed { path, error, reply });
        app.job_event(id, JobEvent::Finished { complete: true });
        assert_eq!(
            app.context(),
            Context::Panel,
            "the question went with the job"
        );
        assert!(decision.try_recv().is_err());

        // Esc in a failure answers Abort.
        app.handle(action(Action::Delete));
        let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        let (reply, mut decision) = oneshot::channel();
        let path = local("/srv/left/y");
        let error = "busy".to_owned();
        app.job_event(id, JobEvent::Failed { path, error, reply });
        app.handle(action(Action::Cancel));
        assert_eq!(decision.try_recv(), Ok(Decision::Abort));
    }

    #[test]
    fn enter_sends_a_job_to_the_background_where_others_join_it() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (first, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
        app.handle(action(Action::Confirm));
        assert!(!cancel.is_cancelled());
        assert_eq!(app.context(), Context::Panel, "the panels take the keys");
        let progress = JobEvent::Progress {
            current: local("/srv/left/a"),
            done: 1,
            total: 4,
            bytes_done: 0,
            bytes_total: 0,
            bytes_copied: 0,
        };
        app.job_event(first, progress);
        let text = screen(&mut app);
        assert!(text.contains(" 1 job 25% ╗"), "{text}");

        // Another job starts in front, and goes behind too.
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (second, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        assert_eq!(app.context(), Context::Dialog);
        app.handle(action(Action::Confirm));
        assert!(screen(&mut app).contains(" 2 jobs 13% "));

        // A question from behind shows over the panels.
        let (reply, mut decision) = oneshot::channel();
        let path = local("/srv/right/x");
        let error = "busy".to_owned();
        app.job_event(second, JobEvent::Failed { path, error, reply });
        assert_eq!(app.context(), Context::Dialog);
        app.handle(action(Action::Confirm));
        assert_eq!(decision.try_recv(), Ok(Decision::Skip));

        // F10 asks while jobs run; No, the default, goes on.
        app.handle(action(Action::Quit));
        let text = screen_of(&mut app, 12);
        assert!(
            text.contains("2 jobs are still running. Quit and stop them?"),
            "{text}"
        );
        app.handle(action(Action::Confirm));
        assert!(!app.quits());

        // A job that ends reads its directory again.
        let effects = app.job_event(first, JobEvent::Finished { complete: true });
        assert_eq!(effects.len(), 2, "both panels show /srv");
        assert!(screen(&mut app).contains(" 1 job 0% "));
        app.job_event(second, JobEvent::Finished { complete: true });
        assert!(!screen(&mut app).contains(" job"));
        app.handle(action(Action::Quit));
        assert!(app.quits(), "nothing to ask");
    }

    #[test]
    fn the_clock_of_a_job_stands_while_its_question_is_open() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (first, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        // The first goes behind the panels, and the second stays in front.
        app.handle(action(Action::Confirm));
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (second, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        let ticking = |app: &App, id| {
            let job = app.jobs.iter().find(|job| job.id == id).unwrap();
            job.view.ticking()
        };
        for id in [first, second] {
            let progress = JobEvent::Progress {
                current: local("/srv/left/a"),
                done: 1,
                total: 4,
                bytes_done: 0,
                bytes_total: 0,
                bytes_copied: 0,
            };
            app.job_event(id, progress);
        }
        assert!(ticking(&app, first) && ticking(&app, second));
        assert!(app.animates(), "the time in front moves");

        // Both ask; the question of the second waits behind that of the first.
        let mut decisions = Vec::new();
        for id in [first, second] {
            let (reply, decision) = oneshot::channel();
            let path = local("/srv/left/b");
            let error = "busy".to_owned();
            app.job_event(id, JobEvent::Failed { path, error, reply });
            decisions.push(decision);
        }
        assert!(!ticking(&app, first) && !ticking(&app, second));
        assert!(!app.animates(), "nothing moves while they wait");

        app.handle(action(Action::Confirm));
        assert_eq!(decisions[0].try_recv(), Ok(Decision::Skip));
        assert!(ticking(&app, first), "the first goes on");
        assert!(
            !ticking(&app, second),
            "the second still waits for its answer"
        );
        app.handle(action(Action::Confirm));
        assert!(ticking(&app, second));

        // A taken name stops it as well.
        let (reply, _conflict) = oneshot::channel();
        let mut metadata = dir("a").metadata;
        metadata.kind = FileKind::File;
        let exists = JobEvent::Exists {
            target: local("/srv/right/a"),
            source_metadata: metadata.clone(),
            target_metadata: metadata,
            reply,
        };
        app.job_event(second, exists);
        assert!(!ticking(&app, second));
        app.handle(action(Action::Cancel));
        assert!(ticking(&app, second));
    }

    /// The delete jobs that `effects` start: their ids and targets.
    fn deletes(effects: &[Effect]) -> Vec<(u64, Vec<Location>)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Delete { id, targets, .. } => Some((*id, targets.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn jobs_wait_their_turn() {
        let mut app = loaded();
        app.parallel_jobs = 1;
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (first, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        app.handle(action(Action::Confirm));
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        assert!(app.handle(action(Action::Confirm)).is_empty(), "it waits");
        let text = screen_of(&mut app, 12);
        assert!(text.contains("Waiting for other jobs to finish"), "{text}");
        app.handle(action(Action::Confirm));
        assert!(screen(&mut app).contains(" 2 jobs 0% "));

        // A third waits too; Abort takes it away at once, as it has not started.
        app.handle(action(Action::Delete));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);
        assert_eq!(app.jobs.len(), 2);

        // When the first ends, the second starts.
        let effects = app.job_event(first, JobEvent::Finished { complete: true });
        let started = deletes(&effects);
        assert_eq!(started.len(), 1, "{effects:?}");
        let (second, targets) = &started[0];
        assert_eq!(targets, &[local("/srv/right")]);
        let effects = app.job_event(*second, JobEvent::Finished { complete: true });
        assert_eq!(deletes(&effects), [] as [(u64, Vec<Location>); 0]);
        assert!(app.jobs.is_empty());
    }

    #[test]
    fn ctrl_x_j_lists_the_jobs_to_show_or_abort() {
        let mut app = loaded();
        app.parallel_jobs = 1;
        app.handle(action(Action::Jobs));
        assert!(screen_of(&mut app, 12).contains("No jobs are running."));
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);

        // Two jobs behind the panels, the second waiting.
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (first, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
        app.handle(action(Action::Confirm));
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        app.handle(action(Action::Confirm));
        app.handle(action(Action::Confirm));
        let progress = JobEvent::Progress {
            current: local("/srv/left/a"),
            done: 1,
            total: 2,
            bytes_done: 0,
            bytes_total: 0,
            bytes_copied: 0,
        };
        app.job_event(first, progress);
        app.handle(action(Action::Jobs));
        assert_eq!(app.context(), Context::Dialog);
        let text = screen_of(&mut app, 12);
        assert!(
            text.contains("Delete") && text.contains("50%") && text.contains("/srv/left/a"),
            "{text}"
        );
        assert!(text.contains("waiting"), "{text}");

        // Abort takes the waiting one away at once, and stops the running one.
        app.handle(action(Action::Down));
        app.handle(action(Action::Right));
        app.handle(action(Action::Confirm));
        assert_eq!(app.jobs.len(), 1);
        app.handle(action(Action::Confirm));
        assert!(cancel.is_cancelled());
        assert!(screen_of(&mut app, 12).contains("aborting"));

        // Show brings it to the front, in place of the list.
        app.handle(action(Action::Left));
        app.handle(action(Action::Confirm));
        assert!(app.jobs_list.is_none());
        assert!(app.in_front().is_some());
        assert!(screen_of(&mut app, 12).contains("Aborting…"));
    }

    #[test]
    fn a_lost_host_makes_room_and_f4_never_waits() {
        let (mut app, connection) = on_a_host();
        app.parallel_jobs = 1;
        app.handle(action(Action::Delete));
        delete_job(app.handle(action(Action::Confirm)));
        app.handle(action(Action::Confirm));
        let (id, _, _, _, _) = copy_job(app.handle(action(Action::Edit)));
        app.job_event(id, JobEvent::Finished { complete: false });

        // A local job waits behind the one on the host, and starts when the host goes.
        let effects = enter_host(&mut app, Side::Right, 0);
        answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        app.handle(action(Action::Confirm));
        let effects = app.closed("web", connection, Some("Broken pipe"));
        assert_eq!(deletes(&effects).len(), 1, "{effects:?}");
        assert_eq!(app.jobs.len(), 1);
    }

    #[test]
    fn quitting_stops_the_jobs_and_yes_quits() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (_, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
        app.handle(action(Action::Confirm));
        app.handle(action(Action::Quit));
        assert!(screen_of(&mut app, 12).contains("A job is still running. Quit and stop it?"));
        app.handle(action(Action::Left));
        app.handle(action(Action::Confirm));
        assert!(app.quits());
        assert!(!cancel.is_cancelled(), "not before the loop stops them");
        app.stop_jobs();
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn a_job_on_a_lost_host_ends_with_it() {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (_, targets, through_host, _) = delete_job(app.handle(action(Action::Confirm)));
        assert_eq!(targets, [remote("web", "www")]);
        assert!(through_host, "through the host's task");

        let effects = app.closed("web", connection, Some("Broken pipe"));
        assert_eq!(effects.len(), 1, "back to the root");
        assert!(app.jobs.is_empty());
        assert_ne!(app.context(), Context::Dialog);
    }

    /// The only effect, which must start copying: the job, its sources, its target, whether
    /// its ends are remote, and its options.
    fn copy_job(effects: Vec<Effect>) -> (u64, Vec<Location>, Location, (bool, bool), CopyOptions) {
        match one(effects) {
            Effect::Copy {
                id,
                sources,
                target,
                hosts,
                options,
                ..
            } => {
                let remote = (hosts.0.is_some(), hosts.1.is_some());
                (id, sources, target, remote, options)
            }
            other => panic!("expected a copy job, got {other:?}"),
        }
    }

    /// An app on `/srv` with the right panel in `/srv/right` and the cursor of the left one
    /// on `left`.
    fn two_directories() -> App {
        let mut app = loaded();
        app.active = Side::Right;
        app.handle(action(Action::End));
        let effects = app.handle(action(Action::Enter));
        answer(&mut app, effects, &Listing::Dir(vec![file("old", 1)]));
        app.active = Side::Left;
        app.handle(action(Action::Down));
        app
    }

    #[test]
    fn f5_copies_to_the_other_panel_and_asks_about_taken_names() {
        let mut app = two_directories();
        app.handle(action(Action::Copy));
        let text = screen(&mut app);
        assert!(text.contains("Copy \"left\" to:"), "{text}");
        assert!(text.contains("/srv/right"), "{text}");
        assert!(text.contains("[x] Preserve attributes"), "{text}");
        let (id, sources, target, remote, options) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(sources, [local("/srv/left")]);
        assert_eq!((target, remote), (local("/srv/right"), (false, false)));
        assert_eq!(
            options,
            CopyOptions {
                preserve: true,
                atomic: true,
                remove_sources: false,
                overwrite: false,
            }
        );
        assert!(screen(&mut app).contains("Counting"), "the job's window");

        // A taken name: No is the default; Left goes to Yes.
        let mut metadata = dir("left").metadata;
        metadata.kind = FileKind::File;
        metadata.size = Some(12_345);
        let ask = |app: &mut App| {
            let (reply, answer) = oneshot::channel();
            let event = JobEvent::Exists {
                target: local("/srv/right/left/a"),
                source_metadata: metadata.clone(),
                target_metadata: metadata.clone(),
                reply,
            };
            app.job_event(id, event);
            answer
        };
        let mut answer = ask(&mut app);
        let text = screen_of(&mut app, 14);
        assert!(
            text.contains("/srv/right/left/a is there already."),
            "{text}"
        );
        assert!(text.contains("New:      12,345 bytes"), "{text}");
        assert!(text.contains("[< No >]"), "{text}");
        app.handle(action(Action::Confirm));
        assert_eq!(answer.try_recv(), Ok(Conflict::Skip));
        let mut answer = ask(&mut app);
        app.handle(action(Action::Left));
        app.handle(action(Action::Confirm));
        assert_eq!(answer.try_recv(), Ok(Conflict::Overwrite));

        // The end reads the target and the source's directory again.
        let effects = app.job_event(id, JobEvent::Finished { complete: true });
        assert_eq!(effects.len(), 2);
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn f5_takes_typed_targets_and_remembers_preserve() {
        let mut app = two_directories();
        let (handle, _requests) = HostHandle::channel();
        let stop = CancellationToken::new();
        let web = Host::Connected {
            connection: 1,
            stop,
            handle,
        };
        app.hosts.insert("web".to_owned(), web);
        let copy_to = |app: &mut App, text: &str| {
            app.handle(action(Action::Copy));
            type_text(app, text);
            app.handle(action(Action::Confirm))
        };
        let (_, _, target, _, _) = copy_job(copy_to(&mut app, "sub"));
        assert_eq!(target, local("/srv/sub"), "from the source's directory");
        app.jobs.clear();
        let (_, _, target, ends, _) = copy_job(copy_to(&mut app, "web:/var/www"));
        assert_eq!((target, ends), (remote("web", "/var/www"), (false, true)));
        app.jobs.clear();
        let (_, _, target, _, _) = copy_job(copy_to(&mut app, "x:y"));
        assert_eq!(target, local("/srv/x:y"), "no host called x");
        app.jobs.clear();

        // Preserve attributes, switched off, stays off.
        app.handle(action(Action::Copy));
        for step in [Action::NextField, Action::Toggle, Action::Confirm] {
            app.handle(action(step));
        }
        app.jobs.clear();
        app.handle(action(Action::Copy));
        assert!(screen(&mut app).contains("[ ] Preserve attributes"));
        let (_, _, _, _, options) = copy_job(app.handle(action(Action::Confirm)));
        assert!(!options.preserve);
    }

    #[test]
    fn copies_write_directly_without_atomic_upload() {
        let transfer = TransferConfig {
            atomic_upload: false,
            ..TransferConfig::default()
        };
        let (mut app, effects) = App::new(
            Path::new("/srv"),
            Path::new("/home/me"),
            &Config {
                transfer,
                ..config()
            },
        );
        answer(&mut app, effects, &Listing::Dir(vec![dir("left")]));
        app.handle(action(Action::Down));
        app.handle(action(Action::Copy));
        type_text(&mut app, "/tmp");
        let (_, _, target, _, options) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(target, local("/tmp"));
        assert!(!options.atomic);
    }

    #[test]
    fn f6_moves_to_the_other_panel_or_renames_in_place() {
        let mut app = two_directories();
        app.handle(action(Action::Move));
        let text = screen(&mut app);
        assert!(text.contains("Move \"left\" to:"), "{text}");
        assert!(text.contains("/srv/right"), "{text}");
        assert!(!text.contains("Preserve"), "moves always preserve: {text}");
        let (id, sources, target, _, options) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(
            (sources, target),
            (vec![local("/srv/left")], local("/srv/right"))
        );
        assert!(options.remove_sources && options.preserve);
        assert!(screen(&mut app).contains("Move"));
        let effects = app.job_event(id, JobEvent::Finished { complete: true });
        assert_eq!(effects.len(), 2, "both panels: the source changed too");

        // A new name renames in place.
        app.handle(action(Action::Move));
        type_text(&mut app, "renamed");
        let (_, _, target, _, options) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(target, local("/srv/renamed"));
        assert!(options.remove_sources);
        app.jobs.clear();

        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Move));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot move to /srv: the source and the target are the same"),
            "{text}"
        );
    }

    #[test]
    fn f5_does_not_copy_onto_or_into_itself() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Copy));
        assert!(
            app.handle(action(Action::Confirm)).is_empty(),
            "the other panel is here too"
        );
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot copy to /srv: the source and the target are the same"),
            "{text}"
        );
        app.handle(action(Action::Confirm));

        app.handle(action(Action::Copy));
        type_text(&mut app, "left/inner");
        assert!(app.handle(action(Action::Confirm)).is_empty());
        let text = screen(&mut app);
        assert!(text.contains("it is in /srv/left"), "{text}");
        assert!(remote_within(
            &RemotePath::from("/a"),
            &RemotePath::from("/a/b/")
        ));
        assert!(!remote_within(
            &RemotePath::from("/a"),
            &RemotePath::from("/ab")
        ));
        assert!(!remote_within(
            &RemotePath::from("a"),
            &RemotePath::from("/a")
        ));
    }

    #[test]
    fn a_copy_to_a_lost_host_ends_with_it() {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 1)) else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        let generation = request.generation;
        let location = remote("web", "/home/deploy");
        let listing = Listing::Dir(Vec::new());
        app.listed(
            app.shown(Side::Right),
            generation,
            Ok(Listed {
                location,
                listing,
                space: None,
            }),
        );

        app.active = Side::Left;
        app.handle(action(Action::Home));
        let effects = app.handle(action(Action::Enter));
        answer(&mut app, effects, &Listing::Dir(vec![file("notes", 5)]));
        app.handle(action(Action::Down));
        app.handle(action(Action::Copy));
        assert!(screen(&mut app).contains("web:/home/deploy"));
        let (_, _, target, ends, _) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(
            (target, ends),
            (remote("web", "/home/deploy"), (false, true))
        );

        app.closed("web", connection, Some("Broken pipe"));
        assert!(app.jobs.is_empty());
    }

    #[test]
    fn f3_views_files_and_opens_directories() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
        );
        app.handle(action(Action::End));
        let Effect::Read {
            id,
            location,
            host,
            cancel,
        } = one(app.handle(action(Action::View)))
        else {
            panic!("expected a read");
        };
        assert_eq!((location, host.is_none()), (local("/srv/notes"), true));
        assert_eq!(app.context(), Context::Viewer);
        assert!(screen(&mut app).contains("Loading"));
        app.read(id + 1, Ok((b"not this".to_vec(), false)));
        app.read(id, Ok((b"hello\nworld\n".to_vec(), false)));
        let text = screen(&mut app);
        assert!(
            text.contains("/srv/notes") && text.contains("hello"),
            "{text}"
        );
        assert!(text.contains("2Wrap") && text.contains("10Quit"), "{text}");

        // F1 shows the help over it, and Esc goes back to it.
        app.handle(action(Action::Help));
        assert_eq!(app.context(), Context::Dialog);
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Viewer);
        app.handle(action(Action::Quit));
        assert_eq!(app.context(), Context::Panel);
        assert!(cancel.is_cancelled(), "a read in flight stops");
        assert!(!app.quits(), "Quit closes the viewer, not the app");

        // On a directory, F3 opens it.
        app.handle(action(Action::Home));
        app.handle(action(Action::Down));
        let Effect::List { request, .. } = one(app.handle(action(Action::View))) else {
            panic!("expected a listing");
        };
        assert_eq!(request.location, local("/srv/sub"));
    }

    #[test]
    fn f4_edits_local_files_where_they_are() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
        );
        app.handle(action(Action::Down));
        assert!(
            app.handle(action(Action::Edit)).is_empty(),
            "not a directory"
        );
        assert_eq!(app.take_edit(), None);
        app.handle(action(Action::End));
        assert!(app.handle(action(Action::Edit)).is_empty());
        assert_eq!(app.take_edit(), Some(PathBuf::from("/srv/notes")));
        assert_eq!(app.take_edit(), None, "once");
        assert_eq!(app.edited(Ok(true)).len(), 2, "both panels read /srv again");

        app.handle(action(Action::Edit));
        app.take_edit();
        app.edited(Err("cannot run nano: not found".to_owned()));
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot edit /srv/notes: cannot run nano: not found"),
            "{text}"
        );
    }

    /// An app whose left panel is on `web:/home/deploy`, which holds `notes`.
    fn on_a_host() -> (App, u64) {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        let generation = request.generation;
        let location = remote("web", "/home/deploy");
        let listing = Listing::Dir(vec![file("notes", 12)]);
        app.listed(
            app.shown(Side::Left),
            generation,
            Ok(Listed {
                location,
                listing,
                space: None,
            }),
        );
        app.set_runtime_dir(PathBuf::from("/run/noc"));
        app.handle(action(Action::Down));
        (app, connection)
    }

    #[test]
    fn f4_edits_a_copy_of_a_remote_file_and_sends_it_back() {
        let (mut app, _) = on_a_host();
        let (id, sources, target, ends, options) = copy_job(app.handle(action(Action::Edit)));
        assert_eq!(sources, [remote("web", "/home/deploy/notes")]);
        let Location::Local(copy) = target else {
            panic!("expected a local copy, got {target:?}");
        };
        assert!(copy.starts_with("/run/noc"), "{copy:?}");
        assert!(
            copy.to_string_lossy().ends_with("-notes"),
            "its own name last"
        );
        assert_eq!(ends, (true, false));
        assert!(options.overwrite && options.preserve);
        assert_eq!(app.take_edit(), None, "not before the copy is there");
        let text = screen_of(&mut app, 12);
        assert!(
            !text.contains("Background"),
            "the editor would open later: {text}"
        );

        app.job_event(id, JobEvent::Finished { complete: true });
        assert_eq!(app.take_edit(), Some(copy.clone()));
        let (id, sources, target, ends, options) = copy_job(app.edited(Ok(true)));
        assert_eq!(
            (sources, target),
            (
                vec![Location::Local(copy.clone())],
                remote("web", "/home/deploy/notes")
            )
        );
        assert_eq!(ends, (false, true));
        assert!(options.overwrite, "it goes back over the original");
        let effects = app.job_event(id, JobEvent::Finished { complete: true });
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Discard(path) if *path == copy)),
            "{effects:?}"
        );
    }

    #[test]
    fn an_unchanged_copy_goes_and_one_that_cannot_go_back_stays() {
        let (mut app, connection) = on_a_host();
        let (id, _, target, _, _) = copy_job(app.handle(action(Action::Edit)));
        app.job_event(id, JobEvent::Finished { complete: true });
        let copy = app.take_edit().unwrap();
        assert_eq!(Location::Local(copy.clone()), target);
        let effects = app.edited(Ok(false));
        assert!(matches!(&effects[..], [Effect::Discard(path)] if *path == copy));

        // The upload fails: the copy stays, and the user hears where it is.
        let (id, _, _, _, _) = copy_job(app.handle(action(Action::Edit)));
        app.job_event(id, JobEvent::Finished { complete: true });
        let copy = app.take_edit().unwrap();
        let (id, _, _, _, _) = copy_job(app.edited(Ok(true)));
        let effects = app.job_event(id, JobEvent::Finished { complete: false });
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Discard(_)))
        );
        let text = screen_of(&mut app, 12);
        assert!(
            text.contains("The changes to web:/home/deploy/notes did not go back"),
            "{text}"
        );
        assert!(text.contains(&*copy.to_string_lossy()), "{text}");
        app.handle(action(Action::Confirm));

        // So does one whose host is gone by then.
        let (id, _, _, _, _) = copy_job(app.handle(action(Action::Edit)));
        app.job_event(id, JobEvent::Finished { complete: true });
        app.take_edit().unwrap();
        app.closed("web", connection, Some("Broken pipe"));
        assert!(app.edited(Ok(true)).is_empty());
        assert!(screen_of(&mut app, 12).contains("did not go back"));
    }

    #[test]
    fn a_host_that_goes_ends_the_jobs_of_f4() {
        // While the copy comes: the partial copy goes, and there is nothing to edit.
        let (mut app, connection) = on_a_host();
        let (_, _, target, _, _) = copy_job(app.handle(action(Action::Edit)));
        let effects = app.closed("web", connection, Some("Broken pipe"));
        let Location::Local(copy) = target else {
            panic!("expected a local copy");
        };
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Discard(path) if *path == copy)),
            "{effects:?}"
        );
        assert!(app.jobs.is_empty());
        assert_eq!(app.take_edit(), None);

        // While the edited copy goes back: it stays, and the user hears where.
        let (mut app, connection) = on_a_host();
        let (id, _, _, _, _) = copy_job(app.handle(action(Action::Edit)));
        app.job_event(id, JobEvent::Finished { complete: true });
        let copy = app.take_edit().unwrap();
        copy_job(app.edited(Ok(true)));
        let effects = app.closed("web", connection, Some("Broken pipe"));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Discard(_)))
        );
        let text = screen_of(&mut app, 12);
        assert!(text.contains("did not go back"), "{text}");
        assert!(text.contains(&*copy.to_string_lossy()), "{text}");
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_error() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        answer(&mut app, effects, &Listing::Dir(vec![file("secret", 1)]));
        app.handle(action(Action::Down));
        let Effect::Read { id, .. } = one(app.handle(action(Action::View))) else {
            panic!("expected a read");
        };
        app.read(id, Err("permission denied".to_owned()));
        assert_eq!(app.context(), Context::Dialog);
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot view /srv/secret: permission denied"),
            "{text}"
        );
    }

    #[test]
    fn pattern_dialogs_wait_for_no_prompt_and_skip_the_root() {
        let mut app = at_root();
        app.handle(action(Action::Select));
        assert_eq!(app.context(), Context::Root, "no names to match");

        let mut app = loaded();
        app.handle(action(Action::Select));
        let (password, answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
        app.ask(password);
        app.prompt_closed(1);
        assert_eq!(app.context(), Context::DialogInput);
        assert!(
            screen(&mut app).contains("Select"),
            "the app's own dialog stays"
        );
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);
        assert!(answer.try_recv().is_err(), "the prompt went with ssh");
    }

    #[test]
    fn replies_reach_their_own_panel() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let Effect::List {
            panel: PanelId { side, .. },
            request,
            ..
        } = one(app.handle(action(Action::Enter)))
        else {
            panic!("expected a listing");
        };
        let reply = |listing| {
            let location = request.location.clone();
            Ok(Listed {
                location,
                listing,
                space: None,
            })
        };
        app.listed(
            app.shown(side.other()),
            request.generation,
            reply(Listing::Dir(Vec::new())),
        );
        app.listed(
            app.shown(side),
            request.generation,
            reply(Listing::Dir(vec![dir("deeper")])),
        );
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, now, &TimeZone::UTC))
            .unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn a_host_connects_once_and_then_lists_for_every_panel_that_waits() {
        let mut app = at_root();
        let Effect::Connect {
            host, connection, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        assert_eq!(host, "web");
        assert!(
            enter_host(&mut app, Side::Right, 1).is_empty(),
            "already on its way"
        );
        let text = screen(&mut app);
        assert_eq!(text.matches("Connecting to web…").count(), 2, "{text}");

        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        let sides: Vec<_> = effects
            .iter()
            .map(|effect| match effect {
                Effect::List {
                    panel: PanelId { side, .. },
                    request,
                    host: Some(_),
                } => {
                    assert_eq!(request.location, remote("web", ""));
                    *side
                }
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(sides, Side::BOTH);
        assert!(!screen(&mut app).contains("Connecting"));

        // A connected host lists at once.
        let effects = enter_host(&mut app, Side::Left, 0);
        assert!(matches!(&effects[..], [Effect::List { host: Some(_), .. }]));
    }

    fn with_host(app: &mut App, name: &str, host: SftpHost) {
        let mut hosts = Hosts::default();
        hosts.hosts.insert(name.to_owned(), HostConfig::Sftp(host));
        app.set_hosts(Arc::new(hosts));
    }

    /// Connects `web` for the panel on `side`, which waits for it, and answers its listing
    /// from `path`.
    fn connect_web(app: &mut App, side: Side, connection: u64, path: &str) -> ListRequest {
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected one listing, got {effects:?}");
        };
        let request = request.clone();
        let listed = Listed {
            location: remote("web", path),
            listing: Listing::Dir(vec![dir("app")]),
            space: None,
        };
        app.listed(app.shown(side), request.generation, Ok(listed));
        request
    }

    #[test]
    fn opening_a_host_sends_the_other_panel_to_its_other_dir_once() {
        let mut app = at_root();
        let site = SftpHost {
            other_dir: Some("~/site".to_owned()),
            ..SftpHost::default()
        };
        with_host(&mut app, "web", site);
        let effects = enter_host(&mut app, Side::Left, 1);
        let [
            Effect::Connect { connection, .. },
            Effect::List {
                panel: PanelId {
                    side: Side::Right, ..
                },
                request,
                host: None,
            },
        ] = &effects[..]
        else {
            panic!("expected a connection and the other directory, got {effects:?}");
        };
        assert_eq!(request.location, local("/home/me/site"));
        let request = connect_web(&mut app, Side::Left, *connection, "/home/web");
        assert_eq!(request.location, remote("web", ""));
        assert_eq!(
            app.panel(Side::Right).pending_request().map(|r| r.location),
            Some(local("/home/me/site")),
            "connecting did not send it again"
        );

        // Other hosts leave the other panel alone.
        let effects = enter_host(&mut app, Side::Right, 2);
        assert!(
            matches!(&effects[..], [Effect::Connect { .. }]),
            "{effects:?}"
        );
    }

    #[test]
    fn bang_runs_a_command_on_the_host_of_the_panel() {
        let mut app = at_root();
        let site = SftpHost {
            label: Some("Site".to_owned()),
            ..SftpHost::default()
        };
        with_host(&mut app, "web", site);
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        connect_web(&mut app, Side::Left, connection, "/var/www");
        app.handle(action(Action::Shell));
        type_text(&mut app, "ls");
        let text = screen_of(&mut app, 10);
        assert!(text.contains("Site:/var/www $ ls"), "{text}");
        app.handle(action(Action::Confirm));
        let Some(Run {
            place: Place::Remote { dir, .. },
            command,
        }) = app.take_run()
        else {
            panic!("expected a remote command");
        };
        assert_eq!((dir.as_bytes(), command.as_str()), (&b"/var/www"[..], "ls"));
        let effects = app.ran(Ok(()));
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::List { request, host: Some(_), .. } if request.location == remote("web", "/var/www")
            )),
            "{effects:?}"
        );
    }

    #[test]
    fn remember_dir_resumes_the_last_directory_of_the_session() {
        for remember_dir in [true, false] {
            let mut app = at_root();
            let host = SftpHost {
                remember_dir,
                ..SftpHost::default()
            };
            with_host(&mut app, "web", host);
            let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1))
            else {
                panic!("expected a connection");
            };
            let first = connect_web(&mut app, Side::Left, connection, "/var/www");
            assert_eq!(first.resume, None);
            let effects = app.handle(action(Action::Enter));
            let [Effect::List { request, .. }] = &effects[..] else {
                panic!("expected a listing, got {effects:?}");
            };
            let listed = Listed {
                location: remote("web", "/var/www/app"),
                listing: Listing::Dir(Vec::new()),
                space: None,
            };
            app.listed(app.shown(Side::Left), request.generation, Ok(listed));

            let effects = app.disconnect("web");
            answer(&mut app, effects, &Listing::Hosts(Vec::new()));
            let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 1))
            else {
                panic!("expected a connection");
            };
            let again = connect_web(&mut app, Side::Right, connection, "/var/www/app");
            let expected = remember_dir.then(|| RemotePath::from("/var/www/app"));
            assert_eq!(again.resume, expected, "remember_dir = {remember_dir}");
        }
    }

    /// Opens the dialog of F4 on `web` from the list of hosts in the left panel.
    fn edit_web(app: &mut App) {
        app.active = Side::Left;
        app.handle(action(Action::Home));
        app.handle(action(Action::Down));
        assert!(app.supports(Action::EditHost));
        assert!(app.handle(action(Action::EditHost)).is_empty());
        assert_eq!(app.context(), Context::DialogInput);
    }

    fn typed(app: &mut App, text: &str) {
        for c in text.chars() {
            app.handle(Resolved::Insert(c));
        }
    }

    #[test]
    fn f4_on_a_host_saves_its_settings() {
        let mut app = at_root();
        assert!(!screen(&mut app).contains("4Edit"), "not on `..`");
        app.handle(action(Action::Down));
        assert!(screen(&mut app).contains("4Edit"));
        edit_web(&mut app);
        assert!(screen_of(&mut app, 20).contains("Host web"));
        typed(&mut app, "Prod");
        let Effect::SaveHost { name, host } = one(app.handle(action(Action::Confirm))) else {
            panic!("expected the settings to be saved");
        };
        let prod = HostConfig::Sftp(SftpHost {
            label: Some("Prod".to_owned()),
            ..SftpHost::default()
        });
        assert_eq!((name.as_str(), host.as_ref()), ("web", Some(&prod)));

        let mut hosts = Hosts::default();
        hosts.hosts.insert("web".to_owned(), prod.clone());
        let effects = app.host_saved(Ok(Arc::new(hosts)));
        assert_eq!(effects.len(), 2, "both lists of hosts are read again");

        // The dialog opens with the settings; unchanged, nothing is saved, and emptied, they go.
        edit_web(&mut app);
        assert!(app.handle(action(Action::Confirm)).is_empty());
        edit_web(&mut app);
        app.handle(action(Action::DeleteToStart));
        let Effect::SaveHost { host, .. } = one(app.handle(action(Action::Confirm))) else {
            panic!("expected the settings to be removed");
        };
        assert_eq!(host, None);

        app.host_saved(Err("cannot write /cfg/hosts.toml: denied".to_owned()));
        let text = screen_of(&mut app, 20);
        assert!(text.contains("Cannot save the host settings"), "{text}");
    }

    #[test]
    fn f4_on_a_host_rejects_another_kind_of_other_dir() {
        let mut app = at_root();
        edit_web(&mut app);
        app.handle(action(Action::Down));
        app.handle(action(Action::Down));
        typed(&mut app, "web:/srv");
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert_eq!(app.dialogs.len(), 2, "the reason shows over the dialog");
        app.handle(action(Action::Confirm));
        app.handle(action(Action::DeleteToStart));
        typed(&mut app, "~/site");
        let Effect::SaveHost { host, .. } = one(app.handle(action(Action::Confirm))) else {
            panic!("expected the settings to be saved");
        };
        assert_eq!(
            host.as_ref().and_then(HostConfig::other_dir),
            Some("~/site")
        );
    }

    #[test]
    fn use_current_fills_in_the_directory_open_on_the_host() {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 1)) else {
            panic!("expected a connection");
        };
        connect_web(&mut app, Side::Right, connection, "/var/www");
        edit_web(&mut app);
        // Past the three fields and the check box, then OK, to Use Current.
        for _ in 0..5 {
            app.handle(action(Action::NextField));
        }
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert_eq!(app.dialogs.len(), 1, "the dialog stays");
        let Effect::SaveHost { host, .. } = one(app.handle(action(Action::Confirm))) else {
            panic!("expected the settings to be saved");
        };
        let start_dir = host.as_ref().and_then(HostConfig::start_dir);
        assert_eq!(start_dir, Some("/var/www"));
    }

    #[test]
    fn a_failed_connection_is_reported_and_can_be_tried_again() {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
            panic!("expected a connection");
        };
        let refused = "deploy@db: Permission denied (publickey).";
        assert!(app.closed("db", connection, Some(refused)).is_empty());
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot open db: deploy@db: Permission denied (publickey)."),
            "{text}"
        );

        let Effect::Connect {
            connection: again, ..
        } = one(enter_host(&mut app, Side::Left, 0))
        else {
            panic!("expected a new connection");
        };
        assert_ne!(again, connection);
    }

    #[test]
    fn cancel_stops_a_connection_attempt_for_every_panel() {
        let mut app = at_root();
        let Effect::Connect {
            connection, stop, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        enter_host(&mut app, Side::Right, 1);
        app.handle(action(Action::Cancel));
        assert!(stop.is_cancelled());
        assert!(!screen(&mut app).contains("Connecting"));
        for side in Side::BOTH {
            assert_eq!(app.panel(side).pending_request(), None);
        }

        // Reports of the stopped attempt change nothing.
        let (handle, _requests) = HostHandle::channel();
        assert!(app.connected("web", connection, handle).is_empty());
        assert!(app.closed("web", connection, None).is_empty());
        assert!(app.hosts.is_empty());
        // Nothing to stop does nothing.
        assert!(app.handle(action(Action::Cancel)).is_empty());
    }

    #[test]
    fn a_lost_connection_sends_its_panels_back_to_the_list_of_hosts() {
        let mut app = at_root();
        let Effect::Connect {
            connection, stop, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
        assert!(screen(&mut app).contains("www"));

        let effects = app.closed("web", connection, Some("Broken pipe"));
        let [
            Effect::List {
                panel: PanelId { side, .. },
                request,
                ..
            },
        ] = &effects[..]
        else {
            panic!("expected the list of hosts, got {effects:?}");
        };
        assert_eq!((*side, &request.location), (Side::Left, &Location::Sftp));
        let text = screen(&mut app);
        assert!(
            text.contains("Lost the connection to web: Broken pipe"),
            "{text}"
        );
        assert!(!stop.is_cancelled(), "it ended on its own");
    }

    #[test]
    fn the_list_of_hosts_disconnects_the_host_under_the_cursor() {
        let mut app = at_root();
        assert_eq!(app.context(), Context::Root);
        assert!(screen(&mut app).contains("8Disconn"));
        let Effect::Connect {
            connection, stop, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
        assert_eq!(app.context(), Context::Panel, "the left panel is on web");
        assert!(!screen(&mut app).contains("Disconn"));
        assert_eq!(app.host_state("web").status, HostStatus::Connected);

        // From the other panel's list of hosts.
        app.active = Side::Right;
        app.handle(action(Action::Down));
        let Effect::List {
            panel: PanelId { side, .. },
            request,
            host: None,
        } = one(app.handle(action(Action::Disconnect)))
        else {
            panic!("expected the list of hosts for the left panel");
        };
        assert_eq!((side, &request.location), (Side::Left, &Location::Sftp));
        assert!(stop.is_cancelled());
        assert_eq!(app.host_state("web").status, HostStatus::Idle);
        assert!(
            app.closed("web", connection, None).is_empty(),
            "already gone"
        );
        assert!(
            !screen(&mut app).contains("Lost"),
            "asked for, so no reason"
        );
        // Nothing to close, or not a host.
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        app.handle(action(Action::Home));
        assert!(app.handle(action(Action::Disconnect)).is_empty());
    }

    /// Opens a location menu with `key`, and answers its listing with the system volume, a USB
    /// stick, and the hosts `web` and `db`.
    fn open_menu(app: &mut App, key: Action) {
        let Effect::ListPlaces { generation } = one(app.handle(action(key))) else {
            panic!("expected a listing for the menu");
        };
        let volume = |path: &str, kind| noc_vfs::Volume {
            mount_point: PathBuf::from(path),
            label: None,
            fs_type: None,
            kind,
            space: None,
        };
        let hosts = ["web", "db"].map(|alias| RootHost {
            alias: alias.to_owned(),
            label: None,
            address: None,
        });
        let listing = Listing::Root {
            volumes: vec![
                volume("/", noc_vfs::VolumeKind::System),
                volume("/Volumes/USB", noc_vfs::VolumeKind::Local),
            ],
            hosts: hosts.to_vec(),
        };
        let location = Location::Root;
        app.places(
            generation,
            Ok(Listed {
                location,
                listing,
                space: None,
            }),
        );
    }

    #[test]
    fn alt_f1_and_alt_f2_change_the_location_of_their_panel() {
        let mut app = loaded();
        open_menu(&mut app, Action::LocationMenuRight);
        assert_eq!(app.context(), Context::Menu);
        let text = screen_of(&mut app, 14);
        assert!(
            text.contains("Right") && text.contains("/Volumes/USB"),
            "{text}"
        );
        assert!(
            text.contains("8Disconn") && text.contains("10Cancel"),
            "{text}"
        );
        // Panel keys do nothing while it is open.
        assert!(app.handle(action(Action::SwitchPanel)).is_empty());
        let Effect::List {
            panel: PanelId { side, .. },
            request,
            ..
        } = one(app.handle(Resolved::Insert('3')))
        else {
            panic!("expected a listing");
        };
        assert_eq!(
            (side, request.location),
            (Side::Right, local("/Volumes/USB"))
        );
        assert_eq!(
            app.active,
            Side::Right,
            "the panel it changed becomes active"
        );
        assert_eq!(app.context(), Context::Panel);

        // Esc closes the menu; a host connects.
        open_menu(&mut app, Action::LocationMenuLeft);
        app.handle(action(Action::Cancel));
        assert!(app.menu.is_none());
        open_menu(&mut app, Action::LocationMenuLeft);
        app.handle(Resolved::Insert('w'));
        let effect = one(app.handle(action(Action::Confirm)));
        assert!(
            matches!(&effect, Effect::Connect { host, .. } if host == "web"),
            "a host connects: {effect:?}"
        );
        assert_eq!(app.active, Side::Left);
    }

    /// The line of `text` that holds `needle`.
    fn line_with<'a>(text: &'a str, needle: &str) -> &'a str {
        text.lines()
            .find(|line| line.contains(needle))
            .unwrap_or("")
    }

    #[test]
    fn f9_opens_the_pull_down_menu_and_its_commands_run() {
        let mut app = loaded();
        app.active = Side::Right;
        app.handle(action(Action::PullDown));
        assert_eq!(app.context(), Context::PullDown);
        let text = screen_of(&mut app, 20);
        assert!(text.contains("Left     File"), "{text}");
        assert!(!text.contains("Change location…"), "the bar alone: {text}");
        app.handle(action(Action::Down));
        let text = screen_of(&mut app, 20);
        assert!(
            line_with(&text, "Change location…").contains("Alt-F2"),
            "{text}"
        );
        assert!(
            line_with(&text, "Sort by size").contains("Ctrl-F6"),
            "keys show for the active panel: {text}"
        );
        assert!(
            text.contains("9Cancel") && text.contains("10Cancel"),
            "{text}"
        );

        // Right goes round to Left, which acts on the panel on its side.
        app.handle(action(Action::Right));
        let text = screen_of(&mut app, 20);
        assert!(
            !line_with(&text, "Sort by size").contains("Ctrl-F6"),
            "{text}"
        );
        assert!(app.handle(Resolved::Insert('z')).is_empty());
        assert!(app.pulldown.is_none(), "a command closes the menu");
        assert_eq!(app.panel(Side::Left).sort_action(), Action::SortBySize);
        assert_eq!(app.panel(Side::Right).sort_action(), Action::SortByName);
        assert_eq!(app.active, Side::Right);

        // F9 opens it again where it closed, on the command that ran.
        app.handle(action(Action::PullDown));
        let text = screen_of(&mut app, 20);
        assert!(text.contains("* Sort by size"), "{text}");
        app.handle(action(Action::Confirm));
        assert_eq!(
            app.panel(Side::Left).sort_action(),
            Action::SortBySize,
            "reversed"
        );

        // Options: the hidden files, in both panels.
        app.handle(action(Action::PullDown));
        app.handle(action(Action::Cancel));
        assert!(app.handle(Resolved::Insert('o')).is_empty());
        assert!(screen_of(&mut app, 20).contains("x Show hidden files"));
        app.handle(Resolved::Insert('h'));
        assert!(!app.config.ui.show_hidden);

        // Commands that cannot run do nothing; Esc goes back to the bar, then closes it.
        app.handle(action(Action::PullDown));
        app.handle(action(Action::Left));
        app.handle(action(Action::Left));
        assert!(
            app.handle(Resolved::Insert('k')).is_empty(),
            "nothing chosen"
        );
        assert!(app.pulldown.is_some());
        app.handle(action(Action::Cancel));
        assert!(app.pulldown.is_some());
        app.handle(action(Action::Cancel));
        assert!(app.pulldown.is_none());

        // A command that opens something: the location menu of the panel on its side.
        app.handle(action(Action::PullDown));
        app.handle(Resolved::Insert('l'));
        let effects = app.handle(Resolved::Insert('l'));
        assert!(matches!(&effects[..], [Effect::ListPlaces { .. }]));
        assert_eq!(app.menu.as_ref().map(LocationMenu::side), Some(Side::Left));
    }

    /// Opens Options → Configuration… through F9.
    fn open_configuration(app: &mut App) {
        app.handle(action(Action::PullDown));
        app.handle(Resolved::Insert('o'));
        assert!(app.handle(Resolved::Insert('c')).is_empty());
        assert!(app.configuration.is_some());
    }

    #[test]
    fn the_configuration_dialog_applies_and_saves_each_change() {
        let mut app = loaded();
        open_configuration(&mut app);
        assert_eq!(app.context(), Context::DialogInput, "on the language");
        let text = screen_of(&mut app, 24);
        assert!(
            text.contains("Configuration") && !text.contains("OK"),
            "{text}"
        );
        assert!(
            app.handle(action(Action::Down)).is_empty(),
            "nothing changed"
        );
        let effects = app.handle(action(Action::Right));
        assert!(app.configuration.is_some(), "it stays open");
        assert_eq!(app.theme, Theme::terminal());
        let [Effect::SaveConfig { old, new, config }] = &effects[..] else {
            panic!("expected the theme to be saved: {effects:?}");
        };
        assert_eq!(old.ui, ui());
        assert_eq!(new.ui, app.config.ui);
        assert_eq!(**config, app.config, "for new connections and listings");
        assert_eq!(
            Config {
                ui: ui(),
                ..(**new).clone()
            },
            Config {
                ui: ui(),
                ..self::config()
            },
            "only the interface"
        );
        app.handle(action(Action::End));
        app.handle(action(Action::Up));
        let effects = app.handle(action(Action::Toggle));
        assert_eq!(app.config.ui.menu_bar, MenuBar::Always);
        let [Effect::SaveConfig { old, .. }] = &effects[..] else {
            panic!("expected the menu bar to be saved: {effects:?}");
        };
        assert_eq!(old.ui.theme, "terminal", "after the theme");
        app.config_saved(Err("cannot write config.toml".to_owned()));
        assert!(app.dialogs.front().is_some(), "a failure says so");
        app.handle(action(Action::Cancel));
        assert!(app.configuration.is_some());
        assert!(app.handle(action(Action::Cancel)).is_empty());
        assert!(app.configuration.is_none(), "Esc closes it");

        // A language that is no tag stays in its field and is not used.
        open_configuration(&mut app);
        app.handle(Resolved::Insert('?'));
        assert!(app.handle(action(Action::Down)).is_empty());
        assert!(app.handle(action(Action::Cancel)).is_empty());
        assert_eq!(app.config.ui.language, "auto");
    }

    /// Opens the category `index` of the Configuration dialog, with the cursor on its first
    /// setting.
    fn config_category(app: &mut App, index: usize) {
        app.handle(action(Action::PrevField));
        app.handle(action(Action::Home));
        for _ in 0..index {
            app.handle(action(Action::Down));
        }
        app.handle(action(Action::Right));
    }

    #[test]
    fn transfer_and_ssh_settings_apply_at_once() {
        let mut app = loaded();
        open_configuration(&mut app);
        config_category(&mut app, 1);
        app.handle(action(Action::Toggle));
        assert!(!app.copy_choices.atomic);
        app.handle(action(Action::Down));
        app.handle(Resolved::Insert('5'));
        config_category(&mut app, 2);
        assert_eq!(app.parallel_jobs, 5, "as the cursor left the field");
        app.handle(action(Action::DeleteToStart));
        for c in "~/bin/ssh".chars() {
            app.handle(Resolved::Insert(c));
        }
        let effects = app.handle(action(Action::Confirm));
        let [Effect::SaveConfig { new, config, .. }] = &effects[..] else {
            panic!("expected the settings to be saved, and nothing to reload: {effects:?}");
        };
        assert_eq!(
            new.ssh.program,
            Path::new("~/bin/ssh"),
            "as typed, for the file"
        );
        assert_eq!(
            config.ssh.program,
            Path::new("/home/me/bin/ssh"),
            "expanded, for ssh"
        );

        // Hidden hosts and volumes change what the root and the list of hosts show, so panels
        // there read them again.
        let mut app = at_root();
        open_configuration(&mut app);
        config_category(&mut app, 3);
        app.handle(Resolved::Insert('x'));
        let effects = app.handle(action(Action::Confirm));
        assert!(matches!(effects[0], Effect::SaveConfig { .. }));
        assert!(
            effects[1..].iter().any(|effect| matches!(
                effect,
                Effect::List {
                    panel: PanelId {
                        side: Side::Left,
                        ..
                    },
                    ..
                }
            )),
            "{effects:?}"
        );
    }

    #[test]
    fn the_menu_bar_stays_above_the_panels_with_ui_menu_bar() {
        let mut app = loaded();
        let top = |app: &mut App| screen_of(app, 10).lines().next().unwrap_or("").to_owned();
        assert!(!top(&mut app).contains("Left"), "on demand");
        app.config.ui.menu_bar = MenuBar::Always;
        let text = screen_of(&mut app, 10);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].contains("Left     File"), "{text}");
        assert!(lines[1].contains('╔'), "the panels start below it: {text}");
        app.handle(action(Action::PullDown));
        app.handle(action(Action::Down));
        assert!(screen_of(&mut app, 20).contains("Change location…"));
    }

    #[test]
    fn the_menu_disconnects_hosts_and_reads_its_listing_again() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        open_menu(&mut app, Action::LocationMenuLeft);
        assert!(
            app.handle(action(Action::Disconnect)).is_empty(),
            "a volume"
        );
        app.handle(action(Action::End));
        let effect = one(app.handle(action(Action::Confirm)));
        assert!(matches!(effect, Effect::Connect { .. }));
        open_menu(&mut app, Action::LocationMenuLeft);
        assert!(app.hosts.contains_key("db"));
        app.handle(action(Action::End));
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        assert!(!app.hosts.contains_key("db"), "stopped connecting");
        // Ctrl-R asks again; the earlier listing no longer counts.
        let effects = app.handle(action(Action::Reload));
        assert!(matches!(
            &effects[..],
            [Effect::ListPlaces { generation: 3 }]
        ));
        assert!(app.menu.is_some());
    }

    #[test]
    fn the_root_shows_connected_hosts_below_its_row_of_hosts() {
        let (mut app, effects) = App::new(Path::new("/"), Path::new("/home/me"), &config());
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        let hosts = ["web", "db"].map(|alias| RootHost {
            alias: alias.to_owned(),
            label: None,
            address: None,
        });
        let root = Listing::Root {
            volumes: Vec::new(),
            hosts: hosts.to_vec(),
        };
        let effects = app.handle(action(Action::Parent));
        answer(&mut app, effects, &root);
        assert!(!screen(&mut app).contains("db"), "{}", screen(&mut app));
        app.active = Side::Right;
        let effects = app.handle(action(Action::Parent));
        answer(&mut app, effects, &root);
        app.handle(action(Action::End));
        let effects = app.handle(action(Action::Enter));
        answer(&mut app, effects, &Listing::Hosts(hosts.to_vec()));
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 2)) else {
            panic!("expected a connection");
        };
        // The row below the left root's row of hosts.
        let below = |app: &mut App| {
            let text = screen_of(app, 10);
            let line = text.lines().nth(4).unwrap_or_default().to_owned();
            line.split("║║").next().unwrap_or_default().to_owned()
        };
        assert!(below(&mut app).contains("db"), "the left root shows it");
        app.closed("db", connection, Some("refused"));
        assert!(!below(&mut app).contains("db"));
    }

    #[test]
    fn disconnect_stops_a_connection_attempt() {
        let mut app = at_root();
        let Effect::Connect { stop, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
            panic!("expected a connection");
        };
        assert_eq!(app.host_state("db").status, HostStatus::Connecting);
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        assert!(stop.is_cancelled());
        assert_eq!(app.panel(Side::Left).pending_request(), None);
        assert_eq!(app.host_state("db").status, HostStatus::Idle);
    }

    #[test]
    fn the_root_shows_failures_and_fresh_addresses() {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        app.resolved("web".to_owned(), "deploy@10.0.0.9".to_owned());
        app.closed("web", connection, Some("Connection refused"));
        assert_eq!(
            app.host_state("web"),
            HostState {
                status: HostStatus::Failed,
                address: Some("deploy@10.0.0.9".to_owned()),
            }
        );
        let text = screen(&mut app);
        assert!(text.contains("✗ web"), "{text}");
        assert!(text.contains("deploy@10.0.0.9"), "{text}");

        // A new attempt clears the failure.
        enter_host(&mut app, Side::Left, 0);
        assert_eq!(app.host_state("web").status, HostStatus::Connecting);
    }

    /// A question from ssh, and where its answer arrives.
    fn ask(id: u64, kind: PromptKind, message: &str) -> (Ask, mpsc::Receiver<Option<String>>) {
        let (sender, answers) = mpsc::channel();
        let reply = Reply::new(move |answer| {
            let answer = answer.map(|text| text.expose_secret().to_owned());
            let _ = sender.send(answer);
        });
        let ask = Ask {
            id,
            context: "web".to_owned(),
            message: message.to_owned(),
            kind,
            reply,
        };
        (ask, answers)
    }

    #[test]
    fn prompts_wait_their_turn_and_take_the_keys() {
        let mut app = at_root();
        let (password, password_answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
        let (host_key, host_key_answer) = ask(2, PromptKind::HostKey, "Continue (yes/no)? ");
        app.ask(password);
        app.ask(host_key);
        assert_eq!(app.context(), Context::DialogInput);
        let text = screen(&mut app);
        assert!(text.contains("deploy@web's password:"), "{text}");
        assert!(!text.contains("Continue"), "the second waits: {text}");
        assert!(text.contains("10Cancel"), "{text}");

        for c in "s3cret".chars() {
            app.handle(Resolved::Insert(c));
        }
        assert!(screen(&mut app).contains("║ ****** "));
        app.handle(action(Action::Confirm));
        assert_eq!(password_answer.try_recv(), Ok(Some("s3cret".to_owned())));

        assert_eq!(app.context(), Context::Dialog);
        assert!(screen(&mut app).contains("Continue (yes/no)?"));
        // Panel keys do nothing while a dialog is open; F10 closes the dialog, not the app.
        app.handle(action(Action::Down));
        app.handle(action(Action::Cancel));
        assert!(!app.quits());
        assert_eq!(host_key_answer.try_recv(), Ok(None));
        assert_eq!(app.context(), Context::Root);
        assert_eq!(
            app.panel(Side::Left).host_under_cursor(),
            None,
            "still on [Local]"
        );
    }

    #[test]
    fn dialogs_close_when_ssh_stops_waiting() {
        let mut app = at_root();
        let (first, first_answer) = ask(1, PromptKind::Secret, "first");
        let (second, second_answer) = ask(2, PromptKind::Confirm, "second");
        app.ask(first);
        app.ask(second);
        app.notice(3, "web", "Confirm user presence for key ED25519-SK");
        app.prompt_closed(2);
        app.prompt_closed(1);
        assert!(first_answer.try_recv().is_err(), "nobody to answer");
        assert!(second_answer.try_recv().is_err());
        assert!(screen(&mut app).contains("Confirm user presence"));
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::Root, "OK dismisses a notice");

        app.notice(4, "web", "Touch your key");
        app.prompt_closed(4);
        assert_eq!(app.context(), Context::Root);
    }

    #[test]
    fn f1_opens_the_help_under_any_prompt() {
        let mut app = loaded();
        app.handle(action(Action::Help));
        assert_eq!(app.context(), Context::Dialog);
        assert!(screen(&mut app).contains("Help"));
        assert!(screen(&mut app).contains("10Cancel"));

        let (password, answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
        app.ask(password);
        assert_eq!(
            app.context(),
            Context::DialogInput,
            "the prompt takes the keys"
        );
        assert!(screen(&mut app).contains("password"));
        app.handle(action(Action::Cancel));
        assert_eq!(answer.try_recv(), Ok(None));
        assert_eq!(app.context(), Context::Dialog, "back to the help");
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);
        assert!(!app.quits());
    }

    #[test]
    fn quitting_stops_every_connection() {
        let mut app = at_root();
        let stops: Vec<CancellationToken> = [(Side::Left, 1), (Side::Right, 2)]
            .into_iter()
            .map(|(side, rows)| match &enter_host(&mut app, side, rows)[..] {
                [Effect::Connect { stop, .. }] => stop.clone(),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        app.disconnect_all();
        assert!(stops.iter().all(CancellationToken::is_cancelled));
        assert!(app.hosts.is_empty());
    }

    #[test]
    fn hidden_files_switch_in_both_panels() {
        let ui = UiConfig {
            show_hidden: false,
            ..ui()
        };
        let (mut app, effects) = App::new(
            Path::new("/srv"),
            Path::new("/home/me"),
            &Config { ui, ..config() },
        );
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir(".git"), dir("src")]),
        );
        assert!(!screen(&mut app).contains(".git"));
        app.handle(action(Action::ToggleHidden));
        assert_eq!(
            screen(&mut app).matches(".git").count(),
            2,
            "in both panels"
        );
        app.handle(action(Action::ToggleHidden));
        assert!(!screen(&mut app).contains(".git"));
    }

    #[test]
    fn ctrl_s_searches_and_other_keys_end_the_search() {
        let mut app = loaded();
        assert!(app.handle(Resolved::Insert('r')).is_empty());
        assert_eq!(
            app.context(),
            Context::Panel,
            "typing does nothing, as in mc"
        );
        app.handle(action(Action::QuickSearch));
        assert!(app.handle(Resolved::Insert('r')).is_empty());
        assert_eq!(app.context(), Context::QuickSearch);
        assert!(screen(&mut app).contains("Search: r"));
        // Enter ends the search and opens what it found.
        let Effect::List { request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        assert_eq!(request.location, local("/srv/right"));
        assert_eq!(app.context(), Context::Panel);

        let mut app = loaded();
        app.handle(action(Action::QuickSearch));
        assert_eq!(
            app.context(),
            Context::QuickSearch,
            "Ctrl-S starts an empty search"
        );
        app.handle(Resolved::Insert('l'));
        app.handle(action(Action::Backspace));
        assert!(screen(&mut app).contains("Search: "));
        assert!(app.handle(action(Action::Cancel)).is_empty());
        assert_eq!(app.context(), Context::Panel, "Esc ends it");
    }

    #[test]
    fn the_f_key_bar_colors_whole_slots() {
        use ratatui::style::Color;

        let mut app = loaded();
        let mut terminal = Terminal::new(TestBackend::new(80, 6)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, now, &TimeZone::UTC))
            .unwrap();
        let buffer = terminal.backend().buffer();
        // `10Quit` starts at column 72; its slot runs to the edge.
        assert_eq!(
            (buffer[(72, 5)].fg, buffer[(72, 5)].bg),
            (Color::White, Color::Black)
        );
        assert_eq!(
            (buffer[(74, 5)].fg, buffer[(74, 5)].bg),
            (Color::Black, Color::Cyan)
        );
        assert_eq!(buffer[(79, 5)].bg, Color::Cyan);
        assert_eq!(
            buffer[(1, 5)].bg,
            Color::Cyan,
            "an empty label is colored too"
        );
    }

    #[test]
    fn handles_quit_redraw_and_text() {
        let mut app = loaded();
        assert!(app.handle(Resolved::Insert('q')).is_empty());
        assert!(!app.quits());
        assert!(!app.take_redraw());
        app.handle(action(Action::Redraw));
        assert!(app.take_redraw());
        assert!(!app.take_redraw(), "a redraw is requested once");
        app.handle(action(Action::Quit));
        assert!(app.quits());
    }

    #[test]
    fn every_supported_f_key_action_has_a_label() {
        let app = loaded();
        for action in app.keymap.fkeys(Context::Panel).into_iter().flatten() {
            if app.supports(action) {
                assert!(fkey_label(action).is_some(), "{action:?}");
            }
        }
    }

    /// An app with both panels on `/srv`, which holds `sub`, `a`, and `b`, the cursor of the
    /// left one on `b`.
    fn on_files() -> App {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let listing = Listing::Dir(vec![dir("sub"), file("a", 3), file("b", 4)]);
        answer(&mut app, effects, &listing);
        app.handle(action(Action::End));
        app
    }

    /// The only effect, which must start a checksum job: its id, its groups of targets with
    /// whether they are remote, and the algorithm.
    fn checksum_job(effects: Vec<Effect>) -> (u64, Vec<(Vec<Location>, bool)>, Algorithm) {
        match one(effects) {
            Effect::Checksum {
                id,
                targets,
                algorithm,
                ..
            } => {
                let targets = targets
                    .into_iter()
                    .map(|(group, host)| (group, host.is_some()))
                    .collect();
                (id, targets, algorithm)
            }
            other => panic!("expected a checksum job, got {other:?}"),
        }
    }

    fn sum(path: &str, name: &str, digest: Option<u8>) -> Sum<Location> {
        Sum {
            path: local(path),
            name: name.as_bytes().to_vec(),
            size: 4,
            digest: digest.map(|byte| vec![byte; 32]),
        }
    }

    /// Ends the job `id` with `sums`.
    fn hashed(app: &mut App, id: u64, sums: Vec<Sum<Location>>) -> Vec<Effect> {
        app.job_event(id, JobEvent::Sums(sums));
        app.job_event(id, JobEvent::Finished { complete: true })
    }

    #[test]
    fn ctrl_x_hash_shows_the_checksum_of_a_file_and_copies_it() {
        let mut app = on_files();
        app.handle(action(Action::Checksum));
        let text = screen_of(&mut app, 20);
        assert!(text.contains("Checksum of \"b\" with:"), "{text}");
        assert!(text.contains("(*) SHA-256"), "{text}");
        assert!(text.contains("( ) BLAKE3"), "{text}");
        assert!(text.contains("Expected checksum"), "{text}");
        assert!(!text.contains("Compare with"), "the same file: {text}");
        let (id, targets, algorithm) = checksum_job(app.handle(action(Action::Confirm)));
        assert_eq!(targets, [(vec![local("/srv/b")], false)]);
        assert_eq!(algorithm, Algorithm::Sha256);
        assert!(screen(&mut app).contains("Counting"), "the job's window");

        let effects = hashed(&mut app, id, vec![sum("/srv/b", "b", Some(0xab))]);
        assert!(effects.is_empty(), "nothing changed: {effects:?}");
        let text = screen_of(&mut app, 20);
        assert!(text.contains(&"ab".repeat(32)), "{text}");
        assert!(!text.contains("Matches"), "nothing expected: {text}");
        assert_eq!(app.context(), Context::Dialog);
        assert_eq!(app.take_clipboard(), None);
        app.handle(action(Action::Confirm));
        assert_eq!(app.take_clipboard(), Some("ab".repeat(32)));
        assert_eq!(app.take_clipboard(), None, "copied once");
        assert!(screen_of(&mut app, 20).contains("Sent to the terminal's clipboard."));
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn the_expected_checksum_picks_its_algorithm_and_gives_the_verdict() {
        let mut app = on_files();
        app.handle(action(Action::Checksum));
        // Pasted as sha256sum prints it, in capitals, while SHA-256 is chosen.
        type_text(&mut app, &format!("{}  b", "CD".repeat(16)));
        let (id, _, algorithm) = checksum_job(app.handle(action(Action::Confirm)));
        assert_eq!(algorithm, Algorithm::Md5);
        let mut md5 = sum("/srv/b", "b", None);
        md5.digest = Some(vec![0xcd; 16]);
        hashed(&mut app, id, vec![md5]);
        let text = screen_of(&mut app, 20);
        assert!(text.contains(" MD5 "), "{text}");
        assert!(text.contains("Matches the expected checksum."), "{text}");
        app.handle(action(Action::Cancel));

        app.handle(action(Action::Checksum));
        let text = screen_of(&mut app, 20);
        assert!(text.contains("(*) MD5"), "the last choice: {text}");
        type_text(&mut app, &"0".repeat(32));
        let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
        let mut md5 = sum("/srv/b", "b", None);
        md5.digest = Some(vec![0xcd; 16]);
        hashed(&mut app, id, vec![md5]);
        let text = screen_of(&mut app, 20);
        assert!(text.contains("Does not match"), "{text}");
        app.handle(action(Action::Cancel));

        app.handle(action(Action::Checksum));
        type_text(&mut app, "not-hex");
        assert!(app.handle(action(Action::Confirm)).is_empty());
        let text = screen_of(&mut app, 20);
        assert!(text.contains("is not a checksum"), "{text}");
    }

    #[test]
    fn choices_take_space_or_enter() {
        let mut app = on_files();
        app.handle(action(Action::Checksum));
        // From the field up to the last choice, BLAKE3.
        app.handle(action(Action::Up));
        let (id, _, algorithm) = checksum_job(app.handle(action(Action::Confirm)));
        assert_eq!(algorithm, Algorithm::Blake3);
        app.job_event(id, JobEvent::Finished { complete: false });
        app.handle(action(Action::Checksum));
        app.handle(action(Action::Up));
        app.handle(action(Action::Up));
        app.handle(action(Action::Toggle));
        app.handle(action(Action::Down));
        app.handle(action(Action::Down));
        let (_, _, algorithm) = checksum_job(app.handle(action(Action::Confirm)));
        assert_eq!(
            algorithm,
            Algorithm::Md5,
            "Space chose it; Enter in the field keeps it"
        );
    }

    #[test]
    fn ctrl_x_hash_compares_with_the_file_in_the_other_panel() {
        let mut app = on_files();
        app.active = Side::Right;
        app.handle(action(Action::Down));
        let effects = app.handle(action(Action::Enter));
        answer(&mut app, effects, &Listing::Dir(vec![file("b", 4)]));
        app.handle(action(Action::End));
        app.active = Side::Left;
        app.handle(action(Action::Checksum));
        let text = screen_of(&mut app, 20);
        assert!(text.contains("[x] Compare with /srv/sub/b"), "{text}");
        let (id, targets, _) = checksum_job(app.handle(action(Action::Confirm)));
        assert_eq!(
            targets,
            [
                (vec![local("/srv/b")], false),
                (vec![local("/srv/sub/b")], false)
            ]
        );
        let sums = vec![sum("/srv/b", "b", Some(1)), sum("/srv/sub/b", "b", Some(1))];
        hashed(&mut app, id, sums);
        let text = screen_of(&mut app, 24);
        assert!(text.contains("The files are the same."), "{text}");
        assert!(text.contains("/srv/sub/b"), "{text}");
        assert!(!text.contains("Save"), "two places: {text}");
        app.handle(action(Action::Cancel));

        app.handle(action(Action::Checksum));
        let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
        let sums = vec![sum("/srv/b", "b", Some(1)), sum("/srv/sub/b", "b", Some(2))];
        hashed(&mut app, id, sums);
        assert!(screen_of(&mut app, 24).contains("The files differ."));
    }

    #[test]
    fn checksums_of_many_files_are_saved_as_sha256sum_writes_them() {
        let mut app = on_files();
        app.handle(action(Action::Home));
        app.handle(action(Action::Down));
        app.handle(action(Action::Mark));
        app.handle(action(Action::Mark));
        app.handle(action(Action::Checksum));
        let text = screen_of(&mut app, 20);
        assert!(
            text.contains("Checksums of 2 files and directories"),
            "{text}"
        );
        assert!(!text.contains("Expected"), "{text}");
        let (id, targets, _) = checksum_job(app.handle(action(Action::Confirm)));
        assert_eq!(targets, [(vec![local("/srv/sub"), local("/srv/a")], false)]);
        let sums = vec![
            sum("/srv/sub/x", "sub/x", Some(1)),
            sum("/srv/sub/y", "sub/y", None),
            sum("/srv/a", "a", Some(2)),
        ];
        hashed(&mut app, id, sums);
        let text = screen_of(&mut app, 24);
        assert!(text.contains("skipped"), "{text}");
        // Copy, Copy all, Save.
        app.handle(action(Action::Right));
        app.handle(action(Action::Confirm));
        let all = app.take_clipboard().unwrap();
        let line = |byte: &str, name: &str| format!("{}  {name}\n", byte.repeat(32));
        assert_eq!(all, line("01", "sub/x") + &line("02", "a"));
        app.handle(action(Action::Right));
        app.handle(action(Action::Confirm));
        let text = screen_of(&mut app, 24);
        assert!(text.contains("SHA256SUMS"), "{text}");
        let written = |effects: Vec<Effect>| match one(effects) {
            Effect::WriteFile {
                location,
                bytes,
                replace,
                host,
            } => {
                assert!(host.is_none());
                (location, String::from_utf8(bytes).unwrap(), replace)
            }
            other => panic!("expected a write, got {other:?}"),
        };
        let (location, text, replace) = written(app.handle(action(Action::Confirm)));
        assert_eq!(
            (&location, &text, replace),
            (&local("/srv/SHA256SUMS"), &all, false)
        );

        // Taken: No is the default, and Yes writes over it.
        assert!(app.written(&location, Err(None)).is_empty());
        assert!(screen_of(&mut app, 24).contains("/srv/SHA256SUMS is there already"));
        app.handle(action(Action::Left));
        let (_, _, replace) = written(app.handle(action(Action::Confirm)));
        assert!(replace);
        let effects = app.written(&location, Ok(()));
        assert_eq!(effects.len(), 2, "both panels show /srv");
        assert!(screen_of(&mut app, 24).contains("Saved to /srv/SHA256SUMS."));
    }

    #[test]
    fn an_aborted_checksum_job_shows_nothing_and_an_empty_one_says_so() {
        let mut app = on_files();
        app.handle(action(Action::Checksum));
        let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
        app.job_event(id, JobEvent::Finished { complete: false });
        assert_eq!(app.context(), Context::Panel);
        app.handle(action(Action::Checksum));
        let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
        hashed(&mut app, id, Vec::new());
        assert!(screen_of(&mut app, 20).contains("There are no files to hash."));
    }

    #[test]
    fn expected_checksums_and_files_of_them() {
        assert_eq!(
            parse_expected(&"A".repeat(64), Algorithm::Blake3),
            Some(("a".repeat(64), Algorithm::Blake3)),
            "the choice, where its length fits"
        );
        assert_eq!(
            parse_expected(&"a".repeat(64), Algorithm::Md5),
            Some(("a".repeat(64), Algorithm::Sha256))
        );
        assert_eq!(
            parse_expected(&"a".repeat(128), Algorithm::Sha256).map(|(_, a)| a),
            Some(Algorithm::Sha512)
        );
        assert_eq!(parse_expected(&"a".repeat(63), Algorithm::Sha256), None);
        assert_eq!(parse_expected("sha256:abc", Algorithm::Sha256), None);
        let lines = [
            (b"plain".to_vec(), Some("01".to_owned())),
            (b"gone".to_vec(), None),
            (b"two\nlines\\x".to_vec(), Some("02".to_owned())),
        ];
        assert_eq!(
            sums_file(&lines),
            b"01  plain\n\\02  two\\nlines\\\\x\n".to_vec()
        );
    }

    /// The tab numbers on `side`, and the one that shows.
    fn tab_numbers(app: &App, side: Side) -> (Vec<u64>, u64) {
        let tabs = app.tabs(side);
        let numbers = tabs.iter().map(|tab| tab.id.tab).collect();
        (numbers, tabs.active().id.tab)
    }

    #[test]
    fn a_new_tab_shows_the_same_directory_at_once_and_closes_again() {
        let mut app = loaded();
        app.handle(action(Action::End));
        assert!(
            app.handle(action(Action::NewTab)).is_empty(),
            "no listing needed"
        );
        assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3], 3));
        assert_eq!(
            app.panel(Side::Left).name_under_cursor(),
            Some(&b"right"[..])
        );
        let text = screen(&mut app);
        assert!(text.contains(" 1 srv │ 2 srv "), "{text}");
        assert!(
            text.contains("╠ /srv ═"),
            "the title keeps the whole path, under tees that join the line: {text}"
        );

        // Each tab keeps its own cursor.
        app.handle(action(Action::Home));
        app.handle(action(Action::PrevTab));
        assert_eq!(
            app.panel(Side::Left).name_under_cursor(),
            Some(&b"right"[..])
        );
        app.handle(action(Action::NextTab));
        assert_eq!(app.panel(Side::Left).name_under_cursor(), None, "on ..");
        assert_eq!(tab_numbers(&app, Side::Right), (vec![2], 2), "its own tabs");

        assert!(app.handle(action(Action::CloseTab)).is_empty());
        assert_eq!(tab_numbers(&app, Side::Left), (vec![1], 1));
        app.handle(action(Action::CloseTab));
        assert_eq!(
            tab_numbers(&app, Side::Left),
            (vec![1], 1),
            "the last one stays"
        );
        assert!(!screen(&mut app).contains(" 1 srv "), "one tab has no bar");
    }

    #[test]
    fn listings_reach_hidden_tabs_and_closed_tabs_drop_them() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let first = one(app.handle(action(Action::Enter)));
        // The new tab waits for the same directory, with its own request.
        let second = one(app.handle(action(Action::NewTab)));
        let (&Effect::List { panel: one_id, .. }, &Effect::List { panel: two_id, .. }) =
            (&first, &second)
        else {
            panic!("expected listings");
        };
        assert_ne!(one_id, two_id);
        assert_eq!(app.shown(Side::Left), two_id);
        answer(&mut app, vec![first], &Listing::Dir(vec![dir("inner")]));
        assert_eq!(
            app.panel_of(one_id).map(Panel::location),
            Some(&local("/srv/left")),
            "the hidden tab took its listing"
        );
        assert_eq!(
            app.panel(Side::Left).location(),
            &local("/srv"),
            "still waits"
        );

        app.handle(action(Action::CloseTab));
        answer(&mut app, vec![second], &Listing::Dir(Vec::new()));
        assert_eq!(app.shown(Side::Left), one_id);
        assert!(screen(&mut app).contains("inner"));
    }

    #[test]
    fn hidden_tabs_read_their_directory_again_once_they_show() {
        let mut app = loaded();
        app.handle(action(Action::NewTab));
        let effects = app.reload(&local("/srv"));
        let shown: Vec<PanelId> = effects
            .iter()
            .map(|effect| match effect {
                Effect::List { panel, .. } => *panel,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(shown, [app.shown(Side::Left), app.shown(Side::Right)]);
        answer(&mut app, effects, &Listing::Dir(vec![dir("left")]));

        let effects = app.handle(action(Action::PrevTab));
        let [Effect::List { panel, request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(
            (panel.tab, &request.location),
            (1, &local("/srv")),
            "the hidden one, now"
        );
        answer(&mut app, effects, &Listing::Dir(vec![dir("left")]));
        assert!(
            app.handle(action(Action::NextTab)).is_empty(),
            "read already"
        );
    }

    #[test]
    fn disconnecting_sends_hidden_tabs_on_the_host_back_too() {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        connect_web(&mut app, Side::Left, connection, "/var/www");
        app.handle(action(Action::NewTab));
        app.handle(action(Action::PrevTab));
        let effects = app.disconnect("web");
        let tabs: Vec<u64> = effects
            .iter()
            .map(|effect| match effect {
                Effect::List { panel, request, .. } => {
                    assert_eq!(request.location, Location::Sftp);
                    panel.tab
                }
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(tabs.len(), 2, "both tabs on web: {effects:?}");
    }

    #[test]
    fn the_list_of_tabs_shows_the_one_chosen() {
        let mut app = loaded();
        app.handle(action(Action::NewTab));
        app.handle(action(Action::Down));
        let effects = app.handle(action(Action::Enter));
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        app.handle(action(Action::TabList));
        let text = screen_of(&mut app, 16);
        assert!(text.contains("Tabs"), "{text}");
        assert!(
            text.contains("1 /srv") && text.contains("2 /srv/left"),
            "{text}"
        );
        app.handle(action(Action::Up));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3], 1));
    }

    /// Saves the tabs of both panels as `name` with Alt-Shift-W, and returns what is saved.
    fn save_as(app: &mut App, name: &str) -> Workspace {
        assert!(app.handle(action(Action::SaveWorkspace)).is_empty());
        app.handle(action(Action::DeleteToStart));
        typed(app, name);
        let Effect::Workspaces(WorkspaceChange::Save(workspace)) =
            one(app.handle(action(Action::Confirm)))
        else {
            panic!("expected a workspace to save");
        };
        workspace
    }

    /// `workspaces.toml` holds `saved` now.
    fn holds(app: &mut App, saved: &[Workspace]) {
        let workspaces = Workspaces {
            workspaces: saved.to_vec(),
        };
        app.workspaces_changed(true, Ok(workspaces));
    }

    /// A workspace: on the left `~/src`, sorted by time, with the cursor on `notes`, then `web:/var/www`,
    /// which shows; on the right the root, which has the keys.
    fn noon() -> Workspace {
        use noc_config::{Place, SavedTab, SortBy};

        let tab = |place: Place| SavedTab {
            place,
            sort: SortBy::Name,
            descending: false,
            cursor: None,
            current: false,
        };
        Workspace {
            name: "noon".to_owned(),
            active: PanelSide::Right,
            left: vec![
                SavedTab {
                    sort: SortBy::Time,
                    descending: true,
                    cursor: Some("notes".to_owned()),
                    ..tab(Place::Local("~/src".to_owned()))
                },
                SavedTab {
                    current: true,
                    ..tab(Place::Remote {
                        host: "web".to_owned(),
                        path: "/var/www".to_owned(),
                    })
                },
            ],
            right: vec![tab(Place::Root)],
        }
    }

    #[test]
    fn alt_shift_w_saves_the_tabs_of_both_panels_under_a_name() {
        use noc_config::{Place, SortBy};

        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::SortBySize));
        app.handle(action(Action::NewTab));
        app.handle(action(Action::PrevTab));
        app.handle(action(Action::SwitchPanel));
        let workspace = save_as(&mut app, " noon ");
        assert_eq!(workspace.name, "noon", "without the spaces around it");
        assert_eq!(workspace.active, PanelSide::Right);
        let left = &workspace.left;
        assert_eq!(left.len(), 2);
        assert_eq!(left[0].place, Place::Local("/srv".to_owned()));
        assert_eq!(left[0].cursor.as_deref(), Some("left"));
        assert_eq!((left[0].sort, left[0].descending), (SortBy::Size, true));
        assert!(left[0].current && !left[1].current);
        assert!(!workspace.right[0].current, "one tab needs no mark");
        assert_eq!(app.workspace.as_deref(), Some("noon"));

        // Its name comes back, and saving it again asks nothing.
        holds(&mut app, &[workspace]);
        app.handle(action(Action::SaveWorkspace));
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("Save workspace") && text.contains("noon"),
            "{text}"
        );
        assert!(matches!(
            one(app.handle(action(Action::Confirm))),
            Effect::Workspaces(WorkspaceChange::Save(_))
        ));
        // An empty name saves nothing.
        app.handle(action(Action::SaveWorkspace));
        app.handle(action(Action::DeleteToStart));
        assert!(app.handle(action(Action::Confirm)).is_empty());
    }

    #[test]
    fn the_name_of_another_workspace_asks_before_replacing_it() {
        let mut app = loaded();
        holds(&mut app, &[noon()]);
        app.handle(action(Action::SaveWorkspace));
        typed(&mut app, "noon");
        assert!(app.handle(action(Action::Confirm)).is_empty(), "asks first");
        assert!(screen_of(&mut app, 16).contains("Replace it"));
        let Effect::Workspaces(WorkspaceChange::Save(workspace)) =
            one(app.handle(action(Action::Confirm)))
        else {
            panic!("expected a workspace to save");
        };
        assert_eq!(
            workspace.right[0].place,
            noc_config::Place::Local("/srv".to_owned())
        );
    }

    #[test]
    fn restoring_replaces_every_tab_and_hidden_ones_list_when_they_show() {
        let mut app = loaded();
        app.handle(action(Action::NewTab));
        holds(&mut app, &[noon()]);
        let effects = app.restore_workspace("noon");
        // The tab that shows on the left waits for web; the hidden one lists nothing yet.
        let [
            Effect::Connect { connection, .. },
            Effect::List {
                panel: right,
                request,
                host: None,
            },
        ] = &effects[..]
        else {
            panic!("unexpected {effects:?}");
        };
        assert_eq!(right.side, Side::Right);
        assert_eq!(request.location, Location::Root);
        assert_eq!(app.active, Side::Right);
        assert_eq!(tab_numbers(&app, Side::Left), (vec![4, 5], 5), "new tabs");
        assert_eq!(app.panel(Side::Left).location(), &remote("web", "/var/www"));
        assert_eq!(app.workspace.as_deref(), Some("noon"));

        // web connects: only the tab that shows lists.
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", *connection, handle);
        let [Effect::List { panel, .. }] = &effects[..] else {
            panic!("expected one listing, got {effects:?}");
        };
        assert_eq!(panel.tab, 5);

        // The hidden one asks once it shows, sorted and with its cursor as saved.
        app.active = Side::Left;
        let effects = app.handle(action(Action::PrevTab));
        let [Effect::List { panel, request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!((panel.tab, &request.location), (4, &local("/home/me/src")));
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("a"), dir("notes"), dir("z")]),
        );
        assert_eq!(
            app.panel(Side::Left).name_under_cursor(),
            Some(&b"notes"[..])
        );
        assert_eq!(
            app.panel(Side::Left).sort_action(),
            Action::SortByTime,
            "sorted as saved"
        );
        assert!(
            app.handle(action(Action::NextTab)).is_empty()
                && app.handle(action(Action::PrevTab)).is_empty(),
            "listed once"
        );
    }

    #[test]
    fn a_workspace_gone_meanwhile_says_so() {
        let mut app = loaded();
        assert!(app.restore_workspace("noon").is_empty());
        assert!(screen_of(&mut app, 16).contains("is not saved any more"));
    }

    #[test]
    fn f9_lists_the_workspaces_and_restores_one_by_its_digit() {
        let mut app = loaded();
        let mut other = noon();
        other.name = "photos".to_owned();
        holds(&mut app, &[other, noon()]);
        app.handle(action(Action::PullDown));
        app.handle(Resolved::Insert('w'));
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("Save workspace…")
                && text.contains("1 photos")
                && text.contains("2 noon"),
            "{text}"
        );
        let effects = app.handle(Resolved::Insert('2'));
        assert!(!effects.is_empty());
        assert_eq!(app.workspace.as_deref(), Some("noon"));
        app.handle(action(Action::PullDown));
        assert!(
            screen_of(&mut app, 16).contains("* 2 noon"),
            "the one restored last"
        );
    }

    #[test]
    fn the_workspaces_window_saves_restores_renames_and_deletes() {
        let mut app = loaded();
        let mut photos = noon();
        photos.name = "photos".to_owned();
        holds(&mut app, &[noon(), photos]);
        app.workspace = Some("noon".to_owned());
        assert!(app.handle(action(Action::Workspaces)).is_empty());
        assert_eq!(app.context(), Context::Workspaces);

        // Insert saves the tabs as a new workspace: the name starts empty.
        app.handle(action(Action::SaveWorkspace));
        assert_eq!(app.context(), Context::DialogInput);
        typed(&mut app, "srv");
        let effects = app.handle(action(Action::Confirm));
        let [Effect::Workspaces(WorkspaceChange::Save(saved))] = &effects[..] else {
            panic!("unexpected {effects:?}");
        };
        assert_eq!(saved.name, "srv");
        let mut photos = noon();
        photos.name = "photos".to_owned();
        holds(&mut app, &[noon(), photos, saved.clone()]);
        assert_eq!(app.context(), Context::Workspaces, "still open");
        assert!(
            matches!(
                &app.handle(action(Action::Confirm))[..],
                [Effect::List { .. }, ..]
            ),
            "the cursor went to the new one, which restores at once"
        );
        assert_eq!(app.workspace.as_deref(), Some("srv"));
        app.workspace = Some("noon".to_owned());
        holds(
            &mut app,
            &[noon(), {
                let mut photos = noon();
                photos.name = "photos".to_owned();
                photos
            }],
        );
        app.open_workspaces();
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("Workspaces") && text.contains("3 tabs"),
            "{text}"
        );
        assert!(
            text.contains("6Rename") && text.contains("8Delete"),
            "{text}"
        );

        // F6: a new name in place; the one restored last follows it.
        app.handle(action(Action::Move));
        app.handle(action(Action::DeleteToStart));
        typed(&mut app, "noc");
        let effects = app.handle(action(Action::Confirm));
        let [Effect::Workspaces(WorkspaceChange::Rename { from, to })] = &effects[..] else {
            panic!("unexpected {effects:?}");
        };
        assert_eq!((from.as_str(), to.as_str()), ("noon", "noc"));
        assert_eq!(app.workspace.as_deref(), Some("noc"));
        let mut renamed = noon();
        renamed.name = "noc".to_owned();
        let mut photos = noon();
        photos.name = "photos".to_owned();
        holds(&mut app, &[renamed, photos]);

        // F6 to the name of another one asks first, No by default.
        app.handle(action(Action::Move));
        app.handle(action(Action::DeleteToStart));
        typed(&mut app, "photos");
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert!(app.handle(action(Action::Confirm)).is_empty(), "No");

        // F8 asks, Yes by default.
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        assert!(screen_of(&mut app, 16).contains("Delete workspace \"photos\"?"));
        let effects = app.handle(action(Action::Confirm));
        assert!(
            matches!(&effects[..], [Effect::Workspaces(WorkspaceChange::Remove(name))] if name == "photos"),
            "{effects:?}"
        );

        // Enter restores and closes the window.
        app.handle(action(Action::Home));
        assert!(!app.handle(action(Action::Confirm)).is_empty());
        assert!(app.workspaces_window.is_none());
    }

    #[test]
    fn a_file_that_cannot_be_read_or_written_says_why() {
        let mut app = loaded();
        app.workspaces_changed(false, Err("invalid workspace".to_owned()));
        assert!(screen_of(&mut app, 16).contains("Cannot read the workspaces"));
        app.handle(action(Action::Confirm));
        app.workspaces_changed(true, Err("cannot write".to_owned()));
        assert!(screen_of(&mut app, 16).contains("Cannot save the workspaces"));
    }

    #[test]
    fn tabs_go_in_the_frame_where_the_config_says() {
        let mut config = config();
        config.ui.tab_bar = TabBar::Frame;
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config);
        answer(&mut app, effects, &Listing::Dir(vec![dir("docs")]));
        app.handle(action(Action::Down));
        let effects = app.handle(action(Action::Enter));
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        app.handle(action(Action::NewTab));
        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, now, &TimeZone::UTC))
            .unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn tabs_in_the_frame_keep_the_column_lines_joined_to_it() {
        let mut config = config();
        config.ui.tab_bar = TabBar::Frame;
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config);
        answer(&mut app, effects, &Listing::Dir(vec![dir("docs")]));
        app.handle(action(Action::NewTab));
        let mut terminal = Terminal::new(TestBackend::new(100, 6)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, now, &TimeZone::UTC))
            .unwrap();
        let text = terminal.backend().to_string();
        let top = text.lines().next().unwrap_or_default();
        let left: String = top.chars().skip(1).take(50).collect();
        assert!(left.contains(" 1 srv ═ 2 /srv "), "{text}");
        assert_eq!(left.matches('╤').count(), 2, "{text}");
    }

    #[test]
    fn the_panel_menus_open_and_close_tabs_on_their_side() {
        let mut app = loaded();
        assert!(app.run(Command::On(Side::Right, Action::NewTab)).is_empty());
        assert_eq!(tab_numbers(&app, Side::Right), (vec![2, 3], 3));
        assert_eq!(app.active, Side::Left, "the keys stay");
        let status = app.command_status(Command::On(Side::Left, Action::CloseTab));
        assert!(!status.enabled, "one tab on the left");
        let status = app.command_status(Command::On(Side::Right, Action::CloseTab));
        assert!(status.enabled);
        assert_eq!(status.key, None, "keys act on the active panel");
        app.run(Command::On(Side::Right, Action::CloseTab));
        assert_eq!(tab_numbers(&app, Side::Right), (vec![2], 2));
    }

    #[test]
    fn a_line_of_many_tabs_parts_the_sides_and_marks_those_left_out() {
        let mut app = loaded();
        for _ in 0..6 {
            app.handle(action(Action::NewTab));
        }
        app.active = Side::Right;
        for _ in 0..6 {
            app.handle(action(Action::NewTab));
        }
        for _ in 0..3 {
            app.handle(action(Action::PrevTab));
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 4)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, now, &TimeZone::UTC))
            .unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    /// An app on `/srv`, which holds `sub` and `notes`, that records directories in zoxide.
    fn recording() -> App {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &with_zoxide());
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
        );
        app
    }

    #[test]
    fn work_in_a_directory_counts_in_zoxide_once_a_visit() {
        let mut app = recording();
        // Passing through does not count.
        app.handle(action(Action::Down));
        let effects = app.handle(action(Action::Enter));
        assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
        answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
        let effects = app.handle(action(Action::Parent));
        assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
        );
        // Viewing a file does, once.
        app.handle(action(Action::End));
        let effects = app.handle(action(Action::View));
        assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);
        app.handle(action(Action::Quit));
        let effects = app.handle(action(Action::Edit));
        assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0], "once a visit");
        app.edited(Ok(false));
        // Reading the directory again is the same visit.
        let effects = app.handle(action(Action::Reload));
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
        );
        app.handle(action(Action::End));
        let effects = app.handle(action(Action::View));
        assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
        app.handle(action(Action::Quit));
        // Coming back is a new visit.
        app.handle(action(Action::Home));
        app.handle(action(Action::Down));
        let effects = app.handle(action(Action::Enter));
        answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
        let effects = app.handle(action(Action::Parent));
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
        );
        app.handle(action(Action::End));
        let effects = app.handle(action(Action::Edit));
        assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);
    }

    #[test]
    fn jobs_count_their_source_and_a_target_a_panel_shows() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &with_zoxide());
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("left"), dir("right")]),
        );
        app.active = Side::Right;
        app.handle(action(Action::End));
        let effects = app.handle(action(Action::Enter));
        answer(&mut app, effects, &Listing::Dir(vec![file("old", 1)]));
        app.active = Side::Left;
        app.handle(action(Action::Down));
        app.handle(action(Action::Copy));
        let effects = app.handle(action(Action::Confirm));
        assert_eq!(
            zoxide_adds(&effects),
            [PathBuf::from("/srv"), PathBuf::from("/srv/right")]
        );
        copy_job(without_zoxide(effects));
        // A typed target that no panel shows does not count; the source did already.
        app.handle(action(Action::Copy));
        app.handle(action(Action::DeleteToStart));
        type_text(&mut app, "/tmp");
        let effects = app.handle(action(Action::Confirm));
        assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
        // Delete, F7, and checksums count in the other panel.
        app.active = Side::Right;
        app.handle(action(Action::End));
        app.handle(action(Action::Checksum));
        let effects = app.handle(action(Action::Confirm));
        assert_eq!(
            zoxide_adds(&effects),
            [] as [PathBuf; 0],
            "counted as the target"
        );
    }

    #[test]
    fn shift_f6_renames_in_the_row_and_asks_before_replacing_a_file() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let listing = Listing::Dir(vec![file("report.pdf", 1), file("taken.pdf", 2)]);
        answer(&mut app, effects, &listing);
        app.handle(action(Action::Down));
        app.handle(action(Action::Rename));
        assert_eq!(app.context(), Context::Rename);
        assert!(screen(&mut app).contains("Rename: report.pdf"));
        type_text(&mut app, "taken");
        let rename = one(app.handle(action(Action::Confirm)));
        let Effect::Rename {
            panel,
            from,
            to,
            replace: false,
            host: None,
        } = rename
        else {
            panic!("expected a rename, got {rename:?}");
        };
        assert_eq!(
            (&from, &to),
            (&local("/srv/report.pdf"), &local("/srv/taken.pdf"))
        );
        assert_eq!(app.context(), Context::Panel, "the field is gone");

        // A file has the name: No is the default; Left goes to Yes.
        assert!(app.entry_renamed(panel, &from, &to, Err(None)).is_empty());
        assert!(screen(&mut app).contains("\"taken.pdf\" is there already."));
        app.handle(action(Action::Left));
        let again = one(app.handle(action(Action::Confirm)));
        assert!(
            matches!(&again, Effect::Rename { replace: true, to: target, .. } if *target == to),
            "{again:?}"
        );
        let effects = app.entry_renamed(panel, &from, &to, Ok(()));
        assert_eq!(effects.len(), 2, "both panels show the directory");
        answer(&mut app, effects, &Listing::Dir(vec![file("taken.pdf", 1)]));
        assert_eq!(
            app.panel(Side::Left).name_under_cursor(),
            Some(&b"taken.pdf"[..])
        );

        let effects = app.entry_renamed(panel, &from, &to, Err(Some("no space".to_owned())));
        assert!(effects.is_empty());
        assert!(screen(&mut app).contains("Cannot rename /srv/report.pdf: no space"));
        app.handle(action(Action::Confirm));

        // A slash would move it: the error goes, and the field stays.
        app.handle(action(Action::Rename));
        type_text(&mut app, "a/b");
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert!(screen(&mut app).contains("\"a/b.pdf\" cannot be a name"));
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::Rename);
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);

        // The same name, or none, renames nothing.
        app.handle(action(Action::Rename));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        app.handle(action(Action::Rename));
        app.handle(action(Action::End));
        app.handle(action(Action::DeleteToStart));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert_eq!(app.context(), Context::Panel);

        // Not on `..`.
        app.handle(action(Action::Home));
        app.handle(action(Action::Rename));
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn mkdir_and_delete_count_and_remote_directories_never_do() {
        let mut app = recording();
        app.handle(action(Action::Mkdir));
        app.handle(action(Action::DeleteToStart));
        type_text(&mut app, "new");
        let effects = app.handle(action(Action::Confirm));
        assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);

        let mut app = recording();
        app.handle(action(Action::End));
        app.handle(action(Action::Delete));
        let effects = app.handle(action(Action::Confirm));
        assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);

        let mut app = recording();
        app.config.zoxide.record = false;
        app.handle(action(Action::End));
        let effects = app.handle(action(Action::View));
        assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0], "record is off");

        let mut app = at_root();
        app.config.zoxide.record = true;
        let hosts = app.panel(Side::Left).location().clone();
        assert_eq!(zoxide_adds(&app.note(&hosts)), [] as [PathBuf; 0]);
        let mut app = on_a_host().0;
        app.config.zoxide.record = true;
        let here = app.panel(app.active).location().clone();
        assert!(matches!(here, Location::Remote { .. }), "{here:?}");
        assert_eq!(zoxide_adds(&app.note(&here)), [] as [PathBuf; 0]);
    }

    #[test]
    fn alt_z_asks_zoxide_and_jumps_in_the_active_panel() {
        let mut app = recording();
        // zoxide matches the keywords.
        app.config.ui.fuzzy_search = false;
        app.active = Side::Right;
        let effects = app.handle(action(Action::Jump));
        let [
            Effect::ZoxideQuery {
                generation: first,
                keywords,
                exclude,
            },
        ] = &effects[..]
        else {
            panic!("expected a query, got {effects:?}");
        };
        assert_eq!(keywords, &[] as &[String]);
        assert_eq!(exclude.as_deref(), Some(Path::new("/srv")));
        assert_eq!(app.context(), Context::Jump);
        app.handle(Resolved::Insert('s'));
        let effects = app.handle(Resolved::Insert('c'));
        let [
            Effect::ZoxideQuery {
                generation,
                keywords,
                ..
            },
        ] = &effects[..]
        else {
            panic!("expected a query, got {effects:?}");
        };
        assert_eq!(keywords, &["sc".to_owned()]);
        let found = |paths: &[&str]| {
            Ok(paths
                .iter()
                .map(|path| Scored {
                    score: 1.0,
                    path: PathBuf::from(path),
                })
                .collect())
        };
        app.jumps(*first, found(&["/stale"]));
        app.jumps(*generation, found(&["/home/me/src", "/opt/scripts"]));
        let text = screen_of(&mut app, 12);
        assert!(text.contains("Jump to: sc"), "{text}");
        assert!(
            text.contains("~/src") && text.contains("/opt/scripts") && !text.contains("/stale"),
            "{text}"
        );
        app.handle(action(Action::Down));
        let effects = app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::Panel);
        assert_eq!(zoxide_adds(&effects), [PathBuf::from("/opt/scripts")]);
        let effects = without_zoxide(effects);
        let [Effect::List { panel, request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(panel.side, Side::Right, "the active panel");
        assert_eq!(request.location, local("/opt/scripts"));
        answer(&mut app, effects, &Listing::Dir(vec![file("run", 1)]));
        // The jump counted the visit.
        app.handle(action(Action::End));
        let effects = app.handle(action(Action::View));
        assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
        app.handle(action(Action::Quit));
        // Esc closes the window; so does a failed query, after saying why.
        app.handle(action(Action::Jump));
        app.jumps(app.jump_queries, Err("zoxide is not installed".to_owned()));
        assert!(screen_of(&mut app, 12).contains("zoxide is not installed"));
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn with_fuzzy_search_the_zoxide_window_asks_once_and_filters_as_fzf() {
        let mut app = recording();
        let effects = app.handle(action(Action::Jump));
        let [
            Effect::ZoxideQuery {
                generation,
                keywords,
                ..
            },
        ] = &effects[..]
        else {
            panic!("expected a query, got {effects:?}");
        };
        assert_eq!(keywords, &[] as &[String], "for every directory");
        let found = ["/srv/scripts/c", "/home/me/work/src", "/opt/www"]
            .iter()
            .map(|path| Scored {
                score: 1.0,
                path: PathBuf::from(path),
            })
            .collect();
        app.jumps(*generation, Ok(found));
        for c in "src".chars() {
            assert!(app.handle(Resolved::Insert(c)).is_empty(), "no query");
        }
        let text = screen_of(&mut app, 12);
        assert!(
            text.contains("~/work/src") && text.contains("/srv/scripts/c") && !text.contains("www"),
            "{text}"
        );
        let effects = without_zoxide(app.handle(action(Action::Confirm)));
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(
            request.location,
            local("/home/me/work/src"),
            "the best match"
        );
    }

    #[test]
    fn bang_runs_a_shell_command_in_the_panels_directory() {
        let mut app = loaded();
        assert!(app.handle(action(Action::Shell)).is_empty());
        assert_eq!(app.context(), Context::CommandLine);
        type_text(&mut app, "make");
        app.handle(action(Action::NewLine));
        type_text(&mut app, "ls");
        let text = screen_of(&mut app, 10);
        assert!(
            text.contains("/srv $ make") && text.contains("> ls"),
            "{text}"
        );
        let effects = without_zoxide(app.handle(action(Action::Confirm)));
        assert!(
            matches!(&effects[..], [Effect::History(HistoryChange::Add { .. })]),
            "{effects:?}"
        );
        assert_eq!(app.context(), Context::Panel);
        let Some(Run {
            place: Place::Local(dir),
            command,
        }) = app.take_run()
        else {
            panic!("expected a local command");
        };
        assert_eq!(
            (dir.as_path(), command.as_str()),
            (Path::new("/srv"), "make\nls")
        );
        assert!(app.take_run().is_none());
        let effects = app.ran(Ok(()));
        assert_eq!(effects.len(), 2, "both panels read /srv again");
        app.ran(Err("cannot run /bin/nosh".to_owned()));
        assert!(screen_of(&mut app, 12).contains("cannot run /bin/nosh"));
    }

    #[test]
    fn colon_needs_a_bang_and_the_root_has_no_command_line() {
        let mut app = loaded();
        app.handle(action(Action::Command));
        assert!(screen(&mut app).contains(':'));
        type_text(&mut app, "q");
        app.handle(action(Action::Confirm));
        assert!(screen_of(&mut app, 12).contains("Unknown command: q"));
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::CommandLine, "the line stays");
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);
        let mut root = at_root();
        root.handle(action(Action::Shell));
        assert_eq!(root.context(), Context::Root);
    }

    /// Runs `command` from the command line of the active panel.
    fn run(app: &mut App, command: &str) -> Vec<Effect> {
        app.handle(action(Action::Shell));
        type_text(app, command);
        let effects = without_zoxide(app.handle(action(Action::Confirm)));
        app.take_run();
        effects
    }

    #[test]
    fn commands_go_to_the_history_of_their_host_but_not_after_a_space() {
        let mut app = loaded();
        let effects = run(&mut app, "make");
        let [Effect::History(HistoryChange::Add { entry, size: 500 })] = &effects[..] else {
            panic!("expected the command to be kept, got {effects:?}");
        };
        assert_eq!(
            (
                entry.command.as_str(),
                entry.host.as_deref(),
                entry.dir.as_str()
            ),
            ("make", None, "/srv")
        );
        run(&mut app, "ls");
        assert!(run(&mut app, " secret").is_empty(), "a space keeps it out");
        // A command of another host does not come up here.
        app.history_changed(Ok(vec![
            entry.clone(),
            HistoryEntry {
                command: "uptime".to_owned(),
                host: Some("web".to_owned()),
                ..entry.clone()
            },
            HistoryEntry {
                command: "ls".to_owned(),
                ..entry.clone()
            },
        ]));
        app.handle(action(Action::Shell));
        type_text(&mut app, "dr");
        app.handle(action(Action::Up));
        assert_eq!(app.command_line.as_ref().map(CommandLine::text), Some("ls"));
        app.handle(action(Action::Up));
        assert_eq!(
            app.command_line.as_ref().map(CommandLine::text),
            Some("make")
        );
        app.handle(action(Action::Up));
        assert_eq!(
            app.command_line.as_ref().map(CommandLine::text),
            Some("make")
        );
        app.handle(action(Action::NewerCommand));
        app.handle(action(Action::NewerCommand));
        assert_eq!(app.command_line.as_ref().map(CommandLine::text), Some("dr"));
        app.handle(action(Action::Cancel));

        app.config.shell.history_size = 0;
        assert!(run(&mut app, "make").is_empty(), "no history at all");
        app.history_changed(Err("history.toml: invalid".to_owned()));
        assert!(screen_of(&mut app, 12).contains("Cannot keep the command history"));
    }

    #[test]
    fn alt_h_takes_a_command_from_the_history_window_and_delete_removes_one() {
        let mut app = loaded();
        let entry = |command: &str, host: Option<&str>| HistoryEntry {
            command: command.to_owned(),
            host: host.map(str::to_owned),
            dir: "/srv".to_owned(),
            time: 0,
        };
        app.history_changed(Ok(vec![entry("make", None), entry("uptime", Some("web"))]));
        app.handle(action(Action::CommandHistory));
        assert_eq!(
            app.context(),
            Context::History,
            "Alt-H opens the line and the window"
        );
        let text = screen_of(&mut app, 16);
        assert!(text.contains("make") && !text.contains("uptime"), "{text}");
        app.handle(action(Action::NextField));
        app.handle(action(Action::Home));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert_eq!(app.context(), Context::CommandLine);
        assert_eq!(
            app.command_line.as_ref().map(CommandLine::text),
            Some("uptime"),
            "it is put on the line, not run"
        );
        assert!(app.take_run().is_none());

        app.handle(action(Action::CommandHistory));
        let effects = app.handle(action(Action::Delete));
        let [
            Effect::History(HistoryChange::Remove {
                host: None,
                command,
            }),
        ] = &effects[..]
        else {
            panic!("expected a removal, got {effects:?}");
        };
        assert_eq!(command, "make");
        assert_eq!(app.history.len(), 1);
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::CommandLine);
    }

    #[test]
    fn pasted_text_never_runs_and_panels_ignore_it() {
        let mut app = loaded();
        assert!(app.paste("+*!\r").is_empty());
        assert_eq!(app.context(), Context::Panel, "no key of the panel ran");
        app.handle(action(Action::Shell));
        app.paste("make\nls\n");
        assert_eq!(app.context(), Context::CommandLine, "nothing ran");
        assert_eq!(
            app.command_line.as_ref().map(CommandLine::text),
            Some("make\nls\n")
        );
        app.handle(action(Action::Cancel));
        app.handle(action(Action::QuickSearch));
        app.paste("ri\nght");
        assert!(screen(&mut app).contains("Search: right"));
    }

    #[test]
    fn ctrl_x_ctrl_e_edits_the_command_and_brings_it_back() {
        let mut app = loaded();
        app.set_runtime_dir(PathBuf::from("/run/noc"));
        app.handle(action(Action::Shell));
        type_text(&mut app, "ls");
        assert!(app.handle(action(Action::EditCommand)).is_empty());
        let (file, text) = app.take_command_edit().unwrap();
        assert!(file.starts_with("/run/noc"), "{file:?}");
        assert_eq!(text, "ls");
        assert!(app.take_command_edit().is_none());
        app.command_edited(Ok("ls -l \\\n  /tmp\n".to_owned()));
        assert_eq!(
            app.command_line.as_ref().map(CommandLine::text),
            Some("ls -l \\\n  /tmp")
        );
        assert_eq!(app.context(), Context::CommandLine, "it does not run");
        app.command_edited(Err("Cannot edit the command: gone".to_owned()));
        assert!(screen_of(&mut app, 12).contains("Cannot edit the command: gone"));
    }

    #[test]
    fn the_prompt_shows_the_directory_with_a_tilde_for_home() {
        let (mut app, _) = App::new(
            Path::new("/home/me/src/noc"),
            Path::new("/home/me"),
            &config(),
        );
        app.handle(action(Action::Shell));
        assert_eq!(app.command_prompt(80), "~/src/noc $ ");
        assert_eq!(app.command_prompt(18), "~/s~oc $ ", "a third of the width");
        app.command_line = Some(CommandLine::new(command::Kind::Noc));
        assert_eq!(app.command_prompt(80), ":");
    }

    #[test]
    fn quick_search_is_fuzzy_unless_turned_off() {
        let mut app = loaded();
        app.handle(action(Action::QuickSearch));
        app.handle(Resolved::Insert('r'));
        app.handle(Resolved::Insert('t'));
        assert!(screen(&mut app).contains("Search: rt"));
        let Effect::List { request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        assert_eq!(request.location, local("/srv/right"));

        app.config.ui.fuzzy_search = false;
        app.handle(action(Action::QuickSearch));
        app.handle(Resolved::Insert('r'));
        app.handle(Resolved::Insert('t'));
        assert!(
            screen(&mut app).contains("Search: r") && !screen(&mut app).contains("Search: rt"),
            "names must start with the text"
        );
    }

    /// Types `text` into Quick cd in the active panel and presses Enter.
    fn quick_cd(app: &mut App, text: &str) -> Vec<Effect> {
        assert!(app.handle(action(Action::QuickCd)).is_empty());
        assert_eq!(app.context(), Context::PathInput);
        type_text(app, text);
        app.handle(action(Action::Confirm))
    }

    #[test]
    fn alt_c_goes_where_cd_would_and_back_with_a_dash() {
        let mut app = loaded();
        let text = {
            app.handle(action(Action::QuickCd));
            let text = screen_of(&mut app, 16);
            app.handle(action(Action::Cancel));
            text
        };
        assert!(text.contains("Quick cd") && text.contains("cd"), "{text}");

        let effects = quick_cd(&mut app, "left/../right/./x");
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/right/x"));
        answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
        assert_eq!(app.panel(Side::Left).location(), &local("/srv/right/x"));

        // `..` puts the cursor on where the panel was, as Ctrl-PgUp does.
        let effects = quick_cd(&mut app, "..");
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("w"), dir("x"), dir("y")]),
        );
        assert_eq!(app.panel(Side::Left).name_under_cursor(), Some(&b"x"[..]));

        // `-` goes back, and back again.
        let effects = quick_cd(&mut app, "-");
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/right/x"));
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        let effects = quick_cd(&mut app, "-");
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        assert_eq!(app.panel(Side::Left).location(), &local("/srv/right"));

        // `~`, an empty line, and where the panel is already.
        let [Effect::List { request, .. }] = &quick_cd(&mut app, "~/src")[..] else {
            panic!("expected a listing");
        };
        assert_eq!(request.location, local("/home/me/src"));
        app.panel_mut(Side::Left).cancel();
        assert!(quick_cd(&mut app, "").is_empty());
        assert!(quick_cd(&mut app, ".").is_empty());
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn alt_c_opens_hosts_and_a_path_that_is_not_there_says_so() {
        let mut app = loaded();
        let effects = quick_cd(&mut app, "web:/var/log");
        let [Effect::Connect { host, .. }] = &effects[..] else {
            panic!("expected a connection, got {effects:?}");
        };
        assert_eq!(host, "web");
        assert_eq!(
            app.panel(Side::Left).pending_request().map(|r| r.location),
            Some(remote("web", "/var/log"))
        );

        let mut app = loaded();
        let effects = quick_cd(&mut app, "/nowhere");
        let [Effect::List { panel, request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        app.listed(
            *panel,
            request.generation,
            Err("no such file or directory".to_owned()),
        );
        assert_eq!(app.panel(Side::Left).location(), &local("/srv"), "it stays");
        let text = screen(&mut app);
        assert!(text.contains("Cannot open /nowhere"), "{text}");
    }

    /// The listing that a Tab asks for: its generation, directory, host, and whether it wants
    /// the hosts of the ssh config.
    fn names_request(effects: &[Effect]) -> (u64, Location, bool, bool) {
        match effects {
            [
                Effect::ListNames {
                    generation,
                    dir: Some(dir),
                    host,
                    hosts,
                },
            ] => (*generation, dir.clone(), host.is_some(), *hosts),
            other => panic!("expected a listing of names, got {other:?}"),
        }
    }

    /// Entries as [`Done::Names`] has them.
    fn names_of(entries: &[(&str, bool)]) -> Vec<(Vec<u8>, bool)> {
        entries
            .iter()
            .map(|(name, dir)| (name.as_bytes().to_vec(), *dir))
            .collect()
    }

    fn field(app: &App) -> String {
        let (before, after) = app.dialogs.front().unwrap().dialog.around_cursor();
        format!("{before}|{after}")
    }

    #[test]
    fn tab_completes_quick_cd_from_the_directory_typed() {
        let mut app = loaded();
        app.handle(action(Action::QuickCd));
        type_text(&mut app, "left/pr");
        let (generation, dir, through, hosts) =
            names_request(&app.handle(action(Action::Complete)));
        assert_eq!((dir, through, hosts), (local("/srv/left"), false, false));
        let entries = [
            ("project-a", true),
            ("project-b", true),
            ("press.txt", false),
        ];
        app.names(generation, Ok(names_of(&entries)), &[]);
        assert_eq!(
            field(&app),
            "left/project-|",
            "as far as they agree; no files"
        );

        // A Tab that gets no further lists them; arrows choose, Enter takes one.
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.names(generation, Ok(names_of(&entries)), &[]);
        assert_eq!(app.context(), Context::Completion);
        let text = screen_of(&mut app, 20);
        assert!(
            text.contains("project-a/") && text.contains("project-b/"),
            "{text}"
        );
        app.handle(action(Action::Down));
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::PathInput);
        assert_eq!(field(&app), "left/project-b/|");

        // Typing closes the list and edits the field.
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.names(generation, Ok(names_of(&[("x", true), ("y", true)])), &[]);
        assert_eq!(app.context(), Context::Completion);
        app.handle(Resolved::Insert('x'));
        assert_eq!(app.context(), Context::PathInput);
        assert_eq!(field(&app), "left/project-b/x|");

        // A reply to text that changed since is dropped.
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.handle(action(Action::Backspace));
        app.names(generation, Ok(names_of(&[("xyz", true)])), &[]);
        assert_eq!(field(&app), "left/project-b/|");
    }

    #[test]
    fn quick_cd_completes_hosts_and_home() {
        let mut app = loaded();
        app.handle(action(Action::QuickCd));
        type_text(&mut app, "~");
        assert!(app.handle(action(Action::Complete)).is_empty());
        assert_eq!(field(&app), "~/|");
        app.handle(action(Action::DeleteToStart));
        type_text(&mut app, "we");
        let (generation, dir, _, hosts) = names_request(&app.handle(action(Action::Complete)));
        assert_eq!((dir, hosts), (local("/srv"), true));
        app.names(
            generation,
            Ok(names_of(&[("left", true)])),
            &["web".to_owned(), "db".to_owned()],
        );
        assert_eq!(field(&app), "web:|");
        // On a host that is not connected, there is nothing to read.
        type_text(&mut app, "/v");
        let (_, dir, through, hosts) = names_request(&app.handle(action(Action::Complete)));
        assert_eq!((dir, through, hosts), (remote("web", "/"), false, false));
    }

    #[test]
    fn f5_completes_files_and_connected_hosts_and_f7_takes_no_hosts() {
        // The left panel is on web, at /home/deploy, with its cursor on notes.
        let (mut app, _) = on_a_host();
        app.handle(action(Action::Copy));
        app.handle(action(Action::DeleteToStart));
        type_text(&mut app, "w");
        let (generation, _, _, hosts) = names_request(&app.handle(action(Action::Complete)));
        assert!(!hosts, "F5 knows the connected hosts itself");
        app.names(generation, Ok(names_of(&[("x", false)])), &[]);
        assert_eq!(field(&app), "web:|");
        type_text(&mut app, "no");
        let (generation, dir, through, _) = names_request(&app.handle(action(Action::Complete)));
        assert_eq!((dir, through), (remote("web", ""), true));
        app.names(generation, Ok(names_of(&[("notes", false)])), &[]);
        assert_eq!(field(&app), "web:notes|");
        app.handle(action(Action::Cancel));

        app.handle(action(Action::Mkdir));
        app.handle(action(Action::DeleteToStart));
        type_text(&mut app, "web:x/");
        let (_, dir, ..) = names_request(&app.handle(action(Action::Complete)));
        assert_eq!(
            dir,
            remote("web", "/home/deploy/web:x"),
            "F7 names no hosts"
        );
    }
}
