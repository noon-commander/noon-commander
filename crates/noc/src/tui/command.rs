//! The command line of `!` and `:` (ADR 0019): a shell command, typed over one or more lines,
//! which the app runs in the active panel's directory with the terminal handed over.

use std::path::PathBuf;

use noc_vfs::RemotePath;

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;
use unicode_width::UnicodeWidthChar as _;

use super::cells;
use super::keymap::{Action, Resolved};
use super::tasks::HostHandle;

/// Rows the command line takes at most, however tall the screen.
const MAX_ROWS: u16 = 10;
/// What starts the lines of a command after the first.
const CONTINUATION: &str = "> ";

/// What the text of the line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Opened with `!`: a shell command.
    Shell,
    /// Opened with `:`: a command of Noon Commander, of which `!` and a shell command is the
    /// only one so far.
    Noc,
}

/// What the app does after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandEvent {
    /// Nothing.
    None,
    /// Closes the line.
    Close,
    /// Closes the line and runs this shell command.
    Run(String),
    /// Says that Noon Commander has no such command; the line stays.
    Unknown(String),
    /// Opens the command in the editor.
    Edit,
    /// Shows the command before in the history.
    Older,
    /// Shows the command after in the history.
    Newer,
    /// Opens the window of the command history.
    History,
}

/// A shell command for the event loop to run, and where.
#[derive(Debug)]
pub(crate) struct Run {
    pub(crate) place: Place,
    pub(crate) command: String,
}

/// Where a command runs: in a local directory, or in a directory on a connected host.
#[derive(Debug)]
pub(crate) enum Place {
    Local(PathBuf),
    Remote { handle: HostHandle, dir: RemotePath },
}

/// The text of the command line and its cursor.
#[derive(Debug)]
pub(crate) struct CommandLine {
    kind: Kind,
    text: String,
    /// In characters.
    cursor: usize,
    /// The first row shown, when the command has more rows than fit.
    scroll: usize,
    /// The command of the history shown, counted from the newest, and what was typed before
    /// going through the history, which comes back after the newest.
    browsing: Option<usize>,
    draft: String,
    /// The command came from the history of another host: the prompt stands out until the
    /// next key, so that it is clear where the command will run.
    foreign: bool,
}

/// A row of the command line as drawn: the prompt or the mark of a continued line, if it
/// starts a line, and the text.
#[derive(Debug, Default, PartialEq, Eq)]
struct Row {
    lead: String,
    text: String,
}

impl CommandLine {
    pub(crate) fn new(kind: Kind) -> Self {
        Self {
            kind,
            text: String::new(),
            cursor: 0,
            scroll: 0,
            browsing: None,
            draft: String::new(),
            foreign: false,
        }
    }

    /// Puts `command`, from the window of the history, on the line in place of what was
    /// typed; `foreign` if it ran on another host.
    pub(crate) fn take(&mut self, command: &str, foreign: bool) {
        self.set_text(command);
        self.browsing = None;
        self.foreign = foreign;
    }

    /// Shows the command before in `commands`, the history of the panel's host newest first,
    /// or the one after; after the newest, what was typed. The cursor goes to the end, so
    /// that Up goes through the lines of a command before the commands before it.
    pub(crate) fn browse(&mut self, older: bool, commands: &[&str]) {
        let next = match (self.browsing, older) {
            (None, true) => 0,
            (Some(index), true) => index + 1,
            (None, false) => return,
            (Some(0), false) => {
                self.browsing = None;
                let draft = std::mem::take(&mut self.draft);
                self.set_text(&draft);
                return;
            }
            (Some(index), false) => index - 1,
        };
        let Some(command) = commands.get(next) else {
            return;
        };
        if self.browsing.is_none() {
            self.draft = self.text.clone();
        }
        self.browsing = Some(next);
        self.set_text(command);
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    /// The command as typed.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the command with `text`, such as what the editor left, without the line breaks
    /// at its end, and puts the cursor after it.
    pub(crate) fn set_text(&mut self, text: &str) {
        self.text = clean(text.trim_end_matches(['\n', '\r']));
        self.cursor = self.text.chars().count();
    }

    /// Puts pasted `text` at the cursor as it is, line breaks included; nothing runs.
    pub(crate) fn paste(&mut self, text: &str) {
        let text = clean(text);
        let offset = self.offset(self.cursor);
        self.text.insert_str(offset, &text);
        self.cursor += text.chars().count();
    }

    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }

