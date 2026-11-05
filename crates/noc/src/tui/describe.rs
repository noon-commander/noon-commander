//! Errors in words, for messages and dialogs.

use std::error::Error;
use std::io;
use std::os::unix::process::ExitStatusExt as _;
use std::process::ExitStatus;

use noc_ssh::SshError;
use noc_tools::ToolError;
use noc_vfs::{Code, VfsError};

use crate::i18n::fl;

/// Why something failed, in words, with the number the system or an SFTP server gave it, which
/// its words then leave out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reason {
    pub(crate) text: String,
    pub(crate) code: Option<Code>,
}

impl Reason {
    /// The same reason, its text safe for the terminal.
    pub(crate) fn sanitized(self) -> Self {
        Self {
            text: super::cells::sanitize(self.text.as_bytes()),
            ..self
        }
    }
}

impl From<String> for Reason {
    fn from(text: String) -> Self {
        Self { text, code: None }
    }
}

/// Why a file system operation failed, with its code.
pub(crate) fn vfs_reason(error: &VfsError) -> Reason {
    let text = match error {
        VfsError::Io(error) => io_text(error),
        VfsError::Sftp(error) => reason(error).text,
        other => vfs_error(other),
    };
    Reason {
        text,
        code: error.code(),
    }
}

/// An error and its sources, as [`chain`] joins them, with the code of the first that the
/// system gave one.
pub(crate) fn reason(error: &(dyn Error + 'static)) -> Reason {
    let mut parts = Vec::new();
    let mut code = None;
    let mut next = Some(error);
    while let Some(cause) = next {
        match cause.downcast_ref::<io::Error>() {
            // It says what the error it wraps says, and passes over that error for its source.
            Some(error) if error.raw_os_error().is_none() && error.get_ref().is_some() => {
                next = error.get_ref().map(|inner| inner as &(dyn Error + 'static));
                continue;
            }
            Some(error) => {
                code = code.or_else(|| Code::of_io(error));
                parts.push(io_text(error));
            }
            None => parts.push(cause.to_string()),
        }
        next = cause.source();
    }
    Reason {
        text: parts.join(": "),
        code,
    }
}

/// What the system says of `error`, without the code it puts after it, which the dialog shows
/// on a line of its own.
fn io_text(error: &io::Error) -> String {
    let text = error.to_string();
    error
        .raw_os_error()
        .and_then(|code| text.strip_suffix(&format!(" (os error {code})")))
        .map_or_else(|| text.clone(), str::to_owned)
}

/// Why a file system operation failed.
pub(crate) fn vfs_error(error: &VfsError) -> String {
    match error {
        VfsError::NotFound(..) => fl!("error-not-found"),
        VfsError::PermissionDenied(..) => fl!("error-permission-denied"),
        VfsError::AlreadyExists(..) => fl!("error-already-exists"),
        VfsError::SameFile(_) => fl!("error-same-file"),
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

/// Why a connection that was up ended: ssh's last words since it connected, else the signal
/// that ended it, as when it was killed, else that it is closed.
pub(crate) fn lost(stderr: &str, status: Option<ExitStatus>) -> String {
    if let Some(line) = last_line(stderr) {
        return line.to_owned();
    }
    match status.and_then(|status| status.signal()) {
        Some(signal) => fl!("error-ssh-signal", signal = signal),
        None => fl!("error-connection-closed"),
    }
}

/// Why zoxide gave no answer: it is not installed, or its own last words.
pub(crate) fn zoxide_error(error: &ToolError) -> String {
    match error {
        ToolError::Spawn { program, .. } if error.is_not_found() => fl!(
            "jump-not-installed",
            program = program.display().to_string()
        ),
        ToolError::Failed { stderr, .. } if !stderr.is_empty() => stderr.clone(),
        other => chain(other),
    }
}

/// Why ripgrep could not search: it is not installed, here or on the host, whose shell says
/// so with 127, or what it said last.
pub(crate) fn ripgrep_error(error: &ToolError) -> String {
    match error {
        ToolError::Spawn { program, .. } if error.is_not_found() => fl!(
            "find-not-installed",
            program = program.display().to_string()
        ),
        ToolError::Failed {
            program, status, ..
        } if status.code() == Some(127) => fl!(
            "find-not-installed",
            program = program.display().to_string()
        ),
        ToolError::Failed { stderr, .. } if !stderr.is_empty() => stderr.clone(),
        other => chain(other),
    }
}

/// Why fd could not search: it is not installed, here or on the host, whose shell says so
/// with 127, or what it said last.
pub(crate) fn fd_error(error: &ToolError) -> String {
    match error {
        ToolError::Spawn { program, .. } if error.is_not_found() => fl!(
            "find-fd-not-installed",
            program = program.display().to_string()
        ),
        ToolError::Failed {
            program, status, ..
        } if status.code() == Some(127) => fl!(
            "find-fd-not-installed",
            program = program.display().to_string()
        ),
        ToolError::Failed { stderr, .. } if !stderr.is_empty() => stderr.clone(),
        other => chain(other),
    }
}

/// Why the desktop's opener did not open a file: it could not run, or how it exited, since its
/// output goes nowhere.
pub(crate) fn open_error(error: &ToolError) -> Reason {
    match error {
        ToolError::Spawn { program, source } => Reason {
            text: fl!(
                "open-cannot-run",
                program = program.display().to_string(),
                reason = io_text(source)
            ),
            code: Code::of_io(source),
        },
        other => reason(other),
    }
}

/// Why `program` could not run, or be waited for, as `error` says: the editor of F4.
pub(crate) fn cannot_run(program: &std::path::Path, error: &ToolError) -> Reason {
    let why = match error {
        ToolError::Spawn { source, .. } => reason(source),
        other => reason(other),
    };
    Reason {
        text: fl!(
            "edit-cannot-run",
            program = program.display().to_string(),
            reason = why.text
        ),
        code: why.code,
    }
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
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn describes_file_system_errors() {
        assert_eq!(
            vfs_error(&VfsError::PermissionDenied("/x".to_owned(), None)),
            "Permission denied"
        );
        assert_eq!(
            vfs_error(&VfsError::NotFound("/x".to_owned(), None)),
            "No such file or directory"
        );
        assert_eq!(
            vfs_error(&VfsError::AlreadyExists("/x".to_owned(), None)),
            "Already exists"
        );
        assert_eq!(
            vfs_error(&VfsError::SameFile("/x".to_owned())),
            "It is the source itself"
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
            Some("The connection is closed")
        );
        let missing = SshError::Spawn {
            program: PathBuf::from("/no/ssh"),
            source: io::ErrorKind::NotFound.into(),
        };
        assert_eq!(
            ssh_error(&missing).as_deref(),
            Some("Cannot run /no/ssh: entity not found")
        );
        assert_eq!(ssh_error(&SshError::Cancelled), None);
    }

    #[test]
    fn describes_a_lost_connection() {
        let killed = Some(ExitStatus::from_raw(9));
        assert_eq!(
            lost("Connection to web closed by remote host.\n", killed),
            "Connection to web closed by remote host."
        );
        assert_eq!(lost("", killed), "ssh was ended by signal 9");
        assert_eq!(
            lost("", Some(ExitStatus::from_raw(255 << 8))),
            "The connection is closed"
        );
        assert_eq!(lost("", None), "The connection is closed");
    }

    #[test]
    fn describes_zoxide_errors() {
        let missing = ToolError::Spawn {
            program: PathBuf::from("zoxide"),
            source: io::ErrorKind::NotFound.into(),
        };
        assert!(
            zoxide_error(&missing).starts_with("zoxide is not installed: cannot run zoxide."),
            "{}",
            zoxide_error(&missing)
        );
        let failed = ToolError::Failed {
            program: PathBuf::from("zoxide"),
            status: ExitStatus::from_raw(1 << 8),
            stderr: "zoxide: unable to create data directory".to_owned(),
        };
        assert_eq!(
            zoxide_error(&failed),
            "zoxide: unable to create data directory"
        );
        let slow = ToolError::Timeout {
            program: PathBuf::from("zoxide"),
        };
        assert_eq!(zoxide_error(&slow), "zoxide did not finish in time");
    }

    #[test]
    fn describes_opener_errors() {
        let missing = ToolError::Spawn {
            program: PathBuf::from("xdg-open"),
            source: io::ErrorKind::NotFound.into(),
        };
        assert_eq!(
            open_error(&missing).text,
            "Cannot run xdg-open: entity not found"
        );
        let gone = ToolError::Spawn {
            program: PathBuf::from("xdg-open"),
            source: io::Error::from_raw_os_error(2),
        };
        let reason = open_error(&gone);
        assert!(!reason.text.contains("os error"), "{}", reason.text);
        assert_eq!(reason.code, Some(Code::Os(2)));
        let failed = ToolError::Failed {
            program: PathBuf::from("xdg-open"),
            status: ExitStatus::from_raw(3 << 8),
            stderr: String::new(),
        };
        assert_eq!(
            open_error(&failed),
            Reason::from("xdg-open exited with exit status: 3".to_owned())
        );
    }

    #[test]
    fn reasons_keep_the_code_of_the_system_apart_from_its_words() {
        let full = VfsError::Io(io::Error::from_raw_os_error(28));
        let reason = vfs_reason(&full);
        assert!(!reason.text.contains("os error"), "{}", reason.text);
        assert_eq!(reason.code, Some(Code::Os(28)));
        let denied = VfsError::PermissionDenied("/x".to_owned(), Some(Code::Sftp(3)));
        assert_eq!(
            vfs_reason(&denied),
            Reason {
                text: "Permission denied".to_owned(),
                code: Some(Code::Sftp(3)),
            }
        );
        let same = vfs_reason(&VfsError::SameFile("/x".to_owned()));
        assert_eq!(same.code, None);
        // Through the sources of an error, as of the configuration.
        let wrapped = io::Error::other(io::Error::from_raw_os_error(13));
        let reason = super::reason(&wrapped);
        assert_eq!(reason.code, Some(Code::Os(13)));
        assert!(!reason.text.contains("os error"), "{}", reason.text);
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
