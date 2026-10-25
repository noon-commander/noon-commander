//! The window with the checksums a job found: the full checksum of the selected file, whether
//! it matches what was expected or the other file, and buttons to copy or save them.

use std::cell::{Cell, RefCell};

use noc_scroll::Motion;
use noc_scroll::scrollbar::{self, Bar};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::cells::{self, Align, Marks};
use super::dialog::{
    Colors, Tone, button_line, button_spots, draw_box, draw_separator, frame_around, right_frame,
};
use super::glyphs;
use super::keymap::{Action, Resolved};
use super::label::Label;
use super::mouse::{Drawn, Pointer, Press, Scroll};
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the window, borders included, where the screen has room.
const WIDTH: u16 = 76;
/// Most rows the list shows at once; it scrolls.
const LIST_ROWS: usize = 10;
/// Hex digits on either side of the gap where the list shortens a checksum.
const SHORT: usize = 8;

/// How a file's checksum compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    /// Nothing to compare it with.
    None,
    Match,
    Mismatch,
    /// It was skipped after a failure, so it has no checksum.
    Skipped,
}

/// A file in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SumRow {
    /// The name, terminal-safe.
    pub(crate) name: String,
    /// The checksum in lowercase hex; `None` if skipped.
    pub(crate) hex: Option<String>,
    pub(crate) mark: Mark,
}

/// What the line under the checksum says about all of them, and whether it is good news.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) text: String,
    pub(crate) good: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SumsButton {
    /// The checksum of the selected file.
    Copy,
    /// Every line, as `sha256sum` prints them.
    CopyAll,
    Save,
    Close,
}

impl SumsButton {
    /// Its name, with the letter that Alt and it press.
    fn label(self) -> Label {
        Label::parse(&match self {
            Self::Copy => fl!("checksum-copy"),
            Self::CopyAll => fl!("checksum-copy-all"),
            Self::Save => fl!("checksum-save"),
            Self::Close => fl!("dialog-ok"),
        })
    }
}

/// What a key did in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SumsEvent {
    Pending,
    /// Copy the checksum of row `0`.
    Copy(usize),
    CopyAll,
    Save,
    Closed,
}

/// The window of one finished checksum job.
#[derive(Debug)]
pub(crate) struct SumsWindow {
    /// The algorithm, such as `SHA-256`.
    title: String,
    rows: Vec<SumRow>,
    selected: usize,
    verdict: Option<Verdict>,
    /// What the last button did, such as copying.
    status: Option<String>,
    buttons: Vec<SumsButton>,
    focus: usize,
    /// The first file of the list shown, and how many show, at the last render.
    top: Cell<usize>,
    page: Cell<usize>,
    /// Where the last render drew it, for the mouse.
    drawn: RefCell<Drawn>,
    /// The scroll bar of the list at the last render, on the right of the frame.
    bar: Cell<Option<Bar>>,
}

impl SumsWindow {
    /// The window for `rows`, with `buttons`, the first of them with the focus.
    pub(crate) fn new(
        title: String,
        rows: Vec<SumRow>,
        verdict: Option<Verdict>,
        buttons: Vec<SumsButton>,
    ) -> Self {
        Self {
            title,
            rows,
            selected: 0,
            verdict,
            status: None,
            buttons,
            focus: 0,
            top: Cell::new(0),
            page: Cell::new(1),
            drawn: RefCell::default(),
            bar: Cell::new(None),
        }
    }

    /// Shows `text` above the buttons, until the next one.
    pub(crate) fn set_status(&mut self, text: String) {
        self.status = Some(text);
    }

