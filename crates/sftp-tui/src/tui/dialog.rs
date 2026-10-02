//! Modal dialogs: prompts from ssh (passwords and passphrases, host keys, confirmations, and
//! notices) and the app's own questions.

use std::fmt;

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use secrecy::SecretString;
use sftp_tui_ssh::askpass::PromptKind;
use unicode_width::UnicodeWidthChar as _;
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
pub(crate) enum Button {
    Ok,
    Cancel,
    Yes,
    No,
    Skip,
    SkipAll,
    Retry,
    Abort,
    /// Overwrite this and every later one.
    All,
    /// Keep this and every later one.
    KeepAll,
    /// Overwrite this and every later one if older.
    Older,
}

impl Button {
    fn label(self) -> String {
        match self {
            Self::Ok => fl!("dialog-ok"),
            Self::Cancel => fl!("dialog-cancel"),
            Self::Yes => fl!("dialog-yes"),
            Self::No => fl!("dialog-no"),
            Self::Skip => fl!("dialog-skip"),
            Self::SkipAll => fl!("dialog-skip-all"),
            Self::Retry => fl!("dialog-retry"),
            Self::Abort => fl!("dialog-abort"),
            Self::All => fl!("dialog-all"),
            Self::KeepAll => fl!("dialog-none"),
            Self::Older => fl!("dialog-older"),
        }
    }
}

/// Where the keys go inside a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Field,
    Check(usize),
    Button(usize),
}

/// A text field. A secret one is masked, and its text lives in memory reserved up front and
/// wiped when the field goes.
struct Field {
    text: Zeroizing<String>,
    /// In characters.
    cursor: usize,
    secret: bool,
    /// Still the text the dialog opened with: typing replaces it, as in mc.
    fresh: bool,
}

impl Field {
    fn secret() -> Self {
        Self {
            text: Zeroizing::new(String::with_capacity(SECRET_CAPACITY)),
            cursor: 0,
            secret: true,
            fresh: false,
        }
    }

    /// A plain field that opens with `text`, the cursor at its end.
    fn plain(text: &str) -> Self {
        Self {
            text: Zeroizing::new(text.to_owned()),
            cursor: text.chars().count(),
            secret: false,
            fresh: !text.is_empty(),
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
        if std::mem::take(&mut self.fresh) {
            self.text.clear();
            self.cursor = 0;
        }
        // Growing would copy the secret to new memory and leave the old one unwiped.
        if self.secret && self.text.len() + c.len_utf8() > self.text.capacity() {
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
        self.fresh = false;
        true
    }

    /// What fits in `room` cells, as shown (stars for a secret), from where the cursor stays
    /// on screen, and the cursor's column in it.
    fn visible(&self, room: usize) -> (String, usize) {
        let chars: Vec<char> = if self.secret {
            vec!['*'; self.chars()]
        } else {
            self.text.chars().collect()
        };
        let width = |c: char| c.width().unwrap_or(0);
        // Back from the cursor while the text before it and the cursor itself fit.
        let mut first = self.cursor;
        let mut column = 0;
        while first > 0 && column + width(chars[first - 1]) < room {
            first -= 1;
            column += width(chars[first]);
        }
        let mut used = 0;
        let shown: String = chars[first..]
            .iter()
            .take_while(|&&c| {
                used += width(c);
                used <= room
            })
            .collect();
        (cells::sanitize(shown.as_bytes()), column)
    }
}

impl fmt::Debug for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Field");
        if !self.secret {
            debug.field("text", &self.text.as_str());
        }
        debug.finish_non_exhaustive()
    }
}

/// The colors of a dialog box.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Colors {
    body: Style,
    title: Style,
    button: Style,
    focused: Style,
}

impl Colors {
    /// The style of the button with the focus.
    pub(crate) fn focused_style(self) -> Style {
        self.focused
    }

