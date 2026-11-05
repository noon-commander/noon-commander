//! The log of the TUI: a file of its own for each run, named for the time it started and its
//! process, in the logs directory under the XDG state directory, kept for `log.keep_days` days
//! after it was last written to (ADR 0067). The terminal belongs to the UI. A thread of its own writes the file, so
//! that no event waits for the disk.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use noc_config::{LogConfig, Paths};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;
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
    /// The file it writes, or why there is none.
    pub(crate) file: Result<PathBuf, Failure>,
    _writer: Option<Writer>,
}

/// Why the log cannot be written: what failed at `path`.
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) path: PathBuf,
    pub(crate) error: io::Error,
}

/// Starts the log of this run in the logs directory of `paths`, as `config` says, and removes
/// the files older than its `keep_days`; without a file to write, nothing is logged.
///
/// Blocking: creates and removes files.
pub(crate) fn start(paths: &Paths, config: &LogConfig) -> Log {
    let dir = paths.log_dir();
    let started = match open(&dir) {
        Ok((path, file)) => Writer::start(file)
            .map(|writer| (path.clone(), writer))
            .map_err(|error| Failure { path, error }),
        Err(failure) => Err(failure),
    };
    let (path, writer) = match started {
        Ok(started) => started,
        Err(failure) => {
            return Log {
                file: Err(failure),
                _writer: None,
            };
        }
    };
    // `Context::load` checked the filter.
    let filter = filter(config).unwrap_or_else(|_| EnvFilter::new(config.level.as_str()));
    let (filter, handle) = reload::Layer::new(filter);
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(writer.lines())
        .with_ansi(false)
        .with_span_events(FmtSpan::CLOSE);
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
    if let Err(error) = prune(&dir, &path, config.keep_days, SystemTime::now()) {
        tracing::warn!(dir = %dir.display(), %error, "cannot remove old logs");
    }
    Log {
        file: Ok(path),
        _writer: Some(writer),
    }
}

/// Uses `config` from now on: its filter, and the days it keeps the files.
///
/// Blocking: removes files.
pub(crate) fn apply(config: &LogConfig) {
    let Some(control) = CONTROL.get() else {
        return;
    };
    if let Ok(filter) = filter(config) {
        let _ = control.filter.reload(filter);
    }
    if let Err(error) = prune(
        &control.dir,
        &control.file,
        config.keep_days,
        SystemTime::now(),
    ) {
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
fn open(dir: &Path) -> Result<(PathBuf, File), Failure> {
    fs::create_dir_all(dir).map_err(|error| Failure {
        path: dir.to_path_buf(),
        error,
    })?;
    let started = jiff::Timestamp::now().strftime("%Y%m%dT%H%M%SZ");
    let path = dir.join(format!("{PREFIX}{started}-{}{SUFFIX}", std::process::id()));
    OpenOptions::new()
        .append(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map(|file| (path.clone(), file))
        .map_err(|error| Failure { path, error })
}

/// Removes the log files in `dir` last written to more than `days` days before `now`, but
/// `current`; `0` days keeps them all. A file whose time cannot be read stays.
fn prune(dir: &Path, current: &Path, days: u16, now: SystemTime) -> io::Result<()> {
    if days == 0 {
        return Ok(());
    }
    let most = Duration::from_secs(u64::from(days) * 24 * 60 * 60);
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path == current || !is_log(&path) {
            continue;
        }
        // Not followed, so a link counts as no file.
        let old = entry.metadata().is_ok_and(|metadata| {
            metadata.is_file()
                && metadata
                    .modified()
                    .ok()
                    .and_then(|written| now.duration_since(written).ok())
                    .is_some_and(|age| age > most)
        });
        if old && let Err(error) = fs::remove_file(&path) {
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

    /// Makes the file `name` in `dir`, last written to `days_ago` days before `now`, and an hour
    /// more if `past`.
    fn written(dir: &Path, name: &str, now: SystemTime, days_ago: u64, past: bool) -> PathBuf {
        let path = dir.join(name);
        let file = File::create(&path).unwrap();
        let hours = days_ago * 24 + u64::from(past);
        file.set_modified(now - Duration::from_secs(hours * 60 * 60))
            .unwrap();
        path
    }

    #[test]
    fn removes_the_logs_not_written_to_for_longer_than_the_days_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let now = SystemTime::now();
        written(dir, "noc-20261020T090000Z-1.log", now, 9, false);
        written(dir, "noc-20261025T090000Z-2.log", now, 7, true);
        // Started long ago, but written to lately, as a run that lasts.
        written(dir, "noc-20261001T090000Z-3.log", now, 7, false);
        written(dir, "noc-20261104T090000Z-4.log", now, 0, false);
        // Not logs of noc.
        written(dir, "noc.log", now, 30, false);
        written(dir, "notes.txt", now, 30, false);
        fs::create_dir(dir.join("noc-20260901T090000Z-5.log")).unwrap();
        // This run's file, whatever its time.
        let current = written(dir, "noc-20261019T090000Z-6.log", now, 30, false);
        prune(dir, &current, 0, now).unwrap();
        assert_eq!(names(dir).len(), 8, "0 keeps them all");
        prune(dir, &current, 7, now).unwrap();
        assert_eq!(
            names(dir),
            [
                "noc-20260901T090000Z-5.log",
                "noc-20261001T090000Z-3.log",
                "noc-20261019T090000Z-6.log",
                "noc-20261104T090000Z-4.log",
                "noc.log",
                "notes.txt",
            ]
        );
        prune(dir, &current, 1, now).unwrap();
        assert_eq!(
            names(dir),
            [
                "noc-20260901T090000Z-5.log",
                "noc-20261019T090000Z-6.log",
                "noc-20261104T090000Z-4.log",
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
    fn says_where_the_log_cannot_be_made() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("state");
        fs::write(&blocker, "").unwrap();
        let failure = open(&blocker.join("logs")).unwrap_err();
        assert_eq!(failure.path, blocker.join("logs"));
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
