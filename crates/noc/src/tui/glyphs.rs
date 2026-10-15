//! Every special character Noon Commander draws, in one place: the Nerd Font glyphs it draws
//! with `ui.icons`, the ones every font has without them, and the lines of frames by
//! `ui.borders`. The rest of the code takes them from here and writes none itself, which `just
//! glyphs` checks; the text of Fluent files is text, not drawing.

use noc_config::Borders;
use noc_scroll::scrollbar::BarGlyphs;
use noc_text::Marks;

/// What goes in front of an entry's name by its kind: a Nerd Font icon, or mc's marker. A file
/// that devicons knows by its name takes devicons' icon instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kinds {
    pub(crate) dir: char,
    pub(crate) link_to_dir: char,
    pub(crate) link: char,
    pub(crate) broken_link: char,
    pub(crate) fifo: char,
    pub(crate) socket: char,
    pub(crate) char_device: char,
    pub(crate) block_device: char,
    pub(crate) executable: char,
    pub(crate) file: char,
}

/// The status of a host in one cell; while it connects, a frame of [`SPINNER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hosts {
    pub(crate) idle: char,
    pub(crate) connected: char,
    pub(crate) failed: char,
}

/// The characters that differ with icons and without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Glyphs {
    /// Nerd Font icons rather than mc's markers.
    pub(crate) icons: bool,
    pub(crate) kinds: Kinds,
    /// In front of `..`.
    pub(crate) parent: &'static str,
    /// In front of the home directory in the location menu.
    pub(crate) home: &'static str,
    /// In front of a volume in the location menu.
    pub(crate) volume: &'static str,
    pub(crate) network_volume: &'static str,
    pub(crate) hosts: Hosts,
    pub(crate) scroll_bar: BarGlyphs,
    /// The marks of text cut short or going on past the screen.
    pub(crate) marks: Marks,
    /// The marks of the pull-down menu's commands: the choice taken among others, and a setting
    /// that is on.
    pub(crate) menu_marks: (char, char),
    /// After a section of the help that has sections of its own: closed, and open.
    pub(crate) folds: (char, char),
    /// In front of each point of a tip in the help.
    pub(crate) bullet: char,
}

impl Glyphs {
    /// With a Nerd Font, `ui.icons`.
    pub(crate) const NERD: Self = Self {
        icons: true,
        kinds: Kinds {
            dir: '',
            link_to_dir: '',
            link: '',
            broken_link: '',
            fifo: '󰟥',
            socket: '󰐧',
            char_device: '󰋊',
            block_device: '󰋊',
            executable: '',
            file: '',
        },
        parent: " ",
        home: "󰋜 ",
        volume: "󰋊 ",
        network_volume: "󰣳 ",
        hosts: Hosts {
            idle: '󰒋',
            connected: '󰒋',
            failed: '󰒑',
        },
        scroll_bar: BarGlyphs {
            up: "",
            down: "",
            track: TRACK,
            thumb: THUMB,
        },
        marks: Marks {
            before: "󰄽",
            after: "󰄾",
        },
        menu_marks: ('•', '✓'),
        // nf-md-chevron_right and nf-md-chevron_down.
        folds: ('󰅂', '󰅀'),
        bullet: '•',
    };

    /// Without a Nerd Font: what every font has, mc's markers and Far's arrows.
    pub(crate) const PLAIN: Self = Self {
        icons: false,
        // mc's: `/` directory, `~` link to a directory, `@` link, `!` broken link, `|` FIFO,
        // `=` socket, `-` character device, `+` block device, `*` executable.
        kinds: Kinds {
            dir: '/',
            link_to_dir: '~',
            link: '@',
            broken_link: '!',
            fifo: '|',
            socket: '=',
            char_device: '-',
            block_device: '+',
            executable: '*',
            file: ' ',
        },
        parent: "/",
        home: "~ ",
        volume: "+ ",
        network_volume: "+ ",
        hosts: Hosts {
            idle: '○',
            connected: '●',
            failed: '✗',
        },
        scroll_bar: BarGlyphs {
            up: "▲",
            down: "▼",
            track: TRACK,
            thumb: THUMB,
        },
        marks: Marks {
            before: "«",
            after: "»",
        },
        menu_marks: ('*', 'x'),
        folds: ('▸', '▾'),
        bullet: '-',
    };

    /// The glyphs of `ui.icons`.
    pub(crate) fn of(icons: bool) -> &'static Self {
        if icons { &Self::NERD } else { &Self::PLAIN }
    }
}

