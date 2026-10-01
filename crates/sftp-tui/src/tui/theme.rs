//! Colors and text styles of the UI.

use ratatui::style::{Color, Style};
use sftp_tui_vfs::{DirEntry, FileKind};

use super::panel::HostStatus;

/// Styles for every part of the UI. Entry and host styles carry a foreground color only; they
/// are drawn over `panel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Theme {
    /// Panel background and default text.
    pub(crate) panel: Style,
    /// Panel frame lines.
    pub(crate) panel_border: Style,
    /// Title of the active panel; the inactive one's is drawn like its frame.
    pub(crate) panel_title_active: Style,
    /// Column headers.
    pub(crate) header: Style,
    /// The row under the cursor in the active panel; replaces the entry style.
    pub(crate) cursor: Style,
    /// A marked row, and the total of the marked entries.
    pub(crate) marked: Style,
    /// A marked row under the cursor.
    pub(crate) marked_cursor: Style,
    /// What quick search has matched so far.
    pub(crate) quick_search: Style,
    /// Regular file.
    pub(crate) file: Style,
    /// Directory, and symlink to a directory.
    pub(crate) directory: Style,
    /// Regular file with an execute bit.
    pub(crate) executable: Style,
    /// Symlink to a non-directory.
    pub(crate) symlink: Style,
    /// Symlink whose target does not exist.
    pub(crate) stale_link: Style,
    /// Block or character device.
    pub(crate) device: Style,
    /// FIFO, socket, or unknown kind.
    pub(crate) special: Style,
    pub(crate) host_idle: Style,
    pub(crate) host_connecting: Style,
    pub(crate) host_connected: Style,
    /// The last connection failed or was lost.
    pub(crate) host_failed: Style,
    /// Host address from `ssh -G`.
    pub(crate) address: Style,
    /// F-key bar: the key numbers.
    pub(crate) fkey_number: Style,
    /// F-key bar: the labels.
    pub(crate) fkey_label: Style,
    /// Dialog body, frame, and text.
    pub(crate) dialog: Style,
    pub(crate) dialog_title: Style,
    /// Dialog button without the focus.
    pub(crate) dialog_button: Style,
    pub(crate) dialog_button_focused: Style,
    /// Text field in a dialog.
    pub(crate) dialog_input: Style,
    /// What mc draws to the right of and below a dialog; `None` draws nothing.
    pub(crate) shadow: Option<Style>,
}

