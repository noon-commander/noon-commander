//! Key bindings: key sequences mapped to actions per context, with the built-in presets.
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
    /// The names of the built-in presets, for `ui.keymap`.
    pub(crate) const NAMES: &'static [&'static str] = &["default", "vim"];

    /// A built-in preset by name.
    pub(crate) fn by_name(name: &str) -> Option<Self> {
        match name {
            "default" => Some(Self::mc()),
            "vim" => Some(Self::vim()),
            _ => None,
        }
    }

    /// The default preset, modelled on Midnight Commander. `Esc` followed by a digit stands for
    /// the F-key, for terminals that lack them.
    pub(crate) fn mc() -> Self {
        Self::of(&mc_presets())
    }

    /// The vim preset: panels, dialogs, and the viewer of its own, and the mc preset's other
    /// contexts.
    fn vim() -> Self {
        let presets = mc_presets().map(|(context, bindings)| match context {
            Context::Panel => (context, VIM_PANEL),
            Context::Dialog => (context, VIM_DIALOG),
            Context::Viewer => (context, VIM_VIEWER),
            _ => (context, bindings),
        });
        Self::of(&presets)
    }

    fn of(presets: &[(Context, Preset)]) -> Self {
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
            contexts.insert(*context, Bindings(bindings));
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
    /// text such as `Ctrl+r, Alt+s`, for the help screen. The `Esc 1` … `Esc 0` aliases are
    /// left out; the help explains them once.
    pub(crate) fn help(&self, context: Context) -> Vec<(Action, String)> {
        let mut rows: Vec<(Action, String)> = Vec::new();
        let Some(bindings) = self.contexts.get(&context) else {
            return rows;
        };
        for (sequence, action) in &bindings.0 {
            if is_esc_digit(sequence) {
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
    /// `Ctrl+F3`, for the pull-down menu. `Esc 1` … `Esc 0` count only where nothing else does.
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
            sequences.partition(|sequence| is_esc_digit(sequence));
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
}

/// The bindings of the mc preset, by context.
fn mc_presets() -> [(Context, Preset); 16] {
    [
        (Context::Panel, PANEL),
        (
            Context::Root,
            &[(Action::EditHost, &["f4"]), (Action::Disconnect, &["f8"])],
        ),
        (
            Context::QuickSearch,
            &[
                (Action::Backspace, &["backspace"]),
                (Action::Cancel, &["esc"]),
            ],
        ),
        (Context::Rename, RENAME),
        (
            Context::Dialog,
            &[
                (Action::Up, &["up"]),
                (Action::Down, &["down"]),
                (Action::Left, &["left"]),
                (Action::Right, &["right"]),
                (Action::PageUp, &["pageup"]),
                (Action::PageDown, &["pagedown"]),
                (Action::Home, &["home"]),
                (Action::End, &["end"]),
                (Action::NextField, &["tab"]),
                (Action::PrevField, &["backtab"]),
                (Action::Confirm, &["enter"]),
                (Action::Toggle, &["space"]),
                (Action::Cancel, &["esc", "f10"]),
            ],
        ),
        (
            Context::Viewer,
            &[
                (Action::Up, &["up", "k", "y", "ctrl-p"]),
                (Action::Down, &["down", "j", "e", "enter", "ctrl-n"]),
                (Action::PageUp, &["pageup", "b", "alt-v", "backspace"]),
                (Action::PageDown, &["pagedown", "space", "f", "ctrl-v"]),
                (Action::Home, &["home", "g", "ctrl-home"]),
                (Action::End, &["end", "shift-g", "ctrl-end"]),
                (Action::Left, &["left", "h"]),
                (Action::Right, &["right", "l"]),
                (Action::ToggleWrap, &["f2"]),
                (Action::Help, &["f1"]),
                (Action::Quit, &["f3", "f10", "q", "esc"]),
                (Action::Redraw, &["ctrl-l"]),
            ],
        ),
        (Context::Menu, MENU),
        (Context::Jump, JUMP),
        (Context::Workspaces, WORKSPACES),
        (Context::PullDown, PULL_DOWN),
        (Context::DialogInput, TEXT_FIELD),
        (Context::PathInput, PATH_FIELD),
        (Context::Completion, COMPLETION),
        (Context::CommandLine, COMMAND_LINE),
        (Context::History, HISTORY),
        (Context::UserScreen, &[(Action::Cancel, &["ctrl-o", "esc"])]),
    ]
}

/// The panels' bindings in the mc preset.
const PANEL: Preset = &[
    (Action::Up, &["up", "ctrl-p"]),
    (Action::Down, &["down", "ctrl-n"]),
    (Action::PageUp, &["pageup", "alt-v"]),
    (Action::PageDown, &["pagedown", "ctrl-v"]),
    (Action::Home, &["home"]),
    (Action::End, &["end"]),
    (Action::Enter, &["enter"]),
    (Action::Mark, &["insert", "ctrl-t", "shift-down"]),
    (Action::MarkUp, &["shift-up"]),
    // mc takes `+`, `-`, `\`, and `*` as commands while its command line is empty.
    (Action::Select, &["+", "alt-+"]),
    (Action::Unselect, &["-", "\\", "alt--"]),
    (Action::InvertMarks, &["*", "alt-*"]),
    (Action::Parent, &["ctrl-pageup"]),
    (Action::SwitchPanel, &["tab"]),
    (Action::SwapPanels, &["ctrl-u"]),
    (Action::OtherPanelOpen, &["alt-o"]),
    (Action::OtherPanelSync, &["alt-i"]),
    (Action::Reload, &["ctrl-r"]),
    (Action::Cancel, &["esc", "esc esc"]),
    (Action::ToggleHidden, &["alt-."]),
    // mc leaves sorting to its menu; these are Far Manager's keys. macOS takes them for
    // keyboard navigation unless those shortcuts are turned off.
    (Action::SortByName, &["ctrl-f3"]),
    (Action::SortByExtension, &["ctrl-f4"]),
    (Action::SortByTime, &["ctrl-f5"]),
    (Action::SortBySize, &["ctrl-f6"]),
    (Action::QuickSearch, &["ctrl-s", "alt-s"]),
    // Not in mc, whose command line takes what is typed (ADR 0019).
    (Action::Shell, &["!"]),
    (Action::Command, &[":"]),
    // mc's: the command line, with its history.
    (Action::CommandHistory, &["alt-h"]),
    // mc's and Far's: the output of commands.
    (Action::UserScreen, &["ctrl-o"]),
    (Action::Help, &["f1"]),
    (Action::View, &["f3"]),
    (Action::Edit, &["f4"]),
    (Action::Copy, &["f5"]),
    (Action::Move, &["f6"]),
    // mc's Shift+F6 asks for a new name in a dialog; here the name is edited in its row.
    // Terminals without Shift+F6 send F16.
    (Action::Rename, &["shift-f6", "f16"]),
    (Action::Mkdir, &["f7"]),
    // In text fields, Delete deletes a character.
    (Action::Delete, &["f8", "delete"]),
    (Action::Jobs, &["ctrl-x j"]),
    // Not in mc; `#` for a hash.
    (Action::Checksum, &["ctrl-x #"]),
    // Far Manager's menus to change drives. Ctrl+x 1 and 2 are for terminals whose Alt+F1
    // never arrives, such as macOS Terminal without Option as Meta.
    (Action::LocationMenuLeft, &["alt-f1", "ctrl-x 1"]),
    (Action::LocationMenuRight, &["alt-f2", "ctrl-x 2"]),
    // Not in mc: zoxide's `z`. Ctrl+x z where Alt never arrives.
    (Action::Jump, &["alt-z", "ctrl-x z"]),
    // mc's Quick cd; Esc c where Alt never arrives.
    (Action::QuickCd, &["alt-c"]),
    // Not in mc. Ctrl+t marks and terminals rarely pass Ctrl+Tab, so tabs live under
    // Ctrl+x; Alt+Left and Alt+Right where the terminal sends them.
    (Action::NewTab, &["ctrl-x t"]),
    (Action::CloseTab, &["ctrl-x w"]),
    (Action::NextTab, &["alt-right", "ctrl-x n"]),
    (Action::PrevTab, &["alt-left", "ctrl-x p"]),
    (Action::TabList, &["ctrl-x tab"]),
    // Not in mc: W for workspaces, and Shift saves the tabs of both panels as one. Esc w
    // and Esc W where Alt never arrives.
    (Action::Workspaces, &["alt-w"]),
    (Action::SaveWorkspace, &["alt-shift-w"]),
    (Action::PullDown, &["f9"]),
    (Action::Quit, &["f10"]),
    (Action::Redraw, &["ctrl-l"]),
];

/// The panels' bindings in the vim preset.
const VIM_PANEL: Preset = &[
    (Action::Shell, &["!"]),
    (Action::Command, &[":"]),
    (Action::PullDown, &["f9"]),
    (Action::Help, &["g ?", "f1"]),
    (Action::Up, &["k", "up"]),
    (Action::Down, &["j", "down"]),
    (Action::Home, &["g g", "home"]),
    (Action::End, &["shift-g", "end"]),
    (Action::Enter, &["l", "enter"]),
    // netrw's and vinegar's `-`, next to `h`.
    (Action::Parent, &["h", "-"]),
    (Action::Quit, &["shift-z shift-z", "f10"]),
    (Action::Redraw, &["ctrl-l"]),
];

/// The bindings of dialogs in the vim preset.
const VIM_DIALOG: Preset = &[
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::Left, &["left"]),
    (Action::Right, &["right"]),
    (Action::PageUp, &["pageup"]),
    (Action::PageDown, &["pagedown"]),
    (Action::Home, &["home"]),
    (Action::End, &["end"]),
    (Action::NextField, &["tab"]),
    (Action::PrevField, &["backtab"]),
    (Action::Confirm, &["enter"]),
    (Action::Toggle, &["space"]),
    (Action::Cancel, &["esc", "ctrl-c", "f10"]),
];

/// The viewer's bindings in the vim preset.
const VIM_VIEWER: Preset = &[
    (Action::Up, &["k", "ctrl-p", "up"]),
    (Action::Down, &["j", "ctrl-n", "down", "enter"]),
    (Action::PageUp, &["ctrl-b", "pageup"]),
    (Action::PageDown, &["ctrl-f", "pagedown"]),
    (Action::Home, &["g g", "home"]),
    (Action::End, &["shift-g", "end"]),
    (Action::Left, &["h", "left"]),
    (Action::Right, &["l", "right"]),
    (Action::ToggleWrap, &["f2"]),
    (Action::Help, &["g ?", "f1"]),
    (Action::Quit, &["f3", "f10", "q", "esc"]),
    (Action::Redraw, &["ctrl-l"]),
];

/// The pull-down menu's bindings in the mc preset; letters run the commands that have them.
const PULL_DOWN: Preset = &[
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::Left, &["left"]),
    (Action::Right, &["right"]),
    (Action::Home, &["home", "pageup"]),
    (Action::End, &["end", "pagedown"]),
    (Action::Confirm, &["enter"]),
    (Action::Cancel, &["esc", "f9", "f10"]),
];

/// The bindings of text fields in the mc preset, on top of the dialog's.
const TEXT_FIELD: Preset = &[
    (Action::Home, &["home", "ctrl-a"]),
    (Action::End, &["end", "ctrl-e"]),
    (Action::Backspace, &["backspace"]),
    (Action::Delete, &["delete"]),
    (Action::DeleteToStart, &["ctrl-u"]),
    (Action::DeleteToEnd, &["ctrl-k"]),
];

/// The bindings of the name field of an entry renamed in its row, in the mc preset.
const RENAME: Preset = &[
    (Action::Left, &["left"]),
    (Action::Right, &["right"]),
    (Action::Home, &["home", "ctrl-a"]),
    (Action::End, &["end", "ctrl-e"]),
    (Action::Backspace, &["backspace"]),
    (Action::Delete, &["delete"]),
    (Action::DeleteToStart, &["ctrl-u"]),
    (Action::DeleteToEnd, &["ctrl-k"]),
    (Action::Confirm, &["enter"]),
    (Action::Cancel, &["esc"]),
];

/// The bindings of the command line of `!` and `:` in the mc preset. Ctrl+j arrives as LF where
/// Enter arrives as CR, so it starts a new line in every terminal; Shift+Enter does where the
/// terminal speaks the kitty keyboard protocol. Ctrl+x Ctrl+e edits the command, as in bash.
const COMMAND_LINE: Preset = &[
    (Action::Left, &["left"]),
    (Action::Right, &["right"]),
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::Home, &["home", "ctrl-a"]),
    (Action::End, &["end", "ctrl-e"]),
    (Action::Backspace, &["backspace"]),
    (Action::Delete, &["delete"]),
    (Action::DeleteToStart, &["ctrl-u"]),
    (Action::DeleteToEnd, &["ctrl-k"]),
    (Action::NewLine, &["ctrl-j", "shift-enter"]),
    (Action::EditCommand, &["ctrl-x ctrl-e"]),
    (Action::OlderCommand, &["alt-p"]),
    (Action::NewerCommand, &["alt-n"]),
    // mc's Alt+h, and bash's Ctrl+r.
    (Action::CommandHistory, &["alt-h", "ctrl-r"]),
    (Action::UserScreen, &["ctrl-o"]),
    (Action::Confirm, &["enter"]),
    (Action::Cancel, &["esc"]),
];

