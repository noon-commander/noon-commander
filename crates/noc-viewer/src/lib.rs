//! The file viewer of Noon Commander, F3: a file read in parts through any
//! [`Vfs`](noc_vfs::Vfs), shown as text that scrolls, with long lines wrapped or cut.
//!
//! Unlike the other library crates, this one is a piece of the UI and has text of its own, in
//! `i18n/`. It knows nothing of the app's keymap, theme, or tasks: the app maps its keys to
//! [`Command`]s, gives [`Styles`] from its theme, reads the parts [`Viewer::want`] asks for
//! with [`read_part`] where it runs its tasks, or cuts them from text it holds with [`part_of`],
//! and closes the viewer itself.

mod i18n;
mod parts;
mod read;
mod text;
mod view;

pub use i18n::select_language;
pub use read::{Part, Progress, Stamp, Want, part_of, read_part};
pub use view::{Command, Sizes, Styles, Taken, Viewer};
