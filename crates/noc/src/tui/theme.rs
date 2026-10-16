//! Colors, text styles, frame lines, and scroll bar arrows of the UI.

use noc_config::Borders;
use noc_vfs::{DirEntry, FileKind};
use ratatui::style::{Color, Style};
use ratatui::widgets::BorderType;

use super::glyphs::{Glyphs, Lines};
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
    /// The row under the cursor in the inactive panel: a background, drawn under the colors of
    /// the row, which keeps them.
    pub(crate) cursor_inactive: Style,
    /// A marked row, and the total of the marked entries.
    pub(crate) marked: Style,
    /// A marked row under the cursor.
    pub(crate) marked_cursor: Style,
    /// `cursor` and `marked_cursor` in a panel where the user is root: on red.
    pub(crate) root_cursor: Style,
    pub(crate) root_marked_cursor: Style,
    /// What quick search has matched so far.
    pub(crate) quick_search: Style,
    /// What a search found: in the lines of the search of a tree, and in the names that quick
    /// search matches. Apart from the cursor, so that it shows on the cursor's row too, and from
    /// the panels and the dialogs it is drawn on.
    pub(crate) find_match: Style,
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
    /// A message on the line of the mode, as vim's error and warning messages.
    pub(crate) message: Style,
    /// Rights of uid 0: `root@host` on the line of the mode and in the prompt, and the glyph
    /// of a Noon Commander run by root.
    pub(crate) root: Style,
    /// The marks of text cut short or going on past the screen, over the style of their row:
    /// an accent that stands out from names of every kind, in the panels, the viewer, and the
    /// line of the mode.
    pub(crate) truncated: Style,
    /// The same marks on the row under the cursor or a selected row of a list or a menu, the
    /// title of the active panel, and the viewer's title.
    pub(crate) truncated_cursor: Style,
    /// The same marks in dialogs and the lists, over `dialog`.
    pub(crate) truncated_dialog: Style,
    /// The same marks in the pull-down menus, over `menu`.
    pub(crate) truncated_menu: Style,
    /// The same marks on the bars: the F-key bar, the menu bar, and the line of tabs.
    pub(crate) truncated_bar: Style,
    /// The same marks in error dialogs, over `error_dialog`.
    pub(crate) truncated_error: Style,
    /// The status of a host in the location menu, drawn over `dialog`, which the panel colors
    /// may not show on: idle, connecting, connected, failed.
    pub(crate) dialog_host: [Style; 4],
    /// F-key bar: the key numbers.
    pub(crate) fkey_number: Style,
    /// F-key bar: the labels.
    pub(crate) fkey_label: Style,
    /// The menu bar of F9 while a menu is open, and the title of that menu.
    pub(crate) menu_bar: Style,
    pub(crate) menu_bar_selected: Style,
    /// The menu bar while no menu is open, with `ui.menu_bar`.
    pub(crate) menu_bar_inactive: Style,
    /// A pull-down menu: body, frame, and commands; the command under the cursor.
    pub(crate) menu: Style,
    pub(crate) menu_selected: Style,
    /// Drawn over the others: the letter of a command or a menu, and a command that cannot
    /// run now.
    pub(crate) menu_hotkey: Style,
    pub(crate) menu_disabled: Style,
    /// The keys of the commands of the pull-down menu: `keys` on the menu's own background.
    pub(crate) menu_keys: Style,
    /// Dialog body, frame, and text.
    pub(crate) dialog: Style,
    pub(crate) dialog_title: Style,
    /// Keys, wherever they are listed with what they do: in the help, where the names of the
    /// facts on its About page take it too, the key hints, and their list of Ctrl+k; a color of
    /// their own, apart from titles. The pull-down menu has [`menu_keys`](Self::menu_keys).
    pub(crate) keys: Style,
    /// Dialog button without the focus.
    pub(crate) dialog_button: Style,
    pub(crate) dialog_button_focused: Style,
    /// Text field in a dialog.
    pub(crate) dialog_input: Style,
    /// The progress bar of a job.
    pub(crate) gauge: Style,
    /// Error dialogs: body, frame, text, and buttons without the focus.
    pub(crate) error_dialog: Style,
    pub(crate) error_title: Style,
    pub(crate) error_button_focused: Style,
    /// Tabs: the line of tabs, the tabs that do not show, and what parts them; tabs drawn
    /// in a panel's frame take it too.
    pub(crate) tab: Style,
    /// The tab that shows, in the panel's colors as if it grew out of it, on the side with the
    /// keys and on the other side.
    pub(crate) tab_active: Style,
    pub(crate) tab_active_idle: Style,
    /// Drawn over `tab_active`: the number of the tab that shows on the side with the keys.
    pub(crate) tab_number: Style,
    /// What mc draws to the right of and below a dialog; `None` draws nothing.
    pub(crate) shadow: Option<Style>,
    /// The lines that frame panels and dialogs.
    pub(crate) borders: Borders,
    /// What it draws with, as `ui.icons` picks them; the lines of frames go by `borders`.
    pub(crate) glyphs: &'static Glyphs,
}

