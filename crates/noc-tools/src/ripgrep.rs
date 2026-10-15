//! [ripgrep](https://github.com/BurntSushi/ripgrep), which searches the files under a directory
//! for a pattern, or lists them. The search window builds its arguments here and reads what it
//! prints with [`Hits`], whether `rg` runs here ([`Ripgrep::spawn`]) or on a remote host
//! through ssh. `--no-config` keeps `RIPGREP_CONFIG_PATH` from changing what it prints; ignore
//! files (`.gitignore` in a git repository, `.ignore`, `.rgignore`) apply as they do in a shell.

use std::ffi::OsString;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use crate::{ToolError, process};

/// The most bytes of a matching line that a [`Hit::Line`] keeps.
const MAX_LINE: usize = 1024;

/// How long `rg --version` may take.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// The most bytes of one message, a JSON line or a listed path, that [`Hits`] holds; longer
/// ones, a match in a huge minified file, are skipped rather than read into memory.
const MAX_RECORD: usize = 16 * 1024 * 1024;

/// What to search for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The text to search for; empty lists the files instead (`--files`).
    pub pattern: String,
    /// The globs of the Files field, split at whitespace by the caller; `!` excludes. Each is
    /// a `--glob`.
    pub globs: Vec<String>,
    /// A regular expression; else `--fixed-strings`.
    pub regex: bool,
    /// `--case-sensitive`; else `--smart-case`.
    pub match_case: bool,
    /// `--hidden`.
    pub hidden: bool,
    /// `--max-count`: the most lines it finds in one file, at least 1.
    pub max_per_file: usize,
}

impl Query {
    /// Whether it lists files rather than lines (pattern empty).
    pub fn lists_files(&self) -> bool {
        self.pattern.is_empty()
    }

    /// rg's arguments, program excluded, searching `.` (the working directory).
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = Vec::new();
        let files = self.lists_files();
        if files {
            args.extend(["--files", "--null"].map(OsString::from));
        } else {
            args.push("--json".into());
        }
        args.push("--no-config".into());
        if !files {
            args.push("--max-count".into());
            args.push(self.max_per_file.max(1).to_string().into());
        }
        if self.hidden {
            args.push("--hidden".into());
        }
        if !files {
            if !self.regex {
                args.push("--fixed-strings".into());
            }
            args.push(
                if self.match_case {
                    "--case-sensitive"
                } else {
                    "--smart-case"
                }
                .into(),
            );
        }
        for glob in &self.globs {
            args.push("--glob".into());
            args.push(glob.into());
        }
        if !files {
            args.push("--regexp".into());
            args.push((&self.pattern).into());
        }
        args.extend(["--", "."].map(OsString::from));
        args
    }
}

/// What rg reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// A file: one that `--files` lists, or the one whose lines come next. Relative to the
    /// directory searched, without a leading `./`, raw bytes.
    File(Vec<u8>),
    /// A line of the file reported last that matches: its number (1-based), its text without
    /// the line end, cut to at most 1024 bytes, and the byte ranges of the matches within that
    /// text, clipped to the cut or left out past it.
    Line {
        number: u64,
        text: Vec<u8>,
        matches: Vec<Range<usize>>,
    },
}

/// Reads what rg prints, hit by hit. Dropping the future of [`Hits::next`] loses nothing: the
/// next call goes on where it stopped.
pub struct Hits<R> {
    reader: BufReader<R>,
    files: bool,
    /// The path of the last [`Hit::File`] of a line search.
    path: Option<Vec<u8>>,
    /// A line whose file was reported first.
    pending: Option<Hit>,
    /// The message being read.
    record: Vec<u8>,
    /// Whether the message being read is too long and skipped.
    overflow: bool,
}

impl<R: AsyncRead + Unpin> Hits<R> {
    /// Reads rg's stdout; `files` as [`Query::lists_files`] of the query it runs.
    pub fn new(stdout: R, files: bool) -> Self {
        Self {
            reader: BufReader::new(stdout),
            files,
            path: None,
            pending: None,
            record: Vec::new(),
            overflow: false,
        }
    }

