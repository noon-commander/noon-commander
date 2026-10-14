//! Vim's modes in a text: a text field, or the command line, whose lines are its text's.
//!
//! In Insert mode, characters go in at the cursor, and a few keys edit around it. In Normal
//! mode, keys move the cursor by vim's motions, change the text, and keep text in registers
//! and put it back; an operator such as `d` acts on what the motion after it moves over, or on
//! whole lines when typed twice. In Visual mode, the motions select, and the operators act on
//! what is selected. A count repeats a motion or a change; undo takes back a change at a time,
//! where all that one stay in Insert mode typed is one change.

use std::collections::HashMap;
use std::ops::Range;

use zeroize::Zeroizing;

use super::keymap::{Action, Extra, MAX_COUNT, Mode};

/// Changes that undo keeps, the oldest dropped first.
const UNDO_LEVELS: usize = 1000;

/// What yanks and deletes keep: the unnamed register, and `a` … `z`, which every text field and
/// the command line share while Noon Commander runs. An uppercase name appends to the register
/// of its letter, as in vim. Nothing goes to the system clipboard.
#[derive(Debug, Default)]
pub(crate) struct Registers {
    unnamed: Register,
    named: HashMap<char, Register>,
}

/// The text of a register.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Register {
    text: String,
    /// Whole lines, as `d d` and `y y` keep them.
    lines: bool,
}

impl Registers {
    /// The register `name` names: the unnamed one without a name, or for `"`; none for `_`,
    /// vim's black hole.
    fn get(&self, name: Option<char>) -> Option<&Register> {
        match name {
            Some('_') => None,
            Some(c) if c.is_ascii_alphabetic() => self.named.get(&c.to_ascii_lowercase()),
            _ => Some(&self.unnamed),
        }
    }

    /// Keeps `register` in the unnamed register, and in the one `name` names, if any; `_`
    /// keeps nothing. What a delete appends to a register (`deleted`) the unnamed one keeps
    /// alone, as vim's register 1 does, which it reads then.
    fn keep(&mut self, name: Option<char>, register: Register, deleted: bool) {
        match name {
            Some('_') => {}
            Some(c) if c.is_ascii_uppercase() => {
                let lower = c.to_ascii_lowercase();
                let kept = if let Some(kept) = self.named.get_mut(&lower) {
                    // Lines go on a line of their own, an empty one too.
                    if kept.lines || register.lines {
                        kept.text.push('\n');
                    }
                    kept.text.push_str(&register.text);
                    kept.lines |= register.lines;
                    kept.clone()
                } else {
                    self.named.insert(lower, register.clone());
                    register.clone()
                };
                self.unnamed = if deleted { register } else { kept };
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.named.insert(c, register.clone());
                self.unnamed = register;
            }
            _ => self.unnamed = register,
        }
    }

    /// The text of the register `name` names, if it has any.
    pub(crate) fn text(&self, name: Option<char>) -> Option<&str> {
        self.get(name).map(|register| register.text.as_str())
    }
}

/// An operator, waiting for the motion that says what it acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    Delete,
    Change,
    Yank,
}

impl Operator {
    fn of(action: Action) -> Option<Self> {
        match action {
            Action::DeleteOperator => Some(Self::Delete),
            Action::ChangeOperator => Some(Self::Change),
            Action::YankOperator => Some(Self::Yank),
            _ => None,
        }
    }

    fn key(self) -> char {
        match self {
            Self::Delete => 'd',
            Self::Change => 'c',
            Self::Yank => 'y',
        }
    }
}

/// An operator typed, with its count and its register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    operator: Operator,
    count: usize,
    register: Option<char>,
    /// The count typed before the register, which `count` takes in.
    outer: Option<usize>,
}

/// Where a motion goes, and whether an operator acts on the character there too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Motion {
    to: usize,
    inclusive: bool,
    /// Whether an operator ends where the motion does, even at the start of a line, as after
    /// Backspace onto the line break before it.
    keep_end: bool,
}

/// The text and its cursor before a change, for undo.
#[derive(Debug, Clone)]
struct Snapshot {
    text: String,
    /// The line and the column of the cursor, counted in characters from 0: vim keeps where
    /// the change started so, for undo and redo alike, whatever the lines became.
    cursor: (usize, usize),
    /// Whether the text had no line, as [`Editor::no_line`].
    no_line: bool,
}

/// After vim's Ctrl+o: Normal mode for one command, then Insert mode again.
#[derive(Debug, Clone, Copy, Default)]
struct Once {
    /// The line, counted from 0, whose end the cursor was after when Ctrl+o left Insert mode.
    eol: Option<usize>,
}

/// What a text holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holds {
    /// One line, as a text field does.
    Line,
    /// Lines, as the command line does.
    Lines,
    /// A secret, on one line: it stays in Insert mode, keeps no undo, and lives in memory
    /// reserved up front and wiped when it goes.
    Secret,
}

/// A text with its cursor in vim's modes.
pub(crate) struct Editor {
    text: Zeroizing<String>,
    /// In characters.
    cursor: usize,
    holds: Holds,
    mode: Mode,
    /// Where Visual mode started.
    anchor: usize,
    /// The lines and the columns the last command of Visual mode took, for a count before `v`,
    /// as vim's `resel_VIsual_line_count` and `resel_VIsual_vcol`: on one line, how many
    /// characters; on more, the column of the last.
    reselect: Option<(usize, usize)>,
    pending: Option<Pending>,
    /// The last `f`, `t`, `F`, or `T`, and its character, for `;` and `,`.
    find: Option<(Action, char)>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Insert mode has changed the text since it started: one change for undo.
    changed: bool,
    /// The text has no line, after `d d` deleted the last one, as vim's buffer with `ML_EMPTY`:
    /// a delete has nothing to do there, not even a change for undo. Any change gives it its
    /// empty line back.
    no_line: bool,
    /// After vim's Ctrl+o: Insert mode again after one command.
    once: Option<Once>,
    /// Whether the command after Ctrl+o left the cursor before where Insert mode goes on: after
    /// the text it put, or after the end of the line it deleted to, as vim's does.
    once_after: bool,
    /// Where Ctrl+w and Ctrl+u stop in Insert mode.
    insert: Insert,
}

/// Where an insert started, and so where Ctrl+w and Ctrl+u stop once, as vim keeps them.
#[derive(Debug, Clone, Copy, Default)]
struct Insert {
    /// Where the insert started, vim's `Insstart`: where Insert mode started, or the cursor at
    /// the first change after a move.
    start: usize,
    /// Where Ctrl+w and Ctrl+u stop once, vim's `Insstart_orig`: where the insert started, as
    /// of the key before.
    stop_at: usize,
    /// Whether the cursor moved in Insert mode since the last change, as vim's arrow keys do.
    moved: bool,
    /// Whether Insert mode changed the text since it started.
    typed: bool,
    /// Whether Ctrl+w and Ctrl+u keep stopping where they do, as after a move to the right of
    /// it with something typed, or after Ctrl+u, which vim's defaults.vim maps after Ctrl+g u.
    keep_stop: bool,
}

impl Insert {
    /// An insert that starts at `at`, as Insert mode starts.
    fn at(at: usize) -> Self {
        Self {
            start: at,
            stop_at: at,
            ..Self::default()
        }
    }

    /// An insert that starts at `at`, with the cursor moved past it since: as vim's Insert mode
    /// after Ctrl+o, which starts on the last character of the line before it goes after it.
    fn moved_from(at: usize) -> Self {
        Self {
            moved: true,
            ..Self::at(at)
        }
    }

    /// Before a key of Insert mode: where Ctrl+w and Ctrl+u stop follows where the insert
    /// started, unless it stays.
    fn next_key(&mut self) {
        if !self.keep_stop {
            self.stop_at = self.start;
        }
    }

    /// Before a change at `cursor`, as vim's `stop_arrow`: after a move, a new insert starts
    /// there; to the right of where Ctrl+w and Ctrl+u stop, after something was typed, they
    /// keep stopping there.
    fn changes(&mut self, cursor: usize) {
        if self.moved {
            self.start = cursor;
            if self.start > self.stop_at && self.typed {
                self.keep_stop = true;
            }
            self.moved = false;
        }
        self.typed = true;
    }
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("Editor");
        if !self.is_secret() {
            debug.field("text", &self.text.as_str());
        }
        debug.field("mode", &self.mode).finish_non_exhaustive()
    }
}

impl Editor {
    /// `text` in `mode`, the cursor at its end.
    pub(crate) fn new(text: &str, mode: Mode) -> Self {
        let mut editor = Self {
            text: Zeroizing::new(text.to_owned()),
            cursor: text.chars().count(),
            holds: Holds::Line,
            mode,
            anchor: 0,
            reselect: None,
            pending: None,
            find: None,
            undo: Vec::new(),
            redo: Vec::new(),
            changed: false,
            no_line: false,
            once: None,
            once_after: false,
            insert: Insert::default(),
        };
        editor.clamp();
        editor.begin_insert(editor.cursor);
        editor
    }

    /// An empty secret, which never grows past `capacity` bytes, so that its text is never
    /// copied to new memory and left there unwiped.
    pub(crate) fn secret(capacity: usize) -> Self {
        Self {
            text: Zeroizing::new(String::with_capacity(capacity)),
            holds: Holds::Secret,
            ..Self::new("", Mode::Insert)
        }
    }

