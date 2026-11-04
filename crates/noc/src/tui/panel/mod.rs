//! A panel that lists a local or remote directory.

use std::cell::Cell;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::tz::TimeZone;
use noc_locale::Formats;
use noc_scroll::scrollbar::Bar;
use noc_vfs::{Code, DirEntry, Location, RemotePath, Space, VfsError};
use ratatui::layout::Rect;

use super::cells;
use super::decor::Decor;
use super::describe;
use super::dialog::Field;
use super::glyphs;
use super::keymap::Action;
use super::scroll::Window;
use super::search::{GlyphSpots, Input};
use super::spinner::{Spin, Wait};
use super::theme::Theme;

mod jumps;
mod marks;
mod pointer;
mod quick_search;
mod rename;
mod render;
mod visual;

#[cfg(test)]
mod tests;

/// Jumps the jump list of a panel keeps, as vim's keeps 100.
const MAX_JUMPS: usize = 100;

/// Narrow panels drop columns to keep at least this many cells for names.
const MIN_NAME_WIDTH: usize = 8;

/// Cells the target of a symlink needs on the status line, its mark of text cut short among
/// them; with less room the name shows alone.
const MIN_LINK_TARGET: usize = 4;

/// A request to list `location` for a panel; the reply must carry `generation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListRequest {
    pub(crate) generation: u64,
    pub(crate) location: Location,
    /// Where a host opens, if the location is the host itself (the empty path) and the
    /// directory is still there: the last one shown on it, for `remember_dir`.
    pub(crate) resume: Option<RemotePath>,
}

/// The reply to a [`ListRequest`]: where the listing is from, which may be more exact than
/// the request (a host's start directory as a path), and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) location: Location,
    pub(crate) listing: Listing,
    /// The space of the file system that holds the directory; `None` when unknown.
    pub(crate) space: Option<Space>,
}

/// What a location holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Listing {
    /// The entries of a directory, in any order.
    Dir(Vec<DirEntry>),
}

/// Why a location could not be listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListError {
    pub(crate) kind: ListErrorKind,
    /// The reason in words.
    pub(crate) reason: String,
    /// The number the system or the server gave the failure, if any.
    pub(crate) code: Option<Code>,
    /// Where the listing failed, if more exact than the request: the directory a host opens
    /// in, as a path.
    pub(crate) location: Option<Location>,
}

impl ListError {
    pub(crate) fn vfs(error: &VfsError) -> Self {
        let kind = match error {
            VfsError::NotFound(..) => ListErrorKind::NotFound,
            VfsError::PermissionDenied(..) => ListErrorKind::Denied,
            _ => ListErrorKind::Other,
        };
        let reason = describe::vfs_reason(error);
        Self {
            kind,
            reason: reason.text,
            code: reason.code,
            location: None,
        }
    }

    pub(crate) fn other(reason: String) -> Self {
        Self {
            kind: ListErrorKind::Other,
            reason,
            code: None,
            location: None,
        }
    }

    /// The same failure, at `location`.
    #[must_use]
    pub(crate) fn at(self, location: Location) -> Self {
        Self {
            location: Some(location),
            ..self
        }
    }
}

/// What kind of failure a [`ListError`] is: a directory that is not there is passed by on the
/// way up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListErrorKind {
    /// The directory is not there.
    NotFound,
    /// The directory may not be read.
    Denied,
    /// Anything else, such as a connection that broke: trying again may help.
    Other,
}

/// A listing that failed, for the app to report with the ways out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    /// Where the panel tried to go, a host by the directory it opens in where known; Retry goes
    /// there again.
    pub(crate) destination: Destination,
    /// The location as the message names it: a subdirectory by its name alone.
    pub(crate) shown: String,
    pub(crate) error: ListError,
    /// The parent of the location, with the cursor on its name.
    pub(crate) parent: Option<Destination>,
    /// The panel stands on the location, as when its directory was removed or the panel never
    /// got to show it: staying there shows nothing.
    pub(crate) stranded: bool,
    /// The panel went up to the location on its way to the nearest directory that is there.
    pub(crate) climbing: bool,
}

/// What the app knows about a host beyond the ssh config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HostState {
    pub(crate) status: HostStatus,
    /// From `ssh -G` in this session, fresher than the cached address in the listing.
    pub(crate) address: Option<String>,
}

/// The connection state of a host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum HostStatus {
    #[default]
    Idle,
    Connecting,
    Connected,
    /// The last attempt failed, or the connection was lost.
    Failed,
    /// As `Failed`, while a dialog asks whether to try again; panels still wait for it.
    Stalled,
}

/// Quick search while it runs, as vim keeps its incremental search. Positions between rows
/// are in half rows: `2 * row + 1` is the start of a row, and `2 * row + 2` is in the row after
/// its start, where Ctrl+g and Ctrl+t leave vim's `search_start`.
#[derive(Debug)]
struct Search {
    field: Field,
    /// The row it started on, where Esc puts the cursor back: vim's `save_cursor`.
    origin: usize,
    /// Where it searches from: down to the first match after it, or up to the first before
    /// it; vim's `search_start`.
    from: usize,
    /// The row of the match the cursor is on, if what is typed matches: vim's `match_start`.
    found: Option<usize>,
    /// The count typed before `/` or `?`: it goes to that match, as vim's `3/` does.
    count: usize,
    /// It goes up the rows, as vim's `?`.
    back: bool,
    /// The rows on screen that Esc puts back, as vim's `old_viewstate`.
    view: Window,
    /// How it matches what is typed, and whether case counts: as the last quick search did,
    /// until the keys after Ctrl+s switch them.
    input: Input,
}

