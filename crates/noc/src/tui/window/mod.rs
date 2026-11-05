//! What windows share: dialogs, lists, and the other windows over the panels. How wide they
//! are, their colors, frames, buttons, and the skeletons that stand for rows still to come.

use ratatui::layout::Rect;

mod buttons;
mod colors;
mod frame;
mod skeleton;

pub(crate) use buttons::{button_line, button_spots};
pub(crate) use colors::{Colors, Tone};
pub(crate) use frame::{draw_box, draw_frame, draw_separator, frame_around, pop_up, right_frame};
pub(crate) use skeleton::{
    SKELETON_ROWS, draw_skeleton, draw_title_dots, shine_bar, status_skeleton,
};

/// Width of a window, in cells, borders included, where the screen has room.
pub(crate) const WIDTH: u16 = 76;
/// Fewest cells across a window's frame takes where the screen is too narrow to leave its
/// sides free, or the whole screen if narrower.
pub(crate) const MIN_WIDTH: u16 = 20;
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

/// The cells of a line inside the frame of a window that would be `width` cells wide in `area`.
pub(crate) fn inner_width(width: u16, area: Rect) -> usize {
    usize::from(box_width(width, area).saturating_sub(4))
}
