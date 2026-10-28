//! Key bindings: key sequences mapped to actions per mode and context, with the built-in
//! preset in `preset.rs`.
//!
//! The keys are vim's, in its modes (ADR 0022): the app tells which mode keys are typed in, and
//! the keymap looks them up in that mode. In Normal and Visual mode, a count can go before a
//! command, as in `3j`, and a register after `"`, as in `"ayw`; some commands take the
//! character typed after them, as `f` does. In Insert mode, characters are text.
//!
//! A binding is a sequence of one or more key combinations, such as `f10` or `g g`. While the
//! keys typed so far are the start of a longer binding, the keymap waits up to
//! [`SEQUENCE_TIMEOUT`] for the next key. `Esc` never waits: it acts at once, or gives up what
//! was typed so far.
//!
//! As which-key does, the keymap lists the keys that can follow a pending sequence, or every
//! key of a context, with what they do ([`Keymap::hints`]). While the hints show, the sequence
//! waits for its next key without a deadline; `Esc` closes them, and `Backspace` takes back the
//! last key of the sequence, or with none closes them. The key of the hints typed again while
//! they show gives them the keys, as a list of whole sequences ([`Keymap::sequences`]).

mod action;
mod preset;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crokey::KeyCombination;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) use action::{Action, Context, Mode, Place};

/// How long a sequence such as `g g` waits for its next key.
pub(crate) const SEQUENCE_TIMEOUT: Duration = Duration::from_secs(1);

/// The largest count; more digits change nothing.
pub(crate) const MAX_COUNT: usize = 99_999;

/// What keys resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// A bound action.
    Action(Action),
    /// A printable character typed in Insert mode that no binding claimed, or after Ctrl+v.
    Insert(char),
    /// Alt and a letter or a digit that nothing binds, where those mark what they act on: the
    /// commands of the pull-down menu, and the buttons, fields, check boxes, and choices of the
    /// dialogs.
    Hotkey(char),
}

/// What vim lets come with a command: a count and a register before it, and a character after
/// it; and noc's Ctrl+x before them, which runs it in the other panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Extra {
    /// The count typed before it: `3` in `3j`.
    pub(crate) count: Option<usize>,
    /// The register named before it: `a` in `"ayw`.
    pub(crate) register: Option<char>,
    /// The count typed before the register, which `count` takes in: `2` in `2"a3d`, for the
    /// command to show as typed.
    pub(crate) outer: Option<usize>,
    /// The character typed after an action that takes one: `x` in `fx`.
    pub(crate) char: Option<char>,
    /// Ctrl+x came before it: it runs in the other panel, which has the keys until it ends.
    pub(crate) other: bool,
}

impl Extra {
    /// The count, or 1 without one.
    pub(crate) fn times(self) -> usize {
        self.count.unwrap_or(1)
    }
}

/// What keys resolved to, with what came with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Typed {
    pub(crate) resolved: Resolved,
    pub(crate) extra: Extra,
}

impl From<Resolved> for Typed {
    fn from(resolved: Resolved) -> Self {
        Self {
            resolved,
            extra: Extra::default(),
        }
    }
}

type Sequence = Vec<KeyCombination>;

/// The bindings of a context in a built-in preset: each action with its key sequences.
type Bound = &'static [(Action, &'static [&'static str])];

/// A built-in preset: each mode and context with its bindings.
type Preset = &'static [(Mode, Context, Bound)];

/// Whether the terminal tells Ctrl+i from Tab. Both send the same byte, unless the terminal
/// speaks the kitty keyboard protocol, which reports Ctrl+i apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Keyboard {
    /// Ctrl+i arrives as Tab, and Tab does what Ctrl+i does, as in vim.
    #[default]
    Legacy,
    /// The kitty keyboard protocol: Tab and Ctrl+i do what the preset binds each to.
    Kitty,
}

/// Bindings for every mode and context.
#[derive(Debug, Clone)]
pub(crate) struct Keymap {
    bindings: HashMap<(Mode, Context), Bindings>,
}

/// The bindings of one context in one mode, in the order they were added.
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

