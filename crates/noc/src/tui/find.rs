//! The search of a whole tree: `g /` in a panel, or F9 → Command → Find file…. ripgrep searches
//! the files under the panel's directory, local or on a host, for the text of the first field,
//! among the files that the globs of the second let through, again as either changes or a check
//! box switches. The dialog lists each file it found, in the order of their paths, with the lines
//! that match under it, the matches highlighted; with no text it lists the files. Its buttons go
//! to the file in the panel, and view and edit it at the line, as in the results of mc's Find
//! File. Its other mode, Files and dirs, finds files and directories whose names match the text
//! with fd, and goes to them.

use std::ops::Range;

use noc_scroll::scrollbar::Bar;
use noc_tools::fd;
use noc_tools::ripgrep::{self, Hit};
use noc_vfs::{DirEntry, FileKind, Location, Metadata};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tokio_util::sync::CancellationToken;

use super::cells::{self, Align};
use super::decor::{self, Decor};
use super::dialog::{Button, Dialog, DialogEvent, Field, ListView};
use super::edit::Registers;
use super::keymap::{Action, Context, Mode, Resolved, Typed};
use super::mouse::Pointer;
use super::panel::child;
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the dialog gets, in cells, borders included.
const WIDTH: u16 = 120;
/// Cells of text kept before the first match of a line that is too long to show whole.
const BEFORE_MATCH: usize = 8;
/// The fields of the dialog, and its check boxes.
const TEXT: usize = 0;
const FILES: usize = 1;
const REGEX: usize = 0;
const MATCH_CASE: usize = 1;
/// Its modes, as its choices: text in the files, or files and directories by name.
const TEXT_MODE: usize = 0;
const NAMES_MODE: usize = 1;

/// What a search looks for: lines with ripgrep, or names with fd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FindQuery {
    Lines(ripgrep::Query),
    Names(fd::Query),
}

#[cfg(test)]
impl FindQuery {
    /// The query of a search of lines.
    pub(crate) fn lines(&self) -> &ripgrep::Query {
        match self {
            Self::Lines(query) => query,
            Self::Names(query) => panic!("expected lines, got {query:?}"),
        }
    }
}

/// What a search found: a file or a line of it, as ripgrep reports them, or a file or a
/// directory whose name fd matched at `matches`, byte ranges in its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Finding {
    Hit(Hit),
    Entry {
        path: Vec<u8>,
        dir: bool,
        matches: Vec<Range<usize>>,
    },
}

/// What a key did in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FindEvent {
    Pending,
    Closed,
    /// The text, the files, or how the text matches changed: the search starts again.
    Search,
    /// Go to the file `name` in the directory at `location`.
    Go {
        location: Location,
        name: Vec<u8>,
    },
    /// Open the directory at `location`, found by its name.
    Open {
        location: Location,
    },
    /// View the file `name` in the directory at `location`, from `line` (1-based), the line
    /// under the cursor or the first that matched.
    View {
        location: Location,
        name: Vec<u8>,
        line: Option<u64>,
    },
    /// Edit the file `name` in the directory at `location`, at `line`, likewise.
    Edit {
        location: Location,
        name: Vec<u8>,
        line: Option<u64>,
    },
}

/// How a search ended: whether it stopped at `[search] max_results`, and how many errors
/// ripgrep told of, such as files it could not read, with the last of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FindSummary {
    pub(crate) limit: bool,
    pub(crate) errors: usize,
    pub(crate) last_error: Option<String>,
}

/// A line that matched.
#[derive(Debug)]
struct LineRow {
    number: u64,
    /// Its text, terminal-safe, without the blanks it starts with, in parts that match or not.
    parts: Vec<(String, bool)>,
}

/// A file found, with the lines of it that matched; or a file or a directory found by name.
#[derive(Debug)]
struct FileRow {
    /// Relative to where the search started, its names joined by `/`.
    path: Vec<u8>,
    /// Its icon, or its marker, and its path, terminal-safe.
    shown: String,
    lines: Vec<LineRow>,
    /// Found by name: whether it is a directory, its icon or its marker, and its path in parts
    /// that match or not.
    entry: Option<EntryRow>,
}

/// A file or a directory that fd found.
#[derive(Debug)]
struct EntryRow {
    dir: bool,
    prefix: String,
    parts: Vec<(String, bool)>,
}

/// A row of the list: a file, or one of its lines, by their indexes.
type Item = (usize, Option<usize>);

/// How the search goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Searching,
    Over(FindSummary),
    /// It could not search, for this reason.
    Failed(String),
}

/// What the search depends on in the dialog: the mode, the fields, and the check boxes.
type Settings = (usize, String, String, bool, bool);

