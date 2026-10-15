//! [fd](https://github.com/sharkdp/fd), which lists the files and directories under a directory
//! whose names match a pattern. The search window builds its arguments here and reads what it
//! prints with [`Entries`], whether `fd` runs here ([`Fd::spawn`]) or on a remote host through
//! ssh. Ignore files (`.gitignore` in a git repository, `.ignore`, `.fdignore`) apply as they do
//! in a shell.
//!
//! fd matches its pattern against the name of each entry, and leaves out the globs of the Files
//! field that start with `!` (`--exclude`); it cannot take the others with a pattern, so
//! [`Names`] keeps what they let through. [`Names`] also shows where a name matches, with the
//! regex engine fd uses.

use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use regex::bytes::{Regex, RegexBuilder};
use tokio::io::AsyncRead;
use tokio::process::{Child, Command};

use crate::ripgrep::{Ended, Hit, Hits};
use crate::{ToolError, process};

/// How long `fd --version` may take.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// What to look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// What a name must match; empty matches every name.
    pub pattern: String,
    /// The globs of the Files field, split at whitespace by the caller: `!` before one leaves
    /// out what it matches (`--exclude`); a name must match one of the others.
    pub globs: Vec<String>,
    /// A regular expression; else `--fixed-strings`.
    pub regex: bool,
    /// `--case-sensitive`; else fd's smart case, its default: case counts only once the
    /// pattern has a capital letter.
    pub match_case: bool,
    /// `--hidden`.
    pub hidden: bool,
}

impl Query {
    /// fd's arguments, program excluded, searching `.` (the working directory): every kind of
    /// entry, each ended by NUL, unreadable directories reported on stderr.
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = ["--print0", "--color", "never", "--show-errors"]
            .map(OsString::from)
            .into();
        if self.hidden {
            args.push("--hidden".into());
        }
        if !self.regex {
            args.push("--fixed-strings".into());
        }
        if self.match_case {
            args.push("--case-sensitive".into());
        }
        for glob in self.globs.iter().filter_map(|glob| glob.strip_prefix('!')) {
            args.push("--exclude".into());
            args.push(glob.into());
        }
        args.push("--".into());
        args.push((&self.pattern).into());
        args.push(".".into());
        args
    }

    /// What decides which names the globs that are not exclusions let through, and where a
    /// name matches the pattern; or why a glob, or the pattern, is not one.
    pub fn names(&self) -> Result<Names, PatternError> {
        let mut names = GlobSetBuilder::new();
        let mut paths = GlobSetBuilder::new();
        let (mut any_name, mut any_path) = (false, false);
        for glob in self.globs.iter().filter(|glob| !glob.starts_with('!')) {
            let invalid = |error: globset::Error| PatternError::Glob(error.to_string());
            // As in a `.gitignore`: a glob with a `/` matches the path, others the name.
            if glob.contains('/') {
                let glob = glob.trim_start_matches('/');
                let built = GlobBuilder::new(glob).literal_separator(true).build();
                paths.add(built.map_err(invalid)?);
                any_path = true;
            } else {
                names.add(Glob::new(glob).map_err(invalid)?);
                any_name = true;
            }
        }
        let set = |builder: GlobSetBuilder, any: bool| {
            any.then(|| builder.build())
                .transpose()
                .map_err(|error| PatternError::Glob(error.to_string()))
        };
        let pattern = if self.pattern.is_empty() {
            None
        } else {
            let source = if self.regex {
                self.pattern.clone()
            } else {
                regex::escape(&self.pattern)
            };
            let caseless = !self.match_case && !has_capital(&self.pattern, self.regex);
            let built = RegexBuilder::new(&source)
                .case_insensitive(caseless)
                .build()
                .map_err(|error| PatternError::Regex(last_line(&error.to_string())))?;
            Some(built)
        };
        Ok(Names {
            by_name: set(names, any_name)?,
            by_path: set(paths, any_path)?,
            pattern,
        })
    }
}

/// Whether `pattern` has a capital letter, as fd's smart case asks; in a regular expression,
/// one not escaped, as `\S` is a class rather than a letter.
fn has_capital(pattern: &str, regex: bool) -> bool {
    let mut escaped = false;
    pattern.chars().any(|c| {
        let literal = !escaped;
        escaped = regex && !escaped && c == '\\';
        literal && c.is_uppercase()
    })
}

/// The last line of a message that has text, trimmed: what is wrong with a regular
/// expression, after the lines that show where.
fn last_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// Why the search cannot start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// A glob of the Files field is not one.
    #[error("{0}")]
    Glob(String),
    /// The text is not a regular expression.
    #[error("{0}")]
    Regex(String),
}

