//! Key bindings: key sequences mapped to actions per context, with the built-in presets in
//! `default.rs` and `vim.rs`.
//!
//! A binding is a sequence of one or more key combinations, such as `f10` or `ctrl-x j`. While
//! the keys typed so far are the start of a longer binding, the keymap waits up to
//! [`SEQUENCE_TIMEOUT`] for the next key. `Esc` never waits: it acts at once. In the panels and
//! the windows that list items, Alt+1 … Alt+0 stand for F1 … F10, for terminals without F-keys.
//!
//! As which-key does, the keymap lists the keys that can follow a pending sequence, or every
//! key of a context, with what they do ([`Keymap::hints`]). While the hints show, the sequence
//! waits for its next key without a deadline; `Esc` closes them, and `Backspace` takes back the
//! last key of the sequence.

mod action;
mod default;
mod diff;
mod vim;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crokey::KeyCombination;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) use action::{Action, Context};
pub(crate) use diff::Diff;

/// How long a sequence such as `Ctrl+x j` waits for its next key.
pub(crate) const SEQUENCE_TIMEOUT: Duration = Duration::from_secs(1);

/// What keys resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// A bound action.
    Action(Action),
    /// A printable character that no binding claimed, in a context that accepts text.
    Insert(char),
}

type Sequence = Vec<KeyCombination>;

/// The bindings of a context in a built-in preset: each action with its key sequences.
type Bound = &'static [(Action, &'static [&'static str])];

/// A built-in preset: each context with its bindings.
type Preset = &'static [(Context, Bound)];

/// Bindings for every context.
#[derive(Debug, Clone)]
pub(crate) struct Keymap {
    contexts: HashMap<Context, Bindings>,
}

/// The bindings of one context, in the order they were added.
#[derive(Debug, Clone, Default)]
struct Bindings(Vec<(Sequence, Action)>);

/// What one context knows about a key sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lookup {
    /// Nothing.
    Unknown,
    /// It is bound, and no longer binding starts with it.
    Exact(Action),
    /// It is the start of longer bindings, and maybe bound itself.
    Prefix(Option<Action>),
}

impl Bindings {
    fn lookup(&self, keys: &[KeyCombination]) -> Lookup {
        let exact = self
            .0
            .iter()
            .find(|(sequence, _)| sequence == keys)
            .map(|(_, action)| *action);
        let longer = self
            .0
            .iter()
            .any(|(sequence, _)| sequence.len() > keys.len() && sequence.starts_with(keys));
        match (exact, longer) {
            (_, true) => Lookup::Prefix(exact),
            (Some(action), false) => Lookup::Exact(action),
            (None, false) => Lookup::Unknown,
        }
    }
}

/// The keys of a sequence typed so far.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyState {
    context: Option<Context>,
    keys: Sequence,
    deadline: Option<Instant>,
    /// When the last key of the pending sequence came.
    since: Option<Instant>,
    /// The hints show: of the keys that can follow the sequence, or of every key while none is
    /// typed.
    hinting: bool,
}

impl KeyState {
    /// When the pending sequence times out; `None` if no sequence is pending, or while the
    /// hints show.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// The context of the keys typed last.
    pub(crate) fn context(&self) -> Option<Context> {
        self.context
    }

    fn clear(&mut self) {
        self.keys.clear();
        self.deadline = None;
        self.since = None;
    }
}

/// A key that can be typed next, with what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hint {
    /// The key as the help writes it; keys that do the same, apart by commas.
    pub(crate) keys: String,
    /// What the key does; `None` for a key that only starts longer sequences.
    pub(crate) action: Option<Action>,
    /// How many longer sequences the key starts.
    pub(crate) more: usize,
}

/// The hints to show: the keys typed so far, and the keys that can follow them in `context`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hints {
    pub(crate) context: Context,
    /// The keys typed so far as the help writes them; `None` while none is typed.
    pub(crate) typed: Option<String>,
    pub(crate) rows: Vec<Hint>,
}

