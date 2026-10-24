//! Modal dialogs: prompts from ssh (passwords and passphrases, host keys, confirmations, and
//! notices), the app's own questions, and the search of a whole tree, whose dialog has a list.
//! As in mc and Far, Alt and the letter that an `&` marks in a label act on what it labels:
//! press a button, switch a check box, pick a choice, or put the focus in a field.

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
use super::edit::{Editor, Registers};
use super::glyphs;
use super::keymap::{Action, Context, Extra, Mode, Resolved, Typed};
use super::label::{self, Label};
use super::mouse::{Pointer, Press, Scroll};
use super::scroll::Window;
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
    Choice(usize),
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
}

impl LabelledField {
    fn unlabelled(field: Field) -> Self {
        Self {
            label: Label::default(),
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
    /// The marks of text cut short, over `body`.
    mark: Style,
    /// The letters of labels, over the rest, and over what has the focus.
    hotkey: Style,
    hotkey_focused: Style,
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

    /// The body's style, and that of the marks of text cut short over it.
    pub(crate) fn body_styles(self) -> (Style, Style) {
        (self.body, self.mark)
    }

    /// A dialog's colors, or an error's: mc draws errors and warnings red.
    pub(crate) fn of(theme: &Theme, error: bool) -> Self {
        if error {
            Self {
                body: theme.error_dialog,
                title: theme.error_title,
                button: theme.error_dialog,
                focused: theme.error_button_focused,
                mark: theme.truncated_error,
                hotkey: theme.error_hotkey,
                hotkey_focused: theme.error_hotkey_focused,
            }
        } else {
            Self {
                body: theme.dialog,
                title: theme.dialog_title,
                button: theme.dialog_button,
                focused: theme.dialog_button_focused,
                mark: theme.truncated_dialog,
                hotkey: theme.dialog_hotkey,
                hotkey_focused: theme.dialog_hotkey_focused,
            }
        }
    }
}

/// A check box.
#[derive(Debug)]
struct Check {
    label: Label,
    on: bool,
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

/// A modal dialog: a message, a group of choices of which one is chosen, text fields, check
/// boxes, and buttons, each of them optional but the buttons. Whoever opens it reads the choice,
/// the fields, and the check boxes once it closes.
#[derive(Debug)]
pub(crate) struct Dialog {
    /// Tells this dialog from one that takes its place.
    id: u64,
    title: String,
    /// The text above the rest; a form's marks the letter of its field.
    message: Label,
    /// Radio buttons, as mc draws them: `(*)` on the chosen one.
    choices: Vec<Label>,
    chosen: usize,
    /// The choices on one line, rather than one under another.
    in_a_row: bool,
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
    /// states, and OK and Cancel. An `&` in `message` marks the letter of the field.
    pub(crate) fn form(title: &str, message: &str, text: &str, checks: &[(String, bool)]) -> Self {
        Self {
            message: Label::parse(message.trim_end()),
            fields: vec![LabelledField::unlabelled(Field::plain(text))],
            checks: Self::checks(checks),
            focus: Focus::Field(0),
            ..Self::new(title, "", vec![Button::Ok, Button::Cancel])
        }
    }

    /// Text fields with their labels and the texts they open with, check boxes with their
    /// labels and states, and `buttons`, the first of them the default.
    pub(crate) fn fields(
        title: &str,
        fields: &[(String, String)],
        checks: &[(String, bool)],
        buttons: Vec<Button>,
    ) -> Self {
        let fields: Vec<LabelledField> = fields
            .iter()
            .map(|(label, text)| LabelledField {
                label: Label::parse(&cells::sanitize(label.as_bytes())),
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
            ..Self::new(title, "", buttons)
        }
    }

    fn checks(checks: &[(String, bool)]) -> Vec<Check> {
        checks
            .iter()
            .map(|(label, on)| Check {
                label: Label::parse(label),
                on: *on,
            })
            .collect()
    }

    fn new(title: &str, message: &str, buttons: Vec<Button>) -> Self {
        Self {
            id: LAST_ID.fetch_add(1, Ordering::Relaxed) + 1,
            title: cells::sanitize(title.as_bytes()),
            message: Label::plain(message.trim_end()),
            choices: Vec::new(),
            chosen: 0,
            in_a_row: false,
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

    /// The same dialog with `choices` above its fields, `chosen` of them chosen. Without
    /// fields, the chosen one starts with the focus.
    pub(crate) fn with_choices(mut self, choices: &[String], chosen: usize) -> Self {
        self.chosen = chosen.min(choices.len().saturating_sub(1));
        if self.fields.is_empty() && !choices.is_empty() {
            self.focus = Focus::Choice(self.chosen);
        }
        self.choices = choices.iter().map(|choice| Label::parse(choice)).collect();
        self
    }

    /// The same dialog with its choices on one line.
    pub(crate) fn in_a_row(mut self) -> Self {
        self.in_a_row = true;
        self
    }

    /// Puts `label` over text field `index`.
    pub(crate) fn set_label(&mut self, index: usize, label: &str) {
        if let Some(field) = self.fields.get_mut(index) {
            field.label = Label::parse(&cells::sanitize(label.as_bytes()));
        }
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
            Focus::Choice(_) | Focus::Check(_) | Focus::List | Focus::Button(_) => Context::Dialog,
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
            Focus::Choice(_) | Focus::Check(_) | Focus::List | Focus::Button(_) => None,
        }
    }

    fn focused_field_mut(&mut self) -> Option<&mut Field> {
        match self.focus {
            Focus::Field(index) => self.fields.get_mut(index).map(|field| &mut field.field),
            Focus::Choice(_) | Focus::Check(_) | Focus::List | Focus::Button(_) => None,
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
        match action {
            // Enter takes the choice it is on, as that is the one that looks chosen.
            Action::Confirm => match self.focus {
                Focus::Button(index) => DialogEvent::Pressed(self.buttons[index]),
                Focus::Choice(index) => {
                    self.chosen = index;
                    DialogEvent::Pressed(self.buttons[self.default])
                }
                Focus::Field(_) | Focus::Check(_) | Focus::List => {
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
    /// switched, and a choice chosen, each with the focus; a field with it, or under a prompt
    /// with it, gets the focus in Insert mode. Nothing else has it.
    fn hotkey(&mut self, c: char) -> DialogEvent {
        let found = if self.message.has(c) && !self.fields.is_empty() {
            Some(Focus::Field(0))
        } else {
            self.stops().into_iter().find(|&stop| match stop {
                Focus::Choice(index) => self.choices[index].has(c),
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
            Focus::Choice(index) => self.chosen = index,
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
        let choices = self.choices.iter().map(Label::letter);
        let fields = self.fields.iter().map(|field| field.label.letter());
        let checks = self.checks.iter().map(|check| check.label.letter());
        let buttons = self.buttons.iter().map(|button| button.label().letter());
        prompt
            .into_iter()
            .chain(choices)
            .chain(fields)
            .chain(checks)
            .chain(buttons)
            .flatten()
            .collect()
    }

    /// Takes a press of the mouse, where the dialog was drawn last: a click chooses a choice,
    /// puts the cursor in a field or on a row of the list, switches a check box, or presses a
    /// button. A double click on a choice or a row presses the default button too, as Enter
    /// does there.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> DialogEvent {
        let Pointer { press, at } = pointer;
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
            Focus::List => {}
            // The first click pressed it.
            Focus::Button(_) if press == Press::DoubleClick => {}
            Focus::Button(index) => return DialogEvent::Pressed(self.buttons[index]),
        }
        DialogEvent::Pending
    }

    /// Where the focus can be: the choices, the fields, the check boxes, the list, and the
    /// buttons.
    fn stops(&self) -> Vec<Focus> {
        (0..self.choices.len())
            .map(Focus::Choice)
            .chain((0..self.fields.len()).map(Focus::Field))
            .chain((0..self.checks.len()).map(Focus::Check))
            .chain(self.list.as_ref().map(|_| Focus::List))
            .chain((0..self.buttons.len()).map(Focus::Button))
            .collect()
    }

    /// Moves the focus through the choices, the fields, the check boxes, the list, and the
    /// buttons, round; a field it comes to is in `mode`, Insert or Normal.
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

    fn render_with(
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
        // Borders, the message, the choices, the fields under their labels, the check boxes, a
        // line, the buttons.
        let labels = self
            .fields
            .iter()
            .filter(|f| !f.label.text.is_empty())
            .count();
        let choice_rows = if self.in_a_row {
            self.choices.len().min(1)
        } else {
            self.choices.len()
        };
        let height = rows(lines.len())
            .saturating_add(rows(choice_rows))
            .saturating_add(rows(self.fields.len() + labels))
            .saturating_add(rows(self.checks.len()));
        // A line, a row at least, and a line over the status, under the rest.
        let below = list.map_or(0, |list| match list.status.len() {
            0 => 2,
            status => rows(status).saturating_add(3),
        });
        let colors = Colors::of(theme, self.error);
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
        // The line and the buttons below the rest.
        let rest = height
            .saturating_sub(rows(lines.len()))
            .saturating_add(2)
            .saturating_add(below);
        let message = (lines.as_slice(), letter);
        let mut index = self.render_message(frame, inner, message, rest, colors, theme);
        let room = usize::from(inner.width).max(1);
        let Some(after) = self.draw_choices(frame, (inner, index), colors, &mut spots) else {
            return;
        };
        index = after;
        for (number, LabelledField { label, field }) in self.fields.iter().enumerate() {
            if !label.text.is_empty() {
                if index >= inner.height {
                    return;
                }
                frame.render_widget(
                    Line::from(label_spans(label, room, colors, theme)),
                    row(index),
                );
                index += 1;
            }
            if index >= inner.height {
                return;
            }
            let (line, column) = field_line(field, room, theme);
            frame.render_widget(line, row(index));
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
        let Some(after) = self.draw_checks(frame, (inner, index), colors, &mut spots) else {
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
                Focus::Choice(_) | Focus::Field(_) | Focus::Check(_) | Focus::List => None,
            };
            let line = button_line(&labels, self.default, focus, colors);
            let buttons = button_spots(&labels, self.default, row(index + 1));
            spots.extend((0..).map(Focus::Button).zip(buttons));
            frame.render_widget(line, row(index + 1));
        }
    }
}

impl Dialog {
    /// Draws the check boxes from row `index` of `inner`, the room inside the frame, and keeps
    /// where for the mouse in `spots`. Returns the row after them, or `None` without room.
    fn draw_checks(
        &self,
        frame: &mut Frame<'_>,
        (inner, mut index): (Rect, u16),
        colors: Colors,
        spots: &mut Vec<(Focus, Rect)>,
    ) -> Option<u16> {
        for (number, check) in self.checks.iter().enumerate() {
            if index >= inner.height {
                return None;
            }
            let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
            let text = format!("[{}] ", mark(check.on, glyphs::CHECKED));
            let focused = self.focus == Focus::Check(number);
            let style = if focused { colors.focused } else { colors.body };
            let mut spans = vec![Span::styled(text, style)];
            let hotkey = colors.hotkey_style(focused);
            spans.extend(check.label.spans(style, Some(hotkey), colors.mark));
            frame.render_widget(Line::from(spans), row);
            spots.push((Focus::Check(number), row));
            index += 1;
        }
        Some(index)
    }

    /// Draws the choices from row `index` of `inner`, the room inside the frame, one under
    /// another or on one line, and keeps where for the mouse in `spots`. Returns the row after
    /// them, or `None` without room.
    fn draw_choices(
        &self,
        frame: &mut Frame<'_>,
        (inner, mut index): (Rect, u16),
        colors: Colors,
        spots: &mut Vec<(Focus, Rect)>,
    ) -> Option<u16> {
        let mut x = inner.x;
        for (number, choice) in self.choices.iter().enumerate() {
            if index >= inner.height {
                return None;
            }
            let text = format!("({}) ", mark(number == self.chosen, glyphs::CHOSEN));
            let focused = self.focus == Focus::Choice(number);
            let style = if focused { colors.focused } else { colors.body };
            let cells = cells::width(&text) + cells::width(&choice.text);
            let width = u16::try_from(cells).unwrap_or(u16::MAX);
            let area = if self.in_a_row {
                Rect::new(x, inner.y + index, width, 1).intersection(inner)
            } else {
                Rect::new(inner.x, inner.y + index, inner.width, 1)
            };
            let mut spans = vec![Span::styled(text, style)];
            let hotkey = colors.hotkey_style(focused);
            spans.extend(choice.spans(style, Some(hotkey), colors.mark));
            frame.render_widget(Line::from(spans), area);
            spots.push((Focus::Choice(number), area));
            if self.in_a_row {
                // Three blanks apart, as mc sets its radio buttons on a line.
                x = x.saturating_add(width).saturating_add(3);
            } else {
                index += 1;
            }
        }
        if self.in_a_row && !self.choices.is_empty() {
            index += 1;
        }
        Some(index)
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

/// `glyph` in a radio button or a check box that is on, `on`; a blank in one that is off.
fn mark(on: bool, glyph: char) -> char {
    if on { glyph } else { ' ' }
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
        let mut dialog = Dialog::question("Long", &lines.join("\n"), vec![Button::Ok], 0, false);
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
        let short = Dialog::question("Short", "one line", vec![Button::Ok], 0, false);
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
            .with_choices(&["O&ne".to_owned(), "T&wo".to_owned()], 0)
    }

    #[test]
    fn alt_and_a_letter_switch_choose_focus_and_press_from_anywhere() {
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
        hotkey(&mut dialog, 'W');
        assert_eq!(
            (dialog.chosen(), dialog.focus),
            (1, Focus::Choice(1)),
            "either case"
        );
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
        let colors = Colors::of(&Theme::mc_classic(), false);
        assert_eq!(colors.hotkey_style(false).fg, Some(Color::Blue));
        let prompt = find("Pattern:");
        assert_eq!((fg(prompt, 0), fg(prompt, 1)), (Color::Blue, Color::Black));
        let choice = find("One");
        assert_eq!(fg(choice, 1), Color::Blue);
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

    /// Asserts that what `dialog` acts on by a letter has one of its own: its choices, its
    /// labelled fields, its check boxes, and its buttons but Cancel.
    fn assert_letters(name: &str, dialog: &Dialog) {
        let labels = dialog
            .choices
            .iter()
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
        let select = [
            (fl!("pattern-files-only"), false),
            (fl!("pattern-case-sensitive"), false),
        ];
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
        let question = |buttons: Vec<Button>| Dialog::question("Title", "?", buttons, 0, false);
        let dialogs = [
            (
                "select",
                Dialog::form("Select", "", "*", &select).with_width(50),
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
                Dialog::fields(
                    "Checksum",
                    &[(fl!("checksum-expected"), String::new())],
                    &[(fl!("checksum-compare", path = "/x"), false)],
                    ok_cancel(),
                )
                .with_message(&fl!("checksum-one", name = "x"))
                .with_choices(&algorithms, 0),
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
            ("error", Dialog::error("Error", "No.")),
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

    #[test]
    fn a_double_click_on_a_choice_chooses_it_and_confirms() {
        let choices = ["one".to_owned(), "two".to_owned()];
        let mut dialog = Dialog::fields("Pick", &[], &[], vec![Button::Ok, Button::Cancel])
            .with_width(40)
            .with_choices(&choices, 0);
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
