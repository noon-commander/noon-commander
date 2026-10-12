//! The default keymap, modelled on Midnight Commander: `ui.keymap = "default"`.
//!
//! Each context with its actions, and each action with its key sequences in crokey's syntax, the
//! keys of a sequence apart; the help lists them in this order. `vim.rs` has the same contexts in
//! the same order, and `noc keymap diff` shows what differs. In the panels, the viewer, and the
//! windows that list items, `alt-1` … `alt-0` stand for F1 … F10 without being written here. An
//! action's keys start with mc's, before those it shares with `vim.rs`: the pull-down menu shows
//! the first.

use super::{Action, Context, Preset};

pub(super) const PRESET: Preset = &[
    // Everywhere but the output of commands: the other contexts fall back to it last.
    (
        Context::Global,
        &[
            (Action::KeyHints, &["alt-/"]),
            (Action::UserScreen, &["ctrl-o"]),
            (Action::Redraw, &["ctrl-l"]),
        ],
    ),
    // The panels.
    (
        Context::Panel,
        &[
            // Navigation
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::PageUp, &["pageup", "alt-v"]),
            (Action::PageDown, &["pagedown", "ctrl-v"]),
            (Action::HalfPageUp, &["shift-pageup"]),
            (Action::HalfPageDown, &["shift-pagedown"]),
            (Action::Home, &["home", "alt-<"]),
            (Action::End, &["end", "alt->"]),
            (Action::Enter, &["enter", "ctrl-m", "ctrl-pagedown"]),
            (Action::Parent, &["ctrl-pageup", "backspace", "ctrl-h"]),
            (Action::Reload, &["ctrl-r"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c"]),
            // PullDown
            (Action::PullDown, &["f9"]),
            (Action::PullDownLast, &["shift-f9"]),
            // Panels
            (Action::SwitchPanel, &["tab", "ctrl-i"]),
            (Action::FocusLeft, &["ctrl-x left", "ctrl-w h"]),
            (Action::FocusRight, &["ctrl-x right", "ctrl-w l"]),
            (Action::SwapPanels, &["ctrl-u"]),
            (Action::OtherPanelOpen, &["alt-o", "ctrl-w f"]),
            (Action::OtherPanelSync, &["alt-i", "ctrl-w v"]),
            (Action::LocationMenuLeft, &["alt-f1", "ctrl-x 1"]),
            (Action::LocationMenuRight, &["alt-f2", "ctrl-x 2"]),
            // Tabs & Workspaces
            (Action::NewTab, &["t n"]),
            (Action::CloseTab, &["t c"]),
            (Action::NextTab, &["] t", "alt-right"]),
            (Action::PrevTab, &["[ t", "alt-left"]),
            (Action::TabListLeft, &["t h"]),
            (Action::TabListRight, &["t l"]),
            (Action::Workspaces, &["t w"]),
            (Action::SaveWorkspace, &["t shift-w"]),
            // Items
            (Action::ToggleHidden, &["alt-.", "o h"]),
            (Action::SortByName, &["o n"]),
            (Action::SortByExtension, &["o e"]),
            (Action::SortByTime, &["o m"]),
            (Action::SortBySize, &["o s"]),
            // Marks
            (Action::Mark, &["insert", "ctrl-t", "shift-down"]),
            (Action::MarkUp, &["shift-up"]),
            (Action::Select, &["+", "alt-+"]),
            (Action::Unselect, &["-", "\\", "alt--"]),
            (Action::InvertMarks, &["*", "alt-*"]),
            // Search
            (Action::QuickSearch, &["ctrl-s", "alt-s"]),
            (Action::NextMatch, &["shift-f7", "n"]),
            (Action::PrevMatch, &["shift-n"]),
            // Tools
            (Action::Shell, &["!"]),
            (Action::Command, &[":"]),
            (Action::CommandHistory, &["alt-h"]),
            (Action::Jump, &["alt-z", "ctrl-x z"]),
            (Action::QuickCd, &["alt-c"]),
            (Action::View, &["f3"]),
            (Action::Edit, &["f4"]),
            (Action::Copy, &["f5"]),
            (Action::Move, &["f6"]),
            (Action::Rename, &["shift-f6"]),
            (Action::Mkdir, &["f7"]),
            (Action::Delete, &["f8"]),
            (Action::Checksum, &["f #"]),
            (Action::Jobs, &["ctrl-x j", "f j"]),
            // Other
            (Action::Help, &["f1"]),
            (Action::Quit, &["f10"]),
        ],
    ),
    // What the text fields share: quick search, the field of an entry renamed in its row, the
    // text fields of dialogs, the command line, and the filters of the lists fall back to it.
    // Ctrl+b, Ctrl+f, and Ctrl+d move and delete as in mc's input lines and the shell; Alt+b and
    // Alt+f go by words, and so do the Option key and the arrows of macOS's Terminal, which sends
    // them. Alt+< and Alt+> go to the start and the end, as in mc's input lines; the lists keep
    // them for their first and last row.
    (
        Context::Input,
        &[
            (Action::Left, &["left", "ctrl-b"]),
            (Action::Right, &["right", "ctrl-f"]),
            (Action::TextStart, &["home", "ctrl-a", "alt-<"]),
            (Action::TextEnd, &["end", "ctrl-e", "alt->"]),
            (Action::WordLeft, &["ctrl-left", "alt-b"]),
            (Action::WordRight, &["ctrl-right", "alt-f"]),
            (Action::Backspace, &["backspace", "ctrl-h"]),
            (Action::DeleteChar, &["delete", "ctrl-d"]),
            (Action::DeleteToStart, &["ctrl-u"]),
            (Action::DeleteToEnd, &["ctrl-k"]),
            (
                Action::DeleteWord,
                &["alt-backspace", "ctrl-w", "ctrl-backspace"],
            ),
            (Action::DeleteNextWord, &["alt-d", "ctrl-delete"]),
        ],
    ),
    // Quick search in the active panel, on top of the text field's; other keys go to the panel.
    // Ctrl+g goes back to where the search started, and Ctrl+r to the previous match, as in
    // emacs's search.
    (
        Context::QuickSearch,
        &[
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c"]),
            (Action::Abort, &["ctrl-g"]),
            (Action::QuickSearch, &["ctrl-s", "alt-s"]),
            (Action::PrevMatch, &["ctrl-r"]),
        ],
    ),
    // The name field of an entry renamed in its row, on top of the text field's.
    (
        Context::Rename,
        &[
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-g", "ctrl-c"]),
        ],
    ),
    // Dialogs whose focus is on a button or a list. Ctrl+p and Ctrl+n move as in the lists; the
    // page keys of the lists stay out, for the text fields fall back to the dialog.
    (
        Context::Dialog,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::Left, &["left"]),
            (Action::Right, &["right"]),
            (Action::PageUp, &["pageup"]),
            (Action::PageDown, &["pagedown"]),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::NextField, &["tab", "ctrl-i"]),
            (Action::PrevField, &["backtab"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Toggle, &["space"]),
            (
                Action::Cancel,
                &["esc", "ctrl-[", "f10", "ctrl-g", "ctrl-c"],
            ),
        ],
    ),
    // The help of F1, on top of the dialog's, which has the arrows, Ctrl+p, and Ctrl+n; the keys
    // of mc's help scroll it too, Space among them, which presses no button here.
    (
        Context::Help,
        &[
            (Action::PageUp, &["alt-v", "b", "backspace", "ctrl-h"]),
            (Action::PageDown, &["ctrl-v", "space", "f"]),
            (Action::HalfPageUp, &["u"]),
            (Action::HalfPageDown, &["d"]),
            (Action::Home, &["alt-<", "g", "ctrl-home", "ctrl-pageup"]),
            (
                Action::End,
                &["alt->", "shift-g", "ctrl-end", "ctrl-pagedown"],
            ),
        ],
    ),
    // The viewer of F3.
    (
        Context::Viewer,
        &[
            (Action::Up, &["up", "k", "y", "insert", "ctrl-p"]),
            (
                Action::Down,
                &["down", "j", "e", "enter", "ctrl-m", "delete", "ctrl-n"],
            ),
            (
                Action::PageUp,
                &["pageup", "alt-v", "b", "backspace", "ctrl-h"],
            ),
            (Action::PageDown, &["pagedown", "space", "f", "ctrl-v"]),
            (Action::HalfPageUp, &["u"]),
            (Action::HalfPageDown, &["d"]),
            (
                Action::Home,
                &["home", "g", "ctrl-home", "ctrl-pageup", "alt-<"],
            ),
            (
                Action::End,
                &["end", "shift-g", "ctrl-end", "ctrl-pagedown", "alt->"],
            ),
            (Action::Left, &["left", "h"]),
            (Action::Right, &["right", "l"]),
            (Action::ToggleWrap, &["f2"]),
            (Action::Help, &["f1"]),
            (Action::Quit, &["f3", "f10", "q", "esc", "ctrl-[", "ctrl-c"]),
        ],
    ),
    // What the windows that list items to choose from share: the location menu, the tab menu, the
    // zoxide window, the workspaces, and the command history fall back to it, then to the text
    // field's keys, which edit their filters.
    (
        Context::List,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::PageUp, &["pageup", "alt-v"]),
            (Action::PageDown, &["pagedown", "ctrl-v"]),
            (Action::Home, &["home", "alt-<"]),
            (Action::End, &["end", "alt->"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (
                Action::Cancel,
                &["esc", "ctrl-[", "f10", "ctrl-g", "ctrl-c"],
            ),
        ],
    ),
    // The location menu, on top of the list's; characters filter it. F4 and F8 edit and
    // disconnect the host under the cursor, and Delete disconnects it too, as in Far's menu of
    // drives.
    (
        Context::LocationMenu,
        &[
            (Action::EditHost, &["f4"]),
            (Action::Disconnect, &["f8", "delete"]),
            (Action::Reload, &["ctrl-r"]),
        ],
    ),
    // The tab menu of a panel, on top of the list's; characters filter it. F8 and Delete close
    // the tab under the cursor, as they delete a workspace.
    (Context::TabMenu, &[(Action::CloseTab, &["f8", "delete"])]),
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
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::Left, &["left", "ctrl-b", "backtab"]),
            (Action::Right, &["right", "ctrl-f", "tab", "ctrl-i"]),
            (Action::Home, &["home", "alt-<", "ctrl-a", "pageup"]),
            (Action::End, &["end", "alt->", "ctrl-e", "pagedown"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (
                Action::Cancel,
                &["esc", "ctrl-[", "f9", "f10", "ctrl-g", "ctrl-c"],
            ),
        ],
    ),
    // The text fields of dialogs: the text field's keys, then the dialog's.
    (Context::DialogInput, &[]),
    // Path fields, on top of the text field's: Tab completes, as in a shell; Shift+Tab and Down
    // still leave the field.
    (
        Context::PathInput,
        &[(Action::Complete, &["tab", "ctrl-i"])],
    ),
    // The list of completions under a path field; other keys close it and go to the field.
    (
        Context::Completion,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::PageUp, &["pageup", "alt-v"]),
            (Action::PageDown, &["pagedown", "ctrl-v"]),
            (Action::Home, &["home", "alt-<"]),
            (Action::End, &["end", "alt->"]),
            (Action::Complete, &["tab", "ctrl-i"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-g", "ctrl-c"]),
        ],
    ),
    // The command line of `!` and `:`, on top of the text field's. Ctrl+j arrives as LF where
    // Enter arrives as CR, so it starts a new line in every terminal; Shift+Enter does where the
    // terminal speaks the kitty keyboard protocol. Ctrl+x Ctrl+e edits the command, and Ctrl+p
    // and Ctrl+n go through the history, as in bash.
    (
        Context::CommandLine,
        &[
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::NewLine, &["ctrl-j", "shift-enter"]),
            (Action::EditCommand, &["ctrl-x ctrl-e"]),
            (Action::OlderCommand, &["alt-p", "ctrl-p"]),
            (Action::NewerCommand, &["alt-n", "ctrl-n"]),
            // mc's Alt+h, and bash's Ctrl+r.
            (Action::CommandHistory, &["alt-h", "ctrl-r"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (
                Action::Cancel,
                &["esc", "ctrl-[", "ctrl-g", "ctrl-c", "f10"],
            ),
        ],
    ),
    // The window of the command history, on top of the list's; characters filter it, and Tab
    // switches between the panel's host and all hosts. Shift+Delete removes a command, as in
    // Far's histories; Ctrl+Backspace and Ctrl+Delete stay the filter's.
    (
        Context::CommandHistory,
        &[
            (Action::NextField, &["tab", "ctrl-i"]),
            (Action::Delete, &["delete", "shift-delete", "f8"]),
        ],
    ),
    // The terminal's own screen, with the output of commands.
    (
        Context::UserScreen,
        &[(Action::Cancel, &["ctrl-o", "esc", "ctrl-["])],
    ),
];
