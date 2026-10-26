//! The keymap: vim's modes, with the F-keys of Midnight Commander.
//!
//! Each mode and context with its actions, and each action with its key sequences in crokey's
//! syntax, the keys of a sequence apart; the help lists them in this order, and the pull-down
//! menu shows an action's first key. The keys of `Global` are the same in every mode.
//!
//! The keys are those of a terminal that speaks the kitty keyboard protocol, where Ctrl+i
//! arrives apart from Tab; elsewhere the keymap gives Tab what Ctrl+i does (`Keyboard`).

use super::{Action, Bound, Context, Mode, Preset};

/// The keys of a field that searches, in Normal and Insert mode alike.
const SEARCH_INPUT: Bound = &[
    (Action::SearchMode, &["ctrl-s ctrl-s", "ctrl-s s"]),
    (Action::SearchPlain, &["ctrl-s p"]),
    (Action::SearchFzf, &["ctrl-s f"]),
    (Action::SearchRegex, &["ctrl-s r"]),
    (Action::SearchGlob, &["ctrl-s g"]),
    (Action::CaseMode, &["ctrl-s c"]),
    (Action::CaseSmart, &["ctrl-s a"]),
    (Action::CaseIgnore, &["ctrl-s i"]),
    (Action::CaseMatch, &["ctrl-s m"]),
];

