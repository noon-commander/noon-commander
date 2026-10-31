//! The window with the checksums a job found: with several files, a list of them with a filter,
//! as the lists have (ADR 0036), and under it, apart, the whole checksum of the file under the
//! cursor, as the location menu shows the whole row under it; whether it matches what was
//! expected or the other file; and buttons to copy them, view them, or save them (ADR 0058).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use noc_scroll::Motion;
use noc_scroll::scrollbar::Bar;

use super::cells::{self, Align, Marks};
use super::dialog::{
    Colors, Tone, button_line, button_spots, draw_box, draw_separator, frame_around,
};
use super::glyphs;
use super::keymap::{Action, Context, Mode, Resolved};
use super::label::Label;
use super::mouse::{Drawn, Pointer, Scroll};
use super::picker::{Body, MenuLine, Pick, Picker};
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
    /// Those lines in the viewer.
    View,
    Save,
    Close,
}

impl SumsButton {
    /// Its name, with the letter that Alt and it press.
    fn label(self) -> Label {
        Label::parse(&match self {
            Self::Copy => fl!("checksum-copy"),
            Self::CopyAll => fl!("checksum-copy-all"),
            Self::View => fl!("checksum-view"),
            Self::Save => fl!("checksum-save"),
            Self::Close => fl!("dialog-ok"),
        })
    }
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
    /// The algorithm, such as `SHA-256`.
    title: String,
    rows: Vec<SumRow>,
    verdict: Option<Verdict>,
    /// What the last button did, such as copying.
    status: Option<String>,
    buttons: Vec<SumsButton>,
    focus: usize,
    /// The filter, the cursor, and the scrolling of the list of several files; one file has no
    /// list.
    list: Option<Picker>,
    /// Where the last render drew the window of one file, for the mouse.
    drawn: Drawn,
}

impl SumsWindow {
    /// The window for `rows`, with `buttons`, the first of them with the focus. Several files
    /// get a list whose filter matches as `input` says, as it was last; it opens on the rows.
    pub(crate) fn new(
        title: String,
        rows: Vec<SumRow>,
        verdict: Option<Verdict>,
        buttons: Vec<SumsButton>,
        input: Input,
    ) -> Self {
        let list = (rows.len() > 1).then(|| {
            let mut picker = Picker::new(input).on_rows();
            picker.set_rows(rows.iter().map(texts).collect());
            picker
        });
        Self {
            title,
            rows,
            verdict,
            status: None,
            buttons,
            focus: 0,
            list,
            drawn: Drawn::default(),
        }
    }

    /// Shows `text` under the checksum, until the next one.
    pub(crate) fn set_status(&mut self, text: String) {
        self.status = Some(text);
    }

    /// Where keys go: the window of one file is a dialog, and the list of several has keys of
    /// its own.
    pub(crate) fn context(&self) -> Context {
        if self.list.is_some() {
            Context::Checksums
        } else {
            Context::Dialog
        }
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    pub(crate) fn mode(&self) -> Mode {
        self.list.as_ref().map_or(Mode::Normal, Picker::mode)
    }

    /// The kind of its filter while that takes the keys.
    pub(crate) fn search(&self) -> Option<Kind> {
        self.list.as_ref().and_then(Picker::search)
    }

    /// How its filter matches now, and whether case counts; `None` without a list.
    pub(crate) fn input(&self) -> Option<Input> {
        self.list.as_ref().map(Picker::input)
    }

    /// The rows, for the count before a page key.
    pub(crate) fn picker_mut(&mut self) -> Option<&mut Picker> {
        self.list.as_mut()
    }

    /// The file under the cursor, by its index among all; none while the filter shows none.
    fn selected(&self) -> Option<usize> {
        match &self.list {
            Some(list) => list.shown(0..self.rows.len()).get(list.cursor()).copied(),
            None => (!self.rows.is_empty()).then_some(0),
        }
    }

    /// Takes a key: the list's, which moves the cursor and types in the filter after `/`;
    /// Left, Right, and Tab choose a button, and Enter or Space presses it, Alt and its letter
    /// too; F3 views the checksums, and Esc closes the window.
    pub(crate) fn handle(&mut self, input: Resolved) -> SumsEvent {
        match input {
            Resolved::Hotkey(c) => {
                return match self.buttons.iter().find(|button| button.label().has(c)) {
                    Some(&button) => self.press(button),
                    None => SumsEvent::Pending,
                };
            }
            Resolved::Action(Action::Confirm | Action::Toggle) => {
                let button = self.buttons.get(self.focus).copied();
                return self.press(button.unwrap_or(SumsButton::Close));
            }
            Resolved::Action(Action::NextField) => self.move_focus(true),
            Resolved::Action(Action::PrevField) => self.move_focus(false),
            Resolved::Action(Action::View) if self.buttons.contains(&SumsButton::View) => {
                return SumsEvent::View;
            }
            _ => {
                let pick = match &mut self.list {
                    Some(list) => list.handle(input),
                    None => match input {
                        Resolved::Action(Action::Cancel) => Pick::Closed,
                        Resolved::Action(action) => Pick::Other(action),
                        Resolved::Insert(_) | Resolved::Hotkey(_) => Pick::Pending,
                    },
                };
                match pick {
                    Pick::Closed => return SumsEvent::Closed,
                    Pick::Other(Action::Right) => self.move_focus(true),
                    Pick::Other(Action::Left) => self.move_focus(false),
                    Pick::Pending | Pick::Filtered | Pick::Chosen(_) | Pick::Other(_) => {}
                }
            }
        }
        SumsEvent::Pending
    }

    /// The focus to the next button, or the one before, round.
    fn move_focus(&mut self, forward: bool) {
        let count = self.buttons.len().max(1);
        self.focus = if forward {
            (self.focus + 1) % count
        } else {
            (self.focus + count - 1) % count
        };
    }

    /// What `button` does.
    fn press(&self, button: SumsButton) -> SumsEvent {
        match button {
            SumsButton::Copy => self.selected().map_or(SumsEvent::Pending, SumsEvent::Copy),
            SumsButton::CopyAll => SumsEvent::CopyAll,
            SumsButton::View => SumsEvent::View,
            SumsButton::Save => SumsEvent::Save,
            SumsButton::Close => SumsEvent::Closed,
        }
    }

    /// The scroll bar of the list at the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.list.as_ref().and_then(Picker::bar)
    }

