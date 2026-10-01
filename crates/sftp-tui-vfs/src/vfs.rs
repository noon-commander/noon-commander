use crate::{DirEntry, Metadata, VfsError};

/// File system operations shared by the local and SFTP backends.
///
/// The futures are cancel-safe: dropping one abandons the operation cleanly.
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

    /// Absolute form of `path` with symlinks resolved.
    fn canonicalize(
        &self,
        path: &Self::Path,
    ) -> impl Future<Output = Result<Self::Path, VfsError>> + Send;
}