/// Which names a query keeps, and where they match: [`Query::names`].
#[derive(Debug, Clone)]
pub struct Names {
    /// The globs without a `/`, that the name of an entry must match one of.
    by_name: Option<GlobSet>,
    /// The globs with a `/`, that its path must match one of instead.
    by_path: Option<GlobSet>,
    pattern: Option<Regex>,
}

impl Names {
    /// Whether the entry at `path`, relative to where fd searched, is one that the globs let
    /// through: every entry without globs that are not exclusions.
    pub fn keeps(&self, path: &[u8]) -> bool {
        if self.by_name.is_none() && self.by_path.is_none() {
            return true;
        }
        let name = path.rsplit(|&byte| byte == b'/').next().unwrap_or(path);
        let matches = |set: &Option<GlobSet>, text: &[u8]| {
            set.as_ref()
                .is_some_and(|set| set.is_match(Path::new(OsStr::from_bytes(text))))
        };
        matches(&self.by_name, name) || matches(&self.by_path, path)
    }

    /// Where `name` matches the pattern, as byte ranges, each place it does; none without a
    /// pattern.
    pub fn found(&self, name: &[u8]) -> Vec<Range<usize>> {
        self.pattern.as_ref().map_or_else(Vec::new, |pattern| {
            pattern
                .find_iter(name)
                .map(|found| found.range())
                .filter(|range| !range.is_empty())
                .collect()
        })
    }
}

/// An entry that fd listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Relative to the directory searched, without a leading `./` or a trailing `/`, raw
    /// bytes.
    pub path: Vec<u8>,
    /// A directory, which fd 8.3 and later print with a `/` at the end; a link to one is not.
    pub dir: bool,
}

/// Reads what fd prints, entry by entry. Dropping the future of [`Entries::next`] loses
/// nothing: the next call goes on where it stopped.
pub struct Entries<R> {
    paths: Hits<R>,
}

impl<R: AsyncRead + Unpin> Entries<R> {
    /// Reads fd's stdout.
    pub fn new(stdout: R) -> Self {
        Self {
            paths: Hits::new(stdout, true),
        }
    }

    /// The next entry, or `None` at the end.
    pub async fn next(&mut self) -> Result<Option<Entry>, ToolError> {
        while let Some(hit) = self.paths.next().await? {
            let Hit::File(mut path) = hit else {
                continue;
            };
            let dir = path.ends_with(b"/");
            while path.ends_with(b"/") {
                path.pop();
            }
            if !path.is_empty() {
                return Ok(Some(Entry { path, dir }));
            }
        }
        Ok(None)
    }
}

/// The fd program of `[search] fd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fd {
    /// A name looked up in `PATH`, or a path.
    program: PathBuf,
}

impl Fd {
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// What fd says it is, such as `fd 10.5.0`: `fd --version`, its first line. A program
    /// that is not there is [`ToolError::is_not_found`].
    pub async fn version(&self) -> Result<String, ToolError> {
        let output = process::output(&self.program, ["--version"], VERSION_TIMEOUT).await?;
        let text = String::from_utf8_lossy(&output);
        Ok(text.lines().next().unwrap_or_default().trim().to_owned())
    }

    /// Starts fd for `query` in `dir`, which is absolute, without a shell, stdin null, stdout
    /// and stderr piped, killed when the child is dropped. Read stderr while reading stdout:
    /// fd stops when either pipe is full. A `dir` that is gone also reads as
    /// [`ToolError::is_not_found`].
    pub fn spawn(&self, dir: &Path, query: &Query) -> Result<Child, ToolError> {
        Command::new(&self.program)
            .args(query.args())
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|source| ToolError::Spawn {
                program: self.program.clone(),
                source,
            })
    }
}

/// How fd ended, from its exit `status` and its whole `stderr`: 0 is Ok, and so is 1, an
/// error such as a pattern it could not parse; `errors` counts its errors, the directories
/// it could not read among them, and `last_error` is the last, without fd's prefix and the
/// note after it. Any other status (127 when a remote shell has no fd, a signal) is
/// [`ToolError::Failed`] with `program` and the last line of stderr.
pub fn ended(program: &Path, status: ExitStatus, stderr: &[u8]) -> Result<Ended, ToolError> {
    if !matches!(status.code(), Some(0 | 1)) {
        return Err(ToolError::Failed {
            program: program.to_path_buf(),
            status,
            stderr: process::last_line(stderr),
        });
    }
    let text = String::from_utf8_lossy(stderr);
    let mut ended = Ended::default();
    // An error starts with fd's prefix; the lines after it show where, then say what is wrong,
    // until a note.
    let mut noted = false;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(error) = line.strip_prefix("[fd error]:") {
            ended.errors += 1;
            ended.last_error = Some(error.trim().to_owned());
            noted = false;
        } else if line.starts_with("Note:") || line.starts_with("[fd warning]:") {
            noted = true;
        } else if ended.errors > 0 && !noted {
            ended.last_error = Some(line.to_owned());
        }
    }
    Ok(ended)
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)] // Matches are ranges; one is common.
mod tests {
    use std::os::unix::process::ExitStatusExt as _;