/// The colors the terminal can show: 24-bit RGB, or only its 256-color palette, to which
/// themes in RGB are brought down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorDepth {
    TrueColor,
    Indexed,
}

impl ColorDepth {
    /// From `COLORTERM`, which terminals with 24-bit color set to `truecolor` or `24bit`.
    pub(crate) fn detect() -> Self {
        let depth = Self::from_colorterm(std::env::var_os("COLORTERM").as_deref());
        if depth == Self::Indexed {
            tracing::info!("COLORTERM does not announce 24-bit color; RGB themes use 256 colors");
        }
        depth
    }

    pub(crate) fn from_colorterm(value: Option<&std::ffi::OsStr>) -> Self {
        match value.and_then(std::ffi::OsStr::to_str) {
            Some(value) if value.eq_ignore_ascii_case("truecolor") => Self::TrueColor,
            Some(value) if value.eq_ignore_ascii_case("24bit") => Self::TrueColor,
            _ => Self::Indexed,
        }
    }
}

impl Theme {
    /// Names of the built-in themes, for `ui.theme`.
    pub(crate) const NAMES: &'static [&'static str] = &[
        "mc-classic",
        "terminal",
        "noon-dark",
        "noon-light",
        "catppuccin-mocha",
        "catppuccin-latte",
    ];

    /// This theme for a panel where the user is root: its cursor on red.
    pub(crate) fn for_root(&self) -> Self {
        Self {
            cursor: self.root_cursor,
            marked_cursor: self.root_marked_cursor,
            ..self.clone()
        }
    }

    /// A built-in theme by name, in the colors that `depth` allows.
    pub(crate) fn by_name(name: &str, depth: ColorDepth) -> Option<Self> {
        let palette = |palette: Palette| match depth {
            ColorDepth::TrueColor => palette,
            ColorDepth::Indexed => palette.indexed(),
        };
        let noon = |noon: Noon| match depth {
            ColorDepth::TrueColor => noon,
            ColorDepth::Indexed => noon.indexed(),
        };
        match name {
            "mc-classic" => Some(Self::mc_classic()),
            "terminal" => Some(Self::terminal()),
            "noon-dark" => Some(Self::noon(&noon(Noon::DARK))),
            "noon-light" => Some(Self::noon(&noon(Noon::LIGHT))),
            "catppuccin-mocha" => Some(Self::catppuccin(&palette(Palette::MOCHA))),
            "catppuccin-latte" => Some(Self::catppuccin(&palette(Palette::LATTE))),
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
            // The only one of the 16 colors that stands out from blue and keeps the names'.
            cursor_inactive: Style::new().bg(Color::DarkGray),
            marked: on(Color::LightYellow, Color::Blue).underlined(),
            marked_cursor: on(Color::LightYellow, Color::Cyan).underlined(),
            // As the titles of error dialogs.
            root_cursor: on(Color::White, Color::Red),
            root_marked_cursor: on(Color::LightYellow, Color::Red).underlined(),
            quick_search: on(Color::Black, Color::Cyan),
            // As mc's editor shows a bookmark found (`bookmarkfound`).
            find_match: on(Color::Black, Color::Green),
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
            message: fg(Color::LightRed),
            root: fg(Color::LightRed).bold(),
            truncated: fg(Color::LightYellow).bold(),
            truncated_cursor: on(Color::LightYellow, Color::Black).bold(),
            // Yellow does not read on the gray of dialogs, nor on the cyan of menus.
            truncated_dialog: fg(Color::Red).bold(),
            truncated_menu: fg(Color::Red).bold(),
            // A cell of its own, which reads on the cyan bars and the black line of tabs.
            truncated_bar: on(Color::LightYellow, Color::Black).bold(),
            truncated_error: fg(Color::LightYellow).bold(),
            // Dark colors, as mc draws on its gray dialogs; idle takes the dialog's black.
            dialog_host: [
                Style::new(),
                fg(Color::Yellow),
                fg(Color::Green),
                fg(Color::Red),
            ],
            fkey_number: on(Color::White, Color::Black),
            fkey_label: on(Color::Black, Color::Cyan),
            // mc's `[menu]` colors.
            menu_bar: on(Color::White, Color::Cyan),
            menu_bar_selected: on(Color::White, Color::Black),
            menu_bar_inactive: on(Color::Black, Color::Cyan),
            menu: on(Color::White, Color::Cyan),
            menu_selected: on(Color::White, Color::Black),
            menu_hotkey: fg(Color::LightYellow),
            menu_disabled: fg(Color::DarkGray),
            // Magenta does not read on the cyan of menus; the labels there are white.
            menu_keys: fg(Color::Black),
            dialog: on(Color::Black, Color::Gray),
            dialog_title: on(Color::Blue, Color::Gray),
            keys: on(Color::Magenta, Color::Gray),
            dialog_button: on(Color::Black, Color::Gray),
            dialog_button_focused: on(Color::Black, Color::Cyan),
            dialog_input: on(Color::Black, Color::Cyan),
            gauge: on(Color::White, Color::Black),
            error_dialog: on(Color::White, Color::Red),
            error_title: on(Color::LightYellow, Color::Red),
            error_button_focused: on(Color::Black, Color::Gray),
            // Dark, as the F-key bar below the panels.
            tab: on(Color::Gray, Color::Black),
            tab_active: on(Color::White, Color::Blue).bold(),
            tab_active_idle: on(Color::Gray, Color::Blue),
            tab_number: fg(Color::LightYellow),
            shadow: Some(on(Color::DarkGray, Color::Black)),
            borders: Borders::default(),
            glyphs: &Glyphs::PLAIN,
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
            cursor_inactive: reversed.dim(),
            // Not bold as in mc: that is what directories are.
            marked: Style::new().underlined(),
            marked_cursor: reversed.underlined(),
            root_cursor: reversed.bold(),
            root_marked_cursor: reversed.underlined().bold(),
            quick_search: reversed,
            // Reversed is the cursor; this shows on it.
            find_match: plain.bold().underlined(),
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
            message: plain.bold(),
            root: plain.bold(),
            // Reversed on plain rows, and plain in the reversed cursor.
            truncated: reversed.bold(),
            truncated_cursor: plain.not_reversed().bold(),
            truncated_dialog: reversed.bold(),
            truncated_menu: reversed.bold(),
            // Plain on the reversed bars, and neither dim nor reversed on the line of tabs.
            truncated_bar: plain.not_reversed().not_dim().bold(),
            truncated_error: reversed.bold(),
            dialog_host: [plain.dim(), plain, plain.bold(), plain],
            fkey_number: plain,
            fkey_label: reversed,
            menu_bar: reversed,
            menu_bar_selected: plain,
            menu_bar_inactive: reversed,
            menu: plain,
            menu_selected: reversed,
            menu_hotkey: Style::new().underlined(),
            menu_disabled: Style::new().dim(),
            menu_keys: plain.bold(),
            dialog: plain,
            dialog_title: plain,
            keys: plain.bold(),
            dialog_button: plain,
            dialog_button_focused: reversed,
            dialog_input: reversed,
            gauge: plain,
            error_dialog: plain,
            error_title: plain.bold(),
            error_button_focused: reversed,
            tab: plain.dim(),
            tab_active: plain.bold().underlined(),
            tab_active_idle: plain.bold(),
            tab_number: plain,
            shadow: None,
            borders: Borders::default(),
            glyphs: &Glyphs::PLAIN,
        }
    }

    /// Catppuccin in the flavor `p`: panels on the base color, dialogs and menus a surface
    /// above it, accents for names and states; the same roles in every flavor.
    fn catppuccin(p: &Palette) -> Self {
        let on = |fg: Color, bg: Color| Style::new().fg(fg).bg(bg);
        let fg = |fg: Color| Style::new().fg(fg);
        let cursor = on(p.base, p.blue);
        Self {
            panel: on(p.text, p.base),
            panel_border: on(p.overlay0, p.base),
            panel_title_active: cursor,
            header: on(p.lavender, p.base),
            cursor,
            cursor_inactive: Style::new().bg(p.surface0),
            marked: on(p.mauve, p.base).underlined(),
            marked_cursor: on(p.base, p.mauve).underlined(),
            root_cursor: on(p.base, p.red),
            root_marked_cursor: on(p.base, p.red).underlined(),
            quick_search: cursor,
            find_match: on(p.base, p.yellow),
            file: fg(p.text),
            directory: fg(p.blue).bold(),
            executable: fg(p.green),
            symlink: fg(p.teal),
            stale_link: fg(p.red),
            device: fg(p.pink),
            special: fg(p.overlay1),
            host_idle: fg(p.overlay1),
            host_connecting: fg(p.yellow),
            host_connected: fg(p.green),
            host_failed: fg(p.red),
            address: fg(p.subtext0),
            message: fg(p.red),
            root: fg(p.red).bold(),
            truncated: fg(p.yellow).bold(),
            truncated_cursor: on(p.yellow, p.crust).bold(),
            truncated_dialog: fg(p.yellow).bold(),
            truncated_menu: fg(p.yellow).bold(),
            truncated_bar: fg(p.yellow).bold(),
            // Yellow does not read on red: the colors of the dialog, the other way round.
            truncated_error: on(p.red, p.base).bold(),
            dialog_host: [fg(p.subtext0), fg(p.yellow), fg(p.green), fg(p.red)],
            fkey_number: on(p.text, p.crust),
            fkey_label: on(p.text, p.surface0),
            menu_bar: on(p.text, p.surface0),
            menu_bar_selected: cursor,
            menu_bar_inactive: on(p.subtext0, p.mantle),
            menu: on(p.text, p.surface0),
            menu_selected: cursor,
            // Without a color of its own: an accent would not show on the selected command.
            menu_hotkey: Style::new().bold().underlined(),
            menu_disabled: fg(p.subtext0),
            menu_keys: fg(p.blue),
            dialog: on(p.text, p.surface0),
            dialog_title: on(p.mauve, p.surface0),
            keys: on(p.blue, p.surface0),
            dialog_button: on(p.text, p.surface0),
            dialog_button_focused: cursor,
            dialog_input: on(p.text, p.surface1),
            gauge: on(p.blue, p.surface0),
            error_dialog: on(p.base, p.red),
            error_title: on(p.base, p.red),
            error_button_focused: on(p.text, p.surface0),
            tab: on(p.overlay1, p.crust),
            tab_active: on(p.text, p.base).bold(),
            tab_active_idle: on(p.subtext0, p.base),
            tab_number: fg(p.blue),
            shadow: Some(on(p.overlay0, p.crust)),
            borders: Borders::default(),
            glyphs: &Glyphs::PLAIN,
        }
    }

    /// The colors of the logo in the variant `p`: a gold cursor and an amber menu bar, as the
    /// logo's selection and frame, with cream text in the dark variant and navy
    /// in the light one, where they are brighter; navy panels in the dark variant, cream ones
    /// with navy text in the light one.
    fn noon(p: &Noon) -> Self {
        let on = |fg: Color, bg: Color| Style::new().fg(fg).bg(bg);
        let fg = |fg: Color| Style::new().fg(fg);
        let cursor = on(p.ink, p.sun);
        let bar = on(p.ink, p.amber);
        Self {
            panel: on(p.text, p.panel),
            panel_border: on(p.line, p.panel),
            panel_title_active: cursor,
            header: on(p.accent, p.panel).bold(),
            cursor,
            cursor_inactive: Style::new().bg(p.cursor_inactive),
            marked: on(p.mark, p.panel).underlined(),
            marked_cursor: on(p.panel, p.mark).underlined(),
            root_cursor: on(p.panel, p.red),
            root_marked_cursor: on(p.panel, p.red).underlined(),
            quick_search: cursor,
            // The golds are the cursor's and the menu bar's.
            find_match: on(p.panel, p.teal),
            file: fg(p.text),
            directory: fg(p.bright).bold(),
            executable: fg(p.green),
            symlink: fg(p.teal),
            stale_link: fg(p.red),
            device: fg(p.pink),
            special: fg(p.faint),
            host_idle: fg(p.faint),
            host_connecting: fg(p.accent),
            host_connected: fg(p.green),
            host_failed: fg(p.red),
            address: fg(p.muted),
            message: fg(p.red),
            root: fg(p.red).bold(),
            truncated: fg(p.pink).bold(),
            truncated_cursor: on(p.sun, p.ink).bold(),
            truncated_dialog: fg(p.pink).bold(),
            truncated_menu: fg(p.pink).bold(),
            // A cell of its own, which reads on the amber menu bar and the deep F-key bar.
            truncated_bar: on(p.sun, p.ink).bold(),
            truncated_error: on(p.red, p.panel).bold(),
            dialog_host: [fg(p.muted), fg(p.accent), fg(p.green), fg(p.red)],
            fkey_number: on(p.accent, p.deep).bold(),
            fkey_label: on(p.text, p.deep),
            menu_bar: bar,
            menu_bar_selected: on(p.text, p.surface),
            menu_bar_inactive: bar,
            menu: on(p.text, p.surface),
            menu_selected: cursor,
            // Without a color of its own: an accent would not show on the selected command.
            menu_hotkey: Style::new().bold().underlined(),
            menu_disabled: fg(p.muted),
            menu_keys: fg(p.teal),
            dialog: on(p.text, p.surface),
            dialog_title: on(p.accent, p.surface).bold(),
            keys: on(p.teal, p.surface),
            dialog_button: on(p.text, p.surface),
            dialog_button_focused: cursor,
            dialog_input: on(p.text, p.field),
            gauge: on(p.accent, p.surface),
            error_dialog: on(p.panel, p.red),
            error_title: on(p.panel, p.red).bold(),
            error_button_focused: on(p.text, p.surface),
            tab: on(p.muted, p.deep),
            tab_active: on(p.text, p.panel).bold(),
            tab_active_idle: on(p.muted, p.panel),
            tab_number: fg(p.accent),
            shadow: Some(on(p.faint, p.deep)),
            borders: Borders::default(),
            glyphs: &Glyphs::PLAIN,
        }
    }

    /// This theme framed with `borders`, from `ui.borders`.
    pub(crate) fn with_borders(self, borders: Borders) -> Self {
        Self { borders, ..self }
    }

    /// This theme drawn with `glyphs`, as `ui.icons` picks them.
    pub(crate) fn with_glyphs(self, glyphs: &'static Glyphs) -> Self {
        Self { glyphs, ..self }
    }

    /// The frame of a panel or a dialog.
    pub(crate) fn border_type(&self) -> BorderType {
        match self.borders {
            Borders::Double => BorderType::Double,
            Borders::Single => BorderType::Plain,
        }
    }

    /// The lines of frames, by `borders`.
    pub(crate) fn lines(&self) -> &'static Lines {
        Lines::of(self.borders)
    }

    /// The ends of a single line across a frame, as mc joins them: `╟` and `╢` on a double one.
    pub(crate) fn tees(&self) -> (char, char) {
        self.lines().tees
    }

    /// Where a line between columns meets the top of the frame and the single line above the
    /// status line: `╤` and `┴` on a double frame.
    pub(crate) fn column_tees(&self) -> (char, char) {
        self.lines().column_tees
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

    /// `style` for what says that what was typed is wrong, such as the glyph of the regex mode
    /// before a text that is no regular expression: crossed out, in the color of a broken link.
    pub(crate) fn invalid(&self, style: Style) -> Style {
        let color = Style {
            fg: self.stale_link.fg,
            ..Style::new()
        };
        style.patch(color).crossed_out()
    }

    /// The style of a host's status marker.
    pub(crate) fn host_status(&self, status: HostStatus) -> Style {
        match status {
            HostStatus::Idle => self.host_idle,
            HostStatus::Connecting => self.host_connecting,
            HostStatus::Connected => self.host_connected,
            HostStatus::Failed | HostStatus::Stalled => self.host_failed,
        }
    }

    /// The style of a host's status icon or marker in a dialog, such as the location menu.
    pub(crate) fn dialog_host_status(&self, status: HostStatus) -> Style {
        let [idle, connecting, connected, failed] = self.dialog_host;
        match status {
            HostStatus::Idle => idle,
            HostStatus::Connecting => connecting,
            HostStatus::Connected => connected,
            HostStatus::Failed | HostStatus::Stalled => failed,
        }
    }
}

