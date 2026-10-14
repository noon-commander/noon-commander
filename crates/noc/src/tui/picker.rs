//! What the windows that list rows to choose one from share: the location menu and the tab
//! menu, drawn over their panel, the window of the saved workspaces, and the window of the
//! command history. They open in Normal mode, where keys move and act; `/` types in the filter,
//! in Insert mode, and Esc goes back to the rows with the filter kept.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;

use super::cells::{self, Align};
use super::dialog::{Colors, Field, draw_box, frame_around};
use super::fuzzy::Fuzzy;
use super::keymap::{Action, Mode, Resolved};
use super::mouse::{Drawn, Pointer};
use super::scroll::Window;
use super::theme::Theme;
use crate::i18n::fl;

/// What a key did in a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    /// Nothing for the window to do: the cursor moved, or the filter changed.
    Pending,
    /// Enter chose the row at this index among those the filter shows.
    Chosen(usize),
    Closed,
    /// A key that the picker does not take, for the window.
    Other(Action),
}

/// When a row shows, if the filter finds it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Shows {
    #[default]
    Always,
    /// Only while the filter has text, such as a row past the first ones of its group.
    Filtered,
    /// Only while the filter is empty: a row that shows more rows.
    Unfiltered,
}

/// A line of a window, under its filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MenuLine {
    /// A row, by its index among those the filter shows.
    Row(usize),
    /// A heading above the rows after it, which shows with the first of them.
    Heading(String),
}

/// What a window draws in its box.
#[derive(Debug, Default)]
pub(crate) struct Body<'a> {
    pub(crate) title: &'a str,
    /// Widest the box gets, in cells, borders included.
    pub(crate) width: u16,
    pub(crate) lines: &'a [MenuLine],
    /// Lines the box shows at most; as many as fit without.
    pub(crate) max_lines: Option<u16>,
    /// Right of the filter, such as which hosts the history shows.
    pub(crate) note: Option<String>,
    /// Shown in place of the lines, such as while the rows load; without, a note that nothing
    /// matches once the filter hides every row.
    pub(crate) message: Option<String>,
    /// Lines under the rows, apart from them, such as the whole command under the cursor.
    pub(crate) footer: Vec<String>,
}

/// A row to draw.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShownRow {
    /// Its index among the rows the filter shows.
    pub(crate) index: usize,
    /// Whether the cursor is on it.
    pub(crate) selected: bool,
    /// Cells of the line.
    pub(crate) width: usize,
}

/// The filter, the cursor, and the scrolling of a window, whose rows the window holds.
#[derive(Debug)]
pub(crate) struct Picker {
    /// What the filter finds each row by, shown or not.
    texts: Vec<Vec<String>>,
    /// When each row shows.
    shows: Vec<Shows>,
    /// The filter, a text field.
    filter: Field,
    /// The filter matches as fzf does: `ui.fuzzy_search`.
    fuzzy: bool,
    /// The filter takes what is typed, in Insert mode.
    filtering: bool,
    /// How well each row, shown or not, matches the filter, higher for better; `None` hides
    /// it.
    matches: Vec<Option<u32>>,
    /// The row under the cursor, among those the filter shows.
    cursor: usize,
    /// The lines on screen, as vim's window scrolls them, and how many at the last render.
    window: Window,
    page: usize,
    /// The row each line of the last render showed, if any: headings show none.
    rows_drawn: Vec<Option<usize>>,
    /// The count before the next Ctrl+f or Ctrl+b, which scroll that many pages at once.
    pages: usize,
    /// Where the last render drew the window, for the mouse.
    drawn: Drawn,
}