/// The dialog, with what the last search found.
#[derive(Debug)]
pub(crate) struct FindWindow {
    /// Where it searches.
    root: Location,
    /// The text and the globs of the files in its fields, regular expressions and case in its
    /// check boxes, the rows in its list, and its buttons.
    dialog: Dialog,
    /// Hidden files too, as the panels show them, and how much it finds: in all, and in a
    /// file.
    hidden: bool,
    max_results: usize,
    max_per_file: usize,
    /// The search whose results it takes; those of others are dropped.
    generation: u64,
    /// The search whose results the rows show: the last one's until the next one finds
    /// something or ends.
    shown: u64,
    rows: Vec<FileRow>,
    /// The file whose lines come next, by its index in `rows`.
    current: Option<usize>,
    state: State,
    /// The row under the cursor, by its path and line, which stays on it as files come in
    /// before it; `None` is the first row.
    cursor: Option<(Vec<u8>, Option<usize>)>,
    decor: Decor,
    /// Stops the search, which the dialog does when it goes or a new one starts.
    cancel: CancellationToken,
}

impl Drop for FindWindow {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl FindWindow {
    /// The dialog of the search in `root`, shown as `title`, typing in its text, which takes
    /// as many results as `[search]` says, in all and in a file; hidden files too if `hidden`.
    pub(crate) fn new(
        (root, title): (Location, String),
        (max_results, max_per_file): (usize, usize),
        hidden: bool,
        decor: Decor,
    ) -> Self {
        let fields = [
            (fl!("find-text"), String::new()),
            (fl!("find-files"), String::new()),
        ];
        let checks = [(fl!("find-regex"), true), (fl!("find-match-case"), false)];
        let title = fl!("find-title", dir = title);
        let modes = vec![fl!("find-mode-text"), fl!("find-mode-names")];
        let dialog = Dialog::fields(&title, &fields, &checks, buttons(TEXT_MODE), WIDTH)
            .with_choices(modes, TEXT_MODE)
            .in_a_row()
            .with_list();
        Self {
            root,
            dialog,
            hidden,
            max_results,
            max_per_file,
            generation: 0,
            shown: 0,
            rows: Vec::new(),
            current: None,
            state: State::Searching,
            cursor: None,
            decor,
            cancel: CancellationToken::new(),
        }
    }

    /// Where it searches.
    pub(crate) fn root(&self) -> &Location {
        &self.root
    }

    /// What it searches for, by its mode: lines with ripgrep, or names with fd; the text, how
    /// it matches, and the globs of the files, split at blanks.
    pub(crate) fn query(&self) -> FindQuery {
        let pattern = self.dialog.text_of(TEXT).to_owned();
        let globs = self
            .dialog
            .text_of(FILES)
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let (regex, match_case) = (self.dialog.checked(REGEX), self.dialog.checked(MATCH_CASE));
        if self.dialog.chosen() == NAMES_MODE {
            return FindQuery::Names(fd::Query {
                pattern,
                globs,
                regex,
                match_case,
                hidden: self.hidden,
            });
        }
        FindQuery::Lines(ripgrep::Query {
            pattern,
            globs,
            regex,
            match_case,
            hidden: self.hidden,
            max_per_file: self.max_per_file,
        })
    }

    /// The most results a search takes.
    pub(crate) fn max_results(&self) -> usize {
        self.max_results
    }

    /// Takes the results of the search `generation` from now on, which `cancel` stops; the
    /// search before it stops.
    pub(crate) fn restart(&mut self, generation: u64, cancel: CancellationToken) {
        self.cancel.cancel();
        self.cancel = cancel;
        self.generation = generation;
        self.state = State::Searching;
    }

    /// Whether it still searches, which turns the spinner.
    pub(crate) fn searching(&self) -> bool {
        self.state == State::Searching
    }

    /// Shows the results of the search `generation` from now on, if they are not yet.
    fn show(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        if self.shown != generation {
            self.shown = generation;
            self.rows.clear();
            self.current = None;
            self.dialog.unsettle_list();
        }
        true
    }

    /// Takes what the search `generation` found.
    pub(crate) fn found(&mut self, generation: u64, found: Vec<Finding>) {
        if !self.show(generation) {
            return;
        }
        for finding in found {
            let hit = match finding {
                Finding::Hit(hit) => hit,
                Finding::Entry { path, dir, matches } => {
                    let parts = parts_of(&path, 0, &matches);
                    self.add_file(path, Some((dir, parts)));
                    continue;
                }
            };
            match hit {
                Hit::File(path) => self.add_file(path, None),
                Hit::Line {
                    number,
                    text,
                    matches,
                } => {
                    let Some(file) = self.current else {
                        continue;
                    };
                    self.rows[file].lines.push(LineRow {
                        number,
                        parts: parts(&text, &matches),
                    });
                }
            }
        }
        self.put_cursor();
    }

