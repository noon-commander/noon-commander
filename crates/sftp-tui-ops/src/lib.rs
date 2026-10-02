//! File operation engine for sftp-tui: jobs that work on any [`Vfs`](sftp_tui_vfs::Vfs) backend,
//! report their progress, ask what to do when something fails, and stop when cancelled.
//!
//! Copying and deleting are here; moving follows (see `docs/roadmap.md`).

mod copy;
mod delete;
mod job;
#[cfg(test)]
mod testing;

pub use copy::{CopyOptions, Endpoint, copy};
pub use delete::delete;
pub use job::{Decision, Event, Outcome, Progress, Reporter};
