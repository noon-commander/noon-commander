//! Modal dialogs: prompts from ssh (passwords and passphrases, host keys, confirmations, and
//! notices), the app's own questions, and the search of a whole tree, whose dialog has rows (a picker without a filter).
//! As in mc and Far, Alt and the letter that an `&` marks in a label act on what it labels:
//! press a button, switch a check box, open a combo box, or put the focus in a field.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use noc_scroll::Motion;
use noc_scroll::scrollbar::Bar;
use noc_ssh::askpass::PromptKind;
use noc_vfs::Code;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use secrecy::SecretString;

use super::cells;
use super::combo_box::ComboBox;
use super::describe::Reason;
use super::edit::Registers;
use super::keymap::{Action, Context, Mode, Resolved};
use super::label::Label;
use super::mouse::Scroll;
use super::picker::{Body, Picker, ShownRow};
use super::search::{GlyphSpots, Input, Kind, Switch};
use super::spinner::Wait;
use super::window::{self, Tone, box_width};
use crate::i18n::fl;

mod field;
mod input;
mod render;
mod text;

#[cfg(test)]
mod tests;

pub(crate) use field::{Field, field_line};
use field::{LabelledField, draw_field, placeholder_line};
pub(crate) use text::even_lines;
use text::label_spans;

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
    /// Leave this entry, or keep the file there and leave the one that would replace it.
    Skip,
    Retry,
    Abort,
    /// Replace the file there.
    Overwrite,
    /// Give the file that would replace another a name of its own.
    Rename,
    /// Open again the dialog where what an error is about was typed.
    Change,
    /// Take what the panel shows.
    UseCurrent,
    /// Go to the row, as in mc's results of Find File; or to a path typed.
    GoTo,
    /// View the file of the row.
    View,
    /// Edit the file of the row.
    Edit,
    /// Go to the nearest parent directory that is there.
    Up,
    /// Connect to the host again.
    Connect,
    /// Go to a local directory.
    Local,
    /// Put the checksum of the selected file on the clipboard.
    ToClipboard,
    /// Put every line of the checksums on the clipboard, as `sha256sum` prints them.
    AllToClipboard,
    /// Save the checksums to a file.
    Save,
}

impl Button {
    /// Its name, with the letter that Alt and it press; Cancel has none, as Esc and F10 cancel.
    pub(crate) fn label(self) -> Label {
        Label::parse(&self.text())
    }

    fn text(self) -> String {
        match self {
            Self::Ok => fl!("dialog-ok"),
            Self::Cancel => fl!("dialog-cancel"),
            Self::Yes => fl!("dialog-yes"),
            Self::No => fl!("dialog-no"),
            Self::Skip => fl!("dialog-skip"),
            Self::Retry => fl!("dialog-retry"),
            Self::Abort => fl!("dialog-abort"),
            Self::Overwrite => fl!("dialog-overwrite"),
            Self::Rename => fl!("dialog-rename"),
            Self::Change => fl!("dialog-change"),
            Self::UseCurrent => fl!("dialog-use-current"),
            Self::GoTo => fl!("dialog-go-to"),
            Self::View => fl!("dialog-view"),
            Self::Edit => fl!("dialog-edit"),
            Self::Up => fl!("dialog-up"),
            Self::Connect => fl!("dialog-connect"),
            Self::Local => fl!("dialog-local"),
            Self::ToClipboard => fl!("dialog-to-clipboard"),
            Self::AllToClipboard => fl!("dialog-all-to-clipboard"),
            Self::Save => fl!("dialog-save"),
        }
    }
}

/// Where the keys go inside a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    ComboBox(usize),
    Field(usize),
    Check(usize),
    Item(usize),
    /// The rows of the picker.
    Picker,
    Button(usize),
}

/// A check box.
#[derive(Debug)]
struct Check {
    label: Label,
    on: bool,
}

/// A combo box, after its label on its line.
#[derive(Debug)]
struct LabelledComboBox {
    label: Label,
    combo: ComboBox,
}

