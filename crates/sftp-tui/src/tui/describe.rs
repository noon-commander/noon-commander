//! Errors in words, for the status line.

use std::error::Error;

use sftp_tui_ssh::SshError;
use sftp_tui_vfs::VfsError;

use crate::i18n::fl;

/// Why a file system operation failed.
pub(crate) fn vfs_error(error: &VfsError) -> String {
    match error {
        VfsError::NotFound(_) => fl!("error-not-found"),
        VfsError::PermissionDenied(_) => fl!("error-permission-denied"),
        VfsError::AlreadyExists(_) => fl!("error-already-exists"),
        VfsError::Io(error) => error.to_string(),
        VfsError::Sftp(error) => chain(error),
    }
}

/// Why a connection failed or ended; `None` if it was cancelled. ssh's own last words on
/// stderr, such as `Permission denied (publickey).`, say it best.
pub(crate) fn ssh_error(error: &SshError) -> Option<String> {
    let text = match error {
        SshError::Cancelled => return None,
        SshError::Exited { status, stderr } => last_line(stderr).map_or_else(
            || fl!("error-ssh-exited", status = status.to_string()),
            str::to_owned,
        ),
        SshError::Disconnected { stderr } => {
            last_line(stderr).map_or_else(|| fl!("error-connection-closed"), str::to_owned)
        }
        SshError::Spawn { program, source } => fl!(
            "error-ssh-spawn",
            program = program.display().to_string(),
            reason = source.to_string()
        ),
        SshError::TooOld { found, required } => fl!(
            "error-ssh-too-old",
            found = found.to_string(),
            required = required.to_string()
        ),
        SshError::NotOpenSsh { program, .. } => {
            fl!("error-not-openssh", program = program.display().to_string())
        }
        other => chain(other),
    };
    Some(text)
}

/// An error and its sources, joined by `: `.
pub(crate) fn chain(error: &(dyn Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// The last line with text, trimmed: for ssh's stderr, usually the reason it gave up.
pub(crate) fn last_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::os::unix::process::ExitStatusExt as _;
    use std::path::PathBuf;
    use std::process::ExitStatus;

    use super::*;

    #[test]
    fn describes_file_system_errors() {
        assert_eq!(
            vfs_error(&VfsError::PermissionDenied("/x".to_owned())),
            "permission denied"
        );
        assert_eq!(
            vfs_error(&VfsError::NotFound("/x".to_owned())),
            "no such file or directory"
        );
        assert_eq!(
            vfs_error(&VfsError::AlreadyExists("/x".to_owned())),
            "already exists"
        );
        let unsupported = VfsError::Io(io::ErrorKind::Unsupported.into());
        assert_eq!(vfs_error(&unsupported), "unsupported");
    }

    #[test]
    fn describes_connection_errors_with_the_words_of_ssh() {
        let exited = |stderr: &str| SshError::Exited {
            status: ExitStatus::from_raw(255 << 8),
            stderr: stderr.to_owned(),
        };
        assert_eq!(
            ssh_error(&exited(
                "Warning: something\ndeploy@web: Permission denied (publickey).\n\n"
            ))
            .as_deref(),
            Some("deploy@web: Permission denied (publickey).")
        );
        assert_eq!(
            ssh_error(&exited("")).as_deref(),
            Some("ssh ended with exit status: 255")
        );
        let closed = SshError::Disconnected {
            stderr: String::new(),
        };
        assert_eq!(
            ssh_error(&closed).as_deref(),
            Some("the connection is closed")
        );
        let missing = SshError::Spawn {
            program: PathBuf::from("/no/ssh"),
            source: io::ErrorKind::NotFound.into(),
        };
        assert_eq!(
            ssh_error(&missing).as_deref(),
            Some("cannot run /no/ssh: entity not found")
        );
        assert_eq!(ssh_error(&SshError::Cancelled), None);
    }

    #[test]
    fn chains_sources() {
        let error = io::Error::other(SshError::Timeout);
        assert_eq!(chain(&error), "ssh did not finish in time");
        let spawn = SshError::Spawn {
            program: PathBuf::from("ssh"),
            source: io::Error::other("boom"),
        };
        assert_eq!(chain(&spawn), "cannot run ssh: boom");
    }
}
