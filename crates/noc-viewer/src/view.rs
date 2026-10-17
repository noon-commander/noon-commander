//! The viewer on screen: a file's text, scrolled, with long lines wrapped or cut.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use noc_scroll::scrollbar::{self, Bar, BarGlyphs};
use noc_scroll::{Motion, Step};
use noc_text::{Align, Cut, Fitted, Marks};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::i18n::fl;
use crate::read::LIMIT;
use crate::text::{cut, lines_of, row_count, row_of, row_starts, wider_than, width};

/// Long lines whose rows are kept, as wrapped: more than a few screens take.
const WRAPPED_LINES: usize = 1024;
/// [`LIMIT`] in mebibytes, as the header and the end say it.
const LIMIT_MIB: usize = LIMIT / (1024 * 1024);

/// Where the rows of lines start, by line.
type Wrapped = HashMap<usize, Rc<[usize]>>;

/// Whether rows after the first of a wrapped line start with a mark in `width` cells: not
/// where it would leave no room for text.
fn breaks(width: usize) -> bool {
    width >= 2
}

/// The cells of text in each row after the first of a line wrapped in `width` cells.
fn continued(width: usize) -> usize {
    if breaks(width) { width - 1 } else { width }
}

/// What the viewer is asked to do. The app maps its keys to these; closing the viewer is the
/// app's own business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Scrolls; never past the end of the file at the bottom of the screen. Home goes back to
    /// the first column too.
    Scroll(Motion),
    /// Puts a line at the top, or as near as the end lets it: where the thumb of the scroll
    /// bar was dragged, in its lines.
    Top(usize),
    /// One column left, when lines are cut.
    Left,
    /// One column right, when lines are cut; no further than the end of the longest line on
    /// screen at the right edge.
    Right,
    /// Count columns left, when lines are cut, as vim's `zh`.
    ScrollLeft(usize),
    /// Count columns right, when lines are cut, as vim's `zl`; no further than
    /// [`Right`](Self::Right) goes.
    ScrollRight(usize),
    /// Count half widths of the text left, when lines are cut, as vim's `zH`.
    HalfLeft(usize),
    /// Count half widths of the text right, when lines are cut, as vim's `zL`; no further than
    /// [`Right`](Self::Right) goes.
    HalfRight(usize),
    /// Back to the first column, when lines are cut, as vim's `zs` with the cursor at the start
    /// of the lines.
    ScrollStart,
    /// The end of the longest line on screen to the right edge, when lines are cut, as vim's
    /// `ze`.
    ScrollEnd,
    /// Wraps long lines, or cuts them.
    ToggleWrap,
}

/// How the viewer looks: the colors of the app's theme, and the characters it draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Styles {
    /// The title line: the path and the position.
    pub header: Style,
    /// The style of the mark where the title is cut, over `header`.
    pub header_mark: Style,
    /// The text, the screen behind it, and the scroll bar; dimmed, the line that ends the text.
    pub text: Style,
    /// The scroll bar.
    pub bar: BarGlyphs,
    /// The line across the screen after the end of the text, a character of one cell.
    pub line: &'static str,
    /// The style of the marks of text that goes on past the screen.
    pub mark: Style,
    /// The marks: [`Marks::before`] over the first cell of a cut line with text hidden on the
    /// left, as vim's 'listchars' `precedes`; [`Marks::after`] over the last cell of one that
    /// goes on to the right, as `extends`, and in front of each row of a wrapped line after its
    /// first, as vim's 'showbreak'; and where the title is cut.
    pub marks: Marks,
}

#[derive(Debug)]
enum State {
    Loading,
    Text {
        /// Terminal-safe, tabs expanded.
        lines: Vec<String>,
        /// Only the start of the file was read.
        truncated: bool,
    },
}

/// A file on screen. Its position is a line and a row within it, so that wrapping takes
/// only the lines in view, whatever the size of the file.
#[derive(Debug)]
pub struct Viewer {
    title: String,
    state: State,
    /// The first line on screen, and its first row there. The line after the last is the one
    /// that marks the end of the text.
    top: (usize, usize),
    /// Cells cut off the left of each line, when lines are not wrapped.
    column: usize,
    wrap: bool,
    /// Rows and columns of text at the last render.
    page: usize,
    width: usize,
    /// The scroll bar of the last render, if the text did not fit.
    bar: Option<Bar>,
    /// The rows of the whole text wrapped, and the width they were counted in.
    measured: Option<(usize, usize)>,
    /// Where the rows of the long lines wrapped lately start, by line, and the width they were
    /// wrapped in: moving a row on in a line of megabytes must not wrap it all again.
    wrapped: RefCell<(usize, Wrapped)>,
}

