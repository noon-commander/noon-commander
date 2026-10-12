//! What the windows that list rows to choose one from share: the location menu and the tab
//! menu, drawn over their panel, the window of the saved workspaces, and the window of the
//! command history. Typing filters the rows; in all but the history, `1` … `9` and `0` choose
//! the first ten while the filter is empty.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;

use super::cells::{self, Align};
use super::dialog::{Colors, Field, draw_box, frame_around};
use super::fuzzy::Fuzzy;
use super::keymap::{Action, Resolved};
use super::mouse::{Drawn, Pointer};
use super::theme::Theme;
use crate::i18n::fl;

/// What a key did in a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    /// Nothing for the window to do: the cursor moved, or the filter changed.
    Pending,
    /// Enter or its digit chose the row at this index among those the filter shows.
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
    /// Only while the filter is empty, and without a digit: a row that shows more rows.
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
    /// Its digit while the filter is empty, else a space.
    pub(crate) hotkey: char,
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
    /// Digits choose the first ten rows while the filter is empty; else they are text.
    hotkeys: bool,
    /// How well each row, shown or not, matches the filter, higher for better; `None` hides
    /// it.
    matches: Vec<Option<u32>>,
    /// The row under the cursor, among those the filter shows.
    cursor: usize,
    /// First line on screen, and lines on screen at the last render.
    offset: usize,
    page: usize,
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
            hotkeys: true,
            matches: Vec::new(),
            cursor: 0,
            offset: 0,
            page: 1,
            drawn: Drawn::default(),
        }
    }

    /// The same picker, whose digits are text, as the other characters.
    pub(crate) fn without_hotkeys(mut self) -> Self {
        self.hotkeys = false;
        self
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

    /// The rows that digits choose while the filter is empty, by their index among those the
    /// filter shows: the first ten that are not rows of more rows.
    fn hotkey_rows(&self) -> Vec<usize> {
        if !self.hotkeys || !self.filter.text().is_empty() {
            return Vec::new();
        }
        self.matches
            .iter()
            .zip(&self.shows)
            .filter(|(score, _)| score.is_some())
            .enumerate()
            .filter(|(_, (_, shows))| **shows != Shows::Unfiltered)
            .map(|(index, _)| index)
            .take(10)
            .collect()
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

    /// Takes a key: Up, Down, the page keys, Home, and End move, Enter chooses, a digit chooses
    /// its row while the filter is empty, other characters filter, the other keys of text fields
    /// edit the filter, and Esc closes the window. Other keys are the window's.
    pub(crate) fn handle(&mut self, input: Resolved) -> Pick {
        let rows = self.len();
        let last = rows.saturating_sub(1);
        let page = self.page.max(1);
        match input {
            Resolved::Insert(c)
                if self.hotkeys && self.filter.text().is_empty() && c.is_ascii_digit() =>
            {
                let digit = c.to_digit(10).map_or(0, |digit| digit as usize);
                return match self.hotkey_rows().get((digit + 9) % 10) {
                    Some(&row) => Pick::Chosen(row),
                    None => Pick::Pending,
                };
            }
            Resolved::Insert(c) => {
                self.filter.insert(c);
                self.refilter();
                self.focus_best();
            }
            Resolved::Action(action) => match action {
                Action::Up => self.cursor = self.cursor.saturating_sub(1),
                Action::Down => self.cursor = (self.cursor + 1).min(last),
                Action::PageUp => self.cursor = self.cursor.saturating_sub(page),
                Action::PageDown => self.cursor = (self.cursor + page).min(last),
                Action::Home => self.cursor = 0,
                Action::End => self.cursor = last,
                Action::Left
                | Action::Right
                | Action::TextStart
                | Action::TextEnd
                | Action::WordLeft
                | Action::WordRight
                | Action::Backspace
                | Action::DeleteChar
                | Action::DeleteToStart
                | Action::DeleteToEnd
                | Action::DeleteWord
                | Action::DeleteNextWord => {
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
        let at = lines
            .iter()
            .position(|shown| *shown == MenuLine::Row(cursor))
            .unwrap_or(0);
        // A heading right above the row under the cursor shows with it.
        let top = if at > 0 && matches!(lines[at - 1], MenuLine::Heading(_)) {
            at - 1
        } else {
            at
        };
        let offset = self.offset.min(top).max((at + 1).saturating_sub(self.page));
        let hotkeys = self.hotkey_rows();
        for (index, shown) in lines.iter().skip(offset).take(self.page).enumerate() {
            let y = u16::try_from(index + 2).unwrap_or(u16::MAX);
            let text = match shown {
                MenuLine::Heading(heading) => {
                    let heading = format!("─ {heading} ");
                    let rest = width.saturating_sub(cells::width(&heading));
                    Line::styled(format!("{heading}{}", "─".repeat(rest)), theme.dialog)
                }
                MenuLine::Row(index) => {
                    let hotkey = hotkeys
                        .iter()
                        .position(|row| row == index)
                        .and_then(|digit| u32::try_from((digit + 1) % 10).ok())
                        .and_then(|digit| char::from_digit(digit, 10))
                        .unwrap_or(' ');
                    self.drawn.rows.push((*index, line(y)));
                    row(ShownRow {
                        index: *index,
                        hotkey,
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
        (self.cursor, self.offset) = (cursor, offset);
    }

    /// The filter, with its cursor, and `note` at the right, on the first line of `inner`; a
    /// line under them.
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
        frame.set_cursor_position(Position::new(
            inner.x.saturating_add(column).min(inner.right() - 1),
            inner.y,
        ));
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

    fn typed(picker: &mut Picker, text: &str) {
        for c in text.chars() {
            assert_eq!(picker.handle(Resolved::Insert(c)), Pick::Pending);
        }
    }

    fn press(picker: &mut Picker, action: Action) -> Pick {
        picker.handle(Resolved::Action(action))
    }

    #[test]
    fn digits_choose_rows_while_the_filter_is_empty() {
        let mut picker = picker(&["one", "two", "three"], false);
        assert_eq!(picker.handle(Resolved::Insert('2')), Pick::Chosen(1));
        assert_eq!(picker.handle(Resolved::Insert('4')), Pick::Pending);
        typed(&mut picker, "t");
        assert_eq!(picker.shown(0..3), [1, 2]);
        typed(&mut picker, "2");
        assert!(picker.shown(0..3).is_empty(), "a digit is text then");
        assert_eq!(press(&mut picker, Action::Confirm), Pick::Pending);
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
        assert_eq!(
            picker.handle(Resolved::Insert('2')),
            Pick::Chosen(2),
            "the row of more rows takes no digit"
        );
        typed(&mut picker, "d");
        assert_eq!(picker.shown(0..4), [1, 3]);
        press(&mut picker, Action::Backspace);
        assert_eq!(picker.shown(0..4), [0, 2, 3]);
    }

    #[test]
    fn without_hotkeys_digits_are_text() {
        let mut commands = picker(&["ls", "sleep 2"], false).without_hotkeys();
        typed(&mut commands, "2");
        assert_eq!(commands.shown(0..2), [1]);
        assert_eq!(press(&mut commands, Action::Confirm), Pick::Chosen(0));
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
        assert_eq!(
            picker.handle(Resolved::Insert('2')),
            Pick::Chosen(1),
            "nothing left: digits choose again"
        );
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