impl Search {
    fn new(origin: usize, count: usize, back: bool, view: Window, input: Input) -> Self {
        Self {
            field: Field::plain(""),
            origin,
            from: 2 * origin + 1,
            found: None,
            count: count.max(1),
            back,
            view,
            input,
        }
    }

    /// The row where it searches from, where the cursor stays while nothing matches.
    fn start_row(&self) -> usize {
        self.from.saturating_sub(1) / 2
    }
}

/// How quick search ended on Enter: what it searched for, which way, and what it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchEnd {
    pub(crate) text: String,
    pub(crate) back: bool,
    /// How it matched, and whether case counted, which `n` and `N` match the same way.
    pub(crate) input: Input,
    /// Whether it went round past the end, or the start, if it found a match.
    pub(crate) found: Option<bool>,
}

/// What drawing a panel needs from the app.
pub(crate) struct View<'a> {
    /// What the app knows about a host.
    pub(crate) hosts: &'a dyn Fn(&str) -> HostState,
    pub(crate) decor: Decor,
    pub(crate) theme: &'a Theme,
    /// What spinners look like in this frame.
    pub(crate) spin: Spin,
    pub(crate) tz: &'a TimeZone,
    /// How dates and numbers are written.
    pub(crate) formats: &'a Formats,
}

/// What a directory is sorted by. Directories always come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortKey {
    Name,
    Extension,
    Time,
    Size,
}

/// How a panel orders names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameOrder {
    /// As the language of the formats orders them, with the numbers in them by value.
    Natural,
    /// By their characters, ignoring case.
    Characters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sort {
    key: SortKey,
    descending: bool,
}

impl Sort {
    /// Sorting by `key`: a new key starts as Far does, newest and largest first, names A to
    /// Z; the same key again reverses the order.
    fn by(self, key: SortKey) -> Self {
        let descending = if key == self.key {
            !self.descending
        } else {
            matches!(key, SortKey::Time | SortKey::Size)
        };
        Self { key, descending }
    }

    fn arrow(self) -> char {
        if self.descending {
            glyphs::DESCENDING
        } else {
            glyphs::ASCENDING
        }
    }
}

/// Which row gets the cursor once a listing arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Focus {
    /// The first row: `..`, or the first entry in a `/`.
    First,
    /// The entry with this name, or the first row if it is gone.
    Name(Vec<u8>),
    /// The entry with this name, or, if it is gone, the row it was on (or the last one), so
    /// that the cursor stays where it was when an entry is deleted.
    Near { name: Vec<u8>, row: usize },
}

impl Focus {
    fn matches(&self, row: Row<'_>) -> bool {
        match (self, row) {
            (Self::Name(name) | Self::Near { name, .. }, Row::Entry(entry)) => entry.name == *name,
            _ => false,
        }
    }
}

#[derive(Debug)]
struct Pending {
    generation: u64,
    location: Location,
    focus: Focus,
    /// Where the panel was, put in the jump list on the way: a listing that fails takes it
    /// out again, as vim keeps no jump that did not happen.
    jumped: Option<Destination>,
    /// The panel goes up to the nearest directory that is there.
    climbing: bool,
}

/// Where a panel can go: a location, and the row to put the cursor on there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Destination {
    location: Location,
    focus: Focus,
}

impl Destination {
    /// `location`, with the cursor on its first row.
    pub(crate) fn to(location: Location) -> Self {
        Self {
            location,
            focus: Focus::First,
        }
    }

    /// `location`, with the cursor on the entry `name`; on the first row if it is not there.
    pub(crate) fn onto(location: Location, name: &str) -> Self {
        Self::onto_entry(location, name.as_bytes())
    }

    /// The same, for a name as the directory lists it, which need not be UTF-8.
    pub(crate) fn onto_entry(location: Location, name: &[u8]) -> Self {
        Self {
            location,
            focus: Focus::Name(name.to_vec()),
        }
    }

    pub(crate) fn location(&self) -> &Location {
        &self.location
    }

    /// The parent directory of `location`, with the cursor on its name; `None` for a `/` and
    /// for the home directory of a host, which has no path.
    fn parent_of(location: &Location) -> Option<Self> {
        let parent = location.parent()?;
        let focus = match location {
            Location::Local(path) => path
                .file_name()
                .map_or(Focus::First, |name| Focus::Name(name.as_bytes().to_vec())),
            Location::Remote { path, .. } => path
                .file_name()
                .map_or(Focus::First, |name| Focus::Name(name.to_vec())),
        };
        Some(Self {
            location: parent,
            focus,
        })
    }
}

/// A row of the listing.
#[derive(Debug, Clone, Copy)]
enum Row<'a> {
    /// `..`: the parent directory; a `/` has none.
    Parent,
    Entry(&'a DirEntry),
}

/// An entry renamed in its row: its name, and the field with the new one.
#[derive(Debug)]
struct Renaming {
    name: Vec<u8>,
    field: Field,
}