/// The bindings of the window of the command history in the mc preset; characters filter it,
/// and Tab switches between the panel's host and all hosts.
const HISTORY: Preset = &[
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::PageUp, &["pageup"]),
    (Action::PageDown, &["pagedown"]),
    (Action::Home, &["home"]),
    (Action::End, &["end"]),
    (Action::NextField, &["tab"]),
    (Action::Confirm, &["enter"]),
    (Action::Backspace, &["backspace"]),
    (Action::Delete, &["delete"]),
    (Action::Cancel, &["esc", "f10"]),
];

/// The bindings of path fields in the mc preset, on top of the text field's: Tab completes, as
/// in a shell; Shift+Tab and Down still leave the field.
const PATH_FIELD: Preset = &[(Action::Complete, &["tab"])];

/// The bindings of the list of completions in the mc preset; other keys close it and go to the
/// field.
const COMPLETION: Preset = &[
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::PageUp, &["pageup"]),
    (Action::PageDown, &["pagedown"]),
    (Action::Home, &["home"]),
    (Action::End, &["end"]),
    (Action::Complete, &["tab"]),
    (Action::Confirm, &["enter"]),
    (Action::Cancel, &["esc"]),
];

/// The location menu's bindings in the mc preset; characters filter it.
const MENU: Preset = &[
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::PageUp, &["pageup"]),
    (Action::PageDown, &["pagedown"]),
    (Action::Home, &["home"]),
    (Action::End, &["end"]),
    (Action::Confirm, &["enter"]),
    (Action::Backspace, &["backspace"]),
    (Action::Disconnect, &["f8"]),
    (Action::Reload, &["ctrl-r"]),
    (Action::Cancel, &["esc", "f10"]),
];

