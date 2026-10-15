//! A vertical scroll bar for lists and pages taller than their room: an arrow at each end, and
//! between them the track with the thumb, which the mouse presses and drags.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::Line;

const TRACK: &str = "░";
const THUMB: &str = "█";
/// Fewest rows that take the arrows: they leave the track two, so that it still tells where
/// the page is.
const ROWS_WITH_ARROWS: u16 = 4;

/// The arrows at the two ends of a scroll bar, one cell each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrows {
    pub up: &'static str,
    pub down: &'static str,
}

impl Arrows {
    /// Arrows that every font has, as mc's and Far's.
    pub const PLAIN: Self = Self {
        up: "▲", down: "▼"
    };
}

impl Default for Arrows {
    fn default() -> Self {
        Self::PLAIN
    }
}

/// What the mouse pressed on a scroll bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Up,
    /// The track above the thumb.
    PageUp,
    /// The thumb, at its row counted from its first.
    Thumb(u16),
    /// The track below the thumb.
    PageDown,
    Down,
}

/// Where the thumb is in a track of `track` rows for `total` rows of which `page` show from
/// `offset`: its first row and its length. `None` when everything fits.
pub fn thumb(track: usize, total: usize, page: usize, offset: usize) -> Option<(usize, usize)> {
    if track == 0 || total <= page {
        return None;
    }
    let length = (track * page / total).clamp(1, track);
    let last = total - page;
    let first = (track - length) * offset.min(last) / last;
    Some((first, length))
}

/// A scroll bar down a single column, for `total` rows of which `page` show from `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bar {
    area: Rect,
    total: usize,
    page: usize,
    offset: usize,
}

impl Bar {
    /// The bar in `area`; `None` when everything fits, which needs none.
    pub fn new(area: Rect, (total, page, offset): (usize, usize, usize)) -> Option<Self> {
        let bar = Self {
            area: Rect { width: 1, ..area },
            total,
            page,
            offset,
        };
        bar.thumb().map(|_| bar)
    }

    /// The cells of the bar.
    pub fn area(&self) -> Rect {
        self.area
    }

    fn arrows(&self) -> bool {
        self.area.height >= ROWS_WITH_ARROWS
    }

    /// The first row of the track on screen, and its rows.
    fn track(&self) -> (u16, u16) {
        if self.arrows() {
            (self.area.y + 1, self.area.height - 2)
        } else {
            (self.area.y, self.area.height)
        }
    }

    /// The first row of the thumb and its length, in the track.
    fn thumb(&self) -> Option<(usize, usize)> {
        let (_, rows) = self.track();
        thumb(usize::from(rows), self.total, self.page, self.offset)
    }

    /// What is at `at`, if it is on the bar.
    pub fn part(&self, at: Position) -> Option<Part> {
        if !self.area.contains(at) {
            return None;
        }
        if self.arrows() && at.y == self.area.y {
            return Some(Part::Up);
        }
        if self.arrows() && at.y == self.area.bottom() - 1 {
            return Some(Part::Down);
        }
        let (top, _) = self.track();
        let (first, length) = self.thumb()?;
        let row = usize::from(at.y - top);
        Some(if row < first {
            Part::PageUp
        } else if row >= first + length {
            Part::PageDown
        } else {
            Part::Thumb(u16::try_from(row - first).unwrap_or(0))
        })
    }

    /// The first row to show with the thumb dragged by its row `grab` to the screen row `y`:
    /// the least that puts the thumb there, so that it stays where it was dragged.
    pub fn dragged(&self, grab: u16, y: u16) -> usize {
        let (top, rows) = self.track();
        let Some((_, length)) = self.thumb() else {
            return self.offset;
        };
        let room = usize::from(rows) - length;
        if room == 0 {
            return self.offset;
        }
        let first = i32::from(y) - i32::from(top) - i32::from(grab);
        let first = usize::try_from(first).unwrap_or(0).min(room);
        (first * (self.total - self.page)).div_ceil(room)
    }

