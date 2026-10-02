//! The viewer of F3: a file's text, scrolled, with long lines wrapped or cut.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar as _;

use super::cells::{self, Align};
use super::keymap::Action;
use super::theme::Theme;
use crate::i18n::fl;

/// Columns between tab stops.
const TAB: usize = 8;

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
pub(crate) struct Viewer {
    id: u64,
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
    /// The viewer `id` of the file `title`, while it loads.
    pub(crate) fn new(id: u64, title: String) -> Self {
        Self {
            id,
            title,
            state: State::Loading,
            top: (0, 0),
            column: 0,
            wrap: true,
            page: 1,
            width: 80,
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Shows `bytes`, the start of the file if `truncated`.
    pub(crate) fn show(&mut self, bytes: &[u8], truncated: bool) {
        self.state = State::Text {
            lines: lines_of(bytes),
            truncated,
        };
        self.top = (0, 0);
    }

    /// Scrolls, or switches wrapping; `true` when the viewer should close.
    pub(crate) fn handle(&mut self, action: Action) -> bool {
        match action {
            Action::Up => self.up(1),
            Action::Down => self.down(1),
            Action::PageUp => self.up(self.page),
            Action::PageDown => self.down(self.page),
            Action::Home => {
                self.top = (0, 0);
                self.column = 0;
            }
            Action::End => self.top = self.last_top(),
            Action::Left => self.column = self.column.saturating_sub(1),
            Action::Right if !self.wrap => self.column += 1,
            Action::ToggleWrap => {
                self.wrap = !self.wrap;
                self.top.1 = 0;
                self.column = 0;
            }
            Action::Quit | Action::Cancel => return true,
            _ => {}
        }
        false
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
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        if area.height < 2 || area.width == 0 {
            return;
        }
        self.width = usize::from(area.width);
        self.page = usize::from(area.height - 1);
        // The width may have changed since the position was taken.
        self.top.1 = self.top.1.min(self.rows_of(self.top.0).saturating_sub(1));
        let row = |index: u16| Rect::new(area.x, area.y + index, area.width, 1);
        frame.render_widget(Line::styled(self.header(), theme.cursor), row(0));
        let text_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        frame.buffer_mut().set_style(text_area, theme.panel);
        if matches!(self.state, State::Loading) {
            frame.render_widget(Line::raw(fl!("panel-loading")), row(1));
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
        let room = width.saturating_sub(cells::width(&position));
        let title = cells::fit(&format!(" {}", self.title), room, Align::Left);
        title + &position
    }
}

/// The lines of `bytes`, as UTF-8 with invalid bytes replaced: `\r\n` ends a line too, tabs
/// go to the next stop, and what a terminal would act on is shown safely.
fn lines_of(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_suffix('\n').unwrap_or(&text);
    text.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let mut expanded = String::with_capacity(line.len());
            let mut column = 0;
            for c in line.chars() {
                if c == '\t' {
                    let spaces = TAB - column % TAB;
                    expanded.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                } else {
                    expanded.push(c);
                    column += c.width().unwrap_or(0);
                }
            }
            cells::sanitize(expanded.as_bytes())
        })
        .collect()
}

/// `line` in rows of at most `width` cells, broken anywhere, as mc wraps; at least one row,
/// and at least one character in each.
fn rows(line: &str, width: usize) -> Vec<&str> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (index, c) in line.char_indices() {
        let char_width = c.width().unwrap_or(0);
        if used + char_width > width && index > start {
            rows.push(&line[start..index]);
            (start, used) = (index, 0);
        }
        used += char_width;
    }
    rows.push(&line[start..]);
    rows
}