    use tokio::io::AsyncReadExt as _;

    use super::*;

    fn query(pattern: &str) -> Query {
        Query {
            pattern: pattern.to_owned(),
            globs: Vec::new(),
            regex: false,
            match_case: false,
            hidden: false,
        }
    }

    fn strings(args: &[OsString]) -> Vec<&str> {
        args.iter().map(|arg| arg.to_str().unwrap()).collect()
    }

    fn entry(path: &str, dir: bool) -> Entry {
        Entry {
            path: path.as_bytes().to_vec(),
            dir,
        }
    }

    #[test]
    fn looks_for_names_as_they_are_with_smart_case() {
        assert_eq!(
            strings(&query("-x").args()),
            [
                "--print0",
                "--color",
                "never",
                "--show-errors",
                "--fixed-strings",
                "--",
                "-x",
                "."
            ]
        );
    }

    #[test]
    fn looks_with_every_option_and_leaves_out_the_excluded_globs() {
        let every = Query {
            globs: vec!["*.rs".to_owned(), "!target".to_owned()],
            regex: true,
            match_case: true,
            hidden: true,
            ..query("^ma.n$")
        };
        assert_eq!(
            strings(&every.args()),
            [
                "--print0",
                "--color",
                "never",
                "--show-errors",
                "--hidden",
                "--case-sensitive",
                "--exclude",
                "target",
                "--",
                "^ma.n$",
                "."
            ]
        );
        assert_eq!(
            strings(&query("").args())[4..],
            ["--fixed-strings", "--", "", "."],
            "every name"
        );
    }

    #[test]
    fn the_other_globs_keep_names_or_paths_that_match_one() {
        let names = Query {
            globs: vec![
                "*.rs".to_owned(),
                "!target".to_owned(),
                "src/*/mod.rs".to_owned(),
            ],
            ..query("")
        }
        .names()
        .unwrap();
        assert!(names.keeps(b"main.rs"));
        assert!(names.keeps(b"crates/noc/src/main.rs"), "a name anywhere");
        assert!(!names.keeps(b"Cargo.toml"));
        assert!(!names.keeps(b"crates/noc"), "a directory too");
        assert!(names.keeps(b"src/tui/mod.rs"));
        let every = query("").names().unwrap();
        assert!(every.keeps(b"anything") && every.keeps(b"a/b"));
        let excluded = Query {
            globs: vec!["!target".to_owned()],
            ..query("")
        };
        assert!(
            excluded.names().unwrap().keeps(b"src"),
            "fd leaves them out"
        );
    }

    #[test]
    fn shows_where_a_name_matches_as_fd_matches_it() {
        let found = |query: Query, name: &str| query.names().unwrap().found(name.as_bytes());
        assert_eq!(found(query("ab"), "cAb-ab.AB"), [1..3, 4..6, 7..9]);
        assert_eq!(found(query("Ab"), "cAb-ab.AB"), [1..3], "smart case");
        let exact = Query {
            match_case: true,
            ..query("ab")
        };
        assert_eq!(found(exact, "cAb-ab"), [4..6]);
        assert_eq!(found(query("a.b"), "axb a.b"), [4..7], "as it is");
        let regex = Query {
            regex: true,
            ..query("a.b")
        };
        assert_eq!(found(regex.clone(), "axb a.b"), [0..3, 4..7]);
        let class = Query {
            regex: true,
            ..query(r"\Sb")
        };
        assert_eq!(found(class, "AB"), [0..2], "\\S is no capital");
        let none: Vec<Range<usize>> = Vec::new();
        assert_eq!(found(query(""), "anything"), none);
        let anchors = Query {
            regex: true,
            ..query("^")
        };
        assert_eq!(found(anchors, "a"), none, "nothing to show");
    }

    #[test]
    fn says_what_is_wrong_with_a_glob_or_a_pattern() {
        let glob = Query {
            globs: vec!["[".to_owned()],
            ..query("")
        };
        assert!(matches!(glob.names(), Err(PatternError::Glob(_))));
        let regex = Query {
            regex: true,
            ..query("(")
        };
        let Err(PatternError::Regex(message)) = regex.names() else {
            panic!("expected an error");
        };
        assert_eq!(message, "error: unclosed group");
        assert!(query("(").names().is_ok(), "as it is");
    }

