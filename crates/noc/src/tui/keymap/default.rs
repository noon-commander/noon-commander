//! The default keymap, modelled on Midnight Commander: `ui.keymap = "default"`.
//!
//! Each context with its actions, and each action with its key sequences in crokey's syntax, the
//! keys of a sequence apart; the help lists them in this order. `vim.rs` has the same contexts in
//! the same order, and `noc keymap diff` shows what differs. Where Esc waits, `esc 1` … `esc 0`
//! stand for F1 … F10 without being written here.

use super::{Action, Context, Preset};

pub(super) const PRESET: Preset = &[
    // Everywhere but the output of commands: the other contexts fall back to it last.
    (
        Context::Global,
        &[
            // Not in mc or Far: the keys that can be typed now, over whatever is in front, as
            // which-key shows them. Alt+? stays free for finding files, as in mc.
            (Action::KeyHints, &["alt-/"]),
        ],
    ),
    // The panels.
    (
        Context::Panel,
        &[
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
            // Not in mc: Far's Shift+F10, next to F9, as mc's Shift+F10 quits. Terminals without
            // Shift+F9 send F19.
            (Action::PullDownLast, &["shift-f9", "f19"]),
            (Action::Quit, &["f10"]),
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
            (Action::Cancel, &["esc", "f10"]),
        ],
    ),
    // Dialogs whose focus is on a button or a list.
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
    // The viewer of F3.
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
    // What the windows that list items to choose from share: the location menu, the zoxide
    // window, the workspaces, and the command history fall back to it.
    (
        Context::List,
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
    // The location menu, on top of the list's; characters filter it.
    (
        Context::Menu,
        &[(Action::Disconnect, &["f8"]), (Action::Reload, &["ctrl-r"])],
    ),
    // The zoxide window, on top of the list's; characters are keywords.
    (Context::Jump, &[]),
    // The window of the saved workspaces, on top of the list's; characters filter it. Insert adds
    // one, as in Far's menus; F6 renames and F8 deletes, as they rename and delete files.
    (
        Context::Workspaces,
        &[
            (Action::SaveWorkspace, &["insert"]),
            (Action::Move, &["f6"]),
            (Action::Delete, &["f8", "delete"]),
        ],
    ),
    // The pull-down menu; letters run the commands that have them.
    (
        Context::PullDown,
        &[
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::Left, &["left", "backtab"]),
            (Action::Right, &["right", "tab"]),
            (Action::Home, &["home", "pageup"]),
            (Action::End, &["end", "pagedown"]),
            (Action::Confirm, &["enter"]),
            (Action::Cancel, &["esc", "f9", "f10"]),
        ],
    ),
    // Text fields, on top of the dialog's.
    (
        Context::DialogInput,
        &[
            (Action::Home, &["home", "ctrl-a"]),
            (Action::End, &["end", "ctrl-e"]),
            (Action::Backspace, &["backspace", "ctrl-h"]),
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
            (Action::Cancel, &["esc", "f10"]),
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
            (Action::Backspace, &["backspace", "ctrl-h"]),
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
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
        ],
    ),
    // The window of the command history, on top of the list's; characters filter it, and Tab
    // switches between the panel's host and all hosts.
    (
        Context::History,
        &[(Action::NextField, &["tab"]), (Action::Delete, &["delete"])],
    ),
    // The terminal's own screen, with the output of commands.
    (Context::UserScreen, &[(Action::Cancel, &["ctrl-o", "esc"])]),
];
