//! The viewer on screen: a file's text, scrolled, with long lines wrapped or cut.

use noc_text::Align;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::i18n::fl;
use crate::text::{cut, lines_of, rows};

/// What the viewer is asked to do. The app maps its keys to these; closing the viewer is the
/// app's own business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Up,
    Down,
    PageUp,
    PageDown,
    /// The start of the file, and the first column.
    Home,
    /// The end of the file at the bottom of the screen.
    End,
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
    /// The text, and the screen behind it.
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
    /// The first line on screen, and its first row there.
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
            Command::Up => self.up(1),
            Command::Down => self.down(1),
            Command::PageUp => self.up(self.page),
            Command::PageDown => self.down(self.page),
            Command::Home => {
                self.top = (0, 0);
                self.column = 0;
            }
            Command::End => self.top = self.last_top(),
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

    /// The position `count` rows after `from`, or the last one.
    fn after(&self, (mut line, mut row): (usize, usize), count: usize) -> (usize, usize) {
        let lines = self.lines().len();
        for _ in 0..count {
            if row + 1 < self.rows_of(line) {
                row += 1;
            } else if line + 1 < lines {
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

    /// The top that shows the end of the file at the bottom of the screen.
    fn last_top(&self) -> (usize, usize) {
        let Some(last) = self.lines().len().checked_sub(1) else {
            return (0, 0);
        };
        let end = (last, self.rows_of(last) - 1);
        self.before(end, self.page.saturating_sub(1))
    }

    fn down(&mut self, count: usize) {
        let moved = self.after(self.top, count);
        self.top = moved.min(self.last_top());
    }

    fn up(&mut self, count: usize) {
        self.top = self.before(self.top, count);
    }

    /// Draws the viewer over `area`: a title line, then the text.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, styles: &Styles) {
        if area.height < 2 || area.width == 0 {
            return;
        }
        self.width = usize::from(area.width);
        self.page = usize::from(area.height - 1);
        // The width may have changed since the position was taken.
        self.top.1 = self.top.1.min(self.rows_of(self.top.0).saturating_sub(1));
        let row = |index: u16| Rect::new(area.x, area.y + index, area.width, 1);
        frame.render_widget(Line::styled(self.header(), styles.header), row(0));
        let text_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        frame.buffer_mut().set_style(text_area, styles.text);
        if matches!(self.state, State::Loading) {
            frame.render_widget(Line::raw(fl!("viewer-loading")), row(1));
            return;
        }
        let mut position = self.top;
        for screen_row in 0..self.page {
            let Some(line) = self.lines().get(position.0) else {
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
            let y = u16::try_from(screen_row + 1).unwrap_or(u16::MAX);
            frame.render_widget(Line::from(Span::raw(text)), row(y));
            let next = self.after(position, 1);
            if next == position {
                break;
            }
            position = next;
        }
    }

    /// The path on the left, and where the screen is on the right: its first line, the
    /// lines, and how far the last one on screen is, in percent.
    fn header(&self) -> String {
        let width = self.width;
        let lines = self.lines().len();
        let mut position = String::new();
        if let State::Text { truncated, .. } = &self.state {
            let last = self.after(self.top, self.page.saturating_sub(1)).0 + 1;
            let percent = (last * 100).checked_div(lines).unwrap_or(100);
            position = fl!(
                "viewer-position",
                line = (self.top.0 + 1).to_string(),
                lines = lines.to_string(),
                percent = percent.to_string()
            );
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
    fn scrolls_within_the_text() {
        let mut viewer = Viewer::new("/srv/log".to_owned());
        assert!(draw(&mut viewer, 30, 6).contains("Loading"));
        viewer.show(&numbered(20), false);
        let top = draw(&mut viewer, 30, 6);
        assert!(top.contains("/srv/log"), "{top}");
        assert!(top.contains("1/20 25%"), "{top}");
        assert!(top.contains("line 5") && !top.contains("line 6"), "{top}");

        viewer.handle(Command::PageDown);
        assert!(draw(&mut viewer, 30, 6).contains("line 6"));
        viewer.handle(Command::End);
        let end = draw(&mut viewer, 30, 6);
        assert!(end.contains("16/20 100%"), "{end}");
        viewer.handle(Command::Down);
        assert_eq!(draw(&mut viewer, 30, 6), end, "no further than the end");
        viewer.handle(Command::Home);
        assert!(draw(&mut viewer, 30, 6).contains("1/20"));
    }

    #[test]
    fn wraps_long_lines_and_scrolls_sideways_without() {
        let mut viewer = Viewer::new("f".to_owned());
        let long: String = ('a'..='z').cycle().take(50).collect();
        viewer.show(format!("{long}\nshort\n").as_bytes(), false);
        let wrapped = draw(&mut viewer, 20, 6);
        insta::assert_snapshot!(wrapped);
        // Three rows of text: Down goes into the rows of the long line, and stops where the
        // end is at the bottom.
        let small = |viewer: &mut Viewer| draw(viewer, 20, 4);
        small(&mut viewer);
        viewer.handle(Command::Down);
        viewer.handle(Command::Down);
        let text = small(&mut viewer);
        assert!(text.contains("short"), "{text}");
        assert!(text.contains("\"uvwxyzabcdefghijklmn\""), "{text}");
        viewer.handle(Command::Up);
        assert!(small(&mut viewer).contains("\"abcdefghijklmnopqrst\""));

        viewer.handle(Command::Right);
        assert!(
            small(&mut viewer).contains("\"abcdefghijklmnopqrst\""),
            "wrapped lines do not scroll sideways"
        );
        viewer.handle(Command::ToggleWrap);
        viewer.handle(Command::Home);
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
        assert!(text.contains("of the first 16 MiB"), "{text}");
    }
}