/// One listing with a cursor.
#[derive(Debug)]
pub(crate) struct Panel {
    /// What is shown; a requested location replaces it once its listing arrives.
    location: Location,
    /// Directories first, then as `sort` says.
    listing: Listing,
    /// The space of the file system that holds the directory shown, on the bottom of the frame.
    space: Option<Space>,
    /// The entries of the listing that are shown, in order: hidden files may be left out.
    shown: Vec<usize>,
    /// The panel is on a host that is not connected, and shows the entries it kept toned down
    /// until it reads the directory again.
    disconnected: bool,
    sort: Sort,
    show_hidden: bool,
    names_by: NameOrder,
    /// Names of the marked entries; only shown ones, and never `..`.
    marked: HashSet<Vec<u8>>,
    /// The home directory, which `~` in dialogs stands for.
    home: PathBuf,
    /// Row under the cursor; row 0 is `..`, if there is one.
    cursor: usize,
    /// The rows on screen, as vim's window scrolls them.
    window: Window,
    /// Rows on screen at the last render.
    page: usize,
    /// Where the rows were at the last render, for the mouse.
    list: Rect,
    /// The scroll bar of the last render, on the right of the frame.
    bar: Option<Bar>,
    generation: u64,
    pending: Option<Pending>,
    /// What the user does in the panel, such as F7, by what it makes, until it is done: its
    /// result reads the directory again.
    working: Vec<Location>,
    /// Since when the panel waits, for a listing or for its work: one wait from the first to
    /// the last, so that the dots go on from one to the next.
    wait: Wait,
    /// Quick search, while it runs.
    search: Option<Search>,
    /// Where the last render drew the glyphs of how quick search matches, for the mouse.
    search_glyphs: Cell<Option<GlyphSpots>>,
    /// The entry under the cursor, while its name is edited in its row.
    renaming: Option<Renaming>,
    /// The row where Visual mode started, while it selects the rows from there to the cursor.
    visual: Option<usize>,
    /// The rows Visual mode selected last, for `g v`: where it started, and the cursor.
    last_visual: Option<(usize, usize)>,
    /// Visual mode, or the last one for `g v`, goes by lines, as vim's `V`, rather than by
    /// characters, as `v`: only the mode line tells them apart, as rows are lines either way.
    visual_line: bool,
    /// How many rows the last command of Visual mode took, for a count before `v`.
    reselect: Option<usize>,
    /// Where the panel was before its jumps, oldest first, as vim's jump list keeps them, and
    /// the place in it after Ctrl+o: its length while no jump went back.
    jumps: Vec<Destination>,
    jump: usize,
    /// Where the lines between columns ran at the last render, so that they can be joined to
    /// a top of the frame drawn again over the panel's.
    column_lines: Vec<u16>,
    /// How far the names are scrolled left, in cells, as vim's `zl` scrolls lines that do not
    /// wrap, their icons or markers staying in front; from their start again in another
    /// directory.
    shift: usize,
    /// The icons and the width of the names at the last render, which the scrolling of the
    /// names goes by.
    names: (Decor, usize),
}

impl Panel {
    /// A panel for `location`, and the request for its first listing. `~` in dialogs stands for
    /// `home`; `show_hidden` shows names that start with a dot.
    pub(crate) fn new(location: Location, home: PathBuf, show_hidden: bool) -> (Self, ListRequest) {
        Self::at(Destination::to(location), home, show_hidden)
    }

    /// A panel for `destination`, with the cursor where it says once the listing arrives, and
    /// the request for that listing.
    pub(crate) fn at(
        destination: Destination,
        home: PathBuf,
        show_hidden: bool,
    ) -> (Self, ListRequest) {
        let Destination { location, focus } = destination;
        let mut panel = Self {
            location: location.clone(),
            listing: Listing::Dir(Vec::new()),
            space: None,
            shown: Vec::new(),
            disconnected: false,
            sort: Sort {
                key: SortKey::Name,
                descending: false,
            },
            show_hidden,
            names_by: NameOrder::Natural,
            marked: HashSet::new(),
            home,
            cursor: 0,
            window: Window::default(),
            page: 1,
            list: Rect::default(),
            bar: None,
            generation: 0,
            pending: None,
            working: Vec::new(),
            wait: Wait::default(),
            search: None,
            search_glyphs: Cell::new(None),
            renaming: None,
            visual: None,
            last_visual: None,
            visual_line: false,
            reselect: None,
            jumps: Vec::new(),
            jump: 0,
            column_lines: Vec::new(),
            shift: 0,
            names: (Decor::new(false), 0),
        };
        let request = panel.open(location, focus);
        (panel, request)
    }

