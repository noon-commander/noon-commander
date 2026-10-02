//! File operation engine for Noon Commander: jobs that work on any [`Vfs`](noc_vfs::Vfs) backend,
//! report their progress, ask what to do when something fails, and stop when cancelled.
//!
//! Copying, moving, deleting, and checksums are here.

mod checksum;
mod copy;
mod delete;
mod job;
#[cfg(test)]
mod testing;

pub use checksum::{Algorithm, Checksum, Files, Hasher, Sum, checksum};
pub use copy::{CopyOptions, Endpoint, copy, move_within};
pub use delete::delete;
pub use job::{Conflict, Decision, Event, Outcome, Progress, Reporter};
