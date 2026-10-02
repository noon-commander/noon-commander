use std::ffi::OsStr;
use std::fmt;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::{DirEntry, FileReader, FileWriter, Metadata, RemotePath, VfsError};

/// What code that works on any backend needs from its paths.
pub trait VfsPath: Clone + fmt::Debug + Send + Sync + 'static {
    /// The entry `name`, as a directory listing names it, of the directory at this path.
    #[must_use]
    fn join_name(&self, name: &[u8]) -> Self;

    /// The last component; `None` for `/` and the empty path.
    fn name(&self) -> Option<&[u8]>;

    /// The path without its last component; `None` for `/` and the empty path.
    fn parent(&self) -> Option<Self>;

    /// The path for messages, with invalid UTF-8 replaced.
    fn display(&self) -> String;
}

impl VfsPath for PathBuf {
    fn join_name(&self, name: &[u8]) -> Self {
        self.join(OsStr::from_bytes(name))
    }

    fn name(&self) -> Option<&[u8]> {
        self.file_name().map(OsStr::as_bytes)
    }

    fn parent(&self) -> Option<Self> {
        self.as_path().parent().map(std::path::Path::to_path_buf)
    }

    fn display(&self) -> String {
        self.as_path().display().to_string()
    }
}

impl VfsPath for RemotePath {
    fn join_name(&self, name: &[u8]) -> Self {
        self.join(name)
    }

    fn name(&self) -> Option<&[u8]> {
        self.file_name()
    }

    fn parent(&self) -> Option<Self> {
        Self::parent(self)
    }

    fn display(&self) -> String {
        Self::display(self).into_owned()
    }
}

/// File system operations shared by the local and SFTP backends.
///
/// The futures are cancel-safe: dropping one stops waiting and leaks nothing. A change that was
/// already sent may still happen, so after a dropped change the file system is in one state or
/// the other.
pub trait Vfs: Send + Sync {
    /// Owned path type: [`PathBuf`](std::path::PathBuf) for the local file system,
    /// [`RemotePath`](crate::RemotePath) for SFTP.
    type Path: VfsPath;
    type Reader: FileReader;
    type Writer: FileWriter;

    /// Lists a directory without `.` and `..`, in no particular order.
    fn list_dir(
        &self,
        path: &Self::Path,
    ) -> impl Future<Output = Result<Vec<DirEntry>, VfsError>> + Send;

    /// Metadata of `path`, following symlinks.
    fn metadata(
        &self,
        path: &Self::Path,
    ) -> impl Future<Output = Result<Metadata, VfsError>> + Send;

    /// Metadata of `path` itself: a symlink is not followed.
    fn symlink_metadata(
        &self,
        path: &Self::Path,
    ) -> impl Future<Output = Result<Metadata, VfsError>> + Send;

    /// Absolute form of `path` with symlinks resolved.
    fn canonicalize(
        &self,
        path: &Self::Path,
    ) -> impl Future<Output = Result<Self::Path, VfsError>> + Send;

    /// Creates the directory `path` in an existing parent; [`VfsError::AlreadyExists`] if the
    /// name is taken.
    fn create_dir(&self, path: &Self::Path) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// Removes a file, a symlink (not its target), or anything else but a directory.
    fn remove_file(&self, path: &Self::Path) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// Removes an empty directory.
    fn remove_dir(&self, path: &Self::Path) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// Renames `from` to `to` within this file system. An existing file at `to` is replaced
    /// where the backend can do that in one step: always locally, and over SFTP when the
    /// server has `posix-rename@openssh.com`, as OpenSSH does. Elsewhere it is
    /// [`VfsError::AlreadyExists`].
    fn rename(
        &self,
        from: &Self::Path,
        to: &Self::Path,
    ) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// What the symlink at `path` points to, as stored: relative or absolute, never resolved.
    fn read_link(
        &self,
        path: &Self::Path,
    ) -> impl Future<Output = Result<Vec<u8>, VfsError>> + Send;

    /// Makes a symlink at `path` that points to `target`, stored as given;
    /// [`VfsError::AlreadyExists`] if the name is taken.
    fn create_symlink(
        &self,
        target: &[u8],
        path: &Self::Path,
    ) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// Sets the permission bits (the lower twelve of `mode`) of `path`, following symlinks.
    fn set_permissions(
        &self,
        path: &Self::Path,
        mode: u32,
    ) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// Opens the file at `path` for reading, following symlinks.
    fn open_file(
        &self,
        path: &Self::Path,
    ) -> impl Future<Output = Result<Self::Reader, VfsError>> + Send;

    /// Creates the file at `path` for writing: a new one, or with `replace` an existing one
    /// emptied (following a symlink there). Without `replace`, a taken name is
    /// [`VfsError::AlreadyExists`].
    fn create_file(
        &self,
        path: &Self::Path,
        replace: bool,
    ) -> impl Future<Output = Result<Self::Writer, VfsError>> + Send;

    /// Sets the modification time of `path`, following symlinks; the access time becomes the
    /// current time. SFTP keeps whole seconds from 1970 to 2106.
    fn set_modified(
        &self,
        path: &Self::Path,
        time: SystemTime,
    ) -> impl Future<Output = Result<(), VfsError>> + Send;
}