/// The keys of a command typed so far.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyState {
    place: Option<Place>,
    keys: Sequence,
    deadline: Option<Instant>,
    /// When the last key of the pending sequence came.
    since: Option<Instant>,
    /// The hints show: of the keys that can follow the sequence, or of every key while none is
    /// typed.
    hinting: bool,
    /// The count typed so far.
    count: Option<usize>,
    /// The count typed before the register, which the count after it multiplies, as in vim's
    /// `2"a3x`.
    outer: Option<usize>,
    /// The register named after `"`.
    register: Option<char>,
    /// The keys of Ctrl+x, if typed: the command to run in the other panel follows.
    other: Option<Sequence>,
    /// An action that waits for the character typed after it, and the keys that typed it.
    awaiting: Option<(Action, Sequence)>,
    /// A command was given up since the app last asked: by Esc, or by a key that cannot go on
    /// with it. The app gives up its part too, such as an operator that waits in a text field.
    dropped: bool,
    /// An operator of a text field waits for its motion, as the app says before each key.
    operator: bool,
}

impl KeyState {
    /// When the pending sequence times out; `None` if no sequence is pending, or while the
    /// hints show.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Where the keys typed last went.
    pub(crate) fn place(&self) -> Option<Place> {
        self.place
    }

    /// What the pending command has typed so far, as vim's `showcmd` shows it, in the order it
    /// was typed: Ctrl+x, the count, the register, and the keys, such as `2"ag`, `^W`, or
    /// `^X3g`.
    pub(crate) fn showcmd(&self) -> String {
        let mut text: String = self
            .other
            .iter()
            .flatten()
            .map(|key| short_key(*key))
            .collect();
        if let Some(count) = self.outer {
            text.push_str(&count.to_string());
        }
        if let Some(register) = self.register {
            text.push('"');
            text.push(register);
        }
        if let Some(count) = self.count {
            text.push_str(&count.to_string());
        }
        let awaiting = self.awaiting.iter().flat_map(|(_, keys)| keys);
        for key in self.keys.iter().chain(awaiting) {
            text.push_str(&short_key(*key));
        }
        text
    }

    fn clear(&mut self) {
        self.keys.clear();
        self.deadline = None;
        self.since = None;
    }

    /// Whether a command was given up since the last call, by Esc or by a key that cannot go
    /// on with it.
    pub(crate) fn take_dropped(&mut self) -> bool {
        std::mem::take(&mut self.dropped)
    }

    /// Says whether an operator of a text field waits for its motion: `"` then gives it up and
    /// names no register, as in vim, so that the key after it is a command of its own.
    pub(crate) fn set_operator(&mut self, waits: bool) {
        self.operator = waits;
    }

    /// Forgets the whole command: the keys, the counts, the register, Ctrl+x, and what waits
    /// for a character.
    fn reset(&mut self) {
        self.clear();
        self.count = None;
        self.outer = None;
        self.register = None;
        self.other = None;
        self.awaiting = None;
    }

    /// Gives up the whole command, and says so to the app.
    fn drop_command(&mut self) {
        self.reset();
        self.dropped = true;
    }

    /// The count, the register, and Ctrl+x typed for the command that resolves now, which they
    /// leave: the counts before and after the register multiplied.
    fn take_extra(&mut self) -> Extra {
        let outer = self.outer.take();
        let count = match (outer, self.count.take()) {
            (Some(outer), Some(count)) => Some(outer.saturating_mul(count).min(MAX_COUNT)),
            (outer, count) => count.or(outer),
        };
        Extra {
            count,
            register: self.register.take(),
            outer,
            char: None,
            other: self.other.take().is_some(),
        }
    }

    /// Whether a count, a register, or Ctrl+x waits for the command they go with.
    fn waits_with(&self) -> bool {
        self.count.is_some()
            || self.outer.is_some()
            || self.register.is_some()
            || self.other.is_some()
    }

    /// Whether anything of a command is typed.
    fn pending(&self) -> bool {
        !self.keys.is_empty() || self.waits_with() || self.awaiting.is_some()
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
    pub(crate) place: Place,
    /// The keys typed so far as the help writes them, Ctrl+x with them; `None` while none is
    /// typed.
    pub(crate) typed: Option<String>,
    /// The keys typed so far.
    pub(crate) prefix: Vec<KeyCombination>,
    pub(crate) rows: Vec<Hint>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new(Keyboard::default())
    }
}

