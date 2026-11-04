//! File operation engine for Noon Commander: jobs that work on any [`Vfs`](noc_vfs::Vfs) backend,
//! report their progress, ask what to do when something fails, and stop when cancelled.
//!
//! Copying, moving, renaming, deleting, and checksums are here.

mod checksum;
mod copy;
mod delete;
mod free_name;
mod job;
mod rename;
mod spelling;
#[cfg(test)]
mod testing;

pub use checksum::{Algorithm, Checksum, Files, Hasher, Sum, checksum};
pub use copy::{CopyOptions, Endpoint, copy, copy_within, move_within};
pub use delete::delete;
pub use free_name::free_name;
pub use job::{
    Choice, Conflict, Decision, Event, Later, Outcome, Progress, Reporter, Resolution, Rest, Step,
    in_time,
};
pub use rename::{RenameError, rename};
