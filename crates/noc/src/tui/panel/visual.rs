//! Visual mode: selecting rows.

use std::ops::RangeInclusive;

use super::{Panel, Row};

impl Panel {
    /// Whether Visual mode selects rows.
    pub(crate) fn visual(&self) -> bool {
        self.visual.is_some()
    }

    /// Starts Visual mode on the row under the cursor. With a count, it selects as many rows
    /// as the last command of Visual mode took, `count` times, or else `count` rows, as vim's
    /// `3V` does.
    pub(crate) fn start_visual(&mut self, count: Option<usize>, line: bool) {
        self.visual = Some(self.cursor);
        self.visual_line = line;
        let Some(count) = count else {
            return;
        };
        let rows = self.reselect.unwrap_or(1).saturating_mul(count);
        let last = self.rows().saturating_sub(1);
        self.cursor = self.cursor.saturating_add(rows.saturating_sub(1)).min(last);
    }

    /// `v` (or `V`, `line`) in Visual mode: the key that started it ends it, and the other
    /// switches its kind, as vim's.
    pub(crate) fn switch_visual(&mut self, line: bool) {
        if self.visual_line == line {
            self.end_visual();
        } else {
            self.visual_line = line;
        }
    }

    /// Whether Visual mode, or the last one, goes by lines, as vim's `V`.
    pub(crate) fn visual_line(&self) -> bool {
        self.visual_line
    }

    /// Ends Visual mode, and keeps what it selected for `g v`.
    pub(crate) fn end_visual(&mut self) {
        if let Some(anchor) = self.visual.take() {
            self.last_visual = Some((anchor, self.cursor));
        }
    }

    /// Selects again the rows Visual mode selected last, as vim's `g v`.
    pub(crate) fn reselect(&mut self) {
        if let Some((anchor, cursor)) = self.last_visual {
            let last = self.rows().saturating_sub(1);
            self.visual = Some(anchor.min(last));
            self.cursor = cursor.min(last);
        }
    }

    /// Puts the cursor at the other end of what Visual mode selects.
    pub(crate) fn other_end(&mut self) {
        if let Some(anchor) = &mut self.visual {
            std::mem::swap(anchor, &mut self.cursor);
        }
    }

    /// How many entries Visual mode selects, as vim's `showcmd` shows its lines: those the
    /// commands of files act on, so not `..`.
    pub(crate) fn visual_size(&self) -> Option<usize> {
        let rows = self.visual_rows()?;
        Some(
            rows.filter(|&index| matches!(self.row(index), Some(Row::Entry(_))))
                .count(),
        )
    }

    /// The rows Visual mode selects.
    pub(super) fn visual_rows(&self) -> Option<RangeInclusive<usize>> {
        let anchor = self.visual?.min(self.rows().saturating_sub(1));
        Some(anchor.min(self.cursor)..=anchor.max(self.cursor))
    }

    /// Marks the entries Visual mode selects, and ends it. With `only`, for a command that acts
    /// on them, they are the only marked ones, and the cursor goes to the first.
    pub(crate) fn mark_visual(&mut self, only: bool) {
        let Some(rows) = self.visual_rows() else {
            return;
        };
        if only {
            self.marked.clear();
        }
        self.reselect = Some(rows.clone().count());
        let first = *rows.start();
        for index in rows {
            if let Some(Row::Entry(entry)) = self.row(index) {
                self.marked.insert(entry.name.clone());
            }
        }
        self.end_visual();
        // As after an operator of vim's Visual mode, the cursor goes to the first row.
        if only {
            self.cursor = first;
        }
    }
}
