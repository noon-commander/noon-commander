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
    /// A panel on the virtual root or the list of hosts; falls back to `Panel`.
    Root,
    /// Keys that the windows listing items to choose from share, whose characters filter the
    /// list: the menus, the zoxide window, the workspaces, the command history fall back to it.
    /// Keys never go to it alone.
    List,
    /// The location menu of Alt+F1 and Alt+F2, which has a filter; falls back to `List`.
    LocationMenu,
    /// The zoxide window of Alt+z, which takes keywords; falls back to `List`.
    Jump,
    /// The window of the saved workspaces, which has a filter; falls back to `List`.
    Workspaces,
    /// The pull-down menu of F9, whose commands have letters.
    PullDown,
    /// Quick search in the active panel. Keys it does not bind fall through to the panel.
    QuickSearch,
    /// The name field of the entry being renamed in its panel row. Keys it does not bind, but
    /// the global ones, do nothing.
    Rename,
    /// A dialog whose focus is on a button or a list.
    Dialog,
    /// A dialog whose focus is on a text field; falls back to `Dialog`.
    DialogInput,
    /// A text field for a path, which Tab completes; falls back to `DialogInput`.
    PathInput,
    /// The list of completions under a path field; keys it does not bind go to the field.
    Completion,
    /// The viewer of F3.
    Viewer,
    /// The command line of `!` and `:`. Keys it does not bind, but the global ones, do nothing.
    CommandLine,
    /// The window of the command history, which has a filter; falls back to `List`.
    History,
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
            Self::Root => "root",
            Self::List => "list",
            Self::LocationMenu => "location_menu",
            Self::Jump => "jump",
            Self::Workspaces => "workspaces",
            Self::PullDown => "pull_down",
            Self::QuickSearch => "quick_search",
            Self::Rename => "rename",
            Self::Dialog => "dialog",
            Self::DialogInput => "dialog_input",
            Self::PathInput => "path_input",
            Self::Completion => "completion",
            Self::Viewer => "viewer",
            Self::CommandLine => "command_line",
            Self::History => "history",
            Self::UserScreen => "user_screen",
        }
    }

    /// This context and its fallbacks, most specific first.
    pub(crate) fn chain(self) -> &'static [Self] {
        match self {
            Self::Global => &[Self::Global],
            Self::Panel => &[Self::Panel, Self::Global],
            Self::Root => &[Self::Root, Self::Panel, Self::Global],
            Self::QuickSearch => &[Self::QuickSearch, Self::Panel, Self::Global],
            Self::Rename => &[Self::Rename, Self::Global],
            // Dialogs are modal: panel keys do nothing while one is open.
            Self::Dialog => &[Self::Dialog, Self::Global],
            Self::DialogInput => &[Self::DialogInput, Self::Dialog, Self::Global],
            Self::PathInput => &[
                Self::PathInput,
                Self::DialogInput,
                Self::Dialog,
                Self::Global,
            ],
            Self::Completion => &[
                Self::Completion,
                Self::PathInput,
                Self::DialogInput,
                Self::Dialog,
                Self::Global,
            ],
            Self::Viewer => &[Self::Viewer, Self::Global],
            Self::CommandLine => &[Self::CommandLine, Self::Global],
            // The terminal's own screen: nothing of noc's draws there, the hints neither.
            Self::UserScreen => &[Self::UserScreen],
            // Menus are modal too.
            Self::List => &[Self::List, Self::Global],
            Self::LocationMenu => &[Self::LocationMenu, Self::List, Self::Global],
            Self::Jump => &[Self::Jump, Self::List, Self::Global],
            Self::Workspaces => &[Self::Workspaces, Self::List, Self::Global],
            Self::History => &[Self::History, Self::List, Self::Global],
            Self::PullDown => &[Self::PullDown, Self::Global],
        }
    }

    /// Whether the help lists this context's keys with each context that falls back to it,
    /// where they mean what that one's window does with them, rather than apart.
    pub(crate) fn helps_with_others(self) -> bool {
        matches!(self, Self::List)
    }

    /// Whether `Esc` waits for another key, as in mc: `Esc 1` … `Esc 0` stand for F1 … F10,
    /// `Esc` and a character for Alt and that character, and `Esc` alone acts once the sequence
    /// times out. Elsewhere `Esc` acts at once.
    pub(crate) fn esc_waits(self) -> bool {
        matches!(self, Self::Panel | Self::Root)
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
                | Self::Jump
                | Self::Workspaces
                | Self::History
                | Self::PullDown
        )
    }

    /// Whether an unbound printable key becomes [`Resolved::Insert`](super::Resolved::Insert).
    pub(crate) fn accepts_text(self) -> bool {
        matches!(
            self,
            Self::Panel
                | Self::Root
                | Self::QuickSearch
                | Self::Rename
                | Self::CommandLine
                | Self::DialogInput
                | Self::PathInput
                | Self::Completion
                | Self::LocationMenu
                | Self::Jump
                | Self::Workspaces
                | Self::History
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
    /// The first row, or the start of a text field.
    Home,
    /// The last row, or the end of a text field.
    End,
    /// Opens the directory or host under the cursor.
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
    /// Opens the parent directory; from `/`, the virtual root.
    Parent,
    /// Makes the other panel active.
    SwitchPanel,
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
    /// Lists the panel's tabs to choose one.
    TabList,
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
    /// Starts quick search, or jumps to the next match.
    QuickSearch,
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
    /// Shows the terminal's own screen, with the output of commands, in place of the panels.
    UserScreen,
    /// Closes the connection to the host under the cursor, or stops connecting to it.
    Disconnect,
    /// Edits the settings of the host under the cursor.
    EditHost,
    /// Deletes the character before the text cursor.
    Backspace,
    /// Deletes the marked entries or the one under the cursor; in a text field, the character
    /// at the text cursor.
    Delete,
    /// Deletes from the start of the text field to the cursor.
    DeleteToStart,
    /// Deletes from the cursor to the end of the text field.
    DeleteToEnd,
    /// Completes the path in a text field; in the list of completions, the next one.
    Complete,
    /// Accepts the dialog, or activates the focused button.
    Confirm,
    /// Switches the focused check box, or presses the focused button.
    Toggle,
    /// Closes the dialog, ends quick search, or stops a pending listing or connection.
    Cancel,
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
            Self::TabList => "tab_list",
            Self::SaveWorkspace => "save_workspace",
            Self::Workspaces => "workspaces",
            Self::Reload => "reload",
            Self::ToggleHidden => "toggle_hidden",
            Self::SortByName => "sort_by_name",
            Self::SortByExtension => "sort_by_extension",
            Self::SortByTime => "sort_by_time",
            Self::SortBySize => "sort_by_size",
            Self::QuickSearch => "quick_search",
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
            Self::Backspace => "backspace",
            Self::Delete => "delete",
            Self::DeleteToStart => "delete_to_start",
            Self::DeleteToEnd => "delete_to_end",
            Self::Complete => "complete",
            Self::Confirm => "confirm",
            Self::Toggle => "toggle",
            Self::Cancel => "cancel",
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
        Self::Root,
        Self::List,
        Self::LocationMenu,
        Self::Jump,
        Self::Workspaces,
        Self::PullDown,
        Self::QuickSearch,
        Self::Rename,
        Self::Dialog,
        Self::DialogInput,
        Self::PathInput,
        Self::Completion,
        Self::Viewer,
        Self::CommandLine,
        Self::History,
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
        Self::TabList,
        Self::SaveWorkspace,
        Self::Workspaces,
        Self::Reload,
        Self::ToggleHidden,
        Self::SortByName,
        Self::SortByExtension,
        Self::SortByTime,
        Self::SortBySize,
        Self::QuickSearch,
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
        Self::Backspace,
        Self::Delete,
        Self::DeleteToStart,
        Self::DeleteToEnd,
        Self::Complete,
        Self::Confirm,
        Self::Toggle,
        Self::Cancel,
        Self::NextField,
        Self::PrevField,
        Self::Help,
        Self::KeyHints,
        Self::Quit,
        Self::Redraw,
    ];
}