impl Picker {
    /// A picker without rows, whose filter matches as fzf does if `fuzzy`
    /// (`ui.fuzzy_search`).
    pub(crate) fn new(fuzzy: bool) -> Self {
        Self {
            texts: Vec::new(),
            shows: Vec::new(),
            filter: Field::plain(""),
            fuzzy,
            filtering: false,
            matches: Vec::new(),
            cursor: 0,
            window: Window::default(),
            page: 1,
            rows_drawn: Vec::new(),
            pages: 1,
            drawn: Drawn::default(),
        }
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    pub(crate) fn mode(&self) -> Mode {
        if self.filtering {
            Mode::Insert
        } else {
            Mode::Normal
        }
    }

    /// Matches as fzf does from now on if `fuzzy`.
    pub(crate) fn set_fuzzy(&mut self, fuzzy: bool) {
        self.fuzzy = fuzzy;
        self.refilter();
    }

    /// Takes the rows again, each by the texts the filter finds it by. The cursor stays on its
    /// index; the window puts it where it belongs.
    pub(crate) fn set_rows(&mut self, texts: Vec<Vec<String>>) {
        self.shows = vec![Shows::Always; texts.len()];
        self.texts = texts;
        self.refilter();
    }

    /// Takes the rows again, each by the texts the filter finds it by and when it shows.
    pub(crate) fn set_rows_showing(&mut self, rows: Vec<(Vec<String>, Shows)>) {
        (self.texts, self.shows) = rows.into_iter().unzip();
        self.refilter();
    }

    /// Those of `rows`, every row in its order, that the filter shows.
    pub(crate) fn shown<T>(&self, rows: impl IntoIterator<Item = T>) -> Vec<T> {
        rows.into_iter()
            .zip(&self.matches)
            .filter_map(|(row, score)| score.map(|_| row))
            .collect()
    }

    /// How many rows the filter shows.
    pub(crate) fn len(&self) -> usize {
        self.matches.iter().flatten().count()
    }

    /// The row under the cursor, by its index among those the filter shows.
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn set_cursor(&mut self, row: usize) {
        self.cursor = row;
    }

    /// Puts the cursor on the first of the rows that match best. Without `fuzzy`, all match as
    /// well, so it is the first.
    pub(crate) fn focus_best(&mut self) {
        let shown: Vec<u32> = self.matches.iter().flatten().copied().collect();
        let best = shown.iter().max();
        self.cursor = shown
            .iter()
            .position(|score| Some(score) == best)
            .unwrap_or(0);
    }

    /// Matches the rows against the filter again. A row shows if one of its texts contains the
    /// filter, ignoring case; `fuzzy`, if they match it as fzf matches a line. Then only rows
    /// for an empty filter, or for one with text, show as `shows` says.
    fn refilter(&mut self) {
        let scores: Vec<Option<u32>> = if self.fuzzy {
            let mut fuzzy = Fuzzy::names(self.filter.text());
            self.texts
                .iter()
                .map(|texts| fuzzy.score(&texts.join(" ")))
                .collect()
        } else {
            let filter = self.filter.text().to_lowercase();
            self.texts
                .iter()
                .map(|texts| {
                    texts
                        .iter()
                        .any(|text| text.to_lowercase().contains(&filter))
                        .then_some(0)
                })
                .collect()
        };
        let empty = self.filter.text().is_empty();
        self.matches = scores
            .into_iter()
            .zip(&self.shows)
            .map(|(score, shows)| match shows {
                Shows::Always => score,
                Shows::Filtered => score.filter(|_| !empty),
                Shows::Unfiltered => empty.then_some(0),
            })
            .collect();
    }

    /// Takes a key: Up, Down, the page keys, Home, and End move, Enter chooses, and Esc closes
    /// the window. `/` types in the filter: characters then filter, and the keys of text fields
    /// edit it, until Esc goes back to the rows. Other keys are the window's.
    pub(crate) fn handle(&mut self, input: Resolved) -> Pick {
        let rows = self.len();
        let last = rows.saturating_sub(1);
        match input {
            Resolved::Insert(c) if self.filtering => {
                self.filter.insert(c);
                self.refilter();
                self.focus_best();
            }
            Resolved::Insert(_) => {}
            Resolved::Action(action) => match action {
                Action::Up => self.cursor = self.cursor.saturating_sub(1),
                Action::Down => self.cursor = (self.cursor + 1).min(last),
                Action::PageUp | Action::PageDown | Action::HalfPageUp | Action::HalfPageDown => {
                    self.scroll(action);
                }
                Action::Home => self.cursor = 0,
                Action::End => self.cursor = last,
                Action::QuickSearch => self.filtering = true,
                Action::NormalMode => self.filtering = false,
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
                | Action::DeleteWord
                    if self.filtering =>
                {
                    let before = self.filter.text().to_owned();
                    self.filter.edit(action);
                    if self.filter.text() != before {
                        self.refilter();
                        self.focus_best();
                    }
                }
                Action::Confirm if self.cursor < rows => return Pick::Chosen(self.cursor),
                Action::Confirm => {}
                Action::Cancel => return Pick::Closed,
                other => return Pick::Other(other),
            },
        }
        Pick::Pending
    }

    /// Takes the count before Ctrl+f or Ctrl+b, which scroll that many pages at once, as vim's.
    pub(crate) fn count_pages(&mut self, pages: usize) {
        self.pages = pages.max(1);
    }

    /// Ctrl+b, Ctrl+f, Ctrl+u, or Ctrl+d: scrolls the lines as vim's window, headings among
    /// them, and puts the cursor on the row where vim's goes, or the next one the way it went.
    fn scroll(&mut self, action: Action) {
        let rows = self.len();
        let lines = if self.rows_drawn.is_empty() {
            rows
        } else {
            self.rows_drawn.len()
        };
        let mut line = self
            .rows_drawn
            .iter()
            .position(|&row| row == Some(self.cursor))
            .unwrap_or(self.cursor);
        let down = matches!(action, Action::PageDown | Action::HalfPageDown);
        let page = self.page.max(1);
        if matches!(action, Action::PageUp | Action::PageDown) {
            let pages = std::mem::replace(&mut self.pages, 1);
            self.window.page(down, pages, &mut line, lines, page);
        } else {
            self.window.half_page(down, &mut line, lines, page);
        }
        if self.rows_drawn.is_empty() {
            self.cursor = line.min(rows.saturating_sub(1));
            return;
        }
        let row = |line: &usize| self.rows_drawn.get(*line).copied().flatten();
        let after = (line..self.rows_drawn.len()).find_map(|line| row(&line));
        let before = (0..=line).rev().find_map(|line| row(&line));
        let found = if down {
            after.or(before)
        } else {
            before.or(after)
        };
        self.cursor = found.unwrap_or(0);
    }

    /// Takes a press of the mouse, where the window was drawn last: a click puts the cursor on
    /// a row, and returns the key that a double click on one, or a click outside the window,
    /// stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        self.drawn.menu_press(pointer, &mut self.cursor)
    }

