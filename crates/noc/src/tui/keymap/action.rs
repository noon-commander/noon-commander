//! What keys do: actions, the contexts they are bound in, and vim's modes.

use noc_scroll::Motion;

/// Which of vim's modes keys are typed in. The app says which from what has the keys: a text
/// field in Insert mode takes characters as text, the panels and the lists take them as
/// commands, and a selection takes the keys that act on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Mode {
    /// Keys are commands: the panels, the lists, the buttons of dialogs, and a text field
    /// after Esc.
    Normal,
    /// Characters are text: a text field, quick search, the command line, a filter.
    Insert,
    /// Keys extend a selection, and commands act on it: rows of a panel, or text of a field.
    Visual,
}

/// Where a key is pressed. Keys are looked up in the context's [chain](Self::chain), in the
/// mode they are typed in; the first context there that knows a key sequence, as a binding or
/// as the start of one, decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Context {
    /// Keys that do the same wherever they are pressed, in every mode: every context but the
    /// user screen falls back to it last. Keys never go to it alone.
    Global,
    /// A panel that lists a directory.
    Panel,
    /// Keys that the windows listing items to choose from share: the menus, the zoxide window,
    /// the workspaces, and the command history fall back to it. Keys never go to it alone.
    List,
    /// The location menu of Alt+F1 and Alt+F2; falls back to `List`, and, while its filter
    /// takes text, to `Input`.
    LocationMenu,
    /// The tab menu of a panel; falls back as the location menu does.
    TabMenu,
    /// The zoxide window, which takes keywords; falls back as the location menu does.
    Jump,
    /// The window of the saved workspaces; falls back as the location menu does.
    Workspaces,
    /// The pull-down menu of F9, whose commands have letters.
    PullDown,
    /// Keys that the text fields share, which edit the text: quick search, the field of an
    /// entry renamed in its row, the text fields of dialogs, the command line, and the filters
    /// of the windows that list items fall back to it. Keys never go to it alone.
    Input,
    /// Quick search in the active panel; falls back to `Input`.
    QuickSearch,
    /// The name field of the entry being renamed in its panel row; falls back to `Input`. Other
    /// keys, but the global ones, do nothing.
    Rename,
    /// A dialog whose focus is on a button, a check box, or a list.
    Dialog,
    /// The help of F1; falls back to `Dialog`, whose keys scroll it and close it.
    Help,
    /// A dialog whose focus is on a text field; falls back to `Input`, then to `Dialog`.
    DialogInput,
    /// A text field for a path, which Tab completes; falls back to `DialogInput`.
    PathInput,
    /// The list of completions under a path field, or over the command line; keys it does not
    /// bind go to the field.
    Completion,
    /// The viewer of F3.
    Viewer,
    /// The command line of `!` and `:`; falls back to `Input`. Other keys, but the global ones,
    /// do nothing.
    CommandLine,
    /// The list of completions over the command line: the keys of `Completion`, then those of
    /// the command line. It binds none of its own.
    CommandCompletion,
    /// The window of the command history; falls back as the location menu does.
    CommandHistory,
    /// The dialog of the search of a whole tree, its focus on a check box, the rows, or a
    /// button; falls back to `Dialog`.
    Find,
    /// A field of the dialog of the search of a whole tree; falls back as the text fields of
    /// dialogs do.
    FindInput,
    /// The key hints with the keys, after a second Ctrl+k: every key of what is under them, to
    /// filter and run; falls back as the location menu does.
    KeyHints,
    /// The terminal's own screen, with the output of commands, in place of the panels; only
    /// the keys that go back to the panels do anything.
    UserScreen,
}

