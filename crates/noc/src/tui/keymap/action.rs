//! What keys do: actions, and the contexts they are bound in.

/// Where a key is pressed. Keys are looked up in the context's [chain](Self::chain); the first
/// context there that knows a key sequence, as a binding or as the start of one, decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Context {
    /// Keys that do the same wherever they are pressed: every context but the user screen falls
    /// back to it last. Keys never go to it alone.
    Global,
    /// A panel that lists a directory.
    Panel,
    /// Keys that the windows listing items to choose from share, whose characters filter the
    /// list: the menus, the zoxide window, the workspaces, the command history fall back to it.
    /// Keys never go to it alone.
    List,
    /// The location menu of Alt+F1 and Alt+F2, which has a filter; falls back to `List`, then
    /// to `Input`.
    LocationMenu,
    /// The tab menu of a panel, which has a filter; falls back to `List`, then to `Input`.
    TabMenu,
    /// The zoxide window of Alt+z, which takes keywords; falls back to `List`, then to `Input`.
    Jump,
    /// The window of the saved workspaces, which has a filter; falls back to `List`, then to
    /// `Input`.
    Workspaces,
    /// The pull-down menu of F9, whose commands have letters.
    PullDown,
    /// Keys that the text fields share, which edit the text: quick search, the field of an
    /// entry renamed in its row, the text fields of dialogs, the command line, and the filters
    /// of the windows that list items fall back to it. Keys never go to it alone.
    Input,
    /// Quick search in the active panel; falls back to `Input`, then to the panel.
    QuickSearch,
    /// The name field of the entry being renamed in its panel row; falls back to `Input`. Other
    /// keys, but the global ones, do nothing.
    Rename,
    /// A dialog whose focus is on a button or a list.
    Dialog,
    /// The help of F1; falls back to `Dialog`, whose keys scroll it and close it.
    Help,
    /// A dialog whose focus is on a text field; falls back to `Input`, then to `Dialog`.
    DialogInput,
    /// A text field for a path, which Tab completes; falls back to `DialogInput`.
    PathInput,
    /// The list of completions under a path field; keys it does not bind go to the field.
    Completion,
    /// The viewer of F3.
    Viewer,
    /// The command line of `!` and `:`; falls back to `Input`. Other keys, but the global ones,
    /// do nothing.
    CommandLine,
    /// The window of the command history, which has a filter; falls back to `List`, then to `Input`.
    CommandHistory,
    /// The terminal's own screen, with the output of commands, in place of the panels; only
    /// the keys that go back to the panels do anything.
    UserScreen,
}

