//! Running jobs: copying, deleting, checksums, renaming, and writing files.

use std::path::PathBuf;

use noc_ops::{Algorithm, Checksum, CopyOptions, Endpoint, Files, RenameError, Reporter};
use noc_vfs::{
    FileKind, FileWriter as _, LocalFs, Location, RemotePath, Vfs, VfsError, VfsPath as _,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::describe;

use super::{
    Done, HostHandle, HostLink, JobEvent, Refused, Rejoin, TakenFile, finished, forward, share,
};

/// Runs the delete job `id` on `targets` with `vfs`, passing its reports on to `done` with
/// paths made locations by `location`, and returns its end.
pub(super) async fn run_delete<V: Vfs>(
    vfs: &V,
    targets: Vec<V::Path>,
    cancel: CancellationToken,
    id: u64,
    done: &mpsc::UnboundedSender<Done>,
    location: impl Fn(V::Path) -> Location,
) -> Done {
    let (events, incoming) = mpsc::unbounded_channel();
    let reporter = Reporter::new(events, cancel.clone());
    let work = async move {
        let mut reporter = reporter;
        noc_ops::delete(vfs, targets, &mut reporter).await
    };
    let work = noc_ops::in_time(&cancel, work);
    let (outcome, ()) = tokio::join!(work, forward(incoming, id, done, location));
    finished(id, outcome)
}

/// Files found for a checksum job, on the file system they are on.
pub(super) enum Found {
    Local(Files<PathBuf>),
    Remote(Rejoin, String, Files<RemotePath>),
}

/// Runs the checksum job `id` on `targets`, each group local or on the host of its handle:
/// counts the files of every group first, so that progress has one total, then hashes them.
pub(super) async fn run_checksum(
    id: u64,
    targets: Vec<(Vec<Location>, Option<HostLink>)>,
    algorithm: Algorithm,
    cancel: CancellationToken,
    done: &mpsc::UnboundedSender<Done>,
) -> Done {
    let sessions: Vec<_> = targets
        .into_iter()
        .map(|(group, link)| (group, link.map(Rejoin::new)))
        .collect();
    let (events, incoming) = mpsc::unbounded_channel();
    let reporter = Reporter::new(events, cancel.clone());
    let work = async move {
        let mut reporter = reporter;
        let mut job = Checksum::new(algorithm, &mut reporter);
        let here = |path: &PathBuf| Location::Local(path.clone());
        let mut found = Vec::new();
        for (group, fs) in sessions {
            match fs {
                None => {
                    let paths = group
                        .into_iter()
                        .filter_map(|location| match location {
                            Location::Local(path) => Some(path),
                            Location::Remote { .. } => None,
                        })
                        .collect();
                    let side = Endpoint {
                        vfs: &LocalFs,
                        report: &here,
                    };
                    found.push(Found::Local(job.scan(&side, paths).await));
                }
                Some(fs) => {
                    let mut host = String::new();
                    let paths = group
                        .into_iter()
                        .filter_map(|location| match location {
                            Location::Remote { host: on, path } => {
                                host = on;
                                Some(path)
                            }
                            Location::Local(_) => None,
                        })
                        .collect();
                    let there = remote_location(host.clone());
                    let side = Endpoint {
                        vfs: &fs,
                        report: &there,
                    };
                    let files = job.scan(&side, paths).await;
                    found.push(Found::Remote(fs, host, files));
                }
            }
        }
        for files in found {
            match files {
                Found::Local(files) => {
                    let side = Endpoint {
                        vfs: &LocalFs,
                        report: &here,
                    };
                    job.hash(&side, files).await;
                }
                Found::Remote(fs, host, files) => {
                    let there = remote_location(host);
                    let side = Endpoint {
                        vfs: &fs,
                        report: &there,
                    };
                    job.hash(&side, files).await;
                }
            }
        }
        job.finish()
    };
    let work = noc_ops::in_time(&cancel, work);
    let (ended, ()) = tokio::join!(work, forward(incoming, id, done, |path| path));
    let Some((outcome, sums)) = ended else {
        return finished(id, None);
    };
    if !outcome.aborted {
        let _ = done.send(Done::Job {
            id,
            event: JobEvent::Sums(sums),
        });
    }
    finished(id, Some(outcome))
}

/// How a remote path on `host` is reported.
pub(super) fn remote_location(host: String) -> impl Fn(&RemotePath) -> Location + Send + Sync {
    move |path| Location::Remote {
        host: host.clone(),
        path: path.clone(),
    }
}

/// Writes `bytes` to the file at `location`, over a file there only if `replace`.
pub(super) async fn write_file(
    location: &Location,
    bytes: Vec<u8>,
    replace: bool,
    host: Option<HostHandle>,
) -> Result<(), Refused> {
    match (location, share(host).await) {
        (Location::Local(path), _) => write_all(&LocalFs, path, bytes, replace).await,
        (Location::Remote { path, .. }, Some(Some(fs))) => {
            write_all(&*fs, path, bytes, replace).await
        }
        _ => Err(Refused::Failed(fl!("error-connection-closed").into())),
    }
}

pub(super) async fn write_all<V: Vfs>(
    vfs: &V,
    path: &V::Path,
    bytes: Vec<u8>,
    replace: bool,
) -> Result<(), Refused> {
    let written = async {
        let mut writer = vfs.create_file(path, replace).await?;
        writer.write(bytes).await?;
        writer.finish().await
    };
    match written.await {
        Ok(()) => Ok(()),
        Err(VfsError::AlreadyExists(..)) => Err(taken(vfs, None, path).await),
        Err(error) => Err(Refused::Failed(describe::vfs_reason(&error))),
    }
}

/// Renames `from` to `to`, in the same directory, over a file there only if `replace`.
pub(super) async fn rename(
    from: &Location,
    to: &Location,
    replace: bool,
    host: Option<HostHandle>,
) -> Result<(), Refused> {
    match (from, to, share(host).await) {
        (Location::Local(from), Location::Local(to), _) => {
            rename_on(&LocalFs, from, to, replace).await
        }
        (
            Location::Remote { path: from, .. },
            Location::Remote { path: to, .. },
            Some(Some(fs)),
        ) => rename_on(&*fs, from, to, replace).await,
        _ => Err(Refused::Failed(fl!("error-connection-closed").into())),
    }
}

pub(super) async fn rename_on<V: Vfs>(
    vfs: &V,
    from: &V::Path,
    to: &V::Path,
    replace: bool,
) -> Result<(), Refused> {
    match noc_ops::rename(vfs, from, to, replace).await {
        Ok(()) => Ok(()),
        Err(RenameError::Exists(FileKind::Dir)) => {
            Err(Refused::Failed(fl!("rename-dir-exists").into()))
        }
        Err(RenameError::Exists(_)) => Err(taken(vfs, Some(from), to).await),
        Err(RenameError::Vfs(error)) => Err(Refused::Failed(describe::vfs_reason(&error))),
    }
}

/// What the question about the name of `target`, which a file has, shows: what `source`, the
/// entry renamed, if any, and that file are, and a name free beside it; or why it cannot be
/// told.
pub(super) async fn taken<V: Vfs>(vfs: &V, source: Option<&V::Path>, target: &V::Path) -> Refused {
    let found = async {
        let source = match source {
            Some(source) => Some(vfs.symlink_metadata(source).await?),
            None => None,
        };
        let metadata = vfs.symlink_metadata(target).await?;
        let free = match (target.parent(), target.name()) {
            (Some(dir), Some(name)) => {
                let free = noc_ops::free_name(vfs, &dir, name).await?;
                free.name().unwrap_or_default().to_vec()
            }
            _ => Vec::new(),
        };
        Ok::<_, VfsError>(TakenFile {
            source,
            target: metadata,
            free,
        })
    };
    match found.await {
        Ok(taken) => Refused::Taken(Box::new(taken)),
        Err(error) => Refused::Failed(describe::vfs_reason(&error)),
    }
}

/// How a side of a copy reports its paths.
pub(super) type Report<'a, P> = &'a (dyn Fn(&P) -> Location + Send + Sync);

/// A copy job, which runs in a task of its own with the sessions of the hosts at its ends.
pub(super) struct CopyJob<'a> {
    pub(super) id: u64,
    pub(super) options: CopyOptions,
    pub(super) cancel: CancellationToken,
    pub(super) done: &'a mpsc::UnboundedSender<Done>,
}