impl Context {
    /// This context and its fallbacks in `mode`, most specific first.
    pub(crate) fn chain(self, mode: Mode) -> &'static [Self] {
        // The lists edit their filters with the keys of `Input` only while those take text: in
        // Normal mode, `Input` has the motions of a text field.
        let filtering = mode == Mode::Insert;
        match self {
            Self::Global => &[Self::Global],
            Self::Panel => &[Self::Panel, Self::Global],
            // Text fields edit their text with the keys of `Input` before anything else does.
            Self::Input => &[Self::Input, Self::Global],
            Self::QuickSearch => &[Self::QuickSearch, Self::Input, Self::Global],
            Self::Rename => &[Self::Rename, Self::Input, Self::Global],
            // Dialogs are modal: panel keys do nothing while one is open.
            Self::Dialog => &[Self::Dialog, Self::Global],
            Self::Help => &[Self::Help, Self::Dialog, Self::Global],
            Self::DialogInput => &[Self::DialogInput, Self::Input, Self::Dialog, Self::Global],
            Self::PathInput => &[
                Self::PathInput,
                Self::DialogInput,
                Self::Input,
                Self::Dialog,
                Self::Global,
            ],
            Self::Completion => &[
                Self::Completion,
                Self::PathInput,
                Self::DialogInput,
                Self::Input,
                Self::Dialog,
                Self::Global,
            ],
            Self::Viewer => &[Self::Viewer, Self::Global],
            Self::CommandLine => &[Self::CommandLine, Self::Input, Self::Global],
            Self::CommandCompletion => &[
                Self::CommandCompletion,
                Self::Completion,
                Self::CommandLine,
                Self::Input,
                Self::Global,
            ],
            // The terminal's own screen: nothing of noc's draws there, the hints neither.
            Self::UserScreen => &[Self::UserScreen],
            // Menus are modal too.
            Self::List => &[Self::List, Self::Global],
            Self::LocationMenu if filtering => {
                &[Self::LocationMenu, Self::List, Self::Input, Self::Global]
            }
            Self::LocationMenu => &[Self::LocationMenu, Self::List, Self::Global],
            Self::TabMenu if filtering => &[Self::TabMenu, Self::List, Self::Input, Self::Global],
            Self::TabMenu => &[Self::TabMenu, Self::List, Self::Global],
            Self::Jump if filtering => &[Self::Jump, Self::List, Self::Input, Self::Global],
            Self::Jump => &[Self::Jump, Self::List, Self::Global],
            Self::Workspaces if filtering => {
                &[Self::Workspaces, Self::List, Self::Input, Self::Global]
            }
            Self::Workspaces => &[Self::Workspaces, Self::List, Self::Global],
            Self::CommandHistory if filtering => {
                &[Self::CommandHistory, Self::List, Self::Input, Self::Global]
            }
            Self::CommandHistory => &[Self::CommandHistory, Self::List, Self::Global],
            Self::Find => &[Self::Find, Self::Dialog, Self::Global],
            Self::FindInput => &[
                Self::FindInput,
                Self::DialogInput,
                Self::Input,
                Self::Dialog,
                Self::Global,
            ],
            Self::KeyHints if filtering => &[Self::KeyHints, Self::List, Self::Input, Self::Global],
            Self::KeyHints => &[Self::KeyHints, Self::List, Self::Global],
            Self::PullDown => &[Self::PullDown, Self::Global],
        }
    }

    /// Whether, in Normal mode, Alt and a character that nothing binds stand for Esc and the
    /// character, which is how an Esc typed quickly before a key arrives. In Insert and Visual
    /// mode they do in every context, as in vim in a terminal, but on a password.
    pub(crate) fn alt_stands_for_esc(self) -> bool {
        matches!(self, Self::Panel)
    }

    /// Whether Alt and a letter run the command of the menu that has that letter: in the
    /// pull-down menu, whose plain letters move as `h`, `j`, `k`, and `l` do.
    pub(crate) fn alt_letters_are_hotkeys(self) -> bool {
        matches!(self, Self::PullDown)
    }
}