/// A line that opens what it names, such as a file to view: its label on the left, what it
/// says of it on the right.
#[derive(Debug)]
struct Item {
    label: Label,
    detail: String,
}

/// Lines under the message that do not wrap as it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Text {
    /// A line that loses its middle where too long, such as the name of a file.
    Line(String),
    /// Text that wraps at its blanks as the message does, such as why something failed.
    Wrap(String),
    /// A label and its value, in bold after it, such as the code of a failure.
    Pair { label: String, value: String },
    /// Text without blanks, such as a checksum, in as few lines of equal length as it takes.
    Even(String),
    /// What the dialog comes to, after ✓ if `good` and ✗ if not, bold, in the color of titles
    /// if good and of errors if not.
    Verdict { text: String, good: bool },
}

/// Something that went wrong, as a dialog shows it: what could not be done, what it was done
/// to, why, and the code the system gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    /// What could not be done, such as `Cannot delete`.
    head: String,
    /// The path, or nothing where it is in `reason` or there is none.
    path: Option<String>,
    reason: Reason,
}

impl Problem {
    pub(crate) fn new(head: String, reason: impl Into<Reason>) -> Self {
        Self {
            head,
            path: None,
            reason: reason.into(),
        }
    }

    /// The same problem, its text safe for the terminal.
    pub(crate) fn sanitized(self) -> Self {
        let clean = |text: &str| cells::sanitize(text.as_bytes());
        Self {
            head: clean(&self.head),
            path: self.path.as_deref().map(clean),
            reason: self.reason.sanitized(),
        }
    }

    /// The code of the failure, if the system or the server gave one.
    pub(crate) fn code(&self) -> Option<Code> {
        self.reason.code
    }

    /// The same problem, on `path`.
    pub(crate) fn at(mut self, path: String) -> Self {
        self.path = Some(path);
        self
    }
}

/// A part of a dialog under its message, by its index among those of its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    ComboBox(usize),
    Field(usize),
    Check(usize),
    Item(usize),
    Text(usize),
    /// The rows of the picker under their filter, and what goes under them.
    Picker,
    /// A blank row that sets what is above it apart from what is under it.
    Separator,
    /// A line across the frame, as over the buttons, between parts.
    Line,
}

/// What the picker of a dialog draws at a render: the picker, which the dialog lends for it,
/// its lines and what goes under them, and how each row looks.
struct PickerDraw<'a, 'b> {
    picker: &'a mut Picker,
    /// Taken once the picker is drawn.
    body: Option<Body<'b>>,
    row: &'a mut dyn FnMut(ShownRow) -> Line<'static>,
}

/// What a key did to a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogEvent {
    /// The dialog stays open.
    Pending,
    /// A button was pressed: Enter in the field or on a check box presses the default one.
    Pressed(Button),
    /// The item at this index was opened; the dialog stays.
    Opened(usize),
    /// Esc or F10.
    Cancelled,
}

