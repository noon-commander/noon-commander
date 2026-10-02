//! State and drawing of the whole screen.

use std::collections::{HashMap, HashSet, VecDeque};
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;
use std::time::SystemTime;

use jiff::tz::TimeZone;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use sftp_tui_config::{TransferConfig, UiConfig};
use sftp_tui_ops::{Conflict, CopyOptions, Decision};
use sftp_tui_vfs::{FileKind, Location, Metadata, RemotePath};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::cells::{self, Align};
use super::decor::Decor;
use super::dialog::{Ask, Button, Dialog, DialogEvent, Reply};
use super::help::Help;
use super::keymap::{Action, Context, Keymap, Resolved};
use super::panel::{
    Destination, HostState, HostStatus, ListRequest, Listed, Panel, View, child, location_text,
};
use super::pattern::Pattern;
use super::progress::{Counts, JobView};
use super::tasks::{HostHandle, JobEvent};
use super::theme::Theme;
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
        side: Side,
        request: ListRequest,
        host: Option<HostHandle>,
    },
    /// Make the directory at `location` and report to [`App::created`]. Remote ones go to the
    /// task of their host.
    CreateDir {
        side: Side,
        location: Location,
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
    /// Run the copy job `id` of `sources`, all local or all on one host, to `target`, and
    /// report to [`App::job_event`]; `hosts` lead to the hosts of the sources and of the target,
    /// if they are remote, and `cancel` stops it.
    Copy {
        id: u64,
        sources: Vec<Location>,
        target: Location,
        hosts: (Option<HostHandle>, Option<HostHandle>),
        options: CopyOptions,
        cancel: CancellationToken,
    },
    /// Connect to a host and report to [`App::connected`] and [`App::closed`]; `stop` ends the
    /// attempt or the connection.
    Connect {
        host: String,
        connection: u64,
        stop: CancellationToken,
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
    /// `+` (`mark`) or `-` in the panel on `side`.
    Pattern { side: Side, mark: bool },
    /// F7 in the panel on `side`.
    Mkdir { side: Side },
    /// F8 in a panel on `dir`, for the entries `names`.
    Delete { dir: Location, names: Vec<Vec<u8>> },
    /// F5 in the panel on `side`, which shows `dir`, for the entries `names`. The field
    /// opened with `offered`, the text for the other panel's location, if it shows one.
    Copy {
        side: Side,
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
    /// Something to read, such as an error.
    Info,
}

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Delete,
    Copy,
}

/// A job on screen, over the panels; one runs at a time.
#[derive(Debug)]
struct Job {
    id: u64,
    kind: JobKind,
    /// Directories it changes, which panels read again when it ends.
    changes: Vec<Location>,
    /// Hosts it works on; it ends with their connections.
    hosts: Vec<String>,
    cancel: CancellationToken,
    view: JobView,
}

impl Job {
    fn new(id: u64, kind: JobKind, changes: Vec<Location>, cancel: CancellationToken) -> Self {
        let hosts = changes
            .iter()
            .filter_map(|location| match location {
                Location::Remote { host, .. } => Some(host.clone()),
                Location::Root | Location::Local(_) => None,
            })
            .collect();
        let view = match kind {
            JobKind::Delete => JobView::new(fl!("delete-title"), fl!("delete-deleting")),
            JobKind::Copy => JobView::new(fl!("copy-title"), fl!("copy-copying")),
        };
        Self {
            id,
            kind,
            changes,
            hosts,
            cancel,
            view,
        }
    }
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
    left: Panel,
    right: Panel,
    active: Side,
    /// The right panel is drawn on the left.
    swapped: bool,
    hosts: HashMap<String, Host>,
    connections: u64,
    /// The `[ui]` settings; `show_hidden` follows Alt-.
    ui: UiConfig,
    decor: Decor,
    theme: Theme,
    /// Counts the frames of spinners.
    tick: u64,
    /// Hosts whose last attempt failed or whose connection was lost.
    failed: HashSet<String>,
    /// Addresses from `ssh -G` in this session.
    addresses: HashMap<String, String>,
    /// The first one is on screen and gets the keys; the others wait, so that a new prompt
    /// never takes the keys from a dialog in use.
    dialogs: VecDeque<Open>,
    pattern_options: PatternOptions,
    copy_choices: CopyChoices,
    /// For the times in questions.
    tz: TimeZone,
    /// Over the panels and the help, under the dialogs.
    job: Option<Job>,
    jobs: u64,
    /// Over the panels, under the dialogs.
    help: Option<Help>,
    keymap: Keymap,
    quit: bool,
    redraw: bool,
}

impl App {
    /// Both panels on the local directory `start`, and the listings to request for them. From
    /// the virtual root, the local file system opens at `home`.
    pub(crate) fn new(
        start: &Path,
        home: &Path,
        ui: &UiConfig,
        transfer: &TransferConfig,
    ) -> (Self, Vec<Effect>) {
        let show_hidden = ui.show_hidden;
        let panel = || {
            let start = Location::Local(start.to_path_buf());
            Panel::new(start, home.to_path_buf(), show_hidden)
        };
        let (left, left_request) = panel();
        let (right, right_request) = panel();
        let mut app = Self {
            left,
            right,
            active: Side::Left,
            swapped: false,
            hosts: HashMap::new(),
            connections: 0,
            ui: ui.clone(),
            decor: Decor::new(ui.icons),
            // `ui.theme` was checked when the config was loaded.
            theme: Theme::by_name(&ui.theme).unwrap_or_else(Theme::mc_classic),
            tick: 0,
            failed: HashSet::new(),
            addresses: HashMap::new(),
            dialogs: VecDeque::new(),
            pattern_options: PatternOptions::default(),
            copy_choices: CopyChoices {
                preserve: true,
                atomic: transfer.atomic_upload,
            },
            tz: TimeZone::UTC,
            job: None,
            jobs: 0,
            help: None,
            keymap: Keymap::mc(),
            quit: false,
            redraw: false,
        };
        let mut effects = app.route(Side::Left, left_request);
        effects.extend(app.route(Side::Right, right_request));
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

    /// Shows times in `tz`.
    pub(crate) fn set_time_zone(&mut self, tz: TimeZone) {
        self.tz = tz;
    }

    /// What turns keys into actions.
    pub(crate) fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Where keys go now.
    pub(crate) fn context(&self) -> Context {
        if let Some(open) = self.dialogs.front() {
            open.dialog.context()
        } else if self.job.is_some() || self.help.is_some() {
            Context::Dialog
        } else if self.panel(self.active).searching() {
            Context::QuickSearch
        } else if self.panel(self.active).shows_root() {
            Context::Root
        } else {
            Context::Panel
        }
    }

    /// Whether the app does something for `action` now; the F-key bar shows only those.
    fn supports(&self, action: Action) -> bool {
        match action {
            Action::Help | Action::Quit | Action::Redraw | Action::Disconnect | Action::Cancel => {
                true
            }
            Action::Mkdir | Action::Delete | Action::Copy => !self.panel(self.active).shows_root(),
            _ => false,
        }
    }

    fn panel(&self, side: Side) -> &Panel {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn panel_mut(&mut self, side: Side) -> &mut Panel {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    pub(crate) fn handle(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(open) = self.dialogs.front_mut() {
            let event = open.dialog.handle(input);
            if event == DialogEvent::Pending {
                return Vec::new();
            }
            if let Some(Open { dialog, purpose }) = self.dialogs.pop_front() {
                return self.dialog_closed(&dialog, purpose, event);
            }
            return Vec::new();
        }
        if let Some(job) = &mut self.job {
            if JobView::wants_abort(input) {
                job.cancel.cancel();
                job.view.abort();
            }
            return Vec::new();
        }
        if let Some(help) = &mut self.help {
            if help.handle(input) {
                self.help = None;
            }
            return Vec::new();
        }
        let type_to_search = self.ui.type_to_search;
        let panel = self.panel_mut(self.active);
        let action = match input {
            Resolved::Insert(c) => {
                if type_to_search || panel.searching() {
                    panel.search_type(c);
                }
                return Vec::new();
            }
            Resolved::Action(action) if panel.searching() => match action {
                Action::Backspace => {
                    panel.search_back();
                    return Vec::new();
                }
                Action::QuickSearch => {
                    panel.search_next();
                    return Vec::new();
                }
                Action::Cancel => {
                    panel.end_search();
                    return Vec::new();
                }
                // Any other key ends the search, then does what it does.
                _ => {
                    panel.end_search();
                    action
                }
            },
            Resolved::Action(action) => action,
        };
        match action {
            Action::QuickSearch => self.panel_mut(self.active).search_next(),
            Action::Quit => self.quit = true,
            Action::Redraw => self.redraw = true,
            Action::Help => self.help = Some(Help::new(&self.keymap, self.ui.type_to_search)),
            Action::SwitchPanel => self.active = self.active.other(),
            // The active panel moves to the other side and stays active, as in mc.
            Action::SwapPanels => self.swapped = !self.swapped,
            Action::OtherPanelOpen => {
                if let Some(destination) = self.panel_mut(self.active).for_other_panel() {
                    return self.go(self.active.other(), destination);
                }
            }
            Action::OtherPanelSync => {
                let here = self.panel(self.active).here();
                return self.go(self.active.other(), here);
            }
            // As in mc, for both panels.
            Action::ToggleHidden => {
                self.ui.show_hidden = !self.ui.show_hidden;
                for side in Side::BOTH {
                    let show = self.ui.show_hidden;
                    self.panel_mut(side).set_show_hidden(show);
                }
            }
            Action::Mkdir => self.ask_mkdir(),
            Action::Delete => self.ask_delete(),
            Action::Copy => self.ask_copy(),
            Action::Select => self.ask_pattern(true),
            Action::Unselect => self.ask_pattern(false),
            Action::Cancel => self.cancel(self.active),
            Action::Disconnect => return self.disconnect(self.active),
            _ => {
                let side = self.active;
                if let Some(request) = self.panel_mut(side).handle(action) {
                    return self.route(side, request);
                }
            }
        }
        Vec::new()
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
            Purpose::Pattern { side, mark } if ok => self.mark_matching(side, mark, dialog),
            Purpose::Mkdir { side } if ok && !dialog.text().is_empty() => {
                if let Some(location) = self.panel(side).resolve(dialog.text()) {
                    return self.create_dir(side, location);
                }
            }
            Purpose::Delete { dir, names } if event == DialogEvent::Pressed(Button::Yes) => {
                return self.start_delete(dir, &names);
            }
            Purpose::Copy {
                side,
                dir,
                names,
                offered,
            } if ok => {
                self.copy_choices.preserve = dialog.checked(0);
                let target = match offered {
                    Some((text, location)) if text == dialog.text() => Some(location),
                    _ => self.resolve_target(side, dialog.text()),
                };
                if let Some(target) = target {
                    return self.start_copy(dir, &names, target);
                }
            }
            Purpose::Failure { reply, .. } => {
                let decision = match event {
                    DialogEvent::Pressed(Button::Skip) => Decision::Skip,
                    DialogEvent::Pressed(Button::SkipAll) => Decision::SkipAll,
                    DialogEvent::Pressed(Button::Retry) => Decision::Retry,
                    _ => Decision::Abort,
                };
                let _ = reply.send(decision);
            }
            Purpose::Conflict { reply, .. } => {
                let conflict = match event {
                    DialogEvent::Pressed(Button::Yes) => Conflict::Overwrite,
                    DialogEvent::Pressed(Button::No) => Conflict::Skip,
                    DialogEvent::Pressed(Button::All) => Conflict::OverwriteAll,
                    DialogEvent::Pressed(Button::KeepAll) => Conflict::SkipAll,
                    DialogEvent::Pressed(Button::Older) => Conflict::OverwriteOlder,
                    _ => Conflict::Abort,
                };
                let _ = reply.send(conflict);
            }
            Purpose::Pattern { .. }
            | Purpose::Mkdir { .. }
            | Purpose::Delete { .. }
            | Purpose::Copy { .. }
            | Purpose::Info => {}
        }
        Vec::new()
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
            Location::Root | Location::Local(_) => None,
        };
        let targets = names.iter().filter_map(|name| child(&dir, name)).collect();
        self.jobs += 1;
        let id = self.jobs;
        let cancel = CancellationToken::new();
        self.job = Some(Job::new(id, JobKind::Delete, vec![dir], cancel.clone()));
        vec![Effect::Delete {
            id,
            targets,
            host,
            cancel,
        }]
    }

    /// Asks where F5 copies the marked entries of the active panel, or the one under the
    /// cursor, as mc does: the field opens with the other panel's location.
    fn ask_copy(&mut self) {
        let panel = self.panel(self.active);
        let chosen = panel.chosen();
        let message = match chosen.as_slice() {
            [] => return,
            [entry] => fl!("copy-one", name = cells::sanitize(&entry.name)),
            many => fl!("copy-many", count = many.len()),
        };
        let names = chosen.iter().map(|entry| entry.name.clone()).collect();
        let dir = panel.location().clone();
        let other = self.panel(self.active.other()).location().clone();
        let offered = (other != Location::Root).then(|| (location_text(&other), other));
        let text = offered
            .as_ref()
            .map(|(text, _)| text.clone())
            .unwrap_or_default();
        let checks = [(fl!("copy-preserve"), self.copy_choices.preserve)];
        let dialog = Dialog::form(
            &fl!("copy-title"),
            &message,
            &text,
            &checks,
            COPY_DIALOG_WIDTH,
        );
        let side = self.active;
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Copy {
                side,
                dir,
                names,
                offered,
            },
        });
    }

    /// Where a typed target points: `host:path` on a host the app knows, or a path from the
    /// directory of the panel on `side`.
    fn resolve_target(&self, side: Side, text: &str) -> Option<Location> {
        if text.is_empty() {
            return None;
        }
        if let Location::Remote { host, path } = Location::parse(text)
            && self.hosts.contains_key(&host)
        {
            return Some(Location::Remote { host, path });
        }
        self.panel(side).resolve(text)
    }

    /// Starts copying `names` from `dir` to `target`, and shows its progress; a target that
    /// is one of the sources, or in one, is an error.
    fn start_copy(&mut self, dir: Location, names: &[Vec<u8>], target: Location) -> Vec<Effect> {
        let sources: Vec<Location> = names.iter().filter_map(|name| child(&dir, name)).collect();
        let error =
            |reason: String| fl!("copy-error", path = location_text(&target), reason = reason);
        if target == dir {
            self.show_error(&error(fl!("copy-same")));
            return Vec::new();
        }
        if let Some(source) = sources.iter().find(|source| within(source, &target)) {
            let reason = fl!("copy-into-itself", path = location_text(source));
            self.show_error(&error(reason));
            return Vec::new();
        }
        let handle = |location: &Location| match location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Ok(Some(handle.clone())),
                _ => Err(()),
            },
            Location::Root | Location::Local(_) => Ok(None),
        };
        let (Ok(from), Ok(to)) = (handle(&dir), handle(&target)) else {
            self.show_error(&error(fl!("error-connection-closed")));
            return Vec::new();
        };
        self.jobs += 1;
        let id = self.jobs;
        let cancel = CancellationToken::new();
        // A copy goes into the target, or to it as a new name in its parent.
        let changes = vec![target.clone(), target.parent(), dir];
        self.job = Some(Job::new(id, JobKind::Copy, changes, cancel.clone()));
        let options = CopyOptions {
            preserve: self.copy_choices.preserve,
            atomic: self.copy_choices.atomic,
            remove_sources: false,
        };
        vec![Effect::Copy {
            id,
            sources,
            target,
            hosts: (from, to),
            options,
            cancel,
        }]
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
        let Some(job) = self.job.as_mut().filter(|job| job.id == id) else {
            return Vec::new();
        };
        match event {
            JobEvent::Scanning { items } => job.view.scanning(items),
            JobEvent::Progress {
                current,
                done,
                total,
                bytes_done,
                bytes_total,
            } => {
                let counts = Counts {
                    done,
                    total,
                    bytes_done,
                    bytes_total,
                };
                job.view.working(location_text(&current), counts);
            }
            JobEvent::Exists {
                target,
                source_metadata,
                target_metadata,
                reply,
            } => {
                let metadata = (&source_metadata, &target_metadata);
                self.ask_conflict(id, &target, metadata, reply);
            }
            JobEvent::Failed { path, error, reply } => {
                let path = location_text(&path);
                let reason = cells::sanitize(error.as_bytes());
                let message = match job.kind {
                    JobKind::Delete => fl!("delete-error", path = path, reason = reason),
                    JobKind::Copy => fl!("copy-error", path = path, reason = reason),
                };
                let buttons = vec![Button::Skip, Button::SkipAll, Button::Retry, Button::Abort];
                let dialog = Dialog::question(&fl!("dialog-error"), &message, buttons, 0, true);
                self.dialogs.push_back(Open {
                    dialog,
                    purpose: Purpose::Failure { job: id, reply },
                });
            }
            JobEvent::Finished => {
                if let Some(job) = self.end_job() {
                    let mut effects = Vec::new();
                    for (index, dir) in job.changes.iter().enumerate() {
                        if !job.changes[..index].contains(dir) {
                            effects.extend(self.reload(dir));
                        }
                    }
                    return effects;
                }
            }
        }
        Vec::new()
    }

    /// Takes the job off the screen, with the questions it asked.
    fn end_job(&mut self) -> Option<Job> {
        let job = self.job.take()?;
        self.dialogs.retain(|open| match open.purpose {
            Purpose::Failure { job: asked, .. } | Purpose::Conflict { job: asked, .. } => {
                asked != job.id
            }
            _ => true,
        });
        Some(job)
    }

    /// Reads `dir` again in the panels that show it, with their cursors where they were.
    fn reload(&mut self, dir: &Location) -> Vec<Effect> {
        let mut effects = Vec::new();
        for side in Side::BOTH {
            let panel = self.panel_mut(side);
            if panel.location() == dir {
                let here = panel.here();
                let request = panel.go(here);
                effects.extend(self.route(side, request));
            }
        }
        effects
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
        let dialog = Dialog::form(&title, &prompt, &name, &[], MKDIR_DIALOG_WIDTH);
        let side = self.active;
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Mkdir { side },
        });
    }

    /// Makes the directory at `location` for the panel on `side`, in the background.
    fn create_dir(&self, side: Side, location: Location) -> Vec<Effect> {
        let host = match &location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Some(handle.clone()),
                _ => None,
            },
            Location::Root | Location::Local(_) => None,
        };
        vec![Effect::CreateDir {
            side,
            location,
            host,
        }]
    }

    /// Takes the result of an [`Effect::CreateDir`]: panels on the directory it is in read it
    /// again, the one that asked with the cursor on it; an error shows in a dialog.
    pub(crate) fn created(
        &mut self,
        side: Side,
        location: &Location,
        result: Result<(), String>,
    ) -> Vec<Effect> {
        if let Err(reason) = result {
            let path = location_text(location);
            let reason = cells::sanitize(reason.as_bytes());
            self.show_error(&fl!("mkdir-error", path = path, reason = reason));
            return Vec::new();
        }
        let parent = location.parent();
        let name = file_name(location);
        let mut effects = Vec::new();
        for panel_side in Side::BOTH {
            let panel = self.panel_mut(panel_side);
            if *panel.location() != parent {
                continue;
            }
            let request = match &name {
                Some(name) if panel_side == side => panel.reload_onto(name.clone()),
                _ => {
                    let here = panel.here();
                    panel.go(here)
                }
            };
            effects.extend(self.route(panel_side, request));
        }
        effects
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
        let side = self.active;
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Pattern { side, mark },
        });
    }

    /// Marks, or unmarks, what the dialog of `+` or `-` asked for. An empty pattern does
    /// nothing.
    fn mark_matching(&mut self, side: Side, mark: bool, dialog: &Dialog) {
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
        self.panel_mut(side).mark_where(mark, |entry| {
            !(options.files_only && entry.is_dir_like())
                && pattern.matches(&fold(&entry.display_name()))
        });
        self.pattern_options = options;
    }

    /// Sends the panel on `side` to `destination`.
    fn go(&mut self, side: Side, destination: Destination) -> Vec<Effect> {
        let request = self.panel_mut(side).go(destination);
        self.route(side, request)
    }

    /// Sends a panel's request where it can be answered. A host that is not connected gets
    /// connected first; its panels' requests go out once it is.
    fn route(&mut self, side: Side, request: ListRequest) -> Vec<Effect> {
        let Location::Remote { host, .. } = &request.location else {
            return vec![Effect::List {
                side,
                request,
                host: None,
            }];
        };
        match self.hosts.get(host) {
            Some(Host::Connected { handle, .. }) => {
                let host = Some(handle.clone());
                vec![Effect::List {
                    side,
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
            for side in Side::BOTH {
                if waits_for(self.panel(side), &host) {
                    self.panel_mut(side).cancel();
                }
            }
        }
        self.panel_mut(side).cancel();
    }

    /// Closes the connection to the host under the cursor of the panel on `side`, or stops
    /// connecting to it. Panels on that host go back to the root.
    fn disconnect(&mut self, side: Side) -> Vec<Effect> {
        let Some(host) = self.panel(side).host_under_cursor().map(str::to_owned) else {
            return Vec::new();
        };
        let Some(state) = self.hosts.remove(&host) else {
            return Vec::new();
        };
        state.stop();
        let mut effects = Vec::new();
        for side in Side::BOTH {
            let panel = self.panel_mut(side);
            if matches!(state, Host::Connecting { .. }) {
                if waits_for(panel, &host) {
                    panel.cancel();
                }
            } else if let Some(request) = panel.leave_host(&host, None) {
                effects.extend(self.route(side, request));
            }
        }
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
    pub(crate) fn listed(&mut self, side: Side, generation: u64, result: Result<Listed, String>) {
        self.panel_mut(side).listed(generation, result);
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
        for side in Side::BOTH {
            if let Some(request) = self.panel(side).pending_request()
                && waits_for(self.panel(side), host)
            {
                effects.extend(self.route(side, request));
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
        // Its task dropped the job; its panels leave the host, so there is nothing to read.
        if self
            .job
            .as_ref()
            .is_some_and(|job| job.hosts.iter().any(|on| on == host))
        {
            self.end_job();
        }
        let mut effects = Vec::new();
        for side in Side::BOTH {
            let panel = self.panel_mut(side);
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
                        effects.extend(self.route(side, request));
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

    /// Whether something on screen moves while time passes: a host that is connecting.
    pub(crate) fn animates(&self) -> bool {
        self.hosts
            .values()
            .any(|host| matches!(host, Host::Connecting { .. }))
    }

    /// Moves spinners on by a frame.
    pub(crate) fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// Stops every connection and connection attempt, for quitting.
    pub(crate) fn disconnect_all(&mut self) {
        for (_, state) in self.hosts.drain() {
            state.stop();
        }
        if let Some(job) = &self.job {
            job.cancel.cancel();
        }
    }

    /// Two panels side by side above the F-key bar.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, now: SystemTime, tz: &TimeZone) {
        let [panels, key_bar] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let [mut left, mut right] = Layout::horizontal([Constraint::Fill(1); 2]).areas(panels);
        if self.swapped {
            std::mem::swap(&mut left, &mut right);
        }
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
            tick: self.tick,
            now,
            tz,
        };
        self.left.render(frame, left, active == Side::Left, &view);
        self.right
            .render(frame, right, active == Side::Right, &view);
        self.render_fkeys(frame, key_bar);
        if let Some(help) = &mut self.help {
            help.render(frame, panels, &self.theme);
        }
        if let Some(job) = &self.job {
            job.view.render(frame, panels, &self.theme);
        }
        if let Some(open) = self.dialogs.front() {
            open.dialog.render(frame, panels, &self.theme);
        }
    }

    /// The F-key bar: ten equal slots, each the key number and the label of its action.
    fn render_fkeys(&self, frame: &mut Frame<'_>, area: Rect) {
        let slots = Layout::horizontal([Constraint::Fill(1); 10]).split(area);
        let actions = self.keymap.fkeys(self.context());
        for (number, (slot, action)) in (1..).zip(slots.iter().zip(actions)) {
            let label = action
                .filter(|action| self.supports(*action))
                .and_then(fkey_label)
                .unwrap_or_default();
            let number = number.to_string();
            // The label's color fills its slot, as in mc.
            let room = usize::from(slot.width).saturating_sub(number.len());
            let line = Line::from(vec![
                Span::styled(number, self.theme.fkey_number),
                Span::styled(cells::fit(&label, room, Align::Left), self.theme.fkey_label),
            ]);
            frame.render_widget(line, *slot);
        }
    }
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
        Location::Root => None,
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
        Action::Mkdir => Some(fl!("fkey-mkdir")),
        Action::Delete => Some(fl!("fkey-delete")),
        Action::Copy => Some(fl!("fkey-copy")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    use std::sync::mpsc;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use secrecy::ExposeSecret as _;
    use sftp_tui_ssh::askpass::PromptKind;
    use sftp_tui_vfs::{DirEntry, FileKind, Metadata, RemotePath};

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

    fn transfer() -> TransferConfig {
        TransferConfig::default()
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
            let Effect::List { side, request, .. } = effect else {
                panic!("expected a listing, got {effect:?}");
            };
            let location = request.location;
            let listing = listing.clone();
            app.listed(side, request.generation, Ok(Listed { location, listing }));
        }
    }

    /// An app on `/srv` whose first listings arrived.
    fn loaded() -> App {
        let (mut app, effects) =
            App::new(Path::new("/srv"), Path::new("/home/me"), &ui(), &transfer());
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("left"), dir("right")]),
        );
        app
    }

    /// An app with both panels on the virtual root, which lists `web` and `db`.
    fn at_root() -> App {
        let (mut app, effects) =
            App::new(Path::new("/"), Path::new("/home/me"), &ui(), &transfer());
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        let hosts = ["web", "db"].map(|alias| RootHost {
            alias: alias.to_owned(),
            label: None,
            address: None,
        });
        for side in Side::BOTH {
            app.active = side;
            let effects = app.handle(action(Action::Parent));
            answer(&mut app, effects, &Listing::Root(hosts.to_vec()));
        }
        app.active = Side::Left;
        app
    }

    /// Opens the host `rows` rows below `[Local]` in the panel on `side`.
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
        let (_, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &ui(), &transfer());
        let sides: Vec<Side> = effects
            .iter()
            .map(|effect| match effect {
                Effect::List {
                    side, host: None, ..
                } => *side,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(sides, [Side::Left, Side::Right]);
    }

    #[test]
    fn keys_go_to_the_active_panel_and_tab_switches_it() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let Effect::List { side, request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        assert_eq!((side, request.location), (Side::Left, local("/srv/left")));

        assert!(app.handle(action(Action::SwitchPanel)).is_empty());
        app.handle(action(Action::End));
        let Effect::List { side, request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        assert_eq!((side, request.location), (Side::Right, local("/srv/right")));

        app.handle(action(Action::SwitchPanel));
        assert_eq!(app.active, Side::Left);
    }

    /// The titles of the panels drawn on the left and on the right.
    fn titles(app: &mut App) -> (String, String) {
        let text = screen(app);
        let top = text.lines().next().unwrap();
        let (left, right) = top.split_once("┐┌").unwrap();
        (left.to_owned(), right.to_owned())
    }

    #[test]
    fn ctrl_u_swaps_where_the_panels_are_and_replies_follow_them() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let Effect::List { side, request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        assert!(app.handle(action(Action::SwapPanels)).is_empty());
        assert_eq!(app.active, Side::Left, "the active panel stays active");

        let location = request.location.clone();
        let listing = Listing::Dir(vec![dir("inner")]);
        app.listed(side, request.generation, Ok(Listed { location, listing }));
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
        let Effect::List { side, request, .. } = one(app.handle(action(Action::OtherPanelOpen)))
        else {
            panic!("expected a listing");
        };
        assert_eq!(
            (side, &request.location),
            (Side::Right, &local("/srv/left"))
        );
        assert_eq!(app.active, Side::Left);
        answer(
            &mut app,
            vec![Effect::List {
                side,
                request,
                host: None,
            }],
            &Listing::Dir(Vec::new()),
        );

        let effects = app.handle(action(Action::OtherPanelSync));
        let [Effect::List { side, request, .. }] = &effects[..] else {
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
                side: Side::Right,
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
        let (mut app, effects) =
            App::new(Path::new("/srv"), Path::new("/home/me"), &ui(), &transfer());
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
    fn create_dir(effects: Vec<Effect>) -> (Side, Location, Option<HostHandle>) {
        match one(effects) {
            Effect::CreateDir {
                side,
                location,
                host,
            } => (side, location, host),
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
        assert_eq!(app.context(), Context::DialogInput);
        let text = screen(&mut app);
        assert!(text.contains("Create a new directory"), "{text}");
        assert!(text.contains("Enter directory name:"), "{text}");
        type_text(&mut app, "new");
        let (side, location, host) = create_dir(app.handle(action(Action::Confirm)));
        assert_eq!(
            (side, &location, host.is_none()),
            (Side::Left, &local("/srv/new"), true)
        );

        // Both panels show /srv and read it again; the one that asked lands on the directory.
        let effects = app.created(side, &location, Ok(()));
        assert_eq!(effects.len(), 2);
        let listing = Listing::Dir(vec![dir("left"), dir("new"), dir("right")]);
        answer(&mut app, effects, &listing);
        assert_eq!(app.left.name_under_cursor(), Some(&b"new"[..]));
        assert_eq!(app.right.name_under_cursor(), Some(&b"right"[..]), "stays");

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
            Side::Left,
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
        assert!(app.created(Side::Left, &local("/tmp/x"), Ok(())).is_empty());
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
        app.listed(Side::Left, generation, Ok(Listed { location, listing }));
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
        assert!(app.job_event(id + 1, JobEvent::Finished).is_empty());
        let effects = app.job_event(id, JobEvent::Finished);
        assert_eq!(effects.len(), 2);
        assert_eq!(app.context(), Context::Panel);
    }

    #[test]
    fn f8_names_what_it_deletes_and_no_keeps_it() {
        let (mut app, effects) =
            App::new(Path::new("/srv"), Path::new("/home/me"), &ui(), &transfer());
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
        app.job_event(id, JobEvent::Finished);
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
        assert!(app.job.is_none());
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
        let effects = app.job_event(id, JobEvent::Finished);
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
        app.end_job();
        let (_, _, target, ends, _) = copy_job(copy_to(&mut app, "web:/var/www"));
        assert_eq!((target, ends), (remote("web", "/var/www"), (false, true)));
        app.end_job();
        let (_, _, target, _, _) = copy_job(copy_to(&mut app, "x:y"));
        assert_eq!(target, local("/srv/x:y"), "no host called x");
        app.end_job();

        // Preserve attributes, switched off, stays off.
        app.handle(action(Action::Copy));
        for step in [Action::NextField, Action::Toggle, Action::Confirm] {
            app.handle(action(step));
        }
        app.end_job();
        app.handle(action(Action::Copy));
        assert!(screen(&mut app).contains("[ ] Preserve attributes"));
        let (_, _, _, _, options) = copy_job(app.handle(action(Action::Confirm)));
        assert!(!options.preserve);
    }

    #[test]
    fn copies_write_directly_without_atomic_upload() {
        let transfer = TransferConfig {
            atomic_upload: false,
        };
        let (mut app, effects) =
            App::new(Path::new("/srv"), Path::new("/home/me"), &ui(), &transfer);
        answer(&mut app, effects, &Listing::Dir(vec![dir("left")]));
        app.handle(action(Action::Down));
        app.handle(action(Action::Copy));
        type_text(&mut app, "/tmp");
        let (_, _, target, _, options) = copy_job(app.handle(action(Action::Confirm)));
        assert_eq!(target, local("/tmp"));
        assert!(!options.atomic);
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
        app.listed(Side::Right, generation, Ok(Listed { location, listing }));

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
        assert!(app.job.is_none());
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
        let Effect::List { side, request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        let reply = |listing| {
            let location = request.location.clone();
            Ok(Listed { location, listing })
        };
        app.listed(
            side.other(),
            request.generation,
            reply(Listing::Dir(Vec::new())),
        );
        app.listed(
            side,
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
                    side,
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
    fn a_lost_connection_sends_its_panels_back_to_the_root() {
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
        let [Effect::List { side, request, .. }] = &effects[..] else {
            panic!("expected the root, got {effects:?}");
        };
        assert_eq!((*side, &request.location), (Side::Left, &Location::Root));
        let text = screen(&mut app);
        assert!(
            text.contains("Lost the connection to web: Broken pipe"),
            "{text}"
        );
        assert!(!stop.is_cancelled(), "it ended on its own");
    }

    #[test]
    fn the_root_disconnects_the_host_under_the_cursor() {
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

        // From the other panel's root.
        app.active = Side::Right;
        app.handle(action(Action::Down));
        let Effect::List {
            side,
            request,
            host: None,
        } = one(app.handle(action(Action::Disconnect)))
        else {
            panic!("expected the root for the left panel");
        };
        assert_eq!((side, &request.location), (Side::Left, &Location::Root));
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
        assert!(screen(&mut app).contains("│ ****** "));
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
        let (mut app, effects) =
            App::new(Path::new("/srv"), Path::new("/home/me"), &ui, &transfer());
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
    fn typing_searches_and_other_keys_end_the_search() {
        let mut app = loaded();
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
    fn without_type_to_search_only_ctrl_s_searches() {
        let ui = UiConfig {
            type_to_search: false,
            ..ui()
        };
        let (mut app, effects) =
            App::new(Path::new("/srv"), Path::new("/home/me"), &ui, &transfer());
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("left"), dir("right")]),
        );
        app.handle(Resolved::Insert('r'));
        assert_eq!(
            app.context(),
            Context::Panel,
            "typing does nothing, as in mc"
        );
        app.handle(action(Action::QuickSearch));
        app.handle(Resolved::Insert('r'));
        assert!(
            screen(&mut app).contains("Search: r"),
            "but goes into a search"
        );
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
}
