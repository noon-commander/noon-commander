//! The shell of the command line: `shell.run`, by default `$SHELL -i -c`, with `/bin/sh` for a
//! program that comes out empty (ADR 0019, ADR 0054).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use noc_vars::Template;
use tokio::process::Command;

use crate::ToolError;
use crate::job::{self, Running};

/// The program that runs commands typed on the command line, and the arguments before them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    program: PathBuf,
    args: Vec<OsString>,
}

impl Shell {
    /// `words` with the variables of this process's environment in them.
    pub fn from_env(words: &[Template]) -> Self {
        Self::expand(words, |name| std::env::var_os(name))
    }

    /// `words` with what `lookup` gives for each variable in them; the program, the first
    /// word, is `/bin/sh` if there is none or it comes out empty.
    pub fn expand(words: &[Template], mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut words = words.iter().map(|word| word.expand(&mut lookup));
        let program = words
            .next()
            .filter(|program| !program.is_empty())
            .map_or_else(|| PathBuf::from("/bin/sh"), PathBuf::from);
        Self {
            program,
            args: words.collect(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn args(&self) -> impl Iterator<Item = &OsStr> {
        self.args.iter().map(OsString::as_os_str)
    }

    /// Starts `command` as the last argument of the shell in `dir`, with this process's
    /// terminal, which the caller has handed over, as a job where this process's group has the
    /// terminal (ADR 0054). The command goes to the shell as it was typed, in one argument; it
    /// may span lines.
    pub fn start(&self, dir: &Path, command: &str) -> Result<Running, ToolError> {
        let mut command_line = Command::new(&self.program);
        command_line.args(&self.args).arg(command).current_dir(dir);
        job::start(command_line, &self.program)
    }
}

#[cfg(test)]
mod tests {
    use std::process::ExitStatus;

    use super::*;
    use crate::job::Waited;

    fn words(texts: &[&str]) -> Vec<Template> {
        texts
            .iter()
            .map(|text| Template::parse(text).unwrap())
            .collect()
    }

    fn shell(texts: &[&str], vars: &[(&str, &str)]) -> Shell {
        Shell::expand(&words(texts), |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        })
    }

    #[test]
    fn expands_the_words_and_falls_back_to_sh() {
        let zsh = shell(&["$SHELL", "-i", "-c"], &[("SHELL", "/bin/zsh")]);
        assert_eq!(zsh.program(), Path::new("/bin/zsh"));
        assert_eq!(zsh.args().collect::<Vec<_>>(), ["-i", "-c"]);
        let spaced = shell(
            &["$SHELL", "--rcfile", "${HOME}/a b", "-c"],
            &[("SHELL", "/bin/bash"), ("HOME", "/home/u")],
        );
        assert_eq!(
            spaced.args().collect::<Vec<_>>(),
            ["--rcfile", "/home/u/a b", "-c"]
        );
        for empty in [("SHELL", ""), ("OTHER", "/bin/zsh")] {
            let sh = shell(&["$SHELL", "-i", "-c"], &[empty]);
            assert_eq!(sh.program(), Path::new("/bin/sh"));
            assert_eq!(sh.args().collect::<Vec<_>>(), ["-i", "-c"]);
        }
        assert_eq!(shell(&[], &[]).program(), Path::new("/bin/sh"));
    }

    /// Starts `command` with `shell` in `dir` and waits for it to end.
    async fn run(shell: &Shell, dir: &Path, command: &str) -> Result<ExitStatus, ToolError> {
        match shell.start(dir, command)?.wait().await? {
            Waited::Ended(status) => Ok(status),
            Waited::Stopped => panic!("{command} was stopped"),
        }
    }

    #[tokio::test]
    async fn runs_the_command_in_the_directory_as_typed() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell(&["/bin/sh", "-c"], &[]);
        let command = "pwd -P > out \\\n  && echo 'two  words' >> out\nexit 3";
        let status = run(&shell, dir.path(), command).await.unwrap();
        assert_eq!(status.code(), Some(3));
        let out = std::fs::read_to_string(dir.path().join("out")).unwrap();
        let pwd = dir.path().canonicalize().unwrap();
        assert_eq!(out, format!("{}\ntwo  words\n", pwd.display()));
        let missing = Shell::expand(&words(&["/nonexistent/shell", "-c"]), |_| None);
        assert!(
            run(&missing, dir.path(), "true")
                .await
                .unwrap_err()
                .is_not_found()
        );
    }

    #[tokio::test]
    async fn the_command_comes_after_the_arguments() {
        let dir = tempfile::tempdir().unwrap();
        // `-c` takes the script, and `sh -c script name args` gives `$0` and `$1`.
        let shell = shell(&["/bin/sh", "-c", "echo \"$0 $1\" > out", "first"], &[]);
        let status = run(&shell, dir.path(), "second").await.unwrap();
        assert!(status.success());
        let out = std::fs::read_to_string(dir.path().join("out")).unwrap();
        assert_eq!(out, "first second\n");
    }
}