    /// Puts the file at `path` among the rows, in the order of the paths; one found by name
    /// with whether it is a directory and its path in parts that match or not.
    fn add_file(&mut self, path: Vec<u8>, entry: Option<(bool, Vec<(String, bool)>)>) {
        let at = match self.rows.binary_search_by(|row| row.path.cmp(&path)) {
            Ok(at) => at,
            Err(at) => {
                let dir = entry.as_ref().is_some_and(|&(dir, _)| dir);
                let entry_of = DirEntry {
                    name: Vec::new(),
                    metadata: Metadata {
                        kind: if dir { FileKind::Dir } else { FileKind::File },
                        size: None,
                        permissions: None,
                        modified: None,
                        uid: None,
                        gid: None,
                    },
                    target_kind: None,
                };
                let prefix = self.decor.entry(&entry_of);
                let shown = format!("{prefix}{}", cells::sanitize(&path));
                let entry = entry.map(|(dir, parts)| EntryRow { dir, prefix, parts });
                self.rows.insert(
                    at,
                    FileRow {
                        path,
                        shown,
                        lines: Vec::new(),
                        entry,
                    },
                );
                at
            }
        };
        self.current = Some(at);
    }

    /// Takes how the search `generation` ended, or why it could not search.
    pub(crate) fn finished(&mut self, generation: u64, result: Result<FindSummary, String>) {
        if !self.show(generation) {
            return;
        }
        self.state = match result {
            Ok(summary) => State::Over(summary),
            Err(reason) => State::Failed(reason),
        };
        self.put_cursor();
    }

    /// The keymap context for the next key: `FindInput` while a field has the focus.
    pub(crate) fn context(&self) -> Context {
        if self.dialog.focused_field().is_some() {
            Context::FindInput
        } else {
            Context::Find
        }
    }

    /// The mode keys are typed in: the focused field's, or Normal mode elsewhere.
    pub(crate) fn mode(&self) -> Mode {
        self.dialog.mode()
    }

    /// The field with the focus, if a field has it.
    pub(crate) fn focused_field(&self) -> Option<&Field> {
        self.dialog.focused_field()
    }

    /// The operator that the focused field has waiting for its motion.
    pub(crate) fn pending(&self) -> Option<String> {
        self.dialog.pending()
    }

    /// Gives up the command of Normal mode that the focused field has waiting.
    pub(crate) fn give_up(&mut self) {
        self.dialog.give_up();
    }

    /// What the search depends on now.
    fn settings(&self) -> Settings {
        (
            self.dialog.chosen(),
            self.dialog.text_of(TEXT).to_owned(),
            self.dialog.text_of(FILES).to_owned(),
            self.dialog.checked(REGEX),
            self.dialog.checked(MATCH_CASE),
        )
    }

    /// Puts pasted `text` in the focused field, as one change.
    pub(crate) fn paste(&mut self, text: &str, registers: &mut Registers) -> FindEvent {
        let before = self.settings();
        self.dialog.paste(text, registers);
        self.changed(&before)
    }

    /// `Search` if the fields or the check boxes changed since `before`.
    fn changed(&self, before: &Settings) -> FindEvent {
        if self.settings() == *before {
            FindEvent::Pending
        } else {
            FindEvent::Search
        }
    }

    /// Every row, in the order they show.
    fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        for (file, row) in self.rows.iter().enumerate() {
            items.push((file, None));
            items.extend((0..row.lines.len()).map(|line| (file, Some(line))));
        }
        items
    }

    /// The index of the row under the cursor among `items`.
    fn cursor_in(&self, items: &[Item]) -> usize {
        let Some((path, line)) = &self.cursor else {
            return 0;
        };
        let Ok(file) = self.rows.binary_search_by(|row| row.path.cmp(path)) else {
            return 0;
        };
        let line = line.filter(|&line| line < self.rows[file].lines.len());
        items
            .iter()
            .position(|&item| item == (file, line))
            .unwrap_or(0)
    }

    /// Gives the list of the dialog its rows, with the cursor on the row it was on.
    fn put_cursor(&mut self) {
        let items = self.items();
        let cursor = self.cursor_in(&items);
        self.dialog.set_list_len(items.len());
        self.dialog.set_list_cursor(cursor);
    }

    /// Keeps the row that the cursor of the list is on, if it moved from `before`.
    fn take_cursor(&mut self, before: usize) {
        let index = self.dialog.list_cursor();
        if index == before {
            return;
        }
        let items = self.items();
        self.cursor = items
            .get(index)
            .map(|&(file, line)| (self.rows[file].path.clone(), line));
    }

    /// The file and line of the row under the cursor.
    fn chosen(&self) -> Option<(&FileRow, Option<u64>)> {
        let items = self.items();
        let &(file, line) = items.get(self.dialog.list_cursor())?;
        let row = &self.rows[file];
        let line = match line {
            Some(line) => Some(row.lines[line].number),
            None => row.lines.first().map(|line| line.number),
        };
        Some((row, line))
    }