    /// The same editor, whose text may hold line breaks.
    pub(crate) fn with_lines(mut self) -> Self {
        self.holds = Holds::Lines;
        self
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The cursor, in characters.
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    pub(crate) fn is_secret(&self) -> bool {
        self.holds == Holds::Secret
    }

    /// Whether the text may hold more than one line.
    fn has_lines(&self) -> bool {
        self.holds == Holds::Lines
    }

    /// Puts the cursor at character `cursor`, as far as the text and the mode let it go. In
    /// Insert mode, as a move of the cursor there: a new change, and a new insert after it.
    pub(crate) fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.len());
        self.clamp();
        if self.mode == Mode::Insert {
            self.insert_moved();
        }
    }

    /// Replaces the text with `text`, as the history or a completion does, the cursor at
    /// `cursor`; undo can take it back.
    pub(crate) fn replace(&mut self, text: &str, cursor: usize) {
        // Growing could leave a copy of a secret behind.
        if self.is_secret() {
            return;
        }
        if self.text.as_str() == text {
            self.set_cursor(cursor);
            return;
        }
        self.begin_change();
        self.text.clear();
        self.text.push_str(text);
        self.set_cursor(cursor);
    }

    /// Insert mode, or Normal mode, as when the focus comes to the field: a key that types
    /// goes to Insert mode, with the cursor at the end of the text if it was in another mode,
    /// and one that moves to Normal mode. Either gives up an operator that waits. A secret
    /// stays in Insert mode.
    pub(crate) fn set_mode(&mut self, mode: Mode) {
        if self.is_secret() {
            return;
        }
        self.pending = None;
        self.once = None;
        self.once_after = false;
        // What is typed after the focus comes back is a change of its own.
        self.changed = false;
        if self.mode != mode {
            match mode {
                Mode::Insert => self.cursor = self.len(),
                Mode::Visual => self.anchor = self.cursor,
                Mode::Normal => {}
            }
            self.mode = mode;
            self.clamp();
        }
        self.begin_insert(self.cursor);
    }

    /// Whether Normal mode runs one command, after Ctrl+o, then goes back to Insert mode.
    pub(crate) fn once(&self) -> bool {
        self.once.is_some()
    }

    /// The operator waiting for its motion, as vim's `showcmd` shows it, such as `2d`.
    pub(crate) fn pending(&self) -> Option<String> {
        let pending = self.pending?;
        // As typed: the count before the register, the register, and the count after it.
        let mut shown = String::new();
        let after = match pending.outer {
            Some(outer) => {
                shown.push_str(&outer.to_string());
                pending.count / outer.max(1)
            }
            None => pending.count,
        };
        if let Some(name) = pending.register {
            shown.push('"');
            shown.push(name);
        }
        if after > 1 {
            shown.push_str(&after.to_string());
        }
        shown.push(pending.operator.key());
        Some(shown)
    }

    /// How much Visual mode selects, as vim's `showcmd` shows it: the characters within a
    /// line, after its end too, else the lines.
    pub(crate) fn selection_size(&self) -> Option<usize> {
        if self.mode != Mode::Visual {
            return None;
        }
        let (lines, columns) = self.reselect_of();
        Some(if lines > 1 { lines } else { columns })
    }

    /// What Visual mode selects, in characters.
    pub(crate) fn selection(&self) -> Option<Range<usize>> {
        if self.mode != Mode::Visual {
            return None;
        }
        let start = self.anchor.min(self.cursor);
        let end = (self.anchor.max(self.cursor) + 1).min(self.len());
        Some(start..end)
    }

    /// Types `c` at the cursor, as in Insert mode.
    pub(crate) fn insert(&mut self, c: char) {
        if self.is_secret() && self.text.len() + c.len_utf8() > self.text.capacity() {
            return;
        }
        if c == '\n' && !self.has_lines() {
            return;
        }
        self.next_insert_key();
        self.insert_changes();
        self.begin_change();
        let offset = self.offset(self.cursor);
        self.text.insert(offset, c);
        self.cursor += 1;
    }

    /// Puts `text` at the cursor, as pasted: line breaks only where the text may have them,
    /// else spaces.
    pub(crate) fn insert_str(&mut self, text: &str) {
        let text = if self.has_lines() {
            text.to_owned()
        } else {
            text.replace('\n', " ")
        };
        if text.is_empty()
            || self.is_secret() && self.text.len() + text.len() > self.text.capacity()
        {
            return;
        }
        self.insert_changes();
        self.begin_change();
        let offset = self.offset(self.cursor);
        self.text.insert_str(offset, &text);
        self.cursor += text.chars().count();
    }

    /// Puts pasted `text` as one change, as vim does: at the cursor in Insert mode. In Normal
    /// mode, as if typed in Insert mode, then Esc: before the cursor at the start of a line,
    /// after it elsewhere. In Visual mode, in place of the selection, which goes to the unnamed
    /// register. After Ctrl+o, Normal mode stays, and an operator that waits is given up with the
    /// paste.
    pub(crate) fn paste(&mut self, text: &str, registers: &mut Registers) {
        if self.mode == Mode::Insert {
            self.next_insert_key();
            self.insert_str(text);
            return;
        }
        if self.pending.is_some() {
            self.give_up();
            return;
        }
        // The paste is the one command after Ctrl+o; Insert mode does not come back, as in vim.
        self.once = None;
        self.once_after = false;
        let text = if self.has_lines() {
            text.to_owned()
        } else {
            text.replace('\n', " ")
        };
        if text.is_empty() {
            return;
        }
        let chars = self.chars();
        let at = if let Some(selected) = self.selection() {
            let taken: String = chars[selected.clone()].iter().collect();
            if !taken.is_empty() {
                registers.keep(
                    None,
                    Register {
                        text: taken,
                        lines: false,
                    },
                    true,
                );
            }
            self.mode = Mode::Normal;
            // Undo puts the cursor back where the change starts.
            self.cursor = selected.start;
            self.begin_change();
            self.remove(selected.start, selected.end);
            selected.start
        } else {
            let (start, _) = line_bounds(&chars, self.cursor);
            self.cursor += usize::from(self.cursor > start);
            self.begin_change();
            self.cursor
        };
        let offset = self.offset(at);
        self.text.insert_str(offset, &text);
        // Esc puts the cursor on the character before, unless it is at the start of a line.
        let end = at + text.chars().count();
        let chars = self.chars();
        self.cursor = if line_bounds(&chars, end).0 == end {
            end
        } else {
            end - 1
        };
        self.clamp();
    }

    /// Gives up a command that waits: an operator for its motion, and after Ctrl+o, the one
    /// command of Normal mode, as the keymap gives up the keys of one that went wrong.
    pub(crate) fn give_up(&mut self) {
        self.pending = None;
        self.end_once(None);
    }

    /// Ends the one command of Normal mode after Ctrl+o, `action` if it ran, and goes back to
    /// Insert mode, as vim does: after the end of the line if the cursor was there and is on
    /// its last character, or after `$`. Visual mode goes on until it ends.
    fn end_once(&mut self, action: Option<Action>) {
        if self.mode == Mode::Visual {
            return;
        }
        let Some(Once { eol }) = self.once.take() else {
            return;
        };
        let after = std::mem::take(&mut self.once_after);
        if self.mode != Mode::Normal {
            return;
        }
        self.mode = Mode::Insert;
        self.changed = false;
        let chars = self.chars();
        let (_, end) = line_bounds(&chars, self.cursor);
        if after && self.cursor < end {
            self.cursor += 1;
        }
        // `0`, `^`, and Home leave the cursor before the character they go to, even the last.
        let to_start = matches!(action, Some(Action::TextStart | Action::FirstNonBlank));
        let back = (eol == Some(line_index(&chars, self.cursor)) && !to_start)
            || action == Some(Action::TextEnd);
        if back && self.cursor + 1 == end {
            self.insert = Insert::moved_from(self.cursor);
            self.cursor = end;
            return;
        }
        self.begin_insert(self.cursor);
    }

    /// Takes `action`, typed with `extra`; whether it was the editor's. Keys that are not, such
    /// as Enter or Esc in Normal mode, are for the field's owner. While an operator waits for
    /// its motion, any other key gives it up, Esc included.
    pub(crate) fn handle(
        &mut self,
        action: Action,
        extra: Extra,
        registers: &mut Registers,
    ) -> bool {
        let handled = match self.mode {
            Mode::Insert => self.insert_mode(action, extra, registers, !self.has_lines()),
            Mode::Normal => self.normal_mode(action, extra, registers),
            Mode::Visual => self.visual_mode(action, extra, registers),
        };
        // After Ctrl+o, Insert mode again once the command is done, whichever mode it left;
        // Visual mode goes on until it ends.
        if handled && action != Action::NormalOnce && self.pending.is_none() {
            self.end_once(Some(action));
        }
        handled
    }

    /// Edits as Insert mode does, without registers and without leaving it: for a filter.
    pub(crate) fn edit(&mut self, action: Action) -> bool {
        if matches!(
            action,
            Action::NormalMode | Action::NormalOnce | Action::PasteRegister
        ) {
            return false;
        }
        self.insert_mode(action, Extra::default(), &mut Registers::default(), false)
    }

    /// The keys of Insert mode; `stop` where Ctrl+w and Ctrl+u stop once where Insert mode
    /// started, as vim's 'backspace' has them in a text, not on its command line.
    fn insert_mode(
        &mut self,
        action: Action,
        extra: Extra,
        registers: &mut Registers,
        stop: bool,
    ) -> bool {
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        self.next_insert_key();
        match action {
            Action::Left
            | Action::Right
            | Action::TextStart
            | Action::TextEnd
            | Action::WordForward
            | Action::WordBackward => {
                let to = match action {
                    Action::Left => self.cursor.saturating_sub(1),
                    Action::Right => (self.cursor + 1).min(chars.len()),
                    Action::TextStart => start,
                    Action::TextEnd => end,
                    Action::WordForward if stop => word_start(&chars, self.cursor, false),
                    Action::WordForward => command_word_forward(&chars, self.cursor),
                    _ if stop => word_back(&chars, self.cursor, false),
                    _ => command_word_back(&chars, self.cursor),
                };
                // As in vim, a move starts a new change, and a new insert; Home and End do
                // where the cursor is already.
                let ends = matches!(action, Action::TextStart | Action::TextEnd);
                if to != self.cursor || ends {
                    self.cursor = to;
                    self.insert_moved();
                }
            }
            Action::Backspace => {
                self.insert_changes();
                self.delete(self.cursor.saturating_sub(1), self.cursor);
                self.insert.stop_at = self.insert.stop_at.min(self.cursor);
            }
            Action::DeleteChar => {
                self.insert_changes();
                // On vim's command line, Delete at the end deletes the character before it. In
                // a text, it is a change to undo even with nothing after it.
                if !stop && self.cursor == chars.len() {
                    self.delete(self.cursor.saturating_sub(1), self.cursor);
                } else if self.cursor == chars.len() {
                    self.save_undo();
                } else {
                    self.delete(self.cursor, self.cursor + 1);
                }
            }
            Action::DeleteToEnd => {
                self.insert_changes();
                self.delete(self.cursor, end);
            }
            Action::DeleteToStart | Action::DeleteWord => {
                // As vim's defaults.vim puts Ctrl+g u before Ctrl+u: what it deletes is a change
                // of its own, and where they stop stays. Ctrl+w deletes within the change.
                if action == Action::DeleteToStart {
                    self.changed = false;
                    self.insert.start = self.cursor;
                    self.insert.keep_stop = true;
                    self.insert.typed = false;
                }
                self.insert_changes();
                let mut from = if action == Action::DeleteWord {
                    word_back(&chars, self.cursor, false).max(start)
                } else {
                    start
                };
                // At the start of a line, the line break before it.
                if from == self.cursor {
                    from = self.cursor.saturating_sub(1);
                }
                let stop_at = self.insert.stop_at;
                if stop && self.cursor > stop_at && from < stop_at {
                    from = stop_at;
                }
                self.delete(from, self.cursor);
                self.insert.stop_at = stop_at.min(self.cursor);
            }
            Action::PasteRegister => {
                if let Some(register) = registers.get(extra.char).cloned() {
                    self.insert_str(&register.text);
                }
            }
            Action::NormalMode if !self.is_secret() => {
                self.mode = Mode::Normal;
                self.changed = false;
                if self.cursor > start {
                    self.cursor -= 1;
                }
                self.clamp();
            }
            // As vim's Ctrl+o, which puts the cursor on the last character from after it, and
            // back when the command is done.
            // At the end of an empty line too, as vim's `ins_at_eol`.
            Action::NormalOnce if !self.is_secret() => {
                self.once = Some(Once {
                    eol: (self.cursor == end).then(|| line_index(&chars, self.cursor)),
                });
                self.once_after = false;
                self.mode = Mode::Normal;
                self.changed = false;
                self.clamp();
            }
            _ => return false,
        }
        true
    }

    fn normal_mode(&mut self, action: Action, extra: Extra, registers: &mut Registers) -> bool {
        let count = extra.times();
        if let Some(operator) = Operator::of(action) {
            match self.pending.take() {
                Some(pending) if pending.operator == operator => {
                    let count = times(count, pending.count);
                    self.back_to_first_non_blank(operator, count);
                    self.lines_operate(pending, count, registers);
                }
                // Another operator gives the first one up, as an unknown motion does.
                Some(_) => {}
                None => {
                    self.pending = Some(Pending {
                        operator,
                        count,
                        register: extra.register,
                        outer: extra.outer,
                    });
                }
            }
            return true;
        }
        if let Some(pending) = self.pending.take() {
            let total = times(count, pending.count);
            if is_line_motion(action) {
                if let Some(line) = self.line_motion(action, total) {
                    self.lines_to(pending, line, registers);
                }
                return true;
            }
            // As vim's `b`, which fails at the start of the text, and the operator with it.
            if matches!(action, Action::WordBackward | Action::BigWordBackward) && self.cursor == 0
            {
                return true;
            }
            let on_word = self
                .char_at(self.cursor)
                .is_some_and(|c| !c.is_whitespace());
            // As in vim, `cw` changes to the end of the word, not the spaces after it, and the
            // word under the cursor ends where it is on its last character.
            let motion = match (pending.operator, action) {
                (Operator::Change, Action::WordForward | Action::BigWordForward) if on_word => {
                    let big = action == Action::BigWordForward;
                    Some(self.change_word(total, big))
                }
                _ => self.motion(action, total, extra.char, Some(pending.operator)),
            };
            if let Some(motion) = motion {
                let (from, to) = if motion.to < self.cursor {
                    (motion.to, self.cursor)
                } else {
                    (self.cursor, motion.to + usize::from(motion.inclusive))
                };
                let to = to.min(self.len());
                self.operate_motion(pending, from..to, motion, registers);
            }
            return true;
        }
        // Esc after Ctrl+o goes back to Insert mode, as in vim.
        if self.once.is_some() && matches!(action, Action::Cancel | Action::NormalMode) {
            return true;
        }
        if let Some(motion) = self.motion(action, count, extra.char, None) {
            self.cursor = motion.to;
            self.clamp();
            return true;
        }
        if is_motion(action) {
            return true;
        }
        self.change(action, extra, registers)
    }

    /// Where `cw` changes to, `count` words: the end of the word under the cursor, which is
    /// the cursor itself on its last character, then the ends of the words after it.
    fn change_word(&self, count: usize, big: bool) -> Motion {
        let chars = self.chars();
        let at = self.cursor;
        let same = |index: usize| {
            chars
                .get(index)
                .is_some_and(|&c| class(c, big) == class(chars[at], big))
        };
        let first = if same(at + 1) {
            word_end(&chars, at, big)
        } else {
            at
        };
        let to = (1..count).try_fold(first, |at, _| {
            let next = word_end(&chars, at, big);
            if next == at { Err(at) } else { Ok(next) }
        });
        Motion {
            to: to.unwrap_or_else(|at| at),
            inclusive: true,
            keep_end: false,
        }
    }

    /// The commands of Normal mode that are not motions or operators: those that go to Insert
    /// or Visual mode, change the text, put it, or undo.
    fn change(&mut self, action: Action, extra: Extra, registers: &mut Registers) -> bool {
        let count = extra.times();
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        match action {
            Action::InsertMode => self.start_insert(self.cursor),
            Action::Append => self.start_insert((self.cursor + 1).min(end)),
            Action::InsertAtStart => self.start_insert(first_non_blank(&chars, start, end)),
            Action::AppendAtEnd => self.start_insert(end),
            Action::VisualMode => {
                self.mode = Mode::Visual;
                self.anchor = self.cursor;
                if let Some(count) = extra.count {
                    self.select_count(count);
                }
            }
            Action::DeleteChar => {
                let to = (self.cursor + count).min(end);
                self.operate(Operator::Delete, self.cursor..to, extra.register, registers);
            }
            Action::Backspace => {
                let from = self.cursor.saturating_sub(count).max(start);
                self.operate(
                    Operator::Delete,
                    from..self.cursor,
                    extra.register,
                    registers,
                );
            }
            Action::Substitute => {
                let to = (self.cursor + count).min(end);
                self.operate(Operator::Change, self.cursor..to, extra.register, registers);
            }
            Action::SubstituteLine => {
                let pending = Pending {
                    operator: Operator::Change,
                    count: 1,
                    register: extra.register,
                    outer: None,
                };
                self.back_to_first_non_blank(Operator::Change, count);
                self.lines_operate(pending, count, registers);
            }
            // As `c $` and `d $`: to the end of the line, and of `count` - 1 lines more.
            Action::ChangeToEnd | Action::DeleteToEnd => {
                let operator = if action == Action::ChangeToEnd {
                    Operator::Change
                } else {
                    Operator::Delete
                };
                if let Some(to) = line_end_after(&chars, end, count) {
                    let pending = Pending {
                        operator,
                        count: 1,
                        register: extra.register,
                        outer: None,
                    };
                    // As `$`, which is inclusive.
                    let motion = Motion {
                        to,
                        inclusive: true,
                        keep_end: true,
                    };
                    self.operate_motion(pending, self.cursor..to, motion, registers);
                }
            }
            Action::ReplaceChar => {
                if let Some(c) = extra.char
                    && self.cursor + count <= end
                {
                    self.begin_change();
                    let replaced: String = std::iter::repeat_n(c, count).collect();
                    let (from, to) = (self.offset(self.cursor), self.offset(self.cursor + count));
                    self.text.replace_range(from..to, &replaced);
                    self.cursor += count - 1;
                }
            }
            Action::ToggleCase => {
                let to = (self.cursor + count).min(end);
                self.toggle_case(self.cursor..to);
                self.cursor = to;
                self.after_once_at_end();
                self.clamp();
            }
            Action::PutAfter | Action::PutBefore => {
                self.put(action == Action::PutAfter, count, extra.register, registers);
            }
            Action::Undo => self.undo(count),
            Action::Redo => self.redo(count),
            _ => return false,
        }
        true
    }

    fn visual_mode(&mut self, action: Action, extra: Extra, registers: &mut Registers) -> bool {
        if let Some(motion) = self.motion(action, extra.times(), extra.char, None) {
            self.cursor = motion.to;
            self.clamp();
            return true;
        }
        if is_motion(action) {
            return true;
        }
        let selected = self.selection().unwrap_or(self.cursor..self.cursor);
        if action == Action::ToggleCase || Operator::of(action).is_some() {
            self.reselect = Some(self.reselect_of());
        }
        match action {
            Action::OtherEnd => std::mem::swap(&mut self.anchor, &mut self.cursor),
            Action::NormalMode | Action::VisualMode => {
                self.mode = Mode::Normal;
                self.clamp();
            }
            Action::ToggleCase => {
                self.mode = Mode::Normal;
                // Undo puts the cursor back where the change starts.
                self.cursor = selected.start;
                if selected.is_empty() {
                    // As vim's, which saves for undo before it looks.
                    self.save_undo();
                }
                self.toggle_case(selected.clone());
                self.clamp();
            }
            _ => {
                let Some(operator) = Operator::of(action) else {
                    return false;
                };
                self.mode = Mode::Normal;
                self.operate(operator, selected, extra.register, registers);
            }
        }
        true
    }

    /// What a count before `v` selects, as vim's: as much as the last command of Visual mode
    /// took, `count` times, from the cursor; without one, `count` characters.
    fn select_count(&mut self, count: usize) {
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        self.cursor = match self.reselect {
            Some((lines, column)) if lines > 1 => {
                let line = line_index(&chars, self.cursor) + (lines * count).saturating_sub(1);
                let last = line_index(&chars, chars.len());
                let start = line_start(&chars, line.min(last)).unwrap_or(start);
                (start + column).min(line_bounds(&chars, start).1)
            }
            Some((_, columns)) => (self.cursor + (columns * count).saturating_sub(1)).min(end),
            None => (self.cursor + count.saturating_sub(1)).min(end),
        };
        self.clamp();
    }

    /// The lines the selection takes, and on one line its characters, else the column where it
    /// ends, as vim counts them, after the end of a line too: for a count before `v` later.
    fn reselect_of(&self) -> (usize, usize) {
        let chars = self.chars();
        let (first, last) = (self.anchor.min(self.cursor), self.anchor.max(self.cursor));
        let lines = line_index(&chars, last) - line_index(&chars, first) + 1;
        if lines > 1 {
            (lines, last - line_bounds(&chars, last).0)
        } else {
            (1, last - first + 1)
        }
    }

    /// Where `action` moves the cursor `count` times, if it is a motion that can; `operator`
    /// while one waits for it.
    fn motion(
        &mut self,
        action: Action,
        count: usize,
        c: Option<char>,
        operator: Option<Operator>,
    ) -> Option<Motion> {
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        let exclusive = |to| {
            Some(Motion {
                to,
                inclusive: false,
                keep_end: false,
            })
        };
        let inclusive = |to| {
            Some(Motion {
                to,
                inclusive: true,
                keep_end: false,
            })
        };
        // A count stops where the motion can go no further, however large.
        let repeat = |big: bool, step: fn(&[char], usize, bool) -> usize| {
            let to = (0..count).try_fold(self.cursor, |at, _| {
                let next = step(&chars, at, big);
                if next == at { Err(at) } else { Ok(next) }
            });
            to.unwrap_or_else(|at| at)
        };
        // Visual mode goes after the end of a line, as vim's with 'selection' inclusive.
        let past_end = self.mode == Mode::Visual;
        match action {
            Action::Left => exclusive(self.cursor.saturating_sub(count).max(start)),
            Action::Right => exclusive((self.cursor + count).min(end)),
            Action::CharForward => {
                Some(char_forward(&chars, self.cursor, count, operator, past_end))
            }
            Action::CharBackward => Some(char_backward(
                &chars,
                self.cursor,
                count,
                operator,
                past_end,
            )),
            Action::TextStart => exclusive(start),
            Action::FirstNonBlank => exclusive(line_first_non_blank(&chars, start)),
            Action::PrevLine | Action::NextLine | Action::ThisLine => {
                let start = line_start(&chars, self.line_motion(action, count)?)?;
                exclusive(line_first_non_blank(&chars, start))
            }
            // As vim's `$`, which is inclusive: an operator ends at the end of the line, even an
            // empty one.
            Action::TextEnd => Some(Motion {
                to: line_end_after(&chars, end, count)?,
                inclusive: false,
                keep_end: true,
            }),
            Action::WordForward | Action::BigWordForward => {
                let big = action == Action::BigWordForward;
                let (mut from, mut to) = (self.cursor, self.cursor);
                for _ in 0..count {
                    let next = word_start(&chars, to, big);
                    if next == to {
                        break;
                    }
                    (from, to) = (to, next);
                }
                // An operator stops at the end of the line of the last word it moves over, but
                // for an empty line, which ends at the start of the next one.
                if operator.is_some()
                    && !empty_line(&chars, from)
                    && let Some(newline) = chars[from..to].iter().position(|&c| c == '\n')
                {
                    to = from + newline;
                }
                exclusive(to)
            }
            Action::WordBackward | Action::BigWordBackward => {
                exclusive(repeat(action == Action::BigWordBackward, word_back))
            }
            Action::WordEnd | Action::BigWordEnd => {
                inclusive(repeat(action == Action::BigWordEnd, word_end))
            }
            Action::FindChar | Action::FindCharBack | Action::TillChar | Action::TillCharBack => {
                let c = c?;
                self.find = Some((action, c));
                find(&chars, self.cursor, (start, end), (action, c), count, false)
            }
            Action::RepeatFind | Action::RepeatFindBack => {
                let (found, c) = self.find?;
                let found = if action == Action::RepeatFind {
                    found
                } else {
                    reverse(found)
                };
                find(&chars, self.cursor, (start, end), (found, c), count, true)
            }
            _ => None,
        }
    }

    /// The line, counted from 0, that vim's `-`, `+`, or `_` goes to `count` times, if there is
    /// one: `-` and `+` fail on the first and the last line, and `_` with a count on the last;
    /// short of that, a count goes as far as there are lines.
    fn line_motion(&self, action: Action, count: usize) -> Option<usize> {
        let chars = self.chars();
        let line = line_index(&chars, self.cursor);
        let last = line_index(&chars, chars.len());
        let down = match action {
            Action::PrevLine => return (line > 0).then(|| line.saturating_sub(count)),
            Action::NextLine => count,
            Action::ThisLine => count - 1,
            _ => return None,
        };
        (down == 0 || line < last).then(|| line.saturating_add(down).min(last))
    }

    /// Does the operator of `pending` on the lines from the cursor's to `line`, as after vim's
    /// linewise motions: on one line as `d d`; from a line above, where the operator starts on
    /// its first character that is not blank, as the motion goes.
    fn lines_to(&mut self, pending: Pending, line: usize, registers: &mut Registers) {
        let chars = self.chars();
        let here = line_index(&chars, self.cursor);
        if line < here {
            let start = line_start(&chars, line).unwrap_or(0);
            self.cursor = line_first_non_blank(&chars, start);
        }
        let count = here.abs_diff(line) + 1;
        self.back_to_first_non_blank(pending.operator, count);
        self.lines_operate(pending, count, registers);
    }

    /// Does `operator` on `range`: keeps its text in the registers, and deletes it, then goes
    /// to Insert mode for a change.
    fn operate(
        &mut self,
        operator: Operator,
        range: Range<usize>,
        register: Option<char>,
        registers: &mut Registers,
    ) {
        if range.is_empty() {
            self.operate_on_nothing(operator, range.start, false, register, registers);
            return;
        }
        let taken: String = self.chars()[range.clone()].iter().collect();
        registers.keep(
            register,
            Register {
                text: taken,
                lines: false,
            },
            operator != Operator::Yank,
        );
        // Undo puts the cursor back where the change starts.
        self.cursor = range.start;
        match operator {
            Operator::Yank => {}
            Operator::Delete => {
                self.begin_change();
                self.remove(range.start, range.end);
                self.after_once_at_end();
            }
            Operator::Change => self.change_range(range.start, range.end, range.start),
        }
        self.clamp();
    }

    /// Does `operator` at `at`, where its motion or the selection takes no character, as vim's:
    /// a yank keeps the empty text; a delete or a change is one to undo all the same, but for a
    /// delete where the text has no line. An `inclusive` motion that takes the end of an empty
    /// line, as `$` does there, is something to vim: a delete gives up, and a change keeps the
    /// empty text it takes.
    fn operate_on_nothing(
        &mut self,
        operator: Operator,
        at: usize,
        inclusive: bool,
        register: Option<char>,
        registers: &mut Registers,
    ) {
        let nothing = Register::default();
        self.cursor = at;
        match operator {
            Operator::Yank => registers.keep(register, nothing, false),
            Operator::Delete => {
                if !inclusive && !self.no_line {
                    self.save_undo();
                }
                self.after_once_at_end();
            }
            Operator::Change => {
                if inclusive && !self.no_line {
                    registers.keep(register, nothing, true);
                }
                self.change_range(at, at, at);
            }
        }
        self.clamp();
    }

    /// Does the operator of `pending` on `range`, which `motion` moved over, as vim adjusts it
    /// on lines: an exclusive motion that ends at the start of a later line ends at the end of
    /// the line before, or takes the lines whole from the indent (`:help exclusive`); and a
    /// delete over lines from the indent to the end of a line takes them whole (`:help d`).
    fn operate_motion(
        &mut self,
        pending: Pending,
        range: Range<usize>,
        motion: Motion,
        registers: &mut Registers,
    ) {
        let chars = self.chars();
        let mut range = range;
        if self.has_lines()
            && !motion.inclusive
            && !motion.keep_end
            && range.end > range.start
            && chars[range.end - 1] == '\n'
        {
            if in_indent(&chars, range.start) {
                let lines = chars[range.clone()].iter().filter(|&&c| c == '\n').count();
                self.cursor = range.start;
                self.lines_operate(pending, lines, registers);
                return;
            }
            range.end -= 1;
        }
        if pending.operator == Operator::Delete
            && self.has_lines()
            && chars[range.clone()].contains(&'\n')
            && in_indent(&chars, range.start)
            && blank_to_line_end(&chars, range.end)
        {
            let lines = line_index(&chars, range.end) - line_index(&chars, range.start) + 1;
            self.cursor = range.start;
            self.lines_operate(pending, lines, registers);
            return;
        }
        if range.is_empty() {
            let inclusive = motion.inclusive || motion.keep_end;
            let Pending {
                operator, register, ..
            } = pending;
            self.operate_on_nothing(operator, range.start, inclusive, register, registers);
            return;
        }
        self.operate(pending.operator, range, pending.register, registers);
    }

    /// After Ctrl+o, a change that leaves the cursor after the end of its line, where Normal
    /// mode cannot have it, goes back to Insert mode there, as vim's does.
    fn after_once_at_end(&mut self) {
        if self.once.is_none() {
            return;
        }
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        if self.cursor == end && end > start {
            self.once_after = true;
        }
    }

    /// Deletes `start..end`, then Insert mode at `start`; one change with what it types, which
    /// undo takes back with the cursor at `undo_at`, even with nothing to delete, as in vim.
    fn change_range(&mut self, start: usize, end: usize, undo_at: usize) {
        self.cursor = undo_at;
        if start < end {
            self.begin_change();
            self.remove(start, end);
        } else {
            self.save_undo();
        }
        self.mode = Mode::Insert;
        self.changed = true;
        self.cursor = start;
        self.begin_insert(start);
    }

    /// Does the operator of `pending` on `count` lines from the cursor's, as `d d`, `c c`, and
    /// `y y` do.
    fn lines_operate(&mut self, pending: Pending, count: usize, registers: &mut Registers) {
        let chars = self.chars();
        let (start, mut end) = line_bounds(&chars, self.cursor);
        // As in vim, more lines than one from the last line go nowhere.
        if count > 1 && end >= chars.len() {
            return;
        }
        for _ in 1..count {
            if end >= chars.len() {
                break;
            }
            end = line_bounds(&chars, end + 1).1;
        }
        // Where the text has no line, vim's delete and change take none.
        if !self.no_line || pending.operator == Operator::Yank {
            let text: String = chars[start..end].iter().collect();
            registers.keep(
                pending.register,
                Register { text, lines: true },
                pending.operator != Operator::Yank,
            );
        }
        match pending.operator {
            Operator::Yank => {}
            Operator::Delete if self.no_line => {}
            Operator::Delete => {
                self.begin_change();
                // The line break after the lines goes with them, or else the one before.
                let (from, to) = if end < chars.len() {
                    (start, end + 1)
                } else {
                    (start.saturating_sub(1), end)
                };
                self.remove(from, to);
                // Every line deleted, the text has none left, as vim's buffer.
                self.no_line = start == 0 && end == chars.len();
                let chars = self.chars();
                let (start, end) = line_bounds(&chars, from.min(chars.len()));
                self.cursor = first_non_blank(&chars, start, end);
            }
            Operator::Change => self.change_range(start, end, self.cursor),
        }
        self.clamp();
    }

    /// Before `d d`, `c c`, or `S` on `count` lines, as vim's: on one line, the cursor goes back
    /// to the first character that is not blank if it is after it ('startofline'), which is
    /// where undo puts it back; on more, it stays, before the line where the motion goes.
    fn back_to_first_non_blank(&mut self, operator: Operator, count: usize) {
        if operator == Operator::Yank || count > 1 {
            return;
        }
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        self.cursor = self.cursor.min(first_non_blank(&chars, start, end));
    }

    /// Puts the register `name` names after the cursor, or before it, `count` times.
    fn put(&mut self, after: bool, count: usize, name: Option<char>, registers: &Registers) {
        // An empty line is something to put too.
        let Some(register) = registers
            .get(name)
            .filter(|register| register.lines || !register.text.is_empty())
        else {
            // As vim's, which saves for undo before it looks at the register.
            self.save_undo();
            return;
        };
        // Lines put more than once stay lines.
        let separator = if register.lines { "\n" } else { "" };
        let text = vec![register.text.as_str(); count].join(separator);
        let chars = self.chars();
        if register.lines && self.has_lines() {
            self.begin_change();
            let (start, end) = line_bounds(&chars, self.cursor);
            let (at, put) = if after {
                (end, format!("\n{text}"))
            } else {
                (start, format!("{text}\n"))
            };
            let offset = self.offset(at);
            self.text.insert_str(offset, &put);
            // As in vim, on the first character of the first line put that is not blank.
            let chars = self.chars();
            let first = if after { end + 1 } else { start };
            self.cursor = first_non_blank(&chars, first, line_bounds(&chars, first).1);
        } else {
            let text = if self.has_lines() {
                text
            } else {
                text.replace('\n', " ")
            };
            if text.is_empty() {
                return;
            }
            self.begin_change();
            let at = if after && !chars.is_empty() && chars.get(self.cursor) != Some(&'\n') {
                self.cursor + 1
            } else {
                self.cursor
            };
            let offset = self.offset(at.min(chars.len()));
            self.text.insert_str(offset, &text);
            self.cursor = at + text.chars().count() - 1;
            // As vim's Ctrl+o p, which goes back to Insert mode after the text put.
            if self.once.is_some() && !text.contains('\n') {
                self.once_after = true;
            }
        }
        self.clamp();
    }

    fn start_insert(&mut self, at: usize) {
        self.cursor = at;
        self.mode = Mode::Insert;
        self.changed = false;
        self.begin_insert(at);
    }

    /// Starts an insert at `at`, as Insert mode starts: Ctrl+w and Ctrl+u stop there once.
    fn begin_insert(&mut self, at: usize) {
        self.insert = Insert::at(at);
    }

    /// A move of the cursor in Insert mode, as vim's arrow keys: the end of the change for
    /// undo; the next change starts a new insert.
    fn insert_moved(&mut self) {
        self.insert.moved = true;
        self.changed = false;
    }

    fn next_insert_key(&mut self) {
        self.insert.next_key();
    }

    fn insert_changes(&mut self) {
        self.insert.changes(self.cursor);
    }

    /// Toggles the case in `range`; a change for undo even if nothing has a case, as in vim.
    fn toggle_case(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let (from, to) = (self.offset(range.start), self.offset(range.end));
        let toggled: String = self.text[from..to].chars().map(toggled_case).collect();
        self.begin_change();
        self.text.replace_range(from..to, &toggled);
    }

    /// Keeps the text as it is for undo, before a change; once for all that Insert mode types.
    fn begin_change(&mut self) {
        self.save_undo();
        self.no_line = false;
    }

    /// Keeps the text as it is for undo, as [`begin_change`](Self::begin_change), for a command
    /// that may change nothing: vim's change of nothing is one to undo all the same.
    fn save_undo(&mut self) {
        if self.is_secret() || self.mode == Mode::Insert && self.changed {
            return;
        }
        self.undo.push(self.snapshot());
        if self.undo.len() > UNDO_LEVELS {
            self.undo.remove(0);
        }
        self.redo.clear();
        if self.mode == Mode::Insert {
            self.changed = true;
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.snapshot_at(self.cursor)
    }

    /// The text as it is, with the cursor at `cursor`.
    fn snapshot_at(&self, cursor: usize) -> Snapshot {
        let chars = self.chars();
        let (start, _) = line_bounds(&chars, cursor);
        Snapshot {
            text: self.text.as_str().to_owned(),
            cursor: (line_index(&chars, cursor), cursor.min(chars.len()) - start),
            no_line: self.no_line,
        }
    }

    /// The text of `snapshot`, with the cursor on the line and in the column it keeps, as far as
    /// they go; past the last line, on the first character of the last one that is not blank,
    /// as vim's undo puts it with 'startofline'.
    fn restore(&mut self, snapshot: &Snapshot) {
        self.text.clear();
        self.text.push_str(&snapshot.text);
        self.no_line = snapshot.no_line;
        let chars = self.chars();
        let (line, column) = snapshot.cursor;
        self.cursor = if let Some(start) = line_start(&chars, line) {
            (start + column).min(line_bounds(&chars, start).1)
        } else {
            let (start, end) = line_bounds(&chars, chars.len());
            first_non_blank(&chars, start, end)
        };
        // After Ctrl+o, as vim's `restart_edit`, the cursor may stay after the end of the line,
        // where Insert mode goes on.
        if self.once.is_none() {
            self.clamp();
        }
    }

    /// Takes back `count` changes. Undo and redo both leave the cursor where the change started,
    /// as vim does.
    fn undo(&mut self, count: usize) {
        for _ in 0..count {
            let Some(snapshot) = self.undo.pop() else {
                return;
            };
            self.redo.push(Snapshot {
                text: self.text.as_str().to_owned(),
                cursor: snapshot.cursor,
                no_line: self.no_line,
            });
            self.restore(&snapshot);
        }
    }

    fn redo(&mut self, count: usize) {
        for _ in 0..count {
            let Some(snapshot) = self.redo.pop() else {
                return;
            };
            self.undo.push(Snapshot {
                text: self.text.as_str().to_owned(),
                cursor: snapshot.cursor,
                no_line: self.no_line,
            });
            self.restore(&snapshot);
        }
    }

    /// Keeps the cursor on a character in Normal mode, as vim does: not after the end of a line
    /// that has characters. Insert mode may have it there, and Visual mode too, as vim's with
    /// 'selection' inclusive, where it selects the line break.
    fn clamp(&mut self) {
        self.cursor = self.cursor.min(self.len());
        if self.mode != Mode::Normal {
            return;
        }
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        if end > start && self.cursor >= end {
            self.cursor = end - 1;
        }
    }

    /// The characters of the text, in memory reserved for all of them up front and wiped when
    /// it goes, so that the copy of a secret is never left behind.
    fn chars(&self) -> Zeroizing<Vec<char>> {
        let mut chars = Zeroizing::new(Vec::with_capacity(self.text.len()));
        chars.extend(self.text.chars());
        chars
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn char_at(&self, index: usize) -> Option<char> {
        self.text.chars().nth(index)
    }

    /// The byte offset of character `index`.
    fn offset(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(offset, _)| offset)
    }

    /// Removes the characters in `start..end` as a change, and leaves the cursor at `start`.
    fn delete(&mut self, start: usize, end: usize) {
        if start < end {
            self.begin_change();
            self.remove(start, end);
        }
    }

    /// Removes the characters in `start..end`, and leaves the cursor at `start`.
    fn remove(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        let (from, to) = (self.offset(start), self.offset(end));
        self.text.replace_range(from..to, "");
        self.cursor = start;
    }
}

