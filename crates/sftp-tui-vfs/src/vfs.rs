use std::time::SystemTime;

use crate::{DirEntry, Metadata, VfsError};

/// File system operations shared by the local and SFTP backends.
///
/// The futures are cancel-safe: dropping one stops waiting and leaks nothing. A change that was
/// already sent may still happen, so after a dropped change the file system is in one state or
/// the other.
pub trait Vfs: Send + Sync {
    /// Owned path type: [`PathBuf`](std::path::PathBuf) for the local file system,
    /// [`RemotePath`](crate::RemotePath) for SFTP.
    type Path: Clone + Send + Sync + 'static;

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

    /// Sets the permission bits (the lower twelve of `mode`) of `path`, following symlinks.
    fn set_permissions(
        &self,
        path: &Self::Path,
        mode: u32,
    ) -> impl Future<Output = Result<(), VfsError>> + Send;

    /// Sets the modification time of `path`, following symlinks; the access time becomes the
    /// current time. SFTP keeps whole seconds from 1970 to 2106.
    fn set_modified(
        &self,
        path: &Self::Path,
        time: SystemTime,
    ) -> impl Future<Output = Result<(), VfsError>> + Send;
}
