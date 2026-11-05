//! The log of the TUI: a file of its own for each run, named for the time it started and its
//! process, in the logs directory under the XDG state directory, of which the last `log.keep`
//! are kept (ADR 0067). The terminal belongs to the UI. A thread of its own writes the file, so
//! that no event waits for the disk.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::num::NonZeroUsize;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use noc_config::{LogConfig, Paths};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Registry, reload};

/// How the names of log files start and end; others in the directory are left alone.
const PREFIX: &str = "noc-";
const SUFFIX: &str = ".log";

/// What a change of the settings changes while the TUI runs.
struct Control {
    filter: reload::Handle<EnvFilter, Registry>,
    dir: PathBuf,
    /// The file of this run, which is always kept.
    file: PathBuf,
}

static CONTROL: OnceLock<Control> = OnceLock::new();

/// The log of this run. Dropping it ends the log: the events before then reach the file.
pub(crate) struct Log {
    /// The file it writes, if it could make one.
    pub(crate) file: Option<PathBuf>,
    _writer: Option<Writer>,
}

/// Starts the log of this run in the logs directory of `paths`, as `config` says, and removes
/// the files over its `keep`; without a file to write, nothing is logged.
///
/// Blocking: creates and removes files.
pub(crate) fn start(paths: &Paths, config: &LogConfig) -> Log {
    let dir = paths.log_dir();
    let started =
        open(&dir).and_then(|(path, file)| Writer::start(file).map(|writer| (path, writer)));
    let Ok((path, writer)) = started else {
        return Log {
            file: None,
            _writer: None,
        };
    };
    // `Context::load` checked the filter.
    let filter = filter(config).unwrap_or_else(|_| EnvFilter::new(config.level.as_str()));
    let (filter, handle) = reload::Layer::new(filter);
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(writer.lines())
        .with_ansi(false);
    if tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init()
        .is_ok()
    {
        let _ = CONTROL.set(Control {
            filter: handle,
            dir: dir.clone(),
            file: path.clone(),
        });
    }
    tracing::info!(
        pid = std::process::id(),
        version = env!("CARGO_PKG_VERSION"),
        "noc started"
    );
    if let Err(error) = prune(&dir, &path, config.keep) {
        tracing::warn!(dir = %dir.display(), %error, "cannot remove old logs");
    }
    Log {
        file: Some(path),
        _writer: Some(writer),
    }
}

/// Uses `config` from now on: its filter, and as many files as it keeps.
///
/// Blocking: removes files.
pub(crate) fn apply(config: &LogConfig) {
    let Some(control) = CONTROL.get() else {
        return;
    };
    if let Ok(filter) = filter(config) {
        let _ = control.filter.reload(filter);
    }
    if let Err(error) = prune(&control.dir, &control.file, config.keep) {
        tracing::warn!(dir = %control.dir.display(), %error, "cannot remove old logs");
    }
}

/// The filter that `config` sets: its directives, or its level where they are empty; or why the
/// directives are not valid.
pub(crate) fn filter(config: &LogConfig) -> Result<EnvFilter, String> {
    let directives = config.filter.trim();
    if directives.is_empty() {
        return Ok(EnvFilter::new(config.level.as_str()));
    }
    EnvFilter::try_new(directives).map_err(|error| error.to_string())
}

/// Creates `dir` and a new file in it for this run, readable by the user alone.
fn open(dir: &Path) -> io::Result<(PathBuf, File)> {
    fs::create_dir_all(dir)?;
    let started = jiff::Timestamp::now().strftime("%Y%m%dT%H%M%SZ");
    let path = dir.join(format!("{PREFIX}{started}-{}{SUFFIX}", std::process::id()));
    let file = OpenOptions::new()
        .append(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    Ok((path, file))
}

/// Removes the oldest log files in `dir` but `keep`, `current` always among those kept. The
/// names start with the time in UTC, so they sort as the files were made.
fn prune(dir: &Path, current: &Path, keep: NonZeroUsize) -> io::Result<()> {
    let mut others: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| path != current && is_log(path))
        .collect();
    others.sort();
    let over = others.len().saturating_sub(keep.get() - 1);
    for path in others.drain(..over) {
        if let Err(error) = fs::remove_file(&path) {
            tracing::debug!(path = %path.display(), %error, "cannot remove an old log");
        }
    }
    Ok(())
}

fn is_log(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with(PREFIX) && name.ends_with(SUFFIX))
}

/// The thread that writes the file, and the way to it.
struct Writer {
    lines: Sender<Vec<u8>>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    fn start(file: File) -> io::Result<Self> {
        let (lines, received) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("noc-log".to_owned())
            .spawn(move || write_lines(file, &received))?;
        Ok(Self {
            lines,
            thread: Some(thread),
        })
    }

