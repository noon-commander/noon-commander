//! A panel that lists a local or remote directory.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::ops::RangeInclusive;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::tz::TimeZone;
use noc_config::Wheel;
use noc_scroll::Motion;
use noc_scroll::scrollbar::{self, Bar};
use noc_vfs::{DirEntry, Location, RemotePath, Space};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use super::cells::{self, Align, Cut, MTIME_WIDTH, Marks};
use super::decor::{self, Decor};
use super::dialog::Field;
use super::fuzzy::Fuzzy;
use super::glyphs;
use super::keymap::Action;
use super::mouse::Scroll;
use super::scroll::Window;
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the size column, as in mc.
const SIZE_WIDTH: usize = 7;
/// Jumps the jump list of a panel keeps, as vim's keeps 100.
const MAX_JUMPS: usize = 100;

/// Narrow panels drop columns to keep at least this many cells for names.
const MIN_NAME_WIDTH: usize = 8;

/// Cells of the free space and size of a file system, such as `212G`.
const SPACE_WIDTH: usize = 5;

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
}

impl Search {
    fn new(origin: usize, count: usize, back: bool, view: Window) -> Self {
        Self {
            field: Field::plain(""),
            origin,
            from: 2 * origin + 1,
            found: None,
            count: count.max(1),
            back,
            view,
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
    /// Whether it went round past the end, or the start, if it found a match.
    pub(crate) found: Option<bool>,
}

/// What drawing a panel needs from the app.
pub(crate) struct View<'a> {
    /// What the app knows about a host.
    pub(crate) hosts: &'a dyn Fn(&str) -> HostState,
    pub(crate) decor: Decor,
    pub(crate) theme: &'a Theme,
    /// The key that reads a directory again, for a panel on a host that is not connected.
    pub(crate) reload_key: Option<&'a str>,
    /// Turns spinners.
    pub(crate) tick: u64,
    pub(crate) now: SystemTime,
    pub(crate) tz: &'a TimeZone,
}

/// What a directory is sorted by. Directories always come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortKey {
    Name,
    Extension,
    Time,
    Size,
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
        Self {
            location,
            focus: Focus::Name(name.as_bytes().to_vec()),
        }
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
    /// The panel is on a host that is not connected, and shows none of its entries until it
    /// reads the directory again.
    disconnected: bool,
    sort: Sort,
    show_hidden: bool,
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
    /// Quick search, while it runs.
    search: Option<Search>,
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
    error: Option<String>,
    /// Where the lines between columns ran at the last render, so that they can be joined to
    /// a top of the frame drawn again over the panel's.
    column_lines: Vec<u16>,
    /// How far the names are scrolled left, in cells, as vim's `zl` scrolls lines that do not
    /// wrap; from their start again in another directory.
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
            marked: HashSet::new(),
            home,
            cursor: 0,
            window: Window::default(),
            page: 1,
            list: Rect::default(),
            bar: None,
            generation: 0,
            pending: None,
            search: None,
            renaming: None,
            visual: None,
            last_visual: None,
            visual_line: false,
            reselect: None,
            jumps: Vec::new(),
            jump: 0,
            error: None,
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
            marked: HashSet::new(),
            home: self.home.clone(),
            cursor: self.cursor,
            window: self.window,
            page: self.page,
            list: Rect::default(),
            bar: None,
            generation: 0,
            pending: None,
            search: None,
            renaming: None,
            visual: None,
            last_visual: None,
            visual_line: false,
            reselect: None,
            jumps: Vec::new(),
            jump: 0,
            error: None,
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
        self.error = None;
        self.search = None;
        if location != self.location {
            self.renaming = None;
            self.visual = None;
            self.last_visual = None;
        }
        self.pending = Some(Pending {
            generation: self.generation,
            location: location.clone(),
            focus,
            jumped: None,
        });
        ListRequest {
            generation: self.generation,
            location,
            resume: None,
        }
    }

    /// Takes the reply to a [`ListRequest`]; replies to older requests are dropped. On error the
    /// panel keeps showing what it showed and reports `reason` below the listing.
    pub(crate) fn listed(&mut self, generation: u64, result: Result<Listed, String>) {
        let Some(pending) = self
            .pending
            .take_if(|pending| pending.generation == generation)
        else {
            return;
        };
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
            }
            Err(reason) => {
                if pending.jumped.is_some()
                    && self.jump == self.jumps.len()
                    && self.jumps.last() == pending.jumped.as_ref()
                {
                    self.jumps.pop();
                    self.jump = self.jumps.len();
                }
                let shown = match (&pending.location, &self.location) {
                    // The title shows the directory, so a subdirectory needs only its name.
                    (Location::Local(path), Location::Local(current)) => path
                        .strip_prefix(current)
                        .ok()
                        .filter(|relative| !relative.as_os_str().is_empty())
                        .map(|relative| cells::sanitize(relative.as_os_str().as_bytes())),
                    _ => None,
                };
                let shown = shown.unwrap_or_else(|| location_text(&pending.location));
                // The reason may quote ssh, which may quote the server.
                let reason = cells::sanitize(reason.as_bytes());
                self.error = Some(fl!("panel-error", path = shown, reason = reason));
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

    /// Stops waiting for a listing; its reply will be dropped.
    pub(crate) fn cancel(&mut self) {
        self.pending = None;
    }

    /// Whether the panel shows a directory on `host`.
    fn on_host(&self, host: &str) -> bool {
        matches!(&self.location, Location::Remote { host: shown, .. } if shown == host)
    }

    /// Shows none of the entries of `host`, which is not connected, if the panel is on it.
    fn empty(&mut self) {
        self.listing = Listing::Dir(Vec::new());
        self.shown.clear();
        self.marked.clear();
        self.renaming = None;
        self.search = None;
        self.visual = None;
        self.last_visual = None;
        self.space = None;
        self.disconnected = true;
    }

    /// Takes the loss of the connection to `host`: on it, the panel shows none of its entries
    /// and waits to read its directory again, with the cursor where it was, once the host is
    /// back. A panel that only waits for the host waits on.
    pub(crate) fn lose_host(&mut self, host: &str) {
        if self.on_host(host) {
            let here = self.here();
            self.empty();
            self.open(here.location, here.focus);
        }
    }

    /// Takes the end of the connection to `host`: on it, the panel shows none of its entries
    /// until it reads its directory again; a panel that waits for the host stops waiting.
    pub(crate) fn drop_host(&mut self, host: &str) {
        let waits = matches!(
            &self.pending,
            Some(Pending { location: Location::Remote { host: wanted, .. }, .. }) if wanted == host
        );
        if waits {
            self.pending = None;
        }
        if self.on_host(host) {
            self.empty();
        }
    }

    /// Shows or hides names that start with a dot; the cursor stays on its entry if it can.
    pub(crate) fn set_show_hidden(&mut self, show: bool) {
        if self.show_hidden != show {
            self.show_hidden = show;
            self.rearrange();
        }
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

    /// Keeps the cursor on the entry being renamed, or stops renaming it once it is not shown.
    fn keep_renaming(&mut self) {
        let Some(renaming) = &self.renaming else {
            return;
        };
        let row = (0..self.rows()).find(|&index| {
            matches!(self.row(index), Some(Row::Entry(entry)) if entry.name == renaming.name)
        });
        match row {
            Some(row) => self.cursor = row,
            None => self.renaming = None,
        }
    }

    /// Starts renaming the entry under the cursor in its row: the field holds its name, in
    /// Insert mode, with the cursor before the last extension of a file. `false` on `..`,
    /// volumes, and hosts, and for a name that is not UTF-8, which the field could not give
    /// back.
    pub(crate) fn start_rename(&mut self) -> bool {
        let Some(entry) = self.entry_under_cursor() else {
            return false;
        };
        let Ok(text) = std::str::from_utf8(&entry.name) else {
            return false;
        };
        let field = Field::at(text, stem_chars(text, entry.is_dir_like()));
        self.renaming = Some(Renaming {
            name: entry.name.clone(),
            field,
        });
        self.search = None;
        true
    }

    /// Whether an entry is being renamed.
    pub(crate) fn renaming(&self) -> bool {
        self.renaming.is_some()
    }

    /// The field with the new name, while an entry is being renamed.
    pub(crate) fn rename_field(&mut self) -> Option<&mut Field> {
        self.renaming.as_mut().map(|renaming| &mut renaming.field)
    }

    /// The field with the new name, to read, while an entry is being renamed.
    pub(crate) fn rename_field_ref(&self) -> Option<&Field> {
        self.renaming.as_ref().map(|renaming| &renaming.field)
    }

    /// Stops renaming: the name of the entry, and the text of the field.
    pub(crate) fn end_rename(&mut self) -> Option<(Vec<u8>, String)> {
        let renaming = self.renaming.take()?;
        Some((renaming.name, renaming.field.text().to_owned()))
    }

    /// Sorts a directory listing and picks the entries to show; entries that are not shown
    /// lose their marks.
    fn arrange(&mut self) {
        let Listing::Dir(entries) = &mut self.listing;
        sort(entries, self.sort);
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

    /// Marks the entry under the cursor, or unmarks it; `..` cannot be marked.
    fn toggle_mark(&mut self) {
        if let Some(Row::Entry(entry)) = self.row(self.cursor) {
            let name = entry.name.clone();
            if !self.marked.remove(&name) {
                self.marked.insert(name);
            }
        }
    }

    /// Scrolls the names `to` cells left, but no further than vim's `zl` goes: until the last
    /// character of the name under the cursor is at the left edge.
    fn shift_names(&mut self, to: usize) {
        let name = self.cursor_name().unwrap_or_default();
        let last = name
            .chars()
            .next_back()
            .map_or(0, |c| cells::width(&c.to_string()));
        self.shift = to.min(cells::width(&name) - last);
    }

    /// The name under the cursor as its row shows it, after its icon or marker, as the last
    /// render drew them.
    fn cursor_name(&self) -> Option<String> {
        self.row(self.cursor)
            .map(|row| shown_name(row, self.names.0).concat())
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

    /// Marks or unmarks `count` rows from the cursor down, or up, as Space or `K` typed that
    /// many times, as far as the listing goes: the row at its end turns once.
    pub(crate) fn mark_rows(&mut self, down: bool, count: usize) {
        let action = if down { Action::Mark } else { Action::MarkUp };
        for _ in 0..count {
            let at = self.cursor;
            self.handle(action);
            if self.cursor == at {
                break;
            }
        }
    }

    /// Inverts the marks on the shown entries that are not directories, as mc does by default.
    fn invert_marks(&mut self) {
        let Listing::Dir(entries) = &self.listing;
        for &index in &self.shown {
            let entry = &entries[index];
            if !entry.is_dir_like() && !self.marked.remove(&entry.name) {
                self.marked.insert(entry.name.clone());
            }
        }
    }

    /// Marks, or unmarks, the shown entries for which `test` holds.
    pub(crate) fn mark_where(&mut self, mark: bool, test: impl Fn(&DirEntry) -> bool) {
        let Listing::Dir(entries) = &self.listing;
        for &index in &self.shown {
            let entry = &entries[index];
            if !test(entry) {
                continue;
            }
            if mark {
                self.marked.insert(entry.name.clone());
            } else {
                self.marked.remove(&entry.name);
            }
        }
    }

    /// How many entries are marked, and the bytes in the files among them.
    fn marked_total(&self) -> (usize, u64) {
        let Listing::Dir(entries) = &self.listing;
        let bytes = entries
            .iter()
            .filter(|entry| !entry.is_dir_like() && self.marked.contains(&entry.name))
            .filter_map(|entry| entry.metadata.size)
            .sum();
        (self.marked.len(), bytes)
    }

    /// Whether quick search runs.
    pub(crate) fn searching(&self) -> bool {
        self.search.is_some()
    }

    /// What quick search has typed, while it runs.
    pub(crate) fn search_text(&self) -> Option<&str> {
        self.search.as_ref().map(|search| search.field.text())
    }

    /// Starts quick search down the rows, as vim's `/`: to match `count` of what is typed,
    /// from the row after the cursor, round to the top.
    pub(crate) fn search_down(&mut self, count: usize) {
        self.search = Some(Search::new(self.cursor, count, false, self.window));
    }

    /// Starts quick search up the rows, as vim's `?`: to match `count` of what is typed, from
    /// the row before the cursor, round to the bottom.
    pub(crate) fn search_up(&mut self, count: usize) {
        self.search = Some(Search::new(self.cursor, count, true, self.window));
    }

    /// Jumps to the next match down, or the next up, of the search that runs, as vim's Ctrl+g
    /// and Ctrl+t: the count before `/` matches on, from the match the cursor is on. Where the
    /// search goes from moves along, so that Enter stays on this match and an edit searches
    /// on from the match before it; `fuzzy`, the next best or the one before, round.
    pub(crate) fn search_step(&mut self, forward: bool, fuzzy: bool) {
        let Some(search) = &self.search else {
            return;
        };
        let text = search.field.text().to_owned();
        if text.trim().is_empty() {
            return;
        }
        let (count, back) = (search.count, search.back);
        let current = search.found.unwrap_or_else(|| search.start_row());
        let target = if fuzzy {
            self.step_fuzzy(&text, current, forward, count)
        } else {
            self.find_from(&text.to_lowercase(), 2 * current + 1, !forward, count)
                .map(|(row, _)| row)
        };
        let Some(row) = target else {
            return;
        };
        let rows = self.rows();
        let page = self.page.max(1);
        let Some(search) = &mut self.search else {
            return;
        };
        search.found = Some(row);
        // vim keeps the rows on screen at the match for Esc to put back.
        self.window.unsettle();
        self.cursor = row;
        self.window.follow(&mut self.cursor, rows, page);
        search.view = self.window;
        // Fuzzy search ranks the rows: where it goes from stays.
        if fuzzy {
            return;
        }
        // As vim's may_adjust_incsearch_highlighting moves search_start.
        let at = 2 * row + 1;
        let before = if row == 0 { 1 } else { 2 * row };
        search.from = 2 * current + 1;
        if !forward && !back {
            search.from = before;
        } else if forward && back {
            search.from = at + 1;
        }
        if forward && at < search.from {
            search.from = if back { at + 1 } else { before };
        }
    }

    /// Searches for `text`, what an earlier search found, as if it were typed: Ctrl+g with
    /// nothing typed yet, as mc's Ctrl+s Ctrl+s.
    pub(crate) fn search_resume(&mut self, text: &str, fuzzy: bool) {
        let Some(search) = &mut self.search else {
            return;
        };
        let text = if fuzzy {
            text.to_owned()
        } else {
            text.to_lowercase()
        };
        search.field = Field::plain(&text);
        self.search_again_typed(fuzzy);
    }

    /// Moves the cursor `times` matches on, as vim's `3n`, and puts where it was in the jump
    /// list, once, even if the match is on the same row, as vim does; a search that finds
    /// nothing leaves the jumps as they are. Whether it went round past the end, or the
    /// start, if it found one.
    pub(crate) fn search_again(
        &mut self,
        text: &str,
        forward: bool,
        fuzzy: bool,
        times: usize,
    ) -> Option<bool> {
        let here = self.here();
        let (row, round) = if fuzzy {
            let ranked = self.ranked(text);
            let count = ranked.len();
            let at = ranked.iter().position(|&(row, _)| row == self.cursor);
            let steps = times % count.max(1);
            let (index, round) = match at {
                Some(at) if forward => ((at + steps) % count, at + times >= count),
                Some(at) => ((at + count - steps) % count, times > at),
                None => (0, false),
            };
            (ranked.get(index).map(|&(row, _)| row)?, round)
        } else {
            self.find_from(&text.to_lowercase(), 2 * self.cursor + 1, !forward, times)?
        };
        self.push_jump(here);
        self.cursor = row;
        Some(round)
    }

    /// The next fuzzy match from `current` as fzf ranks them, `count` on or back, round; the
    /// best from a row that does not match.
    fn step_fuzzy(&self, text: &str, current: usize, forward: bool, count: usize) -> Option<usize> {
        let ranked = self.ranked(text);
        let total = ranked.len();
        let steps = count % total.max(1);
        let index = match ranked.iter().position(|&(row, _)| row == current) {
            Some(at) if forward => (at + steps) % total,
            Some(at) => (at + total - steps) % total,
            None => 0,
        };
        ranked.get(index).map(|&(row, _)| row)
    }

    /// Types `c` at the cursor of quick search and searches again, as vim's incremental search
    /// does: from where the search goes from to the first name that holds the text,
    /// ignoring case, or, `fuzzy` (`ui.fuzzy_search`), to the best match as fzf ranks them, the
    /// first from there of several as good. While nothing matches, the cursor stays where the
    /// search goes from, and the text stays as typed.
    pub(crate) fn search_type(&mut self, c: char, fuzzy: bool) {
        let Some(search) = &mut self.search else {
            return;
        };
        // Smart case needs the capitals.
        if fuzzy {
            search.field.insert(c);
        } else {
            for c in c.to_lowercase() {
                search.field.insert(c);
            }
        }
        self.search_again_typed(fuzzy);
    }

    /// Edits the text of quick search with a key of the text fields; an edit searches again,
    /// as typing does. Moves of the text cursor leave the panel's where it is.
    pub(crate) fn search_edit(&mut self, action: Action, fuzzy: bool) {
        let Some(search) = &mut self.search else {
            return;
        };
        let before = search.field.text().to_owned();
        search.field.edit(action);
        if search.field.text() != before {
            self.search_again_typed(fuzzy);
        }
    }

    /// Searches for the text typed, from where the search goes from, and puts the rows on
    /// screen back as they were before it, for the cursor to move from there, as vim does.
    fn search_again_typed(&mut self, fuzzy: bool) {
        let Some(search) = &self.search else {
            return;
        };
        let text = search.field.text().to_owned();
        let found = if text.trim().is_empty() {
            None
        } else if fuzzy {
            self.best_fuzzy(&text, search)
        } else {
            self.find_from(&text, search.from, search.back, search.count)
                .map(|(row, _)| row)
        };
        let Some(search) = &mut self.search else {
            return;
        };
        search.found = found;
        self.window.restore(search.view);
        self.window.unsettle();
        self.cursor = found.unwrap_or_else(|| search.start_row());
    }

    /// The best fuzzy match for `text`, the first from where `search` goes from of several as
    /// good; with a count before `/` or `?`, the match of that number, round.
    fn best_fuzzy(&self, text: &str, search: &Search) -> Option<usize> {
        let rows = self.rows().max(1);
        let start = search.start_row() % rows;
        let back = search.back;
        let after = |row: usize| {
            if back {
                (start + rows - 1 - row) % rows
            } else {
                (row + rows - start - 1) % rows
            }
        };
        let mut ranked = self.ranked(text);
        ranked.sort_by_key(|&(row, score)| (std::cmp::Reverse(score), after(row)));
        let count = ranked.len();
        (count > 0).then(|| ranked[(search.count - 1) % count].0)
    }

    /// The row of match `count` of `text` (lower case) from `from`, in half rows: down to the
    /// first name after it that holds the text, round to the top, or, `back`, up to the
    /// first before it, round to the bottom; and whether the search went round.
    fn find_from(
        &self,
        text: &str,
        from: usize,
        back: bool,
        count: usize,
    ) -> Option<(usize, bool)> {
        let rows = self.rows();
        let mut from = from;
        let mut found = None;
        let mut round = false;
        // No more steps than rows: round, they come back to where they were.
        for _ in 0..count.clamp(1, rows.max(1)) {
            let matches = |row: &usize| self.contains(*row, text);
            let next = if back {
                (0..rows)
                    .rev()
                    .filter(|row| 2 * row + 1 < from)
                    .chain((0..rows).rev())
                    .find(matches)
            } else {
                (0..rows)
                    .filter(|row| 2 * row + 1 > from)
                    .chain(0..rows)
                    .find(matches)
            }?;
            let at = 2 * next + 1;
            round |= if back { at >= from } else { at <= from };
            from = at;
            found = Some(next);
        }
        found.map(|row| (row, round))
    }

    /// Ends quick search and puts the cursor back on the row it started from, and the rows on
    /// screen as they were, as vim's Esc does.
    pub(crate) fn abort_search(&mut self) {
        if let Some(search) = self.search.take() {
            self.cursor = search.origin.min(self.rows().saturating_sub(1));
            self.window.restore(search.view);
        }
    }

    /// Ends quick search as Enter does in vim: it searches once more from where it goes from,
    /// for the match of the count, and goes there, or stays there if nothing matches. Where it
    /// started goes to the jump list, even if the match is on that row, as vim's; `fuzzy`, the
    /// cursor stays on the match. What it searched for and found, unless nothing was typed.
    pub(crate) fn end_search(&mut self, fuzzy: bool) -> Option<SearchEnd> {
        let search = self.search.take()?;
        let text = search.field.text().to_owned();
        if text.trim().is_empty() {
            return None;
        }
        let start = search.start_row();
        let found = if fuzzy {
            search.found.map(|row| (row, false))
        } else {
            // As vim's Enter: the rows as they were, made right for where it searches from,
            // then for the match.
            self.window.restore(search.view);
            let (rows, page) = (self.rows(), self.page.max(1));
            let mut at = start;
            self.window.follow(&mut at, rows, page);
            self.find_from(&text, search.from, search.back, search.count)
        };
        let jumped = match found {
            Some((row, _)) if fuzzy => row != search.origin,
            Some(_) => true,
            // As vim's, which compares the positions with their columns: after Ctrl+g or Ctrl+t,
            // where it searches from may be within the row it started on.
            None => search.from != 2 * search.origin + 1,
        };
        if jumped {
            let origin = self.spot(search.origin);
            self.push_jump(origin);
        }
        self.cursor = found.map_or(start, |(row, _)| row);
        Some(SearchEnd {
            text,
            back: search.back,
            found: found.map(|(_, round)| round),
        })
    }

    /// Whether the name of row `index` holds `text` (lower case) anywhere, ignoring case, as the
    /// filters of the lists match without fzf.
    fn contains(&self, index: usize, text: &str) -> bool {
        self.search_name(index)
            .is_some_and(|name| name.to_lowercase().contains(text))
    }

    /// The rows whose names match `text` as fzf matches it, with their scores, best first;
    /// rows as good keep their order. `..` never matches.
    fn ranked(&self, text: &str) -> Vec<(usize, u32)> {
        let mut fuzzy = Fuzzy::names(text);
        let mut ranked: Vec<(usize, u32)> = (0..self.rows())
            .filter_map(|index| Some((index, fuzzy.score(&self.search_name(index)?)?)))
            .collect();
        ranked.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
        ranked
    }

    /// The name that quick search finds a row by: the name it shows.
    fn search_name(&self, index: usize) -> Option<String> {
        match self.row(index)? {
            Row::Entry(entry) => Some(entry.display_name().into_owned()),
            Row::Parent => None,
        }
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

    /// Where `text`, typed in a dialog, points from this directory: a name or a relative path,
    /// an absolute path, or `~` or `~/…` for the home directory (the remote one on a host);
    /// `\~` stands for a name that starts with `~`, as in mc. Trailing slashes are dropped.
    pub(crate) fn resolve(&self, text: &str) -> Location {
        let trimmed = text.trim_end_matches('/');
        let text = if trimmed.is_empty() { text } else { trimmed };
        let home_relative = match text.strip_prefix('~') {
            Some("") => Some(""),
            Some(rest) => rest.strip_prefix('/'),
            None => None,
        };
        let text = text.strip_prefix("\\~").map_or(text, |_| &text[1..]);
        match (&self.location, home_relative) {
            (Location::Local(_), Some(rest)) => Location::Local(self.home.join(rest)),
            (Location::Remote { host, .. }, Some(rest)) => Location::Remote {
                host: host.clone(),
                // Relative remote paths start at the remote home directory.
                path: RemotePath::from(rest),
            },
            (location, None) => child(location, text.as_bytes()),
        }
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
                let width = cells::width(&self.cursor_name().unwrap_or_default());
                self.shift = width.saturating_sub(self.names.1);
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

    /// Puts the cursor on row `number`, counted from 1, as vim's `G` with a count does; where
    /// it was goes to the jump list.
    pub(crate) fn go_to_row(&mut self, number: usize) {
        self.remember();
        self.cursor = number.saturating_sub(1).min(self.rows().saturating_sub(1));
    }

    /// Puts where the panel is in the jump list, before a jump from here.
    pub(crate) fn remember(&mut self) {
        let here = self.here();
        self.push_jump(here);
    }

    /// Where the cursor would be on `row` of this location.
    fn spot(&self, row: usize) -> Destination {
        Destination {
            location: self.location.clone(),
            focus: self.focus_at(row),
        }
    }

    /// The entry on `row`, or the row itself if it is gone, as a focus.
    fn focus_at(&self, row: usize) -> Focus {
        match self.row(row) {
            Some(Row::Entry(entry)) => Focus::Near {
                name: entry.name.clone(),
                row,
            },
            Some(Row::Parent) | None => Focus::First,
        }
    }

    /// The rows Visual mode holds, by the entries on them, before the rows change: where it
    /// started, and those it selected last.
    fn visual_focus(&self) -> (Option<Focus>, Option<(Focus, Focus)>) {
        let visual = self.visual.map(|row| self.focus_at(row));
        let last = self
            .last_visual
            .map(|(anchor, cursor)| (self.focus_at(anchor), self.focus_at(cursor)));
        (visual, last)
    }

    /// Puts what Visual mode holds back on its entries, after the rows changed, so that it
    /// never selects other entries than those it did.
    fn restore_visual(&mut self, (visual, last): (Option<Focus>, Option<(Focus, Focus)>)) {
        self.visual = visual.map(|focus| self.row_of(&focus));
        self.last_visual =
            last.map(|(anchor, cursor)| (self.row_of(&anchor), self.row_of(&cursor)));
    }

    /// Adds `spot` to the jump list as the newest jump; jumps that Ctrl+o went back over go.
    fn push_jump(&mut self, spot: Destination) {
        // After Ctrl+o, the place it went to stays, as in vim's 'jumpoptions' stack.
        self.jumps.truncate(self.jump.saturating_add(1));
        if self.jumps.last() != Some(&spot) {
            self.jumps.push(spot);
        }
        if self.jumps.len() > MAX_JUMPS {
            self.jumps.remove(0);
        }
        self.jump = self.jumps.len();
    }

    /// Goes back to where the panel was before its last jump, as vim's Ctrl+o, `count` times;
    /// the listing to request, if that is another location. A jump that went nowhere, such as
    /// `G` on the last row, left the place where the panel still is: Ctrl+o goes there first,
    /// moving nothing, as vim's does.
    pub(crate) fn jump_back(&mut self, count: usize) -> Option<ListRequest> {
        let count = count.max(1);
        // As vim's, a count past the oldest jump goes nowhere, and keeps the list as it is.
        if self.jump.min(self.jumps.len()) < count {
            return None;
        }
        // Where the panel is goes in only after a jump, as vim's; later the places stay as
        // they were.
        if self.jump == self.jumps.len() {
            let here = self.here();
            self.jumps.push(here);
            if self.jumps.len() > MAX_JUMPS {
                self.jumps.remove(0);
                self.jump -= 1;
            }
        }
        self.jump -= count;
        self.travel(self.jumps[self.jump].clone())
    }

    /// Goes forward again where Ctrl+o came from, as vim's Ctrl+i, `count` times.
    pub(crate) fn jump_forward(&mut self, count: usize) -> Option<ListRequest> {
        let count = count.max(1);
        // As vim's, a count past the newest jump goes nowhere.
        if self.jump + count >= self.jumps.len() {
            return None;
        }
        self.jump += count;
        self.travel(self.jumps[self.jump].clone())
    }

    /// Goes to `to` without a new jump: within this location at once, else by its listing.
    fn travel(&mut self, to: Destination) -> Option<ListRequest> {
        if to.location == self.location && self.pending.is_none() {
            self.cursor = self.row_of(&to.focus);
            return None;
        }
        Some(self.open(to.location, to.focus))
    }

    /// Whether Visual mode selects rows.
    pub(crate) fn visual(&self) -> bool {
        self.visual.is_some()
    }

    /// Starts Visual mode on the row under the cursor. With a count, it selects as many rows
    /// as the last command of Visual mode took, `count` times, or else `count` rows, as vim's
    /// `3V` does.
    pub(crate) fn start_visual(&mut self, count: Option<usize>, line: bool) {
        self.visual = Some(self.cursor);
        self.visual_line = line;
        let Some(count) = count else {
            return;
        };
        let rows = self.reselect.unwrap_or(1).saturating_mul(count);
        let last = self.rows().saturating_sub(1);
        self.cursor = self.cursor.saturating_add(rows.saturating_sub(1)).min(last);
    }

    /// `v` (or `V`, `line`) in Visual mode: the key that started it ends it, and the other
    /// switches its kind, as vim's.
    pub(crate) fn switch_visual(&mut self, line: bool) {
        if self.visual_line == line {
            self.end_visual();
        } else {
            self.visual_line = line;
        }
    }

    /// Whether Visual mode, or the last one, goes by lines, as vim's `V`.
    pub(crate) fn visual_line(&self) -> bool {
        self.visual_line
    }

    /// Ends Visual mode, and keeps what it selected for `g v`.
    pub(crate) fn end_visual(&mut self) {
        if let Some(anchor) = self.visual.take() {
            self.last_visual = Some((anchor, self.cursor));
        }
    }

    /// Selects again the rows Visual mode selected last, as vim's `g v`.
    pub(crate) fn reselect(&mut self) {
        if let Some((anchor, cursor)) = self.last_visual {
            let last = self.rows().saturating_sub(1);
            self.visual = Some(anchor.min(last));
            self.cursor = cursor.min(last);
        }
    }

    /// Puts the cursor at the other end of what Visual mode selects.
    pub(crate) fn other_end(&mut self) {
        if let Some(anchor) = &mut self.visual {
            std::mem::swap(anchor, &mut self.cursor);
        }
    }

    /// How many entries Visual mode selects, as vim's `showcmd` shows its lines: those the
    /// commands of files act on, so not `..`.
    pub(crate) fn visual_size(&self) -> Option<usize> {
        let rows = self.visual_rows()?;
        Some(
            rows.filter(|&index| matches!(self.row(index), Some(Row::Entry(_))))
                .count(),
        )
    }

    /// The rows Visual mode selects.
    fn visual_rows(&self) -> Option<RangeInclusive<usize>> {
        let anchor = self.visual?.min(self.rows().saturating_sub(1));
        Some(anchor.min(self.cursor)..=anchor.max(self.cursor))
    }

    /// Marks the entries Visual mode selects, and ends it. With `only`, for a command that acts
    /// on them, they are the only marked ones, and the cursor goes to the first.
    pub(crate) fn mark_visual(&mut self, only: bool) {
        let Some(rows) = self.visual_rows() else {
            return;
        };
        if only {
            self.marked.clear();
        }
        self.reselect = Some(rows.clone().count());
        let first = *rows.start();
        for index in rows {
            if let Some(Row::Entry(entry)) = self.row(index) {
                self.marked.insert(entry.name.clone());
            }
        }
        self.end_visual();
        // As after an operator of vim's Visual mode, the cursor goes to the first row.
        if only {
            self.cursor = first;
        }
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
        let parent = self.location.parent()?;
        let focus = match &self.location {
            Location::Local(path) => path
                .file_name()
                .map_or(Focus::First, |name| Focus::Name(name.as_bytes().to_vec())),
            Location::Remote { path, .. } => path
                .file_name()
                .map_or(Focus::First, |name| Focus::Name(name.to_vec())),
        };
        Some(Destination {
            location: parent,
            focus,
        })
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

    /// Draws the panel: the location in the frame, column headers, the rows, the size and number
    /// of marked entries on the line below them, and a status line with the name under the
    /// cursor, the loading state, or the last error.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        active: bool,
        view: &View<'_>,
    ) {
        let theme = view.theme;
        let title = match &self.location {
            Location::Local(path) => home_text(path, &self.home),
            Location::Remote { .. } => location_text(&self.location),
        };
        let (title_style, mark) = if active {
            (theme.panel_title_active, theme.truncated_cursor)
        } else {
            (theme.panel_border, theme.truncated)
        };
        // A path that does not fit keeps its last directories.
        let room = usize::from(area.width.saturating_sub(4)).min(cells::width(&title));
        let fit = (Align::Left, Cut::Start);
        let mut title = cells::fitted(&title, room, fit, (title_style, mark), theme.glyphs.marks);
        title.spans.insert(0, Span::styled(" ", title_style));
        title.spans.push(Span::styled(" ", title_style));
        let block = Block::bordered()
            .border_type(theme.border_type())
            .title(title)
            .style(theme.panel)
            .border_style(theme.panel_border);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.column_lines.clear();
        self.list = Rect::default();
        self.bar = None;
        if inner.height < 3 || inner.width < 2 {
            return;
        }
        self.render_space(frame, area, theme);
        let width = usize::from(inner.width);
        let list_height = usize::from(inner.height - 3);
        self.page = list_height;
        self.list = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 3);
        self.scroll(list_height);
        let bar = Rect::new(area.right() - 1, self.list.y, 1, self.list.height);
        let position = (self.rows(), list_height, self.window.top());
        let (style, arrows) = (theme.panel_border, theme.glyphs.scroll_bar);
        self.bar = scrollbar::render(frame, bar, position, style, arrows);

        let columns = DirColumns::for_width(width);
        self.names = (view.decor, columns.name);
        let line = |y: u16| Rect::new(inner.x, y, inner.width, 1);
        let header = Line::styled(columns.header(self.sort, theme.glyphs.marks), theme.header);
        frame.render_widget(header, line(inner.y));
        let visual = self.visual_rows();
        for (screen_row, index) in (self.window.top()..self.rows())
            .take(list_height)
            .enumerate()
        {
            let Some(row) = self.row(index) else { break };
            let marked = matches!(row, Row::Entry(entry) if self.marked.contains(&entry.name));
            let cursor = (index == self.cursor).then_some(active);
            let selected = visual.as_ref().is_some_and(|rows| rows.contains(&index));
            let text = row_style(
                columns.row(row, self.shift, view),
                cursor,
                (marked, selected),
                theme,
            );
            let y = inner.y + 1 + u16::try_from(screen_row).unwrap_or(u16::MAX);
            frame.render_widget(text, line(y));
        }

        self.render_separator(frame, area, inner.bottom() - 2, theme);
        self.column_lines = columns
            .lines()
            .into_iter()
            .filter_map(|offset| u16::try_from(offset).ok())
            .filter(|offset| *offset < inner.width)
            .map(|offset| inner.x + offset)
            .collect();
        self.render_column_lines(frame, area, inner, theme);
        self.render_rename(frame, inner, list_height, active, view);
        self.render_status(frame, line(inner.bottom() - 1), active, view);
    }

    /// The status line, at `area`: the name being renamed, quick search with its cursor in the
    /// active panel, the last error, the listing or host it waits for, the host it
    /// is not connected to, or the name under the cursor.
    fn render_status(&self, frame: &mut Frame<'_>, area: Rect, active: bool, view: &View<'_>) {
        let theme = view.theme;
        let width = usize::from(area.width);
        let mut status_style = Style::new();
        // A host the panel waits for turns a spinner in front, in the color of its status.
        let mut waiting = None;
        let status = if let Some(renaming) = &self.renaming {
            fl!("panel-rename", name = cells::sanitize(&renaming.name))
        } else if let Some(search) = &self.search {
            let field = &search.field;
            status_style = theme.quick_search;
            let prompt = |text: String| {
                if search.back {
                    fl!("panel-search-back", text = text)
                } else {
                    fl!("panel-search", text = text)
                }
            };
            let prefix = cells::width(&prompt(String::new()));
            let (shown, column) = field.visible(width.saturating_sub(prefix));
            if active {
                let column = u16::try_from(prefix + column).unwrap_or(u16::MAX);
                let x = area.x.saturating_add(column).min(area.right() - 1);
                frame.set_cursor_position((x, area.y));
            }
            prompt(shown)
        } else if let Some(error) = &self.error {
            error.clone()
        } else if let Some(pending) = &self.pending {
            match &pending.location {
                Location::Remote { host, .. } => {
                    let status = (view.hosts)(host).status;
                    let host = cells::sanitize(host.as_bytes());
                    match status {
                        HostStatus::Connecting => {
                            waiting = Some(status);
                            fl!("panel-connecting", host = host)
                        }
                        HostStatus::Stalled => {
                            waiting = Some(status);
                            fl!("panel-host-waiting", host = host)
                        }
                        HostStatus::Idle | HostStatus::Connected | HostStatus::Failed => {
                            fl!("panel-loading")
                        }
                    }
                }
                Location::Local(_) => fl!("panel-loading"),
            }
        } else if self.disconnected {
            let host = match &self.location {
                Location::Remote { host, .. } => cells::sanitize(host.as_bytes()),
                Location::Local(_) => String::new(),
            };
            match view.reload_key {
                Some(key) => fl!("panel-disconnected-key", host = host, key = key),
                None => fl!("panel-disconnected", host = host),
            }
        } else {
            match self.row(self.cursor) {
                Some(Row::Parent) => "..".to_owned(),
                Some(Row::Entry(entry)) => cells::sanitize(&entry.name),
                None => String::new(),
            }
        };
        let status = match waiting {
            Some(waiting) => {
                let spinner = format!("{} ", decor::spinner(view.tick));
                let status = cells::fit(
                    &status,
                    width.saturating_sub(2),
                    Align::Left,
                    theme.glyphs.marks,
                );
                Line::from(vec![
                    Span::styled(spinner, theme.host_status(waiting)),
                    Span::raw(status),
                ])
            }
            None => Line::styled(
                cells::fit(&status, width, Align::Left, theme.glyphs.marks),
                status_style,
            ),
        };
        frame.render_widget(status, area);
    }

    /// The field of the entry being renamed, over its row from the name on, in the colors of a
    /// dialog's text field, with the selection in reverse.
    fn render_rename(
        &self,
        frame: &mut Frame<'_>,
        inner: Rect,
        list_height: usize,
        active: bool,
        view: &View<'_>,
    ) {
        let Some(renaming) = &self.renaming else {
            return;
        };
        let Some(Row::Entry(entry)) = self.row(self.cursor) else {
            return;
        };
        let Some(screen_row) = self
            .cursor
            .checked_sub(self.window.top())
            .filter(|&row| row < list_height)
        else {
            return;
        };
        let prefix = cells::width(&view.decor.entry(entry));
        let prefix = u16::try_from(prefix).unwrap_or(u16::MAX);
        if prefix.saturating_add(1) >= inner.width {
            return;
        }
        let y = inner.y + 1 + u16::try_from(screen_row).unwrap_or(u16::MAX);
        let area = Rect::new(inner.x + prefix, y, inner.width - prefix, 1);
        let theme = view.theme;
        let ([before, selected, after], column) =
            renaming.field.visible_parts(usize::from(area.width));
        let line = Line::from(vec![
            Span::raw(before),
            Span::styled(selected, selection(theme.dialog_input)),
            Span::raw(after),
        ])
        .style(theme.dialog_input);
        frame.render_widget(Clear, area);
        frame.render_widget(line, area);
        if active {
            let column = u16::try_from(column).unwrap_or(u16::MAX);
            frame.set_cursor_position((area.x.saturating_add(column).min(area.right() - 1), y));
        }
    }

    /// The line between the listing and the status line, across the panel's frame, with the
    /// total of the marked entries in the middle.
    fn render_separator(&self, frame: &mut Frame<'_>, area: Rect, y: u16, theme: &Theme) {
        let inside = area.width.saturating_sub(2);
        let (left, right) = theme.tees();
        let separator = format!("{left}{}{right}", glyphs::LINE.repeat(usize::from(inside)));
        frame.render_widget(
            Line::styled(separator, theme.panel_border),
            Rect::new(area.x, y, area.width, 1),
        );
        let (count, bytes) = self.marked_total();
        if count == 0 {
            return;
        }
        let total = fl!("panel-marked", size = cells::grouped(bytes), count = count);
        let mut total = format!(" {total} ");
        if cells::width(&total) > usize::from(inside) {
            total = cells::fit(&total, usize::from(inside), Align::Left, theme.glyphs.marks);
        }
        let total_width = u16::try_from(cells::width(&total)).unwrap_or(u16::MAX);
        let x = area.x + area.width.saturating_sub(total_width) / 2;
        frame.render_widget(
            // An underline would run along the frame.
            Line::styled(total, theme.marked.not_underlined()),
            Rect::new(x, y, total_width.min(inside), 1),
        );
    }

    /// The lines between columns, down from the header to the line above the status line, in
    /// the color of the frame on the background of the rows they cross, so that the cursor and
    /// marks run across them; never underlined, which would cut them. Joined to the frame where
    /// its line runs, not where the title or the total of the marked entries cuts into it.
    fn render_column_lines(&self, frame: &mut Frame<'_>, area: Rect, inner: Rect, theme: &Theme) {
        let color = theme.panel_border.fg.unwrap_or(Color::Reset);
        let (_, bottom) = theme.column_tees();
        let buffer = frame.buffer_mut();
        for &x in &self.column_lines {
            for y in inner.y..inner.bottom() - 2 {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_char(glyphs::COLUMN).set_fg(color);
                    cell.modifier.remove(Modifier::UNDERLINED | Modifier::BOLD);
                }
            }
            if let Some(cell) = buffer.cell_mut((x, inner.bottom() - 2))
                && cell.symbol() == glyphs::LINE
            {
                cell.set_char(bottom);
            }
        }
        self.join_columns(frame, area, theme);
    }

    /// Joins the lines between columns to the top of the frame, `area`, drawn again over the
    /// panel's, as the tabs do; where it shows a title or tabs, the line stops short of it.
    pub(crate) fn join_columns(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let (top, _) = theme.column_tees();
        let horizontal = theme.lines().horizontal.to_string();
        let buffer = frame.buffer_mut();
        for &x in &self.column_lines {
            if let Some(cell) = buffer.cell_mut((x, area.y))
                && [horizontal.as_str(), glyphs::LINE].contains(&cell.symbol())
            {
                cell.set_char(top);
            }
        }
    }

    /// The free space and size of the directory's file system, on the bottom of the frame at
    /// the right, as in mc; left out when unknown or when it does not fit.
    fn render_space(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        // Pseudo file systems such as devfs report nothing.
        let Some(space) = self.space.filter(|space| space.total > 0) else {
            return;
        };
        let percent = u128::from(space.available.min(space.total)) * 100 / u128::from(space.total);
        let text = fl!(
            "panel-space",
            free = cells::size(space.available, SPACE_WIDTH),
            total = cells::size(space.total, SPACE_WIDTH),
            percent = u64::try_from(percent).unwrap_or(100)
        );
        let text = format!(" {text} ");
        let width = u16::try_from(cells::width(&text)).unwrap_or(u16::MAX);
        // A corner and a line on either side.
        if width.saturating_add(4) > area.width {
            return;
        }
        let x = area.right() - 2 - width;
        frame.render_widget(
            Line::styled(text, theme.panel),
            Rect::new(x, area.bottom() - 1, width, 1),
        );
    }

    /// The row at `at` on screen at the last render, if there is one there.
    fn row_at(&self, at: Position) -> Option<usize> {
        if !self.list.contains(at) {
            return None;
        }
        let row = self.window.top() + usize::from(at.y - self.list.y);
        (row < self.rows()).then_some(row)
    }

    /// Puts the cursor on the row at `at`, as a click does; whether there is one there.
    pub(crate) fn click(&mut self, at: Position) -> bool {
        let row = self.row_at(at);
        if let Some(row) = row {
            self.cursor = row;
        }
        row.is_some()
    }

    /// Puts the cursor on the row at `at` and marks or unmarks it, as a right click does in
    /// Far; the cursor stays there.
    pub(crate) fn mark_at(&mut self, at: Position) {
        if self.click(at) {
            self.toggle_mark();
        }
    }

    /// Scrolls by `step` of the mouse wheel, down or up, as vim's Ctrl+e and Ctrl+y: the cursor
    /// moves only to stay on screen, with its margin. Where the rows cannot scroll that way, as
    /// when they all fit, the cursor moves as far instead, as in mc and Far.
    pub(crate) fn wheel(&mut self, step: Wheel, down: bool) {
        let page = self.page.max(1);
        let rows = match step {
            Wheel::Lines(lines) => usize::from(lines),
            Wheel::Page => page,
        };
        let count = self.rows();
        let top = self.window.top();
        self.window
            .scroll(down, rows, &mut self.cursor, count, page);
        if self.window.top() == top {
            self.cursor = if down {
                self.cursor
                    .saturating_add(rows)
                    .min(count.saturating_sub(1))
            } else {
                self.cursor.saturating_sub(rows)
            };
        }
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// Scrolls as a press on the scroll bar asks: its arrows as a row of the wheel, its track
    /// a page as Ctrl+b and Ctrl+f, and its thumb to where it was dragged; the cursor moves
    /// only to stay on screen, or as Ctrl+f moves it.
    pub(crate) fn scroll_bar(&mut self, scroll: Scroll) {
        let rows = self.rows();
        let page = self.page.max(1);
        match scroll {
            Scroll::By(Motion::Up | Motion::Down) => {
                let down = scroll == Scroll::By(Motion::Down);
                self.window.scroll(down, 1, &mut self.cursor, rows, page);
            }
            Scroll::By(Motion::PageUp) => self.page_by(false, 1),
            Scroll::By(Motion::PageDown) => self.page_by(true, 1),
            Scroll::By(_) => {}
            Scroll::To(top) => self.window.scroll_to(top, &mut self.cursor, rows, page),
        }
    }

    /// Keeps the cursor within the rows and on screen, as vim's window does.
    fn scroll(&mut self, height: usize) {
        let rows = self.rows();
        self.window.follow(&mut self.cursor, rows, height);
    }
}

impl DirColumns {
    /// Where the lines between columns run, in cells from the left of the rows; the rows leave
    /// a blank there for them.
    fn lines(self) -> Vec<usize> {
        let mut lines = Vec::new();
        if self.size {
            lines.push(self.name);
        }
        if self.time {
            lines.push(self.name + 1 + SIZE_WIDTH);
        }
        lines
    }

    /// The column titles; an arrow marks the one the directory is sorted by, and its direction.
    fn header(self, sort: Sort, marks: Marks) -> String {
        let mut titles = [fl!("panel-name"), fl!("panel-size"), fl!("panel-time")];
        let (column, title) = match sort.key {
            SortKey::Name => (0, fl!("panel-name")),
            SortKey::Extension => (0, fl!("panel-name-by-extension")),
            SortKey::Size => (1, fl!("panel-size")),
            SortKey::Time => (2, fl!("panel-time")),
        };
        titles[column] = format!("{}{title}", sort.arrow());
        let [name, size, time] = titles;
        let (name, rest) = self.join(&name, &size, &time, [Align::Center; 3], marks);
        name + &rest
    }

    /// A row, with the name in the color of its kind, as mc highlights files, scrolled `shift`
    /// cells left; and which of its spans are marks of a name cut short.
    fn row(self, row: Row<'_>, shift: usize, view: &View<'_>) -> (Line<'static>, Vec<usize>) {
        const ALIGN: [Align; 3] = [Align::Left, Align::Right, Align::Left];
        let theme = view.theme;
        let [prefix, name] = shown_name(row, view.decor);
        let (size, time, style) = match row {
            Row::Parent => (fl!("panel-up-dir"), String::new(), theme.directory),
            Row::Entry(entry) => {
                let size = if entry.is_dir_like() {
                    fl!("panel-dir")
                } else {
                    entry
                        .metadata
                        .size
                        .map_or_else(String::new, |size| cells::size(size, SIZE_WIDTH))
                };
                let time = cells::mtime(entry.metadata.modified, view.now, view.tz);
                (size, time, theme.entry(entry))
            }
        };
        let (_, rest) = self.join("", &size, &time, ALIGN, view.theme.glyphs.marks);
        let (mut spans, marks) = name_spans(&prefix, &name, (self.name, shift), style, view);
        spans.push(Span::raw(rest));
        (Line::from(spans), marks)
    }
}

/// A row's name, as its row shows it: the icon or marker in front, and the name.
fn shown_name(row: Row<'_>, decor: Decor) -> [String; 2] {
    match row {
        Row::Parent => [decor.parent().to_owned(), "..".to_owned()],
        Row::Entry(entry) => [decor.entry(entry), cells::sanitize(&entry.name)],
    }
}

/// The name cell, `cells` wide, in `style`, with the icon of its `prefix` in a toned-down
/// `style`, and the indexes of the spans that are marks: `…` where a name that does not fit
/// loses its middle, as in mc; scrolled `shift` cells left, `‹` over its first cell and `…`
/// where it goes on past the right one. A cell cut so short that the prefix lost its end, mc's
/// markers, and a scrolled name are drawn whole in `style`.
fn name_spans(
    prefix: &str,
    name: &str,
    (cells, shift): (usize, usize),
    style: Style,
    view: &View<'_>,
) -> (Vec<Span<'static>>, Vec<usize>) {
    let text = format!("{prefix}{name}");
    let shown_marks = view.theme.glyphs.marks;
    let mut spans = Vec::new();
    let mut marks = Vec::new();
    let fitted = if shift > 0 && cells > 0 {
        let (_, shown) = cells::split(&text, shift);
        let (_, rest) = cells::split(&shown, 1);
        marks.push(spans.len());
        spans.push(Span::styled(shown_marks.before, style));
        let fitted = cells::fit_cut(&rest, cells - 1, Align::Left, Cut::End);
        spans.push(Span::styled(fitted.head.clone(), style));
        fitted
    } else {
        let fitted = cells::fit_cut(&text, cells, Align::Left, Cut::Middle);
        match fitted.head.strip_prefix(prefix) {
            Some(rest) if view.decor.icons() && !prefix.is_empty() => {
                spans.push(Span::styled(prefix.to_owned(), Theme::icon(style)));
                spans.push(Span::styled(rest.to_owned(), style));
            }
            _ => spans.push(Span::styled(fitted.head.clone(), style)),
        }
        fitted
    };
    if fitted.cut.is_some() {
        marks.push(spans.len());
        spans.push(Span::styled(fitted.mark(shown_marks), style));
    }
    spans.push(Span::styled(fitted.tail, style));
    (spans, marks)
}

/// Column widths of a directory: the name takes what the size and time columns leave, and
/// narrow panels drop the time, then the size.
#[derive(Debug, Clone, Copy)]
struct DirColumns {
    name: usize,
    size: bool,
    time: bool,
}

impl DirColumns {
    fn for_width(width: usize) -> Self {
        let full = width.saturating_sub(SIZE_WIDTH + MTIME_WIDTH + 2);
        if full >= MIN_NAME_WIDTH {
            return Self {
                name: full,
                size: true,
                time: true,
            };
        }
        let sized = width.saturating_sub(SIZE_WIDTH + 1);
        if sized >= MIN_NAME_WIDTH {
            return Self {
                name: sized,
                size: true,
                time: false,
            };
        }
        Self {
            name: width,
            size: false,
            time: false,
        }
    }

    /// The name cell, and the other cells with the blanks the lines between them go on.
    fn join(
        self,
        name: &str,
        size: &str,
        time: &str,
        align: [Align; 3],
        marks: Marks,
    ) -> (String, String) {
        let mut rest = String::new();
        if self.size {
            rest.push(' ');
            rest.push_str(&cells::fit(size, SIZE_WIDTH, align[1], marks));
        }
        if self.time {
            rest.push(' ');
            rest.push_str(&cells::fit(time, MTIME_WIDTH, align[2], marks));
        }
        (cells::fit(name, self.name, align[0], marks), rest)
    }
}

/// Sorts a directory: directories first, then by `sort`; ties by name ignoring case, then by
/// the bytes of the name.
fn sort(entries: &mut Vec<DirEntry>, sort: Sort) {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Value {
        Name,
        Extension(String),
        Time(Option<SystemTime>),
        Size(u64),
    }
    let mut keyed: Vec<(bool, Value, String, DirEntry)> = std::mem::take(entries)
        .into_iter()
        .map(|entry| {
            let name = String::from_utf8_lossy(&entry.name).to_lowercase();
            let value = match sort.key {
                SortKey::Name => Value::Name,
                SortKey::Extension => Value::Extension(extension(&name).to_owned()),
                SortKey::Time => Value::Time(entry.metadata.modified),
                SortKey::Size => Value::Size(entry.metadata.size.unwrap_or(0)),
            };
            (!entry.is_dir_like(), value, name, entry)
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

/// `style` for selected text: its colors swapped, or else reversed.
fn selection(style: Style) -> Style {
    match (style.fg, style.bg) {
        (Some(fg), Some(bg)) => style.fg(bg).bg(fg),
        _ if style.add_modifier.contains(Modifier::REVERSED) => {
            style.remove_modifier(Modifier::REVERSED)
        }
        _ => style.add_modifier(Modifier::REVERSED),
    }
}

/// Characters of `name` that renaming selects: a directory's whole name; a file's name
/// without its last extension, unless that would leave nothing, as for `.profile`, or the name
/// ends with the dot.
fn stem_chars(name: &str, dir: bool) -> usize {
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

/// `text` as a row under the cursor, `Some(true)` in the active panel, `marked`, and `selected`
/// by Visual mode. The cursor and marks replace the colors of the row, as in mc, and a selection
/// reverses them; the cursor of the inactive panel only puts a background under them. The spans
/// at `marks`, the marks of a name cut short, keep an accent over them.
fn row_style(
    (mut text, marks): (Line<'static>, Vec<usize>),
    cursor: Option<bool>,
    (marked, selected): (bool, bool),
    theme: &Theme,
) -> Line<'static> {
    let style = match (cursor == Some(true), marked, selected) {
        (true, true, _) => Some(theme.marked_cursor),
        (true, false, _) => Some(theme.cursor),
        (false, true, true) => Some(selection(theme.marked)),
        (false, false, true) => Some(selection(theme.panel)),
        (false, true, false) => Some(theme.marked),
        (false, false, false) => None,
    };
    if let Some(style) = style {
        for span in &mut text.spans {
            span.style = Style::new();
        }
        text = text.style(style);
    }
    if cursor == Some(false) {
        text = text.patch_style(theme.cursor_inactive);
    }
    let mark = if cursor == Some(true) || selected {
        theme.truncated_cursor
    } else {
        theme.truncated
    };
    for index in marks {
        if let Some(span) = text.spans.get_mut(index) {
            span.style = span.style.patch(mark);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use noc_vfs::{FileKind, Metadata};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;
    use ratatui::style::Color;

    use super::*;
    use crate::tui::keymap::Mode;

    /// 2023-11-14 22:13:20 UTC.
    const NOW: u64 = 1_700_000_000;

    const HOME: &str = "/home/me";

    fn entry(name: &str, kind: FileKind, size: u64) -> DirEntry {
        DirEntry {
            name: name.as_bytes().to_vec(),
            metadata: Metadata {
                kind,
                size: Some(size),
                permissions: Some(0o644),
                modified: Some(UNIX_EPOCH + Duration::from_secs(NOW - 3600)),
                uid: Some(501),
                gid: Some(20),
            },
            target_kind: None,
        }
    }

    fn link_to_dir(name: &str) -> DirEntry {
        DirEntry {
            target_kind: Some(FileKind::Dir),
            ..entry(name, FileKind::Symlink, 4)
        }
    }

    fn listing() -> Vec<DirEntry> {
        vec![
            entry("zeta.txt", FileKind::File, 12_345),
            entry("Beta", FileKind::Dir, 4096),
            entry(".hidden", FileKind::File, 1),
            link_to_dir("alpha-link"),
            entry("Alpha.md", FileKind::File, 10_000_000),
            entry("bin", FileKind::Dir, 4096),
        ]
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

    /// Answers `request` with `listing` from the location it asked for.
    fn answer(panel: &mut Panel, request: &ListRequest, listing: Listing) {
        let location = request.location.clone();
        panel.listed(
            request.generation,
            Ok(Listed {
                location,
                listing,
                space: None,
            }),
        );
    }

    /// A panel on `location` whose first listing arrived.
    fn loaded_at(location: Location, listing: Listing) -> Panel {
        let (mut panel, request) = Panel::new(location, PathBuf::from(HOME), true);
        answer(&mut panel, &request, listing);
        panel
    }

    fn loaded(path: &str, entries: Vec<DirEntry>) -> Panel {
        loaded_at(local(path), Listing::Dir(entries))
    }

    fn names(panel: &Panel) -> Vec<String> {
        (0..panel.rows())
            .map(|row| match panel.row(row) {
                Some(Row::Parent) => "..".to_owned(),
                Some(Row::Entry(entry)) => entry.display_name().into_owned(),
                None => unreachable!(),
            })
            .collect()
    }

    fn under_cursor(panel: &Panel) -> String {
        names(panel)[panel.cursor].clone()
    }

    /// Draws `panel` with mc's markers, or icons with `decor`, in the `terminal` theme.
    fn render_with(
        panel: &mut Panel,
        (width, height): (u16, u16),
        active: bool,
        hosts: &dyn Fn(&str) -> HostState,
        decor: Decor,
    ) -> Terminal<TestBackend> {
        render_themed(
            panel,
            (width, height),
            active,
            hosts,
            decor,
            &Theme::terminal(),
        )
    }

    fn render_themed(
        panel: &mut Panel,
        (width, height): (u16, u16),
        active: bool,
        hosts: &dyn Fn(&str) -> HostState,
        decor: Decor,
        theme: &Theme,
    ) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let view = View {
            hosts,
            decor,
            theme,
            reload_key: Some("Ctrl+r"),
            tick: 0,
            now: UNIX_EPOCH + Duration::from_secs(NOW),
            tz: &TimeZone::UTC,
        };
        terminal
            .draw(|frame| panel.render(frame, frame.area(), active, &view))
            .unwrap();
        terminal
    }

    fn render(
        panel: &mut Panel,
        width: u16,
        height: u16,
        active: bool,
        hosts: &dyn Fn(&str) -> HostState,
    ) -> TestBackend {
        let terminal = render_with(panel, (width, height), active, hosts, Decor::new(false));
        terminal.backend().clone()
    }

    fn draw(panel: &mut Panel, width: u16, height: u16, active: bool) -> TestBackend {
        render(panel, width, height, active, &|_| HostState::default())
    }

    #[test]
    fn lists_directories_first_then_names() {
        let panel = loaded("/srv", listing());
        assert_eq!(
            names(&panel),
            [
                "..",
                "alpha-link",
                "Beta",
                "bin",
                ".hidden",
                "Alpha.md",
                "zeta.txt"
            ]
        );
        assert_eq!(panel.cursor, 0);
    }

    /// Entries whose names, extensions, times, and sizes all sort differently.
    fn varied() -> Vec<DirEntry> {
        let at = |seconds: u64| Some(UNIX_EPOCH + Duration::from_secs(NOW - 1000 + seconds));
        let file = |name: &str, size: u64, seconds: u64| DirEntry {
            metadata: Metadata {
                modified: at(seconds),
                ..entry(name, FileKind::File, size).metadata
            },
            ..entry(name, FileKind::File, size)
        };
        let dir = |name: &str, seconds: u64| DirEntry {
            metadata: Metadata {
                modified: at(seconds),
                ..entry(name, FileKind::Dir, 4096).metadata
            },
            ..entry(name, FileKind::Dir, 4096)
        };
        vec![
            file("c.txt", 20, 2),
            dir("dir1", 0),
            file("b.md", 10, 1),
            file("Z", 5, 5),
            dir("Adir", 4),
            file("a.txt", 30, 3),
            file(".env", 1, 6),
        ]
    }

    #[test]
    fn sorts_by_name_extension_time_and_size_and_reverses() {
        let mut panel = loaded("/srv", varied());
        let order = |panel: &Panel| names(panel)[1..].join(" ");
        assert_eq!(order(&panel), "Adir dir1 .env a.txt b.md c.txt Z");
        let header = |panel: &mut Panel| draw(panel, 60, 6, true).to_string();
        assert!(header(&mut panel).contains("↑Name"));

        panel.handle(Action::End);
        assert_eq!(panel.handle(Action::SortByExtension), None);
        assert_eq!(order(&panel), "Adir dir1 .env Z b.md a.txt c.txt");
        assert_eq!(under_cursor(&panel), "Z", "the cursor stays on its entry");
        assert!(header(&mut panel).contains("↑Name, by extension"));
        panel.handle(Action::SortByExtension);
        assert_eq!(order(&panel), "dir1 Adir c.txt a.txt b.md Z .env");

        panel.handle(Action::SortByTime);
        assert_eq!(
            order(&panel),
            "Adir dir1 .env Z a.txt c.txt b.md",
            "newest first"
        );
        assert!(header(&mut panel).contains("↓Modify time"));
        panel.handle(Action::SortBySize);
        assert_eq!(
            order(&panel),
            "dir1 Adir a.txt c.txt b.md Z .env",
            "largest first"
        );
        assert!(header(&mut panel).contains("↓Size"));
        panel.handle(Action::SortBySize);
        assert_eq!(order(&panel), "Adir dir1 .env Z b.md c.txt a.txt");
        panel.handle(Action::SortByName);
        assert_eq!(order(&panel), "Adir dir1 .env a.txt b.md c.txt Z");

        // The order holds for the next directory.
        panel.handle(Action::SortByTime);
        let request = panel.handle(Action::Reload).unwrap();
        answer(&mut panel, &request, Listing::Dir(varied()));
        assert_eq!(order(&panel), "Adir dir1 .env Z a.txt c.txt b.md");
    }

    #[test]
    fn hidden_files_come_and_go_and_the_cursor_stays() {
        let (mut panel, request) = Panel::new(local("/srv"), PathBuf::from(HOME), false);
        answer(&mut panel, &request, Listing::Dir(varied()));
        assert_eq!(names(&panel)[1..].join(" "), "Adir dir1 a.txt b.md c.txt Z");
        panel.handle(Action::End);
        panel.set_show_hidden(true);
        assert_eq!(names(&panel).len(), 8);
        assert_eq!(under_cursor(&panel), "Z", "still on its entry");

        panel.handle(Action::Home);
        for _ in 0..3 {
            panel.handle(Action::Down);
        }
        assert_eq!(under_cursor(&panel), ".env");
        panel.set_show_hidden(false);
        assert_eq!(
            under_cursor(&panel),
            "a.txt",
            "the entry is gone; the row stays"
        );
    }

    fn search_files(names: &[&str]) -> Panel {
        let files: Vec<DirEntry> = names
            .iter()
            .map(|name| entry(name, FileKind::File, 1))
            .collect();
        loaded("/srv", files)
    }

    fn typed(panel: &mut Panel, text: &str, fuzzy: bool) {
        for c in text.chars() {
            panel.search_type(c, fuzzy);
        }
    }

    #[test]
    fn quick_search_searches_from_where_it_started_as_vims_incremental_search() {
        let mut panel = search_files(&["bar", "bin", "boot", "cab"]);
        panel.search_down(1);
        assert!(panel.searching());
        typed(&mut panel, "B", false);
        assert_eq!(under_cursor(&panel), "bar", "case does not matter");
        typed(&mut panel, "i", false);
        assert_eq!(under_cursor(&panel), "bin");
        typed(&mut panel, "x", false);
        assert_eq!(panel.search_text(), Some("bix"), "a miss stays typed");
        assert_eq!(panel.cursor, 0, "and the cursor goes back where it started");
        panel.search_edit(Action::Backspace, false);
        assert_eq!(under_cursor(&panel), "bin");
        panel.search_edit(Action::Backspace, false);
        assert_eq!(
            under_cursor(&panel),
            "bar",
            "an edit searches from the start again"
        );
        typed(&mut panel, "o", false);
        let hosts = |_: &str| HostState::default();
        let mut terminal = render_with(&mut panel, (40, 6), true, &hosts, Decor::new(false));
        assert!(terminal.backend().to_string().contains("║Search: bo"));
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            Position::new(11, 4),
            "after the text"
        );
        let end = panel.end_search(false);
        assert_eq!(
            end,
            Some(SearchEnd {
                text: "bo".to_owned(),
                back: false,
                found: Some(false),
            })
        );
        assert!(!panel.searching());
        // Enter on a text that nothing matches stays where the search started.
        panel.search_up(1);
        typed(&mut panel, "z", false);
        assert_eq!(under_cursor(&panel), "boot");
        assert_eq!(panel.end_search(false).map(|end| end.found), Some(None));
        assert_eq!(under_cursor(&panel), "boot");
        // Opening another directory ends a search.
        panel.search_down(1);
        typed(&mut panel, "b", false);
        panel.handle(Action::Reload);
        assert!(!panel.searching());
    }

    #[test]
    fn fuzzy_quick_search_goes_to_the_best_match_and_on_to_the_next_best() {
        let mut panel = search_files(&["Cargo.toml", "config.rs", "cfg-if", "notes", "x.cfg"]);
        panel.search_down(1);
        typed(&mut panel, "cfg", true);
        assert_eq!(under_cursor(&panel), "cfg-if", "together, at the start");
        panel.search_step(true, true);
        assert_eq!(under_cursor(&panel), "x.cfg", "together");
        panel.search_step(true, true);
        assert_eq!(under_cursor(&panel), "config.rs", "apart");
        panel.search_step(true, true);
        assert_eq!(under_cursor(&panel), "cfg-if", "round to the best");
        typed(&mut panel, "q", true);
        assert_eq!(panel.search_text(), Some("cfgq"), "a miss stays typed");
        assert_eq!(panel.cursor, 0);
        panel.search_edit(Action::DeleteToStart, true);
        assert_eq!(panel.search_text(), Some(""));
        typed(&mut panel, "C", true);
        assert_eq!(
            under_cursor(&panel),
            "Cargo.toml",
            "case counts with a capital"
        );
        panel.end_search(true);

        let mut panel = search_files(&["bar", "bat", "baz"]);
        panel.handle(Action::End);
        panel.search_down(1);
        typed(&mut panel, "b", true);
        assert_eq!(
            under_cursor(&panel),
            "bar",
            "as good: the first after the cursor, round, as vim's search goes"
        );
        typed(&mut panel, " ", true);
        assert_eq!(under_cursor(&panel), "bar", "a space stays");
    }

    #[test]
    fn matches_go_both_ways_round_and_an_ended_search_says_what_it_found() {
        let mut panel = search_files(&["bar", "bin", "cat", "boot"]);
        panel.search_down(1);
        typed(&mut panel, "B", false);
        assert_eq!(under_cursor(&panel), "bar");
        panel.search_step(false, false);
        assert_eq!(under_cursor(&panel), "boot", "up, round to the bottom");
        panel.search_step(true, false);
        assert_eq!(under_cursor(&panel), "bar", "down, round to the top");
        let end = panel.end_search(false).expect("a search");
        assert_eq!(
            (end.text.as_str(), end.found),
            ("b", Some(false)),
            "after Ctrl+g, Enter stays on the match without going round, as vim's"
        );
        assert_eq!(panel.search_again("x", true, false, 1), None, "no match");
        assert_eq!(under_cursor(&panel), "bar", "the cursor stays");
        assert_eq!(panel.search_again("B", false, false, 1), Some(true));
        assert_eq!(under_cursor(&panel), "boot", "ignoring case, round");

        // Fuzzy: the previous one as fzf ranks them, round to the worst.
        panel.search_down(1);
        panel.search_resume("bt", true);
        assert_eq!(panel.search_text(), Some("bt"));
        let best = under_cursor(&panel);
        panel.search_step(false, true);
        panel.search_step(true, true);
        assert_eq!(under_cursor(&panel), best, "back and forth");
        panel.search_edit(Action::Backspace, true);
        panel.search_edit(Action::Backspace, true);
        assert_eq!(panel.end_search(true), None, "nothing typed, nothing kept");
    }

    #[test]
    fn a_count_before_the_search_and_ctrl_g_go_that_many_matches_on() {
        let mut panel = search_files(&["a1", "a2", "a3", "a4"]);
        panel.search_down(2);
        typed(&mut panel, "a", false);
        assert_eq!(under_cursor(&panel), "a2");
        panel.search_step(true, false);
        assert_eq!(under_cursor(&panel), "a4");
        panel.search_step(true, false);
        assert_eq!(under_cursor(&panel), "a2", "round");
        // Enter searches once more from the match before, with the count, as vim's does.
        assert_eq!(
            panel.end_search(false).map(|end| end.found),
            Some(Some(false))
        );
        assert_eq!(under_cursor(&panel), "a3");
        // A count that goes round the end says so.
        panel.handle(Action::Home);
        panel.search_down(5);
        typed(&mut panel, "a", false);
        assert_eq!(under_cursor(&panel), "a1");
        assert_eq!(
            panel.end_search(false).map(|end| end.found),
            Some(Some(true))
        );
    }

    #[test]
    fn an_aborted_quick_search_puts_back_the_cursor_and_the_rows_on_screen() {
        let names: Vec<String> = (0..60).map(|n| format!("f{n:02}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut panel = search_files(&names);
        panel.page = 10;
        for _ in 0..20 {
            panel.handle(Action::Down);
        }
        panel.scroll(10);
        let (cursor, top) = (panel.cursor, panel.window.top());
        panel.search_down(1);
        typed(&mut panel, "f5", false);
        panel.scroll(10);
        assert_eq!(under_cursor(&panel), "f50");
        assert_ne!(panel.window.top(), top, "the rows scrolled to the match");
        panel.abort_search();
        panel.scroll(10);
        assert!(!panel.searching());
        assert_eq!((panel.cursor, panel.window.top()), (cursor, top));
    }

    #[test]
    fn literal_quick_search_finds_the_text_anywhere_in_a_name_as_the_filters_do() {
        let mut panel = search_files(&["bar", "cab", "Tabs"]);
        panel.search_down(1);
        typed(&mut panel, "AB", false);
        assert_eq!(under_cursor(&panel), "cab");
        panel.search_step(true, false);
        assert_eq!(under_cursor(&panel), "Tabs", "ignoring case");
        panel.end_search(false);
        assert_eq!(panel.search_again("ab", true, false, 1), Some(true));
        assert_eq!(under_cursor(&panel), "cab", "round");
    }

    #[test]
    fn quick_search_edits_its_text_as_a_text_field_does() {
        let mut panel = search_files(&["bar", "bat", "boot", "cab"]);
        panel.search_down(1);
        typed(&mut panel, "bt", false);
        assert_eq!(panel.cursor, 0, "no name starts with bt");
        panel.search_edit(Action::Backspace, false);
        assert_eq!(under_cursor(&panel), "bar");
        typed(&mut panel, "o", false);
        assert_eq!(under_cursor(&panel), "boot");
        // Typed in the middle, a character searches again.
        panel.search_edit(Action::TextStart, false);
        panel.search_edit(Action::Right, false);
        assert_eq!(under_cursor(&panel), "boot", "a move of the text cursor");
        typed(&mut panel, "a", false);
        assert_eq!(panel.search_text(), Some("bao"));
        assert_eq!(panel.cursor, 0);
        panel.search_edit(Action::DeleteChar, false);
        assert_eq!(panel.search_text(), Some("ba"));
        assert_eq!(under_cursor(&panel), "bar");
        panel.search_edit(Action::TextEnd, false);
        typed(&mut panel, "t", false);
        assert_eq!(under_cursor(&panel), "bat");
        panel.search_edit(Action::TextStart, false);
        panel.search_edit(Action::DeleteToEnd, false);
        assert_eq!(panel.search_text(), Some(""));
        assert_eq!(panel.cursor, 0, "nothing typed: where it started");
        let hosts = |_: &str| HostState::default();
        typed(&mut panel, "c", false);
        panel.search_edit(Action::TextStart, false);
        let mut terminal = render_with(&mut panel, (40, 6), true, &hosts, Decor::new(false));
        assert!(terminal.backend().to_string().contains("║Search: c"));
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            Position::new(9, 4),
            "on the text cursor"
        );
    }

    #[test]
    fn moves_the_cursor_within_the_rows() {
        let mut panel = loaded("/srv", listing());
        panel.page = 3;
        assert_eq!(panel.handle(Action::Up), None);
        assert_eq!(panel.cursor, 0);
        panel.handle(Action::Down);
        assert_eq!(under_cursor(&panel), "alpha-link");
        panel.handle(Action::End);
        assert_eq!(under_cursor(&panel), "zeta.txt");
        panel.handle(Action::Home);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn page_keys_scroll_the_rows_as_vims_and_keep_five_around_the_cursor() {
        let files = (0..29).map(|n| entry(&format!("f{n:02}"), FileKind::File, 1));
        let mut panel = loaded("/srv", files.collect());
        panel.page = 10;
        // Each key, then what a render does, as vim scrolls a window of ten lines.
        let mut press = |action| {
            panel.handle(action);
            panel.scroll(10);
            (panel.cursor, panel.window.top())
        };
        assert_eq!(press(Action::PageDown), (12, 8), "Ctrl+f");
        assert_eq!(press(Action::PageUp), (4, 0), "Ctrl+b");
        assert_eq!(press(Action::HalfPageDown), (9, 5), "Ctrl+d");
        assert_eq!(press(Action::HalfPageDown), (14, 10));
        assert_eq!(press(Action::HalfPageUp), (9, 5), "Ctrl+u");
        assert_eq!(press(Action::End), (29, 20));
        assert_eq!(press(Action::HalfPageDown), (29, 20), "the last row shows");
        assert_eq!(
            press(Action::PageDown),
            (29, 20),
            "unlike vim's, the last row stays at the bottom"
        );
        assert_eq!(press(Action::HalfPageUp), (19, 15));
        assert_eq!(press(Action::Home), (0, 0));
        for _ in 0..5 {
            press(Action::Down);
        }
        assert_eq!(
            press(Action::Down),
            (6, 1),
            "as vim, four rows below in ten"
        );
    }

    #[test]
    fn an_empty_listing_keeps_the_cursor_on_its_first_row() {
        let mut empty = loaded("/srv", Vec::new());
        for action in [
            Action::Down,
            Action::End,
            Action::PageDown,
            Action::HalfPageDown,
        ] {
            assert_eq!(empty.handle(action), None);
            assert_eq!(empty.cursor, 0);
        }
    }

    #[test]
    fn enters_directories_and_links_to_them_but_not_files() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, local("/srv/alpha-link"));
        assert_eq!(request.generation, 2);
        let inside = vec![entry("inside", FileKind::File, 1)];
        answer(&mut panel, &request, Listing::Dir(inside));
        assert_eq!(panel.location, local("/srv/alpha-link"));
        assert_eq!(names(&panel), ["..", "inside"]);

        panel.handle(Action::End);
        assert_eq!(panel.handle(Action::Enter), None, "a file does not open");
    }

    #[test]
    fn going_up_puts_the_cursor_on_the_directory_left() {
        let mut panel = loaded("/srv/bin", vec![entry("tool", FileKind::File, 1)]);
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, local("/srv"));
        answer(&mut panel, &request, Listing::Dir(listing()));
        assert_eq!(under_cursor(&panel), "bin");

        // Enter on `..` does the same.
        panel.handle(Action::Home);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, local("/"));
        let srv = vec![entry("srv", FileKind::Dir, 1)];
        answer(&mut panel, &request, Listing::Dir(srv));
        assert_eq!(under_cursor(&panel), "srv");
    }

    #[test]
    fn a_slash_is_the_top_without_dot_dot() {
        let mut panel = loaded("/", vec![entry("srv", FileKind::Dir, 1)]);
        assert_eq!(names(&panel), ["srv"]);
        assert_eq!(under_cursor(&panel), "srv", "the first row");
        assert_eq!(panel.handle(Action::Parent), None);
        assert_eq!(
            panel.for_other_panel(),
            Some(Destination::to(local("/srv")))
        );
        let mut empty = loaded("/", Vec::new());
        assert_eq!(empty.rows(), 0);
        assert_eq!(empty.handle(Action::Enter), None);
        assert_eq!(empty.handle(Action::Parent), None);
        assert_eq!(empty.handle(Action::End), None);
        let _ = draw(&mut empty, 40, 6, true);
    }

    #[test]
    fn hosts_open_their_home_directory_and_lead_up_to_slash() {
        let mut panel = loaded("/srv", Vec::new());
        let request = panel.go(Destination::to(remote("staging", "")));
        let refused = "deploy@stg: Permission denied (publickey).".to_owned();
        panel.listed(request.generation, Err(refused));
        assert_eq!(panel.location, local("/srv"), "the panel stays");
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open staging: deploy@stg: Permission denied (publickey).")
        );

        // The reply names the start directory, which `..` then leads up from.
        let request = panel.go(Destination::to(remote("staging", "")));
        let home = remote("staging", "/home/ubuntu");
        let reply = Listed {
            location: home.clone(),
            listing: Listing::Dir(Vec::new()),
            space: None,
        };
        panel.listed(request.generation, Ok(reply));
        assert_eq!(panel.location, home);
        assert!(
            draw(&mut panel, 40, 6, true)
                .to_string()
                .contains("staging:/home/ubuntu")
        );
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, remote("staging", "/home"));

        let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(Vec::new()));
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, remote("db", "/"));
        answer(&mut panel, &request, Listing::Dir(listing()));
        assert_eq!(names(&panel)[0], "alpha-link", "no `..` in a remote `/`");
        assert_eq!(panel.handle(Action::Parent), None);
    }

    #[test]
    fn alt_o_picks_what_the_other_panel_opens_and_moves_on() {
        let mut panel = loaded("/srv", listing());
        let up = Destination {
            location: local("/"),
            focus: Focus::Name(b"srv".to_vec()),
        };
        assert_eq!(
            panel.for_other_panel(),
            Some(up.clone()),
            "`..`: the parent"
        );
        assert_eq!(under_cursor(&panel), "alpha-link", "the cursor moves on");
        let into = Destination {
            location: local("/srv/alpha-link"),
            focus: Focus::First,
        };
        assert_eq!(panel.for_other_panel(), Some(into));
        panel.handle(Action::End);
        assert_eq!(
            panel.for_other_panel(),
            Some(up),
            "a file: the parent, as in mc"
        );
        assert_eq!(under_cursor(&panel), "zeta.txt", "the last row stays");
    }

    #[test]
    fn here_is_the_location_and_the_row_under_the_cursor() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        let here = Destination {
            location: local("/srv"),
            focus: Focus::Near {
                name: b"zeta.txt".to_vec(),
                row: 6,
            },
        };
        assert_eq!(panel.here(), here);

        let mut other = loaded("/tmp", Vec::new());
        let request = other.go(here);
        answer(&mut other, &request, Listing::Dir(listing()));
        assert_eq!(under_cursor(&other), "zeta.txt");
    }

    fn marked(panel: &Panel) -> Vec<String> {
        let mut names: Vec<String> = panel
            .marked
            .iter()
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn marks_entries_but_not_dot_dot_and_moves_on() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Mark);
        assert_eq!(under_cursor(&panel), "alpha-link", "moves on from `..`");
        assert_eq!(panel.marked_total(), (0, 0), "but `..` is not marked");
        panel.handle(Action::Mark);
        panel.handle(Action::End);
        panel.handle(Action::Mark);
        assert_eq!(under_cursor(&panel), "zeta.txt", "the last row stays");
        assert_eq!(
            panel.marked_total(),
            (2, 12_345),
            "directories count, but not their size"
        );
        panel.handle(Action::MarkUp);
        assert_eq!(under_cursor(&panel), "Alpha.md");
        assert_eq!(marked(&panel), ["alpha-link"]);

        panel.handle(Action::InvertMarks);
        assert_eq!(
            marked(&panel),
            [".hidden", "Alpha.md", "alpha-link", "zeta.txt"],
            "files only"
        );
        assert_eq!(panel.marked_total(), (4, 10_012_346));
    }

    #[test]
    fn n_that_finds_nothing_leaves_the_jumps_as_they_are() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        let end = panel.cursor;
        assert!(panel.jump_back(1).is_none(), "in this directory");
        assert_eq!(panel.cursor, 0);
        assert!(panel.search_again("nothing", true, false, 1).is_none());
        assert!(panel.jump_forward(1).is_none());
        assert_eq!(panel.cursor, end, "Tab still goes forward");
    }

    #[test]
    fn a_count_marks_as_far_as_the_listing_goes() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        panel.handle(Action::Up);
        let before = under_cursor(&panel);
        panel.mark_rows(true, 4);
        assert_eq!(
            marked(&panel),
            [before.as_str(), "zeta.txt"],
            "the last row once"
        );
        panel.mark_rows(false, 2);
        assert_eq!(marked(&panel), Vec::<String>::new());
        panel.mark_rows(false, 99);
        assert_eq!(panel.cursor, 0, "on `..`, which is never marked");
        assert_eq!(
            panel.marked_total().0,
            4,
            "the rows above, not the two below"
        );
    }

    #[test]
    fn marks_stay_while_the_directory_does() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::InvertMarks);
        panel.handle(Action::SortBySize);
        assert_eq!(marked(&panel), [".hidden", "Alpha.md", "zeta.txt"]);
        panel.set_show_hidden(false);
        assert_eq!(
            marked(&panel),
            ["Alpha.md", "zeta.txt"],
            "hidden ones lose them"
        );
        panel.set_show_hidden(true);
        assert_eq!(marked(&panel), ["Alpha.md", "zeta.txt"]);

        let request = panel.handle(Action::Reload).unwrap();
        let mut changed = listing();
        changed.retain(|entry| entry.name != b"zeta.txt");
        answer(&mut panel, &request, Listing::Dir(changed));
        assert_eq!(marked(&panel), ["Alpha.md"], "names still there keep them");

        panel.handle(Action::Home);
        let request = panel.handle(Action::Enter).unwrap();
        answer(&mut panel, &request, Listing::Dir(listing()));
        assert!(panel.marked.is_empty(), "another directory starts unmarked");
    }

    #[test]
    fn titles_put_a_tilde_for_the_home_directory() {
        let home = Path::new(HOME);
        let text = |path: &str| home_text(Path::new(path), home);
        assert_eq!(text(HOME), "~");
        assert_eq!(text("/home/me/src"), "~/src");
        assert_eq!(text("/home/meow"), "/home/meow", "not inside home");
        assert_eq!(text("/srv"), "/srv");
        assert_eq!(home_text(Path::new("/srv"), Path::new("/")), "/srv");
    }

    #[test]
    fn resolves_what_dialogs_name() {
        let mut panel = loaded("/srv", listing());
        assert_eq!(panel.name_under_cursor(), None, "`..`");
        panel.handle(Action::End);
        assert_eq!(panel.name_under_cursor(), Some(&b"zeta.txt"[..]));
        let resolve = |text: &str| panel.resolve(text);
        assert_eq!(resolve("new"), local("/srv/new"));
        assert_eq!(resolve("a/b//"), local("/srv/a/b"));
        assert_eq!(resolve("/tmp/x"), local("/tmp/x"));
        assert_eq!(resolve("/"), local("/"));
        assert_eq!(resolve("~"), local(HOME));
        assert_eq!(resolve("~/x"), local("/home/me/x"));
        assert_eq!(resolve("~x"), local("/srv/~x"), "not the home directory");
        assert_eq!(
            resolve("\\~/x"),
            local("/srv/~/x"),
            "a name that starts with ~"
        );

        let remote_panel = loaded_at(remote("db", "/srv"), Listing::Dir(Vec::new()));
        let resolve = |text: &str| remote_panel.resolve(text);
        assert_eq!(resolve("new"), remote("db", "/srv/new"));
        assert_eq!(resolve("/abs"), remote("db", "/abs"));
        assert_eq!(resolve("~/x"), remote("db", "x"), "from the remote home");
        assert_eq!(resolve("~"), remote("db", ""));
    }

    #[test]
    fn reload_keeps_the_cursor_on_its_entry() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        let request = panel.handle(Action::Reload).unwrap();
        assert_eq!(request.location, local("/srv"));
        let mut changed = listing();
        changed.push(entry("new.txt", FileKind::File, 1));
        answer(&mut panel, &request, Listing::Dir(changed));
        assert_eq!(under_cursor(&panel), "zeta.txt");

        let request = panel.handle(Action::Reload).unwrap();
        let other = vec![entry("other", FileKind::File, 1)];
        answer(&mut panel, &request, Listing::Dir(other));
        assert_eq!(
            panel.cursor, 1,
            "the entry is gone: its row, or the last one"
        );
    }

    #[test]
    fn drops_stale_replies() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let first = panel.handle(Action::Enter).unwrap();
        panel.handle(Action::Home);
        let second = panel.handle(Action::Enter).unwrap();
        let stale = vec![entry("stale", FileKind::File, 1)];
        answer(&mut panel, &first, Listing::Dir(stale));
        assert_eq!(
            panel.location,
            local("/srv"),
            "an older request was answered"
        );
        answer(&mut panel, &second, Listing::Dir(listing()));
        assert_eq!(panel.location, local("/"));
        answer(&mut panel, &second, Listing::Dir(Vec::new()));
        assert_eq!(
            panel.rows(),
            listing().len(),
            "a reply counts once, and `/` has no `..`"
        );
    }

    #[test]
    fn a_failed_listing_keeps_the_directory_and_reports_why() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        panel.listed(request.generation, Err("permission denied".to_owned()));
        assert_eq!(panel.location, local("/srv"));
        assert_eq!(under_cursor(&panel), "alpha-link");
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open alpha-link: permission denied")
        );
        // The parent is not inside the directory shown, so it gets its full path.
        let request = panel.handle(Action::Parent).unwrap();
        panel.listed(
            request.generation,
            Err("no such file or directory".to_owned()),
        );
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open /: no such file or directory")
        );
        // The next request clears it.
        panel.handle(Action::Reload);
        assert_eq!(panel.error, None);

        let request = panel.handle(Action::Reload).unwrap();
        panel.listed(request.generation, Err("bad\x1b[2Jthing".to_owned()));
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open /srv: bad?[2Jthing"),
            "reasons are shown terminal-safe"
        );
    }

    #[test]
    fn a_cancelled_request_is_forgotten() {
        let mut panel = loaded("/srv", listing());
        let request = panel.go(Destination::to(remote("web", "")));
        assert_eq!(panel.pending_request(), Some(request.clone()));
        let status = draw(&mut panel, 40, 6, true).to_string();
        assert!(status.contains("Loading…"), "{status}");

        panel.cancel();
        assert_eq!(panel.pending_request(), None);
        answer(&mut panel, &request, Listing::Dir(Vec::new()));
        assert_eq!(panel.location, local("/srv"), "the late reply is dropped");
    }

    /// The status line of `panel` while its hosts are in `status`, and the style of its first
    /// cell.
    fn status_line(panel: &mut Panel, status: HostStatus) -> (String, Style) {
        let hosts = |_: &str| HostState {
            status,
            address: None,
        };
        let terminal = render_with(panel, (40, 6), true, &hosts, Decor::new(false));
        let buffer = terminal.backend().buffer();
        let line: String = (1..39).map(|x| buffer[(x, 4)].symbol()).collect();
        (line.trim_end().to_owned(), buffer[(1, 4)].style())
    }

    #[test]
    fn a_panel_waiting_for_a_host_turns_a_spinner() {
        let mut panel = loaded("/srv", listing());
        panel.go(Destination::to(remote("web", "")));
        let theme = Theme::terminal();
        let (line, style) = status_line(&mut panel, HostStatus::Connecting);
        assert_eq!(line, "| Connecting to web…");
        assert_eq!(style.add_modifier, theme.host_connecting.add_modifier);
        let (line, style) = status_line(&mut panel, HostStatus::Stalled);
        assert_eq!(line, "| web: Retry or Cancel?");
        assert_eq!(style.add_modifier, theme.host_failed.add_modifier);
    }

    #[test]
    fn a_lost_host_leaves_its_panels_empty_and_waiting_with_the_cursor_kept() {
        let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
        panel.handle(Action::End);
        panel.handle(Action::Mark);
        panel.lose_host("web");
        assert_eq!(panel.rows(), 1 + listing().len(), "another host");

        panel.lose_host("db");
        assert_eq!(names(&panel), [".."], "the entries go");
        assert!(panel.marked.is_empty());
        let request = panel.pending_request().unwrap();
        assert_eq!(request.location, remote("db", "/srv"));
        let (line, _) = status_line(&mut panel, HostStatus::Stalled);
        assert_eq!(line, "| db: Retry or Cancel?");
        answer(&mut panel, &request, Listing::Dir(listing()));
        assert_eq!(under_cursor(&panel), "zeta.txt", "back on its entry");
        assert!(!panel.disconnected);

        // Cancel: the panel stays, without its entries, until it reads the directory again.
        panel.lose_host("db");
        panel.cancel();
        let (line, _) = status_line(&mut panel, HostStatus::Failed);
        assert_eq!(line, "Not connected to db; Ctrl+r connects");
        let request = panel.handle(Action::Reload).unwrap();
        assert_eq!(request.location, remote("db", "/srv"));
    }

    #[test]
    fn a_disconnected_host_leaves_its_panels_empty() {
        let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
        panel.drop_host("web");
        assert_eq!(panel.rows(), 1 + listing().len(), "another host");
        panel.drop_host("db");
        assert_eq!(names(&panel), [".."]);
        assert_eq!(panel.pending_request(), None);
        assert!(
            draw(&mut panel, 40, 6, true)
                .to_string()
                .contains("Not connected to db")
        );
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, remote("db", "/"), "`..` connects too");

        // A panel that was about to open the host stays where it is.
        let mut panel = loaded("/srv", listing());
        panel.go(Destination::to(remote("db", "")));
        panel.drop_host("db");
        assert_eq!(panel.pending_request(), None);
        assert_eq!(panel.location, local("/srv"));
        assert!(!panel.disconnected);
    }

    #[test]
    fn draws_the_listing() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        insta::assert_snapshot!(draw(&mut panel, 50, 12, true));
    }

    #[test]
    fn the_frame_shows_the_space_of_the_file_system_while_it_fits() {
        let (mut panel, request) = Panel::new(local("/srv"), PathBuf::from(HOME), true);
        let space = Space {
            total: 500 << 30,
            available: 123 << 30,
        };
        let reply = Listed {
            location: local("/srv"),
            listing: Listing::Dir(Vec::new()),
            space: Some(space),
        };
        panel.listed(request.generation, Ok(reply));
        insta::assert_snapshot!(draw(&mut panel, 30, 6, true));
        let bottom = |panel: &mut Panel, width| {
            let backend = draw(panel, width, 6, true);
            let buffer = backend.buffer();
            (0..width)
                .map(|x| buffer[(x, 5)].symbol().to_owned())
                .collect::<String>()
        };
        assert!(bottom(&mut panel, 23).contains("123G / 500G (24%)"));
        assert!(!bottom(&mut panel, 22).contains("123G"), "too narrow");

        // A failed reload keeps it; the next listing replaces it.
        let request = panel.reload_onto(Vec::new());
        panel.listed(request.generation, Err("gone".to_owned()));
        assert_eq!(panel.space, Some(space));
        let request = panel.reload_onto(Vec::new());
        answer(&mut panel, &request, Listing::Dir(Vec::new()));
        assert_eq!(panel.space, None);
        assert!(!bottom(&mut panel, 30).contains('G'));

        // A file system that reports nothing shows nothing.
        panel.space = Some(Space {
            total: 0,
            available: 0,
        });
        assert!(!bottom(&mut panel, 30).contains('0'));
    }

    /// The rows of `panel` drawn 14 cells wide, a name column of 12, four of them, inside the
    /// frame.
    fn name_rows(panel: &mut Panel) -> Vec<String> {
        let screen = draw(panel, 14, 9, true).to_string();
        screen
            .lines()
            .skip(2)
            .take(4)
            .map(|line| {
                let inside: String = line.trim_matches('"').chars().skip(1).collect();
                inside.chars().take(12).collect()
            })
            .collect()
    }

    #[test]
    fn z_scrolls_the_names_as_vim_scrolls_lines_that_do_not_wrap() {
        let mut panel = loaded(
            "/srv",
            vec![
                entry("dir", FileKind::Dir, 0),
                entry("abcdefghijklmnopqrst", FileKind::File, 1),
                entry("short", FileKind::File, 1),
            ],
        );
        panel.handle(Action::Down);
        panel.handle(Action::Down);
        assert_eq!(under_cursor(&panel), "abcdefghijklmnopqrst");
        assert_eq!(
            name_rows(&mut panel),
            [
                "/..         ",
                "/dir        ",
                " abcde»pqrst",
                " short      "
            ],
            "a name that does not fit loses its middle"
        );
        let press = |panel: &mut Panel, actions: &[Action]| {
            for action in actions {
                panel.handle(*action);
            }
            name_rows(panel)
        };
        // `zl`: a column, `«` over the first cell, and a name cut short at its end.
        assert_eq!(
            press(&mut panel, &[Action::ScrollRight]),
            [
                "«.          ",
                "«ir         ",
                "«bcdefghijk»",
                "«hort       "
            ]
        );
        // `zL`: half the column of 12.
        assert_eq!(
            press(&mut panel, &[Action::ScrollHalfRight])[2],
            "«hijklmnopq»"
        );
        assert_eq!(panel.shift, 7);
        // `zh` and `zH`, never past the start.
        assert_eq!(press(&mut panel, &[Action::ScrollLeft])[2], "«ghijklmnop»");
        press(
            &mut panel,
            &[Action::ScrollHalfLeft, Action::ScrollHalfLeft],
        );
        assert_eq!(panel.shift, 0);
        // `ze`: the end of the name under the cursor at the right edge.
        assert_eq!(press(&mut panel, &[Action::ScrollEnd])[2], "«jklmnopqrst");
        // `zl` stops where the last character of the name under the cursor is at the left
        // edge, as vim's stops at the end of the cursor line.
        press(&mut panel, &[Action::ScrollRight; 40]);
        assert_eq!(panel.shift, 20, "' abcdefghijklmnopqrst' is 21 cells");
        // `zs`: from the start again.
        press(&mut panel, &[Action::ScrollStart]);
        assert_eq!(panel.shift, 0);
        // On a shorter name, `zl` brings the names back to its end, as vim's does.
        press(&mut panel, &[Action::ScrollEnd, Action::ScrollRight]);
        assert_eq!(panel.shift, 10);
        panel.handle(Action::Down);
        panel.handle(Action::ScrollRight);
        assert_eq!(panel.shift, 5, "' short' is 6 cells");
        // Moving the cursor keeps the names where they are.
        panel.handle(Action::Up);
        assert_eq!(panel.shift, 5);
        // Another directory shows its names from their start.
        panel.cursor = 1;
        let request = panel.handle(Action::Enter).unwrap();
        answer(&mut panel, &request, Listing::Dir(vec![]));
        assert_eq!(panel.shift, 0);
    }

    #[test]
    fn the_marks_of_names_cut_short_have_an_accent_on_every_row() {
        use ratatui::style::{Color, Modifier};

        let mut panel = loaded(
            "/srv",
            vec![
                entry("abcdefghijklmnopqrst", FileKind::File, 1),
                entry("bcdefghijklmnopqrstu", FileKind::File, 1),
            ],
        );
        panel.handle(Action::Down);
        let hosts = |_: &str| HostState::default();
        let theme = Theme::mc_classic();
        let terminal = render_themed(&mut panel, (14, 8), true, &hosts, Decor::new(false), &theme);
        let buffer = terminal.backend().buffer();
        // The mark: after the frame, the marker, and five letters.
        let cell = |y: u16| &buffer[(7, y)];
        assert_eq!(cell(3).symbol(), "»");
        assert_eq!(
            (cell(3).fg, cell(3).bg),
            (Color::LightYellow, Color::Black),
            "under the cursor"
        );
        assert_eq!((cell(4).fg, cell(4).bg), (Color::LightYellow, Color::Blue));
        assert!(cell(4).modifier.contains(Modifier::BOLD));
        assert_eq!(
            buffer[(8, 4)].fg,
            Color::Gray,
            "the name goes on in its color"
        );
    }

    #[test]
    fn narrow_panels_drop_columns_and_unsafe_names_are_shown_safely() {
        let mut panel = loaded(
            "/a/very/long/path/that/does/not/fit",
            vec![entry("bad\x1b[2Jname", FileKind::File, 3)],
        );
        panel.handle(Action::End);
        insta::assert_snapshot!(draw(&mut panel, 24, 7, true));
    }

    #[test]
    fn marked_rows_and_their_total_are_yellow() {
        use ratatui::style::{Color, Modifier};

        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        panel.handle(Action::MarkUp);
        panel.handle(Action::Mark);
        let hosts = |_: &str| HostState::default();
        let theme = Theme::mc_classic();
        let terminal = render_themed(
            &mut panel,
            (40, 12),
            true,
            &hosts,
            Decor::new(false),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        // Rows: frame, header, `..`, alpha-link, Beta, bin, .hidden, Alpha.md, zeta.txt.
        assert_eq!(colors(1, 7), (Color::LightYellow, Color::Blue), "marked");
        let underlined = |x: u16, y: u16| buffer[(x, y)].modifier.contains(Modifier::UNDERLINED);
        assert!(underlined(1, 7), "marked, and underlined");
        assert!(underlined(1, 8), "under the cursor too");
        assert!(!underlined(1, 6), "not the others");
        assert!(
            buffer[(1, 4)].modifier.contains(Modifier::BOLD),
            "a directory is bold"
        );
        assert_eq!(
            colors(30, 7),
            (Color::LightYellow, Color::Blue),
            "all of it"
        );
        assert_eq!(
            colors(1, 8),
            (Color::LightYellow, Color::Cyan),
            "marked, under the cursor"
        );
        let separator: String = (0..40).map(|x| buffer[(x, 9)].symbol()).collect();
        assert_eq!(separator, "╟────── 10,012,345 B in 2 files ───────╢");
        assert_eq!(colors(7, 9), (Color::LightYellow, Color::Blue));
        assert_eq!(colors(6, 9), (Color::Gray, Color::Blue));
    }

    #[test]
    fn the_scroll_bar_runs_down_the_right_of_the_frame_along_the_rows() {
        let hosts = |_: &str| HostState::default();
        let mut short = loaded("/srv", vec![entry("one", FileKind::File, 1)]);
        let fits = render_with(&mut short, (40, 12), true, &hosts, Decor::new(false));
        assert!(!fits.backend().to_string().contains('░'));
        assert_eq!(short.bar(), None, "everything fits");
        let files = (0..40).map(|n| entry(&format!("f{n:02}"), FileKind::File, 1));
        let mut long = loaded("/srv", files.collect());
        render_with(&mut long, (40, 12), true, &hosts, Decor::new(false));
        let area = long.bar().unwrap().area();
        assert_eq!(
            (area.x, area.y, area.height),
            (39, 2, 7),
            "between the header and the line"
        );
        long.scroll_bar(Scroll::By(Motion::Down));
        assert_eq!(
            (long.window.top(), long.cursor),
            (1, 4),
            "a row, as the wheel"
        );
        long.scroll_bar(Scroll::By(Motion::PageDown));
        assert!(long.window.top() > 1, "a page, as Ctrl+f");
        long.scroll_bar(Scroll::To(30));
        assert_eq!(long.window.top(), 30);
        assert!(
            (30..37).contains(&long.cursor),
            "on the screen: {}",
            long.cursor
        );
    }

    #[test]
    fn single_borders_draw_single_lines() {
        let mut panel = loaded("/srv", listing());
        let hosts = |_: &str| HostState::default();
        let theme = Theme::terminal().with_borders(noc_config::Borders::Single);
        let terminal = render_themed(
            &mut panel,
            (40, 12),
            true,
            &hosts,
            Decor::new(false),
            &theme,
        );
        let text = terminal.backend().to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("\"┌"), "{text}");
        assert!(lines[9].starts_with("\"├────"), "{text}");
        assert!(lines[11].starts_with("\"└"), "{text}");
        assert!(!text.contains(['═', '║', '╟']), "{text}");
        assert_eq!(lines[0].matches('┬').count(), 2, "{text}");
        assert_eq!(lines[9].matches('┴').count(), 2, "{text}");
    }

    #[test]
    fn column_lines_run_down_the_whole_listing_and_join_the_frame() {
        let mut panel = loaded("/srv", vec![entry("only.txt", FileKind::File, 1)]);
        let text = draw(&mut panel, 40, 10, true).to_string();
        let lines: Vec<&str> = text.lines().collect();
        let column = |line: &str| line.chars().nth(19);
        assert_eq!(column(lines[0]), Some('╤'), "{text}");
        for line in &lines[1..7] {
            assert_eq!(column(line), Some('│'), "below the rows too: {text}");
        }
        assert_eq!(column(lines[7]), Some('┴'), "{text}");
        assert_eq!(
            column(lines[8]),
            Some(' '),
            "not across the status line: {text}"
        );
    }

    #[test]
    fn column_lines_take_the_background_of_the_cursor_and_marks() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        panel.handle(Action::MarkUp);
        let hosts = |_: &str| HostState::default();
        let theme = Theme::mc_classic();
        // Row 7 is under the cursor, row 8 marked; a line runs between name and size.
        for (active, cursor) in [(true, Color::Cyan), (false, Color::DarkGray)] {
            let terminal = render_themed(
                &mut panel,
                (40, 12),
                active,
                &hosts,
                Decor::new(false),
                &theme,
            );
            let buffer = terminal.backend().buffer();
            for (y, bg) in [(6, Color::Blue), (7, cursor), (8, Color::Blue)] {
                let cell = &buffer[(18, y)];
                assert_eq!(cell.symbol(), "│");
                assert_eq!((cell.fg, cell.bg), (Color::Gray, bg), "row {y}, {active}");
                assert_eq!(buffer[(17, y)].bg, bg, "row {y}, {active}: as the row");
                assert!(
                    !cell.modifier.contains(Modifier::UNDERLINED),
                    "row {y}, {active}: the mark's underline stops at the line"
                );
            }
        }
    }

    #[test]
    fn column_lines_leave_the_title_whole() {
        let mut panel = loaded("/srv/some-directory", listing());
        let text = draw(&mut panel, 40, 12, true).to_string();
        let top = text.lines().next().unwrap_or_default();
        assert!(top.contains(" /srv/some-directory "), "{text}");
        assert_eq!(top.matches('╤').count(), 1, "only past the title: {text}");
    }

    #[test]
    fn mc_classic_colors_panels_as_mc_does() {
        use ratatui::style::Color;

        let mut panel = loaded("/srv", varied());
        panel.handle(Action::Down);
        let hosts = |_: &str| HostState::default();
        let theme = Theme::mc_classic();
        let terminal = render_themed(
            &mut panel,
            (40, 12),
            true,
            &hosts,
            Decor::new(false),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        // Rows: frame, header, `..`, Adir (cursor), dir1, .env, a.txt, b.md.
        assert_eq!(colors(0, 0), (Color::Gray, Color::Blue), "frame");
        assert_eq!(colors(1, 1), (Color::LightYellow, Color::Blue), "header");
        assert_eq!(colors(1, 3), (Color::Black, Color::Cyan), "the cursor row");
        assert_eq!(colors(30, 3), (Color::Black, Color::Cyan), "all of it");
        assert_eq!(colors(1, 4), (Color::White, Color::Blue), "a directory");
        assert_eq!(colors(1, 6), (Color::Gray, Color::Blue), "a file");
        assert_eq!(colors(30, 4), (Color::Gray, Color::Blue), "other columns");
        assert_eq!(
            colors(2, 0),
            (Color::Black, Color::Cyan),
            "the active title"
        );
    }

    #[test]
    fn draws_icons_with_ui_icons() {
        let mut panel = loaded("/srv", listing());
        let hosts = |_: &str| HostState::default();
        let terminal = render_with(&mut panel, (40, 10), true, &hosts, Decor::new(true));
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn icons_are_dimmed_names_in_their_colors() {
        use ratatui::style::{Color, Modifier};

        let mut panel = loaded("/srv", varied());
        let hosts = |_: &str| HostState::default();
        let theme = Theme::mc_classic();
        let terminal = render_themed(
            &mut panel,
            (40, 12),
            false,
            &hosts,
            Decor::new(true),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let look = |x: u16, y: u16| {
            let cell = &buffer[(x, y)];
            (cell.fg, cell.modifier.contains(Modifier::DIM))
        };
        // Rows: frame, header, `..`, Adir, dir1, .env, a.txt, b.md.
        assert_eq!(look(1, 2), (Color::White, true), "the icon of `..`");
        assert_eq!(look(1, 3), (Color::White, true), "the icon of a directory");
        assert_eq!(look(3, 3), (Color::White, false), "its name");
        assert!(
            !buffer[(1, 3)].modifier.contains(Modifier::BOLD),
            "not bold"
        );
        assert!(
            buffer[(3, 3)].modifier.contains(Modifier::BOLD),
            "unlike the name"
        );
        assert_eq!(look(1, 6), (Color::Gray, true), "the icon of a file");
        assert_eq!(look(3, 6), (Color::Gray, false), "its name");
    }

    #[test]
    fn the_inactive_panel_shows_its_cursor_faintly_and_on_screen() {
        use ratatui::style::Modifier;
        let many: Vec<DirEntry> = (0..30)
            .map(|index| entry(&format!("file{index:02}"), FileKind::File, 1))
            .collect();
        let mut panel = loaded("/srv", many);
        panel.handle(Action::End);
        let backend = draw(&mut panel, 40, 10, false);
        assert!(panel.window.top() > 0, "scrolled to the cursor");
        let buffer = backend.buffer();
        let row_of = |text: &str| {
            (0..buffer.area.height).find(|&y| {
                let line: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                line.contains(text)
            })
        };
        let cursor_row = row_of("file29").unwrap();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let modifier = buffer[(x, y)].modifier;
                let on_cursor = y == cursor_row && x > 0 && x < buffer.area.width - 1;
                assert_eq!(
                    modifier.contains(Modifier::REVERSED | Modifier::DIM),
                    on_cursor,
                    "({x}, {y}): only the cursor's row, dimmed, and no title highlight"
                );
            }
        }
    }

    #[test]
    fn the_inactive_cursor_keeps_the_colors_of_the_names() {
        let mut panel = loaded(
            "/srv",
            vec![
                entry("docs", FileKind::Dir, 0),
                entry("notes", FileKind::File, 1),
            ],
        );
        panel.handle(Action::Down);
        let theme = Theme::mc_classic();
        let terminal = render_themed(
            &mut panel,
            (40, 8),
            false,
            &|_| HostState::default(),
            Decor::new(false),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let (width, height) = (buffer.area.width, buffer.area.height);
        let (x, y) = (0..height)
            .find_map(|y| {
                let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
                let x = line[..line.find("docs")?].chars().count();
                Some((u16::try_from(x).ok()?, y))
            })
            .unwrap();
        let cell = &buffer[(x, y)];
        assert_eq!(cell.fg, Color::White, "the directory's color");
        assert_eq!(cell.bg, Color::DarkGray, "the inactive cursor's background");
        assert_eq!(buffer[(x, y + 1)].bg, Color::Blue, "the next row is plain");
    }

    fn rename_listing() -> Vec<DirEntry> {
        vec![
            entry("report.final.pdf", FileKind::File, 10),
            entry("docs", FileKind::Dir, 4096),
            entry(".profile", FileKind::File, 1),
            entry("notes", FileKind::File, 1),
        ]
    }

    fn cursor_to(panel: &mut Panel, name: &str) {
        panel.cursor = names(panel).iter().position(|shown| shown == name).unwrap();
    }

    /// The text of the field of the entry being renamed before its cursor, and all of it.
    fn rename_field(panel: &mut Panel) -> (String, String) {
        let field = panel.rename_field().unwrap();
        let (shown, column) = field.visible(80);
        let before = shown.chars().take(column).collect();
        (before, field.text().to_owned())
    }

    #[test]
    fn renaming_puts_the_cursor_before_the_last_extension_of_a_file() {
        assert_eq!(stem_chars("report.final.pdf", false), 12);
        assert_eq!(stem_chars("archive.tar.gz", false), 11);
        assert_eq!(stem_chars("notes", false), 5);
        assert_eq!(stem_chars(".profile", false), 8, "a dotfile is all name");
        assert_eq!(stem_chars(".config.toml", false), 7);
        assert_eq!(stem_chars("odd.", false), 4, "no extension after the dot");
        assert_eq!(stem_chars("photos.d", true), 8, "a directory is all name");
        assert_eq!(stem_chars("файл.txt", false), 4, "characters, not bytes");

        let mut panel = loaded("/srv", rename_listing());
        assert!(!panel.start_rename(), "not on `..`");
        cursor_to(&mut panel, "report.final.pdf");
        assert!(panel.start_rename());
        assert!(panel.renaming());
        assert_eq!(
            rename_field(&mut panel),
            ("report.final".to_owned(), "report.final.pdf".to_owned())
        );
        let field = panel.rename_field().unwrap();
        assert_eq!(field.mode(), Mode::Insert);
        field.edit(Action::DeleteToStart);
        for c in "summary".chars() {
            field.insert(c);
        }
        assert_eq!(
            panel.end_rename(),
            Some((b"report.final.pdf".to_vec(), "summary.pdf".to_owned()))
        );
        assert!(!panel.renaming());

        let odd = DirEntry {
            name: b"a\xffb".to_vec(),
            ..entry("x", FileKind::File, 1)
        };
        let mut odd = loaded("/srv", vec![odd]);
        odd.cursor = 1;
        assert!(
            !odd.start_rename(),
            "the field could not give the name back"
        );
    }

    #[test]
    fn renaming_follows_its_entry_and_ends_when_it_goes() {
        let mut panel = loaded("/srv", rename_listing());
        cursor_to(&mut panel, "notes");
        assert!(panel.start_rename());
        panel.rename_field().unwrap().insert('x');

        let request = panel.handle(Action::Reload).unwrap();
        let mut more = rename_listing();
        more.push(entry("a-new-one", FileKind::File, 1));
        answer(&mut panel, &request, Listing::Dir(more));
        assert_eq!(under_cursor(&panel), "notes", "the cursor stays on it");
        assert_eq!(rename_field(&mut panel).1, "notesx", "and so does the text");

        let request = panel.handle(Action::Reload).unwrap();
        answer(&mut panel, &request, Listing::Dir(listing()));
        assert!(!panel.renaming(), "the entry is gone");

        let mut panel = loaded("/srv", rename_listing());
        cursor_to(&mut panel, ".profile");
        assert!(panel.start_rename());
        panel.set_show_hidden(false);
        assert!(!panel.renaming(), "the entry is hidden");

        let mut panel = loaded("/srv", rename_listing());
        cursor_to(&mut panel, "docs");
        assert!(panel.start_rename());
        assert_eq!(rename_field(&mut panel).0, "docs");
        panel.go(Destination::to(local("/")));
        assert!(!panel.renaming(), "the panel goes elsewhere");
    }

    #[test]
    fn draws_the_name_being_renamed_over_its_row() {
        let mut panel = loaded("/srv", rename_listing());
        cursor_to(&mut panel, "report.final.pdf");
        assert!(panel.start_rename());
        let terminal = render_with(
            &mut panel,
            (40, 9),
            true,
            &|_| HostState::default(),
            Decor::new(false),
        );
        insta::assert_snapshot!(terminal.backend());
        let buffer = terminal.backend().buffer();
        let y = 1 + 1 + u16::try_from(panel.cursor - panel.window.top()).unwrap();
        // After the marker, the name, in Insert mode with nothing selected, and the rest of the
        // row, which the size and time columns no longer cut.
        let reversed = |x: u16| buffer[(x, y)].modifier.contains(Modifier::REVERSED);
        assert!(reversed(2) && reversed(14), "nothing is selected");
        assert!(reversed(38), "the field runs to the end of the row");
        assert_eq!(buffer[(30, y)].symbol(), " ", "no line between columns");
        assert_eq!(terminal.backend().cursor_position(), (14, y).into());
    }
}