impl Viewer {
    /// The viewer of the file `title`, while it loads. The title is shown as it is, so it
    /// must be terminal-safe.
    pub fn new(title: String) -> Self {
        Self {
            title,
            state: State::Loading,
            top: (0, 0),
            column: 0,
            wrap: true,
            page: 1,
            width: 80,
            bar: None,
            measured: None,
            wrapped: RefCell::default(),
        }
    }

    /// The scroll bar of the last render, for the mouse; `None` when the text fit.
    pub fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// Shows `bytes`, the start of the file if `truncated`.
    pub fn show(&mut self, bytes: &[u8], truncated: bool) {
        self.state = State::Text {
            lines: lines_of(bytes),
            truncated,
        };
        self.top = (0, 0);
        self.measured = None;
        self.wrapped.borrow_mut().1.clear();
    }

    /// Scrolls, or switches wrapping.
    pub fn handle(&mut self, command: Command) {
        match command {
            Command::Scroll(motion) => self.scroll(motion),
            Command::Top(line) => self.top = (line, 0).min(self.last_top()),
            Command::Left => self.column = self.column.saturating_sub(1),
            Command::ToggleWrap => {
                self.wrap = !self.wrap;
                self.top.1 = 0;
                self.column = 0;
            }
            // Wrapped lines hold still sideways, as in vim.
            _ if self.wrap => {}
            Command::Right => self.go_right(1),
            Command::ScrollLeft(count) => self.column = self.column.saturating_sub(count),
            Command::ScrollRight(count) => self.go_right(count),
            Command::HalfLeft(count) => {
                self.column = self
                    .column
                    .saturating_sub(count.saturating_mul(self.width / 2));
            }
            Command::HalfRight(count) => self.go_right(count.saturating_mul(self.width / 2)),
            Command::ScrollStart => self.column = 0,
            Command::ScrollEnd => self.column = self.last_column(),
        }
    }

    fn lines(&self) -> &[String] {
        match &self.state {
            State::Text { lines, .. } => lines,
            State::Loading => &[],
        }
    }

    /// The cells of the longest line on screen at the last render, lines cut.
    fn longest_on_screen(&self) -> usize {
        let lines = self.lines().iter().skip(self.top.0).take(self.page);
        lines.map(|line| width(line)).max().unwrap_or(0)
    }

    /// How far the lines can scroll left: until the end of the longest line on screen is at the
    /// right edge, as `ze` puts it, so that scrolling never leaves the screen empty.
    fn last_column(&self) -> usize {
        self.longest_on_screen().saturating_sub(self.width)
    }

    /// `count` columns right, as vim's `zl`, but no further than [`Self::last_column`].
    fn go_right(&mut self, count: usize) {
        self.column = self.column.saturating_add(count).min(self.last_column());
    }

    /// The rows line `index` takes on screen.
    fn rows_of(&self, index: usize) -> usize {
        self.row_starts(index).map_or(1, |starts| starts.len())
    }

    /// Where the rows of line `index` start, wrapped; `None` for a line of one row however it
    /// wraps. A line wraps once for each width, while it is among the last ones wrapped.
    fn row_starts(&self, index: usize) -> Option<Rc<[usize]>> {
        let line = self.lines().get(index)?;
        // No cell takes less than a byte.
        if !self.wrap || line.len() <= self.width {
            return None;
        }
        let mut wrapped = self.wrapped.borrow_mut();
        let (width, lines) = &mut *wrapped;
        if *width != self.width || lines.len() >= WRAPPED_LINES {
            *width = self.width;
            lines.clear();
        }
        let starts = lines
            .entry(index)
            .or_insert_with(|| row_starts(line, self.width, continued(self.width)).into());
        Some(Rc::clone(starts))
    }

    /// The position `count` rows after `from`, or the end of the text.
    fn after(&self, (mut line, mut row): (usize, usize), count: usize) -> (usize, usize) {
        let lines = self.lines().len();
        for _ in 0..count {
            if row + 1 < self.rows_of(line) {
                row += 1;
            } else if line < lines {
                (line, row) = (line + 1, 0);
            } else {
                break;
            }
        }
        (line, row)
    }

    /// The position `count` rows before `from`, or the first one.
    fn before(&self, (mut line, mut row): (usize, usize), count: usize) -> (usize, usize) {
        for _ in 0..count {
            if row > 0 {
                row -= 1;
            } else if line > 0 {
                line -= 1;
                row = self.rows_of(line) - 1;
            } else {
                break;
            }
        }
        (line, row)
    }

    /// The position of the line that marks the end of the text, after its last line.
    fn end(&self) -> (usize, usize) {
        (self.lines().len(), 0)
    }