    /// Draws the window centered over `area`: the filter, then the lines of `body`, scrolled
    /// to the cursor, or its message, or a note that nothing matches, then its footer. `row`
    /// draws a row.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        body: Body<'_>,
        row: &mut dyn FnMut(ShownRow) -> Line<'static>,
    ) {
        let lines = body.lines;
        let shown = u16::try_from(lines.len().max(1))
            .unwrap_or(u16::MAX)
            .min(body.max_lines.unwrap_or(u16::MAX));
        // A line between the rows and the footer.
        let below = match body.footer.len() {
            0 => 0,
            footer => u16::try_from(footer + 1).unwrap_or(u16::MAX),
        };
        let colors = Colors::of(theme, false);
        // Borders, the filter, the line under it, the rows, the footer.
        let size = (body.width, shown.saturating_add(4).saturating_add(below));
        let inner = draw_box(frame, area, size, body.title, colors, theme);
        self.drawn = Drawn {
            frame: frame_around(inner),
            ..Drawn::default()
        };
        if inner.height < 3 || inner.width < 4 {
            return;
        }
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        self.draw_filter(frame, inner, theme, body.note.as_deref());

        self.page = usize::from(inner.height.saturating_sub(2 + below)).max(1);
        let rows = self.len();
        let message = body
            .message
            .or_else(|| (rows == 0).then(|| fl!("picker-nothing")));
        if let Some(message) = message {
            let room = 2..inner.height.saturating_sub(below);
            for (index, text) in room.zip(cells::wrap(&message, width)) {
                frame.render_widget(Line::styled(text, theme.dialog), line(index));
            }
            return;
        }
        let cursor = self.cursor.min(rows - 1);
        let mut at = lines
            .iter()
            .position(|shown| *shown == MenuLine::Row(cursor))
            .unwrap_or(0);
        self.rows_drawn = lines
            .iter()
            .map(|shown| match shown {
                MenuLine::Row(row) => Some(*row),
                MenuLine::Heading(_) => None,
            })
            .collect();
        // Five lines around the cursor show the heading of its rows with them too.
        self.window.follow(&mut at, lines.len(), self.page);
        let offset = self.window.top();
        for (index, shown) in lines.iter().skip(offset).take(self.page).enumerate() {
            let y = u16::try_from(index + 2).unwrap_or(u16::MAX);
            let text = match shown {
                MenuLine::Heading(heading) => {
                    let heading = format!("─ {heading} ");
                    let rest = width.saturating_sub(cells::width(&heading));
                    Line::styled(format!("{heading}{}", "─".repeat(rest)), theme.dialog)
                }
                MenuLine::Row(index) => {
                    self.drawn.rows.push((*index, line(y)));
                    row(ShownRow {
                        index: *index,
                        selected: *index == cursor,
                        width,
                    })
                }
            };
            frame.render_widget(text, line(y));
        }
        if below > 0 {
            let top = inner.height.saturating_sub(below);
            frame.render_widget(Line::styled("─".repeat(width), theme.dialog), line(top));
            for (index, text) in (top + 1..inner.height).zip(body.footer) {
                frame.render_widget(Line::styled(text, theme.dialog), line(index));
            }
        }
        self.cursor = cursor;
    }

    /// The filter, with its cursor while it takes text, and `note` at the right, on the first
    /// line of `inner`; a line under them.
    fn draw_filter(&self, frame: &mut Frame<'_>, inner: Rect, theme: &Theme, note: Option<&str>) {
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let note = note.map(|note| cells::sanitize(note.as_bytes()));
        let room = match &note {
            Some(note) => width.saturating_sub(cells::width(note) + 1),
            None => width,
        };
        let prefix = cells::width(&fl!("picker-filter", text = String::new()));
        let (shown, column) = self.filter.visible(room.saturating_sub(prefix));
        let column = u16::try_from(prefix + column).unwrap_or(u16::MAX);
        let filter = cells::fit(&fl!("picker-filter", text = shown), room, Align::Left);
        let top = match note {
            Some(note) => format!("{filter} {note}"),
            None => filter,
        };
        frame.render_widget(Line::styled(top, theme.dialog), line(0));
        if self.filtering {
            frame.set_cursor_position(Position::new(
                inner.x.saturating_add(column).min(inner.right() - 1),
                inner.y,
            ));
        }
        frame.render_widget(Line::styled("─".repeat(width), theme.dialog), line(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(rows: &[&str], fuzzy: bool) -> Picker {
        let mut picker = Picker::new(fuzzy);
        picker.set_rows(rows.iter().map(|row| vec![(*row).to_owned()]).collect());
        picker
    }

    /// Types `text` in the filter.
    fn typed(picker: &mut Picker, text: &str) {
        picker.handle(Resolved::Action(Action::QuickSearch));
        for c in text.chars() {
            assert_eq!(picker.handle(Resolved::Insert(c)), Pick::Pending);
        }
    }

    fn press(picker: &mut Picker, action: Action) -> Pick {
        picker.handle(Resolved::Action(action))
    }

    #[test]
    fn page_keys_scroll_as_vims_and_skip_the_headings() {
        let names: Vec<String> = (0..30).map(|n| format!("row{n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut plain = picker(&names, false);
        plain.page = 10;
        press(&mut plain, Action::PageDown);
        assert_eq!(plain.cursor(), 12, "as vim's Ctrl+f in ten lines");
        press(&mut plain, Action::HalfPageDown);
        assert_eq!(plain.cursor(), 17, "as vim's Ctrl+d");
        // A heading where the cursor would go: the row after it, the way the keys went.
        let mut headed = picker(&names, false);
        headed.page = 10;
        headed.rows_drawn = (0..30).map(Some).collect();
        headed.rows_drawn.insert(12, None);
        press(&mut headed, Action::PageDown);
        assert_eq!(headed.cursor(), 12, "on the line after the heading");
        // A count scrolls that many pages at once, as vim's 2 Ctrl+f.
        let mut counted = picker(&names, false);
        counted.page = 10;
        counted.count_pages(2);
        press(&mut counted, Action::PageDown);
        assert_eq!(counted.cursor(), 20);
        press(&mut counted, Action::PageDown);
        assert_eq!(
            counted.cursor(),
            28,
            "one page, the count used up, as vim's"
        );
    }

    #[test]
    fn the_rows_take_the_keys_until_slash_types_in_the_filter() {
        let mut picker = picker(&["one", "two", "three"], false);
        assert_eq!(picker.mode(), Mode::Normal);
        assert_eq!(picker.handle(Resolved::Insert('t')), Pick::Pending);
        assert_eq!(picker.shown(0..3), [0, 1, 2], "no filter yet");
        typed(&mut picker, "t");
        assert_eq!(picker.mode(), Mode::Insert);
        assert_eq!(picker.shown(0..3), [1, 2]);
        typed(&mut picker, "2");
        assert!(picker.shown(0..3).is_empty(), "a digit is text");
        press(&mut picker, Action::Backspace);
        press(&mut picker, Action::NormalMode);
        assert_eq!(picker.mode(), Mode::Normal);
        assert_eq!(picker.shown(0..3), [1, 2], "the filter stays");
        press(&mut picker, Action::Backspace);
        assert_eq!(
            picker.shown(0..3),
            [1, 2],
            "Backspace is no edit in Normal mode"
        );
        press(&mut picker, Action::HalfPageDown);
        assert_eq!(press(&mut picker, Action::Confirm), Pick::Chosen(1));
    }

    #[test]
    fn rows_show_while_the_filter_is_empty_or_has_text_as_they_say() {
        let mut picker = Picker::new(false);
        let row = |text: &str, shows| (vec![text.to_owned()], shows);
        picker.set_rows_showing(vec![
            row("web", Shows::Always),
            row("db", Shows::Filtered),
            row("", Shows::Unfiltered),
            row("dns", Shows::Always),
        ]);
        assert_eq!(picker.shown(0..4), [0, 2, 3]);
        typed(&mut picker, "d");
        assert_eq!(picker.shown(0..4), [1, 3]);
        press(&mut picker, Action::Backspace);
        assert_eq!(picker.shown(0..4), [0, 2, 3]);
    }

    #[test]
    fn keys_move_filter_and_pass_on_what_they_do_not_take() {
        let mut picker = picker(&["src", "docs", "target"], false);
        press(&mut picker, Action::End);
        assert_eq!(press(&mut picker, Action::Confirm), Pick::Chosen(2));
        press(&mut picker, Action::Down);
        assert_eq!(picker.cursor(), 2, "stops at the last");
        typed(&mut picker, "S");
        assert_eq!(picker.shown(0..3), [0, 1], "ignoring case");
        assert_eq!(picker.cursor(), 0);
        press(&mut picker, Action::Backspace);
        assert_eq!(picker.shown(0..3), [0, 1, 2]);
        assert_eq!(
            press(&mut picker, Action::Reload),
            Pick::Other(Action::Reload)
        );
        assert_eq!(press(&mut picker, Action::Cancel), Pick::Closed);
    }

    #[test]
    fn the_filter_is_a_text_field_and_delete_is_the_windows() {
        let mut picker = picker(&["src", "docs", "target"], false);
        typed(&mut picker, "sc");
        assert!(picker.shown(0..3).is_empty(), "no name contains sc");
        press(&mut picker, Action::Left);
        typed(&mut picker, "r");
        assert_eq!(picker.shown(0..3), [0], "typed in the middle: src");
        press(&mut picker, Action::DeleteChar);
        assert_eq!(picker.shown(0..3), [0], "sr");
        press(&mut picker, Action::Left);
        press(&mut picker, Action::Backspace);
        assert_eq!(picker.shown(0..3), [0, 2], "r");
        press(&mut picker, Action::TextEnd);
        press(&mut picker, Action::DeleteToStart);
        assert_eq!(picker.shown(0..3), [0, 1, 2]);
        // Home is the list's; the filter has TextStart and TextEnd.
        typed(&mut picker, "t");
        press(&mut picker, Action::TextStart);
        typed(&mut picker, "s");
        assert!(picker.shown(0..3).is_empty(), "st");
        press(&mut picker, Action::DeleteToEnd);
        assert_eq!(picker.shown(0..3), [0, 1], "s");
        press(&mut picker, Action::End);
        assert_eq!(picker.cursor(), 1, "the last row shown");
        // Delete is the window's, for the row under the cursor: the workspaces, the history.
        assert_eq!(
            press(&mut picker, Action::Delete),
            Pick::Other(Action::Delete)
        );
        assert_eq!(picker.shown(0..3), [0, 1], "the filter stays");
    }

    #[test]
    fn a_fuzzy_filter_puts_the_cursor_on_the_best_match() {
        let mut picker = picker(&["Macintosh HD", "share"], true);
        typed(&mut picker, "sh");
        assert_eq!(picker.shown(0..2), [0, 1]);
        assert_eq!(
            picker.cursor(),
            1,
            "the start of a word beats the end of one"
        );
        picker.set_fuzzy(false);
        assert_eq!(picker.shown(0..2), [0, 1], "both contain sh");
    }
}
