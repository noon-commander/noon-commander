//! Modal dialogs for prompts from ssh: passwords and passphrases, host keys, confirmations,
//! and notices.

use std::fmt;

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use secrecy::SecretString;
use sftp_tui_ssh::askpass::PromptKind;
use zeroize::Zeroizing;

use super::cells;
use super::keymap::{Action, Context, Resolved};
use super::theme::Theme;
use crate::i18n::fl;

/// Bytes a secret may have. Reserved up front, so typing never moves it in memory and leaves
/// no copy behind.
const SECRET_CAPACITY: usize = 1024;
/// Widest a dialog gets, in cells, borders included.
const MAX_WIDTH: u16 = 76;

/// Sends the answer to a prompt back to ssh; `None` declines it.
pub(crate) struct Reply(Box<dyn FnOnce(Option<SecretString>) + Send>);

impl Reply {
    pub(crate) fn new(send: impl FnOnce(Option<SecretString>) + Send + 'static) -> Self {
        Self(Box::new(send))
    }

    pub(crate) fn send(self, answer: Option<SecretString>) {
        (self.0)(answer);
    }
}

impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Reply")
    }
}

/// A question from ssh.
#[derive(Debug)]
pub(crate) struct Ask {
    /// Matches a later report that ssh stopped waiting.
    pub(crate) id: u64,
    /// The host alias.
    pub(crate) context: String,
    pub(crate) message: String,
    pub(crate) kind: PromptKind,
    pub(crate) reply: Reply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Ok,
    Cancel,
    Yes,
    No,
}

impl Button {
    fn label(self) -> String {
        match self {
            Self::Ok => fl!("dialog-ok"),
            Self::Cancel => fl!("dialog-cancel"),
            Self::Yes => fl!("dialog-yes"),
            Self::No => fl!("dialog-no"),
        }
    }
}

/// Where the keys go inside a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Field,
    Button(usize),
}

/// A masked text field. The text lives in memory that is wiped when the field goes.
struct SecretField {
    text: Zeroizing<String>,
    /// In characters.
    cursor: usize,
}

impl SecretField {
    fn new() -> Self {
        Self {
            text: Zeroizing::new(String::with_capacity(SECRET_CAPACITY)),
            cursor: 0,
        }
    }

    fn chars(&self) -> usize {
        self.text.chars().count()
    }

    /// The byte offset of character `index`.
    fn offset(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(offset, _)| offset)
    }

    fn insert(&mut self, c: char) {
        // Growing would copy the text to new memory and leave the old one unwiped.
        if self.text.len() + c.len_utf8() > self.text.capacity() {
            return;
        }
        let offset = self.offset(self.cursor);
        self.text.insert(offset, c);
        self.cursor += 1;
    }

    /// Removes the characters in `start..end`.
    fn remove(&mut self, start: usize, end: usize) {
        let (start, end) = (self.offset(start), self.offset(end));
        self.text.replace_range(start..end, "");
    }

    /// Edits the text; `false` if `action` is not an edit.
    fn edit(&mut self, action: Action) -> bool {
        let chars = self.chars();
        match action {
            Action::Left => self.cursor = self.cursor.saturating_sub(1),
            Action::Right => self.cursor = (self.cursor + 1).min(chars),
            Action::Home => self.cursor = 0,
            Action::End => self.cursor = chars,
            Action::Backspace if self.cursor > 0 => {
                self.remove(self.cursor - 1, self.cursor);
                self.cursor -= 1;
            }
            Action::Delete if self.cursor < chars => self.remove(self.cursor, self.cursor + 1),
            Action::DeleteToStart => {
                self.remove(0, self.cursor);
                self.cursor = 0;
            }
            Action::DeleteToEnd => self.remove(self.cursor, chars),
            Action::Backspace | Action::Delete => {}
            _ => return false,
        }
        true
    }

    fn secret(&self) -> SecretString {
        // From `&str`: an exact copy, where `String` would be shrunk into new memory.
        SecretString::from(self.text.as_str())
    }
}

impl fmt::Debug for SecretField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretField").finish_non_exhaustive()
    }
}

/// What a key did to a dialog.
#[derive(Debug)]
pub(crate) enum DialogEvent {
    /// The dialog stays open.
    Pending,
    /// Accepted: the text of a secret prompt, or `yes`.
    Answer(SecretString),
    /// Declined: Cancel, No, or Esc on a prompt.
    Decline,
    /// Dismissed: OK on a notice.
    Close,
}

