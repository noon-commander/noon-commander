//! Modal dialogs: prompts from ssh (passwords and passphrases, host keys, confirmations, and
//! notices) and the app's own questions.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use noc_ssh::askpass::PromptKind;
use ratatui::Frame;
use ratatui::layout::{Margin, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use secrecy::SecretString;
use unicode_width::UnicodeWidthChar as _;

use super::cells;
use super::edit::{Editor, Registers};
use super::keymap::{Action, Context, Extra, Mode, Resolved, Typed};
use super::mouse::{Pointer, Press};
use super::theme::Theme;
use crate::i18n::fl;

/// Bytes a secret may have. Reserved up front, so typing never moves it in memory and leaves
/// no copy behind.
const SECRET_CAPACITY: usize = 1024;
/// Widest a dialog gets, in cells, borders included.
const MAX_WIDTH: u16 = 76;
/// Cells between a dialog's frame and its edge, as mc leaves them.
const MARGIN: u16 = 1;

/// The id of the last dialog made.
static LAST_ID: AtomicU64 = AtomicU64::new(0);

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
    /// Take what the panel shows.
    UseCurrent,
}

impl Button {
    pub(crate) fn label(self) -> String {
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
            Self::UseCurrent => fl!("dialog-use-current"),
        }
    }
}

/// Where the keys go inside a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Choice(usize),
    Field(usize),
    Check(usize),
    Button(usize),
}

/// A text field, in vim's modes: a plain one opens in Insert mode, with the cursor at the end
/// of its text. A secret one stays in Insert mode, is masked, and lives in memory reserved up
/// front and wiped when the field goes.
pub(crate) struct Field {
    editor: Editor,
}

impl Field {
    fn secret() -> Self {
        Self {
            editor: Editor::secret(SECRET_CAPACITY),
        }
    }

    /// A plain field that opens in Insert mode with `text`, the cursor at its end.
    pub(crate) fn plain(text: &str) -> Self {
        Self {
            editor: Editor::new(text, Mode::Insert),
        }
    }

    /// A plain field that opens in Insert mode with `text`, the cursor before character
    /// `cursor`.
    pub(crate) fn at(text: &str, cursor: usize) -> Self {
        let mut field = Self::plain(text);
        field.editor.set_cursor(cursor);
        field
    }

    /// A plain field that opens in Normal mode with `text`.
    pub(crate) fn normal(text: &str) -> Self {
        Self {
            editor: Editor::new(text, Mode::Normal),
        }
    }

    /// The text, as typed.
    pub(crate) fn text(&self) -> &str {
        self.editor.text()
    }

    /// The mode the field is in.
    pub(crate) fn mode(&self) -> Mode {
        self.editor.mode()
    }

    /// Insert mode, or Normal mode, as the focus comes to the field; a secret stays in Insert
    /// mode.
    pub(crate) fn set_mode(&mut self, mode: Mode) {
        self.editor.set_mode(mode);
    }

    /// The operator that waits for its motion, as vim's `showcmd` shows it.
    pub(crate) fn pending(&self) -> Option<String> {
        self.editor.pending()
    }

    /// Whether Normal mode runs one command, after Ctrl+o, then goes back to Insert mode.
    pub(crate) fn once(&self) -> bool {
        self.editor.once()
    }

    /// The editor of the text, for what the line of the mode shows.
    pub(crate) fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Types `c` at the cursor.
    pub(crate) fn insert(&mut self, c: char) {
        self.editor.insert(c);
    }

    /// Puts pasted `text` as one change, in any mode, without its control characters; in
    /// Visual mode, what it replaces goes to the unnamed register of `registers`.
    pub(crate) fn paste(&mut self, text: &str, registers: &mut Registers) {
        let typed = text.chars().filter(|c| !c.is_control());
        // A password goes in character by character, so that no copy of it is left behind.
        if self.editor.is_secret() {
            typed.for_each(|c| self.editor.insert(c));
            return;
        }
        self.editor.paste(&typed.collect::<String>(), registers);
    }

    /// Gives up the command of Normal mode that waits, as the keymap gives up its keys.
    pub(crate) fn give_up(&mut self) {
        self.editor.give_up();
    }

    /// Whether the field is masked, as for a password.
    pub(crate) fn is_secret(&self) -> bool {
        self.editor.is_secret()
    }

    /// Edits the text as Insert mode does, without leaving it; `false` if `action` is not an
    /// edit. For filters, which stay in Insert mode.
    pub(crate) fn edit(&mut self, action: Action) -> bool {
        self.editor.edit(action)
    }

    /// Takes `action`, typed with `extra`, in the field's mode; `false` if it is not the
    /// field's, such as Enter, or Esc in Normal mode.
    pub(crate) fn handle(
        &mut self,
        action: Action,
        extra: Extra,
        registers: &mut Registers,
    ) -> bool {
        self.editor.handle(action, extra, registers)
    }

    /// Puts the cursor at `column` of the `room` cells the field was drawn in, as a click does,
    /// in Insert mode.
    pub(crate) fn click(&mut self, column: usize, room: usize) {
        let (first, shown, _) = self.window(room);
        let mut used = 0;
        let before = shown
            .chars()
            .take_while(|&c| {
                used += c.width().unwrap_or(0);
                used <= column
            })
            .count();
        self.editor.set_mode(Mode::Insert);
        self.editor.set_cursor(first + before);
    }