/// The first character of the line that character `at` of `chars` is on, and the end of that
/// line: its line break, or the end of the text.
pub(crate) fn line_bounds(chars: &[char], at: usize) -> (usize, usize) {
    let at = at.min(chars.len());
    let start = chars[..at]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |newline| newline + 1);
    let end = chars[at..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |newline| at + newline);
    (start, end)
}

/// What kind of character `c` is for vim's words: a blank, a letter or a digit or `_`, or
/// another character; for its WORDs, a blank or not.
fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// The end of the line that ends at `end`, or of the line `count` - 1 lines after it, as vim's
/// `$` with a count goes; the end of the text if there are fewer lines, and none from the last
/// line, where vim's goes nowhere.
fn line_end_after(chars: &[char], end: usize, count: usize) -> Option<usize> {
    if count > 1 && end >= chars.len() {
        return None;
    }
    let end = (1..count).try_fold(end, |end, _| {
        if end < chars.len() {
            Ok(line_bounds(chars, end + 1).1)
        } else {
            Err(end)
        }
    });
    Some(end.unwrap_or_else(|end| end))
}

/// The line, counted from 0, that character `at` of `chars` is on.
fn line_index(chars: &[char], at: usize) -> usize {
    chars[..at.min(chars.len())]
        .iter()
        .filter(|&&c| c == '\n')
        .count()
}

