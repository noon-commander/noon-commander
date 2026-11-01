//! The window with the checksums a job found, a dialog (ADR 0061): with several files, a picker
//! of them with a filter, as the lists have (ADR 0036), and under it, apart, the whole checksum
//! of the file under the cursor, as the location menu shows the whole row under it; else the
//! whole checksum of each file and, apart from it, whether it matches what was expected or the
//! other file. Buttons put them on the clipboard, view them, or save them (ADR 0058).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use noc_scroll::Motion;
use noc_scroll::scrollbar::Bar;

use super::cells::{self, Align, Marks};
use super::dialog::{Button, Colors, Dialog, DialogEvent, Text, Tone, even_lines};
use super::edit::Registers;
use super::glyphs;
use super::keymap::{Action, Context, Mode, Resolved, Typed};
use super::mouse::{Pointer, Scroll};
use super::picker::{Body, MenuLine, Picker};
use super::search::{Input, Kind};
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the window, borders included, where the screen has room.
const WIDTH: u16 = 76;
/// Most rows the list shows at once; it scrolls.
const LIST_ROWS: u16 = 10;
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

/// What the window says about all of them, apart from the checksums, and whether it is good
/// news.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) text: String,
    pub(crate) good: bool,
}

/// What a key did in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SumsEvent {
    Pending,
    /// Copy the checksum of row `0`, by its index among all.
    Copy(usize),
    CopyAll,
    View,
    Save,
    Closed,
}

/// The window of one finished checksum job.
#[derive(Debug)]
pub(crate) struct SumsWindow {
    rows: Vec<SumRow>,
    /// It has View, which F3 presses.
    view: bool,
    dialog: Dialog,
}

impl SumsWindow {
    /// The window titled `title` for `rows`, with `buttons`, the first of them the default,
    /// with the focus. Several files without a verdict get a picker whose filter matches as
    /// `input` says, as it was last; it opens on the rows. With a verdict, which only one file
    /// or two compared have, each file shows whole.
    pub(crate) fn new(
        title: &str,
        rows: Vec<SumRow>,
        verdict: Option<Verdict>,
        buttons: Vec<Button>,
        input: Input,
    ) -> Self {
        let view = buttons.contains(&Button::View);
        let mut dialog = Dialog::question(title, "", buttons, 0, Tone::Plain).with_width(WIDTH);
        if rows.len() > 1 && verdict.is_none() {
            let mut picker = Picker::new(input).on_rows();
            picker.set_rows(rows.iter().map(texts).collect());
            dialog = dialog.with_picker(picker);
        } else {
            for (index, row) in rows.iter().enumerate() {
                if index > 0 {
                    dialog = dialog.with_separator();
                }
                let hex = match &row.hex {
                    Some(hex) => Text::Even(hex.clone()),
                    None => Text::Line(fl!("checksum-skipped")),
                };
                dialog = dialog
                    .with_text(Text::Line(row.name.clone()))
                    .with_text(hex);
            }
            if let Some(Verdict { text, good }) = verdict {
                dialog = dialog.with_line().with_text(Text::Verdict { text, good });
            }
        }
        Self { rows, view, dialog }
    }

