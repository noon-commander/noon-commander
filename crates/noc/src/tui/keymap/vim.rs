//! The vim keymap: `ui.keymap = "vim"`.
//!
//! Each context with its actions, and each action with its key sequences in crokey's syntax, the
//! keys of a sequence apart; the help lists them in this order. `default.rs` has the same
//! contexts in the same order, and `noc keymap diff` shows what differs. In the panels, the
//! viewer, and the windows that list items, `alt-1` … `alt-0` stand for F1 … F10 without being
//! written here.

use super::{Action, Context, Preset};

pub(super) const PRESET: Preset = &[
    // Everywhere but the output of commands: the other contexts fall back to it last.
    (
        Context::Global,
        &[
            (Action::KeyHints, &["alt-/"]),
            (Action::UserScreen, &["g <", "ctrl-o"]),
            (Action::Redraw, &["ctrl-l"]),
        ],
    ),
    // The panels.
    (
        Context::Panel,
        &[
            // Navigation
            (Action::Up, &["k", "up", "ctrl-p"]),
            (Action::Down, &["j", "down", "ctrl-n"]),
            (Action::PageUp, &["ctrl-b", "pageup"]),
            (Action::PageDown, &["ctrl-f", "pagedown"]),
            (Action::HalfPageUp, &["ctrl-u"]),
            (Action::HalfPageDown, &["ctrl-d"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::Enter, &["l", "right", "enter", "ctrl-m"]),
            (Action::Parent, &["h", "left", "backspace", "ctrl-h"]),
            (Action::Reload, &["shift-r"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c"]),
            // PullDown
            (Action::PullDown, &["g m", "f9"]),
            (Action::PullDownLast, &["g shift-m", "shift-f9"]),
            // Panels
            (
                Action::SwitchPanel,
                &["tab", "ctrl-i", "ctrl-w w", "ctrl-w ctrl-w", "ctrl-w p"],
            ),
            (Action::FocusLeft, &["ctrl-w h", "ctrl-x left"]),
            (Action::FocusRight, &["ctrl-w l", "ctrl-x right"]),
            (Action::SwapPanels, &["ctrl-w x", "ctrl-w r"]),
            (Action::OtherPanelOpen, &["ctrl-w f"]),
            (Action::OtherPanelSync, &["ctrl-w v"]),
            (Action::LocationMenuLeft, &["ctrl-w 1"]),
            (Action::LocationMenuRight, &["ctrl-w 2"]),
            // Tabs & Workspaces
            (Action::NewTab, &["t n", "g n"]),
            (Action::CloseTab, &["t c", "g c"]),
            (Action::NextTab, &["] t", "g t", "alt-right"]),
            (Action::PrevTab, &["[ t", "g shift-t", "alt-left"]),
            (Action::TabListLeft, &["t h"]),
            (Action::TabListRight, &["t l"]),
            (Action::Workspaces, &["t w"]),
            (Action::SaveWorkspace, &["t shift-w"]),
            // Items
            (Action::ToggleHidden, &["o h"]),
            (Action::SortByName, &["o n"]),
            (Action::SortByExtension, &["o e"]),
            (Action::SortByTime, &["o m"]),
            (Action::SortBySize, &["o s"]),
            // Marks
            (Action::Mark, &["space", "shift-j", "shift-down", "insert"]),
            (Action::MarkUp, &["shift-k", "shift-up"]),
            (Action::Select, &["+"]),
            (Action::Unselect, &["-"]),
            (Action::InvertMarks, &["*"]),
            // Search
            (Action::QuickSearch, &["/"]),
            (Action::NextMatch, &["n", "shift-f7"]),
            (Action::PrevMatch, &["shift-n"]),
            // Tools
            (Action::Shell, &["!"]),
            (Action::Command, &[":"]),
            (Action::CommandHistory, &["q :"]),
            (Action::Jump, &["g z"]),
            (Action::QuickCd, &["g d"]),
            (Action::View, &["i", "f3"]),
            (Action::Edit, &["e", "f4"]),
            (Action::Copy, &["shift-y", "y y", "f5"]),
            (Action::Move, &["x", "f6"]),
            (Action::Rename, &["r", "shift-f6"]),
            (Action::Mkdir, &["shift-a", "f7"]),
            (Action::Delete, &["d shift-d", "f8"]),
            (Action::Checksum, &["f #"]),
            (Action::Jobs, &["f j"]),
            // Other
            (Action::Help, &["g ?", "f1"]),
            (Action::Quit, &["shift-z shift-z", "f10"]),
        ],
    ),
    // What the text fields share: quick search, the field of an entry renamed in its row, the
    // text fields of dialogs, the command line, and the filters of the lists fall back to it.
    // Ctrl+b goes to the start, and Shift+Left and Shift+Right go by words, as on vim's command
    // line; Ctrl+a goes to the start too, as in the shell, where a list's Ctrl+b pages.
    (
        Context::Input,
        &[
            (Action::Left, &["left"]),
            (Action::Right, &["right"]),
            (Action::TextStart, &["home", "ctrl-b", "ctrl-a"]),
            (Action::TextEnd, &["end", "ctrl-e"]),
            (Action::WordLeft, &["shift-left", "ctrl-left"]),
            (Action::WordRight, &["shift-right", "ctrl-right"]),
            (Action::Backspace, &["backspace", "ctrl-h"]),
            (Action::DeleteChar, &["delete"]),
            (Action::DeleteToStart, &["ctrl-u"]),
            (Action::DeleteToEnd, &["ctrl-k"]),
            (
                Action::DeleteWord,
                &["ctrl-w", "alt-backspace", "ctrl-backspace"],
            ),
            (Action::DeleteNextWord, &["ctrl-delete"]),
        ],
    ),
    // Quick search in the active panel, on top of the text field's; other keys go to the panel.
    // Ctrl+g and Ctrl+t go to the next and the previous match, as while vim's search is typed.
    (
        Context::QuickSearch,
        &[
            // As in vim, Enter ends the search on the match, rather than opening the entry, and
            // Esc and Ctrl+c go back to where it started.
            (Action::Cancel, &["enter", "ctrl-m"]),
            (Action::Abort, &["esc", "ctrl-[", "ctrl-c"]),
            (Action::QuickSearch, &["ctrl-g"]),
            (Action::PrevMatch, &["ctrl-t"]),
        ],
    ),
    // The name field of an entry renamed in its row, on top of the text field's. Ctrl+y does
    // not confirm here, on the command line, or in dialogs, whose text fields fall back to them:
    // the shell, mc, and ranger paste with it, so after Ctrl+w or Ctrl+u it would take the text
    // cut short. vim confirms with it only in its pop-up menu, as the lists and menus do here.
    (
        Context::Rename,
        &[
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c"]),
        ],
    ),
    // Dialogs whose focus is on a button or a list; no Ctrl+y, as in renaming.
    (
        Context::Dialog,
        &[
            (Action::Up, &["k", "up", "ctrl-p"]),
            (Action::Down, &["j", "down", "ctrl-n"]),
            (Action::Left, &["h", "left"]),
            (Action::Right, &["l", "right"]),
            (Action::PageUp, &["ctrl-b", "pageup"]),
            (Action::PageDown, &["ctrl-f", "pagedown"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::NextField, &["tab", "ctrl-i"]),
            (Action::PrevField, &["backtab"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Toggle, &["space"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c", "f10"]),
        ],
    ),
    // The help of F1, on top of the dialog's: Ctrl+u and Ctrl+d go half a page, and q closes it,
    // as in the viewer and a pager.
    (
        Context::Help,
        &[
            (Action::HalfPageUp, &["ctrl-u"]),
            (Action::HalfPageDown, &["ctrl-d"]),
            (Action::Cancel, &["q"]),
        ],
    ),
    // The viewer of F3.
    (
        Context::Viewer,
        &[
            (Action::Up, &["k", "ctrl-y", "up", "ctrl-p"]),
            (
                Action::Down,
                &["j", "ctrl-e", "down", "ctrl-n", "enter", "ctrl-m"],
            ),
            (Action::PageUp, &["ctrl-b", "shift-up", "pageup"]),
            (Action::PageDown, &["ctrl-f", "shift-down", "pagedown"]),
            (Action::HalfPageUp, &["ctrl-u"]),
            (Action::HalfPageDown, &["ctrl-d"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::Left, &["h", "left"]),
            (Action::Right, &["l", "right"]),
            (Action::ToggleWrap, &["f2"]),
            (Action::Help, &["g ?", "f1"]),
            (
                Action::Quit,
                &["q", "i", "esc", "ctrl-[", "ctrl-c", "f3", "f10"],
            ),
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
            (Action::PageUp, &["ctrl-b", "shift-up", "pageup"]),
            (Action::PageDown, &["ctrl-f", "shift-down", "pagedown"]),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::Confirm, &["ctrl-y", "enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c", "f10"]),
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
    // The pull-down menu; letters run the commands that have them, so h, j, k, and l stay theirs.
    (
        Context::PullDown,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::Left, &["left", "backtab"]),
            (Action::Right, &["right", "tab", "ctrl-i"]),
            (Action::Home, &["ctrl-b", "shift-up", "home", "pageup"]),
            (Action::End, &["ctrl-f", "shift-down", "end", "pagedown"]),
            (Action::Confirm, &["ctrl-y", "enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c", "f9", "f10"]),
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
    // Ctrl+p and Ctrl+n move, as in vim's pop-up menu, and Ctrl+b and Ctrl+f page, as in the lists.
    (
        Context::Completion,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::PageUp, &["ctrl-b", "pageup"]),
            (Action::PageDown, &["ctrl-f", "pagedown"]),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::Complete, &["tab", "ctrl-i"]),
            (Action::Confirm, &["ctrl-y", "enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c"]),
        ],
    ),
    // The command line of `!` and `:`, on top of the text field's. Ctrl+j arrives as LF where
    // Enter arrives as CR, so it starts a new line in every terminal; Shift+Enter does where the
    // terminal speaks the kitty keyboard protocol. As on vim's command line, Ctrl+p and Ctrl+n go
    // through the history, and Ctrl+f opens the command for editing, as Ctrl+x Ctrl+e does in
    // bash. No Ctrl+y, as in renaming.
    (
        Context::CommandLine,
        &[
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::NewLine, &["ctrl-j", "shift-enter"]),
            (Action::EditCommand, &["ctrl-f", "ctrl-x ctrl-e"]),
            (Action::OlderCommand, &["ctrl-p"]),
            (Action::NewerCommand, &["ctrl-n"]),
            // bash's Ctrl+r.
            (Action::CommandHistory, &["ctrl-r"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c", "f10"]),
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
    // The terminal's own screen, with the output of commands. Enter and q go back, as from the
    // prompt under the output that vim's g < shows.
    (
        Context::UserScreen,
        &[(
            Action::Cancel,
            &["g <", "ctrl-o", "enter", "ctrl-m", "q", "esc", "ctrl-["],
        )],
    ),
];
