//! State and drawing of the whole screen.

use std::collections::{HashMap, HashSet, VecDeque};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use jiff::tz::TimeZone;
use noc_config::{
    Config, HistoryEntry, HostConfig, Hosts, MenuBar, PanelSide, Recent, SftpHost, TabBar,
    UiConfig, Wheel, Workspace, Workspaces,
};
use noc_ops::{Algorithm, Conflict, CopyOptions, Decision, Sum};
use noc_scroll::Motion;
use noc_scroll::scrollbar::{self, Part};
use noc_tools::zoxide::Scored;
use noc_vfs::{DirEntry, FileKind, Location, Metadata, RemotePath, Volume};
use noc_viewer::{
    Command as ViewerCommand, Part as ViewerPart, Progress as ReadProgress, Sizes as ViewerSizes,
    Styles as ViewerStyles, Taken, Viewer, Want,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::cd;
use super::cells::{self, Align};
use super::command::{self, CommandEvent, CommandLine, Place, Run};
use super::complete::{self, Candidate, Choices, ChoicesEvent, Kind, Offer, Outcome, Quote};
use super::configuration::{ConfigEvent, Configuration, run_of, run_text};
use super::decor::Decor;
use super::dialog::{Ask, Button, Dialog, DialogEvent, Field, Reply, Tone};
use super::edit::{Editor, Registers};
use super::find::{FindEvent, FindMode, FindQuery, FindSummary, FindWindow, Finding};
use super::glyphs::{self, Glyphs};
use super::help::{About, Help, Spot};
use super::history::{HistoryEvent, HistoryWindow};
use super::jobs::{JobsEvent, JobsList, Row};
use super::jump::{JumpEvent, JumpMenu};
use super::key_hints::{KeyHintsEvent, KeyHintsWindow};
use super::keymap::{
    self, Action, Context, Extra, Hints, KeyState, Keyboard, Keymap, Mode, Resolved, Typed,
};
use super::label::{self, Label};
use super::machine::{self, Machine};
use super::menu::{LocationMenu, MenuEvent};
use super::mouse::{Pointer, Press, Scroll};
use super::panel::{
    Destination, Failure, HostState, HostStatus, ListError, ListErrorKind, ListRequest, Listed,
    Panel, SearchEnd, View, child, home_text, location_text, stem_chars,
};
use super::picker::FULL_ROWS;
use super::places::{Places, volume_of};
use super::progress::{JobButton, JobView, Look, RowLook};
use super::pulldown::{self, Command, PullDown, PullDownEvent, Status};
use super::reading::{ReadButton, Reading};
use super::search::{self, Input, Slot, Switch};
use super::spinner::{self, Spin, Wait};
use super::sums::{Mark, SumRow, SumsButton, SumsEvent, SumsWindow, Verdict};
use super::tab_menu::{TabMenu, TabMenuEvent};
use super::tabs::{self, Bar, PanelId, Tab, Tabs};
use super::tasks::{HostHandle, JobEvent};
use super::theme::{ColorDepth, Theme};
use super::transfer::{self, Sources};
use super::which_key;
use super::workspaces::{self, WorkspacesEvent, WorkspacesWindow};
use super::zoom::{self, Grows, Stage};
use crate::about::{Build, Detected, Tool};
use crate::i18n::fl;
use crossterm::event::{KeyCode, KeyEvent};

/// One of the two panels, named by the side it starts on. Ctrl+w x swaps where the panels are
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

/// Whether the name under the cursor of the active panel shows whole below the panels too, as
/// `K` asks. The lists always show their row under the cursor whole.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Whole {
    /// Only cut, in its row.
    #[default]
    Cut,
    Shown,
}

impl Whole {
    fn toggled(self) -> Self {
        match self {
            Self::Cut => Self::Shown,
            Self::Shown => Self::Cut,
        }
    }
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
    /// Read the volumes and the hosts for the location menu and pass them to [`App::places`].
    ListPlaces { generation: u64 },
    /// Read a part of a file for the viewer, and report to [`App::read_progress`] and
    /// [`App::read`].
    Read(ReadRequest),
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
    /// Open the local files and directories, absolute paths, with the programs of the desktop,
    /// one after another, and report the first that fails to [`App::open_failed`].
    Open(Vec<PathBuf>),
    /// Ask a tool what it is, for the About page of the help, and pass it to
    /// [`App::detected`].
    Detect(Tool),
    /// Make the formats that `ui.formats` names, use them, and tell [`App::formats_changed`].
    Formats(String),
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
    /// Search the files under `location` with ripgrep, through `host` if it is remote, and
    /// pass what it finds to [`App::found`] as it comes, then how it ended to
    /// [`App::find_ended`]; `cancel` stops it.
    Find(FindRequest),
    /// List the commands in `dirs`, the directories of `$PATH`, whose names start with
    /// `prefix` regardless of case, for completion, and pass them to [`App::names`]; with
    /// `with_dirs`, the directories in them too.
    ListCommands {
        generation: u64,
        dirs: Vec<PathBuf>,
        prefix: String,
        with_dirs: bool,
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
    /// Read or write `recent.toml`, after the changes before, and report what it read to
    /// [`App::recent_loaded`].
    Recent(RecentChange),
}

/// The search of the files under `location` for `query`, through `host` if it is remote, after
/// `delay`, so that typing fast starts one search rather than many; it takes `max_results` at
/// most, until `cancel`.
#[derive(Debug)]
pub(crate) struct FindRequest {
    pub(crate) generation: u64,
    pub(crate) location: Location,
    pub(crate) host: Option<HostHandle>,
    pub(crate) query: FindQuery,
    pub(crate) max_results: usize,
    pub(crate) delay: Duration,
    pub(crate) cancel: CancellationToken,
}

/// The read of the part `want` of the file at `location` for the viewer `id`, through `host`
/// if it is remote; `stop` ends it with what it read, `cancel` drops it.
#[derive(Debug)]
pub(crate) struct ReadRequest {
    pub(crate) id: u64,
    pub(crate) location: Location,
    pub(crate) host: Option<HostHandle>,
    pub(crate) want: Want,
    pub(crate) stop: CancellationToken,
    pub(crate) cancel: CancellationToken,
}

/// What the event loop hands the terminal to, with the panels hidden.
#[derive(Debug)]
pub(crate) enum Handover {
    /// The editor of F4, on a local file, at `line` if there is one.
    Edit { file: PathBuf, line: Option<u64> },
    /// A shell command of the command line.
    Run(Run),
    /// Nothing: the terminal's own screen shows, with the output of commands, until `g <`;
    /// then whatever was in front is there again.
    UserScreen,
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

/// What to do with `recent.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecentChange {
    /// Read it.
    Load,
    /// Write these hosts and volumes in it.
    Save(Recent),
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
    /// The attempt `connection` failed, or the connection was lost, and a dialog asks whether
    /// to try again; the panels that wait for the host wait on.
    Stalled { connection: u64 },
}

impl Host {
    fn connection(&self) -> u64 {
        match self {
            Self::Connecting { connection, .. }
            | Self::Connected { connection, .. }
            | Self::Stalled { connection } => *connection,
        }
    }

    fn stop(&self) {
        match self {
            Self::Connecting { stop, .. } | Self::Connected { stop, .. } => stop.cancel(),
            Self::Stalled { .. } => {}
        }
    }
}

/// Most times a count repeats a motion, so that a long count cannot keep the app busy.
const MAX_REPEAT: usize = 10_000;
/// How long the search of a tree waits after a key before it starts again, so that typing a
/// word starts one search rather than one for each letter.
const FIND_DELAY: Duration = Duration::from_millis(150);
/// Cells of what a pending command has typed that the line above the F-key bar shows, as
/// vim's `showcmd` shows at most.
const SHOWCMD_WIDTH: usize = 10;
/// The fewest cells the prompt keeps for the directory before it leaves out the user, and
/// then the directory.
const PROMPT_DIR_WIDTH: usize = 8;
/// Width of the dialogs of `g +` and `g -`, as of mc's `+` and `-`.
const PATTERN_DIALOG_WIDTH: u16 = 50;
/// The fields of the dialog of F4 on a host.
const HOST_LABEL: usize = 0;
const HOST_START_DIR: usize = 1;
const HOST_OTHER_DIR: usize = 2;
const HOST_RUN: usize = 3;

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
    /// `g +` (`mark`) or `g -` in `panel`.
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
    /// F5, F6, or Shift+F5 (by `kind`) in `panel`, which shows `dir`, for the entries `names`.
    /// The field opened with `offered`, the text for the other panel's location, if it shows
    /// one.
    Transfer {
        kind: transfer::Kind,
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
    /// Go to directory in `panel`: OK opens the path typed there.
    GoToDir { panel: PanelId },
    /// A question about workspaces.
    Workspace(WorkspaceQuestion),
    /// Connecting to `host` failed for `reason`, or its connection was lost: Retry tries
    /// again for the panels that wait for it.
    Reconnect { host: String, reason: String },
    /// `panel` could not read a directory: Retry reads it again, Up goes to the nearest parent
    /// that is there, Go to opens Go to directory with its path; OK stays.
    ListFailed { panel: PanelId, failure: Failure },
    /// A key in `panel`, which is on a host that is not connected: Connect reads its directory
    /// again, Local goes to the last local directory of the panel.
    NotConnected { panel: PanelId },
    /// F10 while jobs run: Yes quits and stops them.
    Quit,
    /// Something to read, such as an error.
    Info,
}

/// What a dialog about workspaces is for.
#[derive(Debug)]
enum WorkspaceQuestion {
    /// t W, or a in the window: OK saves the tabs of both panels under the name typed.
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
    /// Every host of the ssh config, which Go to directory connects to.
    All,
    /// The hosts that are connected, as the targets of F5 and F6.
    Connected,
    /// None: F7 makes local or remote directories by name only.
    None,
}

/// Where a completion goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Completes {
    /// The path field of the dialog in front.
    Field,
    /// The command line of `!` and `:`.
    CommandLine,
}

/// A Tab in a path field or on the command line: what it waits for, then the list it shows.
#[derive(Debug)]
struct Completing {
    generation: u64,
    target: Completes,
    /// The text before the cursor at the Tab: a reply for other text is dropped, and Ctrl+e
    /// puts it back.
    before: String,
    /// What stays of it, before the part completed.
    keep: String,
    /// Where the path completed starts in it, for Left and Right in the list, which go up and
    /// down its directories.
    word: usize,
    /// The part completed, as names are matched: without the escapes of the command line.
    prefix: String,
    offer: Offer,
    quote: Quote,
    /// The hosts whose aliases complete the part too.
    hosts: FieldHosts,
    /// The hosts that were connected at the Tab.
    connected: Vec<String>,
    choices: Option<Choices>,
}

/// The list of completions of `completion` for `target`, if it shows.
fn choices_for(completion: &mut Option<Completing>, target: Completes) -> Option<&mut Choices> {
    completion
        .as_mut()
        .filter(|completing| completing.target == target)
        .and_then(|completing| completing.choices.as_mut())
}

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Delete,
    Transfer(transfer::Kind),
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
    /// The line the editor starts at, if not the first.
    line: Option<u64>,
}

/// How much the screen shows of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sight {
    /// Its window, in front.
    Window,
    /// Its row in the list of jobs, from the background.
    Row,
    /// Only its share of the corner, from the background.
    Corner,
}

/// What the screen shows of a job beyond its share of the corner, before it is put in words.
#[derive(Debug, PartialEq)]
enum Shown {
    Window(Look),
    Row(RowLook),
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
                Location::Local(_) => None,
            })
            .collect();
        let view = match kind {
            JobKind::Delete => JobView::new(fl!("delete-title"), fl!("delete-deleting")),
            JobKind::Transfer(kind) => JobView::new(kind.title(), kind.working()),
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

/// How many lines of rows the windows whose rows come later had the last time, if they came.
#[derive(Debug, Clone, Copy, Default)]
struct Sizes {
    /// Of the location menu, headings of groups too.
    menu: Option<u16>,
    /// Of the zoxide window.
    jump: Option<u16>,
}

/// The viewer on screen, the part it reads, and what stops it loading.
#[derive(Debug)]
struct Viewing {
    id: u64,
    viewer: Viewer,
    /// For the file, whose dots turn on its icon until it is read.
    wait: Wait,
    location: Location,
    host: Option<HostHandle>,
    /// The part being read, if any, with its window.
    reading: Option<Reading>,
    /// The window of the part read last, full for a moment.
    finished: Option<Reading>,
    cancel: CancellationToken,
}

impl Viewing {
    /// Draws the viewer over `area`, the icon of the file in front of its path: while the file
    /// is read, the dots turn in its place. A part that takes a while shows its window over it,
    /// with a gauge.
    fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        (theme, decor): (&Theme, Decor),
        spin: &Spin,
    ) {
        let loading = self.viewer.loading();
        let icon = spin
            .wait_dots(&self.wait)
            .filter(|_| loading)
            .unwrap_or_else(|| decor.file(&file_name(&self.location).unwrap_or_default()));
        self.viewer.render(frame, area, &viewer_styles(theme), icon);
        if self
            .finished
            .as_ref()
            .is_some_and(|finished| finished.gone(spin.at()))
        {
            self.finished = None;
        }
        // The window of the next part, once it shows, covers that of the last.
        for reading in [&self.finished, &self.reading].into_iter().flatten() {
            reading.render(frame, area, theme, spin);
        }
    }

    /// Whether the window of the part being read is on screen.
    fn window_shown(&self) -> bool {
        self.reading.as_ref().is_some_and(Reading::shown)
    }
}

/// How F5 and F6 copy and move: what their dialog asked for last, which it starts with, and
/// the setting.
#[derive(Debug, Clone, Copy)]
struct CopyChoices {
    preserve: bool,
    /// Through temporary names: `transfer.atomic_upload`.
    atomic: bool,
}

/// What `g +` and `g -` asked for last; their dialogs start with it, and with how the pattern
/// matched (`Slot::Mark`).
#[derive(Debug, Clone)]
struct PatternOptions {
    pattern: String,
    /// Leaves directories alone.
    files_only: bool,
}

impl Default for PatternOptions {
    /// As in mc: every name, directories too.
    fn default() -> Self {
        Self {
            pattern: "*".to_owned(),
            files_only: false,
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
    /// The panel that Ctrl+x took the keys from for a command of the other one, and gives them
    /// back to once it ends (ADR 0051).
    returns: Option<Side>,
    /// The right panel is drawn on the left.
    swapped: bool,
    /// Whether `K` shows the whole name under the cursor below the panels, until it is asked
    /// again; for this run only.
    whole: Whole,
    /// What the last quick search found, in any panel, for `n` and `N`, as vim keeps the last
    /// pattern; whether it went up the rows, as `?` does, which `n` then does too; and how it
    /// matched, as `n` matches too.
    last_search: Option<(String, bool, Input)>,
    hosts: HashMap<String, Host>,
    connections: u64,
    /// The settings, with `~` expanded; `ui.show_hidden` follows `o h`.
    config: Config,
    decor: Decor,
    theme: Theme,
    /// What the terminal can show, for themes in RGB.
    color_depth: ColorDepth,
    /// When the first frame was drawn, which spinners of one cell turn from.
    epoch: Wait,
    /// Hosts whose last attempt failed or whose connection was lost.
    failed: HashSet<String>,
    /// Addresses from `ssh -G` in this session.
    addresses: HashMap<String, String>,
    /// The users `ssh -G` gave for hosts in this session, terminal-safe.
    users: HashMap<String, String>,
    /// The machine Noon Commander runs on and who runs it; nothing until told.
    machine: Machine,
    /// The settings from `hosts.toml`.
    host_settings: Arc<Hosts>,
    /// The last directory shown on each host in this session, for `remember_dir`.
    last_dirs: HashMap<String, RemotePath>,
    /// The first one is on screen and gets the keys; the others wait, so that a new prompt
    /// never takes the keys from a dialog in use.
    dialogs: VecDeque<Open>,
    pattern_options: PatternOptions,
    /// How each field that searches was last, which it opens with again.
    searches: search::Memory,
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
    /// What the event loop is to hand the terminal to next.
    handover: Option<Handover>,
    /// The command line of `!` and `:`, above the F-key bar.
    command_line: Option<CommandLine>,
    /// The commands of the command line, oldest first, as `history.toml` held them last.
    history: Vec<HistoryEntry>,
    /// The window of the command history, over the panels and under the dialogs.
    history_window: Option<HistoryWindow>,
    /// The dialog of the search of a whole tree, over the panels and under the other dialogs.
    find: Option<FindWindow>,
    /// The generation of the last search of a tree.
    finds: u64,
    /// A message on the line of the mode until the next key, as vim's on its command line.
    message: Option<String>,
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
    /// Where the help was when it closed last, where it opens the next time.
    help_spot: Spot,
    /// What the About page of the help tells.
    about: About,
    /// The location menu of `g l`, over the panels and under the dialogs.
    menu: Option<LocationMenu>,
    /// The generation of the last listing for the menu.
    menu_listings: u64,
    /// The tab menu of `t t`, over the panels and under the dialogs.
    tab_menu: Option<TabMenu>,
    /// Tab completion in the dialog in front, or on the command line.
    completion: Option<Completing>,
    /// The generation of the last listing for completion.
    completions: u64,
    /// The directories of `$PATH`, whose commands the command line completes.
    command_dirs: Vec<PathBuf>,
    /// The zoxide window of `g z`, over the panels and under the dialogs.
    jump: Option<JumpMenu>,
    /// The generation of the last query for the zoxide window.
    jump_queries: u64,
    /// How many rows the windows whose rows come later had the last time, for the skeleton of
    /// the next, so that their boxes do not grow when the rows come. Not kept past a run.
    sizes: Sizes,
    /// The pull-down menu of F9, over the panels and under the windows and dialogs.
    pulldown: Option<PullDown>,
    /// The command of the pull-down menu that ran last, for Shift+F9 to open the menu on it.
    pulldown_last: Option<Command>,
    /// The Configuration dialog, over the panels and the menus, under the other windows and
    /// the dialogs.
    configuration: Option<Configuration>,
    /// The saved workspaces, as `workspaces.toml` held them last.
    workspaces: Workspaces,
    /// The workspace restored or saved last, which the dialog of t W offers.
    workspace: Option<String>,
    /// The window of the saved workspaces, over the panels and under the dialogs.
    workspaces_window: Option<WorkspacesWindow>,
    /// The home directory, the first row of the location menu.
    home: PathBuf,
    /// The hosts and volumes used last, as `recent.toml` holds them with those used since.
    recent: Recent,
    /// `recent` as `recent.toml` holds it, or will once it is written.
    recent_written: Recent,
    /// The volumes as read last, which tell which one a panel arrives on.
    volumes: Vec<Volume>,
    keymap: Keymap,
    /// The key hints, over everything at the bottom of the panels, as the event loop's key
    /// state has them.
    hints: Option<Hints>,
    /// The key hints were asked for, with every key of this mode and context.
    hints_asked: Option<keymap::Place>,
    /// The key hints with the keys, after Ctrl+k while they show: over everything, the dialogs
    /// too.
    key_hints: Option<KeyHintsWindow>,
    /// What the pending command has typed so far, as the event loop's key state has it, for
    /// vim's `showcmd`.
    showcmd: String,
    /// The count, the register, and the character that came with the key being handled.
    extra: Extra,
    /// What yanks and deletes keep, which every text field and the command line share.
    registers: Registers,
    /// Where the last render drew what the mouse can press.
    spots: Spots,
    /// What was in front after the last click, if that click left it there: a double click
    /// counts only then.
    clicked: Option<Front>,
    /// The thumb of a scroll bar that the left button holds: what its bar scrolls, and by
    /// which of its rows.
    grab: Option<(Scrolled, u16)>,
    /// The windows of the last frame, and those growing or shrinking.
    stage: Stage,
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

/// What a scroll bar scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scrolled {
    Help,
    Configuration,
    Viewer,
    Panel(Side),
    Menu,
    TabMenu,
    Jump,
    Workspaces,
    History,
    Find,
    KeyHints,
    PullDown,
    /// The list of completions.
    Choices,
    JobsList,
    /// The window of a checksum job's results.
    Results,
    CommandLine,
    /// The message of the dialog in front.
    Dialog,
}

/// What is in front, and takes the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    KeyHints,
    /// The dialog in front, by its id.
    Dialog(u64),
    Menu,
    TabMenu,
    Jump,
    Workspaces,
    History,
    Find,
    Results(usize),
    Job,
    JobsList,
    Help,
    Configuration,
    PullDown,
    /// The window of the part the viewer reads.
    Reading,
    Viewer,
    Panels,
}

impl App {
    /// Both panels on the local directory `start`, and the listings to request for them. The
    /// location menu opens `home`, which `~` in dialogs stands for too.
    pub(crate) fn new(start: &Path, home: &Path, config: &Config) -> (Self, Vec<Effect>) {
        let (ui, transfer) = (&config.ui, &config.transfer);
        let show_hidden = ui.show_hidden;
        let panel = || {
            let start = Location::Local(start.to_path_buf());
            let (mut panel, request) = Panel::new(start, home.to_path_buf(), show_hidden);
            panel.set_natural_sort(ui.natural_sort);
            (panel, request)
        };
        let (left, left_request) = panel();
        let (right, right_request) = panel();
        let [left_id, right_id] =
            [(Side::Left, 1), (Side::Right, 2)].map(|(side, tab)| PanelId { side, tab });
        let mut app = Self {
            left: Tabs::new(Tab::new(left_id, left)),
            right: Tabs::new(Tab::new(right_id, right)),
            last_tab: 2,
            active: Side::Left,
            returns: None,
            swapped: false,
            whole: Whole::Cut,
            last_search: None,
            hosts: HashMap::new(),
            connections: 0,
            config: config.clone(),
            decor: Decor::new(ui.icons),
            // `ui.theme` was checked when the config was loaded.
            theme: theme_of(ui, ColorDepth::TrueColor),
            color_depth: ColorDepth::TrueColor,
            epoch: Wait::default(),
            failed: HashSet::new(),
            addresses: HashMap::new(),
            users: HashMap::new(),
            machine: Machine::default(),
            host_settings: Arc::default(),
            last_dirs: HashMap::new(),
            dialogs: VecDeque::new(),
            pattern_options: PatternOptions::default(),
            searches: search::Memory::default(),
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
            handover: None,
            command_line: None,
            history: Vec::new(),
            history_window: None,
            find: None,
            finds: 0,
            message: None,
            work_dir: None,
            runtime_dir: std::env::temp_dir(),
            last_job: 0,
            parallel_jobs: transfer.parallel_jobs.get(),
            jobs_list: None,
            help: None,
            help_spot: Spot::default(),
            about: About::new(Build::current()),
            menu: None,
            menu_listings: 0,
            tab_menu: None,
            completion: None,
            completions: 0,
            command_dirs: Vec::new(),
            jump: None,
            jump_queries: 0,
            sizes: Sizes::default(),
            pulldown: None,
            pulldown_last: None,
            configuration: None,
            workspaces: Workspaces::default(),
            workspace: None,
            workspaces_window: None,
            home: home.to_path_buf(),
            recent: Recent::default(),
            recent_written: Recent::default(),
            volumes: Vec::new(),
            keymap: Keymap::default(),
            hints: None,
            hints_asked: None,
            key_hints: None,
            showcmd: String::new(),
            extra: Extra::default(),
            registers: Registers::default(),
            spots: Spots::default(),
            clicked: None,
            grab: None,
            stage: Stage::default(),
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

    /// What to hand the terminal to now, with the panels hidden; resets the request.
    pub(crate) fn take_handover(&mut self) -> Option<Handover> {
        let handover = self.handover.take();
        if handover.is_some() {
            // The windows were there before the terminal went; they come back as they were.
            self.stage.settle();
        }
        handover
    }

    /// Debug builds only: windows take as long to grow or shrink as `debug` says, and show their
    /// sizes if it says so.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_windows(&mut self, debug: super::DebugOptions) {
        if let Some(whole) = debug.slow_windows {
            self.stage.slow(whole);
        }
        self.stage.show_sizes(debug.show_sizes);
    }

    /// When the next frame of a window growing or shrinking is due, if one is.
    pub(crate) fn frame_due(&self) -> Option<Instant> {
        self.stage.due()
    }

    /// Takes pasted text: the command line gets all of it, line breaks included, and runs
    /// nothing; a text field gets it but its control characters, as one change in any mode;
    /// quick search and the filter of a list get its characters, a list in Normal mode typing
    /// in its filter first; panels and menus whose keys are commands take none of it.
    pub(crate) fn paste(&mut self, text: &str) -> Vec<Effect> {
        let typed = || text.chars().filter(|c| !c.is_control());
        match self.place() {
            (_, Context::CommandLine | Context::CommandCompletion) => {
                self.completion = None;
                if let Some(line) = &mut self.command_line {
                    line.paste(text, &mut self.registers);
                }
            }
            (_, Context::Rename) => {
                let mut registers = std::mem::take(&mut self.registers);
                if let Some(field) = self.panel_mut(self.active).rename_field() {
                    field.paste(text, &mut registers);
                }
                self.registers = registers;
            }
            (_, Context::DialogInput | Context::PathInput | Context::Completion) => {
                self.completion = None;
                match (self.dialogs.front_mut(), &mut self.configuration) {
                    (Some(open), _) => open.dialog.paste(text, &mut self.registers),
                    (None, Some(configuration)) => configuration.paste(text, &mut self.registers),
                    (None, None) => {}
                }
            }
            (_, Context::QuickSearch) => {
                return typed()
                    .flat_map(|c| self.handle(Resolved::Insert(c)))
                    .collect();
            }
            (_, Context::FindInput) => {
                // One search for all of it.
                if let Some(window) = &mut self.find
                    && window.paste(text, &mut self.registers) == FindEvent::Search
                {
                    return self.search_tree(FIND_DELAY);
                }
            }
            (
                mode,
                Context::LocationMenu
                | Context::TabMenu
                | Context::Jump
                | Context::Workspaces
                | Context::CommandHistory
                | Context::KeyHints,
            ) => {
                let mut effects = Vec::new();
                if mode == Mode::Normal {
                    effects.extend(self.handle(Resolved::Action(Action::QuickSearch)));
                }
                effects.extend(typed().flat_map(|c| self.handle(Resolved::Insert(c))));
                return effects;
            }
            _ => {}
        }
        Vec::new()
    }

    /// The directory of the active panel if it is local and new since the last call: the
    /// working directory of the process follows it. A remote panel leaves the working
    /// directory where it was.
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

    /// Tells `about` on the About page of the help.
    pub(crate) fn set_about(&mut self, about: About) {
        self.about = about;
    }

    /// Tells what `tool` is on the About page of the help, open or not.
    pub(crate) fn detected(&mut self, tool: Tool, detected: Detected) {
        self.about.report.set(tool, detected);
        self.refresh_help();
    }

    /// Draws the help again with what it shows now, where it was.
    fn refresh_help(&mut self) {
        if let Some(help) = &self.help {
            let spot = help.spot();
            self.help = Some(self.help_screen().at(spot));
        }
    }

    /// Uses `hosts`, the settings from `hosts.toml`.
    pub(crate) fn set_hosts(&mut self, hosts: Arc<Hosts>) {
        self.host_settings = hosts;
    }

    /// Completes the first word on the command line from the commands in `dirs`, the
    /// directories of `$PATH`.
    pub(crate) fn set_command_dirs(&mut self, dirs: Vec<PathBuf>) {
        self.command_dirs = dirs;
    }

    /// Draws in the colors that the terminal can show, `depth`.
    pub(crate) fn set_color_depth(&mut self, depth: ColorDepth) {
        self.color_depth = depth;
        self.theme = theme_of(&self.config.ui, depth);
    }

    /// Shows who runs Noon Commander where, `machine`, on the line of the mode and in the
    /// prompt.
    pub(crate) fn set_machine(&mut self, machine: Machine) {
        self.machine = machine;
    }

    /// Shows times in `tz`.
    pub(crate) fn set_time_zone(&mut self, tz: TimeZone) {
        self.tz = tz;
    }

    /// Reads keys as a terminal that sends them as `keyboard` says.
    pub(crate) fn set_keyboard(&mut self, keyboard: Keyboard) {
        self.keymap = Keymap::new(keyboard);
    }

    /// What turns keys into actions.
    pub(crate) fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// How long after a key that starts longer sequences the keys that can follow it show:
    /// `ui.which_key_delay_ms`; `None` while `ui.which_key` is off.
    pub(crate) fn hints_delay(&self) -> Option<Duration> {
        let ui = &self.config.ui;
        ui.which_key
            .then(|| Duration::from_millis(u64::from(ui.which_key_delay_ms)))
    }

    /// The mode and the context whose every key the key hints were asked for, since the last
    /// call.
    pub(crate) fn take_hints_asked(&mut self) -> Option<keymap::Place> {
        self.hints_asked.take()
    }

    /// Shows the key hints that `keys` has, if any, and what its pending command has typed,
    /// from the next render.
    pub(crate) fn set_hints(&mut self, keys: &KeyState) {
        self.hints = self.keymap.hints_of(keys, self.key_place());
        // Keys typed where the keys no longer go, as under a dialog that opened, are forgotten
        // at the next key.
        self.showcmd = if keys.place() == Some(self.key_place()) {
            keys.showcmd()
        } else {
            String::new()
        };
    }

    /// Whether the mouse is on: `ui.mouse`.
    pub(crate) fn mouse(&self) -> bool {
        self.config.ui.mouse
    }

    /// Takes a press of the mouse at a cell, as the last render drew the screen. What is in
    /// front takes it, as it takes keys.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Vec<Effect> {
        let effects = self.pointer_input(pointer);
        self.remember_searches();
        effects
    }

    fn pointer_input(&mut self, pointer: Pointer) -> Vec<Effect> {
        if !self.config.ui.mouse {
            return Vec::new();
        }
        if pointer.press == Press::Drag {
            return self.drag(pointer.at);
        }
        // Any other press lets go of the thumb, as the button that held it went up before.
        self.grab = None;
        let front = self.front();
        // A first click that closed a menu or a dialog, or opened one, leaves the second click
        // of a double click nothing to do.
        if pointer.press == Press::DoubleClick && self.clicked != Some(front) {
            return Vec::new();
        }
        let effects = self.route_pointer(pointer);
        self.clicked = (pointer.press == Press::Click && self.front() == front).then_some(front);
        self.settle_visual();
        effects
    }

    /// What is in front, in the order [`handle_over`](Self::handle_over) gives keys.
    fn front(&self) -> Front {
        if self.key_hints.is_some() {
            Front::KeyHints
        } else if let Some(open) = self.dialogs.front() {
            Front::Dialog(open.dialog.id())
        } else if self.menu.is_some() {
            Front::Menu
        } else if self.tab_menu.is_some() {
            Front::TabMenu
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
        } else if self.viewing.as_ref().is_some_and(Viewing::window_shown) {
            Front::Reading
        } else if self.viewing.is_some() {
            Front::Viewer
        } else if self.find.is_some() {
            Front::Find
        } else {
            Front::Panels
        }
    }

    /// The F-key bar presses its keys, and the scroll bar of what is in front its parts; then
    /// what is in front takes the press: a dialog, the pull-down menu, the viewer, the menu
    /// bar that `ui.menu_bar` keeps, or the panels, where a click moves the cursor, a double
    /// click opens, a right click marks, and the wheel scrolls.
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
        if let Some(effects) = self.pointer_scroll_bar(pointer) {
            return effects;
        }
        match self.front() {
            Front::Dialog(_) => return self.pointer_dialog(pointer),
            Front::PullDown => return self.pointer_pulldown(pointer),
            Front::Viewer => return self.wheel_viewer(press),
            Front::Panels => {}
            _ => return self.pointer_window(pointer),
        }
        if self.command_line.is_some() {
            return self.pointer_command_line(pointer);
        }
        if self.panel(self.active).renaming() {
            return Vec::new();
        }
        // A click on a glyph of quick search switches how it matches, or whether case counts,
        // as the keys after Ctrl+s do.
        if let Some(action) = self.panel(self.active).search_action_at(at) {
            if press == Press::Click {
                return self.handle(Resolved::Action(action));
            }
            return Vec::new();
        }
        if let Some(index) = self
            .spots
            .menu_bar
            .iter()
            .position(|title| title.contains(at))
        {
            if press == Press::Click {
                self.open_pulldown(false);
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
            Press::Drag => Vec::new(),
        }
    }

    /// The key that a step of the wheel stands for in windows and lists, and how many times:
    /// Up or Down by `ui.wheel` lines, or a page up or down.
    fn wheel_keys(&self, press: Press) -> Option<(Action, u8)> {
        let down = match press {
            Press::WheelUp => false,
            Press::WheelDown => true,
            Press::Click | Press::DoubleClick | Press::RightClick | Press::Drag => return None,
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
    /// as Up and Down do, or scrolls the help; but not in a job's window or the viewer's
    /// reading, where they move between the buttons. A click does what the window says, often by the key it stands
    /// for.
    fn pointer_window(&mut self, pointer: Pointer) -> Vec<Effect> {
        let front = self.front();
        if let Some(keys) = self.wheel_keys(pointer.press) {
            return match (front, &mut self.help) {
                (Front::Job | Front::Reading, _) => Vec::new(),
                // The wheel scrolls the rows of the search, wherever the focus is.
                (Front::Find, _) => {
                    if let Some(window) = &mut self.find {
                        window.wheel(keys.0, usize::from(keys.1));
                    }
                    Vec::new()
                }
                // The wheel scrolls the help's text, wherever its keys go.
                (Front::Help, Some(help)) => {
                    let (action, count) = keys;
                    for _ in 0..count {
                        help.wheel(action);
                    }
                    Vec::new()
                }
                _ => self.wheel(keys),
            };
        }
        let action = match front {
            Front::KeyHints => self
                .key_hints
                .as_mut()
                .and_then(|window| window.pointer(pointer)),
            Front::Menu => self.menu.as_mut().and_then(|menu| menu.pointer(pointer)),
            Front::TabMenu => self
                .tab_menu
                .as_mut()
                .and_then(|menu| menu.pointer(pointer)),
            Front::Jump => self.jump.as_mut().and_then(|jump| jump.pointer(pointer)),
            Front::Workspaces => self
                .workspaces_window
                .as_mut()
                .and_then(|window| window.pointer(pointer)),
            Front::History => self
                .history_window
                .as_mut()
                .and_then(|window| window.pointer(pointer)),
            Front::Find => {
                let Some(window) = &mut self.find else {
                    return Vec::new();
                };
                let event = window.pointer(pointer);
                return self.find_event(event);
            }
            Front::Results(_) => self
                .results
                .front_mut()
                .and_then(|results| results.window.pointer(pointer)),
            Front::Job => self
                .jobs
                .iter_mut()
                .find(|job| !job.background)
                .and_then(|job| job.view.pointer(pointer)),
            Front::Reading => self
                .viewing
                .as_mut()
                .and_then(|viewing| viewing.reading.as_mut())
                .and_then(|reading| reading.pointer(pointer)),
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
            Front::Help => {
                if let Some(help) = &mut self.help {
                    help.pointer(pointer);
                }
                None
            }
            Front::Dialog(_) | Front::PullDown | Front::Viewer | Front::Panels => None,
        };
        action.map_or_else(Vec::new, |action| self.handle(Resolved::Action(action)))
    }

    /// Scrolls the viewer by a step of the wheel, if `press` is one, and reads the part it
    /// scrolls to.
    fn wheel_viewer(&mut self, press: Press) -> Vec<Effect> {
        let down = match press {
            Press::WheelUp => false,
            Press::WheelDown => true,
            Press::Click | Press::DoubleClick | Press::RightClick | Press::Drag => {
                return Vec::new();
            }
        };
        let (motion, count) = self.wheel_motion(down);
        let Some(viewing) = &mut self.viewing else {
            return Vec::new();
        };
        for _ in 0..count {
            viewing.viewer.handle(ViewerCommand::Scroll(motion));
        }
        self.read_more()
    }

    /// How far a step of the wheel up or down scrolls text, and how many times: by `ui.wheel`
    /// lines, or a page.
    fn wheel_motion(&self, down: bool) -> (Motion, u8) {
        match (self.config.ui.wheel, down) {
            (Wheel::Lines(count), false) => (Motion::Up, count),
            (Wheel::Lines(count), true) => (Motion::Down, count),
            (Wheel::Page, false) => (Motion::PageUp, 1),
            (Wheel::Page, true) => (Motion::PageDown, 1),
        }
    }

    /// The scroll bars of what is in front, as they were last drawn, with what each scrolls:
    /// the list of completions first, which shows over its field.
    fn scroll_bars(&self) -> Vec<(Scrolled, scrollbar::Bar)> {
        let choices = |target: Completes| {
            let completing = self.completion.as_ref()?;
            let choices = completing.choices.as_ref()?;
            let bar = choices.bar().filter(|_| completing.target == target)?;
            Some((Scrolled::Choices, bar))
        };
        let one = |target: Scrolled, bar: Option<scrollbar::Bar>| bar.map(|bar| (target, bar));
        let bars = match self.front() {
            Front::KeyHints => {
                let bar = self.key_hints.as_ref().and_then(KeyHintsWindow::bar);
                vec![one(Scrolled::KeyHints, bar)]
            }
            Front::Dialog(_) => {
                let dialog = self.dialogs.front().and_then(|open| open.dialog.bar());
                vec![choices(Completes::Field), one(Scrolled::Dialog, dialog)]
            }
            Front::Menu => vec![one(
                Scrolled::Menu,
                self.menu.as_ref().and_then(LocationMenu::bar),
            )],
            Front::TabMenu => {
                let bar = self.tab_menu.as_ref().and_then(TabMenu::bar);
                vec![one(Scrolled::TabMenu, bar)]
            }
            Front::Jump => vec![one(
                Scrolled::Jump,
                self.jump.as_ref().and_then(JumpMenu::bar),
            )],
            Front::Workspaces => {
                let bar = self
                    .workspaces_window
                    .as_ref()
                    .and_then(WorkspacesWindow::bar);
                vec![one(Scrolled::Workspaces, bar)]
            }
            Front::History => {
                let bar = self.history_window.as_ref().and_then(HistoryWindow::bar);
                vec![one(Scrolled::History, bar)]
            }
            Front::Find => vec![one(
                Scrolled::Find,
                self.find.as_ref().and_then(FindWindow::bar),
            )],
            Front::Results(_) => {
                let bar = self
                    .results
                    .front()
                    .and_then(|results| results.window.bar());
                vec![one(Scrolled::Results, bar)]
            }
            Front::Job | Front::Reading => Vec::new(),
            Front::JobsList => {
                let bar = self.jobs_list.as_ref().and_then(JobsList::bar);
                vec![one(Scrolled::JobsList, bar)]
            }
            Front::Help => vec![one(Scrolled::Help, self.help.as_ref().and_then(Help::bar))],
            Front::Configuration => {
                let bar = self.configuration.as_ref().and_then(Configuration::bar);
                vec![one(Scrolled::Configuration, bar)]
            }
            Front::PullDown => {
                let bar = self.pulldown.as_ref().and_then(PullDown::bar);
                vec![one(Scrolled::PullDown, bar)]
            }
            Front::Viewer => {
                let bar = self
                    .viewing
                    .as_ref()
                    .and_then(|viewing| viewing.viewer.bar());
                vec![one(Scrolled::Viewer, bar)]
            }
            Front::Panels if self.command_line.is_some() => {
                let line = self.command_line.as_ref().and_then(CommandLine::bar);
                vec![
                    choices(Completes::CommandLine),
                    one(Scrolled::CommandLine, line),
                ]
            }
            Front::Panels if self.panel(self.active).renaming() => Vec::new(),
            Front::Panels => [Side::Left, Side::Right]
                .into_iter()
                .map(|side| one(Scrolled::Panel(side), self.panel(side).bar()))
                .collect(),
        };
        bars.into_iter().flatten().collect()
    }

    /// Scrolls `target` as a press on its scroll bar asks. The lists that the wheel moves by
    /// their keys take the keys of the arrows and the track too.
    fn scroll_target(&mut self, target: Scrolled, scroll: Scroll) -> Vec<Effect> {
        let keys = matches!(
            target,
            Scrolled::Menu
                | Scrolled::TabMenu
                | Scrolled::Jump
                | Scrolled::Workspaces
                | Scrolled::History
                | Scrolled::Find
                | Scrolled::KeyHints
                | Scrolled::Choices
        );
        if let (true, Scroll::By(motion)) = (keys, scroll) {
            let action = match motion {
                Motion::Up => Action::Up,
                Motion::Down => Action::Down,
                Motion::PageUp => Action::PageUp,
                Motion::PageDown => Action::PageDown,
                _ => return Vec::new(),
            };
            return self.handle(Resolved::Action(action));
        }
        if let (true, Scroll::To(top)) = (keys, scroll) {
            self.scroll_list_to(target, top);
            return Vec::new();
        }
        match target {
            Scrolled::Help => {
                if let Some(help) = &mut self.help {
                    help.scroll(scroll);
                }
            }
            Scrolled::Configuration => {
                if let Some(configuration) = &mut self.configuration {
                    let event = configuration.scroll(scroll);
                    return self.configuration_event(event);
                }
            }
            Scrolled::Viewer => {
                if let Some(viewing) = &mut self.viewing {
                    viewing.viewer.handle(match scroll {
                        Scroll::By(motion) => ViewerCommand::Scroll(motion),
                        Scroll::To(at) => ViewerCommand::Bar(at),
                    });
                    return self.read_more();
                }
            }
            Scrolled::Panel(side) => self.panel_mut(side).scroll_bar(scroll),
            Scrolled::PullDown => {
                if let Some(mut pulldown) = self.pulldown.take() {
                    pulldown.scroll(scroll, &|command| self.command_status(command));
                    self.pulldown = Some(pulldown);
                }
            }
            Scrolled::JobsList => {
                let rows = self.job_rows();
                if let Some(list) = &mut self.jobs_list {
                    list.scroll(scroll, &rows);
                }
            }
            Scrolled::Results => {
                if let Some(results) = self.results.front_mut() {
                    results.window.scroll(scroll);
                }
            }
            Scrolled::CommandLine => {
                if let Some(line) = &mut self.command_line {
                    line.scroll(scroll);
                }
            }
            Scrolled::Dialog => {
                if let Some(open) = self.dialogs.front_mut() {
                    open.dialog.scroll(scroll);
                }
            }
            Scrolled::Menu
            | Scrolled::TabMenu
            | Scrolled::Jump
            | Scrolled::Workspaces
            | Scrolled::History
            | Scrolled::Find
            | Scrolled::KeyHints
            | Scrolled::Choices => {}
        }
        Vec::new()
    }

    /// Shows the rows of the list `target` from `top`, where the thumb of its scroll bar was
    /// dragged.
    fn scroll_list_to(&mut self, target: Scrolled, top: usize) {
        match target {
            Scrolled::Menu => {
                if let Some(menu) = &mut self.menu {
                    menu.scroll_to(top);
                }
            }
            Scrolled::TabMenu => {
                if let Some(menu) = &mut self.tab_menu {
                    menu.scroll_to(top);
                }
            }
            Scrolled::Jump => {
                if let Some(jump) = &mut self.jump {
                    jump.scroll_to(top);
                }
            }
            Scrolled::Workspaces => {
                if let Some(window) = &mut self.workspaces_window {
                    window.scroll_to(top);
                }
            }
            Scrolled::History => {
                if let Some(window) = &mut self.history_window {
                    window.scroll_to(top);
                }
            }
            Scrolled::Find => {
                if let Some(window) = &mut self.find {
                    window.scroll_to(top);
                }
            }
            Scrolled::KeyHints => {
                if let Some(window) = &mut self.key_hints {
                    window.scroll_to(top);
                }
            }
            Scrolled::Choices => {
                let target = self.completion.as_ref().map(|completing| completing.target);
                if let Some(choices) =
                    target.and_then(|target| choices_for(&mut self.completion, target))
                {
                    choices.scroll_to(top);
                }
            }
            _ => {}
        }
    }

    /// Takes a click on a scroll bar of what is in front, if it is on one: an arrow scrolls as
    /// a step of the wheel, the track a page towards the click, as in Far, and the thumb is
    /// held for dragging. The second click of a double click scrolls again.
    fn pointer_scroll_bar(&mut self, Pointer { press, at }: Pointer) -> Option<Vec<Effect>> {
        if !matches!(press, Press::Click | Press::DoubleClick) {
            return None;
        }
        let (target, bar) = self
            .scroll_bars()
            .into_iter()
            .find(|(_, bar)| bar.area().contains(at))?;
        let (motion, count) = match bar.part(at)? {
            Part::Up => self.wheel_motion(false),
            Part::Down => self.wheel_motion(true),
            Part::PageUp => (Motion::PageUp, 1),
            Part::PageDown => (Motion::PageDown, 1),
            Part::Thumb(row) => {
                self.grab = Some((target, row));
                return Some(Vec::new());
            }
        };
        let mut effects = Vec::new();
        for _ in 0..count {
            effects.extend(self.scroll_target(target, Scroll::By(motion)));
        }
        Some(effects)
    }

    /// Whether the left button holds the thumb of a scroll bar, which its drags move.
    pub(crate) fn dragging(&self) -> bool {
        self.grab.is_some()
    }

    /// Scrolls what holds the thumb to where it was dragged, at the row of `at`; nothing if no
    /// thumb is held, or its bar has gone.
    fn drag(&mut self, at: Position) -> Vec<Effect> {
        let Some((target, row)) = self.grab else {
            return Vec::new();
        };
        let bar = self
            .scroll_bars()
            .into_iter()
            .find_map(|(scrolled, bar)| (scrolled == target).then_some(bar));
        let Some(bar) = bar else {
            self.grab = None;
            return Vec::new();
        };
        self.scroll_target(target, Scroll::To(bar.dragged(row, at.y)))
    }

    /// Gives a press of the mouse to the dialog in front, and does what it was for if it
    /// closes; or to the list of completions under its field, which goes at a press outside
    /// it. The wheel scrolls a message that does not fit.
    fn pointer_dialog(&mut self, pointer: Pointer) -> Vec<Effect> {
        if let Some(effects) = self.pointer_choices(Completes::Field, pointer) {
            return effects;
        }
        self.completion = None;
        let down = match pointer.press {
            Press::WheelUp => Some(false),
            Press::WheelDown => Some(true),
            Press::Click | Press::DoubleClick | Press::RightClick | Press::Drag => None,
        };
        let (motion, count) = self.wheel_motion(down == Some(true));
        let Some(open) = self.dialogs.front_mut() else {
            return Vec::new();
        };
        if down.is_some() && open.dialog.bar().is_some() {
            for _ in 0..count {
                open.dialog.scroll(Scroll::By(motion));
            }
            return Vec::new();
        }
        let event = open.dialog.pointer(pointer);
        self.dialog_event(event)
    }

    /// Gives a press of the mouse to the list of completions over the command line, which goes
    /// at a press outside it; the line itself takes none.
    fn pointer_command_line(&mut self, pointer: Pointer) -> Vec<Effect> {
        if let Some(effects) = self.pointer_choices(Completes::CommandLine, pointer) {
            return effects;
        }
        if choices_for(&mut self.completion, Completes::CommandLine).is_some() {
            self.completion = None;
        }
        Vec::new()
    }

    /// Gives a press of the mouse to the list of completions for `target`, if it shows and the
    /// press is on it: the wheel moves in it as Up and Down do, a click puts a match in, and a
    /// double click keeps it, as Ctrl+y. `None` for a press elsewhere.
    fn pointer_choices(&mut self, target: Completes, pointer: Pointer) -> Option<Vec<Effect>> {
        let wheel = self.wheel_keys(pointer.press);
        let choices = choices_for(&mut self.completion, target)?;
        if !choices.contains(pointer.at) {
            return None;
        }
        if let Some(keys) = wheel {
            return Some(self.wheel(keys));
        }
        if let Some(event) = choices.pointer(pointer) {
            self.take_choices(event);
        }
        Some(Vec::new())
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
        self.end_search(self.active);
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
        if self.key_hints.is_some() {
            Context::KeyHints
        } else if let Some(open) = self.dialogs.front() {
            if self.lists(Completes::Field) && open.dialog.completes() {
                Context::Completion
            } else {
                open.dialog.context()
            }
        } else if self.menu.is_some() {
            Context::LocationMenu
        } else if self.tab_menu.is_some() {
            Context::TabMenu
        } else if self.jump.is_some() {
            Context::Jump
        } else if self.workspaces_window.is_some() {
            Context::Workspaces
        } else if self.history_window.is_some() {
            Context::CommandHistory
        } else if !self.results.is_empty() || self.in_front().is_some() || self.jobs_list.is_some()
        {
            Context::Dialog
        } else if self.help.is_some() {
            Context::Help
        } else if let Some(configuration) = &self.configuration {
            configuration.context()
        } else if self.pulldown.is_some() {
            Context::PullDown
        } else if self.viewing.as_ref().is_some_and(Viewing::window_shown) {
            Context::Dialog
        } else if self.viewing.is_some() {
            Context::Viewer
        } else if let Some(window) = &self.find {
            window.context()
        } else if self.command_line.is_some() {
            if self.lists(Completes::CommandLine) {
                Context::CommandCompletion
            } else {
                Context::CommandLine
            }
        } else if self.panel(self.active).renaming() {
            Context::Rename
        } else if self.panel(self.active).searching() {
            Context::QuickSearch
        } else {
            Context::Panel
        }
    }

    /// Whether the list of completions for `target` shows.
    fn lists(&self, target: Completes) -> bool {
        self.completion
            .as_ref()
            .is_some_and(|completing| completing.target == target && completing.choices.is_some())
    }

    /// The mode keys are typed in now, as vim's: that of the text field that has them, Visual
    /// mode while a panel selects rows, and else Normal mode.
    pub(crate) fn mode(&self) -> Mode {
        if let Some(window) = &self.key_hints {
            window.mode()
        } else if let Some(open) = self.dialogs.front() {
            if self.context() == Context::Completion {
                Mode::Insert
            } else {
                open.dialog.mode()
            }
        } else if let Some(menu) = &self.menu {
            menu.mode()
        } else if let Some(menu) = &self.tab_menu {
            menu.mode()
        } else if let Some(jump) = &self.jump {
            jump.mode()
        } else if let Some(window) = &self.workspaces_window {
            window.mode()
        } else if let Some(window) = &self.history_window {
            window.mode()
        } else if !self.results.is_empty()
            || self.in_front().is_some()
            || self.jobs_list.is_some()
            || self.help.is_some()
        {
            Mode::Normal
        } else if let Some(configuration) = &self.configuration {
            configuration.mode()
        } else if self.pulldown.is_some() || self.viewing.is_some() {
            Mode::Normal
        } else if let Some(window) = &self.find {
            window.mode()
        } else if let Some(line) = &self.command_line {
            line.mode()
        } else {
            let panel = self.panel(self.active);
            if let Some(field) = panel.rename_field_ref() {
                field.mode()
            } else if panel.searching() {
                Mode::Insert
            } else if panel.visual() {
                Mode::Visual
            } else {
                Mode::Normal
            }
        }
    }

    /// The mode and the context of the next key.
    pub(crate) fn place(&self) -> (Mode, Context) {
        (self.mode(), self.context())
    }

    /// Where the next key goes, for the keymap: the mode, the context, and the kind of the field
    /// that has the keys if it searches (ADR 0045, ADR 0046).
    pub(crate) fn key_place(&self) -> keymap::Place {
        let (mode, context) = self.place();
        keymap::Place {
            mode,
            context,
            search: self.search_focused(),
        }
    }

    /// The kind of the field that has the keys, if it searches: quick search, the filter of a
    /// list while it takes text, or such a field of a dialog.
    fn search_focused(&self) -> Option<search::Kind> {
        match self.context() {
            Context::QuickSearch => self.panel(self.active).search_kind(),
            Context::KeyHints => self.key_hints.as_ref()?.search(),
            Context::LocationMenu => self.menu.as_ref()?.search(),
            Context::TabMenu => self.tab_menu.as_ref()?.search(),
            Context::Jump => self.jump.as_ref()?.search(),
            Context::Workspaces => self.workspaces_window.as_ref()?.search(),
            Context::CommandHistory => self.history_window.as_ref()?.search(),
            Context::DialogInput | Context::PathInput => {
                self.dialogs.front()?.dialog.search_focused()
            }
            Context::FindInput => self.find.as_ref()?.search_focused(),
            _ => None,
        }
    }

    /// The operator that a text field with the keys has waiting for its motion, as `showcmd`
    /// shows it.
    /// The editor of the text field that has the keys, if one does.
    fn focused_editor(&self) -> Option<&Editor> {
        let field = match self.place().1 {
            Context::DialogInput | Context::PathInput | Context::Completion => {
                match (self.dialogs.front(), &self.configuration) {
                    (Some(open), _) => open.dialog.focused_field(),
                    (None, Some(configuration)) => configuration.field(),
                    (None, None) => None,
                }
            }
            Context::Rename => self.panel(self.active).rename_field_ref(),
            Context::FindInput => self.find.as_ref().and_then(FindWindow::focused_field),
            Context::CommandLine | Context::CommandCompletion => {
                return self.command_line.as_ref().map(CommandLine::editor);
            }
            _ => None,
        };
        field.map(Field::editor)
    }

    fn pending_operator(&self) -> Option<String> {
        if self.key_hints.is_some() {
            return None;
        }
        if let Some(open) = self.dialogs.front() {
            return open.dialog.pending();
        }
        if let Some(configuration) = &self.configuration {
            return configuration.pending();
        }
        if self.front() == Front::Find {
            return self.find.as_ref().and_then(FindWindow::pending);
        }
        if let Some(line) = &self.command_line {
            return line.pending();
        }
        self.panel(self.active)
            .rename_field_ref()
            .and_then(Field::pending)
    }

    /// Takes a key press as the event loop gets it: to the keymap, in the mode and the context
    /// of the app, and what it resolves to, to the app. Alt and a key that stand for Esc and the
    /// key go as both, each in the mode and the context it finds, as Esc may leave Insert mode;
    /// on a password they do nothing, as the Esc would close its dialog and send the key to the
    /// panels.
    pub(crate) fn key(&mut self, keys: &mut KeyState, key: KeyEvent, now: Instant) -> Vec<Effect> {
        self.message = None;
        let split = if self.typing_secret() {
            None
        } else {
            self.keymap.alt_as_esc(keys, self.key_place(), key)
        };
        let (first, then) = match split {
            Some(plain) => (KeyEvent::from(KeyCode::Esc), Some(plain)),
            None => (key, None),
        };
        let mut effects = self.feed(keys, first, now);
        if let Some(plain) = then {
            effects.extend(self.feed(keys, plain, now));
        }
        effects
    }

    /// Settles a key sequence of `keys` whose time is up, as [`key`](Self::key) takes a key.
    pub(crate) fn expire_keys(&mut self, keys: &mut KeyState, now: Instant) -> Vec<Effect> {
        let typed = self.keymap.expire(keys, now);
        self.take_typed(keys, typed)
    }

    fn feed(&mut self, keys: &mut KeyState, key: KeyEvent, now: Instant) -> Vec<Effect> {
        keys.set_operator(self.pending_operator().is_some());
        let typed = self.keymap.feed(keys, self.key_place(), key, now);
        self.take_typed(keys, typed)
    }

    /// Takes what keys resolved to, after giving up the part of a command that the keymap gave
    /// up; then lets Visual mode go in the panels that no longer have the keys.
    fn take_typed(&mut self, keys: &mut KeyState, typed: Vec<Typed>) -> Vec<Effect> {
        if keys.take_dropped() {
            self.give_up_command();
        }
        let mut effects = Vec::new();
        for typed in typed {
            effects.extend(self.typed(typed));
        }
        self.settle_visual();
        effects
    }

    /// Whether keys go to the filter of a list, in Insert mode.
    fn filtering(&self) -> bool {
        matches!(
            self.place(),
            (
                Mode::Insert,
                Context::LocationMenu
                    | Context::TabMenu
                    | Context::Jump
                    | Context::Workspaces
                    | Context::CommandHistory
                    | Context::KeyHints
            )
        )
    }

    /// Whether keys go to a secret field, such as the password of a prompt from ssh.
    fn typing_secret(&self) -> bool {
        self.key_hints.is_none()
            && self
                .dialogs
                .front()
                .is_some_and(|open| open.dialog.secret_focused())
    }

    /// Gives up the part of a command that a text field has waiting, such as an operator, when
    /// the keymap gives up the command: after Esc, or a key that cannot go on with it.
    fn give_up_command(&mut self) {
        // The key hints with the keys have no command of their own to give up; what waits under
        // them, such as an operator, waits for the command run from them.
        if self.key_hints.is_some() {
            return;
        }
        if let Some(open) = self.dialogs.front_mut() {
            open.dialog.give_up();
        }
        if let Some(configuration) = &mut self.configuration {
            configuration.give_up();
        }
        if let Some(window) = &mut self.find {
            window.give_up();
        }
        if let Some(line) = &mut self.command_line {
            line.give_up();
        }
        if let Some(field) = self.panel_mut(self.active).rename_field() {
            field.give_up();
        }
    }

    /// Ends Visual mode in the panels that do not have the keys: the other side, and the tabs
    /// that do not show.
    fn settle_visual(&mut self) {
        let active = self.active;
        for side in [Side::Left, Side::Right] {
            let shown = self.tabs(side).index();
            for (index, tab) in self.tabs_mut(side).iter_mut().enumerate() {
                if side != active || index != shown {
                    tab.panel.end_visual();
                }
            }
        }
    }

    /// Takes keys resolved with what came with them: a count repeats a motion where keys
    /// move a cursor over rows; a text field, the commands that take a count as a number,
    /// such as `G`, and those that take a register or a character read them themselves. After
    /// Ctrl+x, the other panel has the keys for the command, and gives them back once it ends;
    /// the list of the keys after Ctrl+x has them already for the command run from it.
    pub(crate) fn typed(&mut self, typed: Typed) -> Vec<Effect> {
        let Typed { resolved, extra } = typed;
        if extra.other && self.returns.is_none() {
            self.returns = Some(self.active);
            self.active = self.active.other();
        }
        let times = match resolved {
            Resolved::Action(action) if self.repeats(action) => extra.times().min(MAX_REPEAT),
            Resolved::Action(Action::PageUp | Action::PageDown) => {
                if let Some(count) = extra.count {
                    self.count_pages(count);
                }
                1
            }
            _ => 1,
        };
        self.extra = extra;
        let place = self.place();
        let mut effects = Vec::new();
        for _ in 0..times {
            effects.extend(self.handle(resolved));
            // What the key left, such as Visual mode after Space, takes the rest no more.
            if self.place() != place {
                break;
            }
        }
        self.extra = Extra::default();
        self.come_back();
        effects
    }

    /// Gives the keys back to the panel that Ctrl+x took them from, once the command it ran in
    /// the other panel ends: when that panel has the keys in Normal mode again, with nothing in
    /// front of it and no program to hand the terminal to, as vim goes back to Insert mode
    /// after the command of Ctrl+o.
    pub(crate) fn come_back(&mut self) {
        if self.handover.is_none()
            && self.place() == (Mode::Normal, Context::Panel)
            && let Some(side) = self.returns.take()
        {
            self.active = side;
        }
    }

    /// Whether a count repeats `action` where keys go now: a motion over rows, the previous
    /// tab, or `d d` on a row of a list, outside the text fields, whose editor reads the count. `n` and `N` take it
    /// themselves, as one jump, and marks, which stop at the end of the listing.
    fn repeats(&self, action: Action) -> bool {
        if self.counts_moves() {
            return false;
        }
        // As vim's `3dd`, which deletes three lines: rows of the lists that `d d` removes.
        let removes = matches!(
            (self.context(), action),
            (Context::TabMenu, Action::CloseTab)
                | (Context::LocationMenu, Action::Disconnect)
                | (
                    Context::Workspaces | Context::CommandHistory,
                    Action::Delete
                )
        );
        if removes {
            return true;
        }
        let text = matches!(
            self.context(),
            Context::Rename
                | Context::DialogInput
                | Context::PathInput
                | Context::Completion
                | Context::CommandLine
                | Context::CommandCompletion
                | Context::QuickSearch
        );
        // A count before Ctrl+f and Ctrl+b scrolls that many pages at once in the panels and
        // the lists, as in vim, rather than one at a time.
        let pages = matches!(
            self.context(),
            Context::Panel
                | Context::LocationMenu
                | Context::TabMenu
                | Context::Workspaces
                | Context::CommandHistory
                | Context::KeyHints
                | Context::Jump
        );
        if pages && matches!(action, Action::PageUp | Action::PageDown) {
            return false;
        }
        !text
            && matches!(
                action,
                Action::Up
                    | Action::Down
                    | Action::Left
                    | Action::Right
                    | Action::ScrollLeft
                    | Action::ScrollRight
                    | Action::ScrollHalfLeft
                    | Action::ScrollHalfRight
                    | Action::PageUp
                    | Action::PageDown
                    | Action::HalfPageUp
                    | Action::HalfPageDown
                    | Action::PrevTab
            )
    }

    /// Gives the count before Ctrl+f or Ctrl+b to the list in front, which scrolls that many
    /// pages at once, as vim's do. The panels read it themselves.
    fn count_pages(&mut self, pages: usize) {
        let picker = match self.context() {
            Context::LocationMenu => self.menu.as_mut().map(LocationMenu::picker_mut),
            Context::TabMenu => self.tab_menu.as_mut().map(TabMenu::picker_mut),
            Context::Workspaces => self
                .workspaces_window
                .as_mut()
                .map(WorkspacesWindow::picker_mut),
            Context::CommandHistory => self.history_window.as_mut().map(HistoryWindow::picker_mut),
            Context::KeyHints => self.key_hints.as_mut().map(KeyHintsWindow::picker_mut),
            Context::Jump => self.jump.as_mut().map(JumpMenu::picker_mut),
            _ => None,
        };
        if let Some(picker) = picker {
            picker.count_pages(pages);
        }
    }

    /// Whether a dialog, the search of a tree, or the configuration has the keys, which count
    /// their moves themselves, past their text fields too, where a count would go to the field.
    fn counts_moves(&self) -> bool {
        if self.key_hints.is_some() {
            return false;
        }
        if !self.dialogs.is_empty() || self.front() == Front::Find {
            return true;
        }
        let over = self.menu.is_some()
            || self.tab_menu.is_some()
            || self.jump.is_some()
            || self.workspaces_window.is_some()
            || self.history_window.is_some()
            || !self.results.is_empty()
            || self.in_front().is_some()
            || self.jobs_list.is_some()
            || self.help.is_some();
        !over && self.configuration.is_some()
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
        if action == Action::CloseTab {
            return self.tab_menu_closes();
        }
        match action {
            Action::Help
            | Action::Quit
            | Action::Redraw
            | Action::Disconnect
            | Action::Cancel
            | Action::PullDown
            | Action::PullDownLast
            | Action::Mkdir
            | Action::Delete
            | Action::Copy
            | Action::Duplicate
            | Action::Move
            | Action::Rename
            | Action::View
            | Action::Edit => true,
            Action::ToggleWrap => self.viewing.is_some(),
            Action::EditHost => self.chosen_host().is_some(),
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

    /// How much the screen shows of `job`: its window while it is in front, else its row while
    /// the list of jobs is open, else only its share of the corner.
    fn sight(&self, job: &Job) -> Sight {
        if !job.background {
            Sight::Window
        } else if self.jobs_list.is_some() {
            Sight::Row
        } else {
            Sight::Corner
        }
    }

    /// The help screen, for the keymap and the settings, where it was when it closed last.
    fn help_screen(&self) -> Help {
        Help::new(&self.keymap, &self.about).at(self.help_spot)
    }

    /// Quits, after asking if jobs would stop.
    fn ask_quit(&mut self) {
        if self.jobs.is_empty() {
            self.quit = true;
            return;
        }
        let message = fl!("quit-jobs", count = self.jobs.len());
        let buttons = vec![Button::Yes, Button::No];
        let dialog = Dialog::question(&fl!("quit-title"), &message, buttons, 1, Tone::Warning);
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
                self.end_search(side);
                if self.tabs_mut(side).step(action == Action::NextTab) {
                    return self.revealed(side);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// Opens a tab on `side` after the one that shows, on the same location, and shows it.
    fn new_tab(&mut self, side: Side) -> Vec<Effect> {
        self.end_search(side);
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

    /// Opens the tab menu for the panel on `side`, which lists where its tabs are.
    fn open_tab_menu(&mut self, side: Side) {
        let titles = self.tab_titles(side);
        let input = self.search_opens(Slot::TabMenu);
        let current = self.tabs(side).index();
        self.tab_menu = Some(TabMenu::new(side, titles, current, input));
    }

    /// Where the tabs on `side` are, as the tab menu lists them.
    fn tab_titles(&self, side: Side) -> Vec<String> {
        self.tabs(side)
            .iter()
            .map(|tab| tabs::title(tab.panel.location(), &self.home))
            .collect()
    }

    /// Gives a key to the tab menu: the tab it shows is its panel's, and the keys stay with the
    /// active one, as after the location menu. A tab it closes goes as `t c` closes it; the menu
    /// stays.
    fn handle_tab_menu(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(menu) = &mut self.tab_menu else {
            return Vec::new();
        };
        match menu.handle(input) {
            TabMenuEvent::Pending => Vec::new(),
            TabMenuEvent::Closed => {
                self.tab_menu = None;
                Vec::new()
            }
            TabMenuEvent::Show(index) => {
                let side = menu.side();
                self.tab_menu = None;
                self.end_search(side);
                if self.tabs_mut(side).select(index) {
                    return self.revealed(side);
                }
                Vec::new()
            }
            TabMenuEvent::Close(index) => {
                let side = menu.side();
                let shown = index == self.tabs(side).index();
                if shown {
                    self.end_search(side);
                }
                if self.tabs_mut(side).close_at(index).is_none() {
                    return Vec::new();
                }
                let titles = self.tab_titles(side);
                if let Some(menu) = &mut self.tab_menu {
                    menu.set_titles(titles);
                }
                if shown {
                    return self.revealed(side);
                }
                Vec::new()
            }
        }
    }

    /// Whether the tab menu is open on a panel with more than one tab, of which F8 closes one.
    fn tab_menu_closes(&self) -> bool {
        self.tab_menu
            .as_ref()
            .is_some_and(|menu| self.tabs(menu.side()).len() > 1)
    }

    /// Gives a key to what is over the panels, front first, if anything is.
    fn handle_over(&mut self, input: Resolved) -> Option<Vec<Effect>> {
        if self.key_hints.is_some() {
            return Some(self.handle_key_hints(input));
        }
        if !self.dialogs.is_empty() {
            return Some(self.handle_dialog(input));
        }
        if self.menu.is_some() {
            return Some(self.handle_menu(input));
        }
        if self.tab_menu.is_some() {
            return Some(self.handle_tab_menu(input));
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
                self.help_spot = help.spot();
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
            return Some(self.handle_viewer(input));
        }
        if self.find.is_some() {
            return Some(self.handle_find(input));
        }
        None
    }

    /// Takes what a key, or keys, resolved to; the fields that search then keep how they match.
    pub(crate) fn handle(&mut self, input: Resolved) -> Vec<Effect> {
        let effects = self.handle_input(input);
        self.remember_searches();
        effects
    }

    /// How the field of `slot` opens: as it was last, else in `ui.default_search_mode`.
    fn search_opens(&self, slot: Slot) -> Input {
        self.searches
            .input(slot, self.config.ui.default_search_mode)
    }

    /// Keeps how each field that searches and is open matches now, and whether case counts,
    /// which it opens with again (ADR 0047): after each key or press, so that a field keeps
    /// what was switched in it however it closes.
    fn remember_searches(&mut self) {
        for slot in Slot::ALL {
            let input = match slot {
                Slot::QuickSearch => self.panel(self.active).search_input(),
                Slot::LocationMenu => self.menu.as_ref().map(LocationMenu::input),
                Slot::TabMenu => self.tab_menu.as_ref().map(TabMenu::input),
                Slot::Workspaces => self.workspaces_window.as_ref().map(WorkspacesWindow::input),
                Slot::CommandHistory => self.history_window.as_ref().map(HistoryWindow::input),
                Slot::KeyHints => self.key_hints.as_ref().map(KeyHintsWindow::input),
                Slot::Jump => self.jump.as_ref().map(JumpMenu::input),
                Slot::FindText => self.find.as_ref().map(FindWindow::text_input),
                Slot::FindFiles => self.find.as_ref().map(FindWindow::files_input),
                Slot::Mark => self
                    .dialogs
                    .iter()
                    .find(|open| matches!(open.purpose, Purpose::Pattern { .. }))
                    .and_then(|open| open.dialog.search(0)),
            };
            if let Some(input) = input {
                self.searches.keep(slot, input);
            }
        }
    }

    fn handle_input(&mut self, input: Resolved) -> Vec<Effect> {
        self.sync_connected();
        if let Some(effects) = self.handle_everywhere(input) {
            return effects;
        }
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
        if self.ask_connect(self.active, action) {
            return Vec::new();
        }
        if let Some(effects) = self.handle_vim(action) {
            return effects;
        }
        match action {
            Action::QuickSearch => self.quick_search(),
            Action::QuickSearchBack => {
                let (count, input) = (self.extra.times(), self.search_opens(Slot::QuickSearch));
                self.panel_mut(self.active).search_up(count, input);
            }
            Action::NextMatch | Action::PrevMatch => self.search_again(action == Action::NextMatch),
            Action::Shell | Action::Command | Action::CommandHistory => {
                self.open_command_line(action);
            }
            Action::Quit => self.ask_quit(),
            Action::Jobs => self.jobs_list = Some(JobsList::default()),
            Action::Help => self.help = Some(self.help_screen()),
            Action::SwitchPanel => self.active = self.active.other(),
            Action::FocusLeft | Action::FocusRight => {
                self.active = self.drawn_on(action == Action::FocusLeft);
            }
            Action::NewTab | Action::CloseTab | Action::NextTab | Action::PrevTab => {
                return self.tab_action(self.active, action);
            }
            Action::TabList => self.open_tab_menu(self.active),
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
            Action::SystemOpen => return self.system_open(),
            Action::Copy => self.ask_transfer(transfer::Kind::Copy),
            Action::Duplicate => self.ask_duplicate(),
            Action::Move => self.ask_transfer(transfer::Kind::Move),
            Action::Rename => self.start_rename(),
            Action::Select | Action::Unselect => self.ask_pattern(action == Action::Select),
            // Esc closes the whole name below the panels when it waits for no listing.
            Action::Cancel if self.panel(self.active).pending_request().is_none() => {
                self.whole = Whole::Cut;
            }
            Action::Cancel => self.cancel(self.active),
            Action::Disconnect => self.disconnect_chosen(),
            Action::EditHost => {
                if let Some(host) = self.chosen_host() {
                    self.ask_edit_host(host);
                }
            }
            Action::LocationMenu => return self.open_menu(self.active),
            Action::Jump => return self.open_jump(),
            Action::FindFile => return self.open_find(FindMode::Text),
            Action::FindNames => return self.open_find(FindMode::Names),
            Action::GoToDir => self.ask_go_to(),
            Action::SaveWorkspace => self.ask_save_workspace(true),
            Action::Workspaces => self.open_workspaces(),
            Action::PullDown => self.open_pulldown(false),
            Action::PullDownLast => self.open_pulldown(true),
            _ => {
                let id = self.shown(self.active);
                if let Some(request) = self.panel_mut(id.side).handle(action) {
                    return self.open(id, request);
                }
            }
        }
        Vec::new()
    }

    /// Does what vim's keys do in a panel that mc's have no likes of: Visual mode, the jump
    /// list, and the commands that take a count as a number; `None` for the other keys.
    fn handle_vim(&mut self, action: Action) -> Option<Vec<Effect>> {
        if self.panel(self.active).visual()
            && let Some(effects) = self.handle_visual(action)
        {
            return Some(effects);
        }
        let count = self.extra.count;
        let id = self.shown(self.active);
        match action {
            Action::VisualMode | Action::VisualLineMode => {
                let line = action == Action::VisualLineMode;
                self.panel_mut(id.side).start_visual(count, line);
            }
            Action::Reselect => self.panel_mut(id.side).reselect(),
            Action::JumpBack | Action::JumpForward => {
                let times = count.unwrap_or(1);
                let panel = self.panel_mut(id.side);
                let request = if action == Action::JumpBack {
                    panel.jump_back(times)
                } else {
                    panel.jump_forward(times)
                };
                if let Some(request) = request {
                    return Some(self.open(id, request));
                }
            }
            // With a count, the row of that number, as vim's `G` and `g g` go to the line.
            Action::Home | Action::End if count.is_some() => {
                self.panel_mut(id.side).go_to_row(count.unwrap_or(1));
            }
            Action::PageUp | Action::PageDown if count.is_some() => {
                let down = action == Action::PageDown;
                self.panel_mut(id.side).page_by(down, count.unwrap_or(1));
            }
            Action::Mark | Action::MarkUp if count.is_some() => {
                let down = action == Action::Mark;
                self.panel_mut(id.side).mark_rows(down, count.unwrap_or(1));
            }
            // With a count, the tab of that number, as vim's `g t`.
            Action::NextTab if count.is_some() => {
                self.end_search(id.side);
                if self.tabs_mut(id.side).select(count.unwrap_or(1) - 1) {
                    return Some(self.revealed(id.side));
                }
            }
            _ => return None,
        }
        Some(Vec::new())
    }

    /// Does what the keys do wherever they go, before what is in front takes them; `None` for
    /// the other keys.
    fn handle_everywhere(&mut self, input: Resolved) -> Option<Vec<Effect>> {
        let Resolved::Action(action) = input else {
            return None;
        };
        match action {
            // The hints show their keys over what is in front, which stays open; again while
            // they show, they take the keys, as a list.
            Action::KeyHints => {
                let place = self.key_place();
                match self.hints.take() {
                    Some(hints) if hints.place == place => {
                        let input = self.search_opens(Slot::KeyHints);
                        let window = KeyHintsWindow::new(&self.keymap, &hints, self.extra, input);
                        self.key_hints = Some(window);
                    }
                    _ => self.hints_asked = Some(place),
                }
            }
            // The panels alone: the lists always show their row under the cursor whole.
            Action::ShowFull if self.context() == Context::Panel => {
                self.whole = self.whole.toggled();
            }
            // So does the output of commands, and what is in front is there again afterwards;
            // as a command of its own, it gives up an operator that waits, and is the one
            // command after Ctrl+o.
            Action::UserScreen => {
                self.give_up_command();
                self.handover = Some(Handover::UserScreen);
            }
            // A redraw leaves what is in front as it was too; quick search goes on after it.
            Action::Redraw => {
                self.give_up_command();
                self.redraw = true;
            }
            // As in a text field, Ctrl+r types the text of a register in the filter of a list.
            Action::PasteRegister if self.filtering() => {
                let text = self.registers.text(self.extra.char).unwrap_or_default();
                let text: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
                return Some(
                    text.into_iter()
                        .flat_map(|c| self.handle(Resolved::Insert(c)))
                        .collect(),
                );
            }
            _ => return None,
        }
        Some(Vec::new())
    }

    /// Takes a key while a panel selects rows in Visual mode: the motions extend the
    /// selection, Space marks it, and the commands of files act on what it selects, which they
    /// find marked. Any other command, such as an F-key or one of the pull-down menu, ends
    /// Visual mode and runs as in Normal mode: `None` then.
    fn handle_visual(&mut self, action: Action) -> Option<Vec<Effect>> {
        let id = self.shown(self.active);
        let count = self.extra.count;
        let panel = self.panel_mut(id.side);
        match action {
            Action::NormalMode => panel.end_visual(),
            Action::VisualMode | Action::VisualLineMode => {
                panel.switch_visual(action == Action::VisualLineMode);
            }
            Action::OtherEnd => panel.other_end(),
            Action::Mark => panel.mark_visual(false),
            Action::Copy
            | Action::Move
            | Action::Delete
            | Action::Checksum
            | Action::SystemOpen => {
                panel.mark_visual(true);
                return Some(self.handle(Resolved::Action(action)));
            }
            Action::Home | Action::End if count.is_some() => {
                panel.go_to_row(count.unwrap_or(1));
            }
            Action::PageUp | Action::PageDown if count.is_some() => {
                panel.page_by(action == Action::PageDown, count.unwrap_or(1));
            }
            Action::Up
            | Action::Down
            | Action::PageUp
            | Action::PageDown
            | Action::HalfPageUp
            | Action::HalfPageDown
            | Action::Home
            | Action::End => {
                if let Some(request) = panel.handle(action) {
                    return Some(self.open(id, request));
                }
            }
            _ => {
                panel.end_visual();
                return None;
            }
        }
        Some(Vec::new())
    }

    /// Gives a key to quick search in the active panel, if it runs; the action that the panel
    /// does next, if any. Characters go nowhere else: typing in a panel does nothing.
    fn search_key(&mut self, input: Resolved) -> Option<Action> {
        let side = self.active;
        let searching = self.panel(side).searching();
        if searching
            && let Resolved::Action(action) = input
            && let Some(switch) = Switch::of(action)
        {
            self.panel_mut(side).switch_search(switch);
            return None;
        }
        match input {
            Resolved::Insert(c) => {
                if searching {
                    self.panel_mut(side).search_type(c);
                }
                None
            }
            Resolved::Hotkey(_) => None,
            Resolved::Action(action) if searching => match action {
                // Unlike vim's search, Backspace and Delete with nothing typed keep it open, as
                // on the command line: a Backspace too many would go to the parent directory.
                Action::Left
                | Action::Right
                | Action::TextStart
                | Action::TextEnd
                | Action::WordForward
                | Action::WordBackward
                | Action::Backspace
                | Action::DeleteChar
                | Action::DeleteToStart
                | Action::DeleteToEnd
                | Action::DeleteWord => {
                    self.panel_mut(side).search_edit(action);
                    None
                }
                Action::QuickSearch => {
                    self.quick_search();
                    None
                }
                // As in vim's search, Ctrl+r types the text of a register.
                Action::PasteRegister => {
                    let text = self.registers.text(self.extra.char).unwrap_or_default();
                    let text: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
                    for c in text {
                        self.panel_mut(side).search_type(c);
                    }
                    None
                }
                Action::NextMatch | Action::PrevMatch => {
                    let forward = action == Action::NextMatch;
                    self.panel_mut(side).search_step(forward);
                    None
                }
                // As vim's Enter, it says when the search went round or found nothing.
                Action::Cancel => {
                    if let Some(end) = self.end_search(side) {
                        self.message = match end.found {
                            None => Some(fl!("message-search-not-found", text = end.text)),
                            Some(true) if end.back => Some(fl!("message-search-top")),
                            Some(true) => Some(fl!("message-search-bottom")),
                            Some(false) => None,
                        };
                    }
                    None
                }
                // What it found is not kept, as vim keeps no pattern its Esc gives up.
                Action::Abort => {
                    self.panel_mut(side).abort_search();
                    None
                }
                // Any other key ends the search, then does what it does.
                _ => {
                    self.end_search(side);
                    Some(action)
                }
            },
            Resolved::Action(action) => Some(action),
        }
    }

    /// Starts quick search in the active panel, or, as Ctrl+g in it, jumps to the next match;
    /// with nothing typed yet, searches for what the last search found, as mc's Ctrl+s Ctrl+s
    /// does.
    fn quick_search(&mut self) {
        let input = self.search_opens(Slot::QuickSearch);
        let last = self.last_search.clone();
        let count = self.extra.times();
        let panel = self.panel_mut(self.active);
        let typed = panel.search_text().is_some_and(|text| !text.is_empty());
        match last {
            Some((last, _, input)) if panel.searching() && !typed => {
                panel.search_resume(&last, input);
            }
            _ if panel.searching() => panel.search_step(true),
            _ => panel.search_down(count, input),
        }
    }

    /// Moves the cursor of the active panel to the next name, or the previous one, that matches
    /// what the last quick search found, `next` the way it went; where it was goes to the jump
    /// list.
    fn search_again(&mut self, next: bool) {
        let Some((last, back, input)) = self.last_search.clone() else {
            self.message = Some(fl!("message-search-none"));
            return;
        };
        let forward = next != back;
        let times = self.extra.times();
        let round = self
            .panel_mut(self.active)
            .search_again((&last, input), forward, times);
        self.message = match round {
            None => Some(fl!("message-search-not-found", text = last)),
            Some(true) if forward => Some(fl!("message-search-bottom")),
            Some(true) => Some(fl!("message-search-top")),
            Some(false) => None,
        };
    }

    /// The side of the panel drawn on the left, or on the right: swapped panels are drawn on
    /// each other's sides.
    fn drawn_on(&self, left: bool) -> Side {
        if left == self.swapped {
            Side::Right
        } else {
            Side::Left
        }
    }

    /// Ends quick search in the panel on `side`, and keeps what it searched for for `n` and
    /// `N`, with the way it went, found or not, as vim keeps its pattern; how it ended.
    fn end_search(&mut self, side: Side) -> Option<SearchEnd> {
        let end = self.panel_mut(side).end_search()?;
        self.last_search = Some((end.text.clone(), end.back, end.input));
        Some(end)
    }

    /// Gives a key to the dialog in front, and does what it was for once it closes. In a path
    /// field, Tab completes, and the list of completions takes the keys while it shows.
    fn handle_dialog(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(effects) = self.change_dir(Completes::Field, input) {
            return effects;
        }
        if self.choose(Completes::Field, input) {
            return Vec::new();
        }
        if input == Resolved::Action(Action::Complete) {
            return self.complete();
        }
        // Any other key makes a completion on its way too late.
        self.completion = None;
        let Some(open) = self.dialogs.front_mut() else {
            return Vec::new();
        };
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        let event = open.dialog.handle_typed(typed, &mut self.registers);
        self.dialog_event(event)
    }

    /// Gives a key to the list of completions for `target`, if it shows; whether the key was
    /// the list's. Any other key closes it, keeping the text, and goes on to the field.
    fn choose(&mut self, target: Completes, input: Resolved) -> bool {
        let Some(choices) = choices_for(&mut self.completion, target) else {
            return false;
        };
        let event = match input {
            Resolved::Action(action) => choices.handle(action),
            Resolved::Insert(_) | Resolved::Hotkey(_) => ChoicesEvent::Passed,
        };
        self.take_choices(event)
    }

    /// Left and Right in the list of completions for `target`, as in vim's pop-up menu of the
    /// command line: Left completes again in the parent of the directory listed, and Right in
    /// the directory chosen; `None` where the key is not one of them, or Right has no
    /// directory to go into.
    fn change_dir(&mut self, target: Completes, input: Resolved) -> Option<Vec<Effect>> {
        let up = match input {
            Resolved::Action(Action::Left) => true,
            Resolved::Action(Action::Right) => false,
            _ => return None,
        };
        let completing = self
            .completion
            .as_ref()
            .filter(|completing| completing.target == target && completing.choices.is_some())?;
        let before = self.before_cursor(target)?;
        let (lead, word) = before.split_at(completing.word.min(before.len()));
        let word = if up {
            complete::up_dir(word)
        } else {
            complete::down_dir(word)?
        };
        let before = format!("{lead}{word}");
        self.completion = None;
        self.put_before_cursor(target, &before);
        Some(match target {
            Completes::Field => self.complete(),
            Completes::CommandLine => self.complete_command(),
        })
    }

    /// Does what a key or a press did to the list of completions: puts a match in, or what
    /// was typed, and closes the list if it is done. Whether it was the list's.
    fn take_choices(&mut self, event: ChoicesEvent) -> bool {
        let Some(completing) = &self.completion else {
            return false;
        };
        let ChoicesEvent::Show { text, close } = event else {
            self.completion = None;
            return false;
        };
        let target = completing.target;
        let before = match text {
            Some(text) => format!("{}{text}", completing.keep),
            None => completing.before.clone(),
        };
        if close {
            self.completion = None;
        }
        self.put_before_cursor(target, &before);
        true
    }

    /// The text before the cursor where a completion for `target` goes, if it can go there
    /// now: the path field of the dialog in front, or the command line with the keys.
    fn before_cursor(&self, target: Completes) -> Option<&str> {
        match target {
            Completes::Field => self
                .dialogs
                .front()
                .filter(|open| open.dialog.completes())
                .map(|open| open.dialog.around_cursor().0),
            Completes::CommandLine => self
                .command_line
                .as_ref()
                .filter(|_| {
                    matches!(
                        self.context(),
                        Context::CommandLine | Context::CommandCompletion
                    )
                })
                .map(CommandLine::before_cursor),
        }
    }

    /// Replaces the text before the cursor where a completion for `target` goes with `before`.
    fn put_before_cursor(&mut self, target: Completes, before: &str) {
        match target {
            Completes::Field => {
                if let Some(open) = self.dialogs.front_mut() {
                    open.dialog.replace_before_cursor(before);
                }
            }
            Completes::CommandLine => {
                if let Some(line) = &mut self.command_line {
                    line.replace_before_cursor(before);
                }
            }
        }
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
        let mut menu = LocationMenu::new(side, current, self.home.clone(), generation)
            .with_input(self.search_opens(Slot::LocationMenu))
            .recent(&self.recent)
            .lines_like(self.sizes.menu);
        menu.set_connected(self.hosts.keys().cloned().collect());
        self.menu = Some(menu);
        vec![Effect::ListPlaces { generation }]
    }

    /// Reads the volumes and hosts of the location menu again, if it is open.
    fn reload_menu(&mut self) -> Vec<Effect> {
        let Some(menu) = &mut self.menu else {
            return Vec::new();
        };
        self.menu_listings += 1;
        let generation = self.menu_listings;
        menu.reload(generation);
        vec![Effect::ListPlaces { generation }]
    }

    /// Gives a key to the location menu: what it opens goes to its panel, and the keys stay with
    /// the active one, as after Ctrl+w f and Ctrl+w v.
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
                self.go(self.shown(side), Destination::to(location))
            }
            MenuEvent::Disconnect(host) => {
                self.disconnect(&host);
                Vec::new()
            }
            MenuEvent::EditHost(host) => {
                self.ask_edit_host(host);
                Vec::new()
            }
            MenuEvent::Reload => self.reload_menu(),
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
        let dialog = Dialog::form(&title, &prompt, &name, &[]);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Workspace(WorkspaceQuestion::Save),
        });
    }

    /// Saves the tabs of both panels under `name`, the text of the dialog of t W. A name
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
            let dialog = Dialog::question(&title, &message, buttons, 0, Tone::Warning);
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
                let (mut panel, request) =
                    workspaces::restored_panel(saved, &self.home, show_hidden);
                panel.set_natural_sort(self.config.ui.natural_sort);
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
        let window = WorkspacesWindow::new(rows, self.search_opens(Slot::Workspaces));
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
                let shown = label::escape(&cells::sanitize(from.as_bytes()));
                let title = fl!("workspaces-rename-title");
                let prompt = fl!("workspaces-rename-prompt", name = shown);
                let dialog = Dialog::form(&title, &prompt, &from, &[]);
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
                let dialog = Dialog::question(&title, &message, buttons, 0, Tone::Warning);
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
            let dialog = Dialog::question(&title, &message, buttons, 1, Tone::Warning);
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
            Purpose::GoToDir { panel } => Some((*panel, Offer::Dirs, FieldHosts::All)),
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
        let word = match before.split_once(':') {
            Some((host, _)) if !host.is_empty() && !host.contains('/') && is_host(host) => {
                host.len() + 1
            }
            _ => 0,
        };
        let Some(shown) = self.panel_of(panel) else {
            return Vec::new();
        };
        let dir = cd::directory(&split.dir, shown.location(), &self.home, is_host);
        // A host that is not connected has nothing to list.
        let host = self.handle_for(&dir).unwrap_or(None);
        let list_hosts = split.hosts && hosts == FieldHosts::All;
        let generation = self.start_completion(Completing {
            generation: 0,
            target: Completes::Field,
            before,
            keep: split.dir,
            word,
            prefix: split.prefix,
            offer,
            quote: Quote::Plain,
            hosts: if split.hosts { hosts } else { FieldHosts::None },
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

    /// Waits for the listing of `completing`, under a new generation, which it returns.
    fn start_completion(&mut self, completing: Completing) -> u64 {
        self.completions += 1;
        let generation = self.completions;
        self.completion = Some(Completing {
            generation,
            ..completing
        });
        generation
    }

    /// Completes the word before the cursor on the command line, as vim's `:!` does: in a
    /// local panel, the first word as a command of `$PATH`, unless it names a directory, and
    /// the others as files from the panel's directory, where the command runs; on a host, every
    /// word as files there, through its session. Names go in escaped for the shell.
    fn complete_command(&mut self) -> Vec<Effect> {
        let Some(line) = &mut self.command_line else {
            return Vec::new();
        };
        let Some((lead, command)) = line.command_before_cursor() else {
            return Vec::new();
        };
        let word = complete::word(command);
        let keep = format!("{lead}{}", word.keep);
        let start = lead.len() + word.start;
        let before = line.before_cursor().to_owned();
        if word.dir.is_empty() && &before[keep.len()..] == "~" {
            line.replace_before_cursor(&format!("{keep}~/"));
            return Vec::new();
        }
        let here = self.panel(self.active).location().clone();
        let commands = word.first && word.dir.is_empty() && matches!(here, Location::Local(_));
        let generation = self.start_completion(Completing {
            generation: 0,
            target: Completes::CommandLine,
            before,
            keep,
            word: start,
            prefix: word.prefix.clone(),
            offer: Offer::Any,
            quote: Quote::Shell,
            hosts: FieldHosts::None,
            connected: Vec::new(),
            choices: None,
        });
        if commands {
            return vec![Effect::ListCommands {
                generation,
                dirs: self.command_dirs.clone(),
                prefix: word.prefix,
                with_dirs: false,
            }];
        }
        let dir = cd::directory(&word.dir, &here, &self.home, |_| false);
        // A command named by its path, as vim's `:!` completes it: directories and programs.
        if word.first
            && let Location::Local(path) = &dir
        {
            return vec![Effect::ListCommands {
                generation,
                dirs: vec![path.clone()],
                prefix: word.prefix,
                with_dirs: true,
            }];
        }
        // The line opens on a host only while it is connected.
        let host = self.handle_for(&dir).unwrap_or(None);
        vec![Effect::ListNames {
            generation,
            dir: Some(dir),
            host,
            hosts: false,
        }]
    }

    /// Takes the result of an [`Effect::ListNames`] or [`Effect::ListCommands`]: one match
    /// goes in the field; of several, the first goes in, and a list of them shows.
    pub(crate) fn names(
        &mut self,
        generation: u64,
        entries: Result<Vec<(Vec<u8>, bool)>, String>,
        hosts: &[String],
    ) {
        let Some(completing) = &self.completion else {
            return;
        };
        if completing.generation != generation || completing.choices.is_some() {
            return;
        }
        if self.before_cursor(completing.target) != Some(completing.before.as_str()) {
            self.completion = None;
            return;
        }
        let entries = entries.unwrap_or_else(|reason| {
            tracing::debug!(%reason, "cannot list names to complete");
            Vec::new()
        });
        let mut candidates = complete::from_names(entries, completing.offer);
        let aliases = match completing.hosts {
            FieldHosts::All => hosts,
            FieldHosts::Connected => &completing.connected[..],
            FieldHosts::None => &[],
        };
        candidates.extend(aliases.iter().map(|alias| Candidate {
            name: alias.clone(),
            kind: Kind::Host,
        }));
        let (target, quote) = (completing.target, completing.quote);
        let keep = completing.keep.clone();
        let text = match complete::complete(&completing.prefix, candidates) {
            Outcome::Nothing => None,
            Outcome::One(candidate) => Some(candidate.typed(quote)),
            Outcome::List(items) => {
                let choices = Choices::new(items, quote);
                let first = choices.current();
                if let Some(completing) = &mut self.completion {
                    completing.choices = Some(choices);
                }
                first
            }
        };
        if !self.lists(target) {
            self.completion = None;
        }
        if let Some(text) = text {
            self.put_before_cursor(target, &format!("{keep}{text}"));
        }
    }

    /// Opens the zoxide window for the active panel, and asks zoxide for its best
    /// directories; in a search mode but the plain one, for all of them, which the window
    /// filters itself.
    fn open_jump(&mut self) -> Vec<Effect> {
        let exclude = match self.panel(self.active).location() {
            Location::Local(path) => Some(path.clone()),
            Location::Remote { .. } => None,
        };
        let jump = JumpMenu::new(self.home.clone(), exclude, 0)
            .with_input(self.search_opens(Slot::Jump))
            .rows_like(self.sizes.jump);
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
        jump.ask(generation);
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
            self.sizes.jump = jump.rows_listed().or(self.sizes.jump);
        }
    }

    /// Opens the dialog of the search of the files under the active panel's directory in `mode`
    /// (`f /` and F9 → Command → Find file… for text, `f f` and Find files and dirs… for
    /// names), as far as `[search]` lets it go, with the hidden files if the panels show them,
    /// and lists the files at once.
    fn open_find(&mut self, mode: FindMode) -> Vec<Effect> {
        let location = self.panel(self.active).location().clone();
        let title = match &location {
            Location::Local(path) => home_text(path, &self.home),
            Location::Remote { .. } => location_text(&location),
        };
        let search = &self.config.search;
        self.find = Some(FindWindow::new(
            (location, title),
            mode,
            (search.max_results.get(), search.max_per_file.get()),
            self.config.ui.show_hidden,
            (
                self.search_opens(Slot::FindText),
                self.search_opens(Slot::FindFiles),
            ),
            self.decor,
        ));
        self.search_tree(Duration::ZERO)
    }

    /// Starts the search of the dialog of the search of a tree again, after `delay`; the one
    /// before stops.
    fn search_tree(&mut self, delay: Duration) -> Vec<Effect> {
        let Some(window) = &self.find else {
            return Vec::new();
        };
        let location = window.root().clone();
        // A host that is not connected has no session to search with: the window says so.
        let host = self.handle_for(&location).ok().flatten();
        self.finds += 1;
        let generation = self.finds;
        let cancel = CancellationToken::new();
        let Some(window) = &mut self.find else {
            return Vec::new();
        };
        window.restart(generation, cancel.clone());
        vec![Effect::Find(FindRequest {
            generation,
            location,
            host,
            query: window.query(),
            max_results: window.max_results(),
            delay,
            cancel,
        })]
    }

    /// Gives a key to the dialog of the search of a tree.
    fn handle_find(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.find else {
            return Vec::new();
        };
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        let event = window.handle(typed, &mut self.registers);
        self.find_event(event)
    }

    /// Does what `event` of the dialog of the search of a tree says: a change searches again,
    /// Go to goes to the file in the active panel, which puts where it was in the jump list, or
    /// into a directory found by name, and View and Edit view and edit a file at the line, the
    /// dialog kept. Closing it stops the search.
    fn find_event(&mut self, event: FindEvent) -> Vec<Effect> {
        match event {
            FindEvent::Pending => Vec::new(),
            FindEvent::Search => self.search_tree(FIND_DELAY),
            FindEvent::Closed => {
                self.find = None;
                Vec::new()
            }
            FindEvent::Go { location, name } => {
                self.find = None;
                let id = self.shown(self.active);
                let panel = self.panel_mut(id.side);
                // Another location goes in the jump list on the way; so does a row of this one.
                if *panel.location() == location {
                    panel.remember();
                }
                self.go(id, Destination::onto_entry(location, &name))
            }
            FindEvent::Open { location } => {
                self.find = None;
                let id = self.shown(self.active);
                self.go(id, Destination::to(location))
            }
            FindEvent::View {
                location,
                name,
                line,
            } => self.view_file(child(&location, &name), &location, line),
            FindEvent::Edit {
                location,
                name,
                line,
            } => self.edit_file(location, &name, line),
        }
    }

    /// Takes what the search `generation` of [`Effect::Find`] found.
    pub(crate) fn found(&mut self, generation: u64, found: Vec<Finding>) {
        if let Some(window) = &mut self.find {
            window.found(generation, found);
        }
    }

    /// Takes how the search `generation` of [`Effect::Find`] ended, or why it could not
    /// search.
    pub(crate) fn find_ended(&mut self, generation: u64, result: Result<FindSummary, String>) {
        if let Some(window) = &mut self.find {
            window.finished(generation, result);
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

    /// Opens the pull-down menu: the bar at the menu of the active panel, or with `last`, on
    /// the command that ran last.
    fn open_pulldown(&mut self, last: bool) {
        let pulldown = PullDown::new(
            self.active,
            self.swapped,
            self.pulldown_last.filter(|_| last),
            &workspaces::names(&self.workspaces),
            self.theme.glyphs.marks,
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
            PullDownEvent::Closed => Vec::new(),
            PullDownEvent::Run(command) => {
                self.pulldown_last = Some(command);
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
                action @ (Action::NewTab | Action::CloseTab | Action::NextTab | Action::PrevTab),
            ) => self.tab_action(side, action),
            Command::On(side, action) if self.ask_connect(side, action) => Vec::new(),
            Command::On(side, action) => match self.panel_mut(side).handle(action) {
                Some(request) => self.open(self.shown(side), request),
                None => Vec::new(),
            },
            Command::Location(side) => self.open_menu(side),
            Command::Tabs(side) => {
                self.open_tab_menu(side);
                Vec::new()
            }
            Command::DisconnectPanel(side) => {
                if let Some(host) = self.host_of(side) {
                    self.disconnect(&host);
                }
                Vec::new()
            }
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
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        let event = configuration.handle_typed(typed, &mut self.registers);
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
        // The dialog offers only the built-in themes, and the one that `ui.theme` names.
        self.theme = theme_of(ui, self.color_depth);
        self.decor = Decor::new(ui.icons);
        if ui.show_hidden != self.config.ui.show_hidden {
            let show = ui.show_hidden;
            for panel in self.panels_mut() {
                panel.set_show_hidden(show);
            }
        }
        if ui.natural_sort != self.config.ui.natural_sort {
            let natural = ui.natural_sort;
            for panel in self.panels_mut() {
                panel.set_natural_sort(natural);
            }
        }
        let formats = (old.ui.formats != new.ui.formats).then(|| new.ui.formats.clone());
        self.copy_choices.atomic = config.transfer.atomic_upload;
        self.parallel_jobs = config.transfer.parallel_jobs.get();
        self.config = config.clone();
        // A program changed: what it is shows on the About page.
        let changed = [
            (Tool::Zoxide, old.zoxide.program != new.zoxide.program),
            (Tool::Ripgrep, old.search.program != new.search.program),
            (Tool::Fd, old.search.fd != new.search.fd),
        ];
        let mut effects = vec![Effect::SaveConfig {
            old: Box::new(old),
            new: Box::new(new),
            config: Box::new(config),
        }];
        effects.extend(
            changed
                .into_iter()
                .filter(|&(_, changed)| changed)
                .map(|(tool, _)| Effect::Detect(tool)),
        );
        effects.extend(formats.map(Effect::Formats));
        effects.extend(self.start_queued());
        effects
    }

    /// Writes dates and numbers, and sorts names, in the formats in use from now on, which
    /// [`Effect::Formats`] made.
    pub(crate) fn formats_changed(&mut self) {
        for panel in self.panels_mut() {
            panel.sort_again();
        }
        self.about.report.formats = Some(crate::about::formats_text(&noc_locale::current()));
        self.refresh_help();
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

    /// The key that does `action` in the panel on `side`, for the pull-down menu: its own in the
    /// active panel, and Ctrl+x and it in the other.
    fn key_on(&self, side: Side, action: Action) -> Option<String> {
        if side == self.active {
            self.keymap.key(Context::Panel, action)
        } else {
            self.keymap.other_key(Context::Panel, action)
        }
    }

    /// Whether a command of the pull-down menu runs now, its mark, and its key.
    fn command_status(&self, command: Command) -> Status {
        let context = Context::Panel;
        let active = self.panel(self.active);
        match command {
            Command::Do(action) => Status {
                enabled: match action {
                    Action::Checksum => !active.chosen().is_empty(),
                    Action::SystemOpen => {
                        matches!(active.location(), Location::Local(_))
                            && !active.chosen().is_empty()
                    }
                    Action::Disconnect => self
                        .chosen_host()
                        .is_some_and(|host| self.hosts.contains_key(&host)),
                    Action::Select
                    | Action::Unselect
                    | Action::InvertMarks
                    | Action::QuickSearch
                    | Action::FindFile
                    | Action::FindNames
                    | Action::Jump
                    | Action::GoToDir
                    | Action::SwapPanels
                    | Action::OtherPanelOpen
                    | Action::OtherPanelSync
                    | Action::Jobs
                    | Action::SaveWorkspace
                    | Action::Workspaces
                    | Action::ToggleHidden
                    | Action::ShowFull => true,
                    _ => self.supports(action),
                },
                checked: match action {
                    Action::ToggleHidden => self.config.ui.show_hidden,
                    Action::ShowFull => self.whole == Whole::Shown,
                    _ => false,
                },
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
                key: self.key_on(side, action),
            },
            Command::Location(side) => Status {
                enabled: true,
                checked: false,
                key: self.key_on(side, Action::LocationMenu),
            },
            Command::Tabs(side) => Status {
                enabled: true,
                checked: false,
                key: self.key_on(side, Action::TabList),
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

    /// Takes the result of an [`Effect::ListPlaces`]: the volumes, which tell which one a
    /// panel arrives on, and, for the location menu, the hosts.
    pub(crate) fn places(&mut self, generation: u64, result: Result<Places, String>) {
        if let Ok(places) = &result {
            self.volumes.clone_from(&places.volumes);
        }
        if let Some(menu) = &mut self.menu {
            menu.listed(generation, result);
            self.sizes.menu = menu.lines_listed().or(self.sizes.menu);
        }
    }

    /// Hosts that are connected, connecting, or stalled, which the location menu always shows.
    fn sync_connected(&mut self) {
        if let Some(menu) = &mut self.menu {
            menu.set_connected(self.hosts.keys().cloned().collect());
        }
    }

    /// Takes the hosts and volumes as `recent.toml` held them; those used since come first.
    pub(crate) fn recent_loaded(&mut self, result: Result<Recent, String>) {
        let Ok(loaded) = result else {
            return;
        };
        let mut recent = loaded.clone();
        for host in self.recent.hosts.iter().rev() {
            recent.use_host(host);
        }
        for volume in self.recent.volumes.iter().rev() {
            recent.use_volume(volume);
        }
        self.recent = recent;
        self.recent_written = loaded;
    }

    /// The hosts and volumes used last, if they changed since the last call, for
    /// `recent.toml`.
    pub(crate) fn take_recent(&mut self) -> Option<Recent> {
        if self.recent == self.recent_written {
            return None;
        }
        self.recent_written.clone_from(&self.recent);
        Some(self.recent.clone())
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
                let resolved = self.panel_of(panel).map(|p| p.resolve(dialog.text()));
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
                self.copy_choices.preserve = transfer::preserve(dialog);
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
                let location = child(&dir, dialog.text().as_bytes());
                return self.write_sums(window, location, bytes, false);
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
            Purpose::GoToDir { panel } if ok => return self.cd(panel, dialog.text()),
            Purpose::Workspace(question) => {
                return self.workspace_dialog_closed(dialog.text(), question, event);
            }
            Purpose::Reconnect { host, reason } => {
                if event == DialogEvent::Pressed(Button::Retry) {
                    return self.reconnect(&host);
                }
                self.give_up(&host, &reason);
            }
            Purpose::ListFailed { panel, failure } => {
                return self.list_failed_closed(panel, failure, event);
            }
            Purpose::NotConnected { panel } => return self.not_connected_closed(panel, event),
            Purpose::Quit => self.quit = event == DialogEvent::Pressed(Button::Yes),
            Purpose::EditHost { .. }
            | Purpose::GoToDir { .. }
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

    /// Views the file under the cursor of the active panel, as F3 does in mc; a directory
    /// opens as with Enter.
    fn view(&mut self) -> Vec<Effect> {
        let side = self.active;
        let panel = self.panel(side);
        let file = panel
            .entry_under_cursor()
            .filter(|entry| !entry.is_dir_like())
            .map(|entry| child(panel.location(), &entry.name));
        let Some(location) = file else {
            if let Some(request) = self.panel_mut(side).handle(Action::Enter) {
                return self.open(self.shown(side), request);
            }
            return Vec::new();
        };
        let dir = self.panel(side).location().clone();
        self.view_file(location, &dir, None)
    }

    /// Views the file at `location`, in `dir`, from `line` if there is one.
    fn view_file(&mut self, location: Location, dir: &Location, line: Option<u64>) -> Vec<Effect> {
        let host = match &location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Some(handle.clone()),
                _ => None,
            },
            Location::Local(_) => None,
        };
        self.last_job += 1;
        let mebibytes = |count: std::num::NonZeroUsize| {
            u64::try_from(count.get())
                .unwrap_or(u64::MAX)
                .saturating_mul(1 << 20)
        };
        let viewer = &self.config.viewer;
        let sizes = ViewerSizes {
            part: mebibytes(viewer.part_mib),
            window: mebibytes(viewer.window_mib),
        };
        let mut viewer = Viewer::new(location_text(&location), sizes);
        if let Some(line) = line {
            let line = usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX);
            viewer.handle(ViewerCommand::Top(line));
        }
        self.viewing = Some(Viewing {
            id: self.last_job,
            viewer,
            wait: Wait::default(),
            location,
            host,
            reading: None,
            finished: None,
            cancel: CancellationToken::new(),
        });
        let mut effects = self.read_more();
        effects.extend(self.note(dir));
        effects
    }

    /// Reads the part the viewer wants next, unless it reads one already.
    fn read_more(&mut self) -> Vec<Effect> {
        let Some(viewing) = &mut self.viewing else {
            return Vec::new();
        };
        if viewing.reading.is_some() {
            return Vec::new();
        }
        let Some(want) = viewing.viewer.want() else {
            return Vec::new();
        };
        let stop = CancellationToken::new();
        viewing.reading = Some(Reading::new(want, stop.clone()));
        vec![Effect::Read(ReadRequest {
            id: viewing.id,
            location: viewing.location.clone(),
            host: viewing.host.clone(),
            want,
            stop,
            cancel: viewing.cancel.clone(),
        })]
    }

    /// Edits the file under the cursor of the active panel with F4: a local one where it is, a
    /// remote one as a local copy, which goes back if it changes.
    fn edit(&mut self) -> Vec<Effect> {
        let panel = self.panel(self.active);
        let dir = panel.location().clone();
        let Some(name) = panel
            .entry_under_cursor()
            .filter(|entry| !entry.is_dir_like())
            .map(|entry| entry.name.clone())
        else {
            return Vec::new();
        };
        self.edit_file(dir, &name, None)
    }

    /// Edits the file `name` in `dir`, at `line` if there is one: a local one where it is, a
    /// remote one as a local copy, which goes back if it changes.
    fn edit_file(&mut self, dir: Location, name: &[u8], line: Option<u64>) -> Vec<Effect> {
        let location = child(&dir, name);
        let Location::Remote { host, .. } = &location else {
            if let Location::Local(path) = location {
                let noted = self.note(&dir);
                self.editing = Some(Editing {
                    file: path.clone(),
                    remote: None,
                    dir,
                    line,
                });
                self.handover = Some(Handover::Edit { file: path, line });
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
        temporary.extend_from_slice(name);
        let file = self
            .runtime_dir
            .join(std::ffi::OsStr::from_bytes(&temporary));
        let cancel = CancellationToken::new();
        let mut job = Job::new(
            id,
            JobKind::Transfer(transfer::Kind::Copy),
            Vec::new(),
            cancel.clone(),
        )
        .then(Then::Edit);
        job.hosts.push(on);
        self.jobs.push(job);
        self.editing = Some(Editing {
            file: file.clone(),
            remote: Some(location.clone()),
            dir,
            line,
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

    /// Opens the marked entries of the active panel, or the one under the cursor, with the
    /// programs of the desktop, as vim's `gx`. On a host, the line of the mode says that only
    /// local files open so.
    fn system_open(&mut self) -> Vec<Effect> {
        let panel = self.panel(self.active);
        let dir = panel.location().clone();
        let chosen = panel.chosen();
        if chosen.is_empty() {
            return Vec::new();
        }
        let Location::Local(here) = &dir else {
            self.message = Some(fl!("message-open-remote"));
            return Vec::new();
        };
        let paths = chosen
            .iter()
            .map(|entry| here.join(std::ffi::OsStr::from_bytes(&entry.name)))
            .collect();
        let mut effects = vec![Effect::Open(paths)];
        effects.extend(self.note(&dir));
        effects
    }

    /// Takes the failure of an [`Effect::Open`]: the path that did not open, and why.
    pub(crate) fn open_failed(&mut self, path: &Path, reason: &str) {
        let path = location_text(&Location::Local(path.to_path_buf()));
        let reason = cells::sanitize(reason.as_bytes());
        self.show_error(&fl!("open-error", path = path, reason = reason));
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
            Location::Local(_) => None,
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
        let job = Job::new(
            id,
            JobKind::Transfer(transfer::Kind::Copy),
            vec![editing.dir],
            cancel.clone(),
        )
        .then(then);
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
    /// Commander, `q :` with the window of the history; in a panel on a local directory or on
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
        };
        if opens {
            let line = CommandLine::new(kind);
            if action == Action::CommandHistory {
                self.command_line = Some(line.for_history());
                self.open_history();
            } else {
                self.command_line = Some(line);
            }
        }
    }

    /// Gives a key to the command line: Enter hands its command to the event loop, to run in
    /// the active panel's directory, Tab completes, and the list of completions takes the keys
    /// while it shows.
    fn handle_command_line(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(effects) = self.change_dir(Completes::CommandLine, input) {
            return effects;
        }
        if self.choose(Completes::CommandLine, input) {
            return Vec::new();
        }
        if input == Resolved::Action(Action::Complete) {
            return self.complete_command();
        }
        // Any other key makes a completion on its way too late.
        self.completion = None;
        let Some(line) = &mut self.command_line else {
            return Vec::new();
        };
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        let event = line.handle_typed(typed, &mut self.registers);
        match event {
            CommandEvent::None => Vec::new(),
            CommandEvent::Close => {
                self.command_line = None;
                Vec::new()
            }
            CommandEvent::Older | CommandEvent::Newer => {
                let host = match self.panel(self.active).location() {
                    Location::Remote { host, .. } => Some(host.as_str()),
                    Location::Local(_) => None,
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
            CommandEvent::Unknown(text) => {
                let text = cells::sanitize(text.as_bytes());
                self.show_error(&fl!("command-unknown", command = text));
                Vec::new()
            }
            CommandEvent::Run(command) => {
                self.command_line = None;
                let dir = self.panel(self.active).location().clone();
                let mut shell = &self.config.shell.run;
                let place = match &dir {
                    Location::Local(path) => Place::Local(path.clone()),
                    Location::Remote { host, path } => {
                        let Some(Host::Connected { handle, .. }) = self.hosts.get(host) else {
                            self.show_error(&fl!("error-connection-closed"));
                            return Vec::new();
                        };
                        if let Some(run) = self.host_settings.get(host).and_then(HostConfig::run) {
                            shell = run;
                        }
                        Place::Remote {
                            handle: handle.clone(),
                            dir: path.clone(),
                        }
                    }
                };
                let shell = shell.clone();
                let mut effects = self.remember(&dir, &command);
                self.handover = Some(Handover::Run(Run {
                    place,
                    shell,
                    command,
                    prompt: cells::text_of(&self.shell_prompt(u16::MAX)),
                    pause: self.config.shell.pause,
                }));
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
            Location::Local(_) => None,
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
        let input = self.search_opens(Slot::CommandHistory);
        self.history_window = Some(HistoryWindow::new(&self.history, here, labels, input));
    }

    /// Gives a key to the window of the command history: a command it takes goes on the command
    /// line, which shows whether it ran on another host; `d d` and F8 remove one.
    fn handle_history(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.history_window else {
            return Vec::new();
        };
        match window.handle(input) {
            HistoryEvent::Pending => Vec::new(),
            HistoryEvent::Closed => {
                self.history_window = None;
                if self.command_line.as_ref().is_some_and(CommandLine::alone) {
                    self.command_line = None;
                }
                Vec::new()
            }
            HistoryEvent::Take { command, host } => {
                self.history_window = None;
                let here = match self.panel(self.active).location() {
                    Location::Remote { host, .. } => Some(host.as_str()),
                    Location::Local(_) => None,
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

    /// Gives a key to the key hints with the keys: a command run from them runs where the keys
    /// went before they opened, with the count and the register typed before them.
    fn handle_key_hints(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.key_hints else {
            return Vec::new();
        };
        match window.handle(input) {
            KeyHintsEvent::Pending => Vec::new(),
            KeyHintsEvent::Closed => {
                self.key_hints = None;
                Vec::new()
            }
            KeyHintsEvent::Run(typed) => {
                self.key_hints = None;
                let extra = self.extra;
                let effects = self.typed(typed);
                self.extra = extra;
                effects
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
    /// the [`shell_prompt`](Self::shell_prompt).
    fn command_prompt(&self, width: u16) -> Vec<Span<'static>> {
        let Some(line) = &self.command_line else {
            return Vec::new();
        };
        if line.kind() == command::Kind::Noc {
            return vec![Span::raw(":")];
        }
        self.shell_prompt(width)
    }

    /// The prompt of a shell command in the active panel, as a shell's: `user@host:dir`, with
    /// `~` for the home directory and the host's label if it has one, and `$`, or `#` where the
    /// user is root. In a third of `width` at most: the middle of the directory cut first, then
    /// the user left out, then the directory.
    fn shell_prompt(&self, width: u16) -> Vec<Span<'static>> {
        let room = usize::from(width / 3).max(1);
        let dir = match self.panel(self.active).location() {
            Location::Local(path) => Some(home_text(path, &self.home)),
            Location::Remote { path, .. } => {
                (!path.as_bytes().is_empty()).then(|| cells::sanitize(path.as_bytes()))
            }
        };
        let sign = if self.active_root() { " # " } else { " $ " };
        let mut spans = match (self.active_who(), dir) {
            (Some(who), dir) => self.who_prompt(who, dir.as_deref(), room),
            (None, Some(dir)) => self.prompt_dir(&dir, room),
            (None, None) => Vec::new(),
        };
        spans.push(Span::raw(sign));
        spans
    }

    /// `user@host:dir` in `room` cells, as [`shell_prompt`](Self::shell_prompt) cuts it.
    fn who_prompt(
        &self,
        (user, host): (Option<String>, String),
        dir: Option<&str>,
        room: usize,
    ) -> Vec<Span<'static>> {
        let style = self.who_style();
        let whole = user.map(|user| format!("{user}@{host}"));
        for who in whole.iter().chain([&host]) {
            let width = cells::width(who);
            match dir {
                None if width <= room => return vec![Span::styled(who.clone(), style)],
                Some(dir) => {
                    let left = room.saturating_sub(width + 1);
                    if left > 0 && left >= cells::width(dir).min(PROMPT_DIR_WIDTH) {
                        let mut spans = vec![Span::styled(who.clone(), style), Span::raw(":")];
                        spans.extend(self.prompt_dir(dir, left));
                        return spans;
                    }
                }
                None => {}
            }
        }
        let styles = (style, self.theme.truncated);
        let room = room.min(cells::width(&host));
        cells::fit(&host, room, Align::Left, styles, self.theme.glyphs.marks)
    }

    /// `dir` in `room` cells, its middle cut if it is wider.
    fn prompt_dir(&self, dir: &str, room: usize) -> Vec<Span<'static>> {
        if cells::width(dir) > room {
            let styles = (Style::new(), self.theme.truncated);
            cells::fit(dir, room, Align::Left, styles, self.theme.glyphs.marks)
        } else {
            vec![Span::raw(dir.to_owned())]
        }
    }

    /// Who is where in the active panel: the user, if known, and the machine's name or the
    /// host's label or alias, terminal-safe; `None` in a local panel while the machine has no
    /// name.
    fn active_who(&self) -> Option<(Option<String>, String)> {
        match self.panel(self.active).location() {
            Location::Local(_) => {
                let host = self.machine.host.clone()?;
                Some((self.machine.user.clone(), host))
            }
            Location::Remote { host, .. } => Some((
                self.users.get(host).cloned(),
                cells::sanitize(self.host_label(host).as_bytes()),
            )),
        }
    }

    /// The user is root in the active panel, as [`root_at`](Self::root_at) tells.
    fn active_root(&self) -> bool {
        self.root_at(self.panel(self.active).location())
    }

    /// The user is root at `location`: a local one while Noon Commander runs as root, on a
    /// host as the user that `ssh -G` gave.
    fn root_at(&self, location: &Location) -> bool {
        match location {
            Location::Local(_) => self.machine.root,
            Location::Remote { host, .. } => self
                .users
                .get(host)
                .is_some_and(|user| user == machine::ROOT),
        }
    }

    /// The style of `user@host` in the prompt and on the line of the mode.
    fn who_style(&self) -> Style {
        if self.active_root() {
            self.theme.root
        } else {
            Style::new()
        }
    }

    /// On the line of the mode, left of what is typed: the glyph of root while Noon Commander
    /// runs as root, whichever panel is active, and `user@host` of the active panel.
    fn mode_who(&self) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        if self.machine.root {
            let glyph = self.theme.glyphs.root.to_string();
            spans.push(Span::styled(glyph, self.theme.root));
        }
        if let Some((user, host)) = self.active_who() {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            let who = user.map_or_else(|| host.clone(), |user| format!("{user}@{host}"));
            spans.push(Span::styled(who, self.who_style()));
        }
        spans
    }

    /// The title of the terminal's window or tab while `ui.terminal_title` is on: where the
    /// active panel is, as the prompt of a shell command shows it, after `[root]` while Noon
    /// Commander runs as root or the active panel is on a host as root.
    pub(crate) fn terminal_title(&self) -> Option<String> {
        if !self.config.ui.terminal_title {
            return None;
        }
        let place = self.active_place();
        Some(if self.machine.root || self.active_root() {
            fl!("terminal-title-root", place = place)
        } else {
            fl!("terminal-title", place = place)
        })
    }

    /// The directory of the active panel, with `~` for the home directory, or `host:path` with
    /// the host's label if it has one.
    fn active_place(&self) -> String {
        match self.panel(self.active).location() {
            Location::Local(path) => home_text(path, &self.home),
            Location::Remote { host, path } => location_text(&Location::Remote {
                host: self.host_label(host).to_owned(),
                path: path.clone(),
            }),
        }
    }

    /// The label of `host` from `hosts.toml`, or else its alias.
    fn host_label<'a>(&'a self, host: &'a str) -> &'a str {
        self.host_settings
            .get(host)
            .and_then(HostConfig::label)
            .unwrap_or(host)
    }

    /// Says that the edited copy of `path` could not go back, and where it stays.
    fn keep_edit(&mut self, path: &str, file: &Path) {
        let copy = cells::sanitize(file.as_os_str().as_bytes());
        self.show_error(&fl!("edit-kept", path = path, copy = copy));
    }

    /// Takes the part [`Effect::Read`] read for the viewer `id`, and reads the next one it
    /// wants. A file that changed is read again from the start. An error closes the viewer
    /// while it has nothing to show; after that, the viewer keeps what it holds.
    pub(crate) fn read(&mut self, id: u64, result: Result<ViewerPart, String>) -> Vec<Effect> {
        let Some(viewing) = self.viewing.as_mut().filter(|viewing| viewing.id == id) else {
            return Vec::new();
        };
        let reading = viewing.reading.take();
        match result {
            Ok(part) => {
                if !part.stopped {
                    viewing.finished = reading.and_then(Reading::finish);
                }
                if viewing.viewer.take(part) == Taken::Changed {
                    let path = location_text(&viewing.location);
                    self.message = Some(fl!("viewer-changed", path = path));
                }
            }
            Err(reason) => {
                let path = location_text(&viewing.location);
                let reason = cells::sanitize(reason.as_bytes());
                if viewing.viewer.loading() {
                    self.close_viewer();
                    self.show_error(&fl!("viewer-error", path = path, reason = reason));
                } else {
                    viewing.viewer.stop();
                    self.show_error(&fl!("viewer-read-error", path = path, reason = reason));
                }
                return Vec::new();
            }
        }
        self.read_more()
    }

    /// Shows how far [`Effect::Read`] is with the part of the viewer `id`.
    pub(crate) fn read_progress(&mut self, id: u64, progress: ReadProgress) {
        if let Some(reading) = self
            .viewing
            .as_mut()
            .filter(|viewing| viewing.id == id)
            .and_then(|viewing| viewing.reading.as_mut())
        {
            reading.progress(progress);
        }
    }

    /// Takes a key for the viewer, or for the window of the part it reads while it shows: F1
    /// shows the help over it. A move may ask for another part.
    fn handle_viewer(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(viewing) = &mut self.viewing
            && viewing.window_shown()
            && let Some(reading) = &mut viewing.reading
        {
            match reading.handle(input) {
                Some(ReadButton::Stop) => {
                    reading.stop();
                    viewing.viewer.stop();
                }
                Some(ReadButton::Close) => self.close_viewer(),
                None => {}
            }
            return Vec::new();
        }
        let Resolved::Action(action) = input else {
            return Vec::new();
        };
        match action {
            Action::Help => {
                self.help = Some(self.help_screen());
            }
            Action::Quit | Action::Cancel => self.close_viewer(),
            action => {
                if let Some(viewing) = &mut self.viewing
                    && let Some(command) = viewer_command(action)
                {
                    viewing.viewer.handle(command);
                    return self.read_more();
                }
            }
        }
        Vec::new()
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
            many => {
                let (files, dirs) = files_and_dirs(many);
                fl!("delete-many", files = files, dirs = dirs)
            }
        };
        let names = chosen.iter().map(|entry| entry.name.clone()).collect();
        let dir = panel.location().clone();
        let buttons = vec![Button::Yes, Button::No];
        let dialog = Dialog::question(&fl!("delete-title"), &message, buttons, 0, Tone::Warning);
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
            Location::Local(_) => None,
        };
        let targets = names.iter().map(|name| child(&dir, name)).collect();
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
    /// under the cursor, as mc does: the field opens with the other panel's location.
    fn ask_transfer(&mut self, kind: transfer::Kind) {
        let panel = self.panel(self.active);
        let chosen = panel.chosen();
        let sources = match chosen.as_slice() {
            [] => return,
            [entry] => Sources::One(prompt_name(&entry.name)),
            many => {
                let (files, dirs) = files_and_dirs(many);
                Sources::Many { files, dirs }
            }
        };
        let names = chosen.iter().map(|entry| entry.name.clone()).collect();
        let other = self.panel(self.active.other()).location().clone();
        let text = location_text(&other);
        let field = (text.clone(), text.chars().count());
        self.open_transfer(kind, &sources, names, field, Some((text, other)));
    }

    /// Asks where Shift+F5 copies the entry under the cursor of the active panel, not the
    /// marked ones, as mc does: the field opens with its name, the cursor before its last
    /// extension, as when it is renamed in its row, so that the copy goes beside it under the
    /// name typed. A name that is not UTF-8 cannot be edited, which an error says: F5 copies it.
    fn ask_duplicate(&mut self) {
        let Some(entry) = self.panel(self.active).entry_under_cursor() else {
            return;
        };
        let (name, dir_like) = (entry.name.clone(), entry.is_dir_like());
        let Ok(text) = std::str::from_utf8(&name) else {
            let name = cells::sanitize(&name);
            self.show_error(&fl!("duplicate-not-utf8", name = name));
            return;
        };
        let cursor = stem_chars(text, dir_like);
        // Alone, `~` is the home directory.
        let field = if text == "~" {
            ("\\~".to_owned(), cursor + 1)
        } else {
            (text.to_owned(), cursor)
        };
        let sources = Sources::One(prompt_name(&name));
        self.open_transfer(transfer::Kind::Copy, &sources, vec![name], field, None);
    }

    /// Opens the dialog of F5, F6, and Shift+F5 to do `kind` to `names` in the active panel's
    /// directory, which its prompt names as `sources`: a field that opens with a text and the
    /// cursor before its character of that number, and whether to preserve attributes, for a
    /// copy or a move alike. `offered` is the location that the text stands for while it stays
    /// as it was.
    fn open_transfer(
        &mut self,
        kind: transfer::Kind,
        sources: &Sources,
        names: Vec<Vec<u8>>,
        (text, cursor): (String, usize),
        offered: Option<(String, Location)>,
    ) {
        let dir = self.panel(self.active).location().clone();
        let preserve = self.copy_choices.preserve;
        let dialog = transfer::dialog(kind, sources, (&text, cursor), preserve);
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
        Some(self.panel_of(panel)?.resolve(text))
    }

    /// Starts copying or moving `names` from `dir` to `target`, and shows its progress; a
    /// target that is one of the sources, or in one, is an error.
    fn start_transfer(
        &mut self,
        kind: transfer::Kind,
        dir: Location,
        names: &[Vec<u8>],
        target: Location,
    ) -> Vec<Effect> {
        let sources: Vec<Location> = names.iter().map(|name| child(&dir, name)).collect();
        let error = |reason: String| kind.error(&location_text(&target), &reason);
        if target == dir || sources.contains(&target) {
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
            Location::Local(_) => Ok(None),
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
        let changes = [Some(target.clone()), target.parent(), Some(dir)]
            .into_iter()
            .flatten()
            .collect();
        let job = Job::new(id, JobKind::Transfer(kind), changes, cancel.clone());
        let options = CopyOptions {
            preserve: self.copy_choices.preserve,
            atomic: self.copy_choices.atomic,
            remove_sources: kind == transfer::Kind::Move,
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
        let formats = noc_locale::current();
        let time = |metadata: &Metadata| cells::mtime(metadata.modified, &self.tz, &formats);
        let message = fl!(
            "copy-exists",
            path = location_text(target),
            new_size = new.size,
            new_time = time(new),
            old_size = old.size,
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
        let dialog = Dialog::question(&title, &message, buttons, 1, Tone::Warning);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Conflict { job: id, reply },
        });
    }

    /// Takes a report from the job `id`, as [`App::job_event`] does, and tells whether the
    /// screen changes with it. A job reports every chunk it moves, which come in batches up to
    /// 50 a second, while its window, or its row in the list of jobs, changes only when what it
    /// shows does, and the corner shows only how far the jobs in the background are together,
    /// in percent: the screen draws when one of these changes, not for every batch.
    pub(crate) fn job_report(&mut self, id: u64, event: JobEvent) -> (Vec<Effect>, bool) {
        if !event.is_progress() {
            return (self.job_event(id, event), true);
        }
        let now = Instant::now();
        let shown = self.job_shown(id, now);
        let corner = self.corner();
        let effects = self.job_event(id, event);
        let changes = self.job_shown(id, now) != shown || self.corner() != corner;
        (effects, changes)
    }

    /// What the screen shows at `now` of the job `id` beyond its share of the corner.
    fn job_shown(&self, id: u64, now: Instant) -> Option<Shown> {
        let job = self.jobs.iter().find(|job| job.id == id)?;
        match self.sight(job) {
            Sight::Window => Some(Shown::Window(job.view.look(now))),
            Sight::Row => Some(Shown::Row(job.view.look(now).row())),
            Sight::Corner => None,
        }
    }

    /// Takes a report from the job `id`: progress for its window, a failure for a dialog
    /// that waits for an answer, or its end.
    pub(crate) fn job_event(&mut self, id: u64, event: JobEvent) -> Vec<Effect> {
        let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return Vec::new();
        };
        match event {
            JobEvent::Scanning { items } => job.view.scanning(items),
            JobEvent::Progress { current, reports } => {
                if let Some((&(at, counts), before)) = reports.split_last() {
                    for &(at, counts) in before {
                        job.view.passed(counts, at);
                    }
                    job.view.working(location_text(&current), counts, at);
                }
            }
            JobEvent::Exists {
                at,
                target,
                source_metadata,
                target_metadata,
                reply,
            } => {
                job.view.ask(at);
                let metadata = (&source_metadata, &target_metadata);
                self.ask_conflict(id, &target, metadata, reply);
            }
            JobEvent::Failed {
                at,
                path,
                error,
                reply,
            } => {
                job.view.failed(at);
                let path = location_text(&path);
                let reason = cells::sanitize(error.as_bytes());
                let message = match job.kind {
                    JobKind::Delete => fl!("delete-error", path = path, reason = reason),
                    JobKind::Transfer(kind) => kind.error(&path, &reason),
                    JobKind::Checksum => fl!("checksum-error", path = path, reason = reason),
                };
                let buttons = vec![Button::Skip, Button::SkipAll, Button::Retry, Button::Abort];
                let dialog = Dialog::failure(&message, buttons, 0);
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
                self.handover = self.editing.as_ref().map(|editing| Handover::Edit {
                    file: editing.file.clone(),
                    line: editing.line,
                });
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
            Location::Local(_) => Ok(None),
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
            many => {
                let (files, dirs) = files_and_dirs(many);
                fl!("checksum-many", files = files, dirs = dirs)
            }
        };
        let names: Vec<Vec<u8>> = chosen.iter().map(|entry| entry.name.clone()).collect();
        let dir = panel.location().clone();
        let other_panel = self.panel(self.active.other());
        let other = other_panel
            .entry_under_cursor()
            .filter(|entry| single_file && !entry.is_dir_like())
            .map(|entry| {
                let location = child(other_panel.location(), &entry.name);
                (location, names.first() == Some(&entry.name))
            })
            .filter(|(location, _)| child(&dir, &names[0]) != *location);
        let fields = if single_file {
            vec![(fl!("checksum-expected"), String::new())]
        } else {
            Vec::new()
        };
        let checks: Vec<(String, bool)> = other
            .iter()
            .map(|(location, same)| {
                let path = label::escape(&location_text(location));
                (fl!("checksum-compare", path = path), *same)
            })
            .collect();
        let choices: Vec<String> = Algorithm::ALL.iter().map(|&a| algorithm_name(a)).collect();
        let chosen = Algorithm::ALL
            .iter()
            .position(|&a| a == self.algorithm)
            .unwrap_or(0);
        let buttons = vec![Button::Ok, Button::Cancel];
        let title = fl!("checksum-title");
        let dialog = Dialog::fields(&title, &fields, &checks, buttons)
            .with_message(&message)
            .with_choices(&choices, chosen);
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
        let mut groups = vec![names.iter().map(|name| child(&dir, name)).collect()];
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
        let title = Label::parse(&algorithm_name(hashing.algorithm)).text;
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
                    let dialog = Dialog::form(&title, &prompt, &name, &[]);
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
                location
                    .parent()
                    .map_or_else(Vec::new, |dir| self.reload(&dir))
            }
            Err(None) => {
                let message = fl!("checksum-exists", path = path);
                let buttons = vec![Button::Yes, Button::No];
                let title = fl!("copy-exists-title");
                let dialog = Dialog::question(&title, &message, buttons, 1, Tone::Warning);
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

    /// Opens Go to directory, `g d`, for the active panel, as mc's Quick cd.
    fn ask_go_to(&mut self) {
        self.ask_go_to_in(self.shown(self.active), "");
    }

    /// Opens Go to directory for `panel`, with `text` typed.
    fn ask_go_to_in(&mut self, panel: PanelId, text: &str) {
        let (title, prompt) = (fl!("goto-title"), fl!("goto-prompt"));
        let dialog = Dialog::form(&title, &prompt, text, &[]).with_completion();
        // First, as it may follow a dialog that just closed, ahead of others waiting.
        self.dialogs.push_front(Open {
            dialog,
            purpose: Purpose::GoToDir { panel },
        });
    }

    /// Sends `panel` where `cd <text>` leads from its directory. A path that is not there
    /// leaves the panel where it is and asks what to do, as any listing does.
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

    /// Opens the dialog of F7 for the active panel, with an empty field.
    fn ask_mkdir(&mut self) {
        let (title, prompt) = (fl!("mkdir-title"), fl!("mkdir-prompt"));
        let dialog = Dialog::form(&title, &prompt, "", &[]).with_completion();
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
        self.begin_work(panel, &location);
        let host = match &location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Some(handle.clone()),
                _ => None,
            },
            Location::Local(_) => None,
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
        let effects = match result {
            Ok(()) => self.show_new(panel, location),
            Err(reason) => {
                let path = location_text(location);
                let reason = cells::sanitize(reason.as_bytes());
                self.show_error(&fl!("mkdir-error", path = path, reason = reason));
                Vec::new()
            }
        };
        self.end_work(panel, location);
        effects
    }

    /// `panel` waits for work that makes `location`, with dots on its title, as for a listing.
    fn begin_work(&mut self, panel: PanelId, location: &Location) {
        if let Some(tab) = self.tab_mut(panel) {
            tab.panel.begin_work(location.clone());
        }
    }

    /// `panel` no longer waits for the work that made `location`; called after the listing its
    /// result asks for, so that the dots go on until that comes.
    fn end_work(&mut self, panel: PanelId, location: &Location) {
        if let Some(tab) = self.tab_mut(panel) {
            tab.panel.end_work(location);
        }
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
            if Some(tab.panel.location()) != parent.as_ref() {
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

    /// Starts renaming the entry under the cursor of the active panel in its row, as Shift+F6;
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
    /// Esc in Normal mode leaves it as it was, and other keys edit the name in vim's modes.
    fn handle_rename(&mut self, input: Resolved) -> Vec<Effect> {
        let extra = self.extra;
        let mut registers = std::mem::take(&mut self.registers);
        let panel = self.panel_mut(self.active);
        let action = match (input, panel.rename_field()) {
            (Resolved::Insert(c), Some(field)) => {
                field.insert(c);
                None
            }
            (Resolved::Action(action), Some(field)) => {
                (!field.handle(action, extra, &mut registers)).then_some(action)
            }
            (Resolved::Hotkey(_), _) | (_, None) => None,
        };
        self.registers = registers;
        match action {
            Some(Action::Confirm) => return self.confirm_rename(),
            Some(Action::Cancel) => {
                self.panel_mut(self.active).end_rename();
            }
            // A key that is not the field's is the one command after Ctrl+o.
            Some(_) => {
                if let Some(field) = self.panel_mut(self.active).rename_field() {
                    field.give_up();
                }
            }
            None => {}
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
        self.rename_entry(id, child(&dir, &name), child(&dir, text.as_bytes()), false)
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
        let noted = from.parent().map_or_else(Vec::new, |dir| self.note(&dir));
        self.begin_work(panel, &to);
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
        let effects = self.take_renamed(panel, from, to, result);
        self.end_work(panel, to);
        effects
    }

    fn take_renamed(
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
                let dialog = Dialog::question(&title, &message, buttons, 1, Tone::Warning);
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

    /// Shows `message` in an error dialog, after the dialogs open before it.
    fn show_error(&mut self, message: &str) {
        self.dialogs.push_back(Open {
            dialog: Dialog::error(message),
            purpose: Purpose::Info,
        });
    }

    /// Shows `message` in an error dialog over the dialog in front, which stays open under it.
    fn show_error_over(&mut self, message: &str) {
        self.dialogs.push_front(Open {
            dialog: Dialog::error(message),
            purpose: Purpose::Info,
        });
    }

    /// Opens the dialog of `g +` (`mark`) or `g -` for the active panel, as mc's `+`
    /// and `-`: a pattern, and whether it applies to files only and whether case counts.
    fn ask_pattern(&mut self, mark: bool) {
        let title = if mark {
            fl!("pattern-select")
        } else {
            fl!("pattern-unselect")
        };
        let options = &self.pattern_options;
        let checks = [(fl!("pattern-files-only"), options.files_only)];
        let dialog = Dialog::form(&title, &fl!("pattern-text"), &options.pattern, &checks)
            .with_width(PATTERN_DIALOG_WIDTH)
            .with_search(0, self.search_opens(Slot::Mark));
        let panel = self.shown(self.active);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Pattern { panel, mark },
        });
    }

    /// Marks, or unmarks, what the dialog of `g +` or `g -` asked for. An empty pattern does
    /// nothing.
    fn mark_matching(&mut self, panel: PanelId, mark: bool, dialog: &Dialog) {
        if dialog.text().is_empty() {
            return;
        }
        let options = PatternOptions {
            pattern: dialog.text().to_owned(),
            files_only: dialog.checked(0),
        };
        let input = dialog
            .search(0)
            .unwrap_or_else(|| self.search_opens(Slot::Mark));
        let mut matcher = input.matcher(&options.pattern);
        if let Some(panel) = self.panel_of_mut(panel) {
            panel.mark_where(mark, |entry| {
                !(options.files_only && entry.is_dir_like())
                    && matcher.score(&entry.display_name()).is_some()
            });
        }
        self.pattern_options = options;
    }

    /// Opens the dialog of F4 on the host `name`: its settings from `hosts.toml`.
    fn ask_edit_host(&mut self, name: String) {
        let old = self.host_settings.get(&name).cloned();
        let sftp = match &old {
            Some(HostConfig::Sftp(sftp)) => sftp.clone(),
            None => SftpHost::default(),
        };
        let current = [self.active, self.active.other()]
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
            (
                fl!("host-run"),
                sftp.run.as_ref().map(run_text).unwrap_or_default(),
            ),
        ];
        let checks = [(fl!("host-remember-dir"), sftp.remember_dir)];
        let mut buttons = vec![Button::Ok, Button::Cancel];
        if current.is_some() {
            buttons.insert(1, Button::UseCurrent);
        }
        let title = fl!("host-edit-title", host = cells::sanitize(name.as_bytes()));
        let dialog = Dialog::fields(&title, &fields, &checks, buttons)
            .with_placeholder(HOST_RUN, &run_text(&self.config.shell.run));
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
                if !other_dir.is_empty() && noc_config::local_dir(other_dir, &self.home).is_none() {
                    self.show_error_over(&fl!("host-other-dir-invalid"));
                    return true;
                }
                let run = dialog.text_of(HOST_RUN).trim();
                if let Err(reason) = (!run.is_empty()).then(|| run_of(run)).transpose() {
                    self.show_error_over(&fl!("host-run-invalid", reason = reason));
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Takes the end of an [`Effect::SaveHost`]: the host settings now, which the location menu
    /// shows at once, or why they could not be saved.
    pub(crate) fn host_saved(&mut self, result: Result<Arc<Hosts>, String>) -> Vec<Effect> {
        match result {
            Ok(hosts) => {
                self.host_settings = hosts;
                self.reload_menu()
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
            Some(Host::Connecting { .. } | Host::Stalled { .. }) => Vec::new(),
            None => {
                let host = host.clone();
                self.connect(&host)
            }
        }
    }

    /// Starts connecting to `host`, which is not connected.
    fn connect(&mut self, host: &str) -> Vec<Effect> {
        self.failed.remove(host);
        self.connections += 1;
        let connection = self.connections;
        let stop = CancellationToken::new();
        let state = Host::Connecting {
            connection,
            stop: stop.clone(),
        };
        self.hosts.insert(host.to_owned(), state);
        vec![Effect::Connect {
            host: host.to_owned(),
            connection,
            stop,
        }]
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

    /// The host that F4 and F8 act on: the one under the cursor of the location menu, else the
    /// one the active panel shows.
    fn chosen_host(&self) -> Option<String> {
        if let Some(menu) = &self.menu {
            return menu.chosen_host().map(str::to_owned);
        }
        match self.panel(self.active).location() {
            Location::Remote { host, .. } => Some(host.clone()),
            Location::Local(_) => None,
        }
    }

    /// Closes the connection to the host that F8 acts on, or stops connecting to it.
    fn disconnect_chosen(&mut self) {
        if let Some(host) = self.chosen_host() {
            self.disconnect(&host);
        }
    }

    /// Closes the connection to `host`, or stops connecting to it. Panels on that host stay
    /// there, with their entries toned down, until they read their directories again.
    fn disconnect(&mut self, host: &str) {
        let Some(state) = self.hosts.remove(host) else {
            return;
        };
        state.stop();
        for panel in self.panels_mut() {
            if matches!(state, Host::Connected { .. }) {
                panel.drop_host(host);
            } else if waits_for(panel, host) {
                panel.cancel();
            }
        }
        self.sync_connected();
    }

    /// Takes the address and the user `ssh -G` gave for a host.
    pub(crate) fn resolved(&mut self, host: String, address: String, user: &str) {
        self.users
            .insert(host.clone(), cells::sanitize(user.as_bytes()));
        self.addresses.insert(host, address);
    }

    /// What the location menu and the panels show for a host.
    fn host_state(&self, host: &str) -> HostState {
        let status = match self.hosts.get(host) {
            Some(Host::Connecting { .. }) => HostStatus::Connecting,
            Some(Host::Connected { .. }) => HostStatus::Connected,
            Some(Host::Stalled { .. }) => HostStatus::Stalled,
            None if self.failed.contains(host) => HostStatus::Failed,
            None => HostStatus::Idle,
        };
        HostState {
            status,
            address: self.addresses.get(host).cloned(),
        }
    }

    /// Takes the result of an [`Effect::List`]. A failure asks what to do, unless the panel
    /// went up past a directory that is not there either, on its way to one that is.
    pub(crate) fn listed(
        &mut self,
        panel: PanelId,
        generation: u64,
        result: Result<Listed, ListError>,
    ) -> Vec<Effect> {
        self.sync_connected();
        let Some(tab) = self.tab_mut(panel) else {
            return Vec::new();
        };
        let before = tab.panel.location().clone();
        let failure = tab.panel.listed(generation, result);
        let location = tab.panel.location().clone();
        if location != before {
            tab.previous = Some(before);
            // A new visit, which counts in zoxide again; a jump counted already.
            let arriving = tab.arriving.take();
            tab.noted =
                matches!(&location, Location::Local(path) if arriving.as_ref() == Some(path));
            if let Location::Local(path) = &location
                && let Some(volume) = volume_of(&self.volumes, path)
            {
                self.recent.use_volume(&volume.mount_point);
            }
        }
        if let Location::Remote { host, path } = location
            && !path.as_bytes().is_empty()
        {
            self.last_dirs.insert(host, path);
        }
        match failure {
            Some(failure) => self.list_failed(panel, failure),
            None => Vec::new(),
        }
    }

    /// Reports that `panel` could not read a directory, in a dialog with the ways out: OK,
    /// Retry, Up where it leads elsewhere, Go to. A directory that is not there on the way up
    /// is passed by. Each panel has one such dialog, the last failure's.
    fn list_failed(&mut self, panel: PanelId, failure: Failure) -> Vec<Effect> {
        if failure.climbing
            && failure.error.kind == ListErrorKind::NotFound
            && let Some(parent) = failure.parent
        {
            return self.climb(panel, parent);
        }
        let Some(shown) = self.panel_of(panel) else {
            return Vec::new();
        };
        let up = failure
            .parent
            .as_ref()
            .is_some_and(|parent| parent.location() != shown.location());
        let mut buttons = vec![Button::Ok, Button::Retry];
        if up {
            buttons.push(Button::Up);
        }
        buttons.push(Button::GoTo);
        let first = match failure.error.kind {
            ListErrorKind::NotFound if up && failure.stranded => Button::Up,
            ListErrorKind::NotFound | ListErrorKind::Denied => Button::Ok,
            ListErrorKind::Other => Button::Retry,
        };
        let default = buttons
            .iter()
            .position(|&button| button == first)
            .unwrap_or_default();
        let message = fl!(
            "list-failed",
            path = failure.shown.as_str(),
            reason = failure.error.reason.as_str()
        );
        let dialog = Dialog::failure(&message, buttons, default);
        let open = Open {
            dialog,
            purpose: Purpose::ListFailed { panel, failure },
        };
        let earlier = self.dialogs.iter_mut().find(|open| {
            matches!(open.purpose, Purpose::ListFailed { panel: failed, .. } if failed == panel)
        });
        match earlier {
            Some(earlier) => *earlier = open,
            None => self.dialogs.push_back(open),
        }
        Vec::new()
    }

    /// Takes the button that closed the dialog of [`App::list_failed`].
    fn list_failed_closed(
        &mut self,
        panel: PanelId,
        failure: Failure,
        event: DialogEvent,
    ) -> Vec<Effect> {
        match event {
            DialogEvent::Pressed(Button::Retry) => self.go(panel, failure.destination),
            DialogEvent::Pressed(Button::Up) => match failure.parent {
                Some(parent) => self.climb(panel, parent),
                None => Vec::new(),
            },
            DialogEvent::Pressed(Button::GoTo) => {
                self.ask_go_to_in(panel, &cd_text(failure.destination.location()));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Sends `panel` up to `destination`, and on up past directories that are not there.
    fn climb(&mut self, panel: PanelId, destination: Destination) -> Vec<Effect> {
        let Some(shown) = self.panel_of_mut(panel) else {
            return Vec::new();
        };
        let request = shown.climb(destination);
        self.open(panel, request)
    }

    /// Asks what to do instead of `action` in the panel on `side`, if it needs the host of the
    /// panel and the host is not connected. The entries the panel kept can be looked through.
    fn ask_connect(&mut self, side: Side, action: Action) -> bool {
        let panel = self.panel(side);
        if works_unconnected(action) || (works_offline(action) && panel.has_entries()) {
            return false;
        }
        let Some(host) = panel.not_connected() else {
            return false;
        };
        let message = fl!(
            "reconnect-not-connected",
            host = cells::sanitize(host.as_bytes())
        );
        let buttons = vec![Button::Connect, Button::Local, Button::Cancel];
        let dialog = Dialog::question(&fl!("reconnect-title"), &message, buttons, 0, Tone::Error);
        let panel = self.shown(side);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::NotConnected { panel },
        });
        true
    }

    /// Takes the button that closed the dialog of [`App::ask_connect`].
    fn not_connected_closed(&mut self, panel: PanelId, event: DialogEvent) -> Vec<Effect> {
        let Some(shown) = self.panel_of(panel) else {
            return Vec::new();
        };
        let destination = match event {
            DialogEvent::Pressed(Button::Connect) => shown.here(),
            DialogEvent::Pressed(Button::Local) => shown
                .last_local()
                .unwrap_or_else(|| Destination::to(Location::Local(self.home.clone()))),
            _ => return Vec::new(),
        };
        self.go(panel, destination)
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
        let (Host::Connecting { stop, .. } | Host::Connected { stop, .. }) = state else {
            return Vec::new();
        };
        *state = Host::Connected {
            connection,
            stop: stop.clone(),
            handle,
        };
        self.recent.use_host(host);
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
        match self.hosts.get(host) {
            Some(state @ (Host::Connecting { .. } | Host::Connected { .. }))
                if state.connection() == connection => {}
            _ => return Vec::new(),
        }
        let Some(state) = self.hosts.remove(host) else {
            return Vec::new();
        };
        if reason.is_some() {
            self.failed.insert(host.to_owned());
        }
        let mut effects = Vec::new();
        // Their tasks dropped the jobs on the host, whose panels show nothing of it until it is
        // back, so there is nothing to read.
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
        match reason {
            Some(reason) => {
                self.hosts
                    .insert(host.to_owned(), Host::Stalled { connection });
                let lost = matches!(state, Host::Connected { .. });
                if lost {
                    for panel in self.panels_mut() {
                        panel.lose_host(host);
                    }
                }
                self.ask_reconnect(host, reason, lost);
            }
            None => {
                for panel in self.panels_mut() {
                    if matches!(state, Host::Connected { .. }) {
                        panel.drop_host(host);
                    } else if waits_for(panel, host) {
                        panel.cancel();
                    }
                }
            }
        }
        self.sync_connected();
        effects
    }

    /// Asks whether to connect to `host` again, after the attempt failed for `reason` or, if
    /// `lost`, the connection was lost.
    fn ask_reconnect(&mut self, host: &str, reason: &str, lost: bool) {
        let shown = cells::sanitize(host.as_bytes());
        let why = cells::sanitize(reason.as_bytes());
        let message = if lost {
            fl!("reconnect-lost", host = shown, reason = why)
        } else {
            fl!("reconnect-failed", host = shown, reason = why)
        };
        let buttons = vec![Button::Retry, Button::Cancel];
        let dialog = Dialog::question(&fl!("reconnect-title"), &message, buttons, 0, Tone::Error);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Reconnect {
                host: host.to_owned(),
                reason: reason.to_owned(),
            },
        });
    }

    /// Tries `host` again, for the panels that wait for it, after Retry.
    fn reconnect(&mut self, host: &str) -> Vec<Effect> {
        if !matches!(self.hosts.get(host), Some(Host::Stalled { .. })) {
            return Vec::new();
        }
        self.hosts.remove(host);
        let effects = self.connect(host);
        self.sync_connected();
        effects
    }

    /// Stops waiting for `host`, which could not be reached for `reason`, after Cancel: the
    /// panels that were to open it stay where they were; those on it stay there with their
    /// entries toned down, until a key in one asks to connect. The dialog said why already.
    fn give_up(&mut self, host: &str, reason: &str) {
        if !matches!(self.hosts.get(host), Some(Host::Stalled { .. })) {
            return;
        }
        self.hosts.remove(host);
        for panel in self.panels_mut() {
            let Some(request) = panel.pending_request() else {
                continue;
            };
            if !waits_for(panel, host) {
                continue;
            }
            if is_on(panel.location(), host) {
                panel.drop_host(host);
            } else {
                panel.listed(request.generation, Err(ListError::other(reason.to_owned())));
            }
        }
        self.sync_connected();
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

    /// When the screen needs drawing again, after the frame drawn at `drawn`, for what changes
    /// as time passes: a spinner that is due to show, or that turns; a host that connects in the
    /// location menu; the second that the time of a working job shows, with its speed and time
    /// left, in its window or in the list of jobs.
    pub(crate) fn spin_due(&self, drawn: Instant) -> Option<Instant> {
        let turns = self.config.animations.spinners;
        let mut due: Option<Instant> = None;
        let mut at = |time: Instant| due = Some(due.map_or(time, |due| due.min(time)));
        for wait in self.waits() {
            match wait.shows() {
                Some(shows) if shows > drawn => at(shows),
                Some(_) if turns => at(drawn + wait.step_after(drawn)),
                _ => {}
            }
        }
        let connecting = self
            .hosts
            .values()
            .any(|host| matches!(host, Host::Connecting { .. }));
        // The dots of a host that connects turn in the location menu and in the titles of panels.
        if turns && connecting && self.viewing.is_none() {
            at(drawn + spinner::DOTS_FRAME);
        }
        let ticks = self
            .jobs
            .iter()
            .filter(|job| match self.sight(job) {
                Sight::Window => true,
                Sight::Row => job.view.moves_data(),
                Sight::Corner => false,
            })
            .filter_map(|job| job.view.next_tick(drawn));
        for tick in ticks {
            at(tick);
        }
        let finished = self
            .viewing
            .as_ref()
            .and_then(|viewing| viewing.finished.as_ref());
        if let Some(goes) = finished.and_then(Reading::goes) {
            at(goes);
        }
        due
    }

    /// What waits on screen, each with a spinner.
    fn waits(&self) -> Vec<&Wait> {
        if let Some(viewing) = &self.viewing {
            return viewing
                .viewer
                .loading()
                .then_some(&viewing.wait)
                .into_iter()
                .chain(viewing.reading.as_ref().and_then(Reading::wait))
                .collect();
        }
        let panels = [Side::Left, Side::Right].map(|side| {
            let panel = self.panel(side);
            panel.wait().filter(|_| !self.stalls(panel))
        });
        panels
            .into_iter()
            .flatten()
            .chain(self.menu.as_ref().and_then(LocationMenu::wait))
            .chain(self.jump.as_ref().and_then(JumpMenu::wait))
            .chain(self.find.as_ref().and_then(FindWindow::wait))
            .chain(self.in_front().and_then(|job| job.view.shining()))
            .collect()
    }

    /// Whether `panel` waits to read a directory on a host that does not answer, whose dialog
    /// asks what to do: a question, not a wait, so nothing turns until the answer.
    fn stalls(&self, panel: &Panel) -> bool {
        self.hosts
            .iter()
            .any(|(host, state)| matches!(state, Host::Stalled { .. }) && waits_for(panel, host))
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
    /// `ui.menu_bar`, else over their top line while a menu is open. Dates are as of `now`;
    /// windows that open or close grow or shrink as of `at`, with `animations.windows`, and
    /// spinners turn.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, at: Instant, tz: &TimeZone) {
        self.spots = Spots::default();
        let always = self.config.ui.menu_bar == MenuBar::Always;
        let bar_height = u16::from(always);
        let screen = frame.area();
        let mut stage = std::mem::take(&mut self.stage);
        let scene = (screen, self.viewing.is_some());
        stage.begin(self.config.animations.windows, scene, &self.theme, at);
        let prompt = self.command_prompt(screen.width);
        let prompt_text = cells::text_of(&prompt);
        let line_height = self.line_height(&prompt_text, screen);
        let [menu_bar, panels, full, command_line, key_bar] = Layout::vertical([
            Constraint::Length(bar_height),
            Constraint::Fill(1),
            Constraint::Length(self.full_height()),
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
        let formats = noc_locale::current();
        let spin = self.spin(at);
        let view = View {
            hosts: &hosts,
            decor: self.decor,
            theme: &self.theme,
            spin,
            tz,
            formats: &formats,
        };
        let root_theme = self.theme.for_root();
        let root_view = View {
            theme: &root_theme,
            ..view
        };
        let roots = [Side::Left, Side::Right].map(|side| self.root_at(self.panel(side).location()));
        if let Some(viewing) = &mut self.viewing {
            viewing.render(frame, panels, (&self.theme, self.decor), &spin);
        } else {
            let sides = [
                (Side::Left, &mut self.left, left_bar, left),
                (Side::Right, &mut self.right, right_bar, right),
            ];
            for ((side, tabs, line, area), root) in sides.into_iter().zip(roots) {
                let focused = active == side;
                let view = if root { &root_view } else { &view };
                let spots = render_side(frame, tabs, (line, area), focused, view, &self.home);
                let whole = line.map_or(area, |line| line.union(area));
                self.spots.sides.push((side, whole));
                let tabs = spots.into_iter().map(|(index, spot)| (side, index, spot));
                self.spots.tabs.extend(tabs);
            }
            self.render_full(frame, full);
        }
        self.spots.fkeys = self.render_fkeys(frame, key_bar);
        self.render_command_line(frame, command_line, &prompt);
        if always {
            self.spots.menu_bar = pulldown::render_idle(frame, menu_bar, &self.theme);
        }
        self.render_pulldown(frame, menu_bar, &mut stage);
        if self.viewing.is_none() {
            // On the menu bar, or on the top line of the panels without it.
            self.render_jobs(frame, menu_bar);
            self.render_menus(frame, panels, (&hosts, &spin), &mut stage);
        }
        self.render_windows(frame, panels, &spin, &mut stage);
        self.render_command_choices(frame, command_line, &prompt_text);
        if let Some(window) = &mut self.key_hints {
            stage.show(
                frame,
                (zoom::Window::KeyHints, Grows::FromMiddle),
                |frame| {
                    window.render(frame, panels, &self.theme);
                },
            );
        }
        stage.end(frame);
        self.stage = stage;
        if let Some(hints) = &self.hints {
            which_key::render(frame, panels, hints, &self.theme);
        }
    }

    /// The spinners as of `at`, counted from the first frame drawn.
    fn spin(&self, at: Instant) -> Spin {
        Spin::new(
            at,
            self.epoch.since(at),
            self.config.animations.spinners,
            self.theme.glyphs.spinners,
        )
    }

    /// The windows of a panel, centered over `panels` as every window is: the location menu,
    /// the tab menu, and the zoxide window, which is the active panel's; they grow and shrink
    /// on `stage`.
    fn render_menus(
        &mut self,
        frame: &mut Frame<'_>,
        panels: Rect,
        (hosts, spin): (&dyn Fn(&str) -> HostState, &Spin),
        stage: &mut Stage,
    ) {
        let (theme, decor) = (&self.theme, self.decor);
        if let Some(menu) = &mut self.menu {
            let side = menu.side();
            stage.show(
                frame,
                (zoom::Window::Menu(side), Grows::FromMiddle),
                |frame| {
                    menu.render(frame, panels, theme, decor, hosts, spin);
                },
            );
        }
        if let Some(menu) = &mut self.tab_menu {
            let side = menu.side();
            stage.show(
                frame,
                (zoom::Window::TabMenu(side), Grows::FromMiddle),
                |frame| {
                    menu.render(frame, panels, theme);
                },
            );
        }
        if let Some(jump) = &mut self.jump {
            let side = self.active;
            stage.show(
                frame,
                (zoom::Window::Jump(side), Grows::FromMiddle),
                |frame| {
                    jump.render(frame, panels, theme, spin);
                },
            );
        }
    }

    /// The pull-down menu of F9, if it is open: its bar at `menu_bar`, its menu below, which
    /// drops from it on `stage`.
    fn render_pulldown(&self, frame: &mut Frame<'_>, menu_bar: Rect, stage: &mut Stage) {
        if let Some(pulldown) = &self.pulldown {
            let screen = Rect {
                height: frame.area().height.saturating_sub(1),
                ..frame.area()
            };
            stage.show(frame, (zoom::Window::PullDown, Grows::Down), |frame| {
                pulldown.render(frame, (menu_bar, screen), &self.theme, &|command| {
                    self.command_status(command)
                });
            });
        }
    }

    /// The rows of the frame below the panels with the whole name under the cursor, while `K`
    /// asks for it.
    fn full_height(&self) -> u16 {
        if self.whole == Whole::Shown && self.viewing.is_none() {
            FULL_ROWS + 2
        } else {
            0
        }
    }

    /// The whole name under the cursor of the active panel, in a frame at `area`, below the
    /// panels, as `K` asks for it: single lines, in the colors of dialogs, which set it apart
    /// from the panels.
    fn render_full(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.height < 3 {
            return;
        }
        let theme = &self.theme;
        let block = Block::bordered()
            .border_type(BorderType::Plain)
            .style(theme.dialog)
            .border_style(theme.dialog);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let name = self.panel(self.active).full_name().unwrap_or_default();
        let size = (usize::from(inner.width), usize::from(inner.height));
        let styles = (theme.dialog, theme.truncated_dialog);
        let rows = cells::whole(&name, size, None, styles, theme.glyphs.marks);
        for (y, row) in (inner.y..inner.bottom()).zip(rows) {
            frame.render_widget(row, Rect::new(inner.x, y, inner.width, 1));
        }
    }

    /// The windows centered over `panels`, those in front last: the workspaces, the
    /// Configuration dialog, the help, the jobs, the checksums, and the dialogs, those for one
    /// panel too; they grow and shrink on `stage`.
    fn render_windows(
        &mut self,
        frame: &mut Frame<'_>,
        panels: Rect,
        spin: &Spin,
        stage: &mut Stage,
    ) {
        let middle = |window| (window, Grows::FromMiddle);
        let theme = &self.theme;
        if let Some(window) = &mut self.workspaces_window {
            stage.show(frame, middle(zoom::Window::Workspaces), |frame| {
                window.render(frame, panels, theme);
            });
        }
        if let Some(window) = &mut self.history_window {
            stage.show(frame, middle(zoom::Window::History), |frame| {
                window.render(frame, panels, theme);
            });
        }
        if let Some(window) = &mut self.find
            && self.viewing.is_none()
        {
            stage.show(frame, middle(zoom::Window::Find), |frame| {
                window.render(frame, panels, theme, spin);
            });
        }
        if let Some(configuration) = &mut self.configuration {
            stage.show(frame, middle(zoom::Window::Configuration), |frame| {
                configuration.render(frame, panels, theme);
            });
        }
        if let Some(help) = &mut self.help {
            stage.show(frame, middle(zoom::Window::Help), |frame| {
                help.render(frame, panels, theme);
            });
        }
        if let Some(list) = &self.jobs_list {
            let rows = self.job_rows();
            stage.show(frame, middle(zoom::Window::JobsList), |frame| {
                list.render(frame, panels, theme, &rows);
            });
        }
        if let Some(job) = self.in_front() {
            stage.show(frame, middle(zoom::Window::Job(job.id)), |frame| {
                job.view.render(frame, panels, theme, (spin.at(), spin));
            });
        }
        if let Some(results) = self.results.front() {
            stage.show(frame, middle(zoom::Window::Results(results.id)), |frame| {
                results.window.render(frame, panels, theme);
            });
        }
        if let Some(open) = self.dialogs.front() {
            stage.show(
                frame,
                middle(zoom::Window::Dialog(open.dialog.id())),
                |frame| {
                    open.dialog.render(frame, panels, theme);
                },
            );
            if let Some(choices) = choices_for(&mut self.completion, Completes::Field)
                && let Some(field) = open.dialog.field_area()
            {
                choices.render(frame, field, panels, &self.theme);
            }
        }
    }

    /// The list of completions over the command line in `line`, after `prompt`, if it shows:
    /// its rows start where the names completed start on the line.
    fn render_command_choices(&mut self, frame: &mut Frame<'_>, line: Rect, prompt: &str) {
        if !self.typing_command() {
            return;
        }
        let Some(command_line) = &self.command_line else {
            return;
        };
        let Some(completing) = self
            .completion
            .as_mut()
            .filter(|completing| completing.target == Completes::CommandLine)
        else {
            return;
        };
        let column = command_line.column_of(prompt, line.width, completing.keep.chars().count());
        let Some(choices) = &mut completing.choices else {
            return;
        };
        let width = choices.width().min(line.width.saturating_sub(2));
        // On the screen, with room for the frame on both sides.
        let x = line
            .x
            .saturating_add(column)
            .min(line.right().saturating_sub(width + 1))
            .max(line.x + 1);
        let field = Rect::new(x, line.y, width, 1);
        // Above the first row of the line.
        let screen = frame.area();
        let above = Rect::new(screen.x, screen.y, screen.width, field.bottom() - screen.y);
        choices.render(frame, field, above, &self.theme);
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

    /// What the corner shows of the jobs in the background: how many, and how far they are
    /// together, in percent; nothing without any, or while the viewer covers the panels.
    fn corner(&self) -> Option<(usize, u64)> {
        if self.viewing.is_some() {
            return None;
        }
        let behind: Vec<&Job> = self.jobs.iter().filter(|job| job.background).collect();
        if behind.is_empty() {
            return None;
        }
        let ratio = behind.iter().map(|job| job.view.ratio()).sum::<f64>();
        // A few jobs, each at 0 … 1.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let percent = (ratio / behind.len() as f64 * 100.0).round() as u64;
        Some((behind.len(), percent))
    }

    /// The jobs in the background at the top right of `area`, where Far has its clock: how
    /// many, and how far they are together.
    fn render_jobs(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some((count, percent)) = self.corner() else {
            return;
        };
        let text = format!(
            " {} ",
            fl!("jobs-running", count = count, percent = percent)
        );
        // Inside the corner of the right panel's frame.
        let room = usize::from(area.width.saturating_sub(2));
        let width = cells::width(&text).min(room);
        let styles = (self.theme.panel_title_active, self.theme.truncated_cursor);
        let text = cells::fit(&text, width, Align::Left, styles, self.theme.glyphs.marks);
        let width = u16::try_from(width).unwrap_or(0);
        let x = area.right().saturating_sub(width + 1);
        let row = Rect::new(x, area.y, width, 1.min(area.height));
        frame.render_widget(Line::from(text), row);
    }

    /// The rows above the F-key bar on `screen`, as vim's command line: the command line's,
    /// after `prompt`, while it has the keys; else the line of the mode and of the command
    /// typed.
    fn line_height(&self, prompt: &str, screen: Rect) -> u16 {
        match &self.command_line {
            Some(line) if self.typing_command() => line.height(prompt, screen.width, screen.height),
            _ => 1,
        }
    }

    /// Whether the command line is open and has the keys, and so takes the line of the mode.
    fn typing_command(&self) -> bool {
        self.command_line.is_some()
            && matches!(
                self.context(),
                Context::CommandLine | Context::CommandCompletion
            )
    }

    /// The command line in `area`, after `prompt`, while it has the keys; else the mode and
    /// what the pending command has typed, on the line it takes.
    fn render_command_line(&mut self, frame: &mut Frame<'_>, area: Rect, prompt: &[Span<'static>]) {
        let typing = self.typing_command();
        match &mut self.command_line {
            Some(line) if typing => {
                let bar = self.theme.glyphs.scroll_bar;
                line.render(frame, area, (prompt, Style::default()), bar);
            }
            _ => self.render_mode(frame, area),
        }
    }

    /// The mode in capitals, as lualine shows it, then the window that has the keys, in `room`
    /// cells: the window goes first where they are too few, then the mode is cut. Returns how
    /// many cells they take whole. Every mode shows, so that the line always says where keys
    /// go: Normal mode and quick search too, which vim's `showmode` does not.
    fn mode_spans(&self, room: usize) -> (Vec<Span<'static>>, usize) {
        let once = self.focused_editor().is_some_and(Editor::once);
        let mode = match self.place() {
            (Mode::Insert, _) => fl!("mode-insert"),
            (Mode::Normal, _) if once => fl!("mode-insert-once"),
            (Mode::Visual, _) if once => fl!("mode-insert-visual"),
            (Mode::Visual, Context::Panel) if self.panel(self.active).visual_line() => {
                fl!("mode-visual-line")
            }
            (Mode::Visual, _) => fl!("mode-visual"),
            (Mode::Normal, _) => fl!("mode-normal"),
        };
        // While the command of Ctrl+x has the keys, as vim says that Insert mode comes back.
        let other =
            (self.returns).and_then(|_| self.keymap.short_key(Context::Panel, Action::OtherPanel));
        let mode = match other {
            Some(key) => fl!("mode-other", key = key, mode = mode),
            None => mode,
        };
        let bold = Style::new().bold();
        let mode_width = cells::width(&mode);
        let window = window_kind(self.context())
            .map(|kind| format!(" {} {kind}", glyphs::MODE_SEPARATOR))
            .filter(|window| mode_width + cells::width(window) <= room);
        if let Some(window) = window {
            let width = mode_width + cells::width(&window);
            return (vec![Span::styled(mode, bold), Span::raw(window)], width);
        }
        let styles = (bold, self.theme.truncated);
        let spans = cells::fit(&mode, room, Align::Left, styles, self.theme.glyphs.marks);
        (spans, mode_width)
    }

    /// The mode and the window that has the keys on the left of `area`, and what the pending
    /// command has typed, as vim's `showcmd` shows it, on the right.
    fn render_mode(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.is_empty() {
            return;
        }
        let editor = self.focused_editor();
        let mut typed = format!(
            "{}{}",
            self.pending_operator().unwrap_or_default(),
            self.showcmd
        );
        // With nothing typed, Visual mode shows how much it selects, as vim's `showcmd` does.
        if typed.is_empty() && self.mode() == Mode::Visual {
            let size = match editor {
                Some(editor) => editor.selection_size(),
                None => self.panel(self.active).visual_size(),
            };
            typed = size
                .and_then(|size| u64::try_from(size).ok())
                .map(|size| noc_locale::current().integer(size))
                .unwrap_or_default();
        }
        // As vim's, the last ten cells of what is typed.
        let typed: String = typed
            .chars()
            .rev()
            .take(SHOWCMD_WIDTH)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let width = usize::from(area.width);
        let typed = cells::sanitize(typed.as_bytes());
        let shown = cells::width(&typed).min(width);
        let room = width.saturating_sub(shown + 1);
        // A message takes the place of the mode, as on vim's command line.
        let marks = self.theme.glyphs.marks;
        let left_width = if let Some(message) = &self.message {
            let styles = (self.theme.message, self.theme.truncated);
            let message_line = cells::fit(message, room, Align::Left, styles, marks);
            frame.render_widget(Line::from(message_line), area);
            cells::width(message)
        } else {
            let (mode_line, left_width) = self.mode_spans(room);
            frame.render_widget(Line::from(mode_line), area);
            left_width
        };
        // Left of the cells of what is typed, as long as it fits after the mode or message.
        let who = self.mode_who();
        let who_width = cells::width(&cells::text_of(&who));
        let reserved = SHOWCMD_WIDTH + 1;
        if who_width > 0 && left_width + 1 + who_width + reserved <= width {
            let end = area.right() - u16::try_from(reserved).unwrap_or(0);
            let x = end - u16::try_from(who_width).unwrap_or(0);
            frame.render_widget(Line::from(who), Rect::new(x, area.y, end - x, 1));
        }
        let x = area
            .right()
            .saturating_sub(u16::try_from(shown).unwrap_or(0));
        let right = Rect::new(x, area.y, area.right() - x, 1);
        frame.render_widget(Line::raw(typed), right);
    }

    /// What F1 … F10 do now, each with its label on the F-key bar; `None` for a key that does
    /// nothing.
    fn fkey_actions(&self) -> [Option<(Action, String)>; 10] {
        let (mode, context) = self.place();
        self.keymap.fkeys(mode, context).map(|action| {
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
            let styles = (self.theme.fkey_label, self.theme.truncated_bar);
            let mut line = Line::from(Span::styled(number, self.theme.fkey_number));
            let label = cells::fit(&label, room, Align::Left, styles, self.theme.glyphs.marks);
            line.spans.extend(label);
            frame.render_widget(line, *slot);
        }
        slots.to_vec()
    }
}

/// What kind of window has the keys in `context`, as the line of the mode names it after the
/// mode: the kind says which keys work there, so windows with the same keys share it (ADR
/// 0032). None in the panels, which the mode alone stands for, on the command line, which
/// shows itself, and in the contexts keys never go to alone.
fn window_kind(context: Context) -> Option<String> {
    let kind = match context {
        Context::QuickSearch => fl!("window-quick-search"),
        Context::Rename => fl!("window-rename"),
        Context::LocationMenu
        | Context::TabMenu
        | Context::Jump
        | Context::Workspaces
        | Context::CommandHistory
        | Context::KeyHints => fl!("window-list"),
        Context::PullDown => fl!("window-menu"),
        Context::Dialog
        | Context::DialogInput
        | Context::PathInput
        | Context::Completion
        | Context::Find
        | Context::FindInput => fl!("window-dialog"),
        Context::Help => fl!("window-help"),
        Context::Viewer => fl!("window-viewer"),
        Context::Panel
        | Context::CommandLine
        | Context::CommandCompletion
        | Context::Global
        | Context::List
        | Context::Input
        | Context::SearchInput
        | Context::UserScreen => return None,
    };
    Some(kind)
}

/// The colors and glyphs of the viewer in `theme`.
fn viewer_styles(theme: &Theme) -> ViewerStyles {
    ViewerStyles {
        header: theme.cursor,
        header_mark: theme.truncated_cursor,
        text: theme.panel,
        bar: theme.glyphs.scroll_bar,
        line: glyphs::LINE,
        mark: theme.truncated,
        marks: theme.glyphs.marks,
        hidden: theme.glyphs.hidden,
        hidden_style: theme.hidden,
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
    let names = tabs.names(home, line.is_none());
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
    if let Some(motion) = action.motion() {
        return Some(ViewerCommand::Scroll(motion));
    }
    Some(match action {
        Action::Left => ViewerCommand::Left,
        Action::Right => ViewerCommand::Right,
        // A count repeats them, as it does `h` and `l`.
        Action::ScrollLeft => ViewerCommand::ScrollLeft(1),
        Action::ScrollRight => ViewerCommand::ScrollRight(1),
        Action::ScrollHalfLeft => ViewerCommand::HalfLeft(1),
        Action::ScrollHalfRight => ViewerCommand::HalfRight(1),
        Action::ScrollStart => ViewerCommand::ScrollStart,
        Action::ScrollEnd => ViewerCommand::ScrollEnd,
        Action::ToggleWrap => ViewerCommand::ToggleWrap,
        _ => return None,
    })
}

/// The theme that `ui` picks, in the colors of `depth`, with its frames and the glyphs of its
/// scroll bars. `ui.theme` was checked when the config was loaded, and the dialog offers only
/// the built-in themes.
fn theme_of(ui: &UiConfig, depth: ColorDepth) -> Theme {
    Theme::by_name(&ui.theme, depth)
        .unwrap_or_else(Theme::mc_classic)
        .with_borders(ui.borders)
        .with_glyphs(Glyphs::of(ui.icons))
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
        // OK checked it.
        run: text(HOST_RUN).and_then(|run| run_of(&run).ok()),
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
        Location::Local(path) => path.file_name().map(|name| name.as_bytes().to_vec()),
        Location::Remote { path, .. } => path.file_name().map(<[u8]>::to_vec),
    }
}

/// Whether `location` is on `host`.
fn is_on(location: &Location, host: &str) -> bool {
    matches!(location, Location::Remote { host: shown, .. } if shown == host)
}

/// Whether `panel` waits for a listing on `host`.
fn waits_for(panel: &Panel, host: &str) -> bool {
    panel.pending_request().is_some_and(
        |request| matches!(&request.location, Location::Remote { host: wanted, .. } if wanted == host),
    )
}

/// Whether `action` does its work in a panel on a host that is not connected, without asking
/// to connect: it leaves the panel, goes elsewhere, or does not touch its entries. Reload
/// connects by itself.
fn works_unconnected(action: Action) -> bool {
    matches!(
        action,
        Action::Quit
            | Action::Help
            | Action::KeyHints
            | Action::Redraw
            | Action::UserScreen
            | Action::Jobs
            | Action::Cancel
            | Action::NormalMode
            | Action::SwitchPanel
            | Action::FocusLeft
            | Action::FocusRight
            | Action::SwapPanels
            | Action::NewTab
            | Action::CloseTab
            | Action::NextTab
            | Action::PrevTab
            | Action::TabList
            | Action::LocationMenu
            | Action::OtherPanel
            | Action::PullDown
            | Action::PullDownLast
            | Action::Reload
            | Action::Disconnect
            | Action::EditHost
            | Action::Jump
            | Action::GoToDir
            | Action::JumpBack
            | Action::JumpForward
            | Action::SaveWorkspace
            | Action::Workspaces
            | Action::ToggleHidden
    )
}

/// Whether `action` only looks through the entries a panel kept from a host that is not
/// connected: it moves the cursor or scrolls, searches, sorts, marks, or selects.
fn works_offline(action: Action) -> bool {
    matches!(
        action,
        Action::Up
            | Action::Down
            | Action::PageUp
            | Action::PageDown
            | Action::HalfPageUp
            | Action::HalfPageDown
            | Action::Home
            | Action::End
            | Action::ScrollLeft
            | Action::ScrollRight
            | Action::ScrollHalfLeft
            | Action::ScrollHalfRight
            | Action::ScrollStart
            | Action::ScrollEnd
            | Action::ShowFull
            | Action::QuickSearch
            | Action::QuickSearchBack
            | Action::NextMatch
            | Action::PrevMatch
            | Action::SearchMode
            | Action::SearchPlain
            | Action::SearchFzf
            | Action::SearchRegex
            | Action::SearchGlob
            | Action::CaseMode
            | Action::CaseSmart
            | Action::CaseIgnore
            | Action::CaseMatch
            | Action::Abort
            | Action::SortByName
            | Action::SortByExtension
            | Action::SortByTime
            | Action::SortBySize
            | Action::Mark
            | Action::MarkUp
            | Action::InvertMarks
            | Action::Select
            | Action::Unselect
            | Action::VisualMode
            | Action::VisualLineMode
            | Action::OtherEnd
            | Action::Reselect
    )
}

/// `location` as Go to directory reads it: a host's home directory as `host:`.
fn cd_text(location: &Location) -> String {
    match location {
        Location::Remote { host, path } if path.as_bytes().is_empty() => {
            format!("{}:", cells::sanitize(host.as_bytes()))
        }
        _ => location_text(location),
    }
}

/// A name for the prompt of a dialog, where `&` marks the letter of the field; a name's is a
/// character.
fn prompt_name(name: &[u8]) -> String {
    label::escape(&cells::sanitize(name))
}

/// How many of `entries` are files and how many directories, counted as the panel's frame
/// counts them: a symlink to a directory is a directory.
fn files_and_dirs(entries: &[&DirEntry]) -> (usize, usize) {
    let dirs = entries.iter().filter(|entry| entry.is_dir_like()).count();
    (entries.len() - dirs, dirs)
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
        Action::CloseTab => Some(fl!("fkey-close-tab")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    use std::sync::mpsc;

    use noc_config::{AnimationsConfig, SearchMode, TransferConfig, ZoxideConfig};
    use noc_ssh::askpass::PromptKind;
    use noc_vfs::{DirEntry, FileKind, Metadata, RemotePath};
    use noc_viewer::Stamp;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use secrecy::ExposeSecret as _;

    use super::super::panel::Listing;
    use super::super::places::KnownHost;
    use super::super::progress::Counts;
    use noc_tools::ripgrep::Hit;

    use super::*;
    use crate::tui::search::Way;
    use crossterm::event::KeyEvent;

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
                inode: None,
            },
            target_kind: None,
            link_target: None,
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
            animations: still(),
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
    impl App {
        /// The file the editor of F4 is to open now, if that is what comes next.
        fn take_edit(&mut self) -> Option<PathBuf> {
            match self.handover.take() {
                Some(Handover::Edit { file, .. }) => Some(file),
                other => {
                    self.handover = other;
                    None
                }
            }
        }

        /// The shell command to run now.
        fn take_run(&mut self) -> Option<Run> {
            match self.handover.take() {
                Some(Handover::Run(run)) => Some(run),
                other => {
                    self.handover = other;
                    None
                }
            }
        }
    }

    fn without_zoxide(effects: Vec<Effect>) -> Vec<Effect> {
        effects
            .into_iter()
            .filter(|effect| !matches!(effect, Effect::ZoxideAdd(_)))
            .collect()
    }

    /// Windows that show whole at once and spinners that stand still, which tests of their own
    /// animate.
    fn still() -> AnimationsConfig {
        AnimationsConfig {
            windows: false,
            spinners: false,
        }
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

    /// What Ctrl+x and `action` do: `action` in the other panel.
    fn on_other(app: &mut App, action: Action) -> Vec<Effect> {
        app.typed(Typed {
            resolved: Resolved::Action(action),
            extra: Extra {
                other: true,
                ..Extra::default()
            },
        })
    }

    /// The only effect.
    fn one(effects: Vec<Effect>) -> Effect {
        let [effect] = <[Effect; 1]>::try_from(effects).unwrap();
        effect
    }

    /// A whole file of `bytes`, read as the viewer's first part.
    fn whole(bytes: impl Into<Vec<u8>>) -> ViewerPart {
        let bytes = bytes.into();
        let length = bytes.len() as u64;
        let want = Want {
            offset: 0,
            length,
            new: true,
        };
        let stamp = Stamp {
            size: Some(length),
            modified: None,
        };
        ViewerPart::new(want, &bytes, (true, false), stamp)
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

    /// Fails every listing in `effects` for `kind`, and returns what that leads to.
    fn fail(app: &mut App, effects: Vec<Effect>, kind: ListErrorKind) -> Vec<Effect> {
        let reason = match kind {
            ListErrorKind::NotFound => "no such file or directory",
            ListErrorKind::Denied => "permission denied",
            ListErrorKind::Other => "connection reset",
        };
        let mut next = Vec::new();
        for effect in effects {
            let Effect::List { panel, request, .. } = effect else {
                panic!("expected a listing, got {effect:?}");
            };
            let error = ListError {
                kind,
                reason: reason.to_owned(),
                location: None,
            };
            next.extend(app.listed(panel, request.generation, Err(error)));
        }
        next
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

    /// The places of the location menu: no volumes, and the hosts `web` and `db`.
    fn places() -> Places {
        Places {
            volumes: Vec::new(),
            hosts: ["web", "db"]
                .map(|alias| KnownHost {
                    alias: alias.to_owned(),
                    label: None,
                    address: None,
                })
                .to_vec(),
        }
    }

    /// An app with both panels on `/`, empty.
    fn at_slash() -> App {
        let (mut app, effects) = App::new(Path::new("/"), Path::new("/home/me"), &config());
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        app
    }

    /// Opens the location menu of the panel on `side`, with its places: `g l` in the active
    /// panel, Ctrl+x `g l` in the other.
    fn menu_on(app: &mut App, side: Side) {
        let effects = if side == app.active {
            app.handle(action(Action::LocationMenu))
        } else {
            on_other(app, Action::LocationMenu)
        };
        let Effect::ListPlaces { generation } = one(effects) else {
            panic!("expected the places");
        };
        app.places(generation, Ok(places()));
    }

    #[test]
    fn the_location_menu_and_the_zoxide_window_remember_how_many_rows_they_had() {
        let mut app = at_slash();
        assert!(app.sizes.menu.is_none());
        menu_on(&mut app, Side::Left);
        let lines = app.menu.as_ref().and_then(LocationMenu::lines_listed);
        assert!(lines.is_some());
        assert_eq!(app.sizes.menu, lines);
        app.handle(action(Action::Cancel));
        assert!(app.menu.is_none());
        let effects = app.handle(action(Action::LocationMenu));
        let Effect::ListPlaces { generation } = one(effects) else {
            panic!("expected the places");
        };
        app.places(generation, Err("no ssh config".to_owned()));
        assert_eq!(app.sizes.menu, lines, "a failed listing keeps them");

        app.handle(action(Action::Cancel));
        app.handle(action(Action::Jump));
        assert!(app.sizes.jump.is_none());
        app.jumps(
            app.jump_queries,
            Ok(vec![Scored {
                score: 1.0,
                path: PathBuf::from("/srv"),
            }]),
        );
        assert_eq!(app.sizes.jump, Some(1));
    }

    /// Opens the host of [`places`] number `host`, from 1, in the panel on `side`, through its
    /// location menu, after the home directory, which 0 opens.
    fn enter_host(app: &mut App, side: Side, host: usize) -> Vec<Effect> {
        menu_on(app, side);
        choose(app, host)
    }

    /// Wide enough for whole status lines, and tall enough for two rows of a panel.
    fn screen(app: &mut App) -> String {
        screen_of(app, 9)
    }

    /// What the prompt of quick search or of a filter shows: `prompt` and `text`. The glyph of
    /// how it matches is at the end of the field.
    fn prompt(prompt: &str, text: &str) -> String {
        format!("{prompt}: {text}")
    }

    /// Whether the status line shows quick search with `text` typed, and `glyph` at its right
    /// end.
    fn searching(app: &mut App, text: &str, glyph: &str) -> bool {
        let screen = screen(app);
        let end = format!(" {glyph} ║");
        screen
            .lines()
            .any(|line| line.contains(&format!("║Search: {text}")) && line.contains(&end))
    }

    /// The glyph of how quick search or a filter matches in the search mode of the settings.
    fn glyph(app: &App) -> &'static str {
        Way::from(app.config.ui.default_search_mode).glyph(app.theme.glyphs.search_modes)
    }

    /// The screen once spinners that start now show, with what they wait for.
    fn screen_of(app: &mut App, height: u16) -> String {
        let now = Instant::now();
        screen_at(app, height, now);
        screen_at(app, height, now + spinner::DELAY)
    }

    /// The screen `height` rows tall, with windows as they grow or shrink `at`.
    fn screen_at(app: &mut App, height: u16, at: Instant) -> String {
        let mut terminal = Terminal::new(TestBackend::new(160, height)).unwrap();
        terminal
            .draw(|frame| app.render(frame, at, &TimeZone::UTC))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn windows_grow_as_they_open_and_shrink_as_they_close() {
        let mut app = loaded();
        app.config.animations.windows = true;
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let panels = screen_at(&mut app, 12, at(0));
        app.handle(action(Action::Help));
        let growing = screen_at(&mut app, 12, at(1000));
        let due = app.frame_due().unwrap();
        assert!(due > at(1000) && due < at(2000), "{due:?}");
        // Keys go to the window while it grows.
        assert_eq!(app.context(), Context::Help);
        let whole = screen_at(&mut app, 12, at(2000));
        assert_ne!(growing, whole);
        assert_eq!(app.frame_due(), None);
        assert_eq!(screen_at(&mut app, 12, at(2500)), whole, "stays as it is");

        app.handle(action(Action::Cancel));
        let shrinking = screen_at(&mut app, 12, at(3000));
        assert_ne!(shrinking, panels);
        assert!(app.frame_due().is_some());
        assert_eq!(screen_at(&mut app, 12, at(4000)), panels);
        assert_eq!(app.frame_due(), None);

        // A menu of F9 drops from its bar, which shows at once.
        app.handle(action(Action::PullDown));
        screen_at(&mut app, 12, at(5000));
        app.handle(action(Action::Down));
        let dropping = screen_at(&mut app, 12, at(6000));
        assert!(dropping.contains("Command"), "{dropping}");
        assert!(!dropping.contains("Sort by name"), "{dropping}");
        let dropped = screen_at(&mut app, 12, at(7000));
        assert!(dropped.contains("Sort by name"), "{dropped}");
        app.handle(action(Action::Cancel));
        app.handle(action(Action::Cancel));
        screen_at(&mut app, 12, at(8000));

        // Turned off, windows come and go at once.
        app.config.animations.windows = false;
        app.handle(action(Action::Help));
        assert_eq!(screen_at(&mut app, 12, at(9000)), whole);
        assert_eq!(app.frame_due(), None);
    }

    #[test]
    fn dots_alone_draw_half_as_often_as_a_shine() {
        let mut app = loaded();
        app.config.animations.spinners = true;
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let shows = at(0) + spinner::DELAY;

        // A panel reads its directory again: dots turn in its title, alone.
        let reload = app.handle(action(Action::Reload));
        assert!(matches!(&reload[..], [Effect::List { .. }]), "{reload:?}");
        screen_at(&mut app, 20, at(0));
        assert_eq!(app.spin_due(at(0)), Some(shows), "nothing shows before");
        screen_at(&mut app, 20, shows);
        assert_eq!(app.spin_due(shows), Some(shows + spinner::DOTS_FRAME));

        // The location menu waits for its places: a shine crosses the skeleton of its rows.
        let Effect::ListPlaces { .. } = one(on_other(&mut app, Action::LocationMenu)) else {
            panic!("expected the places");
        };
        let opened = shows + spinner::DOTS_FRAME;
        screen_at(&mut app, 20, opened);
        let shines = opened + spinner::DELAY;
        let text = screen_at(&mut app, 20, shines);
        assert_eq!(
            app.spin_due(shines),
            Some(shines + spinner::SHINE_STEP),
            "{text}"
        );
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
    fn keys_go_to_the_active_panel_and_ctrl_w_switches_it() {
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

        let mut app = at_slash();
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

    /// Presses the mouse at column `x` and row `y` of the screen last drawn, and gives the keys
    /// back after Ctrl+x if that ends its command, as the event loop does.
    fn press(app: &mut App, press: Press, x: u16, y: u16) -> Vec<Effect> {
        let at = Position::new(x, y);
        let effects = app.pointer(Pointer { press, at });
        app.come_back();
        effects
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
    // 80 … 159; their rows start on line 2, under the frame and the header, and five fit.
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

    /// An app on `/srv`, which holds the files `names`.
    fn with_files(names: &[&str]) -> App {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let files = names.iter().map(|name| file(name, 1)).collect();
        answer(&mut app, effects, &Listing::Dir(files));
        app
    }

    /// Feeds `keys` to the app as the event loop does, in the mode and the context it is in
    /// at each key.
    fn keys(app: &mut App, keys: &[&str]) -> Vec<Effect> {
        let mut state = KeyState::default();
        let mut effects = Vec::new();
        for key in keys {
            let combination = crokey::parse(key).unwrap().normalized();
            let event = KeyEvent::new(*combination.codes.first(), combination.modifiers);
            effects.extend(app.key(&mut state, event, Instant::now()));
        }
        effects
    }

    #[test]
    fn visual_mode_selects_rows_that_the_commands_of_files_act_on() {
        let mut app = with_files(&["a", "b", "c", "d"]);
        keys(&mut app, &["j", "v", "j", "j"]);
        assert_eq!(app.place(), (Mode::Visual, Context::Panel));
        assert_eq!(mode_of(&mut app), "VISUAL");
        keys(&mut app, &["o"]);
        assert_eq!(cursor_name(&app, Side::Left), "a", "the other end");
        keys(&mut app, &["space"]);
        assert_eq!(app.mode(), Mode::Normal, "Space marks the rows and ends it");
        assert_eq!(mode_of(&mut app), "NORMAL");
        assert_eq!(chosen_names(&app, Side::Left), ["a", "b", "c"]);
        // A command acts on the rows selected, and only on them.
        keys(&mut app, &["shift-g", "shift-v", "k", "d"]);
        assert_eq!(chosen_names(&app, Side::Left), ["c", "d"]);
        assert_eq!(app.context(), Context::Dialog, "it asks first");
        keys(&mut app, &["esc"]);
        // g v selects them again; Esc ends Visual mode.
        keys(&mut app, &["g", "v"]);
        assert_eq!(app.mode(), Mode::Visual);
        keys(&mut app, &["esc"]);
        assert_eq!(app.place(), (Mode::Normal, Context::Panel));
    }

    #[test]
    fn k_shows_the_whole_name_under_the_cursor_below_the_panels_until_asked_again() {
        let long = format!("{}.txt", "a-very-long-name-".repeat(6));
        let mut app = with_files(&[&long, "b"]);
        keys(&mut app, &["j"]);
        let shown = |app: &mut App| screen_of(app, 14).contains(&long);
        assert!(!shown(&mut app), "the panel cuts it");
        keys(&mut app, &["shift-k"]);
        let screen = screen_of(&mut app, 14);
        assert!(screen.contains(&long), "{screen}");
        keys(&mut app, &["j"]);
        let screen = screen_of(&mut app, 14);
        assert!(!screen.contains(&long), "it follows the cursor: {screen}");
        keys(&mut app, &["k"]);
        assert!(shown(&mut app), "and stays open");
        keys(&mut app, &["esc"]);
        assert!(!shown(&mut app), "Esc closes it");
        keys(&mut app, &["shift-k"]);
        assert!(shown(&mut app));
        keys(&mut app, &["shift-k"]);
        assert!(!shown(&mut app), "so does K again");
    }

    #[test]
    fn k_is_for_the_panels_alone_as_the_lists_always_show_their_row_whole() {
        let mut app = with_files(&["a"]);
        keys(&mut app, &["t", "t", "esc", "shift-k"]);
        assert!(app.tab_menu.is_some(), "K closes no list");
        assert_eq!(app.whole, Whole::Cut, "nor shows the name below the panels");
        keys(&mut app, &["esc", "shift-k", "t", "t"]);
        assert_eq!(app.whole, Whole::Shown, "the panels keep it under a list");
    }

    #[test]
    fn a_count_scrolls_the_names_that_many_columns_as_vims_zl() {
        let mut app = with_files(&[&"x".repeat(100)]);
        keys(&mut app, &["j", "3", "z", "l"]);
        assert_eq!(app.panel(Side::Left).shift(), 3);
        keys(&mut app, &["2", "z", "h"]);
        assert_eq!(app.panel(Side::Left).shift(), 1);
        keys(&mut app, &["z", "e"]);
        assert!(app.panel(Side::Left).shift() > 1);
        keys(&mut app, &["z", "s"]);
        assert_eq!(app.panel(Side::Left).shift(), 0);
        assert_eq!(
            viewer_command(Action::ScrollHalfRight),
            Some(ViewerCommand::HalfRight(1))
        );
        assert_eq!(
            viewer_command(Action::ScrollEnd),
            Some(ViewerCommand::ScrollEnd)
        );
    }

    #[test]
    fn a_count_repeats_a_motion_and_numbers_a_row_or_a_tab() {
        let mut app = with_files(&["a", "b", "c", "d", "e"]);
        keys(&mut app, &["3", "j"]);
        assert_eq!(cursor_name(&app, Side::Left), "c");
        keys(&mut app, &["2", "k"]);
        assert_eq!(cursor_name(&app, Side::Left), "a");
        keys(&mut app, &["5", "shift-g"]);
        assert_eq!(cursor_name(&app, Side::Left), "d", "row 5, after ..");
        keys(&mut app, &["2", "g", "g"]);
        assert_eq!(cursor_name(&app, Side::Left), "a");
        keys(&mut app, &["2", "space"]);
        assert_eq!(chosen_names(&app, Side::Left), ["a", "b"]);
        // Marks stop at the end of the listing: the last row turns once.
        keys(&mut app, &["shift-g", "k", "4", "space"]);
        assert_eq!(chosen_names(&app, Side::Left), ["a", "b", "d", "e"]);
        keys(&mut app, &["9", "shift-up"]);
        assert_eq!(chosen_names(&app, Side::Left), ["c"]);
        keys(&mut app, &["t", "n", "t", "n"]);
        assert_eq!(app.tabs(Side::Left).index(), 2);
        keys(&mut app, &["1", "g", "t"]);
        assert_eq!(app.tabs(Side::Left).index(), 0, "the first tab");
        keys(&mut app, &["2", "g", "shift-t"]);
        assert_eq!(app.tabs(Side::Left).index(), 1, "two back, round");
    }

    #[test]
    fn ctrl_o_and_tab_go_back_and_forth_through_the_jumps() {
        let mut app = loaded();
        let effects = keys(&mut app, &["j", "l"]);
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![file("x", 1), file("y", 1)]),
        );
        assert_eq!(app.panel(Side::Left).location(), &local("/srv/left"));
        keys(&mut app, &["shift-g"]);
        assert_eq!(cursor_name(&app, Side::Left), "y");
        keys(&mut app, &["ctrl-o"]);
        assert_eq!(cursor_name(&app, Side::Left), "", "back before G, on ..");
        let effects = keys(&mut app, &["ctrl-o"]);
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("left"), dir("right")]),
        );
        assert_eq!(app.panel(Side::Left).location(), &local("/srv"));
        assert_eq!(cursor_name(&app, Side::Left), "left", "where it was");
        let effects = keys(&mut app, &["tab"]);
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![file("x", 1), file("y", 1)]),
        );
        assert_eq!(app.panel(Side::Left).location(), &local("/srv/left"));
        keys(&mut app, &["tab"]);
        assert_eq!(cursor_name(&app, Side::Left), "y", "forward to after G");
        assert!(keys(&mut app, &["tab"]).is_empty(), "nothing further");
        // The panels switch with Ctrl+w, as vim's windows do.
        keys(&mut app, &["ctrl-w", "w"]);
        assert_eq!(app.active, Side::Right);
    }

    #[test]
    fn fields_open_in_insert_mode_and_take_vims_normal_mode_after_esc() {
        let mut app = with_files(&["report.pdf"]);
        keys(&mut app, &["j", "r"]);
        assert_eq!(app.place(), (Mode::Insert, Context::Rename));
        assert_eq!(mode_of(&mut app), "INSERT · Rename");
        // In Insert mode j is text; Esc goes to Normal mode, where it is not.
        keys(&mut app, &["esc", "0", "c", "w", "n", "e", "w", "esc"]);
        assert_eq!(app.place(), (Mode::Normal, Context::Rename));
        let text = |app: &mut App| {
            let panel = app.panel_mut(Side::Left);
            panel.rename_field().map(|field| field.text().to_owned())
        };
        assert_eq!(text(&mut app).as_deref(), Some("new.pdf"));
        // A count and an operator wait on the line above the F-key bar.
        keys(&mut app, &["2", "d"]);
        let shown = screen(&mut app);
        assert!(
            line_with(&shown, "2d").ends_with(" 2d\""),
            "on the right: {shown}"
        );
        keys(&mut app, &["esc"]);
        assert_eq!(app.context(), Context::Rename, "Esc gave the operator up");
        keys(&mut app, &["u"]);
        assert_eq!(text(&mut app).as_deref(), Some("report.pdf"), "undone");
        keys(&mut app, &["\"", "a", "y", "y", "esc"]);
        assert_eq!(
            app.context(),
            Context::Panel,
            "Esc in Normal mode leaves it"
        );
        // Registers go from field to field: here, the command line.
        keys(&mut app, &[":", "!", "ctrl-r", "a"]);
        assert_eq!(
            app.command_line.as_ref().map(CommandLine::text),
            Some("!report.pdf")
        );
        // Alt and a key in Insert mode are Esc and the key.
        keys(&mut app, &["alt-k"]);
        assert!(app.command_line.is_none(), "Esc closes vim's command line");
        assert_eq!(
            cursor_name(&app, Side::Left),
            "",
            "and k goes up from report.pdf"
        );
    }

    #[test]
    fn a_command_the_keymap_gives_up_gives_up_the_operator_of_the_field() {
        let mut app = with_files(&["abc def"]);
        let text = |app: &mut App| {
            let panel = app.panel_mut(Side::Left);
            panel.rename_field().map(|field| field.text().to_owned())
        };
        keys(&mut app, &["j", "r", "esc", "0"]);
        for given_up in [&["d", "f", "esc"][..], &["d", "2", "esc"], &["d", "z"]] {
            keys(&mut app, given_up);
            keys(&mut app, &["w"]);
            assert_eq!(text(&mut app).as_deref(), Some("abc def"), "{given_up:?}");
        }
        keys(&mut app, &["0", "c", "w", "x", "ctrl-o", "esc"]);
        assert_eq!(
            app.place(),
            (Mode::Insert, Context::Rename),
            "Esc after Ctrl+o goes back to Insert mode"
        );
        assert_eq!(text(&mut app).as_deref(), Some("x def"));
    }

    #[test]
    fn alt_and_a_key_do_nothing_on_a_password() {
        let mut app = loaded();
        let (password, _answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
        app.ask(password);
        assert!(keys(&mut app, &["alt-:", "alt-x"]).is_empty());
        assert_eq!(app.context(), Context::DialogInput, "still asking");
        assert!(app.command_line.is_none());
    }

    #[test]
    fn visual_mode_keeps_its_rows_and_ends_where_the_keys_go_elsewhere() {
        let mut app = with_files(&["a", "b", "c", "d", "e"]);
        keys(&mut app, &["3", "j", "v", "j"]);
        assert_eq!(cursor_name(&app, Side::Left), "d");
        // A listing of the same directory with an entry more keeps the same entries selected.
        let request = app.panel_mut(Side::Left).handle(Action::Reload).unwrap();
        let entries = ["a", "aa", "b", "c", "d", "e"].map(|name| file(name, 1));
        app.listed(
            app.shown(Side::Left),
            request.generation,
            Ok(Listed {
                location: local("/srv"),
                listing: Listing::Dir(entries.to_vec()),
                space: None,
            }),
        );
        keys(&mut app, &["space"]);
        assert_eq!(chosen_names(&app, Side::Left), ["c", "d"]);
        // A count does not mark more once Space ends Visual mode.
        keys(&mut app, &["g", "g", "v", "j", "3", "space"]);
        assert_eq!(chosen_names(&app, Side::Left), ["a", "c", "d"]);
        // Another panel takes the keys: this one leaves Visual mode.
        keys(&mut app, &["v", "ctrl-w", "w", "ctrl-w", "w"]);
        assert_eq!(app.place(), (Mode::Normal, Context::Panel));
        keys(&mut app, &["v"]);
        screen_of(&mut app, 12);
        press(&mut app, Press::Click, 100, 4);
        assert_eq!(app.active, Side::Right);
        assert!(!app.panel(Side::Left).visual(), "after a click too");
        keys(&mut app, &["ctrl-w", "w"]);
        // An F-key that does not act on the selection ends Visual mode and does what it does.
        keys(&mut app, &["v", "f7"]);
        assert_eq!(app.context(), Context::PathInput, "F7 asks for a name");
        assert!(!app.panel(Side::Left).visual());
    }

    #[test]
    fn a_failed_listing_is_no_jump_and_n_jumps_once() {
        let mut app = with_files(&["b1", "b2", "b3", "b4", "b5"]);
        // A listing that fails leaves the panel where it was, and no jump.
        keys(&mut app, &["shift-g"]);
        let effects = keys(&mut app, &["g", "g", "l"]);
        fail(&mut app, effects, ListErrorKind::Denied);
        keys(&mut app, &["esc", "ctrl-o"]);
        assert_eq!(
            cursor_name(&app, Side::Left),
            "b5",
            "back where G was, at once"
        );
        // 3n moves three matches on, and goes to the jump list once.
        keys(&mut app, &["g", "g", "/", "b", "enter", "3", "n"]);
        assert_eq!(cursor_name(&app, Side::Left), "b4");
        keys(&mut app, &["ctrl-o"]);
        assert_eq!(cursor_name(&app, Side::Left), "b1");
        // A jump that goes nowhere is one too: Ctrl+o comes back to it first, as in vim.
        keys(&mut app, &["shift-g", "shift-g", "ctrl-o"]);
        assert_eq!(cursor_name(&app, Side::Left), "b5");
        keys(&mut app, &["ctrl-o"]);
        assert_eq!(cursor_name(&app, Side::Left), "b1");
        keys(&mut app, &["tab"]);
        assert_eq!(cursor_name(&app, Side::Left), "b5");
    }

    #[test]
    fn ctrl_l_gives_up_an_operator_and_is_the_one_command_after_ctrl_o() {
        let mut app = with_files(&["report.pdf"]);
        let text = |app: &mut App| {
            let panel = app.panel_mut(Side::Left);
            panel.rename_field().map(|field| field.text().to_owned())
        };
        keys(&mut app, &["j", "r", "esc", "0", "d", "ctrl-l", "w"]);
        assert_eq!(
            text(&mut app).as_deref(),
            Some("report.pdf"),
            "w only moved"
        );
        keys(&mut app, &["shift-a", "ctrl-o"]);
        assert_eq!(mode_of(&mut app), "(insert) · Rename");
        keys(&mut app, &["ctrl-l"]);
        assert_eq!(app.place(), (Mode::Insert, Context::Rename));
        assert_eq!(mode_of(&mut app), "INSERT · Rename");
    }

    #[test]
    fn counts_move_past_the_fields_of_dialogs_and_the_configuration() {
        let mut once = with_files(&["a"]);
        keys(&mut once, &["j", "f", "#", "esc", "g", "g", "9", "j"]);
        let mut each = with_files(&["a"]);
        keys(&mut each, &["j", "f", "#", "esc", "g", "g"]);
        for _ in 0..9 {
            keys(&mut each, &["j"]);
        }
        assert_eq!(once.place(), each.place());
        assert_eq!(screen_of(&mut once, 30), screen_of(&mut each, 30));
        let mut once = loaded();
        open_configuration(&mut once);
        keys(&mut once, &["esc", "3", "j"]);
        let mut each = loaded();
        open_configuration(&mut each);
        keys(&mut each, &["esc", "j", "j", "j"]);
        assert_eq!(
            screen_of(&mut once, 30),
            screen_of(&mut each, 30),
            "from a text field"
        );
    }

    #[test]
    fn q_colon_from_a_panel_closes_with_the_window_of_the_history() {
        let mut app = loaded();
        keys(&mut app, &["q", ":"]);
        assert_eq!(app.place(), (Mode::Insert, Context::CommandHistory));
        keys(&mut app, &["esc", "q"]);
        assert!(
            app.command_line.is_none(),
            "back to the panel, as from vim's :q"
        );
        assert_eq!(app.place().1, Context::Panel);
        // From the command line, it goes back there.
        keys(&mut app, &["!", "ctrl-f", "q", ":"]);
        keys(&mut app, &["esc", "q"]);
        assert_eq!(app.place().1, Context::CommandLine);
    }

    #[test]
    fn a_count_past_the_jump_list_goes_nowhere() {
        let mut app = with_files(&["f1", "f2", "f3", "f4", "f5", "f6"]);
        keys(&mut app, &["2", "shift-g", "4", "shift-g", "6", "shift-g"]);
        assert_eq!(cursor_name(&app, Side::Left), "f5");
        keys(&mut app, &["9", "9", "ctrl-o"]);
        assert_eq!(cursor_name(&app, Side::Left), "f5", "as vim's");
        keys(&mut app, &["2", "ctrl-o"]);
        assert_eq!(cursor_name(&app, Side::Left), "f1");
        keys(&mut app, &["9", "9", "tab"]);
        assert_eq!(cursor_name(&app, Side::Left), "f1");
        keys(&mut app, &["tab"]);
        assert_eq!(cursor_name(&app, Side::Left), "f3");
    }

    #[test]
    fn v_and_shift_v_switch_the_kind_of_visual_mode_and_end_it_as_vims() {
        let mut app = with_files(&["a", "b", "c"]);
        keys(&mut app, &["j", "shift-v", "j"]);
        assert_eq!(mode_of(&mut app), "VISUAL LINE");
        keys(&mut app, &["v"]);
        assert_eq!(mode_of(&mut app), "VISUAL", "the other key switches");
        assert_eq!(
            app.panel(Side::Left).visual_size(),
            Some(2),
            "the rows stay"
        );
        keys(&mut app, &["v"]);
        assert_eq!(app.mode(), Mode::Normal, "its own key ends it");
        keys(&mut app, &["g", "v"]);
        assert_eq!(mode_of(&mut app), "VISUAL", "g v takes the kind back");
        // After a command of files, the cursor is on the first row selected, as in vim.
        keys(&mut app, &["f", "#"]);
        assert_eq!(cursor_name(&app, Side::Left), "a");
    }

    #[test]
    fn counts_select_rows_and_remove_them_from_lists_as_vims_lines() {
        let mut app = with_files(&["a", "b", "c", "d"]);
        keys(&mut app, &["j", "3", "shift-v"]);
        let line = mode_line(&mut app, 160);
        assert!(
            line.starts_with("VISUAL LINE") && line.ends_with(" 3"),
            "{line}"
        );
        keys(&mut app, &["esc", "t", "n", "t", "n", "t", "n"]);
        assert_eq!(app.tabs(Side::Left).len(), 4);
        keys(&mut app, &["t", "t", "esc", "g", "g", "3", "d", "d"]);
        assert_eq!(app.tabs(Side::Left).len(), 1);
    }

    #[test]
    fn quick_search_goes_from_the_row_after_the_cursor_and_to_the_match_of_its_count() {
        let mut app = with_files(&["a1", "a2", "a3", "b"]);
        keys(&mut app, &["j", "/", "a", "enter"]);
        assert_eq!(
            cursor_name(&app, Side::Left),
            "a2",
            "as vim's, not the row it is on"
        );
        keys(&mut app, &["g", "g", "2", "/", "a", "ctrl-j"]);
        assert_eq!(
            cursor_name(&app, Side::Left),
            "a2",
            "the second, and Ctrl+j is Enter"
        );
        assert!(!app.panel(Side::Left).searching());
    }

    #[test]
    fn n_says_when_there_was_no_search_and_when_it_goes_round() {
        let mut app = with_files(&["a1", "b2"]);
        keys(&mut app, &["n"]);
        assert!(screen(&mut app).contains("No previous search"));
        keys(&mut app, &["j"]);
        assert!(
            !screen(&mut app).contains("No previous search"),
            "until the next key"
        );
        keys(&mut app, &["/", "a", "enter", "n"]);
        assert!(screen(&mut app).contains("search hit BOTTOM, continuing at TOP"));
        keys(&mut app, &["shift-n"]);
        assert!(screen(&mut app).contains("search hit TOP, continuing at BOTTOM"));
        // So does Enter in quick search, typed as vim's search, on a match before where it
        // started; fuzzy search ranks the rows instead.
        let mut app = with_files(&["a1", "b2", "c3"]);
        app.config.ui.default_search_mode = SearchMode::Plain;
        keys(&mut app, &["shift-g", "/", "b", "enter"]);
        assert_eq!(cursor_name(&app, Side::Left), "b2");
        assert!(screen(&mut app).contains("search hit BOTTOM, continuing at TOP"));
        keys(&mut app, &["g", "g", "/", "b", "enter"]);
        assert!(!screen(&mut app).contains("search hit"), "after it");
    }

    #[test]
    fn question_mark_searches_up_and_n_goes_the_same_way_as_vims() {
        let mut app = with_files(&["a1", "b2", "a3", "b4"]);
        app.config.ui.default_search_mode = SearchMode::Plain;
        keys(&mut app, &["shift-g", "?"]);
        assert!(screen(&mut app).contains(&prompt("Search up", "")));
        keys(&mut app, &["a", "enter"]);
        assert_eq!(cursor_name(&app, Side::Left), "a3");
        keys(&mut app, &["n"]);
        assert_eq!(cursor_name(&app, Side::Left), "a1");
        keys(&mut app, &["n"]);
        assert_eq!(cursor_name(&app, Side::Left), "a3");
        assert!(screen(&mut app).contains("search hit TOP, continuing at BOTTOM"));
        keys(&mut app, &["shift-n"]);
        assert_eq!(cursor_name(&app, Side::Left), "a1");
        assert!(screen(&mut app).contains("search hit BOTTOM, continuing at TOP"));
        // Enter says so too, on a match not before where it started; a count goes on.
        keys(&mut app, &["g", "g", "?", "b", "enter"]);
        assert_eq!(cursor_name(&app, Side::Left), "b4");
        assert!(screen(&mut app).contains("search hit TOP, continuing at BOTTOM"));
        keys(&mut app, &["2", "?", "a", "enter"]);
        assert_eq!(cursor_name(&app, Side::Left), "a1");
    }

    /// What ripgrep found, as the search reports it.
    fn findings(hits: Vec<Hit>) -> Vec<Finding> {
        hits.into_iter().map(Finding::Hit).collect()
    }

    /// The search the effects start, which must be one.
    fn find_request(effects: &[Effect]) -> &FindRequest {
        let [Effect::Find(request)] = effects else {
            panic!("expected a search, got {effects:?}");
        };
        request
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn f_slash_lists_the_files_then_searches_as_the_text_is_typed() {
        let mut app = with_files(&["a1", "b2"]);
        let effects = keys(&mut app, &["f", "/"]);
        let request = find_request(&effects);
        assert_eq!(request.location, local("/srv"));
        assert!(request.host.is_none());
        assert_eq!(request.delay, Duration::ZERO, "the files show at once");
        assert!(request.query.lines().lists_files());
        assert_eq!(request.query.lines().max_per_file, 100);
        assert!(request.query.lines().hidden, "as the panels show them");
        assert_eq!(request.max_results, 10_000);
        let (first, cancel) = (request.generation, request.cancel.clone());
        assert_eq!(
            app.place(),
            (Mode::Insert, Context::FindInput),
            "typing in its text"
        );
        assert!(screen_of(&mut app, 24).contains("═ … Find in"));
        let effects = keys(&mut app, &["r"]);
        let request = find_request(&effects);
        assert_eq!(request.query.lines().pattern, "r");
        assert_eq!(request.delay, FIND_DELAY);
        assert!(cancel.is_cancelled(), "the search before stops");
        let generation = request.generation;
        app.found(first, vec![Finding::Hit(Hit::File(b"stale".to_vec()))]);
        app.found(
            generation,
            findings(vec![
                Hit::File(b"src/run/main.rs".to_vec()),
                Hit::Line {
                    number: 3,
                    text: b"fn run() {}".to_vec(),
                    matches: vec![3..4],
                },
            ]),
        );
        app.find_ended(generation, Ok(FindSummary::default()));
        let text = screen_of(&mut app, 24);
        assert!(
            text.contains("Find in /srv")
                && text.contains("Text:")
                && text.contains(&format!(" {}  ║", app.theme.glyphs.search_modes.regex))
                && text.contains("1 line"),
            "{text}"
        );
        assert!(
            text.contains("src/run/main.rs") && !text.contains("stale"),
            "{text}"
        );
        // Tab goes to the files, whose globs search again.
        let effects = keys(&mut app, &["tab", "*", "."]);
        assert_eq!(effects.len(), 2);
        assert_eq!(find_request(&effects[1..]).query.lines().globs, ["*."]);
        // Esc leaves Insert mode in the field, and again closes the dialog, which stops the
        // search.
        let cancel = find_request(&effects[1..]).cancel.clone();
        keys(&mut app, &["esc"]);
        assert_eq!(app.place(), (Mode::Normal, Context::FindInput));
        keys(&mut app, &["esc"]);
        assert_eq!(app.context(), Context::Panel);
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn f_slash_on_a_host_searches_through_its_connection() {
        let mut app = at_slash();
        let effects = enter_host(&mut app, Side::Left, 1);
        let Effect::Connect { connection, .. } = one(effects) else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        let effects = keys(&mut app, &["f", "/"]);
        let request = find_request(&effects);
        assert!(matches!(request.location, Location::Remote { .. }));
        assert!(request.host.is_some(), "its connection");
    }

    /// The app with the search of a tree open on `/srv`, which found `src/run/main.rs` with a
    /// line 3 that matches, under the cursor, which has the focus.
    #[allow(clippy::single_range_in_vec_init)]
    fn found_main(app: &mut App) {
        let effects = app.handle(action(Action::FindFile));
        let generation = find_request(&effects).generation;
        app.found(
            generation,
            findings(vec![
                Hit::File(b"src/run/main.rs".to_vec()),
                Hit::Line {
                    number: 3,
                    text: b"fn run() {}".to_vec(),
                    matches: vec![3..6],
                },
            ]),
        );
        app.find_ended(generation, Ok(FindSummary::default()));
        // Past the two fields.
        keys(app, &["tab", "tab", "down"]);
        assert_eq!(app.context(), Context::Find);
    }

    #[test]
    fn enter_on_a_file_found_goes_there_and_the_jump_list_keeps_where_the_panel_was() {
        let mut app = with_files(&["a1", "b2"]);
        found_main(&mut app);
        let effects = keys(&mut app, &["enter"]);
        assert_eq!(app.context(), Context::Panel);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/src/run"));
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![file("lib.rs", 1), file("main.rs", 1)]),
        );
        assert_eq!(cursor_name(&app, Side::Left), "main.rs");
        keys(&mut app, &["ctrl-o"]);
        assert_eq!(app.panel(Side::Left).heading().0, local("/srv"));
    }

    #[test]
    fn f3_views_a_file_found_at_its_line_and_f4_edits_it_there() {
        let mut app = with_files(&["a1", "b2"]);
        found_main(&mut app);
        let effects = keys(&mut app, &["f3"]);
        let Some(Effect::Read(ReadRequest { id, location, .. })) = effects.first() else {
            panic!("expected a read, got {effects:?}");
        };
        assert_eq!(*location, local("/srv/src/run/main.rs"));
        assert_eq!(app.context(), Context::Viewer);
        let text = (1..=40)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.read(*id, Ok(whole(text)));
        let screen = screen_of(&mut app, 12);
        assert!(
            screen.contains("line 3") && !screen.contains("line 2\n"),
            "{screen}"
        );
        // The viewer goes, and the dialog is there again.
        keys(&mut app, &["q"]);
        assert_eq!(app.context(), Context::Find);
        keys(&mut app, &["f4"]);
        assert!(matches!(
            app.take_handover(),
            Some(Handover::Edit { file, line: Some(3) })
                if file == Path::new("/srv/src/run/main.rs")
        ));
    }

    #[test]
    fn files_and_dirs_find_names_with_fd_and_enter_opens_a_directory() {
        let mut app = with_files(&["a1", "b2"]);
        keys(&mut app, &["f", "/"]);
        let effects = keys(&mut app, &["backtab", "space"]);
        let request = find_request(&effects);
        let FindQuery::Names(query) = &request.query else {
            panic!("expected a search by name, got {:?}", request.query);
        };
        assert!(query.pattern.is_empty() && query.hidden);
        let generation = request.generation;
        let found = Finding::Entry {
            path: b"src/run".to_vec(),
            dir: true,
            matches: Vec::new(),
        };
        app.found(generation, vec![found]);
        app.find_ended(generation, Ok(FindSummary::default()));
        // Past the choice and the fields, to the rows.
        keys(&mut app, &["tab", "tab", "tab", "tab"]);
        let effects = keys(&mut app, &["enter"]);
        assert_eq!(app.context(), Context::Panel);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/src/run"));
    }

    #[test]
    fn f_f_opens_the_search_by_name_typing_in_its_text() {
        let mut app = with_files(&["a1", "b2"]);
        let effects = keys(&mut app, &["f", "f"]);
        let request = find_request(&effects);
        let FindQuery::Names(query) = &request.query else {
            panic!("expected a search by name, got {:?}", request.query);
        };
        assert_eq!(query.pattern, "");
        assert_eq!(request.location, local("/srv"));
        assert_eq!(app.place(), (Mode::Insert, Context::FindInput));
        let effects = keys(&mut app, &["m"]);
        let FindQuery::Names(query) = &find_request(&effects).query else {
            panic!("expected a search by name");
        };
        assert_eq!(query.pattern, "m");
    }

    #[test]
    fn the_wheel_scrolls_the_rows_of_the_search_wherever_the_focus_is() {
        let mut app = with_files(&["a1"]);
        let effects = app.handle(action(Action::FindFile));
        let generation = find_request(&effects).generation;
        let hits = (0..60)
            .map(|n| Finding::Hit(Hit::File(format!("f{n:02}").into_bytes())))
            .collect();
        app.found(generation, hits);
        app.find_ended(generation, Ok(FindSummary::default()));
        assert!(screen_of(&mut app, 24).contains("f00"));
        press(&mut app, Press::WheelDown, 40, 12);
        let text = screen_of(&mut app, 24);
        assert!(!text.contains("f00") && text.contains("f10"), "{text}");
        assert_eq!(app.context(), Context::FindInput, "the focus stays");
    }

    #[test]
    fn backspace_and_delete_with_nothing_typed_keep_quick_search_open() {
        let mut app = with_files(&["a1", "b2"]);
        app.config.ui.default_search_mode = SearchMode::Plain;
        let location = app.panel(Side::Left).location().clone();
        for key in ["backspace", "delete"] {
            keys(&mut app, &["g", "g", "j", "/", "b"]);
            assert_eq!(cursor_name(&app, Side::Left), "b2");
            keys(&mut app, &["backspace", key, key, key]);
            assert!(app.panel(Side::Left).searching(), "{key}");
            assert_eq!(app.panel(Side::Left).search_text(), Some(""));
            assert_eq!(cursor_name(&app, Side::Left), "a1", "back where it started");
            assert_eq!(
                app.panel(Side::Left).location(),
                &location,
                "not the parent"
            );
            keys(&mut app, &["esc"]);
            assert!(!app.panel(Side::Left).searching());
        }
    }

    #[test]
    fn a_count_before_v_selects_the_last_selection_that_many_times_and_ctrl_j_goes_down() {
        let mut app = with_files(&["a", "b", "c", "d", "e", "f", "g"]);
        keys(&mut app, &["ctrl-j", "3", "v"]);
        assert_eq!(app.panel(Side::Left).visual_size(), Some(3));
        keys(
            &mut app,
            &["esc", "g", "g", "j", "v", "j", "space", "2", "v"],
        );
        assert_eq!(
            app.panel(Side::Left).visual_size(),
            Some(4),
            "two rows marked, twice"
        );
        keys(&mut app, &["esc", "v"]);
        assert_eq!(
            app.panel(Side::Left).visual_size(),
            Some(1),
            "without a count"
        );
    }

    #[test]
    fn a_quote_after_an_operator_gives_it_up_and_the_next_key_is_a_command() {
        let mut app = loaded();
        keys(
            &mut app,
            &[":", "a", "b", "c", "ctrl-f", "0", "d", "\"", "a", "x"],
        );
        let line = app.command_line.as_ref().expect("the command line");
        assert_eq!((line.text(), line.mode()), ("axbc", Mode::Insert));
    }

    #[test]
    fn visual_mode_counts_the_entries_and_d_and_x_delete_them() {
        let mut app = with_files(&["a", "b"]);
        keys(&mut app, &["v", "j"]);
        let line = mode_line(&mut app, 160);
        assert!(
            line.starts_with("VISUAL") && line.ends_with(" 1"),
            "not ..: {line}"
        );
        keys(&mut app, &["shift-d"]);
        assert_eq!(app.dialogs.len(), 1, "the question of the delete");
    }

    #[test]
    fn visual_mode_shows_how_much_it_selects() {
        let mut app = with_files(&["abc", "bcd", "cde"]);
        keys(&mut app, &["j", "v", "j"]);
        let line = mode_line(&mut app, 160);
        assert!(
            line.starts_with("VISUAL ") && line.ends_with(" 2"),
            "{line}"
        );
        keys(&mut app, &["esc", "r", "esc", "0", "v", "l"]);
        let line = mode_line(&mut app, 160);
        assert!(
            line.starts_with("VISUAL · Rename") && line.ends_with(" 2"),
            "{line}"
        );
    }

    #[test]
    fn pastes_go_to_the_field_as_one_change_and_to_a_list_filter() {
        let mut app = with_files(&["report.pdf"]);
        keys(&mut app, &["j", "r", "esc"]);
        app.paste("new");
        keys(&mut app, &["u"]);
        let text = app
            .panel_mut(Side::Left)
            .rename_field()
            .map(|field| field.text().to_owned());
        assert_eq!(text.as_deref(), Some("report.pdf"), "undone at once");
        keys(&mut app, &["\"", "a", "y", "y", "esc", "t", "n", "t", "t"]);
        app.paste("srv");
        assert_eq!(app.place(), (Mode::Insert, Context::TabMenu));
        assert!(screen_of(&mut app, 16).contains(&prompt("Filter", "srv")));
        // Ctrl+r types a register there, as in a text field.
        keys(&mut app, &["ctrl-u", "ctrl-r", "a"]);
        assert!(screen_of(&mut app, 16).contains(&prompt("Filter", "report.pdf")));
    }

    #[test]
    fn lists_open_in_insert_mode_with_the_filter_taking_the_keys() {
        let mut app = loaded();
        keys(&mut app, &["t", "n", "t", "t"]);
        assert_eq!(app.place(), (Mode::Insert, Context::TabMenu));
        keys(&mut app, &["x"]);
        assert!(screen_of(&mut app, 16).contains(&prompt("Filter", "x")));
        keys(&mut app, &["backspace", "esc"]);
        assert_eq!(app.place(), (Mode::Normal, Context::TabMenu));
        keys(&mut app, &["/"]);
        assert_eq!(app.place(), (Mode::Insert, Context::TabMenu));
        keys(&mut app, &["esc", "i"]);
        assert_eq!(app.place(), (Mode::Insert, Context::TabMenu), "i as /");
        keys(&mut app, &["esc", "g", "g", "d", "d"]);
        assert_eq!(app.tabs(Side::Left).len(), 1, "d d closes the tab");
        keys(&mut app, &["q"]);
        assert_eq!(app.context(), Context::Panel);
        for opens in [&["g", "l"][..], &["t", "w"], &["q", ":"], &["g", "z"]] {
            let mut app = loaded();
            keys(&mut app, opens);
            assert_eq!(app.mode(), Mode::Insert, "after {opens:?}");
        }
    }

    /// Where `glyph` ends a field on the line of `screen` that holds `text`.
    fn glyph_cell(screen: &str, text: &str, glyph: &str) -> Position {
        let (y, line) = (screen.lines().enumerate())
            .find(|(_, line)| line.contains(text))
            .unwrap_or_else(|| panic!("no {text:?} in {screen}"));
        let start = line.find(text).unwrap_or(0) + text.len();
        let end = line[start..]
            .find('║')
            .map_or(line.len(), |end| start + end);
        let at = start
            + line[start..end]
                .rfind(&format!(" {glyph} "))
                .unwrap_or_else(|| panic!("no {glyph:?} at the end of the field in {line}"));
        // The test backend quotes each line, and a blank comes before the glyph.
        let x = line[..at].chars().count();
        Position::new(u16::try_from(x).unwrap(), u16::try_from(y).unwrap())
    }

    #[test]
    fn ctrl_s_and_a_click_on_its_glyph_switch_how_the_field_with_the_focus_matches() {
        let mut app = loaded();
        let glyphs = app.theme.glyphs.search_modes;
        let cases = app.theme.glyphs.case_modes;
        // Quick search, whose glyphs end the status line.
        app.config.ui.default_search_mode = SearchMode::Plain;
        app.handle(action(Action::QuickSearch));
        app.handle(Resolved::Insert('r'));
        assert_eq!(
            app.key_place().search,
            Some(search::Kind::QUICK_SEARCH),
            "the keys after Ctrl+s go before its own"
        );
        let at = glyph_cell(&screen_of(&mut app, 12), "║Search: r", glyphs.plain);
        assert!(press(&mut app, Press::Click, at.x, at.y).is_empty());
        assert!(searching(&mut app, "r", glyphs.fzf), "the search goes on");
        assert!(
            screen(&mut app).contains(&format!(" {} {} ║", cases.smart, glyphs.fzf)),
            "the case before the way"
        );
        let at = glyph_cell(&screen_of(&mut app, 12), "║Search: r", cases.smart);
        press(&mut app, Press::Click, at.x, at.y);
        assert!(screen(&mut app).contains(&format!(" {} {} ║", cases.ignore, glyphs.fzf)));
        keys(&mut app, &["ctrl-s", "m"]);
        assert!(screen(&mut app).contains(&format!(" {} {} ║", cases.exact, glyphs.fzf)));
        keys(&mut app, &["ctrl-s", "r"]);
        assert!(
            searching(&mut app, "r", glyphs.regex),
            "a way by its letter"
        );
        keys(&mut app, &["esc"]);
        // A list, whose filter has the focus while it takes the keys.
        app.config.ui.default_search_mode = SearchMode::Fzf;
        keys(&mut app, &["t", "n", "t", "t"]);
        keys(&mut app, &["x"]);
        let screen = screen_of(&mut app, 16);
        let at = glyph_cell(&screen, &prompt("Filter", "x"), glyph(&app));
        keys(&mut app, &["esc"]);
        assert_eq!(app.key_place().search, None, "the rows have the focus");
        keys(&mut app, &["ctrl-s", "ctrl-s"]);
        glyph_cell(&screen_of(&mut app, 16), &prompt("Filter", "x"), glyphs.fzf);
        press(&mut app, Press::Click, at.x, at.y);
        assert_eq!(
            app.place(),
            (Mode::Insert, Context::TabMenu),
            "the filter takes the keys"
        );
        glyph_cell(
            &screen_of(&mut app, 16),
            &prompt("Filter", "x"),
            glyphs.regex,
        );
        keys(&mut app, &["ctrl-s", "ctrl-s"]);
        glyph_cell(
            &screen_of(&mut app, 16),
            &prompt("Filter", "x"),
            glyphs.plain,
        );
    }

    #[test]
    fn the_wheel_scrolls_the_panel_under_it() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let files = (0..20).map(|n| file(&format!("f{n:02}"), 1)).collect();
        answer(&mut app, effects, &Listing::Dir(files));
        screen_of(&mut app, 13);
        press(&mut app, Press::WheelDown, 90, 5);
        assert_eq!(app.active, Side::Left, "the wheel does not take the keys");
        let names = |app: &mut App| {
            screen_of(app, 13);
            [Side::Left, Side::Right].map(|side| cursor_name(app, side))
        };
        // As vim's Ctrl+e and Ctrl+y: the cursor moves only to keep what of 'scrolloff' fits
        // around it in six rows.
        assert_eq!(names(&mut app), ["", "f04"]);
        press(&mut app, Press::WheelUp, 90, 5);
        assert_eq!(names(&mut app)[1], "f01");
        for _ in 0..10 {
            press(&mut app, Press::WheelDown, 90, 5);
            names(&mut app);
        }
        assert_eq!(
            names(&mut app)[1],
            "f19",
            "never past the last row; then the cursor moves to it"
        );

        app.config.ui.wheel = Wheel::Page;
        press(&mut app, Press::WheelUp, 90, 5);
        assert_eq!(names(&mut app)[1], "f10", "a page up");
    }

    #[test]
    fn the_wheel_moves_the_cursor_where_the_rows_all_fit() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let files = (0..5).map(|n| file(&format!("f{n:02}"), 1)).collect();
        answer(&mut app, effects, &Listing::Dir(files));
        let name = |app: &mut App| {
            screen_of(app, 13);
            cursor_name(app, Side::Right)
        };
        name(&mut app);
        press(&mut app, Press::WheelDown, 90, 5);
        assert_eq!(name(&mut app), "f02", "three rows down from ..");
        press(&mut app, Press::WheelDown, 90, 5);
        assert_eq!(name(&mut app), "f04", "no further than the last row");
        press(&mut app, Press::WheelUp, 90, 5);
        assert_eq!(name(&mut app), "f01");
        assert_eq!(app.active, Side::Left, "the wheel does not take the keys");
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
        terminal
            .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
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
        open_menu(&mut app, Side::Right);
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
        assert_eq!(
            app.active,
            Side::Left,
            "the keys come back to the active panel"
        );
        app.handle(action(Action::FocusRight));
        open_menu(&mut app, Side::Right);
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

    /// The column of the first scroll bar of what is in front, and the rows of its arrows, as
    /// the last render drew it.
    fn bar_ends(app: &App) -> (u16, u16, u16) {
        let (_, bar) = app.scroll_bars()[0];
        let area = bar.area();
        (area.x, area.y, area.bottom() - 1)
    }

    #[test]
    fn the_mouse_scrolls_by_the_arrows_and_the_track_and_drags_the_thumb() {
        let mut app = loaded();
        app.handle(action(Action::Help));
        let top = screen_of(&mut app, 12);
        let (x, up, down) = bar_ends(&app);
        press(&mut app, Press::Click, x, down);
        assert_ne!(screen_of(&mut app, 12), top, "the arrow down");
        press(&mut app, Press::DoubleClick, x, up);
        assert_eq!(screen_of(&mut app, 12), top, "the arrow up, as the wheel");
        press(&mut app, Press::Click, x, down - 1);
        let paged = screen_of(&mut app, 12);
        app.handle(action(Action::PageDown));
        app.handle(action(Action::PageUp));
        assert_eq!(screen_of(&mut app, 12), paged, "a page, as PageDown");
        app.handle(action(Action::PageUp));
        assert_eq!(screen_of(&mut app, 12), top);

        press(&mut app, Press::Drag, x, down - 1);
        assert_eq!(screen_of(&mut app, 12), top, "no thumb is held");
        press(&mut app, Press::Click, x, up + 1);
        assert!(app.dragging());
        press(&mut app, Press::Drag, x, down - 1);
        let end = screen_of(&mut app, 12);
        assert!(end.contains(" End "), "{end}");
        press(&mut app, Press::Click, 10, 1);
        assert!(!app.dragging(), "another press lets go");
        press(&mut app, Press::Drag, x, up + 1);
        assert_eq!(screen_of(&mut app, 12), end);

        // The viewer's, through the same parts.
        app.handle(action(Action::Cancel));
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        answer(&mut app, effects, &Listing::Dir(vec![file("log", 1)]));
        app.handle(action(Action::Down));
        let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
            panic!("expected a read");
        };
        let lines: Vec<String> = (1..=100).map(|n| format!("line {n}")).collect();
        app.read(id, Ok(whole(lines.join("\n"))));
        screen_of(&mut app, 12);
        let (x, up, down) = bar_ends(&app);
        press(&mut app, Press::Click, x, down);
        assert!(screen_of(&mut app, 12).contains(" 4/100 "), "three lines");
        press(&mut app, Press::Click, x, up + 1);
        press(&mut app, Press::Drag, x, down - 1);
        let end = screen_of(&mut app, 12);
        assert!(
            end.contains(" 92/100 100% "),
            "the last line at the bottom: {end}"
        );
    }

    #[test]
    fn the_scroll_bars_of_the_panels_are_on_their_frames() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let files: Vec<DirEntry> = (10..60).map(|n| file(&format!("file{n}"), 1)).collect();
        answer(&mut app, effects, &Listing::Dir(files));
        let halves = |screen: &str| -> (Vec<String>, Vec<String>) {
            let rows = screen.lines();
            let half = |skip: usize| {
                rows.clone()
                    .map(|row| row.chars().skip(skip).take(80).collect())
                    .collect()
            };
            (half(0), half(81))
        };
        let before = halves(&screen_of(&mut app, 12));
        let bars = app.scroll_bars();
        assert_eq!(bars.len(), 2, "one on each panel");
        let (target, bar) = bars[0];
        assert_eq!(target, Scrolled::Panel(Side::Left));
        let area = bar.area();
        assert_eq!(area.x, 79, "on the right of the left panel's frame");
        press(&mut app, Press::Click, area.x, area.bottom() - 1);
        let scrolled = halves(&screen_of(&mut app, 12));
        assert_ne!(scrolled.0, before.0, "the left panel scrolled");
        assert_eq!(scrolled.1, before.1, "the right one did not");
        assert_eq!(app.active, Side::Left);
        press(&mut app, Press::Click, area.x, area.y + 1);
        assert!(app.dragging());
        press(&mut app, Press::Drag, area.x, area.bottom());
        let end = screen_of(&mut app, 12);
        assert!(end.contains("file59"), "{end}");
        assert!(
            cursor_name(&app, Side::Left).starts_with("file5"),
            "on screen"
        );

        // A panel that fits has none.
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        answer(&mut app, effects, &Listing::Dir(vec![file("one", 1)]));
        let screen = screen_of(&mut app, 12);
        assert!(!screen.contains('░'), "{screen}");
        assert_eq!(app.scroll_bars(), Vec::new());
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
            "on the row Ctrl+w f moved on to"
        );
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            app.handle(Resolved::Insert(c));
        }
    }

    /// Chooses row `row`, from 0, of the list in front, as g g, j, and Enter do; Enter as
    /// typed, which gives the keys back after Ctrl+x.
    fn choose(app: &mut App, row: usize) -> Vec<Effect> {
        app.handle(action(Action::Home));
        for _ in 0..row {
            app.handle(action(Action::Down));
        }
        app.typed(Typed::from(action(Action::Confirm)))
    }

    /// Types `text` in the filter of the list in front, after `/`.
    fn filter(app: &mut App, text: &str) {
        app.handle(action(Action::QuickSearch));
        type_text(app, text);
    }

    /// Types `text` in the text field in front in place of what it holds, after Ctrl+u.
    fn retype(app: &mut App, text: &str) {
        app.handle(action(Action::DeleteToStart));
        type_text(app, text);
    }

    #[test]
    fn ctrl_x_g_plus_and_g_minus_mark_in_the_other_panel() {
        let mut app = with_files(&["a.md", "b.txt", "c.md"]);
        app.handle(action(Action::Select));
        let text = screen_of(&mut app, 16);
        assert!(in_the_middle(&text, " Select "), "{text}");
        app.handle(action(Action::Cancel));

        assert!(keys(&mut app, &["ctrl-x", "g", "+"]).is_empty());
        assert_eq!(app.active, Side::Right, "the other panel has the keys");
        let text = screen_of(&mut app, 16);
        assert!(in_the_middle(&text, " Select "), "there too: {text}");
        retype(&mut app, "*.md");
        keys(&mut app, &["enter"]);
        assert_eq!(chosen_names(&app, Side::Right), ["a.md", "c.md"]);
        assert_eq!(chosen_names(&app, Side::Left), [] as [String; 0]);
        assert_eq!(app.active, Side::Left, "the keys come back once it closes");

        on_other(&mut app, Action::Unselect);
        let text = screen_of(&mut app, 16);
        assert!(in_the_middle(&text, " Unselect "), "{text}");
        retype(&mut app, "a*");
        keys(&mut app, &["enter"]);
        assert_eq!(chosen_names(&app, Side::Right), ["c.md"]);

        // g * inverts the marks of the other panel, which has no dialog.
        assert!(keys(&mut app, &["ctrl-x", "g", "*"]).is_empty());
        assert_eq!(chosen_names(&app, Side::Right), ["a.md", "b.txt"]);
        assert_eq!(chosen_names(&app, Side::Left), [] as [String; 0]);
        assert_eq!(app.active, Side::Left);
    }

    #[test]
    fn g_plus_and_g_minus_mark_and_unmark_by_pattern() {
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
        let text = screen(&mut app);
        assert!(text.contains("Select"));
        let glob = app.theme.glyphs.search_modes.glob;
        let smart = app.theme.glyphs.case_modes.smart;
        assert!(
            text.contains("Pattern:") && text.contains(&format!(" {smart} {glob}  ║")),
            "smart case, and shell patterns, end the field, as in every field that searches: {text}"
        );
        assert_eq!(app.key_place().search, Some(search::Kind::MARK));
        retype(&mut app, "*.md");
        app.handle(action(Action::CaseMatch));
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::Panel);
        let text = screen(&mut app);
        assert!(text.contains(" 10 B in 2 files "), "case counts: {text}");

        // The dialog opens with the last pattern, and its case.
        app.handle(action(Action::Select));
        let dialog = &app.dialogs.front().unwrap().dialog;
        assert_eq!(dialog.search(0).map(Input::case), Some(search::Case::Match));
        app.handle(action(Action::CaseSmart));
        app.handle(action(Action::Confirm));
        assert!(
            screen(&mut app).contains(" 30 B in 3 files "),
            "smart case: no capital"
        );

        // `g -` with Files only leaves the directory marked.
        app.handle(action(Action::Unselect));
        assert!(screen(&mut app).contains("Unselect"));
        retype(&mut app, "*");
        for step in [Action::NextField, Action::Toggle, Action::Confirm] {
            app.handle(action(step));
        }
        assert!(screen(&mut app).contains(" 0 B in 1 file "));

        // Esc and an empty pattern change nothing.
        app.handle(action(Action::Select));
        retype(&mut app, "c*");
        app.handle(action(Action::Cancel));
        app.handle(action(Action::Select));
        app.handle(action(Action::DeleteToStart));
        app.handle(action(Action::Confirm));
        assert!(screen(&mut app).contains(" 0 B in 1 file "));
        assert_eq!(app.pattern_options.pattern, "*");
    }

    #[test]
    fn each_field_that_searches_opens_as_it_was_last_until_noc_quits() {
        let mut app = loaded();
        app.config.ui.default_search_mode = SearchMode::Plain;
        let glyphs = app.theme.glyphs.search_modes;
        let cases = app.theme.glyphs.case_modes;
        let ends = |app: &mut App, case: &str, way: &str| {
            let text = screen(app);
            assert!(
                text.contains(&format!(" {case} {way} ║"))
                    || text.contains(&format!(" {case} {way}  ║")),
                "{case} {way}: {text}"
            );
        };
        // g + starts in the default too, which it has.
        app.handle(action(Action::Select));
        ends(&mut app, cases.smart, glyphs.plain);
        keys(&mut app, &["ctrl-s", "g", "ctrl-s", "i", "esc", "esc"]);
        assert_eq!(app.context(), Context::Panel);
        // The tab menu, switched and given up with Esc.
        app.handle(action(Action::TabList));
        ends(&mut app, cases.smart, glyphs.plain);
        keys(&mut app, &["ctrl-s", "r", "ctrl-s", "m", "esc", "esc"]);
        assert_eq!(app.context(), Context::Panel);
        app.config.ui.default_search_mode = SearchMode::Fzf;
        on_other(&mut app, Action::TabList);
        ends(&mut app, cases.exact, glyphs.regex);
        keys(&mut app, &["esc", "esc"]);
        // Each its own: the workspaces open in the default, which changed.
        app.handle(action(Action::Workspaces));
        ends(&mut app, cases.smart, glyphs.fzf);
        keys(&mut app, &["esc", "esc"]);
        app.handle(action(Action::Select));
        ends(&mut app, cases.ignore, glyphs.glob);
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
        retype(&mut app, "new");
        let (panel, location, host) = create_dir(app.handle(action(Action::Confirm)));
        assert_eq!(
            (panel.side, &location, host.is_none()),
            (Side::Left, &local("/srv/new"), true)
        );
        // The panel that asked waits, its dots turning, until the directory is made and read.
        let waits = |app: &App| Side::BOTH.map(|side| app.panel(side).wait().is_some());
        assert_eq!(waits(&app), [true, false]);

        // Both panels show /srv and read it again; the one that asked lands on the directory.
        let effects = app.created(panel, &location, Ok(()));
        assert_eq!(effects.len(), 2);
        assert_eq!(waits(&app), [true, true]);
        let listing = Listing::Dir(vec![dir("left"), dir("new"), dir("right")]);
        answer(&mut app, effects, &listing);
        assert_eq!(waits(&app), [false, false]);
        assert_eq!(app.panel(Side::Left).name_under_cursor(), Some(&b"new"[..]));
        assert_eq!(
            app.panel(Side::Right).name_under_cursor(),
            Some(&b"right"[..]),
            "stays"
        );

        // The field opens empty, not with the name under the cursor.
        app.handle(action(Action::Mkdir));
        assert_eq!(app.dialogs.front().map(|open| open.dialog.text()), Some(""));
        type_text(&mut app, "x");
        let (_, location, _) = create_dir(app.handle(action(Action::Confirm)));
        assert_eq!(location, local("/srv/x"));
        app.handle(action(Action::Mkdir));
        type_text(&mut app, "y");
        let (_, location, _) = create_dir(app.handle(action(Action::Confirm)));
        assert_eq!(location, local("/srv/y"));
        // A directory that is not made ends the wait as well, in an error.
        app.created(panel, &local("/srv/x"), Ok(()));
        assert!(
            app.created(panel, &location, Err("taken".into()))
                .is_empty()
        );
        for side in Side::BOTH {
            app.panel_mut(side).cancel();
        }
        assert_eq!(waits(&app), [false, false]);

        // Esc and an empty name make nothing.
        app.handle(action(Action::Mkdir));
        assert!(app.handle(action(Action::Cancel)).is_empty());
        app.handle(action(Action::Mkdir));
        app.handle(action(Action::DeleteToStart));
        assert!(app.handle(action(Action::Confirm)).is_empty());
    }

    #[test]
    fn alt_and_a_letter_act_on_a_dialog_while_its_field_takes_the_text() {
        let mut app = loaded();
        app.handle(action(Action::Mkdir));
        retype(&mut app, "new");
        // A letter nothing has does nothing: no Esc, no x deleting a character.
        keys(&mut app, &["alt-x"]);
        assert_eq!(app.place(), (Mode::Insert, Context::PathInput));
        assert_eq!(
            app.dialogs.front().map(|open| open.dialog.text()),
            Some("new")
        );
        let (_, location, _) = create_dir(keys(&mut app, &["alt-o"]));
        assert_eq!(location, local("/srv/new"), "OK, pressed from the field");

        // The letter of the prompt brings the focus back to the field, in Insert mode.
        app.handle(action(Action::Mkdir));
        keys(&mut app, &["down"]);
        assert_eq!(app.place(), (Mode::Normal, Context::Dialog));
        keys(&mut app, &["alt-e"]);
        assert_eq!(app.place(), (Mode::Insert, Context::PathInput));
        let text = screen(&mut app);
        assert!(text.contains("Enter directory name:"), "no & shows: {text}");
        keys(&mut app, &["esc", "esc"]);
        assert!(app.dialogs.is_empty(), "Cancel has no letter, Esc closes");
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
    fn f7_works_on_hosts() {
        let mut app = at_slash();
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
        let progress = report(
            current,
            Counts {
                done: 1,
                total: 3,
                ..Counts::default()
            },
        );
        app.job_event(id, progress);
        let text = screen(&mut app);
        assert!(
            text.contains("/srv/left/a") && text.contains("1 of 3"),
            "{text}"
        );

        // A failure waits for an answer: Ignore all is next to Ignore.
        let (reply, mut decision) = oneshot::channel();
        let failed = JobEvent::Failed {
            at: Instant::now(),
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
        assert!(screen(&mut app).contains("Delete 2 files?"));
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
        app.job_event(id, failed(path, error, reply));
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
        app.job_event(id, failed(path, error, reply));
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
        let progress = report(
            local("/srv/left/a"),
            Counts {
                done: 1,
                total: 4,
                ..Counts::default()
            },
        );
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
        app.job_event(second, failed(path, error, reply));
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
    fn a_job_in_the_background_draws_when_its_percentage_changes() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        let changes = |app: &mut App, done| {
            let progress = report(
                local("/srv/left/a"),
                Counts {
                    done,
                    total: 1000,
                    ..Counts::default()
                },
            );
            let (effects, changes) = app.job_report(id, progress);
            assert!(effects.is_empty());
            changes
        };
        assert!(changes(&mut app, 1), "its window shows every report");
        assert!(changes(&mut app, 2));

        app.handle(action(Action::Confirm));
        assert!(screen(&mut app).contains(" 1 job 0% "));
        assert!(!changes(&mut app, 3), "still 0%");
        assert!(changes(&mut app, 10));
        assert!(screen(&mut app).contains(" 1 job 1% "));
        assert!(!changes(&mut app, 11));

        app.handle(action(Action::Jobs));
        assert!(
            !changes(&mut app, 12),
            "its row shows 1%, as the corner does"
        );
        app.handle(action(Action::Cancel));
        assert!(app.jobs_list.is_none());
        assert!(!changes(&mut app, 13));

        let (_, changes) = app.job_report(id, JobEvent::Finished { complete: true });
        assert!(changes, "its end shows");
    }

    #[test]
    fn the_list_of_jobs_draws_for_a_job_behind_only_when_its_row_changes() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        app.handle(action(Action::Confirm));
        app.handle(action(Action::Jobs));
        let changes = |app: &mut App, current: &str, done, bytes_total| {
            let progress = report(
                local(current),
                Counts {
                    done,
                    total: 1000,
                    bytes_total,
                    ..Counts::default()
                },
            );
            app.job_report(id, progress).1
        };
        assert!(changes(&mut app, "/srv/left/a", 1, 0), "counted, it works");
        assert!(
            !changes(&mut app, "/srv/left/a", 2, 0),
            "its row shows no count"
        );
        assert!(changes(&mut app, "/srv/left/b", 3, 0), "the entry at hand");
        assert_eq!(app.spin_due(Instant::now()), None, "its row shows no time");
        changes(&mut app, "/srv/left/b", 3, 1024);
        assert!(
            app.spin_due(Instant::now()).is_some(),
            "the time left shows"
        );
    }

    #[test]
    fn a_job_in_front_draws_only_when_its_window_changes() {
        const MIB: u64 = 1024 * 1024;
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        let mut changes = |done, bytes_done| {
            let progress = report(
                local("/srv/left/a"),
                Counts {
                    done,
                    total: 2,
                    bytes_done,
                    bytes_total: 100 * MIB,
                    bytes_copied: bytes_done,
                },
            );
            app.job_report(id, progress).1
        };
        assert!(changes(0, 20 * MIB), "how far it is shows");
        assert!(!changes(0, 20 * MIB + 1_000), "still 20.0M of 100M, 20%");
        assert!(changes(0, 20 * MIB + 200 * 1024), "20.2M");
        assert!(changes(1, 20 * MIB + 200 * 1024), "1 of 2");
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
            let progress = report(
                local("/srv/left/a"),
                Counts {
                    done: 1,
                    total: 4,
                    ..Counts::default()
                },
            );
            app.job_event(id, progress);
        }
        assert!(ticking(&app, first) && ticking(&app, second));
        let now = Instant::now();
        let due = app.spin_due(now).unwrap();
        assert!(
            due > now && due <= now + Duration::from_millis(500),
            "the time in front shows 0:01 from half a second on"
        );

        // Both ask; the question of the second waits behind that of the first.
        let mut decisions = Vec::new();
        for id in [first, second] {
            let (reply, decision) = oneshot::channel();
            let path = local("/srv/left/b");
            let error = "busy".to_owned();
            app.job_event(id, failed(path, error, reply));
            decisions.push(decision);
        }
        assert!(!ticking(&app, first) && !ticking(&app, second));
        assert!(app.spin_due(now).is_none(), "nothing moves while they wait");

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
            at: Instant::now(),
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

    #[test]
    fn a_job_counts_its_reports_and_questions_at_the_times_it_made_them() {
        const MIB: u64 = 1024 * 1024;
        let mut app = loaded();
        app.handle(action(Action::Down));
        app.handle(action(Action::Delete));
        let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
        // Twenty seconds of work at a mebibyte a second that began half a minute ago, taken
        // only now, as by an event loop held up, in batches of eight reports: no stall, and
        // 20 MiB left.
        let start = Instant::now().checked_sub(Duration::from_secs(30)).unwrap();
        let at = |millis| start + Duration::from_millis(millis);
        let reports: Vec<(Instant, Counts)> = (0..=80)
            .map(|tick| {
                let bytes = tick * MIB / 4;
                let counts = Counts {
                    total: 1,
                    bytes_done: bytes,
                    bytes_total: 40 * MIB,
                    bytes_copied: bytes,
                    ..Counts::default()
                };
                (at(250 * tick), counts)
            })
            .collect();
        for batch in reports.chunks(8) {
            let current = local("/srv/left/a");
            let reports = batch.to_vec();
            app.job_event(id, JobEvent::Progress { current, reports });
        }
        let text = screen_at(&mut app, 12, at(20_000));
        assert!(text.contains("Time 0:20   ETA 0:20"), "{text}");

        // A question it asked at 25 seconds, answered at once at 30, stopped its clock at 25.
        let (reply, _decision) = oneshot::channel();
        let failed = JobEvent::Failed {
            at: at(25_000),
            path: local("/srv/left/a"),
            error: "busy".to_owned(),
            reply,
        };
        app.job_event(id, failed);
        app.handle(action(Action::Confirm));
        let text = screen_at(&mut app, 12, at(30_000));
        assert!(text.contains("Time 0:25"), "{text}");
    }

    /// A failure of a job at `path`, now, that waits for `reply`.
    fn failed(path: Location, error: String, reply: oneshot::Sender<Decision>) -> JobEvent {
        JobEvent::Failed {
            at: Instant::now(),
            path,
            error,
            reply,
        }
    }

    /// A report of a job at `current`, with `counts` behind it, made now.
    fn report(current: Location, counts: Counts) -> JobEvent {
        let reports = vec![(Instant::now(), counts)];
        JobEvent::Progress { current, reports }
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
        let progress = report(
            local("/srv/left/a"),
            Counts {
                done: 1,
                total: 2,
                ..Counts::default()
            },
        );
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
        app.handle(action(Action::FocusRight));
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
        assert!(screen_of(&mut app, 12).contains("1 job is still running. Quit and stop it?"));
        app.handle(action(Action::Left));
        app.handle(action(Action::Confirm));
        assert!(app.quits());
        assert!(!cancel.is_cancelled(), "not before the loop stops them");
        app.stop_jobs();
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn a_job_on_a_lost_host_ends_with_it() {
        let mut app = at_slash();
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
        assert!(effects.is_empty(), "the panel waits for an answer");
        assert!(app.jobs.is_empty());
        assert_eq!(app.dialogs.len(), 1, "no question about the job");
        assert!(matches!(
            app.dialogs.front().map(|open| &open.purpose),
            Some(Purpose::Reconnect { .. })
        ));
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
                at: Instant::now(),
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
        assert!(
            text.contains("New:      12345 bytes, 2023-11-14 19:26"),
            "{text}"
        );
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
            retype(app, text);
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
        app.handle(Resolved::Hotkey('p'));
        app.handle(action(Action::Confirm));
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
        retype(&mut app, "/tmp");
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
        assert!(text.contains("[x] Preserve attributes"), "{text}");
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
        retype(&mut app, "renamed");
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
    fn f6_may_leave_the_attributes_as_f5_does() {
        let mut app = two_directories();
        app.handle(action(Action::Move));
        app.handle(Resolved::Hotkey('p'));
        let (_, _, _, _, options) = copy_job(app.handle(action(Action::Confirm)));
        assert!(options.remove_sources && !options.preserve);
        app.jobs.clear();

        // One setting for both.
        app.handle(action(Action::Copy));
        assert!(screen(&mut app).contains("[ ] Preserve attributes"));
    }

    #[test]
    fn f5_f6_and_f8_count_the_marked_files_and_directories() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let mut link = dir("link");
        link.metadata.kind = FileKind::Symlink;
        link.target_kind = Some(FileKind::Dir);
        let entries = vec![dir("a"), dir("b"), link, file("c", 1), file("d", 2)];
        answer(&mut app, effects, &Listing::Dir(entries));
        app.handle(action(Action::Home));
        app.handle(action(Action::Down));
        for _ in 0..5 {
            app.handle(action(Action::Mark));
        }
        let asks = [
            (Action::Copy, "Copy 2 files and 3 directories to:"),
            (Action::Move, "Move 2 files and 3 directories to:"),
            (Action::Delete, "Delete 2 files and 3 directories?"),
        ];
        for (ask, question) in asks {
            app.handle(action(ask));
            let text = screen(&mut app);
            assert!(text.contains(question), "{text}");
            app.handle(action(Action::Cancel));
            assert!(app.dialogs.is_empty(), "{question}");
        }

        let mut app = loaded();
        app.handle(action(Action::Home));
        app.handle(action(Action::Down));
        app.handle(action(Action::Mark));
        app.handle(action(Action::Mark));
        app.handle(action(Action::Copy));
        let text = screen(&mut app);
        assert!(text.contains("Copy 2 directories to:"), "{text}");
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
        retype(&mut app, "left/inner");
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

    /// Puts the cursor of the left panel on `name`.
    fn cursor_on(app: &mut App, name: &str) {
        app.handle(action(Action::Home));
        for _ in 0..10 {
            if app.panel(Side::Left).name_under_cursor() == Some(name.as_bytes()) {
                return;
            }
            app.handle(action(Action::Down));
        }
        panic!("no {name}");
    }

    /// The text of the field of the dialog in front.
    fn field_text(app: &App) -> String {
        app.dialogs
            .front()
            .map(|open| open.dialog.text().to_owned())
            .unwrap_or_default()
    }

    #[test]
    fn shift_f5_copies_the_entry_under_the_cursor_beside_it_under_a_new_name() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let listing = Listing::Dir(vec![file("report.pdf", 1), dir("docs"), file("~", 2)]);
        answer(&mut app, effects, &listing);
        // The marks stay out of it.
        cursor_on(&mut app, "docs");
        app.handle(action(Action::Mark));
        cursor_on(&mut app, "report.pdf");
        app.handle(action(Action::Duplicate));
        let text = screen(&mut app);
        assert!(text.contains("Copy \"report.pdf\" to:"), "{text}");
        assert!(text.contains("[x] Preserve attributes"), "{text}");
        assert_eq!(field_text(&app), "report.pdf");
        type_text(&mut app, " (1)");
        let (_, sources, target, ends, options) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(sources, [local("/srv/report.pdf")]);
        assert_eq!(target, local("/srv/report (1).pdf"), "before the extension");
        assert_eq!(ends, (false, false));
        assert!(options.preserve && !options.remove_sources);
        let chosen: Vec<&[u8]> = app
            .panel(Side::Left)
            .chosen()
            .iter()
            .map(|entry| entry.name.as_slice())
            .collect();
        assert_eq!(chosen, [b"docs"], "still marked");
        app.jobs.clear();

        // The same name: the source itself.
        app.handle(action(Action::Duplicate));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot copy to /srv/report.pdf: the source and the target are the same"),
            "{text}"
        );
        app.handle(action(Action::Confirm));

        // A directory's name is whole; elsewhere is typed as for F5.
        cursor_on(&mut app, "docs");
        app.handle(action(Action::Duplicate));
        type_text(&mut app, "-2");
        let (_, sources, target, _, _) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(
            (sources, target),
            (vec![local("/srv/docs")], local("/srv/docs-2"))
        );
        app.jobs.clear();
        app.handle(action(Action::Duplicate));
        retype(&mut app, "/tmp");
        let (_, _, target, _, _) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(target, local("/tmp"));
        app.jobs.clear();

        // `~` alone would be the home directory.
        cursor_on(&mut app, "~");
        app.handle(action(Action::Duplicate));
        assert_eq!(field_text(&app), "\\~");
        type_text(&mut app, "2");
        let (_, _, target, _, _) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(target, local("/srv/~2"));
        app.jobs.clear();

        // Visual mode ends, and the entry under the cursor goes alone.
        cursor_on(&mut app, "docs");
        app.handle(action(Action::Down));
        app.handle(action(Action::VisualMode));
        app.handle(action(Action::Up));
        app.handle(action(Action::Duplicate));
        assert!(!app.panel(Side::Left).visual());
        assert_eq!(field_text(&app), "docs");
        app.handle(action(Action::Cancel));

        // Not on `..`.
        app.handle(action(Action::Home));
        app.handle(action(Action::Duplicate));
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn shift_f5_cannot_edit_a_name_that_is_not_utf8() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let entry = DirEntry {
            name: b"caf\xe9".to_vec(),
            ..file("x", 1)
        };
        answer(&mut app, effects, &Listing::Dir(vec![entry]));
        app.handle(action(Action::Down));
        app.handle(action(Action::Duplicate));
        let text = screen(&mut app);
        assert!(
            text.contains("the name is not valid UTF-8; F5 can copy it"),
            "{text}"
        );
    }

    #[test]
    fn a_copy_to_a_lost_host_ends_with_it() {
        let mut app = at_slash();
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
        let effects = app.go(app.shown(Side::Left), Destination::to(local("/srv")));
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
        let Effect::Read(ReadRequest {
            id,
            location,
            host,
            cancel,
            ..
        }) = one(app.handle(action(Action::View)))
        else {
            panic!("expected a read");
        };
        assert_eq!((location, host.is_none()), (local("/srv/notes"), true));
        assert_eq!(app.context(), Context::Viewer);
        // While it is read, the dots stand in place of its icon, and its window shows how far.
        let text = screen(&mut app);
        let rows: Vec<&str> = text.lines().map(|row| row.trim_matches('"')).collect();
        assert!(rows[0].starts_with(" … /srv/notes "), "{text}");
        assert!(text.contains("Reading") && !text.contains("---"), "{text}");
        app.read(id + 1, Ok(whole(*b"not this")));
        app.read(id, Ok(whole(*b"hello\nworld\n")));
        let text = screen(&mut app);
        assert!(text.starts_with("\"   /srv/notes "), "{text}");
        assert!(text.contains("hello") && !text.contains("---"), "{text}");
        assert!(text.contains("2Wrap") && text.contains("10Quit"), "{text}");

        // F1 shows the help over it, and Esc goes back to it.
        app.handle(action(Action::Help));
        assert_eq!(app.context(), Context::Help);
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
    fn a_file_viewed_from_a_line_shows_it_at_the_top() {
        let mut app = with_files(&["a1"]);
        let effects = app.view_file(local("/srv/a1"), &local("/srv"), Some(3));
        let Some(Effect::Read(ReadRequest { id, .. })) = effects.first() else {
            panic!("expected a read, got {effects:?}");
        };
        let text = (1..=40)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.read(*id, Ok(whole(text)));
        let screen = screen_of(&mut app, 12);
        assert!(
            screen.contains("line 3") && !screen.contains("line 2 "),
            "{screen}"
        );
    }

    #[test]
    fn a_file_edited_at_a_line_hands_the_line_to_the_editor() {
        let mut app = with_files(&["a1"]);
        app.edit_file(local("/srv"), b"a1", Some(7));
        assert!(matches!(
            app.take_handover(),
            Some(Handover::Edit { file, line: Some(7) }) if file == Path::new("/srv/a1")
        ));
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

    #[test]
    fn g_x_opens_the_marked_entries_or_the_one_under_the_cursor_externally() {
        let mut app = with_files(&["a b", "c", "d"]);
        assert!(keys(&mut app, &["g", "x"]).is_empty(), "nothing on ..");
        let effects = keys(&mut app, &["j", "g", "x"]);
        let [Effect::Open(paths)] = &effects[..] else {
            panic!("expected an open, got {effects:?}");
        };
        assert_eq!(paths, &[PathBuf::from("/srv/a b")]);
        // In Visual mode, the rows selected, which become the marks.
        let effects = keys(&mut app, &["j", "shift-v", "j", "g", "x"]);
        let [Effect::Open(paths)] = &effects[..] else {
            panic!("expected an open, got {effects:?}");
        };
        assert_eq!(paths, &[PathBuf::from("/srv/c"), PathBuf::from("/srv/d")]);
        assert_eq!(app.mode(), Mode::Normal);
        assert_eq!(chosen_names(&app, Side::Left), ["c", "d"]);

        app.open_failed(Path::new("/srv/c"), "xdg-open exited with exit status: 3");
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot open /srv/c: xdg-open exited with exit status: 3"),
            "{text}"
        );
    }

    #[test]
    fn g_x_counts_in_zoxide() {
        let mut app = recording();
        let effects = keys(&mut app, &["shift-g", "g", "x"]);
        assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);
    }

    #[test]
    fn g_x_on_a_host_says_that_only_local_files_open_externally() {
        let (mut app, _) = on_a_host();
        assert!(keys(&mut app, &["g", "x"]).is_empty());
        let text = screen(&mut app);
        assert!(text.contains("Only local files open externally"), "{text}");
    }

    /// An app whose left panel is on `web:/home/deploy`, which holds `notes`.
    fn on_a_host() -> (App, u64) {
        let mut app = at_slash();
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
        app.handle(action(Action::Cancel));
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
        let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
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

    /// The viewer of `/srv/big` with the read it starts.
    fn view_big() -> (App, Effect) {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        answer(&mut app, effects, &Listing::Dir(vec![file("big", 1000)]));
        app.handle(action(Action::Down));
        let read = one(app.handle(action(Action::View)));
        (app, read)
    }

    /// The part of `/srv/big`, 1,000 bytes long, that `want` asks for, holding `bytes`.
    fn part_of_big(want: Want, bytes: &[u8]) -> ViewerPart {
        let stamp = Stamp {
            size: Some(1000),
            modified: None,
        };
        ViewerPart::new(want, bytes, (false, false), stamp)
    }

    #[test]
    fn a_read_that_takes_long_shows_a_window_that_stops_it() {
        let (mut app, read) = view_big();
        let Effect::Read(ReadRequest {
            id,
            want,
            stop,
            cancel,
            ..
        }) = read
        else {
            panic!("expected a read");
        };
        assert_eq!(want.offset, 0);
        let text = screen_of(&mut app, 16);
        assert!(text.contains("Reading") && text.contains("Stop"), "{text}");
        assert_eq!(app.context(), Context::Dialog);
        assert_eq!(app.place().0, Mode::Normal);
        app.read_progress(
            id,
            ReadProgress {
                read: 250,
                total: 1000,
                size: Some(1000),
            },
        );
        let text = screen_of(&mut app, 16);
        assert!(text.contains("25%"), "{text}");

        // Esc stops the read and keeps what it has read.
        app.handle(action(Action::Cancel));
        assert!(stop.is_cancelled() && !cancel.is_cancelled());
        assert_eq!(app.context(), Context::Viewer);
        let mut part = part_of_big(want, b"one\ntwo\n");
        part.stopped = true;
        assert!(
            app.read(id, Ok(part)).is_empty(),
            "a stopped read starts no other"
        );
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("two") && text.contains("more below"),
            "{text}"
        );
        assert!(!text.contains("Reading"), "{text}");
    }

    #[test]
    fn the_window_of_a_read_ends_full_and_then_goes() {
        let (mut app, read) = view_big();
        let Effect::Read(ReadRequest { id, want, .. }) = read else {
            panic!("expected a read");
        };
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        screen_at(&mut app, 16, at(0));
        let progress = ReadProgress {
            read: 980,
            total: 1000,
            size: Some(1000),
        };
        app.read_progress(id, progress);
        let text = screen_at(&mut app, 16, at(200));
        assert!(text.contains("98%"), "{text}");

        // The part comes before a report of its end: the window shows it full, over the text,
        // and leaves the keys to the viewer.
        app.read(id, Ok(part_of_big(want, b"one\ntwo\n")));
        let text = screen_at(&mut app, 16, at(300));
        assert!(text.contains("100%") && text.contains("two"), "{text}");
        assert_eq!(app.context(), Context::Viewer);
        let goes = app.spin_due(at(300)).unwrap();
        assert_eq!(goes, at(300) + spinner::DELAY);
        let text = screen_at(&mut app, 16, goes);
        assert!(!text.contains("Reading"), "{text}");
        assert_eq!(app.spin_due(goes), None);
    }

    #[test]
    fn close_in_the_window_of_a_read_closes_the_viewer() {
        let (mut app, read) = view_big();
        let Effect::Read(ReadRequest { cancel, .. }) = read else {
            panic!("expected a read");
        };
        screen_of(&mut app, 16);
        app.handle(action(Action::NextField));
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::Panel);
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn the_viewer_reads_the_next_part_at_the_end_and_keeps_its_text_if_it_fails() {
        let (mut app, read) = view_big();
        let Effect::Read(ReadRequest { id, want, .. }) = read else {
            panic!("expected a read");
        };
        assert!(
            app.read(id, Ok(part_of_big(want, b"one\ntwo\n")))
                .is_empty()
        );
        screen_of(&mut app, 16);
        let Effect::Read(ReadRequest { id, want, .. }) = one(app.handle(action(Action::End)))
        else {
            panic!("expected the next part");
        };
        assert_eq!((want.offset, want.new), (8, true));
        assert!(app.read(id, Err("broken pipe".to_owned())).is_empty());
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("Cannot read more of /srv/big: broken pipe"),
            "{text}"
        );
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Viewer);
        let text = screen_of(&mut app, 16);
        assert!(text.contains("two"), "{text}");
    }

    #[test]
    fn a_file_that_changed_between_parts_is_read_again() {
        let (mut app, read) = view_big();
        let Effect::Read(ReadRequest { id, want, .. }) = read else {
            panic!("expected a read");
        };
        assert!(
            app.read(id, Ok(part_of_big(want, b"one\ntwo\n")))
                .is_empty()
        );
        screen_of(&mut app, 16);
        let Effect::Read(ReadRequest { id, want, .. }) = one(app.handle(action(Action::End)))
        else {
            panic!("expected the next part");
        };
        let mut part = part_of_big(want, b"three\n");
        part.stamp.size = Some(2000);
        let Effect::Read(ReadRequest { want, .. }) = one(app.read(id, Ok(part))) else {
            panic!("expected the file again");
        };
        assert_eq!(want.offset, 0);
        let text = screen_of(&mut app, 16);
        assert!(text.contains("/srv/big changed"), "{text}");
    }

    #[test]
    fn pattern_dialogs_wait_for_no_prompt() {
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
        terminal
            .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
            .unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn a_host_connects_once_and_then_lists_for_every_panel_that_waits() {
        let mut app = at_slash();
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
        assert_eq!(text.matches("╔ … web ═").count(), 2, "{text}");

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
        let effects = enter_host(&mut app, Side::Left, 1);
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
        let mut app = at_slash();
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
        let mut app = at_slash();
        let words = |words: &[&str]| {
            noc_config::Run::try_from(
                words
                    .iter()
                    .map(|&word| word.to_owned())
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        let site = SftpHost {
            label: Some("Site".to_owned()),
            run: Some(words(&["$SHELL", "-c"])),
            ..SftpHost::default()
        };
        with_host(&mut app, "web", site);
        app.config.shell.run = words(&["/bin/zsh", "-ic"]);
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
            shell,
            command,
            ..
        }) = app.take_run()
        else {
            panic!("expected a remote command");
        };
        assert_eq!((dir.as_bytes(), command.as_str()), (&b"/var/www"[..], "ls"));
        assert_eq!(shell, words(&["$SHELL", "-c"]), "the host's own");
        let effects = app.ran(Ok(()));
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::List { request, host: Some(_), .. } if request.location == remote("web", "/var/www")
            )),
            "{effects:?}"
        );

        // The local panel runs with shell.run.
        app.handle(action(Action::SwitchPanel));
        app.handle(action(Action::Shell));
        type_text(&mut app, "ls");
        app.handle(action(Action::Confirm));
        let Some(Run {
            place: Place::Local(_),
            shell,
            ..
        }) = app.take_run()
        else {
            panic!("expected a local command");
        };
        assert_eq!(shell, words(&["/bin/zsh", "-ic"]));
    }

    #[test]
    fn remember_dir_resumes_the_last_directory_of_the_session() {
        for remember_dir in [true, false] {
            let mut app = at_slash();
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

            app.disconnect("web");
            let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 1))
            else {
                panic!("expected a connection");
            };
            let again = connect_web(&mut app, Side::Right, connection, "/var/www/app");
            let expected = remember_dir.then(|| RemotePath::from("/var/www/app"));
            assert_eq!(again.resume, expected, "remember_dir = {remember_dir}");
        }
    }

    /// Opens the dialog of F4 on `web` from the location menu of the left panel.
    fn edit_web(app: &mut App) {
        if app.menu.is_none() {
            menu_on(app, Side::Left);
        }
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
        let mut app = at_slash();
        menu_on(&mut app, Side::Left);
        assert!(!screen_of(&mut app, 14).contains("4Edit"), "not on Home");
        app.handle(action(Action::Down));
        assert!(screen_of(&mut app, 14).contains("4Edit"));
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
        assert!(
            matches!(&effects[..], [Effect::ListPlaces { .. }]),
            "the menu reads the hosts again"
        );

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
        let mut app = at_slash();
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
    fn f4_on_a_host_shows_shell_run_until_it_gets_a_run_of_its_own() {
        let mut app = at_slash();
        edit_web(&mut app);
        let text = screen_of(&mut app, 24);
        assert!(text.contains("$SHELL -i -c"), "the placeholder: {text}");
        for _ in 0..3 {
            app.handle(action(Action::Down));
        }
        typed(&mut app, "${SHELL -c");
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert_eq!(app.dialogs.len(), 2, "the reason shows over the dialog");
        let text = screen_of(&mut app, 24);
        assert!(text.contains("has no closing }"), "{text}");
        app.handle(action(Action::Confirm));
        app.handle(action(Action::DeleteToStart));
        typed(&mut app, "$SHELL -c");
        let Effect::SaveHost { host, .. } = one(app.handle(action(Action::Confirm))) else {
            panic!("expected the settings to be saved");
        };
        let run = host.as_ref().and_then(HostConfig::run).unwrap();
        assert_eq!(Vec::<String>::from(run.clone()), ["$SHELL", "-c"]);
    }

    #[test]
    fn use_current_fills_in_the_directory_open_on_the_host() {
        let mut app = at_slash();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 1)) else {
            panic!("expected a connection");
        };
        connect_web(&mut app, Side::Right, connection, "/var/www");
        edit_web(&mut app);
        // Past the four fields and the check box, then OK, to Use Current.
        for _ in 0..6 {
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
    fn a_failed_connection_asks_to_retry_while_the_panel_waits() {
        let mut app = at_slash();
        // The dots turn, so that whether they are due says what waits, however long the test
        // takes: without turning, they are due only before they first show.
        app.config.animations.spinners = true;
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
            panic!("expected a connection");
        };
        let refused = "deploy@db: Permission denied (publickey).";
        assert!(app.closed("db", connection, Some(refused)).is_empty());
        assert!(
            app.closed("db", connection, Some(refused)).is_empty(),
            "reported once"
        );
        assert_eq!(app.host_state("db").status, HostStatus::Stalled);
        assert_eq!(app.dialogs.len(), 1);
        let text = screen_of(&mut app, 20);
        assert_eq!(
            app.spin_due(Instant::now()),
            None,
            "the panel waits for the answer, not for work: nothing turns"
        );
        assert!(
            text.contains("Cannot connect to db: deploy@db: Permission denied (publickey)."),
            "{text}"
        );
        assert!(!text.contains("Retry or Cancel"), "the dialog asks: {text}");

        // Retry, the default, connects again for the panel that still waits.
        let Effect::Connect {
            connection: again, ..
        } = one(app.handle(action(Action::Confirm)))
        else {
            panic!("expected a new connection");
        };
        assert_ne!(again, connection);
        assert_eq!(app.host_state("db").status, HostStatus::Connecting);
        assert!(
            app.spin_due(Instant::now()).is_some(),
            "the dots turn again"
        );
        let waiting = app.panel(Side::Left).pending_request();
        assert_eq!(
            waiting.map(|request| request.location),
            Some(remote("db", ""))
        );

        // Cancel: the panel stays where it was; the dialog said why, so no other one does.
        assert!(app.closed("db", again, Some(refused)).is_empty());
        assert!(app.handle(action(Action::Cancel)).is_empty());
        assert_eq!(app.host_state("db").status, HostStatus::Failed);
        assert_eq!(app.panel(Side::Left).location(), &local("/"));
        assert_eq!(app.panel(Side::Left).pending_request(), None);
        assert!(app.dialogs.is_empty());
        assert_eq!(app.panel(Side::Left).not_connected(), None);
        let text = screen(&mut app);
        assert!(!text.contains("Cannot open"), "{text}");
        let Effect::Connect { .. } = one(enter_host(&mut app, Side::Left, 2)) else {
            panic!("expected a new connection");
        };
    }

    #[test]
    fn cancel_stops_a_connection_attempt_for_every_panel() {
        let mut app = at_slash();
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
    fn a_lost_connection_tones_down_its_panels_and_asks_to_retry() {
        let mut app = at_slash();
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

        assert!(
            app.closed("web", connection, Some("Broken pipe"))
                .is_empty()
        );
        assert!(!stop.is_cancelled(), "it ended on its own");
        let text = screen_of(&mut app, 20);
        assert!(text.contains("www"), "the entries stay: {text}");
        assert!(
            text.contains("Lost the connection to web: Broken pipe"),
            "{text}"
        );
        assert_eq!(app.dialogs.len(), 1);
        assert_eq!(app.panel(Side::Left).location(), &remote("web", ""));
        assert_eq!(
            app.spin_due(Instant::now()),
            None,
            "the dialog may wait for hours; the screen sleeps"
        );

        // Retry reads the directory again once the host is back.
        let Effect::Connect { connection, .. } = one(app.handle(action(Action::Confirm))) else {
            panic!("expected a new connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected the directory, got {effects:?}");
        };
        assert_eq!(request.location, remote("web", ""));
        answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
        assert!(screen(&mut app).contains("www"));

        // Cancel leaves the panel on the host, with its entries toned down, until it reads it
        // again.
        app.closed("web", connection, Some("Broken pipe"));
        app.handle(action(Action::Cancel));
        assert!(app.dialogs.is_empty(), "the dialog said why already");
        assert_eq!(app.panel(Side::Left).not_connected(), Some("web"));
        let text = screen(&mut app);
        assert!(text.contains("www"), "{text}");
        assert!(!text.contains("Cannot open"), "{text}");
        let Effect::Connect { .. } = one(app.handle(action(Action::Reload))) else {
            panic!("expected a new connection");
        };
    }

    #[test]
    fn f8_in_the_location_menu_disconnects_the_host_under_the_cursor() {
        let mut app = at_slash();
        let Effect::Connect {
            connection, stop, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
        assert_eq!(app.host_state("web").status, HostStatus::Connected);
        assert!(!screen(&mut app).contains("Disconn"), "F8 deletes here");

        // From the other panel's menu.
        menu_on(&mut app, Side::Right);
        assert!(screen_of(&mut app, 14).contains("8Disconn"));
        app.handle(action(Action::Down));
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        assert!(stop.is_cancelled());
        assert_eq!(app.host_state("web").status, HostStatus::Idle);
        assert!(
            app.closed("web", connection, None).is_empty(),
            "already gone"
        );
        app.handle(action(Action::Cancel));
        let text = screen(&mut app);
        assert!(text.contains("www"), "the entries stay: {text}");
        assert_eq!(app.panel(Side::Left).not_connected(), Some("web"));
        assert!(app.dialogs.is_empty(), "asked for, so no dialog");
        assert_eq!(app.panel(Side::Left).location(), &remote("web", ""));
        // Nothing to close, or not a host.
        menu_on(&mut app, Side::Right);
        app.handle(action(Action::Down));
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        app.handle(action(Action::Home));
        assert!(app.handle(action(Action::Disconnect)).is_empty());
    }

    /// Opens the location menu of the panel on `side`, `g l` or Ctrl+x `g l`, and answers its
    /// listing with the system volume, a USB stick, and the hosts `web` and `db`.
    fn open_menu(app: &mut App, side: Side) {
        let effects = if side == app.active {
            app.handle(action(Action::LocationMenu))
        } else {
            on_other(app, Action::LocationMenu)
        };
        let Effect::ListPlaces { generation } = one(effects) else {
            panic!("expected a listing for the menu");
        };
        let volume = |path: &str, kind| noc_vfs::Volume {
            mount_point: PathBuf::from(path),
            label: None,
            fs_type: None,
            kind,
            space: None,
        };
        let places = Places {
            volumes: vec![
                volume("/", noc_vfs::VolumeKind::System),
                volume("/Volumes/USB", noc_vfs::VolumeKind::Local),
            ],
            ..places()
        };
        app.places(generation, Ok(places));
    }

    #[test]
    fn g_l_and_ctrl_x_g_l_change_the_location_of_their_panel() {
        let mut app = loaded();
        open_menu(&mut app, Side::Right);
        assert_eq!(app.context(), Context::LocationMenu);
        let text = screen_of(&mut app, 14);
        assert!(
            in_the_middle(&text, " Location ") && text.contains("/Volumes/USB"),
            "centered over both panels, for the other one: {text}"
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
        } = one(choose(&mut app, 2))
        else {
            panic!("expected a listing");
        };
        assert_eq!(
            (side, request.location),
            (Side::Right, local("/Volumes/USB"))
        );
        assert_eq!(
            app.active,
            Side::Left,
            "the keys come back to the active panel"
        );
        assert_eq!(app.context(), Context::Panel);

        // Ctrl+c closes the menu; a host connects.
        app.handle(action(Action::SwitchPanel));
        open_menu(&mut app, Side::Left);
        keys(&mut app, &["ctrl-c"]);
        assert!(app.menu.is_none());
        assert_eq!(app.active, Side::Right);
        open_menu(&mut app, Side::Left);
        filter(&mut app, "w");
        let effect = one(keys(&mut app, &["enter"]));
        assert!(
            matches!(&effect, Effect::Connect { host, .. } if host == "web"),
            "a host connects: {effect:?}"
        );
        assert_eq!(app.active, Side::Right, "from the right panel too");
    }

    #[test]
    fn the_menus_of_a_panel_name_no_side_and_reach_it_after_ctrl_w_x() {
        let mut app = loaded();
        app.handle(action(Action::SwapPanels));
        open_menu(&mut app, Side::Right);
        let text = screen_of(&mut app, 14);
        assert!(in_the_middle(&text, " Location "), "{text}");
        assert!(
            !text.contains(" Left ") && !text.contains(" Right "),
            "{text}"
        );
        let Effect::List {
            panel: PanelId { side, .. },
            ..
        } = one(choose(&mut app, 2))
        else {
            panic!("expected a listing");
        };
        assert_eq!(side, Side::Right, "the right panel, drawn on the left");

        assert!(on_other(&mut app, Action::TabList).is_empty());
        let text = screen_of(&mut app, 14);
        assert!(in_the_middle(&text, " Tabs "), "the tabs too: {text}");
        assert!(!text.contains("Left tabs") && !text.contains("Right tabs"));
        assert_eq!(app.tab_menu.as_ref().map(TabMenu::side), Some(Side::Right));
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
        let location = line_with(&text, "Change location…");
        assert!(
            location.contains("g l") && !location.contains("Ctrl+x"),
            "{text}"
        );
        let sort = line_with(&text, "Sort by size");
        assert!(
            sort.contains("o s") && !sort.contains("Ctrl+x"),
            "the keys of the active panel: {text}"
        );
        assert!(
            text.contains("9Cancel") && text.contains("10Cancel"),
            "{text}"
        );

        // Right goes round to Left, which acts on the panel on its side: Ctrl+x and its keys.
        app.handle(action(Action::Right));
        let text = screen_of(&mut app, 20);
        assert!(
            line_with(&text, "Sort by size").contains("Ctrl+x o s"),
            "{text}"
        );
        assert!(
            line_with(&text, "Change location…").contains("Ctrl+x g l"),
            "{text}"
        );
        assert!(
            line_with(&text, "Tab list…").contains("Ctrl+x t t"),
            "{text}"
        );
        assert!(app.handle(Resolved::Hotkey('z')).is_empty());
        assert!(app.pulldown.is_none(), "a command closes the menu");
        assert_eq!(app.panel(Side::Left).sort_action(), Action::SortBySize);
        assert_eq!(app.panel(Side::Right).sort_action(), Action::SortByName);
        assert_eq!(app.active, Side::Right);

        // F9 opens the bar alone again; Shift+F9, the menu on the command that ran last, even
        // after Esc left another one.
        app.handle(action(Action::PullDown));
        let text = screen_of(&mut app, 20);
        assert!(!text.contains("Sort by size"), "the bar alone: {text}");
        app.handle(Resolved::Hotkey('f'));
        app.handle(action(Action::Cancel));
        app.handle(action(Action::Cancel));
        assert!(app.pulldown.is_none());
        app.handle(action(Action::PullDownLast));
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
        assert!(app.handle(Resolved::Hotkey('o')).is_empty());
        assert!(screen_of(&mut app, 20).contains("x Show hidden files"));
        app.handle(Resolved::Hotkey('h'));
        assert!(!app.config.ui.show_hidden);

        // Commands that cannot run do nothing; Esc goes back to the bar, then closes it.
        app.handle(action(Action::PullDown));
        app.handle(Resolved::Hotkey('f'));
        assert!(
            app.handle(Resolved::Hotkey('k')).is_empty(),
            "nothing chosen"
        );
        assert!(app.pulldown.is_some());
        app.handle(action(Action::Cancel));
        assert!(app.pulldown.is_some());
        app.handle(action(Action::Cancel));
        assert!(app.pulldown.is_none());

        // A command that opens something: the location menu of the panel on its side.
        app.handle(action(Action::PullDown));
        app.handle(Resolved::Hotkey('l'));
        let effects = app.handle(Resolved::Hotkey('l'));
        assert!(matches!(&effects[..], [Effect::ListPlaces { .. }]));
        assert_eq!(app.menu.as_ref().map(LocationMenu::side), Some(Side::Left));
    }

    /// Opens Options → Configuration… through F9.
    fn open_configuration(app: &mut App) {
        app.handle(action(Action::PullDown));
        app.handle(Resolved::Hotkey('o'));
        assert!(app.handle(Resolved::Hotkey('c')).is_empty());
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
        for _ in 0..2 {
            assert!(
                app.handle(action(Action::Down)).is_empty(),
                "nothing changed"
            );
        }
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

    /// Types `text` in the text field under the cursor in place of what it holds: A, Ctrl+u,
    /// the text, and Esc, as typed in Normal mode.
    fn edit_field(app: &mut App, text: &str) {
        app.handle(action(Action::AppendAtEnd));
        retype(app, text);
        app.handle(action(Action::NormalMode));
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
        config_category(&mut app, 2);
        app.handle(action(Action::Toggle));
        assert!(!app.copy_choices.atomic);
        app.handle(action(Action::Down));
        edit_field(&mut app, "5");
        config_category(&mut app, 3);
        assert_eq!(app.parallel_jobs, 5, "as the cursor left the field");
        edit_field(&mut app, "~/bin/ssh");
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
        open_menu(&mut app, Side::Left);
        assert!(
            app.handle(action(Action::Disconnect)).is_empty(),
            "a volume"
        );
        app.handle(action(Action::End));
        let effect = one(app.handle(action(Action::Confirm)));
        assert!(matches!(effect, Effect::Connect { .. }));
        open_menu(&mut app, Side::Left);
        assert!(app.hosts.contains_key("db"));
        app.handle(action(Action::End));
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        assert!(!app.hosts.contains_key("db"), "stopped connecting");
        // R asks again; the earlier listing no longer counts.
        let effects = app.handle(action(Action::Reload));
        assert!(matches!(
            &effects[..],
            [Effect::ListPlaces { generation: 3 }]
        ));
        assert!(app.menu.is_some());
    }

    #[test]
    fn hosts_and_volumes_used_last_come_first_and_go_to_recent_toml() {
        let mut app = at_slash();
        assert_eq!(app.take_recent(), None);
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
            panic!("expected a connection");
        };
        assert_eq!(app.take_recent(), None, "not before it connects");
        let (handle, _requests) = HostHandle::channel();
        app.connected("db", connection, handle);
        let recent = app.take_recent().unwrap();
        assert_eq!(recent.hosts, ["db"]);
        assert_eq!(app.take_recent(), None, "once");

        // `recent.toml` arrives late: what was used since comes first.
        let saved = Recent {
            hosts: vec!["web".to_owned(), "db".to_owned()],
            volumes: vec![PathBuf::from("/")],
        };
        app.recent_loaded(Ok(saved));
        assert_eq!(app.recent.hosts, ["db", "web"]);
        assert_eq!(
            app.take_recent().map(|recent| recent.hosts),
            Some(app.recent.hosts.clone())
        );
        app.recent_loaded(Err("unreadable".to_owned()));
        assert_eq!(app.recent.hosts, ["db", "web"]);

        // db comes first in the menu, after the home directory.
        menu_on(&mut app, Side::Right);
        let Effect::List { request, host, .. } = one(choose(&mut app, 1)) else {
            panic!("expected a listing");
        };
        assert_eq!((request.location, host.is_some()), (remote("db", ""), true));

        // A panel that arrives on a volume uses it.
        let volume = |path: &str| noc_vfs::Volume {
            mount_point: PathBuf::from(path),
            label: None,
            fs_type: None,
            kind: noc_vfs::VolumeKind::Local,
            space: None,
        };
        let places = Places {
            volumes: vec![volume("/"), volume("/Volumes/USB")],
            hosts: Vec::new(),
        };
        app.places(0, Ok(places));
        app.active = Side::Left;
        let effects = app.go(
            app.shown(Side::Left),
            Destination::to(local("/Volumes/USB/a")),
        );
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        assert_eq!(
            app.take_recent().map(|recent| recent.volumes),
            Some(vec![PathBuf::from("/Volumes/USB"), PathBuf::from("/")])
        );
        let effects = app.go(
            app.shown(Side::Left),
            Destination::to(local("/Volumes/USB/b")),
        );
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        assert_eq!(app.take_recent(), None, "already the first");
    }

    #[test]
    fn disconnect_stops_a_connection_attempt() {
        let mut app = at_slash();
        let Effect::Connect { stop, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
            panic!("expected a connection");
        };
        assert_eq!(app.host_state("db").status, HostStatus::Connecting);
        menu_on(&mut app, Side::Left);
        app.handle(action(Action::End));
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        assert!(stop.is_cancelled());
        assert_eq!(app.panel(Side::Left).pending_request(), None);
        assert_eq!(app.host_state("db").status, HostStatus::Idle);
    }

    #[test]
    fn the_menu_shows_failures_and_fresh_addresses() {
        let mut app = at_slash();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        app.resolved("web".to_owned(), "deploy@10.0.0.9".to_owned(), "deploy");
        app.closed("web", connection, Some("Connection refused"));
        app.handle(action(Action::Cancel));
        assert_eq!(
            app.host_state("web"),
            HostState {
                status: HostStatus::Failed,
                address: Some("deploy@10.0.0.9".to_owned()),
            }
        );
        menu_on(&mut app, Side::Left);
        let text = screen_of(&mut app, 14);
        assert!(text.contains("✗ web"), "{text}");
        assert!(text.contains("deploy@10.0.0.9"), "{text}");

        // A new attempt clears the failure.
        app.handle(action(Action::Cancel));
        enter_host(&mut app, Side::Left, 1);
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
        let mut app = at_slash();
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
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn dialogs_close_when_ssh_stops_waiting() {
        let mut app = at_slash();
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
        assert_eq!(app.context(), Context::Panel, "OK dismisses a notice");

        app.notice(4, "web", "Touch your key");
        app.prompt_closed(4);
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn the_help_opens_on_about_and_then_where_it_was_left() {
        let mut app = loaded();
        app.handle(action(Action::Help));
        let text = screen_of(&mut app, 30);
        assert!(
            text.contains("Noon Commander") && text.contains("Search and renaming"),
            "{text}"
        );
        // The section and how far down it was last until the help opens again, from anywhere.
        app.handle(action(Action::NextField));
        for _ in 0..4 {
            app.handle(action(Action::Down));
        }
        app.handle(action(Action::Confirm));
        let top = screen_of(&mut app, 30);
        assert!(top.contains("Text fields in Insert mode"), "{top}");
        app.handle(action(Action::Down));
        let left = screen_of(&mut app, 30);
        assert!(!left.contains("Text fields in Insert mode"), "{left}");
        app.handle(action(Action::Cancel));
        app.handle(action(Action::Help));
        assert_eq!(screen_of(&mut app, 30), left);
        // So does Keys folded; a click on it opens it again.
        app.handle(action(Action::NextField));
        app.handle(action(Action::Left));
        app.handle(action(Action::Left));
        app.handle(action(Action::Cancel));
        app.handle(action(Action::Help));
        let text = screen_of(&mut app, 30);
        assert!(
            !text.contains("Search and renaming") && text.contains("Keys ▸"),
            "{text}"
        );
        let (x, y) = find(&mut app, " Keys ");
        press(&mut app, Press::Click, x, y);
        assert!(screen_of(&mut app, 30).contains("Search and renaming"));
        app.handle(action(Action::Cancel));
        app.handle(action(Action::Help));
        assert!(screen_of(&mut app, 30).contains("Search and renaming"));
    }

    #[test]
    fn about_shows_the_tools_once_they_answer_and_asks_again_for_another_program() {
        let mut app = loaded();
        app.handle(action(Action::Help));
        screen_of(&mut app, 40);
        app.handle(action(Action::End));
        let text = screen_of(&mut app, 40);
        assert!(text.contains("External tools"), "{text}");
        app.detected(Tool::Zoxide, Detected::Found("zoxide 0.9.8".to_owned()));
        app.detected(Tool::Ripgrep, Detected::Found("ripgrep 15.2.0".to_owned()));
        app.detected(Tool::Fd, Detected::Found("fd 10.5.0".to_owned()));
        let text = screen_of(&mut app, 40);
        assert!(
            ["zoxide 0.9.8", "ripgrep 15.2.0", "fd 10.5.0"]
                .iter()
                .all(|version| text.contains(version)),
            "the open help shows them: {text}"
        );
        assert_eq!(app.context(), Context::Help);

        let mut new = config();
        new.zoxide.program = PathBuf::from("/opt/zoxide");
        let effects = app.apply_config(config(), new);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Detect(Tool::Zoxide))),
            "{effects:?}"
        );
        let mut new = config();
        new.search.program = PathBuf::from("/opt/rg");
        let effects = app.apply_config(config(), new);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Detect(Tool::Ripgrep))),
            "{effects:?}"
        );
        let mut new = config();
        new.search.fd = PathBuf::from("fdfind");
        let effects = app.apply_config(config(), new);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Detect(Tool::Fd))),
            "{effects:?}"
        );
        let mut new = config();
        new.ui.show_hidden = !new.ui.show_hidden;
        let effects = app.apply_config(config(), new);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Detect(_)))
        );
    }

    #[test]
    fn f1_opens_the_help_under_any_prompt() {
        let mut app = loaded();
        app.handle(action(Action::Help));
        assert_eq!(app.context(), Context::Help);
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
        assert_eq!(app.context(), Context::Help, "back to the help");
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);
        assert!(!app.quits());
    }

    #[test]
    fn quitting_stops_every_connection() {
        let mut app = at_slash();
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
    fn slash_searches_as_vim_does_and_enter_ends_on_the_match() {
        let mut app = loaded();
        assert!(keys(&mut app, &["a"]).is_empty());
        assert_eq!(app.context(), Context::Panel, "typing does nothing");
        assert!(keys(&mut app, &["/", "r"]).is_empty());
        assert_eq!(app.context(), Context::QuickSearch);
        let shown = screen(&mut app);
        assert!(shown.contains(&prompt("Search", "r")));
        assert_eq!(mode_of(&mut app), "INSERT · Quick search");
        // Enter ends the search on what it found, as in vim; l opens it.
        assert!(keys(&mut app, &["enter"]).is_empty());
        assert_eq!(app.context(), Context::Panel);
        let Effect::List { request, .. } = one(keys(&mut app, &["l"])) else {
            panic!("expected a listing");
        };
        assert_eq!(request.location, local("/srv/right"));

        let mut app = loaded();
        app.handle(action(Action::QuickSearch));
        assert_eq!(
            app.context(),
            Context::QuickSearch,
            "/ starts an empty search"
        );
        app.handle(Resolved::Insert('l'));
        app.handle(action(Action::Backspace));
        assert!(screen(&mut app).contains(&prompt("Search", "")));
        assert!(app.handle(action(Action::Cancel)).is_empty());
        assert_eq!(app.context(), Context::Panel, "Esc ends it");
    }

    #[test]
    fn quick_search_edits_its_text_with_the_keys_of_text_fields() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let names = ["bar", "bin", "boot", "dev"];
        answer(&mut app, effects, &Listing::Dir(names.map(dir).to_vec()));
        app.config.ui.default_search_mode = SearchMode::Plain;
        let under = |app: &App| cursor_name(app, Side::Left);
        app.handle(action(Action::QuickSearch));
        for c in "bo".chars() {
            app.handle(Resolved::Insert(c));
        }
        assert_eq!(under(&app), "boot");
        // Home goes to the start of the text, not to the first row, and the search goes on.
        app.handle(action(Action::TextStart));
        assert_eq!(under(&app), "boot");
        assert_eq!(app.context(), Context::QuickSearch);
        app.handle(action(Action::DeleteChar));
        assert!(screen(&mut app).contains(&prompt("Search", "o")));
        app.handle(action(Action::DeleteToEnd));
        assert!(screen(&mut app).contains(&prompt("Search", "")));
        assert_eq!(app.context(), Context::QuickSearch, "still searching");
        app.handle(Resolved::Insert('d'));
        assert_eq!(under(&app), "dev");
        // Up ends the search and moves the cursor, as in mc.
        keys(&mut app, &["up"]);
        assert_eq!(app.context(), Context::Panel);
        assert_eq!(under(&app), "boot");

        // Abort goes back to where the search started, and keeps nothing for n.
        app.handle(action(Action::QuickSearch));
        for c in "ba".chars() {
            app.handle(Resolved::Insert(c));
        }
        assert_eq!(under(&app), "bar");
        app.handle(action(Action::Abort));
        assert_eq!(app.context(), Context::Panel);
        assert_eq!(under(&app), "boot");
        app.handle(action(Action::NextMatch));
        assert_eq!(under(&app), "dev", "the search before: ba was given up");
    }

    #[test]
    fn n_and_shift_n_repeat_the_last_quick_search_and_ctrl_g_brings_it_back() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
        let names = ["bar", "bin", "boot", "dev"];
        answer(&mut app, effects, &Listing::Dir(names.map(dir).to_vec()));
        app.config.ui.default_search_mode = SearchMode::Plain;
        let under = |app: &App| cursor_name(app, Side::Left);
        app.handle(action(Action::NextMatch));
        assert_eq!(under(&app), "", "no search yet: the cursor stays on ..");

        app.handle(action(Action::QuickSearch));
        app.handle(Resolved::Insert('b'));
        assert_eq!(under(&app), "bar");
        app.handle(action(Action::PrevMatch));
        assert_eq!(under(&app), "boot", "back, round to the bottom");
        assert_eq!(app.context(), Context::QuickSearch, "still searching");
        app.handle(action(Action::Cancel));
        for (key, name) in [
            (Action::NextMatch, "bar"),
            (Action::NextMatch, "bin"),
            (Action::PrevMatch, "bar"),
            (Action::PrevMatch, "boot"),
        ] {
            app.handle(action(key));
            assert_eq!(under(&app), name, "{key:?}");
        }
        assert_eq!(app.context(), Context::Panel);

        // With nothing typed, Ctrl+g searches for what the last search found.
        keys(&mut app, &["/", "ctrl-g"]);
        assert_eq!(under(&app), "bar");
        assert!(screen(&mut app).contains(&prompt("Search", "b")));
        // A key that ends a search keeps it too.
        keys(&mut app, &["i", "down"]);
        app.handle(action(Action::Home));
        app.handle(action(Action::NextMatch));
        assert_eq!(under(&app), "bin");
        app.handle(action(Action::NextMatch));
        assert_eq!(under(&app), "bin", "the only match");
    }

    #[test]
    fn focus_left_and_right_go_by_where_the_panels_are_drawn() {
        let mut app = loaded();
        app.handle(action(Action::FocusRight));
        assert_eq!(app.active, Side::Right);
        app.handle(action(Action::FocusLeft));
        assert_eq!(app.active, Side::Left);
        app.handle(action(Action::SwapPanels));
        app.handle(action(Action::FocusLeft));
        assert_eq!(
            app.active,
            Side::Right,
            "the right panel is drawn on the left"
        );
        app.handle(action(Action::FocusRight));
        assert_eq!(app.active, Side::Left);
    }

    #[test]
    fn the_f_key_bar_colors_whole_slots() {
        use ratatui::style::Color;

        let mut app = loaded();
        let mut terminal = Terminal::new(TestBackend::new(80, 6)).unwrap();
        terminal
            .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
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
        for action in app
            .keymap
            .fkeys(Mode::Normal, Context::Panel)
            .into_iter()
            .flatten()
        {
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
    fn f_hash_shows_the_checksum_of_a_file_and_copies_it() {
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
    fn f_hash_compares_with_the_file_in_the_other_panel() {
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
            text.contains("Checksums of 1 file and 1 directory with:"),
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
            text.contains("╠ / /srv ═"),
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
    fn disconnecting_tones_down_hidden_tabs_on_the_host_too() {
        let mut app = at_slash();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        connect_web(&mut app, Side::Left, connection, "/var/www");
        app.handle(action(Action::NewTab));
        app.handle(action(Action::PrevTab));
        app.disconnect("web");
        assert_eq!(app.left.len(), 2);
        for tab in app.left.iter_mut() {
            assert_eq!(tab.panel.location(), &remote("web", "/var/www"), "it stays");
            assert_eq!(tab.panel.not_connected(), Some("web"));
            tab.panel.handle(Action::End);
            assert!(tab.panel.entry_under_cursor().is_some(), "with its entries");
        }
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
        assert!(in_the_middle(&text, " Tabs "), "{text}");
        assert!(
            text.contains("║ /srv ") && text.contains("║ /srv/left "),
            "{text}"
        );
        app.handle(action(Action::Up));
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3], 1));
    }

    #[test]
    fn the_tab_menu_closes_tabs_and_stays_open() {
        let mut app = loaded();
        app.handle(action(Action::NewTab));
        app.handle(action(Action::NewTab));
        assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3, 4], 4));
        app.handle(action(Action::TabList));
        let fkey = |app: &App| app.fkey_actions()[7].as_ref().map(|(action, _)| *action);
        assert_eq!(fkey(&app), Some(Action::CloseTab), "F8");
        // A tab that does not show goes; the one that shows stays.
        app.handle(action(Action::Home));
        assert!(app.handle(action(Action::CloseTab)).is_empty());
        assert_eq!(tab_numbers(&app, Side::Left), (vec![3, 4], 4));
        assert_eq!(app.context(), Context::TabMenu, "the menu stays");
        // The one that shows goes, and the one before it shows.
        app.handle(action(Action::End));
        app.handle(action(Action::CloseTab));
        assert_eq!(tab_numbers(&app, Side::Left), (vec![3], 3));
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("║ /srv ") && !text.contains("║ /srv/"),
            "{text}"
        );
        // The last one stays, and F8 does nothing for it.
        assert!(app.handle(action(Action::CloseTab)).is_empty());
        assert_eq!(tab_numbers(&app, Side::Left), (vec![3], 3));
        assert_eq!(fkey(&app), None);
        assert_eq!(app.active, Side::Left);
    }

    #[test]
    fn the_list_of_the_other_panels_tabs_gives_the_keys_back() {
        let mut app = loaded();
        app.handle(action(Action::NewTab));
        on_other(&mut app, Action::TabList);
        assert_eq!(app.context(), Context::TabMenu);
        let text = screen_of(&mut app, 16);
        assert!(
            in_the_middle(&text, " Tabs "),
            "centered over both panels, as for the active one: {text}"
        );
        assert!(keys(&mut app, &["enter"]).is_empty());
        assert!(app.tab_menu.is_none());
        assert_eq!(
            app.active,
            Side::Left,
            "the keys come back to the active panel"
        );
        assert_eq!(tab_numbers(&app, Side::Right), (vec![2], 2));
        assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3], 3));
    }

    /// Saves the tabs of both panels as `name` with t W, and returns what is saved.
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
    /// which shows; on the right `/`, which has the keys.
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
            right: vec![tab(Place::Local("/".to_owned()))],
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
        assert_eq!(request.location, local("/"));
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
        app.handle(Resolved::Hotkey('w'));
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("Save workspace…")
                && text.contains("1 photos")
                && text.contains("2 noon"),
            "{text}"
        );
        let effects = app.handle(Resolved::Hotkey('2'));
        assert!(!effects.is_empty());
        assert_eq!(app.workspace.as_deref(), Some("noon"));
        app.handle(action(Action::PullDownLast));
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

        // a saves the tabs as a new workspace: the name starts empty.
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
            text.contains("Workspaces") && text.contains("2 │ 1") && text.contains("web:/var/www"),
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
        terminal
            .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
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
        let mut terminal = Terminal::new(TestBackend::new(100, 7)).unwrap();
        terminal
            .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
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
        assert_eq!(
            status.key.as_deref(),
            Some("Ctrl+x t c"),
            "keys act on the active panel, and after Ctrl+x on the other"
        );
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
        terminal
            .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
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
        let text = screen(&mut app);
        assert!(!text.contains("Rename:"), "the status line as ever: {text}");
        retype(&mut app, "taken");
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
        assert!(app.panel(Side::Left).wait().is_some(), "its dots turn");

        // A file has the name: No is the default; Left goes to Yes.
        assert!(app.entry_renamed(panel, &from, &to, Err(None)).is_empty());
        assert!(
            app.panel(Side::Left).wait().is_none(),
            "while the question waits"
        );
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
        retype(&mut app, "a/b");
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
        app.handle(action(Action::TextEnd));
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
        app.config.ui.default_search_mode = SearchMode::Plain;
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
        assert_eq!(
            app.place(),
            (Mode::Insert, Context::Jump),
            "it opens typing keywords"
        );
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
        assert!(text.contains(&prompt("Jump to", "sc")), "{text}");
        assert!(
            text.contains("~/src") && text.contains("/opt/scripts") && !text.contains("/stale"),
            "{text}"
        );
        keys(&mut app, &["esc"]);
        assert_eq!(
            app.place(),
            (Mode::Normal, Context::Jump),
            "Esc goes to the rows"
        );
        keys(&mut app, &["j"]);
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
        // Esc on the rows closes the window; so does a failed query, after saying why.
        app.handle(action(Action::Jump));
        app.jumps(app.jump_queries, Err("zoxide is not installed".to_owned()));
        assert!(screen_of(&mut app, 12).contains("zoxide is not installed"));
        keys(&mut app, &["esc", "esc"]);
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn in_the_fzf_mode_the_zoxide_window_asks_once_and_filters_as_fzf() {
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
            prompt,
            ..
        }) = app.take_run()
        else {
            panic!("expected a local command");
        };
        assert_eq!(
            (dir.as_path(), command.as_str(), prompt.as_str()),
            (Path::new("/srv"), "make\nls", "/srv $ ")
        );
        assert!(app.take_run().is_none());
        let effects = app.ran(Ok(()));
        assert_eq!(effects.len(), 2, "both panels read /srv again");
        app.ran(Err("cannot run /bin/nosh".to_owned()));
        assert!(screen_of(&mut app, 12).contains("cannot run /bin/nosh"));
    }

    #[test]
    fn colon_needs_a_bang_and_a_host_its_connection() {
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
        let mut app = at_slash();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        connect_web(&mut app, Side::Left, connection, "/var/www");
        app.disconnect("web");
        app.handle(action(Action::Shell));
        assert!(
            screen_of(&mut app, 12).contains("Not connected to web"),
            "it asks to connect first"
        );
        app.handle(action(Action::Cancel));
        assert_eq!(app.context(), Context::Panel);
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
            Context::CommandHistory,
            "q : opens the line and the window"
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
    fn ctrl_o_shows_the_output_from_a_panel_and_the_command_line() {
        let mut app = loaded();
        app.handle(action(Action::UserScreen));
        assert!(matches!(app.take_handover(), Some(Handover::UserScreen)));
        app.handle(action(Action::Shell));
        type_text(&mut app, "ls");
        app.handle(action(Action::UserScreen));
        assert!(matches!(app.take_handover(), Some(Handover::UserScreen)));
        assert_eq!(
            app.command_line.as_ref().map(CommandLine::text),
            Some("ls"),
            "the line stays as it was"
        );
    }

    #[test]
    fn ctrl_o_shows_the_output_over_whatever_is_in_front_which_stays() {
        for opens in [
            Action::QuickSearch,
            Action::Mkdir,
            Action::Help,
            Action::PullDown,
            Action::Jobs,
        ] {
            let mut app = loaded();
            app.handle(action(opens));
            let context = app.context();
            assert_ne!(context, Context::Panel, "{opens:?} opens something");
            app.handle(action(Action::UserScreen));
            assert!(
                matches!(app.take_handover(), Some(Handover::UserScreen)),
                "{opens:?}"
            );
            assert_eq!(app.context(), context, "{opens:?} stays in front");
        }
    }

    #[test]
    fn ctrl_l_redraws_under_whatever_is_in_front_which_stays() {
        for opens in [
            Action::QuickSearch,
            Action::Mkdir,
            Action::Help,
            Action::PullDown,
            Action::Jobs,
            Action::Shell,
        ] {
            let mut app = loaded();
            app.handle(action(opens));
            let context = app.context();
            assert_ne!(context, Context::Panel, "{opens:?} opens something");
            app.handle(action(Action::Redraw));
            assert!(app.take_redraw(), "{opens:?}");
            assert_eq!(app.context(), context, "{opens:?} stays in front");
        }
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
        assert!(screen(&mut app).contains(&prompt("Search", "right")));
    }

    #[test]
    fn the_prompt_shows_the_directory_with_a_tilde_for_home() {
        let (mut app, _) = App::new(
            Path::new("/home/me/src/noc"),
            Path::new("/home/me"),
            &config(),
        );
        app.handle(action(Action::Shell));
        let prompt = |app: &App, width| cells::text_of(&app.command_prompt(width));
        assert_eq!(prompt(&app, 80), "~/src/noc $ ");
        assert_eq!(prompt(&app, 18), "~/s»oc $ ", "a third of the width");
        let mark = app
            .command_prompt(18)
            .into_iter()
            .find(|span| span.content == "»");
        assert_eq!(
            mark.map(|span| span.style),
            Some(app.theme.truncated),
            "the mark stands out"
        );
        app.command_line = Some(CommandLine::new(command::Kind::Noc));
        assert_eq!(prompt(&app, 80), ":");
    }

    #[test]
    fn the_terminal_title_shows_the_active_panel_unless_turned_off() {
        let (mut app, _) = App::new(
            Path::new("/home/me/src/noc"),
            Path::new("/home/me"),
            &config(),
        );
        assert_eq!(app.terminal_title().as_deref(), Some("~/src/noc — noc"));
        app.config.ui.terminal_title = false;
        assert_eq!(app.terminal_title(), None);

        let (mut app, effects) = App::new(Path::new("/"), Path::new("/home/me"), &config());
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        assert_eq!(app.terminal_title().as_deref(), Some("/ — noc"));
    }

    fn alice_on_mac() -> Machine {
        Machine {
            host: Some("mac".to_owned()),
            user: Some("alice".to_owned()),
            root: false,
        }
    }

    fn root_on_mac() -> Machine {
        Machine {
            user: Some("root".to_owned()),
            root: true,
            ..alice_on_mac()
        }
    }

    /// The line of the mode on a screen `width` cells wide, its end trimmed.
    fn mode_line(app: &mut App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 9)).unwrap();
        terminal
            .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let y = buffer.area.height - 2;
        let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        line.trim_end().to_owned()
    }

    /// The left of the line of the mode, before what is on its right: the mode and the window
    /// that has the keys, or a message.
    fn mode_of(app: &mut App) -> String {
        let line = mode_line(app, 160);
        line.split("  ").next().unwrap_or_default().to_owned()
    }

    #[test]
    fn the_line_of_the_mode_names_the_kind_of_window_by_its_keys() {
        let cases: &[(&[&str], &str)] = &[
            (&[], "NORMAL"),
            (&["/"], "INSERT · Quick search"),
            (&["?"], "INSERT · Quick search"),
            (&["j", "r", "esc"], "NORMAL · Rename"),
            (&["g", "l"], "INSERT · List"),
            (&["t", "t"], "INSERT · List"),
            (&["t", "t", "esc"], "NORMAL · List"),
            (&["g", "z"], "INSERT · List"),
            (&["t", "w"], "INSERT · List"),
            (&["q", ":"], "INSERT · List"),
            (&["f1"], "NORMAL · Help"),
            (&["f9"], "NORMAL · Menu"),
            (&["f7"], "INSERT · Dialog"),
            (&["j", "f8"], "NORMAL · Dialog"),
            (&["f", "/"], "INSERT · Dialog"),
            (&["f", "j"], "NORMAL · Dialog"),
            (&["j", "g", "f"], "NORMAL · Viewer"),
            (&["ctrl-x", "g", "d"], "(^X) INSERT · Dialog"),
            (&["ctrl-x", "/"], "(^X) INSERT · Quick search"),
            (&["ctrl-x", "v"], "(^X) VISUAL"),
            (&["ctrl-x", "j"], "NORMAL"),
        ];
        for (typed, shown) in cases {
            let mut app = with_files(&["a", "b"]);
            keys(&mut app, typed);
            assert_eq!(mode_of(&mut app), *shown, "after {typed:?}");
        }
        let mut app = loaded();
        open_configuration(&mut app);
        assert_eq!(mode_of(&mut app), "INSERT · Dialog");
    }

    #[test]
    fn the_command_line_takes_the_line_of_the_mode_while_it_has_the_keys() {
        let mut app = with_files(&["a"]);
        keys(&mut app, &["!", "l", "s"]);
        assert!(mode_line(&mut app, 160).ends_with("$ ls"));
        keys(&mut app, &["ctrl-f", "q", ":"]);
        assert_eq!(app.place().1, Context::CommandHistory);
        assert_eq!(mode_of(&mut app), "INSERT · List");
        keys(&mut app, &["esc", "q"]);
        assert_eq!(app.place().1, Context::CommandLine);
        assert!(mode_line(&mut app, 160).ends_with("$ ls"));
    }

    #[test]
    fn the_prompt_says_who_is_where_as_a_shell_does() {
        let (mut app, _) = App::new(
            Path::new("/home/me/src/noc"),
            Path::new("/home/me"),
            &config(),
        );
        app.set_machine(alice_on_mac());
        app.handle(action(Action::Shell));
        let prompt = |app: &App, width| cells::text_of(&app.command_prompt(width));
        assert_eq!(prompt(&app, 120), "alice@mac:~/src/noc $ ");
        assert_eq!(
            prompt(&app, 54),
            "alice@mac:~/sr»noc $ ",
            "the directory cut first"
        );
        assert_eq!(
            prompt(&app, 48),
            "mac:~/src/noc $ ",
            "then the user left out"
        );
        assert_eq!(prompt(&app, 30), "mac $ ", "then the directory");
        assert_eq!(prompt(&app, 6), "m» $ ");
        let who = app.command_prompt(120).remove(0);
        assert_eq!(who.style, Style::new(), "only root stands out");

        app.set_machine(root_on_mac());
        assert_eq!(prompt(&app, 120), "root@mac:~/src/noc # ");
        let who = app.command_prompt(120).remove(0);
        assert_eq!(
            (who.content.as_ref(), who.style),
            ("root@mac", app.theme.root)
        );
        assert_eq!(
            app.terminal_title().as_deref(),
            Some("[root] ~/src/noc — noc")
        );
    }

    #[test]
    fn the_line_of_the_mode_says_who_is_where_while_it_fits() {
        let mut app = loaded();
        assert_eq!(
            mode_line(&mut app, 60),
            "NORMAL",
            "a machine without a name"
        );
        app.set_machine(alice_on_mac());
        let line = mode_line(&mut app, 60);
        assert!(line.starts_with("NORMAL "), "{line}");
        assert!(
            line.ends_with(" alice@mac"),
            "left of what is typed: {line}"
        );
        assert_eq!(line.chars().count(), 60 - SHOWCMD_WIDTH - 1);
        assert_eq!(mode_line(&mut app, 26), "NORMAL", "no room");
        app.set_machine(root_on_mac());
        assert!(mode_line(&mut app, 60).ends_with(" # root@mac"));
    }

    /// Whether a cell of the screen has the background of `style`.
    fn shows_background(app: &mut App, style: Style) -> bool {
        let mut terminal = Terminal::new(TestBackend::new(100, 9)).unwrap();
        terminal
            .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content
            .iter()
            .any(|cell| cell.bg == style.bg.unwrap())
    }

    #[test]
    fn the_cursor_is_on_red_in_the_panels_where_the_user_is_root() {
        let mut app = loaded();
        let (cursor, root) = (app.theme.cursor, app.theme.root_cursor);
        assert_ne!(cursor.bg, root.bg);
        assert!(shows_background(&mut app, cursor));
        assert!(!shows_background(&mut app, root));
        app.set_machine(root_on_mac());
        assert!(shows_background(&mut app, root));

        let (mut app, _) = on_a_host();
        app.resolved("web".to_owned(), "deploy@10.0.0.5".to_owned(), "deploy");
        assert!(!shows_background(&mut app, root), "not root there");
        app.resolved("web".to_owned(), "root@10.0.0.5".to_owned(), "root");
        assert!(shows_background(&mut app, root), "root on the host");
        assert_eq!(app.theme.root_cursor, root, "the theme stays");
    }

    #[test]
    fn on_a_host_the_user_comes_from_ssh_and_a_local_root_still_shows() {
        let (mut app, _) = on_a_host();
        let prompt = |app: &App| cells::text_of(&app.shell_prompt(120));
        assert_eq!(prompt(&app), "web:/home/deploy $ ", "before ssh -G");
        let line = mode_line(&mut app, 100);
        assert!(line.ends_with(" web"), "{line:?}");

        app.resolved("web".to_owned(), "deploy@10.0.0.5".to_owned(), "deploy");
        assert_eq!(prompt(&app), "deploy@web:/home/deploy $ ");
        app.set_machine(root_on_mac());
        assert_eq!(prompt(&app), "deploy@web:/home/deploy $ ");
        assert!(
            mode_line(&mut app, 100).ends_with(" # deploy@web"),
            "root here"
        );
        assert_eq!(
            app.terminal_title().as_deref(),
            Some("[root] web:/home/deploy — noc")
        );

        app.set_machine(alice_on_mac());
        app.resolved("web".to_owned(), "root@10.0.0.5".to_owned(), "root");
        assert_eq!(prompt(&app), "root@web:/home/deploy # ");
        assert!(mode_line(&mut app, 100).ends_with(" root@web"));
        assert!(!mode_line(&mut app, 100).contains('#'));
        assert_eq!(
            app.terminal_title().as_deref(),
            Some("[root] web:/home/deploy — noc")
        );
    }

    #[test]
    fn key_hints_show_every_key_of_the_panel_over_it_when_asked_for() {
        let mut app = loaded();
        let mut keys = KeyState::default();
        app.set_hints(&keys);
        assert!(!screen_of(&mut app, 16).contains("Esc closes"));
        app.handle(action(Action::KeyHints));
        let place = app.take_hints_asked().unwrap();
        assert_eq!(place, (Mode::Normal, Context::Panel).into());
        assert_eq!(app.take_hints_asked(), None, "asked once");
        app.keymap().show_hints(&mut keys, place);
        app.set_hints(&keys);
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("Esc closes") && text.contains("Ctrl+w") && text.contains("+9 keys"),
            "{text}"
        );
        // A dialog in front hides them…
        app.handle(action(Action::Mkdir));
        app.set_hints(&keys);
        assert!(!screen_of(&mut app, 16).contains("Esc closes"));
        // …and shows its own when asked, staying open.
        let dialog = app.place();
        assert_eq!(dialog, (Mode::Insert, Context::PathInput));
        assert!(app.handle(action(Action::KeyHints)).is_empty());
        assert_eq!(app.take_hints_asked(), Some(dialog.into()));
        assert_eq!(app.place(), dialog);
        app.keymap().show_hints(&mut keys, dialog);
        app.set_hints(&keys);
        let text = screen_of(&mut app, 16);
        assert!(
            text.contains("Esc closes") && text.contains("Complete the path"),
            "{text}"
        );
        // So does a menu.
        app.handle(action(Action::Cancel));
        app.handle(action(Action::PullDown));
        assert_eq!(app.context(), Context::PullDown);
        app.handle(action(Action::KeyHints));
        assert_eq!(
            app.take_hints_asked(),
            Some((Mode::Normal, Context::PullDown).into())
        );
        assert_eq!(app.context(), Context::PullDown);
    }

    /// Feeds `keys` to the app as the event loop does, with the key hints, which `state` keeps.
    fn keys_with_hints(app: &mut App, state: &mut KeyState, keys: &[&str]) -> Vec<Effect> {
        let mut effects = Vec::new();
        for key in keys {
            let combination = crokey::parse(key).unwrap().normalized();
            let event = KeyEvent::new(*combination.codes.first(), combination.modifiers);
            effects.extend(app.key(state, event, Instant::now()));
            if let Some(place) = app.take_hints_asked() {
                app.keymap().show_hints(state, place);
            }
            app.set_hints(state);
        }
        effects
    }

    #[test]
    fn ctrl_k_twice_lists_the_keys_to_filter_and_run_where_they_went() {
        let mut app = loaded();
        let mut state = KeyState::default();
        keys_with_hints(&mut app, &mut state, &["2", "ctrl-k"]);
        assert!(app.hints.is_some());
        assert_eq!(app.place(), (Mode::Normal, Context::Panel));
        keys_with_hints(&mut app, &mut state, &["ctrl-k"]);
        assert_eq!(app.place(), (Mode::Insert, Context::KeyHints));
        assert!(app.hints.is_none(), "the list takes the place of the hints");
        let text = screen_of(&mut app, 30);
        assert!(
            text.contains("Ctrl+w h") && text.contains(&prompt("Filter", "")),
            "{text}"
        );
        // What is typed filters, and Enter runs the command in the panel, with the count.
        let typed: Vec<String> = "'row down".chars().map(String::from).collect();
        let typed: Vec<&str> = typed
            .iter()
            .map(|c| if c == " " { "space" } else { c.as_str() })
            .collect();
        keys_with_hints(&mut app, &mut state, &typed);
        keys_with_hints(&mut app, &mut state, &["enter"]);
        assert_eq!(app.place(), (Mode::Normal, Context::Panel));
        assert_eq!(cursor_name(&app, Side::Left), "right", "two rows down");

        // After the first key of a sequence, the list has the sequences after it.
        keys_with_hints(&mut app, &mut state, &["ctrl-w", "ctrl-k"]);
        assert_eq!(
            app.hints.as_ref().and_then(|hints| hints.typed.as_deref()),
            Some("Ctrl+w")
        );
        keys_with_hints(&mut app, &mut state, &["ctrl-k"]);
        assert_eq!(app.context(), Context::KeyHints);
        let text = screen_of(&mut app, 30);
        assert!(text.contains("Ctrl+w h") && !text.contains("g g"), "{text}");
        // Esc goes back to the rows, Esc again closes it, and the hints do not come back.
        keys_with_hints(&mut app, &mut state, &["esc"]);
        assert_eq!(app.place(), (Mode::Normal, Context::KeyHints));
        keys_with_hints(&mut app, &mut state, &["esc"]);
        assert_eq!(app.place(), (Mode::Normal, Context::Panel));
        assert!(app.hints.is_none());
    }

    #[test]
    fn the_list_of_the_keys_after_ctrl_x_runs_its_command_in_the_other_panel() {
        let mut app = loaded();
        let mut state = KeyState::default();
        keys_with_hints(&mut app, &mut state, &["ctrl-x", "ctrl-k", "ctrl-k"]);
        assert_eq!(app.place(), (Mode::Insert, Context::KeyHints));
        assert_eq!(app.active, Side::Right, "the other panel has the keys");
        let typed: Vec<String> = "'row down".chars().map(String::from).collect();
        let typed: Vec<&str> = typed
            .iter()
            .map(|c| if c == " " { "space" } else { c.as_str() })
            .collect();
        keys_with_hints(&mut app, &mut state, &typed);
        keys_with_hints(&mut app, &mut state, &["enter"]);
        assert_eq!(cursor_name(&app, Side::Right), "left");
        assert_eq!(cursor_name(&app, Side::Left), "");
        assert_eq!(app.active, Side::Left);
    }

    #[test]
    fn ctrl_k_twice_lists_the_keys_of_a_dialog_over_it() {
        let mut app = loaded();
        let mut state = KeyState::default();
        app.handle(action(Action::Mkdir));
        let dialog = app.place();
        assert_eq!(dialog, (Mode::Insert, Context::PathInput));
        keys_with_hints(&mut app, &mut state, &["ctrl-k", "ctrl-k"]);
        assert_eq!(app.place(), (Mode::Insert, Context::KeyHints));
        assert!(screen_of(&mut app, 30).contains("Complete the path"));
        keys_with_hints(&mut app, &mut state, &["esc", "esc"]);
        assert_eq!(app.place(), dialog, "the dialog has the keys again");
    }

    #[test]
    fn key_hints_follow_ui_which_key() {
        let mut app = loaded();
        assert_eq!(app.hints_delay(), Some(Duration::from_millis(500)));
        app.config.ui.which_key_delay_ms = 0;
        assert_eq!(app.hints_delay(), Some(Duration::ZERO));
        app.config.ui.which_key = false;
        assert_eq!(app.hints_delay(), None);
    }

    #[test]
    fn ctrl_s_ctrl_s_switches_how_one_quick_search_matches_and_n_matches_as_it_did() {
        let mut app = loaded();
        app.config.ui.default_search_mode = SearchMode::Plain;
        let start = app
            .panel(app.active)
            .name_under_cursor()
            .map(<[u8]>::to_vec);
        app.handle(action(Action::QuickSearch));
        keys(&mut app, &["ctrl-s", "ctrl-s"]);
        type_text(&mut app, "^r");
        assert_eq!(
            app.panel(app.active).name_under_cursor(),
            Some(&b"right"[..]),
            "fzf"
        );
        keys(&mut app, &["ctrl-s", "ctrl-s"]);
        let glyphs = app.theme.glyphs.search_modes;
        assert!(searching(&mut app, "^r", glyphs.regex));
        assert_eq!(
            app.panel(app.active).name_under_cursor(),
            Some(&b"right"[..]),
            "a regular expression"
        );
        keys(&mut app, &["ctrl-s", "ctrl-s"]);
        assert_eq!(
            app.panel(app.active)
                .name_under_cursor()
                .map(<[u8]>::to_vec),
            start,
            "as it is: no name holds it"
        );
        assert!(searching(&mut app, "^r", glyphs.plain), "the text stays");
        keys(
            &mut app,
            &["ctrl-s", "s", "ctrl-s", "s", "ctrl-s", "m", "enter"],
        );
        app.config.ui.default_search_mode = SearchMode::Fzf;
        app.handle(action(Action::QuickSearch));
        assert!(
            searching(&mut app, "", glyphs.regex),
            "the next search starts as the last one was, whatever the default"
        );
        let cases = app.theme.glyphs.case_modes;
        assert!(screen(&mut app).contains(&format!(" {} {} ║", cases.exact, glyphs.regex)));
        keys(&mut app, &["esc"]);
        app.handle(action(Action::Home));
        app.handle(action(Action::NextMatch));
        assert_eq!(
            app.panel(app.active).name_under_cursor(),
            Some(&b"right"[..]),
            "a regular expression, as the last search"
        );
        let last = app.last_search.as_ref().map(|(_, _, input)| input.case());
        assert_eq!(last, Some(search::Case::Match), "and its case");
    }

    #[test]
    fn quick_search_matches_as_fzf_does_or_as_the_search_mode_says() {
        let mut app = loaded();
        app.handle(action(Action::QuickSearch));
        app.handle(Resolved::Insert('r'));
        app.handle(Resolved::Insert('t'));
        assert!(screen(&mut app).contains(&prompt("Search", "rt")));
        let Effect::List { request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        assert_eq!(request.location, local("/srv/right"));

        let mut app = loaded();
        app.config.ui.default_search_mode = SearchMode::Plain;
        app.handle(action(Action::QuickSearch));
        app.handle(Resolved::Insert('r'));
        app.handle(Resolved::Insert('t'));
        assert_eq!(
            app.panel(app.active).search_text(),
            Some("r"),
            "no name holds rt: t is not typed"
        );
        assert_eq!(
            app.panel(app.active).name_under_cursor(),
            Some(&b"right"[..]),
            "the cursor stays"
        );
    }

    /// The column of the first line of `text` that holds `needle`, where it starts.
    fn column_of(text: &str, needle: &str) -> Option<usize> {
        text.lines()
            .find_map(|line| Some(line[..line.find(needle)?].chars().count()))
    }

    /// Whether the first line of `text` that holds `needle` has it in the middle of the screen
    /// of [`screen_at`], as a window centered over both panels has its title.
    fn in_the_middle(text: &str, needle: &str) -> bool {
        let width = needle.chars().count();
        column_of(text, needle).is_some_and(|column| (column + width / 2).abs_diff(80) <= 1)
    }

    #[test]
    fn ctrl_x_g_d_goes_in_the_other_panel_from_where_it_is() {
        let mut app = loaded();
        app.handle(action(Action::GoToDir));
        let text = screen_of(&mut app, 16);
        assert!(in_the_middle(&text, " Go to directory "), "{text}");
        app.handle(action(Action::Cancel));

        assert!(on_other(&mut app, Action::GoToDir).is_empty());
        let text = screen_of(&mut app, 16);
        assert!(
            in_the_middle(&text, " Go to directory "),
            "there too: {text}"
        );
        type_text(&mut app, "right");
        let effects = keys(&mut app, &["enter"]);
        let [Effect::List { panel, request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(
            (panel.side, &request.location),
            (Side::Right, &local("/srv/right"))
        );
        answer(&mut app, effects, &Listing::Dir(vec![dir("deeper")]));
        assert_eq!(
            app.active,
            Side::Left,
            "the keys come back to the active panel"
        );

        // `-` and relative paths read from the other panel's directory.
        on_other(&mut app, Action::GoToDir);
        type_text(&mut app, "deeper");
        let effects = keys(&mut app, &["enter"]);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/right/deeper"));
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        on_other(&mut app, Action::GoToDir);
        type_text(&mut app, "-");
        let effects = keys(&mut app, &["enter"]);
        let [Effect::List { panel, request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(
            (panel.side, &request.location),
            (Side::Right, &local("/srv/right"))
        );
        assert_eq!(app.panel(Side::Left).location(), &local("/srv"));
    }

    #[test]
    fn ctrl_x_g_z_jumps_in_the_other_panel() {
        let mut app = recording();
        app.config.ui.default_search_mode = SearchMode::Plain;
        on_other(&mut app, Action::GoToDir);
        type_text(&mut app, "sub");
        let effects = keys(&mut app, &["enter"]);
        answer(&mut app, effects, &Listing::Dir(Vec::new()));

        let effects = keys(&mut app, &["ctrl-x", "g", "z"]);
        let [
            Effect::ZoxideQuery {
                generation,
                exclude,
                ..
            },
        ] = &effects[..]
        else {
            panic!("expected a query, got {effects:?}");
        };
        assert_eq!(
            exclude.as_deref(),
            Some(Path::new("/srv/sub")),
            "the other panel's directory"
        );
        app.jumps(
            *generation,
            Ok(vec![Scored {
                score: 1.0,
                path: PathBuf::from("/opt/scripts"),
            }]),
        );
        let text = screen_of(&mut app, 12);
        assert!(in_the_middle(&text, " zoxide "), "{text}");
        let effects = keys(&mut app, &["enter"]);
        assert_eq!(zoxide_adds(&effects), [PathBuf::from("/opt/scripts")]);
        let effects = without_zoxide(effects);
        let [Effect::List { panel, request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(
            (panel.side, &request.location),
            (Side::Right, &local("/opt/scripts"))
        );
        assert_eq!(
            app.active,
            Side::Left,
            "the keys come back to the active panel"
        );
    }

    #[test]
    fn ctrl_x_runs_any_command_in_the_other_panel_and_gives_the_keys_back() {
        let mut app = loaded();
        // Those of the cursor too, as if the other panel were active.
        assert!(keys(&mut app, &["ctrl-x", "j"]).is_empty());
        assert_eq!(cursor_name(&app, Side::Right), "left");
        assert_eq!(cursor_name(&app, Side::Left), "");
        assert_eq!(app.active, Side::Left, "at once, as nothing is left open");
        assert_eq!(app.returns, None);

        assert!(keys(&mut app, &["ctrl-x", "t", "n"]).is_empty());
        assert_eq!(tab_numbers(&app, Side::Right), (vec![2, 3], 3));
        assert_eq!(tab_numbers(&app, Side::Left), (vec![1], 1));
        keys(&mut app, &["ctrl-x", "1", "g", "t"]);
        assert_eq!(
            tab_numbers(&app, Side::Right),
            (vec![2, 3], 2),
            "by its number, the count after Ctrl+x"
        );
        keys(&mut app, &["ctrl-x", "g", "shift-t"]);
        assert_eq!(tab_numbers(&app, Side::Right), (vec![2, 3], 3));
        keys(&mut app, &["ctrl-x", "t", "c"]);
        assert_eq!(tab_numbers(&app, Side::Right), (vec![2], 2));

        keys(&mut app, &["ctrl-x", "o", "s"]);
        assert_eq!(app.panel(Side::Right).sort_action(), Action::SortBySize);
        assert_eq!(app.panel(Side::Left).sort_action(), Action::SortByName);
        let effects = keys(&mut app, &["ctrl-x", "shift-r"]);
        let [Effect::List { panel, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(panel.side, Side::Right);
        assert_eq!(app.active, Side::Left);
    }

    #[test]
    fn the_other_panel_has_the_keys_until_the_command_of_ctrl_x_ends() {
        let mut app = loaded();
        keys(&mut app, &["ctrl-x", "v", "j"]);
        assert_eq!(app.active, Side::Right);
        assert_eq!(app.place(), (Mode::Visual, Context::Panel));
        keys(&mut app, &["esc"]);
        assert_eq!(app.active, Side::Left, "once Visual mode ends");

        keys(&mut app, &["ctrl-x", "/", "r", "i"]);
        assert_eq!(app.active, Side::Right);
        keys(&mut app, &["enter"]);
        assert_eq!(app.active, Side::Left, "once quick search ends");
        assert_eq!(cursor_name(&app, Side::Right), "right");

        assert!(keys(&mut app, &["ctrl-x", "g", "d"]).is_empty());
        assert_eq!(app.active, Side::Right);
        type_text(&mut app, "right");
        let effects = keys(&mut app, &["enter"]);
        assert_eq!(app.active, Side::Left, "once its dialog closes");
        answer(&mut app, effects, &Listing::Dir(vec![file("notes.txt", 1)]));
        // Ctrl+w v there shows its directory here.
        let effects = keys(&mut app, &["ctrl-x", "ctrl-w", "v"]);
        let [Effect::List { panel, request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(
            (panel.side, &request.location),
            (Side::Left, &local("/srv/right"))
        );
        let files = vec![file("a", 1), file("b", 1)];
        answer(&mut app, effects, &Listing::Dir(files));

        // A command run in a shell: the keys come back once the terminal does.
        keys(&mut app, &["ctrl-x", "!", "l", "s", "enter"]);
        assert_eq!(app.active, Side::Right, "the terminal is the command's");
        let Some(Run {
            place: Place::Local(dir),
            ..
        }) = app.take_run()
        else {
            panic!("expected a local command");
        };
        assert_eq!(
            dir,
            Path::new("/srv/right"),
            "in the other panel's directory"
        );
        app.ran(Ok(()));
        app.come_back();
        assert_eq!(app.active, Side::Left);

        // Whatever the command does with the keys, they come back where they were.
        keys(&mut app, &["ctrl-x", "ctrl-w", "w"]);
        assert_eq!(app.active, Side::Left);

        // Esc gives Ctrl+x up, and Ctrl+x again: the keys stay here.
        let right = cursor_name(&app, Side::Right);
        keys(&mut app, &["ctrl-x", "esc", "j"]);
        assert_eq!(cursor_name(&app, Side::Left), "a");
        keys(&mut app, &["ctrl-x", "ctrl-x", "j"]);
        assert_eq!(cursor_name(&app, Side::Left), "b");
        assert_eq!(cursor_name(&app, Side::Right), right);
        assert_eq!(app.active, Side::Left);
    }

    /// Types `text` into Go to directory in the active panel and presses Enter.
    fn go_to_dir(app: &mut App, text: &str) -> Vec<Effect> {
        assert!(app.handle(action(Action::GoToDir)).is_empty());
        assert_eq!(app.context(), Context::PathInput);
        type_text(app, text);
        app.handle(action(Action::Confirm))
    }

    #[test]
    fn g_d_goes_where_cd_would_and_back_with_a_dash() {
        let mut app = loaded();
        let text = {
            app.handle(action(Action::GoToDir));
            let text = screen_of(&mut app, 16);
            app.handle(action(Action::Cancel));
            text
        };
        assert!(
            text.contains("Go to directory") && text.contains("Path:"),
            "{text}"
        );

        let effects = go_to_dir(&mut app, "left/../right/./x");
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/right/x"));
        answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
        assert_eq!(app.panel(Side::Left).location(), &local("/srv/right/x"));

        // `..` puts the cursor on where the panel was, as Ctrl+PgUp does.
        let effects = go_to_dir(&mut app, "..");
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("w"), dir("x"), dir("y")]),
        );
        assert_eq!(app.panel(Side::Left).name_under_cursor(), Some(&b"x"[..]));

        // `-` goes back, and back again.
        let effects = go_to_dir(&mut app, "-");
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/right/x"));
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        let effects = go_to_dir(&mut app, "-");
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        assert_eq!(app.panel(Side::Left).location(), &local("/srv/right"));

        // `~`, an empty line, and where the panel is already.
        let [Effect::List { request, .. }] = &go_to_dir(&mut app, "~/src")[..] else {
            panic!("expected a listing");
        };
        assert_eq!(request.location, local("/home/me/src"));
        app.panel_mut(Side::Left).cancel();
        assert!(go_to_dir(&mut app, "").is_empty());
        assert!(go_to_dir(&mut app, ".").is_empty());
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn alt_c_opens_hosts_and_a_path_that_is_not_there_says_so() {
        let mut app = loaded();
        let effects = go_to_dir(&mut app, "web:/var/log");
        let [Effect::Connect { host, .. }] = &effects[..] else {
            panic!("expected a connection, got {effects:?}");
        };
        assert_eq!(host, "web");
        assert_eq!(
            app.panel(Side::Left).pending_request().map(|r| r.location),
            Some(remote("web", "/var/log"))
        );

        let mut app = loaded();
        let effects = go_to_dir(&mut app, "/nowhere");
        fail(&mut app, effects, ListErrorKind::NotFound);
        assert_eq!(app.panel(Side::Left).location(), &local("/srv"), "it stays");
        let text = screen(&mut app);
        assert!(text.contains("Cannot open /nowhere"), "{text}");
    }

    #[test]
    fn a_directory_that_cannot_be_read_asks_what_to_do() {
        let mut app = loaded();
        // OK, the default when it is not allowed, stays; Up would lead where the panel is.
        let effects = go_to_dir(&mut app, "left");
        assert!(fail(&mut app, effects, ListErrorKind::Denied).is_empty());
        let text = screen_of(&mut app, 12);
        assert!(
            text.contains("Cannot open left: permission denied"),
            "{text}"
        );
        assert!(text.contains("Retry") && text.contains("Go to"), "{text}");
        assert!(!text.contains(" Up "), "{text}");
        assert!(app.handle(action(Action::Confirm)).is_empty());
        assert!(app.dialogs.is_empty());
        assert_eq!(app.panel(Side::Left).location(), &local("/srv"));

        // Retry, the default for other errors, reads it again.
        let effects = go_to_dir(&mut app, "left");
        assert!(fail(&mut app, effects, ListErrorKind::Other).is_empty());
        let Effect::List { request, .. } = one(app.handle(action(Action::Confirm))) else {
            panic!("expected the directory again");
        };
        assert_eq!(request.location, local("/srv/left"));

        // A panel has one such dialog at a time: the last failure replaces the one before.
        let panel = app.shown(Side::Left);
        let error = |reason: &str| ListError::other(reason.to_owned());
        app.listed(panel, request.generation, Err(error("first")));
        let Some(request) = app.panel_mut(Side::Left).handle(Action::Reload) else {
            panic!("expected the directory again");
        };
        app.listed(panel, request.generation, Err(error("second")));
        assert_eq!(app.dialogs.len(), 1);
        let text = screen_of(&mut app, 12);
        assert!(text.contains("Cannot open /srv: second"), "{text}");

        // Go to opens Go to directory with the path, to mend it.
        app.handle(action(Action::Cancel));
        let effects = go_to_dir(&mut app, "/nowhere/deep");
        fail(&mut app, effects, ListErrorKind::NotFound);
        assert!(app.handle(Resolved::Hotkey('g')).is_empty());
        assert_eq!(app.context(), Context::PathInput);
        let field = app.dialogs.front().map(|open| open.dialog.text());
        assert_eq!(field, Some("/nowhere/deep"));
        assert_eq!(cd_text(&remote("web", "")), "web:");
        assert_eq!(cd_text(&remote("web", "/var")), "web:/var");
    }

    #[test]
    fn up_climbs_to_the_nearest_directory_that_is_there() {
        let mut app = loaded();
        let effects = go_to_dir(&mut app, "/srv/x/y/z");
        answer(&mut app, effects, &Listing::Dir(Vec::new()));

        // The directory of the panel is gone: Up is the default.
        let effects = app.handle(action(Action::Reload));
        assert!(fail(&mut app, effects, ListErrorKind::NotFound).is_empty());
        let text = screen_of(&mut app, 12);
        assert!(
            text.contains("Cannot open /srv/x/y/z: no such file or directory"),
            "{text}"
        );
        let effects = app.handle(action(Action::Confirm));
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected the parent, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/x/y"));
        // Past every directory that is gone too, without asking, up to one that it cannot read.
        let effects = fail(&mut app, effects, ListErrorKind::NotFound);
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected the next parent, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/x"));
        assert!(fail(&mut app, effects, ListErrorKind::Denied).is_empty());
        let text = screen_of(&mut app, 12);
        assert!(
            text.contains("Cannot open /srv/x: permission denied"),
            "{text}"
        );

        // Up again, by its letter, to /srv, on the directory it came from.
        let effects = app.handle(Resolved::Hotkey('u'));
        answer(&mut app, effects, &Listing::Dir(vec![dir("w"), dir("x")]));
        assert_eq!(app.panel(Side::Left).location(), &local("/srv"));
        assert_eq!(cursor_name(&app, Side::Left), "x");
        assert!(app.dialogs.is_empty());
    }

    #[test]
    fn a_host_that_cannot_open_its_start_dir_names_it_for_up_and_go_to() {
        let mut app = at_slash();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        let fail_at_start_dir = |app: &mut App, effects: Vec<Effect>| {
            let Effect::List { panel, request, .. } = one(effects) else {
                panic!("expected a listing");
            };
            assert_eq!(request.location, remote("web", ""));
            let error = ListError {
                kind: ListErrorKind::NotFound,
                reason: "no such file or directory".to_owned(),
                location: Some(remote("web", "/var/www/gone")),
            };
            assert!(app.listed(panel, request.generation, Err(error)).is_empty());
        };
        fail_at_start_dir(&mut app, effects);
        let text = screen_of(&mut app, 12);
        assert!(
            text.contains("Cannot open web:/var/www/gone: no such file or directory"),
            "{text}"
        );
        assert!(text.contains(" Up "), "{text}");
        let Effect::List { request, .. } = one(app.handle(Resolved::Hotkey('u'))) else {
            panic!("expected the parent");
        };
        assert_eq!(request.location, remote("web", "/var/www"));

        app.panel_mut(Side::Left).cancel();
        let effects = enter_host(&mut app, Side::Left, 1);
        fail_at_start_dir(&mut app, effects);
        assert!(app.handle(Resolved::Hotkey('g')).is_empty());
        let field = app.dialogs.front().map(|open| open.dialog.text());
        assert_eq!(field, Some("web:/var/www/gone"));
    }

    #[test]
    fn a_key_in_a_panel_not_connected_asks_to_connect() {
        let mut app = at_slash();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        connect_web(&mut app, Side::Left, connection, "/var/www");
        app.disconnect("web");
        // Keys that do not need the host work as ever.
        app.handle(action(Action::SwitchPanel));
        app.handle(action(Action::SwitchPanel));
        assert_eq!(app.active, Side::Left);
        assert!(app.dialogs.is_empty());
        // So do those that look through the entries the panel kept.
        assert!(app.handle(action(Action::Down)).is_empty());
        assert!(app.handle(action(Action::Mark)).is_empty());
        assert!(app.handle(action(Action::SortBySize)).is_empty());
        assert!(app.handle(action(Action::Home)).is_empty());
        assert!(app.dialogs.is_empty());
        let panel = app.panel(Side::Left);
        let chosen: Vec<&[u8]> = panel.chosen().iter().map(|e| e.name.as_slice()).collect();
        assert_eq!(chosen, [b"app".as_slice()], "marked");

        // Any other asks; Connect, the default, reads the directory again.
        assert!(app.handle(action(Action::Enter)).is_empty());
        let text = screen_of(&mut app, 12);
        assert!(text.contains("Not connected to web"), "{text}");
        let Effect::Connect { .. } = one(app.handle(action(Action::Confirm))) else {
            panic!("expected a connection");
        };
        assert_eq!(
            app.panel(Side::Left).pending_request().map(|r| r.location),
            Some(remote("web", "/var/www"))
        );
        app.handle(action(Action::Cancel));
        assert_eq!(app.panel(Side::Left).not_connected(), Some("web"));

        // Local goes to the last local directory of the panel; Cancel stays.
        app.handle(action(Action::Enter));
        assert!(app.handle(action(Action::Cancel)).is_empty());
        assert_eq!(app.panel(Side::Left).not_connected(), Some("web"));
        app.handle(action(Action::Enter));
        let Effect::List { request, .. } = one(app.handle(Resolved::Hotkey('l'))) else {
            panic!("expected a local directory");
        };
        assert_eq!(request.location, local("/"));

        // R connects at once.
        app.panel_mut(Side::Left).cancel();
        let Effect::Connect { .. } = one(app.handle(action(Action::Reload))) else {
            panic!("expected a connection");
        };
        assert!(app.dialogs.is_empty());
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
    fn ctrl_c_in_the_list_of_completions_closes_the_dialog_as_everywhere() {
        let mut app = loaded();
        app.handle(action(Action::GoToDir));
        type_text(&mut app, "left/");
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.names(generation, Ok(names_of(&[("a", true), ("b", true)])), &[]);
        assert_eq!(app.context(), Context::Completion);
        keys(&mut app, &["ctrl-c"]);
        assert!(app.dialogs.is_empty());
    }

    #[test]
    fn tab_completes_go_to_dir_from_the_directory_typed() {
        let mut app = loaded();
        app.handle(action(Action::GoToDir));
        type_text(&mut app, "left/pr");
        let (generation, dir, through, hosts) =
            names_request(&app.handle(action(Action::Complete)));
        assert_eq!((dir, through, hosts), (local("/srv/left"), false, false));
        let entries = [
            ("project-b", true),
            ("project-a", true),
            ("press.txt", false),
        ];
        app.names(generation, Ok(names_of(&entries)), &[]);
        assert_eq!(
            field(&app),
            "left/project-a/|",
            "the first in the list's order, whole; no files"
        );
        assert_eq!(app.place(), (Mode::Insert, Context::Completion));
        let text = screen_of(&mut app, 20);
        assert!(
            text.contains("project-a/") && text.contains("project-b/"),
            "{text}"
        );

        // Moves put each match in at once, round through what was typed, as in vim.
        let mut after = |keys_typed: &[&str]| {
            keys(&mut app, keys_typed);
            field(&app)
        };
        assert_eq!(after(&["tab"]), "left/project-b/|");
        assert_eq!(
            after(&["tab"]),
            "left/pr|",
            "after the last, what was typed"
        );
        assert_eq!(after(&["ctrl-n"]), "left/project-a/|");
        assert_eq!(after(&["backtab"]), "left/pr|", "before the first, too");
        assert_eq!(after(&["backtab"]), "left/project-b/|");
        assert_eq!(after(&["ctrl-p"]), "left/project-a/|");
        assert_eq!(after(&["up"]), "left/pr|");
        assert_eq!(after(&["down"]), "left/project-a/|");
        assert_eq!(after(&["pagedown"]), "left/project-b/|");
        assert_eq!(after(&["pagedown"]), "left/pr|");
        assert_eq!(after(&["pageup"]), "left/project-b/|");
        // Ctrl+y keeps the match.
        assert_eq!(after(&["ctrl-y"]), "left/project-b/|");
        assert_eq!(app.place(), (Mode::Insert, Context::PathInput));

        // Ctrl+e puts back what was typed.
        let two = names_of(&[("x", true), ("y", true)]);
        let (generation, dir, ..) = names_request(&keys(&mut app, &["tab"]));
        assert_eq!(dir, local("/srv/left/project-b"));
        app.names(generation, Ok(two.clone()), &[]);
        assert_eq!(field(&app), "left/project-b/x/|");
        keys(&mut app, &["down", "ctrl-e"]);
        assert_eq!(field(&app), "left/project-b/|");
        assert_eq!(app.place(), (Mode::Insert, Context::PathInput));

        // Right completes in the directory chosen, and Left in the one above, as vim's
        // pop-up menu of the command line.
        let (generation, ..) = names_request(&keys(&mut app, &["tab"]));
        app.names(generation, Ok(two.clone()), &[]);
        let (generation, dir, ..) = names_request(&keys(&mut app, &["right"]));
        assert_eq!(field(&app), "left/project-b/x/|");
        assert_eq!(dir, local("/srv/left/project-b/x"));
        app.names(generation, Ok(two.clone()), &[]);
        assert_eq!(field(&app), "left/project-b/x/x/|");
        let (generation, dir, ..) = names_request(&keys(&mut app, &["left"]));
        assert_eq!(
            (field(&app), dir),
            ("left/project-b/|".to_owned(), local("/srv/left/project-b"))
        );
        app.names(generation, Ok(two.clone()), &[]);
        assert_eq!(field(&app), "left/project-b/x/|");
        keys(&mut app, &["down", "down", "ctrl-e"]);
        assert_eq!(field(&app), "left/project-b/|");

        // Other keys close the list and go to the field, with the match in it: Esc to Normal
        // mode, as in vim; typing edits the field.
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.names(generation, Ok(two.clone()), &[]);
        keys(&mut app, &["esc"]);
        assert_eq!(app.place(), (Mode::Normal, Context::PathInput));
        assert_eq!(app.dialogs[0].dialog.text(), "left/project-b/x/");
        app.dialogs[0].dialog.set_text(0, "left/");
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.names(generation, Ok(two.clone()), &[]);
        app.handle(Resolved::Insert('z'));
        assert_eq!(app.context(), Context::PathInput);
        assert_eq!(field(&app), "left/x/z|");

        // A reply to text that changed since is dropped.
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.handle(action(Action::Backspace));
        app.names(generation, Ok(names_of(&[("xyz", true)])), &[]);
        assert_eq!(field(&app), "left/x/|");
        // So is one that nothing matches.
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.names(generation, Ok(names_of(&[(".git", true)])), &[]);
        assert_eq!(field(&app), "left/x/|");
        assert_eq!(app.context(), Context::PathInput);
    }

    #[test]
    fn enter_in_the_list_of_completions_confirms_with_the_match() {
        let mut app = loaded();
        app.handle(action(Action::GoToDir));
        type_text(&mut app, "ri");
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        app.names(
            generation,
            Ok(names_of(&[("right", true), ("rig", true)])),
            &[],
        );
        let effects = keys(&mut app, &["tab", "enter"]);
        assert!(app.dialogs.is_empty());
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        assert_eq!(request.location, local("/srv/right"));
    }

    #[test]
    fn a_click_on_the_list_of_completions_puts_a_match_in_and_a_double_click_keeps_it() {
        let mut app = loaded();
        app.handle(action(Action::GoToDir));
        let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
        let entries = names_of(&[("alpha", true), ("beta", true), ("gamma", true)]);
        app.names(generation, Ok(entries), &[]);
        assert_eq!(field(&app), "alpha/|");
        let (x, y) = find(&mut app, "gamma/");
        press(&mut app, Press::Click, x, y);
        assert_eq!(field(&app), "gamma/|");
        assert_eq!(app.context(), Context::Completion, "the list stays");
        let (x, y) = find(&mut app, "beta/");
        press(&mut app, Press::Click, x, y);
        press(&mut app, Press::DoubleClick, x, y);
        assert_eq!(field(&app), "beta/|");
        assert_eq!(
            app.context(),
            Context::PathInput,
            "a double click closes it"
        );
    }

    #[test]
    fn go_to_dir_completes_hosts_and_home() {
        let mut app = loaded();
        app.handle(action(Action::GoToDir));
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

    /// The text of the command line, with `|` at the cursor.
    fn line_text(app: &App) -> String {
        let line = app.command_line.as_ref().expect("the command line");
        let before = line.before_cursor();
        format!("{before}|{}", &line.text()[before.len()..])
    }

    /// The listing of commands that a Tab asks for: its generation, directories, and prefix.
    fn commands_request(effects: &[Effect]) -> (u64, Vec<PathBuf>, String) {
        match effects {
            [
                Effect::ListCommands {
                    generation,
                    dirs,
                    prefix,
                    with_dirs: false,
                },
            ] => (*generation, dirs.clone(), prefix.clone()),
            other => panic!("expected a listing of commands, got {other:?}"),
        }
    }

    #[test]
    fn tab_on_the_command_line_completes_a_command_then_files_escaped_for_the_shell() {
        let mut app = loaded();
        let path = vec![PathBuf::from("/opt/bin"), PathBuf::from("/usr/bin")];
        app.set_command_dirs(path.clone());
        app.handle(action(Action::Shell));
        type_text(&mut app, "my");
        let (generation, dirs, prefix) = commands_request(&keys(&mut app, &["tab"]));
        assert_eq!((dirs, prefix.as_str()), (path, "my"));
        let commands = names_of(&[("mytool", false), ("my tool", false)]);
        app.names(generation, Ok(commands), &[]);
        assert_eq!(line_text(&app), "my\\ tool|", "the first, escaped");
        assert_eq!(app.place(), (Mode::Insert, Context::CommandCompletion));
        // The list goes above the line, where the name starts, after the prompt `/srv $ `.
        assert_eq!(find(&mut app, "mytool"), (7, 8));
        assert_eq!(find(&mut app, "my tool"), (7, 7));
        keys(&mut app, &["tab", "ctrl-y"]);
        assert_eq!(line_text(&app), "mytool|");
        assert_eq!(app.context(), Context::CommandLine);

        // Later words are files and directories, from the panel's directory.
        type_text(&mut app, " sr");
        let (generation, dir, through, hosts) = names_request(&keys(&mut app, &["tab"]));
        assert_eq!((dir, through, hosts), (local("/srv"), false, false));
        let entries = [("src dir", true), ("srv.txt", false), ("other", false)];
        app.names(generation, Ok(names_of(&entries)), &[]);
        assert_eq!(line_text(&app), "mytool src\\ dir/|");
        keys(&mut app, &["tab"]);
        assert_eq!(line_text(&app), "mytool srv.txt|");
        keys(&mut app, &["ctrl-e"]);
        assert_eq!(line_text(&app), "mytool sr|");

        // Escapes are taken out to match, and the word's directory is where it looks.
        type_text(&mut app, "c\\ d");
        let (generation, ..) = names_request(&keys(&mut app, &["tab"]));
        app.names(generation, Ok(names_of(&[("src dir", true)])), &[]);
        assert_eq!(line_text(&app), "mytool src\\ dir/|");
        assert_eq!(app.context(), Context::CommandLine, "one match, no list");
        let (generation, dir, ..) = names_request(&keys(&mut app, &["tab"]));
        assert_eq!(dir, local("/srv/src dir"));
        app.names(generation, Ok(names_of(&[("a$b", false)])), &[]);
        assert_eq!(line_text(&app), "mytool src\\ dir/a\\$b|");

        // `~` alone becomes `~/`, the home directory, as absolute paths are absolute.
        type_text(&mut app, " ~");
        assert!(keys(&mut app, &["tab"]).is_empty());
        assert_eq!(line_text(&app), "mytool src\\ dir/a\\$b ~/|");
        let (_, dir, ..) = names_request(&keys(&mut app, &["tab"]));
        assert_eq!(dir, local("/home/me"));
        type_text(&mut app, " /usr/b");
        let (_, dir, ..) = names_request(&keys(&mut app, &["tab"]));
        assert_eq!(dir, local("/usr"));
        app.handle(action(Action::Confirm));
        let run = app.take_run().expect("a command");
        assert_eq!(run.command, "mytool src\\ dir/a\\$b ~/ /usr/b");

        // A first word with a slash names a program, or a directory, as vim's `:!` has it.
        app.handle(action(Action::Shell));
        type_text(&mut app, "./scr");
        let effects = keys(&mut app, &["tab"]);
        let [
            Effect::ListCommands {
                dirs, with_dirs, ..
            },
        ] = &effects[..]
        else {
            panic!("expected a listing of programs, got {effects:?}");
        };
        assert_eq!(
            (&dirs[..], *with_dirs),
            (&[PathBuf::from("/srv")][..], true)
        );
        // Ctrl+c closes the line, from the list too.
        let (generation, ..) =
            names_request(&keys(&mut app, &["ctrl-u", "c", "a", "t", " ", "tab"]));
        app.names(generation, Ok(names_of(&[("a", false), ("b", false)])), &[]);
        assert_eq!(app.context(), Context::CommandCompletion);
        keys(&mut app, &["ctrl-c"]);
        assert!(app.command_line.is_none());
    }

    #[test]
    fn the_list_on_the_command_line_passes_other_keys_to_the_line() {
        let mut app = loaded();
        let two = names_of(&[("left", true), ("right", true)]);
        // Enter runs the command with the match in it, as vim's.
        app.handle(action(Action::Shell));
        type_text(&mut app, "cat ");
        let (generation, ..) = names_request(&keys(&mut app, &["ctrl-i"]));
        app.names(generation, Ok(two.clone()), &[]);
        assert_eq!(line_text(&app), "cat left/|");
        keys(&mut app, &["tab", "enter"]);
        assert_eq!(app.take_run().expect("a command").command, "cat right/");
        // Esc closes the line, as vim's.
        app.handle(action(Action::Shell));
        type_text(&mut app, "cat ");
        let (generation, ..) = names_request(&keys(&mut app, &["tab"]));
        app.names(generation, Ok(two), &[]);
        keys(&mut app, &["esc"]);
        assert!(app.command_line.is_none());
        // In Normal mode, Tab does nothing.
        app.handle(action(Action::Shell));
        type_text(&mut app, "cat ");
        keys(&mut app, &["ctrl-f"]);
        assert!(keys(&mut app, &["tab"]).is_empty());
        assert!(app.completion.is_none());
    }

    #[test]
    fn colon_completes_after_its_bang_and_a_host_completes_files_there() {
        let mut app = loaded();
        app.handle(action(Action::Command));
        type_text(&mut app, "wq");
        assert!(keys(&mut app, &["tab"]).is_empty(), "no shell command");
        app.command_line.as_mut().unwrap().set_text("");
        type_text(&mut app, "!gi");
        let (generation, _, prefix) = commands_request(&keys(&mut app, &["tab"]));
        assert_eq!(prefix, "gi");
        app.names(generation, Ok(names_of(&[("git", false)])), &[]);
        assert_eq!(line_text(&app), "!git|");

        // On a host, every word completes as files there, through its session.
        let (mut app, _) = on_a_host();
        app.handle(action(Action::Shell));
        type_text(&mut app, "no");
        let (generation, dir, through, hosts) = names_request(&keys(&mut app, &["tab"]));
        assert_eq!(
            (dir, through, hosts),
            (remote("web", "/home/deploy"), true, false)
        );
        app.names(generation, Ok(names_of(&[("notes", false)])), &[]);
        assert_eq!(line_text(&app), "notes|");
        type_text(&mut app, " ~/s");
        let (_, dir, ..) = names_request(&keys(&mut app, &["tab"]));
        assert_eq!(dir, remote("web", ""));
    }

    #[test]
    fn a_click_on_the_list_over_the_command_line_puts_a_match_in() {
        let mut app = loaded();
        app.handle(action(Action::Shell));
        type_text(&mut app, "cat ");
        let (generation, ..) = names_request(&keys(&mut app, &["tab"]));
        let entries = names_of(&[("alpha", false), ("beta", false), ("gamma", false)]);
        app.names(generation, Ok(entries), &[]);
        let (x, y) = find(&mut app, "beta");
        press(&mut app, Press::Click, x, y);
        assert_eq!(line_text(&app), "cat beta|");
        assert_eq!(app.context(), Context::CommandCompletion);
        let (x, y) = find(&mut app, "gamma");
        // Three lines up: alpha, what was typed, and round to gamma.
        press(&mut app, Press::WheelUp, x, y);
        assert_eq!(line_text(&app), "cat gamma|", "the wheel moves as Up");
        press(&mut app, Press::Click, 0, 0);
        assert_eq!(
            app.context(),
            Context::CommandLine,
            "a press elsewhere closes it"
        );
        assert_eq!(line_text(&app), "cat gamma|");
    }
}