    /// A panel that shows what this one shows, sorted the same way and with the cursor on
    /// the same row, but without marks; and the request it waits for, if this one waits.
    pub(crate) fn duplicate(&self) -> (Self, Option<ListRequest>) {
        let mut panel = Self {
            location: self.location.clone(),
            listing: self.listing.clone(),
            space: self.space,
            shown: self.shown.clone(),
            disconnected: self.disconnected,
            sort: self.sort,
            show_hidden: self.show_hidden,
            names_by: self.names_by,
            marked: HashSet::new(),
            home: self.home.clone(),
            cursor: self.cursor,
            window: self.window,
            page: self.page,
            list: Rect::default(),
            bar: None,
            generation: 0,
            pending: None,
            working: Vec::new(),
            wait: Wait::default(),
            search: None,
            search_glyphs: Cell::new(None),
            renaming: None,
            visual: None,
            last_visual: None,
            visual_line: false,
            reselect: None,
            jumps: Vec::new(),
            jump: 0,
            column_lines: Vec::new(),
            shift: 0,
            names: (Decor::new(false), 0),
        };
        let request = self
            .pending
            .as_ref()
            .map(|pending| panel.open(pending.location.clone(), pending.focus.clone()));
        (panel, request)
    }

    fn open(&mut self, location: Location, focus: Focus) -> ListRequest {
        self.generation += 1;
        self.search = None;
        if location != self.location {
            self.renaming = None;
            self.visual = None;
            self.last_visual = None;
        }
        if !self.waits() {
            self.wait = Wait::default();
        }
        self.pending = Some(Pending {
            generation: self.generation,
            location: location.clone(),
            focus,
            jumped: None,
            climbing: false,
        });
        ListRequest {
            generation: self.generation,
            location,
            resume: None,
        }
    }

    /// Takes the reply to a [`ListRequest`]; replies to older requests are dropped. On error the
    /// panel keeps showing what it showed and returns the failure, for the app to report.
    pub(crate) fn listed(
        &mut self,
        generation: u64,
        result: Result<Listed, ListError>,
    ) -> Option<Failure> {
        let pending = self
            .pending
            .take_if(|pending| pending.generation == generation)?;
        match result {
            Ok(Listed {
                location,
                listing,
                space,
            }) => {
                // Reading the same directory again keeps the marks on names still there.
                if location != self.location {
                    self.shift = 0;
                    self.marked.clear();
                    self.renaming = None;
                    self.visual = None;
                    self.last_visual = None;
                }
                let visual = self.visual_focus();
                self.location = location;
                self.listing = listing;
                self.space = space;
                self.disconnected = false;
                self.arrange();
                self.window.reset();
                self.cursor = self.row_of(&pending.focus);
                self.restore_visual(visual);
                self.keep_renaming();
                None
            }
            Err(mut error) => {
                if pending.jumped.is_some()
                    && self.jump == self.jumps.len()
                    && self.jumps.last() == pending.jumped.as_ref()
                {
                    self.jumps.pop();
                    self.jump = self.jumps.len();
                }
                let stranded = pending.location == self.location;
                // A host by its name alone: Up, Go to and the message need the directory.
                let location = error.location.take().unwrap_or(pending.location);
                let shown = match (&location, &self.location) {
                    // The title shows the directory, so a subdirectory needs only its name.
                    (Location::Local(path), Location::Local(current)) => path
                        .strip_prefix(current)
                        .ok()
                        .filter(|relative| !relative.as_os_str().is_empty())
                        .map(|relative| cells::sanitize(relative.as_os_str().as_bytes())),
                    _ => None,
                };
                let shown = shown.unwrap_or_else(|| location_text(&location));
                // The reason may quote ssh, which may quote the server.
                let reason = cells::sanitize(error.reason.as_bytes());
                Some(Failure {
                    parent: Destination::parent_of(&location),
                    stranded,
                    climbing: pending.climbing,
                    destination: Destination {
                        location,
                        focus: pending.focus,
                    },
                    shown,
                    error: ListError { reason, ..error },
                })
            }
        }
    }

    /// The row that `focus` puts the cursor on.
    fn row_of(&self, focus: &Focus) -> usize {
        let rows = self.rows();
        (0..rows)
            .find(|&index| self.row(index).is_some_and(|row| focus.matches(row)))
            .unwrap_or(match focus {
                Focus::Near { row, .. } => (*row).min(rows.saturating_sub(1)),
                _ => 0,
            })
    }

    /// The request the panel waits for, if any.
    pub(crate) fn pending_request(&self) -> Option<ListRequest> {
        self.pending.as_ref().map(|pending| ListRequest {
            generation: pending.generation,
            location: pending.location.clone(),
            resume: None,
        })
    }

    /// The wait for a listing or for work, whose dots the title shows.
    pub(crate) fn wait(&self) -> Option<&Wait> {
        self.waits().then_some(&self.wait)
    }

    fn waits(&self) -> bool {
        self.pending.is_some() || !self.working.is_empty()
    }

    /// Starts work that makes `location`, such as a directory of F7, which the title waits for.
    pub(crate) fn begin_work(&mut self, location: Location) {
        if !self.waits() {
            self.wait = Wait::default();
        }
        self.working.push(location);
    }

    /// Ends the work that made `location`, once its result has asked for the directory again.
    pub(crate) fn end_work(&mut self, location: &Location) {
        if let Some(index) = self.working.iter().position(|work| work == location) {
            self.working.swap_remove(index);
        }
    }

    /// Stops waiting for work on `host`, whose connection took its result away.
    fn forget_work(&mut self, host: &str) {
        self.working
            .retain(|work| !matches!(work, Location::Remote { host: on, .. } if on == host));
    }

    /// Stops waiting for a listing; its reply will be dropped.
    pub(crate) fn cancel(&mut self) {
        self.pending = None;
    }

