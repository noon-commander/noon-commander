//! The editor of F4: `$VISUAL`, else `$EDITOR`, else `vi`.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use tokio::process::Command;

use crate::ToolError;

/// An editor command, such as `code -w`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    program: PathBuf,
    args: Vec<String>,
}

impl Editor {
    /// The editor this process's environment names.
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// The editor that `var` names: `VISUAL`, else `EDITOR`, else `vi`. The value is split at
    /// whitespace, so `code -w` works; quotes are not read.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        let mut words = ["VISUAL", "EDITOR"]
            .iter()
            .filter_map(|name| var(name))
            .map(|value| {
                value
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .find(|words| !words.is_empty())
            .unwrap_or_else(|| vec!["vi".to_owned()]);
        let program = PathBuf::from(words.remove(0));
        Self {
            program,
            args: words,
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The words after the program, such as the `-w` of `code -w`.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Runs the editor on `file` with this process's terminal, which the caller has handed
    /// over, and waits for it. `file` is absolute, so no editor takes it for an option.
    pub async fn edit(&self, file: &Path) -> Result<ExitStatus, ToolError> {
        self.edit_at(file, None).await
    }

    /// The same, with the cursor on `line` (1-based) if there is one.
    pub async fn edit_at(&self, file: &Path, line: Option<u64>) -> Result<ExitStatus, ToolError> {
        debug_assert!(file.is_absolute());
        Command::new(&self.program)
            .args(&self.args)
            .args(self.file_args(file, line))
            .status()
            .await
            .map_err(|source| ToolError::Spawn {
                program: self.program.clone(),
                source,
            })
    }
}

impl Editor {
    /// How the editor takes `file` and the `line` it starts at: `+N file`, as vi, vim, Neovim,
    /// nano, Emacs, micro, and kak do; `-g file:N` for VS Code and its kin; `file:N` for Helix,
    /// Sublime Text, and Zed.
    fn file_args(&self, file: &Path, line: Option<u64>) -> Vec<OsString> {
        let Some(line) = line else {
            return vec![file.into()];
        };
        let name = self
            .program
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default();
        let at = || {
            let mut at = file.as_os_str().to_owned();
            at.push(format!(":{line}"));
            at
        };
        match name {
            "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => {
                vec!["-g".into(), at()]
            }
            "hx" | "helix" | "subl" | "zed" => vec![at()],
            _ => vec![format!("+{line}").into(), file.into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(vars: &[(&str, &str)]) -> Editor {
        Editor::from_vars(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn visual_comes_before_editor_and_vi_comes_last() {
        let both = editor(&[("VISUAL", "code -w"), ("EDITOR", "nano")]);
        assert_eq!(both.program(), Path::new("code"));
        assert_eq!(both.args, ["-w"]);
        let blank = editor(&[("VISUAL", "  "), ("EDITOR", "nano")]);
        assert_eq!(blank.program(), Path::new("nano"));
        assert_eq!(blank.args, [] as [String; 0]);
        assert_eq!(editor(&[]).program(), Path::new("vi"));
    }

    #[test]
    fn a_line_goes_as_each_editor_takes_it() {
        let file = Path::new("/tmp/a b.rs");
        let args = |program: &str, line| -> Vec<String> {
            editor(&[("EDITOR", program)])
                .file_args(file, line)
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(args("nvim", None), ["/tmp/a b.rs"]);
        assert_eq!(args("/usr/bin/vim", Some(7)), ["+7", "/tmp/a b.rs"]);
        assert_eq!(args("code -w", Some(7)), ["-g", "/tmp/a b.rs:7"]);
        assert_eq!(args("hx", Some(7)), ["/tmp/a b.rs:7"]);
    }

    #[tokio::test]
    async fn runs_with_the_file_last() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        let script = r#"printf '%s|' "$@" > "$0.args""#;
        let editor = Editor {
            program: PathBuf::from("/bin/sh"),
            args: vec![
                "-c".to_owned(),
                script.to_owned(),
                file.to_string_lossy().into_owned(),
                "-w".to_owned(),
            ],
        };
        let status = editor.edit(&file).await.unwrap();
        assert!(status.success());
        let args = std::fs::read_to_string(dir.path().join("notes.txt.args")).unwrap();
        assert_eq!(args, format!("-w|{}|", file.display()));
        let missing = Editor {
            program: PathBuf::from("/nonexistent/editor"),
            args: Vec::new(),
        };
        assert!(missing.edit(&file).await.unwrap_err().is_not_found());
    }
}