    /// The directory of the file at `path`, and its name.
    fn place(&self, path: &[u8]) -> (Location, Vec<u8>) {
        let (dir, name) = match path.iter().rposition(|&byte| byte == b'/') {
            Some(slash) => (&path[..slash], &path[slash + 1..]),
            None => (&[][..], path),
        };
        let location = dir
            .split(|&byte| byte == b'/')
            .filter(|name| !name.is_empty())
            .fold(self.root.clone(), |location, name| child(&location, name));
        (location, name.to_vec())
    }

    /// What `button` does to the row under the cursor: go to its file, or into a directory
    /// found by name, view it, or edit it.
    fn press(&self, button: Button) -> FindEvent {
        let Some((row, line)) = self.chosen() else {
            return FindEvent::Pending;
        };
        let (location, name) = self.place(&row.path);
        let dir = row.entry.as_ref().is_some_and(|entry| entry.dir);
        match button {
            Button::GoTo if dir => FindEvent::Open {
                location: child(&location, &name),
            },
            Button::View | Button::Edit if dir => FindEvent::Pending,
            Button::GoTo => FindEvent::Go { location, name },
            Button::View => FindEvent::View {
                location,
                name,
                line,
            },
            Button::Edit => FindEvent::Edit {
                location,
                name,
                line,
            },
            _ => FindEvent::Closed,
        }
    }

    /// Takes a key as a dialog does: the fields edit, Tab and the motions move the focus, and
    /// over the list they move its cursor; Enter presses Go to, the default. F3 and F4 view and
    /// edit the file of the row, and Alt+r and Alt+c switch the check boxes, from anywhere.
    pub(crate) fn handle(&mut self, typed: Typed, registers: &mut Registers) -> FindEvent {
        let before = self.settings();
        let cursor = self.dialog.list_cursor();
        let event = match typed.resolved {
            Resolved::Action(Action::View) => return self.press(Button::View),
            Resolved::Action(Action::Edit) => return self.press(Button::Edit),
            Resolved::Action(Action::ToggleRegex) => {
                self.dialog.toggle_check(REGEX);
                DialogEvent::Pending
            }
            Resolved::Action(Action::MatchCase) => {
                self.dialog.toggle_check(MATCH_CASE);
                DialogEvent::Pending
            }
            _ => self.dialog.handle_typed(typed, registers),
        };
        self.take_cursor(cursor);
        self.event(event, &before)
    }

    /// The buttons of the mode chosen now.
    fn follow_mode(&mut self, before: &Settings) {
        let mode = self.dialog.chosen();
        if mode != before.0 {
            self.dialog.set_buttons(buttons(mode));
        }
    }

    /// What `event` of the dialog does, its settings `before` it.
    fn event(&mut self, event: DialogEvent, before: &Settings) -> FindEvent {
        self.follow_mode(before);
        match event {
            DialogEvent::Pending => self.changed(before),
            DialogEvent::Cancelled | DialogEvent::Pressed(Button::Cancel) => FindEvent::Closed,
            DialogEvent::Pressed(button) => self.press(button),
        }
    }

