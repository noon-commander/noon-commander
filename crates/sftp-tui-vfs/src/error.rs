use std::io;
use std::path::Path;

use openssh_sftp_client::error::SftpErrorKind;

use crate::RemotePath;

/// Errors of file system operations.
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    /// The path does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// Access to the path was denied.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// Something already has the name to create or rename to.
    #[error("already exists: {0}")]
    AlreadyExists(String),
    /// Any other error of the local file system.
    #[error("I/O error")]
    Io(#[source] io::Error),
    /// Any other error of the SFTP server or session.
    #[error("SFTP error")]
    Sftp(#[source] openssh_sftp_client::Error),
}

impl VfsError {
    pub(crate) fn local(err: io::Error, path: &Path) -> Self {
        match err.kind() {
            io::ErrorKind::NotFound => Self::NotFound(path.display().to_string()),
            io::ErrorKind::PermissionDenied => Self::PermissionDenied(path.display().to_string()),
            io::ErrorKind::AlreadyExists => Self::AlreadyExists(path.display().to_string()),
            _ => Self::Io(err),
        }
    }

    pub(crate) fn remote(err: openssh_sftp_client::Error, path: &RemotePath) -> Self {
        match err {
            openssh_sftp_client::Error::SftpError(SftpErrorKind::NoSuchFile, _) => {
                Self::NotFound(path.display().into_owned())
            }
            openssh_sftp_client::Error::SftpError(SftpErrorKind::PermDenied, _) => {
                Self::PermissionDenied(path.display().into_owned())
            }
            err => Self::Sftp(err),
        }
    }

    /// Whether a rename failed because its ends are on different file systems, locally
    /// (`EXDEV`). SFTP v3 reports that as a plain failure, so over SFTP any plain failure
    /// counts. Copying and removing moves such entries instead.
    pub fn is_cross_device(&self) -> bool {
        match self {
            Self::Io(err) => err.kind() == io::ErrorKind::CrossesDevices,
            Self::Sftp(openssh_sftp_client::Error::SftpError(SftpErrorKind::Failure, _)) => true,
            _ => false,
        }
    }

    pub(crate) fn task_failed(err: tokio::task::JoinError) -> Self {
        Self::Io(io::Error::other(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_local_errors() {
        let path = Path::new("/x");
        let err = VfsError::local(io::ErrorKind::NotFound.into(), path);
        assert!(
            matches!(&err, VfsError::NotFound(p) if p == "/x"),
            "{err:?}"
        );
        let err = VfsError::local(io::ErrorKind::PermissionDenied.into(), path);
        assert!(
            matches!(&err, VfsError::PermissionDenied(p) if p == "/x"),
            "{err:?}"
        );
        let err = VfsError::local(io::ErrorKind::AlreadyExists.into(), path);
        assert!(
            matches!(&err, VfsError::AlreadyExists(p) if p == "/x"),
            "{err:?}"
        );
        let err = VfsError::local(io::ErrorKind::InvalidData.into(), path);
        assert!(matches!(&err, VfsError::Io(e) if e.kind() == io::ErrorKind::InvalidData));
    }

    #[test]
    fn tells_moves_across_file_systems() {
        assert!(VfsError::Io(io::ErrorKind::CrossesDevices.into()).is_cross_device());
        assert!(!VfsError::Io(io::ErrorKind::Other.into()).is_cross_device());
        assert!(!VfsError::NotFound("x".into()).is_cross_device());
    }

    #[test]
    fn messages() {
        assert_eq!(VfsError::NotFound("/x".into()).to_string(), "not found: /x");
        assert_eq!(
            VfsError::PermissionDenied("a".into()).to_string(),
            "permission denied: a"
        );
        assert_eq!(
            VfsError::AlreadyExists("b".into()).to_string(),
            "already exists: b"
        );
        assert_eq!(
            VfsError::Io(io::ErrorKind::Other.into()).to_string(),
            "I/O error"
        );
    }

    #[test]
    fn errors_cross_threads() {
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<VfsError>();
    }
}