    /// Whether the panel shows a directory on `host`.
    fn on_host(&self, host: &str) -> bool {
        matches!(&self.location, Location::Remote { host: shown, .. } if shown == host)
    }

    /// Keeps the entries of its host, which is not connected, toned down, with the cursor and the
    /// marks: they may be gone by now. A rename, which needs the host, ends.
    fn go_offline(&mut self) {
        self.renaming = None;
        self.disconnected = true;
    }

    /// Takes the loss of the connection to `host`: on it, the panel keeps its entries, toned
    /// down, and waits to read its directory again, with the cursor where it was, once the host
    /// is back. A panel that only waits for the host waits on.
    pub(crate) fn lose_host(&mut self, host: &str) {
        self.forget_work(host);
        if self.on_host(host) {
            let here = self.here();
            self.go_offline();
            self.open(here.location, here.focus);
        }
    }

    /// Takes the end of the connection to `host`: on it, the panel keeps its entries, toned
    /// down, until it reads its directory again; a panel that waits for the host stops waiting.
    pub(crate) fn drop_host(&mut self, host: &str) {
        self.forget_work(host);
        let waits = matches!(
            &self.pending,
            Some(Pending { location: Location::Remote { host: wanted, .. }, .. }) if wanted == host
        );
        if waits {
            self.pending = None;
        }
        if self.on_host(host) {
            self.go_offline();
        }
    }

    /// The host the panel is on, if it is not connected, and the panel does not wait to read its
    /// directory either.
    pub(crate) fn not_connected(&self) -> Option<&str> {
        match &self.location {
            Location::Remote { host, .. } if self.disconnected && self.pending.is_none() => {
                Some(host)
            }
            _ => None,
        }
    }

    /// Whether the panel holds entries, if only those it kept from a host not connected.
    pub(crate) fn has_entries(&self) -> bool {
        let Listing::Dir(entries) = &self.listing;
        !entries.is_empty()
    }

    /// The last local directory in the jump list, with the cursor where it was.
    pub(crate) fn last_local(&self) -> Option<Destination> {
        self.jumps
            .iter()
            .rev()
            .find(|jump| matches!(jump.location, Location::Local(_)))
            .cloned()
    }

    /// Shows or hides names that start with a dot; the cursor stays on its entry if it can.
    pub(crate) fn set_show_hidden(&mut self, show: bool) {
        if self.show_hidden != show {
            self.show_hidden = show;
            self.rearrange();
        }
    }

    /// Sorts names as the language of the formats orders them, with the numbers in them by
    /// value, or by their characters.
    pub(crate) fn set_natural_sort(&mut self, natural: bool) {
        let names_by = if natural {
            NameOrder::Natural
        } else {
            NameOrder::Characters
        };
        if self.names_by != names_by {
            self.names_by = names_by;
            self.rearrange();
        }
    }

    /// Sorts the listing again, after the formats, and so the order of names, changed.
    pub(crate) fn sort_again(&mut self) {
        self.rearrange();
    }

    /// Sorts and filters a listing again, keeping the cursor on its entry, or on its row if the
    /// entry is no longer shown.
    fn rearrange(&mut self) {
        let focus = self.focus();
        let visual = self.visual_focus();
        self.arrange();
        self.restore_visual(visual);
        let rows = self.rows();
        self.cursor = (0..rows)
            .find(|&row| self.row(row).is_some_and(|row| focus.matches(row)))
            .unwrap_or_else(|| self.cursor.min(rows.saturating_sub(1)));
        self.keep_renaming();
    }

    /// Sorts a directory listing and picks the entries to show; entries that are not shown
    /// lose their marks.
    fn arrange(&mut self) {
        let Listing::Dir(entries) = &mut self.listing;
        let formats = noc_locale::current();
        let natural = (self.names_by == NameOrder::Natural).then_some(&*formats);
        sort(entries, self.sort, natural);
        let show_hidden = self.show_hidden;
        self.shown = (0..entries.len())
            .filter(|&index| show_hidden || !entries[index].name.starts_with(b"."))
            .collect();
        if !self.marked.is_empty() {
            let shown: HashSet<&[u8]> = self
                .shown
                .iter()
                .map(|&index| entries[index].name.as_slice())
                .collect();
            self.marked.retain(|name| shown.contains(name.as_slice()));
        }
    }

    /// Scrolls the names `to` cells left, but no further than [`Self::shift_limit`].
    fn shift_names(&mut self, to: usize) {
        self.shift = to.min(self.shift_limit());
    }

    /// How far the names can scroll left: until the end of the longest name on screen is at the
    /// right edge, as `ze` puts it, so that scrolling never leaves the column empty. The cursor's
    /// row counts even before a render brings it on screen.
    fn shift_limit(&self) -> usize {
        let on_screen = (self.window.top()..self.rows()).take(self.page);
        on_screen
            .chain([self.cursor])
            .filter_map(|index| self.row(index))
            .map(|row| self.shift_to_end(row))
            .max()
            .unwrap_or(0)
    }

    /// How far the names scroll for the end of `row`'s name to be at the right edge, as the
    /// last render drew them: its icon or marker stays in front, and only the name scrolls.
    fn shift_to_end(&self, row: Row<'_>) -> usize {
        let (decor, column) = self.names;
        let [prefix, name] = shown_name(row, decor);
        let room = column.saturating_sub(cells::width(&prefix));
        cells::width(&name).saturating_sub(room)
    }