/// A modal dialog: a message, combo boxes, text fields, check boxes, lines of text, a picker,
/// and buttons, each of them optional but the buttons. Whoever opens it reads the
/// choices of the combo boxes, the fields, and the check boxes once it closes.
#[derive(Debug)]
pub(crate) struct Dialog {
    /// Tells this dialog from one that takes its place.
    id: u64,
    title: String,
    /// The text above the rest; a form's marks the letter of its field.
    message: Label,
    /// Each on a line of its own, the boxes lined up after the labels.
    combo_boxes: Vec<LabelledComboBox>,
    fields: Vec<LabelledField>,
    checks: Vec<Check>,
    items: Vec<Item>,
    texts: Vec<Text>,
    /// The combo boxes, the fields, the check boxes, the items, the lines of text, the picker,
    /// and the separators under the message, top to bottom, in the order they were added.
    parts: Vec<Part>,
    buttons: Vec<Button>,
    /// The button that Enter in the field activates, and that starts with the focus otherwise;
    /// drawn bold in `{ … }`.
    default: usize,
    focus: Focus,
    /// Widest the dialog gets, in cells, borders included.
    width: u16,
    tone: Tone,
    /// No answers `no` rather than declining: a server asking through ssh gets an empty answer
    /// when the prompt is declined, which it need not take for `no`.
    answers_no: bool,
    /// Its first field holds a path, which Tab completes.
    completes: bool,
    /// Where its first field was drawn last, for the list of completions under it.
    field_area: Cell<Option<Rect>>,
    /// Where the terminal cursor goes in the focused field, at the last render.
    cursor: Cell<Option<Position>>,
    /// Where each combo box, field, check box, and button was drawn last, for the mouse.
    spots: RefCell<Vec<(Focus, Rect)>>,
    /// Where the glyphs of each field that searches were drawn last, by the field's index, for
    /// the mouse.
    glyphs: RefCell<Vec<(usize, GlyphSpots)>>,
    /// The first line of a message taller than the screen shown, and how many show, at the
    /// last render.
    offset: Cell<usize>,
    page: Cell<usize>,
    /// The scroll bar of such a message at the last render, on the right of the frame.
    bar: Cell<Option<Bar>>,
    /// While its rows have the focus, the motions move their cursor, which Tab leaves, and what
    /// is typed goes to its filter after `/`.
    picker: Option<Box<Picker>>,
    /// Where the picker was drawn last, for the mouse.
    picker_area: Cell<Option<Rect>>,
    /// What the dialog waits for, such as ssh that waits for a touch of a security key: dots
    /// turn in front of its title.
    wait: Option<Wait>,
}

impl Dialog {
    /// A dialog for a prompt of `kind`: a masked field with OK and Cancel for secrets, Yes and
    /// No (with the focus) for questions, in the colors of warnings for host keys. `context` is
    /// the host alias.
    pub(crate) fn prompt(context: &str, message: &str, kind: PromptKind) -> Self {
        let question =
            |tone| Self::question(context, message, vec![Button::Yes, Button::No], 1, tone);
        match kind {
            PromptKind::Secret => Self::new(context, message, vec![Button::Ok, Button::Cancel])
                .with_part_field(LabelledField::unlabelled(Field::secret())),
            PromptKind::HostKey => question(Tone::Warning),
            PromptKind::Confirm => question(Tone::Plain),
            PromptKind::Question => Self {
                answers_no: true,
                ..question(Tone::Plain)
            },
        }
    }

    /// Information from ssh that needs no answer, such as a request to touch a security key,
    /// shown while ssh waits for it: the dots turn in front of the title, and Cancel stops
    /// connecting.
    pub(crate) fn notice(context: &str, message: &str) -> Self {
        Self {
            wait: Some(Wait::default()),
            ..Self::new(context, message, vec![Button::Cancel])
        }
    }

    /// A question in `tone` with `buttons`, of which `default` has the focus.
    pub(crate) fn question(
        title: &str,
        message: &str,
        buttons: Vec<Button>,
        default: usize,
        tone: Tone,
    ) -> Self {
        Self {
            default,
            focus: Focus::Button(default),
            tone,
            ..Self::new(title, message, buttons)
        }
    }

    /// Something to heed, such as that there was nothing to do, titled `title`, with OK.
    pub(crate) fn warning(title: &str, message: &str) -> Self {
        Self::question(title, message, vec![Button::Ok], 0, Tone::Warning)
    }

    /// Something that went wrong, titled Error, with OK.
    pub(crate) fn error(message: &str) -> Self {
        Self::question(
            &fl!("dialog-error"),
            message,
            vec![Button::Ok],
            0,
            Tone::Error,
        )
    }

    /// `problem`, titled Error, with OK.
    pub(crate) fn problem(problem: &Problem) -> Self {
        Self::failure(problem, vec![Button::Ok], 0)
    }

    /// `problem`, titled Error, with `buttons` for what to do now, of which `default` has the
    /// focus.
    pub(crate) fn failure(problem: &Problem, buttons: Vec<Button>, default: usize) -> Self {
        Self::failure_titled(&fl!("dialog-error"), problem, buttons, default)
    }

