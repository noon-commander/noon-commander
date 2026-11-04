//! The mouse on the panel: clicks, the wheel, and the scroll bar.

use noc_config::Wheel;
use noc_scroll::Motion;
use noc_scroll::scrollbar::Bar;
use ratatui::layout::Position;

use crate::tui::mouse::Scroll;

use super::Panel;

impl Panel {
    /// The row at `at` on screen at the last render, if there is one there.
    fn row_at(&self, at: Position) -> Option<usize> {
        if !self.list.contains(at) {
            return None;
        }
        let row = self.window.top() + usize::from(at.y - self.list.y);
        (row < self.rows()).then_some(row)
    }

    /// Puts the cursor on the row at `at`, as a click does; whether there is one there.
    pub(crate) fn click(&mut self, at: Position) -> bool {
        let row = self.row_at(at);
        if let Some(row) = row {
            self.cursor = row;
        }
        row.is_some()
    }

    /// Puts the cursor on the row at `at` and marks or unmarks it, as a right click does in
    /// Far; the cursor stays there.
    pub(crate) fn mark_at(&mut self, at: Position) {
        if self.click(at) {
            self.toggle_mark();
        }
    }

    /// Scrolls by `step` of the mouse wheel, down or up, as vim's Ctrl+e and Ctrl+y: the cursor
    /// moves only to stay on screen, with its margin. Where the rows cannot scroll that way, as
    /// when they all fit, the cursor moves as far instead, as in mc and Far.
    pub(crate) fn wheel(&mut self, step: Wheel, down: bool) {
        let page = self.page.max(1);
        let rows = match step {
            Wheel::Lines(lines) => usize::from(lines),
            Wheel::Page => page,
        };
        let count = self.rows();
        let top = self.window.top();
        self.window
            .scroll(down, rows, &mut self.cursor, count, page);
        if self.window.top() == top {
            self.cursor = if down {
                self.cursor
                    .saturating_add(rows)
                    .min(count.saturating_sub(1))
            } else {
                self.cursor.saturating_sub(rows)
            };
        }
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// Scrolls as a press on the scroll bar asks: its arrows as a row of the wheel, its track
    /// a page as Ctrl+b and Ctrl+f, and its thumb to where it was dragged; the cursor moves
    /// only to stay on screen, or as Ctrl+f moves it.
    pub(crate) fn scroll_bar(&mut self, scroll: Scroll) {
        let rows = self.rows();
        let page = self.page.max(1);
        match scroll {
            Scroll::By(Motion::Up | Motion::Down) => {
                let down = scroll == Scroll::By(Motion::Down);
                self.window.scroll(down, 1, &mut self.cursor, rows, page);
            }
            Scroll::By(Motion::PageUp) => self.page_by(false, 1),
            Scroll::By(Motion::PageDown) => self.page_by(true, 1),
            Scroll::By(_) => {}
            Scroll::To(top) => self.window.scroll_to(top, &mut self.cursor, rows, page),
        }
    }

    /// Keeps the cursor within the rows and on screen, as vim's window does.
    pub(super) fn scroll(&mut self, height: usize) {
        let rows = self.rows();
        self.window.follow(&mut self.cursor, rows, height);
    }
}