    /// The position of the last row of the text, before the line that marks its end.
    fn last_row(&self) -> (usize, usize) {
        match self.lines().len().checked_sub(1) {
            Some(last) => (last, self.rows_of(last) - 1),
            None => (0, 0),
        }
    }

    /// Counts the rows of the whole text in the width of this render, if it wraps and they were
    /// not counted in it yet: once for each width, not for each frame.
    fn measure(&mut self) {
        let width = self.width;
        if !self.wrap || self.measured.is_some_and(|(_, counted)| counted == width) {
            return;
        }
        let rest = continued(width);
        let rows = self
            .lines()
            .iter()
            .map(|line| row_count(line, width, rest))
            .sum();
        self.measured = Some((rows, width));
    }

    /// The rows of the whole text, as [`measure`](Self::measure) counted them.
    fn total_rows(&self) -> usize {
        match self.measured {
            Some((rows, width)) if self.wrap && width == self.width => rows,
            _ => self.lines().len(),
        }
    }

    /// The top that shows the end of the text at the bottom of the screen.
    fn last_top(&self) -> (usize, usize) {
        self.before(self.end(), self.page.saturating_sub(1))
    }

    /// The last position on screen.
    fn bottom(&self) -> (usize, usize) {
        self.after(self.top, self.page.saturating_sub(1))
    }

    fn scroll(&mut self, motion: Motion) {
        self.top = match motion.step(self.page) {
            Step::Back(count) => self.before(self.top, count),
            Step::On(count) => self
                .after(self.top, count)
                .min(self.last_top().max(self.top)),
            Step::Start => {
                self.column = 0;
                (0, 0)
            }
            Step::End => self.last_top(),
        };
    }