    fn lines(&self) -> Lines {
        Lines(self.lines.clone())
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // An empty line, which no event makes, ends the thread once it wrote those before.
        let _ = self.lines.send(Vec::new());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Writes the lines to `file` as they come, until an empty one; it flushes whenever no more
/// wait.
fn write_lines(file: File, lines: &Receiver<Vec<u8>>) {
    let mut out = BufWriter::new(file);
    while let Ok(line) = lines.recv() {
        let mut next = Some(line);
        while let Some(line) = next.take() {
            if line.is_empty() {
                let _ = out.flush();
                return;
            }
            let _ = out.write_all(&line);
            next = lines.try_recv().ok();
        }
        let _ = out.flush();
    }
    let _ = out.flush();
}

/// Gives each event a buffer, which goes to the thread whole.
struct Lines(Sender<Vec<u8>>);

impl<'a> MakeWriter<'a> for Lines {
    type Writer = Line<'a>;

    fn make_writer(&'a self) -> Line<'a> {
        Line {
            text: Vec::new(),
            to: &self.0,
        }
    }
}

/// What an event writes, sent to the thread as it drops; sending never waits.
struct Line<'a> {
    text: Vec<u8>,
    to: &'a Sender<Vec<u8>>,
}

impl Write for Line<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.text.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Line<'_> {
    fn drop(&mut self) {
        if !self.text.is_empty() {
            let _ = self.to.send(std::mem::take(&mut self.text));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use noc_config::LogLevel;

    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn keeps_the_newest_logs_and_this_runs_and_leaves_other_files_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for name in [
            "noc-20261101T090000Z-11.log",
            "noc-20261102T090000Z-12.log",
            "noc-20261103T090000Z-13.log",
            "noc-20261104T090000Z-14.log",
            "noc.log",
            "notes.txt",
        ] {
            fs::write(dir.join(name), "").unwrap();
        }
        fs::create_dir(dir.join("noc-20261001T090000Z-1.log")).unwrap();
        // This run's file is older than the others, as after the clock went back.
        let current = dir.join("noc-20261031T090000Z-10.log");
        fs::write(&current, "").unwrap();
        prune(dir, &current, NonZeroUsize::new(3).unwrap()).unwrap();
        assert_eq!(
            names(dir),
            [
                "noc-20261001T090000Z-1.log",
                "noc-20261031T090000Z-10.log",
                "noc-20261103T090000Z-13.log",
                "noc-20261104T090000Z-14.log",
                "noc.log",
                "notes.txt",
            ]
        );
        prune(dir, &current, NonZeroUsize::MIN).unwrap();
        assert_eq!(
            names(dir),
            [
                "noc-20261001T090000Z-1.log",
                "noc-20261031T090000Z-10.log",
                "noc.log",
                "notes.txt",
            ]
        );
    }

    #[test]
    fn a_new_file_for_each_run_named_for_its_time_and_process() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("state/noc/logs");
        let (path, _file) = open(&dir).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        let pid = format!("-{}{SUFFIX}", std::process::id());
        let stamp = name
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.strip_suffix(&pid))
            .unwrap();
        assert_eq!(stamp.len(), "20261105T142301Z".len(), "{name}");
        assert!(
            stamp.ends_with('Z') && stamp.as_bytes()[8] == b'T',
            "{name}"
        );
        let mode = fs::metadata(&path).unwrap().permissions();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
            0o600
        );
        // The same name again is not taken over.
        let error = OpenOptions::new()
            .append(true)
            .create_new(true)
            .open(&path)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn no_log_is_made_under_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("state");
        fs::write(&blocker, "").unwrap();
        assert!(open(&blocker.join("logs")).is_err());
    }

    #[test]
    fn every_line_reaches_the_file_before_the_writer_ends() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("noc-test.log");
        let file = File::create(&path).unwrap();
        let writer = Writer::start(file).unwrap();
        let lines = writer.lines();
        for n in 0..1000 {
            let mut line = lines.make_writer();
            write!(line, "line {n}").unwrap();
            line.write_all(b"\n").unwrap();
        }
        // A writer that writes nothing sends nothing, so it does not end the log.
        drop(lines.make_writer());
        drop(writer);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1000);
        assert_eq!(text.lines().last(), Some("line 999"));
        // Lines after the end go nowhere, and nothing waits for them.
        write!(lines.make_writer(), "late").unwrap();
    }

    #[test]
    fn the_filter_is_the_level_unless_directives_take_its_place() {
        let config = |level, filter: &str| LogConfig {
            level,
            filter: filter.to_owned(),
            ..LogConfig::default()
        };
        let shown = |config: &LogConfig| filter(config).map(|filter| filter.to_string());
        assert_eq!(shown(&config(LogLevel::Debug, "")).unwrap(), "debug");
        assert_eq!(shown(&config(LogLevel::Off, "  ")).unwrap(), "off");
        assert_eq!(
            shown(&config(LogLevel::Off, " info,noc_ssh=trace ")).unwrap(),
            "noc_ssh=trace,info"
        );
        assert!(filter(&config(LogLevel::Info, "noc_ssh=loud")).is_err());
    }
}
