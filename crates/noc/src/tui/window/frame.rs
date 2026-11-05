//! The frame of a window: its box with its title and shadow, the lines across it, and where a
//! list that pops up from it goes.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear};

use crate::tui::glyphs;
use crate::tui::theme::Theme;

use super::{Colors, MARGIN, box_width};

/// Draws a line across a window whose room inside is `inner`, on its row `y`, joined to the
/// frame as mc joins them: `╟───╢`, or `├───┤` with single lines. It sets the buttons apart
/// from what is above them.
pub(crate) fn draw_separator(
    frame: &mut Frame<'_>,
    inner: Rect,
    y: u16,
    colors: Colors,
    theme: &Theme,
) {
    let (left, right) = theme.tees();
    // The frame is a cell of padding and a cell of border away from the room inside.
    let x = inner.x.saturating_sub(2);
    let width = inner.width.saturating_add(4);
    let line = format!(
        "{left}{}{right}",
        glyphs::LINE.repeat(usize::from(width.saturating_sub(2)))
    );
    let row = Rect::new(x, y, width, 1).intersection(frame.area());
    frame.render_widget(Line::styled(line, colors.body), row);
}

/// The cells of the right side of the frame around `inner`, the room inside a box that
/// [`draw_box`] drew, from the row `y` down `height` rows: where its scroll bar goes.
pub(crate) fn right_frame(inner: Rect, y: u16, height: u16) -> Rect {
    Rect::new(inner.right().saturating_add(1), y, 1, height)
}

/// The frame around `inner`, the room inside a box that [`draw_box`] drew.
pub(crate) fn frame_around(inner: Rect) -> Rect {
    Rect::new(
        inner.x.saturating_sub(2),
        inner.y.saturating_sub(1),
        inner.width.saturating_add(4),
        inner.height.saturating_add(2),
    )
}

/// Where a list that pops up from `anchor` goes in `area`, `size` with its frame, from the
/// column `x`, as vim's pop-up menu: under `anchor`, or above it where there is more room there,
/// as tall as the room lets it be.
pub(crate) fn pop_up(anchor: Rect, area: Rect, x: u16, (width, height): (u16, u16)) -> Rect {
    let below = area.bottom().saturating_sub(anchor.bottom());
    let above = anchor.y.saturating_sub(area.y);
    let (y, height) = if below >= height || below >= above {
        (anchor.bottom(), height.min(below))
    } else {
        let height = height.min(above);
        (anchor.y - height, height)
    };
    let width = width.min(area.right().saturating_sub(x));
    Rect::new(x, y, width, height)
}

/// Draws an empty window box centered in `area`: a frame of `size` in the theme's lines, as
/// wide as [`box_width`] lets it be, with its title bold in the middle, a blank cell around it
/// where there is room, and mc's shadow. Returns the room inside, one column in from the frame
/// on either side.
pub(crate) fn draw_box(
    frame: &mut Frame<'_>,
    area: Rect,
    (width, height): (u16, u16),
    title: &str,
    colors: Colors,
    theme: &Theme,
) -> Rect {
    let size = (box_width(width, area), height);
    draw_frame(frame, area, size, title, colors, theme)
}

/// Draws an empty box as [`draw_box`] does, but of `size` as it is, where the screen has room:
/// for a strip that spans the screen, such as the key hints at its bottom.
pub(crate) fn draw_frame(
    frame: &mut Frame<'_>,
    area: Rect,
    (width, height): (u16, u16),
    title: &str,
    colors: Colors,
    theme: &Theme,
) -> Rect {
    // On a small screen the frame keeps its room and the margin gives way.
    let fit = |size: u16, room: u16| {
        let size = size.min(room);
        let margin = ((room - size) / 2).min(MARGIN);
        (size + 2 * margin, margin)
    };
    let (width, margin_x) = fit(width, area.width);
    let (height, margin_y) = fit(height, area.height);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let outer = Rect::new(x, y, width, height);
    frame.render_widget(Clear, outer);
    frame.render_widget(Block::new().style(colors.body), outer);
    if let Some(shadow) = theme.shadow {
        // Two columns to the right and a row below, as mc draws it.
        let right = Rect::new(outer.right(), outer.y + 1, 2, outer.height);
        let below = Rect::new(outer.x + 2, outer.bottom(), outer.width, 1);
        for rect in [right, below] {
            frame
                .buffer_mut()
                .set_style(rect.intersection(area), shadow);
        }
    }
    let title = Line::styled(format!(" {title} "), colors.title.bold()).centered();
    let block = Block::bordered()
        .border_type(theme.border_type())
        .title(title)
        .style(colors.body);
    let framed = outer.inner(Margin::new(margin_x, margin_y));
    let inner = block.inner(framed).inner(Margin::new(1, 0));
    frame.render_widget(block, framed);
    inner
}
