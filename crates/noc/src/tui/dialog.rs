//! Modal dialogs: prompts from ssh (passwords and passphrases, host keys, confirmations, and
//! notices), the app's own questions, and the search of a whole tree, whose dialog has a list.
//! As in mc and Far, Alt and the letter that an `&` marks in a label act on what it labels:
//! press a button, switch a check box, open a combo box, or put the focus in a field.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use noc_scroll::Motion;
use noc_scroll::scrollbar::{self, Bar};
use noc_ssh::askpass::PromptKind;
use ratatui::Frame;
use ratatui::layout::{Margin, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use secrecy::SecretString;
use unicode_width::UnicodeWidthChar as _;

use super::cells;
use super::combo_box::{ComboBox, ComboEvent};
use super::edit::{Editor, Registers};
use super::glyphs;
use super::keymap::{Action, Context, Extra, Mode, Resolved, Typed};
use super::label::{self, Label};
use super::mouse::{Pointer, Press, Scroll};
use super::scroll::Window;
use super::search::{GlyphSpots, Input, Kind, Switch};
use super::spinner::Shine;
use super::theme::{self, Theme};
use crate::i18n::fl;

/// Bytes a secret may have. Reserved up front, so typing never moves it in memory and leaves
/// no copy behind.
const SECRET_CAPACITY: usize = 1024;
/// Width of a dialog, in cells, borders included, where the screen has room; `with_width`
/// sets another.
pub(crate) const MAX_WIDTH: u16 = 76;
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
    /// Go to the row of the list, as in mc's results of Find File; or to a path typed.
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
            Self::SkipAll => fl!("dialog-skip-all"),
            Self::Retry => fl!("dialog-retry"),
            Self::Abort => fl!("dialog-abort"),
            Self::All => fl!("dialog-all"),
            Self::KeepAll => fl!("dialog-none"),
            Self::Older => fl!("dialog-older"),
            Self::UseCurrent => fl!("dialog-use-current"),
            Self::GoTo => fl!("dialog-go-to"),
            Self::View => fl!("dialog-view"),
            Self::Edit => fl!("dialog-edit"),
            Self::Up => fl!("dialog-up"),
            Self::Connect => fl!("dialog-connect"),
            Self::Local => fl!("dialog-local"),
        }
    }
}

/// Where the keys go inside a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    ComboBox(usize),
    Field(usize),
    Check(usize),
    List,
    Button(usize),
}

/// A text field, in vim's modes: a plain one opens in Insert mode, with the cursor at the end
/// of its text. A secret one stays in Insert mode, is masked, and lives in memory reserved up
/// front and wiped when the field goes.
pub(crate) struct Field {
    editor: Editor,
    /// The first character the last render showed: the text stays where it is while the
    /// cursor shows, as a vim window's `leftcol`.
    first: Cell<usize>,
}

impl Field {
    fn secret() -> Self {
        Self {
            editor: Editor::secret(SECRET_CAPACITY),
            first: Cell::new(0),
        }
    }

