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

/// The status of a host in one cell; while it connects, a frame of [`Spinners::dots`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hosts {
    pub(crate) idle: char,
    pub(crate) connected: char,
    pub(crate) failed: char,
}

/// What turns while something waits, in one cell: the frames of the dots, and what stands still
/// in their place without animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Spinners {
    pub(crate) dots: &'static [char],
    pub(crate) still: char,
}

/// At the end of every field that searches: how it matches what is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchModes {
    pub(crate) plain: &'static str,
    pub(crate) fzf: &'static str,
    pub(crate) regex: &'static str,
    /// Shell patterns, as `g +` and `g -` take.
    pub(crate) glob: &'static str,
}

/// At the end of every field that searches, before its search mode: whether case counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaseModes {
    /// Once what is typed has a capital letter.
    pub(crate) smart: &'static str,
    /// Never.
    pub(crate) ignore: &'static str,
    /// Always.
    pub(crate) exact: &'static str,
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
    pub(crate) spinners: Spinners,
    /// The bars of a skeleton, which stand where rows are still to come.
    pub(crate) skeleton: char,
    /// On the line of the mode while Noon Commander runs as root.
    pub(crate) root: char,
    pub(crate) scroll_bar: BarGlyphs,
    /// The marks of text cut short or going on past the screen.
    pub(crate) marks: Marks,
    /// In the viewer, in place of what cannot be shown: invalid UTF-8, and characters the
    /// terminal would act on. Quiet, as mc's `.`, and one cell in every font.
    pub(crate) hidden: &'static str,
    /// After a section of the help that has sections of its own: closed, and open.
    pub(crate) folds: (char, char),
    /// At the end of a combo box, whose list opens under it.
    pub(crate) combo_box: char,
    /// In a check box that is on, and in front of a command of the pull-down menu that is on.
    pub(crate) check: char,
    /// In front of the choice taken among others, as the dot of a radio button: in the list of a
    /// combo box, and in the pull-down menu, as its sort order.
    pub(crate) choice: char,
    /// In front of each point of a tip in the help.
    pub(crate) bullet: char,
    pub(crate) search_modes: SearchModes,
    pub(crate) case_modes: CaseModes,
    /// Between a symlink's name and its target on the status line.
    pub(crate) link_arrow: &'static str,
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
        spinners: Spinners {
            dots: &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
            still: '⋯',
        },
        skeleton: '▬',
        // nf-md-shield_account.
        root: '󰢏',
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
        hidden: "·",
        // nf-md-chevron_right and nf-md-chevron_down.
        folds: ('󰅂', '󰅀'),
        // nf-md-chevron_down, as an open fold.
        combo_box: '󰅀',
        // nf-md-check.
        check: '󰄬',
        // nf-md-circle_medium.
        choice: '󰧞',
        bullet: '•',
        // nf-cod-whole_word, nf-md-approximately_equal, nf-cod-regex, and nf-seti-shell, which
        // is smaller than nf-md-asterisk.
        search_modes: SearchModes {
            plain: "",
            fzf: "󰾞",
            regex: "",
            glob: "",
        },
        // nf-md-format_letter_case_upper, nf-md-format_letter_case_lower, and
        // nf-cod-case_sensitive, as VS Code's Match Case.
        case_modes: CaseModes {
            smart: "󰬶",
            ignore: "󰬵",
            exact: "",
        },
        // nf-md-arrow_right.
        link_arrow: "󰁔",
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
        // Fonts without the smaller squares and braille have these.
        spinners: Spinners {
            dots: &['|', '/', '-', '\\'],
            still: '…',
        },
        skeleton: '-',
        // The prompt of root's shell.
        root: '#',
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
        hidden: "·",
        folds: ('▸', '▾'),
        // Far's arrow, as the scroll bar's.
        combo_box: '▼',
        // As the checks of checksums.
        check: '✓',
        // As Far's radio buttons, `(•)`.
        choice: '•',
        bullet: '-',
        search_modes: SearchModes {
            plain: "=",
            fzf: "~",
            regex: ".*",
            glob: "*",
        },
        case_modes: CaseModes {
            smart: "Aa",
            ignore: "aa",
            exact: "AA",
        },
        // As mc and `ls -l` write it.
        link_arrow: "->",
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
    // nf-md-animation_play.
    pub(crate) const ANIMATIONS: &str = "󰤺";
    pub(crate) const TRANSFERS: &str = "󰓡";
    pub(crate) const SSH: &str = "󰣀";
    pub(crate) const VOLUMES: &str = "󰋊";
    pub(crate) const ZOXIDE: &str = "󰥨";
    pub(crate) const SHELL: &str = "󰆍";
    pub(crate) const SEARCH: &str = "󰍉";
    // nf-md-eye.
    pub(crate) const VIEWER: &str = "󰈈";
    pub(crate) const ABOUT: &str = "󰋽";
    pub(crate) const KEYS: &str = "󰌌";
}