impl Context {
    /// The context's name in snake case, as `noc keymap diff` writes it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Panel => "panel",
            Self::List => "list",
            Self::LocationMenu => "location_menu",
            Self::TabMenu => "tab_menu",
            Self::Jump => "jump",
            Self::Workspaces => "workspaces",
            Self::PullDown => "pull_down",
            Self::Input => "input",
            Self::QuickSearch => "quick_search",
            Self::Rename => "rename",
            Self::Dialog => "dialog",
            Self::Help => "help",
            Self::DialogInput => "dialog_input",
            Self::PathInput => "path_input",
            Self::Completion => "completion",
            Self::Viewer => "viewer",
            Self::CommandLine => "command_line",
            Self::CommandHistory => "command_history",
            Self::UserScreen => "user_screen",
        }
    }

    /// This context and its fallbacks, most specific first.
    pub(crate) fn chain(self) -> &'static [Self] {
        match self {
            Self::Global => &[Self::Global],
            Self::Panel => &[Self::Panel, Self::Global],
            // Text fields edit their text with the keys of `Input` before anything else does.
            Self::Input => &[Self::Input, Self::Global],
            Self::QuickSearch => &[Self::QuickSearch, Self::Input, Self::Panel, Self::Global],
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
            // The terminal's own screen: nothing of noc's draws there, the hints neither.
            Self::UserScreen => &[Self::UserScreen],
            // Menus are modal too. Their filters are text fields, whose keys come after the
            // list's: Home and End go to the first and the last row.
            Self::List => &[Self::List, Self::Global],
            Self::LocationMenu => &[Self::LocationMenu, Self::List, Self::Input, Self::Global],
            Self::TabMenu => &[Self::TabMenu, Self::List, Self::Input, Self::Global],
            Self::Jump => &[Self::Jump, Self::List, Self::Input, Self::Global],
            Self::Workspaces => &[Self::Workspaces, Self::List, Self::Input, Self::Global],
            Self::CommandHistory => &[Self::CommandHistory, Self::List, Self::Input, Self::Global],
            Self::PullDown => &[Self::PullDown, Self::Global],
        }
    }

    /// Whether the help lists this context's keys with each context that falls back to it,
    /// where they mean what that one's window does with them, rather than apart.
    pub(crate) fn helps_with_others(self) -> bool {
        matches!(self, Self::List | Self::Input)
    }

    /// Whether Alt and a key stand for Esc and the key, as in mc: Alt+1 … Alt+0 for F1 … F10,
    /// which mc reads as `Esc 1` … `Esc 0`, and an Alt and a character that nothing binds for
    /// `Esc` and the character, which is how an Esc typed quickly before a key arrives. `Esc`
    /// itself acts at once, here as everywhere.
    pub(crate) fn alt_stands_for_esc(self) -> bool {
        matches!(self, Self::Panel)
    }

    /// Whether Alt+1 … Alt+0 stand for the F-keys bound here, for terminals without them: in
    /// the panels, and in the windows that list items, whose F-keys act on the item under the
    /// cursor. Dialogs keep Alt as it is.
    pub(crate) fn alt_digits_stand_for_f_keys(self) -> bool {
        matches!(
            self,
            Self::Panel
                | Self::List
                | Self::LocationMenu
                | Self::TabMenu
                | Self::Jump
                | Self::Workspaces
                | Self::CommandHistory
        )
    }

    /// Whether every character is text, even one that a fallback context binds: in quick
    /// search, `*` is part of a name, not a command; in a text field, Space is a space; in the
    /// menus, digits are hotkeys and letters filter; in the pull-down menu, letters run commands.
    pub(crate) fn text_first(self) -> bool {
        matches!(
            self,
            Self::QuickSearch
                | Self::Rename
                | Self::CommandLine
                | Self::DialogInput
                | Self::PathInput
                | Self::Completion
                | Self::LocationMenu
                | Self::TabMenu
                | Self::Jump
                | Self::Workspaces
                | Self::CommandHistory
                | Self::PullDown
        )
    }

    /// Whether an unbound printable key becomes [`Resolved::Insert`](super::Resolved::Insert).
    pub(crate) fn accepts_text(self) -> bool {
        matches!(
            self,
            Self::Panel
                | Self::QuickSearch
                | Self::Rename
                | Self::CommandLine
                | Self::DialogInput
                | Self::PathInput
                | Self::Completion
                | Self::LocationMenu
                | Self::TabMenu
                | Self::Jump
                | Self::Workspaces
                | Self::CommandHistory
                | Self::PullDown
        )
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
    /// One page up.
    PageUp,
    /// One page down.
    PageDown,
    /// Half a page up.
    HalfPageUp,
    /// Half a page down.
    HalfPageDown,
    /// The first row, or the start of a text field.
    Home,
    /// The last row, or the end of a text field.
    End,
    /// Opens the directory under the cursor.
    Enter,
    /// Marks the entry under the cursor, or unmarks it, and moves down.
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
    /// Edits the file under the cursor in the editor of `$VISUAL` or `$EDITOR`.
    Edit,
    /// Asks where to, and copies the marked entries or the one under the cursor.
    Copy,
    /// Asks where to, and moves or renames the marked entries or the one under the cursor.
    Move,
    /// Renames the entry under the cursor in its row.
    Rename,
    /// Asks for a name and makes a directory.
    Mkdir,
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
    /// Shows the panel's next tab, round.
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
    /// what the last search found.
    QuickSearch,
    /// Moves to the next name that matches quick search: what is typed while it runs, or else
    /// what the last search found.
    NextMatch,
    /// Moves to the previous name that matches quick search, likewise.
    PrevMatch,
    /// Opens the command line for a shell command.
    Shell,
    /// Opens the command line for a command of Noon Commander, of which `!` runs a shell
    /// command.
    Command,
    /// Starts a new line in the command.
    NewLine,
    /// Opens the command in the editor of `$VISUAL` or `$EDITOR`; what it leaves comes back.
    EditCommand,
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
    /// The text cursor to the start of the text, or of its line on the command line.
    TextStart,
    /// The text cursor to the end of the text, or of its line on the command line.
    TextEnd,
    /// The text cursor to the start of the word before it.
    WordLeft,
    /// The text cursor to the end of the word after it.
    WordRight,
    /// Deletes the character before the text cursor.
    Backspace,
    /// Deletes the marked entries or the one under the cursor; in a text field, the character
    /// at the text cursor.
    Delete,
    /// Deletes from the start of the text field to the cursor.
    DeleteToStart,
    /// Deletes from the cursor to the end of the text field.
    DeleteToEnd,
    /// Deletes the word before the text cursor, and the spaces after it, as the shell's Ctrl+w.
    DeleteWord,
    /// Completes the path in a text field; in the list of completions, the next one.
    Complete,
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
    /// typed runs as it would have, or shows the keys that can follow it.
    KeyHints,
    /// Quits Noon Commander.
    Quit,
    /// Redraws the whole screen.
    Redraw,
}