impl CopyJob<'_> {
    /// Copies `sources`, all local or all on one host, to `target`, or moves them if the
    /// options remove sources; `hosts` lead to the hosts of the sources and of the target, if
    /// they are remote.
    pub(super) async fn run(
        self,
        sources: Vec<Location>,
        target: Location,
        (from, to): (Option<HostLink>, Option<HostLink>),
    ) -> Done {
        let (from, to) = (from.map(Rejoin::new), to.map(Rejoin::new));
        let mut local: Vec<PathBuf> = Vec::new();
        let mut remote = Vec::new();
        let mut source_host = String::new();
        for source in sources {
            match source {
                Location::Local(path) => local.push(path),
                Location::Remote { host, path } => {
                    source_host = host;
                    remote.push(path);
                }
            }
        }
        let on_host = |host: String| {
            move |path: &RemotePath| Location::Remote {
                host: host.clone(),
                path: path.clone(),
            }
        };
        let here = |path: &PathBuf| Location::Local(path.clone());
        match (from, to, target) {
            // Within one file system, a move renames, and neither goes over its source under a
            // name spelled otherwise.
            (None, None, Location::Local(target)) => {
                self.within(&LocalFs, local, target, &here).await
            }
            (Some(from), Some(_), Location::Remote { host, path }) if host == source_host => {
                let there = on_host(host);
                self.within(&from, remote, path, &there).await
            }
            (None, Some(to), Location::Remote { host, path }) => {
                let there = on_host(host);
                self.copy((&LocalFs, local, &here), (&to, path, &there))
                    .await
            }
            (Some(from), None, Location::Local(target)) => {
                let there = on_host(source_host);
                self.copy((&from, remote, &there), (&LocalFs, target, &here))
                    .await
            }
            (Some(from), Some(to), Location::Remote { host, path }) => {
                let (source, target) = (on_host(source_host), on_host(host));
                self.copy((&from, remote, &source), (&to, path, &target))
                    .await
            }
            _ => finished(self.id, None),
        }
    }

    /// Copies, or moves if the options remove sources, within one file system.
    async fn within<V: Vfs>(
        &self,
        vfs: &V,
        sources: Vec<V::Path>,
        target: V::Path,
        report: Report<'_, V::Path>,
    ) -> Done {
        let (events, incoming) = mpsc::unbounded_channel();
        let reporter = Reporter::new(events, self.cancel.clone());
        let options = self.options;
        let work = async move {
            let mut reporter = reporter;
            let side = Endpoint { vfs, report };
            if options.remove_sources {
                noc_ops::move_within(side, sources, target, options, &mut reporter).await
            } else {
                noc_ops::copy_within(side, sources, target, options, &mut reporter).await
            }
        };
        let work = noc_ops::in_time(&self.cancel, work);
        let (outcome, ()) = tokio::join!(work, forward(incoming, self.id, self.done, |path| path));
        finished(self.id, outcome)
    }

    async fn copy<A: Vfs, B: Vfs>(
        &self,
        (from, sources, from_report): (&A, Vec<A::Path>, Report<'_, A::Path>),
        (to, target, to_report): (&B, B::Path, Report<'_, B::Path>),
    ) -> Done {
        let (events, incoming) = mpsc::unbounded_channel();
        let reporter = Reporter::new(events, self.cancel.clone());
        let options = self.options;
        let work = async move {
            let mut reporter = reporter;
            let from = Endpoint {
                vfs: from,
                report: from_report,
            };
            let to = Endpoint {
                vfs: to,
                report: to_report,
            };
            noc_ops::copy(from, sources, to, target, options, &mut reporter).await
        };
        let work = noc_ops::in_time(&self.cancel, work);
        let (outcome, ()) = tokio::join!(work, forward(incoming, self.id, self.done, |path| path));
        finished(self.id, outcome)
    }
}