/// The colors of a Catppuccin flavor that the themes use, by their names in Catppuccin
/// (<https://github.com/catppuccin/palette>, v1.8.0).
#[derive(Debug, Clone, Copy)]
struct Palette {
    pink: Color,
    mauve: Color,
    red: Color,
    yellow: Color,
    green: Color,
    teal: Color,
    blue: Color,
    lavender: Color,
    text: Color,
    subtext0: Color,
    overlay1: Color,
    overlay0: Color,
    surface1: Color,
    surface0: Color,
    base: Color,
    mantle: Color,
    crust: Color,
}

impl Palette {
    const MOCHA: Self = Self {
        pink: Color::from_u32(0x00f5_c2e7),
        mauve: Color::from_u32(0x00cb_a6f7),
        red: Color::from_u32(0x00f3_8ba8),
        yellow: Color::from_u32(0x00f9_e2af),
        green: Color::from_u32(0x00a6_e3a1),
        teal: Color::from_u32(0x0094_e2d5),
        blue: Color::from_u32(0x0089_b4fa),
        lavender: Color::from_u32(0x00b4_befe),
        text: Color::from_u32(0x00cd_d6f4),
        subtext0: Color::from_u32(0x00a6_adc8),
        overlay1: Color::from_u32(0x007f_849c),
        overlay0: Color::from_u32(0x006c_7086),
        surface1: Color::from_u32(0x0045_475a),
        surface0: Color::from_u32(0x0031_3244),
        base: Color::from_u32(0x001e_1e2e),
        mantle: Color::from_u32(0x0018_1825),
        crust: Color::from_u32(0x0011_111b),
    };

