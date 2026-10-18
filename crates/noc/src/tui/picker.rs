//! What the windows that list rows to choose one from share: the location menu and the tab
//! menu, drawn over their panel, the window of the saved workspaces, and the window of the
//! command history. They open in Insert mode, typing in the filter, a text field; Esc goes to
//! the rows with the filter kept, in Normal mode, where keys move and act and `/` types in the
//! filter again (ADR 0036).

use noc_config::SearchMode;
use noc_scroll::scrollbar::{self, Bar};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::cells::{self, Align, Cut};
use super::dialog::{
    Colors, Field, SKELETON_ROWS, draw_box, draw_separator, draw_skeleton, draw_title_dots,
    frame_around, right_frame, rows_area,
};
use super::keymap::{Action, Mode, Resolved};
use super::mouse::{Drawn, Pointer};
use super::scroll::Window;
use super::search_mode::{self, Matcher, Prompt};
use super::spinner::Shine;
use super::theme::Theme;
use crate::i18n::fl;

/// The lines the whole text of the row under the cursor takes, under the rows, and the whole
/// name under the cursor of the panels, under them: always as many, so that nothing moves as
/// the cursor goes from one to the next.
pub(crate) const FULL_ROWS: u16 = 2;

/// Whether text that does not fit shows whole too, as `K` asks: below the panels, or under the
/// rows of a list. The panels and each list have their own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Whole {
    /// Only cut, in its rows.
    #[default]
    Cut,
    Shown,
}

impl Whole {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Cut => Self::Shown,
            Self::Shown => Self::Cut,
        }
    }
}

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
    /// Right of the filter, in half the line at most, such as which hosts the history shows.
    pub(crate) note: Option<Line<'static>>,
    /// Shown in place of the lines, such as why the rows did not load; without, a note that
    /// nothing matches once the filter hides every row.
    pub(crate) message: Option<String>,
    /// Whether the rows are still to come: a skeleton stands in their place.
    pub(crate) waiting: bool,
    /// The shine that crosses the skeleton.
    pub(crate) shine: Option<Shine>,
    /// Turning in front of the title while the window waits, in their color.
    pub(crate) dots: Option<(char, Style)>,
    /// Lines under the rows, apart from them, such as the whole command under the cursor.
    pub(crate) footer: Vec<String>,
    /// The whole text of the row under the cursor, which shows under the rows while `K` asks
    /// for it, in the place of a footer.
    pub(crate) full: Option<String>,
}

impl Body<'_> {
    /// Lines the box has for the rows: those of the skeleton while they are still to come.
    fn shown(&self) -> u16 {
        let lines = if self.waiting {
            SKELETON_ROWS
        } else {
            u16::try_from(self.lines.len().max(1)).unwrap_or(u16::MAX)
        };
        lines.min(self.max_lines.unwrap_or(u16::MAX))
    }
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
    /// How the filter matches: `ui.search_mode` until Ctrl+s switches it.
    mode: SearchMode,
    /// Why the filter is not a regular expression, in the regex mode.
    error: Option<String>,
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
    /// The scroll bar of the lines at the last render, on the right of the frame.
    bar: Option<Bar>,
    /// Whether `K` asks for the whole text of the row under the cursor under the rows.
    full: Whole,
}

impl Picker {
    /// A picker without rows, whose filter matches in `mode` (`ui.search_mode`). It opens with
    /// the filter taking what is typed, in Insert mode, as the zoxide window does (ADR 0036).
    pub(crate) fn new(mode: SearchMode) -> Self {
        Self {
            texts: Vec::new(),
            shows: Vec::new(),
            filter: Field::plain(""),
            mode,
            error: None,
            filtering: true,
            matches: Vec::new(),
            cursor: 0,
            window: Window::default(),
            page: 1,
            rows_drawn: Vec::new(),
            pages: 1,
            drawn: Drawn::default(),
            bar: None,
            full: Whole::Cut,
        }
    }