    /// The scroll bar of the list at the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.dialog.list_bar()
    }

    /// Shows the rows from `top`, where the thumb of the scroll bar was dragged.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        let cursor = self.dialog.list_cursor();
        self.dialog.scroll_list_to(top);
        self.take_cursor(cursor);
    }

    /// A step of the wheel, as `action` `count` times: it scrolls the rows.
    pub(crate) fn wheel(&mut self, action: Action, count: usize) {
        let cursor = self.dialog.list_cursor();
        self.dialog.wheel_list(action, count);
        self.take_cursor(cursor);
    }

    /// Takes a press of the mouse, as a dialog does: a double click on a row goes to its file.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> FindEvent {
        let before = self.settings();
        let cursor = self.dialog.list_cursor();
        let event = self.dialog.pointer(pointer);
        self.take_cursor(cursor);
        self.event(event, &before)
    }

    /// The lines under the rows: how many it found, with the spinner of `tick` while it
    /// searches; that it stopped at the limit; and what went wrong.
    fn status(&self, tick: u64) -> Vec<String> {
        let count = self.count();
        let mut status = vec![if self.searching() {
            format!("{} {count}", decor::spinner(tick))
        } else {
            count
        }];
        let State::Over(summary) = &self.state else {
            return status;
        };
        if summary.limit {
            status.push(fl!("find-limit", count = self.max_results));
        }
        if summary.errors > 0 && !self.rows.is_empty() {
            let last = summary.last_error.clone().unwrap_or_default();
            status.push(fl!(
                "find-errors",
                count = summary.errors,
                last = cells::sanitize(last.as_bytes())
            ));
        }
        status
    }

    /// What shows in place of the rows: while it searches, that nothing was found, or why.
    fn message(&self) -> Option<String> {
        match &self.state {
            State::Failed(reason) => Some(fl!(
                "find-failed",
                reason = cells::sanitize(reason.as_bytes())
            )),
            _ if !self.rows.is_empty() => None,
            State::Searching => Some(fl!("find-searching")),
            State::Over(FindSummary {
                last_error: Some(error),
                ..
            }) => Some(cells::sanitize(error.as_bytes())),
            State::Over(_) => Some(fl!("find-none")),
        }
    }

    /// How many it found: lines in files, files, or files and directories by name.
    fn count(&self) -> String {
        let lines: usize = self.rows.iter().map(|row| row.lines.len()).sum();
        if self.rows.first().is_some_and(|row| row.entry.is_some()) {
            fl!("find-count-entries", count = self.rows.len())
        } else if lines == 0 {
            fl!("find-count-files", files = self.rows.len())
        } else {
            fl!("find-count", lines = lines, files = self.rows.len())
        }
    }

    /// Draws the dialog centered over `area`, the panels, as tall as they are: the rows in its
    /// list, scrolled to the cursor, and under them how many it found, with the spinner of
    /// `tick` while it searches, and what kept it from finding all.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme, tick: u64) {
        let items = self.items();
        let row = |index: usize, width: usize, style: Style| {
            let Some(&(file, number)) = items.get(index) else {
                return Line::default();
            };
            let row = &self.rows[file];
            let spans = match (number, &row.entry) {
                (None, Some(entry)) => entry_spans(entry, width, style, theme),
                (None, None) => file_spans(row, (width, self.max_per_file), style, theme),
                (Some(number), _) => line_spans(row, number, width, style, theme),
            };
            Line::from(spans).style(style)
        };
        let view = ListView {
            row: &row,
            note: self.message(),
            status: self.status(tick),
        };
        self.dialog.render_list(frame, area, theme, &view);
    }
}

/// The buttons of `mode`: by name, nothing to view or edit at a line.
fn buttons(mode: usize) -> Vec<Button> {
    if mode == NAMES_MODE {
        vec![Button::GoTo, Button::Cancel]
    } else {
        vec![Button::GoTo, Button::View, Button::Edit, Button::Cancel]
    }
}

/// The text of a line that matched, without the blanks it starts with, in parts that are a
/// match of `matches`, byte ranges in `text`, or not; each terminal-safe.
fn parts(text: &[u8], matches: &[Range<usize>]) -> Vec<(String, bool)> {
    let start = text
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(text.len());
    parts_of(text, start, matches)
}

/// `text` from byte `start` on, in parts that are a match of `matches`, byte ranges in `text`,
/// or not; each terminal-safe.
fn parts_of(text: &[u8], start: usize, matches: &[Range<usize>]) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    let mut at = start;
    for range in matches {
        let (from, to) = (range.start.max(at), range.end.min(text.len()));
        if from >= to {
            continue;
        }
        if at < from {
            parts.push((cells::sanitize(&text[at..from]), false));
        }
        parts.push((cells::sanitize(&text[from..to]), true));
        at = to;
    }
    if at < text.len() {
        parts.push((cells::sanitize(&text[at..]), false));
    }
    parts
}

/// The row of a file or a directory found by name in `width` cells: its icon or marker, then
/// its path with what matched highlighted, losing its start where it is too long, as its name
/// is at the end.
fn entry_spans(entry: &EntryRow, width: usize, style: Style, theme: &Theme) -> Vec<Span<'static>> {
    let room = width.saturating_sub(2 + cells::width(&entry.prefix));
    let path = entry.parts.iter().map(|(part, matched)| {
        let style = if *matched {
            style.patch(theme.find_match)
        } else {
            style
        };
        Span::styled(part.clone(), style)
    });
    let mut spans = vec![
        Span::styled(" ", style),
        Span::styled(entry.prefix.clone(), style),
    ];
    let marks = theme.glyphs.marks;
    let fitted = cells::fit_spans_end(path.collect(), room, (style, theme.truncated_dialog), marks);
    spans.extend(fitted);
    spans.push(Span::styled(" ", style));
    spans
}

/// The row of a file in `width` cells: its path, cut in its middle where it is too long, and
/// how many of its lines matched at the right, with a mark once they are `max_per_file`, after
/// which the search takes no more of them.
fn file_spans(
    row: &FileRow,
    (width, max_per_file): (usize, usize),
    style: Style,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let room = width.saturating_sub(2);
    let lines = row.lines.len();
    let count = match lines {
        0 => String::new(),
        _ if lines >= max_per_file => format!(" {}", fl!("find-file-count-more", count = lines)),
        _ => format!(" {lines}"),
    };
    let count_width = cells::width(&count);
    let mut spans = vec![Span::styled(" ", style)];
    spans.extend(cells::fit(
        &row.shown,
        room.saturating_sub(count_width),
        Align::Left,
        (style.add_modifier(Modifier::BOLD), theme.truncated_dialog),
        theme.glyphs.marks,
    ));
    spans.push(Span::styled(count, style.add_modifier(Modifier::DIM)));
    spans.push(Span::styled(" ", style));
    spans
}