impl Theme {
    /// Names of the built-in themes, for `ui.theme`.
    pub(crate) const NAMES: &'static [&'static str] = &["mc-classic", "terminal"];

    /// A built-in theme by name.
    pub(crate) fn by_name(name: &str) -> Option<Self> {
        match name {
            "mc-classic" => Some(Self::mc_classic()),
            "terminal" => Some(Self::terminal()),
            _ => None,
        }
    }

    /// Midnight Commander's default skin: blue panels, a cyan cursor, gray dialogs.
    pub(crate) fn mc_classic() -> Self {
        let on = |fg: Color, bg: Color| Style::new().fg(fg).bg(bg);
        let fg = |fg: Color| Style::new().fg(fg);
        Self {
            panel: on(Color::Gray, Color::Blue),
            panel_border: on(Color::Gray, Color::Blue),
            panel_title_active: on(Color::Black, Color::Cyan),
            header: on(Color::LightYellow, Color::Blue),
            cursor: on(Color::Black, Color::Cyan),
            marked: on(Color::LightYellow, Color::Blue),
            marked_cursor: on(Color::LightYellow, Color::Cyan),
            quick_search: on(Color::Black, Color::Cyan),
            file: fg(Color::Gray),
            directory: fg(Color::White),
            executable: fg(Color::LightGreen),
            symlink: fg(Color::Gray),
            stale_link: fg(Color::LightRed),
            device: fg(Color::LightMagenta),
            special: fg(Color::Black),
            host_idle: fg(Color::Gray),
            host_connecting: fg(Color::LightYellow),
            host_connected: fg(Color::LightGreen),
            host_failed: fg(Color::LightRed),
            address: fg(Color::Gray),
            fkey_number: on(Color::White, Color::Black),
            fkey_label: on(Color::Black, Color::Cyan),
            dialog: on(Color::Black, Color::Gray),
            dialog_title: on(Color::Blue, Color::Gray),
            dialog_button: on(Color::Black, Color::Gray),
            dialog_button_focused: on(Color::Black, Color::Cyan),
            dialog_input: on(Color::Black, Color::Cyan),
            shadow: Some(on(Color::DarkGray, Color::Black)),
        }
    }

    /// The terminal's own colors, with reverse video where mc uses color.
    pub(crate) fn terminal() -> Self {
        let plain = Style::new();
        let reversed = Style::new().reversed();
        Self {
            panel: plain,
            panel_border: plain,
            panel_title_active: reversed,
            header: plain,
            cursor: reversed,
            // As mc draws marks in black and white.
            marked: Style::new().bold(),
            marked_cursor: reversed.bold(),
            quick_search: reversed,
            file: plain,
            directory: plain,
            executable: plain,
            symlink: plain,
            stale_link: plain,
            device: plain,
            special: plain,
            host_idle: plain,
            host_connecting: plain,
            host_connected: plain,
            host_failed: plain,
            address: plain,
            fkey_number: plain,
            fkey_label: reversed,
            dialog: plain,
            dialog_title: plain,
            dialog_button: plain,
            dialog_button_focused: reversed,
            dialog_input: reversed,
            shadow: None,
        }
    }

    /// The style of an entry's name by its kind, as mc highlights files.
    pub(crate) fn entry(&self, entry: &DirEntry) -> Style {
        match entry.metadata.kind {
            FileKind::Dir => self.directory,
            FileKind::Symlink => match entry.target_kind {
                Some(FileKind::Dir) => self.directory,
                Some(_) => self.symlink,
                None => self.stale_link,
            },
            FileKind::BlockDevice | FileKind::CharDevice => self.device,
            FileKind::Fifo | FileKind::Socket | FileKind::Unknown => self.special,
            FileKind::File
                if entry
                    .metadata
                    .permissions
                    .is_some_and(|bits| bits & 0o111 != 0) =>
            {
                self.executable
            }
            FileKind::File => self.file,
        }
    }

    /// The style of a host's status marker.
    pub(crate) fn host_status(&self, status: HostStatus) -> Style {
        match status {
            HostStatus::Idle => self.host_idle,
            HostStatus::Connecting => self.host_connecting,
            HostStatus::Connected => self.host_connected,
            HostStatus::Failed => self.host_failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use sftp_tui_vfs::Metadata;

    use super::*;

    fn entry(kind: FileKind, permissions: u32, target_kind: Option<FileKind>) -> DirEntry {
        DirEntry {
            name: b"x".to_vec(),
            metadata: Metadata {
                kind,
                size: None,
                permissions: Some(permissions),
                modified: None,
                uid: None,
                gid: None,
            },
            target_kind,
        }
    }

    #[test]
    fn every_name_is_a_theme() {
        for name in Theme::NAMES {
            assert!(Theme::by_name(name).is_some(), "{name}");
        }
        assert_eq!(Theme::by_name("solarized"), None);
    }

    #[test]
    fn mc_classic_highlights_entries_by_kind_as_mc_does() {
        let theme = Theme::mc_classic();
        let fg = |entry: DirEntry| theme.entry(&entry).fg;
        assert_eq!(fg(entry(FileKind::Dir, 0o755, None)), Some(Color::White));
        let link_to_dir = entry(FileKind::Symlink, 0o777, Some(FileKind::Dir));
        assert_eq!(fg(link_to_dir), Some(Color::White));
        assert_eq!(
            fg(entry(FileKind::Symlink, 0o777, None)),
            Some(Color::LightRed)
        );
        assert_eq!(
            fg(entry(FileKind::File, 0o755, None)),
            Some(Color::LightGreen)
        );
        assert_eq!(fg(entry(FileKind::File, 0o644, None)), Some(Color::Gray));
        assert_eq!(
            fg(entry(FileKind::CharDevice, 0o644, None)),
            Some(Color::LightMagenta)
        );
        assert_eq!(
            theme.host_status(HostStatus::Connected).fg,
            Some(Color::LightGreen)
        );
    }
}