    /// Whether `K` shows the whole row under the cursor.
    #[cfg(test)]
    pub(crate) fn full(&self) -> Whole {
        self.full
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    pub(crate) fn mode(&self) -> Mode {
        if self.filtering {
            Mode::Insert
        } else {
            Mode::Normal
        }
    }

    /// Matches in `mode` from now on.
    pub(crate) fn set_mode(&mut self, mode: SearchMode) {
        self.mode = mode;
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

    /// Puts the cursor on the first of the rows that match best. But as fzf, all match as well,
    /// so it is the first.
    pub(crate) fn focus_best(&mut self) {
        let shown: Vec<u32> = self.matches.iter().flatten().copied().collect();
        let best = shown.iter().max();
        self.cursor = shown
            .iter()
            .position(|score| Some(score) == best)
            .unwrap_or(0);
    }

    /// Matches the rows against the filter again. A row shows if one of its texts matches the
    /// filter; as fzf, if they match it as one line. Then only rows for an empty filter, or for
    /// one with text, show as `shows` says.
    fn refilter(&mut self) {
        let mut matcher = Matcher::names(self.mode, self.filter.text());
        self.error = matcher.error().map(ToString::to_string);
        let scores: Vec<Option<u32>> = if matcher.ranks() {
            self.texts
                .iter()
                .map(|texts| matcher.score(&texts.join(" ")))
                .collect()
        } else {
            self.texts
                .iter()
                .map(|texts| {
                    texts
                        .iter()
                        .any(|text| matcher.score(text).is_some())
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
    /// the window. The filter takes what is typed, as it does when the window opens: characters
    /// filter, and the keys of text fields edit it, until Esc goes back to the rows; `/` types
    /// in it again. Other keys are the window's.
    pub(crate) fn handle(&mut self, input: Resolved) -> Pick {
        let rows = self.len();
        let last = rows.saturating_sub(1);
        match input {
            Resolved::Insert(c) if self.filtering => {
                self.filter.insert(c);
                self.refilter();
                self.focus_best();
            }
            Resolved::Insert(_) | Resolved::Hotkey(_) => {}
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
                Action::ShowFull => self.full = self.full.toggled(),
                Action::SearchMode => {
                    self.mode = search_mode::next(self.mode);
                    self.refilter();
                    self.focus_best();
                }
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

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// Shows the lines from `top`, where the thumb of the scroll bar was dragged, and puts the
    /// cursor on the row nearest to where the window left it, the way the lines went.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        let lines = self.rows_drawn.len();
        if lines == 0 {
            return;
        }
        let mut line = self
            .rows_drawn
            .iter()
            .position(|&row| row == Some(self.cursor))
            .unwrap_or(0);
        let down = top > self.window.top();
        self.window
            .scroll_to(top, &mut line, lines, self.page.max(1));
        let row = |line: usize| self.rows_drawn.get(line).copied().flatten();
        let after = (line..lines).find_map(row);
        let before = (0..=line).rev().find_map(row);
        let found = if down {
            after.or(before)
        } else {
            before.or(after)
        };
        self.cursor = found.unwrap_or(self.cursor);
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
        let shown = body.shown();
        let full = body
            .full
            .filter(|_| self.full == Whole::Shown && body.footer.is_empty() && !lines.is_empty());
        // A line between the rows and the footer.
        let below = match (body.footer.len(), &full) {
            (0, Some(_)) => FULL_ROWS + 1,
            (0, None) => 0,
            (footer, _) => u16::try_from(footer + 1).unwrap_or(u16::MAX),
        };
        let colors = Colors::of(theme, false);
        // Borders, the filter, the line under it, the rows, the footer.
        let size = (body.width, shown.saturating_add(4).saturating_add(below));
        let inner = draw_box(frame, area, size, body.title, colors, theme);
        if let Some(dots) = body.dots {
            draw_title_dots(frame, inner, body.title, dots, colors);
        }
        self.drawn = Drawn {
            frame: frame_around(inner),
            ..Drawn::default()
        };
        self.bar = None;
        if inner.height < 3 || inner.width < 4 {
            return;
        }
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        self.draw_filter(frame, inner, theme, body.note);

        self.page = usize::from(inner.height.saturating_sub(2 + below)).max(1);
        let rows = self.len();
        if body.waiting {
            draw_skeleton(frame, rows_area(inner, below), theme, body.shine);
            return;
        }
        let message = body.message.or_else(|| self.nothing());
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
        let height = u16::try_from(self.page).unwrap_or(u16::MAX);
        let bar = right_frame(inner, inner.y + 2, height);
        let position = (lines.len(), self.page, offset);
        self.bar = scrollbar::render(frame, bar, position, theme.dialog, theme.glyphs.scroll_bar);
        for (index, shown) in lines.iter().skip(offset).take(self.page).enumerate() {
            let y = u16::try_from(index + 2).unwrap_or(u16::MAX);
            let text = match shown {
                MenuLine::Heading(heading) => heading_line(heading, width, theme),
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
            draw_separator(frame, inner, inner.y + top, colors, theme);
            let footer: Vec<Line<'static>> = match full {
                Some(full) => {
                    let styles = (theme.dialog, theme.truncated_dialog);
                    cells::whole(
                        &full,
                        (width, usize::from(FULL_ROWS)),
                        Some('/'),
                        styles,
                        theme.glyphs.marks,
                    )
                }
                None => body
                    .footer
                    .into_iter()
                    .map(|text| Line::styled(text, theme.dialog))
                    .collect(),
            };
            for (index, text) in (top + 1..inner.height).zip(footer) {
                frame.render_widget(text, line(index));
            }
        }
        self.cursor = cursor;
    }

    /// What shows in place of the rows once the filter hides every row: why the filter is no
    /// regular expression, or that nothing matches.
    fn nothing(&self) -> Option<String> {
        (self.len() == 0).then(|| match &self.error {
            Some(reason) => fl!(
                "picker-not-regex",
                reason = cells::sanitize(reason.as_bytes())
            ),
            None => fl!("picker-nothing"),
        })
    }

    /// The filter, a text field in the colors of those of dialogs, with its cursor while it
    /// takes text, and `note` at the right, on the first line of `inner`; a line under them.
    /// The line joins the frame, as the one over the buttons of a dialog does.
    fn draw_filter(
        &self,
        frame: &mut Frame<'_>,
        inner: Rect,
        theme: &Theme,
        note: Option<Line<'static>>,
    ) {
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let note = note.map(|note| {
            let spans: Vec<Span<'static>> = note
                .spans
                .into_iter()
                .map(|span| {
                    Span::styled(
                        cells::sanitize(span.content.as_bytes()),
                        note.style.patch(span.style),
                    )
                })
                .collect();
            let most = width / 2;
            if spans.iter().map(Span::width).sum::<usize>() > most {
                let styles = (theme.dialog, theme.truncated_dialog);
                cells::fit_spans(spans, most, styles, theme.glyphs.marks)
            } else {
                spans
            }
        });
        let room = match &note {
            Some(note) => width.saturating_sub(note.iter().map(Span::width).sum::<usize>() + 1),
            None => width,
        };
        let mark = search_mode::marked(self.mode, self.error.is_some(), theme.dialog, theme);
        let prompt = Prompt::new(|text| fl!("picker-filter", mode = mark.glyph, text = text));
        if let Some(mut note) = note {
            note.insert(0, Span::raw(" "));
            frame.render_widget(
                Line::from(note).style(theme.dialog).right_aligned(),
                line(0),
            );
        }
        let area = Rect::new(inner.x, inner.y, u16::try_from(room).unwrap_or(u16::MAX), 1);
        let field = (&prompt, &self.filter);
        let column = mark.render_field(frame, area, field, theme.dialog, theme);
        if self.filtering {
            frame.set_cursor_position(Position::new(
                inner.x.saturating_add(column).min(inner.right() - 1),
                inner.y,
            ));
        }
        draw_separator(frame, inner, inner.y + 1, Colors::of(theme, false), theme);
    }
}

/// A heading over the rows after it, `width` cells across: its text alone, in the color of
/// titles, so that no line but those that join the frame crosses the window.
fn heading_line(heading: &str, width: usize, theme: &Theme) -> Line<'static> {
    let styles = (theme.dialog_title, theme.truncated_dialog);
    cells::fitted(
        heading,
        width,
        (Align::Left, Cut::End),
        styles,
        theme.glyphs.marks,
    )
    .style(theme.dialog)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(rows: &[&str], mode: SearchMode) -> Picker {
        let mut picker = Picker::new(mode);
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
        let mut plain = picker(&names, SearchMode::Plain);
        plain.page = 10;
        press(&mut plain, Action::PageDown);
        assert_eq!(plain.cursor(), 12, "as vim's Ctrl+f in ten lines");
        press(&mut plain, Action::HalfPageDown);
        assert_eq!(plain.cursor(), 17, "as vim's Ctrl+d");
        // A heading where the cursor would go: the row after it, the way the keys went.
        let mut headed = picker(&names, SearchMode::Plain);
        headed.page = 10;
        headed.rows_drawn = (0..30).map(Some).collect();
        headed.rows_drawn.insert(12, None);
        press(&mut headed, Action::PageDown);
        assert_eq!(headed.cursor(), 12, "on the line after the heading");
        // A count scrolls that many pages at once, as vim's 2 Ctrl+f.
        let mut counted = picker(&names, SearchMode::Plain);
        counted.page = 10;
        counted.count_pages(2);
        press(&mut counted, Action::PageDown);
        assert_eq!(counted.cursor(), 20);
        press(&mut counted, Action::PageDown);
        assert_eq!(
            counted.cursor(),
            24,
            "one page, the count used up, until the last row shows"
        );
        press(&mut counted, Action::PageDown);
        assert_eq!(counted.cursor(), 29, "then the last row");
    }

    #[test]
    fn a_dragged_thumb_scrolls_the_lines_and_the_cursor_stays_on_screen() {
        let names: Vec<String> = (0..30).map(|n| format!("row{n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut picker = picker(&names, SearchMode::Plain);
        picker.page = 10;
        picker.rows_drawn = (0..30).map(Some).collect();
        picker.scroll_to(15);
        assert_eq!(picker.window.top(), 15);
        assert_eq!(
            picker.cursor(),
            19,
            "four lines from the top, as vim's scrolloff in ten"
        );
        picker.scroll_to(0);
        assert_eq!((picker.window.top(), picker.cursor()), (0, 4));
        // A heading where the cursor would go: the row after it, the way the lines went.
        picker.rows_drawn.insert(19, None);
        picker.scroll_to(15);
        assert_eq!(picker.cursor(), 19, "the row on the line after the heading");
    }

    #[test]
    fn the_filter_takes_the_keys_as_it_opens_until_esc_goes_back_to_the_rows() {
        let mut picker = picker(&["one", "two", "three"], SearchMode::Plain);
        assert_eq!(picker.mode(), Mode::Insert);
        assert_eq!(picker.handle(Resolved::Insert('t')), Pick::Pending);
        assert_eq!(picker.shown(0..3), [1, 2]);
        assert_eq!(picker.handle(Resolved::Insert('2')), Pick::Pending);
        assert!(picker.shown(0..3).is_empty(), "a digit is text");
        press(&mut picker, Action::Backspace);
        press(&mut picker, Action::NormalMode);
        assert_eq!(picker.mode(), Mode::Normal);
        assert_eq!(picker.shown(0..3), [1, 2], "the filter stays");
        assert_eq!(picker.handle(Resolved::Insert('o')), Pick::Pending);
        assert_eq!(
            picker.shown(0..3),
            [1, 2],
            "characters are no text in Normal mode"
        );
        press(&mut picker, Action::Backspace);
        assert_eq!(
            picker.shown(0..3),
            [1, 2],
            "Backspace is no edit in Normal mode"
        );
        typed(&mut picker, "w");
        assert_eq!(picker.mode(), Mode::Insert, "/ types in the filter again");
        assert_eq!(picker.shown(0..3), [1]);
        press(&mut picker, Action::Backspace);
        press(&mut picker, Action::NormalMode);
        press(&mut picker, Action::HalfPageDown);
        assert_eq!(press(&mut picker, Action::Confirm), Pick::Chosen(1));
    }

    #[test]
    fn a_heading_is_its_text_in_the_color_of_titles_without_a_line() {
        let theme = Theme::terminal();
        let line = heading_line("Mounts", 12, &theme);
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text.trim_end(), "Mounts");
        assert!(!text.contains('─'), "{text}");
        assert_eq!(line.spans[0].style, theme.dialog_title);
        let cut = heading_line("Mounts", 4, &theme);
        assert_eq!(cut.width(), 4, "cut to the width");
    }

    #[test]
    fn rows_show_while_the_filter_is_empty_or_has_text_as_they_say() {
        let mut picker = Picker::new(SearchMode::Plain);
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
        let mut picker = picker(&["src", "docs", "target"], SearchMode::Plain);
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
        let mut picker = picker(&["src", "docs", "target"], SearchMode::Plain);
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
        let mut picker = picker(&["Macintosh HD", "share"], SearchMode::Fzf);
        typed(&mut picker, "sh");
        assert_eq!(picker.shown(0..2), [0, 1]);
        assert_eq!(
            picker.cursor(),
            1,
            "the start of a word beats the end of one"
        );
        picker.set_mode(SearchMode::Plain);
        assert_eq!(picker.shown(0..2), [0, 1], "both contain sh");
    }

    #[test]
    fn alt_r_switches_how_the_filter_matches() {
        let mut picker = picker(&["main.rs", "Cargo.toml", "rs.md"], SearchMode::Plain);
        typed(&mut picker, "ms");
        assert_eq!(picker.len(), 0, "as it is");
        press(&mut picker, Action::SearchMode);
        assert_eq!(picker.mode, SearchMode::Fzf);
        assert_eq!(picker.shown(0..3), [0], "m, then s");
        press(&mut picker, Action::SearchMode);
        assert_eq!(picker.mode, SearchMode::Regex);
        assert_eq!(picker.len(), 0);
        press(&mut picker, Action::DeleteToStart);
        typed(&mut picker, "^r|toml$");
        assert_eq!(picker.shown(0..3), [1, 2]);
        assert_eq!(picker.error, None);
        typed(&mut picker, "(");
        assert_eq!(picker.len(), 0);
        assert_eq!(picker.error.as_deref(), Some("error: unclosed group"));
        press(&mut picker, Action::SearchMode);
        assert_eq!(
            (picker.mode, picker.error.as_deref()),
            (SearchMode::Plain, None)
        );
    }
}
