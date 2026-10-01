//! Key bindings: key sequences mapped to actions per context, with the mc preset.
//!
//! A binding is a sequence of one or more key combinations, such as `f10` or `esc 0`. While the
//! keys typed so far are the start of a longer binding, the keymap waits up to
//! [`SEQUENCE_TIMEOUT`] for the next key. As in mc, a pending `Esc` followed by a character
//! stands for Alt and that character, for terminals whose Alt key sends nothing (by default,
//! Option on macOS).

mod action;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crokey::KeyCombination;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) use action::{Action, Context};

/// How long a sequence such as `Esc 1` waits for its next key.
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

/// Preset bindings of one context: each action with its key sequences.
type Preset = &'static [(Action, &'static [&'static str])];

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
}

impl KeyState {
    /// When the pending sequence times out; `None` if no sequence is pending.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    fn clear(&mut self) {
        self.keys.clear();
        self.deadline = None;
    }
}

impl Keymap {
    /// The default preset, modelled on Midnight Commander. `Esc` followed by a digit stands for
    /// the F-key, for terminals that lack them.
    pub(crate) fn mc() -> Self {
        use Action::{
            Backspace, Cancel, Confirm, Delete, DeleteToEnd, DeleteToStart, Disconnect, Down, End,
            Enter, Help, Home, Left, NextField, OtherPanelOpen, OtherPanelSync, PageDown, PageUp,
            Parent, PrevField, QuickSearch, Quit, Redraw, Reload, Right, SortByExtension,
            SortByName, SortBySize, SortByTime, SwapPanels, SwitchPanel, ToggleHidden, Up,
        };
        let presets: [(Context, Preset); 5] = [
            (
                Context::Panel,
                &[
                    (Up, &["up", "ctrl-p"]),
                    (Down, &["down", "ctrl-n"]),
                    (PageUp, &["pageup", "alt-v"]),
                    (PageDown, &["pagedown", "ctrl-v"]),
                    (Home, &["home"]),
                    (End, &["end"]),
                    (Enter, &["enter"]),
                    (Parent, &["ctrl-pageup"]),
                    (SwitchPanel, &["tab"]),
                    (SwapPanels, &["ctrl-u"]),
                    (OtherPanelOpen, &["alt-o"]),
                    (OtherPanelSync, &["alt-i"]),
                    (Reload, &["ctrl-r"]),
                    (Cancel, &["esc", "esc esc"]),
                    (ToggleHidden, &["alt-."]),
                    // mc leaves sorting to its menu; these are Far Manager's keys. macOS takes
                    // them for keyboard navigation unless those shortcuts are turned off.
                    (SortByName, &["ctrl-f3"]),
                    (SortByExtension, &["ctrl-f4"]),
                    (SortByTime, &["ctrl-f5"]),
                    (SortBySize, &["ctrl-f6"]),
                    (QuickSearch, &["ctrl-s", "alt-s"]),
                    (Help, &["f1"]),
                    (Quit, &["f10"]),
                    (Redraw, &["ctrl-l"]),
                ],
            ),
            (Context::Root, &[(Disconnect, &["f8"])]),
            (
                Context::QuickSearch,
                &[(Backspace, &["backspace"]), (Cancel, &["esc"])],
            ),
            (
                Context::Dialog,
                &[
                    (Up, &["up"]),
                    (Down, &["down"]),
                    (Left, &["left"]),
                    (Right, &["right"]),
                    (PageUp, &["pageup"]),
                    (PageDown, &["pagedown"]),
                    (Home, &["home"]),
                    (End, &["end"]),
                    (NextField, &["tab"]),
                    (PrevField, &["backtab"]),
                    (Confirm, &["enter"]),
                    (Cancel, &["esc", "f10"]),
                ],
            ),
            (
                Context::DialogInput,
                &[
                    (Home, &["home", "ctrl-a"]),
                    (End, &["end", "ctrl-e"]),
                    (Backspace, &["backspace"]),
                    (Delete, &["delete"]),
                    (DeleteToStart, &["ctrl-u"]),
                    (DeleteToEnd, &["ctrl-k"]),
                ],
            ),
        ];
        let mut contexts = HashMap::new();
        for (context, bindings) in presets {
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
            let bindings = if context.esc_waits() {
                with_esc_digits(bindings)
            } else {
                bindings
            };
            contexts.insert(context, Bindings(bindings));
        }
        Self { contexts }
    }