    /// How far the names are scrolled left, in cells.
    #[cfg(test)]
    pub(crate) fn shift(&self) -> usize {
        self.shift
    }

    /// The whole name under the cursor, terminal-safe, without its icon or marker.
    pub(crate) fn full_name(&self) -> Option<String> {
        self.row(self.cursor)
            .map(|row| shown_name(row, self.names.0)[1].clone())
    }

    /// What the panel shows.
    pub(crate) fn location(&self) -> &Location {
        &self.location
    }

    /// The entry under the cursor; `None` on `..`.
    pub(crate) fn entry_under_cursor(&self) -> Option<&DirEntry> {
        match self.row(self.cursor)? {
            Row::Entry(entry) => Some(entry),
            Row::Parent => None,
        }
    }

    /// The name of the entry under the cursor; `None` on `..`.
    pub(crate) fn name_under_cursor(&self) -> Option<&[u8]> {
        self.entry_under_cursor().map(|entry| entry.name.as_slice())
    }

    /// Where `text`, typed in a dialog, points from this directory, as [`resolve`] reads it.
    pub(crate) fn resolve(&self, text: &str) -> Location {
        resolve(&self.location, &self.home, text)
    }

    /// What an operation acts on: the marked entries in the order shown, or else the entry
    /// under the cursor. Nothing on `..`.
    pub(crate) fn chosen(&self) -> Vec<&DirEntry> {
        let Listing::Dir(entries) = &self.listing;
        if !self.marked.is_empty() {
            return self
                .shown
                .iter()
                .map(|&index| &entries[index])
                .filter(|entry| self.marked.contains(&entry.name))
                .collect();
        }
        match self.row(self.cursor) {
            Some(Row::Entry(entry)) => vec![entry],
            _ => Vec::new(),
        }
    }

    /// Reads the directory again with the cursor on `name`, such as a directory just made.
    pub(crate) fn reload_onto(&mut self, name: Vec<u8>) -> ListRequest {
        self.open(self.location.clone(), Focus::Name(name))
    }

    /// Moves the cursor or opens a directory. Returns the listing to request, if any.
    pub(crate) fn handle(&mut self, action: Action) -> Option<ListRequest> {
        let rows = self.rows();
        let last = rows.saturating_sub(1);
        let page = self.page.max(1);
        match action {
            Action::Up => self.cursor = self.cursor.saturating_sub(1),
            Action::Down => self.cursor = (self.cursor + 1).min(last),
            // As vim's Ctrl+b, Ctrl+f, Ctrl+u, and Ctrl+d, which scroll the rows.
            Action::PageUp | Action::PageDown => self.page_by(action == Action::PageDown, 1),
            Action::HalfPageUp | Action::HalfPageDown => {
                let down = action == Action::HalfPageDown;
                self.window.half_page(down, &mut self.cursor, rows, page);
            }
            Action::Home | Action::End => {
                self.remember();
                self.cursor = if action == Action::Home { 0 } else { last };
            }
            // As in mc, the cursor moves on even from `..`, which cannot be marked.
            Action::Mark => {
                self.toggle_mark();
                self.cursor = (self.cursor + 1).min(last);
            }
            Action::MarkUp => {
                self.toggle_mark();
                self.cursor = self.cursor.saturating_sub(1);
            }
            Action::ScrollLeft => self.shift = self.shift.saturating_sub(1),
            Action::ScrollRight => self.shift_names(self.shift + 1),
            Action::ScrollHalfLeft => self.shift = self.shift.saturating_sub(self.names.1 / 2),
            Action::ScrollHalfRight => self.shift_names(self.shift + self.names.1 / 2),
            Action::ScrollStart => self.shift = 0,
            Action::ScrollEnd => {
                self.shift = self
                    .row(self.cursor)
                    .map_or(0, |row| self.shift_to_end(row));
            }
            Action::InvertMarks => self.invert_marks(),
            Action::Enter => return self.row_destination().map(|to| self.go(to)),
            Action::Parent => return self.parent_destination().map(|to| self.go(to)),
            Action::SortByName => self.sort_by(SortKey::Name),
            Action::SortByExtension => self.sort_by(SortKey::Extension),
            Action::SortByTime => self.sort_by(SortKey::Time),
            Action::SortBySize => self.sort_by(SortKey::Size),
            Action::Reload => return Some(self.go(self.here())),
            _ => {}
        }
        None
    }

    /// Scrolls `pages` windows down or up at once, as vim's Ctrl+f and Ctrl+b with a count.
    pub(crate) fn page_by(&mut self, down: bool, pages: usize) {
        let rows = self.rows();
        let page = self.page.max(1);
        self.window.page(down, pages, &mut self.cursor, rows, page);
    }

    /// The action that sorts as the panel is sorted, for the mark in the pull-down menu.
    pub(crate) fn sort_action(&self) -> Action {
        match self.sort.key {
            SortKey::Name => Action::SortByName,
            SortKey::Extension => Action::SortByExtension,
            SortKey::Time => Action::SortByTime,
            SortKey::Size => Action::SortBySize,
        }
    }

