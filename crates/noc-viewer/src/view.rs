//! The viewer on screen: a file's text, scrolled, with long lines wrapped or cut.

use noc_scroll::{Motion, Step, scrollbar};
use noc_text::Align;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::i18n::fl;
use crate::text::{cut, lines_of, rows};

/// What the viewer is asked to do. The app maps its keys to these; closing the viewer is the
/// app's own business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Scrolls; never past the end of the file at the bottom of the screen. Home goes back to
    /// the first column too.
    Scroll(Motion),
    /// One column left, when lines are cut.
    Left,
    /// One column right, when lines are cut.
    Right,
    /// Wraps long lines, or cuts them.
    ToggleWrap,
}

/// The colors of the viewer, from the app's theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Styles {
    /// The title line: the path and the position.
    pub header: Style,
    /// The text, the screen behind it, and the scroll bar; dimmed, the line that ends the text.
    pub text: Style,
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
        }
    }

    /// Shows `bytes`, the start of the file if `truncated`.
    pub fn show(&mut self, bytes: &[u8], truncated: bool) {
        self.state = State::Text {
            lines: lines_of(bytes),
            truncated,
        };
        self.top = (0, 0);
    }

    /// Scrolls, or switches wrapping.
    pub fn handle(&mut self, command: Command) {
        match command {
            Command::Scroll(motion) => self.scroll(motion),
            Command::Left => self.column = self.column.saturating_sub(1),
            Command::Right if !self.wrap => self.column += 1,
            Command::Right => {}
            Command::ToggleWrap => {
                self.wrap = !self.wrap;
                self.top.1 = 0;
                self.column = 0;
            }
        }
    }

    fn lines(&self) -> &[String] {
        match &self.state {
            State::Text { lines, .. } => lines,
            State::Loading => &[],
        }
    }

    /// The rows line `index` takes on screen.
    fn rows_of(&self, index: usize) -> usize {
        match self.lines().get(index) {
            Some(line) if self.wrap => rows(line, self.width).len(),
            _ => 1,
        }
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
        if area.height < 2 || area.width == 0 {
            return;
        }
        self.width = usize::from(area.width);
        self.page = usize::from(area.height - 1);
        // The scroll bar takes the last column when the text does not fit, which, wrapped
        // narrower, fits even less.
        let bar = area.width > 1 && self.after((0, 0), self.page - 1) != self.end();
        if bar {
            self.width -= 1;
        }
        // The width may have changed since the position was taken.
        self.top.1 = self.top.1.min(self.rows_of(self.top.0).saturating_sub(1));
        let row = |index: u16| Rect::new(area.x, area.y + index, area.width, 1);
        let header = self.header(usize::from(area.width));
        frame.render_widget(Line::styled(header, styles.header), row(0));
        let text_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        frame.buffer_mut().set_style(text_area, styles.text);
        if matches!(self.state, State::Loading) {
            frame.render_widget(Line::raw(fl!("viewer-loading")), row(1));
            return;
        }
        if bar {
            let column = Rect::new(area.right() - 1, text_area.y, 1, text_area.height);
            scrollbar::render(frame, column, self.thumb(), styles.text);
        }
        let mut position = self.top;
        for screen_row in 0..self.page {
            let y = u16::try_from(screen_row + 1).unwrap_or(u16::MAX);
            let Some(line) = self.lines().get(position.0) else {
                let end = noc_text::fit(&self.end_text(), self.width, Align::Center);
                let dim = styles.text.add_modifier(Modifier::DIM);
                frame.render_widget(Line::styled(end, dim), row(y));
                break;
            };
            let text = if self.wrap {
                rows(line, self.width)
                    .get(position.1)
                    .copied()
                    .unwrap_or_default()
                    .to_owned()
            } else {
                cut(line, self.column, self.width)
            };
            frame.render_widget(Line::from(Span::raw(text)), row(y));
            let next = self.after(position, 1);
            if next == position {
                break;
            }
            position = next;
        }
    }

    /// The line that marks the end of the text.
    fn end_text(&self) -> String {
        match self.state {
            State::Text {
                truncated: true, ..
            } => fl!("viewer-end-truncated"),
            _ => fl!("viewer-end"),
        }
    }

    /// For the scroll bar, in lines of the file and the line that ends it: how many, how many
    /// show, and the first on screen, which is the last there can be once the end shows.
    fn thumb(&self) -> (usize, usize, usize) {
        let total = self.lines().len() + 1;
        let bottom = self.bottom();
        let shown = (bottom.0 + 1 - self.top.0).clamp(1, total.saturating_sub(1).max(1));
        let first = if bottom == self.end() {
            total - shown
        } else {
            self.top.0
        };
        (total, shown, first)
    }

    /// The path on the left, and where the screen is on the right, `width` cells wide in all:
    /// its first line, the lines, and how far the last one on screen is, in percent, or that the
    /// end shows.
    fn header(&self, width: usize) -> String {
        let lines = self.lines().len();
        let mut position = String::new();
        if let State::Text { truncated, .. } = &self.state {
            let line = (self.top.0 + 1).min(lines).to_string();
            let bottom = self.bottom();
            position = if bottom == self.end() {
                fl!(
                    "viewer-position-end",
                    line = line,
                    lines = lines.to_string()
                )
            } else {
                let percent = ((bottom.0 + 1) * 100).checked_div(lines).unwrap_or(100);
                fl!(
                    "viewer-position",
                    line = line,
                    lines = lines.to_string(),
                    percent = percent.to_string()
                )
            };
            if *truncated {
                position = fl!("viewer-truncated", position = position);
            }
        }
        let position = format!(" {position} ");
        let room = width.saturating_sub(noc_text::width(&position));
        let title = noc_text::fit(&format!(" {}", self.title), room, Align::Left);
        title + &position
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn draw(viewer: &mut Viewer, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| viewer.render(frame, frame.area(), &Styles::default()))
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
        assert!(end.contains("line 20") && end.contains("— end —"), "{end}");
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
    fn a_scroll_bar_shows_where_the_text_does_not_fit() {
        let mut viewer = Viewer::new("f".to_owned());
        viewer.show(&numbered(20), false);
        let column = |screen: &str, row: usize| {
            let line = screen.lines().nth(row).unwrap_or_default();
            line.trim_end_matches('"').chars().last()
        };
        let top = draw(&mut viewer, 20, 6);
        assert_eq!(column(&top, 1), Some('█'), "{top}");
        assert_eq!(column(&top, 5), Some('░'), "{top}");
        viewer.handle(Command::Scroll(Motion::End));
        let end = draw(&mut viewer, 20, 6);
        assert_eq!(column(&end, 1), Some('░'), "{end}");
        assert_eq!(column(&end, 5), Some('█'), "{end}");

        viewer.show(&numbered(3), false);
        let short = draw(&mut viewer, 20, 6);
        assert!(!short.contains('█') && !short.contains('░'), "{short}");
        assert!(short.contains("1/3 End"), "{short}");
        let end_row = short.lines().nth(4).unwrap_or_default();
        assert!(end_row.contains("— end —"), "right after the text: {short}");
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
        assert!(text.contains("\"uvwxyzabcdefghijklmn█"), "{text}");
        viewer.handle(Command::Scroll(Motion::Up));
        assert!(small(&mut viewer).contains("\"abcdefghijklmnopqrst█"));

        viewer.handle(Command::Right);
        assert!(
            small(&mut viewer).contains("\"abcdefghijklmnopqrst█"),
            "wrapped lines do not scroll sideways"
        );
        viewer.handle(Command::ToggleWrap);
        viewer.handle(Command::Scroll(Motion::Home));
        viewer.handle(Command::Right);
        viewer.handle(Command::Right);
        let cut = draw(&mut viewer, 20, 6);
        assert!(cut.contains("\"cdefghijklmnopqrstuv\""), "{cut}");
        assert!(cut.contains("ort"), "{cut}");
    }

    #[test]
    fn says_when_it_shows_only_the_start() {
        let mut viewer = Viewer::new("big".to_owned());
        viewer.show(&numbered(3), true);
        let text = draw(&mut viewer, 60, 6);
        assert!(text.contains("End of the first 16 MiB"), "{text}");
        assert!(text.contains("— end of the first 16 MiB —"), "{text}");
    }
}