    /// Takes a key: Up and Down choose a file, Left, Right, and Tab a button, Enter or Space
    /// press it, Alt and its letter too, and Esc closes the window.
    pub(crate) fn handle(&mut self, input: Resolved) -> SumsEvent {
        let action = match input {
            Resolved::Action(action) => action,
            Resolved::Hotkey(c) => {
                return match self.buttons.iter().find(|button| button.label().has(c)) {
                    Some(&button) => self.press(button),
                    None => SumsEvent::Pending,
                };
            }
            Resolved::Insert(_) => return SumsEvent::Pending,
        };
        let last = self.rows.len().saturating_sub(1);
        let count = self.buttons.len().max(1);
        match action {
            Action::Up => self.selected = self.selected.saturating_sub(1),
            Action::Down => self.selected = (self.selected + 1).min(last),
            Action::PageUp => self.selected = self.selected.saturating_sub(LIST_ROWS),
            Action::PageDown => self.selected = (self.selected + LIST_ROWS).min(last),
            Action::Home => self.selected = 0,
            Action::End => self.selected = last,
            Action::Right | Action::NextField => self.focus = (self.focus + 1) % count,
            Action::Left | Action::PrevField => self.focus = (self.focus + count - 1) % count,
            Action::Confirm | Action::Toggle => {
                let button = self.buttons.get(self.focus).copied();
                return self.press(button.unwrap_or(SumsButton::Close));
            }
            Action::Cancel => return SumsEvent::Closed,
            _ => {}
        }
        SumsEvent::Pending
    }

    /// What `button` does.
    fn press(&self, button: SumsButton) -> SumsEvent {
        match button {
            SumsButton::Copy => SumsEvent::Copy(self.selected),
            SumsButton::CopyAll => SumsEvent::CopyAll,
            SumsButton::Save => SumsEvent::Save,
            SumsButton::Close => SumsEvent::Closed,
        }
    }

    /// The scroll bar of the list at the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar.get()
    }

    /// Chooses a file as a press on the scroll bar asks: the next one or the one before, a page
    /// on or back, or the nearest in the list scrolled to where the thumb was dragged.
    pub(crate) fn scroll(&mut self, scroll: Scroll) {
        let last = self.rows.len().saturating_sub(1);
        let page = self.page.get().max(1);
        let selected = match scroll {
            Scroll::By(Motion::Up) => self.selected.saturating_sub(1),
            Scroll::By(Motion::Down) => self.selected + 1,
            Scroll::By(Motion::PageUp) => self.selected.saturating_sub(page),
            Scroll::By(Motion::PageDown) => self.selected + page,
            Scroll::By(_) => self.selected,
            Scroll::To(top) => {
                self.top.set(top);
                self.selected.clamp(top, top + page - 1)
            }
        };
        self.selected = selected.min(last);
    }

    /// Takes a press of the mouse, where the window was drawn last: a click chooses a file, and
    /// one on a button returns Enter, which presses it.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        let drawn = self.drawn.borrow().clone();
        if let Some(row) = drawn.row_at(pointer.at) {
            if matches!(pointer.press, Press::Click | Press::DoubleClick) {
                self.selected = row;
            }
            return None;
        }
        drawn.button_press(pointer, &mut self.focus)
    }

    /// Draws the window centered in `area`: the list of files if there are several, then the
    /// selected one with its whole checksum, the verdict, and the buttons.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let colors = Colors::of(theme, Tone::Plain);
        let width = WIDTH.min(area.width.saturating_sub(4)).max(20);
        let room = usize::from(width.saturating_sub(4)).max(1);
        let mut lines: Vec<Line<'static>> = Vec::new();
        // The files of the list, by their lines.
        let mut listed = Vec::new();
        if self.rows.len() > 1 {
            let shown = self.rows.len().min(LIST_ROWS);
            let top = self
                .top
                .get()
                .min(self.selected)
                .max((self.selected + 1).saturating_sub(shown))
                .min(self.rows.len() - shown);
            self.top.set(top);
            self.page.set(shown);
            listed.extend(top..top + shown);
            for (index, row) in self.rows.iter().enumerate().skip(top).take(shown) {
                let styles = if index == self.selected {
                    (colors.focused_style(), theme.truncated_cursor)
                } else {
                    colors.body_styles()
                };
                let text = list_line(row, room, styles, theme.glyphs.marks);
                lines.push(Line::from(text).style(styles.0));
            }
            lines.push(Line::default());
        }
        if let Some(row) = self.rows.get(self.selected) {
            lines.push(Line::from(cells::fit(
                &row.name,
                room,
                Align::Left,
                colors.body_styles(),
                theme.glyphs.marks,
            )));
            match &row.hex {
                Some(hex) => lines.extend(hex_lines(hex, room).into_iter().map(Line::raw)),
                None => lines.push(Line::raw(fl!("checksum-skipped"))),
            }
        }
        if let Some(verdict) = &self.verdict {
            let style = if verdict.good {
                theme.dialog_title
            } else {
                theme.error_title
            };
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(verdict.text.clone(), style.bold())));
        }
        if let Some(status) = &self.status {
            lines.push(Line::default());
            lines.push(Line::from(cells::fit(
                status,
                room,
                Align::Left,
                colors.body_styles(),
                theme.glyphs.marks,
            )));
        }
        let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
        // Borders, the lines, a line, the buttons.
        let inner = draw_box(frame, area, (width, height + 2), &self.title, colors, theme);
        let visible = usize::from(inner.height.saturating_sub(2));
        let mut drawn = Drawn {
            frame: frame_around(inner),
            ..Drawn::default()
        };
        for (index, line) in lines.into_iter().take(visible).enumerate() {
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            let row = Rect::new(inner.x, y, inner.width, 1);
            frame.render_widget(line, row);
            if let Some(file) = listed.get(index) {
                drawn.rows.push((*file, row));
            }
        }
        let list = u16::try_from(listed.len().min(visible)).unwrap_or(u16::MAX);
        let bar = right_frame(inner, inner.y, list);
        let position = (self.rows.len(), listed.len(), self.top.get());
        let bar = if listed.is_empty() {
            None
        } else {
            scrollbar::render(frame, bar, position, theme.dialog, theme.glyphs.scroll_bar)
        };
        self.bar.set(bar);
        if inner.height >= 2 {
            let labels: Vec<Label> = self.buttons.iter().map(|button| button.label()).collect();
            let buttons = button_line(&labels, 0, Some(self.focus), colors);
            let y = inner.bottom() - 1;
            draw_separator(frame, inner, y - 1, colors, theme);
            let row = Rect::new(inner.x, y, inner.width, 1);
            drawn.buttons = button_spots(&labels, 0, row);
            frame.render_widget(buttons, row);
        }
        *self.drawn.borrow_mut() = drawn;
    }
}