    /// Draws the viewer over `area`: a title line, then the text.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, styles: &Styles) {
        self.bar = None;
        if area.height < 2 || area.width == 0 {
            return;
        }
        self.width = usize::from(area.width);
        self.page = usize::from(area.height - 1);
        // The scroll bar takes the last column when the text, without the line that ends it,
        // does not fit; wrapped narrower, it fits even less.
        let bar = area.width > 1 && self.after((0, 0), self.page - 1) < self.last_row();
        if bar {
            self.width -= 1;
        }
        // The width may have changed since the position was taken.
        self.top.1 = self.top.1.min(self.rows_of(self.top.0).saturating_sub(1));
        // Other lines on screen, or a wider screen, may need less of the scroll.
        self.column = self.column.min(self.last_column());
        let row = |index: u16| Rect::new(area.x, area.y + index, area.width, 1);
        // Rows of text leave the scroll bar's column alone, which the end line's style would dim.
        let text_width = u16::try_from(self.width).unwrap_or(area.width);
        let text_row = |index: u16| Rect::new(area.x, area.y + index, text_width, 1);
        frame.render_widget(self.header(usize::from(area.width), styles), row(0));
        let text_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        frame.buffer_mut().set_style(text_area, styles.text);
        if matches!(self.state, State::Loading) {
            frame.render_widget(Line::raw(fl!("viewer-loading")), row(1));
            return;
        }
        if bar {
            self.measure();
            let column = Rect::new(area.right() - 1, text_area.y, 1, text_area.height);
            self.bar = scrollbar::render(frame, column, self.thumb(), styles.text, styles.bar);
        }
        let mut position = self.top;
        for screen_row in 0..self.page {
            let y = u16::try_from(screen_row + 1).unwrap_or(u16::MAX);
            let Some(line) = self.lines().get(position.0) else {
                let end = noc_text::rule(&self.end_text(), self.width, styles.line);
                let dim = styles.text.add_modifier(Modifier::DIM);
                let mark = styles.text.patch(styles.mark);
                let line = fitted_line(end, (dim, mark), styles.marks);
                frame.render_widget(line, text_row(y));
                break;
            };
            let text = if self.wrap {
                let text = match self.row_starts(position.0) {
                    Some(starts) => row_of(line, &starts, position.1).to_owned(),
                    None if position.1 == 0 => line.clone(),
                    None => String::new(),
                };
                if position.1 > 0 && breaks(self.width) {
                    let mark = Span::styled(styles.marks.after, styles.mark);
                    Line::from(vec![mark, Span::raw(text)])
                } else {
                    Line::raw(text)
                }
            } else {
                self.cut_line(line, styles)
            };
            frame.render_widget(text, text_row(y));
            let next = self.after(position, 1);
            if next == position {
                break;
            }
            position = next;
        }
    }

    /// The cells of `line` that show from the column on, with the marks of `styles` where text
    /// goes on past either edge: a mark takes the place of the cell of text under it.
    fn cut_line(&self, line: &str, styles: &Styles) -> Line<'static> {
        let (marks, mark) = (styles.marks, styles.mark);
        let extends = wider_than(line, self.column + self.width);
        let precedes = self.column > 0 && wider_than(line, 0) && (self.width > 1 || !extends);
        let taken = usize::from(precedes) + usize::from(extends);
        let room = self.width.saturating_sub(taken);
        let mut spans = Vec::with_capacity(3);
        if precedes {
            spans.push(Span::styled(marks.before, mark));
        }
        let mut text = cut(line, self.column + usize::from(precedes), room);
        if extends {
            // A wide character that does not fit leaves a gap before the mark at the edge.
            let gap = room.saturating_sub(width(&text));
            text.extend(std::iter::repeat_n(' ', gap));
        }
        spans.push(Span::raw(text));
        if extends {
            spans.push(Span::styled(marks.after, mark));
        }
        Line::from(spans)
    }

    /// The line that marks the end of the text.
    fn end_text(&self) -> String {
        match self.state {
            State::Text {
                truncated: true, ..
            } => fl!("viewer-end-truncated", mebibytes = LIMIT_MIB),
            _ => fl!("viewer-end"),
        }
    }

    /// For the scroll bar, without the line that ends the text, so that the bar reaches its
    /// bottom with the last line as the percentage reaches 100: how many, how many show, and
    /// the first on screen. The thumb goes in lines of the file, from the first to the one on
    /// top when the last shows at the bottom, where it holds still, the end line shown too or
    /// not. Its length is the share of the rows of the whole text that show, so that it keeps
    /// it however many lines wrapped lines leave on screen.
    fn thumb(&self) -> (usize, usize, usize) {
        let last_row = self.last_row();
        let last_top = self.before(last_row, self.page.saturating_sub(1)).0;
        let top = if self.bottom() >= last_row {
            last_top
        } else {
            self.top.0.min(last_top)
        };
        // The page, in lines, that gives the thumb `page` rows of the whole text over the
        // `last_top` lines it moves across: as many lines as rows, where none wraps.
        let others = self.total_rows().saturating_sub(self.page);
        let page = (self.page * last_top + others / 2)
            .checked_div(others)
            .unwrap_or(0)
            .max(1);
        (last_top + page, page, top)
    }

    /// The path on the left, and where the screen is on the right, `width` cells wide in all:
    /// its first line, the lines, and how far the last one on screen is, in percent, or that the
    /// end shows.
    fn header(&self, width: usize, styles: &Styles) -> Line<'static> {
        let lines = self.lines().len();
        let mut position = String::new();
        if let State::Text { truncated, .. } = &self.state {
            let line = (self.top.0 + 1).min(lines);
            let bottom = self.bottom();
            position = if bottom == self.end() {
                fl!("viewer-position-end", line = line, lines = lines)
            } else {
                let percent = ((bottom.0 + 1) * 100).checked_div(lines).unwrap_or(100);
                fl!(
                    "viewer-position",
                    line = line,
                    lines = lines,
                    percent = percent
                )
            };
            if *truncated {
                position = fl!(
                    "viewer-truncated",
                    position = position,
                    mebibytes = LIMIT_MIB
                );
            }
        }
        let position = format!(" {position} ");
        let room = width.saturating_sub(noc_text::width(&position));
        let title = format!(" {}", self.title);
        let title = noc_text::fit_cut(&title, room, Align::Left, Cut::Middle);
        let mark = styles.header.patch(styles.header_mark);
        let mut line = fitted_line(title, (styles.header, mark), styles.marks).style(styles.header);
        line.push_span(Span::styled(position, styles.header));
        line
    }
}