    /// What fits in `room` cells, as shown (stars for a secret), from where the cursor stays
    /// on screen, and the cursor's column in it.
    pub(crate) fn visible(&self, room: usize) -> (String, usize) {
        let (_, shown, column) = self.window(room);
        (shown, column)
    }

    /// What [`visible`](Self::visible) shows, cut in three: before what Visual mode selects,
    /// the selection, and after it; and the cursor's column.
    pub(crate) fn visible_parts(&self, room: usize) -> ([String; 3], usize) {
        let (first, shown, column) = self.window(room);
        let chars: Vec<char> = shown.chars().collect();
        let (start, end) = self.editor.selection().map_or((0, 0), |selected| {
            let clamp = |index: usize| index.saturating_sub(first).min(chars.len());
            (clamp(selected.start), clamp(selected.end))
        });
        let part = |range: Range<usize>| chars[range].iter().collect::<String>();
        let parts = [part(0..start), part(start..end), part(end..chars.len())];
        (parts, column)
    }

    /// The first character shown, what fits from there, and the cursor's column.
    fn window(&self, room: usize) -> (usize, String, usize) {
        let chars: Vec<char> = if self.editor.is_secret() {
            vec!['*'; self.editor.text().chars().count()]
        } else {
            self.editor.text().chars().collect()
        };
        let cursor = self.editor.cursor();
        let width = |c: char| c.width().unwrap_or(0);
        // Back from the cursor while the text before it and the cursor itself fit.
        let mut first = cursor;
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
        // Character for character: `shown` is UTF-8, and only control characters change.
        (first, cells::sanitize(shown.as_bytes()), column)
    }
}

impl fmt::Debug for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.editor.fmt(f)
    }
}

/// A text field, with its label drawn on a line of its own above it unless empty.
#[derive(Debug)]
struct LabelledField {
    label: String,
    field: Field,
}