    fn sort_by(&mut self, key: SortKey) {
        self.sort = self.sort.by(key);
        self.rearrange();
    }

    /// What the panel is sorted by, and whether the order is reversed.
    pub(crate) fn sort_order(&self) -> (SortKey, bool) {
        (self.sort.key, self.sort.descending)
    }

    /// Sorts by `key`, reversed if `descending`.
    pub(crate) fn set_sort_order(&mut self, key: SortKey, descending: bool) {
        self.sort = Sort { key, descending };
        self.rearrange();
    }

    /// Goes to `destination`; returns the listing to request. Where the panel was goes to the
    /// jump list, if it is another location.
    pub(crate) fn go(&mut self, destination: Destination) -> ListRequest {
        let jumped = (destination.location != self.location).then(|| {
            self.remember();
            self.here()
        });
        let request = self.open(destination.location, destination.focus);
        if let Some(pending) = &mut self.pending {
            pending.jumped = jumped;
        }
        request
    }

    /// Goes to `destination` as [`go`](Self::go) does, on the way up to the nearest directory
    /// that is there: the failure it may return says so.
    pub(crate) fn climb(&mut self, destination: Destination) -> ListRequest {
        let request = self.go(destination);
        if let Some(pending) = &mut self.pending {
            pending.climbing = true;
        }
        request
    }

    /// Puts the cursor on row `number`, counted from 1, as vim's `G` with a count does; where
    /// it was goes to the jump list.
    pub(crate) fn go_to_row(&mut self, number: usize) {
        self.remember();
        self.cursor = number.saturating_sub(1).min(self.rows().saturating_sub(1));
    }

    /// This location, with the cursor on the row it is on.
    pub(crate) fn here(&self) -> Destination {
        Destination {
            location: self.location.clone(),
            focus: self.focus(),
        }
    }

    /// Where the panel is going, as a workspace saves it: the location it waits for, else the
    /// one it shows; and the name of the entry the cursor is on, or goes to, there.
    pub(crate) fn heading(&self) -> (Location, Option<Vec<u8>>) {
        let (location, focus) = match &self.pending {
            Some(pending) => (pending.location.clone(), pending.focus.clone()),
            None => (self.location.clone(), self.focus()),
        };
        let name = match focus {
            Focus::Name(name) | Focus::Near { name, .. } => Some(name),
            Focus::First => None,
        };
        (location, name)
    }

    /// `location`, with the cursor on the directory the panel shows if `location` is its
    /// parent, as `..` puts it; else on its first row.
    pub(crate) fn destination(&self, location: Location) -> Destination {
        match self.parent_destination() {
            Some(parent) if parent.location == location => parent,
            _ => Destination::to(location),
        }
    }

    /// What the cursor is on.
    fn focus(&self) -> Focus {
        match self.row(self.cursor) {
            Some(Row::Entry(entry)) => Focus::Near {
                name: entry.name.clone(),
                row: self.cursor,
            },
            Some(Row::Parent) | None => Focus::First,
        }
    }

    /// What Ctrl+w f opens in the other panel, as mc's Alt+o: the directory under the cursor,
    /// or, for a file, the parent directory with the cursor on this one. The cursor moves on to
    /// the next row.
    pub(crate) fn for_other_panel(&mut self) -> Option<Destination> {
        let destination = match self.row(self.cursor)? {
            Row::Entry(entry) if !entry.is_dir_like() => self.parent_destination(),
            _ => self.row_destination(),
        };
        self.cursor = (self.cursor + 1).min(self.rows().saturating_sub(1));
        destination
    }

    /// Where Enter on the row under the cursor leads: into a directory, or up from `..`;
    /// nowhere from a file.
    fn row_destination(&self) -> Option<Destination> {
        let location = match self.row(self.cursor)? {
            Row::Parent => return self.parent_destination(),
            Row::Entry(entry) if entry.is_dir_like() => child(&self.location, &entry.name),
            Row::Entry(_) => return None,
        };
        Some(Destination {
            location,
            focus: Focus::First,
        })
    }

    /// The parent, with the cursor on the directory this is; `None` in a `/`.
    fn parent_destination(&self) -> Option<Destination> {
        Destination::parent_of(&self.location)
    }

    /// Whether the first row is `..`: everywhere but in a `/`.
    fn has_parent(&self) -> bool {
        self.location.has_parent()
    }

    /// The rows: `..`, if there is one, and the entries shown.
    fn rows(&self) -> usize {
        usize::from(self.has_parent()) + self.shown.len()
    }

    fn row(&self, index: usize) -> Option<Row<'_>> {
        let Listing::Dir(entries) = &self.listing;
        let index = if self.has_parent() {
            match index.checked_sub(1) {
                None => return Some(Row::Parent),
                Some(index) => index,
            }
        } else {
            index
        };
        entries.get(*self.shown.get(index)?).map(Row::Entry)
    }
}

/// A row's name, as its row shows it: the icon or marker in front, and the name.
fn shown_name(row: Row<'_>, decor: Decor) -> [String; 2] {
    match row {
        Row::Parent => [decor.parent().to_owned(), "..".to_owned()],
        Row::Entry(entry) => [decor.entry(entry), cells::sanitize(&entry.name)],
    }
}