    const LATTE: Self = Self {
        pink: Color::from_u32(0x00ea_76cb),
        mauve: Color::from_u32(0x0088_39ef),
        red: Color::from_u32(0x00d2_0f39),
        yellow: Color::from_u32(0x00df_8e1d),
        green: Color::from_u32(0x0040_a02b),
        teal: Color::from_u32(0x0017_9299),
        blue: Color::from_u32(0x001e_66f5),
        lavender: Color::from_u32(0x0072_87fd),
        text: Color::from_u32(0x004c_4f69),
        subtext0: Color::from_u32(0x006c_6f85),
        overlay1: Color::from_u32(0x008c_8fa1),
        overlay0: Color::from_u32(0x009c_a0b0),
        surface1: Color::from_u32(0x00bc_c0cc),
        surface0: Color::from_u32(0x00cc_d0da),
        base: Color::from_u32(0x00ef_f1f5),
        mantle: Color::from_u32(0x00e6_e9ef),
        crust: Color::from_u32(0x00dc_e0e8),
    };

    /// The nearest colors of the 256-color palette, for terminals without 24-bit color.
    fn indexed(self) -> Self {
        Self {
            pink: indexed(self.pink),
            mauve: indexed(self.mauve),
            red: indexed(self.red),
            yellow: indexed(self.yellow),
            green: indexed(self.green),
            teal: indexed(self.teal),
            blue: indexed(self.blue),
            lavender: indexed(self.lavender),
            text: indexed(self.text),
            subtext0: indexed(self.subtext0),
            overlay1: indexed(self.overlay1),
            overlay0: indexed(self.overlay0),
            surface1: indexed(self.surface1),
            surface0: indexed(self.surface0),
            base: indexed(self.base),
            mantle: indexed(self.mantle),
            crust: indexed(self.crust),
        }
    }
}