    /// Moves the cursor as a press on the scroll bar asks: the next file or the one before, a
    /// page on or back, or the rows from where the thumb was dragged.
    pub(crate) fn scroll(&mut self, scroll: Scroll) {
        let Some(list) = &mut self.list else {
            return;
        };
        let action = match scroll {
            Scroll::By(Motion::Up) => Action::Up,
            Scroll::By(Motion::Down) => Action::Down,
            Scroll::By(Motion::PageUp) => Action::PageUp,
            Scroll::By(Motion::PageDown) => Action::PageDown,
            Scroll::By(_) => return,
            Scroll::To(top) => {
                list.scroll_to(top);
                return;
            }
        };
        list.handle(Resolved::Action(action));
    }

    /// Takes a press of the mouse, where the window was drawn last: a click chooses a file, or
    /// switches how the filter matches on its glyphs, and one on a button returns Enter, which
    /// presses it.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        let Some(list) = &mut self.list else {
            return self.drawn.button_press(pointer, &mut self.focus);
        };
        if let Some(action) = list.button_press(pointer, &mut self.focus) {
            return Some(action);
        }
        // A double click on a row presses no button, and a click outside closes no dialog.
        list.pointer(pointer)
            .filter(|action| !matches!(action, Action::Confirm | Action::Cancel))
    }

    /// Draws the window centered in `area`: the list of files if there are several, under its
    /// filter, with the file under the cursor apart below it; else the one file. Then the
    /// buttons.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        if self.list.is_some() {
            self.render_list(frame, area, theme);
        } else {
            self.render_one(frame, area, theme);
        }
    }

    /// The list of several files and, under a line, the name and the whole checksum of the one
    /// under the cursor, in as many lines for each, so that nothing moves as the cursor goes
    /// from one to the next; then the verdict and what the last button did.
    fn render_list(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let colors = Colors::of(theme, Tone::Plain);
        let Self {
            title,
            rows,
            verdict,
            status,
            buttons,
            focus,
            list,
            ..
        } = self;
        let Some(list) = list else {
            return;
        };
        let room = usize::from(WIDTH.min(area.width).saturating_sub(4)).max(1);
        let shown = list.shown(rows.iter());
        let lines: Vec<MenuLine> = (0..shown.len()).map(MenuLine::Row).collect();
        let cursor = list.cursor().min(shown.len().saturating_sub(1));
        let hex_rows = rows
            .iter()
            .map(|row| row.hex.as_ref().map_or(1, |hex| hex_lines(hex, room).len()))
            .max()
            .unwrap_or(1)
            .max(1);
        let mut footer = vec![Line::default(); 1 + hex_rows];
        if let Some(row) = shown.get(cursor) {
            let styles = colors.body_styles();
            let name = cells::fit(&row.name, room, Align::Left, styles, theme.glyphs.marks);
            footer[0] = Line::from(name);
            let hex = match &row.hex {
                Some(hex) => hex_lines(hex, room),
                None => vec![fl!("checksum-skipped")],
            };
            for (line, text) in footer[1..].iter_mut().zip(hex) {
                *line = Line::raw(text);
            }
        }
        footer.extend(verdict.as_ref().map(|verdict| verdict_line(verdict, theme)));
        footer.extend(status.as_ref().map(|status| {
            let styles = colors.body_styles();
            Line::from(cells::fit(
                status,
                room,
                Align::Left,
                styles,
                theme.glyphs.marks,
            ))
        }));
        let footer = footer
            .into_iter()
            .map(|line| line.style(theme.dialog))
            .collect();
        let labels: Vec<Label> = buttons.iter().map(|button| button.label()).collect();
        let body = Body {
            title: title.as_str(),
            width: WIDTH,
            lines: &lines,
            max_lines: Some(LIST_ROWS),
            footer,
            buttons: Some((&labels, *focus)),
            ..Body::default()
        };
        let marks = theme.glyphs.marks;
        list.render(frame, area, theme, body, &mut |row| {
            let styles = if row.selected {
                (colors.focused_style(), theme.truncated_cursor)
            } else {
                colors.body_styles()
            };
            Line::from(list_line(shown[row.index], row.width, styles, marks)).style(styles.0)
        });
    }

    /// The one file: its name and whole checksum, the verdict, and what the last button did.
    fn render_one(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let colors = Colors::of(theme, Tone::Plain);
        let width = WIDTH.min(area.width.saturating_sub(4)).max(20);
        let room = usize::from(width.saturating_sub(4)).max(1);
        let mut lines: Vec<Line<'static>> = Vec::new();
        if let Some(row) = self.rows.first() {
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
            lines.push(Line::default());
            lines.push(verdict_line(verdict, theme));
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
            frame.render_widget(line, Rect::new(inner.x, y, inner.width, 1));
        }
        if inner.height >= 2 {
            let labels: Vec<Label> = self.buttons.iter().map(|button| button.label()).collect();
            let buttons = button_line(&labels, 0, Some(self.focus), colors);
            let y = inner.bottom() - 1;
            draw_separator(frame, inner, y - 1, colors, theme);
            let row = Rect::new(inner.x, y, inner.width, 1);
            drawn.buttons = button_spots(&labels, 0, row);
            frame.render_widget(buttons, row);
        }
        self.drawn = drawn;
    }
}