/// Whether character `index` of `chars` is where an empty line is, which vim's `w` and `b` take
/// for a word of its own: a line break right after another one, or after nothing.
fn empty_line(chars: &[char], index: usize) -> bool {
    let starts = index == 0 || chars.get(index - 1) == Some(&'\n');
    starts && chars.get(index).is_none_or(|&c| c == '\n') && !chars.is_empty()
}

/// Where the next word after character `at` starts, or the end of the text: vim's `w`. An
/// empty line is a word.
/// Where Shift+Right and Ctrl+Right go on vim's command line: on at least one character, to
/// the next space, or the end. A line break counts as a space.
fn command_word_forward(chars: &[char], at: usize) -> usize {
    let mut to = at;
    while to < chars.len() {
        to += 1;
        if chars.get(to).is_some_and(|&c| c == ' ' || c == '\n') {
            break;
        }
    }
    to
}

/// Where Shift+Left and Ctrl+Left go on vim's command line: back at least one character, to
/// the start of the text after a space, or the start.
fn command_word_back(chars: &[char], at: usize) -> usize {
    let mut to = at.saturating_sub(1);
    while to > 0 && !matches!(chars[to - 1], ' ' | '\n') {
        to -= 1;
    }
    to
}

fn word_start(chars: &[char], at: usize, big: bool) -> usize {
    let mut index = at;
    if let Some(&c) = chars.get(index) {
        let kind = class(c, big);
        while kind != 0 && chars.get(index).is_some_and(|&c| class(c, big) == kind) {
            index += 1;
        }
    }
    while chars.get(index).is_some_and(|&c| class(c, big) == 0) {
        index += 1;
        if empty_line(chars, index) {
            return index;
        }
    }
    index.min(chars.len())
}

/// Where the word before character `at` starts: vim's `b`. An empty line is a word.
fn word_back(chars: &[char], at: usize, big: bool) -> usize {
    let mut index = at.min(chars.len());
    while index > 0 && class(chars[index - 1], big) == 0 {
        index -= 1;
        if empty_line(chars, index) {
            return index;
        }
    }
    if index == 0 {
        return 0;
    }
    let kind = class(chars[index - 1], big);
    while index > 0 && class(chars[index - 1], big) == kind {
        index -= 1;
    }
    index
}

/// Where the word ends that character `at` is before, or in: vim's `e`. Without one, the end
/// of the text, where vim's stops: Visual mode keeps the cursor there, Normal mode puts it back
/// on the last character.
fn word_end(chars: &[char], at: usize, big: bool) -> usize {
    let mut index = at + 1;
    while chars.get(index).is_some_and(|&c| class(c, big) == 0) {
        index += 1;
    }
    let Some(&c) = chars.get(index) else {
        return chars.len();
    };
    let kind = class(c, big);
    while chars.get(index + 1).is_some_and(|&c| class(c, big) == kind) {
        index += 1;
    }
    index
}

/// `c` in the other case, if that is one character too: vim's `~` never changes how many
/// characters there are.
fn toggled_case(c: char) -> char {
    let other: Vec<char> = if c.is_uppercase() {
        c.to_lowercase().collect()
    } else {
        c.to_uppercase().collect()
    };
    match other[..] {
        [one] => one,
        _ => c,
    }
}

/// How many times an operator and its motion go: their counts multiplied, as far as a count
/// goes.
fn times(motion: usize, operator: usize) -> usize {
    motion.saturating_mul(operator).min(MAX_COUNT)
}

/// Where vim's Space goes `count` times: a character right, and from the last character of a
/// line to the start of the next, as 'whichwrap' has it; `past_end`, as in Visual mode, after
/// the last character first. An operator takes the last character first, then the line break.
fn char_forward(
    chars: &[char],
    cursor: usize,
    count: usize,
    operator: Option<Operator>,
    past_end: bool,
) -> Motion {
    let mut to = cursor;
    let mut inclusive = false;
    for _ in 0..count {
        let (start, end) = line_bounds(chars, to);
        if to + 1 < end || past_end && to < end {
            to += 1;
        } else if end >= chars.len() {
            // On the last line, as `l`.
            to = end;
            break;
        } else if operator.is_some() && !inclusive && end > start {
            inclusive = true;
        } else {
            to = end + 1;
            inclusive = false;
        }
    }
    Motion {
        to,
        inclusive,
        keep_end: false,
    }
}

/// Where vim's Backspace goes `count` times: a character left, and from the start of a line to
/// the end of the line before, as 'whichwrap' has it; `d` and `c` go onto its line break, which
/// they take, and so does `past_end`, as Visual mode.
fn char_backward(
    chars: &[char],
    cursor: usize,
    count: usize,
    operator: Option<Operator>,
    past_end: bool,
) -> Motion {
    let mut to = cursor;
    let mut keep_end = false;
    for _ in 0..count {
        let (start, _) = line_bounds(chars, to);
        if to > start {
            to -= 1;
            continue;
        }
        if start == 0 {
            break;
        }
        let line_break = start - 1;
        let (before, _) = line_bounds(chars, line_break);
        let takes_break = matches!(operator, Some(Operator::Delete | Operator::Change));
        if takes_break && line_break > before {
            to = line_break;
            keep_end = true;
        } else if past_end {
            to = line_break;
        } else {
            to = line_break.saturating_sub(1).max(before);
        }
    }
    Motion {
        to,
        inclusive: false,
        keep_end,
    }
}

/// Whether only blanks are before character `at` on its line, as vim's `inindent`.
fn in_indent(chars: &[char], at: usize) -> bool {
    let (start, _) = line_bounds(chars, at);
    chars[start..at]
        .iter()
        .all(|&c| c != '\n' && c.is_whitespace())
}

/// Whether only blanks are from character `at` to the end of its line.
fn blank_to_line_end(chars: &[char], at: usize) -> bool {
    let (_, end) = line_bounds(chars, at);
    chars[at.min(end)..end].iter().all(|c| c.is_whitespace())
}

/// The first character of the line in `start..end` that is not blank, or its end.
/// Where line `line` of `chars` starts, counted from 0, if it has that many.
fn line_start(chars: &[char], line: usize) -> Option<usize> {
    match line.checked_sub(1) {
        None => Some(0),
        Some(breaks) => chars
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c == '\n')
            .nth(breaks)
            .map(|(index, _)| index + 1),
    }
}

fn first_non_blank(chars: &[char], start: usize, end: usize) -> usize {
    (start..end)
        .find(|&index| !chars[index].is_whitespace())
        .unwrap_or(end)
}

/// The first character that is not blank of the line that starts at `start`, as vim's `^`
/// goes: on a line of blanks, the last of them.
fn line_first_non_blank(chars: &[char], start: usize) -> usize {
    let (start, end) = line_bounds(chars, start);
    first_non_blank(chars, start, end).min(end.saturating_sub(1).max(start))
}

/// The motion of `f`, `F`, `t`, or `T` for `c`, `count` times, within the line `bounds`; `again`
/// for `;` and `,`, whose `t` and `T` without a count do not stop right before the character
/// they stopped at.
fn find(
    chars: &[char],
    cursor: usize,
    (start, end): (usize, usize),
    (action, c): (Action, char),
    count: usize,
    again: bool,
) -> Option<Motion> {
    let forward = matches!(action, Action::FindChar | Action::TillChar);
    let till = matches!(action, Action::TillChar | Action::TillCharBack);
    // As vim's without `cpo-;`: only one `;` stops at the character after the one it stopped
    // before.
    let skip = usize::from(till && again && count == 1);
    let found = if forward {
        (cursor + 1 + skip..end)
            .filter(|&index| chars[index] == c)
            .nth(count - 1)?
    } else {
        (start..cursor.saturating_sub(skip))
            .rev()
            .filter(|&index| chars[index] == c)
            .nth(count - 1)?
    };
    let to = match (forward, till) {
        (true, true) => found - 1,
        (false, true) => found + 1,
        (_, false) => found,
    };
    Some(Motion {
        to,
        inclusive: forward,
        keep_end: false,
    })
}

/// Whether `action` is one of vim's motions, which a text moves its cursor by, even where it
/// cannot go.
fn is_motion(action: Action) -> bool {
    matches!(
        action,
        Action::Left
            | Action::Right
            | Action::CharForward
            | Action::CharBackward
            | Action::TextStart
            | Action::FirstNonBlank
            | Action::TextEnd
            | Action::WordForward
            | Action::WordBackward
            | Action::WordEnd
            | Action::BigWordForward
            | Action::BigWordBackward
            | Action::BigWordEnd
            | Action::FindChar
            | Action::FindCharBack
            | Action::TillChar
            | Action::TillCharBack
            | Action::RepeatFind
            | Action::RepeatFindBack
    ) || is_line_motion(action)
}

/// Whether `action` is one of vim's motions over lines: `-`, `+`, and `_`.
fn is_line_motion(action: Action) -> bool {
    matches!(
        action,
        Action::PrevLine | Action::NextLine | Action::ThisLine
    )
}

