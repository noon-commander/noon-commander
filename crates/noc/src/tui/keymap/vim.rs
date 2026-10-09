//! The vim keymap: `ui.keymap = "vim"`.
//!
//! Each context with its actions, and each action with its key sequences in crokey's syntax, the
//! keys of a sequence apart; the help lists them in this order. `default.rs` has the same
//! contexts in the same order, and `noc keymap diff` shows what differs. Where Esc waits,
//! `esc 1` … `esc 0` stand for F1 … F10 without being written here.

use super::{Action, Context, Preset};

pub(super) const PRESET: Preset = &[
    // The panels.
    (
        Context::Panel,
        &[
            (Action::Up, &["k", "up", "ctrl-p"]),
            (Action::Down, &["j", "down", "ctrl-n"]),
            (Action::PageUp, &["ctrl-b", "shift-up", "pageup"]),
            (
                Action::PageDown,
                &["ctrl-f", "shift-down", "shift-enter", "pagedown"],
            ),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::Enter, &["l", "enter"]),
            // netrw's and vinegar's `-`, next to `h`.
            (Action::Parent, &["h", "-", "backspace"]),
            // Not in mc, whose command line takes what is typed (ADR 0019).
            (Action::Shell, &["!"]),
            (Action::Command, &[":"]),
            (Action::Help, &["g ?", "f1"]),
            (Action::PullDown, &["g m", "f9"]),
            (Action::Quit, &["shift-z shift-z", "f10"]),
            (Action::Redraw, &["ctrl-l"]),
        ],
    ),
    // A panel on the virtual root or the list of hosts, on top of the panel's.
    (
        Context::Root,
        &[(Action::EditHost, &["f4"]), (Action::Disconnect, &["f8"])],
    ),
    // Quick search in the active panel; other keys go to the panel.
    (
        Context::QuickSearch,
        &[
            (Action::Backspace, &["backspace"]),
            (Action::Cancel, &["esc"]),
        ],
    ),
    // The name field of an entry renamed in its row.
    (
        Context::Rename,
        &[
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
        ],
    ),
    // Dialogs whose focus is on a button or a list.
    (
        Context::Dialog,
        &[
            (Action::Up, &["k", "up"]),
            (Action::Down, &["j", "down"]),
            (Action::Left, &["left"]),
            (Action::Right, &["right"]),
            (Action::PageUp, &["pageup"]),
            (Action::PageDown, &["pagedown"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::NextField, &["tab"]),
            (Action::PrevField, &["backtab"]),
            (Action::Confirm, &["enter"]),
            (Action::Toggle, &["space"]),
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
        ],
    ),
    // The viewer of F3.
    (
        Context::Viewer,
        &[
            (Action::Up, &["k", "up", "ctrl-p"]),
            (Action::Down, &["j", "down", "ctrl-n", "enter"]),
            (Action::PageUp, &["ctrl-b", "shift-up", "pageup"]),
            (
                Action::PageDown,
                &["ctrl-f", "shift-down", "shift-enter", "pagedown"],
            ),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::Left, &["h", "left"]),
            (Action::Right, &["l", "right"]),
            (Action::ToggleWrap, &["f2"]),
            (Action::Help, &["g ?", "f1"]),
            (Action::Quit, &["f3", "f10", "q", "esc"]),
            (Action::Redraw, &["ctrl-l"]),
        ],
    ),
    // The location menu; characters filter it.
    (
        Context::Menu,
        &[
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
        ],
    ),
    // The zoxide window; characters are keywords.
    (
        Context::Jump,
        &[
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::PageUp, &["pageup"]),
            (Action::PageDown, &["pagedown"]),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::Confirm, &["enter"]),
            (Action::Backspace, &["backspace"]),
            (Action::Cancel, &["esc", "f10"]),
        ],
    ),
    // The window of the saved workspaces; characters filter it. Insert adds one, as in Far's menus;
    // F6 renames and F8 deletes, as they rename and delete files.
    (
        Context::Workspaces,
        &[
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
        ],
    ),
    // The pull-down menu; letters run the commands that have them, so h, j, k, and l stay theirs.
    (
        Context::PullDown,
        &[
            (Action::Up, &["ctrl-p", "up"]),
            (Action::Down, &["ctrl-n", "down"]),
            (Action::Left, &["left"]),
            (Action::Right, &["right"]),
            (Action::Home, &["home", "pageup"]),
            (Action::End, &["end", "pagedown"]),
            (Action::Confirm, &["enter"]),
            (Action::Cancel, &["esc", "ctrl-c", "f9", "f10"]),
        ],
    ),
    // Text fields, on top of the dialog's.
    (
        Context::DialogInput,
        &[
            (Action::Home, &["home", "ctrl-a"]),
            (Action::End, &["end", "ctrl-e"]),
            (Action::Backspace, &["backspace"]),
            (Action::Delete, &["delete"]),
            (Action::DeleteToStart, &["ctrl-u"]),
            (Action::DeleteToEnd, &["ctrl-k"]),
        ],
    ),
    // Path fields, on top of the text field's: Tab completes, as in a shell; Shift+Tab and Down
    // still leave the field.
    (Context::PathInput, &[(Action::Complete, &["tab"])]),
    // The list of completions under a path field; other keys close it and go to the field.
    (
        Context::Completion,
        &[
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::PageUp, &["pageup"]),
            (Action::PageDown, &["pagedown"]),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::Complete, &["tab"]),
            (Action::Confirm, &["enter"]),
            (Action::Cancel, &["esc"]),
        ],
    ),
    // The command line of `!` and `:`. Ctrl+j arrives as LF where Enter arrives as CR, so it starts
    // a new line in every terminal; Shift+Enter does where the terminal speaks the kitty keyboard
    // protocol. Ctrl+x Ctrl+e edits the command, as in bash.
    (
        Context::CommandLine,
        &[
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
        ],
    ),
    // The window of the command history; characters filter it, and Tab switches between the panel's
    // host and all hosts.
    (
        Context::History,
        &[
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
        ],
    ),
    // The terminal's own screen, with the output of commands.
    (Context::UserScreen, &[(Action::Cancel, &["ctrl-o", "esc"])]),
];