    /// A dialog's colors, or an error's: mc draws errors and warnings red.
    pub(crate) fn of(theme: &Theme, error: bool) -> Self {
        if error {
            Self {
                body: theme.error_dialog,
                title: theme.error_title,
                button: theme.error_dialog,
                focused: theme.error_button_focused,
            }
        } else {
            Self {
                body: theme.dialog,
                title: theme.dialog_title,
                button: theme.dialog_button,
                focused: theme.dialog_button_focused,
            }
        }
    }
}

/// A check box.
#[derive(Debug)]
struct Check {
    label: String,
    on: bool,
}

/// What a key did to a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogEvent {
    /// The dialog stays open.
    Pending,
    /// A button was pressed: Enter in the field or on a check box presses the default one.
    Pressed(Button),
    /// Esc or F10.
    Cancelled,
}

/// A modal dialog: a message, a text field, check boxes, and buttons, each of them optional
/// but the buttons. Whoever opens it reads the field and the check boxes once it closes.
#[derive(Debug)]
pub(crate) struct Dialog {
    title: String,
    message: String,
    field: Option<Field>,
    checks: Vec<Check>,
    buttons: Vec<Button>,
    /// The button that Enter in the field activates, and that starts with the focus otherwise;
    /// drawn as `[< … >]`, as in mc.
    default: usize,
    focus: Focus,
    /// Widest the dialog gets, in cells, borders included.
    width: u16,
    /// Drawn in the colors of errors.
    error: bool,
}

impl Dialog {
    /// A dialog for a prompt of `kind`: a masked field with OK and Cancel for secrets, Yes and
    /// No (with the focus) for questions. `context` is the host alias.
    pub(crate) fn prompt(context: &str, message: &str, kind: PromptKind) -> Self {
        match kind {
            PromptKind::Secret => Self {
                field: Some(Field::secret()),
                focus: Focus::Field,
                ..Self::new(context, message, vec![Button::Ok, Button::Cancel])
            },
            PromptKind::HostKey | PromptKind::Confirm => Self {
                default: 1,
                focus: Focus::Button(1),
                ..Self::new(context, message, vec![Button::Yes, Button::No])
            },
        }
    }

    /// Information from ssh that needs no answer, such as a request to touch a security key.
    pub(crate) fn notice(context: &str, message: &str) -> Self {
        Self::new(context, message, vec![Button::Ok])
    }

    /// A question with `buttons`, of which `default` has the focus, in the colors of errors if
    /// `error`, as mc asks before deleting and after a failure.
    pub(crate) fn question(
        title: &str,
        message: &str,
        buttons: Vec<Button>,
        default: usize,
        error: bool,
    ) -> Self {
        Self {
            default,
            focus: Focus::Button(default),
            error,
            ..Self::new(title, message, buttons)
        }
    }

    /// Something that went wrong, with OK, in the colors of errors.
    pub(crate) fn error(title: &str, message: &str) -> Self {
        Self {
            error: true,
            ..Self::new(title, message, vec![Button::Ok])
        }
    }

    /// `message`, a text field that opens with `text`, check boxes with their labels and
    /// states, and OK and Cancel, `width` cells wide.
    pub(crate) fn form(
        title: &str,
        message: &str,
        text: &str,
        checks: &[(String, bool)],
        width: u16,
    ) -> Self {
        let checks = checks
            .iter()
            .map(|(label, on)| Check {
                label: label.clone(),
                on: *on,
            })
            .collect();
        Self {
            field: Some(Field::plain(text)),
            checks,
            focus: Focus::Field,
            width,
            ..Self::new(title, message, vec![Button::Ok, Button::Cancel])
        }
    }

    fn new(title: &str, message: &str, buttons: Vec<Button>) -> Self {
        Self {
            title: cells::sanitize(title.as_bytes()),
            message: message.trim_end().to_owned(),
            field: None,
            checks: Vec::new(),
            buttons,
            default: 0,
            focus: Focus::Button(0),
            width: MAX_WIDTH,
            error: false,
        }
    }