/// The cells `skip` … `skip + width` of `line`; a wide character cut in two shows as a space.
fn cut(line: &str, skip: usize, width: usize) -> String {
    let mut text = String::new();
    let (mut column, mut used) = (0, 0);
    for c in line.chars() {
        let char_width = c.width().unwrap_or(0);
        let start = column;
        column += char_width;
        if column <= skip {
            continue;
        }
        let shown = if start < skip {
            column - skip
        } else {
            char_width
        };
        if used + shown > width {
            break;
        }
        if shown < char_width {
            text.push_str(&" ".repeat(shown));
        } else {
            text.push(c);
        }
        used += shown;
    }
    text
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn draw(viewer: &mut Viewer, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| viewer.render(frame, frame.area(), &Theme::terminal()))
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
    fn makes_lines_safe_and_expands_tabs() {
        let lines = lines_of(b"a\tb\r\nwide\xe6\x96\x87\tx\n\x1b[2Jbad \xff\n\n");
        assert_eq!(
            lines,
            ["a       b", "wide文  x", "?[2Jbad \u{fffd}", ""],
            "one line per newline, the last ending none"
        );
        assert_eq!(lines_of(b""), [""]);
    }

    #[test]
    fn wraps_and_cuts_by_cells() {
        assert_eq!(rows("abcdefg", 3), ["abc", "def", "g"]);
        assert_eq!(rows("", 3), [""]);
        assert_eq!(rows("文文文", 5), ["文文", "文"]);
        assert_eq!(
            rows("文", 1),
            ["文"],
            "a character wider than a row gets one"
        );
        assert_eq!(cut("abcdef", 2, 3), "cde");
        assert_eq!(
            cut("文文文", 1, 4),
            " 文",
            "a cut half is a space, and what does not fit is left out"
        );
        assert_eq!(cut("ab", 5, 3), "");
    }

    #[test]
    fn scrolls_within_the_text() {
        let mut viewer = Viewer::new(1, "/srv/log".to_owned());
        assert!(draw(&mut viewer, 30, 6).contains("Loading"));
        viewer.show(&numbered(20), false);
        let top = draw(&mut viewer, 30, 6);
        assert!(top.contains("/srv/log"), "{top}");
        assert!(top.contains("1/20 25%"), "{top}");
        assert!(top.contains("line 5") && !top.contains("line 6"), "{top}");

        viewer.handle(Action::PageDown);
        assert!(draw(&mut viewer, 30, 6).contains("line 6"));
        viewer.handle(Action::End);
        let end = draw(&mut viewer, 30, 6);
        assert!(end.contains("16/20 100%"), "{end}");
        viewer.handle(Action::Down);
        assert_eq!(draw(&mut viewer, 30, 6), end, "no further than the end");
        viewer.handle(Action::Home);
        assert!(draw(&mut viewer, 30, 6).contains("1/20"));
        assert!(viewer.handle(Action::Quit));
        assert!(viewer.handle(Action::Cancel));
    }

    #[test]
    fn wraps_long_lines_and_scrolls_sideways_without() {
        let mut viewer = Viewer::new(1, "f".to_owned());
        let long: String = ('a'..='z').cycle().take(50).collect();
        viewer.show(format!("{long}\nshort\n").as_bytes(), false);
        let wrapped = draw(&mut viewer, 20, 6);
        insta::assert_snapshot!(wrapped);
        // Three rows of text: Down goes into the rows of the long line, and stops where the
        // end is at the bottom.
        let small = |viewer: &mut Viewer| draw(viewer, 20, 4);
        small(&mut viewer);
        viewer.handle(Action::Down);
        viewer.handle(Action::Down);
        let text = small(&mut viewer);
        assert!(text.contains("short"), "{text}");
        assert!(text.contains("\"uvwxyzabcdefghijklmn\""), "{text}");
        viewer.handle(Action::Up);
        assert!(small(&mut viewer).contains("\"abcdefghijklmnopqrst\""));

        viewer.handle(Action::ToggleWrap);
        viewer.handle(Action::Home);
        viewer.handle(Action::Right);
        viewer.handle(Action::Right);
        let cut = draw(&mut viewer, 20, 6);
        assert!(cut.contains("\"cdefghijklmnopqrstuv\""), "{cut}");
        assert!(cut.contains("ort"), "{cut}");
    }

    #[test]
    fn says_when_it_shows_only_the_start() {
        let mut viewer = Viewer::new(1, "big".to_owned());
        viewer.show(&numbered(3), true);
        let text = draw(&mut viewer, 60, 6);
        assert!(text.contains("of the first 16 MiB"), "{text}");
    }
}
