//! Errors from running ssh.

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::args::ArgsError;
use crate::resolve::ParseError;
use crate::version::OpenSshVersion;

/// Why an ssh operation failed.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    /// The ssh program could not be started.
    #[error("cannot run {}", program.display())]
    Spawn {
        program: PathBuf,
        #[source]
        source: io::Error,
    },
    /// `ssh -V` did not report an OpenSSH version.
    #[error("{} is not OpenSSH: {banner}", program.display())]
    NotOpenSsh { program: PathBuf, banner: String },
    /// The OpenSSH client is older than sftp-tui supports.
    #[error("OpenSSH {found} is too old; sftp-tui needs {required} or newer")]
    TooOld {
        found: OpenSshVersion,
        required: OpenSshVersion,
    },
    /// User-supplied ssh arguments were rejected.
    #[error(transparent)]
    Args(#[from] ArgsError),
    /// The destination is empty or starts with `-`.
    #[error("invalid ssh destination `{0}`")]
    InvalidDestination(String),
    /// The control socket path does not fit into a Unix socket address.
    #[error("control socket path {} is too long ({len} bytes, at most {max})", path.display())]
    ControlPathTooLong {
        path: PathBuf,
        len: usize,
        max: usize,
    },
    /// ssh exited unexpectedly; `stderr` holds its last lines.
    #[error("ssh exited with {status}{}", stderr_suffix(stderr))]
    Exited { status: ExitStatus, stderr: String },
    /// The master connection to the host is gone.
    #[error("the connection is closed{}", stderr_suffix(stderr))]
    Disconnected { stderr: String },
    /// `ssh -G` printed something sftp-tui cannot read.
    #[error("cannot read the output of `ssh -G`")]
    Resolve(#[from] ParseError),
    /// ssh did not finish in time.
    #[error("ssh did not finish in time")]
    Timeout,
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// Any other I/O error.
    #[error("I/O error")]
    Io(#[from] io::Error),
}

fn stderr_suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}
