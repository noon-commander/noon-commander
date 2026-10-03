//! [zoxide](https://github.com/ajeetdsouza/zoxide), which ranks directories by how often and
//! how recently they were used. Noon Commander adds the local directories the user works in
//! and asks it for the best matches of a few keywords, as `z foo bar` does in a shell. The
//! database is zoxide's own: `_ZO_DATA_DIR`, `_ZO_EXCLUDE_DIRS`, and the other variables of
//! the environment apply as they do in the shell.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{ToolError, process};

/// How long zoxide may take; it reads and writes one small file.
const TIMEOUT: Duration = Duration::from_secs(5);

/// A directory and its rank, as `zoxide query --score` prints them.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    pub score: f64,
    pub path: PathBuf,
}

/// How to run zoxide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zoxide {
    /// A name looked up in `PATH`, or a path.
    program: PathBuf,
}

impl Default for Zoxide {
    fn default() -> Self {
        Self::new("zoxide")
    }
}

impl Zoxide {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Adds the directory `dir`, an absolute path, or raises its rank: `zoxide add -- <dir>`.
    /// zoxide leaves out what `_ZO_EXCLUDE_DIRS` excludes, the home directory by default.
    pub async fn add(&self, dir: &Path) -> Result<(), ToolError> {
        let args = [OsStr::new("add"), OsStr::new("--"), dir.as_os_str()];
        process::output(&self.program, args, TIMEOUT).await?;
        Ok(())
    }

    /// The directories that match all `keywords`, best first, without `exclude`, the
    /// directory the panel shows: `zoxide query --list --score --exclude <dir> -- <keywords>`.
    /// No keywords list every directory. zoxide leaves out directories that are gone.
    pub async fn query(
        &self,
        keywords: &[String],
        exclude: Option<&Path>,
    ) -> Result<Vec<Scored>, ToolError> {
        let mut args: Vec<&OsStr> = vec!["query", "--list", "--score"]
            .into_iter()
            .map(OsStr::new)
            .collect();
        if let Some(dir) = exclude {
            args.push(OsStr::new("--exclude"));
            args.push(dir.as_os_str());
        }
        args.push(OsStr::new("--"));
        args.extend(keywords.iter().map(OsStr::new));
        let output = process::output(&self.program, args, TIMEOUT).await?;
        Ok(parse_scored(&output))
    }
}

/// Lines such as ` 56.0 /home/me/src`: a score, a space, and a path to the end of the line.
/// Lines that do not look like that are left out.
fn parse_scored(output: &[u8]) -> Vec<Scored> {
    output
        .split(|&byte| byte == b'\n')
        .filter_map(|line| {
            let line = line.trim_ascii_start();
            let space = line.iter().position(|&byte| byte == b' ')?;
            let score = std::str::from_utf8(&line[..space]).ok()?.parse().ok()?;
            let path = &line[space + 1..];
            path.starts_with(b"/").then(|| Scored {
                score,
                path: PathBuf::from(OsStr::from_bytes(path)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scored(score: f64, path: &[u8]) -> Scored {
        Scored {
            score,
            path: PathBuf::from(OsStr::from_bytes(path)),
        }
    }

    #[test]
    fn parses_scores_and_paths_with_spaces_and_any_bytes() {
        let output = b"  360.0 /Users/me/.ssh\n   4.5 /tmp/a b  \n  0.2 /srv/\xff\xfe\n";
        assert_eq!(
            parse_scored(output),
            [
                scored(360.0, b"/Users/me/.ssh"),
                scored(4.5, b"/tmp/a b  "),
                scored(0.2, b"/srv/\xff\xfe"),
            ]
        );
    }

    #[test]
    fn leaves_out_lines_it_cannot_read() {
        let output = b"\nzoxide: warning\n12 relative/path\nnan\n 1.0 /ok\n";
        assert_eq!(parse_scored(output), [scored(1.0, b"/ok")]);
        assert_eq!(parse_scored(b""), []);
    }
}