/// A row of the list in `room` cells: its mark, its checksum shortened in the middle, and its
/// name, in `style` with the marks of text cut short in `mark` over it.
fn list_line(
    row: &SumRow,
    room: usize,
    (style, mark): (Style, Style),
    marks: Marks,
) -> Vec<Span<'static>> {
    let sign = match row.mark {
        Mark::None => ' ',
        Mark::Match => glyphs::MATCH,
        Mark::Mismatch => glyphs::MISMATCH,
        Mark::Skipped => glyphs::SKIPPED,
    };
    let styles = (style, mark);
    let mut spans = vec![Span::styled(format!("{sign} "), style)];
    match &row.hex {
        Some(hex) if hex.len() > 2 * SHORT + 1 => {
            spans.push(Span::styled(hex[..SHORT].to_owned(), style));
            spans.push(Span::styled(marks.after, style.patch(mark)));
            spans.push(Span::styled(hex[hex.len() - SHORT..].to_owned(), style));
        }
        Some(hex) => spans.extend(cells::fit(hex, 2 * SHORT + 1, Align::Left, styles, marks)),
        None => spans.extend(cells::fit(
            &fl!("checksum-skipped"),
            2 * SHORT + 1,
            Align::Left,
            styles,
            marks,
        )),
    }
    spans.push(Span::styled("  ", style));
    let name_room = room.saturating_sub(2 * SHORT + 5);
    spans.extend(cells::fit(&row.name, name_room, Align::Left, styles, marks));
    cells::fit_spans(spans, room, styles, marks)
}

