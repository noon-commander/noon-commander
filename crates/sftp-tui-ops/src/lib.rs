//! File operation engine for sftp-tui: jobs that work on any [`Vfs`](sftp_tui_vfs::Vfs) backend,
//! report their progress, ask what to do when something fails, and stop when cancelled.
//!
//! Deleting is here; copying and moving follow (see `docs/roadmap.md`).

mod delete;
mod job;

pub use delete::delete;
pub use job::{Decision, Event, Outcome, Progress, Reporter};
