//! Errors from running external programs.

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

/// Why an external program did not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The program could not be started; see [`ToolError::is_not_found`].
    #[error("cannot run {}", program.display())]
    Spawn {
        program: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The program exited unsuccessfully; `stderr` holds its last line.
    #[error("{} exited with {status}{}", program.display(), stderr_suffix(stderr))]
    Failed {
        program: PathBuf,
        status: ExitStatus,
        stderr: String,
    },
    /// The program did not finish in time and was stopped.
    #[error("{} did not finish in time", program.display())]
    Timeout { program: PathBuf },
    /// Reading the program's output failed.
    #[error("I/O error")]
    Io(#[from] io::Error),
}

impl ToolError {
    /// Whether the program is not installed: nothing by that name in `PATH`, or no such file.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Spawn { source, .. } if source.kind() == io::ErrorKind::NotFound)
    }
}

fn stderr_suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt as _;

    use super::*;

    #[test]
    fn tells_a_missing_program_from_other_failures() {
        let missing = ToolError::Spawn {
            program: PathBuf::from("zoxide"),
            source: io::ErrorKind::NotFound.into(),
        };
        assert!(missing.is_not_found());
        let denied = ToolError::Spawn {
            program: PathBuf::from("zoxide"),
            source: io::ErrorKind::PermissionDenied.into(),
        };
        assert!(!denied.is_not_found());
        let failed = ToolError::Failed {
            program: PathBuf::from("zoxide"),
            status: ExitStatus::from_raw(1 << 8),
            stderr: "zoxide: not a directory: /x".to_owned(),
        };
        assert!(!failed.is_not_found());
        assert_eq!(
            failed.to_string(),
            "zoxide exited with exit status: 1: zoxide: not a directory: /x"
        );
    }
}
