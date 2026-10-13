//! The command line of `!` and `:` (ADR 0019): a shell command, typed over one or more lines,
//! which the app runs in the active panel's directory with the terminal handed over.

use std::ops::Range;
use std::path::PathBuf;

use noc_config::Pause;
use noc_vfs::RemotePath;

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;
use unicode_width::UnicodeWidthChar as _;

use super::cells;
use super::edit::{Editor, Registers, line_bounds};
use super::keymap::{Action, Mode, Resolved, Typed};
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
    /// What the terminal shows before the command, as a shell would.
    pub(crate) prompt: String,
    /// Whether its output waits for a key: `shell.pause`.
    pub(crate) pause: Pause,
}

/// Where a command runs: in a local directory, or in a directory on a connected host.
#[derive(Debug)]
pub(crate) enum Place {
    Local(PathBuf),
    Remote { handle: HostHandle, dir: RemotePath },
}

/// The text of the command line, in vim's modes: it opens in Insert mode, as vim's command
/// line, and Ctrl+f goes to Normal mode, where vim opens the window of the command line.
#[derive(Debug)]
pub(crate) struct CommandLine {
    kind: Kind,
    editor: Editor,
    /// The first row shown, when the command has more rows than fit.
    scroll: usize,
    /// The command of the history shown, counted from the newest, and what was typed before
    /// going through the history, which comes back after the newest.
    browsing: Option<usize>,
    draft: String,
    /// The text that going through the history showed last, to tell whether it changed since.
    browsed: String,
    /// The column that `j` and `k` keep, as vim's `curswant`, while one follows another; past
    /// the end of every line after `$`.
    column: Option<usize>,
    /// The command came from the history of another host: the prompt stands out until the
    /// next key, so that it is clear where the command will run.
    foreign: bool,
}

/// A row of the command line as drawn: the prompt or the mark of a continued line, if it
/// starts a line, the text, and what of it Visual mode selects, in characters.
#[derive(Debug, Default, PartialEq, Eq)]
struct Row {
    lead: String,
    text: String,
    selected: Option<Range<usize>>,
}

impl CommandLine {
    pub(crate) fn new(kind: Kind) -> Self {
        Self {
            kind,
            editor: Editor::new("", Mode::Insert).with_lines(),
            scroll: 0,
            browsing: None,
            draft: String::new(),
            browsed: String::new(),
            column: None,
            foreign: false,
        }
    }

    /// Puts `command`, from the window of the history, on the line in place of what was
    /// typed; `foreign` if it ran on another host.
    pub(crate) fn take(&mut self, command: &str, foreign: bool) {
        self.set_text(&self.recalled(command));
        self.browsing = None;
        self.foreign = foreign;
    }