    /// What the first context in `context`'s chain that knows `keys` knows about them. A prefix
    /// that this context does not bind itself does, once it times out, what a later context
    /// binds it to: `Esc` still cancels where a context adds `Esc 8`.
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
    /// key. A change of `context` forgets a pending sequence.
    pub(crate) fn feed(
        &self,
        state: &mut KeyState,
        context: Context,
        event: KeyEvent,
        now: Instant,
    ) -> Vec<Resolved> {
        if state.context != Some(context) {
            state.clear();
            state.context = Some(context);
        }
        // An Esc and the key right after it reach a terminal program in one read, which
        // crossterm reports as Alt and the key; so do terminals whose Alt key sends Esc.
        if context.esc_waits()
            && state.keys.is_empty()
            && let Some(plain) = without_alt(event)
            && self.lookup(context, &[KeyCombination::from(event)]) == Lookup::Unknown
        {
            let mut resolved = self.feed(state, context, KeyEvent::from(KeyCode::Esc), now);
            resolved.extend(self.feed(state, context, plain, now));
            return resolved;
        }
        let mut resolved = Vec::new();
        let mut alt_from_esc = false;
        state.keys.push(KeyCombination::from(event));
        loop {
            match self.lookup(context, &state.keys) {
                Lookup::Exact(action) => {
                    resolved.push(Resolved::Action(action));
                    state.clear();
                }
                Lookup::Prefix(_) => state.deadline = Some(now + SEQUENCE_TIMEOUT),
                Lookup::Unknown if state.keys.len() == 1 => {
                    if let Some(c) = text(event).filter(|_| context.accepts_text() && !alt_from_esc)
                    {
                        resolved.push(Resolved::Insert(c));
                    }
                    state.clear();
                }
                Lookup::Unknown => {
                    let key = state.keys.pop().unwrap_or_else(|| unreachable!());
                    if state.keys == [ESC] && is_character(key) {
                        state.keys = vec![KeyCombination {
                            modifiers: key.modifiers | KeyModifiers::ALT,
                            ..key
                        }];
                        alt_from_esc = true;
                        continue;
                    }
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
    /// text such as `Ctrl-r, Alt-s`, for the help screen. The `Esc 1` … `Esc 0` aliases are
    /// left out; the help explains them once.
    pub(crate) fn help(&self, context: Context) -> Vec<(Action, String)> {
        let format = crokey::KeyCombinationFormat::default();
        let mut rows: Vec<(Action, String)> = Vec::new();
        let Some(bindings) = self.contexts.get(&context) else {
            return rows;
        };
        for (sequence, action) in &bindings.0 {
            if let [first, digit] = sequence.as_slice()
                && *first == ESC
                && matches!(digit.codes.first(), KeyCode::Char('0'..='9'))
            {
                continue;
            }
            let keys = sequence
                .iter()
                .map(|key| format.to_string(*key))
                .collect::<Vec<_>>()
                .join(" ");
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
}

const ESC: KeyCombination = KeyCombination::one_key(KeyCode::Esc, KeyModifiers::NONE);

/// Adds `Esc 1` … `Esc 0` next to every binding of F1 … F10, unless that sequence is bound.
fn with_esc_digits(mut bindings: Vec<(Sequence, Action)>) -> Vec<(Sequence, Action)> {
    let aliases: Vec<(Sequence, Action)> = bindings
        .iter()
        .filter_map(|(sequence, action)| match sequence.as_slice() {
            [key] if key.modifiers.is_empty() => match *key.codes.first() {
                KeyCode::F(number @ 1..=10) => {
                    let digit = char::from_digit(u32::from(number % 10), 10)?;
                    let digit = KeyCombination::one_key(KeyCode::Char(digit), KeyModifiers::NONE);
                    Some((vec![ESC, digit], *action))
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

/// Whether `key` is a character without Ctrl or Alt, which a pending `Esc` turns into Alt.
fn is_character(key: KeyCombination) -> bool {
    key.is_ansi_compatible()
        && matches!(key.codes.first(), KeyCode::Char(_))
        && (key.modifiers - KeyModifiers::SHIFT).is_empty()
}

/// Parses a space-separated key sequence such as `esc 0`, with `crokey` key names.
fn parse_sequence(text: &str) -> Option<Sequence> {
    let sequence: Sequence = text
        .split_whitespace()
        .map(|key| crokey::parse(key).ok().map(KeyCombination::normalized))
        .collect::<Option<_>>()?;
    (!sequence.is_empty()).then_some(sequence)
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
    fn esc_digit_stands_for_the_f_key() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(feed(&keymap, &mut state, Context::Panel, &["esc"]), []);
        assert!(state.deadline().is_some(), "waits for the next key");
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["0"]),
            actions(&[Action::Quit])
        );
        assert_eq!(state.deadline(), None);
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["esc", "1"]),
            actions(&[Action::Help])
        );
    }

    #[test]
    fn a_pending_sequence_expires() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        let start = Instant::now();
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("esc"), start),
            []
        );
        assert_eq!(state.deadline(), Some(start + SEQUENCE_TIMEOUT));
        assert_eq!(keymap.expire(&mut state, start), [], "not yet");
        // A lone Esc cancels once nothing follows, as in mc.
        assert_eq!(
            keymap.expire(&mut state, start + SEQUENCE_TIMEOUT),
            actions(&[Action::Cancel])
        );
        assert_eq!(state.deadline(), None);
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, key("0"), start),
            [Resolved::Insert('0')],
            "a later digit is just a digit"
        );
    }

    #[test]
    fn esc_and_a_character_stand_for_alt() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["esc", "."]),
            actions(&[Action::ToggleHidden])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["alt-."]),
            actions(&[Action::ToggleHidden])
        );
        // An unbound Alt combination types nothing.
        assert_eq!(feed(&keymap, &mut state, Context::Panel, &["esc", "z"]), []);
        assert_eq!(feed(&keymap, &mut state, Context::Panel, &["alt-z"]), []);
        assert_eq!(state.deadline(), None);
    }

    #[test]
    fn alt_and_a_digit_stand_for_esc_and_the_digit() {
        // What `Esc 0` typed quickly, or Alt-0 where Alt sends Esc, arrives as.
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["alt-0", "alt-1"]),
            actions(&[Action::Quit, Action::Help])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Root, &["alt-8"]),
            actions(&[Action::Disconnect])
        );
        assert_eq!(state.deadline(), None);
        // Bound Alt keys keep their binding, and dialogs keep Alt as it is.
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["alt-."]),
            actions(&[Action::ToggleHidden])
        );
        assert_eq!(feed(&keymap, &mut state, Context::Dialog, &["alt-0"]), []);
    }

    #[test]
    fn a_broken_sequence_lets_the_new_key_through() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        // The pending Esc settles as if it had timed out, then the new key runs.
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["esc", "tab"]),
            actions(&[Action::Cancel, Action::SwitchPanel])
        );
        // The new key may start a sequence of its own.
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["esc", "f10", "esc"]),
            actions(&[Action::Cancel, Action::Quit])
        );
        assert!(state.deadline().is_some());
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["0"]),
            actions(&[Action::Quit])
        );
        // Esc Esc cancels without waiting.
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["esc", "esc"]),
            actions(&[Action::Cancel])
        );
        assert_eq!(state.deadline(), None);
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
    fn the_root_adds_disconnect_to_the_panel_keys() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Root,
                &["f8", "esc", "8", "esc", "0", "enter"]
            ),
            actions(&[
                Action::Disconnect,
                Action::Disconnect,
                Action::Quit,
                Action::Enter
            ])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["f8"]),
            [],
            "only the root disconnects"
        );
        // `Esc 8` makes `Esc` a prefix in the root too, but alone it still cancels.
        let start = Instant::now();
        assert_eq!(
            keymap.feed(&mut state, Context::Root, key("esc"), start),
            []
        );
        assert_eq!(
            keymap.expire(&mut state, start + SEQUENCE_TIMEOUT),
            actions(&[Action::Cancel])
        );
        let mut root = [None; 10];
        root[0] = Some(Action::Help);
        root[7] = Some(Action::Disconnect);
        root[9] = Some(Action::Quit);
        assert_eq!(keymap.fkeys(Context::Root), root);
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
    fn a_new_context_forgets_a_pending_sequence() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(feed(&keymap, &mut state, Context::Panel, &["esc"]), []);
        assert_eq!(
            feed(&keymap, &mut state, Context::DialogInput, &["0"]),
            [Resolved::Insert('0')]
        );
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
        panel[9] = Some(Action::Quit);
        assert_eq!(keymap.fkeys(Context::Panel), panel);
        assert_eq!(keymap.fkeys(Context::QuickSearch), panel);
        let mut dialog = [None; 10];
        dialog[9] = Some(Action::Cancel);
        assert_eq!(keymap.fkeys(Context::Dialog), dialog);
    }

    #[test]
    fn the_preset_covers_every_action_and_has_no_conflicts() {
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
        for (context, bindings) in &keymap.contexts {
            for (index, (sequence, _)) in bindings.0.iter().enumerate() {
                assert!(
                    !bindings.0[..index]
                        .iter()
                        .any(|(other, _)| other == sequence),
                    "{sequence:?} is bound twice in {context:?}"
                );
            }
        }
    }

    #[test]
    fn every_f_key_has_an_esc_digit_alias_where_esc_waits() {
        let keymap = Keymap::mc();
        for (context, bindings) in &keymap.contexts {
            if !context.esc_waits() {
                continue;
            }
            for (sequence, action) in &bindings.0 {
                if let [key] = sequence.as_slice()
                    && key.modifiers.is_empty()
                    && let KeyCode::F(number @ 1..=10) = *key.codes.first()
                {
                    let alias = parse_sequence(&format!("esc {}", number % 10)).unwrap();
                    assert!(
                        bindings.0.contains(&(alias, *action)),
                        "F{number} in {context:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn help_lists_keys_by_action_without_esc_digit_aliases() {
        let keymap = Keymap::mc();
        let panel = keymap.help(Context::Panel);
        let keys = |action| {
            panel
                .iter()
                .find(|(bound, _)| *bound == action)
                .map(|(_, keys)| keys.as_str())
        };
        assert_eq!(keys(Action::Up), Some("Up, Ctrl-p"));
        assert_eq!(keys(Action::Quit), Some("F10"));
        assert_eq!(keys(Action::Cancel), Some("Esc, Esc Esc"));
        assert_eq!(keys(Action::SortByName), Some("Ctrl-F3"));
        assert_eq!(panel[0].0, Action::Up, "in the order of the preset");
        assert_eq!(
            keymap.help(Context::Root),
            [(Action::Disconnect, "F8".to_owned())]
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
