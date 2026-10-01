//! What keys do: actions, and the contexts they are bound in.

/// Where a key is pressed. Keys are looked up in the context's [chain](Self::chain); the first
/// context there that knows a key sequence, as a binding or as the start of one, decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Context {
    /// A panel that lists a directory.
    Panel,
    /// Quick search in the active panel. Keys it does not bind fall through to the panel.
    QuickSearch,
    /// A dialog whose focus is on a button or a list.
    Dialog,
    /// A dialog whose focus is on a text field; falls back to `Dialog`.
    DialogInput,
}

impl Context {
    /// This context and its fallbacks, most specific first.
    pub(crate) fn chain(self) -> &'static [Self] {
        match self {
            Self::Panel => &[Self::Panel],
            Self::QuickSearch => &[Self::QuickSearch, Self::Panel],
            // Dialogs are modal: panel keys do nothing while one is open.
            Self::Dialog => &[Self::Dialog],
            Self::DialogInput => &[Self::DialogInput, Self::Dialog],
        }
    }

    /// Whether `Esc` waits for another key, as in mc: `Esc 1` … `Esc 0` stand for F1 … F10,
    /// `Esc` and a character for Alt and that character, and `Esc` alone acts once the sequence
    /// times out. Elsewhere `Esc` acts at once.
    pub(crate) fn esc_waits(self) -> bool {
        matches!(self, Self::Panel)
    }

    /// Whether an unbound printable key becomes [`Resolved::Insert`](super::Resolved::Insert).
    pub(crate) fn accepts_text(self) -> bool {
        matches!(self, Self::Panel | Self::QuickSearch | Self::DialogInput)
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
    /// Reads the directory again.
    Reload,
    /// Shows or hides files whose names start with a dot.
    ToggleHidden,
    /// Starts quick search, or jumps to the next match.
    QuickSearch,
    /// Deletes the character before the text cursor.
    Backspace,
    /// Deletes the character at the text cursor.
    Delete,
    /// Deletes from the start of the text field to the cursor.
    DeleteToStart,
    /// Deletes from the cursor to the end of the text field.
    DeleteToEnd,
    /// Accepts the dialog, or activates the focused button.
    Confirm,
    /// Closes the dialog, ends quick search, or stops a pending listing or connection.
    Cancel,
    /// Moves the focus to the next field or button.
    NextField,
    /// Moves the focus to the previous field or button.
    PrevField,
    /// Shows the key bindings.
    Help,
    /// Quits sftp-tui.
    Quit,
    /// Redraws the whole screen.
    Redraw,
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
        Self::Parent,
        Self::SwitchPanel,
        Self::SwapPanels,
        Self::OtherPanelOpen,
        Self::OtherPanelSync,
        Self::Reload,
        Self::ToggleHidden,
        Self::QuickSearch,
        Self::Backspace,
        Self::Delete,
        Self::DeleteToStart,
        Self::DeleteToEnd,
        Self::Confirm,
        Self::Cancel,
        Self::NextField,
        Self::PrevField,
        Self::Help,
        Self::Quit,
        Self::Redraw,
    ];
}