/// The zoxide window's bindings in the mc preset; characters are keywords.
const JUMP: Preset = &[
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::PageUp, &["pageup"]),
    (Action::PageDown, &["pagedown"]),
    (Action::Home, &["home"]),
    (Action::End, &["end"]),
    (Action::Confirm, &["enter"]),
    (Action::Backspace, &["backspace"]),
    (Action::Cancel, &["esc", "f10"]),
];

/// The bindings of the window of the saved workspaces in the mc preset; characters filter it.
/// Insert adds one, as in Far's menus; F6 renames and F8 deletes, as they rename and delete
/// files.
const WORKSPACES: Preset = &[
    (Action::SaveWorkspace, &["insert"]),
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::PageUp, &["pageup"]),
    (Action::PageDown, &["pagedown"]),
    (Action::Home, &["home"]),
    (Action::End, &["end"]),
    (Action::Confirm, &["enter"]),
    (Action::Backspace, &["backspace"]),
    (Action::Move, &["f6"]),
    (Action::Delete, &["f8", "delete"]),
    (Action::Cancel, &["esc", "f10"]),
];

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

/// Whether `sequence` is `Esc` and a digit, which stands for an F-key.
fn is_esc_digit(sequence: &[KeyCombination]) -> bool {
    matches!(sequence, [first, digit]
        if *first == ESC && matches!(digit.codes.first(), KeyCode::Char('0'..='9')))
}

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
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["esc", "z"]),
            actions(&[Action::Jump])
        );
        // An unbound Alt combination types nothing.
        assert_eq!(feed(&keymap, &mut state, Context::Panel, &["esc", "q"]), []);
        assert_eq!(feed(&keymap, &mut state, Context::Panel, &["alt-q"]), []);
        assert_eq!(state.deadline(), None);
    }

    #[test]
    fn alt_and_a_digit_stand_for_esc_and_the_digit() {
        // What `Esc 0` typed quickly, or Alt+0 where Alt sends Esc, arrives as.
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
            feed(&keymap, &mut state, Context::Panel, &["esc", "*"]),
            actions(&[Action::InvertMarks]),
            "Esc * is Alt+*"
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &["+", "-", "\\", "esc", "-"]
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
    fn the_root_adds_host_keys_to_the_panel_keys() {
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
            actions(&[Action::Delete]),
            "only the root disconnects; panels delete"
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
        root[2] = Some(Action::View);
        root[3] = Some(Action::EditHost);
        root[4] = Some(Action::Copy);
        root[5] = Some(Action::Move);
        root[6] = Some(Action::Mkdir);
        root[7] = Some(Action::Disconnect);
        root[8] = Some(Action::PullDown);
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
    fn ctrl_x_hash_computes_checksums() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["ctrl-x", "#"]),
            actions(&[Action::Checksum])
        );
    }

    #[test]
    fn tabs_live_under_ctrl_x_and_alt_arrows() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &[
                    "ctrl-x",
                    "t",
                    "ctrl-x",
                    "w",
                    "ctrl-x",
                    "n",
                    "ctrl-x",
                    "p",
                    "ctrl-x",
                    "tab",
                    "alt-right",
                    "alt-left"
                ]
            ),
            actions(&[
                Action::NewTab,
                Action::CloseTab,
                Action::NextTab,
                Action::PrevTab,
                Action::TabList,
                Action::NextTab,
                Action::PrevTab
            ])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Root, &["ctrl-x", "t"]),
            actions(&[Action::NewTab]),
            "in the root too"
        );
    }

    #[test]
    fn alt_w_lists_workspaces_alt_shift_w_saves_and_the_window_takes_insert_f6_and_f8() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Panel,
                &["alt-w", "esc", "w", "alt-shift-w", "esc", "shift-w"]
            ),
            actions(&[
                Action::Workspaces,
                Action::Workspaces,
                Action::SaveWorkspace,
                Action::SaveWorkspace
            ])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Root, &["alt-w"]),
            actions(&[Action::Workspaces])
        );
        // Some terminals send an uppercase letter with Alt but without Shift.
        let upper = KeyEvent::new(KeyCode::Char('W'), KeyModifiers::ALT);
        assert_eq!(
            keymap.feed(&mut state, Context::Panel, upper, Instant::now()),
            actions(&[Action::SaveWorkspace])
        );
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
        assert_eq!(
            feed(&keymap, &mut state, Context::Root, &["alt-f2"]),
            actions(&[Action::LocationMenuRight])
        );
        // In the menu, every character filters or is a hotkey; keys move and act.
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::Menu,
                &["1", "p", "+", "space", "down", "enter", "f8", "esc"]
            ),
            [
                Resolved::Insert('1'),
                Resolved::Insert('p'),
                Resolved::Insert('+'),
                Resolved::Insert(' '),
                Resolved::Action(Action::Down),
                Resolved::Action(Action::Confirm),
                Resolved::Action(Action::Disconnect),
                Resolved::Action(Action::Cancel),
            ]
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Menu, &["tab", "ctrl-u", "f5"]),
            [],
            "the menu is modal"
        );
        let mut menu = [None; 10];
        menu[7] = Some(Action::Disconnect);
        menu[9] = Some(Action::Cancel);
        assert_eq!(keymap.fkeys(Context::Menu), menu);
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
            feed(&keymap, &mut state, Context::Root, &["alt-z"]),
            actions(&[Action::Jump])
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
    fn quick_cd_opens_with_alt_c_or_esc_c() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["alt-c", "esc", "c"]),
            actions(&[Action::QuickCd, Action::QuickCd])
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::Root, &["alt-c"]),
            actions(&[Action::QuickCd])
        );
    }

    #[test]
    fn the_pull_down_menu_opens_with_f9_and_takes_letters() {
        let keymap = Keymap::mc();
        let mut state = KeyState::default();
        assert_eq!(
            feed(&keymap, &mut state, Context::Panel, &["f9", "esc", "9"]),
            actions(&[Action::PullDown, Action::PullDown])
        );
        assert_eq!(
            feed(
                &keymap,
                &mut state,
                Context::PullDown,
                &["v", "+", "left", "pagedown", "enter", "f9", "esc"]
            ),
            [
                Resolved::Insert('v'),
                Resolved::Insert('+'),
                Resolved::Action(Action::Left),
                Resolved::Action(Action::End),
                Resolved::Action(Action::Confirm),
                Resolved::Action(Action::Cancel),
                Resolved::Action(Action::Cancel),
            ]
        );
        assert_eq!(
            feed(&keymap, &mut state, Context::PullDown, &["tab", "f5"]),
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
            Some("Ctrl+x #")
        );
        assert_eq!(key(Context::Panel, Action::Disconnect), None);
        // In the root F8 disconnects, so Delete has only its own key there.
        assert_eq!(
            key(Context::Root, Action::Disconnect).as_deref(),
            Some("F8")
        );
        assert_eq!(
            key(Context::Root, Action::Delete).as_deref(),
            Some("Delete")
        );
        assert_eq!(key(Context::Root, Action::EditHost).as_deref(), Some("F4"));
        assert_eq!(key(Context::Root, Action::Edit), None, "F4 edits the host");
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
    fn the_vim_panels_open_the_command_line_and_the_menu_and_quit() {
        let vim = Keymap::by_name("vim").unwrap();
        let panel: Vec<Action> = vim
            .help(Context::Panel)
            .into_iter()
            .map(|(action, _)| action)
            .collect();
        assert_eq!(
            panel,
            [
                Action::Shell,
                Action::Command,
                Action::PullDown,
                Action::Help,
                Action::Up,
                Action::Down,
                Action::Home,
                Action::End,
                Action::Enter,
                Action::Parent,
                Action::Quit,
                Action::Redraw
            ]
        );
        let mut fkeys = [None; 10];
        fkeys[0] = Some(Action::Help);
        fkeys[8] = Some(Action::PullDown);
        fkeys[9] = Some(Action::Quit);
        assert_eq!(vim.fkeys(Context::Panel), fkeys);
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["!", ":", "g", "?"]),
            actions(&[Action::Shell, Action::Command, Action::Help])
        );
        // A character that no binding claims, which a panel takes for nothing.
        assert_eq!(
            feed(&vim, &mut state, Context::Panel, &["x"]),
            [Resolved::Insert('x')]
        );
        assert_eq!(
            vim.help(Context::CommandLine),
            Keymap::mc().help(Context::CommandLine)
        );
    }

    #[test]
    fn j_and_k_move_down_and_up_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        for context in [
            Context::Panel,
            Context::Root,
            Context::Dialog,
            Context::Viewer,
        ] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["j", "k"]),
                actions(&[Action::Down, Action::Up]),
                "{context:?}"
            );
        }
        // Where letters are text or filter a list, they stay text.
        for context in [Context::DialogInput, Context::Menu, Context::QuickSearch] {
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
        for context in [Context::Panel, Context::Root] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["l", "enter"]),
                actions(&[Action::Enter, Action::Enter]),
                "{context:?}"
            );
        }
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["l"]),
            [Resolved::Insert('l')],
            "a letter of the name searched for"
        );
    }

    #[test]
    fn h_and_minus_go_to_the_parent_directory_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        for context in [Context::Panel, Context::Root] {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&vim, &mut state, context, &["h", "-"]),
                actions(&[Action::Parent, Action::Parent]),
                "{context:?}"
            );
        }
        let mut state = KeyState::default();
        assert_eq!(
            feed(&vim, &mut state, Context::QuickSearch, &["h", "-"]),
            [Resolved::Insert('h'), Resolved::Insert('-')],
            "characters of the name searched for"
        );
    }

    #[test]
    fn g_g_and_shift_g_go_to_the_first_and_the_last_row_in_vim() {
        let vim = Keymap::by_name("vim").unwrap();
        for context in [
            Context::Panel,
            Context::Root,
            Context::Dialog,
            Context::Viewer,
        ] {
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
        assert_eq!(keys(Action::Up), Some("Up, Ctrl+p"));
        assert_eq!(keys(Action::Quit), Some("F10"));
        assert_eq!(keys(Action::Cancel), Some("Esc, Esc Esc"));
        assert_eq!(keys(Action::SortByName), Some("Ctrl+F3"));
        assert_eq!(panel[0].0, Action::Up, "in the order of the preset");
        assert_eq!(
            keymap.help(Context::Root),
            [
                (Action::EditHost, "F4".to_owned()),
                (Action::Disconnect, "F8".to_owned())
            ]
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