/// The row of line `index` of `row` in `width` cells: its number, right-aligned under those of
/// the file, then its text with the matches highlighted; a text too long to show whole starts
/// a little before its first match, if that would not show, and is cut at its end.
fn line_spans(
    row: &FileRow,
    index: usize,
    width: usize,
    style: Style,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let line = &row.lines[index];
    let digits = row
        .lines
        .iter()
        .map(|line| line.number.to_string().len())
        .max()
        .unwrap_or(1);
    let number = format!("   {:>digits$}  ", line.number);
    let room = width.saturating_sub(cells::width(&number) + 1);
    let marks = theme.glyphs.marks;
    let mut parts: Vec<(String, bool)> = line.parts.clone();
    let first_match_end: usize = parts
        .iter()
        .scan(0, |at, (text, matched)| {
            *at += cells::width(text);
            Some((*at, *matched))
        })
        .find(|(_, matched)| *matched)
        .map_or(0, |(end, _)| end);
    let mut text = Vec::new();
    if first_match_end > room {
        // Drop what comes before, but a few cells, and say so with the mark.
        let mut skip = first_match_end - room + BEFORE_MATCH.min(room / 2) + 1;
        while skip > 0 && !parts.is_empty() {
            let cells_of = cells::width(&parts[0].0);
            if cells_of <= skip {
                skip -= cells_of;
                parts.remove(0);
            } else {
                let (_, rest) = cells::split(&parts[0].0, skip);
                parts[0].0 = rest;
                skip = 0;
            }
        }
        text.push(Span::styled(
            marks.before,
            style.patch(theme.truncated_dialog),
        ));
    }
    text.extend(parts.into_iter().map(|(part, matched)| {
        if matched {
            Span::styled(part, style.patch(theme.find_match))
        } else {
            Span::styled(part, style)
        }
    }));
    let mut spans = vec![Span::styled(number, style.add_modifier(Modifier::DIM))];
    spans.extend(cells::fit_spans(
        text,
        room,
        (style, theme.truncated_dialog),
        marks,
    ));
    spans.push(Span::styled(" ", style));
    spans
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn window() -> FindWindow {
        FindWindow::new(
            (Location::Local("/src/noc".into()), "~/src/noc".to_owned()),
            (50, 2),
            true,
            Decor::new(false),
        )
    }

    fn line(number: u64, text: &str, matches: &[Range<usize>]) -> Finding {
        Finding::Hit(Hit::Line {
            number,
            text: text.as_bytes().to_vec(),
            matches: matches.to_vec(),
        })
    }

    fn file(path: &str) -> Finding {
        Finding::Hit(Hit::File(path.as_bytes().to_vec()))
    }

    fn entry(path: &str, dir: bool, matches: &[Range<usize>]) -> Finding {
        Finding::Entry {
            path: path.as_bytes().to_vec(),
            dir,
            matches: matches.to_vec(),
        }
    }

    fn hits() -> Vec<Finding> {
        vec![
            file("crates/noc/src/tui/app.rs"),
            line(
                2599,
                "    fn handle(&mut self, input: Resolved) {",
                &[7..13],
            ),
            line(3700, "    fn handle_find(&mut self) {", &[7..13]),
            file("Cargo.toml"),
            line(4, "handle = 1", &[0..6]),
        ]
    }

    fn press(window: &mut FindWindow, action: Action) -> FindEvent {
        let typed = Typed::from(Resolved::Action(action));
        window.handle(typed, &mut Registers::default())
    }

    fn typed(window: &mut FindWindow, text: &str) {
        for c in text.chars() {
            let typed = Typed::from(Resolved::Insert(c));
            let event = window.handle(typed, &mut Registers::default());
            assert_eq!(event, FindEvent::Search);
        }
    }

    /// Tab from the text to the rows: past the files and the two check boxes.
    fn to_rows(window: &mut FindWindow) {
        for _ in 0..4 {
            press(window, Action::NextField);
        }
        assert_eq!(window.context(), Context::Find);
    }

    fn draw(window: &mut FindWindow, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), &Theme::terminal(), 1))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn the_fields_and_the_check_boxes_make_the_query_as_in_a_dialog() {
        let mut window = window();
        assert_eq!(
            (window.mode(), window.context()),
            (Mode::Insert, Context::FindInput)
        );
        typed(&mut window, "fn x");
        // Down goes to the next field, as in every dialog.
        press(&mut window, Action::Down);
        typed(&mut window, "*.rs  !target");
        assert_eq!(press(&mut window, Action::Backspace), FindEvent::Search);
        assert_eq!(press(&mut window, Action::ToggleRegex), FindEvent::Search);
        let query = window.query();
        let query = query.lines();
        assert_eq!(query.pattern, "fn x");
        assert_eq!(query.globs, ["*.rs", "!targe"]);
        assert!(!query.regex && !query.match_case && query.hidden);
        assert_eq!(query.max_per_file, 2);
        // Tab goes on to the check boxes, where Space switches one.
        press(&mut window, Action::NextField);
        press(&mut window, Action::NextField);
        assert_eq!(window.context(), Context::Find);
        assert_eq!(press(&mut window, Action::Toggle), FindEvent::Search);
        assert!(window.query().lines().match_case);
        press(&mut window, Action::Up);
        press(&mut window, Action::Up);
        press(&mut window, Action::Up);
        assert_eq!(
            window.mode(),
            Mode::Normal,
            "a motion brings it in Normal mode"
        );
        press(&mut window, Action::Append);
        typed(&mut window, "y");
        assert_eq!(window.query().lines().pattern, "fn xy");
        // Esc leaves Insert mode in the field, and again closes the dialog.
        press(&mut window, Action::NormalMode);
        assert_eq!(
            (window.mode(), window.context()),
            (Mode::Normal, Context::FindInput)
        );
        assert_eq!(press(&mut window, Action::Cancel), FindEvent::Closed);
    }

    #[test]
    fn files_come_in_path_order_with_their_lines_and_keys_act_on_the_row() {
        let mut window = window();
        window.restart(3, CancellationToken::new());
        window.found(3, hits());
        window.found(2, vec![file("stale.rs")]);
        assert_eq!(window.rows.len(), 2, "an earlier search's are dropped");
        assert_eq!(window.rows[0].path, b"Cargo.toml");
        // The first row is Cargo.toml; F3 views it at its first line, from the field too.
        assert_eq!(
            press(&mut window, Action::View),
            FindEvent::View {
                location: Location::Local("/src/noc".into()),
                name: b"Cargo.toml".to_vec(),
                line: Some(4)
            }
        );
        // Down in the rows moves their cursor, and Enter there presses Go to.
        to_rows(&mut window);
        for _ in 0..3 {
            press(&mut window, Action::Down);
        }
        assert_eq!(
            press(&mut window, Action::Edit),
            FindEvent::Edit {
                location: Location::Local("/src/noc/crates/noc/src/tui".into()),
                name: b"app.rs".to_vec(),
                line: Some(2599)
            }
        );
        // A file that comes in before the cursor leaves it on its row.
        window.found(3, vec![file("README.md")]);
        assert_eq!(
            press(&mut window, Action::Confirm),
            FindEvent::Go {
                location: Location::Local("/src/noc/crates/noc/src/tui".into()),
                name: b"app.rs".to_vec()
            }
        );
        // The next search keeps the rows until it finds something.
        window.restart(4, CancellationToken::new());
        assert_eq!(window.rows.len(), 3);
        window.finished(4, Ok(FindSummary::default()));
        assert!(window.rows.is_empty() && !window.searching());
        assert_eq!(press(&mut window, Action::Confirm), FindEvent::Pending);
        // Tab leaves the rows for the buttons, where Cancel closes.
        press(&mut window, Action::NextField);
        for _ in 0..3 {
            press(&mut window, Action::Right);
        }
        assert_eq!(press(&mut window, Action::Confirm), FindEvent::Closed);
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn by_name_it_finds_files_and_directories_and_goes_into_one() {
        let mut window = window();
        // Shift+Tab from the text goes to the modes, where Space chooses one.
        press(&mut window, Action::PrevField);
        assert_eq!(press(&mut window, Action::Toggle), FindEvent::Search);
        let FindQuery::Names(query) = window.query() else {
            panic!("expected a search by name");
        };
        assert!(query.pattern.is_empty() && query.regex && query.hidden);
        window.restart(1, CancellationToken::new());
        window.found(
            1,
            vec![
                entry("src/find.rs", false, &[4..8]),
                entry("src/find", true, &[4..8]),
                entry("Find.md", false, &[0..4]),
            ],
        );
        window.finished(1, Ok(FindSummary::default()));
        let screen = draw(&mut window, 70);
        assert!(screen.contains("Files and dirs"), "{screen}");
        assert!(screen.contains("3 files and directories"), "{screen}");
        assert!(
            screen.contains("Go to") && !screen.contains("View"),
            "{screen}"
        );
        to_rows_from_the_modes(&mut window);
        // Find.md first, then src/find, a directory, which Go to goes into, and F3 views none.
        assert_eq!(
            press(&mut window, Action::Confirm),
            FindEvent::Go {
                location: Location::Local("/src/noc".into()),
                name: b"Find.md".to_vec()
            }
        );
        press(&mut window, Action::Down);
        assert_eq!(press(&mut window, Action::View), FindEvent::Pending);
        assert_eq!(
            press(&mut window, Action::Confirm),
            FindEvent::Open {
                location: Location::Local("/src/noc/src/find".into())
            }
        );
        press(&mut window, Action::Down);
        assert_eq!(
            press(&mut window, Action::Edit),
            FindEvent::Edit {
                location: Location::Local("/src/noc/src".into()),
                name: b"find.rs".to_vec(),
                line: None
            }
        );
        // Back to the text, the first mode, six stops before the rows: its buttons come back.
        for _ in 0..6 {
            press(&mut window, Action::PrevField);
        }
        press(&mut window, Action::Toggle);
        assert!(matches!(window.query(), FindQuery::Lines(_)));
        assert!(draw(&mut window, 70).contains("View"));
    }

    /// Tab from the second mode to the rows: past the fields and the check boxes.
    fn to_rows_from_the_modes(window: &mut FindWindow) {
        for _ in 0..5 {
            press(window, Action::NextField);
        }
        assert_eq!(window.context(), Context::Find);
    }

    #[test]
    fn a_long_path_found_by_name_keeps_its_name() {
        let mut window = window();
        window.restart(1, CancellationToken::new());
        let path = format!("{}/needle.txt", "deep/".repeat(30));
        let at = path.len() - "needle.txt".len();
        window.found(1, vec![entry(&path, false, &[at..at + 6])]);
        window.finished(1, Ok(FindSummary::default()));
        let screen = draw(&mut window, 60);
        assert!(screen.contains("needle.txt"), "{screen}");
    }

    #[test]
    fn a_new_search_or_closing_stops_the_last() {
        let mut window = window();
        let first = CancellationToken::new();
        window.restart(1, first.clone());
        let second = CancellationToken::new();
        window.restart(2, second.clone());
        assert!(first.is_cancelled() && !second.is_cancelled());
        drop(window);
        assert!(second.is_cancelled());
    }

    #[test]
    fn parts_drop_leading_blanks_and_split_at_the_matches() {
        assert_eq!(
            parts(b"\t  let a = a\x07b;", &[1..2, 7..8, 11..12, 40..50]),
            [
                ("let ".to_owned(), false),
                ("a".to_owned(), true),
                (" = ".to_owned(), false),
                ("a".to_owned(), true),
                (cells::sanitize(b"\x07b;"), false),
            ]
        );
    }

    #[test]
    fn draws_the_fields_and_the_files_with_their_lines() {
        let mut window = window();
        typed(&mut window, "handle");
        window.restart(1, CancellationToken::new());
        window.found(1, hits());
        window.finished(
            1,
            Ok(FindSummary {
                limit: true,
                errors: 2,
                last_error: Some("rg: ./secret: Permission denied (os error 13)".to_owned()),
            }),
        );
        insta::assert_snapshot!(draw(&mut window, 70));
    }

    #[test]
    fn a_match_has_a_color_of_its_own_on_the_cursor_too() {
        let mut window = window();
        window.restart(1, CancellationToken::new());
        window.found(1, hits());
        let theme = Theme::mc_classic();
        let row = &window.rows[1];
        for style in [theme.dialog, theme.cursor] {
            let spans = line_spans(row, 0, 60, style, &theme);
            let found = spans.iter().find(|span| span.content == "handle").unwrap();
            assert_eq!(found.style, style.patch(theme.find_match));
            assert_ne!(found.style.bg, theme.cursor.bg);
        }
    }

    #[test]
    fn a_long_line_starts_near_its_match() {
        let mut window = window();
        window.restart(1, CancellationToken::new());
        let text = format!("{}needle and more", "x".repeat(200));
        window.found(1, vec![file("a"), line(1, &text, &[200..206])]);
        window.finished(1, Ok(FindSummary::default()));
        let screen = draw(&mut window, 60);
        assert!(screen.contains("needle"), "{screen}");
    }

    #[test]
    fn says_why_it_found_nothing() {
        let mut window = window();
        window.restart(1, CancellationToken::new());
        assert!(draw(&mut window, 60).contains("Searching…"));
        window.finished(
            1,
            Ok(FindSummary {
                limit: false,
                errors: 1,
                last_error: Some("error: unclosed group".to_owned()),
            }),
        );
        assert!(draw(&mut window, 60).contains("error: unclosed group"));
        window.restart(2, CancellationToken::new());
        window.finished(2, Err("rg: command not found".to_owned()));
        assert!(draw(&mut window, 60).contains("Cannot search here: rg: command not found"));
        window.restart(3, CancellationToken::new());
        window.finished(3, Ok(FindSummary::default()));
        assert!(draw(&mut window, 60).contains("Nothing found"));
    }
}