impl Keymap {
    /// The names of the built-in presets, for `ui.keymap`.
    pub(crate) const NAMES: &'static [&'static str] = &["default", "vim"];

    /// A built-in preset by name.
    pub(crate) fn by_name(name: &str) -> Option<Self> {
        preset(name).map(Self::of)
    }

    /// The default preset, modelled on Midnight Commander.
    pub(crate) fn mc() -> Self {
        Self::of(default::PRESET)
    }

    fn of(preset: Preset) -> Self {
        let mut contexts = HashMap::new();
        for (context, bindings) in preset {
            let bindings = bindings
                .iter()
                .flat_map(|(action, keys)| {
                    keys.iter().map(|keys| {
                        let sequence = parse_sequence(keys)
                            .unwrap_or_else(|| unreachable!("invalid preset key `{keys}`"));
                        (sequence, *action)
                    })
                })
                .collect();
            let bindings = if context.alt_digits_stand_for_f_keys() {
                with_alt_digits(bindings)
            } else {
                bindings
            };
            contexts.insert(*context, Bindings(bindings));
        }
        Self { contexts }
    }

    /// What the first context in `context`'s chain that knows `keys` knows about them. A prefix
    /// that this context does not bind itself does, once it times out, what a later context
    /// binds it to.
    fn lookup(&self, context: Context, keys: &[KeyCombination]) -> Lookup {
        let mut known = context
            .chain()
            .iter()
            .filter_map(|context| self.contexts.get(context))
            .map(|bindings| bindings.lookup(keys))
            .filter(|lookup| *lookup != Lookup::Unknown);
        match known.next() {
            Some(Lookup::Prefix(None)) => Lookup::Prefix(known.find_map(|lookup| match lookup {
                Lookup::Exact(action) | Lookup::Prefix(Some(action)) => Some(action),
                Lookup::Prefix(None) | Lookup::Unknown => None,
            })),
            Some(lookup) => lookup,
            None => Lookup::Unknown,
        }
    }

    /// Feeds one key press. Returns what to do now; nothing while a sequence waits for its next
    /// key. A change of `context` forgets a pending sequence, and closes the hints.
    pub(crate) fn feed(
        &self,
        state: &mut KeyState,
        context: Context,
        event: KeyEvent,
        now: Instant,
    ) -> Vec<Resolved> {
        if state.hinting && state.context == Some(context) {
            let key = KeyCombination::from(event);
            let mut keys = state.keys.clone();
            keys.push(key);
            let continues =
                !state.keys.is_empty() && self.lookup(context, &keys) != Lookup::Unknown;
            if !continues && key == ESC {
                state.clear();
                state.hinting = false;
                return Vec::new();
            }
            if !continues && key == BACKSPACE && state.keys.pop().is_some() {
                state.deadline = None;
                return Vec::new();
            }
        }
        let resolved = self.feed_key(state, context, event, now);
        if state.keys.is_empty() {
            state.hinting = false;
        }
        resolved
    }

    fn feed_key(
        &self,
        state: &mut KeyState,
        context: Context,
        event: KeyEvent,
        now: Instant,
    ) -> Vec<Resolved> {
        if state.context != Some(context) {
            state.clear();
            state.hinting = false;
            state.context = Some(context);
        }
        // An Esc and the key right after it can reach a terminal program in one read, which
        // crossterm reports as Alt and the key, so an Alt and a character that nothing binds
        // count as both.
        if context.alt_stands_for_esc()
            && state.keys.is_empty()
            && let Some(plain) = without_alt(event)
            && self.lookup(context, &[KeyCombination::from(event)]) == Lookup::Unknown
        {
            let mut resolved = self.feed_key(state, context, KeyEvent::from(KeyCode::Esc), now);
            resolved.extend(self.feed_key(state, context, plain, now));
            return resolved;
        }
        if context.text_first()
            && state.keys.is_empty()
            && let Some(c) = text(event)
            && self
                .contexts
                .get(&context)
                .is_none_or(|own| own.lookup(&[KeyCombination::from(event)]) == Lookup::Unknown)
        {
            return vec![Resolved::Insert(c)];
        }
        let mut resolved = Vec::new();
        state.keys.push(KeyCombination::from(event));
        loop {
            match self.lookup(context, &state.keys) {
                Lookup::Exact(action) => {
                    resolved.push(Resolved::Action(action));
                    state.clear();
                }
                Lookup::Prefix(own) => {
                    state.since = Some(now);
                    // While the hints show, a sequence that does nothing alone waits.
                    let waits = state.hinting && own.is_none();
                    state.deadline = (!waits).then_some(now + SEQUENCE_TIMEOUT);
                }
                Lookup::Unknown if state.keys.len() == 1 => {
                    if let Some(c) = text(event).filter(|_| context.accepts_text()) {
                        resolved.push(Resolved::Insert(c));
                    }
                    state.clear();
                }
                Lookup::Unknown => {
                    let key = state.keys.pop().unwrap_or_else(|| unreachable!());
                    // The new key cannot continue the pending sequence: settle that one as if
                    // it had timed out, then try the new key on its own.
                    resolved.extend(self.settle(state, context));
                    state.keys.push(key);
                    continue;
                }
            }
            return resolved;
        }
    }

    /// Settles a pending sequence whose deadline has passed: it runs its own binding if it has
    /// one, and is dropped otherwise.
    pub(crate) fn expire(&self, state: &mut KeyState, now: Instant) -> Vec<Resolved> {
        match (state.deadline, state.context) {
            (Some(deadline), Some(context)) if deadline <= now => self.settle(state, context),
            _ => Vec::new(),
        }
    }

    fn settle(&self, state: &mut KeyState, context: Context) -> Vec<Resolved> {
        let resolved = match self.lookup(context, &state.keys) {
            Lookup::Prefix(Some(action)) | Lookup::Exact(action) => vec![Resolved::Action(action)],
            Lookup::Prefix(None) | Lookup::Unknown => Vec::new(),
        };
        state.clear();
        resolved
    }

    /// The actions `context` binds itself, in the order of the preset, each with its keys as
    /// text such as `Ctrl+r, Alt+s`, for the help screen; then those of the contexts it falls
    /// back to whose keys the help lists with it, as far as they still do that here. The
    /// Alt+1 … Alt+0 aliases are left out; the help explains them once.
    pub(crate) fn help(&self, context: Context) -> Vec<(Action, String)> {
        let mut rows: Vec<(Action, String)> = Vec::new();
        let mut seen: Vec<&Sequence> = Vec::new();
        let listed = context
            .chain()
            .iter()
            .filter(|other| **other == context || other.helps_with_others())
            .filter_map(|other| self.contexts.get(other))
            .flat_map(|bindings| &bindings.0);
        for (sequence, action) in listed {
            // A key that two of the contexts bind, such as Home in a list and in its filter,
            // does what the first one says, and shows once.
            if is_alt_digit(sequence) || seen.contains(&sequence) {
                continue;
            }
            seen.push(sequence);
            let does = match self.lookup(context, sequence) {
                Lookup::Exact(bound) | Lookup::Prefix(Some(bound)) => bound == *action,
                Lookup::Prefix(None) | Lookup::Unknown => false,
            };
            if !does {
                continue;
            }
            let keys = describe(sequence);
            match rows.iter_mut().find(|(bound, _)| bound == action) {
                Some((_, text)) => {
                    text.push_str(", ");
                    text.push_str(&keys);
                }
                None => rows.push((*action, keys)),
            }
        }
        rows
    }

    /// The first key sequence along `context`'s chain that does `action` there, as text such as
    /// `Ctrl+x j`, for the pull-down menu. Alt+1 … Alt+0 count only where nothing else does.
    pub(crate) fn key(&self, context: Context, action: Action) -> Option<String> {
        let does = |sequence: &[KeyCombination]| match self.lookup(context, sequence) {
            Lookup::Exact(bound) | Lookup::Prefix(Some(bound)) => bound == action,
            Lookup::Prefix(None) | Lookup::Unknown => false,
        };
        let sequences = context
            .chain()
            .iter()
            .filter_map(|context| self.contexts.get(context))
            .flat_map(|bindings| &bindings.0)
            .filter(|(sequence, bound)| *bound == action && does(sequence))
            .map(|(sequence, _)| sequence);
        let (aliases, keys): (Vec<&Sequence>, Vec<&Sequence>) =
            sequences.partition(|sequence| is_alt_digit(sequence));
        let sequence = keys.first().or(aliases.first())?;
        Some(describe(sequence))
    }

    /// The actions on F1 … F10 in `context`'s chain, for the F-key bar.
    pub(crate) fn fkeys(&self, context: Context) -> [Option<Action>; 10] {
        std::array::from_fn(|index| {
            let number = u8::try_from(index + 1).unwrap_or_else(|_| unreachable!());
            let key = KeyCombination::one_key(KeyCode::F(number), KeyModifiers::NONE);
            match self.lookup(context, &[key]) {
                Lookup::Exact(action) | Lookup::Prefix(Some(action)) => Some(action),
                Lookup::Prefix(None) | Lookup::Unknown => None,
            }
        })
    }

    /// When the hints of the pending sequence are due, `delay` after its last key; `None` if
    /// they show already, or for a sequence that does something alone, which has none. Until
    /// then the sequence does not time out.
    pub(crate) fn hints_due(&self, state: &KeyState, delay: Duration) -> Option<Instant> {
        let context = state.context?;
        let since = state.since?;
        let hinted = !state.hinting
            && !state.keys.is_empty()
            && self.lookup(context, &state.keys) == Lookup::Prefix(None);
        hinted.then_some(since + delay)
    }

    /// Shows the hints in `context`: of the keys that can follow the pending sequence, or of
    /// every key while none is pending. They show until a key ends the sequence.
    pub(crate) fn show_hints(&self, state: &mut KeyState, context: Context) {
        if state.context != Some(context) {
            state.clear();
            state.context = Some(context);
        }
        state.hinting = true;
        // A sequence that does something alone still does it when it times out.
        if state.keys.is_empty() || self.lookup(context, &state.keys) == Lookup::Prefix(None) {
            state.deadline = None;
        }
    }

    /// The hints that show in `context`, if they do.
    pub(crate) fn hints_of(&self, state: &KeyState, context: Context) -> Option<Hints> {
        if !state.hinting || state.context != Some(context) {
            return None;
        }
        let rows = self.hints(context, &state.keys);
        let typed = (!state.keys.is_empty()).then(|| describe(&state.keys));
        (!rows.is_empty()).then_some(Hints {
            context,
            typed,
            rows,
        })
    }

    /// The keys that can follow `prefix` in `context`'s chain, in the order of the preset, the
    /// context's own first. Keys that do the same share a row; a key that starts longer
    /// sequences has one of its own. `Esc`, which closes the hints, and the Alt+1 … Alt+0
    /// aliases are left out, and where every character is text, so are characters that only a
    /// fallback context binds.
    pub(crate) fn hints(&self, context: Context, prefix: &[KeyCombination]) -> Vec<Hint> {
        let mut next: Vec<KeyCombination> = Vec::new();
        for (sequence, _) in self.chain_bindings(context) {
            if sequence.len() > prefix.len()
                && sequence.starts_with(prefix)
                && sequence.first() != Some(&ESC)
                && !is_alt_digit(sequence)
                && let Some(key) = sequence.get(prefix.len())
                && !next.contains(key)
            {
                next.push(*key);
            }
        }
        let own = self.contexts.get(&context);
        let mut hints: Vec<Hint> = Vec::new();
        for key in next {
            if prefix.is_empty()
                && context.text_first()
                && is_character(key)
                && own.is_none_or(|own| own.lookup(&[key]) == Lookup::Unknown)
            {
                continue;
            }
            let mut keys = prefix.to_vec();
            keys.push(key);
            let (action, more) = match self.lookup(context, &keys) {
                Lookup::Exact(action) => (Some(action), 0),
                Lookup::Prefix(action) => (action, self.count_under(context, &keys)),
                Lookup::Unknown => continue,
            };
            let text = describe_key(key);
            match hints
                .iter_mut()
                .find(|hint| hint.more == 0 && more == 0 && hint.action == action)
            {
                Some(hint) => {
                    hint.keys.push_str(", ");
                    hint.keys.push_str(&text);
                }
                None => hints.push(Hint {
                    keys: text,
                    action,
                    more,
                }),
            }
        }
        hints
    }

    /// The bindings of `context`'s chain, most specific first.
    fn chain_bindings(&self, context: Context) -> impl Iterator<Item = &(Sequence, Action)> {
        context
            .chain()
            .iter()
            .filter_map(|context| self.contexts.get(context))
            .flat_map(|bindings| &bindings.0)
    }

    /// How many sequences longer than `prefix` that start with it do something in `context`.
    fn count_under(&self, context: Context, prefix: &[KeyCombination]) -> usize {
        let mut counted: Vec<&Sequence> = Vec::new();
        for (sequence, _) in self.chain_bindings(context) {
            let does = matches!(
                self.lookup(context, sequence),
                Lookup::Exact(_) | Lookup::Prefix(Some(_))
            );
            if sequence.len() > prefix.len()
                && sequence.starts_with(prefix)
                && does
                && !counted.contains(&sequence)
            {
                counted.push(sequence);
            }
        }
        counted.len()
    }
}

/// A key sequence as the help, the menus, and the docs write it: modifiers as `Ctrl+`, `Alt+`,
/// and `Shift+`, letters in lowercase and in uppercase for Shift and the letter, symbols as they
/// are typed, and the keys of a sequence apart, as in `Ctrl+x t` or `Z Z`.
fn describe(sequence: &[KeyCombination]) -> String {
    sequence
        .iter()
        .map(|key| describe_key(*key))
        .collect::<Vec<_>>()
        .join(" ")
}

fn describe_key(key: KeyCombination) -> String {
    let mut shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let names: Vec<String> = key
        .codes
        .iter()
        .map(|code| match *code {
            KeyCode::Char(' ') => "Space".to_owned(),
            KeyCode::Char(c) if c.is_alphabetic() => {
                let letter = if shift || c.is_uppercase() {
                    c.to_uppercase().collect()
                } else {
                    c.to_lowercase().collect()
                };
                shift = false;
                letter
            }
            KeyCode::Char(c) => {
                shift = false;
                c.to_string()
            }
            KeyCode::BackTab => {
                shift = true;
                "Tab".to_owned()
            }
            KeyCode::PageUp => "PgUp".to_owned(),
            KeyCode::PageDown => "PgDn".to_owned(),
            KeyCode::F(number) => format!("F{number}"),
            other => format!("{other:?}"),
        })
        .collect();
    let mut text = String::new();
    for (modifier, name) in [
        (KeyModifiers::CONTROL, "Ctrl+"),
        (KeyModifiers::ALT, "Alt+"),
        (KeyModifiers::SUPER, "Cmd+"),
    ] {
        if key.modifiers.contains(modifier) {
            text.push_str(name);
        }
    }
    if shift {
        text.push_str("Shift+");
    }
    text.push_str(&names.join("+"));
    text
}

const ESC: KeyCombination = KeyCombination::one_key(KeyCode::Esc, KeyModifiers::NONE);
const BACKSPACE: KeyCombination = KeyCombination::one_key(KeyCode::Backspace, KeyModifiers::NONE);

/// Whether `sequence` is Alt and a digit, which stands for an F-key.
fn is_alt_digit(sequence: &[KeyCombination]) -> bool {
    matches!(sequence, [key]
        if key.modifiers == KeyModifiers::ALT && matches!(key.codes.first(), KeyCode::Char('0'..='9')))
}