/// The colors of the Noon themes, from the logo (`assets/icons/logo.svg`), by role.
#[derive(Debug, Clone, Copy)]
struct Noon {
    /// Behind the others: the F-key bar, the line of tabs, the shadow.
    deep: Color,
    panel: Color,
    /// Dialogs and menus.
    surface: Color,
    /// Text fields.
    field: Color,
    /// Behind the row under the cursor in the inactive panel.
    cursor_inactive: Color,
    /// Panel frames.
    line: Color,
    faint: Color,
    muted: Color,
    text: Color,
    /// Directories.
    bright: Color,
    /// The cursor, as the logo's selection; twice as light in the light variant.
    sun: Color,
    /// The menu bar, as the logo's frame; twice as light in the light variant.
    amber: Color,
    /// Text on `sun` and `amber`: cream in the dark variant, navy in the light one.
    ink: Color,
    /// Headers, titles, the F-key numbers, and connecting hosts: a gold that reads on `panel`,
    /// `surface`, and `deep`.
    accent: Color,
    mark: Color,
    green: Color,
    teal: Color,
    red: Color,
    pink: Color,
}

impl Noon {
    const DARK: Self = Self {
        deep: Color::from_u32(0x000e_1a36),
        panel: Color::from_u32(0x001a_2b4e),
        surface: Color::from_u32(0x0022_3860),
        field: Color::from_u32(0x002d_4672),
        // `surface` is a single step of gray above `panel` in 256 colors.
        cursor_inactive: Color::from_u32(0x002d_4672),
        line: Color::from_u32(0x005a_72a0),
        faint: Color::from_u32(0x0070_84ab),
        muted: Color::from_u32(0x00a3_b0c8),
        text: Color::from_u32(0x00e2_e6ee),
        bright: Color::from_u32(0x00ff_fbea),
        sun: Color::from_u32(0x00a4_6d00),
        amber: Color::from_u32(0x0099_6400),
        ink: Color::from_u32(0x00ff_fbea),
        accent: Color::from_u32(0x00ff_c24a),
        mark: Color::from_u32(0x00ff_9447),
        green: Color::from_u32(0x009b_d67e),
        teal: Color::from_u32(0x006c_cfd6),
        red: Color::from_u32(0x00f2_6b5e),
        pink: Color::from_u32(0x00e5_9ae0),
    };