    /// The byte offset of character `index`.
    fn offset(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(offset, _)| offset)
    }

    /// The first character of the line the cursor is on, and the end of that line.
    fn line_bounds(&self) -> (usize, usize) {
        let chars = self.chars();
        let start = chars[..self.cursor]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |newline| newline + 1);
        let end = chars[self.cursor..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(chars.len(), |newline| self.cursor + newline);
        (start, end)
    }

    fn insert(&mut self, c: char) {
        let offset = self.offset(self.cursor);
        self.text.insert(offset, c);
        self.cursor += 1;
    }

    /// Removes the characters in `start..end`, and leaves the cursor at `start`.
    fn remove(&mut self, start: usize, end: usize) {
        let (from, to) = (self.offset(start), self.offset(end));
        self.text.replace_range(from..to, "");
        self.cursor = start;
    }

    /// Takes a key.
    pub(crate) fn handle(&mut self, input: Resolved) -> CommandEvent {
        self.foreign = false;
        let action = match input {
            Resolved::Insert(c) => {
                self.insert(c);
                return CommandEvent::None;
            }
            Resolved::Action(action) => action,
        };
        let length = self.text.chars().count();
        let (start, end) = self.line_bounds();
        match action {
            Action::Left => self.cursor = self.cursor.saturating_sub(1),
            Action::Right => self.cursor = (self.cursor + 1).min(length),
            Action::Home => self.cursor = start,
            Action::End => self.cursor = end,
            Action::Up if start == 0 => return CommandEvent::Older,
            Action::Down if end == length => return CommandEvent::Newer,
            Action::Up => self.move_line(false),
            Action::Down => self.move_line(true),
            Action::CommandHistory => return CommandEvent::History,
            Action::OlderCommand => return CommandEvent::Older,
            Action::NewerCommand => return CommandEvent::Newer,
            Action::Backspace if self.text.is_empty() => return CommandEvent::Close,
            Action::Backspace if self.cursor > 0 => self.remove(self.cursor - 1, self.cursor),
            Action::Delete if self.cursor < length => self.remove(self.cursor, self.cursor + 1),
            Action::DeleteToStart => self.remove(start, self.cursor),
            Action::DeleteToEnd => {
                let cursor = self.cursor;
                self.remove(cursor, end);
            }
            Action::NewLine => self.insert('\n'),
            Action::EditCommand => return CommandEvent::Edit,
            Action::Confirm => return self.confirm(end),
            Action::Cancel => return CommandEvent::Close,
            _ => {}
        }
        CommandEvent::None
    }

    /// Puts the cursor on the next line, or the one before, in the same column or at the end
    /// of a shorter line. From the last line or the first one, it stays.
    fn move_line(&mut self, next: bool) {
        let chars = self.chars();
        let (start, end) = self.line_bounds();
        let column = self.cursor - start;
        let (start, end) = if next {
            if end == chars.len() {
                return;
            }
            let start = end + 1;
            let end = chars[start..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(chars.len(), |newline| start + newline);
            (start, end)
        } else {
            if start == 0 {
                return;
            }
            let end = start - 1;
            let start = chars[..end]
                .iter()
                .rposition(|&c| c == '\n')
                .map_or(0, |newline| newline + 1);
            (start, end)
        };
        self.cursor = (start + column).min(end);
    }

    /// Enter: a new line after an odd number of `\` at the end of the line, as a shell
    /// continues one; otherwise the command to run, if there is one.
    fn confirm(&mut self, end: usize) -> CommandEvent {
        let chars = self.chars();
        let escapes = chars[..self.cursor]
            .iter()
            .rev()
            .take_while(|&&c| c == '\\')
            .count();
        if self.cursor == end && escapes % 2 == 1 {
            self.insert('\n');
            return CommandEvent::None;
        }
        let command = match self.kind {
            Kind::Shell => self.text.clone(),
            Kind::Noc => match self.text.strip_prefix('!') {
                Some(command) => command.to_owned(),
                None if self.text.trim().is_empty() => return CommandEvent::Close,
                None => return CommandEvent::Unknown(self.text.clone()),
            },
        };
        if command.trim().is_empty() {
            CommandEvent::Close
        } else {
            CommandEvent::Run(command)
        }
    }

    /// The rows of the command in `width` cells, after `prompt`, and the row and column of the
    /// cursor. Long lines go on in the next row.
    fn layout(&self, prompt: &str, width: usize) -> (Vec<Row>, (usize, usize)) {
        let width = width.max(1);
        let mut rows = Vec::new();
        let mut at = (0, 0);
        let mut index = 0;
        for (number, line) in self.text.split('\n').enumerate() {
            let lead = if number == 0 { prompt } else { CONTINUATION };
            let mut row = Row {
                lead: lead.to_owned(),
                text: String::new(),
            };
            let mut used = cells::width(lead);
            for c in line.chars() {
                let c = shown(c);
                let cell = c.width().unwrap_or(0);
                if used + cell > width && used > 0 {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                if index == self.cursor {
                    at = (rows.len(), used);
                }
                row.text.push(c);
                used += cell;
                index += 1;
            }
            if index == self.cursor {
                if used >= width {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                at = (rows.len(), used);
            }
            rows.push(row);
            // The newline.
            index += 1;
        }
        (rows, at)
    }

    /// The rows the line takes in `width` cells after `prompt`, on a screen `height` rows
    /// tall: as many as the command has, up to a third of the screen and [`MAX_ROWS`].
    pub(crate) fn height(&self, prompt: &str, width: u16, height: u16) -> u16 {
        let most = (height / 3).clamp(1, MAX_ROWS);
        let rows = self.layout(prompt, usize::from(width)).0.len();
        u16::try_from(rows).unwrap_or(u16::MAX).min(most)
    }

    /// Draws the line in `area`, after `prompt`, and puts the cursor there if it has the keys.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        prompt: &str,
        style: Style,
        focused: bool,
    ) {
        if area.is_empty() {
            return;
        }
        let (rows, (row, column)) = self.layout(prompt, usize::from(area.width));
        let height = usize::from(area.height);
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + height {
            self.scroll = row + 1 - height;
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(height));
        frame.render_widget(Block::default().style(style), area);
        let lead = if self.foreign {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        };
        for (index, shown) in rows.into_iter().skip(self.scroll).take(height).enumerate() {
            let y = area.y + u16::try_from(index).unwrap_or(u16::MAX);
            let line = Line::from(vec![Span::styled(shown.lead, lead), Span::raw(shown.text)]);
            frame.render_widget(line.style(style), Rect::new(area.x, y, area.width, 1));
        }
        if focused {
            let x = u16::try_from(column).unwrap_or(u16::MAX);
            let y = u16::try_from(row - self.scroll).unwrap_or(u16::MAX);
            frame.set_cursor_position(Position::new(
                area.x.saturating_add(x).min(area.right() - 1),
                area.y + y,
            ));
        }
    }
}

/// `text` with its line breaks as `\n`, and without control characters other than line breaks
/// and tabs.
fn clean(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
        .collect()
}

/// `c` as the line shows it: a tab as a space, and another control character, which could move
/// the cursor, as `?`.
fn shown(c: char) -> char {
    if c == '\t' {
        return ' ';
    }
    let mut bytes = [0; 4];
    cells::sanitize(c.encode_utf8(&mut bytes).as_bytes())
        .chars()
        .next()
        .unwrap_or('?')
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn typed(line: &mut CommandLine, text: &str) {
        for c in text.chars() {
            let input = if c == '\n' {
                Resolved::Action(Action::NewLine)
            } else {
                Resolved::Insert(c)
            };
            assert_eq!(line.handle(input), CommandEvent::None);
        }
    }

    fn press(line: &mut CommandLine, action: Action) -> CommandEvent {
        line.handle(Resolved::Action(action))
    }

    fn shell(text: &str) -> CommandLine {
        let mut line = CommandLine::new(Kind::Shell);
        typed(&mut line, text);
        line
    }

    fn draw(line: &mut CommandLine, prompt: &str, width: u16) -> String {
        let height = line.height(prompt, width, 30);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| line.render(frame, frame.area(), prompt, Style::default(), true))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn enter_runs_what_was_typed() {
        let mut line = shell("ls -l");
        assert_eq!(
            press(&mut line, Action::Confirm),
            CommandEvent::Run("ls -l".to_owned())
        );
        let mut blank = shell("  ");
        assert_eq!(press(&mut blank, Action::Confirm), CommandEvent::Close);
    }

    #[test]
    fn a_command_spans_lines_with_ctrl_j_or_a_backslash() {
        let mut line = shell("for f in *; do\necho \"$f\"; \\");
        assert_eq!(press(&mut line, Action::Confirm), CommandEvent::None);
        typed(&mut line, "done");
        assert_eq!(
            press(&mut line, Action::Confirm),
            CommandEvent::Run("for f in *; do\necho \"$f\"; \\\ndone".to_owned())
        );
        // An escaped backslash ends the command, as in a shell.
        let mut escaped = shell("echo \\\\");
        assert_eq!(
            press(&mut escaped, Action::Confirm),
            CommandEvent::Run("echo \\\\".to_owned())
        );
        // The backslash counts only at the end of the line.
        let mut inside = shell("a\\b");
        press(&mut inside, Action::Left);
        assert_eq!(
            press(&mut inside, Action::Confirm),
            CommandEvent::Run("a\\b".to_owned())
        );
    }

    #[test]
    fn colon_runs_a_shell_command_after_a_bang() {
        let mut line = CommandLine::new(Kind::Noc);
        typed(&mut line, "!make");
        assert_eq!(
            press(&mut line, Action::Confirm),
            CommandEvent::Run("make".to_owned())
        );
        let mut unknown = CommandLine::new(Kind::Noc);
        typed(&mut unknown, "wq");
        assert_eq!(
            press(&mut unknown, Action::Confirm),
            CommandEvent::Unknown("wq".to_owned())
        );
        let mut empty = CommandLine::new(Kind::Noc);
        assert_eq!(press(&mut empty, Action::Confirm), CommandEvent::Close);
    }

    #[test]
    fn esc_or_backspace_on_an_empty_line_closes_it() {
        let mut line = shell("x");
        assert_eq!(press(&mut line, Action::Backspace), CommandEvent::None);
        assert_eq!(press(&mut line, Action::Backspace), CommandEvent::Close);
        assert_eq!(press(&mut shell("x"), Action::Cancel), CommandEvent::Close);
    }

    #[test]
    fn pasted_text_goes_in_whole_and_never_runs() {
        let mut line = shell("echo ");
        press(&mut line, Action::Left);
        line.paste("a\r\nb\rc\u{1b}[2J\td");
        assert_eq!(line.text(), "echoa\nb\nc[2J\td ");
        assert_eq!(line.cursor, 14, "after the pasted text");
        line.set_text("make\n  && ls\n\n");
        assert_eq!(line.text(), "make\n  && ls");
        assert_eq!(line.cursor, 12);
    }

    #[test]
    fn up_and_down_go_through_the_history_from_the_first_and_last_lines() {
        let history = ["ls", "make\ntest"];
        let mut line = shell("draft");
        assert_eq!(press(&mut line, Action::Up), CommandEvent::Older);
        line.browse(true, &history);
        assert_eq!(line.text(), "ls");
        line.browse(true, &history);
        assert_eq!(line.text(), "make\ntest");
        line.browse(true, &history);
        assert_eq!(line.text(), "make\ntest", "the oldest stays");
        // The cursor is at the end: Up goes through the lines first, Down goes on at once.
        assert_eq!(press(&mut line, Action::Up), CommandEvent::None);
        assert_eq!(press(&mut line, Action::Up), CommandEvent::Older);
        assert_eq!(press(&mut line, Action::Down), CommandEvent::None);
        assert_eq!(press(&mut line, Action::Down), CommandEvent::Newer);
        line.browse(false, &history);
        assert_eq!(line.text(), "ls");
        line.browse(false, &history);
        assert_eq!(line.text(), "draft", "what was typed comes back");
        line.browse(false, &history);
        assert_eq!(line.text(), "draft");
        assert_eq!(press(&mut line, Action::OlderCommand), CommandEvent::Older);
        assert_eq!(press(&mut line, Action::NewerCommand), CommandEvent::Newer);
    }

    #[test]
    fn keys_edit_within_the_line_of_the_cursor() {
        let mut line = shell("first line\nsecond");
        press(&mut line, Action::Home);
        assert_eq!(line.cursor, 11, "the start of its line");
        press(&mut line, Action::End);
        press(&mut line, Action::Up);
        assert_eq!(line.cursor, 6, "the same column above");
        press(&mut line, Action::DeleteToEnd);
        assert_eq!(line.text, "first \nsecond");
        press(&mut line, Action::Down);
        assert_eq!(line.cursor, 13, "the end of a shorter line");
        press(&mut line, Action::Down);
        assert_eq!(line.cursor, 13, "the last line stays");
        press(&mut line, Action::DeleteToStart);
        assert_eq!(line.text, "first \n");
        press(&mut line, Action::Backspace);
        press(&mut line, Action::Left);
        press(&mut line, Action::Delete);
        assert_eq!(line.text, "first");
        press(&mut line, Action::Up);
        assert_eq!(line.cursor, 5, "the first line stays");
    }

    #[test]
    fn rows_wrap_and_lines_after_the_first_are_marked() {
        let line = shell("echo 12345\nwc");
        let (rows, at) = line.layout("~ $ ", 10);
        let rows: Vec<(&str, &str)> = rows
            .iter()
            .map(|row| (row.lead.as_str(), row.text.as_str()))
            .collect();
        assert_eq!(rows, [("~ $ ", "echo 1"), ("", "2345"), ("> ", "wc")]);
        assert_eq!(at, (2, 4));
        // A cursor after a full row goes to the next one.
        assert_eq!(shell("123456").layout("~ $ ", 10).1, (1, 0));
    }

    #[test]
    fn takes_up_to_a_third_of_the_screen() {
        let line = shell("1\n2\n3\n4\n5");
        assert_eq!(line.height("$ ", 40, 30), 5);
        assert_eq!(line.height("$ ", 40, 9), 3);
        assert_eq!(line.height("$ ", 40, 2), 1);
        let tall = shell(&"x\n".repeat(20));
        assert_eq!(tall.height("$ ", 40, 60), MAX_ROWS);
    }

    #[test]
    fn scrolls_to_the_cursor() {
        let mut line = shell(
            &(1..=12)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let bottom = draw(&mut line, "$ ", 12);
        assert!(
            bottom.contains("> 12") && !bottom.contains("$ 1"),
            "{bottom}"
        );
        for _ in 0..11 {
            press(&mut line, Action::Up);
        }
        let top = draw(&mut line, "$ ", 12);
        assert!(top.contains("$ 1") && !top.contains("> 12"), "{top}");
    }

    #[test]
    fn draws_the_prompt_and_the_lines() {
        let mut line = shell("tar czf a.tgz \\\n  src docs");
        insta::assert_snapshot!(draw(&mut line, "~/src/noc $ ", 24));
    }
}