impl Action {
    /// The action's name in snake case, as `noc keymap diff` writes it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::PageUp => "page_up",
            Self::PageDown => "page_down",
            Self::HalfPageUp => "half_page_up",
            Self::HalfPageDown => "half_page_down",
            Self::Home => "home",
            Self::End => "end",
            Self::Enter => "enter",
            Self::Mark => "mark",
            Self::MarkUp => "mark_up",
            Self::InvertMarks => "invert_marks",
            Self::View => "view",
            Self::ToggleWrap => "toggle_wrap",
            Self::Edit => "edit",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Rename => "rename",
            Self::Mkdir => "mkdir",
            Self::Checksum => "checksum",
            Self::Jobs => "jobs",
            Self::Select => "select",
            Self::Unselect => "unselect",
            Self::Parent => "parent",
            Self::SwitchPanel => "switch_panel",
            Self::FocusLeft => "focus_left",
            Self::FocusRight => "focus_right",
            Self::SwapPanels => "swap_panels",
            Self::OtherPanelOpen => "other_panel_open",
            Self::OtherPanelSync => "other_panel_sync",
            Self::LocationMenuLeft => "location_menu_left",
            Self::LocationMenuRight => "location_menu_right",
            Self::Jump => "jump",
            Self::QuickCd => "quick_cd",
            Self::PullDown => "pull_down",
            Self::PullDownLast => "pull_down_last",
            Self::NewTab => "new_tab",
            Self::CloseTab => "close_tab",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
            Self::TabListLeft => "tab_list_left",
            Self::TabListRight => "tab_list_right",
            Self::SaveWorkspace => "save_workspace",
            Self::Workspaces => "workspaces",
            Self::Reload => "reload",
            Self::ToggleHidden => "toggle_hidden",
            Self::SortByName => "sort_by_name",
            Self::SortByExtension => "sort_by_extension",
            Self::SortByTime => "sort_by_time",
            Self::SortBySize => "sort_by_size",
            Self::QuickSearch => "quick_search",
            Self::NextMatch => "next_match",
            Self::PrevMatch => "prev_match",
            Self::Shell => "shell",
            Self::Command => "command",
            Self::NewLine => "new_line",
            Self::EditCommand => "edit_command",
            Self::OlderCommand => "older_command",
            Self::NewerCommand => "newer_command",
            Self::CommandHistory => "command_history",
            Self::UserScreen => "user_screen",
            Self::Disconnect => "disconnect",
            Self::EditHost => "edit_host",
            Self::TextStart => "text_start",
            Self::TextEnd => "text_end",
            Self::WordLeft => "word_left",
            Self::WordRight => "word_right",
            Self::Backspace => "backspace",
            Self::Delete => "delete",
            Self::DeleteToStart => "delete_to_start",
            Self::DeleteToEnd => "delete_to_end",
            Self::DeleteWord => "delete_word",
            Self::Complete => "complete",
            Self::Confirm => "confirm",
            Self::Toggle => "toggle",
            Self::Cancel => "cancel",
            Self::Abort => "abort",
            Self::NextField => "next_field",
            Self::PrevField => "prev_field",
            Self::Help => "help",
            Self::KeyHints => "key_hints",
            Self::Quit => "quit",
            Self::Redraw => "redraw",
        }
    }
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
        Self::CommandHistory,
        Self::UserScreen,
    ];
}

#[cfg(test)]
impl Action {
    /// Every action, in declaration order.
    pub(crate) const ALL: &'static [Self] = &[
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::PageUp,
        Self::PageDown,
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
        Self::Edit,
        Self::Copy,
        Self::Move,
        Self::Rename,
        Self::Mkdir,
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
        Self::NextMatch,
        Self::PrevMatch,
        Self::Shell,
        Self::Command,
        Self::NewLine,
        Self::EditCommand,
        Self::OlderCommand,
        Self::NewerCommand,
        Self::CommandHistory,
        Self::UserScreen,
        Self::Disconnect,
        Self::EditHost,
        Self::TextStart,
        Self::TextEnd,
        Self::WordLeft,
        Self::WordRight,
        Self::Backspace,
        Self::Delete,
        Self::DeleteToStart,
        Self::DeleteToEnd,
        Self::DeleteWord,
        Self::Complete,
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