    const LIGHT: Self = Self {
        deep: Color::from_u32(0x00cf_d7e6),
        panel: Color::from_u32(0x00ff_fbea),
        surface: Color::from_u32(0x00e8_ecf4),
        field: Color::from_u32(0x00ff_ffff),
        cursor_inactive: Color::from_u32(0x00e8_ecf4),
        line: Color::from_u32(0x008c_9bbb),
        faint: Color::from_u32(0x006f_7c99),
        muted: Color::from_u32(0x004f_5e80),
        text: Color::from_u32(0x0026_375c),
        bright: Color::from_u32(0x0013_2040),
        sun: Color::from_u32(0x00ff_c24a),
        amber: Color::from_u32(0x00ff_b833),
        ink: Color::from_u32(0x0013_2040),
        accent: Color::from_u32(0x00a3_5400),
        mark: Color::from_u32(0x00c4_501a),
        green: Color::from_u32(0x003a_8a2c),
        teal: Color::from_u32(0x000f_7c88),
        red: Color::from_u32(0x00c4_2b3a),
        pink: Color::from_u32(0x00a8_439a),
    };

    /// The nearest colors of the 256-color palette, for terminals without 24-bit color.
    fn indexed(self) -> Self {
        Self {
            deep: indexed(self.deep),
            panel: indexed(self.panel),
            surface: indexed(self.surface),
            field: indexed(self.field),
            cursor_inactive: indexed(self.cursor_inactive),
            line: indexed(self.line),
            faint: indexed(self.faint),
            muted: indexed(self.muted),
            text: indexed(self.text),
            bright: indexed(self.bright),
            sun: indexed(self.sun),
            amber: indexed(self.amber),
            ink: indexed(self.ink),
            accent: indexed(self.accent),
            mark: indexed(self.mark),
            green: indexed(self.green),
            teal: indexed(self.teal),
            red: indexed(self.red),
            pink: indexed(self.pink),
        }
    }
}

