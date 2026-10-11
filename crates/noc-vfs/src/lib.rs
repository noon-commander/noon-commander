//! Virtual file system for Noon Commander: the `Vfs` trait and its backends
//! (local file system, SFTP), and the mounted volumes, which the location menu lists.

mod entry;
mod error;
mod files;
mod local;
mod location;
mod remote_path;
mod sftp;
mod vfs;
mod volumes;

#[cfg(test)]
mod fixture;

pub use entry::{DirEntry, FileKind, Metadata};
pub use error::VfsError;
pub use files::{FileReader, FileWriter};
pub use local::{LocalFs, LocalReader, LocalWriter};
pub use location::Location;
pub use remote_path::RemotePath;
pub use sftp::{SftpFs, SftpReader, SftpWriter};
pub use vfs::{Vfs, VfsPath};
pub use volumes::{Space, Volume, VolumeKind, volumes};