/// A modal dialog.
#[derive(Debug)]
pub(crate) struct Dialog {
    id: u64,
    title: String,
    message: String,
    field: Option<SecretField>,
    buttons: Vec<Button>,
    /// The button that Enter in the field activates, and that starts with the focus otherwise;
    /// drawn as `[< … >]`, as in mc.
    default: usize,
    focus: Focus,
}

impl Dialog {
    /// A dialog for a prompt of `kind`: a masked field with OK and Cancel for secrets, Yes and
    /// No (with the focus) for questions. `context` is the host alias.
    pub(crate) fn prompt(id: u64, context: &str, message: &str, kind: PromptKind) -> Self {
        match kind {
            PromptKind::Secret => Self {
                field: Some(SecretField::new()),
                focus: Focus::Field,
                ..Self::new(id, context, message, vec![Button::Ok, Button::Cancel])
            },
            PromptKind::HostKey | PromptKind::Confirm => Self {
                default: 1,
                focus: Focus::Button(1),
                ..Self::new(id, context, message, vec![Button::Yes, Button::No])
            },
        }
    }

    /// Information from ssh that needs no answer, such as a request to touch a security key.
    pub(crate) fn notice(id: u64, context: &str, message: &str) -> Self {
        Self::new(id, context, message, vec![Button::Ok])
    }

    fn new(id: u64, context: &str, message: &str, buttons: Vec<Button>) -> Self {
        Self {
            id,
            title: cells::sanitize(context.as_bytes()),
            message: message.trim_end().to_owned(),
            field: None,
            buttons,
            default: 0,
            focus: Focus::Button(0),
        }
    }

    /// The askpass id of the prompt or notice.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// The keymap context for the next key: `DialogInput` while the text field has the focus.
    pub(crate) fn context(&self) -> Context {
        match self.focus {
            Focus::Field => Context::DialogInput,
            Focus::Button(_) => Context::Dialog,
        }
    }

    /// Handles an action or a typed character.
    pub(crate) fn handle(&mut self, input: Resolved) -> DialogEvent {
        let field = match (self.focus, &mut self.field) {
            (Focus::Field, Some(field)) => Some(field),
            _ => None,
        };
        let action = match (input, field) {
            (Resolved::Insert(c), Some(field)) => {
                field.insert(c);
                return DialogEvent::Pending;
            }
            (Resolved::Insert(_), None) => return DialogEvent::Pending,
            (Resolved::Action(action), Some(field)) => {
                if field.edit(action) {
                    return DialogEvent::Pending;
                }
                action
            }
            (Resolved::Action(action), None) => action,
        };
        match action {
            Action::Confirm => self.activate(),
            Action::Cancel if self.buttons == [Button::Ok] => DialogEvent::Close,
            Action::Cancel => DialogEvent::Decline,
            Action::NextField | Action::Right | Action::Down => {
                self.move_focus(true);
                DialogEvent::Pending
            }
            Action::PrevField | Action::Left | Action::Up => {
                self.move_focus(false);
                DialogEvent::Pending
            }
            _ => DialogEvent::Pending,
        }
    }

    /// What Enter does where the focus is.
    fn activate(&self) -> DialogEvent {
        let index = match self.focus {
            Focus::Field => self.default,
            Focus::Button(index) => index,
        };
        let button = self.buttons[index];
        match (button, &self.field) {
            (Button::Ok, Some(field)) => DialogEvent::Answer(field.secret()),
            (Button::Ok, None) => DialogEvent::Close,
            (Button::Yes, _) => DialogEvent::Answer(SecretString::from("yes")),
            (Button::Cancel | Button::No, _) => DialogEvent::Decline,
        }
    }

    /// Moves the focus through the field and the buttons, round.
    fn move_focus(&mut self, forward: bool) {
        let stops: Vec<Focus> = self
            .field
            .iter()
            .map(|_| Focus::Field)
            .chain((0..self.buttons.len()).map(Focus::Button))
            .collect();
        let current = stops
            .iter()
            .position(|stop| *stop == self.focus)
            .unwrap_or(0);
        let step = if forward { 1 } else { stops.len() - 1 };
        self.focus = stops[(current + step) % stops.len()];
    }