/// The track of a scroll bar.
const TRACK: &str = "░";
/// The thumb of a scroll bar.
const THUMB: &str = "█";

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
/// The gauge of a job that does not know yet how far it is.
pub(crate) const GAUGE_UNKNOWN: &str = "▒";

/// Where a command's output ended without a newline, as zsh marks it.
pub(crate) const NO_NEWLINE: &str = "⏎";

/// On the line of the mode, between the mode and the window that has the keys, as lualine
/// parts its sections.
pub(crate) const MODE_SEPARATOR: char = '·';

/// In a secret field, as mc draws it.
pub(crate) const SECRET: char = '*';

/// A single line across a dialog, a menu, or a list, and the line after the end of a text.
pub(crate) const LINE: &str = "─";
/// The line between columns, between tabs, and beside the categories of the Configuration
/// dialog, and where it meets a line across below it and above it.
pub(crate) const COLUMN: char = '│';
pub(crate) const COLUMN_FOOT: char = '┴';
pub(crate) const COLUMN_HEAD: char = '┬';

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
    /// Where a line between columns meets its bottom: `╧` on a double frame.
    pub(crate) bottom_tee: char,
}

impl Lines {
    pub(crate) const DOUBLE: Self = Self {
        horizontal: '═',
        vertical: '║',
        tees: ('╟', '╢'),
        column_tees: ('╤', '┴'),
        corner_tees: ('╠', '╣'),
        bottom_tee: '╧',
    };

    pub(crate) const SINGLE: Self = Self {
        horizontal: '─',
        vertical: '│',
        tees: ('├', '┤'),
        column_tees: ('┬', '┴'),
        corner_tees: ('├', '┤'),
        bottom_tee: '┴',
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
    fn the_arrow_of_a_link_is_nerd_fonts_or_ascii() {
        let code = |text: &str| text.chars().map(u32::from).collect::<Vec<_>>();
        // nf-md-arrow_right, in one cell.
        assert_eq!(code(Glyphs::NERD.link_arrow), [0xf0054]);
        assert_eq!(noc_text::width(Glyphs::NERD.link_arrow), 1);
        assert_eq!(Glyphs::PLAIN.link_arrow, "->");
    }

    #[test]
    fn every_glyph_takes_one_cell() {
        for glyphs in [Glyphs::NERD, Glyphs::PLAIN] {
            let bar = glyphs.scroll_bar;
            for text in [bar.up, bar.down, bar.track, bar.thumb, glyphs.marks.before] {
                assert_eq!(noc_text::width(text), 1, "{text:?}");
            }
            assert_eq!(noc_text::width(glyphs.marks.after), 1);
            assert_eq!(noc_text::width(&glyphs.root.to_string()), 1);
            let (closed, open) = glyphs.folds;
            assert_eq!(
                noc_text::width(&format!("{closed}{open}{}", glyphs.bullet)),
                3
            );
            assert_eq!(noc_text::width(&glyphs.combo_box.to_string()), 1);
            assert_eq!(noc_text::width(&glyphs.check.to_string()), 1);
            assert_eq!(noc_text::width(&glyphs.choice.to_string()), 1);
            let spinners = glyphs.spinners;
            let still = [spinners.still, glyphs.skeleton];
            for glyph in spinners.dots.iter().chain(&still) {
                assert_eq!(noc_text::width(&glyph.to_string()), 1, "{glyph:?}");
            }
        }
        for text in [GAUGE_DONE, GAUGE_LEFT, GAUGE_UNKNOWN] {
            assert_eq!(noc_text::width(text), 1, "{text:?}");
        }
    }
}
