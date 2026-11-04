//! The dialog that saves a file (ADR 0059), for whatever has one to save: a field with the name
//! it offers, which takes a path from a directory, completed as F7 completes one, on the host of
//! that directory; the question of a copy about a taken name before it writes over a file; an
//! error if it cannot. What saves
//! gives the title, the directory, the name, and the bytes; the app writes them, and the panels
//! on the directory show the file.

use noc_vfs::Location;

use super::describe::Reason;
use super::dialog::{Dialog, Problem};
use crate::i18n::fl;

/// A file to save: what the dialog asks, and what goes in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Save {
    /// The title of the dialog, such as `Save checksums`.
    pub(crate) title: String,
    /// Where a name or a relative path points from, and the host the file goes to.
    pub(crate) dir: Location,
    /// The name the field offers.
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
}

impl Save {
    /// The dialog, its field with the name offered and the cursor after it.
    pub(crate) fn dialog(&self) -> Dialog {
        Dialog::form(&self.title, &fl!("save-prompt"), &self.name, &[]).with_completion()
    }
}

/// The file at `path` was not saved, for `reason`.
pub(crate) fn error(path: String, reason: impl Into<Reason>) -> Problem {
    Problem::new(fl!("save-error"), reason).at(path)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_dialog_offers_the_name_with_the_title_given() {
        let save = Save {
            title: "Save checksums".to_owned(),
            dir: Location::Local(PathBuf::from("/srv")),
            name: "SHA256SUMS".to_owned(),
            bytes: Vec::new(),
        };
        let dialog = save.dialog();
        assert_eq!(dialog.text(), "SHA256SUMS");
        assert!(dialog.completes(), "a path, completed");
    }
}