/// A checksum in lines of at most `width` hex digits, of equal length where it breaks.
fn hex_lines(hex: &str, width: usize) -> Vec<String> {
    let parts = hex.len().div_ceil(width.max(1)).max(1);
    let per = hex.len().div_ceil(parts).max(1);
    hex.as_bytes()
        .chunks(per)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn row(name: &str, hex: Option<&str>, mark: Mark) -> SumRow {
        SumRow {
            name: name.to_owned(),
            hex: hex.map(str::to_owned),
            mark,
        }
    }

    fn press(window: &mut SumsWindow, action: Action) -> SumsEvent {
        window.handle(Resolved::Action(action))
    }

    fn draw(window: &SumsWindow, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    fn many() -> SumsWindow {
        let rows = vec![
            row("dir/a.txt", Some(ABC), Mark::None),
            row("dir/b.txt", None, Mark::Skipped),
            row("dir/sub/c.txt", Some(&ABC.replace('b', "0")), Mark::None),
        ];
        let buttons = vec![
            SumsButton::Copy,
            SumsButton::CopyAll,
            SumsButton::Save,
            SumsButton::Close,
        ];
        SumsWindow::new("SHA-256".to_owned(), rows, None, buttons)
    }

    #[test]
    fn draws_one_file_with_its_whole_checksum_and_the_verdict() {
        let verdict = Verdict {
            text: "Matches the expected checksum.".to_owned(),
            good: true,
        };
        let rows = vec![row("abc.txt", Some(ABC), Mark::Match)];
        let buttons = vec![SumsButton::Copy, SumsButton::Save, SumsButton::Close];
        let mut window = SumsWindow::new("SHA-256".to_owned(), rows, Some(verdict), buttons);
        window.set_status("Sent to the terminal's clipboard.".to_owned());
        insta::assert_snapshot!(draw(&window, 80, 14));
    }

    #[test]
    fn draws_a_list_with_the_selected_file_below_it() {
        let mut window = many();
        press(&mut window, Action::Down);
        insta::assert_snapshot!(draw(&window, 80, 14));
    }

    #[test]
    fn the_scroll_bar_of_a_long_list_chooses_a_file_on_the_screen() {
        let rows = (1..=15)
            .map(|n| row(&format!("file{n:02}"), Some(ABC), Mark::None))
            .collect();
        let mut window = SumsWindow::new("SHA-256".to_owned(), rows, None, vec![]);
        let top = draw(&window, 80, 30);
        assert!(top.contains('▲') && top.contains('█'), "{top}");
        let bar = window.bar().unwrap();
        window.scroll(Scroll::By(Motion::Down));
        assert_eq!(window.selected, 1);
        window.scroll(Scroll::To(bar.dragged(0, u16::MAX)));
        let end = draw(&window, 80, 30);
        assert!(end.contains("file15") && !end.contains("file01"), "{end}");
        assert_eq!(window.selected, 5, "the nearest file on the screen");
        window.scroll(Scroll::By(Motion::PageUp));
        assert_eq!(window.selected, 0);
        let few = many();
        draw(&few, 80, 14);
        assert_eq!(few.bar(), None, "three files fit");
    }

    #[test]
    fn long_checksums_break_into_even_lines() {
        let hex = "0123456789".repeat(12) + "abcdefgh";
        assert_eq!(hex_lines(&hex, 72), [&hex[..64], &hex[64..]]);
        assert_eq!(hex_lines(ABC, 72), [ABC]);
        assert_eq!(hex_lines("", 72), Vec::<String>::new());
    }

    #[test]
    fn arrows_choose_a_file_and_the_buttons_act_on_it() {
        let mut window = many();
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(0));
        press(&mut window, Action::End);
        press(&mut window, Action::Down);
        assert_eq!(press(&mut window, Action::Toggle), SumsEvent::Copy(2));
        press(&mut window, Action::Up);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(1));
        press(&mut window, Action::Right);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::CopyAll);
        press(&mut window, Action::NextField);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Save);
        press(&mut window, Action::Right);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Closed);
        press(&mut window, Action::Right);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(1));
        press(&mut window, Action::Left);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Closed);
        assert_eq!(press(&mut window, Action::Cancel), SumsEvent::Closed);
        assert_eq!(
            window.handle(Resolved::Insert('x')),
            SumsEvent::Pending,
            "typing does nothing"
        );
    }

    #[test]
    fn alt_and_a_letter_press_its_button() {
        let mut window = many();
        press(&mut window, Action::Down);
        let mut hotkey = |c| window.handle(Resolved::Hotkey(c));
        assert_eq!(hotkey('c'), SumsEvent::Copy(1));
        assert_eq!(hotkey('a'), SumsEvent::CopyAll);
        assert_eq!(hotkey('s'), SumsEvent::Save);
        assert_eq!(hotkey('o'), SumsEvent::Closed);
        assert_eq!(hotkey('x'), SumsEvent::Pending);
        let all = [
            SumsButton::Copy,
            SumsButton::CopyAll,
            SumsButton::Save,
            SumsButton::Close,
        ];
        let letters: Vec<_> = all.iter().filter_map(|b| b.label().letter()).collect();
        assert_eq!(letters, ['c', 'a', 's', 'o']);
    }
}