/// Something a key does. The context gives the meaning: `Up` moves the cursor in a panel and
/// the focus in a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    /// One row up, or the focus to the previous item.
    Up,
    /// One row down, or the focus to the next item.
    Down,
    /// The text cursor one character left, or the focus to the previous button.
    Left,
    /// The text cursor one character right, or the focus to the next button.
    Right,
    /// The text cursor one character right, and from the end of a line to the next one: vim's
    /// Space.
    CharForward,
    /// The text cursor one character left, and from the start of a line to the end of the one
    /// before: vim's Backspace.
    CharBackward,
    /// One page up; in the viewer and the help, a window less two rows, as vim's Ctrl+b.
    PageUp,
    /// One page down; in the viewer and the help, a window less two rows, as vim's Ctrl+f.
    PageDown,
    /// A whole window up in the viewer and the help, as less's `b`.
    WindowUp,
    /// A whole window down in the viewer and the help, as less's Space and `f`.
    WindowDown,
    /// Half a page up.
    HalfPageUp,
    /// Half a page down.
    HalfPageDown,
    /// The first row; with a count, that row.
    Home,
    /// The last row; with a count, that row.
    End,
    /// Opens the directory under the cursor.
    Enter,
    /// Marks the entry under the cursor, or unmarks it, and moves down; in Visual mode, marks
    /// the rows selected.
    Mark,
    /// Marks the entry under the cursor, or unmarks it, and moves up.
    MarkUp,
    /// Marks the files that are not marked and unmarks those that are; directories stay as
    /// they are.
    InvertMarks,
    /// Views the file under the cursor; opens a directory.
    View,
    /// Wraps long lines in the viewer, or cuts them.
    ToggleWrap,
    /// The names of a panel, or the lines of the viewer that are cut, a column left, as vim's
    /// `zh`; with a count, that many.
    ScrollLeft,
    /// A column right, as vim's `zl`; with a count, that many.
    ScrollRight,
    /// Half their width left, as vim's `zH`.
    ScrollHalfLeft,
    /// Half their width right, as vim's `zL`.
    ScrollHalfRight,
    /// Back to their start, as vim's `zs` with the cursor at the start of the line.
    ScrollStart,
    /// The end of the name under the cursor, or of the longest line on screen, at the right
    /// edge, as vim's `ze`.
    ScrollEnd,
    /// Shows the whole name under the cursor of the panels, and the whole row under the cursor
    /// of the lists, until it is asked again.
    ShowFull,
    /// Edits the file under the cursor in the editor of `$VISUAL` or `$EDITOR`.
    Edit,
    /// Opens the marked entries, or the one under the cursor, with the programs the desktop has
    /// for them (`open`, `xdg-open`), as vim's `gx`; local ones only.
    SystemOpen,
    /// Asks where to, and copies the marked entries or the one under the cursor.
    Copy,
    /// Asks where to, and moves or renames the marked entries or the one under the cursor.
    Move,
    /// Renames the entry under the cursor in its row.
    Rename,
    /// Asks for a name and makes a directory.
    Mkdir,
    /// Deletes the marked entries or the one under the cursor; in a window that lists items,
    /// the item under the cursor. Text fields delete a character with `DeleteChar`, so that a
    /// window whose filter falls back to them tells the two apart.
    Delete,
    /// Asks for an algorithm, and computes the checksums of the marked files or the one under
    /// the cursor, with the files in marked directories.
    Checksum,
    /// Lists the running jobs.
    Jobs,
    /// Asks for a pattern and marks the entries whose names match it.
    Select,
    /// Asks for a pattern and unmarks the entries whose names match it.
    Unselect,
    /// Opens the parent directory; nothing in a `/`.
    Parent,
    /// Makes the other panel active.
    SwitchPanel,
    /// Makes the panel drawn on the left active, wherever the panels were swapped.
    FocusLeft,
    /// Makes the panel drawn on the right active.
    FocusRight,
    /// Swaps the two panels.
    SwapPanels,
    /// Opens the directory under the cursor in the other panel.
    OtherPanelOpen,
    /// Shows this panel's directory in the other panel.
    OtherPanelSync,
    /// Opens the location menu of the left panel: volumes and hosts.
    LocationMenuLeft,
    /// Opens the location menu of the right panel.
    LocationMenuRight,
    /// Opens the zoxide window, to jump to a directory zoxide ranks.
    Jump,
    /// Asks for a path, as `cd` in a shell takes it, and opens it in the panel.
    QuickCd,
    /// Opens the pull-down menu, at the menu of the active panel.
    PullDown,
    /// Opens the pull-down menu on the command of it that ran last.
    PullDownLast,
    /// Opens a new tab in the panel, on the same location.
    NewTab,
    /// Closes the panel's tab; the last one stays.
    CloseTab,
    /// Shows the panel's next tab, round; with a count, the tab of that number.
    NextTab,
    /// Shows the panel's previous tab, round.
    PrevTab,
    /// Lists the left panel's tabs to choose one.
    TabListLeft,
    /// Lists the right panel's tabs to choose one.
    TabListRight,
    /// Asks for a name and saves the tabs of both panels as a workspace.
    SaveWorkspace,
    /// Opens the window of the saved workspaces.
    Workspaces,
    /// Reads the directory, or the volumes and hosts, again.
    Reload,
    /// Shows or hides files whose names start with a dot, in both panels.
    ToggleHidden,
    /// Sorts by name; once more: reverses the order. Likewise for the other sort actions.
    SortByName,
    /// Sorts by extension, then name.
    SortByExtension,
    /// Sorts by modification time.
    SortByTime,
    /// Sorts by size.
    SortBySize,
    /// Starts quick search, or jumps to the next match; with nothing typed yet, searches for
    /// what the last search found. In a window that lists items, types in its filter.
    QuickSearch,
    /// Starts quick search up the rows, as vim's `?`; `n` then goes up too, and `N` down.
    QuickSearchBack,
    /// Moves to the next name that matches quick search: what is typed while it runs, or else
    /// what the last search found.
    NextMatch,
    /// Moves to the previous name that matches quick search, likewise.
    PrevMatch,
    /// Searches the files under the panel's directory for text, in a window.
    FindFile,
    /// In quick search and the filters of the lists, matches what is typed the next way: as
    /// it is, as fzf does, or as a regular expression, round; in the search of a tree, as it is
    /// or as a regular expression.
    SearchMode,
    /// In the search of a tree, matches case, or ignores it unless the text has a capital
    /// letter.
    MatchCase,
    /// Goes back to where the panel was before its last jump, as vim's Ctrl+o.
    JumpBack,
    /// Goes forward again to where a jump back came from, as vim's Ctrl+i.
    JumpForward,
    /// Opens the command line for a shell command.
    Shell,
    /// Opens the command line for a command of Noon Commander, of which `!` runs a shell
    /// command.
    Command,
    /// Starts a new line in the command.
    NewLine,
    /// The command before in the history of the panel's host.
    OlderCommand,
    /// The command after in the history of the panel's host.
    NewerCommand,
    /// Opens the window of the command history.
    CommandHistory,
    /// Shows the terminal's own screen, with the output of commands, in place of the panels and
    /// whatever is in front of them, which stays as it was.
    UserScreen,
    /// Closes the connection to the host under the cursor, or stops connecting to it.
    Disconnect,
    /// Edits the settings of the host under the cursor.
    EditHost,
    /// Leaves Insert or Visual mode for Normal mode; in a window that lists items, leaves its
    /// filter.
    NormalMode,
    /// Insert mode, before the cursor: vim's `i`.
    InsertMode,
    /// Insert mode, after the cursor: vim's `a`.
    Append,
    /// Insert mode, before the first character that is not blank: vim's `I`.
    InsertAtStart,
    /// Insert mode, at the end of the line: vim's `A`.
    AppendAtEnd,
    /// Visual mode, which selects from the cursor to where it goes; again: Normal mode.
    VisualMode,
    /// Visual mode by lines, as vim's `V`: in the panels, where rows are lines either way, the
    /// mode line says so; `v` in it and `V` in `v`'s switch the kind, as in vim.
    VisualLineMode,
    /// Puts the cursor at the other end of the selection.
    OtherEnd,
    /// Selects again the rows selected last.
    Reselect,
    /// Runs one command of Normal mode, then goes back to Insert mode: vim's Ctrl+o.
    NormalOnce,
    /// The text cursor to the start of the line.
    TextStart,
    /// The text cursor to the first character of the line that is not blank.
    FirstNonBlank,
    /// The text cursor to the end of the line.
    TextEnd,
    /// The text cursor to the first character that is not blank of the line above, or of the
    /// line a count goes up to: vim's `-`. An operator takes the lines whole.
    PrevLine,
    /// The same, of the line below, or the line a count goes down to: vim's `+`.
    NextLine,
    /// The same, of the line of the cursor, or of the line a count less one goes down to: vim's
    /// `_`, with which `d _` deletes the line as `d d`.
    ThisLine,
    /// The text cursor to the start of the next word: vim's `w`.
    WordForward,
    /// The text cursor to the start of the word before it: vim's `b`.
    WordBackward,
    /// The text cursor to the end of the word: vim's `e`.
    WordEnd,
    /// As `WordForward`, with words only apart where blanks are: vim's `W`.
    BigWordForward,
    /// As `WordBackward`, likewise: vim's `B`.
    BigWordBackward,
    /// As `WordEnd`, likewise: vim's `E`.
    BigWordEnd,
    /// The text cursor to the next character typed after it in the line: vim's `f`.
    FindChar,
    /// The text cursor back to the character typed after it: vim's `F`.
    FindCharBack,
    /// The text cursor to right before the character typed after it: vim's `t`.
    TillChar,
    /// The text cursor back to right after the character typed after it: vim's `T`.
    TillCharBack,
    /// Finds the character of the last `f`, `t`, `F`, or `T` again: vim's `;`.
    RepeatFind,
    /// The same, the other way: vim's `,`.
    RepeatFindBack,
    /// Deletes the character before the text cursor.
    Backspace,
    /// Deletes the character at the text cursor.
    DeleteChar,
    /// Deletes from the start of the line to the cursor.
    DeleteToStart,
    /// Deletes from the cursor to the end of the line.
    DeleteToEnd,
    /// Deletes the word before the text cursor, as vim's Ctrl+w.
    DeleteWord,
    /// Deletes what the motion after it moves over, or the line for `d d`: vim's `d`. In
    /// Visual mode, deletes what is selected.
    DeleteOperator,
    /// Deletes as `DeleteOperator` does, then Insert mode: vim's `c`.
    ChangeOperator,
    /// Copies what the motion after it moves over to a register: vim's `y`.
    YankOperator,
    /// Deletes the character at the cursor, then Insert mode: vim's `s`.
    Substitute,
    /// Deletes the text of the line, then Insert mode: vim's `S`.
    SubstituteLine,
    /// Deletes to the end of the line, then Insert mode: vim's `C`.
    ChangeToEnd,
    /// Replaces the character at the cursor with the one typed after it: vim's `r`.
    ReplaceChar,
    /// Switches the case of the character at the cursor, and moves on: vim's `~`.
    ToggleCase,
    /// Puts the text of a register after the cursor: vim's `p`.
    PutAfter,
    /// Puts the text of a register before the cursor: vim's `P`.
    PutBefore,
    /// Undoes the last change of the text.
    Undo,
    /// Does again what Undo undid.
    Redo,
    /// Names the register for the next command with the character typed after it: vim's `"`.
    Register,
    /// Puts the text of the register named by the character typed after it at the cursor: vim's
    /// Ctrl+r in Insert mode.
    PasteRegister,
    /// Types the key after it as it is: vim's Ctrl+v.
    InsertLiteral,
    /// Completes the word before the cursor, in a path field or on the command line: one match
    /// goes in, or the first of several, which a list shows.
    Complete,
    /// Keeps the match in the field and closes the list of completions: vim's Ctrl+y.
    AcceptCompletion,
    /// Closes the list of completions and puts back what was typed: vim's Ctrl+e.
    CancelCompletion,
    /// Accepts the dialog, or activates the focused button.
    Confirm,
    /// Switches the focused check box, or presses the focused button.
    Toggle,
    /// Closes the dialog, ends quick search, or stops a pending listing or connection.
    Cancel,
    /// Ends quick search and puts the cursor back where the search started, as Esc does while
    /// vim's search is typed.
    Abort,
    /// Moves the focus to the next field or button.
    NextField,
    /// Moves the focus to the previous field or button.
    PrevField,
    /// Shows the key bindings.
    Help,
    /// Shows the keys that can be typed now and what they do, as which-key does: the next key
    /// typed runs as it would have, or shows the keys that can follow it. Again while they
    /// show, it gives them the keys, as a list to filter and run from.
    KeyHints,
    /// Quits Noon Commander.
    Quit,
    /// Redraws the whole screen.
    Redraw,
}

