//! Searching through files with ripgrep and fd, here or on a host.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noc_tools::ToolError;
use noc_tools::fd::{self, Entries, Fd, Names};
use noc_tools::ripgrep::{self, Hits, Query, Ripgrep};
use noc_vfs::{Location, RemotePath};
use tokio::io::AsyncReadExt as _;
use tokio::sync::{mpsc, oneshot};

use crate::i18n::fl;
use crate::tui::describe;
use crate::tui::find::{FindSummary, Finding};

use super::{Done, HostHandle, HostRequest};

/// How often a search of a tree reports what it found, at most: the window takes its rows
/// then, not after each line.
pub(super) const FIND_REPORTS: Duration = Duration::from_millis(50);

/// ripgrep on a host, as the host's `PATH` finds it.
pub(super) const REMOTE_RIPGREP: &str = "rg";

/// fd on a host, as the host's `PATH` finds it; Debian and Ubuntu name it `fdfind`.
pub(super) const REMOTE_FD: [&str; 2] = ["fd", "fdfind"];

/// The most of the stderr of ripgrep or fd that is kept, for why it failed.
pub(super) const SEARCH_STDERR: u64 = 64 * 1024;

/// What ripgrep or fd found and has not been reported yet.
pub(super) struct Batches {
    pub(super) generation: u64,
    pub(super) found: Vec<Finding>,
    /// When it reported last.
    pub(super) sent: Option<Instant>,
    pub(super) done: mpsc::UnboundedSender<Done>,
}

impl Batches {
    /// Takes what was found, and reports it with what came before if the last report was long
    /// enough ago.
    fn take(&mut self, finding: Finding) {
        self.found.push(finding);
        if self.sent.is_none_or(|sent| sent.elapsed() >= FIND_REPORTS) {
            self.flush();
        }
    }

    /// Reports what was found and not reported yet, if anything.
    pub(super) fn flush(&mut self) {
        if self.found.is_empty() {
            return;
        }
        self.sent = Some(Instant::now());
        let found = std::mem::take(&mut self.found);
        let _ = self.done.send(Done::Found {
            generation: self.generation,
            found,
        });
    }
}

/// A search of a tree with ripgrep: what it looks for, and the most results it takes.
pub(super) struct Searched<'a> {
    pub(super) query: &'a Query,
    pub(super) max_results: usize,
}

impl Searched<'_> {
    /// Runs ripgrep in `location`, locally as `ripgrep`, or on its host through `host`, and
    /// passes what it finds to `batches`; then how it ended, or why it could not search.
    pub(super) async fn search(
        &self,
        ripgrep: &Ripgrep,
        location: Location,
        host: Option<HostHandle>,
        batches: &mut Batches,
    ) -> Result<FindSummary, String> {
        let (mut child, program) = match location {
            Location::Local(path) => {
                let child = ripgrep
                    .spawn(&path, self.query)
                    .map_err(|error| describe::ripgrep_error(&error))?;
                (child, ripgrep.program().to_path_buf())
            }
            Location::Remote { path, .. } => {
                let handle = host.ok_or_else(|| fl!("error-connection-closed"))?;
                let args = self.query.args();
                let child = remote_spawn(&handle, path, REMOTE_RIPGREP, args).await?;
                (child, PathBuf::from(REMOTE_RIPGREP))
            }
        };
        let closed = || fl!("error-connection-closed");
        let stdout = child.stdout.take().ok_or_else(closed)?;
        let stderr = child.stderr.take().ok_or_else(closed)?;
        // ripgrep stops once a pipe is full: its stderr is read while its stdout is.
        let stderr = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(SEARCH_STDERR).read_to_end(&mut bytes).await;
            bytes
        });
        let limit = self.read(stdout, batches).await?;
        if limit {
            // What is left is never read: ripgrep goes.
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Ok(FindSummary {
                limit: true,
                ..FindSummary::default()
            });
        }
        let status = child.wait().await.map_err(|error| error.to_string())?;
        let stderr = stderr.await.unwrap_or_default();
        let ended = ripgrep::ended(&program, status, &stderr)
            .map_err(|error| describe::ripgrep_error(&error))?;
        Ok(FindSummary {
            limit: false,
            errors: ended.errors,
            last_error: ended.last_error,
        })
    }

    /// Reads what ripgrep prints into `batches`, reporting them at least every
    /// [`FIND_REPORTS`] while it is slow; `true` once it found `max_results`.
    async fn read(
        &self,
        stdout: impl tokio::io::AsyncRead + Unpin,
        batches: &mut Batches,
    ) -> Result<bool, String> {
        let files = self.query.lists_files();
        let mut hits = Hits::new(stdout, files);
        let mut results = 0;
        loop {
            // Reading is cancel-safe: what a timeout cut off is read on the next turn.
            let Ok(next) = tokio::time::timeout(FIND_REPORTS, hits.next()).await else {
                batches.flush();
                continue;
            };
            let Some(hit) = next.map_err(|error| describe::chain(&error))? else {
                return Ok(false);
            };
            let counts = files || matches!(hit, ripgrep::Hit::Line { .. });
            batches.take(Finding::Hit(hit));
            if counts {
                results += 1;
                if results >= self.max_results {
                    return Ok(true);
                }
            }
        }
    }
}