    /// `problem` in the colors of errors, titled `title`: what could not be done, the path it
    /// was done to on a line of its own, then, under a line across the frame, why, and the code
    /// the system gave it, its number in bold, or a dash; with `buttons`, of which `default` has
    /// the focus.
    pub(crate) fn failure_titled(
        title: &str,
        problem: &Problem,
        buttons: Vec<Button>,
        default: usize,
    ) -> Self {
        let mut dialog = Self::question(title, &problem.head, buttons, default, Tone::Error);
        if let Some(path) = &problem.path {
            dialog = dialog.with_text(Text::Line(path.clone()));
        }
        let (label, value) = match problem.reason.code {
            Some(Code::Os(code)) => (fl!("dialog-code"), code.to_string()),
            Some(Code::Sftp(code)) => (fl!("dialog-code-sftp"), code.to_string()),
            None => (fl!("dialog-code"), fl!("dialog-code-none")),
        };
        dialog
            .with_line()
            .with_text(Text::Wrap(problem.reason.text.clone()))
            .with_text(Text::Pair { label, value })
    }

    /// `message`, a text field that opens with `text`, check boxes with their labels and
    /// states, and OK and Cancel. An `&` in `message` marks the letter of the field.
    pub(crate) fn form(title: &str, message: &str, text: &str, checks: &[(String, bool)]) -> Self {
        let dialog = Self {
            message: Label::parse(message.trim_end()),
            ..Self::new(title, "", vec![Button::Ok, Button::Cancel])
        }
        .with_part_field(LabelledField::unlabelled(Field::plain(text)));
        dialog.with_checks(checks)
    }

    /// Text fields with their labels and the texts they open with, check boxes with their
    /// labels and states, and `buttons`, the first of them the default.
    pub(crate) fn fields(
        title: &str,
        fields: &[(String, String)],
        checks: &[(String, bool)],
        buttons: Vec<Button>,
    ) -> Self {
        let dialog = fields
            .iter()
            .fold(Self::new(title, "", buttons), |dialog, (label, text)| {
                dialog.with_field(label, text)
            });
        dialog.with_checks(checks)
    }

    /// The same dialog with a text field under the rest, labelled `label`, that opens with
    /// `text`. The first field starts with the focus.
    pub(crate) fn with_field(self, label: &str, text: &str) -> Self {
        self.with_part_field(LabelledField {
            label: Label::parse(&cells::sanitize(label.as_bytes())),
            field: Field::plain(text),
            search: None,
            placeholder: None,
            disabled: false,
        })
    }

    fn with_part_field(mut self, field: LabelledField) -> Self {
        if self.fields.is_empty() {
            self.focus = Focus::Field(0);
        }
        self.parts.push(Part::Field(self.fields.len()));
        self.fields.push(field);
        self
    }

    /// The same dialog with a check box under the rest, labelled `label`, checked if `on`.
    pub(crate) fn with_check(mut self, label: &str, on: bool) -> Self {
        self.parts.push(Part::Check(self.checks.len()));
        self.checks.push(Check {
            label: Label::parse(label),
            on,
        });
        self
    }

    fn with_checks(self, checks: &[(String, bool)]) -> Self {
        checks
            .iter()
            .fold(self, |dialog, (label, on)| dialog.with_check(label, *on))
    }

    /// The same dialog with a blank row under the rest, which sets what is above it apart
    /// from what comes under it.
    pub(crate) fn with_separator(mut self) -> Self {
        self.parts.push(Part::Separator);
        self
    }

    /// The same dialog with a line across the frame under the rest.
    pub(crate) fn with_line(mut self) -> Self {
        self.parts.push(Part::Line);
        self
    }

    /// The same dialog with an item under the rest, labelled `label`, with `detail` on the
    /// right: Enter or Space on it, a click, or Alt and its letter open it
    /// ([`DialogEvent::Opened`]).
    pub(crate) fn with_item(mut self, label: &str, detail: &str) -> Self {
        self.parts.push(Part::Item(self.items.len()));
        self.items.push(Item {
            label: Label::parse(label),
            detail: detail.to_owned(),
        });
        self
    }

