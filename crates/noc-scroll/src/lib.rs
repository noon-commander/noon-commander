//! Paging through text that ends: how far the keys of vim and less move a page, and a scroll
//! bar.
//!
//! The help and the viewer page alike: every motion down stops with the end of the text at the
//! bottom of the screen, and never moves a page that is already past it back up.

pub mod scrollbar;

/// What a key asks of a page of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Up,
    Down,
    /// A page up, less the two rows that vim's Ctrl+b keeps on screen.
    PageUp,
    /// A page down, less the two rows that vim's Ctrl+f keeps on screen.
    PageDown,
    /// A whole page up, as less's `b`.
    WindowUp,
    /// A whole page down, as less's Space and `f`.
    WindowDown,
    /// Half a page, rounded down, as vim's 'scroll' and less's `u` and `d`.
    HalfPageUp,
    HalfPageDown,
    /// The start of the text.
    Home,
    /// The end of the text at the bottom of the screen.
    End,
}

/// Where a motion goes, for a page of a given height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// That many rows up.
    Back(usize),
    /// That many rows down, but not past the end.
    On(usize),
    Start,
    End,
}

impl Motion {
    /// Where the motion goes on a page of `page` rows.
    pub fn step(self, page: usize) -> Step {
        let page = page.max(1);
        let half = (page / 2).max(1);
        match self {
            Self::Up => Step::Back(1),
            Self::Down => Step::On(1),
            Self::PageUp => Step::Back(page_less_overlap(page)),
            Self::PageDown => Step::On(page_less_overlap(page)),
            Self::WindowUp => Step::Back(page),
            Self::WindowDown => Step::On(page),
            Self::HalfPageUp => Step::Back(half),
            Self::HalfPageDown => Step::On(half),
            Self::Home => Step::Start,
            Self::End => Step::End,
        }
    }
}

/// The first row on screen after `motion` from `top`, in text of `rows` rows, `page` of which
/// show.
pub fn scrolled(motion: Motion, top: usize, rows: usize, page: usize) -> usize {
    let last = rows.saturating_sub(page.max(1));
    match motion.step(page) {
        Step::Back(count) => top.saturating_sub(count),
        Step::On(count) => top.saturating_add(count).min(last.max(top)),
        Step::Start => 0,
        Step::End => last,
    }
}

/// How far vim's Ctrl+f and Ctrl+b scroll a window of `height` rows: all but the two rows
/// they keep on screen, one in a window of four, none in a lower one, as vim's
/// `get_scroll_overlap` has it.
fn page_less_overlap(height: usize) -> usize {
    match height {
        0..=3 => height.max(1),
        4 => 3,
        _ => height - 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_f_keeps_two_rows_on_screen_as_vims_and_space_turns_a_whole_page_as_less() {
        assert_eq!([1, 3, 4, 5, 23].map(page_less_overlap), [1, 3, 3, 3, 21]);
        assert_eq!(Motion::PageDown.step(10), Step::On(8));
        assert_eq!(Motion::WindowDown.step(10), Step::On(10));
        assert_eq!(Motion::HalfPageUp.step(5), Step::Back(2), "rounded down");
        assert_eq!(Motion::HalfPageUp.step(1), Step::Back(1));
    }

    #[test]
    fn every_motion_down_stops_with_the_last_row_at_the_bottom() {
        for motion in [
            Motion::Down,
            Motion::PageDown,
            Motion::WindowDown,
            Motion::HalfPageDown,
            Motion::End,
        ] {
            assert_eq!(scrolled(motion, 19, 30, 10), 20, "{motion:?}");
            assert_eq!(scrolled(motion, 20, 30, 10), 20, "{motion:?}");
        }
        assert_eq!(scrolled(Motion::PageDown, 0, 30, 10), 8);
        assert_eq!(scrolled(Motion::PageUp, 5, 30, 10), 0);
        assert_eq!(scrolled(Motion::Home, 20, 30, 10), 0);
    }

    #[test]
    fn text_shorter_than_the_page_never_scrolls() {
        assert_eq!(scrolled(Motion::PageDown, 0, 5, 10), 0);
        assert_eq!(scrolled(Motion::End, 0, 5, 10), 0);
        assert_eq!(scrolled(Motion::Down, 0, 0, 10), 0);
    }

    #[test]
    fn a_page_past_the_end_stays_until_moved_up() {
        assert_eq!(
            scrolled(Motion::Down, 25, 30, 10),
            25,
            "after the screen grew"
        );
        assert_eq!(scrolled(Motion::Up, 25, 30, 10), 24);
        assert_eq!(scrolled(Motion::End, 25, 30, 10), 20);
    }
}
