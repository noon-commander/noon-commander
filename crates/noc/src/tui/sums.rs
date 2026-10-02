//! The window with the checksums a job found: the full checksum of the selected file, whether
//! it matches what was expected or the other file, and buttons to copy or save them.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, button_line, draw_box, draw_separator};
use super::keymap::{Action, Resolved};
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
    fn label(self) -> String {
        match self {
            Self::Copy => fl!("checksum-copy"),
            Self::CopyAll => fl!("checksum-copy-all"),
            Self::Save => fl!("checksum-save"),
            Self::Close => fl!("dialog-ok"),
        }
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
        }
    }

    /// Shows `text` above the buttons, until the next one.
    pub(crate) fn set_status(&mut self, text: String) {
        self.status = Some(text);
    }

    /// Takes a key: Up and Down choose a file, Left, Right, and Tab a button, Enter or Space
    /// press it, and Esc closes the window.
    pub(crate) fn handle(&mut self, input: Resolved) -> SumsEvent {
        let Resolved::Action(action) = input else {
            return SumsEvent::Pending;
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
                return match self.buttons.get(self.focus) {
                    Some(SumsButton::Copy) => SumsEvent::Copy(self.selected),
                    Some(SumsButton::CopyAll) => SumsEvent::CopyAll,
                    Some(SumsButton::Save) => SumsEvent::Save,
                    Some(SumsButton::Close) | None => SumsEvent::Closed,
                };
            }
            Action::Cancel => return SumsEvent::Closed,
            _ => {}
        }
        SumsEvent::Pending
    }

    /// Draws the window centered in `area`: the list of files if there are several, then the
    /// selected one with its whole checksum, the verdict, and the buttons.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let colors = Colors::of(theme, false);
        let width = WIDTH.min(area.width.saturating_sub(4)).max(20);
        let room = usize::from(width.saturating_sub(4)).max(1);
        let mut lines: Vec<Line<'static>> = Vec::new();
        if self.rows.len() > 1 {
            let shown = self.rows.len().min(LIST_ROWS);
            let top = (self.selected + 1).saturating_sub(shown);
            for (index, row) in self.rows.iter().enumerate().skip(top).take(shown) {
                let text = cells::fit(&list_line(row), room, Align::Left);
                let style = if index == self.selected {
                    colors.focused_style()
                } else {
                    Style::default()
                };
                lines.push(Line::styled(text, style));
            }
            lines.push(Line::default());
        }
        if let Some(row) = self.rows.get(self.selected) {
            lines.push(Line::raw(cells::fit(&row.name, room, Align::Left)));
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
            lines.push(Line::raw(cells::fit(status, room, Align::Left)));
        }
        let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
        // Borders, the lines, a line, the buttons.
        let inner = draw_box(frame, area, (width, height + 2), &self.title, colors, theme);
        let visible = usize::from(inner.height.saturating_sub(2));
        for (index, line) in lines.into_iter().take(visible).enumerate() {
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            frame.render_widget(line, Rect::new(inner.x, y, inner.width, 1));
        }
        if inner.height >= 2 {
            let labels: Vec<String> = self.buttons.iter().map(|button| button.label()).collect();
            let buttons = button_line(&labels, 0, Some(self.focus), colors);
            let y = inner.bottom() - 1;
            draw_separator(frame, inner, y - 1, colors, theme);
            frame.render_widget(buttons, Rect::new(inner.x, y, inner.width, 1));
        }
    }
}

/// A row of the list: its mark, its checksum shortened in the middle, and its name.
fn list_line(row: &SumRow) -> String {
    let mark = match row.mark {
        Mark::None => ' ',
        Mark::Match => '✓',
        Mark::Mismatch => '✗',
        Mark::Skipped => '-',
    };
    let sum = match &row.hex {
        Some(hex) if hex.len() > 2 * SHORT + 1 => {
            format!("{}…{}", &hex[..SHORT], &hex[hex.len() - SHORT..])
        }
        Some(hex) => hex.clone(),
        None => fl!("checksum-skipped"),
    };
    let sum = cells::fit(&sum, 2 * SHORT + 1, Align::Left);
    format!("{mark} {sum}  {}", row.name)
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
}