/// What the filter finds a file by: its name, and its checksum, or that it was skipped.
fn texts(row: &SumRow) -> Vec<String> {
    let hex = row.hex.clone().unwrap_or_else(|| fl!("checksum-skipped"));
    vec![row.name.clone(), hex]
}

/// The verdict, bold, in the color of titles if it is good news and of errors if not.
fn verdict_line(verdict: &Verdict, theme: &Theme) -> Line<'static> {
    let style = if verdict.good {
        theme.dialog_title
    } else {
        theme.error_title
    };
    Line::from(Span::styled(verdict.text.clone(), style.bold()))
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

    fn window(rows: Vec<SumRow>, verdict: Option<Verdict>, buttons: Vec<SumsButton>) -> SumsWindow {
        let input = Input::new(Kind::FILTER, SearchMode::Plain);
        SumsWindow::new("SHA-256".to_owned(), rows, verdict, buttons, input)
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

    const ALL: [SumsButton; 5] = [
        SumsButton::Copy,
        SumsButton::CopyAll,
        SumsButton::View,
        SumsButton::Save,
        SumsButton::Close,
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
        let buttons = vec![SumsButton::Copy, SumsButton::Save, SumsButton::Close];
        let mut window = window(rows, Some(verdict), buttons);
        window.set_status("Sent to the terminal's clipboard.".to_owned());
        assert_eq!(window.context(), Context::Dialog);
        insta::assert_snapshot!(draw(&mut window, 80, 14));
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
        let verdict = Verdict {
            text: "The files differ.".to_owned(),
            good: false,
        };
        let mut window = window(rows, Some(verdict), ALL.to_vec());
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
        assert_eq!(press(&mut window, Action::NextField), SumsEvent::Pending);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::CopyAll);
        press(&mut window, Action::NormalMode);
        assert_eq!(window.mode(), Mode::Normal);
        assert_eq!(press(&mut window, Action::Cancel), SumsEvent::Closed);
    }

    #[test]
    fn the_scroll_bar_of_a_long_list_chooses_a_file_on_the_screen() {
        let rows = (1..=15)
            .map(|n| row(&format!("file{n:02}"), Some(ABC), Mark::None))
            .collect();
        let mut window = window(rows, None, vec![SumsButton::Close]);
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
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::View);
        press(&mut window, Action::Right);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Save);
        press(&mut window, Action::Right);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Closed);
        press(&mut window, Action::Right);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Copy(1));
        press(&mut window, Action::Left);
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::Closed);
        assert_eq!(press(&mut window, Action::View), SumsEvent::View, "F3");
        assert_eq!(press(&mut window, Action::Cancel), SumsEvent::Closed);
    }

    #[test]
    fn one_file_has_no_list_and_no_view() {
        let rows = vec![row("abc.txt", Some(ABC), Mark::None)];
        let mut window = window(rows, None, vec![SumsButton::Copy, SumsButton::Close]);
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
        assert_eq!(window.pointer(double), None, "no button pressed");
        assert_eq!(window.selected(), Some(0));
        let view = find(buffer, "View");
        assert_eq!(window.pointer(click(view)), Some(Action::Confirm));
        assert_eq!(press(&mut window, Action::Confirm), SumsEvent::View);
        let outside = ratatui::layout::Position::new(0, 0);
        assert_eq!(window.pointer(click(outside)), None);
    }
}
