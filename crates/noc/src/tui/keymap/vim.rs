//! The vim keymap: `ui.keymap = "vim"`.
//!
//! Each context with its actions, and each action with its key sequences in crokey's syntax, the
//! keys of a sequence apart; the help lists them in this order. `default.rs` has the same
//! contexts in the same order, and `noc keymap diff` shows what differs. Where Esc waits,
//! `esc 1` … `esc 0` stand for F1 … F10 without being written here.

use super::{Action, Context, Preset};

pub(super) const PRESET: Preset = &[
    // Everywhere but the output of commands: the other contexts fall back to it last.
    (
        Context::Global,
        &[
            // The keys that can be typed now, over whatever is in front, as which-key shows them.
            (Action::KeyHints, &["alt-/"]),
            // mc's and Far's: the output of commands, over whatever is in front, which stays.
            (Action::UserScreen, &["ctrl-o"]),
        ],
    ),
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
            // Space marks, as in yazi and ranger; Shift+Down and Shift+Up turn pages here.
            (Action::Mark, &["space", "insert"]),
            // `-` goes to the parent, so `\` alone unmarks; ranger's `v` inverts the marks.
            (Action::Select, &["+"]),
            (Action::Unselect, &["\\"]),
            (Action::InvertMarks, &["v"]),
            // netrw's and vinegar's `-`, next to `h`.
            (Action::Parent, &["h", "-", "backspace", "ctrl-h"]),
            // vim's window commands, with Ctrl held or not; with two panels the previous window
            // is the other one.
            (
                Action::SwitchPanel,
                &["tab", "ctrl-w w", "ctrl-w ctrl-w", "ctrl-w p"],
            ),
            (Action::SwapPanels, &["ctrl-w x"]),
            (Action::OtherPanelOpen, &["alt-o"]),
            (Action::OtherPanelSync, &["alt-i"]),
            // ranger's R.
            (Action::Reload, &["shift-r"]),
            (Action::Cancel, &["esc", "ctrl-c"]),
            // ranger's z for settings and o for the order of sorting.
            (Action::ToggleHidden, &["z h"]),
            (Action::SortByName, &["o n"]),
            (Action::SortByExtension, &["o e"]),
            (Action::SortByTime, &["o m"]),
            (Action::SortBySize, &["o s"]),
            // vim's `/`; in quick search, Ctrl+g goes to the next match.
            (Action::QuickSearch, &["/"]),
            // Not in mc, whose command line takes what is typed (ADR 0019).
            (Action::Shell, &["!"]),
            (Action::Command, &[":"]),
            // vim's `q:`, the window of the command-line history.
            (Action::CommandHistory, &["q :"]),
            (Action::Help, &["g ?", "f1"]),
            // ranger's i shows the file; e edits it.
            (Action::View, &["i", "f3"]),
            (Action::Edit, &["e", "f4"]),
            // Doubled as vim's operators on a line: y y copies, d d moves, and D D deletes; c w
            // changes the name in its row.
            (Action::Copy, &["y y", "f5"]),
            (Action::Move, &["d d", "f6"]),
            (Action::Rename, &["c w", "shift-f6", "f16"]),
            (Action::Mkdir, &["f7"]),
            // In text fields, Delete deletes a character.
            (Action::Delete, &["shift-d shift-d", "f8", "delete"]),
            // yazi's and ranger's w, for their tasks.
            (Action::Jobs, &["w"]),
            // Not in mc; `#` for a hash.
            (Action::Checksum, &["ctrl-x #"]),
            // Far Manager's menus to change drives.
            (Action::LocationMenuLeft, &["alt-f1"]),
            (Action::LocationMenuRight, &["alt-f2"]),
            // Not in mc: zoxide's `z`.
            (Action::Jump, &["alt-z"]),
            // ranger's `cd`.
            (Action::QuickCd, &["c d"]),
            // ranger's g n, g c, g t, and g T, as vim's tabs.
            (Action::NewTab, &["g n"]),
            (Action::CloseTab, &["g c"]),
            (Action::NextTab, &["g t", "alt-right"]),
            (Action::PrevTab, &["g shift-t", "alt-left"]),
            (Action::TabList, &["ctrl-x tab"]),
            // Not in mc: W for workspaces, and Shift saves the tabs of both panels as one.
            (Action::Workspaces, &["alt-w"]),
            (Action::SaveWorkspace, &["alt-shift-w"]),
            (Action::PullDown, &["g m", "f9"]),
            (Action::PullDownLast, &["g shift-m", "shift-f9", "f19"]),
            (Action::Quit, &["shift-z shift-z", "f10"]),
            (Action::Redraw, &["ctrl-l"]),
        ],
    ),
    // A panel on the virtual root or the list of hosts, on top of the panel's.
    (
        Context::Root,
        &[
            (Action::EditHost, &["f4"]),
            (Action::Disconnect, &["shift-d shift-d", "f8"]),
        ],
    ),
    // Quick search in the active panel; other keys go to the panel.
    (
        Context::QuickSearch,
        &[
            (Action::Backspace, &["backspace", "ctrl-h"]),
            (Action::Cancel, &["esc", "ctrl-c"]),
            // vim's Ctrl+g, to the next match while a pattern is typed.
            (Action::QuickSearch, &["ctrl-g"]),
        ],
    ),
    // The name field of an entry renamed in its row.
    (
        Context::Rename,
        &[
            (Action::Left, &["left"]),
            (Action::Right, &["right"]),
            // Ctrl+b, as on vim's command line.
            (Action::Home, &["home", "ctrl-b"]),
            (Action::End, &["end", "ctrl-e"]),
            (Action::Backspace, &["backspace", "ctrl-h"]),
            (Action::Delete, &["delete"]),
            (Action::DeleteToStart, &["ctrl-u"]),
            (Action::DeleteToEnd, &["ctrl-k"]),
            (Action::Confirm, &["enter"]),
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
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
            (Action::PageUp, &["ctrl-b", "pageup"]),
            (Action::PageDown, &["ctrl-f", "pagedown"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::NextField, &["tab"]),
            (Action::PrevField, &["backtab"]),
            (Action::Confirm, &["ctrl-y", "enter"]),
            (Action::Toggle, &["space"]),
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
        ],
    ),
    // The help of F1, on top of the dialog's: q closes it, as it closes the viewer and a pager.
    (Context::Help, &[(Action::Cancel, &["q"])]),
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
            (Action::Quit, &["q", "esc", "f3", "f10"]),
            (Action::Redraw, &["ctrl-l"]),
        ],
    ),
    // What the windows that list items to choose from share: the location menu, the zoxide
    // window, the workspaces, and the command history fall back to it.
    (
        Context::List,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::PageUp, &["ctrl-b", "shift-up", "pageup"]),
            (
                Action::PageDown,
                &["ctrl-f", "shift-down", "shift-enter", "pagedown"],
            ),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::Confirm, &["ctrl-y", "enter"]),
            (Action::Backspace, &["backspace", "ctrl-h"]),
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
        ],
    ),
    // The location menu, on top of the list's; characters filter it.
    (
        Context::LocationMenu,
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
    // The pull-down menu; letters run the commands that have them, so h, j, k, and l stay theirs.
    (
        Context::PullDown,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::Left, &["left", "backtab"]),
            (Action::Right, &["right", "tab"]),
            (Action::Home, &["ctrl-b", "shift-up", "home", "pageup"]),
            (
                Action::End,
                &["ctrl-f", "shift-down", "shift-enter", "end", "pagedown"],
            ),
            (Action::Confirm, &["ctrl-y", "enter"]),
            (Action::Cancel, &["esc", "ctrl-c", "f9", "f10"]),
        ],
    ),
    // Text fields, on top of the dialog's. Ctrl+b goes to the start, as on vim's command line.
    (
        Context::DialogInput,
        &[
            (Action::Home, &["home", "ctrl-b"]),
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
    // Ctrl+p and Ctrl+n move, as in vim's pop-up menu.
    (
        Context::Completion,
        &[
            (Action::Up, &["ctrl-p", "up"]),
            (Action::Down, &["ctrl-n", "down"]),
            (Action::PageUp, &["pageup"]),
            (Action::PageDown, &["pagedown"]),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::Complete, &["tab"]),
            (Action::Confirm, &["ctrl-y", "enter"]),
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
        ],
    ),
    // The command line of `!` and `:`. Ctrl+j arrives as LF where Enter arrives as CR, so it starts
    // a new line in every terminal; Shift+Enter does where the terminal speaks the kitty keyboard
    // protocol. As on vim's command line, Ctrl+b goes to the start, Ctrl+p and Ctrl+n go through
    // the history, and Ctrl+f opens the command for editing, as Ctrl+x Ctrl+e does in bash.
    (
        Context::CommandLine,
        &[
            (Action::Left, &["left"]),
            (Action::Right, &["right"]),
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::Home, &["home", "ctrl-b"]),
            (Action::End, &["end", "ctrl-e"]),
            (Action::Backspace, &["backspace", "ctrl-h"]),
            (Action::Delete, &["delete"]),
            (Action::DeleteToStart, &["ctrl-u"]),
            (Action::DeleteToEnd, &["ctrl-k"]),
            (Action::NewLine, &["ctrl-j", "shift-enter"]),
            (Action::EditCommand, &["ctrl-f", "ctrl-x ctrl-e"]),
            (Action::OlderCommand, &["ctrl-p"]),
            (Action::NewerCommand, &["ctrl-n"]),
            // bash's Ctrl+r.
            (Action::CommandHistory, &["ctrl-r"]),
            (Action::Confirm, &["enter"]),
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
        ],
    ),
    // The window of the command history, on top of the list's; characters filter it, and Tab
    // switches between the panel's host and all hosts.
    (
        Context::CommandHistory,
        &[(Action::NextField, &["tab"]), (Action::Delete, &["delete"])],
    ),
    // The terminal's own screen, with the output of commands.
    (Context::UserScreen, &[(Action::Cancel, &["ctrl-o", "esc"])]),
];
