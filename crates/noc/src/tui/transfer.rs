//! The dialog of F5, F6, and Shift+F5: one for copying and moving, with the same field and the
//! same check box, whose words alone tell the two apart. Preserve attributes applies to a move
//! that copies, between file systems, as to a copy.

use super::dialog::Dialog;
use crate::i18n::fl;

/// The check box of attributes.
const PRESERVE: usize = 0;

/// Whether a transfer copies or moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Copy,
    Move,
}

impl Kind {
    /// How the Fluent messages select it.
    fn arg(self) -> &'static str {
        match self {
            Kind::Copy => "copy",
            Kind::Move => "move",
        }
    }

    /// The title of its dialog, and of the window of its job.
    pub(crate) fn title(self) -> String {
        fl!("transfer-title", kind = self.arg())
    }

    /// Above the entry its job works on.
    pub(crate) fn working(self) -> String {
        fl!("transfer-working", kind = self.arg())
    }

    /// Why it cannot write to `path`.
    pub(crate) fn error(self, path: &str, reason: &str) -> String {
        fl!(
            "transfer-error",
            kind = self.arg(),
            path = path,
            reason = reason
        )
    }

    fn prompt(self, sources: &Sources) -> String {
        match sources {
            Sources::One(name) => fl!("transfer-one", kind = self.arg(), name = name.as_str()),
            Sources::Many { files, dirs } => {
                fl!(
                    "transfer-many",
                    kind = self.arg(),
                    files = files,
                    dirs = dirs
                )
            }
        }
    }
}

/// What the prompt names: an entry, its `&` doubled, or how many files and directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Sources {
    One(String),
    Many { files: usize, dirs: usize },
}

/// The dialog for `kind` of `sources`, its field open with a text and the cursor before its
/// character of that number, attributes preserved if `preserve`.
pub(crate) fn dialog(
    kind: Kind,
    sources: &Sources,
    (text, cursor): (&str, usize),
    preserve: bool,
) -> Dialog {
    let checks = [(fl!("transfer-preserve"), preserve)];
    Dialog::form(&kind.title(), &kind.prompt(sources), text, &checks)
        .with_completion()
        .with_cursor(cursor)
}

/// Whether the dialog preserves attributes.
pub(crate) fn preserve(dialog: &Dialog) -> bool {
    dialog.checked(PRESERVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_texts_follow_the_kind() {
        let many = Sources::Many { files: 2, dirs: 1 };
        assert_eq!(
            Kind::Move.prompt(&many),
            "&Move 2 files and 1 directory to:"
        );
        assert_eq!(Kind::Copy.title(), "Copy");
        assert_eq!(Kind::Move.working(), "Moving");
        assert_eq!(
            Kind::Copy.error("/x", "denied"),
            "Cannot copy to /x: denied"
        );
    }
}