impl LabelledField {
    fn unlabelled(field: Field) -> Self {
        Self {
            label: String::new(),
            field,
        }
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

/// A modal dialog: a message, a group of choices of which one is chosen, text fields, check
/// boxes, and buttons, each of them optional but the buttons. Whoever opens it reads the choice,
/// the fields, and the check boxes once it closes.
#[derive(Debug)]
pub(crate) struct Dialog {
    /// Tells this dialog from one that takes its place.
    id: u64,
    title: String,
    message: String,
    /// Radio buttons, as mc draws them: `(*)` on the chosen one.
    choices: Vec<String>,
    chosen: usize,
    fields: Vec<LabelledField>,
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
    /// Its first field holds a path, which Tab completes.
    completes: bool,
    /// Where its first field was drawn last, for the list of completions under it.
    field_area: Cell<Option<Rect>>,
    /// Where each choice, field, check box, and button was drawn last, for the mouse.
    spots: RefCell<Vec<(Focus, Rect)>>,
}

impl Dialog {
    /// A dialog for a prompt of `kind`: a masked field with OK and Cancel for secrets, Yes and
    /// No (with the focus) for questions. `context` is the host alias.
    pub(crate) fn prompt(context: &str, message: &str, kind: PromptKind) -> Self {
        match kind {
            PromptKind::Secret => Self {
                fields: vec![LabelledField::unlabelled(Field::secret())],
                focus: Focus::Field(0),
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
        Self {
            fields: vec![LabelledField::unlabelled(Field::plain(text))],
            checks: Self::checks(checks),
            focus: Focus::Field(0),
            width,
            ..Self::new(title, message, vec![Button::Ok, Button::Cancel])
        }
    }

    /// Text fields with their labels and the texts they open with, check boxes with their
    /// labels and states, and `buttons`, the first of them the default, `width` cells wide.
    pub(crate) fn fields(
        title: &str,
        fields: &[(String, String)],
        checks: &[(String, bool)],
        buttons: Vec<Button>,
        width: u16,
    ) -> Self {
        let fields: Vec<LabelledField> = fields
            .iter()
            .map(|(label, text)| LabelledField {
                label: cells::sanitize(label.as_bytes()),
                field: Field::plain(text),
            })
            .collect();
        let focus = if fields.is_empty() {
            Focus::Button(0)
        } else {
            Focus::Field(0)
        };
        Self {
            fields,
            checks: Self::checks(checks),
            focus,
            width,
            ..Self::new(title, "", buttons)
        }
    }

    fn checks(checks: &[(String, bool)]) -> Vec<Check> {
        checks
            .iter()
            .map(|(label, on)| Check {
                label: label.clone(),
                on: *on,
            })
            .collect()
    }

    fn new(title: &str, message: &str, buttons: Vec<Button>) -> Self {
        Self {
            id: LAST_ID.fetch_add(1, Ordering::Relaxed) + 1,
            title: cells::sanitize(title.as_bytes()),
            message: message.trim_end().to_owned(),
            choices: Vec::new(),
            chosen: 0,
            fields: Vec::new(),
            checks: Vec::new(),
            buttons,
            default: 0,
            focus: Focus::Button(0),
            width: MAX_WIDTH,
            error: false,
            completes: false,
            field_area: Cell::new(None),
            spots: RefCell::new(Vec::new()),
        }
    }

    /// Tells this dialog from every other one.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// The same dialog, with a path in its first field, which Tab completes.
    pub(crate) fn with_completion(mut self) -> Self {
        self.completes = true;
        self
    }

    /// Whether the first field has the focus and holds a path that Tab completes.
    pub(crate) fn completes(&self) -> bool {
        self.completes && self.focus == Focus::Field(0)
    }

    /// Where the first field was drawn last.
    pub(crate) fn field_area(&self) -> Option<Rect> {
        self.field_area.get()
    }

    /// The text of the first field before its cursor, and after it.
    pub(crate) fn around_cursor(&self) -> (&str, &str) {
        match self.fields.first() {
            Some(LabelledField { field, .. }) if !field.editor.is_secret() => {
                let text = field.text();
                let offset = text
                    .char_indices()
                    .nth(field.editor.cursor())
                    .map_or(text.len(), |(offset, _)| offset);
                text.split_at(offset)
            }
            _ => ("", ""),
        }
    }

    /// Replaces the text of the first field before its cursor with `before`, keeping what is
    /// after it, with the cursor after `before`.
    pub(crate) fn replace_before_cursor(&mut self, before: &str) {
        let after = self.around_cursor().1.to_owned();
        let Some(LabelledField { field, .. }) = self.fields.first_mut() else {
            return;
        };
        if field.editor.is_secret() {
            return;
        }
        field
            .editor
            .replace(&format!("{before}{after}"), before.chars().count());
    }

    /// The same dialog with `message` above the rest.
    pub(crate) fn with_message(mut self, message: &str) -> Self {
        message.trim_end().clone_into(&mut self.message);
        self
    }

    /// The same dialog with `choices` above its fields, `chosen` of them chosen. Without
    /// fields, the chosen one starts with the focus.
    pub(crate) fn with_choices(mut self, choices: Vec<String>, chosen: usize) -> Self {
        self.chosen = chosen.min(choices.len().saturating_sub(1));
        if self.fields.is_empty() && !choices.is_empty() {
            self.focus = Focus::Choice(self.chosen);
        }
        self.choices = choices;
        self
    }

    /// The choice that is chosen.
    pub(crate) fn chosen(&self) -> usize {
        self.chosen
    }

    /// The answer for ssh after `event`: the secret for OK on a secret prompt, `yes` for Yes,
    /// and `None` to decline.
    pub(crate) fn answer(&self, event: DialogEvent) -> Option<SecretString> {
        match (event, self.fields.first()) {
            (DialogEvent::Pressed(Button::Ok), Some(LabelledField { field, .. })) => {
                // From `&str`: an exact copy, where `String` would be shrunk into new memory.
                Some(SecretString::from(field.text()))
            }
            (DialogEvent::Pressed(Button::Yes), _) => Some(SecretString::from("yes")),
            _ => None,
        }
    }

    /// The text of the first field, if plain.
    pub(crate) fn text(&self) -> &str {
        self.text_of(0)
    }

    /// The text of field `index`, if plain.
    pub(crate) fn text_of(&self, index: usize) -> &str {
        match self.fields.get(index) {
            Some(LabelledField { field, .. }) if !field.editor.is_secret() => field.text(),
            _ => "",
        }
    }

    /// Replaces the text of plain field `index` and gives it the focus, in Insert mode.
    pub(crate) fn set_text(&mut self, index: usize, text: &str) {
        let Some(LabelledField { field, .. }) = self.fields.get_mut(index) else {
            return;
        };
        if field.editor.is_secret() {
            return;
        }
        *field = Field::plain(text);
        self.focus = Focus::Field(index);
    }

    /// Whether check box `index` is checked.
    pub(crate) fn checked(&self, index: usize) -> bool {
        self.checks.get(index).is_some_and(|check| check.on)
    }

    /// The keymap context for the next key: `DialogInput` while a text field has the focus,
    /// `PathInput` in a field that completes paths.
    pub(crate) fn context(&self) -> Context {
        match self.focus {
            Focus::Field(0) if self.completes => Context::PathInput,
            Focus::Field(_) => Context::DialogInput,
            Focus::Choice(_) | Focus::Check(_) | Focus::Button(_) => Context::Dialog,
        }
    }

    /// The mode keys are typed in: the focused field's, or Normal mode on a button, a check
    /// box, or a choice.
    pub(crate) fn mode(&self) -> Mode {
        self.focused_field().map_or(Mode::Normal, Field::mode)
    }

    /// The operator that the focused field has waiting for its motion, as `showcmd` shows it.
    pub(crate) fn pending(&self) -> Option<String> {
        self.focused_field().and_then(Field::pending)
    }

    /// Whether the focused field is a secret, such as a password.
    pub(crate) fn secret_focused(&self) -> bool {
        self.focused_field().is_some_and(Field::is_secret)
    }

    /// Puts pasted `text` in the focused field, if any.
    pub(crate) fn paste(&mut self, text: &str, registers: &mut Registers) {
        if let Some(field) = self.focused_field_mut() {
            field.paste(text, registers);
        }
    }

    /// Gives up the command of Normal mode that the focused field has waiting.
    pub(crate) fn give_up(&mut self) {
        if let Some(field) = self.focused_field_mut() {
            field.give_up();
        }
    }

    /// Lets the focused field go: what Visual mode selects there stops showing.
    fn leave_field(&mut self) {
        if let Some(field) = self.focused_field_mut()
            && field.mode() == Mode::Visual
        {
            field.set_mode(Mode::Normal);
        }
    }

    pub(crate) fn focused_field(&self) -> Option<&Field> {
        match self.focus {
            Focus::Field(index) => self.fields.get(index).map(|field| &field.field),
            Focus::Choice(_) | Focus::Check(_) | Focus::Button(_) => None,
        }
    }

    fn focused_field_mut(&mut self) -> Option<&mut Field> {
        match self.focus {
            Focus::Field(index) => self.fields.get_mut(index).map(|field| &mut field.field),
            Focus::Choice(_) | Focus::Check(_) | Focus::Button(_) => None,
        }
    }

    /// Handles an action or a typed character, as typed without a count or a register.
    #[cfg(test)]
    pub(crate) fn handle(&mut self, input: Resolved) -> DialogEvent {
        self.handle_typed(Typed::from(input), &mut Registers::default())
    }

    /// Handles an action or a typed character: the focused field takes what it edits with,
    /// in its mode. Tab brings the focus to a field in Insert mode; the other keys that move it
    /// bring it there in Normal mode, but from a field in Insert mode.
    pub(crate) fn handle_typed(&mut self, typed: Typed, registers: &mut Registers) -> DialogEvent {
        let Typed { resolved, extra } = typed;
        let action = match (resolved, self.focused_field_mut()) {
            (Resolved::Insert(c), Some(field)) => {
                field.insert(c);
                return DialogEvent::Pending;
            }
            (Resolved::Insert(_), None) => return DialogEvent::Pending,
            (Resolved::Action(action), Some(field)) => {
                if field.handle(action, extra, registers) {
                    return DialogEvent::Pending;
                }
                // A secret has no Normal mode: Esc closes its dialog.
                if action == Action::NormalMode {
                    return DialogEvent::Cancelled;
                }
                // A key of the dialog's is the one command after Ctrl+o, which leaves the field
                // in Insert mode, so that one that moves the focus moves it there in Insert mode.
                field.give_up();
                action
            }
            (Resolved::Action(action), None) => action,
        };
        match action {
            // Enter takes the choice it is on, as that is the one that looks chosen.
            Action::Confirm => match self.focus {
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::Choice(index) => {
                    self.chosen = index;
                    DialogEvent::Pressed(self.buttons[self.default])
                }
                Focus::Field(_) | Focus::Check(_) => {
                    DialogEvent::Pressed(self.buttons[self.default])
                }
            },
            Action::Toggle => match self.focus {
                Focus::Choice(index) => {
                    self.chosen = index;
                    DialogEvent::Pending
                }
                Focus::Check(index) => {
                    self.checks[index].on = !self.checks[index].on;
                    DialogEvent::Pending
                }
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::Field(_) => DialogEvent::Pending,
            },
            Action::Cancel => DialogEvent::Cancelled,
            Action::NextField | Action::PrevField => {
                self.move_focus(action == Action::NextField, Mode::Insert);
                DialogEvent::Pending
            }
            // A count moves as many stops, past the fields too, as vim's `3j` moves three lines.
            Action::Right | Action::Down | Action::Left | Action::Up => {
                let mode = self.mode();
                for _ in 0..extra.times() {
                    self.move_focus(matches!(action, Action::Right | Action::Down), mode);
                }
                DialogEvent::Pending
            }
            // As vim's `g g` and `G` go to the first and the last line.
            Action::Home | Action::End | Action::PageUp | Action::PageDown => {
                self.focus_end(matches!(action, Action::End | Action::PageDown));
                DialogEvent::Pending
            }
            _ => DialogEvent::Pending,
        }
    }

    /// Takes a press of the mouse, where the dialog was drawn last: a click chooses a choice,
    /// puts the cursor in a field, switches a check box, or presses a button. A double click
    /// on a choice presses the default button too, as Enter does there.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> DialogEvent {
        let Pointer { press, at } = pointer;
        let spot = self
            .spots
            .borrow()
            .iter()
            .find(|(_, area)| area.contains(at))
            .copied();
        let Some((focus, area)) = spot else {
            return DialogEvent::Pending;
        };
        if !matches!(press, Press::Click | Press::DoubleClick) {
            return DialogEvent::Pending;
        }
        if focus != self.focus {
            self.leave_field();
        }
        self.focus = focus;
        match focus {
            Focus::Choice(index) => {
                self.chosen = index;
                if press == Press::DoubleClick {
                    return DialogEvent::Pressed(self.buttons[self.default]);
                }
            }
            Focus::Field(index) => {
                if let Some(LabelledField { field, .. }) = self.fields.get_mut(index) {
                    let column = usize::from(at.x - area.x);
                    field.click(column, usize::from(area.width));
                }
            }
            Focus::Check(index) => self.checks[index].on = !self.checks[index].on,
            // The first click pressed it.
            Focus::Button(_) if press == Press::DoubleClick => {}
            Focus::Button(index) => return DialogEvent::Pressed(self.buttons[index]),
        }
        DialogEvent::Pending
    }

    /// Where the focus can be: the choices, the fields, the check boxes, and the buttons.
    fn stops(&self) -> Vec<Focus> {
        (0..self.choices.len())
            .map(Focus::Choice)
            .chain((0..self.fields.len()).map(Focus::Field))
            .chain((0..self.checks.len()).map(Focus::Check))
            .chain((0..self.buttons.len()).map(Focus::Button))
            .collect()
    }

    /// Moves the focus through the choices, the fields, the check boxes, and the buttons,
    /// round; a field it comes to is in `mode`, Insert or Normal.
    fn move_focus(&mut self, forward: bool, mode: Mode) {
        let stops = self.stops();
        let current = stops
            .iter()
            .position(|stop| *stop == self.focus)
            .unwrap_or(0);
        let step = if forward { 1 } else { stops.len() - 1 };
        self.focus_on(stops[(current + step) % stops.len()], mode);
    }

    /// Moves the focus to the first stop, or the last; a field it comes to is in Normal mode.
    fn focus_end(&mut self, last: bool) {
        let stops = self.stops();
        let stop = if last { stops.last() } else { stops.first() };
        if let Some(&stop) = stop
            && stop != self.focus
        {
            self.focus_on(stop, Mode::Normal);
        }
    }

    /// Moves the focus to `stop`; a field there is in `mode`, Insert or Normal.
    fn focus_on(&mut self, stop: Focus, mode: Mode) {
        self.leave_field();
        self.focus = stop;
        let mode = if mode == Mode::Insert {
            Mode::Insert
        } else {
            Mode::Normal
        };
        if let Some(field) = self.focused_field_mut() {
            field.set_mode(mode);
        }
    }

    /// Draws the dialog centered in `area`, with the terminal cursor in the focused field.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let width = self.width.min(area.width.saturating_sub(4)).max(20);
        let text_width = usize::from(width.saturating_sub(4));
        let lines = cells::wrap(&self.message, text_width);
        let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
        // Borders, the message, the choices, the fields under their labels, the check boxes, a
        // line, the buttons.
        let labels = self.fields.iter().filter(|f| !f.label.is_empty()).count();
        let height = rows(lines.len())
            .saturating_add(rows(self.choices.len()))
            .saturating_add(rows(self.fields.len() + labels))
            .saturating_add(rows(self.checks.len()));
        let colors = Colors::of(theme, self.error);
        let size = (width, height + 4);
        let inner = draw_box(frame, area, size, &self.title, colors, theme);
        let row = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let mut spots = self.spots.borrow_mut();
        spots.clear();
        let mut index = 0;
        for line in &lines {
            if index >= inner.height {
                return;
            }
            frame.render_widget(Line::raw(line.as_str()), row(index));
            index += 1;
        }
        let room = usize::from(inner.width).max(1);
        for (number, choice) in self.choices.iter().enumerate() {
            if index >= inner.height {
                return;
            }
            let mark = if number == self.chosen { '*' } else { ' ' };
            let text = format!("({mark}) {choice}");
            let style = if self.focus == Focus::Choice(number) {
                colors.focused
            } else {
                colors.body
            };
            frame.render_widget(Line::from(Span::styled(text, style)), row(index));
            spots.push((Focus::Choice(number), row(index)));
            index += 1;
        }
        for (number, LabelledField { label, field }) in self.fields.iter().enumerate() {
            if !label.is_empty() {
                if index >= inner.height {
                    return;
                }
                let label = cells::fit(label, room, cells::Align::Left);
                frame.render_widget(Line::raw(label), row(index));
                index += 1;
            }
            if index >= inner.height {
                return;
            }
            let ([before, selected, after], column) = field.visible_parts(room);
            let used = cells::width(&before) + cells::width(&selected);
            let after = cells::fit(&after, room.saturating_sub(used), cells::Align::Left);
            let line = Line::from(vec![
                Span::raw(before),
                Span::styled(
                    selected,
                    theme.dialog_input.add_modifier(Modifier::REVERSED),
                ),
                Span::raw(after),
            ]);
            frame.render_widget(line.style(theme.dialog_input), row(index));
            spots.push((Focus::Field(number), row(index)));
            if number == 0 {
                self.field_area.set(Some(row(index)));
            }
            if self.focus == Focus::Field(number) {
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
            spots.push((Focus::Check(number), row(index)));
            index += 1;
        }
        if index + 1 < inner.height {
            draw_separator(frame, inner, inner.y + index, colors, theme);
            let labels: Vec<String> = self.buttons.iter().map(|button| button.label()).collect();
            let focus = match self.focus {
                Focus::Button(index) => Some(index),
                Focus::Choice(_) | Focus::Field(_) | Focus::Check(_) => None,
            };
            let line = button_line(&labels, self.default, focus, colors);
            let buttons = button_spots(&line, row(index + 1));
            spots.extend((0..).map(Focus::Button).zip(buttons));
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

/// Where the buttons of `line`, from [`button_line`], are when it is drawn on `area`, which
/// centers it.
pub(crate) fn button_spots(line: &Line<'_>, area: Rect) -> Vec<Rect> {
    let width = |span: &Span<'_>| i32::try_from(span.width()).unwrap_or(i32::MAX);
    let total = line.spans.iter().map(width).sum::<i32>();
    // Ratatui centers a line that does not fit by cutting as much off either side.
    let mut x = i32::from(area.x) + (i32::from(area.width) - total).div_euclid(2);
    if total > i32::from(area.width) {
        x = i32::from(area.x) - (total - i32::from(area.width)) / 2;
    }
    let mut spots = Vec::new();
    // Buttons and the spaces between them take turns.
    for (index, span) in line.spans.iter().enumerate() {
        let start = x;
        x += width(span);
        if index % 2 == 1 {
            continue;
        }
        let left = start.max(i32::from(area.x));
        let right = x.min(i32::from(area.right()));
        let (Ok(left), Ok(right)) = (u16::try_from(left), u16::try_from(right)) else {
            continue;
        };
        spots.push(Rect::new(left, area.y, right.saturating_sub(left), 1));
    }
    spots
}

/// Draws a line across a dialog whose room inside is `inner`, on its row `y`, joined to the
/// frame as mc joins them: `╟───╢`, or `├───┤` with single lines. It sets the buttons apart
/// from what is above them.
pub(crate) fn draw_separator(
    frame: &mut Frame<'_>,
    inner: Rect,
    y: u16,
    colors: Colors,
    theme: &Theme,
) {
    let (left, right) = theme.tees();
    // The frame is a cell of padding and a cell of border away from the room inside.
    let x = inner.x.saturating_sub(2);
    let width = inner.width.saturating_add(4);
    let line = format!(
        "{left}{}{right}",
        "─".repeat(usize::from(width.saturating_sub(2)))
    );
    let row = Rect::new(x, y, width, 1).intersection(frame.area());
    frame.render_widget(Line::styled(line, colors.body), row);
}

/// The frame around `inner`, the room inside a box that [`draw_box`] drew.
pub(crate) fn frame_around(inner: Rect) -> Rect {
    Rect::new(
        inner.x.saturating_sub(2),
        inner.y.saturating_sub(1),
        inner.width.saturating_add(4),
        inner.height.saturating_add(2),
    )
}

/// Draws an empty dialog box centered in `area`: a frame of `size` in the theme's lines with
/// its title bold in the middle, a blank cell around it where there is room, and mc's shadow. Returns the room
/// inside, one column in from the frame on either side.
pub(crate) fn draw_box(
    frame: &mut Frame<'_>,
    area: Rect,
    (width, height): (u16, u16),
    title: &str,
    colors: Colors,
    theme: &Theme,
) -> Rect {
    // On a small screen the frame keeps its room and the margin gives way.
    let fit = |size: u16, room: u16| {
        let size = size.min(room);
        let margin = ((room - size) / 2).min(MARGIN);
        (size + 2 * margin, margin)
    };
    let (width, margin_x) = fit(width, area.width);
    let (height, margin_y) = fit(height, area.height);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let outer = Rect::new(x, y, width, height);
    frame.render_widget(Clear, outer);
    frame.render_widget(Block::new().style(colors.body), outer);
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
    let title = Line::styled(format!(" {title} "), colors.title.bold()).centered();
    let block = Block::bordered()
        .border_type(theme.border_type())
        .title(title)
        .style(colors.body);
    let framed = outer.inner(Margin::new(margin_x, margin_y));
    let inner = block.inner(framed).inner(Margin::new(1, 0));
    frame.render_widget(block, framed);
    inner
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
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

    /// The column where `title` starts on the dialog's top row, row 1.
    fn title_column(buffer: &Buffer, title: &str) -> u16 {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 1)].symbol())
            .collect();
        let start = row.find(title).unwrap();
        u16::try_from(row[..start].chars().count()).unwrap()
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
        for edit in [
            Action::Left,
            Action::Left,
            Action::Backspace,
            Action::TextStart,
        ] {
            dialog.handle(action(edit));
        }
        typed(&mut dialog, "X");
        dialog.handle(action(Action::DeleteChar));
        dialog.handle(action(Action::TextEnd));
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
        use ratatui::style::{Color, Modifier};

        let dialog = secret();
        let terminal = draw_themed(&dialog, 50, 9, &Theme::mc_classic());
        let buffer = terminal.backend().buffer();
        let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        // The frame spans columns 2 … 47 and rows 1 … 6, and the margin around it the rest of
        // columns 1 … 48 and rows 0 … 7.
        assert_eq!(colors(2, 1), (Color::Black, Color::Gray), "frame");
        assert_eq!(buffer[(2, 1)].symbol(), "╔");
        let title = title_column(buffer, "web");
        assert_eq!(colors(title, 1), (Color::Blue, Color::Gray), "title");
        assert!(buffer[(title, 1)].modifier.contains(Modifier::BOLD));
        // ` web ` between the corners in columns 2 and 47.
        let (left, right) = (title - 1 - 3, 46 - (title + 3));
        assert!(left.abs_diff(right) <= 1, "centered: {left} and {right}");
        assert_eq!(colors(4, 3), (Color::Black, Color::Cyan), "field");
        for (x, y) in [(1, 0), (1, 3), (48, 3), (10, 0), (10, 7)] {
            assert_eq!(colors(x, y).1, Color::Gray, "margin at {x}, {y}");
            assert_eq!(buffer[(x, y)].symbol(), " ", "margin at {x}, {y}");
        }
        assert_eq!(colors(0, 3).1, Color::Reset, "outside");
        assert_eq!(
            colors(49, 2),
            (Color::DarkGray, Color::Black),
            "shadow to the right"
        );
        assert_eq!(
            colors(10, 8),
            (Color::DarkGray, Color::Black),
            "shadow below"
        );
        assert_eq!(
            colors(2, 8).1,
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
    fn a_form_opens_in_insert_mode_and_esc_goes_to_normal_mode() {
        let mut dialog = form();
        assert_eq!(dialog.context(), Context::DialogInput);
        assert_eq!(dialog.mode(), Mode::Insert);
        assert_eq!(dialog.text(), "*");
        typed(&mut dialog, ".txt");
        assert_eq!(dialog.text(), "*.txt", "typing goes on from the end");
        assert_eq!(
            dialog.handle(action(Action::NormalMode)),
            DialogEvent::Pending
        );
        assert_eq!(dialog.mode(), Mode::Normal);
        dialog.handle(action(Action::TextStart));
        dialog.handle(action(Action::DeleteChar));
        assert_eq!(dialog.text(), ".txt");
        dialog.handle(action(Action::Undo));
        assert_eq!(dialog.text(), "*.txt");
        assert_eq!(
            dialog.handle(action(Action::Cancel)),
            DialogEvent::Cancelled,
            "Esc in Normal mode closes it"
        );
        let mut dialog = form();
        dialog.handle(action(Action::NormalMode));
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pressed(Button::Ok),
            "Enter in Normal mode too"
        );
        // A secret has no Normal mode: Esc closes its dialog.
        assert_eq!(
            secret().handle(action(Action::NormalMode)),
            DialogEvent::Cancelled
        );
    }

    #[test]
    fn esc_after_ctrl_o_goes_back_to_insert_mode_and_leaves_the_dialog_open() {
        let mut dialog = form();
        assert_eq!(
            dialog.handle(action(Action::NormalOnce)),
            DialogEvent::Pending
        );
        assert_eq!(dialog.handle(action(Action::Cancel)), DialogEvent::Pending);
        assert_eq!(dialog.mode(), Mode::Insert);
        assert!(!dialog.secret_focused());
        assert!(secret().secret_focused());
    }

    #[test]
    fn tab_brings_the_focus_to_a_field_in_insert_mode_and_j_and_k_in_normal_mode() {
        let mut dialog = host_form();
        dialog.handle(action(Action::NormalMode));
        dialog.handle(action(Action::Down));
        assert_eq!(
            (dialog.text_of(1), dialog.mode()),
            ("/var/www", Mode::Normal)
        );
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.mode(), Mode::Insert, "Tab");
        dialog.handle(action(Action::Up));
        assert_eq!(dialog.mode(), Mode::Insert, "from a field in Insert mode");
        assert_eq!(dialog.pending(), None);
        dialog.handle(action(Action::NormalMode));
        dialog.handle(action(Action::DeleteOperator));
        assert_eq!(dialog.pending().as_deref(), Some("d"));
        assert_eq!(
            dialog.handle(action(Action::Cancel)),
            DialogEvent::Pending,
            "Esc gives the operator up first"
        );
        assert_eq!(dialog.pending(), None);
    }

    #[test]
    fn a_key_of_the_dialog_after_ctrl_o_leaves_insert_mode_on_and_g_g_and_g_go_to_the_ends() {
        let mut dialog = host_form();
        dialog.handle(action(Action::NormalOnce));
        dialog.handle(action(Action::Down));
        assert_eq!(
            (dialog.focus, dialog.mode()),
            (Focus::Field(1), Mode::Insert)
        );
        dialog.handle(action(Action::NormalMode));
        dialog.handle(action(Action::End));
        assert_eq!(dialog.focus, Focus::Button(dialog.buttons.len() - 1));
        dialog.handle(action(Action::Home));
        assert_eq!(
            (dialog.focus, dialog.mode()),
            (Focus::Field(0), Mode::Normal)
        );
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
        field.editor.set_cursor(0);
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
    fn ctrl_w_deletes_the_word_before_the_cursor_as_vim_does() {
        let mut field = Field::plain("cp a.txt  backup");
        assert!(field.edit(Action::DeleteWord));
        assert_eq!(field.text(), "cp a.txt  ");
        field.edit(Action::DeleteWord);
        assert_eq!(
            field.text(),
            "cp a.",
            "the spaces go with the word, and a dot is one"
        );
        field.edit(Action::TextStart);
        field.edit(Action::DeleteWord);
        assert_eq!(field.text(), "cp a.", "nothing before the cursor");
    }

    #[test]
    fn word_keys_go_to_the_starts_of_words() {
        let mut field = Field::plain("cp  a.txt backup");
        assert!(field.edit(Action::WordBackward));
        assert_eq!(field.editor.cursor(), 10, "the start of backup");
        field.edit(Action::WordBackward);
        assert_eq!(field.editor.cursor(), 6, "txt");
        field.edit(Action::TextStart);
        field.edit(Action::WordForward);
        assert_eq!(field.editor.cursor(), 4, "past the spaces");
        field.edit(Action::TextEnd);
        field.edit(Action::WordForward);
        assert_eq!(field.editor.cursor(), 16, "the end stays");
    }

    #[test]
    fn visual_mode_shows_what_it_selects() {
        let parts = |field: &Field, room| field.visible_parts(room);
        let mut registers = Registers::default();
        let mut field = Field::normal("report.pdf");
        field.editor.set_cursor(0);
        for action in [Action::VisualMode, Action::WordEnd] {
            assert!(field.handle(action, Extra::default(), &mut registers));
        }
        assert_eq!(
            parts(&field, 20),
            ([String::new(), "report".to_owned(), ".pdf".to_owned()], 5)
        );
        field.handle(Action::NormalMode, Extra::default(), &mut registers);
        assert_eq!(parts(&field, 20).0[1], "", "nothing is selected any more");
    }

    /// Presses the mouse on the first cell of `text` where `dialog` is drawn at 40 by 12.
    fn press_on(dialog: &mut Dialog, press: Press, text: &str) -> DialogEvent {
        let terminal = draw(dialog, 40, 12);
        let at = super::super::mouse::find(terminal.backend().buffer(), text);
        dialog.pointer(Pointer { press, at })
    }

    #[test]
    fn the_mouse_switches_check_boxes_edits_fields_and_presses_buttons() {
        let mut dialog = form();
        assert_eq!(
            press_on(&mut dialog, Press::Click, "Files only"),
            DialogEvent::Pending
        );
        assert!(dialog.checked(0));
        assert_eq!(dialog.focus, Focus::Check(0));
        press_on(&mut dialog, Press::Click, "*");
        assert_eq!(dialog.focus, Focus::Field(0));
        dialog.handle(Resolved::Insert('x'));
        assert_eq!(
            dialog.text(),
            "x*",
            "the text stays and the cursor is where clicked"
        );
        assert_eq!(
            press_on(&mut dialog, Press::DoubleClick, "Cancel"),
            DialogEvent::Pending,
            "the first click pressed it"
        );
        assert_eq!(
            press_on(&mut dialog, Press::Click, "Cancel"),
            DialogEvent::Pressed(Button::Cancel)
        );
        assert_eq!(
            press_on(&mut dialog, Press::Click, "OK"),
            DialogEvent::Pressed(Button::Ok)
        );
    }

    #[test]
    fn a_double_click_on_a_choice_chooses_it_and_confirms() {
        let choices = vec!["one".to_owned(), "two".to_owned()];
        let mut dialog = Dialog::fields("Pick", &[], &[], vec![Button::Ok, Button::Cancel], 40)
            .with_choices(choices, 0);
        press_on(&mut dialog, Press::Click, "two");
        assert_eq!((dialog.chosen(), dialog.focus), (1, Focus::Choice(1)));
        press_on(&mut dialog, Press::Click, "one");
        assert_eq!(
            press_on(&mut dialog, Press::DoubleClick, "two"),
            DialogEvent::Pressed(Button::Ok)
        );
        assert_eq!(dialog.chosen(), 1);
    }

    #[test]
    fn draws_a_form_with_its_check_boxes() {
        let mut dialog = form();
        dialog.handle(action(Action::DeleteToStart));
        typed(&mut dialog, "*.md");
        dialog.handle(action(Action::NextField));
        let terminal = draw(&dialog, 60, 10);
        insta::assert_snapshot!(terminal.backend());
    }

    fn host_form() -> Dialog {
        let fields = [
            ("Label:".to_owned(), "Prod".to_owned()),
            ("Remote directory:".to_owned(), "/var/www".to_owned()),
            ("Other directory:".to_owned(), String::new()),
        ];
        let checks = [("Remember".to_owned(), true)];
        let buttons = vec![Button::Ok, Button::UseCurrent, Button::Cancel];
        Dialog::fields("Host web", &fields, &checks, buttons, 60)
    }

    #[test]
    fn the_focus_moves_through_the_fields_in_order() {
        let mut dialog = host_form();
        assert_eq!(dialog.context(), Context::DialogInput);
        typed(&mut dialog, "Staging");
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::DialogInput);
        typed(&mut dialog, "/srv");
        assert_eq!(dialog.text_of(0), "ProdStaging");
        assert_eq!(dialog.text(), "ProdStaging");
        assert_eq!(dialog.text_of(1), "/var/www/srv");
        assert_eq!(dialog.text_of(2), "");
        assert_eq!(dialog.text_of(3), "", "no such field");
        dialog.handle(action(Action::Up));
        typed(&mut dialog, "!");
        assert_eq!(
            dialog.text_of(0),
            "ProdStaging!",
            "Up goes back to the first field"
        );
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Down));
        typed(&mut dialog, "/tmp");
        assert_eq!(dialog.text_of(2), "/tmp");
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::Dialog, "the check box");
        dialog.handle(action(Action::Toggle));
        assert!(!dialog.checked(0));
        dialog.handle(action(Action::NextField));
        dialog.handle(action(Action::NextField));
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pressed(Button::UseCurrent)
        );
        dialog.handle(action(Action::NextField));
        dialog.handle(action(Action::NextField));
        assert_eq!(
            dialog.context(),
            Context::DialogInput,
            "round to the first field"
        );
        dialog.handle(action(Action::Down));
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pressed(Button::Ok),
            "Enter in a field presses OK"
        );
    }

    #[test]
    fn set_text_replaces_a_field_and_focuses_it() {
        let mut dialog = host_form();
        dialog.set_text(2, "/home/deploy");
        assert_eq!(dialog.text_of(2), "/home/deploy");
        assert_eq!(dialog.context(), Context::DialogInput);
        typed(&mut dialog, "/app");
        assert_eq!(dialog.text_of(2), "/home/deploy/app", "typing keeps it");
        assert_eq!(dialog.text_of(0), "Prod");
        dialog.set_text(7, "ignored");

        let mut secret = secret();
        secret.set_text(0, "nope");
        assert_eq!(confirm(&mut secret).as_deref(), Some(""), "a secret stays");
    }

    #[test]
    fn draws_a_form_with_labelled_fields() {
        let mut dialog = host_form();
        dialog.handle(action(Action::Down));
        let mut terminal = draw(&dialog, 64, 16);
        insta::assert_snapshot!(terminal.backend());
        // At the end of `/var/www`, under its label.
        let cursor = terminal.get_cursor_position().unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..64).map(|x| buffer[(x, cursor.y)].symbol()).collect();
        assert!(row.contains("/var/www"), "{row}");
        let above: String = (0..64)
            .map(|x| buffer[(x, cursor.y - 1)].symbol())
            .collect();
        assert!(above.contains("Remote directory:"), "{above}");
    }

    #[test]
    fn errors_are_red_in_mc_classic() {
        use ratatui::style::{Color, Modifier};

        let mut dialog = Dialog::error("Error", "Cannot create directory /x: already exists");
        let terminal = draw_themed(&dialog, 60, 8, &Theme::mc_classic());
        let buffer = terminal.backend().buffer();
        let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        // The dialog spans columns 2 … 57 and rows 1 … 6.
        assert_eq!(colors(2, 1), (Color::White, Color::Red), "frame");
        let title = title_column(buffer, "Error");
        assert_eq!(colors(title, 1), (Color::LightYellow, Color::Red), "title");
        assert!(buffer[(title, 1)].modifier.contains(Modifier::BOLD));
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
}
