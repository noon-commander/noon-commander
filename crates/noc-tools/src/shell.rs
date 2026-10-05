//! The shell of the command line: `$SHELL`, else `/bin/sh` (ADR 0019).

use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use tokio::process::Command;

use crate::ToolError;

/// The program that runs commands typed on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    program: PathBuf,
}

impl Shell {
    /// The shell this process's environment names.
    pub fn from_env() -> Self {
        Self::from_var(std::env::var_os("SHELL").map(PathBuf::from))
    }

    /// `shell`, unless it is missing or empty: then `/bin/sh`.
    pub fn from_var(shell: Option<PathBuf>) -> Self {
        let program = shell
            .filter(|program| !program.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("/bin/sh"));
        Self { program }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Runs `command` as the shell's `-c` argument in `dir`, with this process's terminal,
    /// which the caller has handed over, and waits for it. The command goes to the shell as
    /// it was typed, in one argument; it may span lines.
    pub async fn run(&self, dir: &Path, command: &str) -> Result<ExitStatus, ToolError> {
        Command::new(&self.program)
            .arg("-c")
            .arg(command)
            .current_dir(dir)
            .kill_on_drop(true)
            .status()
            .await
            .map_err(|source| ToolError::Spawn {
                program: self.program.clone(),
                source,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_comes_first_and_sh_last() {
        let zsh = Shell::from_var(Some(PathBuf::from("/bin/zsh")));
        assert_eq!(zsh.program(), Path::new("/bin/zsh"));
        assert_eq!(
            Shell::from_var(Some(PathBuf::new())).program(),
            Path::new("/bin/sh")
        );
        assert_eq!(Shell::from_var(None).program(), Path::new("/bin/sh"));
    }

    #[tokio::test]
    async fn runs_the_command_in_the_directory_as_typed() {
        let dir = tempfile::tempdir().unwrap();
        let shell = Shell::from_var(None);
        let command = "pwd -P > out \\\n  && echo 'two  words' >> out\nexit 3";
        let status = shell.run(dir.path(), command).await.unwrap();
        assert_eq!(status.code(), Some(3));
        let out = std::fs::read_to_string(dir.path().join("out")).unwrap();
        let pwd = dir.path().canonicalize().unwrap();
        assert_eq!(out, format!("{}\ntwo  words\n", pwd.display()));
        let missing = Shell::from_var(Some(PathBuf::from("/nonexistent/shell")));
        assert!(
            missing
                .run(dir.path(), "true")
                .await
                .unwrap_err()
                .is_not_found()
        );
    }
}
