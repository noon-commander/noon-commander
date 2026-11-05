use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, UNIX_EPOCH};

use crossterm::event::KeyEvent;
use noc_config::{
    AnimationsConfig, LogLevel, MenuBar, PanelSide, SearchMode, SftpHost, TabBar, TransferConfig,
    Wheel, ZoxideConfig,
};
use noc_ops::{Choice, Decision, Later, Rest, Step};
use noc_ssh::askpass::PromptKind;
use noc_tools::ripgrep::Hit;
use noc_tools::zoxide::Scored;
use noc_vfs::{Code, DirEntry, FileKind, Metadata, RemotePath};
use noc_viewer::{Part as ViewerPart, Progress as ReadProgress, Stamp};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::style::Style;
use secrecy::ExposeSecret as _;

use super::*;
use crate::tui::command::{self, Place};
use crate::tui::describe::Reason;
use crate::tui::dialog::{Ask, Problem};
use crate::tui::find::{FindSummary, Finding};
use crate::tui::keymap::Typed;
use crate::tui::mouse::{Pointer, Press};
use crate::tui::panel::{
    Destination, HostState, HostStatus, ListError, ListErrorKind, Listed, Listing,
};
use crate::tui::places::{KnownHost, Places};
use crate::tui::progress::Counts;
use crate::tui::save::Save;
use crate::tui::search::{Slot, Way};
use crate::tui::spinner;
use crate::tui::tasks::{JobEvent, Refused, TakenFile, TakenName, Unmade};

mod basics;
mod checksums;
mod command_line;
mod completion;
mod copying;
mod editing;
mod files;
mod finding;
mod go_to;
mod help;
mod hosts;
mod jobs;
mod key_hints;
mod keys;
mod menus;
mod mode_line;
mod other_panel;
mod pointer;
mod searching;
mod tab;
mod viewer;
mod workspace;
mod zoxide;

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

/// Whether `text`, a screen, shows a problem as its dialog does: `head`, then `path` on the
/// next line, if any, then `reason` under a line across the frame.
fn shows_problem(text: &str, head: &str, path: Option<&str>, reason: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let Some(at) = lines.iter().position(|line| line.contains(head)) else {
        return false;
    };
    let has = |index: usize, part: &str| lines.get(index).is_some_and(|line| line.contains(part));
    match path {
        Some(path) => has(at + 1, path) && has(at + 3, reason),
        None => has(at + 2, reason),
    }
}

/// A file of `size` bytes in the way of a rename, or of a save if `source` is `None`, with
/// `free` free beside it.
fn taken(source: Option<u64>, size: u64, free: &str) -> Result<(), Refused> {
    Err(Refused::Taken(Box::new(TakenFile {
        source: source.map(|size| file("new", size).metadata),
        target: file("there", size).metadata,
        free: free.as_bytes().to_vec(),
    })))
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

/// The effects but those for zoxide.
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
    let (reason, code) = match kind {
        ListErrorKind::NotFound => ("no such file or directory", Some(Code::Os(2))),
        ListErrorKind::Denied => ("permission denied", Some(Code::Os(13))),
        ListErrorKind::Other => ("connection reset", None),
    };
    let mut next = Vec::new();
    for effect in effects {
        let Effect::List { panel, request, .. } = effect else {
            panic!("expected a listing, got {effect:?}");
        };
        let error = ListError {
            kind,
            reason: reason.to_owned(),
            code,
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

/// Draws `app` 160 by 12 and finds where `text` starts on the screen.
fn find(app: &mut App, text: &str) -> (u16, u16) {
    let mut terminal = Terminal::new(TestBackend::new(160, 12)).unwrap();
    terminal
        .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
        .unwrap();
    let at = super::super::mouse::find(terminal.backend().buffer(), text);
    (at.x, at.y)
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

/// Types `text` in the text field in front in place of what it holds, after Ctrl+u.
fn retype(app: &mut App, text: &str) {
    app.handle(action(Action::DeleteToStart));
    type_text(app, text);
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

/// A failure of a job at `path`, now, that waits for `reply`.
fn failed(path: Location, error: String, reply: oneshot::Sender<Resolution>) -> JobEvent {
    JobEvent::Failed {
        at: Instant::now(),
        path,
        step: Step::Remove,
        reason: error.into(),
        retry: true,
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

fn typed(app: &mut App, text: &str) {
    for c in text.chars() {
        app.handle(Resolved::Insert(c));
    }
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

/// The line of `text` that holds `needle`.
fn line_with<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|line| line.contains(needle))
        .unwrap_or("")
}

/// Opens Options → Configuration… through F9.
fn open_configuration(app: &mut App) {
    app.handle(action(Action::PullDown));
    app.handle(Resolved::Hotkey('o'));
    assert!(app.handle(Resolved::Hotkey('c')).is_empty());
    assert!(app.configuration.is_some());
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

/// The tab numbers on `side`, and the one that shows.
fn tab_numbers(app: &App, side: Side) -> (Vec<u64>, u64) {
    let tabs = app.tabs(side);
    let numbers = tabs.iter().map(|tab| tab.id.tab).collect();
    (numbers, tabs.active().id.tab)
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

/// Runs `command` from the command line of the active panel.
fn run(app: &mut App, command: &str) -> Vec<Effect> {
    app.handle(action(Action::Shell));
    type_text(app, command);
    let effects = without_zoxide(app.handle(action(Action::Confirm)));
    app.take_run();
    effects
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