    /// The next hit, or `None` at the end. Lines that are not rg's JSON, and JSON messages
    /// other than begin and match (end, summary, context), are skipped.
    pub async fn next(&mut self) -> Result<Option<Hit>, ToolError> {
        if let Some(hit) = self.pending.take() {
            return Ok(Some(hit));
        }
        while let Some(record) = self.record().await? {
            if self.files {
                if !record.is_empty() {
                    return Ok(Some(Hit::File(relative(record))));
                }
                continue;
            }
            let Ok(message) = serde_json::from_slice::<Message>(&record) else {
                continue;
            };
            match message {
                Message::Begin { path } => {
                    let Some(path) = path.into_bytes() else {
                        continue;
                    };
                    let path = relative(path);
                    self.path = Some(path.clone());
                    return Ok(Some(Hit::File(path)));
                }
                Message::Match {
                    path,
                    lines,
                    line_number,
                    submatches,
                } => {
                    let (Some(path), Some(text), Some(number)) =
                        (path.into_bytes(), lines.into_bytes(), line_number)
                    else {
                        continue;
                    };
                    let hit = line(number, text, &submatches);
                    let path = relative(path);
                    if self.path.as_ref() == Some(&path) {
                        return Ok(Some(hit));
                    }
                    self.path = Some(path.clone());
                    self.pending = Some(hit);
                    return Ok(Some(Hit::File(path)));
                }
                Message::Other => {}
            }
        }
        Ok(None)
    }

    /// The next message, up to a newline, or a NUL when listing files; `None` at the end.
    /// What has been read stays in `self` when the future is dropped.
    async fn record(&mut self) -> Result<Option<Vec<u8>>, ToolError> {
        let delimiter = if self.files { 0 } else { b'\n' };
        loop {
            let available = self.reader.fill_buf().await?;
            if available.is_empty() {
                let overflow = std::mem::take(&mut self.overflow);
                if overflow || self.record.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(std::mem::take(&mut self.record)));
            }
            let end = available.iter().position(|&byte| byte == delimiter);
            let content = &available[..end.unwrap_or(available.len())];
            if !self.overflow {
                if self.record.len() + content.len() > MAX_RECORD {
                    self.overflow = true;
                    self.record = Vec::new();
                } else {
                    self.record.extend_from_slice(content);
                }
            }
            let used = end.map_or(available.len(), |end| end + 1);
            self.reader.consume(used);
            if end.is_some() {
                if std::mem::take(&mut self.overflow) {
                    continue;
                }
                return Ok(Some(std::mem::take(&mut self.record)));
            }
        }
    }
}

/// A message of `rg --json`; only begin and match matter.
#[derive(Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
enum Message {
    Begin {
        path: Data,
    },
    Match {
        path: Data,
        lines: Data,
        line_number: Option<u64>,
        #[serde(default)]
        submatches: Vec<Submatch>,
    },
    #[serde(other)]
    Other,
}

/// Text as rg prints it: as a string when it is UTF-8, else as base64.
#[derive(Deserialize)]
struct Data {
    text: Option<String>,
    bytes: Option<String>,
}

impl Data {
    fn into_bytes(self) -> Option<Vec<u8>> {
        match (self.text, self.bytes) {
            (Some(text), _) => Some(text.into_bytes()),
            (None, Some(bytes)) => base64(&bytes),
            (None, None) => None,
        }
    }
}

#[derive(Deserialize)]
struct Submatch {
    start: usize,
    end: usize,
}

/// `path` without a leading `./`.
fn relative(mut path: Vec<u8>) -> Vec<u8> {
    if path.starts_with(b"./") {
        path.drain(..2);
    }
    path
}

/// A matching line without its line end, cut to [`MAX_LINE`] bytes, with its matches.
fn line(number: u64, mut text: Vec<u8>, submatches: &[Submatch]) -> Hit {
    if text.ends_with(b"\n") {
        text.pop();
        if text.ends_with(b"\r") {
            text.pop();
        }
    }
    if text.len() > MAX_LINE {
        // Not in the middle of a UTF-8 character, if the text is UTF-8 there.
        let mut cut = MAX_LINE;
        while cut > MAX_LINE - 3 && text[cut] & 0xc0 == 0x80 {
            cut -= 1;
        }
        text.truncate(cut);
    }
    let len = text.len();
    let matches = submatches
        .iter()
        .filter_map(|submatch| {
            let (start, end) = (submatch.start, submatch.end.min(len));
            let empty = submatch.start == submatch.end;
            (start < end || (empty && start <= len)).then_some(start..end)
        })
        .collect();
    Hit::Line {
        number,
        text,
        matches,
    }
}