    /// The answer for ssh after `event`: the secret for OK on a secret prompt, `yes` for Yes,
    /// and `None` to decline.
    pub(crate) fn answer(&self, event: DialogEvent) -> Option<SecretString> {
        match (event, &self.field) {
            (DialogEvent::Pressed(Button::Ok), Some(field)) => {
                // From `&str`: an exact copy, where `String` would be shrunk into new memory.
                Some(SecretString::from(field.text.as_str()))
            }
            (DialogEvent::Pressed(Button::Yes), _) => Some(SecretString::from("yes")),
            _ => None,
        }
    }

    /// The text of a plain field.
    pub(crate) fn text(&self) -> &str {
        match &self.field {
            Some(field) if !field.secret => &field.text,
            _ => "",
        }
    }

    /// Whether check box `index` is checked.
    pub(crate) fn checked(&self, index: usize) -> bool {
        self.checks.get(index).is_some_and(|check| check.on)
    }

    /// The keymap context for the next key: `DialogInput` while the text field has the focus.
    pub(crate) fn context(&self) -> Context {
        match self.focus {
            Focus::Field => Context::DialogInput,
            Focus::Check(_) | Focus::Button(_) => Context::Dialog,
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
            Action::Confirm => match self.focus {
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::Field | Focus::Check(_) => DialogEvent::Pressed(self.buttons[self.default]),
            },
            Action::Toggle => match self.focus {
                Focus::Check(index) => {
                    self.checks[index].on = !self.checks[index].on;
                    DialogEvent::Pending
                }
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::Field => DialogEvent::Pending,
            },
            Action::Cancel => DialogEvent::Cancelled,
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

    /// Moves the focus through the field, the check boxes, and the buttons, round.
    fn move_focus(&mut self, forward: bool) {
        let stops: Vec<Focus> = self
            .field
            .iter()
            .map(|_| Focus::Field)
            .chain((0..self.checks.len()).map(Focus::Check))
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
        let width = self.width.min(area.width.saturating_sub(4)).max(20);
        let text_width = usize::from(width.saturating_sub(4));
        let lines = cells::wrap(&self.message, text_width);
        let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
        // Borders, the message, the field, the check boxes, a blank line, the buttons.
        let height = rows(lines.len()) + u16::from(self.field.is_some()) + rows(self.checks.len());
        let colors = Colors::of(theme, self.error);
        let size = (width, height + 4);
        let inner = draw_box(frame, area, size, &self.title, colors, theme.shadow);
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
            let room = usize::from(inner.width).max(1);
            let (text, column) = field.visible(room);
            let style = if field.fresh {
                theme.dialog_input_fresh
            } else {
                theme.dialog_input
            };
            let text = cells::fit(&text, room, cells::Align::Left);
            frame.render_widget(Line::styled(text, style), row(index));
            if self.focus == Focus::Field {
                let column = u16::try_from(column).unwrap_or(0);
                frame.set_cursor_position(Position::new(inner.x + column, inner.y + index));
            }
            index += 1;
        }
        for (number, check) in self.checks.iter().enumerate() {
            if index >= inner.height {
                return;
            }
            let mark = if check.on { 'x' } else { ' ' };
            let text = format!("[{mark}] {}", check.label);
            let style = if self.focus == Focus::Check(number) {
                colors.focused
            } else {
                colors.body
            };
            frame.render_widget(Line::from(Span::styled(text, style)), row(index));
            index += 1;
        }
        if index + 1 < inner.height {
            let labels: Vec<String> = self.buttons.iter().map(|button| button.label()).collect();
            let focus = match self.focus {
                Focus::Button(index) => Some(index),
                Focus::Field | Focus::Check(_) => None,
            };
            let line = button_line(&labels, self.default, focus, colors);
            frame.render_widget(line, row(index + 1));
        }
    }
}

/// Buttons, centered, as mc draws them: the default one in `[< >]`, the one with the focus in
/// its color.
pub(crate) fn button_line(
    labels: &[String],
    default: usize,
    focus: Option<usize>,
    colors: Colors,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        let text = if index == default {
            format!("[< {label} >]")
        } else {
            format!("[ {label} ]")
        };
        let style = if focus == Some(index) {
            colors.focused
        } else {
            colors.button
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans).centered()
}

/// Draws an empty dialog box of `size` centered in `area`, with its title and mc's `shadow`,
/// and returns the room inside, one column in from the frame on either side.
pub(crate) fn draw_box(
    frame: &mut Frame<'_>,
    area: Rect,
    (width, height): (u16, u16),
    title: &str,
    colors: Colors,
    shadow: Option<Style>,
) -> Rect {
    let (width, height) = (width.min(area.width), height.min(area.height));
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let outer = Rect::new(x, y, width, height);
    frame.render_widget(Clear, outer);
    if let Some(shadow) = shadow {
        // Two columns to the right and a row below, as mc draws it.
        let right = Rect::new(outer.right(), outer.y + 1, 2, outer.height);
        let below = Rect::new(outer.x + 2, outer.bottom(), outer.width, 1);
        for rect in [right, below] {
            frame
                .buffer_mut()
                .set_style(rect.intersection(area), shadow);
        }
    }
    let title = Line::styled(format!(" {title} "), colors.title);
    let block = Block::bordered().title(title).style(colors.body);
    let inner = block.inner(outer).inner(ratatui::layout::Margin::new(1, 0));
    frame.render_widget(block, outer);
    inner
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

    /// Presses Enter and returns what ssh would get.
    fn confirm(dialog: &mut Dialog) -> Option<String> {
        let event = dialog.handle(action(Action::Confirm));
        assert_ne!(event, DialogEvent::Pending);
        dialog
            .answer(event)
            .map(|answer| answer.expose_secret().to_owned())
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
        Dialog::prompt("web", "deploy@10.0.0.5's password: ", PromptKind::Secret)
    }

    #[test]
    fn a_secret_prompt_answers_what_was_typed() {
        let mut dialog = secret();
        assert_eq!(dialog.context(), Context::DialogInput);
        typed(&mut dialog, "pässwd");
        assert_eq!(dialog.text(), "", "a secret is not text");
        assert_eq!(confirm(&mut dialog).as_deref(), Some("pässwd"));
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
        assert_eq!(confirm(&mut dialog).as_deref(), Some("Xbce"));

        let mut dialog = secret();
        typed(&mut dialog, "abcdef");
        dialog.handle(action(Action::Left));
        dialog.handle(action(Action::DeleteToEnd));
        dialog.handle(action(Action::Left));
        dialog.handle(action(Action::DeleteToStart));
        assert_eq!(confirm(&mut dialog).as_deref(), Some("e"));
    }

    #[test]
    fn the_secret_field_never_grows_its_memory() {
        let mut dialog = secret();
        typed(&mut dialog, &"x".repeat(SECRET_CAPACITY + 10));
        assert_eq!(confirm(&mut dialog).unwrap().len(), SECRET_CAPACITY);
    }

    #[test]
    fn buttons_take_the_focus_in_turn() {
        let mut dialog = secret();
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::Dialog);
        typed(&mut dialog, "ignored");
        dialog.handle(action(Action::NextField));
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pressed(Button::Cancel)
        );
        assert!(
            dialog
                .answer(DialogEvent::Pressed(Button::Cancel))
                .is_none()
        );
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::DialogInput, "round to the field");
        assert_eq!(confirm(&mut dialog).as_deref(), Some(""));
        assert_eq!(
            dialog.handle(action(Action::Cancel)),
            DialogEvent::Cancelled
        );
        assert!(dialog.answer(DialogEvent::Cancelled).is_none());
    }

    #[test]
    fn questions_default_to_no() {
        let mut dialog = Dialog::prompt("web", "Continue? (yes/no)", PromptKind::HostKey);
        assert_eq!(dialog.context(), Context::Dialog);
        assert_eq!(confirm(&mut dialog), None);
        dialog.handle(action(Action::Left));
        assert_eq!(confirm(&mut dialog).as_deref(), Some("yes"));

        let mut notice = Dialog::notice("web", "Confirm user presence for key");
        assert_eq!(
            notice.handle(action(Action::Cancel)),
            DialogEvent::Cancelled
        );
        assert_eq!(confirm(&mut notice), None);
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
        let dialog = Dialog::prompt("web", message, PromptKind::HostKey);
        insta::assert_snapshot!(draw(&dialog, 60, 14).backend());
    }

    fn form() -> Dialog {
        let checks = [
            ("Files only".to_owned(), false),
            ("Case sensitive".to_owned(), true),
        ];
        Dialog::form("Select", "", "*", &checks, 50)
    }

    #[test]
    fn typing_replaces_the_text_a_form_opens_with() {
        let mut dialog = form();
        assert_eq!(dialog.context(), Context::DialogInput);
        assert_eq!(dialog.text(), "*");
        typed(&mut dialog, "*.txt");
        assert_eq!(dialog.text(), "*.txt");
        typed(&mut dialog, " x");
        assert_eq!(dialog.text(), "*.txt x", "only the first key replaces it");

        let mut dialog = form();
        dialog.handle(action(Action::Home));
        typed(&mut dialog, "a");
        assert_eq!(dialog.text(), "a*", "an edit keeps it");
    }

    #[test]
    fn check_boxes_switch_and_enter_presses_the_default_button() {
        let mut dialog = form();
        assert_eq!(dialog.handle(action(Action::Toggle)), DialogEvent::Pending);
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::Dialog);
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Toggle));
        assert!(dialog.checked(0));
        assert!(!dialog.checked(1));
        assert!(!dialog.checked(2), "no such box");
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pressed(Button::Ok)
        );
        dialog.handle(action(Action::NextField));
        dialog.handle(action(Action::NextField));
        assert_eq!(
            dialog.handle(action(Action::Toggle)),
            DialogEvent::Pressed(Button::Cancel),
            "Space presses a button"
        );
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::DialogInput, "round to the field");
        assert_eq!(dialog.text(), "*");
    }

    #[test]
    fn a_long_field_shows_the_part_around_its_cursor() {
        let mut field = Field::plain("abcdefghij");
        assert_eq!(field.visible(5), ("ghij".to_owned(), 4));
        field.cursor = 0;
        assert_eq!(field.visible(5), ("abcde".to_owned(), 0));
        let wide = Field::plain("文件文件");
        assert_eq!(wide.visible(5), ("文件".to_owned(), 4));
        let mut secret = Field::secret();
        for c in "hunter2".chars() {
            secret.insert(c);
        }
        assert_eq!(secret.visible(4), ("***".to_owned(), 3));
    }

    #[test]
    fn draws_a_form_with_its_check_boxes() {
        let mut dialog = form();
        typed(&mut dialog, "*.md");
        dialog.handle(action(Action::NextField));
        let terminal = draw(&dialog, 60, 10);
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn errors_are_red_in_mc_classic() {
        use ratatui::style::Color;

        let mut dialog = Dialog::error("Error", "Cannot create directory /x: already exists");
        let terminal = draw_themed(&dialog, 60, 8, &Theme::mc_classic());
        let buffer = terminal.backend().buffer();
        let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        // The dialog spans columns 2 … 57 and rows 1 … 6.
        assert_eq!(colors(2, 1), (Color::White, Color::Red), "frame");
        assert_eq!(colors(4, 1), (Color::LightYellow, Color::Red), "title");
        assert_eq!(colors(4, 2), (Color::White, Color::Red), "message");
        let (x, y) = (0..60)
            .flat_map(|x| (0..8).map(move |y| (x, y)))
            .find(|&(x, y)| buffer[(x, y)].symbol() == "[")
            .unwrap();
        assert_eq!(
            colors(x, y),
            (Color::Black, Color::Gray),
            "the focused button"
        );
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pressed(Button::Ok)
        );
    }

    #[test]
    fn mc_classic_draws_the_opening_text_dimmed() {
        use ratatui::style::Color;

        let terminal = draw_themed(&form(), 60, 10, &Theme::mc_classic());
        let buffer = terminal.backend().buffer();
        // The dialog spans columns 5 … 54 and rows 1 … 7; the field is on row 2.
        assert_eq!(
            (buffer[(7, 2)].fg, buffer[(7, 2)].bg),
            (Color::DarkGray, Color::Cyan)
        );
    }
}