    /// Shows the command before in `commands`, the history of the panel's host newest first,
    /// or the one after; after the newest, what was typed. The cursor goes to the end, so
    /// that Up goes through the lines of a command before the commands before it.
    pub(crate) fn browse(&mut self, older: bool, commands: &[&str]) {
        // The text changed since, by an edit or an undo: the command it is now, if it is one,
        // is where the history goes on from; else it is what was typed.
        if self.browsing.is_some() && self.text() != self.browsed {
            self.browsing = commands
                .iter()
                .position(|command| self.shown(command) == self.text());
        }
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
            self.draft = self.text().to_owned();
        }
        self.browsing = Some(next);
        self.set_text(&self.recalled(command));
        self.browsed = self.text().to_owned();
    }

    /// A command of the history as the line shows it.
    fn shown(&self, command: &str) -> String {
        clean(self.recalled(command).trim_end_matches(['\n', '\r']))
    }

    /// A shell command of the history as this line runs it: after `!` on the line of `:`.
    fn recalled(&self, command: &str) -> String {
        match self.kind {
            Kind::Shell => command.to_owned(),
            Kind::Noc => format!("!{command}"),
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    /// The command as typed.
    pub(crate) fn text(&self) -> &str {
        self.editor.text()
    }

    /// The mode keys are typed in.
    pub(crate) fn mode(&self) -> Mode {
        self.editor.mode()
    }

    /// The operator that waits for its motion, as vim's `showcmd` shows it.
    pub(crate) fn pending(&self) -> Option<String> {
        self.editor.pending()
    }

    /// The editor of the command, for what the line of the mode shows.
    pub(crate) fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Replaces the command with `text`, without the line breaks at its end, and puts the
    /// cursor after it.
    pub(crate) fn set_text(&mut self, text: &str) {
        let text = clean(text.trim_end_matches(['\n', '\r']));
        self.editor.replace(&text, text.chars().count());
    }

    /// Puts pasted `text` as it is, line breaks included; nothing runs.
    pub(crate) fn paste(&mut self, text: &str, registers: &mut Registers) {
        self.editor.paste(&clean(text), registers);
    }

    /// Gives up the command of Normal mode that waits, as the keymap gives up its keys.
    pub(crate) fn give_up(&mut self) {
        self.editor.give_up();
    }

    fn chars(&self) -> Vec<char> {
        self.text().chars().collect()
    }

    /// The first character of the line the cursor is on, and the end of that line.
    fn line_bounds(&self) -> (usize, usize) {
        line_bounds(&self.chars(), self.editor.cursor())
    }

    /// Takes a key, as typed without a count or a register.
    #[cfg(test)]
    pub(crate) fn handle(&mut self, input: Resolved) -> CommandEvent {
        self.handle_typed(Typed::from(input), &mut Registers::default())
    }

    /// Takes a key, with its count and register: the line's own keys, and the editor's for
    /// the rest.
    pub(crate) fn handle_typed(&mut self, typed: Typed, registers: &mut Registers) -> CommandEvent {
        self.foreign = false;
        // `j` and `k` keep the column of the first of them.
        let column = self.column.take();
        let Typed { resolved, extra } = typed;
        let action = match resolved {
            Resolved::Insert(c) => {
                self.editor.insert(c);
                return CommandEvent::None;
            }
            Resolved::Action(action) => action,
        };
        // An operator that waits takes the next key, which gives it up if it is no motion;
        // Enter gives it up and runs the command.
        if self.editor.pending().is_some() {
            if action != Action::Confirm {
                self.editor.handle(action, extra, registers);
                return CommandEvent::None;
            }
            self.editor.give_up();
        }
        let length = self.text().chars().count();
        let end = self.line_bounds().1;
        let event = match action {
            Action::Up | Action::Down => {
                let mut event = CommandEvent::None;
                let (start, _) = self.line_bounds();
                let column = column.unwrap_or(self.editor.cursor() - start);
                self.column = Some(column);
                for _ in 0..extra.times() {
                    let (start, end) = self.line_bounds();
                    if action == Action::Up && start == 0 {
                        event = CommandEvent::Older;
                        break;
                    }
                    if action == Action::Down && end == length {
                        event = CommandEvent::Newer;
                        break;
                    }
                    self.move_line(action == Action::Down, column);
                }
                if event != CommandEvent::None {
                    self.column = None;
                }
                event
            }
            Action::CommandHistory => CommandEvent::History,
            Action::OlderCommand => CommandEvent::Older,
            Action::NewerCommand => CommandEvent::Newer,
            Action::NewLine => {
                self.editor.insert('\n');
                CommandEvent::None
            }
            Action::Confirm => return self.confirm(end),
            // Ctrl+f goes to Normal mode where the cursor is, as vim's window of the command
            // line opens, rather than one character back, as Esc does.
            Action::NormalMode if self.mode() == Mode::Insert => {
                self.editor.set_mode(Mode::Normal);
                return CommandEvent::None;
            }
            // Esc after Ctrl+o goes back to Insert mode before it closes the line.
            Action::Cancel => {
                if self.editor.handle(action, extra, registers) {
                    return CommandEvent::None;
                }
                return CommandEvent::Close;
            }
            _ => {
                // A key that is not the editor's is the one command after Ctrl+o too.
                if !self.editor.handle(action, extra, registers) {
                    self.editor.give_up();
                }
                // After `$`, `j` and `k` go to the ends of the lines.
                if action == Action::TextEnd {
                    self.column = Some(usize::MAX);
                }
                return CommandEvent::None;
            }
        };
        // A key of the line's own is the one command after Ctrl+o.
        self.editor.give_up();
        event
    }

    /// Puts the cursor on the next line, or the one before, in `column` or at the end of a
    /// shorter line. From the last line or the first one, it stays.
    fn move_line(&mut self, next: bool, column: usize) {
        let chars = self.chars();
        let (start, end) = self.line_bounds();
        let (start, end) = if next {
            if end == chars.len() {
                return;
            }
            line_bounds(&chars, end + 1)
        } else {
            if start == 0 {
                return;
            }
            line_bounds(&chars, start - 1)
        };
        self.editor
            .set_cursor(start.saturating_add(column).min(end));
    }

    /// Enter: a new line after an odd number of `\` at the end of the line, as a shell
    /// continues one; otherwise the command to run, if there is one.
    fn confirm(&mut self, end: usize) -> CommandEvent {
        let chars = self.chars();
        let cursor = self.editor.cursor();
        let escapes = chars[..cursor]
            .iter()
            .rev()
            .take_while(|&&c| c == '\\')
            .count();
        if self.mode() == Mode::Insert && cursor == end && escapes % 2 == 1 {
            self.editor.insert('\n');
            return CommandEvent::None;
        }
        let text = self.text().to_owned();
        let command = match self.kind {
            Kind::Shell => text,
            Kind::Noc => match text.strip_prefix('!') {
                Some(command) => command.to_owned(),
                None if text.trim().is_empty() => return CommandEvent::Close,
                None => return CommandEvent::Unknown(text),
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
        let cursor = self.editor.cursor();
        let selection = self.editor.selection();
        let mut rows = Vec::new();
        let mut at = (0, 0);
        let mut index = 0;
        for (number, line) in self.text().split('\n').enumerate() {
            let lead = if number == 0 { prompt } else { CONTINUATION };
            let mut row = Row {
                lead: lead.to_owned(),
                ..Row::default()
            };
            let mut used = cells::width(lead);
            for c in line.chars() {
                let c = shown(c);
                let cell = c.width().unwrap_or(0);
                if used + cell > width && used > 0 {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                if index == cursor {
                    at = (rows.len(), used);
                }
                if selection
                    .as_ref()
                    .is_some_and(|selected| selected.contains(&index))
                {
                    let column = row.text.chars().count();
                    let selected = row.selected.get_or_insert(column..column);
                    selected.end = column + 1;
                }
                row.text.push(c);
                used += cell;
                index += 1;
            }
            if index == cursor {
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
            let mut spans = vec![Span::styled(shown.lead, lead)];
            match shown.selected {
                Some(selected) => {
                    let chars: Vec<char> = shown.text.chars().collect();
                    let part = |range: Range<usize>| chars[range].iter().collect::<String>();
                    spans.push(Span::raw(part(0..selected.start)));
                    spans.push(Span::styled(
                        part(selected.clone()),
                        style.add_modifier(Modifier::REVERSED),
                    ));
                    spans.push(Span::raw(part(selected.end..chars.len())));
                }
                None => spans.push(Span::raw(shown.text)),
            }
            let line = Line::from(spans);
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
    fn colon_recalls_shell_commands_after_a_bang() {
        // The history keeps shell commands, which the line of `:` runs after `!`.
        let history = ["make"];
        let mut line = CommandLine::new(Kind::Noc);
        line.browse(true, &history);
        assert_eq!(line.text(), "!make");
        assert_eq!(
            press(&mut line, Action::Confirm),
            CommandEvent::Run("make".to_owned())
        );
        let mut taken = CommandLine::new(Kind::Noc);
        taken.take("ls", false);
        assert_eq!(taken.text(), "!ls");
        let mut bang = shell("");
        bang.take("ls", false);
        assert_eq!(bang.text(), "ls", "the line of `!` runs them as they are");
    }

    #[test]
    fn only_esc_closes_the_line() {
        let mut line = shell("x");
        assert_eq!(press(&mut line, Action::Backspace), CommandEvent::None);
        assert_eq!(
            press(&mut line, Action::Backspace),
            CommandEvent::None,
            "Backspace on an empty line keeps it open"
        );
        assert_eq!(line.text(), "");
        assert_eq!(press(&mut shell("x"), Action::Cancel), CommandEvent::Close);
    }

    #[test]
    fn pasted_text_goes_in_whole_and_never_runs() {
        let mut line = shell("echo ");
        press(&mut line, Action::Left);
        line.paste("a\r\nb\rc\u{1b}[2J\td", &mut Registers::default());
        assert_eq!(line.text(), "echoa\nb\nc[2J\td ");
        assert_eq!(line.editor.cursor(), 14, "after the pasted text");
        line.set_text("make\n  && ls\n\n");
        assert_eq!(line.text(), "make\n  && ls");
        assert_eq!(line.editor.cursor(), 12);
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
    fn j_and_k_keep_their_column_and_go_to_the_ends_after_dollar() {
        let mut line = shell("abcdef\nab\nabcdef");
        press(&mut line, Action::NormalMode);
        assert_eq!(line.mode(), Mode::Normal);
        press(&mut line, Action::TextStart);
        for _ in 0..5 {
            press(&mut line, Action::Right);
        }
        assert_eq!(line.editor.cursor(), 15, "column 5 of the last line");
        press(&mut line, Action::Up);
        assert_eq!(line.editor.cursor(), 8, "the end of a shorter line");
        press(&mut line, Action::Up);
        assert_eq!(line.editor.cursor(), 5, "column 5 again, as vim's curswant");
        press(&mut line, Action::Left);
        press(&mut line, Action::Down);
        press(&mut line, Action::Down);
        assert_eq!(line.editor.cursor(), 14, "a move forgets it");
        press(&mut line, Action::Up);
        press(&mut line, Action::Up);
        press(&mut line, Action::TextEnd);
        press(&mut line, Action::Down);
        press(&mut line, Action::Down);
        assert_eq!(
            line.editor.cursor(),
            15,
            "after $, the last character of each line"
        );
    }

    #[test]
    fn ctrl_f_goes_to_normal_mode_where_the_cursor_is() {
        let mut line = shell("abc");
        press(&mut line, Action::Left);
        press(&mut line, Action::Left);
        press(&mut line, Action::NormalMode);
        assert_eq!((line.mode(), line.editor.cursor()), (Mode::Normal, 1));
    }

    #[test]
    fn an_undo_after_the_history_goes_on_from_the_command_it_brings_back() {
        let history = ["one", "two", "three"];
        let mut line = shell("");
        line.browse(true, &history);
        line.browse(true, &history);
        assert_eq!(line.text(), "two");
        press(&mut line, Action::NormalMode);
        press(&mut line, Action::Undo);
        assert_eq!(line.text(), "one");
        line.browse(true, &history);
        assert_eq!(line.text(), "two", "after one, not after two");
        // An edit makes the text what was typed.
        press(&mut line, Action::DeleteChar);
        line.browse(true, &history);
        assert_eq!(line.text(), "one");
        line.browse(false, &history);
        assert_eq!(line.text(), "tw");
    }

    #[test]
    fn keys_edit_within_the_line_of_the_cursor() {
        let mut line = shell("first line\nsecond");
        press(&mut line, Action::TextStart);
        assert_eq!(line.editor.cursor(), 11, "the start of its line");
        for _ in 0..6 {
            press(&mut line, Action::Right);
        }
        press(&mut line, Action::Up);
        assert_eq!(line.editor.cursor(), 6, "the same column above");
        press(&mut line, Action::DeleteToEnd);
        assert_eq!(line.text(), "first \nsecond");
        press(&mut line, Action::Down);
        assert_eq!(line.editor.cursor(), 13, "the end of a shorter line");
        press(&mut line, Action::Down);
        assert_eq!(line.editor.cursor(), 13, "the last line stays");
        press(&mut line, Action::DeleteToStart);
        assert_eq!(line.text(), "first \n");
        press(&mut line, Action::Backspace);
        press(&mut line, Action::Left);
        press(&mut line, Action::DeleteChar);
        assert_eq!(line.text(), "first");
        press(&mut line, Action::Up);
        assert_eq!(line.editor.cursor(), 5, "the first line stays");
        let mut words = shell("echo one\ntwo three");
        press(&mut words, Action::DeleteWord);
        assert_eq!(words.text(), "echo one\ntwo ");
        press(&mut words, Action::DeleteWord);
        press(&mut words, Action::DeleteWord);
        assert_eq!(
            words.text(),
            "echo one",
            "the line break, then the line before"
        );
    }

    #[test]
    fn an_operator_that_waits_gives_itself_up_at_a_key_of_the_line() {
        let mut registers = Registers::default();
        let typed = |action| Typed::from(Resolved::Action(action));
        let mut line = shell("make\ntest");
        press(&mut line, Action::NormalMode);
        line.handle_typed(typed(Action::DeleteOperator), &mut registers);
        let up = line.handle_typed(typed(Action::Up), &mut registers);
        assert_eq!(up, CommandEvent::None, "k gives d up, and stays");
        line.handle_typed(typed(Action::WordBackward), &mut registers);
        assert_eq!(line.text(), "make\ntest");
        // Ctrl+o, then Esc: Insert mode again, and the line stays open.
        let mut line = shell("ls");
        press(&mut line, Action::NormalOnce);
        assert_eq!(press(&mut line, Action::Cancel), CommandEvent::None);
        assert_eq!(line.mode(), Mode::Insert);
        // A key of the line's own is the one command after Ctrl+o.
        press(&mut line, Action::NormalOnce);
        press(&mut line, Action::Up);
        assert_eq!(line.mode(), Mode::Insert);
    }

    #[test]
    fn ctrl_f_goes_to_normal_mode_where_j_and_k_go_through_lines_and_the_history() {
        let mut registers = Registers::default();
        let mut line = shell("make\ntest");
        assert_eq!(line.mode(), Mode::Insert);
        press(&mut line, Action::NormalMode);
        assert_eq!(line.mode(), Mode::Normal);
        let typed = |action| Typed::from(Resolved::Action(action));
        line.handle_typed(typed(Action::WordBackward), &mut registers);
        line.handle_typed(typed(Action::DeleteOperator), &mut registers);
        line.handle_typed(typed(Action::DeleteOperator), &mut registers);
        assert_eq!(line.text(), "make");
        assert_eq!(press(&mut line, Action::Up), CommandEvent::Older);
        line.handle_typed(typed(Action::PutAfter), &mut registers);
        assert_eq!(line.text(), "make\ntest", "a line goes in as a line");
        line.handle_typed(typed(Action::DeleteOperator), &mut registers);
        assert_eq!(
            press(&mut line, Action::Cancel),
            CommandEvent::None,
            "the operator goes"
        );
        assert_eq!(press(&mut line, Action::Cancel), CommandEvent::Close);
        assert_eq!(
            press(&mut line, Action::Confirm),
            CommandEvent::Run("make\ntest".to_owned()),
            "Enter runs it in Normal mode too"
        );
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
