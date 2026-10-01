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
        let err = VfsError::local(io::ErrorKind::InvalidData.into(), path);
        assert!(matches!(&err, VfsError::Io(e) if e.kind() == io::ErrorKind::InvalidData));
    }

    #[test]
    fn messages() {
        assert_eq!(VfsError::NotFound("/x".into()).to_string(), "not found: /x");
        assert_eq!(
            VfsError::PermissionDenied("a".into()).to_string(),
            "permission denied: a"
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
