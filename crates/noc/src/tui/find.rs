//! The search of a whole tree: `g /` in a panel, or F9 → Command → Find file…. ripgrep searches
//! the files under the panel's directory, local or on a host, for the text of the first field,
//! among the files that the globs of the second let through, again as either changes. The window
//! shows each file it found, in the order of their paths, with the lines that match under it,
//! the matches highlighted; with no text it lists the files. Enter goes to the file in the panel,
//! F3 and F4 view and edit it at the line, as in the results of mc's Find File.

use noc_scroll::scrollbar::{self, Bar};
use noc_tools::ripgrep::{Hit, Query};
use noc_vfs::{DirEntry, FileKind, Location, Metadata};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tokio_util::sync::CancellationToken;

use super::cells::{self, Align, Cut};
use super::decor::{self, Decor};
use super::dialog::{Colors, Field, draw_box, frame_around, right_frame};
use super::glyphs;
use super::keymap::{Action, Mode, Resolved};
use super::mouse::{Drawn, Pointer, Press};
use super::panel::child;
use super::scroll::Window;
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the window gets, in cells, borders included.
const WIDTH: u16 = 120;
/// Cells of text kept before the first match of a line that is too long to show whole.
const BEFORE_MATCH: usize = 8;

/// What a key did in the window.
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

/// A file found, with the lines of it that matched.
#[derive(Debug)]
struct FileRow {
    /// Relative to where the search started, its names joined by `/`.
    path: Vec<u8>,
    /// Its icon, or its marker, and its path, terminal-safe.
    shown: String,
    lines: Vec<LineRow>,
}

/// A row of the window: a file, or one of its lines, by their indexes.
type Item = (usize, Option<usize>);

/// Where keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Text,
    Files,
    Rows,
}

/// How the search goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Searching,
    Over(FindSummary),
    /// It could not search, for this reason.
    Failed(String),
}

/// The window, with what the last search found.
#[derive(Debug)]
pub(crate) struct FindWindow {
    /// Where it searches, and that as the panel's title shows it.
    root: Location,
    title: String,
    text: Field,
    files: Field,
    focus: Focus,
    /// The field that `i` and `/` go back to from the rows.
    field: Focus,
    regex: bool,
    match_case: bool,
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
    window: Window,
    page: usize,
    /// The count before the next Ctrl+f or Ctrl+b.
    pages: usize,
    drawn: Drawn,
    /// Where the fields were drawn, and their room, for the mouse.
    fields: [(Rect, usize); 2],
    bar: Option<Bar>,
    decor: Decor,
    /// Stops the search, which the window does when it goes or a new one starts.
    cancel: CancellationToken,
}

impl Drop for FindWindow {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl FindWindow {
    /// The window of the search in `root`, shown as `title`, typing in its text, which takes
    /// as many results as `[search]` says, in all and in a file; hidden files too if `hidden`.
    pub(crate) fn new(
        (root, title): (Location, String),
        (max_results, max_per_file): (usize, usize),
        hidden: bool,
        decor: Decor,
    ) -> Self {
        Self {
            root,
            title,
            text: Field::plain(""),
            files: Field::plain(""),
            focus: Focus::Text,
            field: Focus::Text,
            regex: true,
            match_case: false,
            hidden,
            max_results,
            max_per_file,
            generation: 0,
            shown: 0,
            rows: Vec::new(),
            current: None,
            state: State::Searching,
            cursor: None,
            window: Window::default(),
            page: 1,
            pages: 1,
            drawn: Drawn::default(),
            fields: [(Rect::default(), 0); 2],
            bar: None,
            decor,
            cancel: CancellationToken::new(),
        }
    }

    /// Where it searches.
    pub(crate) fn root(&self) -> &Location {
        &self.root
    }

    /// What ripgrep searches for: the text, how it matches, and the globs of the files, split
    /// at blanks.
    pub(crate) fn query(&self) -> Query {
        Query {
            pattern: self.text.text().to_owned(),
            globs: self
                .files
                .text()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
            regex: self.regex,
            match_case: self.match_case,
            hidden: self.hidden,
            max_per_file: self.max_per_file,
        }
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
            self.window.unsettle();
        }
        true
    }