    #[tokio::test]
    async fn reads_entries_and_tells_directories() {
        let output = b"./a/\0./a/x.rs\0b\0./link\0\0./\0";
        let mut entries = Entries::new(&output[..]);
        let mut all = Vec::new();
        while let Some(entry) = entries.next().await.unwrap() {
            all.push(entry);
        }
        assert_eq!(
            all,
            [
                entry("a", true),
                entry("a/x.rs", false),
                entry("b", false),
                entry("link", false),
            ]
        );
    }

    #[test]
    fn errors_are_counted_and_the_last_says_what_is_wrong() {
        let program = Path::new("fd");
        let status = |code: i32| ExitStatus::from_raw(code << 8);
        assert_eq!(ended(program, status(0), b"").unwrap(), Ended::default());
        let denied = b"[fd error]: ./a: Permission denied (os error 13)\n\
            [fd error]: ./b: Permission denied (os error 13)\n";
        let end = ended(program, status(0), denied).unwrap();
        assert_eq!(end.errors, 2);
        assert_eq!(
            end.last_error.as_deref(),
            Some("./b: Permission denied (os error 13)")
        );
        let regex = b"[fd error]: regex parse error:\n    (\n    ^\nerror: unclosed group\n\n\
            Note: You can search for literal substrings with '--fixed-strings'.\n";
        let end = ended(program, status(1), regex).unwrap();
        assert_eq!(end.errors, 1);
        assert_eq!(end.last_error.as_deref(), Some("error: unclosed group"));
        let missing = ended(program, status(127), b"sh: fd: command not found\n").unwrap_err();
        assert!(
            matches!(missing, ToolError::Failed { ref stderr, .. } if stderr.contains("not found"))
        );
    }

    #[tokio::test]
    async fn a_missing_program_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let fd = Fd::new("/nonexistent/fd".into());
        let error = fd.spawn(dir.path(), &query("x")).unwrap_err();
        assert!(error.is_not_found(), "{error:?}");
    }

    /// Runs fd in `dir` and returns its entries, sorted, and how it ended, or `None` without
    /// fd.
    async fn run(dir: &Path, query: &Query) -> Option<(Vec<Entry>, Ended)> {
        let fd = Fd::new("fd".into());
        let mut child = match fd.spawn(dir, query) {
            Ok(child) => child,
            Err(error) if error.is_not_found() => return None,
            Err(error) => panic!("{error:?}"),
        };
        let mut entries = Entries::new(child.stdout.take().unwrap());
        let mut stderr = child.stderr.take().unwrap();
        let read_entries = async {
            let mut all = Vec::new();
            while let Some(entry) = entries.next().await.unwrap() {
                all.push(entry);
            }
            all
        };
        let mut errors = Vec::new();
        let (mut all, read) = tokio::join!(read_entries, stderr.read_to_end(&mut errors));
        read.unwrap();
        all.sort_by(|a, b| a.path.cmp(&b.path));
        let status = child.wait().await.unwrap();
        Some((all, ended(fd.program(), status, &errors).unwrap()))
    }

    #[tokio::test]
    async fn runs_fd() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();
        std::fs::write(root.join("src/deep/Main.md"), "").unwrap();
        std::fs::write(root.join(".hidden"), "").unwrap();
        std::fs::write(root.join("ignored.txt"), "").unwrap();
        std::fs::write(root.join(".ignore"), "ignored.txt\n").unwrap();

        let Some((all, end)) = run(root, &query("")).await else {
            return;
        };
        let version = Fd::new("fd".into()).version().await.unwrap();
        assert!(version.starts_with("fd"), "{version}");
        assert_eq!(end, Ended::default());
        assert_eq!(
            all,
            [
                entry("empty", true),
                entry("src", true),
                entry("src/deep", true),
                entry("src/deep/Main.md", false),
                entry("src/main.rs", false),
            ]
        );

        let (all, _) = run(root, &query("main")).await.unwrap();
        let paths: Vec<_> = all.iter().map(|entry| entry.path.as_slice()).collect();
        assert_eq!(paths, [&b"src/deep/Main.md"[..], b"src/main.rs"]);

        let exact = Query {
            match_case: true,
            hidden: true,
            globs: vec!["!deep".to_owned()],
            ..query("main")
        };
        let (all, _) = run(root, &exact).await.unwrap();
        assert_eq!(all, [entry("src/main.rs", false)]);

        let hidden = Query {
            hidden: true,
            ..query(".hid")
        };
        let (all, _) = run(root, &hidden).await.unwrap();
        assert_eq!(all, [entry(".hidden", false)]);

        let bad = Query {
            regex: true,
            ..query("(")
        };
        let (all, end) = run(root, &bad).await.unwrap();
        assert_eq!(all, []);
        assert_eq!(end.last_error.as_deref(), Some("error: unclosed group"));
    }
}