/// `bytes` with its unit, as [`cells::short_size`] writes sizes on the frame.
pub(crate) fn short_size(bytes: u64) -> String {
    // Sizes stay far below 2^52, where `f64` would round them.
    #[allow(clippy::cast_precision_loss)]
    cells::short_size(bytes as f64, &noc_locale::current())
}

/// Sorts a directory: directories first, then by `sort`; ties by name, then by the bytes of
/// the name. Names and extensions compare as the language of `natural` orders them, with the
/// numbers in them by value; without it, by their characters, ignoring case.
fn sort(entries: &mut Vec<DirEntry>, sort: Sort, natural: Option<&Formats>) {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Value {
        Name,
        Extension(Vec<u8>),
        Time(Option<SystemTime>),
        Size(u64),
    }
    // Keys that order as the text does: the collator's, or the text in lower case.
    let key = |text: &str| match natural {
        Some(formats) => formats.sort_key(text),
        None => text.to_lowercase().into_bytes(),
    };
    let mut keyed: Vec<(bool, Value, Vec<u8>, DirEntry)> = std::mem::take(entries)
        .into_iter()
        .map(|entry| {
            let name = String::from_utf8_lossy(&entry.name);
            let value = match sort.key {
                SortKey::Name => Value::Name,
                SortKey::Extension => Value::Extension(key(extension(&name))),
                SortKey::Time => Value::Time(entry.metadata.modified),
                SortKey::Size => Value::Size(entry.metadata.size.unwrap_or(0)),
            };
            (!entry.is_dir_like(), value, key(&name), entry)
        })
        .collect();
    keyed.sort_by(|a, b| {
        let order = (&a.1, &a.2, &a.3.name).cmp(&(&b.1, &b.2, &b.3.name));
        a.0.cmp(&b.0).then(if sort.descending {
            order.reverse()
        } else {
            order
        })
    });
    *entries = keyed.into_iter().map(|(.., entry)| entry).collect();
}

/// What follows the last dot of a name, unless the dot starts it.
fn extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot + 1..],
        _ => "",
    }
}

/// The entry `name` in `location`; `name` may be a path, relative or absolute.
pub(crate) fn child(location: &Location, name: &[u8]) -> Location {
    match location {
        Location::Local(path) => Location::Local(path.join(OsStr::from_bytes(name))),
        Location::Remote { host, path } => Location::Remote {
            host: host.clone(),
            path: path.join(name),
        },
    }
}

/// Where `text`, typed in a dialog, points from `dir`: a name or a relative path, an absolute
/// path, or `~` or `~/…` for the home directory, `home` here and the remote one on a host;
/// `\~` stands for a name that starts with `~`, as in mc. Trailing slashes are dropped. It
/// stays on the host of `dir`.
pub(crate) fn resolve(dir: &Location, home: &Path, text: &str) -> Location {
    let trimmed = text.trim_end_matches('/');
    let text = if trimmed.is_empty() { text } else { trimmed };
    let home_relative = match text.strip_prefix('~') {
        Some("") => Some(""),
        Some(rest) => rest.strip_prefix('/'),
        None => None,
    };
    let text = text.strip_prefix("\\~").map_or(text, |_| &text[1..]);
    match (dir, home_relative) {
        (Location::Local(_), Some(rest)) => Location::Local(home.join(rest)),
        (Location::Remote { host, .. }, Some(rest)) => Location::Remote {
            host: host.clone(),
            // Relative remote paths start at the remote home directory.
            path: RemotePath::from(rest),
        },
        (location, None) => child(location, text.as_bytes()),
    }
}

/// A location for the title and messages: a local path, `host:path`, or only `host` for the
/// remote home directory.
pub(crate) fn location_text(location: &Location) -> String {
    match location {
        Location::Local(path) => cells::sanitize(path.as_os_str().as_bytes()),
        Location::Remote { host, path } => {
            let mut text = host.as_bytes().to_vec();
            if !path.as_bytes().is_empty() {
                text.push(b':');
                text.extend_from_slice(path.as_bytes());
            }
            cells::sanitize(&text)
        }
    }
}

/// A local directory as titles show it: with `~` for the home directory `home`.
pub(crate) fn home_text(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        // A home directory of `/` would put `~` before every path.
        _ if home.parent().is_none() => cells::sanitize(path.as_os_str().as_bytes()),
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", cells::sanitize(rest.as_os_str().as_bytes())),
        Err(_) => cells::sanitize(path.as_os_str().as_bytes()),
    }
}

/// Characters of `name` that renaming selects: a directory's whole name; a file's name
/// without its last extension, unless that would leave nothing, as for `.profile`, or the name
/// ends with the dot.
pub(crate) fn stem_chars(name: &str, dir: bool) -> usize {
    let whole = name.chars().count();
    if dir {
        return whole;
    }
    match name.rfind('.') {
        Some(dot) if !name[..dot].trim_start_matches('.').is_empty() && dot + 1 < name.len() => {
            name[..dot].chars().count()
        }
        _ => whole,
    }
}

/// The text of `field` as it would be with `c` typed at its cursor.
fn typed_at_cursor(field: &Field, c: char) -> String {
    let at = field.editor().cursor();
    let mut text: String = field.text().chars().take(at).collect();
    text.push(c);
    text.extend(field.text().chars().skip(at));
    text
}
