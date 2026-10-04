//! The file viewer of Noon Commander, F3: the start of a file, read through any
//! [`Vfs`](noc_vfs::Vfs), shown as text that scrolls, with long lines wrapped or cut.
//!
//! Unlike the other library crates, this one is a piece of the UI and has text of its own, in
//! `i18n/`. It knows nothing of the app's keymap, theme, or tasks: the app maps its keys to
//! [`Command`]s, gives [`Styles`] from its theme, reads the file with [`read_start`] where it
//! runs its tasks, and closes the viewer itself.

mod i18n;
mod read;
mod text;
mod view;

pub use i18n::select_language;
pub use read::{LIMIT, read_start};
pub use view::{Command, Styles, Viewer};