    /// Draws the dialog centered in `area`, with the terminal cursor in the text field.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let width = MAX_WIDTH.min(area.width.saturating_sub(4)).max(20);
        let text_width = usize::from(width.saturating_sub(4));
        let lines = cells::wrap(&self.message, text_width);
        let field_rows = u16::from(self.field.is_some());
        let lines_rows = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        // Borders, the message, the field, a blank line, the buttons.
        let height = (lines_rows + field_rows + 4).min(area.height);
        let x = area.x + area.width.saturating_sub(width) / 2;
        let y = area.y + area.height.saturating_sub(height) / 2;
        let outer = Rect::new(x, y, width.min(area.width), height);
        frame.render_widget(Clear, outer);
        if let Some(shadow) = theme.shadow {
            // Two columns to the right and a row below, as mc draws it.
            let right = Rect::new(outer.right(), outer.y + 1, 2, outer.height);
            let below = Rect::new(outer.x + 2, outer.bottom(), outer.width, 1);
            for rect in [right, below] {
                frame
                    .buffer_mut()
                    .set_style(rect.intersection(area), shadow);
            }
        }
        let title = Line::styled(format!(" {} ", self.title), theme.dialog_title);
        let block = Block::bordered().title(title).style(theme.dialog);
        let inner = block.inner(outer).inner(ratatui::layout::Margin::new(1, 0));
        frame.render_widget(block, outer);
        let row = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let mut index = 0;
        for line in &lines {
            if index >= inner.height {
                return;
            }
            frame.render_widget(Line::raw(line.as_str()), row(index));
            index += 1;
        }
        if let Some(field) = &self.field
            && index < inner.height
        {
            // A bar of stars; the visible part follows the cursor.
            let room = usize::from(inner.width).max(1);
            let first = field.cursor.saturating_sub(room - 1);
            let shown = field.chars().saturating_sub(first).min(room);
            let text = format!("{}{}", "*".repeat(shown), " ".repeat(room - shown));
            frame.render_widget(Line::styled(text, theme.dialog_input), row(index));
            if self.focus == Focus::Field {
                let column = u16::try_from(field.cursor - first).unwrap_or(0);
                frame.set_cursor_position(Position::new(inner.x + column, inner.y + index));
            }
            index += 1;
        }
        if index + 1 < inner.height {
            frame.render_widget(self.button_line(theme), row(index + 1));
        }
    }

    /// The buttons, centered.
    fn button_line(&self, theme: &Theme) -> Line<'static> {
        let mut spans = Vec::new();
        for (index, button) in self.buttons.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw(" "));
            }
            let text = if index == self.default {
                format!("[< {} >]", button.label())
            } else {
                format!("[ {} ]", button.label())
            };
            let style = if self.focus == Focus::Button(index) {
                theme.dialog_button_focused
            } else {
                theme.dialog_button
            };
            spans.push(Span::styled(text, style));
        }
        Line::from(spans).centered()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use secrecy::ExposeSecret as _;

    use super::*;

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    fn typed(dialog: &mut Dialog, text: &str) {
        for c in text.chars() {
            assert!(matches!(
                dialog.handle(Resolved::Insert(c)),
                DialogEvent::Pending
            ));
        }
    }

    fn answer(event: DialogEvent) -> String {
        match event {
            DialogEvent::Answer(text) => text.expose_secret().to_owned(),
            other => panic!("expected an answer, got {other:?}"),
        }
    }

    fn draw(dialog: &Dialog, width: u16, height: u16) -> Terminal<TestBackend> {
        draw_themed(dialog, width, height, &Theme::terminal())
    }

    fn draw_themed(
        dialog: &Dialog,
        width: u16,
        height: u16,
        theme: &Theme,
    ) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| dialog.render(frame, frame.area(), theme))
            .unwrap();
        terminal
    }

    fn secret() -> Dialog {
        Dialog::prompt(7, "web", "deploy@10.0.0.5's password: ", PromptKind::Secret)
    }

    #[test]
    fn a_secret_prompt_answers_what_was_typed() {
        let mut dialog = secret();
        assert_eq!(dialog.id(), 7);
        assert_eq!(dialog.context(), Context::DialogInput);
        typed(&mut dialog, "pässwd");
        assert_eq!(answer(dialog.handle(action(Action::Confirm))), "pässwd");
    }

    #[test]
    fn the_secret_field_edits_around_its_cursor() {
        let mut dialog = secret();
        typed(&mut dialog, "abcdef");
        for edit in [Action::Left, Action::Left, Action::Backspace, Action::Home] {
            dialog.handle(action(edit));
        }
        typed(&mut dialog, "X");
        dialog.handle(action(Action::Delete));
        dialog.handle(action(Action::End));
        dialog.handle(action(Action::Backspace));
        assert_eq!(answer(dialog.handle(action(Action::Confirm))), "Xbce");

        let mut dialog = secret();
        typed(&mut dialog, "abcdef");
        dialog.handle(action(Action::Left));
        dialog.handle(action(Action::DeleteToEnd));
        dialog.handle(action(Action::Left));
        dialog.handle(action(Action::DeleteToStart));
        assert_eq!(answer(dialog.handle(action(Action::Confirm))), "e");
    }

    #[test]
    fn the_secret_field_never_grows_its_memory() {
        let mut dialog = secret();
        typed(&mut dialog, &"x".repeat(SECRET_CAPACITY + 10));
        assert_eq!(
            answer(dialog.handle(action(Action::Confirm))).len(),
            SECRET_CAPACITY
        );
    }

    #[test]
    fn buttons_take_the_focus_in_turn() {
        let mut dialog = secret();
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::Dialog);
        typed(&mut dialog, "ignored");
        dialog.handle(action(Action::NextField));
        assert!(matches!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Decline
        ));
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::DialogInput, "round to the field");
        assert_eq!(answer(dialog.handle(action(Action::Confirm))), "");
        assert!(matches!(
            dialog.handle(action(Action::Cancel)),
            DialogEvent::Decline
        ));
    }

    #[test]
    fn questions_default_to_no() {
        let mut dialog = Dialog::prompt(1, "web", "Continue? (yes/no)", PromptKind::HostKey);
        assert_eq!(dialog.context(), Context::Dialog);
        assert!(matches!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Decline
        ));
        dialog.handle(action(Action::Left));
        assert_eq!(answer(dialog.handle(action(Action::Confirm))), "yes");

        let mut notice = Dialog::notice(2, "web", "Confirm user presence for key");
        assert!(matches!(
            notice.handle(action(Action::Cancel)),
            DialogEvent::Close
        ));
        assert!(matches!(
            notice.handle(action(Action::Confirm)),
            DialogEvent::Close
        ));
    }

    #[test]
    fn draws_a_secret_prompt_with_the_cursor_in_the_field() {
        let mut dialog = secret();
        typed(&mut dialog, "hunter2");
        let mut terminal = draw(&dialog, 50, 9);
        insta::assert_snapshot!(terminal.backend());
        // After the seven stars: the dialog starts at column 2, its field at column 4.
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            Position::new(11, 3)
        );
    }

    #[test]
    fn mc_classic_draws_gray_dialogs_with_a_shadow() {
        use ratatui::style::Color;

        let dialog = secret();
        let terminal = draw_themed(&dialog, 50, 9, &Theme::mc_classic());
        let buffer = terminal.backend().buffer();
        let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        // The dialog spans columns 2 … 47 and rows 1 … 6.
        assert_eq!(colors(2, 1), (Color::Black, Color::Gray), "frame");
        assert_eq!(colors(4, 1), (Color::Blue, Color::Gray), "title");
        assert_eq!(colors(4, 3), (Color::Black, Color::Cyan), "field");
        assert_eq!(
            colors(48, 2),
            (Color::DarkGray, Color::Black),
            "shadow to the right"
        );
        assert_eq!(
            colors(10, 7),
            (Color::DarkGray, Color::Black),
            "shadow below"
        );
        assert_eq!(
            colors(2, 7).1,
            Color::Reset,
            "the shadow starts two columns in"
        );
    }

    #[test]
    fn draws_a_host_key_question_wrapped_and_terminal_safe() {
        let message = "The authenticity of host 'web (10.0.0.5)' can't be established.\n\
            ED25519 key fingerprint is SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU.\n\
            This key is not known by any other names.\x1b[2J\n\
            Are you sure you want to continue connecting (yes/no/[fingerprint])? ";
        let dialog = Dialog::prompt(1, "web", message, PromptKind::HostKey);
        insta::assert_snapshot!(draw(&dialog, 60, 14).backend());
    }
}
