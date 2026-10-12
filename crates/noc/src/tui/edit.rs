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

use super::keymap::{Action, Extra, Mode};

/// Changes that undo keeps, the oldest dropped first.
const UNDO_LEVELS: usize = 100;

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
    /// The register `name` names: the unnamed one without a name, or for `"`.
    fn get(&self, name: Option<char>) -> Option<&Register> {
        match name {
            Some(c) if c.is_ascii_alphabetic() => self.named.get(&c.to_ascii_lowercase()),
            _ => Some(&self.unnamed),
        }
    }

    /// Keeps `register` in the unnamed register, and in the one `name` names, if any.
    fn keep(&mut self, name: Option<char>, register: Register) {
        match name {
            Some(c) if c.is_ascii_uppercase() => {
                let kept = self.named.entry(c.to_ascii_lowercase()).or_default();
                if (kept.lines || register.lines) && !kept.text.is_empty() {
                    kept.text.push('\n');
                }
                kept.text.push_str(&register.text);
                kept.lines |= register.lines;
                self.unnamed = kept.clone();
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
}

/// Where a motion goes, and whether an operator acts on the character there too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Motion {
    to: usize,
    inclusive: bool,
}

/// The text and its cursor before a change, for undo.
#[derive(Debug, Clone)]
struct Snapshot {
    text: String,
    cursor: usize,
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
    pending: Option<Pending>,
    /// The last `f`, `t`, `F`, or `T`, and its character, for `;` and `,`.
    find: Option<(Action, char)>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Insert mode has changed the text since it started: one change for undo.
    changed: bool,
    /// After vim's Ctrl+o: Insert mode again after one command.
    once: bool,
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
            pending: None,
            find: None,
            undo: Vec::new(),
            redo: Vec::new(),
            changed: false,
            once: false,
        };
        editor.clamp();
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

    /// Puts the cursor at character `cursor`, as far as the text and the mode let it go.
    pub(crate) fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.len());
        self.clamp();
    }

    /// Replaces the text with `text`, as the history or a completion does, the cursor at
    /// `cursor`; undo can take it back.
    pub(crate) fn replace(&mut self, text: &str, cursor: usize) {
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
    /// goes to Insert mode, with the cursor at the end of the text, and one that moves to
    /// Normal mode. A secret stays in Insert mode.
    pub(crate) fn set_mode(&mut self, mode: Mode) {
        if self.is_secret() || self.mode == mode {
            return;
        }
        self.pending = None;
        self.once = false;
        self.changed = false;
        match mode {
            Mode::Insert => self.cursor = self.len(),
            Mode::Visual => self.anchor = self.cursor,
            Mode::Normal => {}
        }
        self.mode = mode;
        self.clamp();
    }

    /// The operator waiting for its motion, as vim's `showcmd` shows it, such as `2d`.
    pub(crate) fn pending(&self) -> Option<String> {
        let pending = self.pending?;
        let count = if pending.count > 1 {
            pending.count.to_string()
        } else {
            String::new()
        };
        let register = pending
            .register
            .map(|name| format!("\"{name}"))
            .unwrap_or_default();
        Some(format!("{register}{count}{}", pending.operator.key()))
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
        self.begin_change();
        let offset = self.offset(self.cursor);
        self.text.insert_str(offset, &text);
        self.cursor += text.chars().count();
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
            Mode::Insert => self.insert_mode(action, extra, registers),
            Mode::Normal => self.normal_mode(action, extra, registers),
            Mode::Visual => self.visual_mode(action, extra, registers),
        };
        if handled
            && self.once
            && action != Action::NormalOnce
            && self.mode == Mode::Normal
            && self.pending.is_none()
        {
            self.once = false;
            self.mode = Mode::Insert;
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
        self.insert_mode(action, Extra::default(), &mut Registers::default())
    }

    fn insert_mode(&mut self, action: Action, extra: Extra, registers: &mut Registers) -> bool {
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        match action {
            Action::Left => self.cursor = self.cursor.saturating_sub(1),
            Action::Right => self.cursor = (self.cursor + 1).min(chars.len()),
            Action::TextStart => self.cursor = start,
            Action::TextEnd => self.cursor = end,
            Action::WordForward => self.cursor = word_start(&chars, self.cursor, false),
            Action::WordBackward => self.cursor = word_back(&chars, self.cursor, false),
            Action::Backspace => self.delete(self.cursor.saturating_sub(1), self.cursor),
            Action::DeleteChar => self.delete(self.cursor, (self.cursor + 1).min(chars.len())),
            Action::DeleteToStart => self.delete(start, self.cursor),
            Action::DeleteToEnd => self.delete(self.cursor, end),
            Action::DeleteWord => {
                let from = word_back(&chars, self.cursor, false).max(start);
                // At the start of a line, the line break before it.
                let from = if from == self.cursor {
                    self.cursor.saturating_sub(1)
                } else {
                    from
                };
                self.delete(from, self.cursor);
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
            Action::NormalOnce if !self.is_secret() => {
                self.mode = Mode::Normal;
                self.changed = false;
                self.once = true;
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
                    self.lines_operate(pending, count * pending.count, registers);
                }
                // Another operator gives the first one up, as an unknown motion does.
                Some(_) => {}
                None => {
                    self.pending = Some(Pending {
                        operator,
                        count,
                        register: extra.register,
                    });
                }
            }
            return true;
        }
        if let Some(pending) = self.pending.take() {
            let total = count * pending.count;
            // As in vim, `cw` changes to the end of the word, not the spaces after it.
            let action = match (pending.operator, action) {
                (Operator::Change, Action::WordForward)
                    if self
                        .char_at(self.cursor)
                        .is_some_and(|c| !c.is_whitespace()) =>
                {
                    Action::WordEnd
                }
                (Operator::Change, Action::BigWordForward)
                    if self
                        .char_at(self.cursor)
                        .is_some_and(|c| !c.is_whitespace()) =>
                {
                    Action::BigWordEnd
                }
                _ => action,
            };
            if let Some(motion) = self.motion(action, total, extra.char, true) {
                let (from, to) = if motion.to < self.cursor {
                    (motion.to, self.cursor)
                } else {
                    (self.cursor, motion.to + usize::from(motion.inclusive))
                };
                let to = to.min(self.len());
                self.operate(pending.operator, from..to, pending.register, registers);
            }
            return true;
        }
        if let Some(motion) = self.motion(action, count, extra.char, false) {
            self.cursor = motion.to;
            self.clamp();
            return true;
        }
        if is_motion(action) {
            return true;
        }
        self.change(action, extra, registers)
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
                };
                self.lines_operate(pending, count, registers);
            }
            Action::ChangeToEnd => {
                self.operate(
                    Operator::Change,
                    self.cursor..end,
                    extra.register,
                    registers,
                );
            }
            Action::DeleteToEnd => {
                self.operate(
                    Operator::Delete,
                    self.cursor..end,
                    extra.register,
                    registers,
                );
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
        if let Some(motion) = self.motion(action, extra.times(), extra.char, false) {
            self.cursor = motion.to;
            self.clamp();
            return true;
        }
        if is_motion(action) {
            return true;
        }
        let selected = self.selection().unwrap_or(self.cursor..self.cursor);
        match action {
            Action::OtherEnd => std::mem::swap(&mut self.anchor, &mut self.cursor),
            Action::NormalMode | Action::VisualMode => {
                self.mode = Mode::Normal;
                self.clamp();
            }
            Action::ToggleCase => {
                self.toggle_case(selected.clone());
                self.mode = Mode::Normal;
                self.cursor = selected.start;
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

    /// Where `action` moves the cursor `count` times, if it is a motion that can; `operator`
    /// while one waits for it.
    fn motion(
        &mut self,
        action: Action,
        count: usize,
        c: Option<char>,
        operator: bool,
    ) -> Option<Motion> {
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        let exclusive = |to| {
            Some(Motion {
                to,
                inclusive: false,
            })
        };
        let inclusive = |to| {
            Some(Motion {
                to,
                inclusive: true,
            })
        };
        let repeat = |big: bool, step: fn(&[char], usize, bool) -> usize| {
            (0..count).fold(self.cursor, |at, _| step(&chars, at, big))
        };
        match action {
            Action::Left => exclusive(self.cursor.saturating_sub(count).max(start)),
            Action::Right => exclusive((self.cursor + count).min(end)),
            Action::TextStart => exclusive(start),
            Action::FirstNonBlank => exclusive(first_non_blank(&chars, start, end)),
            Action::TextEnd => {
                let end = (1..count).fold(end, |end, _| {
                    if end < chars.len() {
                        line_bounds(&chars, end + 1).1
                    } else {
                        end
                    }
                });
                exclusive(end)
            }
            Action::WordForward | Action::BigWordForward => {
                let big = action == Action::BigWordForward;
                let mut to = repeat(big, word_start);
                // An operator stops at the end of the line of the last word it moves over.
                if operator
                    && let Some(newline) = chars[self.cursor..to].iter().position(|&c| c == '\n')
                {
                    to = self.cursor + newline;
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

    /// Does `operator` on `range`: keeps its text in the registers, and deletes it, then goes
    /// to Insert mode for a change.
    fn operate(
        &mut self,
        operator: Operator,
        range: Range<usize>,
        register: Option<char>,
        registers: &mut Registers,
    ) {
        let taken: String = self.chars()[range.clone()].iter().collect();
        if !taken.is_empty() {
            registers.keep(
                register,
                Register {
                    text: taken,
                    lines: false,
                },
            );
        }
        match operator {
            Operator::Yank => self.cursor = range.start,
            Operator::Delete => {
                if !range.is_empty() {
                    self.begin_change();
                    self.remove(range.start, range.end);
                }
                self.cursor = range.start;
            }
            Operator::Change => {
                self.begin_change();
                self.remove(range.start, range.end);
                self.mode = Mode::Insert;
                self.changed = true;
                self.cursor = range.start;
            }
        }
        self.clamp();
    }

    /// Does the operator of `pending` on `count` lines from the cursor's, as `d d`, `c c`, and
    /// `y y` do.
    fn lines_operate(&mut self, pending: Pending, count: usize, registers: &mut Registers) {
        let chars = self.chars();
        let (start, mut end) = line_bounds(&chars, self.cursor);
        for _ in 1..count {
            if end >= chars.len() {
                break;
            }
            end = line_bounds(&chars, end + 1).1;
        }
        let text: String = chars[start..end].iter().collect();
        registers.keep(pending.register, Register { text, lines: true });
        match pending.operator {
            Operator::Yank => {}
            Operator::Delete => {
                self.begin_change();
                // The line break after the lines goes with them, or else the one before.
                let (from, to) = if end < chars.len() {
                    (start, end + 1)
                } else {
                    (start.saturating_sub(1), end)
                };
                self.remove(from, to);
                let chars = self.chars();
                let (start, end) = line_bounds(&chars, from.min(chars.len()));
                self.cursor = first_non_blank(&chars, start, end);
            }
            Operator::Change => {
                self.begin_change();
                self.remove(start, end);
                self.mode = Mode::Insert;
                self.changed = true;
                self.cursor = start;
            }
        }
        self.clamp();
    }

    /// Puts the register `name` names after the cursor, or before it, `count` times.
    fn put(&mut self, after: bool, count: usize, name: Option<char>, registers: &Registers) {
        let Some(register) = registers
            .get(name)
            .filter(|register| !register.text.is_empty())
        else {
            return;
        };
        let text = register.text.repeat(count);
        let chars = self.chars();
        self.begin_change();
        if register.lines && self.has_lines() {
            let (start, end) = line_bounds(&chars, self.cursor);
            let (at, put) = if after {
                (end, format!("\n{text}"))
            } else {
                (start, format!("{text}\n"))
            };
            let offset = self.offset(at);
            self.text.insert_str(offset, &put);
            self.cursor = if after { end + 1 } else { start };
        } else {
            let text = if self.has_lines() {
                text
            } else {
                text.replace('\n', " ")
            };
            let at = if after && !chars.is_empty() && chars.get(self.cursor) != Some(&'\n') {
                self.cursor + 1
            } else {
                self.cursor
            };
            let offset = self.offset(at.min(chars.len()));
            self.text.insert_str(offset, &text);
            self.cursor = at + text.chars().count() - 1;
        }
        self.clamp();
    }

    fn start_insert(&mut self, at: usize) {
        self.cursor = at;
        self.mode = Mode::Insert;
        self.changed = false;
    }

    fn toggle_case(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.begin_change();
        let toggled: String = self.chars()[range.clone()]
            .iter()
            .flat_map(|&c| {
                if c.is_uppercase() {
                    c.to_lowercase().collect::<Vec<_>>()
                } else {
                    c.to_uppercase().collect()
                }
            })
            .collect();
        let (from, to) = (self.offset(range.start), self.offset(range.end));
        self.text.replace_range(from..to, &toggled);
    }

    /// Keeps the text as it is for undo, before a change; once for all that Insert mode types.
    fn begin_change(&mut self) {
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
        Snapshot {
            text: self.text.as_str().to_owned(),
            cursor: self.cursor,
        }
    }

    fn restore(&mut self, snapshot: &Snapshot) {
        self.text.clear();
        self.text.push_str(&snapshot.text);
        self.cursor = snapshot.cursor;
        self.clamp();
    }

    fn undo(&mut self, count: usize) {
        for _ in 0..count {
            let Some(snapshot) = self.undo.pop() else {
                return;
            };
            self.redo.push(self.snapshot());
            self.restore(&snapshot);
        }
    }

    fn redo(&mut self, count: usize) {
        for _ in 0..count {
            let Some(snapshot) = self.redo.pop() else {
                return;
            };
            self.undo.push(self.snapshot());
            self.restore(&snapshot);
        }
    }

    /// Keeps the cursor on a character outside Insert mode, as vim does: not after the end of
    /// a line that has characters.
    fn clamp(&mut self) {
        self.cursor = self.cursor.min(self.len());
        if self.mode == Mode::Insert {
            return;
        }
        let chars = self.chars();
        let (start, end) = line_bounds(&chars, self.cursor);
        if end > start && self.cursor >= end {
            self.cursor = end - 1;
        }
    }

    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
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

/// Where the next word after character `at` starts, or the end of the text: vim's `w`.
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
    }
    index.min(chars.len())
}

/// Where the word before character `at` starts: vim's `b`.
fn word_back(chars: &[char], at: usize, big: bool) -> usize {
    let mut index = at.min(chars.len());
    while index > 0 && class(chars[index - 1], big) == 0 {
        index -= 1;
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

/// Where the word ends that character `at` is before, or in: vim's `e`.
fn word_end(chars: &[char], at: usize, big: bool) -> usize {
    let mut index = at + 1;
    while chars.get(index).is_some_and(|&c| class(c, big) == 0) {
        index += 1;
    }
    let Some(&c) = chars.get(index) else {
        return chars.len().saturating_sub(1).max(at.min(chars.len()));
    };
    let kind = class(c, big);
    while chars.get(index + 1).is_some_and(|&c| class(c, big) == kind) {
        index += 1;
    }
    index
}

/// The first character of the line in `start..end` that is not blank, or its end.
fn first_non_blank(chars: &[char], start: usize, end: usize) -> usize {
    (start..end)
        .find(|&index| !chars[index].is_whitespace())
        .unwrap_or(end)
}

/// The motion of `f`, `F`, `t`, or `T` for `c`, `count` times, within the line `bounds`; `again`
/// for `;` and `,`, whose `t` and `T` do not stop right before the character they stopped at.
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
    let skip = usize::from(till && again);
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
    })
}

/// Whether `action` is one of vim's motions, which a text moves its cursor by, even where it
/// cannot go.
fn is_motion(action: Action) -> bool {
    matches!(
        action,
        Action::Left
            | Action::Right
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
    }
}
