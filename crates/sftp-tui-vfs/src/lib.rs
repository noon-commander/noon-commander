//! Virtual file system for sftp-tui: the `Vfs` trait and its backends
//! (virtual root, local file system, SFTP).
//!
//! The virtual root is not a backend: it is [`Location::Root`], listed with [`root_entries`].

mod entry;
mod error;
mod local;
mod location;
mod remote_path;
mod sftp;
mod vfs;

#[cfg(test)]
mod fixture;

pub use entry::{DirEntry, FileKind, Metadata};
pub use error::VfsError;
pub use local::LocalFs;
pub use location::{Location, RootEntry, root_entries};
pub use remote_path::RemotePath;
pub use sftp::SftpFs;
pub use vfs::Vfs;