pub(super) const PRESET: Preset = &[
    // Everywhere but the output of commands: the other contexts fall back to it last. `"`
    // names a register wherever keys are commands, as in vim, so that the name is never one;
    // in Insert mode it is text.
    (
        Mode::Normal,
        Context::Global,
        &[
            (Action::KeyHints, &["ctrl-k"]),
            (Action::Register, &["\""]),
            (Action::UserScreen, &["g <"]),
            (Action::Redraw, &["ctrl-l"]),
        ],
    ),
    // The panels.
    (
        Mode::Normal,
        Context::Panel,
        &[
            // Navigation
            (Action::Up, &["k", "up", "ctrl-p"]),
            (Action::Down, &["j", "down", "ctrl-n", "ctrl-j"]),
            (Action::PageUp, &["ctrl-b", "pageup"]),
            (Action::PageDown, &["ctrl-f", "pagedown"]),
            (Action::HalfPageUp, &["ctrl-u"]),
            (Action::HalfPageDown, &["ctrl-d"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::Enter, &["l", "right", "enter", "ctrl-m"]),
            (Action::Parent, &["h", "left", "backspace", "ctrl-h"]),
            (Action::JumpBack, &["ctrl-o"]),
            (Action::JumpForward, &["ctrl-i"]),
            (Action::Reload, &["shift-r"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c"]),
            // The pull-down menu
            (Action::PullDown, &["g m", "f9"]),
            (Action::PullDownLast, &["g shift-m", "shift-f9"]),
            // Panels, as vim's windows; Tab too, as in mc, where the terminal tells it from
            // Ctrl+i.
            (
                Action::SwitchPanel,
                &["ctrl-w w", "ctrl-w ctrl-w", "ctrl-w p", "tab"],
            ),
            (Action::FocusLeft, &["ctrl-w h"]),
            (Action::FocusRight, &["ctrl-w l"]),
            (Action::SwapPanels, &["ctrl-w x", "ctrl-w r"]),
            (Action::OtherPanelOpen, &["ctrl-w f"]),
            (Action::OtherPanelSync, &["ctrl-w v"]),
            (Action::LocationMenuLeft, &["ctrl-w 1"]),
            (Action::LocationMenuRight, &["ctrl-w 2"]),
            // Tabs & Workspaces
            (Action::NewTab, &["t n", "g n"]),
            (Action::CloseTab, &["t c", "g c"]),
            (Action::NextTab, &["g t", "] t"]),
            (Action::PrevTab, &["g shift-t", "[ t"]),
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
            (Action::VisualMode, &["v"]),
            (Action::VisualLineMode, &["shift-v"]),
            (Action::Reselect, &["g v"]),
            (Action::Mark, &["space", "shift-down"]),
            (Action::MarkUp, &["shift-up"]),
            // Names that do not fit: scrolled as vim scrolls lines that do not wrap, and shown
            // whole under the panels, as vim's `K` shows what is under the cursor.
            (Action::ScrollLeft, &["z h", "z left"]),
            (Action::ScrollRight, &["z l", "z right"]),
            (Action::ScrollHalfLeft, &["z shift-h"]),
            (Action::ScrollHalfRight, &["z shift-l"]),
            (Action::ScrollStart, &["z s"]),
            (Action::ScrollEnd, &["z e"]),
            (Action::ShowFull, &["shift-k"]),
            // mc's `+`, `-`, and `*`, under `g`, leaving vim's keys to vim.
            (Action::Select, &["g +"]),
            (Action::Unselect, &["g -"]),
            (Action::InvertMarks, &["g *"]),
            // Search
            (Action::QuickSearch, &["/"]),
            (Action::QuickSearchBack, &["?"]),
            (Action::NextMatch, &["n", "shift-f7"]),
            (Action::PrevMatch, &["shift-n"]),
            // In the group of f, the files: f / for text, as / searches, and f f for names, as
            // telescope's <leader>ff finds files.
            (Action::FindFile, &["f /"]),
            (Action::FindNames, &["f f"]),
            // Tools. i starts Insert mode where there is text to type: on the command line, as
            // typing in mc's panels goes into its command line.
            (Action::Shell, &["i", "!"]),
            (Action::Command, &[":"]),
            (Action::CommandHistory, &["q :"]),
            (Action::Jump, &["g z"]),
            (Action::GoToDir, &["g d"]),
            (Action::View, &["g f", "f3"]),
            (Action::Edit, &["e", "f4"]),
            (Action::SystemOpen, &["g x"]),
            (Action::Copy, &["shift-y", "y y", "f5"]),
            // As vim's `y y p` puts a copy of the line under it.
            (Action::Duplicate, &["y p", "shift-f5"]),
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
    // Rows selected in a panel, from where Visual mode started to the cursor. The commands of
    // files act on them, as on marked entries; Space marks them. The other F-keys, and Ctrl+w
    // to another panel, end Visual mode and do what they do in Normal mode.
    (
        Mode::Visual,
        Context::Panel,
        &[
            (Action::Up, &["k", "up", "ctrl-p"]),
            (Action::Down, &["j", "down", "ctrl-n", "ctrl-j"]),
            (Action::PageUp, &["ctrl-b", "pageup"]),
            (Action::PageDown, &["ctrl-f", "pagedown"]),
            (Action::HalfPageUp, &["ctrl-u"]),
            (Action::HalfPageDown, &["ctrl-d"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::OtherEnd, &["o"]),
            (Action::Mark, &["space"]),
            (Action::Copy, &["y", "f5"]),
            (Action::Move, &["x", "f6"]),
            // As vim's `D` and `X`, which delete the lines selected.
            (Action::Delete, &["d", "shift-d", "shift-x", "f8"]),
            (Action::Checksum, &["f #"]),
            (Action::SystemOpen, &["g x"]),
            (Action::NormalMode, &["esc", "ctrl-[", "ctrl-c"]),
            // The key that started Visual mode ends it; the other switches its kind, as vim's.
            (Action::VisualMode, &["v"]),
            (Action::VisualLineMode, &["shift-v"]),
            (Action::Help, &["f1"]),
            (Action::View, &["f3"]),
            (Action::Edit, &["f4"]),
            (Action::Duplicate, &["shift-f5"]),
            (Action::Rename, &["shift-f6"]),
            (Action::Mkdir, &["f7"]),
            (Action::NextMatch, &["shift-f7"]),
            (Action::PullDown, &["f9"]),
            (Action::PullDownLast, &["shift-f9"]),
            (Action::Quit, &["f10"]),
            (
                Action::SwitchPanel,
                &["ctrl-w w", "ctrl-w ctrl-w", "ctrl-w p", "tab"],
            ),
            (Action::FocusLeft, &["ctrl-w h"]),
            (Action::FocusRight, &["ctrl-w l"]),
        ],
    ),
    // What the text fields share in Insert mode: quick search, the field of an entry renamed in
    // its row, the text fields of dialogs, the command line, and the filters of the lists fall
    // back to it. Esc goes to Normal mode where a field has one.
    (
        Mode::Insert,
        Context::Input,
        &[
            (Action::Left, &["left"]),
            (Action::Right, &["right"]),
            (Action::TextStart, &["home"]),
            (Action::TextEnd, &["end"]),
            (Action::WordBackward, &["shift-left", "ctrl-left"]),
            (Action::WordForward, &["shift-right", "ctrl-right"]),
            (Action::Backspace, &["backspace", "ctrl-h"]),
            (Action::DeleteChar, &["delete"]),
            (Action::DeleteWord, &["ctrl-w"]),
            (Action::DeleteToStart, &["ctrl-u"]),
            (Action::PasteRegister, &["ctrl-r"]),
            (Action::InsertLiteral, &["ctrl-v"]),
            (Action::NormalMode, &["esc", "ctrl-["]),
        ],
    ),
    // What the text fields share in Normal mode: vim's motions, and its commands that change
    // the text. A count goes before them, and a register after `"`, which `global` binds.
    (
        Mode::Normal,
        Context::Input,
        &[
            // Motions
            (Action::Left, &["h", "left"]),
            (Action::Right, &["l", "right"]),
            (Action::CharBackward, &["backspace", "ctrl-h"]),
            (Action::CharForward, &["space"]),
            (Action::TextStart, &["0", "home"]),
            (Action::FirstNonBlank, &["^"]),
            (Action::TextEnd, &["$", "end"]),
            (Action::PrevLine, &["-"]),
            (Action::NextLine, &["+"]),
            (Action::ThisLine, &["_"]),
            (Action::WordForward, &["w", "shift-right"]),
            (Action::WordBackward, &["b", "shift-left"]),
            (Action::WordEnd, &["e"]),
            (Action::BigWordForward, &["shift-w", "ctrl-right"]),
            (Action::BigWordBackward, &["shift-b", "ctrl-left"]),
            (Action::BigWordEnd, &["shift-e"]),
            (Action::FindChar, &["f"]),
            (Action::FindCharBack, &["shift-f"]),
            (Action::TillChar, &["t"]),
            (Action::TillCharBack, &["shift-t"]),
            (Action::RepeatFind, &[";"]),
            (Action::RepeatFindBack, &[","]),
            // Modes
            (Action::InsertMode, &["i", "insert"]),
            (Action::Append, &["a"]),
            (Action::InsertAtStart, &["shift-i"]),
            (Action::AppendAtEnd, &["shift-a"]),
            (Action::VisualMode, &["v"]),
            // Changes
            (Action::DeleteChar, &["x", "delete"]),
            (Action::Backspace, &["shift-x"]),
            (Action::Substitute, &["s"]),
            (Action::SubstituteLine, &["shift-s"]),
            (Action::ChangeToEnd, &["shift-c"]),
            (Action::DeleteToEnd, &["shift-d"]),
            (Action::ReplaceChar, &["r"]),
            (Action::ToggleCase, &["~"]),
            (Action::DeleteOperator, &["d"]),
            (Action::ChangeOperator, &["c"]),
            (Action::YankOperator, &["y"]),
            (Action::PutAfter, &["p"]),
            (Action::PutBefore, &["shift-p"]),
            (Action::Undo, &["u"]),
            (Action::Redo, &["ctrl-r"]),
        ],
    ),
    // Text selected in a field, from where Visual mode started to the cursor.
    (
        Mode::Visual,
        Context::Input,
        &[
            (Action::Left, &["h", "left"]),
            (Action::Right, &["l", "right"]),
            (Action::CharBackward, &["backspace", "ctrl-h"]),
            (Action::CharForward, &["space"]),
            (Action::TextStart, &["0", "home"]),
            (Action::FirstNonBlank, &["^"]),
            (Action::TextEnd, &["$", "end"]),
            (Action::PrevLine, &["-"]),
            (Action::NextLine, &["+"]),
            (Action::ThisLine, &["_"]),
            (Action::WordForward, &["w"]),
            (Action::WordBackward, &["b"]),
            (Action::WordEnd, &["e"]),
            (Action::BigWordForward, &["shift-w"]),
            (Action::BigWordBackward, &["shift-b"]),
            (Action::BigWordEnd, &["shift-e"]),
            (Action::FindChar, &["f"]),
            (Action::FindCharBack, &["shift-f"]),
            (Action::TillChar, &["t"]),
            (Action::TillCharBack, &["shift-t"]),
            (Action::RepeatFind, &[";"]),
            (Action::RepeatFindBack, &[","]),
            (Action::OtherEnd, &["o"]),
            (Action::DeleteOperator, &["d", "x", "delete"]),
            (Action::ChangeOperator, &["c", "s"]),
            (Action::YankOperator, &["y"]),
            (Action::ToggleCase, &["~"]),
            (Action::NormalMode, &["esc", "ctrl-["]),
            // As vim's, `v` ends Visual mode keeping the column for `j` and `k`, where Esc
            // does not.
            (Action::VisualMode, &["v"]),
        ],
    ),
    // Quick search in the active panel, as vim's search is typed: Enter (Ctrl+j) ends it on the
    // match, and Esc and Ctrl+c go back to where it started. Ctrl+g and Ctrl+t go to the next and the
    // previous match, and Ctrl+b and Ctrl+e to the start and the end, as on vim's command line.
    // The arrows and the page keys, and the F-keys, end it and do what they do in the panel,
    // as in mc; Shift+F7 goes to the next match. Ctrl+s and the key after it, which switch how
    // it matches and whether case counts, are the keys of every field that searches
    // (`search_input`).
    (
        Mode::Insert,
        Context::QuickSearch,
        &[
            (Action::Cancel, &["enter", "ctrl-m", "ctrl-j"]),
            (Action::Abort, &["esc", "ctrl-[", "ctrl-c"]),
            (Action::QuickSearch, &["ctrl-g"]),
            (Action::PrevMatch, &["ctrl-t"]),
            (Action::TextStart, &["ctrl-b"]),
            (Action::TextEnd, &["ctrl-e"]),
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::PageUp, &["pageup"]),
            (Action::PageDown, &["pagedown"]),
            (Action::Help, &["f1"]),
            (Action::View, &["f3"]),
            (Action::Edit, &["f4"]),
            (Action::Copy, &["f5"]),
            (Action::Duplicate, &["shift-f5"]),
            (Action::Move, &["f6"]),
            (Action::Rename, &["shift-f6"]),
            (Action::NextMatch, &["shift-f7"]),
            (Action::Mkdir, &["f7"]),
            (Action::Delete, &["f8"]),
            (Action::PullDown, &["f9"]),
            (Action::PullDownLast, &["shift-f9"]),
            (Action::Quit, &["f10"]),
        ],
    ),
    // The name field of an entry renamed in its row. Enter renames in every mode; Esc leaves
    // Insert mode, then leaves the name as it was.
    (
        Mode::Insert,
        Context::Rename,
        &[
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::NormalOnce, &["ctrl-o"]),
            (Action::Cancel, &["ctrl-c"]),
        ],
    ),
    (
        Mode::Normal,
        Context::Rename,
        &[
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c"]),
        ],
    ),
    (
        Mode::Visual,
        Context::Rename,
        &[
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["ctrl-c"]),
        ],
    ),
    // Dialogs whose focus is on a button or a list, and the text fields of dialogs in Normal
    // mode, after the keys of the field.
    (
        Mode::Normal,
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
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c", "q", "f10"]),
        ],
    ),
    // The help of F1, on top of the dialog's, which has j, k, Ctrl+b, Ctrl+f, g g, G, and q:
    // Ctrl+y and Ctrl+e scroll it by a line, and Ctrl+u and Ctrl+d by half a page, as in the
    // viewer; Space, f, and b turn the page, and u and d half of it, as in less. Space presses
    // no button here.
    (
        Mode::Normal,
        Context::Help,
        &[
            (Action::Up, &["ctrl-y"]),
            (Action::Down, &["ctrl-e"]),
            (Action::WindowUp, &["b"]),
            (Action::WindowDown, &["space", "f"]),
            (Action::HalfPageUp, &["ctrl-u", "u"]),
            (Action::HalfPageDown, &["ctrl-d", "d"]),
        ],
    ),
    // The text fields of dialogs: the field's keys, then the dialog's. In Insert mode, Up,
    // Down, Tab, and Shift+Tab still move between the fields and the buttons.
    (
        Mode::Insert,
        Context::DialogInput,
        &[
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::NextField, &["tab", "ctrl-i"]),
            (Action::PrevField, &["backtab"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::NormalOnce, &["ctrl-o"]),
            (Action::Cancel, &["ctrl-c", "f10"]),
        ],
    ),
    (
        Mode::Visual,
        Context::DialogInput,
        &[
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["ctrl-c", "f10"]),
        ],
    ),
    // Path fields: Tab completes, as on vim's command line; Shift+Tab and Down still leave the
    // field.
    (
        Mode::Insert,
        Context::PathInput,
        &[(Action::Complete, &["tab", "ctrl-i"])],
    ),
    // A field that searches, while it has the focus: quick search, the filter of a list while
    // it takes the keys, and such a field of a dialog, as the text of the search of a tree; its
    // keys come before those of where it is (ADR 0045). Ctrl+s starts them, and the key after it
    // says what (ADR 0046): Ctrl+s again or `s` the next way to match, round, or a way by its
    // letter, of those the field has; `c` the next of smart case, case ignored, and case, or one
    // by its letter. vim binds no Ctrl+s.
    (Mode::Normal, Context::SearchInput, SEARCH_INPUT),
    (Mode::Insert, Context::SearchInput, SEARCH_INPUT),
    // The list of completions under a path field or over the command line, as vim's `wildmenu`:
    // Tab, Ctrl+n, and Down put the next match in the field, Shift+Tab, Ctrl+p, and Up the one
    // before, round through what was typed; Left and Right complete in the directory above and
    // in the one chosen, as vim's pop-up menu; Ctrl+y keeps the match, and Ctrl+e puts back
    // what was typed. Other keys close the list and go to the field: Enter confirms with the
    // match in it, Esc goes to Normal mode, or closes the command line, and Ctrl+c closes, as
    // in vim.
    (
        Mode::Insert,
        Context::Completion,
        &[
            (Action::Down, &["tab", "ctrl-i", "ctrl-n", "down"]),
            (Action::Up, &["backtab", "ctrl-p", "up"]),
            (Action::PageDown, &["pagedown"]),
            (Action::PageUp, &["pageup"]),
            (Action::Left, &["left"]),
            (Action::Right, &["right"]),
            (Action::AcceptCompletion, &["ctrl-y"]),
            (Action::CancelCompletion, &["ctrl-e"]),
        ],
    ),
    // The viewer of F3. Space, f, and b turn the page, and u and d half of it, as in less.
    (
        Mode::Normal,
        Context::Viewer,
        &[
            (Action::Up, &["k", "ctrl-y", "up", "ctrl-p"]),
            (
                Action::Down,
                &["j", "ctrl-e", "down", "ctrl-n", "enter", "ctrl-m"],
            ),
            (Action::PageUp, &["ctrl-b", "pageup"]),
            (Action::PageDown, &["ctrl-f", "pagedown"]),
            (Action::WindowUp, &["b"]),
            (Action::WindowDown, &["space", "f"]),
            (Action::HalfPageUp, &["ctrl-u", "u"]),
            (Action::HalfPageDown, &["ctrl-d", "d"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::Left, &["h", "left"]),
            (Action::Right, &["l", "right"]),
            (Action::ScrollLeft, &["z h", "z left"]),
            (Action::ScrollRight, &["z l", "z right"]),
            (Action::ScrollHalfLeft, &["z shift-h"]),
            (Action::ScrollHalfRight, &["z shift-l"]),
            (Action::ScrollStart, &["z s"]),
            (Action::ScrollEnd, &["z e"]),
            (Action::ToggleWrap, &["f2"]),
            (Action::Help, &["g ?", "f1"]),
            (Action::Quit, &["q", "esc", "ctrl-[", "ctrl-c", "f3", "f10"]),
        ],
    ),
    // What the windows that list items to choose from share: the location menu, the tab menu,
    // the zoxide window, the workspaces, the command history, and the list of the key hints
    // fall back to it. They open with the filter taking the keys, in Insert mode, where Esc goes
    // back to the list and `/` or `i` types in the filter again, as `i` starts Insert mode in a
    // text field. While the filter takes the keys, Ctrl+s and the key after it switch how it
    // matches (`search_input`).
    (
        Mode::Normal,
        Context::List,
        &[
            (Action::Up, &["k", "up", "ctrl-p"]),
            (Action::Down, &["j", "down", "ctrl-n", "ctrl-j"]),
            (Action::PageUp, &["ctrl-b", "pageup"]),
            (Action::PageDown, &["ctrl-f", "pagedown"]),
            (Action::HalfPageUp, &["ctrl-u"]),
            (Action::HalfPageDown, &["ctrl-d"]),
            (Action::Home, &["g g", "home"]),
            (Action::End, &["shift-g", "end"]),
            (Action::QuickSearch, &["/", "i"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c", "q", "f10"]),
        ],
    ),
    (
        Mode::Insert,
        Context::List,
        &[
            (Action::Up, &["up", "ctrl-p"]),
            (Action::Down, &["down", "ctrl-n"]),
            (Action::PageUp, &["pageup"]),
            (Action::PageDown, &["pagedown"]),
            (Action::Confirm, &["enter", "ctrl-m", "ctrl-y"]),
            (Action::NormalMode, &["esc", "ctrl-["]),
            (Action::Cancel, &["ctrl-c", "f10"]),
        ],
    ),
    // The location menu, on top of the list's. F4 and F8 edit and disconnect the host under the
    // cursor; d d disconnects it too.
    (
        Mode::Normal,
        Context::LocationMenu,
        &[
            (Action::EditHost, &["f4"]),
            (Action::Disconnect, &["d d", "f8"]),
            (Action::Reload, &["shift-r"]),
        ],
    ),
    (
        Mode::Insert,
        Context::LocationMenu,
        &[(Action::EditHost, &["f4"]), (Action::Disconnect, &["f8"])],
    ),
    // The tab menu of a panel, on top of the list's: d d and F8 close the tab under the cursor.
    (
        Mode::Normal,
        Context::TabMenu,
        &[(Action::CloseTab, &["d d", "f8"])],
    ),
    (
        Mode::Insert,
        Context::TabMenu,
        &[(Action::CloseTab, &["f8"])],
    ),
    // The zoxide window, on top of the list's; its filter takes keywords.
    (Mode::Normal, Context::Jump, &[]),
    // The window of the saved workspaces, on top of the list's: a adds one; r and F6 rename,
    // and d d and F8 delete, as they rename and delete files.
    (
        Mode::Normal,
        Context::Workspaces,
        &[
            (Action::SaveWorkspace, &["a"]),
            (Action::Move, &["r", "f6"]),
            (Action::Delete, &["d d", "f8"]),
        ],
    ),
    (
        Mode::Insert,
        Context::Workspaces,
        &[(Action::Move, &["f6"]), (Action::Delete, &["f8"])],
    ),
    // The pull-down menu. Alt and a letter run the command that has it, so h, j, k, and l
    // stay the motions.
    (
        Mode::Normal,
        Context::PullDown,
        &[
            (Action::Up, &["k", "up", "ctrl-p"]),
            (Action::Down, &["j", "down", "ctrl-n"]),
            (Action::Left, &["h", "left", "backtab"]),
            (Action::Right, &["l", "right", "tab", "ctrl-i"]),
            (Action::Home, &["g g", "home", "ctrl-b", "pageup"]),
            (Action::End, &["shift-g", "end", "ctrl-f", "pagedown"]),
            (Action::Confirm, &["enter", "ctrl-m", "ctrl-y"]),
            (
                Action::Cancel,
                &["esc", "ctrl-[", "ctrl-c", "q", "f9", "f10"],
            ),
        ],
    ),
    // The command line of `i`, `!`, and `:`, as vim's command line: Esc closes it, but leaves
    // Insert mode after Ctrl+f, as in vim's window of the command line; Ctrl+c closes it. Ctrl+j
    // arrives as LF where Enter arrives as CR, so it starts a new line in every terminal;
    // Shift+Enter does where the terminal speaks the kitty keyboard protocol. Ctrl+b and Ctrl+e go
    // to the start and the end of the line, Ctrl+p and Ctrl+n through the history, and Ctrl+f to
    // Normal mode, in place of vim's window of the command line. Tab completes as vim's `:!`
    // does.
    (
        Mode::Insert,
        Context::CommandLine,
        &[
            (Action::Up, &["up"]),
            (Action::Down, &["down"]),
            (Action::TextStart, &["ctrl-b"]),
            (Action::TextEnd, &["ctrl-e"]),
            (Action::NewLine, &["ctrl-j", "shift-enter"]),
            (Action::NormalMode, &["ctrl-f"]),
            (Action::OlderCommand, &["ctrl-p"]),
            (Action::NewerCommand, &["ctrl-n"]),
            (Action::NormalOnce, &["ctrl-o"]),
            (Action::Complete, &["tab", "ctrl-i"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-["]),
            (Action::Abort, &["ctrl-c", "f10"]),
        ],
    ),
    // The command line in Normal mode: j and k go through its lines, then the history, and
    // q : opens the window of the history.
    (
        Mode::Normal,
        Context::CommandLine,
        &[
            (Action::Up, &["k", "up"]),
            (Action::Down, &["j", "down"]),
            (Action::CommandHistory, &["q :"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["esc", "ctrl-[", "ctrl-c", "f10"]),
        ],
    ),
    // In Visual mode, j and k go through its lines only.
    (
        Mode::Visual,
        Context::CommandLine,
        &[
            (Action::Up, &["k", "up"]),
            (Action::Down, &["j", "down"]),
            (Action::Confirm, &["enter", "ctrl-m"]),
            (Action::Cancel, &["ctrl-c", "f10"]),
        ],
    ),
    // The window of the command history, on top of the list's: Tab switches between the
    // panel's host and all hosts, and d d and F8 remove a command.
    (
        Mode::Normal,
        Context::CommandHistory,
        &[
            (Action::NextField, &["tab", "ctrl-i"]),
            (Action::Delete, &["d d", "f8"]),
        ],
    ),
    (
        Mode::Insert,
        Context::CommandHistory,
        &[
            (Action::NextField, &["tab", "ctrl-i"]),
            (Action::Delete, &["f8"]),
        ],
    ),
    // The dialog of the search of a whole tree, on top of the dialogs': F3 and F4 view and edit
    // the file of the row at the line, as in the results of mc's Find File, wherever the focus
    // is. On the rows, Ctrl+u and Ctrl+d move half a page, as in the lists. In the field of the
    // text, Ctrl+s and the key after it switch how it matches (`search_input`).
    (
        Mode::Normal,
        Context::Find,
        &[
            (Action::View, &["f3"]),
            (Action::Edit, &["f4"]),
            (Action::HalfPageUp, &["ctrl-u"]),
            (Action::HalfPageDown, &["ctrl-d"]),
        ],
    ),
    (
        Mode::Normal,
        Context::FindInput,
        &[(Action::View, &["f3"]), (Action::Edit, &["f4"])],
    ),
    (
        Mode::Insert,
        Context::FindInput,
        &[(Action::View, &["f3"]), (Action::Edit, &["f4"])],
    ),
    // The terminal's own screen, with the output of commands. Enter and q go back, as from the
    // prompt under the output that vim's g < shows.
    (
        Mode::Normal,
        Context::UserScreen,
        &[(
            Action::Cancel,
            &["g <", "enter", "ctrl-m", "q", "esc", "ctrl-[", "ctrl-c"],
        )],
    ),
];
