//! State and drawing of the whole screen.

use std::collections::{HashMap, HashSet, VecDeque};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use jiff::tz::TimeZone;
use noc_config::{
    Config, HistoryEntry, HostConfig, Hosts, LogConfig, Recent, UiConfig, Workspace, Workspaces,
};
use noc_ops::{Algorithm, Conflict, CopyOptions, Resolution, Sum};
use noc_vfs::{DirEntry, Location, RemotePath, Volume};
use noc_viewer::{Command as ViewerCommand, Styles as ViewerStyles, Viewer, Want};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::cells::{self};
use super::command::{CommandLine, Run};
use super::complete::{Choices, Offer, Quote};
use super::configuration::Configuration;
use super::decor::Decor;
use super::dialog::{Button, Dialog, Reply};
use super::edit::Registers;
use super::find::{FindEvent, FindQuery, FindWindow};
use super::glyphs::{self, Glyphs};
use super::help::{About, Help, Spot};
use super::history::HistoryWindow;
use super::jobs::JobsList;
use super::jump::JumpMenu;
use super::key_hints::KeyHintsWindow;
use super::keymap::{
    self, Action, Context, Extra, Hints, KeyState, Keyboard, Keymap, Mode, Resolved,
};
use super::machine::Machine;
use super::menu::LocationMenu;
use super::panel::{Failure, ListRequest, Panel, location_text};
use super::progress::{JobView, Look, RowLook};
use super::pulldown::{Command, PullDown};
use super::reading::Reading;
use super::search::{self, Input};
use super::spinner::{Spin, Wait};
use super::sums::SumsWindow;
use super::tab_menu::TabMenu;
use super::tabs::{PanelId, Tab, Tabs};
use super::tasks::{HostHandle, HostLink, Relink};
use super::theme::{ColorDepth, Theme};
use super::transfer::{self, Sources};
use super::window::Tone;
use super::workspaces::WorkspacesWindow;
use super::zoom::Stage;
use crate::about::{Build, Detected, Tool};
use crate::i18n::fl;

mod checksums;
mod command_line;
mod completion;
mod copying;
mod dialogs;
mod editing;
mod files;
mod finding;
mod hosts;
mod input;
mod jobs;
mod keys;
mod menus;
mod pointer;
mod render;
mod searching;
mod tab;
mod viewer;
mod workspace;