/// The find of `action`, the other way.
fn reverse(action: Action) -> Action {
    match action {
        Action::FindChar => Action::FindCharBack,
        Action::FindCharBack => Action::FindChar,
        Action::TillChar => Action::TillCharBack,
        _ => Action::TillChar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An editor on `text` in Normal mode, with the cursor at `cursor`.
    fn at(text: &str, cursor: usize) -> Editor {
        let mut editor = Editor::new(text, Mode::Normal);
        editor.set_cursor(cursor);
        editor
    }

    fn press(editor: &mut Editor, registers: &mut Registers, action: Action) -> bool {
        editor.handle(action, Extra::default(), registers)
    }

    /// Runs `actions`, each with `extra`, which a later one replaces.
    fn run(editor: &mut Editor, registers: &mut Registers, actions: &[(Action, Extra)]) {
        for &(action, extra) in actions {
            assert!(editor.handle(action, extra, registers), "{action:?}");
        }
    }

    fn plain(action: Action) -> (Action, Extra) {
        (action, Extra::default())
    }

    fn counted(action: Action, count: usize) -> (Action, Extra) {
        (
            action,
            Extra {
                count: Some(count),
                ..Extra::default()
            },
        )
    }

    fn char(action: Action, c: char) -> (Action, Extra) {
        (
            action,
            Extra {
                char: Some(c),
                ..Extra::default()
            },
        )
    }

    fn line_register(text: &str) -> Register {
        Register {
            text: text.to_owned(),
            lines: true,
        }
    }

    fn text_register(text: &str) -> Register {
        Register {
            text: text.to_owned(),
            lines: false,
        }
    }

    #[test]
    fn motions_move_by_characters_words_and_finds() {
        let mut registers = Registers::default();
        let mut editor = at("foo.bar baz  qux", 0);
        let mut go = |action: (Action, Extra)| {
            run(&mut editor, &mut registers, &[action]);
            editor.cursor()
        };
        assert_eq!(
            go(plain(Action::WordForward)),
            3,
            "the dot is a word of its own"
        );
        assert_eq!(go(plain(Action::WordForward)), 4);
        assert_eq!(go(plain(Action::BigWordForward)), 8);
        assert_eq!(go(plain(Action::WordEnd)), 10);
        assert_eq!(go(counted(Action::WordBackward, 2)), 4);
        assert_eq!(go(plain(Action::BigWordBackward)), 0);
        assert_eq!(go(plain(Action::BigWordEnd)), 6);
        assert_eq!(go(plain(Action::TextEnd)), 15, "on the last character");
        assert_eq!(go(plain(Action::Right)), 15);
        assert_eq!(go(plain(Action::TextStart)), 0);
        assert_eq!(go(char(Action::FindChar, 'a')), 5);
        assert_eq!(go(plain(Action::RepeatFind)), 9);
        assert_eq!(go(plain(Action::RepeatFindBack)), 5);
        assert_eq!(go(char(Action::TillChar, 'z')), 9);
        assert_eq!(go(char(Action::FindCharBack, 'o')), 2);
        assert_eq!(go(char(Action::TillCharBack, 'f')), 1);
        assert_eq!(
            go(char(Action::FindChar, '!')),
            1,
            "nothing found: it stays"
        );
        assert_eq!(go(counted(Action::Right, 3)), 4);
        assert_eq!(go(plain(Action::WordForward)), 8);
        assert_eq!(go(plain(Action::FirstNonBlank)), 0);
        let mut indented = at("  ls", 3);
        run(
            &mut indented,
            &mut registers,
            &[plain(Action::FirstNonBlank)],
        );
        assert_eq!(indented.cursor(), 2);
    }

    #[test]
    fn operators_act_on_what_the_motion_moves_over() {
        let mut registers = Registers::default();
        let mut editor = at("one two three", 0);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::DeleteOperator), plain(Action::WordForward)],
        );
        assert_eq!(editor.text(), "two three");
        assert_eq!(registers.text(None), Some("one "));
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::ChangeOperator), plain(Action::WordForward)],
        );
        assert_eq!(editor.text(), " three", "cw changes to the end of the word");
        assert_eq!(editor.mode(), Mode::Insert);
        editor.insert('2');
        run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
        assert_eq!(editor.text(), "2 three");
        assert_eq!(editor.cursor(), 0);
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::YankOperator),
                char(Action::FindChar, 'h'),
                plain(Action::TextEnd),
                plain(Action::PutAfter),
            ],
        );
        assert_eq!(editor.text(), "2 three2 th");
        // 2d3l deletes six characters.
        let mut editor = at("abcdefgh", 0);
        run(
            &mut editor,
            &mut registers,
            &[
                counted(Action::DeleteOperator, 2),
                counted(Action::Right, 3),
            ],
        );
        assert_eq!(editor.text(), "gh");
        // Another key gives the operator up.
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::DeleteOperator)],
        );
        assert_eq!(editor.pending().as_deref(), Some("d"));
        assert!(press(&mut editor, &mut registers, Action::Cancel));
        assert_eq!(editor.pending(), None);
        assert!(
            !press(&mut editor, &mut registers, Action::Cancel),
            "Esc is the field's"
        );
        assert_eq!(editor.text(), "gh");
    }

    #[test]
    fn doubled_operators_act_on_lines() {
        let mut registers = Registers::default();
        let mut field = at("rename me", 3);
        run(
            &mut field,
            &mut registers,
            &[plain(Action::YankOperator), plain(Action::YankOperator)],
        );
        assert_eq!(registers.text(None), Some("rename me"));
        run(
            &mut field,
            &mut registers,
            &[plain(Action::ChangeOperator), plain(Action::ChangeOperator)],
        );
        assert_eq!(field.text(), "");
        assert_eq!(field.mode(), Mode::Insert);
        run(&mut field, &mut registers, &[plain(Action::NormalMode)]);
        run(&mut field, &mut registers, &[plain(Action::PutAfter)]);
        assert_eq!(
            field.text(),
            "rename me",
            "a line goes in as text in a field"
        );
        let mut lines = at("one\ntwo\nthree", 5).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[plain(Action::DeleteOperator), plain(Action::DeleteOperator)],
        );
        assert_eq!(lines.text(), "one\nthree");
        assert_eq!(lines.cursor(), 4);
        run(&mut lines, &mut registers, &[plain(Action::PutBefore)]);
        assert_eq!(lines.text(), "one\ntwo\nthree");
        assert_eq!(lines.cursor(), 4);
        lines.set_cursor(0);
        run(
            &mut lines,
            &mut registers,
            &[
                counted(Action::DeleteOperator, 2),
                plain(Action::DeleteOperator),
            ],
        );
        assert_eq!(lines.text(), "three");
        let mut last = at("one\ntwo", 5).with_lines();
        run(
            &mut last,
            &mut registers,
            &[plain(Action::DeleteOperator), plain(Action::DeleteOperator)],
        );
        assert_eq!(last.text(), "one", "the break before the last line goes");
    }

    #[test]
    fn changes_of_normal_mode() {
        let mut registers = Registers::default();
        let mut editor = at("hello world", 0);
        run(
            &mut editor,
            &mut registers,
            &[counted(Action::DeleteChar, 2)],
        );
        assert_eq!(editor.text(), "llo world");
        run(
            &mut editor,
            &mut registers,
            &[counted(Action::ToggleCase, 3)],
        );
        assert_eq!(editor.text(), "LLO world");
        assert_eq!(editor.cursor(), 3);
        run(&mut editor, &mut registers, &[plain(Action::Backspace)]);
        assert_eq!(editor.text(), "LL world");
        run(
            &mut editor,
            &mut registers,
            &[char(Action::ReplaceChar, '_')],
        );
        assert_eq!(editor.text(), "LL_world");
        run(&mut editor, &mut registers, &[plain(Action::DeleteToEnd)]);
        assert_eq!(editor.text(), "LL");
        assert_eq!(editor.cursor(), 1);
        run(&mut editor, &mut registers, &[plain(Action::Substitute)]);
        assert_eq!((editor.text(), editor.mode()), ("L", Mode::Insert));
        run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::SubstituteLine)],
        );
        assert_eq!(editor.text(), "");
        let mut editor = at("a b", 0);
        run(&mut editor, &mut registers, &[plain(Action::AppendAtEnd)]);
        assert_eq!((editor.cursor(), editor.mode()), (3, Mode::Insert));
        run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
        assert_eq!(editor.cursor(), 2, "Esc moves back onto the last character");
        run(&mut editor, &mut registers, &[plain(Action::ChangeToEnd)]);
        assert_eq!(editor.text(), "a ");
        run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
        run(&mut editor, &mut registers, &[plain(Action::InsertAtStart)]);
        assert_eq!(editor.cursor(), 0);
    }

    #[test]
    fn undo_takes_back_a_change_and_all_one_insert_typed() {
        let mut registers = Registers::default();
        let mut editor = Editor::new("abc", Mode::Insert);
        editor.insert('d');
        editor.insert('e');
        run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
        run(&mut editor, &mut registers, &[plain(Action::DeleteChar)]);
        assert_eq!(editor.text(), "abcd");
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(editor.text(), "abcde");
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(editor.text(), "abc", "what Insert mode typed, at once");
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(editor.text(), "abc", "nothing more");
        run(&mut editor, &mut registers, &[counted(Action::Redo, 2)]);
        assert_eq!(editor.text(), "abcd");
        // A change operator and what is typed after it are one change too.
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::TextStart),
                plain(Action::ChangeOperator),
                plain(Action::WordEnd),
            ],
        );
        editor.insert('x');
        run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
        assert_eq!(editor.text(), "x");
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(editor.text(), "abcd");
    }

    #[test]
    fn visual_mode_selects_and_operators_act_on_the_selection() {
        let mut registers = Registers::default();
        let mut editor = at("copy this text", 5);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::VisualMode), plain(Action::WordEnd)],
        );
        assert_eq!(editor.selection(), Some(5..9));
        run(&mut editor, &mut registers, &[plain(Action::OtherEnd)]);
        assert_eq!(editor.cursor(), 5);
        assert_eq!(editor.selection(), Some(5..9));
        run(&mut editor, &mut registers, &[plain(Action::YankOperator)]);
        assert_eq!(registers.text(None), Some("this"));
        assert_eq!(editor.mode(), Mode::Normal);
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::VisualMode),
                plain(Action::TextEnd),
                plain(Action::ChangeOperator),
            ],
        );
        assert_eq!((editor.text(), editor.mode()), ("copy ", Mode::Insert));
        let mut cased = at("abc", 0);
        run(
            &mut cased,
            &mut registers,
            &[
                plain(Action::VisualMode),
                plain(Action::Right),
                plain(Action::ToggleCase),
            ],
        );
        assert_eq!(cased.text(), "ABc");
        assert_eq!(cased.mode(), Mode::Normal);
    }

    #[test]
    fn registers_are_named_and_shared() {
        let mut registers = Registers::default();
        let named = |name| Extra {
            register: Some(name),
            ..Extra::default()
        };
        let mut one = at("alpha beta", 0);
        run(
            &mut one,
            &mut registers,
            &[
                (Action::YankOperator, named('a')),
                plain(Action::WordEnd),
                plain(Action::WordForward),
                (Action::YankOperator, named('A')),
                plain(Action::WordEnd),
            ],
        );
        assert_eq!(registers.text(Some('a')), Some("alphabeta"));
        assert_eq!(registers.text(Some('b')), None);
        let mut other = Editor::new("", Mode::Insert);
        run(
            &mut other,
            &mut registers,
            &[char(Action::PasteRegister, 'a')],
        );
        assert_eq!(other.text(), "alphabeta");
        run(&mut other, &mut registers, &[plain(Action::NormalMode)]);
        run(
            &mut other,
            &mut registers,
            &[(Action::PutBefore, named('a'))],
        );
        assert_eq!(other.text(), "alphabetalphabetaa");
        // A yank appended leaves the whole register in the unnamed one, and a delete only what
        // it deleted, as vim's register 1 holds it.
        assert_eq!(registers.text(None), Some("alphabeta"));
        let mut deletes = at("one two", 0);
        run(
            &mut deletes,
            &mut registers,
            &[
                (Action::DeleteChar, named('a')),
                (Action::DeleteChar, named('A')),
            ],
        );
        assert_eq!(registers.text(Some('a')), Some("on"));
        assert_eq!(registers.text(None), Some("n"));
    }

    #[test]
    fn insert_mode_edits_around_the_cursor() {
        let mut registers = Registers::default();
        let mut editor = Editor::new("echo one two", Mode::Insert);
        run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
        assert_eq!(editor.text(), "echo one ");
        run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
        assert_eq!(editor.text(), "echo ");
        run(&mut editor, &mut registers, &[plain(Action::WordBackward)]);
        assert_eq!(editor.cursor(), 0);
        run(&mut editor, &mut registers, &[plain(Action::WordForward)]);
        assert_eq!(editor.cursor(), 5);
        run(&mut editor, &mut registers, &[plain(Action::DeleteToStart)]);
        assert_eq!(editor.text(), "");
        // Ctrl+o: one command of Normal mode, then Insert mode again.
        let mut editor = Editor::new("one two", Mode::Insert);
        run(&mut editor, &mut registers, &[plain(Action::NormalOnce)]);
        assert_eq!(editor.mode(), Mode::Normal);
        run(&mut editor, &mut registers, &[plain(Action::WordBackward)]);
        assert_eq!((editor.cursor(), editor.mode()), (4, Mode::Insert));
        // A secret has no Normal mode.
        let mut secret = Editor::secret(16);
        assert!(!press(&mut secret, &mut registers, Action::NormalMode));
        secret.set_mode(Mode::Normal);
        assert_eq!(secret.mode(), Mode::Insert);
    }

    #[test]
    fn a_secret_never_grows_its_memory() {
        let mut secret = Editor::secret(4);
        for c in "secret".chars() {
            secret.insert(c);
        }
        assert_eq!(secret.text(), "secr");
        assert_eq!(format!("{secret:?}"), "Editor { mode: Insert, .. }");
        let capacity = secret.text.capacity();
        secret.replace("a much longer text than it can hold", 0);
        secret.insert_str("more than it can hold");
        assert_eq!((secret.text(), secret.text.capacity()), ("secr", capacity));
        // The characters it reads its text as never grow either, so no copy is left behind.
        let chars = secret.chars();
        assert!(chars.capacity() >= chars.len() && chars.capacity() <= secret.text.len());
    }

    #[test]
    fn cw_ends_the_word_under_the_cursor_where_it_is_on_its_last_character() {
        let mut registers = Registers::default();
        let change_word = |text: &str, cursor: usize, count: usize| {
            let mut editor = at(text, cursor);
            let mut registers = Registers::default();
            run(
                &mut editor,
                &mut registers,
                &[
                    plain(Action::ChangeOperator),
                    counted(Action::WordForward, count),
                ],
            );
            editor.text().to_owned()
        };
        assert_eq!(change_word("a b", 0, 1), " b", "a word of one letter");
        assert_eq!(change_word("ab cd", 1, 1), "a cd", "on its last letter");
        assert_eq!(change_word("a.b", 1, 1), "ab", "a dot is a word");
        assert_eq!(change_word("one two", 0, 1), " two");
        assert_eq!(change_word("a b c", 0, 2), " c", "c2w");
        assert_eq!(change_word("one  two", 3, 1), "onetwo", "on blanks, as dw");
        let mut big = at("a.b c", 0);
        run(
            &mut big,
            &mut registers,
            &[plain(Action::ChangeOperator), plain(Action::BigWordForward)],
        );
        assert_eq!(big.text(), " c");
    }

    #[test]
    fn ctrl_o_runs_one_command_whatever_mode_it_leaves() {
        let mut registers = Registers::default();
        // A command that goes to Insert mode itself leaves nothing behind.
        let mut editor = Editor::new("one two", Mode::Insert);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalOnce), plain(Action::AppendAtEnd)],
        );
        assert_eq!(editor.mode(), Mode::Insert);
        run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
        assert_eq!(editor.mode(), Mode::Normal, "Esc goes to Normal mode again");
        // Esc right after Ctrl+o goes back to Insert mode, as in vim, rather than to the owner.
        let mut editor = Editor::new("one two", Mode::Insert);
        run(&mut editor, &mut registers, &[plain(Action::NormalOnce)]);
        assert!(press(&mut editor, &mut registers, Action::Cancel));
        assert_eq!(editor.mode(), Mode::Insert);
        assert!(
            !press(&mut editor, &mut registers, Action::Cancel),
            "then the owner's"
        );
        // An operator after Ctrl+o, then Esc, gives the operator up first.
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalOnce), plain(Action::DeleteOperator)],
        );
        assert!(press(&mut editor, &mut registers, Action::Cancel));
        assert_eq!((editor.mode(), editor.text()), (Mode::Insert, "one two"));
        // Ctrl+o $ goes after the last character.
        let mut editor = at("one two", 0);
        run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalOnce), plain(Action::TextEnd)],
        );
        assert_eq!((editor.mode(), editor.cursor()), (Mode::Insert, 7));
        // What the keymap gives up, such as Ctrl+o and a key that does nothing, ends it too.
        run(&mut editor, &mut registers, &[plain(Action::NormalOnce)]);
        editor.give_up();
        assert_eq!(editor.mode(), Mode::Insert);
        let mut pending = at("abc def", 0);
        run(
            &mut pending,
            &mut registers,
            &[plain(Action::DeleteOperator)],
        );
        pending.give_up();
        run(&mut pending, &mut registers, &[plain(Action::WordForward)]);
        assert_eq!(pending.text(), "abc def", "d and Esc in the keymap, then w");
    }

    #[test]
    fn counts_go_as_far_as_text_goes_and_lines_stay_lines() {
        let mut registers = Registers::default();
        // A count of words stops at a line's end only in the last word.
        let mut lines = at("a\nb\nc d", 0).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[
                plain(Action::DeleteOperator),
                counted(Action::WordForward, 3),
            ],
        );
        assert_eq!(lines.text(), "d");
        // Lines put three times stay three lines.
        let mut lines = at("one\ntwo", 0).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[
                plain(Action::YankOperator),
                plain(Action::YankOperator),
                counted(Action::PutAfter, 3),
            ],
        );
        assert_eq!(lines.text(), "one\none\none\none\ntwo");
        let mut field = at("x", 0);
        run(&mut field, &mut registers, &[counted(Action::PutAfter, 2)]);
        assert_eq!(
            field.text(),
            "xone one",
            "lines in a field, apart by spaces"
        );
        // However large, a count stops where the motion can go no further.
        let mut editor = at("one two three", 0);
        let huge = Extra {
            count: Some(MAX_COUNT),
            ..Extra::default()
        };
        run(
            &mut editor,
            &mut registers,
            &[(Action::DeleteOperator, huge), (Action::WordForward, huge)],
        );
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn an_empty_line_is_a_word_for_w_and_b() {
        let mut registers = Registers::default();
        let mut lines = at("ab\n\ncd\n\n\nef", 0).with_lines();
        let mut go = |action: Action| {
            run(&mut lines, &mut registers, &[plain(action)]);
            lines.cursor()
        };
        assert_eq!(go(Action::WordForward), 3, "the empty line");
        assert_eq!(go(Action::WordForward), 4, "cd");
        assert_eq!(go(Action::WordForward), 7, "the first of two empty lines");
        assert_eq!(go(Action::WordForward), 8, "the second");
        assert_eq!(go(Action::WordForward), 9, "ef");
        assert_eq!(go(Action::WordBackward), 8);
        assert_eq!(go(Action::WordBackward), 7);
        assert_eq!(go(Action::WordBackward), 4);
        assert_eq!(go(Action::WordBackward), 3);
        assert_eq!(go(Action::WordBackward), 0);
        assert_eq!(go(Action::BigWordForward), 3, "for WORDs too");
        // A line with blanks is no empty line.
        let mut blanks = at("ab\n  \ncd", 0).with_lines();
        run(&mut blanks, &mut registers, &[plain(Action::WordForward)]);
        assert_eq!(blanks.cursor(), 6);
    }

    #[test]
    fn c_and_d_take_a_count_of_lines_as_dollar_does() {
        let mut registers = Registers::default();
        let mut lines = at("one\ntwo\nthree", 1).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[counted(Action::DeleteToEnd, 2)],
        );
        assert_eq!(lines.text(), "o\nthree");
        assert_eq!(registers.text(None), Some("ne\ntwo"));
        let mut lines = at("one\ntwo", 1).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[counted(Action::ChangeToEnd, 5)],
        );
        assert_eq!(
            (lines.text(), lines.mode()),
            ("o", Mode::Insert),
            "as far as there are lines"
        );
        // From the last line, more lines than one go nowhere, as in vim: in a field, always.
        let mut field = at("one two", 4);
        run(
            &mut field,
            &mut registers,
            &[counted(Action::DeleteToEnd, 3)],
        );
        assert_eq!(field.text(), "one two");
        let mut last = at("one\ntwo", 5).with_lines();
        run(
            &mut last,
            &mut registers,
            &[
                counted(Action::DeleteOperator, 2),
                plain(Action::DeleteOperator),
            ],
        );
        assert_eq!(last.text(), "one\ntwo", "2dd");
        run(&mut last, &mut registers, &[counted(Action::TextEnd, 2)]);
        assert_eq!(last.cursor(), 5, "2$ stays");
        last.set_cursor(0);
        run(
            &mut last,
            &mut registers,
            &[
                counted(Action::DeleteOperator, 9),
                plain(Action::DeleteOperator),
            ],
        );
        assert_eq!(
            last.text(),
            "",
            "9dd from the first line deletes to the end"
        );
    }

    #[test]
    fn toggling_case_keeps_the_characters_and_changes_nothing_else() {
        let mut registers = Registers::default();
        let mut editor = at("ßab", 0);
        run(
            &mut editor,
            &mut registers,
            &[counted(Action::ToggleCase, 2)],
        );
        assert_eq!((editor.text(), editor.cursor()), ("ßAb", 2));
        let mut digits = at("123", 0);
        run(
            &mut digits,
            &mut registers,
            &[counted(Action::ToggleCase, 3)],
        );
        run(&mut digits, &mut registers, &[plain(Action::DeleteChar)]);
        run(&mut digits, &mut registers, &[plain(Action::Undo)]);
        run(&mut digits, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(
            digits.text(),
            "123",
            "the toggle that changed nothing is no change"
        );
    }

    #[test]
    fn a_change_that_deletes_nothing_is_one_with_what_it_types() {
        let mut registers = Registers::default();
        let mut editor = at("ab", 0);
        run(&mut editor, &mut registers, &[plain(Action::DeleteChar)]);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::ChangeOperator), plain(Action::Left)],
        );
        assert_eq!(editor.mode(), Mode::Insert);
        editor.insert('x');
        run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
        assert_eq!(editor.text(), "xb");
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(editor.text(), "b", "c h and what it typed, one change");
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::ChangeOperator),
                plain(Action::Left),
                plain(Action::NormalMode),
                plain(Action::Undo),
            ],
        );
        assert_eq!(editor.text(), "b", "c h alone is a change to undo too");
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(editor.text(), "ab");
    }

    #[test]
    fn deletes_of_nothing_are_changes_to_undo_but_where_the_text_has_no_line() {
        let empty_then_undo = |keys: &[(Action, Extra)], cursor: usize| {
            let mut editor = at("abc", cursor);
            let mut registers = Registers::default();
            run(&mut editor, &mut registers, keys);
            run(&mut editor, &mut registers, &[plain(Action::Undo)]);
            editor.text().to_owned()
        };
        // Emptied by D, the line is still there: x on it takes a step of its own to undo.
        for (keys, cursor, text) in [
            (
                &[plain(Action::DeleteToEnd), plain(Action::DeleteChar)][..],
                0,
                "",
            ),
            (
                &[plain(Action::DeleteToEnd), plain(Action::Backspace)],
                0,
                "",
            ),
            (
                &[
                    plain(Action::DeleteToEnd),
                    plain(Action::VisualMode),
                    plain(Action::DeleteOperator),
                ],
                0,
                "",
            ),
            (
                &[
                    plain(Action::DeleteChar),
                    plain(Action::DeleteOperator),
                    plain(Action::TextStart),
                ],
                0,
                "bc",
            ),
            (
                &[
                    plain(Action::DeleteChar),
                    plain(Action::DeleteOperator),
                    plain(Action::Left),
                ],
                0,
                "bc",
            ),
            // D on an empty line has nothing to delete, and is no change.
            (
                &[plain(Action::DeleteToEnd), plain(Action::DeleteToEnd)],
                0,
                "abc",
            ),
            // d d deleted the last line: nothing is left to delete, and undo brings it back.
            (
                &[
                    plain(Action::DeleteOperator),
                    plain(Action::DeleteOperator),
                    plain(Action::DeleteChar),
                ],
                0,
                "abc",
            ),
            // C on an empty line, and v ~ even where the text has no line, are changes.
            (
                &[
                    plain(Action::DeleteToEnd),
                    plain(Action::ChangeToEnd),
                    plain(Action::NormalMode),
                ],
                0,
                "",
            ),
            (
                &[
                    plain(Action::DeleteOperator),
                    plain(Action::DeleteOperator),
                    plain(Action::VisualMode),
                    plain(Action::ToggleCase),
                ],
                0,
                "",
            ),
        ] {
            assert_eq!(empty_then_undo(keys, cursor), text, "{keys:?}");
        }
    }

    #[test]
    fn a_put_of_nothing_and_delete_at_the_end_are_changes_to_undo_too() {
        let mut registers = Registers::default();
        // A put from an empty register, as vim's, which saves for undo first.
        let mut editor = at("abc", 1);
        let empty = Extra {
            register: Some('b'),
            ..Extra::default()
        };
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::DeleteChar),
                (Action::PutBefore, empty),
                plain(Action::Undo),
            ],
        );
        assert_eq!(editor.text(), "ac");
        // Delete at the end of a field in Insert mode is a change too.
        let mut editor = at("abc", 1);
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::DeleteToEnd),
                plain(Action::Append),
                plain(Action::DeleteChar),
                plain(Action::NormalMode),
                plain(Action::Undo),
            ],
        );
        assert_eq!(editor.text(), "a");
    }

    #[test]
    fn what_there_is_nothing_to_take_from_goes_to_the_registers_as_vims() {
        let named = |name| Extra {
            register: Some(name),
            ..Extra::default()
        };
        let mut registers = Registers::default();
        let mut editor = at("abc", 0);
        run(
            &mut editor,
            &mut registers,
            &[
                (Action::YankOperator, named('a')),
                plain(Action::YankOperator),
                plain(Action::YankOperator),
                plain(Action::TextStart),
            ],
        );
        assert_eq!(
            registers.text(None),
            Some(""),
            "y 0 at the start yanks nothing"
        );
        assert_eq!(registers.text(Some('a')), Some("abc"));
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::DeleteToEnd),
                (Action::YankOperator, named('A')),
                plain(Action::TextEnd),
            ],
        );
        assert_eq!(
            registers.text(Some('a')),
            Some("abc\n"),
            "\"A y $ appends an empty line to lines"
        );
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::YankOperator), plain(Action::YankOperator)],
        );
        run(&mut editor, &mut registers, &[plain(Action::ChangeToEnd)]);
        assert_eq!(
            registers.get(None).cloned(),
            Some(text_register("")),
            "C on an empty line takes it"
        );
        // d d on the empty line takes it as a line, then the text has none: d d, c c, S, and C
        // take nothing more.
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::NormalMode),
                plain(Action::DeleteOperator),
                plain(Action::DeleteOperator),
            ],
        );
        assert_eq!(registers.get(None).cloned(), Some(line_register("")));
        assert!(editor.no_line);
        let kept = line_register("abc");
        for keys in [
            &[plain(Action::DeleteOperator), plain(Action::DeleteOperator)][..],
            &[
                (Action::ChangeOperator, named('a')),
                plain(Action::ChangeOperator),
                plain(Action::NormalMode),
            ],
            &[plain(Action::SubstituteLine), plain(Action::NormalMode)],
            &[plain(Action::ChangeToEnd), plain(Action::NormalMode)],
        ] {
            let mut editor = at("abc", 0);
            run(
                &mut editor,
                &mut registers,
                &[plain(Action::DeleteOperator), plain(Action::DeleteOperator)],
            );
            registers.keep(Some('a'), kept.clone(), false);
            run(&mut editor, &mut registers, keys);
            assert_eq!(registers.get(None).cloned(), Some(kept.clone()), "{keys:?}");
            assert_eq!(
                registers.get(Some('a')).cloned(),
                Some(kept.clone()),
                "{keys:?}"
            );
        }
    }

    #[test]
    fn visual_mode_goes_after_the_end_of_a_line_and_takes_its_line_break() {
        let visual = |text: &str, cursor: usize, keys: &[(Action, Extra)]| {
            let mut editor = at(text, cursor).with_lines();
            let mut registers = Registers::default();
            run(&mut editor, &mut registers, &[plain(Action::VisualMode)]);
            run(&mut editor, &mut registers, keys);
            (editor, registers)
        };
        let (editor, _) = visual("abc", 2, &[plain(Action::Right)]);
        assert_eq!(editor.cursor(), 3, "l from the last character");
        let (editor, _) = visual(
            "abc",
            2,
            &[
                plain(Action::Right),
                plain(Action::Left),
                plain(Action::DeleteOperator),
            ],
        );
        assert_eq!(editor.text(), "ab", "h comes back to the last character");
        let (_, registers) = visual(
            "abc",
            0,
            &[
                plain(Action::TextEnd),
                plain(Action::Left),
                plain(Action::YankOperator),
            ],
        );
        assert_eq!(registers.text(None), Some("abc"), "$ goes after the end");
        let (editor, _) = visual(
            "a.b c-d  ef",
            9,
            &[
                plain(Action::WordForward),
                plain(Action::Left),
                plain(Action::DeleteOperator),
            ],
        );
        assert_eq!(editor.text(), "a.b c-d  ", "w past the last word");
        let (editor, _) = visual("ab cd", 4, &[plain(Action::WordEnd)]);
        assert_eq!(editor.cursor(), 5, "e at the end");
        let (editor, _) = visual("ab\ncd", 1, &[plain(Action::CharForward)]);
        assert_eq!(editor.cursor(), 2, "Space at the last character");
        let (editor, _) = visual("ab\ncd", 1, &[counted(Action::CharForward, 2)]);
        assert_eq!(editor.cursor(), 3, "then the next line");
        let (editor, _) = visual("ab\ncd", 3, &[plain(Action::CharBackward)]);
        assert_eq!(
            editor.cursor(),
            2,
            "Backspace to the end of the line before"
        );
        // An operator takes the line break where a line follows, and nothing on the last line.
        let (editor, registers) = visual(
            "ab cd\nef",
            1,
            &[plain(Action::TextEnd), plain(Action::YankOperator)],
        );
        assert_eq!(registers.text(None), Some("b cd\n"));
        assert_eq!((editor.mode(), editor.cursor()), (Mode::Normal, 1));
        let (editor, _) = visual(
            "ab cd\nef",
            1,
            &[counted(Action::Right, 9), plain(Action::DeleteOperator)],
        );
        assert_eq!(editor.text(), "aef");
        let (editor, _) = visual(
            "abc",
            1,
            &[plain(Action::TextEnd), plain(Action::NormalMode)],
        );
        assert_eq!(
            editor.cursor(),
            2,
            "Esc puts the cursor back on a character"
        );
        // A count before v goes after the end as l does.
        let mut registers = Registers::default();
        let mut editor = at("ab", 1);
        run(
            &mut editor,
            &mut registers,
            &[
                counted(Action::VisualMode, 5),
                plain(Action::YankOperator),
                counted(Action::VisualMode, 1),
            ],
        );
        assert_eq!(editor.cursor(), 2, "the column after the end counts too");
    }

    #[test]
    fn after_ctrl_o_at_the_end_ctrl_w_and_ctrl_u_stop_once_before_the_last_character() {
        let mut registers = Registers::default();
        let yanked = |typed: &str, keys: &[Action]| {
            let mut editor = at("foo bar", 0);
            let mut registers = Registers::default();
            press(&mut editor, &mut registers, Action::AppendAtEnd);
            for c in typed.chars() {
                editor.insert(c);
            }
            let line = [
                Action::NormalOnce,
                Action::YankOperator,
                Action::YankOperator,
            ];
            for &action in line.iter().chain(keys) {
                press(&mut editor, &mut registers, action);
            }
            editor.text().to_owned()
        };
        assert_eq!(yanked("", &[Action::DeleteWord]), "foo ba");
        assert_eq!(yanked("", &[Action::DeleteToStart]), "foo ba");
        assert_eq!(yanked("xy", &[Action::DeleteWord]), "foo barx");
        assert_eq!(
            yanked("", &[Action::DeleteWord, Action::DeleteWord]),
            "foo "
        );
        assert_eq!(yanked("", &[Action::Backspace, Action::Backspace]), "foo b");
        // On an empty line too, as vim's.
        let mut editor = Editor::new("", Mode::Insert);
        registers.keep(None, text_register("x"), false);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalOnce), plain(Action::PutBefore)],
        );
        assert_eq!((editor.text(), editor.cursor()), ("x", 1));
    }

    #[test]
    fn minus_plus_and_underscore_go_to_lines_and_take_them_whole() {
        let text = "ab\n  cd\nef";
        let go = |cursor: usize, keys: &[(Action, Extra)]| {
            let mut editor = at(text, cursor).with_lines();
            let mut registers = Registers::default();
            run(&mut editor, &mut registers, keys);
            let taken = registers.get(None).cloned().unwrap_or_default();
            (editor.text().to_owned(), editor.cursor(), taken)
        };
        let moved = |cursor, keys: &[(Action, Extra)]| go(cursor, keys).1;
        assert_eq!(moved(4, &[plain(Action::PrevLine)]), 0);
        assert_eq!(moved(4, &[plain(Action::NextLine)]), 8);
        assert_eq!(moved(3, &[plain(Action::ThisLine)]), 5);
        assert_eq!(moved(4, &[counted(Action::ThisLine, 2)]), 8);
        assert_eq!(
            moved(0, &[counted(Action::NextLine, 5)]),
            8,
            "as far as lines go"
        );
        assert_eq!(moved(8, &[counted(Action::PrevLine, 3)]), 0);
        assert_eq!(moved(1, &[plain(Action::PrevLine)]), 1, "no line above");
        assert_eq!(moved(8, &[plain(Action::NextLine)]), 8, "no line below");
        assert_eq!(moved(8, &[counted(Action::ThisLine, 2)]), 8);
        let delete = |motion| (Action::DeleteOperator, Extra::default(), motion);
        for ((operator, extra, motion), cursor, want) in [
            (delete(plain(Action::ThisLine)), 4, ("ab\nef", 3, "  cd")),
            (
                delete(counted(Action::ThisLine, 2)),
                4,
                ("ab", 0, "  cd\nef"),
            ),
            (delete(plain(Action::PrevLine)), 4, ("ef", 0, "ab\n  cd")),
            (delete(plain(Action::NextLine)), 4, ("ab", 0, "  cd\nef")),
            (delete(plain(Action::NextLine)), 8, (text, 8, "")),
        ] {
            let got = go(cursor, &[(operator, extra), motion]);
            assert_eq!(
                (got.0.as_str(), got.1, got.2.text.as_str()),
                want,
                "{motion:?}"
            );
            assert!(got.2.lines || got.2.text.is_empty(), "{motion:?}");
        }
        let (_, cursor, taken) = go(4, &[plain(Action::YankOperator), plain(Action::PrevLine)]);
        assert_eq!(
            (cursor, taken),
            (0, line_register("ab\n  cd")),
            "y - goes up"
        );
        let (_, cursor, _) = go(4, &[plain(Action::YankOperator), plain(Action::NextLine)]);
        assert_eq!(cursor, 4, "y + stays");
        let mut editor = at(text, 4).with_lines();
        let mut registers = Registers::default();
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::ChangeOperator), plain(Action::PrevLine)],
        );
        editor.insert('x');
        assert_eq!(editor.text(), "x\nef", "c - as 2 c c from the line above");
        // In a field of one line, - and + fail, and _ stays on the line.
        let mut field = at("  ab", 3);
        run(&mut field, &mut registers, &[plain(Action::PrevLine)]);
        assert_eq!(field.cursor(), 3);
        run(&mut field, &mut registers, &[plain(Action::ThisLine)]);
        assert_eq!(field.cursor(), 2);
        run(
            &mut field,
            &mut registers,
            &[plain(Action::DeleteOperator), plain(Action::ThisLine)],
        );
        assert_eq!(field.text(), "", "d _ as d d");
    }

    #[test]
    fn the_black_hole_register_keeps_nothing() {
        let mut registers = Registers::default();
        let mut editor = at("keep gone", 0);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::YankOperator), plain(Action::WordEnd)],
        );
        let black_hole = Extra {
            register: Some('_'),
            ..Extra::default()
        };
        run(
            &mut editor,
            &mut registers,
            &[(Action::DeleteToEnd, black_hole)],
        );
        assert_eq!(registers.text(None), Some("keep"));
        assert_eq!(registers.text(Some('_')), None);
    }

    #[test]
    fn a_paste_is_one_change_in_any_mode_as_in_vim() {
        let mut registers = Registers::default();
        // In Normal mode as typed in Insert mode, then Esc: after the cursor, but at the start
        // of a line before it; the cursor goes to its last character.
        let mut editor = at("ab", 1);
        editor.paste("xyz", &mut registers);
        assert_eq!((editor.text(), editor.mode()), ("abxyz", Mode::Normal));
        assert_eq!(editor.cursor(), 4);
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(
            (editor.text(), editor.cursor()),
            ("ab", 1),
            "where the insert started"
        );
        let mut editor = at("ab", 0);
        editor.paste("xyz", &mut registers);
        assert_eq!((editor.text(), editor.cursor()), ("xyzab", 2));
        let mut empty = Editor::new("", Mode::Normal);
        empty.paste("xy", &mut registers);
        assert_eq!(empty.cursor(), 1, "on the last character, not after it");
        let mut lines = at("ab\ncd", 0).with_lines();
        lines.paste("x\n", &mut registers);
        assert_eq!(
            (lines.text(), lines.cursor()),
            ("x\nab\ncd", 2),
            "Esc stays at a line start"
        );
        // After Ctrl+o, Normal mode stays; an operator that waits is given up, and the paste.
        let mut once = after_ctrl_o("ab");
        once.paste("x", &mut registers);
        assert_eq!((once.text(), once.mode()), ("abx", Mode::Normal));
        let mut waits = at("ab", 0);
        run(&mut waits, &mut registers, &[plain(Action::DeleteOperator)]);
        waits.paste("x", &mut registers);
        assert_eq!(waits.text(), "ab");
        assert!(waits.pending().is_none());
        // In Visual mode in place of the selection, which the unnamed register keeps.
        let mut selected = at("abc def", 0);
        run(
            &mut selected,
            &mut registers,
            &[plain(Action::VisualMode), plain(Action::Right)],
        );
        selected.paste("xyz", &mut registers);
        assert_eq!(
            (selected.text(), selected.mode()),
            ("xyzc def", Mode::Normal)
        );
        assert_eq!(registers.text(None), Some("ab"));
        run(&mut selected, &mut registers, &[plain(Action::Undo)]);
        assert_eq!(selected.text(), "abc def", "one change");
        // In Insert mode at the cursor.
        let mut typing = Editor::new("ab", Mode::Insert);
        typing.paste("x", &mut registers);
        assert_eq!((typing.text(), typing.cursor()), ("abx", 3));
    }
    /// An editor in Insert mode on `text`, the cursor at its end, after Ctrl+o.
    fn after_ctrl_o(text: &str) -> Editor {
        let mut editor = Editor::new(text, Mode::Insert);
        assert!(editor.handle(
            Action::NormalOnce,
            Extra::default(),
            &mut Registers::default()
        ));
        editor
    }

    #[test]
    fn after_ctrl_o_0_and_home_go_on_before_the_last_character_as_vim() {
        let mut registers = Registers::default();
        for (text, key) in [
            ("x", Action::TextStart),
            ("x", Action::FirstNonBlank),
            (" x", Action::FirstNonBlank),
        ] {
            let mut editor = after_ctrl_o(text);
            run(&mut editor, &mut registers, &[plain(key)]);
            editor.insert('Q');
            assert_eq!(
                editor.text(),
                format!("{}Qx", &text[..text.len() - 1]),
                "{key:?}"
            );
        }
        // Another key that leaves the cursor on the last character goes on after it.
        let mut editor = after_ctrl_o("x");
        run(&mut editor, &mut registers, &[plain(Action::WordBackward)]);
        editor.insert('Q');
        assert_eq!(editor.text(), "xQ");
    }

    #[test]
    fn home_and_end_start_a_new_change_where_the_cursor_is_already_as_vim() {
        let mut registers = Registers::default();
        let mut editor = at("foo", 0);
        run(&mut editor, &mut registers, &[plain(Action::AppendAtEnd)]);
        editor.insert('X');
        run(&mut editor, &mut registers, &[plain(Action::TextEnd)]);
        editor.insert('Y');
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalMode), plain(Action::Undo)],
        );
        assert_eq!(editor.text(), "fooX");
        // Ctrl+w still stops where the insert started, as after a move to the right.
        let mut editor = at("foo", 0);
        run(&mut editor, &mut registers, &[plain(Action::AppendAtEnd)]);
        editor.insert('X');
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::TextEnd), plain(Action::DeleteWord)],
        );
        assert_eq!(editor.text(), "foo");
    }

    #[test]
    fn a_count_before_v_selects_as_much_as_the_last_visual_command_times_it() {
        let mut registers = Registers::default();
        let mut editor = at("abcdefgh", 1);
        run(
            &mut editor,
            &mut registers,
            &[
                counted(Action::VisualMode, 2),
                plain(Action::DeleteOperator),
            ],
        );
        assert_eq!(editor.text(), "adefgh", "without one, that many characters");
        let mut editor = at("abcdefgh abcdefgh", 0);
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::VisualMode),
                counted(Action::Right, 2),
                plain(Action::YankOperator),
                plain(Action::WordForward),
                counted(Action::VisualMode, 2),
            ],
        );
        assert_eq!(editor.selection(), Some(9..15), "three characters, twice");
        // Without a count, one character.
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalMode), plain(Action::VisualMode)],
        );
        assert_eq!(editor.selection(), Some(14..15));
    }

    #[test]
    fn ctrl_o_at_the_end_of_the_line_acts_on_its_last_character_as_vim() {
        type Case<'a> = (&'a [(Action, Extra)], &'a str, usize);
        let mut registers = Registers::default();
        let cases: [Case<'_>; 7] = [
            (&[plain(Action::DeleteChar)], "ab", 2),
            (&[plain(Action::DeleteToEnd)], "ab", 2),
            (&[plain(Action::ToggleCase)], "abC", 3),
            (&[char(Action::ReplaceChar, 'X')], "abX", 3),
            (&[plain(Action::Backspace)], "ac", 2),
            (&[plain(Action::Left)], "abc", 1),
            (&[plain(Action::Right)], "abc", 3),
        ];
        for (keys, text, cursor) in cases {
            let mut editor = after_ctrl_o("abc");
            run(&mut editor, &mut registers, keys);
            assert_eq!(editor.mode(), Mode::Insert, "{keys:?}");
            assert_eq!((editor.text(), editor.cursor()), (text, cursor), "{keys:?}");
        }
        let mut editor = after_ctrl_o("abc");
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::DeleteOperator), plain(Action::WordBackward)],
        );
        assert_eq!((editor.text(), editor.cursor()), ("c", 1), "d b");
        let mut editor = after_ctrl_o("abc");
        run(
            &mut editor,
            &mut registers,
            &[char(Action::Substitute, 'X')],
        );
        editor.insert('X');
        assert_eq!(editor.text(), "abX", "s");
        let mut editor = after_ctrl_o("abc def");
        run(&mut editor, &mut registers, &[plain(Action::WordEnd)]);
        assert_eq!(
            editor.cursor(),
            7,
            "e stays on the last character, then after it"
        );
        let mut editor = after_ctrl_o("abc def");
        run(
            &mut editor,
            &mut registers,
            &[char(Action::FindCharBack, 'f')],
        );
        assert_eq!(editor.cursor(), 7, "F f finds no other f");
        // From inside a line, the cursor stays where the command leaves it.
        let mut editor = Editor::new("abc", Mode::Insert);
        editor.set_cursor(1);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalOnce), plain(Action::Right)],
        );
        assert_eq!(editor.cursor(), 2);
    }

    #[test]
    fn an_empty_line_is_put_and_appended_as_a_line() {
        let mut registers = Registers::default();
        let mut lines = at("ab\n\ncd", 3).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[
                plain(Action::YankOperator),
                plain(Action::YankOperator),
                plain(Action::PutAfter),
            ],
        );
        assert_eq!(lines.text(), "ab\n\n\ncd", "y y p");
        run(&mut lines, &mut registers, &[counted(Action::PutAfter, 2)]);
        assert_eq!(lines.text(), "ab\n\n\n\n\ncd", "2p");
        let append = |name| Extra {
            register: Some(name),
            ..Extra::default()
        };
        let mut lines = at("\nfoo", 0).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[
                (Action::YankOperator, append('a')),
                plain(Action::YankOperator),
            ],
        );
        lines.set_cursor(1);
        run(
            &mut lines,
            &mut registers,
            &[
                (Action::YankOperator, append('A')),
                plain(Action::YankOperator),
            ],
        );
        assert_eq!(registers.text(Some('a')), Some("\nfoo"), "both lines");
        // In a field of one line, an empty line puts nothing.
        let mut field = at("x", 0);
        run(
            &mut field,
            &mut registers,
            &[(Action::PutAfter, append('z'))],
        );
        assert_eq!(field.text(), "x");
    }

    #[test]
    fn w_with_an_operator_takes_an_empty_line_whole() {
        let mut registers = Registers::default();
        let mut lines = at("ab\n\ncd", 3).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[plain(Action::DeleteOperator), plain(Action::WordForward)],
        );
        assert_eq!(lines.text(), "ab\ncd");
        assert_eq!(registers.text(None), Some(""), "an empty line");
        let mut lines = at("ab\n\ncd", 3).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[plain(Action::ChangeOperator), plain(Action::WordForward)],
        );
        assert_eq!((lines.text(), lines.mode()), ("ab\n\ncd", Mode::Insert));
    }

    #[test]
    fn ctrl_w_and_ctrl_u_stop_once_where_insert_mode_started() {
        let mut registers = Registers::default();
        let mut field = Editor::new("report.txt", Mode::Insert);
        field.set_cursor(6);
        for c in "_v2".chars() {
            field.insert(c);
        }
        run(&mut field, &mut registers, &[plain(Action::DeleteWord)]);
        assert_eq!(field.text(), "report.txt", "what was typed");
        run(&mut field, &mut registers, &[plain(Action::DeleteWord)]);
        assert_eq!(field.text(), ".txt", "then on");
        let mut field = Editor::new("foo bar", Mode::Insert);
        field.insert('x');
        run(&mut field, &mut registers, &[plain(Action::DeleteToStart)]);
        assert_eq!(field.text(), "foo bar");
        run(&mut field, &mut registers, &[plain(Action::DeleteToStart)]);
        assert_eq!(field.text(), "", "a field opens ready to be cleared");
        // On the command line, as on vim's, they do not stop; Ctrl+u at the start of a line
        // joins it to the line above, as Ctrl+w does.
        let mut lines = Editor::new("ab\ncd", Mode::Insert).with_lines();
        lines.insert('x');
        run(&mut lines, &mut registers, &[plain(Action::DeleteToStart)]);
        assert_eq!(lines.text(), "ab\n");
        run(&mut lines, &mut registers, &[plain(Action::DeleteToStart)]);
        assert_eq!(lines.text(), "ab");
    }

    #[test]
    fn moves_in_insert_mode_and_ctrl_u_start_a_new_change_and_redo_keeps_the_cursor() {
        let mut registers = Registers::default();
        let mut editor = at("abc", 0);
        run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
        editor.insert('x');
        run(&mut editor, &mut registers, &[plain(Action::Left)]);
        editor.insert('y');
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalMode), plain(Action::Undo)],
        );
        assert_eq!(editor.text(), "xabc", "the Left split the change");
        let mut editor = at("abc", 0);
        run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
        editor.insert('x');
        run(&mut editor, &mut registers, &[plain(Action::DeleteToStart)]);
        editor.insert('y');
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalMode), plain(Action::Undo)],
        );
        assert_eq!(
            editor.text(),
            "xabc",
            "Ctrl+u too, as vim's defaults.vim maps it"
        );
        let mut editor = at("abc", 0);
        run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
        editor.insert('x');
        run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
        editor.insert('y');
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalMode), plain(Action::Undo)],
        );
        assert_eq!(editor.text(), "abc", "but not Ctrl+w");
        // The focus coming back starts a new change.
        let mut editor = Editor::new("", Mode::Insert);
        editor.insert('a');
        editor.set_mode(Mode::Insert);
        editor.insert('b');
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::NormalMode), plain(Action::Undo)],
        );
        assert_eq!(editor.text(), "a");
        let mut editor = at("one two three", 4);
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::DeleteOperator),
                plain(Action::WordForward),
                plain(Action::TextStart),
                plain(Action::Undo),
                plain(Action::Redo),
            ],
        );
        assert_eq!((editor.text(), editor.cursor()), ("one three", 4));
    }

    #[test]
    fn the_operator_waiting_shows_as_typed_and_visual_mode_its_size() {
        let mut registers = Registers::default();
        let register = |outer: Option<usize>, count| Extra {
            count: Some(count),
            register: Some('a'),
            outer,
            char: None,
        };
        let mut editor = at("abc", 0);
        run(
            &mut editor,
            &mut registers,
            &[(Action::DeleteOperator, register(Some(2), 2))],
        );
        assert_eq!(editor.pending().as_deref(), Some("2\"ad"));
        editor.give_up();
        run(
            &mut editor,
            &mut registers,
            &[(Action::DeleteOperator, register(None, 2))],
        );
        assert_eq!(editor.pending().as_deref(), Some("\"a2d"));
        editor.give_up();
        run(
            &mut editor,
            &mut registers,
            &[(Action::DeleteOperator, register(Some(2), 6))],
        );
        assert_eq!(editor.pending().as_deref(), Some("2\"a3d"));
        editor.give_up();
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::VisualMode), plain(Action::Right)],
        );
        assert_eq!(editor.selection_size(), Some(2), "characters");
        let mut lines = at("ab\ncd\nef", 1).with_lines();
        run(
            &mut lines,
            &mut registers,
            &[plain(Action::VisualMode), char(Action::FindChar, 'x')],
        );
        lines.set_cursor(7);
        assert_eq!(lines.selection_size(), Some(3), "lines");
        lines.set_cursor(2);
        assert_eq!(lines.selection_size(), Some(2), "and the end of the line");
    }
    #[test]
    fn undo_and_redo_put_the_cursor_where_the_change_starts() {
        type Case<'a> = (&'a str, usize, &'a [(Action, Extra)], usize);
        let mut registers = Registers::default();
        let cases: [Case<'_>; 6] = [
            (
                "foo bar",
                5,
                &[plain(Action::DeleteOperator), plain(Action::WordBackward)],
                4,
            ),
            ("abcdef", 3, &[plain(Action::Backspace)], 2),
            (
                "abcdef",
                3,
                &[plain(Action::DeleteOperator), plain(Action::TextStart)],
                0,
            ),
            (
                "abcdef",
                3,
                &[
                    plain(Action::DeleteOperator),
                    char(Action::FindCharBack, 'a'),
                ],
                0,
            ),
            (
                "abcdef",
                0,
                &[
                    plain(Action::VisualMode),
                    counted(Action::Right, 2),
                    plain(Action::DeleteOperator),
                ],
                0,
            ),
            (
                "  abc",
                3,
                &[plain(Action::SubstituteLine), plain(Action::NormalMode)],
                2,
            ),
        ];
        for (text, cursor, keys, start) in cases {
            let mut editor = at(text, cursor);
            run(&mut editor, &mut registers, keys);
            run(&mut editor, &mut registers, &[plain(Action::Undo)]);
            assert_eq!((editor.text(), editor.cursor()), (text, start), "{keys:?}");
            run(
                &mut editor,
                &mut registers,
                &[plain(Action::Redo), plain(Action::Undo)],
            );
            assert_eq!(editor.cursor(), start, "{keys:?} again");
        }
        // Visual mode from its right end too.
        let mut editor = at("abcdef", 3);
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::VisualMode),
                counted(Action::Left, 2),
                plain(Action::OtherEnd),
                plain(Action::ToggleCase),
                plain(Action::Undo),
            ],
        );
        assert_eq!(editor.cursor(), 1);
    }

    #[test]
    fn undo_and_redo_of_lines_put_the_cursor_where_vim_does() {
        let mut registers = Registers::default();
        let undone = |text: &str, cursor: usize, keys: &[(Action, Extra)], redo: bool| {
            let mut editor = at(text, 0).with_lines();
            editor.set_cursor(cursor);
            let mut registers = Registers::default();
            run(&mut editor, &mut registers, keys);
            run(&mut editor, &mut registers, &[plain(Action::Undo)]);
            if redo {
                run(&mut editor, &mut registers, &[plain(Action::Redo)]);
            }
            (editor.text().to_owned(), editor.cursor())
        };
        let dd = [plain(Action::DeleteOperator), plain(Action::DeleteOperator)];
        let cc = [
            plain(Action::ChangeOperator),
            plain(Action::ChangeOperator),
            plain(Action::NormalMode),
        ];
        // On one line, the first character that is not blank, or the cursor before it.
        assert_eq!(
            undone("  ab cd\nef", 5, &dd, false),
            ("  ab cd\nef".to_owned(), 2)
        );
        assert_eq!(undone("  ab cd\nef", 1, &dd, false).1, 1);
        assert_eq!(undone("  ab cd\nef", 5, &cc, false).1, 2);
        assert_eq!(undone("  ab cd\nef", 0, &cc, false).1, 0);
        // On more lines, the cursor.
        let two_dd = [
            plain(Action::DeleteOperator),
            counted(Action::DeleteOperator, 2),
        ];
        assert_eq!(undone("  ab cd\nef", 5, &two_dd, false).1, 5);
        // Redo keeps the line and the column, and from a line that is gone goes to the first
        // character of the last one that is not blank.
        assert_eq!(undone("  ab cd\nef", 5, &cc, true), ("\nef".to_owned(), 0));
        assert_eq!(
            undone("ab cd\nef gh\nij", 13, &dd, true),
            ("ab cd\nef gh".to_owned(), 6)
        );
        assert_eq!(
            undone("ab cd\nef gh\nij", 8, &two_dd, true),
            ("ab cd".to_owned(), 0)
        );
        assert_eq!(undone("  ab\n  cd", 5, &dd, true), ("  ab".to_owned(), 2));
        // A change within a line: the same column, on the same line.
        let mut editor = at("abc\ndef", 6).with_lines();
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::DeleteChar),
                plain(Action::Undo),
                plain(Action::Redo),
            ],
        );
        assert_eq!((editor.text(), editor.cursor()), ("abc\nde", 5));
    }

    #[test]
    fn ctrl_o_goes_back_to_insert_mode_after_the_text_put_or_the_end_deleted_to() {
        type Case<'a> = (usize, &'a [(Action, Extra)], &'a str, usize);
        let mut registers = Registers::default();
        let mut x = Registers::default();
        x.keep(
            None,
            Register {
                text: "X".to_owned(),
                lines: false,
            },
            false,
        );
        let cases: [Case<'_>; 6] = [
            (2, &[plain(Action::DeleteToEnd)], "ab", 2),
            (3, &[plain(Action::DeleteChar)], "abc", 3),
            (
                2,
                &[plain(Action::DeleteOperator), plain(Action::WordForward)],
                "ab",
                2,
            ),
            (3, &[plain(Action::ToggleCase)], "abcD", 4),
            (2, &[plain(Action::PutAfter)], "abcXd", 4),
            (2, &[plain(Action::PutBefore)], "abXcd", 3),
        ];
        for (cursor, keys, text, after) in cases {
            let mut editor = Editor::new("abcd", Mode::Insert);
            editor.set_cursor(cursor);
            let registers = if keys[0].0 == Action::PutAfter || keys[0].0 == Action::PutBefore {
                &mut x
            } else {
                &mut registers
            };
            assert!(editor.handle(Action::NormalOnce, Extra::default(), registers));
            run(&mut editor, registers, keys);
            assert_eq!(editor.mode(), Mode::Insert, "{keys:?}");
            assert_eq!((editor.text(), editor.cursor()), (text, after), "{keys:?}");
        }
        // From the end of the line, P puts before the last character, and the cursor goes back
        // to the end.
        let mut editor = Editor::new("ab", Mode::Insert);
        run(
            &mut editor,
            &mut x,
            &[plain(Action::NormalOnce), plain(Action::PutBefore)],
        );
        assert_eq!((editor.text(), editor.cursor()), ("aXb", 3));
    }

    #[test]
    fn a_count_on_semicolon_after_t_counts_the_character_it_stopped_before() {
        let mut registers = Registers::default();
        let mut editor = at("a.b a.b a.b", 0);
        run(
            &mut editor,
            &mut registers,
            &[char(Action::TillChar, 'a'), counted(Action::RepeatFind, 2)],
        );
        assert_eq!(
            editor.cursor(),
            7,
            "before the second a after it, as vim's 2;"
        );
        let mut editor = at("a.b a.b a.b", 0);
        run(
            &mut editor,
            &mut registers,
            &[char(Action::TillChar, 'a'), plain(Action::RepeatFind)],
        );
        assert_eq!(editor.cursor(), 7, "one ; skips the a it stopped before");
    }

    #[test]
    fn ctrl_w_and_ctrl_u_after_a_move_stop_where_the_insert_started_as_vims() {
        let mut registers = Registers::default();
        let typed = |text: &str, keys: &[(Action, Extra)], typed: &str| {
            let mut editor = Editor::new(text, Mode::Insert);
            let mut registers = Registers::default();
            for c in typed.chars() {
                editor.insert(c);
            }
            for &(action, extra) in keys {
                assert!(editor.handle(action, extra, &mut registers));
            }
            editor
        };
        // A, XY, Left, Ctrl+w: what was typed before the cursor only.
        let editor = typed(
            "foo bar",
            &[plain(Action::Left), plain(Action::DeleteWord)],
            "XY",
        );
        assert_eq!(editor.text(), "foo barY");
        // A move to the right after typing keeps them stopping where Insert mode started.
        let mut editor = typed("foo bar", &[plain(Action::Left)], "XY");
        editor.insert('Z');
        run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
        assert_eq!(editor.text(), "foo barY");
        // A move with nothing typed starts the insert where it goes.
        let mut editor = typed("foo bar", &[plain(Action::TextStart)], "");
        run(&mut editor, &mut registers, &[plain(Action::TextEnd)]);
        editor.insert('Z');
        run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
        assert_eq!(editor.text(), "foo bar");
        // Backspace before where they stop does not move it for what is typed next.
        let mut editor = typed("foo", &[plain(Action::Backspace)], "");
        editor.insert('x');
        run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
        assert_eq!(editor.text(), "");
        // After Ctrl+w, a move to the left starts the insert where it goes: Ctrl+w stops there.
        let mut editor = typed(
            "foo bar baz",
            &[
                plain(Action::DeleteWord),
                plain(Action::Left),
                plain(Action::Left),
            ],
            "",
        );
        editor.insert('Z');
        run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
        assert_eq!(editor.text(), "foo bar ");
        // After Ctrl+u, which vim's defaults.vim maps after Ctrl+g u, they keep stopping where
        // they did.
        let mut editor = typed(
            "foo bar baz",
            &[
                plain(Action::DeleteToStart),
                plain(Action::Left),
                plain(Action::Left),
            ],
            "XY",
        );
        editor.insert('Z');
        run(&mut editor, &mut registers, &[plain(Action::DeleteToStart)]);
        assert_eq!(editor.text(), "az");
    }

    #[test]
    fn on_lines_operators_take_what_vims_exclusive_and_d_rules_give() {
        let mut registers = Registers::default();
        let lines = |text: &str, cursor: usize, keys: &[(Action, Extra)]| {
            let mut editor = at(text, 0).with_lines();
            editor.set_cursor(cursor);
            let mut registers = Registers::default();
            run(&mut editor, &mut registers, keys);
            (editor.text().to_owned(), registers.get(None).cloned())
        };
        let line = |text: &str| {
            Some(Register {
                text: text.to_owned(),
                lines: true,
            })
        };
        let chars = |text: &str| {
            Some(Register {
                text: text.to_owned(),
                lines: false,
            })
        };
        let d = plain(Action::DeleteOperator);
        // An exclusive motion that ends at the start of a line ends at the end of the one
        // before.
        assert_eq!(
            lines("ab cd\nef gh", 6, &[d, plain(Action::WordBackward)]),
            ("ab \nef gh".to_owned(), chars("cd"))
        );
        // From the indent, it takes the lines whole.
        assert_eq!(
            lines("ab\ncd", 3, &[d, plain(Action::WordBackward)]),
            ("cd".to_owned(), line("ab"))
        );
        assert_eq!(
            lines("ab\n\ncd", 0, &[d, counted(Action::WordForward, 2)]),
            ("cd".to_owned(), line("ab\n"))
        );
        // A delete over lines from the indent to the end of a line takes them whole.
        assert_eq!(
            lines("ab cd\nef gh\nij", 0, &[d, counted(Action::TextEnd, 2)]),
            ("ij".to_owned(), line("ab cd\nef gh"))
        );
        assert_eq!(
            lines("ab cd\nef gh\nij", 6, &[d, counted(Action::WordForward, 3)]),
            ("ab cd".to_owned(), line("ef gh\nij"))
        );
        assert_eq!(
            lines("ab\n\ncd", 0, &[counted(Action::DeleteToEnd, 2)]),
            ("cd".to_owned(), line("ab\n"))
        );
        // Not from inside a line, and not for c and y.
        assert_eq!(
            lines("ab cd\nef gh\nij", 1, &[d, counted(Action::TextEnd, 2)]),
            ("a\nij".to_owned(), chars("b cd\nef gh"))
        );
        assert_eq!(
            lines(
                "ab cd\nef gh\nij",
                0,
                &[plain(Action::YankOperator), counted(Action::TextEnd, 2)]
            )
            .1,
            chars("ab cd\nef gh")
        );
        // Space and Backspace go on to the next line and back to the one before.
        let mut editor = at("ab cd\nef", 4).with_lines();
        run(&mut editor, &mut registers, &[plain(Action::CharForward)]);
        assert_eq!(editor.cursor(), 6);
        run(
            &mut editor,
            &mut registers,
            &[counted(Action::CharBackward, 2)],
        );
        assert_eq!(editor.cursor(), 3);
        assert_eq!(
            lines("ab cd\nef gh", 4, &[d, counted(Action::CharForward, 3)]),
            ("ab cf gh".to_owned(), chars("d\ne"))
        );
        assert_eq!(
            lines("ab cd\nef gh", 4, &[d, counted(Action::CharForward, 2)]),
            ("ab c\nef gh".to_owned(), chars("d"))
        );
        assert_eq!(
            lines("ab\n\ncd", 3, &[d, plain(Action::CharForward)]),
            ("ab\ncd".to_owned(), line(""))
        );
        assert_eq!(
            lines("ab cd\nef gh", 6, &[d, plain(Action::CharBackward)]),
            ("ab cdef gh".to_owned(), chars("\n"))
        );
        assert_eq!(
            lines(
                "ab cd\nef gh",
                6,
                &[plain(Action::YankOperator), plain(Action::CharBackward)]
            )
            .1,
            chars("d")
        );
        // In a field of one line, they stop at its ends, as h and l.
        let mut field = at("ab", 1);
        run(&mut field, &mut registers, &[plain(Action::CharForward)]);
        assert_eq!(field.cursor(), 1);
        run(&mut field, &mut registers, &[d, plain(Action::CharForward)]);
        assert_eq!(field.text(), "a");
    }

    #[test]
    fn smaller_rules_of_vims_motions_and_changes() {
        let mut registers = Registers::default();
        // c b at the start of the text goes nowhere, and stays in Normal mode.
        let mut editor = at("ab", 0);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::ChangeOperator), plain(Action::WordBackward)],
        );
        assert_eq!(editor.mode(), Mode::Normal);
        // On a line of blanks, ^ is the last of them.
        let mut editor = at("   ", 0);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::DeleteOperator), plain(Action::FirstNonBlank)],
        );
        assert_eq!(editor.text(), " ");
        // ~ on a character without a case is a change for undo too.
        let mut editor = at("ab cd", 0);
        run(
            &mut editor,
            &mut registers,
            &[
                counted(Action::ToggleCase, 2),
                plain(Action::ToggleCase),
                plain(Action::Undo),
            ],
        );
        assert_eq!(editor.text(), "AB cd");
        // Delete at the end deletes the character before on the command line and in a filter,
        // as on vim's command line, and nothing in a field.
        let mut filter = Editor::new("abc", Mode::Insert);
        assert!(filter.edit(Action::DeleteChar));
        assert_eq!(filter.text(), "ab");
        let mut line = Editor::new("abc", Mode::Insert).with_lines();
        press(&mut line, &mut registers, Action::DeleteChar);
        assert_eq!(line.text(), "ab");
        let mut field = Editor::new("abc", Mode::Insert);
        press(&mut field, &mut registers, Action::DeleteChar);
        assert_eq!(field.text(), "abc");
    }
}