/// Adds Alt+1 … Alt+0 next to every binding of F1 … F10, unless that key is bound.
fn with_alt_digits(mut bindings: Vec<(Sequence, Action)>) -> Vec<(Sequence, Action)> {
    let aliases: Vec<(Sequence, Action)> = bindings
        .iter()
        .filter_map(|(sequence, action)| match sequence.as_slice() {
            [key] if key.modifiers.is_empty() => match *key.codes.first() {
                KeyCode::F(number @ 1..=10) => {
                    let digit = char::from_digit(u32::from(number % 10), 10)?;
                    let digit = KeyCombination::one_key(KeyCode::Char(digit), KeyModifiers::ALT);
                    Some((vec![digit], *action))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    for (sequence, action) in aliases {
        if !bindings.iter().any(|(bound, _)| *bound == sequence) {
            bindings.push((sequence, action));
        }
    }
    bindings
}

/// The same character key without Alt, if `event` is Alt and a character.
fn without_alt(event: KeyEvent) -> Option<KeyEvent> {
    let KeyCode::Char(_) = event.code else {
        return None;
    };
    (event.modifiers - KeyModifiers::SHIFT == KeyModifiers::ALT)
        .then(|| KeyEvent::new(event.code, event.modifiers - KeyModifiers::ALT))
}

/// Whether `key` is a character without Ctrl or Alt.
fn is_character(key: KeyCombination) -> bool {
    key.is_ansi_compatible()
        && matches!(key.codes.first(), KeyCode::Char(_))
        && (key.modifiers - KeyModifiers::SHIFT).is_empty()
}

/// A built-in preset by name.
fn preset(name: &str) -> Option<Preset> {
    match name {
        "default" => Some(default::PRESET),
        "vim" => Some(vim::PRESET),
        _ => None,
    }
}

/// Parses a space-separated key sequence such as `ctrl-x j`, with `crokey` key names.
fn parse_sequence(text: &str) -> Option<Sequence> {
    let sequence: Sequence = text
        .split_whitespace()
        .map(|key| {
            crokey::parse(key)
                .ok()
                .map(|key| back_tab(key.normalized()))
        })
        .collect::<Option<_>>()?;
    (!sequence.is_empty()).then_some(sequence)
}

/// Terminals send Shift+Tab as `BackTab`, never as Tab with Shift, so `shift-tab` means `backtab`.
fn back_tab(key: KeyCombination) -> KeyCombination {
    if key == KeyCombination::new(KeyCode::Tab, KeyModifiers::SHIFT) {
        KeyCombination::new(KeyCode::BackTab, KeyModifiers::SHIFT)
    } else {
        key
    }
}

/// The character a key types, if it types one.
fn text(event: KeyEvent) -> Option<char> {
    match event.code {
        KeyCode::Char(c)
            if (event.modifiers - KeyModifiers::SHIFT).is_empty() && !c.is_control() =>
        {
            Some(c)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key press from its `crokey` name, or a single character such as `é`.
    fn key(text: &str) -> KeyEvent {
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && !c.is_ascii()
        {
            return KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        }
        let combination = crokey::parse(text).unwrap().normalized();
        KeyEvent::new(*combination.codes.first(), combination.modifiers)
    }

    /// Feeds `keys` one by one at the same instant and collects what they resolved to.
    fn feed(
        keymap: &Keymap,
        state: &mut KeyState,
        context: Context,
        keys: &[&str],
    ) -> Vec<Resolved> {
        let now = Instant::now();
        keys.iter()
            .flat_map(|text| keymap.feed(state, context, key(text), now))
            .collect()
    }

    fn actions(actions: &[Action]) -> Vec<Resolved> {
        actions.iter().copied().map(Resolved::Action).collect()
    }

    #[test]
    fn shift_f6_renames_in_the_row_where_only_field_keys_act() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["shift-f6", "f16"]),
            actions(&[Action::Rename, Action::Rename])
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Rename,
                &["ctrl-u", "left", "enter", "esc"]
            ),
            actions(&[
                Action::DeleteToStart,
                Action::Left,
                Action::Confirm,
                Action::Cancel
            ])
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Rename,
                &["*", "+", "f5", "tab", "up"]
            ),
            [Resolved::Insert('*'), Resolved::Insert('+')],
            "panel keys do nothing"
        );
    }

    #[test]
    fn single_keys_resolve_at_once() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &["f10", "tab", "ctrl-l", "down"]
            ),
            actions(&[
                Action::Quit,
                Action::SwitchPanel,
                Action::Redraw,
                Action::Down
            ])
        );
        assert_eq!(state.deadline(), None);
    }

    #[test]
    fn esc_acts_at_once_and_a_digit_after_it_is_a_digit() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["esc"]),
                actions(&[Action::Cancel]),
                "{name}"
            );
            assert_eq!(state.deadline(), None, "{name}: Esc does not wait");
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["esc", "0"]),
                [Resolved::Action(Action::Cancel), Resolved::Insert('0')],
                "{name}"
            );
        }
    }

    #[test]
    fn a_key_right_after_esc_does_what_it_does_alone() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["esc", "o", "n"]),
            actions(&[Action::Cancel, Action::SortByName]),
            "not Alt+o"
        );
        let vim = Keymap::by_name("vim").unwrap();
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::Panel,
                &["esc", "j", "esc", "g", "g"]
            ),
            actions(&[Action::Cancel, Action::Down, Action::Cancel, Action::Home])
        );
    }

    #[test]
    fn a_pending_sequence_expires() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        let start = Instant::now();
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("ctrl-x"), start),
            []
        );
        assert_eq!(state.deadline(), Some(start + SEQUENCE_TIMEOUT));
        assert_eq!(keymap.expire(&mut state, start), [], "not yet");
        // A sequence that does nothing alone does nothing when nothing follows.
        assert_eq!(keymap.expire(&mut state, start + SEQUENCE_TIMEOUT), []);
        assert_eq!(state.deadline(), None);
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("j"), start),
            [Resolved::Insert('j')],
            "a later letter is just a letter"
        );
    }

    #[test]
    fn an_unbound_alt_and_a_character_stand_for_esc_and_the_character() {
        // What an Esc typed quickly before a key, or with an Alt that sends Esc, arrives as.
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["alt-."]),
            actions(&[Action::ToggleHidden])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["alt-q"]),
            [Resolved::Action(Action::Cancel), Resolved::Insert('q')]
        );
        let vim = Keymap::by_name("vim").unwrap();
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["alt-j", "alt-/"]),
            actions(&[Action::Cancel, Action::Down, Action::KeyHints])
        );
        assert_eq!(state.deadline(), None);
        // Dialogs keep Alt as it is.
        assert_eq!(feed(&keymap, &mut state, Context::Dialog, &["alt-q"]), []);
    }

    #[test]
    fn alt_and_a_digit_stand_for_the_f_key() {
        // For terminals without F-keys, as mc's Esc 1 … Esc 0.
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["alt-0", "alt-1"]),
                actions(&[Action::Quit, Action::Help]),
                "{name}"
            );
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["alt-8"]),
                actions(&[Action::Delete]),
                "{name}"
            );
            assert_eq!(state.deadline(), None);
            // The windows that list items take them for their F-keys too, theirs and the list's.
            assert_eq!(
                feed(
                    &keymap,
                    &mut state,
                    Context::LocationMenu,
                    &["alt-4", "alt-8", "alt-0"]
                ),
                actions(&[Action::EditHost, Action::Disconnect, Action::Cancel]),
                "{name}"
            );
            assert_eq!(
                feed(&keymap, &mut state, Context::Workspaces, &["alt-6"]),
                actions(&[Action::Move]),
                "{name}"
            );
            assert_eq!(
                feed(&keymap, &mut state, Context::TabMenu, &["alt-0", "alt-5"]),
                actions(&[Action::Cancel]),
                "{name}: no F5 there"
            );
            assert_eq!(
                feed(&keymap, &mut state, Context::Dialog, &["alt-0"]),
                [],
                "{name}"
            );
        }
    }

    #[test]
    fn a_broken_sequence_lets_the_new_key_through() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        // The pending Ctrl+x settles as if it had timed out, doing nothing, then the new key runs.
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["ctrl-x", "tab"]),
            actions(&[Action::SwitchPanel])
        );
        // The new key may start a sequence of its own…
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["ctrl-x", "t", "n"]),
            actions(&[Action::NewTab])
        );
        // …the same one too.
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &["ctrl-x", "ctrl-x", "j"]
            ),
            actions(&[Action::Jobs])
        );
        assert_eq!(state.deadline(), None);
    }

    #[test]
    fn quick_search_takes_characters_that_the_panel_binds() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &["*", "insert", "ctrl-t", "shift-down", "shift-up"]
            ),
            actions(&[
                Action::InvertMarks,
                Action::Mark,
                Action::Mark,
                Action::Mark,
                Action::MarkUp
            ])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["alt-*"]),
            actions(&[Action::InvertMarks])
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &["+", "-", "\\", "alt--"]
            ),
            actions(&[
                Action::Select,
                Action::Unselect,
                Action::Unselect,
                Action::Unselect
            ])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::QuickSearch, &["+", "-"]),
            [Resolved::Insert('+'), Resolved::Insert('-')]
        );
        // Space switches check boxes, but in a text field it is a space.
        assert_eq!(
            feed(&keymap, &mut state, Context::Dialog, &["space"]),
            actions(&[Action::Toggle])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::DialogInput, &["space"]),
            [Resolved::Insert(' ')]
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::QuickSearch,
                &["*", "insert", "ctrl-s"]
            ),
            [
                Resolved::Insert('*'),
                Resolved::Action(Action::Mark),
                Resolved::Action(Action::QuickSearch)
            ]
        );
    }

    #[test]
    fn contexts_fall_back_along_their_chain() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        // Quick search binds Esc itself, so it does not wait for a digit…
        assert_eq!(
            feed(&keymap, &mut state, Context::QuickSearch, &["esc"]),
            actions(&[Action::Cancel])
        );
        // …and leaves other keys to the panel.
        assert_eq!(
            feed(&keymap, &mut state, Context::QuickSearch, &["enter", "f10"]),
            actions(&[Action::Enter, Action::Quit])
        );
        // Esc closes a dialog at once; it does not wait for a digit.
        assert_eq!(
            feed(&keymap, &mut state, Context::Dialog, &["esc"]),
            actions(&[Action::Cancel])
        );
        assert_eq!(state.deadline(), None);
        // Dialogs are modal: panel keys do nothing there.
        assert_eq!(
            feed(&keymap, &mut state, Context::Dialog, &["ctrl-u", "alt-o"]),
            []
        );
        // A text field adds editing keys to the dialog's.
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::DialogInput,
                &["ctrl-u", "enter", "esc"]
            ),
            actions(&[Action::DeleteToStart, Action::Confirm, Action::Cancel])
        );
    }

    #[test]
    fn the_text_fields_share_the_editing_keys_of_input() {
        let fields = [
            Context::QuickSearch,
            Context::Rename,
            Context::DialogInput,
            Context::PathInput,
            Context::CommandLine,
        ];
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let home = if *name == "vim" { "ctrl-b" } else { "ctrl-a" };
            let keys = [
                "left",
                "right",
                "home",
                home,
                "end",
                "ctrl-e",
                "backspace",
                "ctrl-h",
                "delete",
                "ctrl-u",
                "ctrl-k",
            ];
            let edits = actions(&[
                Action::Left,
                Action::Right,
                Action::Home,
                Action::Home,
                Action::End,
                Action::End,
                Action::Backspace,
                Action::Backspace,
                Action::Delete,
                Action::DeleteToStart,
                Action::DeleteToEnd,
            ]);
            for context in fields {
                assert!(context.chain().contains(&Context::Input), "{context:?}");
                let mut state = KeyState::default();
                assert_eq!(
                    feed(&keymap, &mut state, context, &keys),
                    edits,
                    "{name}: {context:?}"
                );
            }
            // Quick search edits its text with them, rather than moving the panel's cursor;
            // keys that the text fields leave alone still go to the panel.
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::QuickSearch, &["up", "f5"]),
                actions(&[Action::Up, Action::Copy]),
                "{name}"
            );
            // Its key for the next match is its own, so the help lists it with quick search.
            assert!(
                keymap
                    .help(Context::QuickSearch)
                    .iter()
                    .any(|(action, _)| *action == Action::QuickSearch),
                "{name}"
            );
        }
    }

    #[test]
    fn f4_and_f8_act_on_hosts_in_the_location_menu_and_on_files_in_panels() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                feed(
                    &keymap,
                    &mut state,
                    Context::LocationMenu,
                    &["f4", "f8", "ctrl-r", "f10"]
                ),
                actions(&[
                    Action::EditHost,
                    Action::Disconnect,
                    Action::Reload,
                    Action::Cancel
                ]),
                "{name}"
            );
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["f4", "f8"]),
                actions(&[Action::Edit, Action::Delete]),
                "{name}: only the location menu disconnects; panels delete"
            );
        }
    }

    #[test]
    fn printable_keys_insert_text_where_text_is_accepted() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        let typed = feed(
            &keymap,
            &mut state,
            Context::DialogInput,
            &["a", "shift-b", "1", "space", "é", "ctrl-x", "alt-z"],
        );
        assert_eq!(
            typed,
            ['a', 'B', '1', ' ', 'é'].map(Resolved::Insert),
            "Ctrl and Alt combinations type nothing"
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::QuickSearch, &["x"]),
            [Resolved::Insert('x')]
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["q"]),
            [Resolved::Insert('q')]
        );
        assert_eq!(feed(&keymap, &mut state, Context::Dialog, &["y"]), []);
    }

    #[test]
    fn f_lists_the_jobs_and_computes_checksums() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["f", "j", "f", "#"]),
                actions(&[Action::Jobs, Action::Checksum]),
                "{name}"
            );
        }
        // The default keymap keeps mc's Ctrl+x j, first.
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["ctrl-x", "j"]),
            actions(&[Action::Jobs])
        );
        assert_eq!(
            keymap.key(Context::Panel, Action::Jobs).as_deref(),
            Some("Ctrl+x j")
        );
    }

    #[test]
    fn o_h_shows_or_hides_dot_names_and_alt_dot_comes_first_in_the_default_keymap() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["o", "h"]),
                actions(&[Action::ToggleHidden]),
                "{name}"
            );
        }
        assert_eq!(
            Keymap::mc()
                .key(Context::Panel, Action::ToggleHidden)
                .as_deref(),
            Some("Alt+.")
        );
    }

    #[test]
    fn tabs_live_under_t_brackets_and_alt_arrows() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &[
                    "t",
                    "n",
                    "t",
                    "c",
                    "]",
                    "t",
                    "[",
                    "t",
                    "t",
                    "h",
                    "t",
                    "l",
                    "alt-right",
                    "alt-left"
                ]
            ),
            actions(&[
                Action::NewTab,
                Action::CloseTab,
                Action::NextTab,
                Action::PrevTab,
                Action::TabListLeft,
                Action::TabListRight,
                Action::NextTab,
                Action::PrevTab
            ])
        );
    }

    #[test]
    fn t_w_lists_workspaces_t_shift_w_saves_and_the_window_takes_insert_f6_and_f8() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                feed(
                    &keymap,
                    &mut state,
                    Context::Panel,
                    &["t", "w", "t", "shift-w"]
                ),
                actions(&[Action::Workspaces, Action::SaveWorkspace]),
                "{name}"
            );
        }
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert!(
            !feed(&keymap, &mut state, Context::Panel, &["ctrl-x", "s"])
                .contains(&Resolved::Action(Action::SaveWorkspace)),
            "left for mc's symbolic links"
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Workspaces,
                &[
                    "2", "n", "space", "down", "enter", "insert", "f6", "f8", "esc"
                ]
            ),
            [
                Resolved::Insert('2'),
                Resolved::Insert('n'),
                Resolved::Insert(' '),
                Resolved::Action(Action::Down),
                Resolved::Action(Action::Confirm),
                Resolved::Action(Action::SaveWorkspace),
                Resolved::Action(Action::Move),
                Resolved::Action(Action::Delete),
                Resolved::Action(Action::Cancel),
            ]
        );
        let mut window = [None; 10];
        window[5] = Some(Action::Move);
        window[7] = Some(Action::Delete);
        window[9] = Some(Action::Cancel);
        assert_eq!(keymap.fkeys(Context::Workspaces), window);
    }

    #[test]
    fn location_menus_open_with_alt_f1_and_f2_or_ctrl_x_and_the_digit() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &["alt-f1", "alt-f2", "ctrl-x", "1", "ctrl-x", "2"]
            ),
            actions(&[
                Action::LocationMenuLeft,
                Action::LocationMenuRight,
                Action::LocationMenuLeft,
                Action::LocationMenuRight
            ])
        );
        // In the menu, every character filters or is a hotkey; keys move and act.
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::LocationMenu,
                &[
                    "1", "p", "+", "space", "down", "enter", "f8", "delete", "esc"
                ]
            ),
            [
                Resolved::Insert('1'),
                Resolved::Insert('p'),
                Resolved::Insert('+'),
                Resolved::Insert(' '),
                Resolved::Action(Action::Down),
                Resolved::Action(Action::Confirm),
                Resolved::Action(Action::Disconnect),
                Resolved::Action(Action::Disconnect),
                Resolved::Action(Action::Cancel),
            ]
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::LocationMenu, &["tab", "f5"]),
            [],
            "the menu is modal"
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::LocationMenu, &["ctrl-u"]),
            actions(&[Action::DeleteToStart]),
            "the filter is a text field"
        );
        let mut menu = [None; 10];
        menu[3] = Some(Action::EditHost);
        menu[7] = Some(Action::Disconnect);
        menu[9] = Some(Action::Cancel);
        assert_eq!(keymap.fkeys(Context::LocationMenu), menu);
    }

    #[test]
    fn the_zoxide_window_opens_with_alt_z_or_ctrl_x_z_and_takes_keywords() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &["alt-z", "ctrl-x", "z"]
            ),
            actions(&[Action::Jump, Action::Jump])
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Jump,
                &["1", "s", "space", "down", "backspace", "enter", "f8", "esc"]
            ),
            [
                Resolved::Insert('1'),
                Resolved::Insert('s'),
                Resolved::Insert(' '),
                Resolved::Action(Action::Down),
                Resolved::Action(Action::Backspace),
                Resolved::Action(Action::Confirm),
                Resolved::Action(Action::Cancel),
            ]
        );
    }

    #[test]
    fn tab_completes_in_path_fields_and_moves_in_the_list() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::PathInput,
                &["tab", "backtab", "down", "ctrl-u", "a", "esc"]
            ),
            [
                Resolved::Action(Action::Complete),
                Resolved::Action(Action::PrevField),
                Resolved::Action(Action::Down),
                Resolved::Action(Action::DeleteToStart),
                Resolved::Insert('a'),
                Resolved::Action(Action::Cancel),
            ]
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Completion,
                &["tab", "home", "enter", "backspace", "b", "esc"]
            ),
            [
                Resolved::Action(Action::Complete),
                Resolved::Action(Action::Home),
                Resolved::Action(Action::Confirm),
                Resolved::Action(Action::Backspace),
                Resolved::Insert('b'),
                Resolved::Action(Action::Cancel),
            ]
        );
    }

    #[test]
    fn quick_cd_opens_with_alt_c() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["alt-c"]),
            actions(&[Action::QuickCd])
        );
    }

    #[test]
    fn the_pull_down_menu_opens_with_f9_and_takes_letters() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["f9", "alt-9"]),
            actions(&[Action::PullDown, Action::PullDown])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["shift-f9", "f19"]),
            actions(&[Action::PullDownLast, Action::PullDownLast])
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::PullDown,
                &[
                    "v", "+", "left", "backtab", "tab", "pagedown", "enter", "f9", "esc"
                ]
            ),
            [
                Resolved::Insert('v'),
                Resolved::Insert('+'),
                Resolved::Action(Action::Left),
                Resolved::Action(Action::Left),
                Resolved::Action(Action::Right),
                Resolved::Action(Action::End),
                Resolved::Action(Action::Confirm),
                Resolved::Action(Action::Cancel),
                Resolved::Action(Action::Cancel),
            ]
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::PullDown, &["insert", "f5"]),
            [],
            "the menu is modal"
        );
    }

    #[test]
    fn key_names_the_first_sequence_that_does_the_action_there() {
        let keymap = Keymap::mc();
        let key = |context, action| keymap.key(context, action);
        assert_eq!(key(Context::Panel, Action::View).as_deref(), Some("F3"));
        assert_eq!(
            key(Context::Panel, Action::QuickSearch).as_deref(),
            Some("Ctrl+s")
        );
        assert_eq!(
            key(Context::Panel, Action::Checksum).as_deref(),
            Some("f #")
        );
        assert_eq!(key(Context::Panel, Action::Disconnect), None);
        assert_eq!(key(Context::Panel, Action::Edit).as_deref(), Some("F4"));
        assert_eq!(key(Context::Panel, Action::Delete).as_deref(), Some("F8"));
        // In the location menu F4 and F8 act on the host under the cursor.
        assert_eq!(
            key(Context::LocationMenu, Action::EditHost).as_deref(),
            Some("F4")
        );
        assert_eq!(
            key(Context::LocationMenu, Action::Disconnect).as_deref(),
            Some("F8")
        );
        assert_eq!(key(Context::LocationMenu, Action::Edit), None);
        assert_eq!(key(Context::LocationMenu, Action::Delete), None);
    }

    #[test]
    fn a_new_context_forgets_a_pending_sequence() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(feed(&keymap, &mut state, Context::Panel, &["ctrl-x"]), []);
        assert_eq!(
            feed(&keymap, &mut state, Context::DialogInput, &["j"]),
            [Resolved::Insert('j')]
        );
    }

    #[test]
    fn bang_and_colon_open_the_command_line_where_every_character_is_text() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["!", ":"]),
            actions(&[Action::Shell, Action::Command])
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::CommandLine,
                &[
                    "!",
                    "*",
                    "space",
                    "ctrl-j",
                    "shift-enter",
                    "ctrl-x",
                    "ctrl-e",
                    "ctrl-e",
                    "enter",
                    "f5",
                    "tab",
                    "esc"
                ]
            ),
            [
                Resolved::Insert('!'),
                Resolved::Insert('*'),
                Resolved::Insert(' '),
                Resolved::Action(Action::NewLine),
                Resolved::Action(Action::NewLine),
                Resolved::Action(Action::EditCommand),
                Resolved::Action(Action::End),
                Resolved::Action(Action::Confirm),
                Resolved::Action(Action::Cancel),
            ]
        );
    }

    #[test]
    fn only_ctrl_o_and_esc_leave_the_output_of_commands() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["ctrl-o"]),
            actions(&[Action::UserScreen])
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::UserScreen,
                &["a", "enter", "f10", "ctrl-o", "esc"]
            ),
            actions(&[Action::Cancel, Action::Cancel])
        );
    }

    #[test]
    fn describes_keys_as_the_docs_write_them() {
        let described = |keys: &str| describe(&parse_sequence(keys).unwrap());
        for (keys, text) in [
            ("ctrl-r", "Ctrl+r"),
            ("alt-shift-w", "Alt+W"),
            ("shift-g", "G"),
            ("shift-z shift-z", "Z Z"),
            ("ctrl-x t", "Ctrl+x t"),
            ("shift-f6", "Shift+F6"),
            ("shift-down", "Shift+Down"),
            ("backtab", "Shift+Tab"),
            ("pageup", "PgUp"),
            ("pagedown", "PgDn"),
            ("space", "Space"),
            ("alt-.", "Alt+."),
            ("alt-+", "Alt++"),
            ("esc esc", "Esc Esc"),
            ("ctrl-f3", "Ctrl+F3"),
        ] {
            assert_eq!(described(keys), text, "{keys}");
        }
        let typed = KeyCombination::from(KeyEvent::new(KeyCode::Char('*'), KeyModifiers::SHIFT));
        assert_eq!(describe(&[typed]), "*", "a symbol as it is typed");
    }

    #[test]
    fn shifted_letters_match_their_bindings() {
        assert_eq!(
            KeyCombination::from(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            parse_sequence("shift-a").unwrap()[0]
        );
    }

    #[test]
    fn f_key_bar_follows_the_bindings() {
        let keymap = Keymap::mc();
        let mut panel = [None; 10];
        panel[0] = Some(Action::Help);
        panel[2] = Some(Action::View);
        panel[3] = Some(Action::Edit);
        panel[4] = Some(Action::Copy);
        panel[5] = Some(Action::Move);
        panel[6] = Some(Action::Mkdir);
        panel[7] = Some(Action::Delete);
        panel[8] = Some(Action::PullDown);
        panel[9] = Some(Action::Quit);
        assert_eq!(keymap.fkeys(Context::Panel), panel);
        assert_eq!(keymap.fkeys(Context::QuickSearch), panel);
        let mut dialog = [None; 10];
        dialog[9] = Some(Action::Cancel);
        assert_eq!(keymap.fkeys(Context::Dialog), dialog);
    }

    #[test]
    fn the_default_preset_covers_every_action() {
        let keymap = Keymap::mc();
        for action in Action::ALL {
            assert!(
                keymap
                    .contexts
                    .values()
                    .any(|bindings| bindings.0.iter().any(|(_, bound)| bound == action)),
                "{action:?} has no key"
            );
        }
    }

    #[test]
    fn no_preset_binds_a_key_twice() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            for (context, bindings) in &keymap.contexts {
                for (index, (sequence, _)) in bindings.0.iter().enumerate() {
                    assert!(
                        !bindings.0[..index]
                            .iter()
                            .any(|(other, _)| other == sequence),
                        "{name}: {sequence:?} is bound twice in {context:?}"
                    );
                }
            }
        }
        assert!(Keymap::by_name("emacs").is_none());
    }

    #[test]
    fn the_vim_panels_do_what_the_default_ones_do() {
        let vim = Keymap::by_name("vim").unwrap();
        let mc = Keymap::mc();
        let panel = |keymap: &Keymap| -> Vec<Action> {
            keymap
                .help(Context::Panel)
                .into_iter()
                .map(|(action, _)| action)
                .collect()
        };
        assert_eq!(
            panel(&vim),
            panel(&mc),
            "in the order of the default preset"
        );
        assert_eq!(vim.fkeys(Context::Panel), mc.fkeys(Context::Panel));
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["!", ":", "g", "?"]),
            actions(&[Action::Shell, Action::Command, Action::Help])
        );
        // A character that no binding claims, which a panel takes for nothing.
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["u"]),
            [Resolved::Insert('u')]
        );
    }

    #[test]
    fn vim_letters_act_on_the_entries() {
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::Panel,
                &[
                    "shift-j", "space", "+", "-", "*", "shift-r", "o", "h", "o", "n", "o", "e",
                    "o", "m", "o", "s", "/", "q", ":", "i", "e", "y", "y", "x", "r", "shift-a",
                    "d", "shift-d", "f", "j", "g", "d", "g", "n", "g", "c", "g", "t", "g",
                    "shift-t", "ctrl-c"
                ]
            ),
            actions(&[
                Action::Mark,
                Action::Mark,
                Action::Select,
                Action::Unselect,
                Action::InvertMarks,
                Action::Reload,
                Action::ToggleHidden,
                Action::SortByName,
                Action::SortByExtension,
                Action::SortByTime,
                Action::SortBySize,
                Action::QuickSearch,
                Action::CommandHistory,
                Action::View,
                Action::Edit,
                Action::Copy,
                Action::Move,
                Action::Rename,
                Action::Mkdir,
                Action::Delete,
                Action::Jobs,
                Action::QuickCd,
                Action::NewTab,
                Action::CloseTab,
                Action::NextTab,
                Action::PrevTab,
                Action::Cancel,
            ])
        );
        // w no longer lists the jobs: f j does; v is left for a visual mode.
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["w", "v"]),
            [Resolved::Insert('w'), Resolved::Insert('v')]
        );
        // d alone deletes nothing: it waits for D, and d d is left for the trash.
        let now = Instant::now();
        for keys in [&["d"][..], &["d", "d"]] {
            let mut resolved = feed(&vim, &mut state, Context::Panel, keys);
            resolved.extend(vim.expire(&mut state, now + SEQUENCE_TIMEOUT));
            assert_eq!(resolved, [], "{keys:?}");
        }
        // In quick search, letters are the name searched for, and Ctrl+g goes to the next match.
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["y", "ctrl-g"]),
            [Resolved::Insert('y'), Resolved::Action(Action::QuickSearch)]
        );
    }

    #[test]
    fn vim_fields_and_the_command_line_take_the_keys_of_vims_command_line() {
        let vim = Keymap::by_name("vim").unwrap();
        for context in [Context::Rename, Context::DialogInput, Context::CommandLine] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["ctrl-b", "ctrl-e"]),
                actions(&[Action::Home, Action::End]),
                "{context:?}"
            );
        }
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::CommandLine,
                &["ctrl-p", "ctrl-n", "ctrl-f", "ctrl-r"]
            ),
            actions(&[
                Action::OlderCommand,
                Action::NewerCommand,
                Action::EditCommand,
                Action::CommandHistory
            ])
        );
        assert_eq!(
            feed(&vim, &mut state, Context::Dialog, &["ctrl-b", "ctrl-f"]),
            actions(&[Action::PageUp, Action::PageDown])
        );
        assert_eq!(
            feed(&vim, &mut state, Context::Completion, &["ctrl-p", "ctrl-n"]),
            actions(&[Action::Up, Action::Down])
        );
    }

    #[test]
    fn q_closes_the_help_in_vim_and_no_other_dialog() {
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::Help, &["q", "j", "esc"]),
            actions(&[Action::Cancel, Action::Down, Action::Cancel])
        );
        assert_eq!(feed(&vim, &mut state, Context::Dialog, &["q"]), []);
        let mc = Keymap::mc();
        assert_eq!(feed(&mc, &mut state, Context::Help, &["q"]), []);
        assert_eq!(
            feed(&mc, &mut state, Context::Help, &["esc", "f10"]),
            actions(&[Action::Cancel, Action::Cancel])
        );
    }

    #[test]
    fn j_and_k_move_down_and_up_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        for context in [Context::Panel, Context::Dialog, Context::Viewer] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["j", "k"]),
                actions(&[Action::Down, Action::Up]),
                "{context:?}"
            );
        }
        // Where letters are text or filter a list, they stay text.
        for context in [
            Context::DialogInput,
            Context::LocationMenu,
            Context::TabMenu,
            Context::QuickSearch,
        ] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["j", "k"]),
                [Resolved::Insert('j'), Resolved::Insert('k')],
                "{context:?}"
            );
        }
    }

    #[test]
    fn l_opens_the_entry_under_the_cursor_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["l", "enter"]),
            actions(&[Action::Enter, Action::Enter])
        );
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["l"]),
            [Resolved::Insert('l')],
            "a letter of the name searched for"
        );
    }

    #[test]
    fn h_and_left_go_to_the_parent_directory_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["h", "left"]),
            actions(&[Action::Parent, Action::Parent])
        );
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["h"]),
            [Resolved::Insert('h')],
            "a letter of the name searched for"
        );
    }

    #[test]
    fn g_g_and_shift_g_go_to_the_first_and_the_last_row_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        for context in [Context::Panel, Context::Dialog, Context::Viewer] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["g", "g", "shift-g"]),
                actions(&[Action::Home, Action::End]),
                "{context:?}"
            );
            // A lone `g` waits for its second key, then does nothing.
            let now = Instant::now();
            assert_eq!(vim.feed(&mut state, context, key("g"), now), []);
            assert_eq!(
                vim.expire(&mut state, now + SEQUENCE_TIMEOUT),
                [],
                "{context:?}"
            );
        }
        // Where letters are text, they stay text.
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::DialogInput,
                &["g", "g", "shift-g"]
            ),
            [
                Resolved::Insert('g'),
                Resolved::Insert('g'),
                Resolved::Insert('G')
            ]
        );
        assert_eq!(
            vim.help(Context::Viewer)
                .into_iter()
                .find(|(action, _)| *action == Action::Home),
            Some((Action::Home, "g g, Home".to_owned()))
        );
    }

    #[test]
    fn tab_and_ctrl_w_switch_and_swap_the_panels_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::Panel,
                &["tab", "ctrl-w", "w", "ctrl-w", "ctrl-w", "ctrl-w", "x"]
            ),
            actions(&[
                Action::SwitchPanel,
                Action::SwitchPanel,
                Action::SwitchPanel,
                Action::SwapPanels
            ])
        );
        // A lone Ctrl+w waits for its second key, then does nothing.
        let now = Instant::now();
        assert_eq!(vim.feed(&mut state, Context::Panel, key("ctrl-w"), now), []);
        assert_eq!(vim.expire(&mut state, now + SEQUENCE_TIMEOUT), []);
        // The location menus take the numbers, as Ctrl+x 1 and Ctrl+x 2 in the default keymap.
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::Panel,
                &["ctrl-w", "1", "ctrl-w", "2"]
            ),
            actions(&[Action::LocationMenuLeft, Action::LocationMenuRight])
        );
    }

    #[test]
    fn ctrl_w_h_and_l_or_ctrl_x_left_and_right_go_to_the_left_and_the_right_panel() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                feed(
                    &keymap,
                    &mut state,
                    Context::Panel,
                    &[
                        "ctrl-w", "h", "ctrl-w", "l", "ctrl-x", "left", "ctrl-x", "right"
                    ]
                ),
                actions(&[
                    Action::FocusLeft,
                    Action::FocusRight,
                    Action::FocusLeft,
                    Action::FocusRight
                ]),
                "{name}"
            );
            // The default keymap shares vim's Ctrl+w f and Ctrl+w v, which need no Alt.
            assert_eq!(
                feed(
                    &keymap,
                    &mut state,
                    Context::Panel,
                    &["ctrl-w", "f", "ctrl-w", "v"]
                ),
                actions(&[Action::OtherPanelOpen, Action::OtherPanelSync]),
                "{name}"
            );
        }
    }

    #[test]
    fn half_pages_with_shift_and_the_page_keys_or_vims_ctrl_u_and_ctrl_d() {
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &Keymap::mc(),
                &mut state,
                Context::Panel,
                &["shift-pageup", "shift-pagedown"]
            ),
            actions(&[Action::HalfPageUp, Action::HalfPageDown])
        );
        let vim = Keymap::by_name("vim").unwrap();
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["ctrl-u", "ctrl-d"]),
            actions(&[Action::HalfPageUp, Action::HalfPageDown])
        );
    }

    #[test]
    fn n_shift_n_and_shift_f7_repeat_the_last_quick_search() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                feed(
                    &keymap,
                    &mut state,
                    Context::Panel,
                    &["n", "shift-n", "shift-f7", "f17"]
                ),
                actions(&[
                    Action::NextMatch,
                    Action::PrevMatch,
                    Action::NextMatch,
                    Action::NextMatch
                ]),
                "{name}: F17, as terminals may send Shift+F7"
            );
            // While a search runs, n and N are part of the name; Shift+F7 goes on.
            assert_eq!(
                feed(
                    &keymap,
                    &mut state,
                    Context::QuickSearch,
                    &["n", "shift-n", "shift-f7"]
                ),
                [
                    Resolved::Insert('n'),
                    Resolved::Insert('N'),
                    Resolved::Action(Action::NextMatch)
                ],
                "{name}"
            );
        }
        // Enter ends vim's search on the match, and Ctrl+t goes back, as in vim; in the default
        // keymap Enter opens the entry, as in mc.
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["ctrl-t", "enter"]),
            actions(&[Action::PrevMatch, Action::Cancel])
        );
        assert_eq!(
            feed(&Keymap::mc(), &mut state, Context::QuickSearch, &["enter"]),
            actions(&[Action::Enter])
        );
    }

    #[test]
    fn space_marks_and_star_inverts_in_vim_and_backspace_goes_up_in_both() {
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["space", "*"]),
            actions(&[Action::Mark, Action::InvertMarks])
        );
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["backspace"]),
                actions(&[Action::Parent]),
                "{name}"
            );
            assert_eq!(
                feed(&keymap, &mut state, Context::QuickSearch, &["backspace"]),
                actions(&[Action::Backspace]),
                "{name}: a character of the search back"
            );
        }
    }

    #[test]
    fn g_m_opens_the_pull_down_menu_in_vim_and_its_letters_stay_letters() {
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["g", "m", "f9"]),
            actions(&[Action::PullDown, Action::PullDown])
        );
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::Panel,
                &["g", "shift-m", "shift-f9", "f19"]
            ),
            actions(&[
                Action::PullDownLast,
                Action::PullDownLast,
                Action::PullDownLast
            ])
        );
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::PullDown,
                &[
                    "h",
                    "j",
                    "k",
                    "l",
                    "ctrl-n",
                    "ctrl-p",
                    "ctrl-f",
                    "ctrl-b",
                    "shift-down",
                    "shift-up",
                    "tab",
                    "backtab",
                    "ctrl-c"
                ]
            ),
            [
                Resolved::Insert('h'),
                Resolved::Insert('j'),
                Resolved::Insert('k'),
                Resolved::Insert('l'),
                Resolved::Action(Action::Down),
                Resolved::Action(Action::Up),
                Resolved::Action(Action::End),
                Resolved::Action(Action::Home),
                Resolved::Action(Action::End),
                Resolved::Action(Action::Home),
                Resolved::Action(Action::Right),
                Resolved::Action(Action::Left),
                Resolved::Action(Action::Cancel),
            ]
        );
    }

    #[test]
    fn ctrl_h_takes_a_character_back_as_backspace_does() {
        let backspace = actions(&[Action::Backspace]);
        let mc = Keymap::mc();
        for context in [Context::DialogInput, Context::CommandLine] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&mc, &mut state, context, &["ctrl-h"]),
                backspace,
                "{context:?}"
            );
        }
        let vim = Keymap::by_name("vim").unwrap();
        for context in [
            Context::QuickSearch,
            Context::Rename,
            Context::LocationMenu,
            Context::TabMenu,
            Context::Jump,
            Context::Workspaces,
            Context::DialogInput,
            Context::CommandLine,
            Context::CommandHistory,
        ] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["ctrl-h"]),
                backspace,
                "{context:?}"
            );
        }
    }

    #[test]
    fn ctrl_c_and_f10_cancel_where_esc_does() {
        let cancel = |keymap: &Keymap, context, key| {
            let mut state = KeyState::default();
            feed(keymap, &mut state, context, &[key]) == actions(&[Action::Cancel])
        };
        let mc = Keymap::mc();
        for context in [Context::Completion, Context::CommandLine] {
            assert!(cancel(&mc, context, "f10"), "{context:?}");
        }
        assert!(!cancel(&mc, Context::Rename, "f10"));
        for context in [Context::CommandLine, Context::Panel, Context::QuickSearch] {
            assert!(cancel(&mc, context, "ctrl-c"), "{context:?}");
        }
        let vim = Keymap::by_name("vim").unwrap();
        // In vim's quick search, Ctrl+c gives the search up, as Esc does.
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["ctrl-c", "esc"]),
            actions(&[Action::Abort, Action::Abort])
        );
        for context in [
            Context::Rename,
            Context::LocationMenu,
            Context::TabMenu,
            Context::Jump,
            Context::Workspaces,
            Context::Completion,
            Context::CommandLine,
            Context::CommandHistory,
        ] {
            assert!(cancel(&vim, context, "ctrl-c"), "{context:?}");
        }
    }

    #[test]
    fn ctrl_bracket_and_ctrl_m_do_what_esc_and_enter_do() {
        // With the kitty keyboard protocol, they no longer arrive as Esc and Enter.
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            for (_, bindings) in preset(name).unwrap() {
                for (action, keys) in *bindings {
                    for (key, alias) in [("esc", "ctrl-["), ("enter", "ctrl-m")] {
                        assert_eq!(
                            keys.contains(&key),
                            keys.contains(&alias),
                            "{name}: {action:?} has {key} without {alias}, or the other way"
                        );
                    }
                }
            }
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::Dialog, &["ctrl-[", "ctrl-m"]),
                actions(&[Action::Cancel, Action::Confirm]),
                "{name}"
            );
        }
    }

    #[test]
    fn quick_search_gives_up_with_ctrl_g_in_the_default_keymap_and_esc_in_vim() {
        let mc = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &mc,
                &mut state,
                Context::QuickSearch,
                &["ctrl-g", "esc", "ctrl-r", "ctrl-w"]
            ),
            actions(&[
                Action::Abort,
                Action::Cancel,
                Action::PrevMatch,
                Action::DeleteWord
            ])
        );
        let vim = Keymap::by_name("vim").unwrap();
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["enter", "ctrl-w"]),
            actions(&[Action::Cancel, Action::DeleteWord]),
            "Ctrl+w deletes a word rather than waiting for a window command"
        );
    }

    #[test]
    fn shift_tab_is_the_back_tab_terminals_send() {
        assert_eq!(parse_sequence("shift-tab"), parse_sequence("backtab"));
        assert_eq!(
            parse_sequence("backtab"),
            Some(vec![KeyCombination::from(KeyEvent::new(
                KeyCode::BackTab,
                KeyModifiers::SHIFT
            ))])
        );
    }

    #[test]
    fn the_location_menu_moves_as_the_panels_do_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &vim,
                &mut state,
                Context::LocationMenu,
                &["ctrl-n", "ctrl-p", "ctrl-f", "ctrl-b", "ctrl-y", "ctrl-c"]
            ),
            actions(&[
                Action::Down,
                Action::Up,
                Action::PageDown,
                Action::PageUp,
                Action::Confirm,
                Action::Cancel
            ])
        );
    }

    #[test]
    fn windows_that_list_items_share_the_keys_of_the_list() {
        let windows = [
            Context::LocationMenu,
            Context::TabMenu,
            Context::Jump,
            Context::Workspaces,
            Context::CommandHistory,
        ];
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let (_, bound) = preset(name)
                .unwrap()
                .iter()
                .find(|(context, _)| *context == Context::List)
                .unwrap();
            assert!(!bound.is_empty(), "{name}");
            for (action, keys) in *bound {
                for keys in *keys {
                    for context in windows {
                        let mut state = KeyState::default();
                        assert_eq!(
                            feed(&keymap, &mut state, context, &[*keys]),
                            actions(&[*action]),
                            "{name}: `{keys}` in {context:?}"
                        );
                    }
                }
            }
            // The help lists them with each window, after the window's own keys and before
            // those of the filter.
            let actions = |rows: Vec<(Action, String)>| -> Vec<Action> {
                rows.into_iter().map(|(action, _)| action).collect()
            };
            let list = actions(keymap.help(Context::List));
            for context in windows {
                let help = actions(keymap.help(context));
                assert!(
                    help.windows(list.len()).any(|rows| rows == list),
                    "{name}: {context:?}"
                );
            }
        }
    }

    #[test]
    fn the_filters_of_the_lists_are_text_fields_whose_home_and_end_go_to_rows() {
        let windows = [
            Context::LocationMenu,
            Context::TabMenu,
            Context::Jump,
            Context::Workspaces,
            Context::CommandHistory,
        ];
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            for context in windows {
                assert_eq!(
                    context.chain()[1..],
                    [Context::List, Context::Input, Context::Global],
                    "{context:?}"
                );
                let mut state = KeyState::default();
                assert_eq!(
                    feed(
                        &keymap,
                        &mut state,
                        context,
                        &[
                            "left",
                            "right",
                            "backspace",
                            "ctrl-u",
                            "ctrl-k",
                            "home",
                            "end"
                        ]
                    ),
                    actions(&[
                        Action::Left,
                        Action::Right,
                        Action::Backspace,
                        Action::DeleteToStart,
                        Action::DeleteToEnd,
                        Action::Home,
                        Action::End,
                    ]),
                    "{name}: {context:?}"
                );
            }
            // Home is the list's and the filter's: the help shows it once.
            for context in Context::ALL {
                for (action, keys) in keymap.help(*context) {
                    let keys: Vec<&str> = keys.split(", ").collect();
                    for (index, key) in keys.iter().enumerate() {
                        assert!(
                            !keys[..index].contains(key),
                            "{name}: {key} twice for {action:?} in {context:?}"
                        );
                    }
                }
            }
            // Delete takes a character of the filter, but where the window deletes rows.
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::TabMenu, &["delete"]),
                actions(&[Action::Delete]),
                "{name}"
            );
            assert_eq!(
                feed(&keymap, &mut state, Context::LocationMenu, &["delete"]),
                actions(&[Action::Disconnect]),
                "{name}"
            );
        }
    }

    #[test]
    fn every_f_key_has_an_alt_digit_alias_where_alt_digits_stand_for_f_keys() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            for (context, bindings) in &keymap.contexts {
                if !context.alt_digits_stand_for_f_keys() {
                    continue;
                }
                for (sequence, action) in &bindings.0 {
                    if let [key] = sequence.as_slice()
                        && key.modifiers.is_empty()
                        && let KeyCode::F(number @ 1..=10) = *key.codes.first()
                    {
                        let alias = parse_sequence(&format!("alt-{}", number % 10)).unwrap();
                        assert!(
                            bindings.0.contains(&(alias, *action)),
                            "{name}: F{number} in {context:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn help_lists_keys_by_action_without_alt_digit_aliases() {
        let keymap = Keymap::mc();
        let panel = keymap.help(Context::Panel);
        let keys = |action| {
            panel
                .iter()
                .find(|(bound, _)| *bound == action)
                .map(|(_, keys)| keys.as_str())
        };
        assert_eq!(keys(Action::Up), Some("Up, Ctrl+p"));
        assert_eq!(keys(Action::Quit), Some("F10"));
        assert_eq!(keys(Action::Cancel), Some("Esc, Ctrl+[, Ctrl+c"));
        assert_eq!(keys(Action::SortByName), Some("o n"));
        assert_eq!(panel[0].0, Action::Up, "in the order of the preset");
        assert_eq!(
            keymap.help(Context::LocationMenu)[..3],
            [
                (Action::EditHost, "F4".to_owned()),
                (Action::Disconnect, "F8, Delete".to_owned()),
                (Action::Reload, "Ctrl+r".to_owned())
            ],
            "the menu's own keys first"
        );
    }

    #[test]
    fn presets_list_every_context_in_the_same_order() {
        let contexts = |preset: Preset| -> Vec<Context> {
            preset.iter().map(|(context, _)| *context).collect()
        };
        let default = contexts(default::PRESET);
        assert_eq!(default.len(), Context::ALL.len());
        for context in Context::ALL {
            assert!(default.contains(context), "{context:?}");
        }
        for name in Keymap::NAMES {
            assert_eq!(contexts(preset(name).unwrap()), default, "{name}");
        }
    }

    #[test]
    fn every_preset_key_parses_and_no_action_is_listed_twice() {
        for name in Keymap::NAMES {
            for (context, bindings) in preset(name).unwrap() {
                for (index, (action, keys)) in bindings.iter().enumerate() {
                    assert!(
                        !bindings[..index].iter().any(|(other, _)| other == action),
                        "{name}: {action:?} twice in {context:?}"
                    );
                    for keys in *keys {
                        assert!(parse_sequence(keys).is_some(), "{name}: `{keys}`");
                    }
                }
            }
        }
    }

    fn hint_keys(hints: &[Hint]) -> Vec<&str> {
        hints.iter().map(|hint| hint.keys.as_str()).collect()
    }

    #[test]
    fn hints_list_the_keys_that_can_follow_in_the_order_of_the_preset() {
        let keymap = Keymap::mc();
        let t = parse_sequence("t").unwrap();
        let hints = keymap.hints(Context::Panel, &t);
        assert_eq!(hint_keys(&hints), ["n", "c", "h", "l", "w", "W"]);
        assert_eq!(
            hints[0],
            Hint {
                keys: "n".to_owned(),
                action: Some(Action::NewTab),
                more: 0
            }
        );
        let ctrl_x = parse_sequence("ctrl-x").unwrap();
        let hints = keymap.hints(Context::Panel, &ctrl_x);
        assert_eq!(hint_keys(&hints), ["Left", "Right", "1", "2", "z", "j"]);
        // Quick search falls back to the panel.
        assert_eq!(keymap.hints(Context::QuickSearch, &ctrl_x), hints);
        let vim = Keymap::by_name("vim").unwrap();
        let g = parse_sequence("g").unwrap();
        assert_eq!(
            hint_keys(&vim.hints(Context::Panel, &g)),
            ["g", "m", "M", "n", "c", "t", "T", "z", "d", "?", "<"]
        );
    }

    #[test]
    fn hints_of_every_key_share_rows_by_action_and_count_the_keys_under_a_prefix() {
        let keymap = Keymap::mc();
        let menu = keymap.hints(Context::LocationMenu, &[]);
        assert_eq!(
            hint_keys(&menu[..3]),
            ["F4", "F8, Delete", "Ctrl+r"],
            "the menu's own keys first"
        );
        assert_eq!(menu[0].action, Some(Action::EditHost));
        let hints = keymap.hints(Context::Panel, &[]);
        let hint = |keys: &str| hints.iter().find(|hint| hint.keys == keys);
        assert_eq!(
            hint("Up, Ctrl+p").map(|hint| hint.action),
            Some(Some(Action::Up))
        );
        assert_eq!(
            hint("Ctrl+x"),
            Some(&Hint {
                keys: "Ctrl+x".to_owned(),
                action: None,
                more: 6
            })
        );
        assert_eq!(hint("Ctrl+w").map(|hint| hint.more), Some(4));
        assert_eq!(hint("t").map(|hint| hint.more), Some(6));
        assert!(
            hints.iter().all(|hint| !hint.keys.contains("Esc")),
            "Esc closes the hints"
        );
        let vim = Keymap::by_name("vim").unwrap();
        let hints = vim.hints(Context::Panel, &[]);
        let hint = |keys: &str| hints.iter().find(|hint| hint.keys == keys).cloned();
        assert_eq!(hint("g").map(|hint| hint.more), Some(11));
        assert_eq!(hint("Ctrl+w").map(|hint| hint.more), Some(11));
        assert_eq!(hint("Ctrl+x").map(|hint| hint.more), Some(2));
        assert_eq!(hint("Z").map(|hint| hint.more), Some(1));
        // Where characters are text, the panel's are left out.
        let search = keymap.hints(Context::QuickSearch, &[]);
        assert!(!hint_keys(&search).contains(&"*"));
        assert!(hint_keys(&search).contains(&"Ctrl+s, Alt+s"));
    }

    #[test]
    fn hints_wait_for_the_next_key_which_runs_as_it_would_have() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        let start = Instant::now();
        let delay = Duration::from_millis(500);
        assert_eq!(keymap.hints_due(&state, delay), None);
        assert_eq!(keymap.feed(&mut state, Context::Panel, key("t"), start), []);
        assert_eq!(keymap.hints_due(&state, delay), Some(start + delay));
        assert_eq!(keymap.hints_of(&state, Context::Panel), None, "not yet");
        keymap.show_hints(&mut state, Context::Panel);
        assert_eq!(state.deadline(), None, "no timeout while they show");
        assert_eq!(keymap.hints_due(&state, delay), None, "they show");
        let hints = keymap.hints_of(&state, Context::Panel).unwrap();
        assert_eq!(hints.typed.as_deref(), Some("t"));
        assert_eq!(keymap.hints_of(&state, Context::Dialog), None);
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("n"), start),
            actions(&[Action::NewTab])
        );
        assert_eq!(keymap.hints_of(&state, Context::Panel), None, "done");
        // Esc never has hints of its own: it acts alone after its timeout.
        keymap.feed(&mut state, Context::Panel, key("esc"), start);
        assert_eq!(keymap.hints_due(&state, delay), None);
    }

    #[test]
    fn esc_closes_the_hints_and_backspace_takes_back_a_key() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        let now = Instant::now();
        keymap.show_hints(&mut state, Context::Panel);
        let every_key = keymap.hints_of(&state, Context::Panel).unwrap();
        assert_eq!(every_key.typed, None);
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("ctrl-x"), now),
            []
        );
        assert_eq!(state.deadline(), None, "the hints show at once");
        assert_eq!(
            keymap.hints_of(&state, Context::Panel).unwrap().typed,
            Some("Ctrl+x".to_owned())
        );
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("backspace"), now),
            []
        );
        assert_eq!(keymap.hints_of(&state, Context::Panel), Some(every_key));
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("esc"), now),
            [],
            "no Cancel"
        );
        assert_eq!(keymap.hints_of(&state, Context::Panel), None);
        assert_eq!(state.deadline(), None, "no Esc pending");
        // A key that does something closes them too.
        keymap.show_hints(&mut state, Context::Panel);
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("f10"), now),
            actions(&[Action::Quit])
        );
        assert_eq!(keymap.hints_of(&state, Context::Panel), None);
        // Backspace with nothing typed does what it does.
        let vim = Keymap::by_name("vim").unwrap();
        vim.show_hints(&mut state, Context::Panel);
        assert_eq!(
            vim.feed(&mut state, Context::Panel, key("backspace"), now),
            actions(&[Action::Parent])
        );
    }

    #[test]
    fn ctrl_backtick_shows_the_output_of_commands_everywhere() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            // Ctrl+o, mc's, only in the default keymap; Ctrl+Space, which switches the input
            // source on macOS, in neither.
            let mut keys = vec![("ctrl-`", "Ctrl+`")];
            if *name == "default" {
                keys.push(("ctrl-o", "Ctrl+o"));
            }
            for &context in Context::ALL {
                if matches!(context, Context::Global | Context::UserScreen) {
                    continue;
                }
                let hints = keymap.hints(context, &[]);
                for (key, text) in &keys {
                    let mut state = KeyState::default();
                    assert_eq!(
                        feed(&keymap, &mut state, context, &[key]),
                        actions(&[Action::UserScreen]),
                        "{name}: {context:?} {key}"
                    );
                    assert!(
                        hints
                            .iter()
                            .any(|hint| hint.action == Some(Action::UserScreen)
                                && hint.keys.split(", ").any(|keys| keys == *text)),
                        "{name}: {context:?} hints {key}"
                    );
                }
            }
            let mut state = KeyState::default();
            for (key, _) in &keys {
                assert_eq!(
                    feed(&keymap, &mut state, Context::UserScreen, &[key]),
                    actions(&[Action::Cancel]),
                    "{name}: {key}"
                );
            }
        }
    }

    #[test]
    fn ctrl_space_is_left_to_the_input_source_and_g_lt_shows_the_output_in_vim() {
        let ctrl_space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL);
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            let mut state = KeyState::default();
            assert_eq!(
                keymap.feed(&mut state, Context::Panel, ctrl_space, Instant::now()),
                [],
                "{name}"
            );
        }
        // g < shows the output of the last command in vim; again, it goes back.
        let vim = Keymap::by_name("vim").unwrap();
        for (context, action) in [
            (Context::Panel, Action::UserScreen),
            (Context::Viewer, Action::UserScreen),
            (Context::Dialog, Action::UserScreen),
            (Context::UserScreen, Action::Cancel),
        ] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["g", "<"]),
                actions(&[action]),
                "{context:?}"
            );
        }
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["g", "<"]),
            [Resolved::Insert('g'), Resolved::Insert('<')],
            "text where text is typed"
        );
    }

    #[test]
    fn alt_slash_asks_for_the_hints_everywhere_but_on_the_user_screen() {
        for name in Keymap::NAMES {
            let keymap = Keymap::by_name(name).unwrap();
            for &context in Context::ALL {
                if matches!(context, Context::Global | Context::UserScreen) {
                    continue;
                }
                let mut state = KeyState::default();
                assert_eq!(
                    feed(&keymap, &mut state, context, &["alt-/"]),
                    actions(&[Action::KeyHints]),
                    "{name}: {context:?}"
                );
                let hints = keymap.hints(context, &[]);
                assert!(
                    hints
                        .iter()
                        .any(|hint| hint.keys == "Alt+/" && hint.action == Some(Action::KeyHints)),
                    "{name}: {context:?} hints"
                );
            }
            // Alt+/ is the one key for them; ? in a panel is just a character.
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["?"]),
                [Resolved::Insert('?')],
                "{name}"
            );
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, Context::UserScreen, &["alt-/"]),
                [],
                "{name}"
            );
            // Esc does not wait for /, so Esc / in vim's panels cancels, then searches.
            let slash = if *name == "vim" {
                Resolved::Action(Action::QuickSearch)
            } else {
                Resolved::Insert('/')
            };
            assert_eq!(
                feed(&keymap, &mut state, Context::Panel, &["esc", "/"]),
                [Resolved::Action(Action::Cancel), slash],
                "{name}"
            );
        }
    }

    #[test]
    fn alt_and_a_shifted_symbol_match_as_the_kitty_protocol_reports_alternate_keys() {
        // With alternate keys reported, crossterm gives the symbol typed and drops the Shift.
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        let now = Instant::now();
        let mut alt = |c| {
            let event = KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
            keymap.feed(&mut state, Context::Panel, event, now)
        };
        assert_eq!(alt('+'), actions(&[Action::Select]));
        assert_eq!(alt('*'), actions(&[Action::InvertMarks]));
        assert_eq!(alt('/'), actions(&[Action::KeyHints]));
        // No preset binds Alt and an uppercase letter, so one is made up for it.
        let keymap = Keymap::of(&[(Context::Panel, &[(Action::SaveWorkspace, &["alt-shift-w"])])]);
        let upper = KeyEvent::new(KeyCode::Char('W'), KeyModifiers::ALT);
        assert_eq!(
            keymap.feed(&mut KeyState::default(), Context::Panel, upper, now),
            actions(&[Action::SaveWorkspace]),
            "Shift comes back"
        );
    }

    #[test]
    fn rejects_invalid_sequences() {
        assert_eq!(parse_sequence(""), None);
        assert_eq!(parse_sequence("ctrl-"), None);
        assert_eq!(parse_sequence("f10 nosuchkey"), None);
        assert_eq!(parse_sequence("esc 0").map(|keys| keys.len()), Some(2));
    }
}