impl Keymap {
    /// The preset, for a terminal that sends its keys as `keyboard` says.
    pub(crate) fn new(keyboard: Keyboard) -> Self {
        let mut keymap = Self::of(preset::PRESET);
        if keyboard == Keyboard::Legacy {
            for bindings in keymap.bindings.values_mut() {
                bindings.0 = tab_as_ctrl_i(std::mem::take(&mut bindings.0));
            }
        }
        keymap
    }

    fn of(preset: Preset) -> Self {
        let mut bindings = HashMap::new();
        for (mode, context, bound) in preset {
            let bound = bound
                .iter()
                .flat_map(|(action, keys)| {
                    keys.iter().map(|keys| {
                        let sequence = parse_sequence(keys)
                            .unwrap_or_else(|| unreachable!("invalid preset key `{keys}`"));
                        (sequence, *action)
                    })
                })
                .collect();
            bindings.insert((*mode, *context), Bindings(bound));
        }
        Self { bindings }
    }

    /// The bindings of `context` in `mode`; those of `Global` are the same in every mode.
    fn bindings(&self, mode: Mode, context: Context) -> Option<&Bindings> {
        let mode = if context == Context::Global {
            Mode::Normal
        } else {
            mode
        };
        self.bindings.get(&(mode, context))
    }