    /// Draws the bar in `style`, with `arrows` at its ends where they fit.
    pub fn render(&self, frame: &mut Frame<'_>, style: Style, arrows: Arrows) {
        let Some((first, length)) = self.thumb() else {
            return;
        };
        let cell = |y| Rect::new(self.area.x, y, 1, 1);
        if self.arrows() {
            frame.render_widget(Line::styled(arrows.up, style), cell(self.area.y));
            let bottom = self.area.bottom() - 1;
            frame.render_widget(Line::styled(arrows.down, style), cell(bottom));
        }
        let (top, rows) = self.track();
        for row in 0..rows {
            let symbol = if (first..first + length).contains(&usize::from(row)) {
                THUMB
            } else {
                TRACK
            };
            frame.render_widget(Line::styled(symbol, style), cell(top + row));
        }
    }
}

/// Draws a scroll bar down the single column `area` for `total` rows of which `page` show
/// from `offset`, with `arrows` at its ends, and returns it for the mouse; nothing when
/// everything fits.
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    position: (usize, usize, usize),
    style: Style,
    arrows: Arrows,
) -> Option<Bar> {
    let bar = Bar::new(area, position)?;
    bar.render(frame, style, arrows);
    Some(bar)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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

    fn drawn(height: u16, position: (usize, usize, usize)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(1, height)).unwrap();
        terminal
            .draw(|frame| {
                render(frame, frame.area(), position, Style::new(), Arrows::PLAIN);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height).map(|y| buffer[(0, y)].symbol()).collect()
    }

    #[test]
    fn arrows_end_the_bar_where_they_leave_the_track_two_rows() {
        assert_eq!(drawn(12, (20, 10, 0)), "▲█████░░░░░▼");
        assert_eq!(drawn(12, (20, 10, 10)), "▲░░░░░█████▼");
        assert_eq!(drawn(4, (20, 10, 0)), "▲█░▼");
        assert_eq!(drawn(3, (20, 10, 0)), "█░░", "no room for arrows");
        assert_eq!(drawn(12, (10, 10, 0)), "            ", "everything fits");
    }

    #[test]
    fn tells_the_arrows_the_track_and_the_thumb() {
        let bar = Bar::new(Rect::new(5, 2, 1, 12), (20, 10, 0)).unwrap();
        let part = |y| bar.part(Position::new(5, y));
        assert_eq!(part(2), Some(Part::Up));
        assert_eq!(part(3), Some(Part::Thumb(0)));
        assert_eq!(part(7), Some(Part::Thumb(4)));
        assert_eq!(part(8), Some(Part::PageDown));
        assert_eq!(part(13), Some(Part::Down));
        assert_eq!(part(14), None, "below");
        assert_eq!(bar.part(Position::new(4, 3)), None, "beside");
        let end = Bar::new(Rect::new(5, 2, 1, 12), (20, 10, 10)).unwrap();
        assert_eq!(end.part(Position::new(5, 3)), Some(Part::PageUp));
        assert_eq!(Bar::new(Rect::new(5, 2, 1, 12), (10, 10, 0)), None);
    }

    #[test]
    fn a_dragged_thumb_scrolls_to_where_it_is_and_stops_at_the_ends() {
        // Ten rows of track, a thumb of five: each row it moves is two rows of text.
        let bar = Bar::new(Rect::new(0, 0, 1, 12), (20, 10, 0)).unwrap();
        assert_eq!(bar.dragged(0, 1), 0);
        assert_eq!(bar.dragged(0, 2), 2);
        assert_eq!(bar.dragged(2, 5), 4, "by the row it was grabbed at");
        assert_eq!(bar.dragged(0, 6), 10);
        assert_eq!(bar.dragged(0, 11), 10, "past the bottom");
        assert_eq!(bar.dragged(3, 0), 0, "past the top");
        for offset in 0..=990 {
            let long = Bar::new(Rect::new(0, 0, 1, 12), (1000, 10, offset)).unwrap();
            let (first, _) = long.thumb().unwrap();
            let y = u16::try_from(first).unwrap() + 1;
            let moved = Bar::new(Rect::new(0, 0, 1, 12), (1000, 10, long.dragged(0, y)));
            assert_eq!(moved.unwrap().thumb(), long.thumb(), "stays at {y}");
        }
    }
}