    /// Takes what the search `generation` found.
    pub(crate) fn found(&mut self, generation: u64, hits: Vec<Hit>) {
        if !self.show(generation) {
            return;
        }
        for hit in hits {
            match hit {
                Hit::File(path) => self.add_file(path),
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
    }

    /// Puts the file at `path` among the rows, in the order of the paths.
    fn add_file(&mut self, path: Vec<u8>) {
        let at = match self.rows.binary_search_by(|row| row.path.cmp(&path)) {
            Ok(at) => at,
            Err(at) => {
                let entry = DirEntry {
                    name: Vec::new(),
                    metadata: Metadata {
                        kind: FileKind::File,
                        size: None,
                        permissions: None,
                        modified: None,
                        uid: None,
                        gid: None,
                    },
                    target_kind: None,
                };
                let shown = format!("{}{}", self.decor.entry(&entry), cells::sanitize(&path));
                self.rows.insert(
                    at,
                    FileRow {
                        path,
                        shown,
                        lines: Vec::new(),
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
    }

    /// The mode keys are typed in: Insert in the fields, Normal on the rows.
    pub(crate) fn mode(&self) -> Mode {
        if self.focus == Focus::Rows {
            Mode::Normal
        } else {
            Mode::Insert
        }
    }

    /// The field that takes what is typed, if one does.
    fn field_mut(&mut self) -> Option<&mut Field> {
        match self.focus {
            Focus::Text => Some(&mut self.text),
            Focus::Files => Some(&mut self.files),
            Focus::Rows => None,
        }
    }

    /// Puts pasted `text` in the field that takes what is typed, without its control
    /// characters.
    pub(crate) fn paste(&mut self, text: &str) -> FindEvent {
        let Some(field) = self.field_mut() else {
            return FindEvent::Pending;
        };
        text.chars()
            .filter(|c| !c.is_control())
            .for_each(|c| field.insert(c));
        FindEvent::Search
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

    /// Puts the cursor on the row at `index` of `items`.
    fn set_cursor(&mut self, items: &[Item], index: usize) {
        self.cursor = items
            .get(index)
            .map(|&(file, line)| (self.rows[file].path.clone(), line));
    }

    /// The file and line of the row under the cursor.
    fn chosen(&self) -> Option<(&FileRow, Option<u64>)> {
        let items = self.items();
        let &(file, line) = items.get(self.cursor_in(&items))?;
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

    /// Takes the count before Ctrl+f or Ctrl+b, which scroll that many pages at once.
    pub(crate) fn count_pages(&mut self, pages: usize) {
        self.pages = pages.max(1);
    }

    /// Takes a key: in a field, characters and the keys of text fields edit it, and Tab and
    /// Shift+Tab go to the other one; Esc goes to the rows, where `i` and `/` go back. Up, Down,
    /// and the page keys move the cursor in both; Enter, F3, and F4 act on its row.
    pub(crate) fn handle(&mut self, input: Resolved) -> FindEvent {
        let action = match input {
            Resolved::Insert(c) => {
                return match self.field_mut() {
                    Some(field) => {
                        field.insert(c);
                        FindEvent::Search
                    }
                    None => FindEvent::Pending,
                };
            }
            Resolved::Action(action) => action,
        };
        match action {
            Action::Up
            | Action::Down
            | Action::PageUp
            | Action::PageDown
            | Action::HalfPageUp
            | Action::HalfPageDown
            | Action::Home
            | Action::End => self.go(action),
            Action::NextField | Action::PrevField => {
                self.focus = match self.focus {
                    Focus::Text => Focus::Files,
                    Focus::Files => Focus::Text,
                    Focus::Rows => self.field,
                };
                self.field = self.focus;
            }
            Action::NormalMode => self.focus = Focus::Rows,
            Action::InsertMode | Action::QuickSearch => self.focus = self.field,
            Action::ToggleRegex => {
                self.regex = !self.regex;
                return FindEvent::Search;
            }
            Action::MatchCase => {
                self.match_case = !self.match_case;
                return FindEvent::Search;
            }
            Action::Confirm => {
                return self.chosen().map_or(FindEvent::Pending, |(row, _)| {
                    let (location, name) = self.place(&row.path);
                    FindEvent::Go { location, name }
                });
            }
            Action::View | Action::Edit => {
                return self.chosen().map_or(FindEvent::Pending, |(row, line)| {
                    let (location, name) = self.place(&row.path);
                    if action == Action::View {
                        FindEvent::View {
                            location,
                            name,
                            line,
                        }
                    } else {
                        FindEvent::Edit {
                            location,
                            name,
                            line,
                        }
                    }
                });
            }
            Action::Cancel => return FindEvent::Closed,
            action => {
                if let Some(field) = self.field_mut() {
                    let before = field.text().to_owned();
                    if field.edit(action) && field.text() != before {
                        return FindEvent::Search;
                    }
                }
            }
        }
        FindEvent::Pending
    }

    /// Moves the cursor as `action` says, scrolling as vim's window does for the page keys.
    fn go(&mut self, action: Action) {
        let items = self.items();
        if items.is_empty() {
            return;
        }
        let last = items.len() - 1;
        let mut cursor = self.cursor_in(&items);
        let page = self.page.max(1);
        match action {
            Action::Up => cursor = cursor.saturating_sub(1),
            Action::Down => cursor = (cursor + 1).min(last),
            Action::Home => cursor = 0,
            Action::End => cursor = last,
            Action::PageUp | Action::PageDown => {
                let pages = std::mem::replace(&mut self.pages, 1);
                let down = action == Action::PageDown;
                self.window
                    .page(down, pages, &mut cursor, items.len(), page);
            }
            _ => {
                let down = action == Action::HalfPageDown;
                self.window.half_page(down, &mut cursor, items.len(), page);
            }
        }
        self.set_cursor(&items, cursor);
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// Shows the rows from `top`, where the thumb of the scroll bar was dragged.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        let items = self.items();
        let mut cursor = self.cursor_in(&items);
        self.window
            .scroll_to(top, &mut cursor, items.len(), self.page.max(1));
        self.set_cursor(&items, cursor);
    }

    /// Takes a press of the mouse, where the window was drawn last: a click in a field types
    /// there, a click puts the cursor on a row, and returns the key that a double click on one,
    /// or a click outside the window, stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        if pointer.press == Press::Click {
            for (index, (area, room)) in self.fields.iter().enumerate() {
                if area.contains(pointer.at) {
                    let column = usize::from(pointer.at.x - area.x);
                    self.focus = if index == 0 {
                        Focus::Text
                    } else {
                        Focus::Files
                    };
                    self.field = self.focus;
                    let room = *room;
                    if let Some(field) = self.field_mut() {
                        field.click(column, room);
                    }
                    return None;
                }
            }
        }
        let items = self.items();
        let mut cursor = self.cursor_in(&items);
        let action = self.drawn.menu_press(pointer, &mut cursor);
        self.set_cursor(&items, cursor);
        action
    }

    /// The lines under the rows: that it stopped at the limit, and what went wrong.
    fn footer(&self) -> Vec<String> {
        let State::Over(summary) = &self.state else {
            return Vec::new();
        };
        let mut footer = Vec::new();
        if summary.limit {
            footer.push(fl!("find-limit", count = self.max_results));
        }
        if summary.errors > 0 && !self.rows.is_empty() {
            let last = summary.last_error.clone().unwrap_or_default();
            footer.push(fl!(
                "find-errors",
                count = summary.errors,
                last = cells::sanitize(last.as_bytes())
            ));
        }
        footer
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

    /// How many it found: lines in files, or files.
    fn count(&self) -> String {
        let lines: usize = self.rows.iter().map(|row| row.lines.len()).sum();
        if lines == 0 {
            fl!("find-count-files", files = self.rows.len())
        } else {
            fl!("find-count", lines = lines, files = self.rows.len())
        }
    }

    /// Draws the window centered over `area`, the panels: the fields, with whether the text is
    /// a regular expression and matches case, how many it found, and the spinner of `tick`
    /// while it searches; the rows, scrolled to the cursor; and what kept it from finding all.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme, tick: u64) {
        let footer = self.footer();
        let below = if footer.is_empty() {
            0
        } else {
            u16::try_from(footer.len() + 1).unwrap_or(u16::MAX)
        };
        let colors = Colors::of(theme, false);
        let inner = draw_box(
            frame,
            area,
            (WIDTH, u16::MAX),
            &fl!("find-title", dir = self.title.clone()),
            colors,
            theme,
        );
        self.drawn = Drawn {
            frame: frame_around(inner),
            ..Drawn::default()
        };
        self.bar = None;
        if inner.height < 4 || inner.width < 8 {
            return;
        }
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        self.draw_fields(frame, inner, theme, tick);
        frame.render_widget(
            Line::styled(glyphs::LINE.repeat(width), theme.dialog),
            line(2),
        );
        let rows_top = 3;
        self.page = usize::from(inner.height.saturating_sub(rows_top + below)).max(1);
        if below > 0 {
            let top = inner.height - below;
            frame.render_widget(
                Line::styled(glyphs::LINE.repeat(width), theme.dialog),
                line(top),
            );
            for (index, text) in (top + 1..inner.height).zip(footer) {
                frame.render_widget(
                    cells::fitted(
                        &text,
                        width,
                        (Align::Left, Cut::End),
                        (theme.dialog, theme.truncated_dialog),
                        theme.glyphs.marks,
                    ),
                    line(index),
                );
            }
        }
        if let Some(message) = self.message() {
            let room = rows_top..inner.height.saturating_sub(below);
            for (index, text) in room.zip(cells::wrap(&message, width)) {
                frame.render_widget(Line::styled(text, theme.dialog), line(index));
            }
            return;
        }
        let items = self.items();
        let mut cursor = self.cursor_in(&items);
        self.window.follow(&mut cursor, items.len(), self.page);
        let top = self.window.top();
        let height = u16::try_from(self.page).unwrap_or(u16::MAX);
        let bar = right_frame(inner, inner.y + rows_top, height);
        let position = (items.len(), self.page, top);
        self.bar = scrollbar::render(frame, bar, position, theme.dialog, theme.glyphs.scroll_bar);
        let focused = colors.focused_style();
        for (offset, &(file, number)) in items.iter().enumerate().skip(top).take(self.page) {
            let y = rows_top + u16::try_from(offset - top).unwrap_or(u16::MAX);
            self.drawn.rows.push((offset, line(y)));
            let style = if offset == cursor {
                focused
            } else {
                theme.dialog
            };
            let row = &self.rows[file];
            let spans = match number {
                None => file_spans(row, (width, self.max_per_file), style, theme),
                Some(number) => line_spans(row, number, width, style, theme),
            };
            frame.render_widget(Line::from(spans).style(style), line(y));
        }
    }

    /// The two fields, with their labels lined up, on the first two lines of `inner`: the text,
    /// with the marks of regular expressions and case at its right, and the files, with how
    /// many it found and the spinner of `tick` at theirs.
    fn draw_fields(&mut self, frame: &mut Frame<'_>, inner: Rect, theme: &Theme, tick: u64) {
        let width = usize::from(inner.width);
        let labels = [fl!("find-text"), fl!("find-files")];
        let label_width = labels
            .iter()
            .map(|label| cells::width(label))
            .max()
            .unwrap_or(0)
            + 1;
        let on = |on: bool| {
            if on {
                theme.quick_search
            } else {
                theme.dialog.add_modifier(Modifier::DIM)
            }
        };
        let switches = vec![
            Span::styled(fl!("find-regex-mark"), on(self.regex)),
            Span::styled(" ", theme.dialog),
            Span::styled(fl!("find-case-mark"), on(self.match_case)),
        ];
        let count = self.count();
        let count = if self.searching() {
            format!("{} {count}", decor::spinner(tick))
        } else {
            count
        };
        let notes = [switches, vec![Span::styled(count, theme.dialog)]];
        let fields = [(&self.text, Focus::Text), (&self.files, Focus::Files)];
        for (index, ((field, focus), (label, note))) in
            fields.into_iter().zip(labels.iter().zip(notes)).enumerate()
        {
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            let note_width: usize = note.iter().map(Span::width).sum();
            let room = width.saturating_sub(label_width + note_width + 1);
            let (shown, column) = field.visible(room);
            let mut spans = vec![Span::styled(format!("{label:<label_width$}"), theme.dialog)];
            let shown_width = cells::width(&shown);
            spans.push(Span::styled(shown, theme.dialog));
            let gap = room.saturating_sub(shown_width) + 1;
            spans.push(Span::styled(" ".repeat(gap), theme.dialog));
            spans.extend(note);
            frame.render_widget(
                Line::from(spans).style(theme.dialog),
                Rect::new(inner.x, y, inner.width, 1),
            );
            let x = inner.x + u16::try_from(label_width).unwrap_or(0);
            let area = Rect::new(x, y, u16::try_from(room).unwrap_or(0), 1);
            self.fields[index] = (area, room);
            if self.focus == focus {
                let column = u16::try_from(column).unwrap_or(u16::MAX);
                frame.set_cursor_position(Position::new(
                    x.saturating_add(column).min(inner.right() - 1),
                    y,
                ));
            }
        }
    }
}

/// The text of a line that matched, without the blanks it starts with, in parts that are a
/// match of `matches`, byte ranges in `text`, or not; each terminal-safe.
fn parts(text: &[u8], matches: &[std::ops::Range<usize>]) -> Vec<(String, bool)> {
    let start = text
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(text.len());
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
            Span::styled(part, theme.quick_search)
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

    fn line(number: u64, text: &str, matches: &[std::ops::Range<usize>]) -> Hit {
        Hit::Line {
            number,
            text: text.as_bytes().to_vec(),
            matches: matches.to_vec(),
        }
    }

    fn file(path: &str) -> Hit {
        Hit::File(path.as_bytes().to_vec())
    }

    fn hits() -> Vec<Hit> {
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
        window.handle(Resolved::Action(action))
    }

    fn typed(window: &mut FindWindow, text: &str) {
        for c in text.chars() {
            assert_eq!(window.handle(Resolved::Insert(c)), FindEvent::Search);
        }
    }

    fn draw(window: &mut FindWindow, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 14)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), &Theme::terminal(), 1))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn the_fields_make_the_query_and_tab_goes_between_them() {
        let mut window = window();
        assert_eq!(window.mode(), Mode::Insert);
        typed(&mut window, "fn x");
        press(&mut window, Action::NextField);
        typed(&mut window, "*.rs  !target");
        assert_eq!(press(&mut window, Action::Backspace), FindEvent::Search);
        assert_eq!(press(&mut window, Action::ToggleRegex), FindEvent::Search);
        assert_eq!(press(&mut window, Action::MatchCase), FindEvent::Search);
        let query = window.query();
        assert_eq!(query.pattern, "fn x");
        assert_eq!(query.globs, ["*.rs", "!targe"]);
        assert!(!query.regex && query.match_case && query.hidden);
        assert_eq!(query.max_per_file, 2);
        press(&mut window, Action::PrevField);
        typed(&mut window, "y");
        assert_eq!(window.query().pattern, "fn xy");
        // Esc goes to the rows, where characters type nothing, and i goes back.
        press(&mut window, Action::NormalMode);
        assert_eq!(window.mode(), Mode::Normal);
        assert_eq!(window.handle(Resolved::Insert('z')), FindEvent::Pending);
        press(&mut window, Action::InsertMode);
        assert_eq!(window.mode(), Mode::Insert);
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
        // The first row is Cargo.toml; F3 views it at its first line.
        assert_eq!(
            press(&mut window, Action::View),
            FindEvent::View {
                location: Location::Local("/src/noc".into()),
                name: b"Cargo.toml".to_vec(),
                line: Some(4)
            }
        );
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
