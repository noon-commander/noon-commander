//! Running a helper program in the background and reading what it prints.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::ToolError;

/// Runs `program` with `args`, without a shell and without input, and returns what it printed
/// on stdout. A failure carries the last line of its stderr. Dropping the future, or `timeout`
/// passing, kills the program.
pub(crate) async fn output<I, S>(
    program: &Path,
    args: I,
    timeout: Duration,
) -> Result<Vec<u8>, ToolError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| ToolError::Spawn {
            program: program.to_path_buf(),
            source,
        })?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| ToolError::Timeout {
            program: program.to_path_buf(),
        })??;
    if !output.status.success() {
        return Err(ToolError::Failed {
            program: program.to_path_buf(),
            status: output.status,
            stderr: last_line(&output.stderr),
        });
    }
    Ok(output.stdout)
}

/// The last line with text, trimmed: usually why a program gave up.
fn last_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn sh(script: &str) -> [&str; 2] {
        ["-c", script]
    }

    #[tokio::test]
    async fn returns_stdout_and_passes_arguments_as_they_are() {
        let out = output(
            Path::new("/bin/sh"),
            ["-c", r#"printf '%s|' "$@""#, "sh", "a b", "-x", "$HOME"],
            SECOND,
        )
        .await
        .unwrap();
        assert_eq!(out, b"a b|-x|$HOME|");
    }

    #[tokio::test]
    async fn gives_no_input() {
        let out = output(Path::new("/bin/sh"), sh("cat; echo done"), SECOND)
            .await
            .unwrap();
        assert_eq!(out, b"done\n");
    }

    #[tokio::test]
    async fn a_failure_carries_the_last_line_of_stderr() {
        let error = output(
            Path::new("/bin/sh"),
            sh("echo first >&2; echo 'last words  ' >&2; echo >&2; exit 3"),
            SECOND,
        )
        .await
        .unwrap_err();
        match error {
            ToolError::Failed { status, stderr, .. } => {
                assert_eq!(status.code(), Some(3));
                assert_eq!(stderr, "last words");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_missing_program_is_not_found() {
        let error = output(Path::new("/nonexistent/zoxide"), [""; 0], SECOND)
            .await
            .unwrap_err();
        assert!(error.is_not_found(), "{error:?}");
        let error = output(Path::new("noc-no-such-program"), [""; 0], SECOND)
            .await
            .unwrap_err();
        assert!(error.is_not_found(), "{error:?}");
    }

    #[tokio::test]
    async fn a_program_that_hangs_times_out() {
        let start = std::time::Instant::now();
        let error = output(
            Path::new("/bin/sh"),
            sh("exec sleep 30"),
            Duration::from_millis(200),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&error, ToolError::Timeout { program } if program == Path::new("/bin/sh")),
            "{error:?}"
        );
        assert!(start.elapsed() < Duration::from_secs(10));
    }
}
