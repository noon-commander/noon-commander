//! Marking entries and the totals of what is marked and shown.

use noc_vfs::DirEntry;

use crate::tui::keymap::Action;

use super::{Listing, Panel, Row};

impl Panel {
    /// Marks the entry under the cursor, or unmarks it; `..` cannot be marked.
    pub(super) fn toggle_mark(&mut self) {
        if let Some(Row::Entry(entry)) = self.row(self.cursor) {
            let name = entry.name.clone();
            if !self.marked.remove(&name) {
                self.marked.insert(name);
            }
        }
    }

    /// Marks or unmarks `count` rows from the cursor down, or up, as Space or `K` typed that
    /// many times, as far as the listing goes: the row at its end turns once.
    pub(crate) fn mark_rows(&mut self, down: bool, count: usize) {
        let action = if down { Action::Mark } else { Action::MarkUp };
        for _ in 0..count {
            let at = self.cursor;
            self.handle(action);
            if self.cursor == at {
                break;
            }
        }
    }

    /// Inverts the marks on the shown entries that are not directories, as mc does by default.
    pub(super) fn invert_marks(&mut self) {
        let Listing::Dir(entries) = &self.listing;
        for &index in &self.shown {
            let entry = &entries[index];
            if !entry.is_dir_like() && !self.marked.remove(&entry.name) {
                self.marked.insert(entry.name.clone());
            }
        }
    }

    /// Marks, or unmarks, the shown entries for which `test` holds.
    pub(crate) fn mark_where(&mut self, mark: bool, mut test: impl FnMut(&DirEntry) -> bool) {
        let Listing::Dir(entries) = &self.listing;
        for &index in &self.shown {
            let entry = &entries[index];
            if !test(entry) {
                continue;
            }
            if mark {
                self.marked.insert(entry.name.clone());
            } else {
                self.marked.remove(&entry.name);
            }
        }
    }

    /// How many entries are marked, and the bytes in the files among them.
    pub(super) fn marked_total(&self) -> (usize, u64) {
        let Listing::Dir(entries) = &self.listing;
        let bytes = entries
            .iter()
            .filter(|entry| !entry.is_dir_like() && self.marked.contains(&entry.name))
            .filter_map(|entry| entry.metadata.size)
            .sum();
        (self.marked.len(), bytes)
    }

    /// How many of the shown entries are files and how many directories, and the bytes in the
    /// files.
    pub(super) fn shown_total(&self) -> (usize, usize, u64) {
        let Listing::Dir(entries) = &self.listing;
        let (mut files, mut dirs, mut bytes) = (0, 0, 0u64);
        for entry in self.shown.iter().map(|&index| &entries[index]) {
            if entry.is_dir_like() {
                dirs += 1;
            } else {
                files += 1;
                bytes = bytes.saturating_add(entry.metadata.size.unwrap_or(0));
            }
        }
        (files, dirs, bytes)
    }
}