/// `fitted` in `style`, with its mark of `marks`, if it was cut, in `mark`.
fn fitted_line(fitted: Fitted, (style, mark): (Style, Style), marks: Marks) -> Line<'static> {
    let mut spans = vec![Span::styled(fitted.head.clone(), style)];
    if fitted.cut.is_some() {
        spans.push(Span::styled(fitted.mark(marks), mark));
    }
    spans.push(Span::styled(fitted.tail, style));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    const STYLES: Styles = Styles {
        header: Style::new(),
        header_mark: Style::new(),
        text: Style::new(),
        bar: BarGlyphs {
            up: "▲",
            down: "▼",
            track: "░",
            thumb: "█",
        },
        line: "─",
        mark: Style::new(),
        marks: Marks {
            before: "«",
            after: "»",
        },
    };

    fn draw(viewer: &mut Viewer, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| viewer.render(frame, frame.area(), &STYLES))
            .unwrap();
        terminal.backend().to_string()
    }

    fn numbered(count: usize) -> Vec<u8> {
        use std::fmt::Write as _;
        let mut text = String::new();
        for n in 1..=count {
            writeln!(text, "line {n}").unwrap();
        }
        text.into_bytes()
    }

    #[test]
    fn scrolls_within_the_text_and_stops_at_its_end() {
        let mut viewer = Viewer::new("/srv/log".to_owned());
        assert!(draw(&mut viewer, 30, 6).contains("Loading"));
        viewer.show(&numbered(20), false);
        let mut press = |motion| {
            viewer.handle(Command::Scroll(motion));
            draw(&mut viewer, 30, 6)
        };
        let top = press(Motion::Home);
        assert!(top.contains("/srv/log"), "{top}");
        assert!(top.contains("1/20 25%"), "{top}");
        assert!(top.contains("line 5") && !top.contains("line 6"), "{top}");
        // Ctrl+f keeps two of the five rows on screen, as vim's; Space turns the whole page, as
        // less.
        assert!(press(Motion::PageDown).contains("4/20"));
        assert!(press(Motion::WindowDown).contains("9/20"));
        press(Motion::WindowUp);
        assert!(press(Motion::PageUp).contains("1/20"));
        press(Motion::WindowDown);
        // Half of the five rows, rounded down, as vim's 'scroll'.
        assert!(press(Motion::HalfPageDown).contains("8/20"));
        assert!(press(Motion::HalfPageUp).contains("6/20"));
        let end = press(Motion::End);
        assert!(end.contains("17/20 End"), "{end}");
        assert!(end.contains("line 20") && end.contains("─ EOF ─"), "{end}");
        for motion in [
            Motion::Down,
            Motion::PageDown,
            Motion::WindowDown,
            Motion::HalfPageDown,
        ] {
            assert_eq!(press(motion), end, "{motion:?} stops at the end");
        }
        assert!(press(Motion::Home).contains("1/20"));
    }

    #[test]
    fn long_lines_wrap_again_in_a_new_width_and_for_a_new_file() {
        let mut viewer = Viewer::new("f".to_owned());
        viewer.show(&[b'a'; 50], false);
        let narrow = draw(&mut viewer, 10, 8);
        assert!(
            narrow.contains(&format!("\"{}\"", "a".repeat(10))),
            "{narrow}"
        );
        let wide = draw(&mut viewer, 30, 8);
        assert!(wide.contains(&format!("\"{}\"", "a".repeat(30))), "{wide}");
        viewer.show(&[b'b'; 50], false);
        let other = draw(&mut viewer, 30, 8);
        assert!(
            other.contains(&"b".repeat(30)) && !other.contains('a'),
            "{other}"
        );
    }

    #[test]
    fn the_thumb_keeps_its_length_through_wrapped_lines_of_every_length() {
        let mut viewer = Viewer::new("f".to_owned());
        // Short lines first, then lines of three rows each.
        let text = format!(
            "{}{}",
            "x\n".repeat(15),
            format!("{}\n", "x".repeat(50)).repeat(15)
        );
        viewer.show(text.as_bytes(), false);
        let bar = |screen: &str| -> String {
            let rows = screen.lines().skip(1);
            rows.filter_map(|line| line.trim_end_matches('"').chars().last())
                .collect()
        };
        let mut lengths = std::collections::BTreeSet::new();
        let mut screens = Vec::new();
        for _ in 0..60 {
            let screen = bar(&draw(&mut viewer, 20, 24));
            lengths.insert(screen.matches('█').count());
            screens.push(screen);
            viewer.handle(Command::Scroll(Motion::Down));
        }
        assert_eq!(lengths.len(), 1, "{lengths:?}");
        let (first, last) = (&screens[0], &screens[screens.len() - 1]);
        assert!(
            first.starts_with("▲█") && last.ends_with("█▼"),
            "{first} {last}"
        );
    }

    #[test]
    fn a_scroll_bar_shows_where_the_text_does_not_fit() {
        let mut viewer = Viewer::new("f".to_owned());
        viewer.show(&numbered(20), false);
        let column = |screen: &str, row: usize| {
            let line = screen.lines().nth(row).unwrap_or_default();
            line.trim_end_matches('"').chars().last()
        };
        let top = draw(&mut viewer, 20, 6);
        assert_eq!(column(&top, 1), Some('▲'), "{top}");
        assert_eq!(column(&top, 2), Some('█'), "{top}");
        assert_eq!(column(&top, 4), Some('░'), "{top}");
        assert_eq!(column(&top, 5), Some('▼'), "{top}");
        viewer.handle(Command::Scroll(Motion::End));
        let end = draw(&mut viewer, 20, 6);
        assert_eq!(column(&end, 2), Some('░'), "{end}");
        assert_eq!(column(&end, 4), Some('█'), "{end}");
        viewer.handle(Command::Scroll(Motion::Up));
        let last = draw(&mut viewer, 20, 6);
        assert!(last.contains("100%") && !last.contains("EOF"), "{last}");
        let bar = |screen: &str| (1..6).map(|row| column(screen, row)).collect::<Vec<_>>();
        assert_eq!(
            bar(&last),
            bar(&end),
            "at the bottom with the last line, and still, the end line or not: {last}"
        );
        viewer.handle(Command::Scroll(Motion::Up));
        let before = draw(&mut viewer, 20, 6);
        assert_eq!(column(&before, 4), Some('░'), "{before}");
        // A long thumb keeps its length too, though the end line takes a row of the text.
        viewer.show(&numbered(10), false);
        let tall_bar = |viewer: &mut Viewer| {
            let screen = draw(viewer, 20, 9);
            (1..9).map(|row| column(&screen, row)).collect::<Vec<_>>()
        };
        tall_bar(&mut viewer);
        viewer.handle(Command::Scroll(Motion::End));
        let tall_end = tall_bar(&mut viewer);
        viewer.handle(Command::Scroll(Motion::Up));
        assert_eq!(tall_bar(&mut viewer), tall_end);

        viewer.show(&numbered(5), false);
        let filled = draw(&mut viewer, 20, 6);
        assert!(
            !filled.contains('█'),
            "the end line alone needs no bar: {filled}"
        );
        viewer.show(&numbered(3), false);
        let short = draw(&mut viewer, 20, 6);
        assert!(!short.contains('█') && !short.contains('░'), "{short}");
        assert!(short.contains("1/3 End"), "{short}");
        let end_row = short.lines().nth(4).unwrap_or_default();
        assert!(end_row.contains("─ EOF ─"), "right after the text: {short}");
    }

    #[test]
    fn the_scroll_bar_is_kept_for_the_mouse_and_its_thumb_puts_a_line_at_the_top() {
        let mut viewer = Viewer::new("f".to_owned());
        viewer.show(&numbered(20), false);
        draw(&mut viewer, 20, 12);
        let bar = viewer.bar().unwrap();
        assert_eq!(
            bar.part(ratatui::layout::Position::new(19, 1)),
            Some(scrollbar::Part::Up)
        );
        viewer.handle(Command::Top(bar.dragged(0, 5)));
        assert!(draw(&mut viewer, 20, 12).contains("7/20"));
        viewer.handle(Command::Top(99));
        let end = draw(&mut viewer, 20, 12);
        assert!(end.contains("11/20 End"), "no further than the end: {end}");
        viewer.show(&numbered(3), false);
        draw(&mut viewer, 20, 12);
        assert_eq!(viewer.bar(), None, "the text fits");
    }

    #[test]
    fn the_end_line_leaves_the_scroll_bar_undimmed() {
        let mut viewer = Viewer::new("f".to_owned());
        viewer.show(&numbered(20), false);
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        let mut render = |viewer: &mut Viewer| {
            terminal
                .draw(|frame| viewer.render(frame, frame.area(), &STYLES))
                .unwrap()
                .buffer
                .clone()
        };
        render(&mut viewer);
        viewer.handle(Command::Scroll(Motion::End));
        let screen = render(&mut viewer);
        let end_row = 5;
        assert_eq!(screen[(19, end_row)].symbol(), "▼");
        assert!(!screen[(19, end_row)].modifier.contains(Modifier::DIM));
        assert_eq!(screen[(19, end_row - 1)].symbol(), "█");
        assert!(screen[(8, end_row)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn wraps_long_lines_and_scrolls_sideways_without() {
        let mut viewer = Viewer::new("f".to_owned());
        let long: String = ('a'..='z').cycle().take(50).collect();
        viewer.show(format!("{long}\nshort\n").as_bytes(), false);
        let wrapped = draw(&mut viewer, 20, 6);
        insta::assert_snapshot!(wrapped);
        // Three rows of text, twenty cells wide beside the scroll bar: Down goes into the rows
        // of the long line.
        let small = |viewer: &mut Viewer| draw(viewer, 21, 4);
        small(&mut viewer);
        viewer.handle(Command::Scroll(Motion::Down));
        let text = small(&mut viewer);
        assert!(text.contains("short"), "{text}");
        assert!(text.contains("\"»uvwxyzabcdefghijklm "), "{text}");
        let first_row = |text: String| {
            text.contains("\"abcdefghijklmnopqrst") && !text.contains("abcdefghijklmnopqrstu")
        };
        viewer.handle(Command::Scroll(Motion::Up));
        assert!(first_row(small(&mut viewer)));

        viewer.handle(Command::Right);
        assert!(
            first_row(small(&mut viewer)),
            "wrapped lines do not scroll sideways"
        );
        viewer.handle(Command::ToggleWrap);
        viewer.handle(Command::Scroll(Motion::Home));
        viewer.handle(Command::Right);
        viewer.handle(Command::Right);
        let cut = draw(&mut viewer, 20, 6);
        assert!(cut.contains("\"«defghijklmnopqrstu»\""), "{cut}");
        assert!(cut.contains("\"«rt "), "{cut}");
    }

    /// The screen, and whether each of its cells is drawn as a mark.
    fn draw_marks(viewer: &mut Viewer, width: u16, height: u16) -> (String, Vec<String>) {
        let mark = Style::new().add_modifier(Modifier::UNDERLINED);
        let styles = Styles {
            header_mark: mark,
            mark,
            ..STYLES
        };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let buffer = terminal
            .draw(|frame| viewer.render(frame, frame.area(), &styles))
            .unwrap()
            .buffer
            .clone();
        let marks = (0..height)
            .map(|y| {
                (0..width)
                    .filter(|&x| buffer[(x, y)].modifier.contains(Modifier::UNDERLINED))
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect()
            })
            .collect();
        (terminal.backend().to_string(), marks)
    }

    #[test]
    fn cut_lines_mark_text_hidden_past_either_edge() {
        let mut viewer = Viewer::new("f".to_owned());
        viewer.show(
            "abcdefghijklmnopqrstuvwxyz\nshort\n\nabcdefghij\nabcdefgh文x\n".as_bytes(),
            false,
        );
        viewer.handle(Command::ToggleWrap);
        let (screen, marks) = draw_marks(&mut viewer, 10, 8);
        let rows: Vec<&str> = screen.lines().collect();
        assert_eq!(rows[1], "\"abcdefghi»\"", "{screen}");
        assert_eq!(rows[2], "\"short     \"", "{screen}");
        assert_eq!(rows[4], "\"abcdefghij\"", "a line that fits: {screen}");
        assert_eq!(
            rows[5], "\"abcdefgh »\"",
            "a wide character that does not fit leaves a gap: {screen}"
        );
        assert_eq!(marks[1], "»");
        assert_eq!(marks[2..=4], ["", "", ""]);
        assert_eq!(marks[5], "»");
        assert_eq!(marks[6], "", "the end line has no marks");

        for _ in 0..3 {
            viewer.handle(Command::Right);
        }
        let (screen, marks) = draw_marks(&mut viewer, 10, 8);
        let rows: Vec<&str> = screen.lines().collect();
        assert_eq!(rows[1], "\"«efghijkl»\"", "{screen}");
        assert_eq!(rows[2], "\"«t        \"", "{screen}");
        assert_eq!(
            rows[3], "\"          \"",
            "an empty line hides nothing: {screen}"
        );
        assert_eq!(rows[4], "\"«efghij   \"", "{screen}");
        assert!(rows[6].contains("EOF") && marks[6].is_empty(), "{screen}");
        assert_eq!(marks[1], "«»");
        assert_eq!(marks[2], "«");
        assert_eq!(marks[3], "");

        viewer.handle(Command::ScrollEnd);
        let (screen, _) = draw_marks(&mut viewer, 10, 8);
        assert!(screen.contains("\"«rstuvwxyz\""), "{screen}");
    }

    #[test]
    fn a_title_and_an_end_line_cut_short_draw_their_marks_as_marks() {
        let mut viewer = Viewer::new("/a/very/long/path/to/the/file.txt".to_owned());
        viewer.show(b"x\n", false);
        let (screen, marks) = draw_marks(&mut viewer, 30, 4);
        let rows: Vec<&str> = screen.lines().collect();
        assert!(rows[0].contains('»'), "{screen}");
        assert_eq!(marks[0], "»", "{screen}");
        viewer.show(b"x\n", true);
        let (screen, marks) = draw_marks(&mut viewer, 10, 4);
        let rows: Vec<&str> = screen.lines().collect();
        assert!(rows[2].contains('»'), "{screen}");
        assert_eq!(marks[2], "»", "{screen}");
    }

    #[test]
    fn wrapped_rows_after_the_first_start_with_a_break_mark() {
        let mut viewer = Viewer::new("f".to_owned());
        viewer.show(format!("{}\nab\n", "x".repeat(12)).as_bytes(), false);
        let (screen, marks) = draw_marks(&mut viewer, 5, 7);
        let rows: Vec<&str> = screen.lines().skip(1).take(4).collect();
        assert_eq!(
            rows,
            ["\"xxxxx\"", "\"»xxxx\"", "\"»xxx \"", "\"ab   \""],
            "{screen}"
        );
        assert_eq!(marks[1..=4], ["", "»", "»", ""]);

        let (screen, marks) = draw_marks(&mut viewer, 1, 6);
        let rows: Vec<&str> = screen.lines().skip(1).take(3).collect();
        assert_eq!(rows, ["\"x\"", "\"x\"", "\"x\""], "no room for the mark");
        assert!(marks.iter().all(String::is_empty));
    }

    #[test]
    fn scrolls_sideways_as_vims_z_commands() {
        let mut viewer = Viewer::new("f".to_owned());
        let mut text = format!("{}\n", "x".repeat(40));
        text.push_str(&"y\n".repeat(10));
        viewer.show(text.as_bytes(), false);
        viewer.handle(Command::ToggleWrap);
        // Three rows of text, nineteen cells wide beside the scroll bar.
        draw(&mut viewer, 20, 4);
        assert_eq!(viewer.width, 19);
        let mut press = |command| {
            viewer.handle(command);
            viewer.column
        };
        assert_eq!(press(Command::ScrollRight(3)), 3);
        assert_eq!(press(Command::ScrollLeft(2)), 1);
        assert_eq!(
            press(Command::ScrollLeft(5)),
            0,
            "not below the first column"
        );
        assert_eq!(
            press(Command::HalfRight(2)),
            18,
            "two halves of 19 cells, rounded down"
        );
        assert_eq!(press(Command::HalfLeft(1)), 9);
        assert_eq!(press(Command::HalfLeft(3)), 0);
        assert_eq!(
            press(Command::ScrollEnd),
            21,
            "the end of the longest line at the edge"
        );
        assert_eq!(press(Command::ScrollStart), 0);
        assert_eq!(
            press(Command::ScrollRight(100)),
            21,
            "no further than the end of the longest line at the right edge"
        );
        assert_eq!(press(Command::Right), 21, "Right stops there too");
        assert_eq!(press(Command::HalfRight(1)), 21);
        assert_eq!(press(Command::Left), 20);

        viewer.handle(Command::Scroll(Motion::Down));
        draw(&mut viewer, 20, 4);
        assert_eq!(
            viewer.column, 0,
            "the long line is off screen and the lines on screen fit"
        );
        viewer.handle(Command::ScrollRight(5));
        assert_eq!(viewer.column, 0, "nor do they scroll");
    }

    #[test]
    fn the_lines_scroll_back_as_far_as_the_lines_on_screen_need() {
        let mut viewer = Viewer::new("f".to_owned());
        let text = format!("{}\n{}\nz\n", "x".repeat(40), "y".repeat(25));
        viewer.show(text.as_bytes(), false);
        viewer.handle(Command::ToggleWrap);
        // Two rows of text, nineteen cells wide beside the scroll bar.
        draw(&mut viewer, 20, 3);
        assert_eq!(viewer.width, 19);
        viewer.handle(Command::ScrollRight(100));
        assert_eq!(viewer.column, 21);
        viewer.handle(Command::Scroll(Motion::Down));
        let screen = draw(&mut viewer, 20, 3);
        assert_eq!(viewer.column, 6, "the end of the line of 25 at the edge");
        let rows: Vec<&str> = screen.lines().collect();
        assert!(rows[1].starts_with("\"«yyyyyyyyyyyyyyyyyy"), "{screen}");
    }

    #[test]
    fn wrapped_lines_hold_still_sideways() {
        let mut viewer = Viewer::new("f".to_owned());
        viewer.show("x".repeat(40).as_bytes(), false);
        let before = draw(&mut viewer, 20, 6);
        for command in [
            Command::Right,
            Command::ScrollRight(3),
            Command::HalfRight(1),
            Command::ScrollEnd,
            Command::ScrollLeft(1),
            Command::HalfLeft(1),
            Command::ScrollStart,
        ] {
            viewer.handle(command);
            assert_eq!(viewer.column, 0, "{command:?}");
            assert_eq!(draw(&mut viewer, 20, 6), before, "{command:?}");
        }
    }

    #[test]
    fn says_when_it_shows_only_the_start() {
        let mut viewer = Viewer::new("big".to_owned());
        viewer.show(&numbered(3), true);
        let text = draw(&mut viewer, 60, 6);
        assert!(text.contains("End of the first 16 MiB"), "{text}");
        assert!(text.contains("─ end of the first 16 MiB ─"), "{text}");
    }
}