    /// What the first context in `place`'s chain that knows `keys` knows about them. A prefix
    /// that this context does not bind itself does, once it times out, what a later context
    /// binds it to.
    fn lookup(&self, place: Place, keys: &[KeyCombination]) -> Lookup {
        let mut known = place
            .chain()
            .filter_map(|context| self.bindings(place.mode, context))
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

    /// Whether `context` binds `keys` itself in `mode`, not a context it falls back to.
    fn binds_itself(&self, mode: Mode, context: Context, keys: &[KeyCombination]) -> bool {
        self.bindings(mode, context)
            .is_some_and(|own| own.lookup(keys) != Lookup::Unknown)
    }

    /// Feeds one key press, typed where `place` says. Returns what to do now; nothing while a
    /// command waits for its next key. A change of place forgets a pending command, and closes
    /// the hints.
    pub(crate) fn feed(
        &self,
        state: &mut KeyState,
        place: impl Into<Place>,
        event: KeyEvent,
        now: Instant,
    ) -> Vec<Typed> {
        let place = place.into();
        if state.hinting && state.place() == Some(place) {
            let key = KeyCombination::from(event);
            let mut keys = state.keys.clone();
            keys.push(key);
            let continues = !state.keys.is_empty() && self.lookup(place, &keys) != Lookup::Unknown;
            if !continues && (key == ESC || key == CTRL_BRACKET) {
                state.drop_command();
                state.hinting = false;
                return Vec::new();
            }
            // Backspace takes back the last key typed; with none, it closes the hints, rather
            // than doing what it does in what is in front.
            if !continues && (key == BACKSPACE || key == CTRL_H) {
                if state.keys.pop().is_none() {
                    state.hinting = false;
                }
                state.deadline = None;
                return Vec::new();
            }
        }
        let typed = self.feed_key(state, place, event, now);
        if state.keys.is_empty() {
            state.hinting = false;
        }
        typed
    }

    fn feed_key(
        &self,
        state: &mut KeyState,
        place: Place,
        event: KeyEvent,
        now: Instant,
    ) -> Vec<Typed> {
        let Place { mode, context, .. } = place;
        if state.place() != Some(place) {
            state.reset();
            state.hinting = false;
            state.place = Some(place);
        }
        let key = KeyCombination::from(event);
        if let Some((action, _)) = state.awaiting.take() {
            return Self::argument(state, action, event);
        }
        // Esc gives up what is typed of a command, rather than doing what it does alone.
        if (key == ESC || key == CTRL_BRACKET) && state.pending() {
            state.drop_command();
            return Vec::new();
        }
        if mode != Mode::Insert
            && state.keys.is_empty()
            && let Some(digit) = digit(event)
            && (digit > 0 || state.count.is_some())
        {
            let count = state.count.unwrap_or(0).saturating_mul(10);
            state.count = Some(count.saturating_add(digit).min(MAX_COUNT));
            return Vec::new();
        }
        if mode == Mode::Insert
            && state.keys.is_empty()
            && let Some(c) = text(event)
            && !self.binds_itself(mode, context, &[key])
        {
            return vec![Typed::from(Resolved::Insert(c))];
        }
        if context.alt_letters_are_hotkeys()
            && state.keys.is_empty()
            && let Some(plain) = without_alt(event)
            && let Some(c) = text(plain)
            && self.lookup(place, &[key]) == Lookup::Unknown
        {
            // The letter is the command: a count typed before it goes with nothing.
            state.reset();
            return vec![Typed::from(Resolved::Hotkey(c))];
        }
        state.keys.push(key);
        match self.lookup(place, &state.keys) {
            // The hints show what can follow a count, a register, or Ctrl+x, which wait for it,
            // as they do after the first key of a sequence.
            Lookup::Exact(Action::KeyHints) if state.keys.len() == 1 && state.waits_with() => {
                state.keys.clear();
                if state.hinting {
                    return Self::focus_hints(state);
                }
                vec![Typed::from(Resolved::Action(Action::KeyHints))]
            }
            Lookup::Exact(action) => {
                let typed = Self::resolve(state, action);
                // The hints after Ctrl+x are due as those of a key that starts others.
                if action == Action::OtherPanel && state.other.is_some() {
                    state.since = Some(now);
                }
                typed.into_iter().collect()
            }
            Lookup::Prefix(own) => {
                state.since = Some(now);
                // While the hints show, a sequence that does nothing alone waits.
                let waits = state.hinting && own.is_none();
                state.deadline = (!waits).then_some(now + SEQUENCE_TIMEOUT);
                Vec::new()
            }
            Lookup::Unknown if state.keys.len() == 1 => {
                if let Some(c) = text(event).filter(|_| mode == Mode::Insert) {
                    state.reset();
                    return vec![Typed::from(Resolved::Insert(c))];
                }
                // A key that does nothing gives up the command it comes in, such as `d` and a
                // key that is no motion.
                state.drop_command();
                Vec::new()
            }
            Lookup::Unknown => {
                state.keys.pop();
                // The key of the hints shows what can follow the keys typed so far; again, it
                // gives the hints the keys.
                if self.lookup(place, &[key]) == Lookup::Exact(Action::KeyHints) {
                    if state.hinting {
                        return Self::focus_hints(state);
                    }
                    self.show_hints(state, place);
                    return Vec::new();
                }
                // In Normal and Visual mode, as in vim, a key that cannot go on with a
                // sequence that does nothing alone gives up both, as `g x` or Ctrl+w and a key
                // that names no panel: run alone, it would do what was never asked.
                let own = matches!(self.lookup(place, &state.keys), Lookup::Prefix(Some(_)));
                if mode != Mode::Insert && !own {
                    state.drop_command();
                    return Vec::new();
                }
                // Else the pending sequence settles as if it had timed out, and the new key is
                // a key of its own, which may be a count, text, or the character an action
                // waits for.
                let mut typed = self.settle(state);
                typed.extend(self.feed_key(state, place, event, now));
                typed
            }
        }
    }

    /// The key of the hints typed while they show: the app gives them the keys, with the count
    /// and the register typed, for the command run from them. The keys typed so far stay in the
    /// hints the app has.
    fn focus_hints(state: &mut KeyState) -> Vec<Typed> {
        let extra = state.take_extra();
        state.reset();
        vec![Typed {
            resolved: Resolved::Action(Action::KeyHints),
            extra,
        }]
    }

    /// What `action` does now that its keys are typed: it waits for a character if it takes
    /// one, and else goes with the count, the register, and Ctrl+x typed for it. Ctrl+x waits
    /// for the command, whose count and register come after it: those typed before it go, and
    /// Ctrl+x again gives it up, as the keys would come back where they are.
    fn resolve(state: &mut KeyState, action: Action) -> Option<Typed> {
        if action == Action::Register && state.operator {
            state.drop_command();
            return None;
        }
        if action == Action::OtherPanel {
            if state.other.is_some() {
                state.drop_command();
            } else {
                let keys = std::mem::take(&mut state.keys);
                state.reset();
                state.other = Some(keys);
            }
            return None;
        }
        if action.takes_char() {
            let keys = std::mem::take(&mut state.keys);
            state.clear();
            state.awaiting = Some((action, keys));
            return None;
        }
        let extra = state.take_extra();
        state.reset();
        Some(Typed {
            resolved: Resolved::Action(action),
            extra,
        })
    }

    /// The character typed after `action`, which waited for it; any other key gives the
    /// command up.
    fn argument(state: &mut KeyState, action: Action, event: KeyEvent) -> Vec<Typed> {
        let c = match event.code {
            KeyCode::Tab if action == Action::InsertLiteral => Some('\t'),
            _ => text(event),
        };
        let Some(c) = c else {
            state.drop_command();
            return Vec::new();
        };
        match action {
            // The register goes with the command after it, and the count typed before it too,
            // which a count typed after it multiplies.
            Action::Register => {
                state.register = Some(c);
                if let Some(count) = state.count.take() {
                    let outer = state.outer.unwrap_or(1).saturating_mul(count);
                    state.outer = Some(outer.min(MAX_COUNT));
                }
                Vec::new()
            }
            Action::InsertLiteral => {
                state.reset();
                vec![Typed::from(Resolved::Insert(c))]
            }
            _ => {
                let extra = Extra {
                    char: Some(c),
                    ..state.take_extra()
                };
                state.reset();
                vec![Typed {
                    resolved: Resolved::Action(action),
                    extra,
                }]
            }
        }
    }

    /// Settles a pending sequence whose deadline has passed: it runs its own binding if it has
    /// one, and is dropped otherwise.
    pub(crate) fn expire(&self, state: &mut KeyState, now: Instant) -> Vec<Typed> {
        match state.deadline {
            Some(deadline) if deadline <= now => {
                let typed = self.settle(state);
                if state.keys.is_empty() {
                    state.hinting = false;
                }
                typed
            }
            _ => Vec::new(),
        }
    }

    fn settle(&self, state: &mut KeyState) -> Vec<Typed> {
        let Some(place) = state.place() else {
            state.reset();
            return Vec::new();
        };
        match self.lookup(place, &state.keys) {
            Lookup::Prefix(Some(action)) | Lookup::Exact(action) => {
                Self::resolve(state, action).into_iter().collect()
            }
            Lookup::Prefix(None) | Lookup::Unknown => {
                state.drop_command();
                Vec::new()
            }
        }
    }

    /// The key without Alt, if Alt and `event` stand for Esc and that key, typed in `mode`
    /// where `context` has the keys: an Esc and the key right after it can reach a terminal
    /// program in one read, which crossterm reports as Alt and the key. In Insert and Visual
    /// mode, Alt and a key that nothing binds leave it for Normal mode and do what the key does
    /// there; in the panels, they cancel and do it too. Without the kitty keyboard protocol vim
    /// gets them so; with it, vim would type the key with the eighth bit set, and the keymap
    /// does as nvim does in Insert and Visual mode (ADR 0022).
    /// A command typed so far is given up by the Esc, unless the key goes on with it.
    pub(crate) fn alt_as_esc(
        &self,
        state: &KeyState,
        place: impl Into<Place>,
        event: KeyEvent,
    ) -> Option<KeyEvent> {
        let place = place.into();
        let Place { mode, context, .. } = place;
        let applies = mode != Mode::Normal || context.alt_stands_for_esc();
        if !applies || context.alt_letters_are_hotkeys() {
            return None;
        }
        let key = KeyCombination::from(event);
        let here = state.place() == Some(place);
        if here && !state.keys.is_empty() {
            let mut keys = state.keys.clone();
            keys.push(key);
            if self.lookup(place, &keys) != Lookup::Unknown {
                return None;
            }
        }
        let plain = alt_off(event)?;
        (self.lookup(place, &[key]) == Lookup::Unknown).then_some(plain)
    }

    /// The actions `context` binds itself in `mode`, in the order of the preset, each with its
    /// keys as text such as `g g, Home`, for the help screen, as far as they still do that
    /// along its chain.
    pub(crate) fn help(&self, mode: Mode, context: Context) -> Vec<(Action, String)> {
        let mut rows: Vec<(Action, String)> = Vec::new();
        let own = self
            .bindings(mode, context)
            .into_iter()
            .flat_map(|bindings| &bindings.0);
        for (sequence, action) in own {
            let does = match self.lookup(Place::from((mode, context)), sequence) {
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

    /// The first key sequence along `context`'s chain that does `action` there in Normal mode,
    /// as text such as `g t`, for the pull-down menu.
    pub(crate) fn key(&self, context: Context, action: Action) -> Option<String> {
        self.first_sequence(context, action)
            .map(|sequence| describe(sequence))
    }

    /// The same key sequence as vim's `showcmd` writes it, such as `^X`, for the line of the
    /// mode.
    pub(crate) fn short_key(&self, context: Context, action: Action) -> Option<String> {
        let sequence = self.first_sequence(context, action)?;
        Some(sequence.iter().map(|key| short_key(*key)).collect())
    }

    /// The first key sequence along `context`'s chain that does `action` there in Normal mode.
    fn first_sequence(&self, context: Context, action: Action) -> Option<&Sequence> {
        let place = Place::from((Mode::Normal, context));
        let does = |sequence: &[KeyCombination]| match self.lookup(place, sequence) {
            Lookup::Exact(bound) | Lookup::Prefix(Some(bound)) => bound == action,
            Lookup::Prefix(None) | Lookup::Unknown => false,
        };
        self.chain_bindings(place)
            .find(|(sequence, bound)| *bound == action && does(sequence))
            .map(|(sequence, _)| sequence)
    }

    /// The keys that do `action` in the other panel, Ctrl+x and its key, as text such as
    /// `Ctrl+x g l`, for the pull-down menu.
    pub(crate) fn other_key(&self, context: Context, action: Action) -> Option<String> {
        let other = self.key(context, Action::OtherPanel)?;
        let key = self.key(context, action)?;
        Some(format!("{other} {key}"))
    }

    /// The actions on F1 … F10 in `context`'s chain in `mode`, for the F-key bar.
    pub(crate) fn fkeys(&self, mode: Mode, context: Context) -> [Option<Action>; 10] {
        std::array::from_fn(|index| {
            let number = u8::try_from(index + 1).unwrap_or_else(|_| unreachable!());
            let key = KeyCombination::one_key(KeyCode::F(number), KeyModifiers::NONE);
            match self.lookup(Place::from((mode, context)), &[key]) {
                Lookup::Exact(action) | Lookup::Prefix(Some(action)) => Some(action),
                Lookup::Prefix(None) | Lookup::Unknown => None,
            }
        })
    }

    /// When the hints of the pending sequence are due, `delay` after its last key, or of every
    /// key, after Ctrl+x; `None` if they show already, or for a sequence that does something
    /// alone, which has none. Until then the sequence does not time out.
    pub(crate) fn hints_due(&self, state: &KeyState, delay: Duration) -> Option<Instant> {
        let place = state.place()?;
        let since = state.since?;
        let hinted = !state.hinting
            && if state.keys.is_empty() {
                state.other.is_some()
            } else {
                self.lookup(place, &state.keys) == Lookup::Prefix(None)
            };
        hinted.then_some(since + delay)
    }

    /// Shows the hints where `place` says: of the keys that can follow the pending sequence,
    /// or of every key while none is pending. They show until a key ends the sequence.
    pub(crate) fn show_hints(&self, state: &mut KeyState, place: impl Into<Place>) {
        let place = place.into();
        if state.place() != Some(place) {
            state.reset();
            state.place = Some(place);
        }
        state.hinting = true;
        // A sequence that does something alone still does it when it times out.
        if state.keys.is_empty() || self.lookup(place, &state.keys) == Lookup::Prefix(None) {
            state.deadline = None;
        }
    }

    /// The hints that show where `place` says, if they do.
    pub(crate) fn hints_of(&self, state: &KeyState, place: impl Into<Place>) -> Option<Hints> {
        let place = place.into();
        if !state.hinting || state.place() != Some(place) {
            return None;
        }
        let rows = self.hints(place, &state.keys);
        let typed: Sequence = (state.other.iter().flatten())
            .chain(&state.keys)
            .copied()
            .collect();
        (!rows.is_empty()).then_some(Hints {
            place,
            typed: (!typed.is_empty()).then(|| describe(&typed)),
            prefix: state.keys.clone(),
            rows,
        })
    }

    /// Every sequence longer than `prefix` that starts with it and does something in
    /// `context`'s chain in `mode`, whole, for the key hints with the keys: one row for each
    /// action, in the order of the preset, the context's own first, with its sequences apart by
    /// commas. The hints leave out the same keys as [`hints`](Self::hints).
    pub(crate) fn sequences(
        &self,
        place: impl Into<Place>,
        prefix: &[KeyCombination],
    ) -> Vec<Hint> {
        let place = place.into();
        let Place { mode, context, .. } = place;
        let mut seen: Vec<&Sequence> = Vec::new();
        let mut rows: Vec<Hint> = Vec::new();
        for (sequence, action) in self.chain_bindings(place) {
            if sequence.len() <= prefix.len()
                || !sequence.starts_with(prefix)
                || seen.contains(&sequence)
            {
                continue;
            }
            seen.push(sequence);
            let closes = sequence
                .first()
                .is_some_and(|first| [ESC, CTRL_BRACKET].contains(first));
            let text = matches!(sequence.as_slice(), [key] if mode == Mode::Insert
                && is_character(*key)
                && !self.binds_itself(mode, context, sequence));
            let does = match self.lookup(place, sequence) {
                Lookup::Exact(bound) | Lookup::Prefix(Some(bound)) => bound == *action,
                Lookup::Prefix(None) | Lookup::Unknown => false,
            };
            if closes || text || !does {
                continue;
            }
            let keys = describe(sequence);
            match rows.iter_mut().find(|row| row.action == Some(*action)) {
                Some(row) => {
                    row.keys.push_str(", ");
                    row.keys.push_str(&keys);
                }
                None => rows.push(Hint {
                    keys,
                    action: Some(*action),
                    more: 0,
                }),
            }
        }
        rows
    }

    /// The keys that can follow `prefix` in `context`'s chain in `mode`, in the order of the
    /// preset, the context's own first. Keys that do the same share a row; a key that starts
    /// longer sequences has one of its own. `Esc` and `Ctrl+[`, which close the hints, are left
    /// out, and in Insert mode, so are characters that only a fallback context binds, which are
    /// text.
    pub(crate) fn hints(&self, place: impl Into<Place>, prefix: &[KeyCombination]) -> Vec<Hint> {
        let place = place.into();
        let Place { mode, context, .. } = place;
        let mut next: Vec<KeyCombination> = Vec::new();
        for (sequence, _) in self.chain_bindings(place) {
            if sequence.len() > prefix.len()
                && sequence.starts_with(prefix)
                && sequence
                    .first()
                    .is_none_or(|first| ![ESC, CTRL_BRACKET].contains(first))
                && let Some(key) = sequence.get(prefix.len())
                && !next.contains(key)
            {
                next.push(*key);
            }
        }
        let mut hints: Vec<Hint> = Vec::new();
        for key in next {
            if prefix.is_empty()
                && mode == Mode::Insert
                && is_character(key)
                && !self.binds_itself(mode, context, &[key])
            {
                continue;
            }
            let mut keys = prefix.to_vec();
            keys.push(key);
            let (action, longer) = match self.lookup(place, &keys) {
                Lookup::Exact(action) => (Some(action), 0),
                Lookup::Prefix(action) => (action, self.count_under(place, &keys)),
                Lookup::Unknown => continue,
            };
            let text = describe_key(key);
            match hints
                .iter_mut()
                .find(|hint| hint.more == 0 && longer == 0 && hint.action == action)
            {
                Some(hint) => {
                    hint.keys.push_str(", ");
                    hint.keys.push_str(&text);
                }
                None => hints.push(Hint {
                    keys: text,
                    action,
                    more: longer,
                }),
            }
        }
        hints
    }

    /// The bindings of `place`'s chain that work there, most specific first: a key after
    /// Ctrl+s for a way the field has not still does its nothing, rather than type, but no
    /// hints show it.
    fn chain_bindings(&self, place: Place) -> impl Iterator<Item = &(Sequence, Action)> {
        place
            .chain()
            .filter_map(move |context| self.bindings(place.mode, context))
            .flat_map(|bindings| &bindings.0)
            .filter(move |(_, action)| place.allows(*action))
    }

    /// How many sequences longer than `prefix` that start with it do something in `place`.
    fn count_under(&self, place: Place, prefix: &[KeyCombination]) -> usize {
        let mut counted: Vec<&Sequence> = Vec::new();
        for (sequence, _) in self.chain_bindings(place) {
            let does = matches!(
                self.lookup(place, sequence),
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
/// are typed, and the keys of a sequence apart, as in `g t` or `Z Z`.
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

/// A key as vim's `showcmd` shows it: a character as it is, Ctrl and a letter as `^W`, and
/// another key as the help writes it.
fn short_key(key: KeyCombination) -> String {
    let code = *key.codes.first();
    match code {
        KeyCode::Char(c) if is_character(key) => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                c.to_uppercase().collect()
            } else {
                c.to_string()
            }
        }
        KeyCode::Char(c) if key.modifiers == KeyModifiers::CONTROL && c.is_ascii() => {
            format!("^{}", c.to_ascii_uppercase())
        }
        _ => describe_key(key),
    }
}

const ESC: KeyCombination = KeyCombination::one_key(KeyCode::Esc, KeyModifiers::NONE);
const BACKSPACE: KeyCombination = KeyCombination::one_key(KeyCode::Backspace, KeyModifiers::NONE);
// Where the terminal speaks the kitty keyboard protocol, these no longer arrive as Esc and
// Backspace, but do what they do.
const CTRL_BRACKET: KeyCombination =
    KeyCombination::one_key(KeyCode::Char('['), KeyModifiers::CONTROL);
const CTRL_H: KeyCombination = KeyCombination::one_key(KeyCode::Char('h'), KeyModifiers::CONTROL);
const TAB: KeyCombination = KeyCombination::one_key(KeyCode::Tab, KeyModifiers::NONE);
const CTRL_I: KeyCombination = KeyCombination::one_key(KeyCode::Char('i'), KeyModifiers::CONTROL);

/// The same key without Alt, if `event` has Alt: any key but Esc, Enter and the arrows too, as an
/// Esc and a quick key arrive without the kitty keyboard protocol.
fn alt_off(event: KeyEvent) -> Option<KeyEvent> {
    (event.modifiers.contains(KeyModifiers::ALT) && event.code != KeyCode::Esc)
        .then(|| KeyEvent::new(event.code, event.modifiers - KeyModifiers::ALT))
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

/// Parses a space-separated key sequence such as `ctrl-w h`, with `crokey` key names.
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

/// The bindings of a context as a terminal without the kitty keyboard protocol sends their keys:
/// Ctrl+i arrives as Tab, so Tab does what Ctrl+i does, right before it in the help, and nothing
/// where Ctrl+i does nothing. Ctrl+i keeps its own bindings, for a terminal that reports it
/// apart without having said so.
fn tab_as_ctrl_i(bound: Vec<(Sequence, Action)>) -> Vec<(Sequence, Action)> {
    let mut legacy = Vec::with_capacity(bound.len());
    for (sequence, action) in bound {
        if sequence.contains(&TAB) {
            continue;
        }
        if sequence.contains(&CTRL_I) {
            let tab = sequence
                .iter()
                .map(|key| if *key == CTRL_I { TAB } else { *key })
                .collect();
            legacy.push((tab, action));
        }
        legacy.push((sequence, action));
    }
    legacy
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

/// The digit a key types, if it types one.
fn digit(event: KeyEvent) -> Option<usize> {
    let c = text(event)?;
    c.to_digit(10).and_then(|digit| usize::try_from(digit).ok())
}

#[cfg(test)]
mod tests;