impl Action {
    /// Whether the action takes the character typed after it, as `f` and `r` do.
    pub(crate) fn takes_char(self) -> bool {
        matches!(
            self,
            Self::FindChar
                | Self::FindCharBack
                | Self::TillChar
                | Self::TillCharBack
                | Self::ReplaceChar
                | Self::Register
                | Self::PasteRegister
                | Self::InsertLiteral
        )
    }

    /// How the action moves a page of text, the help's and the viewer's, if it does.
    pub(crate) fn motion(self) -> Option<Motion> {
        Some(match self {
            Self::Up => Motion::Up,
            Self::Down => Motion::Down,
            Self::PageUp => Motion::PageUp,
            Self::PageDown => Motion::PageDown,
            Self::WindowUp => Motion::WindowUp,
            Self::WindowDown => Motion::WindowDown,
            Self::HalfPageUp => Motion::HalfPageUp,
            Self::HalfPageDown => Motion::HalfPageDown,
            Self::Home => Motion::Home,
            Self::End => Motion::End,
            _ => return None,
        })
    }
}

#[cfg(test)]
impl Mode {
    /// Every mode.
    pub(crate) const ALL: &'static [Self] = &[Self::Normal, Self::Insert, Self::Visual];
}

#[cfg(test)]
impl Action {
    /// Every action, in declaration order.
    pub(crate) const ALL: &'static [Self] = &[
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::CharForward,
        Self::CharBackward,
        Self::PageUp,
        Self::PageDown,
        Self::WindowUp,
        Self::WindowDown,
        Self::HalfPageUp,
        Self::HalfPageDown,
        Self::Home,
        Self::End,
        Self::Enter,
        Self::Mark,
        Self::MarkUp,
        Self::InvertMarks,
        Self::View,
        Self::ToggleWrap,
        Self::ScrollLeft,
        Self::ScrollRight,
        Self::ScrollHalfLeft,
        Self::ScrollHalfRight,
        Self::ScrollStart,
        Self::ScrollEnd,
        Self::ShowFull,
        Self::Edit,
        Self::SystemOpen,
        Self::Copy,
        Self::Move,
        Self::Rename,
        Self::Mkdir,
        Self::Delete,
        Self::Checksum,
        Self::Jobs,
        Self::Select,
        Self::Unselect,
        Self::Parent,
        Self::SwitchPanel,
        Self::FocusLeft,
        Self::FocusRight,
        Self::SwapPanels,
        Self::OtherPanelOpen,
        Self::OtherPanelSync,
        Self::LocationMenuLeft,
        Self::LocationMenuRight,
        Self::Jump,
        Self::QuickCd,
        Self::PullDown,
        Self::PullDownLast,
        Self::NewTab,
        Self::CloseTab,
        Self::NextTab,
        Self::PrevTab,
        Self::TabListLeft,
        Self::TabListRight,
        Self::SaveWorkspace,
        Self::Workspaces,
        Self::Reload,
        Self::ToggleHidden,
        Self::SortByName,
        Self::SortByExtension,
        Self::SortByTime,
        Self::SortBySize,
        Self::QuickSearch,
        Self::QuickSearchBack,
        Self::NextMatch,
        Self::PrevMatch,
        Self::FindFile,
        Self::SearchMode,
        Self::MatchCase,
        Self::JumpBack,
        Self::JumpForward,
        Self::Shell,
        Self::Command,
        Self::NewLine,
        Self::OlderCommand,
        Self::NewerCommand,
        Self::CommandHistory,
        Self::UserScreen,
        Self::Disconnect,
        Self::EditHost,
        Self::NormalMode,
        Self::InsertMode,
        Self::Append,
        Self::InsertAtStart,
        Self::AppendAtEnd,
        Self::VisualMode,
        Self::VisualLineMode,
        Self::OtherEnd,
        Self::Reselect,
        Self::NormalOnce,
        Self::TextStart,
        Self::FirstNonBlank,
        Self::TextEnd,
        Self::PrevLine,
        Self::NextLine,
        Self::ThisLine,
        Self::WordForward,
        Self::WordBackward,
        Self::WordEnd,
        Self::BigWordForward,
        Self::BigWordBackward,
        Self::BigWordEnd,
        Self::FindChar,
        Self::FindCharBack,
        Self::TillChar,
        Self::TillCharBack,
        Self::RepeatFind,
        Self::RepeatFindBack,
        Self::Backspace,
        Self::DeleteChar,
        Self::DeleteToStart,
        Self::DeleteToEnd,
        Self::DeleteWord,
        Self::DeleteOperator,
        Self::ChangeOperator,
        Self::YankOperator,
        Self::Substitute,
        Self::SubstituteLine,
        Self::ChangeToEnd,
        Self::ReplaceChar,
        Self::ToggleCase,
        Self::PutAfter,
        Self::PutBefore,
        Self::Undo,
        Self::Redo,
        Self::Register,
        Self::PasteRegister,
        Self::InsertLiteral,
        Self::Complete,
        Self::AcceptCompletion,
        Self::CancelCompletion,
        Self::Confirm,
        Self::Toggle,
        Self::Cancel,
        Self::Abort,
        Self::NextField,
        Self::PrevField,
        Self::Help,
        Self::KeyHints,
        Self::Quit,
        Self::Redraw,
    ];
}

#[cfg(test)]
impl Context {
    /// Every context, in declaration order.
    pub(crate) const ALL: &'static [Self] = &[
        Self::Global,
        Self::Panel,
        Self::List,
        Self::LocationMenu,
        Self::TabMenu,
        Self::Jump,
        Self::Workspaces,
        Self::PullDown,
        Self::Input,
        Self::QuickSearch,
        Self::Rename,
        Self::Dialog,
        Self::Help,
        Self::DialogInput,
        Self::PathInput,
        Self::Completion,
        Self::Viewer,
        Self::CommandLine,
        Self::CommandCompletion,
        Self::CommandHistory,
        Self::Find,
        Self::FindInput,
        Self::KeyHints,
        Self::UserScreen,
    ];
}
