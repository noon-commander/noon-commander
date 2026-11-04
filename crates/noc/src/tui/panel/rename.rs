//! Renaming in the row, Shift+F6.

use noc_vfs::DirEntry;

use crate::tui::dialog::Field;

use super::{Panel, Renaming, Row, stem_chars};

impl Panel {
    /// Keeps the cursor on the entry being renamed, or stops renaming it once it is not shown.
    pub(super) fn keep_renaming(&mut self) {
        let Some(renaming) = &self.renaming else {
            return;
        };
        let row = (0..self.rows()).find(|&index| {
            matches!(self.row(index), Some(Row::Entry(entry)) if entry.name == renaming.name)
        });
        match row {
            Some(row) => self.cursor = row,
            None => self.renaming = None,
        }
    }

    /// Starts renaming the entry under the cursor in its row: the field holds its name, in
    /// Insert mode, with the cursor before the last extension of a file. `false` on `..`,
    /// volumes, and hosts, and for a name that is not UTF-8, which the field could not give
    /// back.
    pub(crate) fn start_rename(&mut self) -> bool {
        let Some(entry) = self.entry_under_cursor() else {
            return false;
        };
        let Ok(text) = std::str::from_utf8(&entry.name) else {
            return false;
        };
        let field = Field::at(text, stem_chars(text, entry.is_dir_like()));
        self.renaming = Some(Renaming {
            name: entry.name.clone(),
            field,
        });
        self.search = None;
        true
    }

    /// Starts renaming the entry `name` again, with `text` in the field and its cursor before
    /// its last extension; the cursor of the panel goes to it. `false` if it is not shown.
    pub(crate) fn rename_again(&mut self, name: &[u8], text: &str) -> bool {
        let row = (0..self.rows()).find(
            |&index| matches!(self.row(index), Some(Row::Entry(entry)) if entry.name == name),
        );
        let Some(row) = row else {
            return false;
        };
        self.cursor = row;
        let dir_like = self.entry_under_cursor().is_some_and(DirEntry::is_dir_like);
        self.renaming = Some(Renaming {
            name: name.to_vec(),
            field: Field::at(text, stem_chars(text, dir_like)),
        });
        self.search = None;
        true
    }

    /// Whether an entry is being renamed.
    pub(crate) fn renaming(&self) -> bool {
        self.renaming.is_some()
    }

    /// The field with the new name, while an entry is being renamed.
    pub(crate) fn rename_field(&mut self) -> Option<&mut Field> {
        self.renaming.as_mut().map(|renaming| &mut renaming.field)
    }

    /// The field with the new name, to read, while an entry is being renamed.
    pub(crate) fn rename_field_ref(&self) -> Option<&Field> {
        self.renaming.as_ref().map(|renaming| &renaming.field)
    }

    /// Stops renaming: the name of the entry, and the text of the field.
    pub(crate) fn end_rename(&mut self) -> Option<(Vec<u8>, String)> {
        let renaming = self.renaming.take()?;
        Some((renaming.name, renaming.field.text().to_owned()))
    }
}