    /// The same dialog with `text` under the rest.
    pub(crate) fn with_text(mut self, text: Text) -> Self {
        self.parts.push(Part::Text(self.texts.len()));
        self.texts.push(text);
        self
    }

    /// The same dialog with `picker` under the rest; whoever opens the dialog draws its rows
    /// ([`render_picker`](Self::render_picker)). Without fields or combo boxes, its rows start
    /// with the focus.
    pub(crate) fn with_picker(mut self, picker: Picker) -> Self {
        if self.fields.is_empty() && self.combo_boxes.is_empty() {
            self.focus = Focus::Picker;
        }
        self.parts.push(Part::Picker);
        self.picker = Some(Box::new(picker));
        self
    }

    pub(crate) fn picker(&self) -> Option<&Picker> {
        self.picker.as_deref()
    }

    pub(crate) fn picker_mut(&mut self) -> Option<&mut Picker> {
        self.picker.as_deref_mut()
    }

    /// The cells of a line inside the dialog, drawn in `area`.
    pub(crate) fn text_width(&self, area: Rect) -> usize {
        usize::from(self.box_width(area).saturating_sub(4))
    }

    /// The width of the box drawn in `area`, borders included.
    fn box_width(&self, area: Rect) -> u16 {
        box_width(self.width.max(window::MIN_WIDTH), area)
    }

    /// The same dialog with the focus on its default button.
    pub(crate) fn with_focus_on_default(mut self) -> Self {
        self.focus = Focus::Button(self.default);
        self
    }

    fn new(title: &str, message: &str, buttons: Vec<Button>) -> Self {
        Self {
            id: LAST_ID.fetch_add(1, Ordering::Relaxed) + 1,
            title: cells::sanitize(title.as_bytes()),
            message: Label::plain(message.trim_end()),
            combo_boxes: Vec::new(),
            fields: Vec::new(),
            checks: Vec::new(),
            items: Vec::new(),
            texts: Vec::new(),
            parts: Vec::new(),
            buttons,
            default: 0,
            focus: Focus::Button(0),
            width: window::WIDTH,
            tone: Tone::Plain,
            answers_no: false,
            completes: false,
            field_area: Cell::new(None),
            cursor: Cell::new(None),
            spots: RefCell::new(Vec::new()),
            glyphs: RefCell::new(Vec::new()),
            offset: Cell::new(0),
            page: Cell::new(1),
            bar: Cell::new(None),
            picker: None,
            picker_area: Cell::new(None),
            wait: None,
        }
    }

    /// What the dialog waits for, whose dots turn in front of its title.
    pub(crate) fn wait(&self) -> Option<&Wait> {
        self.wait.as_ref()
    }