    /// A plain field that opens in Insert mode with `text`, the cursor at its end.
    pub(crate) fn plain(text: &str) -> Self {
        Self {
            editor: Editor::new(text, Mode::Insert),
            first: Cell::new(0),
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
            first: Cell::new(0),
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

    /// The first character shown, what fits from there, and the cursor's column. The text
    /// shown stays while the cursor shows; once it would not, the cursor goes to the middle,
    /// as vim's 'sidescroll' 0 has it.
    fn window(&self, room: usize) -> (usize, String, usize) {
        let chars: Vec<char> = if self.editor.is_secret() {
            vec![glyphs::SECRET; self.editor.text().chars().count()]
        } else {
            self.editor.text().chars().collect()
        };
        let cursor = self.editor.cursor().min(chars.len());
        let width = |c: char| c.width().unwrap_or(0);
        let cells = |range: Range<usize>| chars[range].iter().map(|&c| width(c)).sum::<usize>();
        let first = self.first.get().min(chars.len());
        let (left, start) = (cells(0..first), cells(0..cursor));
        let end = start + chars.get(cursor).map_or(1, |&c| width(c).max(1));
        let first = if start < left || end > left + room {
            let target = start.saturating_sub(room / 2);
            (0..=cursor)
                .find(|&index| cells(0..index) >= target)
                .unwrap_or(cursor)
        } else {
            first
        };
        self.first.set(first);
        let column = cells(first..cursor);
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
    label: Label,
    field: Field,
    /// How it matches, if it searches: the glyphs of its case and its way end it, and the keys
    /// after Ctrl+s or a click on a glyph switch them while it has the focus (ADR 0045).
    search: Option<Input>,
    /// What shows, faint, while the field is empty, such as the value that then counts; it is
    /// never the field's text. Terminal-safe. Fields that search show none.
    placeholder: Option<String>,
    /// Shown where it cannot be used: its label faint, the field empty and faint; the focus,
    /// the mouse, and its letter pass it by.
    disabled: bool,
}

impl LabelledField {
    fn unlabelled(field: Field) -> Self {
        Self {
            label: Label::default(),
            field,
            search: None,
            placeholder: None,
            disabled: false,
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
    /// The marks of text cut short, over `body`.
    mark: Style,
    /// The letters of labels, over the rest, and over what has the focus.
    hotkey: Style,
    hotkey_focused: Style,
    /// The brackets of check boxes, and their checks, over `body`.
    bracket: Style,
    symbol: Style,
}

impl Colors {
    /// The style of the button with the focus.
    pub(crate) fn focused_style(self) -> Style {
        self.focused
    }

    /// The style of the letters of labels, over the others: over what has the focus if
    /// `focused`, where the focus's colors may hide those of the others.
    pub(crate) fn hotkey_style(self, focused: bool) -> Style {
        if focused {
            self.hotkey_focused
        } else {
            self.hotkey
        }
    }

    /// The same colors, for what cannot be used: the body faint, its letters too.
    fn disabled(self, theme: &Theme) -> Self {
        let faint = self.body.patch(theme.disabled);
        Self {
            body: faint,
            hotkey: faint,
            ..self
        }
    }

    /// The body's style, and that of the marks of text cut short over it.
    pub(crate) fn body_styles(self) -> (Style, Style) {
        (self.body, self.mark)
    }

    /// A check box, `[✓]` while `on` and `[ ]` else, in the body's colors, its brackets faint
    /// and its check in the color of titles; or, `focused`, in the colors of the focus alone.
    pub(crate) fn check_box(self, on: bool, focused: bool, theme: &Theme) -> Vec<Span<'static>> {
        let (bracket, symbol) = if focused {
            (self.focused, self.focused)
        } else {
            (self.body.patch(self.bracket), self.body.patch(self.symbol))
        };
        let check = if on { theme.glyphs.check } else { ' ' };
        vec![
            Span::styled("[", bracket),
            Span::styled(check.to_string(), symbol),
            Span::styled("]", bracket),
        ]
    }

    /// The colors of a dialog in `tone`: errors red, as mc draws them, and warnings on a
    /// background of their own. Only plain dialogs set the brackets and checks of check boxes
    /// apart, whose colors are for theirs.
    pub(crate) fn of(theme: &Theme, tone: Tone) -> Self {
        match tone {
            Tone::Plain => Self {
                body: theme.dialog,
                title: theme.dialog_title,
                button: theme.dialog_button,
                focused: theme.dialog_button_focused,
                mark: theme.truncated_dialog,
                hotkey: theme.dialog_hotkey,
                hotkey_focused: theme.dialog_hotkey_focused,
                bracket: theme.dialog_bracket,
                symbol: theme.dialog_symbol,
            },
            Tone::Warning => Self {
                body: theme.warning_dialog,
                title: theme.warning_title,
                button: theme.warning_dialog,
                focused: theme.warning_button_focused,
                mark: theme.truncated_warning,
                hotkey: theme.warning_hotkey,
                hotkey_focused: theme.warning_hotkey_focused,
                bracket: Style::new(),
                symbol: Style::new(),
            },
            Tone::Error => Self {
                body: theme.error_dialog,
                title: theme.error_title,
                button: theme.error_dialog,
                focused: theme.error_button_focused,
                mark: theme.truncated_error,
                hotkey: theme.error_hotkey,
                hotkey_focused: theme.error_hotkey_focused,
                bracket: Style::new(),
                symbol: Style::new(),
            },
        }
    }
}

/// What a dialog says, which its colors show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    /// Anything else.
    Plain,
    /// Something went wrong.
    Error,
    /// A question before something that cannot be undone, such as deleting or writing over a
    /// file.
    Warning,
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

/// A part of a dialog under its message, by its index among those of its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    ComboBox(usize),
    Field(usize),
    Check(usize),
    /// A blank row that sets what is above it apart from what is under it.
    Separator,
}

/// Rows under the check boxes, which whoever opened the dialog draws at each render
/// ([`ListView`]); the dialog moves the cursor over them and scrolls them as vim's window.
#[derive(Debug, Default)]
struct List {
    len: usize,
    cursor: Cell<usize>,
    window: Cell<Window>,
    /// How many rows showed at the last render.
    page: Cell<usize>,
    /// Where each row that showed was drawn, by its index, for the mouse.
    rows: RefCell<Vec<(usize, Rect)>>,
    bar: Cell<Option<Bar>>,
}

/// What the list of a dialog shows at a render.
pub(crate) struct ListView<'a> {
    /// Draws the row at an index in so many cells, in a style: the dialog's, or the cursor's.
    pub(crate) row: &'a dyn Fn(usize, usize, Style) -> Line<'static>,
    /// Shown in place of the rows, such as why there are none.
    pub(crate) note: Option<String>,
    /// Lines under the rows, apart from them, cut at their end where too long.
    pub(crate) status: Vec<String>,
    /// Whether the first of them is still to come: a skeleton stands in its place, crossed by
    /// the shine.
    pub(crate) counting: bool,
    /// Turning in front of the title while the list waits for more, in their color.
    pub(crate) dots: Option<(char, Style)>,
    /// Whether the first rows are still to come: a skeleton stands in their place.
    pub(crate) waiting: bool,
    /// The shine that crosses the skeleton.
    pub(crate) shine: Option<Shine>,
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

/// A modal dialog: a message, combo boxes, text fields, check boxes, and buttons, each of them
/// optional but the buttons. Whoever opens it reads the choices of the combo boxes, the fields,
/// and the check boxes once it closes.
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
    /// The combo boxes, the fields, the check boxes, and the separators under the message, top
    /// to bottom, in the order they were added.
    parts: Vec<Part>,
    buttons: Vec<Button>,
    /// The button that Enter in the field activates, and that starts with the focus otherwise;
    /// drawn as `[< … >]`, as in mc.
    default: usize,
    focus: Focus,
    /// Widest the dialog gets, in cells, borders included.
    width: u16,
    tone: Tone,
    /// Its first field holds a path, which Tab completes.
    completes: bool,
    /// Where its first field was drawn last, for the list of completions under it.
    field_area: Cell<Option<Rect>>,
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
    list: Option<List>,
}

impl Dialog {
    /// A dialog for a prompt of `kind`: a masked field with OK and Cancel for secrets, Yes and
    /// No (with the focus) for questions. `context` is the host alias.
    pub(crate) fn prompt(context: &str, message: &str, kind: PromptKind) -> Self {
        match kind {
            PromptKind::Secret => Self::new(context, message, vec![Button::Ok, Button::Cancel])
                .with_part_field(LabelledField::unlabelled(Field::secret())),
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

    /// Something that went wrong, titled Error, with OK.
    pub(crate) fn error(message: &str) -> Self {
        Self::failure(message, vec![Button::Ok], 0)
    }

    /// Something that went wrong, titled Error, with `buttons` for what to do now, of which
    /// `default` has the focus.
    pub(crate) fn failure(message: &str, buttons: Vec<Button>, default: usize) -> Self {
        Self::question(&fl!("dialog-error"), message, buttons, default, Tone::Error)
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

    fn new(title: &str, message: &str, buttons: Vec<Button>) -> Self {
        Self {
            id: LAST_ID.fetch_add(1, Ordering::Relaxed) + 1,
            title: cells::sanitize(title.as_bytes()),
            message: Label::plain(message.trim_end()),
            combo_boxes: Vec::new(),
            fields: Vec::new(),
            checks: Vec::new(),
            parts: Vec::new(),
            buttons,
            default: 0,
            focus: Focus::Button(0),
            width: MAX_WIDTH,
            tone: Tone::Plain,
            completes: false,
            field_area: Cell::new(None),
            spots: RefCell::new(Vec::new()),
            glyphs: RefCell::new(Vec::new()),
            offset: Cell::new(0),
            page: Cell::new(1),
            bar: Cell::new(None),
            list: None,
        }
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

    /// The same dialog with a list under its check boxes, as tall as the room lets it be.
    pub(crate) fn with_list(mut self) -> Self {
        self.list = Some(List::default());
        self
    }

    /// Makes the list `len` rows long, keeping the cursor inside.
    pub(crate) fn set_list_len(&mut self, len: usize) {
        if let Some(list) = &mut self.list {
            list.len = len;
            list.cursor
                .set(list.cursor.get().min(len.saturating_sub(1)));
        }
    }

    /// The row of the list under the cursor.
    pub(crate) fn list_cursor(&self) -> usize {
        self.list.as_ref().map_or(0, |list| list.cursor.get())
    }

    /// Puts the cursor of the list on row `index`, where it stays as rows come in before it.
    pub(crate) fn set_list_cursor(&mut self, index: usize) {
        if let Some(list) = &mut self.list {
            list.cursor.set(index.min(list.len.saturating_sub(1)));
        }
    }

    /// Shows the list from its cursor again at the next render, as for a new listing.
    pub(crate) fn unsettle_list(&mut self) {
        if let Some(list) = &mut self.list {
            list.window.get_mut().unsettle();
        }
    }

    /// The scroll bar of the list at the last render, for the mouse.
    pub(crate) fn list_bar(&self) -> Option<Bar> {
        self.list.as_ref().and_then(|list| list.bar.get())
    }

    /// Shows the rows of the list from `top`, where the thumb of its scroll bar was dragged.
    pub(crate) fn scroll_list_to(&mut self, top: usize) {
        self.with_list_window(|window, cursor, len, page| {
            window.scroll_to(top, cursor, len, page);
        });
    }

    /// A step of the wheel over the list: Up and Down scroll it `count` rows, keeping the
    /// cursor inside, and the page keys move it a page.
    pub(crate) fn wheel_list(&mut self, action: Action, count: usize) {
        self.with_list_window(|window, cursor, len, page| match action {
            Action::Up | Action::Down => {
                window.scroll(action == Action::Down, count, cursor, len, page);
            }
            Action::PageUp | Action::PageDown => {
                window.page(action == Action::PageDown, count, cursor, len, page);
            }
            _ => {}
        });
    }

    /// Runs `f` on the window of the list, its cursor, its length, and its page.
    fn with_list_window(&mut self, f: impl FnOnce(&mut Window, &mut usize, usize, usize)) {
        let Some(list) = &mut self.list else {
            return;
        };
        let (len, page) = (list.len, list.page.get().max(1));
        f(list.window.get_mut(), list.cursor.get_mut(), len, page);
    }

    /// Moves the cursor of the list as `action` says, as many rows or pages as `extra`
    /// counts; whether `action` moves it.
    fn move_in_list(&mut self, action: Action, extra: Extra) -> bool {
        let times = extra.times();
        let last = self
            .list
            .as_ref()
            .map_or(0, |list| list.len.saturating_sub(1));
        let mut moves = true;
        self.with_list_window(|window, cursor, len, page| match action {
            Action::Up => *cursor = cursor.saturating_sub(times),
            Action::Down => *cursor = cursor.saturating_add(times).min(last),
            Action::Home => *cursor = 0,
            Action::End => *cursor = last,
            Action::PageUp | Action::PageDown => {
                window.page(action == Action::PageDown, times, cursor, len, page);
            }
            Action::HalfPageUp | Action::HalfPageDown => {
                window.half_page(action == Action::HalfPageDown, cursor, len, page);
            }
            _ => moves = false,
        });
        moves && self.list.is_some()
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

    /// The same dialog, whose field `index` shows, empty and faint, but cannot be used; the
    /// focus on it goes to the first stop.
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

    /// The kind of the field with the focus if it searches, whose keys after Ctrl+s go in front
    /// of those of the dialog.
    pub(crate) fn search_focused(&self) -> Option<Kind> {
        match self.focus {
            Focus::Field(index) => Some(self.fields.get(index)?.search?.kind()),
            Focus::ComboBox(_) | Focus::Check(_) | Focus::List | Focus::Button(_) => None,
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
            Focus::ComboBox(_) | Focus::Check(_) | Focus::List | Focus::Button(_) => {
                Context::Dialog
            }
        }
    }

    /// The mode keys are typed in: the focused field's, or Normal mode on a button, a check
    /// box, or a combo box.
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
            Focus::ComboBox(_) | Focus::Check(_) | Focus::List | Focus::Button(_) => None,
        }
    }

    fn focused_field_mut(&mut self) -> Option<&mut Field> {
        match self.focus {
            Focus::Field(index) => self.fields.get_mut(index).map(|field| &mut field.field),
            Focus::ComboBox(_) | Focus::Check(_) | Focus::List | Focus::Button(_) => None,
        }
    }

    /// Handles an action or a typed character, as typed without a count or a register.
    #[cfg(test)]
    pub(crate) fn handle(&mut self, input: Resolved) -> DialogEvent {
        self.handle_typed(Typed::from(input), &mut Registers::default())
    }

    /// Handles an action or a typed character: the focused field takes what it edits with,
    /// in its mode. Tab brings the focus to a field in Insert mode; the other keys that move it
    /// bring it there in Normal mode, but from a field in Insert mode. The open list of a combo
    /// box takes every key, Alt and a letter too, until it closes.
    pub(crate) fn handle_typed(&mut self, typed: Typed, registers: &mut Registers) -> DialogEvent {
        let Typed { resolved, extra } = typed;
        if let Some(combo) = self.open_combo_box_mut() {
            match resolved {
                Resolved::Action(action) => combo.handle(action, extra),
                Resolved::Hotkey(c) => combo.hotkey(c),
                Resolved::Insert(_) => ComboEvent::Pending,
            };
            return DialogEvent::Pending;
        }
        if let Resolved::Hotkey(c) = resolved {
            return self.hotkey(c);
        }
        let action = match (resolved, self.focused_field_mut()) {
            (Resolved::Insert(c), Some(field)) => {
                field.insert(c);
                return DialogEvent::Pending;
            }
            (Resolved::Hotkey(_), _) | (Resolved::Insert(_), None) => {
                return DialogEvent::Pending;
            }
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
        // The keys that move the focus move the cursor of the list, which Tab leaves.
        if self.focus == Focus::List && self.move_in_list(action, extra) {
            return DialogEvent::Pending;
        }
        if let Some(switch) = Switch::of(action) {
            if let Focus::Field(index) = self.focus {
                self.switch_search(index, switch);
            }
            return DialogEvent::Pending;
        }
        match action {
            Action::Confirm => match self.focus {
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::ComboBox(_) | Focus::Field(_) | Focus::Check(_) | Focus::List => {
                    DialogEvent::Pressed(self.buttons[self.default])
                }
            },
            Action::Toggle => match self.focus {
                Focus::ComboBox(index) => {
                    self.combo_boxes[index].combo.open();
                    DialogEvent::Pending
                }
                Focus::Check(index) => {
                    self.checks[index].on = !self.checks[index].on;
                    DialogEvent::Pending
                }
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::Field(_) | Focus::List => DialogEvent::Pending,
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

    /// Alt and `c`, as mc and Far take them: a button with the letter is pressed, a check box
    /// switched, and the list of a combo box opened, each with the focus; a field with it, or
    /// under a prompt with it, gets the focus in Insert mode. Nothing else has it.
    fn hotkey(&mut self, c: char) -> DialogEvent {
        let usable = self.fields.first().is_some_and(|field| !field.disabled);
        let found = if self.message.has(c) && usable {
            Some(Focus::Field(0))
        } else {
            self.stops().into_iter().find(|&stop| match stop {
                Focus::ComboBox(index) => self.combo_boxes[index].label.has(c),
                Focus::Field(index) => self.fields[index].label.has(c),
                Focus::Check(index) => self.checks[index].label.has(c),
                Focus::List => false,
                Focus::Button(index) => self.buttons[index].label().has(c),
            })
        };
        let Some(stop) = found else {
            return DialogEvent::Pending;
        };
        if let Focus::Button(index) = stop {
            return DialogEvent::Pressed(self.buttons[index]);
        }
        self.give_up();
        self.focus_on(stop, Mode::Insert);
        match stop {
            Focus::ComboBox(index) => self.combo_boxes[index].combo.open(),
            Focus::Check(index) => self.checks[index].on = !self.checks[index].on,
            Focus::Field(_) | Focus::List | Focus::Button(_) => {}
        }
        DialogEvent::Pending
    }

    /// The letters of everything Alt and a letter act on, the prompt of a lone field first, in
    /// the order of the stops; one for each that has a letter.
    #[cfg(test)]
    pub(crate) fn letters(&self) -> Vec<char> {
        let prompt = (!self.fields.is_empty()).then(|| self.message.letter());
        let stops = self.stops().into_iter().map(|stop| match stop {
            Focus::ComboBox(index) => self.combo_boxes[index].label.letter(),
            Focus::Field(index) => self.fields[index].label.letter(),
            Focus::Check(index) => self.checks[index].label.letter(),
            Focus::List => None,
            Focus::Button(index) => self.buttons[index].label().letter(),
        });
        prompt.into_iter().chain(stops).flatten().collect()
    }

    /// Takes a press of the mouse, where the dialog was drawn last: a click opens the list of a
    /// combo box, puts the cursor in a field or on a row of the list, switches a check box, or
    /// presses a button. A double click on a row presses the default button too, as Enter does
    /// there. A click on a glyph at the end of a field that searches gives it the focus and
    /// switches whether case counts, or how it matches, as the keys after Ctrl+s do there. The
    /// open list of a combo box takes every press, and the second click of a double click that
    /// closed it does nothing.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> DialogEvent {
        let mut spent = false;
        for labelled in &mut self.combo_boxes {
            spent |= labelled.combo.spent(pointer);
        }
        if let Some(combo) = self.open_combo_box_mut() {
            combo.pointer(pointer);
            return DialogEvent::Pending;
        }
        if spent {
            return DialogEvent::Pending;
        }
        let Pointer { press, at } = pointer;
        let glyph = self
            .glyphs
            .borrow()
            .iter()
            .find_map(|(index, drawn)| Some((*index, drawn.action_at(at)?)));
        if let Some((index, action)) = glyph {
            if press == Press::Click {
                if self.focus != Focus::Field(index) {
                    self.focus_on(Focus::Field(index), Mode::Insert);
                }
                if let Some(switch) = Switch::of(action) {
                    self.switch_search(index, switch);
                }
            }
            return DialogEvent::Pending;
        }
        let row = self.list.as_ref().and_then(|list| {
            let rows = list.rows.borrow();
            rows.iter()
                .find(|(_, area)| area.contains(at))
                .map(|(row, _)| *row)
        });
        if let Some(row) = row
            && matches!(press, Press::Click | Press::DoubleClick)
        {
            if self.focus != Focus::List {
                self.leave_field();
                self.focus = Focus::List;
            }
            self.set_list_cursor(row);
            if press == Press::DoubleClick {
                return DialogEvent::Pressed(self.buttons[self.default]);
            }
            return DialogEvent::Pending;
        }
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
            Focus::ComboBox(index) => self.combo_boxes[index].combo.open(),
            Focus::Field(index) => {
                if let Some(LabelledField { field, .. }) = self.fields.get_mut(index) {
                    let column = usize::from(at.x - area.x);
                    field.click(column, usize::from(area.width));
                }
            }
            Focus::Check(index) => self.checks[index].on = !self.checks[index].on,
            Focus::List => {}
            // The first click pressed it.
            Focus::Button(_) if press == Press::DoubleClick => {}
            Focus::Button(index) => return DialogEvent::Pressed(self.buttons[index]),
        }
        DialogEvent::Pending
    }

    /// Where the focus can be: the combo boxes, the fields that are not disabled, and the check
    /// boxes, top to bottom, then the list and the buttons.
    fn stops(&self) -> Vec<Focus> {
        self.parts
            .iter()
            .filter_map(|&part| match part {
                Part::ComboBox(index) => Some(Focus::ComboBox(index)),
                Part::Field(index) if !self.fields[index].disabled => Some(Focus::Field(index)),
                Part::Check(index) => Some(Focus::Check(index)),
                Part::Field(_) | Part::Separator => None,
            })
            .chain(self.list.as_ref().map(|_| Focus::List))
            .chain((0..self.buttons.len()).map(Focus::Button))
            .collect()
    }

    /// Moves the focus through the stops, round; a field it comes to is in `mode`, Insert or
    /// Normal.
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

    /// Draws the lines of the message at the top of `inner`, as many as the `rest` of the
    /// dialog leaves room for, scrolled, with a scroll bar on the right of the frame in `style`
    /// when they do not all fit. Returns how many rows they took.
    fn render_message(
        &self,
        frame: &mut Frame<'_>,
        inner: Rect,
        (lines, letter): (&[String], Option<(usize, usize)>),
        rest: u16,
        colors: Colors,
        theme: &Theme,
    ) -> u16 {
        let style = colors.body;
        let room = usize::from(inner.height.saturating_sub(rest)).max(1);
        let page = lines.len().min(room);
        let offset = self.offset.get().min(lines.len() - page);
        self.offset.set(offset);
        self.page.set(page);
        let rows = u16::try_from(page).unwrap_or(u16::MAX).min(inner.height);
        let bar = right_frame(inner, inner.y, rows);
        let position = (lines.len(), page, offset);
        let arrows = theme.glyphs.scroll_bar;
        self.bar
            .set(scrollbar::render(frame, bar, position, style, arrows));
        for (index, line) in (0..rows).zip(lines.iter().enumerate().skip(offset)) {
            let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
            let (number, text) = line;
            let over = letter
                .filter(|&(at, _)| at == number)
                .map(|(_, column)| (column, colors.hotkey_style(false)));
            let spans = label::spans_over(text, style, over.into_iter().collect());
            frame.render_widget(Line::from(spans), row);
        }
        rows
    }

    /// Draws the dialog centered in `area`, with the terminal cursor in the focused field.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        self.render_with(frame, area, theme, None);
    }

    /// Draws the dialog as [`render`](Self::render) does, as tall as `area`, with its list
    /// as `list` shows it.
    pub(crate) fn render_list(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        list: &ListView<'_>,
    ) {
        self.render_with(frame, area, theme, Some(list));
    }

    /// Draws the dialog, then over it the open list of a combo box, which may go past its
    /// frame, within `area`.
    fn render_with(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        list: Option<&ListView<'_>>,
    ) {
        self.draw_parts(frame, area, theme, list);
        if let Some(combo) = self.open_combo_box() {
            combo.render_list(frame, area, theme);
        }
    }

    fn draw_parts(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        list: Option<&ListView<'_>>,
    ) {
        let width = self.width.min(area.width.saturating_sub(4)).max(20);
        let text_width = usize::from(width.saturating_sub(4));
        let (lines, letter) = if self.fields.is_empty() {
            (cells::wrap(&self.message.text, text_width), None)
        } else {
            self.message.wrap(text_width)
        };
        let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
        // Borders, the message, the parts under it, a line, the buttons.
        let height = self.parts.iter().fold(rows(lines.len()), |height, &part| {
            height.saturating_add(self.part_rows(part))
        });
        // A line, a row at least, and a line over the status, under the rest.
        let below = list.map_or(0, |list| match list.status.len() {
            0 => 2,
            status => rows(status).saturating_add(3),
        });
        let colors = Colors::of(theme, self.tone);
        let size = if list.is_some() {
            (width, u16::MAX)
        } else {
            (width, height + 4)
        };
        let inner = draw_box(frame, area, size, &self.title, colors, theme);
        if let Some(dots) = list.and_then(|list| list.dots) {
            draw_title_dots(frame, inner, &self.title, dots, colors);
        }
        let row = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let mut spots = self.spots.borrow_mut();
        spots.clear();
        self.glyphs.borrow_mut().clear();
        // The line and the buttons below the rest.
        let rest = height
            .saturating_sub(rows(lines.len()))
            .saturating_add(2)
            .saturating_add(below);
        let message = (lines.as_slice(), letter);
        let mut index = self.render_message(frame, inner, message, rest, colors, theme);
        let Some(after) = self.draw_body(frame, (inner, index), (colors, theme), &mut spots) else {
            return;
        };
        index = after;
        if let Some(view) = list {
            let Some(end) = self.draw_list_part(frame, (inner, index), view, colors, theme) else {
                return;
            };
            index = end;
        }
        if index + 1 < inner.height {
            draw_separator(frame, inner, inner.y + index, colors, theme);
            let labels: Vec<Label> = self.buttons.iter().map(|button| button.label()).collect();
            let focus = match self.focus {
                Focus::Button(index) => Some(index),
                Focus::ComboBox(_) | Focus::Field(_) | Focus::Check(_) | Focus::List => None,
            };
            let line = button_line(&labels, self.default, focus, colors);
            let buttons = button_spots(&labels, self.default, row(index + 1));
            spots.extend((0..).map(Focus::Button).zip(buttons));
            frame.render_widget(line, row(index + 1));
        }
    }
}

impl Dialog {
    /// The rows `part` takes: a field's label takes one of its own.
    fn part_rows(&self, part: Part) -> u16 {
        match part {
            Part::Field(index) if !self.fields[index].label.text.is_empty() => 2,
            Part::ComboBox(_) | Part::Field(_) | Part::Check(_) | Part::Separator => 1,
        }
    }

    /// Draws the parts under the message from row `index` of `inner`, the room inside the
    /// frame, top to bottom, a separator as a blank row; and keeps where each that takes the
    /// focus is for the mouse in `spots`. Returns the row after them, or `None` without room.
    fn draw_body(
        &self,
        frame: &mut Frame<'_>,
        (inner, mut index): (Rect, u16),
        style: (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
    ) -> Option<u16> {
        // The boxes line up a blank after the widest label.
        let room = usize::from(inner.width);
        let widest = self
            .combo_boxes
            .iter()
            .map(|labelled| cells::width(&labelled.label.text))
            .max()
            .unwrap_or(0);
        let label_width = (widest + 1).min(room / 2);
        for labelled in &self.combo_boxes {
            labelled.combo.place(None);
        }
        for &part in &self.parts {
            if index >= inner.height {
                return None;
            }
            let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
            match part {
                Part::ComboBox(number) => {
                    self.draw_combo_box(frame, (row, number, label_width), style, spots);
                    index += 1;
                }
                Part::Field(number) => {
                    index =
                        self.draw_labelled_field(frame, (inner, index), number, style, spots)?;
                }
                Part::Check(number) => {
                    self.draw_check(frame, (row, number), style, spots);
                    index += 1;
                }
                Part::Separator => index += 1,
            }
        }
        Some(index)
    }

    /// Draws field `number` under its label from row `index` of `inner`, with the terminal
    /// cursor in it if it has the focus, and keeps where for the mouse in `spots`, and where
    /// the glyph that switches how it searches is. A disabled one is faint, label and field,
    /// and takes no clicks. Returns the row after it, or `None` without room.
    fn draw_labelled_field(
        &self,
        frame: &mut Frame<'_>,
        (inner, mut index): (Rect, u16),
        number: usize,
        (colors, theme): (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
    ) -> Option<u16> {
        let row = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let labelled = &self.fields[number];
        let label = &labelled.label;
        if !label.text.is_empty() {
            let colors = if labelled.disabled {
                colors.disabled(theme)
            } else {
                colors
            };
            let room = usize::from(inner.width).max(1);
            frame.render_widget(
                Line::from(label_spans(label, room, colors, theme)),
                row(index),
            );
            index += 1;
        }
        if index >= inner.height {
            return None;
        }
        if labelled.disabled {
            let style = theme.dialog_input.patch(theme.disabled);
            frame.buffer_mut().set_style(row(index), style);
            return Some(index + 1);
        }
        let (column, area, glyph) = draw_field(frame, row(index), labelled, theme);
        self.glyphs
            .borrow_mut()
            .extend(glyph.map(|glyph| (number, glyph)));
        spots.push((Focus::Field(number), area));
        if number == 0 {
            self.field_area.set(Some(row(index)));
        }
        if self.focus == Focus::Field(number) {
            let column = u16::try_from(column).unwrap_or(0);
            frame.set_cursor_position(Position::new(inner.x + column, inner.y + index));
        }
        Some(index + 1)
    }

    /// Draws check box `number` on `row`, and keeps where for the mouse in `spots`.
    fn draw_check(
        &self,
        frame: &mut Frame<'_>,
        (row, number): (Rect, usize),
        (colors, theme): (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
    ) {
        let check = &self.checks[number];
        let focused = self.focus == Focus::Check(number);
        let style = if focused { colors.focused } else { colors.body };
        let mut spans = colors.check_box(check.on, focused, theme);
        spans.push(Span::styled(" ", style));
        let hotkey = colors.hotkey_style(focused);
        spans.extend(check.label.spans(style, Some(hotkey), colors.mark));
        frame.render_widget(Line::from(spans), row);
        spots.push((Focus::Check(number), row));
    }

    /// Draws combo box `number` on `row` after its label, the box `label_width` cells in, in the
    /// colors of fields, apart with the focus; and keeps where for the mouse in `spots`, the
    /// whole line.
    fn draw_combo_box(
        &self,
        frame: &mut Frame<'_>,
        (row, number, label_width): (Rect, usize, usize),
        (colors, theme): (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
    ) {
        let labelled = &self.combo_boxes[number];
        let room = usize::from(row.width);
        let hotkey = colors.hotkey_style(false);
        let mut spans = labelled.label.fit(
            label_width,
            colors.body_styles(),
            hotkey,
            theme.glyphs.marks,
        );
        let focused = self.focus == Focus::ComboBox(number);
        let boxed = labelled.combo.box_spans(room - label_width, focused, theme);
        let cells = boxed.iter().map(Span::width).sum::<usize>();
        let x = row.x + u16::try_from(label_width).unwrap_or(0);
        let width = u16::try_from(cells).unwrap_or(u16::MAX);
        let area = Rect::new(x, row.y, width, 1).intersection(row);
        labelled.combo.place(Some(area));
        spans.extend(boxed);
        frame.render_widget(Line::from(spans).style(colors.body), row);
        spots.push((Focus::ComboBox(number), row));
    }

    /// Draws the list from row `index` of `inner`, the room inside the frame, to the line over
    /// the buttons: a line, the rows, and the status of `view` over a line of its own. Returns
    /// where the buttons go, or `None` without room for a row.
    fn draw_list_part(
        &self,
        frame: &mut Frame<'_>,
        (inner, index): (Rect, u16),
        view: &ListView<'_>,
        colors: Colors,
        theme: &Theme,
    ) -> Option<u16> {
        let row = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let status = u16::try_from(view.status.len()).unwrap_or(u16::MAX);
        // The rows take what the buttons and the status leave them.
        let end = inner.height.saturating_sub(match status {
            0 => 2,
            _ => status.saturating_add(3),
        });
        if index + 1 >= end {
            return None;
        }
        draw_separator(frame, inner, inner.y + index, colors, theme);
        let area = Rect::new(inner.x, inner.y + index + 1, inner.width, end - index - 1);
        self.draw_list(frame, area, view, colors, theme);
        let mut index = end;
        if status > 0 {
            draw_separator(frame, inner, inner.y + index, colors, theme);
            let cut = (cells::Align::Left, cells::Cut::End);
            let width = usize::from(inner.width);
            for (number, text) in view.status.iter().enumerate() {
                index += 1;
                let marks = theme.glyphs.marks;
                let line = if number == 0 && view.counting {
                    status_skeleton(inner.width, colors.body, theme, view.shine)
                } else {
                    cells::fitted(text, width, cut, colors.body_styles(), marks)
                };
                frame.render_widget(line.style(colors.body), row(index));
            }
            index += 1;
        }
        Some(index)
    }

    /// Draws the rows of the list in `area`, scrolled to the cursor, with a scroll bar on the
    /// right of the frame; or the skeleton or the note of `view` in place of them. The cursor's
    /// row is in the color of the focus, dimmed while the focus is elsewhere.
    fn draw_list(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: &ListView<'_>,
        colors: Colors,
        theme: &Theme,
    ) {
        let Some(list) = &self.list else {
            return;
        };
        let mut drawn = list.rows.borrow_mut();
        drawn.clear();
        list.bar.set(None);
        let page = usize::from(area.height).max(1);
        list.page.set(page);
        let width = usize::from(area.width);
        let line = |offset: usize| {
            let y = area.y + u16::try_from(offset).unwrap_or(u16::MAX);
            Rect::new(area.x, y, area.width, 1)
        };
        if view.waiting {
            draw_skeleton(frame, line(0), theme, view.shine);
            return;
        }
        if let Some(note) = &view.note {
            for (offset, text) in cells::wrap(note, width).into_iter().take(page).enumerate() {
                frame.render_widget(Line::styled(text, colors.body), line(offset));
            }
            return;
        }
        let (mut cursor, mut window) = (list.cursor.get(), list.window.get());
        window.follow(&mut cursor, list.len, page);
        list.cursor.set(cursor);
        list.window.set(window);
        let top = window.top();
        let bar = right_frame(area, area.y, area.height);
        let position = (list.len, page, top);
        list.bar.set(scrollbar::render(
            frame,
            bar,
            position,
            colors.body,
            theme.glyphs.scroll_bar,
        ));
        let cursor_style = if self.focus == Focus::List {
            colors.focused
        } else {
            colors.focused.add_modifier(Modifier::DIM)
        };
        for (offset, index) in (top..list.len).take(page).enumerate() {
            let style = if index == cursor {
                cursor_style
            } else {
                colors.body
            };
            frame.render_widget((view.row)(index, width, style), line(offset));
            drawn.push((index, line(offset)));
        }
    }
}

/// A label in `room` cells of a dialog's body, its letter set apart; one too long loses its
/// middle, and its letter with it.
fn label_spans(label: &Label, room: usize, colors: Colors, theme: &Theme) -> Vec<Span<'static>> {
    if cells::width(&label.text) <= room {
        let hotkey = colors.hotkey_style(false);
        return label.spans(colors.body, Some(hotkey), colors.mark);
    }
    let styles = colors.body_styles();
    cells::fit(
        &label.text,
        room,
        cells::Align::Left,
        styles,
        theme.glyphs.marks,
    )
}

/// The brackets around a button: `[< >]` for the default one, `[ ]` for the others.
fn brackets(default: bool) -> (&'static str, &'static str) {
    if default {
        ("[< ", " >]")
    } else {
        ("[ ", " ]")
    }
}

/// Buttons, centered, as mc draws them: the default one in `[< >]`, the one with the focus in
/// its color, each with its letter set apart.
pub(crate) fn button_line(
    labels: &[Label],
    default: usize,
    focus: Option<usize>,
    colors: Colors,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        let focused = focus == Some(index);
        let style = if focused {
            colors.focused
        } else {
            colors.button
        };
        let (open, close) = brackets(index == default);
        spans.push(Span::styled(open, style));
        let hotkey = colors.hotkey_style(focused);
        spans.extend(label.spans(style, Some(hotkey), colors.mark));
        spans.push(Span::styled(close, style));
    }
    Line::from(spans).centered()
}

/// Where the buttons of `labels`, as [`button_line`] draws them with the one at `default` the
/// default, are when it is drawn on `area`, which centers it.
pub(crate) fn button_spots(labels: &[Label], default: usize, area: Rect) -> Vec<Rect> {
    let widths: Vec<i32> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let (open, close) = brackets(index == default);
            let cells = cells::width(open) + cells::width(&label.text) + cells::width(close);
            i32::try_from(cells).unwrap_or(i32::MAX)
        })
        .collect();
    let gaps = i32::try_from(labels.len().saturating_sub(1)).unwrap_or(i32::MAX);
    let total = widths.iter().sum::<i32>().saturating_add(gaps);
    // Ratatui centers a line that does not fit by cutting as much off either side.
    let mut x = i32::from(area.x) + (i32::from(area.width) - total).div_euclid(2);
    if total > i32::from(area.width) {
        x = i32::from(area.x) - (total - i32::from(area.width)) / 2;
    }
    let mut spots = Vec::new();
    for width in widths {
        let start = x;
        // A blank between buttons.
        x += width + 1;
        let left = start.max(i32::from(area.x));
        let right = (start + width).min(i32::from(area.right()));
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
        glyphs::LINE.repeat(usize::from(width.saturating_sub(2)))
    );
    let row = Rect::new(x, y, width, 1).intersection(frame.area());
    frame.render_widget(Line::styled(line, colors.body), row);
}

/// The cells of the right side of the frame around `inner`, the room inside a box that
/// [`draw_box`] drew, from the row `y` down `height` rows: where its scroll bar goes.
pub(crate) fn right_frame(inner: Rect, y: u16, height: u16) -> Rect {
    Rect::new(inner.right().saturating_add(1), y, 1, height)
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

/// Where a list that pops up from `anchor` goes in `area`, `size` with its frame, from the
/// column `x`, as vim's pop-up menu: under `anchor`, or above it where there is more room there,
/// as tall as the room lets it be.
pub(crate) fn pop_up(anchor: Rect, area: Rect, x: u16, (width, height): (u16, u16)) -> Rect {
    let below = area.bottom().saturating_sub(anchor.bottom());
    let above = anchor.y.saturating_sub(area.y);
    let (y, height) = if below >= height || below >= above {
        (anchor.bottom(), height.min(below))
    } else {
        let height = height.min(above);
        (anchor.y - height, height)
    };
    let width = width.min(area.right().saturating_sub(x));
    Rect::new(x, y, width, height)
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

/// Draws `dots` in `color` in front of `title` on the frame around `inner`, the room inside a box that
/// [`draw_box`] drew with that title: over the line of the frame, so that the title stays where
/// it is when they come and go. Nothing where the line has no room for them.
pub(crate) fn draw_title_dots(
    frame: &mut Frame<'_>,
    inner: Rect,
    title: &str,
    (dots, color): (char, Style),
    colors: Colors,
) {
    // Where the frame puts its title: between its corners.
    let left = inner.x.saturating_sub(1);
    let room = inner.width.saturating_add(2);
    // The title with a blank cell on either side.
    let width = u16::try_from(cells::width(title))
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let Some(free) = room.checked_sub(width) else {
        return;
    };
    // A blank cell and the dots.
    if free / 2 < 2 || inner.y == 0 {
        return;
    }
    let x = left + free / 2 - 2;
    let title = colors.title.bold();
    let line = Line::from(vec![
        Span::styled(" ", title),
        Span::styled(dots.to_string(), title.patch(color)),
    ]);
    let area = Rect::new(x, inner.y - 1, 2, 1).intersection(frame.area());
    frame.render_widget(line, area);
}

/// Where the rows go in `inner`, the room inside a window's box: under its first line and the
/// line under that, above the last `below` lines.
pub(crate) fn rows_area(inner: Rect, below: u16) -> Rect {
    let height = inner.height.saturating_sub(2 + below);
    Rect::new(inner.x, inner.y + 2, inner.width, height)
}

/// Rows a window shows while its first ones are still to come, the first time it opens; then as
/// many as came the last time.
pub(crate) const SKELETON_ROWS: u16 = 1;
/// Twelfths of the width: of the bar on the left of each skeleton row, of the one on the right,
/// over and over, so that rows under each other do not look alike.
const SKELETON_BARS: [(u16, u16); 4] = [(6, 3), (4, 3), (7, 2), (5, 3)];

/// Draws a skeleton on every line of `area`, where rows are still to come: a faint bar on the
/// left for a name and one on the right for what is known of it. A `shine`, if any, crosses
/// every bar at once, lighting them where it passes.
pub(crate) fn draw_skeleton(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    shine: Option<Shine>,
) {
    if area.width < 8 {
        return;
    }
    let glyph = theme.glyphs.skeleton.to_string();
    let bars = SKELETON_BARS
        .iter()
        .cycle()
        .take(usize::from(area.height))
        .map(|(left, right)| (area.width * left / 12, area.width * right / 12));
    let longest = bars
        .clone()
        .map(|(left, right)| left.max(right))
        .max()
        .unwrap_or(0);
    let middle = shine_middle(shine, longest);
    for (y, (left, right)) in (area.y..area.bottom()).zip(bars) {
        let line: Line<'_> = shine_bar(left, middle, &glyph, theme.dialog)
            .into_iter()
            .chain([Span::styled(
                " ".repeat(usize::from(area.width - left - right)),
                theme.dialog,
            )])
            .chain(shine_bar(right, middle, &glyph, theme.dialog))
            .collect();
        frame.render_widget(line, Rect::new(area.x, y, area.width, 1));
    }
}

/// A skeleton of a line of status over `width` cells, such as how many a search found, in the
/// colors of `style`: a faint bar from the left edge, as long as the right bar of a skeleton row.
/// A `shine`, if any, crosses it with the bars of a skeleton row over as many cells.
fn status_skeleton(width: u16, style: Style, theme: &Theme, shine: Option<Shine>) -> Line<'static> {
    let length = if width < 8 {
        0
    } else {
        width * SKELETON_BARS[0].1 / 12
    };
    let middle = shine_middle(shine, width * SKELETON_BARS[0].0 / 12);
    let glyph = theme.glyphs.skeleton.to_string();
    Line::from(shine_bar(length, middle, &glyph, style))
}

/// Cells from the middle of a shine to where it fades out.
const SHINE_HALF: u16 = 5;

/// Where the middle of `shine` is, in cells from the start of bars of `longest` cells at most:
/// it comes in at one end and goes out at the other, as fast over every bar. None while it
/// rests.
pub(crate) fn shine_middle(shine: Option<Shine>, longest: u16) -> Option<i32> {
    let cells = shine?.over(longest.saturating_add(2 * SHINE_HALF))?;
    Some(i32::from(cells) - i32::from(SHINE_HALF))
}

/// A faint bar of `length` cells of `glyph` in the color of `style` blended into its
/// background, which a shine whose middle is at `middle` (from [`shine_middle`]), if any,
/// lights where it passes. Where the colors are not known, the bar is dim and the shine plain.
pub(crate) fn shine_bar(
    length: u16,
    middle: Option<i32>,
    glyph: &str,
    style: Style,
) -> Vec<Span<'static>> {
    // How much of the color shows: in the bar, in the middle of the shine; per mille.
    const FAINT: u16 = 250;
    const BRIGHT: u16 = 650;
    let shade = |alpha: u16| match (style.fg, style.bg) {
        (Some(fg), Some(bg)) => theme::blend(fg, bg, (1000, alpha)).map(|fg| style.fg(fg)),
        _ => None,
    };
    let faint = shade(FAINT).unwrap_or_else(|| style.add_modifier(Modifier::DIM));
    (0..length)
        .map(|x| {
            let near = middle
                .map(|middle| (i32::from(x) - middle).unsigned_abs())
                .filter(|distance| *distance < u32::from(SHINE_HALF));
            let style = near.map_or(faint, |distance| {
                let distance = u16::try_from(distance).unwrap_or(SHINE_HALF);
                shade(BRIGHT - (BRIGHT - FAINT) * distance / SHINE_HALF).unwrap_or(style)
            });
            Span::styled(glyph.to_owned(), style)
        })
        .collect()
}

/// Draws the field of `labelled` on the line `area`, with the glyphs of how it matches at its
/// end if it searches. Returns the cursor's column in `area`, where the field takes a click, and
/// where the glyphs are.
fn draw_field(
    frame: &mut Frame<'_>,
    area: Rect,
    labelled: &LabelledField,
    theme: &Theme,
) -> (usize, Rect, Option<GlyphSpots>) {
    let field = &labelled.field;
    let Some(input) = labelled.search else {
        let room = usize::from(area.width);
        let (line, column) = match &labelled.placeholder {
            Some(placeholder) if field.text().is_empty() => {
                (placeholder_line(placeholder, room, theme), 0)
            }
            _ => field_line(field, room, theme),
        };
        frame.render_widget(line, area);
        return (column, area, None);
    };
    let invalid = input.invalid(field.text());
    let mark = input.mark(invalid, theme.dialog_input, theme);
    let (column, drawn) = mark.render_in_field(frame, area, field, theme);
    let right = drawn.map_or(area.right(), GlyphSpots::left);
    let text = Rect {
        width: right.saturating_sub(area.x),
        ..area
    };
    (usize::from(column), text, drawn)
}

/// What of `field` fits in `room` cells, in the color of fields, what Visual mode selects
/// reversed; and the cursor's column.
pub(crate) fn field_line(field: &Field, room: usize, theme: &Theme) -> (Line<'static>, usize) {
    let ([before, selected, after], column) = field.visible_parts(room);
    let used = cells::width(&before) + cells::width(&selected);
    let after = cells::fit(
        &after,
        room.saturating_sub(used),
        cells::Align::Left,
        (Style::new(), theme.truncated_cursor),
        theme.glyphs.marks,
    );
    let mut line = Line::from(vec![
        Span::raw(before),
        Span::styled(
            selected,
            theme.dialog_input.add_modifier(Modifier::REVERSED),
        ),
    ]);
    line.spans.extend(after);
    (line.style(theme.dialog_input), column)
}

/// `placeholder` in an empty field of `room` cells, faint, as the hints of settings are.
fn placeholder_line(placeholder: &str, room: usize, theme: &Theme) -> Line<'static> {
    let faint = theme.dialog_input.patch(theme.disabled);
    let spans = cells::fit(
        placeholder,
        room,
        cells::Align::Left,
        (faint, theme.truncated_cursor),
        theme.glyphs.marks,
    );
    Line::from(spans).style(theme.dialog_input)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
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

    fn skeleton_colors(shine: Option<Shine>) -> Vec<Option<Color>> {
        let mut theme = Theme::terminal();
        theme.dialog = Style::new()
            .fg(Color::Rgb(255, 255, 255))
            .bg(Color::Rgb(0, 0, 0));
        let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
        terminal
            .draw(|frame| draw_skeleton(frame, frame.area(), &theme, shine))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..30).map(|x| buffer[(x, 0)].style().fg).collect()
    }

    #[test]
    fn a_skeleton_lights_its_bars_where_the_shine_passes() {
        let faint = Some(Color::Rgb(64, 64, 64));
        let still = skeleton_colors(None);
        // Bars of 15 and 7 cells, the gap between them in the color of the dialog.
        assert!(
            still[..15]
                .iter()
                .chain(&still[23..])
                .all(|fg| *fg == faint)
        );
        assert!(
            still[15..23]
                .iter()
                .all(|fg| *fg == Some(Color::Rgb(255, 255, 255)))
        );
        assert_eq!(
            skeleton_colors(Some(Shine::after(0))),
            still,
            "the shine comes in from the left"
        );
        // Twelve cells on, coming in five cells before the bars, its middle is on cell 7 of both:
        // they shine at once and as fast, the shorter bar on the right at its end already.
        let shining = skeleton_colors(Some(Shine::after(12)));
        let next = Some(Color::Rgb(145, 145, 145));
        assert_eq!(shining[7], Some(Color::Rgb(166, 166, 166)));
        assert_eq!(shining[8], next);
        assert_eq!(shining[..3], still[..3]);
        assert_eq!(shining[23 + 6], next);
        assert_eq!(shining[23 + 2], faint);
    }

    #[test]
    fn a_skeleton_fills_its_area_with_rows_of_bars_of_their_own_lengths() {
        let theme = Theme::terminal();
        let mut terminal = Terminal::new(TestBackend::new(24, 5)).unwrap();
        terminal
            .draw(|frame| draw_skeleton(frame, frame.area(), &theme, None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = (0..5)
            .map(|y| (0..24).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        assert_eq!(
            lines,
            [
                "------------      ------",
                "--------          ------",
                "--------------      ----",
                "----------        ------",
                "------------      ------",
            ]
        );
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
    fn a_message_taller_than_the_screen_scrolls_and_keeps_the_buttons() {
        let lines: Vec<String> = (1..=30).map(|n| format!("line {n}")).collect();
        let mut dialog =
            Dialog::question("Long", &lines.join("\n"), vec![Button::Ok], 0, Tone::Plain);
        let shown = |dialog: &Dialog| draw(dialog, 40, 12).backend().to_string();
        let top = shown(&dialog);
        assert!(top.contains("line 1 ") && top.contains("[< OK >]"), "{top}");
        assert!(top.contains('▲') && top.contains('█'), "{top}");
        assert!(!top.contains("line 30"), "{top}");
        dialog.scroll(Scroll::By(Motion::Down));
        let down = shown(&dialog);
        assert!(
            !down.contains("line 1 ") && down.contains("line 2 "),
            "{down}"
        );
        dialog.scroll(Scroll::To(usize::MAX));
        let end = shown(&dialog);
        assert!(end.contains("line 30") && end.contains("[< OK >]"), "{end}");
        let short = Dialog::question("Short", "one line", vec![Button::Ok], 0, Tone::Plain);
        let text = shown(&short);
        assert!(!text.contains('░') && short.bar().is_none(), "{text}");
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
        Dialog::form("Select", "", "*", &checks).with_width(50)
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

    fn lettered() -> Dialog {
        let checks = [
            ("&Files only".to_owned(), false),
            ("&Case sensitive".to_owned(), false),
        ];
        Dialog::form("Select", "&Pattern:", "*", &checks)
            .with_width(50)
            .with_combo_box("&Number:", &["O&ne".to_owned(), "T&wo".to_owned()], 0)
    }

    #[test]
    fn alt_and_a_letter_switch_open_focus_and_press_from_anywhere() {
        let mut dialog = lettered();
        assert_eq!(
            dialog.focus,
            Focus::Field(0),
            "the field first, as forms open"
        );
        let hotkey = |dialog: &mut Dialog, c| dialog.handle(Resolved::Hotkey(c));
        assert_eq!(hotkey(&mut dialog, 'f'), DialogEvent::Pending);
        assert!(dialog.checked(0));
        assert_eq!(dialog.focus, Focus::Check(0));
        hotkey(&mut dialog, 'N');
        assert_eq!(
            (dialog.focus, dialog.context()),
            (Focus::ComboBox(0), Context::ComboBox),
            "either case opens the list"
        );
        assert_eq!(
            hotkey(&mut dialog, 'o'),
            DialogEvent::Pending,
            "the list's letters alone, not OK's"
        );
        assert_eq!(dialog.context(), Context::ComboBox);
        hotkey(&mut dialog, 'W');
        assert_eq!((dialog.chosen(0), dialog.context()), (1, Context::Dialog));
        hotkey(&mut dialog, 'p');
        assert_eq!(
            dialog.focus,
            Focus::Field(0),
            "the prompt's letter is the field's"
        );
        assert_eq!(dialog.mode(), Mode::Insert);
        typed(&mut dialog, "x");
        assert_eq!(dialog.text(), "*x");
        hotkey(&mut dialog, 'c');
        assert!(dialog.checked(1), "from the field too");
        hotkey(&mut dialog, 'c');
        assert!(!dialog.checked(1), "again, back");
        hotkey(&mut dialog, 'z');
        assert_eq!(
            dialog.focus,
            Focus::Check(1),
            "a letter nothing has does nothing"
        );
        assert_eq!(dialog.text(), "*x");
        assert_eq!(hotkey(&mut dialog, 'o'), DialogEvent::Pressed(Button::Ok));
        assert_eq!(Button::Cancel.label().letter(), None, "Esc and F10 cancel");
    }

    #[test]
    fn a_letter_in_normal_mode_gives_up_the_command_that_waits() {
        let mut dialog = lettered();
        dialog.handle(action(Action::NormalMode));
        dialog.handle(action(Action::DeleteOperator));
        assert!(dialog.pending().is_some());
        dialog.handle(Resolved::Hotkey('f'));
        assert_eq!(dialog.pending(), None);
        dialog.handle(Resolved::Hotkey('p'));
        assert_eq!(
            dialog.mode(),
            Mode::Insert,
            "a letter brings Insert mode, as Tab"
        );
    }

    #[test]
    fn letters_stand_out_where_they_are_drawn() {
        use ratatui::style::Color;

        let mut dialog = lettered();
        dialog.handle(Resolved::Hotkey('f'));
        let terminal = draw_themed(&dialog, 50, 14, &Theme::mc_classic());
        let buffer = terminal.backend().buffer();
        let find = |text: &str| super::super::mouse::find(buffer, text);
        let fg = |at: Position, offset: u16| buffer[(at.x + offset, at.y)].fg;
        let colors = Colors::of(&Theme::mc_classic(), Tone::Plain);
        assert_eq!(colors.hotkey_style(false).fg, Some(Color::Blue));
        let prompt = find("Pattern:");
        assert_eq!((fg(prompt, 0), fg(prompt, 1)), (Color::Blue, Color::Black));
        let combo = find("Number:");
        assert_eq!((fg(combo, 0), fg(combo, 1)), (Color::Blue, Color::Black));
        let choice = find("One");
        assert_eq!(fg(choice, 1), Color::Black, "its letter shows in its list");
        let check = find("Files only");
        assert_eq!(fg(check, 0), Color::Blue, "on the focus too");
        assert_eq!(buffer[(check.x, check.y)].bg, Color::Cyan);
        let ok = find("OK");
        assert_eq!((fg(ok, 0), fg(ok, 1)), (Color::Blue, Color::Black));
        let cancel = find("Cancel");
        assert_eq!(fg(cancel, 0), Color::Black);
        let text: String = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .map(|at| buffer[at].symbol())
            .collect();
        assert!(!text.contains('&'), "{text}");
    }

    /// Asserts that what `dialog` acts on by a letter has one of its own: its combo boxes, its
    /// labelled fields, its check boxes, and its buttons but Cancel; and each choice of a combo
    /// box one of its own in the list.
    fn assert_letters(name: &str, dialog: &Dialog) {
        for labelled in &dialog.combo_boxes {
            let letters = labelled.combo.letters();
            let unique: std::collections::HashSet<char> = letters.iter().copied().collect();
            assert_eq!(unique.len(), letters.len(), "{name}: {letters:?}");
        }
        let labels = dialog
            .combo_boxes
            .iter()
            .map(|labelled| &labelled.label)
            .chain(dialog.fields.iter().map(|field| &field.label))
            .chain(dialog.checks.iter().map(|check| &check.label));
        let missing: Vec<&str> = labels
            .filter(|label| !label.text.is_empty() && label.letter().is_none())
            .map(|label| label.text.as_str())
            .collect();
        assert!(missing.is_empty(), "{name}: {missing:?} have no letter");
        for &button in &dialog.buttons {
            let none = button.label().letter().is_none();
            assert_eq!(none, button == Button::Cancel, "{name}: {button:?}");
        }
        let letters = dialog.letters();
        let unique: std::collections::HashSet<char> = letters.iter().copied().collect();
        assert_eq!(unique.len(), letters.len(), "{name}: {letters:?}");
    }

    #[test]
    fn every_element_of_the_transfer_dialog_has_a_letter_of_its_own() {
        use super::super::transfer::{self, Kind, Sources};

        let one = Sources::One("R&&D".to_owned());
        let many = Sources::Many { files: 1, dirs: 1 };
        for kind in [Kind::Copy, Kind::Move] {
            for sources in [&one, &many] {
                let dialog = transfer::dialog(kind, sources, ("", 0), true);
                let name = format!("{kind:?} {sources:?}");
                assert!(dialog.message.letter().is_some(), "{name}: the prompt");
                assert_letters(&name, &dialog);
            }
        }
    }

    #[test]
    fn every_element_of_the_apps_dialogs_has_a_letter_of_its_own() {
        use crate::i18n::fl;

        let ok_cancel = || vec![Button::Ok, Button::Cancel];
        let form = |message: String, checks: &[(String, bool)]| {
            Dialog::form("Title", &message, "", checks)
        };
        let forms = [
            ("mkdir", form(fl!("mkdir-prompt"), &[])),
            ("go to directory", form(fl!("goto-prompt"), &[])),
            ("save checksums", form(fl!("checksum-save-prompt"), &[])),
            ("save workspace", form(fl!("workspaces-save-prompt"), &[])),
            (
                "rename workspace",
                form(fl!("workspaces-rename-prompt", name = "x"), &[]),
            ),
        ];
        for (name, dialog) in &forms {
            assert!(dialog.message.letter().is_some(), "{name}: the prompt");
            assert_letters(name, dialog);
        }
        let select = [(fl!("pattern-files-only"), false)];
        let host_fields = [
            (fl!("host-label"), String::new()),
            (fl!("host-start-dir"), String::new()),
            (fl!("host-other-dir"), String::new()),
        ];
        let host_buttons = vec![Button::Ok, Button::UseCurrent, Button::Cancel];
        let algorithms = [
            fl!("checksum-sha256"),
            fl!("checksum-sha512"),
            fl!("checksum-sha1"),
            fl!("checksum-md5"),
            fl!("checksum-blake3"),
        ];
        let question =
            |buttons: Vec<Button>| Dialog::question("Title", "?", buttons, 0, Tone::Plain);
        let dialogs = [
            (
                "select",
                Dialog::form("Select", &fl!("pattern-text"), "*", &select).with_width(50),
            ),
            (
                "host",
                Dialog::fields(
                    "Host",
                    &host_fields,
                    &[(fl!("host-remember-dir"), false)],
                    host_buttons,
                ),
            ),
            (
                "checksum",
                Dialog::fields("Checksum", &[], &[], ok_cancel())
                    .with_message(&fl!("checksum-one", name = "x"))
                    .with_combo_box(&fl!("checksum-algorithm"), &algorithms, 0)
                    .with_separator()
                    .with_field(&fl!("checksum-expected"), "")
                    .with_check(&fl!("checksum-compare", path = "/x"), false),
            ),
            ("password", secret()),
            (
                "host key",
                Dialog::prompt("web", "yes?", PromptKind::HostKey),
            ),
            ("yes or no", question(vec![Button::Yes, Button::No])),
            (
                "file exists",
                question(vec![
                    Button::Yes,
                    Button::No,
                    Button::All,
                    Button::KeepAll,
                    Button::Older,
                    Button::Abort,
                ]),
            ),
            (
                "failure",
                question(vec![
                    Button::Skip,
                    Button::SkipAll,
                    Button::Retry,
                    Button::Abort,
                ]),
            ),
            ("reconnect", question(vec![Button::Retry, Button::Cancel])),
            ("error", Dialog::error("No.")),
        ];
        for (name, dialog) in &dialogs {
            assert_letters(name, dialog);
        }
    }

    #[test]
    fn a_long_field_keeps_its_text_still_while_the_cursor_shows_as_vim() {
        // Off the edge, the cursor goes to the middle, as with vim's 'sidescroll' 0.
        let mut field = Field::plain("abcdefghij");
        assert_eq!(field.visible(5), ("ij".to_owned(), 2));
        field.edit(Action::Left);
        field.edit(Action::Left);
        assert_eq!(field.visible(5), ("ij".to_owned(), 0), "the text stays");
        field.edit(Action::Left);
        assert_eq!(field.visible(5), ("fghij".to_owned(), 2));
        field.editor.set_cursor(0);
        assert_eq!(field.visible(5), ("abcde".to_owned(), 0));
        let wide = Field::plain("文件文件");
        assert_eq!(wide.visible(5), ("件".to_owned(), 2));
        let mut secret = Field::secret();
        for c in "hunter2".chars() {
            secret.insert(c);
        }
        assert_eq!(secret.visible(4), ("**".to_owned(), 2));
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
    fn word_keys_go_by_what_spaces_part_as_on_vims_command_line() {
        let mut field = Field::plain("cp  a.txt backup");
        assert!(field.edit(Action::WordBackward));
        assert_eq!(field.editor.cursor(), 10, "the start of backup");
        field.edit(Action::WordBackward);
        assert_eq!(field.editor.cursor(), 4, "a.txt, the dot no break");
        field.edit(Action::TextStart);
        field.edit(Action::WordForward);
        assert_eq!(field.editor.cursor(), 2, "on the space after cp");
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
        // After the end of the text, as vim's with 'selection' inclusive.
        for action in [Action::TextStart, Action::VisualMode, Action::TextEnd] {
            assert!(field.handle(action, Extra::default(), &mut registers));
        }
        assert_eq!(
            parts(&field, 20),
            ([String::new(), "report.pdf".to_owned(), String::new()], 10)
        );
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

    fn picker() -> Dialog {
        let choices = ["&one".to_owned(), "&two".to_owned(), "t&hree".to_owned()];
        Dialog::fields("Pick", &[], &[], vec![Button::Ok, Button::Cancel])
            .with_width(40)
            .with_combo_box("&Count:", &choices, 0)
            .with_check("&Verbose", false)
    }

    #[test]
    fn space_opens_a_combo_box_whose_list_has_the_keys() {
        let mut dialog = picker();
        assert_eq!(
            (dialog.focus, dialog.context()),
            (Focus::ComboBox(0), Context::Dialog),
            "without fields, the combo box first"
        );
        dialog.handle(action(Action::Toggle));
        assert_eq!(
            (dialog.context(), dialog.mode()),
            (Context::ComboBox, Mode::Normal)
        );
        let two = Typed {
            resolved: action(Action::Down),
            extra: Extra {
                count: Some(2),
                ..Extra::default()
            },
        };
        dialog.handle_typed(two, &mut Registers::default());
        assert_eq!(dialog.chosen(0), 0, "the cursor moves, not the choice");
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pending,
            "Enter takes the row and closes the list"
        );
        assert_eq!((dialog.chosen(0), dialog.context()), (2, Context::Dialog));
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Up));
        dialog.handle(action(Action::Cancel));
        assert_eq!(dialog.chosen(0), 2, "Esc keeps the choice");
        dialog.handle(action(Action::Down));
        assert_eq!(dialog.focus, Focus::Check(0), "j and k move the focus");
        dialog.handle(action(Action::Up));
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pressed(Button::Ok),
            "Enter on the box presses the default button"
        );
    }

    #[test]
    fn a_click_opens_a_combo_box_and_one_in_its_list_takes_a_row() {
        let mut dialog = picker();
        dialog.handle(action(Action::Down));
        assert_eq!(
            press_on(&mut dialog, Press::Click, "Count:"),
            DialogEvent::Pending
        );
        assert_eq!(
            (dialog.focus, dialog.context()),
            (Focus::ComboBox(0), Context::ComboBox)
        );
        assert_eq!(
            press_on(&mut dialog, Press::DoubleClick, "Count:"),
            DialogEvent::Pending
        );
        assert!(
            dialog.combo_box_open(),
            "the second click of the one that opened it"
        );
        let terminal = draw(&dialog, 40, 12);
        let two = super::super::mouse::find(terminal.backend().buffer(), "two");
        let at = |press| Pointer { press, at: two };
        assert_eq!(dialog.pointer(at(Press::Click)), DialogEvent::Pending);
        assert_eq!((dialog.chosen(0), dialog.combo_box_open()), (1, false));
        // The row was over the buttons.
        assert_eq!(dialog.pointer(at(Press::DoubleClick)), DialogEvent::Pending);
        assert_eq!(
            dialog.focus,
            Focus::ComboBox(0),
            "its second click acts on nothing under it"
        );
        press_on(&mut dialog, Press::Click, "two");
        assert!(dialog.combo_box_open());
        press_on(&mut dialog, Press::Click, "Pick");
        assert!(!dialog.combo_box_open(), "a click outside closes the list");
        assert_eq!(dialog.chosen(0), 1);
    }

    #[test]
    fn a_combo_box_has_the_colors_of_fields_and_its_focus_stands_apart() {
        use ratatui::style::Color;

        let theme = Theme::mc_classic();
        let mut dialog = picker();
        // The colors of the cells of `text`, from `from` cells before it, as many as `count`.
        let colors = |dialog: &Dialog, text: &str, from: u16, count: u16| {
            let terminal = draw_themed(dialog, 40, 12, &theme);
            let buffer = terminal.backend().buffer();
            let at = super::super::mouse::find(buffer, text);
            (0..count)
                .map(|offset| {
                    let cell = &buffer[(at.x - from + offset, at.y)];
                    (cell.fg, cell.bg, cell.modifier.contains(Modifier::REVERSED))
                })
                .collect::<Vec<_>>()
        };
        // `[one   ▼]`: the bracket, a letter, the arrow, and the bracket.
        let combo = |dialog: &Dialog| {
            let cells = colors(dialog, "[one", 0, 9);
            [cells[0], cells[1], cells[7], cells[8]]
        };
        let focused = (Color::Black, Color::Cyan, true);
        assert_eq!(
            combo(&dialog),
            [focused; 4],
            "fields have the focus's colors in mc-classic: reversed, all of it"
        );
        dialog.handle(action(Action::Down));
        let (faint, field) = (Color::DarkGray, Color::Cyan);
        assert_eq!(
            combo(&dialog),
            [
                (faint, field, false),
                (Color::Black, field, false),
                (Color::Blue, field, false),
                (faint, field, false),
            ],
            "as a field, its brackets faint and its arrow blue as titles"
        );
        // `[✓] Verbose`, with the focus, then without it; its letter is blue either way.
        dialog.handle(action(Action::Toggle));
        let check = |dialog: &Dialog| colors(dialog, "Verbose", 4, 4);
        let focus = (Color::Black, Color::Cyan, false);
        assert_eq!(check(&dialog), [focus; 4]);
        dialog.handle(action(Action::Down));
        let gray = Color::Gray;
        assert_eq!(
            check(&dialog),
            [
                (faint, gray, false),
                (Color::Blue, gray, false),
                (faint, gray, false),
                (Color::Black, gray, false),
            ]
        );
    }

    #[test]
    fn draws_a_combo_box_and_its_list_over_the_dialog() {
        let mut dialog = picker();
        dialog.handle(action(Action::Toggle));
        dialog.handle(action(Action::Down));
        let terminal = draw(&dialog, 40, 12);
        insta::assert_snapshot!(terminal.backend());
    }

    fn hashing(disabled: bool) -> Dialog {
        let choices = ["&one".to_owned(), "&two".to_owned()];
        let dialog = Dialog::fields("Hash", &[], &[], vec![Button::Ok, Button::Cancel])
            .with_width(40)
            .with_message("Of x")
            .with_combo_box("&Count:", &choices, 0)
            .with_separator()
            .with_field("&Expected:", "")
            .with_check("&Verbose", false);
        if disabled {
            dialog.with_disabled(0)
        } else {
            dialog
        }
    }

    /// The row of the first line of `terminal` that holds `text`.
    fn row_of(terminal: &Terminal<TestBackend>, text: &str) -> u16 {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (0..area.height)
            .find(|&y| {
                let row: String = (0..area.width).map(|x| buffer[(x, y)].symbol()).collect();
                row.contains(text)
            })
            .unwrap()
    }

    #[test]
    fn draws_the_parts_in_order_with_a_separator_and_a_disabled_field_faint() {
        let dialog = hashing(true);
        let theme = Theme::terminal();
        let terminal = draw_themed(&dialog, 40, 12, &theme);
        insta::assert_snapshot!(terminal.backend());
        let (combo, label) = (row_of(&terminal, "Count:"), row_of(&terminal, "Expected:"));
        assert_eq!(label, combo + 2, "a blank row between them");
        let buffer = terminal.backend().buffer();
        let x = (0..40)
            .find(|&x| buffer[(x, label)].symbol() == "E")
            .unwrap();
        let dim = |style: Style| style.add_modifier.contains(Modifier::DIM);
        assert!(dim(theme.disabled));
        assert!(dim(buffer[(x, label)].style()), "its letter too");
        let field = buffer[(x, label + 1)].style();
        assert!(dim(field), "the field");
        assert_eq!(
            field.add_modifier,
            theme.dialog_input.add_modifier | Modifier::DIM,
            "still in the colors of fields"
        );

        let enabled = draw_themed(&hashing(false), 40, 12, &theme);
        let buffer = enabled.backend().buffer();
        assert!(
            !dim(buffer[(x, label)].style()),
            "else in its letter's color"
        );
    }

    #[test]
    fn a_disabled_field_takes_no_focus_from_keys_letters_or_the_mouse() {
        let mut dialog = hashing(true);
        assert_eq!(dialog.focus, Focus::ComboBox(0), "the first stop");
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.focus, Focus::Check(0), "past the field");
        dialog.handle(Resolved::Hotkey('e'));
        assert_eq!(dialog.focus, Focus::Check(0), "its letter does nothing");
        assert!(!dialog.letters().contains(&'E'));
        let terminal = draw(&dialog, 40, 12);
        let buffer = terminal.backend().buffer();
        let at = (0..12)
            .find_map(|y| {
                let x = (0..40).find(|&x| buffer[(x, y)].symbol() == "E")?;
                Some(Position::new(x, y + 1))
            })
            .unwrap();
        dialog.pointer(Pointer {
            press: Press::Click,
            at,
        });
        assert_eq!(dialog.focus, Focus::Check(0), "a click on it does nothing");
        assert_eq!(
            hashing(false).focus,
            Focus::Field(0),
            "else the field first"
        );
    }

    #[test]
    fn a_separator_goes_anywhere_and_takes_neither_the_focus_nor_clicks() {
        let mut dialog = Dialog::fields("Pick", &[], &[], vec![Button::Ok, Button::Cancel])
            .with_width(40)
            .with_check("&Fast", false)
            .with_separator()
            .with_check("&Slow", false);
        let terminal = draw(&dialog, 40, 12);
        let (fast, slow) = (row_of(&terminal, "Fast"), row_of(&terminal, "Slow"));
        assert_eq!(slow, fast + 2, "a blank row between the check boxes");
        assert_eq!(dialog.stops().len(), 4, "two check boxes and two buttons");
        dialog.focus = Focus::Check(0);
        dialog.handle(action(Action::Down));
        assert_eq!(dialog.focus, Focus::Check(1), "straight past it");
        let blank = Position::new(6, fast + 1);
        assert_eq!(
            dialog.pointer(Pointer {
                press: Press::Click,
                at: blank,
            }),
            DialogEvent::Pending
        );
        assert_eq!(dialog.focus, Focus::Check(1), "a click on it does nothing");
        assert!(!dialog.checked(0) && !dialog.checked(1));
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

    #[test]
    fn draws_the_transfer_dialog_of_a_move_as_that_of_a_copy() {
        use super::super::transfer::{self, Kind, Sources};

        let sources = Sources::One("notes.txt".to_owned());
        let dialog = transfer::dialog(Kind::Move, &sources, ("/srv/backup", 11), true);
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
        Dialog::fields("Host web", &fields, &checks, buttons).with_width(60)
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
    fn an_empty_field_shows_its_placeholder_faint_until_typed_in() {
        let theme = Theme::terminal();
        let mut dialog = host_form().with_placeholder(2, "~/site\u{1b}[2J");
        dialog.handle(action(Action::Down));
        dialog.handle(action(Action::Down));
        let row = |mut terminal: Terminal<TestBackend>| {
            let cursor = terminal.get_cursor_position().unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = (0..64).map(|x| buffer[(x, cursor.y)].symbol()).collect();
            let first = (0..64)
                .find(|&x| buffer[(x, cursor.y)].symbol() == "~")
                .map(|x| (x, buffer[(x, cursor.y)].style()));
            (cursor.x, text, first)
        };
        let (cursor, text, first) = row(draw_themed(&dialog, 64, 16, &theme));
        assert!(text.contains("~/site"), "{text}");
        assert!(!text.contains('\u{1b}'), "terminal-safe: {text}");
        let (x, style) = first.unwrap();
        assert_eq!(cursor, x, "the cursor stands at its start");
        let faint = theme.dialog_input.patch(theme.disabled);
        assert_eq!(style.add_modifier, faint.add_modifier);
        assert_ne!(faint.add_modifier, theme.dialog_input.add_modifier);
        assert_eq!(dialog.text_of(2), "", "it is not the text");

        typed(&mut dialog, "/srv");
        let (_, text, _) = row(draw_themed(&dialog, 64, 16, &theme));
        assert!(text.contains("/srv") && !text.contains("~/site"), "{text}");
        dialog.handle(action(Action::DeleteToStart));
        let (_, text, _) = row(draw_themed(&dialog, 64, 16, &theme));
        assert!(text.contains("~/site"), "back once empty: {text}");
        assert_eq!(dialog.text_of(2), "");
    }

    /// A dialog with a field, a check box, a list of `len` rows, and OK and Cancel.
    fn with_rows(len: usize) -> Dialog {
        let fields = [("Text:".to_owned(), String::new())];
        let checks = [("Case".to_owned(), false)];
        let mut dialog = Dialog::fields("Find", &fields, &checks, vec![Button::Ok, Button::Cancel])
            .with_width(40)
            .with_list();
        dialog.set_list_len(len);
        dialog
    }

    fn draw_rows(dialog: &Dialog, note: Option<&str>) -> Terminal<TestBackend> {
        let row =
            |index: usize, _: usize, style: Style| Line::styled(format!("row {index}"), style);
        let view = ListView {
            row: &row,
            note: note.map(str::to_owned),
            status: vec!["30 rows".to_owned()],
            counting: false,
            dots: None,
            waiting: false,
            shine: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(40, 16)).unwrap();
        terminal
            .draw(|frame| dialog.render_list(frame, frame.area(), &Theme::terminal(), &view))
            .unwrap();
        terminal
    }

    #[test]
    fn a_list_is_a_stop_whose_motions_move_its_cursor() {
        let mut dialog = with_rows(30);
        let screen = draw_rows(&dialog, None).backend().to_string();
        assert!(
            screen.contains("row 0") && screen.contains("30 rows"),
            "{screen}"
        );
        dialog.handle(action(Action::NextField));
        dialog.handle(action(Action::NextField));
        assert_eq!(dialog.context(), Context::Dialog);
        dialog.handle(action(Action::Down));
        assert_eq!(
            dialog.list_cursor(),
            1,
            "Down moves the cursor, not the focus"
        );
        let three = Extra {
            count: Some(3),
            ..Extra::default()
        };
        let down = Typed {
            resolved: action(Action::Down),
            extra: three,
        };
        dialog.handle_typed(down, &mut Registers::default());
        assert_eq!(dialog.list_cursor(), 4);
        dialog.handle(action(Action::End));
        assert_eq!(dialog.list_cursor(), 29);
        let screen = draw_rows(&dialog, None).backend().to_string();
        assert!(
            screen.contains("row 29") && !screen.contains("row 0 "),
            "{screen}"
        );
        dialog.handle(action(Action::Home));
        assert_eq!(dialog.list_cursor(), 0);
        assert_eq!(
            dialog.handle(action(Action::Confirm)),
            DialogEvent::Pressed(Button::Ok),
            "Enter presses the default button"
        );
        // Fewer rows keep the cursor inside; Tab goes on to the buttons.
        dialog.set_list_cursor(20);
        dialog.set_list_len(5);
        assert_eq!(dialog.list_cursor(), 4);
        dialog.handle(action(Action::NextField));
        assert_eq!(
            dialog.handle(action(Action::Toggle)),
            DialogEvent::Pressed(Button::Ok)
        );
    }

    #[test]
    fn the_mouse_and_the_wheel_move_over_the_list() {
        let mut dialog = with_rows(30);
        let terminal = draw_rows(&dialog, None);
        let at = super::super::mouse::find(terminal.backend().buffer(), "row 2");
        let click = Pointer {
            press: Press::Click,
            at,
        };
        assert_eq!(dialog.pointer(click), DialogEvent::Pending);
        assert_eq!(
            (dialog.list_cursor(), dialog.context()),
            (2, Context::Dialog)
        );
        let double = Pointer {
            press: Press::DoubleClick,
            at,
        };
        assert_eq!(dialog.pointer(double), DialogEvent::Pressed(Button::Ok));
        dialog.wheel_list(Action::Down, 10);
        assert!(dialog.list_cursor() > 2, "kept inside what shows");
        draw_rows(&dialog, None);
        assert!(dialog.list_bar().is_some());
        // A note shows in place of the rows.
        let screen = draw_rows(&dialog, Some("Nothing found."))
            .backend()
            .to_string();
        assert!(
            screen.contains("Nothing found.") && !screen.contains("row "),
            "{screen}"
        );
    }

    #[test]
    fn errors_are_red_in_mc_classic() {
        use ratatui::style::{Color, Modifier};

        let mut dialog = Dialog::error("Cannot create directory /x: already exists");
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

    #[test]
    fn warnings_have_a_background_apart_from_dialogs_and_errors() {
        use ratatui::style::Color;

        let theme = Theme::mc_classic();
        // The colors of the title and the message.
        let colors = |tone: Tone| {
            let dialog = Dialog::question("Delete", "Delete x?", vec![Button::Yes], 0, tone);
            let terminal = draw_themed(&dialog, 60, 8, &theme);
            let buffer = terminal.backend().buffer();
            let at = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
            (at(title_column(buffer, "Delete"), 1), at(4, 2))
        };
        let (title, body) = colors(Tone::Warning);
        assert_eq!(body, (Color::Black, Color::Yellow));
        assert_eq!(title, (Color::Black, Color::Yellow));
        assert_ne!(body, colors(Tone::Plain).1);
        assert_ne!(body, colors(Tone::Error).1);
    }
}