/// The nearest color to `color` in the 6×6×6 cube or the gray ramp of the 256-color palette.
/// The 16 colors below them are left out: the terminal's palette decides those.
fn indexed(color: Color) -> Color {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let Color::Rgb(r, g, b) = color else {
        return color;
    };
    let distance = |(r2, g2, b2): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(r, r2) + d(g, g2) + d(b, b2)
    };
    let level = |value: u8| {
        (0u8..6)
            .min_by_key(|&index| (i32::from(LEVELS[usize::from(index)]) - i32::from(value)).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (
        LEVELS[usize::from(ri)],
        LEVELS[usize::from(gi)],
        LEVELS[usize::from(bi)],
    );
    // Gray `index` is 8 + 10 × index, for index 0 … 23.
    let mean = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let gray_index = u8::try_from((mean.saturating_sub(3) / 10).min(23)).unwrap_or(23);
    let gray = 8 + 10 * gray_index;
    if distance((gray, gray, gray)) < distance(cube) {
        Color::Indexed(232 + gray_index)
    } else {
        Color::Indexed(16 + 36 * ri + 6 * gi + bi)
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
        let themes = Theme::NAMES.iter().flat_map(|name| {
            [ColorDepth::TrueColor, ColorDepth::Indexed]
                .into_iter()
                .filter_map(move |depth| Theme::by_name(name, depth))
        });
        for theme in themes {
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
    fn marks_of_text_cut_short_stand_out_where_they_are_drawn() {
        for name in Theme::NAMES {
            for depth in [ColorDepth::TrueColor, ColorDepth::Indexed] {
                let theme = Theme::by_name(name, depth).unwrap();
                let surfaces = [
                    ("panel", theme.panel, theme.truncated),
                    ("cursor", theme.cursor, theme.truncated_cursor),
                    ("dialog", theme.dialog, theme.truncated_dialog),
                    ("menu", theme.menu, theme.truncated_menu),
                    ("selected", theme.menu_selected, theme.truncated_cursor),
                    ("F-key bar", theme.fkey_label, theme.truncated_bar),
                    ("menu bar", theme.menu_bar_inactive, theme.truncated_bar),
                    ("tabs", theme.tab, theme.truncated_bar),
                    ("error", theme.error_dialog, theme.truncated_error),
                ];
                for (surface, style, mark) in surfaces {
                    let marked = style.patch(mark);
                    assert_ne!(marked, style, "{name} {depth:?}: {surface}");
                    if let (Some(fg), Some(bg)) = (marked.fg, marked.bg) {
                        assert_ne!(fg, bg, "{name} {depth:?}: {surface}");
                    }
                }
            }
        }
    }

    #[test]
    fn matches_found_stand_out_on_the_panels_the_dialogs_and_the_cursor() {
        for name in Theme::NAMES {
            for depth in [ColorDepth::TrueColor, ColorDepth::Indexed] {
                let theme = Theme::by_name(name, depth).unwrap();
                let surfaces = [
                    ("panel", theme.panel),
                    ("dialog", theme.dialog),
                    ("cursor", theme.cursor),
                ];
                for (surface, style) in surfaces {
                    let found = style.patch(theme.find_match);
                    assert_ne!(found, style, "{name} {depth:?}: {surface}");
                    assert!(
                        found.bg.is_none() || found.bg != style.bg || found.fg != style.fg,
                        "{name} {depth:?}: {surface}"
                    );
                    if let (Some(fg), Some(bg)) = (found.fg, found.bg) {
                        assert_ne!(fg, bg, "{name} {depth:?}: {surface}");
                    }
                }
                let on_cursor = theme.cursor.patch(theme.find_match);
                if on_cursor.bg.is_some() {
                    assert_ne!(on_cursor.bg, theme.cursor.bg, "{name} {depth:?}");
                    assert_ne!(on_cursor.bg, theme.dialog.bg, "{name} {depth:?}");
                    assert_ne!(on_cursor.bg, theme.panel.bg, "{name} {depth:?}");
                }
            }
        }
    }

    #[test]
    fn the_inactive_cursor_stands_out_from_the_panel() {
        for name in Theme::NAMES {
            for depth in [ColorDepth::TrueColor, ColorDepth::Indexed] {
                let theme = Theme::by_name(name, depth).unwrap();
                let row = theme.panel.patch(theme.cursor_inactive);
                assert!(row != theme.panel, "{name} {depth:?}");
                assert_eq!(
                    theme.cursor_inactive.fg, None,
                    "{name}: keeps the names' colors"
                );
            }
        }
    }

    #[test]
    fn every_name_is_a_theme() {
        for name in Theme::NAMES {
            assert!(
                Theme::by_name(name, ColorDepth::TrueColor).is_some(),
                "{name}"
            );
        }
        assert_eq!(Theme::by_name("solarized", ColorDepth::TrueColor), None);
    }

    #[test]
    fn catppuccin_flavors_share_their_roles() {
        let theme = |name| Theme::by_name(name, ColorDepth::TrueColor).unwrap();
        let (mocha, latte) = (theme("catppuccin-mocha"), theme("catppuccin-latte"));
        assert_eq!(mocha.panel.bg, Some(Color::Rgb(0x1e, 0x1e, 0x2e)));
        assert_eq!(latte.panel.bg, Some(Color::Rgb(0xef, 0xf1, 0xf5)));
        assert_eq!(mocha.directory.fg, Some(Color::Rgb(0x89, 0xb4, 0xfa)));
        assert_eq!(latte.directory.fg, Some(Color::Rgb(0x1e, 0x66, 0xf5)));
        for theme in [mocha, latte] {
            assert_ne!(theme.panel.bg, theme.dialog.bg);
            assert_ne!(theme.dialog.bg, theme.dialog_input.bg);
            assert_eq!(theme.cursor, theme.dialog_button_focused);
        }
    }

    #[test]
    fn noon_variants_take_the_logo_colors() {
        let theme = |name| Theme::by_name(name, ColorDepth::TrueColor).unwrap();
        let (dark, light) = (theme("noon-dark"), theme("noon-light"));
        assert_eq!(dark.panel.bg, Some(Color::Rgb(0x1a, 0x2b, 0x4e)));
        assert_eq!(light.panel.bg, Some(Color::Rgb(0xff, 0xfb, 0xea)));
        assert_eq!(dark.cursor.bg, Some(Color::Rgb(0xa4, 0x6d, 0x00)));
        assert_eq!(light.cursor.bg, Some(Color::Rgb(0xff, 0xc2, 0x4a)));
        assert_eq!(
            dark.menu_bar_inactive.bg,
            Some(Color::Rgb(0x99, 0x64, 0x00))
        );
        assert_eq!(
            light.menu_bar_inactive.bg,
            Some(Color::Rgb(0xff, 0xb8, 0x33))
        );
        for theme in [&dark, &light] {
            assert_ne!(theme.panel.bg, theme.dialog.bg);
            assert_ne!(theme.dialog.bg, theme.dialog_input.bg);
        }
        for depth in [ColorDepth::TrueColor, ColorDepth::Indexed] {
            for name in ["noon-dark", "noon-light"] {
                let theme = Theme::by_name(name, depth).unwrap();
                let fg = |style: Style| style.fg;
                let kinds = [
                    fg(theme.directory),
                    fg(theme.executable),
                    fg(theme.symlink),
                    fg(theme.stale_link),
                    fg(theme.device),
                    fg(theme.marked),
                ];
                for (index, kind) in kinds.iter().enumerate() {
                    assert!(!kinds[index + 1..].contains(kind), "{name}: {kind:?}");
                }
            }
        }
    }

    #[test]
    fn rgb_comes_down_to_the_256_color_palette() {
        assert_eq!(indexed(Color::Rgb(0, 0, 0)), Color::Indexed(16));
        assert_eq!(indexed(Color::Rgb(255, 0, 0)), Color::Indexed(196));
        assert_eq!(indexed(Color::Rgb(255, 255, 255)), Color::Indexed(231));
        assert_eq!(indexed(Color::Rgb(0x1e, 0x1e, 0x2e)), Color::Indexed(235));
        assert_eq!(indexed(Color::Rgb(0x89, 0xb4, 0xfa)), Color::Indexed(111));
        assert_eq!(indexed(Color::Blue), Color::Blue);
        for name in [
            "catppuccin-mocha",
            "catppuccin-latte",
            "noon-dark",
            "noon-light",
        ] {
            let theme = Theme::by_name(name, ColorDepth::Indexed).unwrap();
            let layers = [
                theme.panel.bg,
                theme.dialog.bg,
                theme.dialog_input.bg,
                theme.fkey_number.bg,
            ];
            for (index, layer) in layers.iter().enumerate() {
                assert!(
                    matches!(layer, Some(Color::Indexed(16..))),
                    "{name}: {layer:?}"
                );
                assert!(!layers[index + 1..].contains(layer), "{name}: {layer:?}");
            }
            assert!(matches!(theme.directory.fg, Some(Color::Indexed(16..))));
        }
    }

    #[test]
    fn colorterm_tells_true_color() {
        let depth =
            |value: Option<&str>| ColorDepth::from_colorterm(value.map(std::ffi::OsStr::new));
        assert_eq!(depth(Some("truecolor")), ColorDepth::TrueColor);
        assert_eq!(depth(Some("24bit")), ColorDepth::TrueColor);
        assert_eq!(depth(Some("256color")), ColorDepth::Indexed);
        assert_eq!(depth(Some("")), ColorDepth::Indexed);
        assert_eq!(depth(None), ColorDepth::Indexed);
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