    /// The scroll bar of a message that did not fit at the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar.get()
    }

    /// Scrolls a message that does not fit as a press on its scroll bar asks, or the wheel: a
    /// line, a page, or to where the thumb was dragged.
    pub(crate) fn scroll(&mut self, scroll: Scroll) {
        let (offset, page) = (self.offset.get(), self.page.get().max(1));
        self.offset.set(match scroll {
            Scroll::By(Motion::Up) => offset.saturating_sub(1),
            Scroll::By(Motion::Down) => offset + 1,
            Scroll::By(Motion::PageUp) => offset.saturating_sub(page),
            Scroll::By(Motion::PageDown) => offset + page,
            Scroll::By(_) => offset,
            Scroll::To(top) => top,
        });
    }

    /// Tells this dialog from every other one.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// The same dialog, `width` cells wide where the screen has room.
    #[cfg(test)]
    pub(crate) fn with_width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    /// The same dialog, with a path in its first field, which Tab completes.
    pub(crate) fn with_completion(mut self) -> Self {
        self.completes = true;
        self
    }

    /// The same dialog, with the cursor of its first field before character `cursor`.
    pub(crate) fn with_cursor(mut self, cursor: usize) -> Self {
        if let Some(first) = self.fields.first_mut() {
            first.field.editor.set_cursor(cursor);
        }
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
        self.message = Label::plain(message.trim_end());
        self
    }

    /// The same dialog with a combo box of `items` under the rest, labelled `label`, of which
    /// `chosen` is chosen. Without fields, its first combo box starts with the focus.
    pub(crate) fn with_combo_box(mut self, label: &str, items: &[String], chosen: usize) -> Self {
        if self.fields.is_empty() && self.combo_boxes.is_empty() {
            self.focus = Focus::ComboBox(0);
        }
        self.parts.push(Part::ComboBox(self.combo_boxes.len()));
        self.combo_boxes.push(LabelledComboBox {
            label: Label::parse(&cells::sanitize(label.as_bytes())),
            combo: ComboBox::new(items, chosen),
        });
        self
    }

    /// The same dialog, whose field `index` shows, faint and empty but for its placeholder, but
    /// cannot be used; the focus on it goes to the first stop.
    pub(crate) fn with_disabled(mut self, index: usize) -> Self {
        if let Some(field) = self.fields.get_mut(index) {
            field.disabled = true;
        }
        if self.focus == Focus::Field(index)
            && let Some(&first) = self.stops().first()
        {
            self.focus = first;
        }
        self
    }

    /// Puts `buttons` in place of the buttons, the first the default; the focus on one of them
    /// stays on one.
    pub(crate) fn set_buttons(&mut self, buttons: Vec<Button>) {
        let last = buttons.len().saturating_sub(1);
        self.buttons = buttons;
        self.default = self.default.min(last);
        if let Focus::Button(index) = &mut self.focus {
            *index = (*index).min(last);
        }
    }

    /// The index of the choice of combo box `index`.
    pub(crate) fn chosen(&self, index: usize) -> usize {
        self.combo_boxes
            .get(index)
            .map_or(0, |labelled| labelled.combo.chosen())
    }

    /// The combo box whose list is open, which has the keys.
    pub(crate) fn open_combo_box(&self) -> Option<&ComboBox> {
        let Focus::ComboBox(index) = self.focus else {
            return None;
        };
        let labelled = self.combo_boxes.get(index)?;
        labelled.combo.is_open().then_some(&labelled.combo)
    }

    pub(crate) fn open_combo_box_mut(&mut self) -> Option<&mut ComboBox> {
        let Focus::ComboBox(index) = self.focus else {
            return None;
        };
        let combo = &mut self.combo_boxes.get_mut(index)?.combo;
        combo.is_open().then_some(combo)
    }

    /// Whether the list of a combo box is open, and has the keys.
    pub(crate) fn combo_box_open(&self) -> bool {
        self.open_combo_box().is_some()
    }

    /// The answer for ssh after `event`: the secret for OK on a secret prompt, `yes` for Yes,
    /// `no` for No on a question of a server, and `None` to decline.
    pub(crate) fn answer(&self, event: DialogEvent) -> Option<SecretString> {
        match (event, self.fields.first()) {
            (DialogEvent::Pressed(Button::Ok), Some(LabelledField { field, .. })) => {
                // From `&str`: an exact copy, where `String` would be shrunk into new memory.
                Some(SecretString::from(field.text()))
            }
            (DialogEvent::Pressed(Button::Yes), _) => Some(SecretString::from("yes")),
            (DialogEvent::Pressed(Button::No), _) if self.answers_no => {
                Some(SecretString::from("no"))
            }
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

    /// The same dialog, whose field `index` shows `text`, faint, while it is empty.
    pub(crate) fn with_placeholder(mut self, index: usize, text: &str) -> Self {
        if let Some(field) = self.fields.get_mut(index) {
            field.placeholder = Some(cells::sanitize(text.as_bytes()));
        }
        self
    }

    /// The same dialog, whose field `index` searches as `input` says, starting in its way.
    pub(crate) fn with_search(mut self, index: usize, input: Input) -> Self {
        self.set_search(index, input);
        self
    }

    /// Field `index` searches as `input` says from now on.
    pub(crate) fn set_search(&mut self, index: usize, input: Input) {
        if let Some(field) = self.fields.get_mut(index) {
            field.search = Some(input);
        }
    }

    /// How field `index` matches, and whether case counts, if it searches.
    pub(crate) fn search(&self, index: usize) -> Option<Input> {
        self.fields.get(index)?.search
    }

    /// The kind of the field with the focus if it searches, or of the filter of the picker
    /// while it takes the keys, whose keys after Ctrl+s go in front of those of the dialog.
    pub(crate) fn search_focused(&self) -> Option<Kind> {
        match self.focus {
            Focus::Field(index) => Some(self.fields.get(index)?.search?.kind()),
            Focus::Picker => self.picker.as_ref()?.search(),
            Focus::ComboBox(_) | Focus::Check(_) | Focus::Item(_) | Focus::Button(_) => None,
        }
    }

    /// Switches how field `index` matches, or whether case counts, as `switch` says, as the keys
    /// after Ctrl+s do in it.
    fn switch_search(&mut self, index: usize, switch: Switch) {
        if let Some(input) = self
            .fields
            .get_mut(index)
            .and_then(|field| field.search.as_mut())
        {
            input.switch(switch);
        }
    }

    /// Whether check box `index` is checked.
    pub(crate) fn checked(&self, index: usize) -> bool {
        self.checks.get(index).is_some_and(|check| check.on)
    }

    /// The keymap context for the next key: `DialogInput` while a text field has the focus,
    /// `PathInput` in a field that completes paths, and `ComboBox` while the list of a combo
    /// box is open.
    pub(crate) fn context(&self) -> Context {
        match self.focus {
            Focus::Field(0) if self.completes => Context::PathInput,
            Focus::Field(_) => Context::DialogInput,
            Focus::ComboBox(_) if self.combo_box_open() => Context::ComboBox,
            Focus::ComboBox(_)
            | Focus::Check(_)
            | Focus::Item(_)
            | Focus::Picker
            | Focus::Button(_) => Context::Dialog,
        }
    }

    /// The mode keys are typed in: the focused field's or picker's, or Normal mode on a button,
    /// a check box, or a combo box.
    pub(crate) fn mode(&self) -> Mode {
        match (self.focus, self.focused_field(), &self.picker) {
            (_, Some(field), _) => field.mode(),
            (Focus::Picker, None, Some(picker)) => picker.mode(),
            _ => Mode::Normal,
        }
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

    /// Lets the focused field go: what Visual mode selects there stops showing. The filter of
    /// the picker with the focus stops taking what is typed.
    fn leave_field(&mut self) {
        if self.focus == Focus::Picker
            && let Some(picker) = &mut self.picker
            && picker.mode() == Mode::Insert
        {
            picker.handle(Resolved::Action(Action::NormalMode));
        }
        if let Some(field) = self.focused_field_mut()
            && field.mode() == Mode::Visual
        {
            field.set_mode(Mode::Normal);
        }
    }

    pub(crate) fn focused_field(&self) -> Option<&Field> {
        match self.focus {
            Focus::Field(index) => self.fields.get(index).map(|field| &field.field),
            Focus::ComboBox(_)
            | Focus::Check(_)
            | Focus::Item(_)
            | Focus::Picker
            | Focus::Button(_) => None,
        }
    }

    fn focused_field_mut(&mut self) -> Option<&mut Field> {
        match self.focus {
            Focus::Field(index) => self.fields.get_mut(index).map(|field| &mut field.field),
            Focus::ComboBox(_)
            | Focus::Check(_)
            | Focus::Item(_)
            | Focus::Picker
            | Focus::Button(_) => None,
        }
    }

    /// What it says, which its colors show.
    #[cfg(test)]
    pub(crate) fn tone(&self) -> Tone {
        self.tone
    }
}
