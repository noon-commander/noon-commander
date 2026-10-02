//! Colors, text styles, and frame lines of the UI.

use noc_config::Borders;
use noc_vfs::{DirEntry, FileKind};
use ratatui::style::{Color, Style};
use ratatui::widgets::BorderType;

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
    /// The status of a host in the location menu, drawn over `dialog`, which the panel colors
    /// may not show on: idle, connecting, connected, failed.
    pub(crate) dialog_host: [Style; 4],
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
    /// Text field that still holds the text it opened with, which typing replaces.
    pub(crate) dialog_input_fresh: Style,
    /// The progress bar of a job.
    pub(crate) gauge: Style,
    /// Error dialogs: body, frame, text, and buttons without the focus.
    pub(crate) error_dialog: Style,
    pub(crate) error_title: Style,
    pub(crate) error_button_focused: Style,
    /// What mc draws to the right of and below a dialog; `None` draws nothing.
    pub(crate) shadow: Option<Style>,
    /// The lines that frame panels and dialogs.
    pub(crate) borders: Borders,
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
            marked: on(Color::LightYellow, Color::Blue).underlined(),
            marked_cursor: on(Color::LightYellow, Color::Cyan).underlined(),
            quick_search: on(Color::Black, Color::Cyan),
            file: fg(Color::Gray),
            directory: fg(Color::White).bold(),
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
            // Dark colors, as mc draws on its gray dialogs; idle takes the dialog's black.
            dialog_host: [
                Style::new(),
                fg(Color::Yellow),
                fg(Color::Green),
                fg(Color::Red),
            ],
            fkey_number: on(Color::White, Color::Black),
            fkey_label: on(Color::Black, Color::Cyan),
            dialog: on(Color::Black, Color::Gray),
            dialog_title: on(Color::Blue, Color::Gray),
            dialog_button: on(Color::Black, Color::Gray),
            dialog_button_focused: on(Color::Black, Color::Cyan),
            dialog_input: on(Color::Black, Color::Cyan),
            dialog_input_fresh: on(Color::DarkGray, Color::Cyan),
            gauge: on(Color::White, Color::Black),
            error_dialog: on(Color::White, Color::Red),
            error_title: on(Color::LightYellow, Color::Red),
            error_button_focused: on(Color::Black, Color::Gray),
            shadow: Some(on(Color::DarkGray, Color::Black)),
            borders: Borders::default(),
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
            // Not bold as in mc: that is what directories are.
            marked: Style::new().underlined(),
            marked_cursor: reversed.underlined(),
            quick_search: reversed,
            file: plain,
            directory: plain.bold(),
            executable: plain,
            symlink: plain,
            stale_link: plain,
            device: plain,
            special: plain,
            // Without colors, connected hosts stand out by weight; a failure has a glyph or
            // marker of its own.
            host_idle: plain.dim(),
            host_connecting: plain,
            host_connected: plain.bold(),
            host_failed: plain,
            address: plain,
            dialog_host: [plain.dim(), plain, plain.bold(), plain],
            fkey_number: plain,
            fkey_label: reversed,
            dialog: plain,
            dialog_title: plain,
            dialog_button: plain,
            dialog_button_focused: reversed,
            dialog_input: reversed,
            dialog_input_fresh: reversed.dim(),
            gauge: plain,
            error_dialog: plain,
            error_title: plain.bold(),
            error_button_focused: reversed,
            shadow: None,
            borders: Borders::default(),
        }
    }

    /// This theme framed with `borders`, from `ui.borders`.
    pub(crate) fn with_borders(self, borders: Borders) -> Self {
        Self { borders, ..self }
    }

    /// The frame of a panel or a dialog.
    pub(crate) fn border_type(&self) -> BorderType {
        match self.borders {
            Borders::Double => BorderType::Double,
            Borders::Single => BorderType::Plain,
        }
    }

    /// The ends of a single line across a frame, as mc joins them: `╟` and `╢` on a double one.
    pub(crate) fn tees(&self) -> (char, char) {
        match self.borders {
            Borders::Double => ('╟', '╢'),
            Borders::Single => ('├', '┤'),
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

    /// The style of the icon in front of a name in `name`: the name's color, toned down so the
    /// name stands out. Not bold, which many terminals cannot draw together with dim.
    pub(crate) fn icon(name: Style) -> Style {
        name.not_bold().dim()
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

    /// The style of a host's status icon or marker in a dialog, such as the location menu.
    pub(crate) fn dialog_host_status(&self, status: HostStatus) -> Style {
        let [idle, connecting, connected, failed] = self.dialog_host;
        match status {
            HostStatus::Idle => idle,
            HostStatus::Connecting => connecting,
            HostStatus::Connected => connected,
            HostStatus::Failed => failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use noc_vfs::Metadata;

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
    fn host_states_show_on_dialogs() {
        let statuses = [
            HostStatus::Idle,
            HostStatus::Connecting,
            HostStatus::Connected,
            HostStatus::Failed,
        ];
        for theme in Theme::NAMES.iter().filter_map(|name| Theme::by_name(name)) {
            let background = theme.dialog.bg;
            for status in statuses {
                let style = theme.dialog.patch(theme.dialog_host_status(status));
                assert!(
                    style.fg.is_none() || style.fg != background,
                    "{status:?} on {background:?}"
                );
            }
        }
        let theme = Theme::mc_classic();
        assert_eq!(
            theme
                .dialog
                .patch(theme.dialog_host_status(HostStatus::Idle))
                .fg,
            Some(Color::Black)
        );
        assert_eq!(
            theme.dialog_host_status(HostStatus::Connected).fg,
            Some(Color::Green)
        );
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
        let directory = theme.entry(&entry(FileKind::Dir, 0o755, None));
        assert_eq!(directory, Style::new().fg(Color::White).bold());
        assert_eq!(
            Theme::icon(directory),
            Style::new().fg(Color::White).not_bold().dim()
        );
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
