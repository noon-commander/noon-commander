//! A panel that lists a local or remote directory.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::tz::TimeZone;
use noc_config::Wheel;
use noc_vfs::{DirEntry, Location, RemotePath, Space};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use super::cells::{self, Align, MTIME_WIDTH};
use super::decor::{self, Decor};
use super::dialog::Field;
use super::fuzzy::Fuzzy;
use super::keymap::Action;
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the size column, as in mc.
const SIZE_WIDTH: usize = 7;

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
        if self.descending { '↓' } else { '↑' }
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
    /// First row on screen.
    offset: usize,
    /// Rows on screen at the last render.
    page: usize,
    /// Where the rows were at the last render, for the mouse.
    list: Rect,
    generation: u64,
    pending: Option<Pending>,
    /// The text field of quick search, with what it has matched so far, while it runs.
    search: Option<Field>,
    /// The entry under the cursor, while its name is edited in its row.
    renaming: Option<Renaming>,
    error: Option<String>,
    /// Where the lines between columns ran at the last render, so that they can be joined to
    /// a top of the frame drawn again over the panel's.
    column_lines: Vec<u16>,
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
            offset: 0,
            page: 1,
            list: Rect::default(),
            generation: 0,
            pending: None,
            search: None,
            renaming: None,
            error: None,
            column_lines: Vec::new(),
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
            offset: self.offset,
            page: self.page,
            list: Rect::default(),
            generation: 0,
            pending: None,
            search: None,
            renaming: None,
            error: None,
            column_lines: Vec::new(),
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
        }
        self.pending = Some(Pending {
            generation: self.generation,
            location: location.clone(),
            focus,
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
                    self.marked.clear();
                    self.renaming = None;
                }
                self.location = location;
                self.listing = listing;
                self.space = space;
                self.disconnected = false;
                self.arrange();
                self.offset = 0;
                let rows = self.rows();
                self.cursor = (0..rows)
                    .find(|&index| {
                        self.row(index)
                            .is_some_and(|row| pending.focus.matches(row))
                    })
                    .unwrap_or(match pending.focus {
                        Focus::Near { row, .. } => row.min(rows.saturating_sub(1)),
                        _ => 0,
                    });
                self.keep_renaming();
            }
            Err(reason) => {
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
        self.arrange();
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

    /// Starts renaming the entry under the cursor in its row: the field holds its name, with
    /// all but the last extension of a file selected. `false` on `..`, volumes, and hosts, and
    /// for a name that is not UTF-8, which the field could not give back.
    pub(crate) fn start_rename(&mut self) -> bool {
        let Some(entry) = self.entry_under_cursor() else {
            return false;
        };
        let Ok(text) = std::str::from_utf8(&entry.name) else {
            return false;
        };
        let field = Field::selecting(text, stem_chars(text, entry.is_dir_like()));
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
        self.search.as_ref().map(Field::text)
    }

    /// Starts quick search, or jumps to the next match while it runs: the next one down, round
    /// to the top; `fuzzy` (`ui.fuzzy_search`), the next best, round to the best.
    pub(crate) fn search_next(&mut self, fuzzy: bool) {
        let Some(text) = self.search_text().map(str::to_owned) else {
            self.search = Some(Field::plain(""));
            return;
        };
        self.step_match(&text, true, fuzzy);
    }

    /// Jumps to the next or the previous match of the search that runs, if one does.
    pub(crate) fn search_step(&mut self, forward: bool, fuzzy: bool) {
        if let Some(text) = self.search_text().map(str::to_owned) {
            self.step_match(&text, forward, fuzzy);
        }
    }

    /// Searches for `text`, what an earlier search found, as if it were typed again, and jumps
    /// to its next match. Nothing changes if no name matches it.
    pub(crate) fn search_resume(&mut self, text: &str, fuzzy: bool) {
        if self.step_match(text, true, fuzzy) {
            let text = if fuzzy {
                text.to_owned()
            } else {
                text.to_lowercase()
            };
            let mut field = Field::plain(&text);
            // Typing goes on from the end rather than replacing the text.
            field.edit(Action::End);
            self.search = Some(field);
        }
    }

    /// Moves the cursor to the next name that matches `text`, down and round to the top, or to
    /// the previous one, up and round to the bottom; `fuzzy`, to the next or the previous as fzf
    /// ranks them, round, or to the best from a row that does not match. Whether one matches.
    pub(crate) fn step_match(&mut self, text: &str, forward: bool, fuzzy: bool) -> bool {
        let found = if fuzzy {
            let ranked = self.ranked(text);
            let count = ranked.len();
            let at = ranked.iter().position(|&(row, _)| row == self.cursor);
            let index = match at {
                Some(at) if forward => (at + 1) % count,
                Some(at) => (at + count - 1) % count,
                None => 0,
            };
            ranked.get(index).map(|&(row, _)| row)
        } else {
            let text = text.to_lowercase();
            if forward {
                self.find(&text, self.cursor + 1)
            } else {
                self.find_back(&text, self.cursor)
            }
        };
        if let Some(row) = found {
            self.cursor = row;
        }
        found.is_some()
    }

    /// Types `c` at the cursor of quick search, starting it if needed, and moves the cursor of
    /// the panel to the first name from where it is that starts with the text, ignoring case;
    /// `fuzzy` (`ui.fuzzy_search`), to the best match as fzf ranks them, the first from where it
    /// is if several are as good. As in mc, a character that nothing matches is dropped.
    pub(crate) fn search_type(&mut self, c: char, fuzzy: bool) {
        let started = self.search.is_none();
        let mut field = self.search.take().unwrap_or_else(|| Field::plain(""));
        // Smart case needs the capitals.
        let typed: Vec<char> = if fuzzy {
            vec![c]
        } else {
            c.to_lowercase().collect()
        };
        for &c in &typed {
            field.insert(c);
        }
        // Spaces match everything; only between words do they mean something.
        if started && fuzzy && field.text().trim().is_empty() {
            return;
        }
        match self.best_match(field.text(), fuzzy) {
            Some(row) => self.cursor = row,
            None if started => return,
            None => {
                for _ in &typed {
                    field.edit(Action::Backspace);
                }
            }
        }
        self.search = Some(field);
    }

    /// Edits the text of quick search with a key of the text fields; then the cursor of the
    /// panel stays on its name if that still matches, and goes to the match that typing would
    /// have found otherwise. Moves of the text cursor leave the panel's where it is.
    pub(crate) fn search_edit(&mut self, action: Action, fuzzy: bool) {
        let Some(field) = &mut self.search else {
            return;
        };
        let before = field.text().to_owned();
        field.edit(action);
        let text = field.text().to_owned();
        if text == before || text.trim().is_empty() {
            return;
        }
        let stays = if fuzzy {
            self.ranked(&text)
                .iter()
                .any(|&(row, _)| row == self.cursor)
        } else {
            self.starts_with(self.cursor, &text)
        };
        if !stays && let Some(row) = self.best_match(&text, fuzzy) {
            self.cursor = row;
        }
    }

    /// The row that quick search goes to for `text`: the first from the cursor whose name
    /// starts with it, ignoring case; `fuzzy`, the best as fzf ranks them, the first from the
    /// cursor if several are as good.
    fn best_match(&self, text: &str, fuzzy: bool) -> Option<usize> {
        if fuzzy {
            let ranked = self.ranked(text);
            let best = ranked.first().map(|&(_, score)| score);
            let rows = self.rows().max(1);
            ranked
                .iter()
                .take_while(|&&(_, score)| Some(score) == best)
                .map(|&(row, _)| row)
                .min_by_key(|&row| (row + rows - self.cursor) % rows)
        } else {
            self.find(text, self.cursor)
        }
    }

    /// Ends quick search; what it typed, unless that was nothing.
    pub(crate) fn end_search(&mut self) -> Option<String> {
        self.search
            .take()
            .map(|field| field.text().to_owned())
            .filter(|text| !text.trim().is_empty())
    }

    /// The first row from `start` on, round to the top, whose name starts with `text` (lower
    /// case). `..` never matches.
    fn find(&self, text: &str, start: usize) -> Option<usize> {
        let rows = self.rows();
        (start..rows)
            .chain(0..start.min(rows))
            .find(|&index| self.starts_with(index, text))
    }

    /// The first row before `start`, up and round to the bottom, whose name starts with `text`
    /// (lower case).
    fn find_back(&self, text: &str, start: usize) -> Option<usize> {
        let rows = self.rows();
        let start = start.min(rows);
        (0..start)
            .rev()
            .chain((start..rows).rev())
            .find(|&index| self.starts_with(index, text))
    }

    /// Whether the name of row `index` starts with `text` (lower case), ignoring case.
    fn starts_with(&self, index: usize, text: &str) -> bool {
        self.search_name(index)
            .is_some_and(|name| name.to_lowercase().starts_with(text))
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
        let last = self.rows().saturating_sub(1);
        let page = self.page.max(1);
        let half = (page / 2).max(1);
        match action {
            Action::Up => self.cursor = self.cursor.saturating_sub(1),
            Action::Down => self.cursor = (self.cursor + 1).min(last),
            Action::PageUp => self.cursor = self.cursor.saturating_sub(page),
            Action::PageDown => self.cursor = (self.cursor + page).min(last),
            Action::HalfPageUp => self.cursor = self.cursor.saturating_sub(half),
            Action::HalfPageDown => self.cursor = (self.cursor + half).min(last),
            Action::Home => self.cursor = 0,
            Action::End => self.cursor = last,
            // As in mc, the cursor moves on even from `..`, which cannot be marked.
            Action::Mark => {
                self.toggle_mark();
                self.cursor = (self.cursor + 1).min(last);
            }
            Action::MarkUp => {
                self.toggle_mark();
                self.cursor = self.cursor.saturating_sub(1);
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

    /// Goes to `destination`; returns the listing to request.
    pub(crate) fn go(&mut self, destination: Destination) -> ListRequest {
        self.open(destination.location, destination.focus)
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

    /// What Alt+o opens in the other panel, as in mc: the directory under the cursor, or, for a
    /// file, the parent directory with the cursor on this one. The cursor moves on
    /// to the next row.
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
        let mut title = match &self.location {
            Location::Local(path) => home_text(path, &self.home),
            Location::Remote { .. } => location_text(&self.location),
        };
        let room = usize::from(area.width.saturating_sub(4));
        if cells::width(&title) > room {
            title = cells::fit(&title, room, Align::Left);
        }
        let title_style = if active {
            theme.panel_title_active
        } else {
            theme.panel_border
        };
        let title = Line::styled(format!(" {title} "), title_style);
        let block = Block::bordered()
            .border_type(theme.border_type())
            .title(title)
            .style(theme.panel)
            .border_style(theme.panel_border);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.column_lines.clear();
        self.list = Rect::default();
        if inner.height < 3 || inner.width < 2 {
            return;
        }
        self.render_space(frame, area, theme);
        let width = usize::from(inner.width);
        let list_height = usize::from(inner.height - 3);
        self.page = list_height;
        self.list = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 3);
        self.scroll(list_height);

        let columns = DirColumns::for_width(width);
        let line = |y: u16| Rect::new(inner.x, y, inner.width, 1);
        let header = Line::styled(columns.header(self.sort), theme.header);
        frame.render_widget(header, line(inner.y));
        for (screen_row, index) in (self.offset..self.rows()).take(list_height).enumerate() {
            let Some(row) = self.row(index) else { break };
            let marked = matches!(row, Row::Entry(entry) if self.marked.contains(&entry.name));
            let cursor = (index == self.cursor).then_some(active);
            let text = row_style(columns.row(row, view), cursor, marked, theme);
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
        } else if let Some(field) = &self.search {
            status_style = theme.quick_search;
            let prefix = cells::width(&fl!("panel-search", text = String::new()));
            let (shown, column) = field.visible(width.saturating_sub(prefix));
            if active {
                let column = u16::try_from(prefix + column).unwrap_or(u16::MAX);
                let x = area.x.saturating_add(column).min(area.right() - 1);
                frame.set_cursor_position((x, area.y));
            }
            fl!("panel-search", text = shown)
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
                let status = cells::fit(&status, width.saturating_sub(2), Align::Left);
                Line::from(vec![
                    Span::styled(spinner, theme.host_status(waiting)),
                    Span::raw(status),
                ])
            }
            None => Line::styled(cells::fit(&status, width, Align::Left), status_style),
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
            .checked_sub(self.offset)
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
        let separator = format!("{left}{}{right}", "─".repeat(usize::from(inside)));
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
            total = cells::fit(&total, usize::from(inside), Align::Left);
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
                    cell.set_char('│').set_fg(color);
                    cell.modifier.remove(Modifier::UNDERLINED | Modifier::BOLD);
                }
            }
            if let Some(cell) = buffer.cell_mut((x, inner.bottom() - 2))
                && cell.symbol() == "─"
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
        let buffer = frame.buffer_mut();
        for &x in &self.column_lines {
            if let Some(cell) = buffer.cell_mut((x, area.y))
                && matches!(cell.symbol(), "═" | "─")
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
        let row = self.offset + usize::from(at.y - self.list.y);
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

    /// Scrolls by `step` of the mouse wheel, down or up, without moving the cursor while it
    /// stays on screen.
    pub(crate) fn wheel(&mut self, step: Wheel, down: bool) {
        let page = self.page.max(1);
        let rows = match step {
            Wheel::Lines(lines) => usize::from(lines),
            Wheel::Page => page,
        };
        let last = self.rows().saturating_sub(1);
        self.offset = if down {
            (self.offset + rows).min(self.rows().saturating_sub(page))
        } else {
            self.offset.saturating_sub(rows)
        };
        self.cursor = self
            .cursor
            .max(self.offset)
            .min(self.offset + page - 1)
            .min(last);
    }

    /// Keeps the cursor within the rows and on screen.
    fn scroll(&mut self, height: usize) {
        self.cursor = self.cursor.min(self.rows().saturating_sub(1));
        let height = height.max(1);
        self.offset = self.offset.min(self.rows().saturating_sub(height));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }
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
    fn header(self, sort: Sort) -> String {
        let mut titles = [fl!("panel-name"), fl!("panel-size"), fl!("panel-time")];
        let (column, title) = match sort.key {
            SortKey::Name => (0, fl!("panel-name")),
            SortKey::Extension => (0, fl!("panel-name-by-extension")),
            SortKey::Size => (1, fl!("panel-size")),
            SortKey::Time => (2, fl!("panel-time")),
        };
        titles[column] = format!("{}{title}", sort.arrow());
        let [name, size, time] = titles;
        let (name, rest) = self.join(&name, &size, &time, [Align::Center; 3]);
        name + &rest
    }

    /// A row, with the name in the color of its kind, as mc highlights files.
    fn row(self, row: Row<'_>, view: &View<'_>) -> Line<'static> {
        const ALIGN: [Align; 3] = [Align::Left, Align::Right, Align::Left];
        let (decor, theme) = (view.decor, view.theme);
        let styled = |(name, rest): (String, String), prefix: &str, style: Style| {
            let mut spans = icon_spans(name, prefix, style, view);
            spans.push(Span::raw(rest));
            Line::from(spans)
        };
        match row {
            Row::Parent => {
                let prefix = decor.parent();
                let name = format!("{prefix}..");
                styled(
                    self.join(&name, &fl!("panel-up-dir"), "", ALIGN),
                    prefix,
                    theme.directory,
                )
            }
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
                let prefix = decor.entry(entry);
                let name = format!("{prefix}{}", cells::sanitize(&entry.name));
                styled(
                    self.join(&name, &size, &time, ALIGN),
                    &prefix,
                    theme.entry(entry),
                )
            }
        }
    }
}