#[cfg(test)]
mod tests;

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
        host: Option<HostLink>,
        cancel: CancellationToken,
    },
    /// Run the copy job `id` of `sources`, all local or all on one host, to `target`, or the
    /// move job if the options remove sources, and report to [`App::job_event`]; `hosts` lead to the hosts of the sources and of the target,
    /// if they are remote, and `cancel` stops it.
    Copy {
        id: u64,
        sources: Vec<Location>,
        target: Location,
        hosts: (Option<HostLink>, Option<HostLink>),
        options: CopyOptions,
        cancel: CancellationToken,
    },
    /// Run the checksum job `id` on `targets`, groups of locations that are all local or all on
    /// the host of their handle, and report to [`App::job_event`]; `cancel` stops it.
    Checksum {
        id: u64,
        targets: Vec<(Vec<Location>, Option<HostLink>)>,
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
    /// Log as the settings say from now on: with their filter, and keeping as many files.
    Log(LogConfig),
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
    /// `declined` holds the prompts declined since it was asked to stop, unanswered while it
    /// lasts: ssh would take the refusal for an empty answer and send it to the server.
    Connecting {
        connection: u64,
        stop: CancellationToken,
        declined: Vec<Reply>,
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

impl Open {
    /// Whether it is a question of the job `id`, or the new name for one.
    fn asks_for(&self, id: u64) -> bool {
        match &self.purpose {
            Purpose::Failure { job, .. }
            | Purpose::Conflict {
                answer: Answer::Job { job, .. },
                ..
            } => *job == id,
            Purpose::ConflictRename { question } => question.asks_for(id),
            _ => false,
        }
    }
}

#[derive(Debug)]
enum Purpose {
    /// A prompt from ssh connecting to `host`, or a notice, which has no `reply`. Declining a
    /// prompt that `stops` stops connecting: ssh would ask again; a notice always stops.
    Ssh {
        id: u64,
        host: String,
        stops: bool,
        reply: Option<Reply>,
    },
    /// `g +` (`mark`) or `g -` in `panel`.
    Pattern { panel: PanelId, mark: bool },
    /// F7 in `panel`.
    Mkdir { panel: PanelId },
    /// F8 in a panel on `dir`, for the entries `names`.
    Delete { dir: Location, names: Vec<Vec<u8>> },
    /// F5, F6, or Shift+F5 (by `kind`) in `panel`, which shows `dir`, for the entries `names`.
    /// The field opened with `offered`, the text for the other panel's location, if it shows
    /// one.
    Transfer {
        kind: transfer::Kind,
        panel: PanelId,
        dir: Location,
        names: Vec<Vec<u8>>,
        sources: Sources,
        offered: Option<(String, Location)>,
    },
    /// A failure in the job `job`, which waits for `reply`.
    Failure {
        job: u64,
        reply: oneshot::Sender<Resolution>,
    },
    /// A taken name (ADR 0060): a new file was to go to `target`, where a file is, and `free`
    /// is a name free beside it, which Rename… offers; `answer` says who waits for the answer.
    Conflict {
        answer: Answer,
        target: Location,
        free: Vec<u8>,
    },
    /// The new name for Rename… in `question`, a [`Purpose::Conflict`], which comes back in
    /// front if it is cancelled.
    ConflictRename { question: Box<Open> },
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
    /// The dialog that saves a file (ADR 0059): OK writes `bytes` where the name typed points
    /// from `dir`.
    SaveFile { dir: Location, bytes: Vec<u8> },
    /// Go to directory in `panel`: OK opens the path typed there.
    GoToDir { panel: PanelId },
    /// A question about workspaces.
    Workspace(WorkspaceQuestion),
    /// Connecting to `host` failed for `reason`, or its connection was lost: Retry tries
    /// again for the panels that wait for it.
    Reconnect { host: String, reason: String },
    /// `panel` could not read a directory: Retry reads it again, Up goes to the nearest parent
    /// that is there, Go to opens Go to directory with its path; Cancel stays.
    ListFailed { panel: PanelId, failure: Failure },
    /// A key in `panel`, which is on a host that is not connected: Connect reads its directory
    /// again, Local goes to the last local directory of the panel.
    NotConnected { panel: PanelId },
    /// F10: Yes quits, and stops the jobs that run.
    Quit,
    /// An error, with a way on from it besides Cancel, which `way` says.
    Problem { way: Way },
    /// Something to read, such as an error.
    Info,
    /// An error about the dialog after it, which shows under it and has the keys again once
    /// it closes.
    InfoOver,
}

/// Who waits for the answer to the question about a taken name, and what the new file is.
#[derive(Debug)]
enum Answer {
    /// The copy job `job`, whose `source` was to go there, waits for `reply`.
    Job {
        job: u64,
        source: Location,
        reply: oneshot::Sender<Conflict>,
    },
    /// `panel` renamed `from` in its row: Overwrite renames it, and the file there goes;
    /// Rename… renames it to another name in its directory.
    Rename { panel: PanelId, from: Location },
    /// The dialog that saves a file: Overwrite writes `bytes` over the file there; Rename…
    /// writes them under another name.
    Save { bytes: Vec<u8> },
}

/// What an error dialog offers besides Cancel, its first button and the default (ADR 0063).
#[derive(Debug)]
enum Way {
    /// Retry does it again; where a host it needs is not connected, Connect connects, and
    /// does it once the host is.
    Again(Again),
    /// Change… opens the dialog it came from again, with what was typed there.
    Change(Change),
}

/// What Retry does again after an error, or Connect once the hosts it needs are connected.
#[derive(Debug)]
enum Again {
    /// F8 deletes `names` in `dir`.
    Delete {
        dir: Location,
        names: Vec<Vec<u8>>,
    },
    /// F5 or F6, by `kind`, takes `names` in `dir` to `target`; `typed` is the dialog again.
    Transfer {
        kind: transfer::Kind,
        dir: Location,
        names: Vec<Vec<u8>>,
        target: Location,
        typed: Change,
    },
    /// `f #` hashes the `groups` of files with `algorithm`, against `expected`, comparing the
    /// two groups if `compare`; their checksums may be saved in `dir`.
    Checksum {
        dir: Location,
        groups: Vec<Vec<Location>>,
        algorithm: Algorithm,
        expected: Option<String>,
        compare: bool,
    },
    /// F4 edits the file `name` in `dir`, at `line` if there is one.
    EditFile {
        dir: Location,
        name: Vec<u8>,
        line: Option<u64>,
    },
    /// `!` or `:` runs `command` in `dir`, after `prompt`.
    RunIn {
        dir: Location,
        command: String,
        prompt: String,
    },
    /// F7 makes the directory at `location` for `panel`.
    CreateDir {
        panel: PanelId,
        location: Location,
    },
    /// Shift+F6 renames `from` to `to` for `panel`.
    Rename {
        panel: PanelId,
        from: Location,
        to: Location,
    },
    /// The dialog that saves a file writes `bytes` to `location`.
    Write {
        location: Location,
        bytes: Vec<u8>,
    },
    /// F3 views the file at `location`.
    View(Location),
    /// Ctrl+\ views the log of this run.
    ViewLog,
    /// The viewer reads the part it stopped at.
    ReadMore,
    /// F4 runs the editor again; Cancel ends the edit as if the file did not change.
    Edit(Editing),
    /// The edited `copy` of `remote` goes back; `dir` is read again then.
    SendBack {
        copy: PathBuf,
        remote: Location,
        dir: Location,
    },
    /// `!` or `:` runs the command again.
    Run(Run),
    /// The configuration is written again.
    SaveConfig {
        old: Box<Config>,
        new: Box<Config>,
    },
    /// The settings of the host `name` are written again.
    SaveHost {
        name: String,
        host: Option<HostConfig>,
    },
    Workspaces(WorkspaceChange),
    History(HistoryChange),
}

impl Again {
    /// The hosts it works on, which must be connected for it to work.
    fn hosts(&self) -> Vec<&str> {
        let locations: Vec<&Location> = match self {
            Self::Delete { dir, .. } | Self::EditFile { dir, .. } | Self::RunIn { dir, .. } => {
                vec![dir]
            }
            Self::Transfer { dir, target, .. } => vec![dir, target],
            Self::Checksum { groups, .. } => {
                groups.iter().filter_map(|group| group.first()).collect()
            }
            Self::CreateDir { location, .. }
            | Self::Write { location, .. }
            | Self::View(location) => vec![location],
            Self::Rename { from, .. } => vec![from],
            Self::Edit(editing) => editing.remote.iter().collect(),
            Self::SendBack { remote, .. } => vec![remote],
            Self::ViewLog
            | Self::ReadMore
            | Self::Run(_)
            | Self::SaveConfig { .. }
            | Self::SaveHost { .. }
            | Self::Workspaces(_)
            | Self::History(_) => Vec::new(),
        };
        let mut hosts = Vec::new();
        for location in locations {
            if let Location::Remote { host, .. } = location
                && !hosts.contains(&host.as_str())
            {
                hosts.push(host.as_str());
            }
        }
        hosts
    }
}

/// The dialog that Change… opens again, with the text typed in it.
#[derive(Debug)]
enum Change {
    /// Shift+F6 on the entry `name` in `panel`, the field holding `text`.
    Rename {
        panel: PanelId,
        name: Vec<u8>,
        text: String,
    },
    /// F5, F6, or Shift+F5, by `kind`, in `panel` on `dir`, for `names`, which the prompt names
    /// as `sources`.
    Transfer {
        kind: transfer::Kind,
        panel: PanelId,
        dir: Location,
        names: Vec<Vec<u8>>,
        sources: Sources,
        text: String,
    },
    /// F7 in `panel`.
    Mkdir { panel: PanelId, text: String },
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

/// What the path field of a dialog completes, from [`App::completion_of`].
struct FieldCompletion {
    /// The directory a relative path starts from.
    here: Location,
    /// Where `:` without a host starts a local path, in Go to directory only.
    last_local: Option<PathBuf>,
    offer: Offer,
    hosts: FieldHosts,
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
    /// The edited `copy` went back to `remote`, whose directory `dir` panels read again: remove
    /// it.
    Discard {
        copy: PathBuf,
        remote: Location,
        dir: Location,
    },
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
    background: bool,
    /// What starts the job when its turn comes, while it waits.
    queued: Option<Effect>,
    then: Option<Then>,
    hashing: Option<Hashing>,
    /// Directories it changes, which panels read again when it ends.
    changes: Vec<Location>,
    /// Hosts it works on. It waits while one is lost, and goes on once the host connects
    /// again; it ends when the user gives the host up (ADR 0064).
    hosts: Vec<String>,
    /// The answer to the failure it asked about while a host it works on was away, which
    /// Retry gives once the host connects again.
    held: Option<oneshot::Sender<Resolution>>,
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
            background: false,
            queued: None,
            then: None,
            hashing: None,
            changes,
            hosts,
            held: None,
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

/// What the viewer shows.
#[derive(Debug)]
enum Source {
    /// A file, which a task reads in parts.
    File {
        location: Location,
        host: Option<HostHandle>,
    },
    /// Text that the app holds, under a name of its own, such as the lines of the checksums of
    /// a window of them, `results`, which goes back in front when the viewer closes (ADR
    /// 0058), or a file that a save would write.
    Text {
        name: String,
        bytes: Vec<u8>,
        results: Option<Box<Results>>,
    },
}

/// The viewer on screen, the part it reads, and what stops it loading.
#[derive(Debug)]
struct Viewing {
    id: u64,
    viewer: Viewer,
    /// For the file, whose dots turn on its icon until it is read.
    wait: Wait,
    source: Source,
    /// The part being read, if any, with its window.
    reading: Option<Reading>,
    /// The window of the part read last, full for a moment.
    finished: Option<Reading>,
    cancel: CancellationToken,
    /// The question about a taken name whose file it views, which waits for it to close; the
    /// viewer is in front of every window but the dialogs until then.
    question: Option<Box<Open>>,
    /// It views the log of this run, in front of every window and of the dialogs open when it
    /// opened, by their ids, which stay as they were under it; a dialog that comes in front of
    /// them later stands in front of it.
    log: Option<Vec<u64>>,
}

impl Viewing {
    /// The path of the file, or the name of the text, terminal-safe, as messages name it.
    fn name(&self) -> String {
        match &self.source {
            Source::File { location, .. } => location_text(location),
            Source::Text { name, .. } => cells::sanitize(name.as_bytes()),
        }
    }

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
        let name = match &self.source {
            Source::File { location, .. } => file_name(location).unwrap_or_default(),
            Source::Text { name, .. } => name.clone().into_bytes(),
        };
        let icon = spin
            .wait_dots(&self.wait)
            .filter(|_| loading)
            .unwrap_or_else(|| decor.file(&name));
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
    /// back to once it ends (ADR 0051), with the mode the other panel had, which its command may
    /// end in too: Visual mode, if it kept that (ADR 0066).
    returns: Option<(Side, Mode)>,
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
    /// Files that the dialog of saving writes, with what goes in them.
    saving: HashMap<Location, Vec<u8>>,
    /// For the times in questions.
    tz: TimeZone,
    /// Running jobs: at most one in front, over the panels and the help and under the
    /// dialogs, and any number in the background.
    jobs: Vec<Job>,
    /// How the jobs on each host reach it, through each of its connections in turn.
    links: HashMap<String, Relink>,
    /// What waits for its hosts to connect, after Connect in an error, and runs once they are.
    pending: Vec<Again>,
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
    /// The open list of a combo box in what is in front.
    ComboBox,
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
        let panel = || {
            let start = Location::Local(start.to_path_buf());
            let (mut panel, request) = Panel::new(start, home.to_path_buf(), ui.show_hidden);
            panel.set_natural_sort(ui.natural_sort);
            (panel, request)
        };
        let [(left, left_request), (right, right_request)] = [panel(), panel()];
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
            links: HashMap::new(),
            pending: Vec::new(),
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
                | Context::KeyHints
                | Context::Checksums,
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

    /// The help screen, for the keymap and the settings, where it was when it closed last.
    fn help_screen(&self) -> Help {
        Help::new(&self.keymap, &self.about).at(self.help_spot)
    }

    /// Asks before quitting; Yes is the default unless jobs would stop.
    fn ask_quit(&mut self) {
        let (message, default) = if self.jobs.is_empty() {
            (fl!("quit-ask"), 0)
        } else {
            (fl!("quit-jobs", count = self.jobs.len()), 1)
        };
        let buttons = vec![Button::Yes, Button::No];
        let dialog = Dialog::question(
            &fl!("quit-title"),
            &message,
            buttons,
            default,
            Tone::Warning,
        );
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

/// Whether `text` can be the name of an entry in a directory, which a rename gives: not `.` or
/// `..`, and without a slash or a NUL. Empty text is a name nothing has.
fn names_an_entry(text: &str) -> bool {
    text != "." && text != ".." && !text.contains(['/', '\0'])
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

/// The name of a file of the checksums of `results`: `name.sha256` for one file, `SHA256SUMS`
/// for several, with the extension of the algorithm.
fn sums_name(results: &Results) -> String {
    match results.lines.as_slice() {
        [(name, _)] => {
            let name = String::from_utf8_lossy(name).into_owned();
            format!("{name}.{}", extension(results.algorithm))
        }
        _ => format!("{}SUMS", extension(results.algorithm).to_uppercase()),
    }
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

/// Whether `panel` waits for a listing on `host`.
fn waits_for(panel: &Panel, host: &str) -> bool {
    panel.pending_request().is_some_and(
        |request| matches!(&request.location, Location::Remote { host: wanted, .. } if wanted == host),
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