    /// Where keys go: the dialog's, and the picker of several files has keys of its own.
    pub(crate) fn context(&self) -> Context {
        if self.dialog.picker().is_some() {
            Context::Checksums
        } else {
            Context::Dialog
        }
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    pub(crate) fn mode(&self) -> Mode {
        self.dialog.mode()
    }

    /// The kind of its filter while that takes the keys.
    pub(crate) fn search(&self) -> Option<Kind> {
        self.dialog.picker().and_then(Picker::search)
    }

    /// How its filter matches now, and whether case counts; `None` without a picker.
    pub(crate) fn input(&self) -> Option<Input> {
        self.dialog.picker().map(Picker::input)
    }

    /// The rows, for the count before a page key.
    pub(crate) fn picker_mut(&mut self) -> Option<&mut Picker> {
        self.dialog.picker_mut()
    }

    /// The file under the cursor, by its index among all; none while the filter shows none.
    /// Without a picker, the first: the one file, or that of the active panel.
    fn selected(&self) -> Option<usize> {
        match self.dialog.picker() {
            Some(picker) => (picker.shown(0..self.rows.len()))
                .get(picker.cursor())
                .copied(),
            None => (!self.rows.is_empty()).then_some(0),
        }
    }

    /// Takes a key as the dialog does: on the rows the motions move the cursor, past the last
    /// row or the first to the buttons, which they go through as Tab does, and `/` types in the
    /// filter; Enter or Space presses the button with the focus, or To clipboard on the rows,
    /// Alt and a letter its button; F3 views the checksums, and Esc closes the window.
    pub(crate) fn handle(&mut self, input: Resolved) -> SumsEvent {
        if input == Resolved::Action(Action::View) {
            return if self.view {
                SumsEvent::View
            } else {
                SumsEvent::Pending
            };
        }
        let typed = Typed::from(input);
        match self.dialog.handle_typed(typed, &mut Registers::default()) {
            DialogEvent::Pressed(button) => self.press(button),
            DialogEvent::Cancelled => SumsEvent::Closed,
            DialogEvent::Pending | DialogEvent::Opened(_) => SumsEvent::Pending,
        }
    }

    /// What `button` does.
    fn press(&self, button: Button) -> SumsEvent {
        match button {
            Button::ToClipboard => self.selected().map_or(SumsEvent::Pending, SumsEvent::Copy),
            Button::AllToClipboard => SumsEvent::CopyAll,
            Button::View => SumsEvent::View,
            Button::Save => SumsEvent::Save,
            _ => SumsEvent::Closed,
        }
    }

    /// The scroll bar of the list at the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.dialog.picker().and_then(Picker::bar)
    }

    /// Moves the cursor as a press on the scroll bar asks: the next file or the one before, a
    /// page on or back, or the rows from where the thumb was dragged.
    pub(crate) fn scroll(&mut self, scroll: Scroll) {
        let Some(picker) = self.dialog.picker_mut() else {
            return;
        };
        let action = match scroll {
            Scroll::By(Motion::Up) => Action::Up,
            Scroll::By(Motion::Down) => Action::Down,
            Scroll::By(Motion::PageUp) => Action::PageUp,
            Scroll::By(Motion::PageDown) => Action::PageDown,
            Scroll::By(_) => return,
            Scroll::To(top) => {
                picker.scroll_to(top);
                return;
            }
        };
        picker.handle(Resolved::Action(action));
    }

    /// Takes a press of the mouse, where the window was drawn last, as the dialog does: a click
    /// chooses a file, or switches how the filter matches on its glyphs, and one on a button
    /// returns Enter, which presses it.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        match self.dialog.pointer(pointer) {
            DialogEvent::Pressed(_) => Some(Action::Confirm),
            DialogEvent::Pending | DialogEvent::Opened(_) | DialogEvent::Cancelled => None,
        }
    }

    /// Draws the window centered in `area`: the picker of files if there are several, with the
    /// name and the whole checksum of the one under the cursor apart below it, in as many
    /// lines for each, so that nothing moves as the cursor goes from one to the next; else each
    /// file and the verdict. Then the buttons.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let Some(picker) = self.dialog.picker() else {
            self.dialog.render(frame, area, theme);
            return;
        };
        let colors = Colors::of(theme, Tone::Plain);
        let room = self.dialog.text_width(area).max(1);
        let shown = picker.shown(self.rows.iter());
        let lines: Vec<MenuLine> = (0..shown.len()).map(MenuLine::Row).collect();
        let cursor = picker.cursor().min(shown.len().saturating_sub(1));
        let hex_rows = (self.rows.iter())
            .map(|row| {
                row.hex
                    .as_ref()
                    .map_or(1, |hex| even_lines(hex, room).len())
            })
            .max()
            .unwrap_or(1)
            .max(1);
        let mut footer = vec![Line::default(); 1 + hex_rows];
        if let Some(row) = shown.get(cursor) {
            let styles = colors.body_styles();
            let name = cells::fit(&row.name, room, Align::Left, styles, theme.glyphs.marks);
            footer[0] = Line::from(name);
            let hex = match &row.hex {
                Some(hex) => even_lines(hex, room),
                None => vec![fl!("checksum-skipped")],
            };
            for (line, text) in footer[1..].iter_mut().zip(hex) {
                *line = Line::raw(text);
            }
        }
        let footer = footer
            .into_iter()
            .map(|line| line.style(theme.dialog))
            .collect();
        let body = Body {
            lines: &lines,
            max_lines: Some(LIST_ROWS),
            footer,
            ..Body::default()
        };
        let marks = theme.glyphs.marks;
        self.dialog
            .render_picker(frame, area, theme, body, &mut |row| {
                let styles = if row.selected {
                    (colors.focused_style(), theme.truncated_cursor)
                } else {
                    colors.body_styles()
                };
                Line::from(list_line(shown[row.index], row.width, styles, marks)).style(styles.0)
            });
    }
}

