//! The jump list: Ctrl+o and Tab.

use super::{Destination, Focus, ListRequest, MAX_JUMPS, Panel, Row};

impl Panel {
    /// Puts where the panel is in the jump list, before a jump from here.
    pub(crate) fn remember(&mut self) {
        let here = self.here();
        self.push_jump(here);
    }

    /// Where the cursor would be on `row` of this location.
    pub(super) fn spot(&self, row: usize) -> Destination {
        Destination {
            location: self.location.clone(),
            focus: self.focus_at(row),
        }
    }

    /// The entry on `row`, or the row itself if it is gone, as a focus.
    fn focus_at(&self, row: usize) -> Focus {
        match self.row(row) {
            Some(Row::Entry(entry)) => Focus::Near {
                name: entry.name.clone(),
                row,
            },
            Some(Row::Parent) | None => Focus::First,
        }
    }

    /// The rows Visual mode holds, by the entries on them, before the rows change: where it
    /// started, and those it selected last.
    pub(super) fn visual_focus(&self) -> (Option<Focus>, Option<(Focus, Focus)>) {
        let visual = self.visual.map(|row| self.focus_at(row));
        let last = self
            .last_visual
            .map(|(anchor, cursor)| (self.focus_at(anchor), self.focus_at(cursor)));
        (visual, last)
    }

    /// Puts what Visual mode holds back on its entries, after the rows changed, so that it
    /// never selects other entries than those it did.
    pub(super) fn restore_visual(
        &mut self,
        (visual, last): (Option<Focus>, Option<(Focus, Focus)>),
    ) {
        self.visual = visual.map(|focus| self.row_of(&focus));
        self.last_visual =
            last.map(|(anchor, cursor)| (self.row_of(&anchor), self.row_of(&cursor)));
    }

    /// Adds `spot` to the jump list as the newest jump; jumps that Ctrl+o went back over go.
    pub(super) fn push_jump(&mut self, spot: Destination) {
        // After Ctrl+o, the place it went to stays, as in vim's 'jumpoptions' stack.
        self.jumps.truncate(self.jump.saturating_add(1));
        if self.jumps.last() != Some(&spot) {
            self.jumps.push(spot);
        }
        if self.jumps.len() > MAX_JUMPS {
            self.jumps.remove(0);
        }
        self.jump = self.jumps.len();
    }

    /// Goes back to where the panel was before its last jump, as vim's Ctrl+o, `count` times;
    /// the listing to request, if that is another location. A jump that went nowhere, such as
    /// `G` on the last row, left the place where the panel still is: Ctrl+o goes there first,
    /// moving nothing, as vim's does.
    pub(crate) fn jump_back(&mut self, count: usize) -> Option<ListRequest> {
        let count = count.max(1);
        // As vim's, a count past the oldest jump goes nowhere, and keeps the list as it is.
        if self.jump.min(self.jumps.len()) < count {
            return None;
        }
        // Where the panel is goes in only after a jump, as vim's; later the places stay as
        // they were.
        if self.jump == self.jumps.len() {
            let here = self.here();
            self.jumps.push(here);
            if self.jumps.len() > MAX_JUMPS {
                self.jumps.remove(0);
                self.jump -= 1;
            }
        }
        self.jump -= count;
        self.travel(self.jumps[self.jump].clone())
    }

    /// Goes forward again where Ctrl+o came from, as vim's Ctrl+i, `count` times.
    pub(crate) fn jump_forward(&mut self, count: usize) -> Option<ListRequest> {
        let count = count.max(1);
        // As vim's, a count past the newest jump goes nowhere.
        if self.jump + count >= self.jumps.len() {
            return None;
        }
        self.jump += count;
        self.travel(self.jumps[self.jump].clone())
    }

    /// Goes to `to` without a new jump: within this location at once, else by its listing.
    fn travel(&mut self, to: Destination) -> Option<ListRequest> {
        if to.location == self.location && self.pending.is_none() {
            self.cursor = self.row_of(&to.focus);
            return None;
        }
        Some(self.open(to.location, to.focus))
    }
}