/// Standard base64, padded or not; `None` for anything else.
fn base64(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        Some(u32::from(value))
    }
    let text = text.as_bytes();
    let data = text
        .strip_suffix(b"==")
        .or_else(|| text.strip_suffix(b"="))
        .unwrap_or(text);
    if data.len() % 4 == 1 || (data.len() < text.len() && !text.len().is_multiple_of(4)) {
        return None;
    }
    let mut bytes = Vec::with_capacity(data.len() / 4 * 3 + 2);
    for chunk in data.chunks(4) {
        let mut bits = 0;
        for &byte in chunk {
            bits = bits << 6 | value(byte)?;
        }
        bits <<= 6 * (4 - chunk.len());
        bytes.extend_from_slice(&bits.to_be_bytes()[1..=chunk.len() * 6 / 8]);
    }
    Some(bytes)
}

/// The rg program of `[search] program`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ripgrep {
    /// A name looked up in `PATH`, or a path.
    program: PathBuf,
}

impl Ripgrep {
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// What rg says it is, such as `ripgrep 15.2.0`: `rg --version`, its first line. A
    /// program that is not there is [`ToolError::is_not_found`].
    pub async fn version(&self) -> Result<String, ToolError> {
        let output = process::output(&self.program, ["--version"], VERSION_TIMEOUT).await?;
        let text = String::from_utf8_lossy(&output);
        Ok(text.lines().next().unwrap_or_default().trim().to_owned())
    }

    /// Starts rg for `query` in `dir`, which is absolute, without a shell, stdin null, stdout
    /// and stderr piped, killed when the child is dropped. Read stderr while reading stdout:
    /// rg stops when either pipe is full. A `dir` that is gone also reads as
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

/// How a search that ran to its end went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ended {
    /// How many errors rg reported: lines of stderr with text.
    pub errors: usize,
    /// The last of them, trimmed.
    pub last_error: Option<String>,
}

