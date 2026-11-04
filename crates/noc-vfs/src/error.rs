use std::io;
use std::path::Path;

use openssh_sftp_client::error::SftpErrorKind;

use crate::RemotePath;

/// The number the system or the SFTP protocol gave a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
    /// An `errno` of the local system.
    Os(i32),
    /// A status of SFTP v3 (`SSH_FX_*`), which tells nothing of the `errno` on the host.
    Sftp(u32),
}

impl Code {
    /// The code of `err`, an error of the local system, if it has one.
    pub fn of_io(err: &io::Error) -> Option<Self> {
        err.raw_os_error().map(Self::Os)
    }

    /// The code of `err`, an error of SFTP, if it has one: the status the server sent, or the
    /// `errno` of the pipes to it.
    pub fn of_sftp(err: &openssh_sftp_client::Error) -> Option<Self> {
        match err {
            openssh_sftp_client::Error::SftpError(kind, _) => {
                let status = match kind {
                    SftpErrorKind::NoSuchFile => 2,
                    SftpErrorKind::PermDenied => 3,
                    SftpErrorKind::Failure => 4,
                    SftpErrorKind::BadMessage => 5,
                    SftpErrorKind::OpUnsupported => 8,
                    _ => return None,
                };
                Some(Self::Sftp(status))
            }
            openssh_sftp_client::Error::IOError(err) => Self::of_io(err),
            _ => None,
        }
    }
}

/// Errors of file system operations. Each that the system or the server reported keeps its
/// [`Code`].
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    /// The path does not exist.
    #[error("not found: {0}")]
    NotFound(String, Option<Code>),
    /// Access to the path was denied.
    #[error("permission denied: {0}")]
    PermissionDenied(String, Option<Code>),
    /// Something already has the name to create or rename to.
    #[error("already exists: {0}")]
    AlreadyExists(String, Option<Code>),
    /// A copy or a move would go over its own source: the path leads to the source, under
    /// another name or the same.
    #[error("same file: {0}")]
    SameFile(String),
    /// Any other error of the local file system.
    #[error("I/O error")]
    Io(#[source] io::Error),
    /// Any other error of the SFTP server or session.
    #[error("SFTP error")]
    Sftp(#[source] openssh_sftp_client::Error),
}

impl VfsError {
    pub(crate) fn local(err: io::Error, path: &Path) -> Self {
        let (path, code) = (path.display().to_string(), Code::of_io(&err));
        match err.kind() {
            io::ErrorKind::NotFound => Self::NotFound(path, code),
            io::ErrorKind::PermissionDenied => Self::PermissionDenied(path, code),
            io::ErrorKind::AlreadyExists => Self::AlreadyExists(path, code),
            _ => Self::Io(err),
        }
    }

    pub(crate) fn remote(err: openssh_sftp_client::Error, path: &RemotePath) -> Self {
        let code = Code::of_sftp(&err);
        match err {
            openssh_sftp_client::Error::SftpError(SftpErrorKind::NoSuchFile, _) => {
                Self::NotFound(path.display().into_owned(), code)
            }
            openssh_sftp_client::Error::SftpError(SftpErrorKind::PermDenied, _) => {
                Self::PermissionDenied(path.display().into_owned(), code)
            }
            err => Self::Sftp(err),
        }
    }

    /// The number the system or the server gave the failure, if it gave one: none for one that
    /// Noon Commander found itself, such as a copy onto its source.
    pub fn code(&self) -> Option<Code> {
        match self {
            Self::NotFound(_, code)
            | Self::PermissionDenied(_, code)
            | Self::AlreadyExists(_, code) => *code,
            Self::SameFile(_) => None,
            Self::Io(err) => Code::of_io(err),
            Self::Sftp(err) => Code::of_sftp(err),
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
            matches!(&err, VfsError::NotFound(p, _) if p == "/x"),
            "{err:?}"
        );
        let err = VfsError::local(io::ErrorKind::PermissionDenied.into(), path);
        assert!(
            matches!(&err, VfsError::PermissionDenied(p, _) if p == "/x"),
            "{err:?}"
        );
        let err = VfsError::local(io::ErrorKind::AlreadyExists.into(), path);
        assert!(
            matches!(&err, VfsError::AlreadyExists(p, _) if p == "/x"),
            "{err:?}"
        );
        let err = VfsError::local(io::ErrorKind::InvalidData.into(), path);
        assert!(matches!(&err, VfsError::Io(e) if e.kind() == io::ErrorKind::InvalidData));
    }

    #[test]
    fn keeps_the_code_of_the_system() {
        let path = Path::new("/x");
        let denied = VfsError::local(io::Error::from_raw_os_error(13), path);
        assert!(
            matches!(denied, VfsError::PermissionDenied(..)),
            "{denied:?}"
        );
        assert_eq!(denied.code(), Some(Code::Os(13)));
        let full = VfsError::local(io::Error::from_raw_os_error(28), path);
        assert_eq!(full.code(), Some(Code::Os(28)));
        assert_eq!(VfsError::SameFile("x".into()).code(), None);
        let unsupported = VfsError::Io(io::ErrorKind::Unsupported.into());
        assert_eq!(unsupported.code(), None, "no number from the system");
    }

    #[test]
    fn tells_moves_across_file_systems() {
        assert!(VfsError::Io(io::ErrorKind::CrossesDevices.into()).is_cross_device());
        assert!(!VfsError::Io(io::ErrorKind::Other.into()).is_cross_device());
        assert!(!VfsError::NotFound("x".into(), None).is_cross_device());
    }

    #[test]
    fn messages() {
        assert_eq!(
            VfsError::NotFound("/x".into(), None).to_string(),
            "not found: /x"
        );
        assert_eq!(
            VfsError::PermissionDenied("a".into(), None).to_string(),
            "permission denied: a"
        );
        assert_eq!(
            VfsError::AlreadyExists("b".into(), None).to_string(),
            "already exists: b"
        );
        assert_eq!(VfsError::SameFile("c".into()).to_string(), "same file: c");
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
