//! A vertical scroll bar for lists and pages taller than their room.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

const TRACK: &str = "░";
const THUMB: &str = "█";

/// Where the thumb is in a track of `track` rows for `total` rows of which `page` show from
/// `offset`: its first row and its length. `None` when everything fits.
pub(crate) fn thumb(
    track: usize,
    total: usize,
    page: usize,
    offset: usize,
) -> Option<(usize, usize)> {
    if track == 0 || total <= page {
        return None;
    }
    let length = (track * page / total).clamp(1, track);
    let last = total - page;
    let first = (track - length) * offset.min(last) / last;
    Some((first, length))
}

/// Draws a scroll bar down the single column `area` for `total` rows of which `page` show
/// from `offset`; nothing when everything fits.
pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    (total, page, offset): (usize, usize, usize),
    style: Style,
) {
    let track = usize::from(area.height);
    let Some((first, length)) = thumb(track, total, page, offset) else {
        return;
    };
    for row in 0..area.height {
        let index = usize::from(row);
        let symbol = if (first..first + length).contains(&index) {
            THUMB
        } else {
            TRACK
        };
        let cell = Rect::new(area.x, area.y + row, 1, 1);
        frame.render_widget(Line::styled(symbol, style), cell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_shows_how_much_shows_and_where() {
        assert_eq!(thumb(10, 10, 10, 0), None, "everything fits");
        assert_eq!(thumb(10, 20, 10, 0), Some((0, 5)));
        assert_eq!(thumb(10, 20, 10, 10), Some((5, 5)), "at the end");
        assert_eq!(thumb(10, 20, 10, 99), Some((5, 5)), "past the end");
        assert_eq!(
            thumb(10, 1000, 10, 495),
            Some((4, 1)),
            "never less than a row"
        );
        assert_eq!(thumb(0, 20, 10, 0), None);
    }
}