/// A search of a tree for names with fd: what it looks for, and the most it takes.
pub(super) struct Named<'a> {
    pub(super) query: &'a fd::Query,
    pub(super) max_results: usize,
}

impl Named<'_> {
    /// Runs fd in `location`, locally as `fd`, or on its host through `host`, as `fd` or, where
    /// the host has none, as `fdfind`, and passes what it finds to `batches`; then how it
    /// ended, or why it could not search. A glob or a pattern that is not one says so in place
    /// of the rows, as ripgrep's error does.
    pub(super) async fn search(
        &self,
        fd: &Fd,
        location: Location,
        host: Option<HostHandle>,
        batches: &mut Batches,
    ) -> Result<FindSummary, String> {
        let names = match self.query.names() {
            Ok(names) => names,
            Err(error) => {
                return Ok(FindSummary {
                    limit: false,
                    errors: 1,
                    last_error: Some(error.to_string()),
                });
            }
        };
        let ended = match location {
            Location::Local(path) => {
                let child = fd
                    .spawn(&path, self.query)
                    .map_err(|error| describe::fd_error(&error))?;
                self.run(child, fd.program(), &names, batches).await?
            }
            Location::Remote { path, .. } => {
                let handle = host.ok_or_else(|| fl!("error-connection-closed"))?;
                let mut ended = Ok(FindSummary::default());
                for program in REMOTE_FD {
                    let args = self.query.args();
                    let child = remote_spawn(&handle, path.clone(), program, args).await?;
                    ended = self.run(child, Path::new(program), &names, batches).await?;
                    // The host's shell says 127 for a program it does not find.
                    let missing = matches!(&ended, Err(ToolError::Failed { status, .. })
                        if status.code() == Some(127));
                    if !missing {
                        break;
                    }
                }
                ended
            }
        };
        ended.map_err(|error| describe::fd_error(&error))
    }

    /// Reads what `child`, fd as `program`, prints, its stderr at the same time, and how it
    /// ended: why reading failed, or else how fd ended, or that it stopped at the limit.
    async fn run(
        &self,
        mut child: tokio::process::Child,
        program: &Path,
        names: &Names,
        batches: &mut Batches,
    ) -> Result<Result<FindSummary, ToolError>, String> {
        let closed = || fl!("error-connection-closed");
        let stdout = child.stdout.take().ok_or_else(closed)?;
        let stderr = child.stderr.take().ok_or_else(closed)?;
        let stderr = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(SEARCH_STDERR).read_to_end(&mut bytes).await;
            bytes
        });
        if self.read(stdout, names, batches).await? {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Ok(Ok(FindSummary {
                limit: true,
                ..FindSummary::default()
            }));
        }
        let status = child.wait().await.map_err(|error| error.to_string())?;
        let stderr = stderr.await.unwrap_or_default();
        Ok(
            fd::ended(program, status, &stderr).map(|ended| FindSummary {
                limit: false,
                errors: ended.errors,
                last_error: ended.last_error,
            }),
        )
    }

    /// Reads what fd prints into `batches`, the entries that `names` keeps, with where their
    /// names match, reporting them at least every [`FIND_REPORTS`] while it is slow; `true`
    /// once it found `max_results`.
    async fn read(
        &self,
        stdout: impl tokio::io::AsyncRead + Unpin,
        names: &Names,
        batches: &mut Batches,
    ) -> Result<bool, String> {
        let mut entries = Entries::new(stdout);
        let mut results = 0;
        loop {
            // Reading is cancel-safe: what a timeout cut off is read on the next turn.
            let Ok(next) = tokio::time::timeout(FIND_REPORTS, entries.next()).await else {
                batches.flush();
                continue;
            };
            let Some(entry) = next.map_err(|error| describe::chain(&error))? else {
                return Ok(false);
            };
            if !names.keeps(&entry.path) {
                continue;
            }
            let name = entry
                .path
                .iter()
                .rposition(|&byte| byte == b'/')
                .map_or(0, |slash| slash + 1);
            let matches = names
                .found(&entry.path[name..])
                .into_iter()
                .map(|found| found.start + name..found.end + name)
                .collect();
            batches.take(Finding::Entry {
                path: entry.path,
                dir: entry.dir,
                matches,
            });
            results += 1;
            if results >= self.max_results {
                return Ok(true);
            }
        }
    }
}

/// Starts `program` with `args` in `dir` on the host of `handle`, over its connection, or why
/// it cannot.
pub(super) async fn remote_spawn(
    handle: &HostHandle,
    dir: RemotePath,
    program: &'static str,
    args: Vec<OsString>,
) -> Result<tokio::process::Child, String> {
    let (reply, answer) = oneshot::channel();
    let request = HostRequest::Spawn {
        dir,
        program,
        args,
        reply,
    };
    let closed = || fl!("error-connection-closed");
    handle.0.send(request).map_err(|_| closed())?;
    answer.await.map_err(|_| closed())?
}