/// What the filter finds a file by: its name, and its checksum, or that it was skipped.
fn texts(row: &SumRow) -> Vec<String> {
    let hex = row.hex.clone().unwrap_or_else(|| fl!("checksum-skipped"));
    vec![row.name.clone(), hex]
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

#[cfg(test)]
mod tests {
    use noc_config::SearchMode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::mouse::{Press, find};

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn row(name: &str, hex: Option<&str>, mark: Mark) -> SumRow {
        SumRow {
            name: name.to_owned(),
            hex: hex.map(str::to_owned),
            mark,
        }
    }

    fn window(rows: Vec<SumRow>, verdict: Option<Verdict>, buttons: Vec<Button>) -> SumsWindow {
        let input = Input::new(Kind::FILTER, SearchMode::Plain);
        SumsWindow::new("SHA-256", rows, verdict, buttons, input)
    }

    fn press(window: &mut SumsWindow, action: Action) -> SumsEvent {
        window.handle(Resolved::Action(action))
    }

    fn typed(window: &mut SumsWindow, text: &str) {
        for c in text.chars() {
            window.handle(Resolved::Insert(c));
        }
    }

    fn terminal(window: &mut SumsWindow, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal
    }

    fn draw(window: &mut SumsWindow, width: u16, height: u16) -> String {
        terminal(window, width, height).backend().to_string()
    }

    const ALL: [Button; 5] = [
        Button::ToClipboard,
        Button::AllToClipboard,
        Button::View,
        Button::Save,
        Button::Ok,
    ];

    fn many() -> SumsWindow {
        let rows = vec![
            row("dir/a.txt", Some(ABC), Mark::None),
            row("dir/b.txt", None, Mark::Skipped),
            row("dir/sub/c.txt", Some(&ABC.replace('b', "0")), Mark::None),
        ];
        window(rows, None, ALL.to_vec())
    }

    #[test]
    fn draws_one_file_with_its_whole_checksum_and_the_verdict() {
        let verdict = Verdict {
            text: "Matches the expected checksum.".to_owned(),
            good: true,
        };
        let rows = vec![row("abc.txt", Some(ABC), Mark::Match)];
        let buttons = vec![Button::ToClipboard, Button::Save, Button::Ok];
        let mut window = window(rows, Some(verdict), buttons);
        assert_eq!(window.context(), Context::Dialog);
        insta::assert_snapshot!(draw(&mut window, 80, 14));
    }

    #[test]
    fn two_files_compared_are_a_dialog_with_both_whole_and_the_verdict_apart() {
        let other = ABC.replace('b', "0");
        let rows = vec![
            row("/srv/a.iso", Some(ABC), Mark::Mismatch),
            row("host:/srv/a.iso", Some(&other), Mark::Mismatch),
        ];
        let verdict = Verdict {
            text: "The files differ.".to_owned(),
            good: false,
        };
        let buttons = vec![Button::ToClipboard, Button::AllToClipboard, Button::Ok];
        let mut window = window(rows, Some(verdict), buttons);
        assert_eq!(window.context(), Context::Dialog);
        assert_eq!(window.input(), None, "no filter");
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(0));
        // The motions move the focus, as in every dialog.
        assert_eq!(press(&mut window, Action::Down), SumsEvent::Pending);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::CopyAll);
        press(&mut window, Action::Up);
        insta::assert_snapshot!(draw(&mut window, 80, 16));
    }

    #[test]
    fn draws_a_list_with_the_selected_file_apart_below_it() {
        let mut window = many();
        press(&mut window, Action::Down);
        assert_eq!(window.context(), Context::Checksums);
        insta::assert_snapshot!(draw(&mut window, 80, 18));
    }

    #[test]
    fn the_whole_checksum_takes_as_many_lines_whichever_file_is_under_the_cursor() {
        let long = "0123456789abcdef".repeat(8);
        let rows = vec![
            row("short", Some(ABC), Mark::None),
            row("long", Some(&long), Mark::None),
        ];
        let mut window = window(rows, None, ALL.to_vec());
        insta::assert_snapshot!(draw(&mut window, 80, 18));
        let first = draw(&mut window, 80, 18);
        press(&mut window, Action::Down);
        let second = draw(&mut window, 80, 18);
        let frame = |screen: &str| {
            screen
                .lines()
                .map(|line| line.contains('╚'))
                .position(|bottom| bottom)
        };
        assert_eq!(frame(&first), frame(&second), "{first}\n{second}");
        assert!(second.contains(&long[..64]) && second.contains(&long[64..]));
    }

    #[test]
    fn the_filter_finds_files_by_name_and_checksum_and_copy_takes_the_one_found() {
        let mut window = many();
        assert_eq!(window.mode(), Mode::Normal, "it opens on the rows");
        assert_eq!(window.search(), None);
        assert_eq!(window.handle(Resolved::Insert('c')), SumsEvent::Pending);
        assert_eq!(
            window.selected(),
            Some(0),
            "a letter is no text on the rows"
        );
        press(&mut window, Action::QuickSearch);
        assert_eq!(window.mode(), Mode::Insert);
        assert_eq!(window.search(), Some(Kind::FILTER));
        typed(&mut window, "sub");
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(2));
        let screen = draw(&mut window, 80, 18);
        assert!(screen.contains("Filter: sub"), "{screen}");
        assert!(!screen.contains("dir/a.txt"), "{screen}");
        // By its checksum, which shows shortened.
        press(&mut window, Action::DeleteToStart);
        typed(&mut window, "0a78160f");
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(2));
        press(&mut window, Action::DeleteToStart);
        typed(&mut window, "skipped");
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(1));
        // Nothing found: Copy has nothing to copy, the other buttons still work.
        typed(&mut window, "zz");
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Pending);
        assert!(draw(&mut window, 80, 18).contains(&fl!("picker-nothing")));
        // Tab leaves the filter for the buttons.
        assert_eq!(press(&mut window, Action::NextField), SumsEvent::Pending);
        assert_eq!(window.mode(), Mode::Normal);
        press(&mut window, Action::NextField);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::CopyAll);
        // `/` on the buttons types in the filter again.
        press(&mut window, Action::QuickSearch);
        assert_eq!(window.mode(), Mode::Insert);
        assert!(draw(&mut window, 80, 18).contains("Filter: skippedzz"));
        press(&mut window, Action::NormalMode);
        assert_eq!(window.mode(), Mode::Normal);
        assert_eq!(press(&mut window, Action::Cancel), SumsEvent::Closed);
    }

    #[test]
    fn the_scroll_bar_of_a_long_list_chooses_a_file_on_the_screen() {
        let rows = (1..=15)
            .map(|n| row(&format!("file{n:02}"), Some(ABC), Mark::None))
            .collect();
        let mut window = window(rows, None, vec![Button::Ok]);
        let top = draw(&mut window, 80, 30);
        assert!(top.contains('▲') && top.contains('█'), "{top}");
        let bar = window.bar().unwrap();
        window.scroll(Scroll::By(Motion::Down));
        assert_eq!(window.selected(), Some(1));
        window.scroll(Scroll::To(bar.dragged(0, u16::MAX)));
        let end = draw(&mut window, 80, 30);
        assert!(end.contains("file15") && !end.contains("file01"), "{end}");
        let before = window.selected().unwrap();
        window.scroll(Scroll::By(Motion::PageUp));
        assert!(window.selected().unwrap() < before, "a page back");
        let mut few = many();
        draw(&mut few, 80, 18);
        assert_eq!(few.bar(), None, "three files fit");
    }

    #[test]
    fn long_checksums_break_into_even_lines() {
        let hex = "0123456789".repeat(12) + "abcdefgh";
        assert_eq!(even_lines(&hex, 72), [&hex[..64], &hex[64..]]);
        assert_eq!(even_lines(ABC, 72), [ABC]);
        assert_eq!(even_lines("", 72), Vec::<String>::new());
    }

    #[test]
    fn arrows_choose_a_file_and_tab_goes_on_to_the_buttons() {
        let mut window = many();
        // Enter and Space on a row press the default button, To clipboard.
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(0));
        press(&mut window, Action::Down);
        assert_eq!(press(&mut window, Action::Toggle), SumsEvent::Copy(1));
        // The motions stay in the rows, at the last one and the first.
        press(&mut window, Action::End);
        press(&mut window, Action::Down);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(2));
        press(&mut window, Action::Home);
        press(&mut window, Action::Up);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(0));
        // Tab, or Right, goes on to the buttons, through which the motions go.
        press(&mut window, Action::Down);
        press(&mut window, Action::NextField);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(1));
        press(&mut window, Action::Down);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::CopyAll);
        press(&mut window, Action::Down);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::View);
        press(&mut window, Action::Right);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Save);
        press(&mut window, Action::Down);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Closed);
        // Round to the rows, which kept their cursor.
        press(&mut window, Action::Down);
        assert_eq!(window.selected(), Some(1));
        press(&mut window, Action::Left);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Closed);
        assert_eq!(press(&mut window, Action::View), SumsEvent::View, "F3");
        assert_eq!(press(&mut window, Action::Cancel), SumsEvent::Closed);
    }

    #[test]
    fn one_file_has_no_list_and_no_view() {
        let rows = vec![row("abc.txt", Some(ABC), Mark::None)];
        let mut window = window(rows, None, vec![Button::ToClipboard, Button::Ok]);
        assert_eq!(window.mode(), Mode::Normal);
        assert_eq!(window.input(), None);
        assert_eq!(press(&mut window, Action::QuickSearch), SumsEvent::Pending);
        assert_eq!(press(&mut window, Action::View), SumsEvent::Pending);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(0));
        press(&mut window, Action::Right);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Closed);
        assert_eq!(press(&mut window, Action::Cancel), SumsEvent::Closed);
    }

    #[test]
    fn alt_and_a_letter_press_its_button() {
        let mut window = many();
        press(&mut window, Action::Down);
        press(&mut window, Action::QuickSearch);
        let mut hotkey = |c| window.handle(Resolved::Hotkey(c));
        assert_eq!(
            hotkey('c'),
            SumsEvent::Copy(1),
            "while the filter takes text too"
        );
        assert_eq!(hotkey('a'), SumsEvent::CopyAll);
        assert_eq!(hotkey('v'), SumsEvent::View);
        assert_eq!(hotkey('s'), SumsEvent::Save);
        assert_eq!(hotkey('o'), SumsEvent::Closed);
        assert_eq!(hotkey('x'), SumsEvent::Pending);
        let letters: Vec<_> = ALL.iter().filter_map(|b| b.label().letter()).collect();
        assert_eq!(letters, ['c', 'a', 'v', 's', 'o']);
    }

    #[test]
    fn clicks_choose_a_file_and_press_a_button_but_close_nothing() {
        // A double click on a row copies its checksum, as Enter does there.
        let mut window = many();
        let terminal = terminal(&mut window, 80, 18);
        let buffer = terminal.backend().buffer();
        let click = |at| Pointer {
            press: Press::Click,
            at,
        };
        assert_eq!(window.pointer(click(find(buffer, "dir/sub/c.txt"))), None);
        assert_eq!(window.selected(), Some(2));
        let double = Pointer {
            press: Press::DoubleClick,
            ..click(find(buffer, "dir/a.txt"))
        };
        assert_eq!(window.pointer(double), Some(Action::Confirm));
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(0));
        let view = find(buffer, "View");
        assert_eq!(window.pointer(click(view)), Some(Action::Confirm));
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::View);
        let outside = ratatui::layout::Position::new(0, 0);
        assert_eq!(window.pointer(click(outside)), None);
    }
}