/// The icons of the categories of the Configuration dialog and of the sections of the help,
/// shown with `ui.icons` only.
pub(crate) mod categories {
    pub(crate) const INTERFACE: &str = "󰍹";
    pub(crate) const TRANSFERS: &str = "󰓡";
    pub(crate) const SSH: &str = "󰣀";
    pub(crate) const VOLUMES: &str = "󰋊";
    pub(crate) const ZOXIDE: &str = "󰥨";
    pub(crate) const SHELL: &str = "󰆍";
    pub(crate) const SEARCH: &str = "󰍉";
    pub(crate) const ABOUT: &str = "󰋽";
    pub(crate) const KEYS: &str = "󰌌";
}

/// The track of a scroll bar.
const TRACK: &str = "░";
/// The thumb of a scroll bar.
const THUMB: &str = "█";

/// The frames of the spinner while something waits, one cell each.
pub(crate) const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

/// In the header of a panel, the order of the column it is sorted by.
pub(crate) const ASCENDING: char = '↑';
pub(crate) const DESCENDING: char = '↓';

/// What a checksum found: the one expected, another, or none, skipped.
pub(crate) const MATCH: char = '✓';
pub(crate) const MISMATCH: char = '✗';
pub(crate) const SKIPPED: char = '-';

/// The done and the rest of a progress bar.
pub(crate) const GAUGE_DONE: &str = "█";
pub(crate) const GAUGE_LEFT: &str = "░";

/// Where a command's output ended without a newline, as zsh marks it.
pub(crate) const NO_NEWLINE: &str = "⏎";

/// In a radio button, a check box, and a secret field, as mc draws them.
pub(crate) const CHOSEN: char = '*';
pub(crate) const CHECKED: char = 'x';
pub(crate) const SECRET: char = '*';

/// A single line across a dialog, a menu, or a list, and the line after the end of a text.
pub(crate) const LINE: &str = "─";
/// The line between columns, between tabs, and beside the categories of the Configuration
/// dialog, and where it meets a line across below.
pub(crate) const COLUMN: char = '│';
pub(crate) const COLUMN_FOOT: char = '┴';

/// The lines of frames, by `ui.borders`: double, as mc's, or single.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lines {
    /// Along the top of a frame.
    pub(crate) horizontal: char,
    /// Down its sides.
    pub(crate) vertical: char,
    /// The ends of a single line across it: `╟` and `╢` on a double frame.
    pub(crate) tees: (char, char),
    /// Where a line between columns meets its top, and the single line across it.
    pub(crate) column_tees: (char, char),
    /// Where the sides of a line of tabs above it meet its top corners: `╠` and `╣`.
    pub(crate) corner_tees: (char, char),
}

impl Lines {
    pub(crate) const DOUBLE: Self = Self {
        horizontal: '═',
        vertical: '║',
        tees: ('╟', '╢'),
        column_tees: ('╤', '┴'),
        corner_tees: ('╠', '╣'),
    };

    pub(crate) const SINGLE: Self = Self {
        horizontal: '─',
        vertical: '│',
        tees: ('├', '┤'),
        column_tees: ('┬', '┴'),
        corner_tees: ('├', '┤'),
    };

    /// The lines of `ui.borders`.
    pub(crate) fn of(borders: Borders) -> &'static Self {
        match borders {
            Borders::Double => &Self::DOUBLE,
            Borders::Single => &Self::SINGLE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_of_text_cut_short_are_double_chevrons() {
        let code = |mark: &str| mark.chars().map(u32::from).collect::<Vec<_>>();
        let nerd = Glyphs::of(true).marks;
        // nf-md-chevron_double_left and nf-md-chevron_double_right.
        assert_eq!(
            (code(nerd.before), code(nerd.after)),
            (vec![0xf013d], vec![0xf013e])
        );
        let plain = Glyphs::of(false).marks;
        assert_eq!((plain.before, plain.after), ("«", "»"));
    }

    #[test]
    fn every_glyph_takes_one_cell() {
        for glyphs in [Glyphs::NERD, Glyphs::PLAIN] {
            let bar = glyphs.scroll_bar;
            for text in [bar.up, bar.down, bar.track, bar.thumb, glyphs.marks.before] {
                assert_eq!(noc_text::width(text), 1, "{text:?}");
            }
            assert_eq!(noc_text::width(glyphs.marks.after), 1);
            let (closed, open) = glyphs.folds;
            assert_eq!(
                noc_text::width(&format!("{closed}{open}{}", glyphs.bullet)),
                3
            );
        }
    }
}