/// A name cell in `style`, with the icon of its `prefix` in a toned-down `style`. A cell cut so
/// short that the prefix lost its end, and mc's markers, are drawn whole in `style`.
fn icon_spans(name: String, prefix: &str, style: Style, view: &View<'_>) -> Vec<Span<'static>> {
    match name.strip_prefix(prefix) {
        Some(rest) if view.decor.icons() && !prefix.is_empty() => vec![
            Span::styled(prefix.to_owned(), Theme::icon(style)),
            Span::styled(rest.to_owned(), style),
        ],
        _ => vec![Span::styled(name, style)],
    }
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
    fn join(self, name: &str, size: &str, time: &str, align: [Align; 3]) -> (String, String) {
        let mut rest = String::new();
        if self.size {
            rest.push(' ');
            rest.push_str(&cells::fit(size, SIZE_WIDTH, align[1]));
        }
        if self.time {
            rest.push(' ');
            rest.push_str(&cells::fit(time, MTIME_WIDTH, align[2]));
        }
        (cells::fit(name, self.name, align[0]), rest)
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

/// `text` as a row under the cursor, `Some(true)` in the active panel, and `marked`. The cursor
/// and marks replace the colors of the row, as in mc; the cursor of the inactive panel only puts
/// a background under them.
fn row_style(
    mut text: Line<'static>,
    cursor: Option<bool>,
    marked: bool,
    theme: &Theme,
) -> Line<'static> {
    let style = match (cursor == Some(true), marked) {
        (true, true) => Some(theme.marked_cursor),
        (true, false) => Some(theme.cursor),
        (false, true) => Some(theme.marked),
        (false, false) => None,
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

    #[test]
    fn quick_search_finds_name_prefixes_from_the_cursor() {
        let mut panel = loaded("/srv", listing());
        panel.search_type('b', false);
        assert!(panel.searching());
        assert_eq!(under_cursor(&panel), "Beta", "case does not matter");
        panel.search_type('I', false);
        assert_eq!(under_cursor(&panel), "bin");
        panel.search_type('x', false);
        assert_eq!(panel.search_text(), Some("bi"), "a miss is dropped");
        panel.search_edit(Action::Backspace, false);
        assert_eq!(under_cursor(&panel), "bin", "the cursor stays");
        panel.search_next(false);
        assert_eq!(under_cursor(&panel), "Beta", "round to the top");
        panel.search_type('e', false);
        let hosts = |_: &str| HostState::default();
        let mut terminal = render_with(&mut panel, (40, 6), true, &hosts, Decor::new(false));
        assert!(terminal.backend().to_string().contains("║Search: be"));
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            Position::new(11, 4),
            "after the text"
        );

        panel.end_search();
        assert!(!panel.searching());
        // A character that starts no match starts no search.
        panel.search_type('q', false);
        assert!(!panel.searching());
        // Opening another directory ends a search.
        panel.search_type('b', false);
        panel.handle(Action::Reload);
        assert!(!panel.searching());
    }

    #[test]
    fn fuzzy_quick_search_goes_to_the_best_match_and_on_to_the_next_best() {
        let mut panel = loaded(
            "/srv",
            ["Cargo.toml", "config.rs", "cfg-if", "notes", "x.cfg"]
                .map(|name| entry(name, FileKind::File, 1))
                .into(),
        );
        for c in "cfg".chars() {
            panel.search_type(c, true);
        }
        assert_eq!(under_cursor(&panel), "cfg-if", "together, at the start");
        panel.search_next(true);
        assert_eq!(under_cursor(&panel), "x.cfg", "together");
        panel.search_next(true);
        assert_eq!(under_cursor(&panel), "config.rs", "apart");
        panel.search_next(true);
        assert_eq!(under_cursor(&panel), "cfg-if", "round to the best");
        panel.search_type('q', true);
        assert_eq!(panel.search_text(), Some("cfg"), "a miss is dropped");
        panel.search_edit(Action::DeleteToStart, true);
        assert_eq!(panel.search_text(), Some(""));
        panel.search_type('C', true);
        assert_eq!(
            under_cursor(&panel),
            "Cargo.toml",
            "case counts with a capital"
        );
        panel.end_search();

        let mut panel = loaded(
            "/srv",
            ["bar", "bat", "baz"]
                .map(|name| entry(name, FileKind::File, 1))
                .into(),
        );
        panel.handle(Action::End);
        panel.search_type('b', true);
        assert_eq!(
            under_cursor(&panel),
            "baz",
            "as good: the first from the cursor"
        );
        panel.search_type(' ', true);
        assert_eq!(under_cursor(&panel), "baz", "a space stays");
        panel.end_search();
        panel.search_type('x', true);
        assert!(!panel.searching(), "no match, no search");
        panel.search_type(' ', true);
        assert!(!panel.searching(), "nor from a space");
    }

    #[test]
    fn matches_go_both_ways_round_and_an_ended_search_says_what_it_found() {
        let names = ["bar", "bin", "cat", "boot"];
        let files = names.map(|name| entry(name, FileKind::File, 1));
        let mut panel = loaded("/srv", files.into());
        panel.search_type('B', false);
        assert_eq!(under_cursor(&panel), "bar");
        panel.search_step(false, false);
        assert_eq!(under_cursor(&panel), "boot", "up, round to the bottom");
        panel.search_step(true, false);
        assert_eq!(under_cursor(&panel), "bar", "down, round to the top");
        assert_eq!(panel.end_search().as_deref(), Some("b"));
        assert!(!panel.step_match("x", true, false), "no match");
        assert_eq!(under_cursor(&panel), "bar", "the cursor stays");
        assert!(panel.step_match("B", false, false), "ignoring case");
        assert_eq!(under_cursor(&panel), "boot");

        // Fuzzy: the previous one as fzf ranks them, round to the worst.
        panel.search_resume("bt", true);
        assert_eq!(panel.search_text(), Some("bt"));
        let best = under_cursor(&panel);
        panel.search_step(false, true);
        panel.search_step(true, true);
        assert_eq!(under_cursor(&panel), best, "back and forth");
        panel.search_edit(Action::Backspace, true);
        panel.search_edit(Action::Backspace, true);
        assert_eq!(panel.end_search(), None, "nothing typed, nothing kept");
        panel.search_resume("x", false);
        assert!(!panel.searching(), "no match, no search");
    }

    #[test]
    fn quick_search_edits_its_text_as_a_text_field_does() {
        let names = ["bar", "bat", "boot", "cab"];
        let files = names.map(|name| entry(name, FileKind::File, 1));
        let mut panel = loaded("/srv", files.into());
        for c in "bt".chars() {
            panel.search_type(c, false);
        }
        assert_eq!(panel.search_text(), Some("b"), "no name starts with bt");
        panel.search_type('o', false);
        assert_eq!(under_cursor(&panel), "boot");
        // Typed in the middle, a character searches again; one that nothing matches is dropped.
        panel.search_edit(Action::Home, false);
        panel.search_edit(Action::Right, false);
        assert_eq!(under_cursor(&panel), "boot", "a move of the text cursor");
        panel.search_type('x', false);
        assert_eq!(panel.search_text(), Some("bo"));
        panel.search_edit(Action::Delete, false);
        assert_eq!(panel.search_text(), Some("b"));
        assert_eq!(under_cursor(&panel), "boot", "it still matches");
        panel.search_type('a', false);
        assert_eq!(panel.search_text(), Some("ba"));
        assert_eq!(under_cursor(&panel), "bar", "round to the top");
        panel.search_edit(Action::End, false);
        panel.search_type('t', false);
        assert_eq!(under_cursor(&panel), "bat");
        panel.search_edit(Action::Home, false);
        panel.search_edit(Action::DeleteToEnd, false);
        assert_eq!(panel.search_text(), Some(""));
        assert_eq!(
            under_cursor(&panel),
            "bat",
            "nothing typed: the cursor stays"
        );
        let hosts = |_: &str| HostState::default();
        panel.search_type('c', false);
        panel.search_edit(Action::Home, false);
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
        panel.handle(Action::PageDown);
        assert_eq!(under_cursor(&panel), ".hidden");
        panel.handle(Action::PageDown);
        assert_eq!(under_cursor(&panel), "zeta.txt", "stops at the last row");
        panel.handle(Action::PageUp);
        assert_eq!(under_cursor(&panel), "bin");
        panel.handle(Action::Home);
        assert_eq!(panel.cursor, 0);
        panel.handle(Action::End);
        assert_eq!(under_cursor(&panel), "zeta.txt");
        panel.page = 4;
        let row = panel.cursor;
        panel.handle(Action::HalfPageUp);
        assert_eq!(panel.cursor, row - 2, "half of four rows");
        panel.handle(Action::HalfPageDown);
        assert_eq!(panel.cursor, row);
        panel.handle(Action::HalfPageDown);
        assert_eq!(panel.cursor, row, "stops at the last row");
        panel.page = 1;
        panel.handle(Action::HalfPageUp);
        assert_eq!(panel.cursor, row - 1, "at least one row");

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
        assert!(panel.offset > 0, "scrolled to the cursor");
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

    /// The selected text of the field, and all of it.
    fn rename_field(panel: &mut Panel) -> (String, String) {
        let field = panel.rename_field().unwrap();
        let ([_, selected, _], _) = field.visible_parts(80);
        (selected, field.text().to_owned())
    }

    #[test]
    fn renaming_selects_all_but_the_last_extension_of_a_file() {
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
        assert_eq!(rename_field(&mut panel).1, "x", "and so does the text");

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
        let y = 1 + 1 + u16::try_from(panel.cursor - panel.offset).unwrap();
        // After the marker, the selection, then the extension and the rest of the row, which
        // the size and time columns no longer cut.
        let reversed = |x: u16| buffer[(x, y)].modifier.contains(Modifier::REVERSED);
        assert!(!reversed(2), "the selection is not reversed in the field");
        assert!(!reversed(13));
        assert!(reversed(14), "the extension is not selected");
        assert!(reversed(38), "the field runs to the end of the row");
        assert_eq!(buffer[(30, y)].symbol(), " ", "no line between columns");
        assert_eq!(terminal.backend().cursor_position(), (14, y).into());
    }
}
