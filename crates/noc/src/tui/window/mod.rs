//! What windows share: dialogs, lists, and the other windows over the panels. How wide they
//! are, their colors, frames, buttons, gauges, and the skeletons that stand for rows still to
//! come.

use ratatui::layout::Rect;

mod buttons;
mod colors;
mod frame;
mod gauge;
mod skeleton;

pub(crate) use buttons::{button_line, button_spots};
pub(crate) use colors::{Colors, Tone};
pub(crate) use frame::{draw_box, draw_frame, draw_separator, frame_around, pop_up, right_frame};
pub(crate) use gauge::{gauge, ratio, unknown_gauge};
pub(crate) use skeleton::{
    SKELETON_ROWS, cell_skeleton, draw_skeleton, draw_title_dots, shine_bar, status_skeleton,
};

/// Width of a window, in cells, borders included, where the screen has room.
pub(crate) const WIDTH: u16 = 76;
/// Part of the screen a large window takes, in percent of its width and of its height: a
/// window to read and scroll through rather than to choose from and close, as the help is.
const LARGE_PERCENT: (u16, u16) = (80, 80);
/// Fewest cells across a window's frame takes where the screen is too narrow to leave its
/// sides free, or the whole screen if narrower.
pub(crate) const MIN_WIDTH: u16 = 20;
/// Fewest rows a window takes, borders included: a row inside its frame. One with less room is
/// not drawn.
pub(crate) const MIN_HEIGHT: u16 = 3;
/// Cells a window's frame leaves free on either side of the screen, for its margin and its
/// shadow.
const SIDES: u16 = 2;
/// Cells between a window's frame and its edge, as mc leaves them.
const MARGIN: u16 = 1;

/// The width of the frame of a window that would be `width` cells wide, borders included, in
/// `area`: as wide as that, but leaving [`SIDES`] cells free on either side; where that leaves
/// too few, [`MIN_WIDTH`], or the whole of `area`. Every window that [`draw_box`] draws has it.
pub(crate) fn box_width(width: u16, area: Rect) -> u16 {
    let room = area.width.saturating_sub(2 * SIDES);
    width.min(room.max(area.width.min(MIN_WIDTH)))
}

/// The size of the frame of a large window in `area`, borders included: [`LARGE_PERCENT`] of
/// it, but never narrower than [`WIDTH`], nor lower than [`MIN_HEIGHT`], where it has room.
pub(crate) fn large(area: Rect) -> (u16, u16) {
    let (width, height) = LARGE_PERCENT;
    (
        percent(area.width, width).max(WIDTH),
        percent(area.height, height).max(MIN_HEIGHT),
    )
}

/// `percent` of `cells`, rounded down.
fn percent(cells: u16, percent: u16) -> u16 {
    u16::try_from(u32::from(cells) * u32::from(percent) / 100).unwrap_or(cells)
}

/// Whether a window framed at `outer` has room to be drawn: [`MIN_HEIGHT`] rows, and four cells
/// across, its two sides and two between them.
pub(crate) fn fits(outer: Rect) -> bool {
    outer.height >= MIN_HEIGHT && outer.width >= 4
}

/// The cells of a line inside the frame of a window that would be `width` cells wide in `area`.
pub(crate) fn inner_width(width: u16, area: Rect) -> usize {
    usize::from(box_width(width, area).saturating_sub(4))
}

/// The width and the height of the frame of the window on `screen`, a test backend's text: from
/// its top left corner to its bottom right one.
#[cfg(test)]
pub(crate) fn frame_size(screen: &str) -> (usize, usize) {
    use ratatui::symbols::line::{DOUBLE_BOTTOM_LEFT, DOUBLE_TOP_LEFT, DOUBLE_TOP_RIGHT};
    let rows: Vec<&str> = screen.lines().collect();
    let top = rows.iter().position(|row| row.contains(DOUBLE_TOP_LEFT));
    let bottom = rows.iter().position(|row| row.contains(DOUBLE_BOTTOM_LEFT));
    let (top, bottom) = (top.unwrap(), bottom.unwrap());
    // Characters before the corner, not bytes.
    let column = |corner| rows[top].split(corner).next().unwrap().chars().count();
    let width = column(DOUBLE_TOP_RIGHT) - column(DOUBLE_TOP_LEFT) + 1;
    (width, bottom - top + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_window_takes_its_part_of_the_screen_but_never_less_than_a_window() {
        assert_eq!(large(Rect::new(0, 0, 200, 50)), (160, 40));
        // 80% of 80 cells would be narrower than any other window.
        assert_eq!(large(Rect::new(0, 0, 80, 24)), (WIDTH, 19));
        assert_eq!(large(Rect::new(0, 0, 40, 2)).1, MIN_HEIGHT);
        // Where the screen is narrower, the box gives way, as every window's does.
        let narrow = Rect::new(0, 0, 60, 20);
        assert_eq!(box_width(large(narrow).0, narrow), 56);
    }
}