/// How rg ended, from its exit `status` and its whole `stderr`: 0 (found) and 1 (found nothing)
/// are Ok, and so is 2 (an error happened, which may be a path it could not read, or a pattern
/// it could not parse); `errors` counts the lines of stderr with text and `last_error` is the
/// last of them, trimmed. Any other status (127 when a remote shell has no rg, a signal) is
/// [`ToolError::Failed`] with `program` and the last line of stderr.
pub fn ended(program: &Path, status: ExitStatus, stderr: &[u8]) -> Result<Ended, ToolError> {
    if !matches!(status.code(), Some(0..=2)) {
        return Err(ToolError::Failed {
            program: program.to_path_buf(),
            status,
            stderr: process::last_line(stderr),
        });
    }
    let text = String::from_utf8_lossy(stderr);
    let mut ended = Ended::default();
    for error in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        ended.errors += 1;
        ended.last_error = Some(error.to_owned());
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
            max_per_file: 3,
        }
    }

    fn strings(args: &[OsString]) -> Vec<&str> {
        args.iter().map(|arg| arg.to_str().unwrap()).collect()
    }

    fn file(path: &str) -> Hit {
        Hit::File(path.as_bytes().to_vec())
    }

    fn hit(number: u64, text: &str, matches: &[Range<usize>]) -> Hit {
        Hit::Line {
            number,
            text: text.as_bytes().to_vec(),
            matches: matches.to_vec(),
        }
    }

    async fn all(output: &[u8], files: bool) -> Vec<Hit> {
        let mut hits = Hits::new(output, files);
        let mut all = Vec::new();
        while let Some(hit) = hits.next().await.unwrap() {
            all.push(hit);
        }
        all
    }

    #[test]
    fn searches_lines_as_plain_text_with_smart_case() {
        let query = query("-x y");
        assert!(!query.lists_files());
        assert_eq!(
            strings(&query.args()),
            [
                "--json",
                "--no-config",
                "--max-count",
                "3",
                "--fixed-strings",
                "--smart-case",
                "--regexp",
                "-x y",
                "--",
                "."
            ]
        );
    }

    #[test]
    fn searches_lines_with_every_option() {
        let query = Query {
            globs: vec!["*.rs".to_owned(), "!target".to_owned()],
            regex: true,
            match_case: true,
            hidden: true,
            max_per_file: 0,
            ..query("fn \\w+")
        };
        assert_eq!(
            strings(&query.args()),
            [
                "--json",
                "--no-config",
                "--max-count",
                "1",
                "--hidden",
                "--case-sensitive",
                "--glob",
                "*.rs",
                "--glob",
                "!target",
                "--regexp",
                "fn \\w+",
                "--",
                "."
            ]
        );
    }

    #[test]
    fn lists_files_without_a_pattern() {
        let plain = query("");
        assert!(plain.lists_files());
        assert_eq!(
            strings(&plain.args()),
            ["--files", "--null", "--no-config", "--", "."]
        );
        let every = Query {
            globs: vec!["*.md".to_owned()],
            regex: true,
            match_case: true,
            hidden: true,
            max_per_file: 7,
            ..plain
        };
        assert_eq!(
            strings(&every.args()),
            [
                "--files",
                "--null",
                "--no-config",
                "--hidden",
                "--glob",
                "*.md",
                "--",
                "."
            ]
        );
    }

    #[tokio::test]
    async fn reads_files_and_their_matching_lines() {
        let output = br#"{"type":"begin","data":{"path":{"text":"./a.txt"}}}
{"type":"match","data":{"path":{"text":"./a.txt"},"lines":{"text":"hello world hello\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"hello"},"start":0,"end":5},{"match":{"text":"hello"},"start":12,"end":17}]}}
{"type":"context","data":{"path":{"text":"./a.txt"},"lines":{"text":"next\n"},"line_number":2,"absolute_offset":18,"submatches":[]}}
{"type":"match","data":{"path":{"text":"./a.txt"},"lines":{"text":"dos hello\r\n"},"line_number":3,"absolute_offset":23,"submatches":[{"match":{"text":"hello"},"start":4,"end":9}]}}
{"type":"end","data":{"path":{"text":"./a.txt"},"binary_offset":null,"stats":{"matches":3}}}
{"type":"begin","data":{"path":{"text":"sub/b c.txt"}}}
{"type":"match","data":{"path":{"text":"sub/b c.txt"},"lines":{"text":"no newline hello"},"line_number":9,"absolute_offset":0,"submatches":[{"match":{"text":"hello"},"start":11,"end":16}]}}
{"type":"end","data":{"path":{"text":"sub/b c.txt"},"binary_offset":null,"stats":{}}}
{"data":{"elapsed_total":{"secs":0}},"type":"summary"}
"#;
        assert_eq!(
            all(output, false).await,
            [
                file("a.txt"),
                hit(1, "hello world hello", &[0..5, 12..17]),
                hit(3, "dos hello", &[4..9]),
                file("sub/b c.txt"),
                hit(9, "no newline hello", &[11..16]),
            ]
        );
    }

    #[tokio::test]
    async fn decodes_paths_and_lines_that_are_not_utf8() {
        // "./\xff.txt" and "a\xfeb\n".
        let output = br#"{"type":"begin","data":{"path":{"bytes":"Li//LnR4dA=="}}}
{"type":"match","data":{"path":{"bytes":"Li//LnR4dA=="},"lines":{"bytes":"Yf5iCg=="},"line_number":2,"absolute_offset":0,"submatches":[{"match":{"text":"b"},"start":2,"end":3}]}}
{"type":"match","data":{"path":{"bytes":"Li//LnR4dA=="},"lines":{"bytes":"not base64!"},"line_number":3,"absolute_offset":0,"submatches":[]}}
"#;
        assert_eq!(
            all(output, false).await,
            [
                Hit::File(b"\xff.txt".to_vec()),
                Hit::Line {
                    number: 2,
                    text: b"a\xfeb".to_vec(),
                    matches: vec![2..3],
                },
            ]
        );
    }

    #[tokio::test]
    async fn cuts_a_long_line_and_its_matches() {
        let text = format!("{}{}\n", "x".repeat(1020), "hello".repeat(4));
        let output = format!(
            r#"{{"type":"begin","data":{{"path":{{"text":"long"}}}}}}
{{"type":"match","data":{{"path":{{"text":"long"}},"lines":{{"text":"{text}"}},"line_number":1,"submatches":[{{"start":0,"end":1}},{{"start":1020,"end":1025}},{{"start":1024,"end":1024}},{{"start":1025,"end":1030}}]}}}}
"#
        )
        .replace("\n\"", "\\n\"");
        let Hit::Line { text, matches, .. } = all(output.as_bytes(), false).await.remove(1) else {
            panic!("expected a line");
        };
        assert_eq!(text.len(), MAX_LINE);
        assert!(text.ends_with(b"xhell"));
        assert_eq!(matches, [0..1, 1020..1024, 1024..1024]);
    }

    #[test]
    fn cuts_a_long_line_between_characters() {
        let text = format!("{}é\n", "x".repeat(MAX_LINE - 1)).into_bytes();
        let Hit::Line { text, .. } = line(1, text, &[]) else {
            panic!("expected a line");
        };
        assert_eq!(text, "x".repeat(MAX_LINE - 1).as_bytes());
    }

    #[tokio::test]
    async fn skips_what_it_cannot_read() {
        let output = br#"rg: warning
{"type":"begin","data":{"path":{"text":"a"}}}

{"type":"match","data":{"path":{"text":"a"},"lines":{"text":"x\n"},"line_number":null,"submatches":[]}}
{"type":"match","data":{"path":{"text":"a"}}}
{"type":"surprise","data":{}}
{"type":"match"
[1, 2]
{"type":"match","data":{"path":{"text":"a"},"lines":{"text":"y\n"},"line_number":4,"submatches":[{"start":0,"end":1}]}}"#;
        assert_eq!(all(output, false).await, [file("a"), hit(4, "y", &[0..1])]);
        assert_eq!(all(b"", false).await, []);
    }

    #[tokio::test]
    async fn reports_the_file_of_a_match_without_its_begin() {
        let output = br#"{"type":"begin","data":{"path":{"text":"a"}}}
{"type":"match","data":{"path":{"text":"./b"},"lines":{"text":"x\n"},"line_number":1,"submatches":[]}}
{"type":"match","data":{"path":{"text":"b"},"lines":{"text":"y\n"},"line_number":2,"submatches":[]}}
"#;
        assert_eq!(
            all(output, false).await,
            [file("a"), file("b"), hit(1, "x", &[]), hit(2, "y", &[])]
        );
    }

    #[tokio::test]
    async fn reads_listed_files() {
        let output = b"./a.txt\0sub/b c\nd\0\0./\xff\0last";
        assert_eq!(
            all(output, true).await,
            [
                file("a.txt"),
                file("sub/b c\nd"),
                Hit::File(b"\xff".to_vec()),
                file("last"),
            ]
        );
    }

    #[test]
    fn decodes_base64() {
        assert_eq!(base64("").unwrap(), b"");
        assert_eq!(base64("bg==").unwrap(), b"n");
        assert_eq!(base64("bm8=").unwrap(), b"no");
        assert_eq!(base64("bm9k").unwrap(), b"nod");
        assert_eq!(base64("bm9kZQ").unwrap(), b"node");
        assert_eq!(base64("+/+/").unwrap(), [0xfb, 0xff, 0xbf]);
        assert_eq!(base64("Z"), None);
        assert_eq!(base64("Zg="), None);
        assert_eq!(base64("Zm9v-A=="), None);
    }

    #[test]
    fn found_nothing_and_errors_are_ok() {
        let rg = Path::new("rg");
        let status = |code| ExitStatus::from_raw(code << 8);
        assert_eq!(ended(rg, status(0), b"").unwrap(), Ended::default());
        assert_eq!(ended(rg, status(1), b"").unwrap(), Ended::default());
        assert_eq!(
            ended(
                rg,
                status(2),
                b"rg: ./a: Permission denied (os error 13)\n\n  rg: ./b: Permission denied  \n"
            )
            .unwrap(),
            Ended {
                errors: 2,
                last_error: Some("rg: ./b: Permission denied".to_owned()),
            }
        );
        match ended(rg, status(127), b"sh: rg: command not found\n").unwrap_err() {
            ToolError::Failed {
                program,
                status,
                stderr,
            } => {
                assert_eq!(program, rg);
                assert_eq!(status.code(), Some(127));
                assert_eq!(stderr, "sh: rg: command not found");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(ended(rg, ExitStatus::from_raw(9), b"").is_err());
    }

    #[tokio::test]
    async fn a_missing_program_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let rg = Ripgrep::new("/nonexistent/rg".into());
        let error = rg.spawn(dir.path(), &query("x")).unwrap_err();
        assert!(error.is_not_found(), "{error:?}");
    }

    /// Runs rg in `dir` and returns its hits and how it ended, or `None` without rg.
    async fn run(dir: &Path, query: &Query) -> Option<(Vec<Hit>, Ended)> {
        let rg = Ripgrep::new("rg".into());
        let mut child = match rg.spawn(dir, query) {
            Ok(child) => child,
            Err(error) if error.is_not_found() => return None,
            Err(error) => panic!("{error:?}"),
        };
        let mut hits = Hits::new(child.stdout.take().unwrap(), query.lists_files());
        let mut stderr = child.stderr.take().unwrap();
        let read_hits = async {
            let mut all = Vec::new();
            while let Some(hit) = hits.next().await.unwrap() {
                all.push(hit);
            }
            all
        };
        let mut errors = Vec::new();
        let (all, read) = tokio::join!(read_hits, stderr.read_to_end(&mut errors));
        read.unwrap();
        let status = child.wait().await.unwrap();
        Some((all, ended(rg.program(), status, &errors).unwrap()))
    }

    /// The hits of each file, in the order of the files' paths.
    fn by_file(hits: Vec<Hit>) -> Vec<(Vec<u8>, Vec<Hit>)> {
        let mut files: Vec<(Vec<u8>, Vec<Hit>)> = Vec::new();
        for hit in hits {
            match hit {
                Hit::File(path) => files.push((path, Vec::new())),
                line @ Hit::Line { .. } => files.last_mut().unwrap().1.push(line),
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    #[tokio::test]
    async fn runs_ripgrep() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::write(root.join("a.txt"), "Hello world\nnothing\nhello -x\n").unwrap();
        std::fs::write(root.join("sub/b.md"), "say hello\n").unwrap();
        std::fs::write(root.join("sub/deep/c.txt"), "hello deep\n").unwrap();
        std::fs::write(root.join(".hidden"), "hello\n").unwrap();
        std::fs::write(root.join("ignored.txt"), "hello\n").unwrap();
        std::fs::write(root.join(".ignore"), "ignored.txt\n").unwrap();

        let search = query("hello");
        let Some((hits, end)) = run(root, &search).await else {
            return;
        };
        let version = Ripgrep::new("rg".into()).version().await.unwrap();
        assert!(version.starts_with("ripgrep "), "{version}");
        assert_eq!(end, Ended::default());
        assert_eq!(
            by_file(hits),
            [
                (
                    b"a.txt".to_vec(),
                    vec![hit(1, "Hello world", &[0..5]), hit(3, "hello -x", &[0..5])]
                ),
                (b"sub/b.md".to_vec(), vec![hit(1, "say hello", &[4..9])]),
                (
                    b"sub/deep/c.txt".to_vec(),
                    vec![hit(1, "hello deep", &[0..5])]
                ),
            ]
        );

        let first = Query {
            max_per_file: 1,
            globs: vec!["a.txt".to_owned()],
            ..search.clone()
        };
        let (hits, _) = run(root, &first).await.unwrap();
        assert_eq!(
            by_file(hits),
            [(b"a.txt".to_vec(), vec![hit(1, "Hello world", &[0..5])])],
            "one line of a file"
        );

        let exact = Query {
            match_case: true,
            hidden: true,
            globs: vec!["!*.md".to_owned()],
            ..search.clone()
        };
        let (hits, _) = run(root, &exact).await.unwrap();
        assert_eq!(
            by_file(hits),
            [
                (b".hidden".to_vec(), vec![hit(1, "hello", &[0..5])]),
                (b"a.txt".to_vec(), vec![hit(3, "hello -x", &[0..5])]),
                (
                    b"sub/deep/c.txt".to_vec(),
                    vec![hit(1, "hello deep", &[0..5])]
                ),
            ]
        );

        let dash = Query {
            pattern: "-x".to_owned(),
            ..search.clone()
        };
        let (hits, _) = run(root, &dash).await.unwrap();
        assert_eq!(
            by_file(hits),
            [(b"a.txt".to_vec(), vec![hit(3, "hello -x", &[6..8])])]
        );

        let list = Query {
            pattern: String::new(),
            hidden: true,
            ..search.clone()
        };
        let (files, end) = run(root, &list).await.unwrap();
        assert_eq!(end, Ended::default());
        let mut paths: Vec<_> = files
            .into_iter()
            .map(|hit| match hit {
                Hit::File(path) => String::from_utf8(path).unwrap(),
                line @ Hit::Line { .. } => panic!("expected a file, got {line:?}"),
            })
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            [".hidden", ".ignore", "a.txt", "sub/b.md", "sub/deep/c.txt"]
        );

        let bad = Query {
            regex: true,
            ..query("(")
        };
        let (hits, end) = run(root, &bad).await.unwrap();
        assert_eq!(hits, []);
        assert!(end.errors > 0);
        assert!(end.last_error.is_some());
    }
}
