//! The vim keymap: `ui.keymap = "vim"`.
//!
//! Each context with its actions, and each action with its key sequences in crokey's syntax, the
//! keys of a sequence apart; the help lists them in this order. `default.rs` has the same
//! contexts in the same order, and `noc keymap diff` shows what differs. In the panels and the
//! windows that list items, `alt-1` … `alt-0` stand for F1 … F10 without being written here.

// NOTE: Comments marked `OK1` are the maintainer's: the binding was checked and approved.
// Do not touch them.

use super::{Action, Context, Preset};

pub(super) const PRESET: Preset = &[
    // Everywhere but the output of commands: the other contexts fall back to it last.
    (
        Context::Global,
        &[
            (Action::KeyHints, &["alt-/"]),                  // OK1
            (Action::UserScreen, &["ctrl-`", "ctrl-space"]), // OK1
        ],
    ),
    // The panels.
    (
        Context::Panel,
        &[
            // Navigation
            (Action::Up, &["k", "up", "ctrl-p"]),        // OK1
            (Action::Down, &["j", "down", "ctrl-n"]),    // OK1
            (Action::PageUp, &["ctrl-b", "pageup"]),     // OK1
            (Action::PageDown, &["ctrl-f", "pagedown"]), // OK1
            (Action::HalfPageUp, &["ctrl-u"]),
            (Action::HalfPageDown, &["ctrl-d"]),
            (Action::Home, &["g g", "home"]),              // OK1
            (Action::End, &["shift-g", "end"]),            // OK1
            (Action::Enter, &["l", "right", "enter"]),     // OK1
            (Action::Parent, &["h", "left", "backspace"]), // OK1
            (Action::Reload, &["shift-r"]),                // OK1
            (Action::Cancel, &["esc", "ctrl-c"]),          // OK1
            // PullDown
            (Action::PullDown, &["g m", "f9"]), // OK1
            (Action::PullDownLast, &["g shift-m", "shift-f9", "f19"]), // OK1
            // Panels
            (Action::SwitchPanel, &["tab", "ctrl-w w", "ctrl-w ctrl-w"]),
            // vim's Ctrl+w h and Ctrl+w l go to the window on the left and on the right; the
            // location menus take the numbers of the default keymap's Ctrl+x 1 and Ctrl+x 2.
            (Action::FocusLeft, &["ctrl-w h", "ctrl-x left"]),
            (Action::FocusRight, &["ctrl-w l", "ctrl-x right"]),
            (Action::SwapPanels, &["ctrl-w x"]),     // OK1
            (Action::OtherPanelOpen, &["ctrl-w f"]), // OK1
            (Action::OtherPanelSync, &["ctrl-w v"]), // OK1
            (Action::LocationMenuLeft, &["ctrl-w 1"]),
            (Action::LocationMenuRight, &["ctrl-w 2"]),
            // Tabs & Workspaces
            (Action::NewTab, &["t n", "g n"]),               // OK1
            (Action::CloseTab, &["t c", "g c"]),             // OK1
            (Action::NextTab, &["] t", "g t", "alt-right"]), // OK1
            (Action::PrevTab, &["[ t", "g shift-t", "alt-left"]), // OK1
            (Action::TabListLeft, &["t h"]),                 // OK1
            (Action::TabListRight, &["t l"]),                // OK1
            (Action::Workspaces, &["t w"]),                  // OK1
            (Action::SaveWorkspace, &["t shift-w"]),         // OK1
            // Items
            (Action::ToggleHidden, &["o h"]),    // OK1
            (Action::SortByName, &["o n"]),      // OK1
            (Action::SortByExtension, &["o e"]), // OK1
            (Action::SortByTime, &["o m"]),      // OK1
            (Action::SortBySize, &["o s"]),      // OK1
            // Marks
            // Space, as in yazi and ranger. * inverts, as in mc, and leaves v for a visual mode.
            (Action::Mark, &["shift-j", "space", "shift-down", "insert"]),
            (Action::MarkUp, &["shift-k", "shift-up"]), // OK1
            (Action::Select, &["+"]),                   // OK1
            (Action::Unselect, &["-"]),                 // OK1
            (Action::InvertMarks, &["*"]),
            // Tools
            (Action::QuickSearch, &["/"]), // OK1
            // The last quick search again, as vim's n and N; Shift+F7 as in the default keymap.
            (Action::NextMatch, &["n", "shift-f7", "f17"]),
            (Action::PrevMatch, &["shift-n"]),
            (Action::Shell, &["!"]),            // OK1
            (Action::Command, &[":"]),          // OK1
            (Action::CommandHistory, &["q :"]), // OK1
            (Action::Jump, &["g z"]),           // OK1
            (Action::QuickCd, &["g d"]),        // OK1
            // File operations. f stands for mc's Ctrl+x, whose letters it keeps for mc's commands:
            // c chmod, o chown, e chattr, l, s, and v links, j the jobs. mc has no checksums, so
            // they take #, for a hash.
            (Action::View, &["i", "f3"]),                // OK1
            (Action::Edit, &["e", "f4"]),                // OK1
            (Action::Copy, &["shift-y", "y y", "f5"]),   // OK1
            (Action::Move, &["x", "f6"]),                // OK1
            (Action::Rename, &["r", "shift-f6", "f16"]), // OK1
            (Action::Mkdir, &["shift-a", "f7"]),         // OK1
            (Action::Delete, &["d shift-d", "f8"]),      // OK1
            (Action::Checksum, &["f #"]),                // OK1
            (Action::Jobs, &["f j"]),                    // OK1
            // Other
            (Action::Help, &["g ?", "f1"]),              // OK1
            (Action::Quit, &["shift-z shift-z", "f10"]), // OK1
            (Action::Redraw, &["ctrl-l"]),               // OK1
        ],
    ),
    // Quick search in the active panel; other keys go to the panel.
    (
        Context::QuickSearch,
        &[
            (Action::Backspace, &["backspace", "ctrl-h"]),
            // Enter ends the search on the match, as in vim, rather than opening the entry.
            (Action::Cancel, &["esc", "ctrl-c", "enter"]),
            // vim's Ctrl+g and Ctrl+t, to the next and the previous match while a pattern is
            // typed.
            (Action::QuickSearch, &["ctrl-g"]),
            (Action::PrevMatch, &["ctrl-t"]),
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
            (Action::Confirm, &["ctrl-y", "enter"]),
            (Action::Cancel, &["esc", "ctrl-c"]),
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
            (Action::PageDown, &["ctrl-f", "shift-down", "pagedown"]),
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
    // What the windows that list items to choose from share: the location menu, the tab menu, the
    // zoxide window, the workspaces, and the command history fall back to it.
    (
        Context::List,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::PageUp, &["ctrl-b", "shift-up", "pageup"]),
            (Action::PageDown, &["ctrl-f", "shift-down", "pagedown"]),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::Confirm, &["ctrl-y", "enter"]),
            (Action::Backspace, &["backspace", "ctrl-h"]),
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
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
    // The tab menu of a panel, on top of the list's; characters filter it.
    (Context::TabMenu, &[]),
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
            (Action::End, &["ctrl-f", "shift-down", "end", "pagedown"]),
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
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::PageUp, &["pageup"]),
            (Action::PageDown, &["pagedown"]),
            (Action::Home, &["home"]),
            (Action::End, &["end"]),
            (Action::Complete, &["tab"]),
            (Action::Confirm, &["ctrl-y", "enter"]),
            (Action::Cancel, &["esc", "ctrl-c"]),
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
            (Action::Confirm, &["ctrl-y", "enter"]),
            (Action::Cancel, &["esc", "ctrl-c", "f10"]),
        ],
    ),
    // The window of the command history, on top of the list's; characters filter it, and Tab
    // switches between the panel's host and all hosts.
    (
        Context::CommandHistory,
        &[
            (Action::NextField, &["tab"]),
            (Action::Delete, &["delete", "ctrl-backspace", "f8"]),
        ],
    ),
    // The terminal's own screen, with the output of commands.
    (
        Context::UserScreen,
        &[(Action::Cancel, &["esc", "ctrl-`", "ctrl-space"])],
    ),
];
