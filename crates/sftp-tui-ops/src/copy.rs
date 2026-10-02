//! Copying files, symlinks, and directory trees between any two backends.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

use sftp_tui_vfs::{FileKind, FileReader as _, FileWriter as _, Metadata, Vfs, VfsError, VfsPath};

use crate::job::{Conflict, Decision, Event, Outcome, Progress, Reporter};

/// How to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyOptions {
    /// Give copies the modification times and permission bits of their sources.
    pub preserve: bool,
    /// Write each file under a temporary name next to its target, then rename it to the
    /// target, so that the target never holds part of a file.
    pub atomic: bool,
    /// Remove each source once all of it is copied, which moves it.
    pub remove_sources: bool,
}

/// One side of a copy: a file system, and how its paths are reported.
pub struct Endpoint<'a, V: Vfs, L> {
    pub vfs: &'a V,
    pub report: &'a (dyn Fn(&V::Path) -> L + Send + Sync),
}

/// Names temporary files apart within this process.
static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// An entry to copy, found while counting.
struct Item<P> {
    source: P,
    /// The directory it is in, unless it is one of the sources.
    parent: Option<usize>,
    name: Vec<u8>,
    metadata: Metadata,
    /// Not copied: its directory could not be read.
    skip: bool,
}

impl<V: Vfs, L> Job<'_, '_, V, V, L> {
    async fn run_move(
        &mut self,
        sources: Vec<V::Path>,
        target: V::Path,
        count: usize,
    ) -> Result<(), Aborted> {
        let Some(destination) = self.destination(target, count).await? else {
            self.outcome.skipped = count as u64;
            return Ok(());
        };
        let mut left = Vec::new();
        for source in sources {
            if self.reporter.cancelled() {
                return Err(Aborted);
            }
            self.progress(&source);
            let target = destination.path(source.name().unwrap_or_default());
            match self.rename_one(&source, &target).await? {
                Renamed::Done => self.outcome.done += 1,
                Renamed::Skipped => self.outcome.skipped += 1,
                Renamed::Copy => left.push(source),
            }
        }
        if left.is_empty() {
            return Ok(());
        }
        let items = self.scan(left).await?;
        self.copy_items(&items, &destination).await
    }

    async fn rename_one(&mut self, source: &V::Path, target: &V::Path) -> Result<Renamed, Aborted> {
        loop {
            match self.try_rename(source, target).await {
                Ok(renamed) => return Ok(renamed),
                Err(Stop::Aborted) => return Err(Aborted),
                Err(Stop::Failed(path, error)) => match self.reporter.ask(path, error).await {
                    Decision::Retry => {}
                    Decision::Skip | Decision::SkipAll => return Ok(Renamed::Skipped),
                    Decision::Abort => return Err(Aborted),
                },
            }
        }
    }

    /// Renames `source` to `target`, asking first if the name is taken by something a rename
    /// would replace.
    async fn try_rename(&mut self, source: &V::Path, target: &V::Path) -> Result<Renamed, Stop<L>> {
        let vfs = self.to.vfs;
        let at_source = |error| Stop::Failed((self.from.report)(source), error);
        let metadata = vfs.symlink_metadata(source).await.map_err(at_source)?;
        let replace = match vfs.symlink_metadata(target).await {
            Err(VfsError::NotFound(_)) => false,
            Ok(existing) if existing.kind == FileKind::Dir && metadata.kind == FileKind::Dir => {
                return Ok(Renamed::Copy);
            }
            Ok(_) => {
                let item = Item {
                    source: source.clone(),
                    parent: None,
                    name: Vec::new(),
                    metadata,
                    skip: false,
                };
                match self.plan(&item, target).await? {
                    Plan::New => false,
                    Plan::Skip => return Ok(Renamed::Skipped),
                    Plan::Replace(_) => true,
                }
            }
            Err(error) => return Err(self.at_target(target, error)),
        };
        let error = match vfs.rename(source, target).await {
            Ok(()) => return Ok(Renamed::Done),
            Err(error) if error.is_cross_device() => return Ok(Renamed::Copy),
            // Without posix-rename the target goes first.
            Err(VfsError::AlreadyExists(_)) if replace => match vfs.remove_file(target).await {
                Ok(()) => vfs.rename(source, target).await.err(),
                Err(error) => Some(error),
            },
            Err(error) => Some(error),
        };
        match error {
            None => Ok(Renamed::Done),
            Some(error) => Err(Stop::Failed((self.from.report)(source), error)),
        }
    }
}

/// Where the sources go.
enum Destination<Q> {
    /// Into this directory, under their own names.
    Into(Q),
    /// The only source, to this name in this directory.
    As { dir: Q, name: Vec<u8> },
}

impl<Q: VfsPath> Destination<Q> {
    /// Where the source called `name` goes.
    fn path(&self, name: &[u8]) -> Q {
        match self {
            Self::Into(dir) => dir.join_name(name),
            Self::As { dir, name } => dir.join_name(name),
        }
    }
}

/// Marks the source of `items[index]` and the directories it is in as staying.
fn keep<P>(kept: &mut [bool], items: &[Item<P>], mut index: usize) {
    loop {
        kept[index] = true;
        match items[index].parent {
            Some(parent) if !kept[parent] => index = parent,
            _ => return,
        }
    }
}

/// Why one attempt at an entry did not work.
enum Stop<L> {
    Failed(L, VfsError),
    /// Cancelled, or Abort.
    Aborted,
}

/// What to do about the name of a copy.
enum Plan {
    /// It is free.
    New,
    /// Replace what is there: something of this kind.
    Replace(FileKind),
    /// Leave what is there, and the source with it.
    Skip,
}

/// What every later taken name gets, after an answer for all of them.
#[derive(Debug, Clone, Copy)]
enum Policy {
    OverwriteAll,
    SkipAll,
    OverwriteOlder,
}

/// Why the job stopped early.
struct Aborted;

/// Copies `sources` from one side to `target` on the other. If `target` is a directory, the
/// sources go into it under their own names; otherwise a single source becomes `target`, and
/// several make it a directory to go into. Directories are copied with everything in them,
/// into a directory of the same name if there is one; symlinks are copied as symlinks, and
/// other special files fail. The job counts entries and bytes first, so that progress has
/// totals. When something fails, or a name is taken, it asks `reporter` what to do; a file it
/// leaves half written is removed.
pub async fn copy<A: Vfs, B: Vfs, L>(
    from: Endpoint<'_, A, L>,
    sources: Vec<A::Path>,
    to: Endpoint<'_, B, L>,
    target: B::Path,
    options: CopyOptions,
    reporter: &mut Reporter<L>,
) -> Outcome {
    let mut job = Job {
        from,
        to,
        options,
        reporter,
        outcome: Outcome::default(),
        items_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        policy: None,
    };
    let count = sources.len();
    if job.run(sources, target, count).await.is_err() {
        job.outcome.aborted = true;
    }
    job.outcome
}

/// Moves `sources` to `target` within one file system, as [`copy`] places them: renames each
/// source where it can, asking first when its name is taken, and copies and removes it where
/// it cannot: into a directory of the same name, which merges, or across file systems.
/// Copies that remove their sources keep times and permissions, and the copy options
/// otherwise apply.
pub async fn move_within<V: Vfs, L>(
    side: Endpoint<'_, V, L>,
    sources: Vec<V::Path>,
    target: V::Path,
    options: CopyOptions,
    reporter: &mut Reporter<L>,
) -> Outcome {
    let options = CopyOptions {
        preserve: true,
        remove_sources: true,
        ..options
    };
    let mut job = Job {
        from: Endpoint {
            vfs: side.vfs,
            report: side.report,
        },
        to: side,
        options,
        reporter,
        outcome: Outcome::default(),
        items_total: sources.len() as u64,
        bytes_done: 0,
        bytes_total: 0,
        policy: None,
    };
    let count = sources.len();
    if job.run_move(sources, target, count).await.is_err() {
        job.outcome.aborted = true;
    }
    job.outcome
}

/// What a rename did.
enum Renamed {
    Done,
    Skipped,
    /// It cannot rename this one; a copy has to move it.
    Copy,
}

struct Job<'a, 'r, A: Vfs, B: Vfs, L> {
    from: Endpoint<'a, A, L>,
    to: Endpoint<'a, B, L>,
    options: CopyOptions,
    reporter: &'r mut Reporter<L>,
    outcome: Outcome,
    items_total: u64,
    bytes_done: u64,
    bytes_total: u64,
    policy: Option<Policy>,
}

impl<A: Vfs, B: Vfs, L> Job<'_, '_, A, B, L> {
    async fn run(
        &mut self,
        sources: Vec<A::Path>,
        target: B::Path,
        count: usize,
    ) -> Result<(), Aborted> {
        let Some(destination) = self.destination(target, count).await? else {
            self.outcome.skipped = count as u64;
            return Ok(());
        };
        let items = self.scan(sources).await?;
        self.copy_items(&items, &destination).await
    }

    /// Copies `items`, as counted, to `destination`; when moving, removes each source once
    /// all of it is copied.
    async fn copy_items(
        &mut self,
        items: &[Item<A::Path>],
        destination: &Destination<B::Path>,
    ) -> Result<(), Aborted> {
        self.items_total = self.outcome.done + self.outcome.skipped + items.len() as u64;
        let mut targets: Vec<Option<B::Path>> = Vec::with_capacity(items.len());
        // Sources that stay: not copied, or holding something that was not.
        let mut kept = vec![false; items.len()];
        let mut made_dirs = Vec::new();
        for (index, item) in items.iter().enumerate() {
            if self.reporter.cancelled() {
                return Err(Aborted);
            }
            let target = match item.parent {
                Some(parent) => targets[parent]
                    .as_ref()
                    .map(|dir| dir.join_name(&item.name)),
                None => Some(destination.path(&item.name)),
            };
            let size = file_size(item);
            let Some(target) = target.filter(|_| !item.skip) else {
                self.outcome.skipped += 1;
                self.bytes_done += size;
                keep(&mut kept, items, index);
                targets.push(None);
                continue;
            };
            self.progress(&item.source);
            let before = self.bytes_done;
            let copied = loop {
                let attempt = match item.metadata.kind {
                    FileKind::Dir => self.copy_dir(&target).await.map(|made| {
                        if made {
                            made_dirs.push((target.clone(), item.metadata.clone()));
                        }
                        true
                    }),
                    FileKind::Symlink => self.copy_link(item, &target).await,
                    FileKind::File | FileKind::Unknown => self.copy_file(item, &target).await,
                    FileKind::Fifo
                    | FileKind::Socket
                    | FileKind::BlockDevice
                    | FileKind::CharDevice => Err(Stop::Failed(
                        (self.from.report)(&item.source),
                        VfsError::Io(io::ErrorKind::Unsupported.into()),
                    )),
                };
                match attempt {
                    Ok(copied) => break copied,
                    Err(Stop::Aborted) => return Err(Aborted),
                    Err(Stop::Failed(path, error)) => {
                        self.bytes_done = before;
                        match self.reporter.ask(path, error).await {
                            Decision::Retry => {}
                            Decision::Skip | Decision::SkipAll => break false,
                            Decision::Abort => return Err(Aborted),
                        }
                    }
                }
            };
            if copied {
                self.outcome.done += 1;
                let dir = item.metadata.kind == FileKind::Dir;
                if self.options.remove_sources && !dir && !self.remove_source(item).await? {
                    keep(&mut kept, items, index);
                }
                // Only directories have entries to go into them.
                targets.push(dir.then_some(target));
            } else {
                self.outcome.skipped += 1;
                self.bytes_done = before + size;
                keep(&mut kept, items, index);
                targets.push(None);
            }
        }
        // Writing into a directory changes its time, so directories get theirs last.
        if self.options.preserve {
            for (dir, metadata) in made_dirs.iter().rev() {
                self.attributes(dir, metadata).await?;
            }
        }
        if self.options.remove_sources {
            // The deepest first, so that each is empty by its turn.
            for index in (0..items.len()).rev() {
                let item = &items[index];
                if item.metadata.kind == FileKind::Dir
                    && !kept[index]
                    && !self.remove_source(item).await?
                {
                    keep(&mut kept, items, index);
                }
            }
        }
        Ok(())
    }

    /// Removes the source of `item`, which is copied; `false` if it stays.
    async fn remove_source(&mut self, item: &Item<A::Path>) -> Result<bool, Aborted> {
        let from = self.from.vfs;
        loop {
            let result = if item.metadata.kind == FileKind::Dir {
                from.remove_dir(&item.source).await
            } else {
                from.remove_file(&item.source).await
            };
            let error = match result {
                Ok(()) | Err(VfsError::NotFound(_)) => return Ok(true),
                Err(error) => error,
            };
            match self
                .reporter
                .ask((self.from.report)(&item.source), error)
                .await
            {
                Decision::Retry => {}
                Decision::Skip | Decision::SkipAll => return Ok(false),
                Decision::Abort => return Err(Aborted),
            }
        }
    }

    /// Where the sources go; `None` if the target is skipped.
    async fn destination(
        &mut self,
        target: B::Path,
        count: usize,
    ) -> Result<Option<Destination<B::Path>>, Aborted> {
        let to = self.to.vfs;
        loop {
            let error = match to.metadata(&target).await {
                Ok(metadata) if metadata.kind == FileKind::Dir => {
                    return Ok(Some(Destination::Into(target)));
                }
                Ok(_) | Err(VfsError::NotFound(_)) if count == 1 => {
                    return Ok(Some(match (target.parent(), target.name()) {
                        (Some(dir), Some(name)) => Destination::As {
                            name: name.to_vec(),
                            dir,
                        },
                        _ => Destination::Into(target),
                    }));
                }
                Err(VfsError::NotFound(_)) => match to.create_dir(&target).await {
                    Ok(()) => return Ok(Some(Destination::Into(target))),
                    Err(error) => error,
                },
                Ok(_) => VfsError::AlreadyExists(target.display()),
                Err(error) => error,
            };
            match self.reporter.ask((self.to.report)(&target), error).await {
                Decision::Retry => {}
                Decision::Skip | Decision::SkipAll => return Ok(None),
                Decision::Abort => return Err(Aborted),
            }
        }
    }

    /// Lists the trees under `sources`, parents before what they hold, and counts their
    /// bytes. Listings report symlinks without following them.
    async fn scan(&mut self, sources: Vec<A::Path>) -> Result<Vec<Item<A::Path>>, Aborted> {
        let from = self.from.vfs;
        let mut items: Vec<Item<A::Path>> = Vec::new();
        let mut dirs = Vec::new();
        for source in sources {
            let metadata = loop {
                if self.reporter.cancelled() {
                    return Err(Aborted);
                }
                match from.symlink_metadata(&source).await {
                    Ok(metadata) => break Some(metadata),
                    Err(error) => match self.reporter.ask((self.from.report)(&source), error).await
                    {
                        Decision::Retry => {}
                        Decision::Skip | Decision::SkipAll => break None,
                        Decision::Abort => return Err(Aborted),
                    },
                }
            };
            let Some(metadata) = metadata else {
                self.outcome.skipped += 1;
                continue;
            };
            let name = source.name().unwrap_or_default().to_vec();
            self.add(&mut items, &mut dirs, source, None, name, metadata);
            while let Some(index) = dirs.pop() {
                let entries = loop {
                    if self.reporter.cancelled() {
                        return Err(Aborted);
                    }
                    let dir: &Item<A::Path> = &items[index];
                    match from.list_dir(&dir.source).await {
                        Ok(entries) => break entries,
                        Err(error) => {
                            let path = (self.from.report)(&dir.source);
                            match self.reporter.ask(path, error).await {
                                Decision::Retry => {}
                                Decision::Skip | Decision::SkipAll => {
                                    items[index].skip = true;
                                    break Vec::new();
                                }
                                Decision::Abort => return Err(Aborted),
                            }
                        }
                    }
                };
                for entry in entries {
                    let source = items[index].source.join_name(&entry.name);
                    let (name, metadata) = (entry.name, entry.metadata);
                    self.add(&mut items, &mut dirs, source, Some(index), name, metadata);
                }
                self.reporter.report(Event::Scanning {
                    items: items.len() as u64,
                });
            }
        }
        Ok(items)
    }

    fn add(
        &mut self,
        items: &mut Vec<Item<A::Path>>,
        dirs: &mut Vec<usize>,
        source: A::Path,
        parent: Option<usize>,
        name: Vec<u8>,
        metadata: Metadata,
    ) {
        if metadata.kind == FileKind::Dir {
            dirs.push(items.len());
        }
        let item = Item {
            source,
            parent,
            name,
            metadata,
            skip: false,
        };
        self.bytes_total += file_size(&item);
        items.push(item);
    }

    fn progress(&self, current: &A::Path) {
        self.reporter.report(Event::Progress(Progress {
            current: (self.from.report)(current),
            items_done: self.outcome.done + self.outcome.skipped,
            items_total: self.items_total,
            bytes_done: self.bytes_done,
            bytes_total: self.bytes_total,
        }));
    }

    fn at_target(&self, path: &B::Path, error: VfsError) -> Stop<L> {
        Stop::Failed((self.to.report)(path), error)
    }

    /// Makes the directory `target`, or goes into one that is there; `true` if it made it.
    async fn copy_dir(&self, target: &B::Path) -> Result<bool, Stop<L>> {
        let to = self.to.vfs;
        match to.create_dir(target).await {
            Ok(()) => Ok(true),
            Err(VfsError::AlreadyExists(name)) => match to.metadata(target).await {
                Ok(metadata) if metadata.kind == FileKind::Dir => Ok(false),
                _ => Err(self.at_target(target, VfsError::AlreadyExists(name))),
            },
            Err(error) => Err(self.at_target(target, error)),
        }
    }

    /// What to do about the name `target` of a copy of `item`: free, or taken by something
    /// that the answer to a question, or an earlier answer for all, says to replace or keep.
    /// A directory there is a failure: a file does not replace one.
    async fn plan(&mut self, item: &Item<A::Path>, target: &B::Path) -> Result<Plan, Stop<L>> {
        let existing = match self.to.vfs.symlink_metadata(target).await {
            Ok(existing) => existing,
            Err(VfsError::NotFound(_)) => return Ok(Plan::New),
            Err(error) => return Err(self.at_target(target, error)),
        };
        if existing.kind == FileKind::Dir {
            let error = VfsError::AlreadyExists(target.display());
            return Err(self.at_target(target, error));
        }
        let policy = if let Some(policy) = self.policy {
            policy
        } else {
            let source = (self.from.report)(&item.source);
            let target = (self.to.report)(target);
            let (source_metadata, target_metadata) = (item.metadata.clone(), existing.clone());
            let answer = self
                .reporter
                .question(|reply| Event::Exists {
                    source,
                    target,
                    source_metadata,
                    target_metadata,
                    reply,
                })
                .await;
            match answer.unwrap_or(Conflict::Abort) {
                Conflict::Overwrite => return Ok(Plan::Replace(existing.kind)),
                Conflict::Skip => return Ok(Plan::Skip),
                Conflict::Abort => return Err(Stop::Aborted),
                Conflict::OverwriteAll => *self.policy.insert(Policy::OverwriteAll),
                Conflict::SkipAll => *self.policy.insert(Policy::SkipAll),
                Conflict::OverwriteOlder => *self.policy.insert(Policy::OverwriteOlder),
            }
        };
        let replace = match policy {
            Policy::OverwriteAll => true,
            Policy::SkipAll => false,
            // Unknown times keep the target.
            Policy::OverwriteOlder => matches!(
                (existing.modified, item.metadata.modified),
                (Some(there), Some(source)) if there < source
            ),
        };
        Ok(if replace {
            Plan::Replace(existing.kind)
        } else {
            Plan::Skip
        })
    }

    /// Copies a symlink, as a symlink; `false` if the name is taken and stays.
    async fn copy_link(&mut self, item: &Item<A::Path>, target: &B::Path) -> Result<bool, Stop<L>> {
        match self.plan(item, target).await? {
            Plan::New => {}
            Plan::Skip => return Ok(false),
            Plan::Replace(_) => self
                .to
                .vfs
                .remove_file(target)
                .await
                .map_err(|error| self.at_target(target, error))?,
        }
        let link = self
            .from
            .vfs
            .read_link(&item.source)
            .await
            .map_err(|error| Stop::Failed((self.from.report)(&item.source), error))?;
        self.to
            .vfs
            .create_symlink(&link, target)
            .await
            .map_err(|error| self.at_target(target, error))?;
        Ok(true)
    }

    /// Copies a file to `target`, through a temporary name if the copy is atomic; `false` if
    /// the name is taken and stays. What it wrote goes again if it does not finish.
    async fn copy_file(&mut self, item: &Item<A::Path>, target: &B::Path) -> Result<bool, Stop<L>> {
        let to = self.to.vfs;
        let replace = match self.plan(item, target).await? {
            Plan::New => false,
            Plan::Skip => return Ok(false),
            // Written directly, a file would go through a symlink to whatever it points to.
            Plan::Replace(FileKind::Symlink) if !self.options.atomic => {
                to.remove_file(target)
                    .await
                    .map_err(|error| self.at_target(target, error))?;
                false
            }
            Plan::Replace(_) => true,
        };
        let mut reader = self
            .from
            .vfs
            .open_file(&item.source)
            .await
            .map_err(|error| Stop::Failed((self.from.report)(&item.source), error))?;
        let (writer, written) = self.create(target, replace).await?;
        let result = self.transfer(item, &mut reader, writer, &written).await;
        let result = match result {
            Ok(()) if self.options.atomic => self.rename_over(&written, target, replace).await,
            other => other,
        };
        if result.is_err() {
            // Best effort: what is left of it is no use, and an error here would hide the first.
            let _ = to.remove_file(&written).await;
        }
        result.map(|()| true)
    }

    /// Gives the file written at `written` its name `target`, replacing what is there if
    /// `replace`. Servers without `posix-rename` refuse to rename over a file, so then the
    /// target goes first.
    async fn rename_over(
        &self,
        written: &B::Path,
        target: &B::Path,
        replace: bool,
    ) -> Result<(), Stop<L>> {
        let to = self.to.vfs;
        let error = match to.rename(written, target).await {
            Ok(()) => return Ok(()),
            Err(VfsError::AlreadyExists(_)) if replace => to.remove_file(target).await.err(),
            Err(error) => Some(error),
        };
        if let Some(error) = error {
            return Err(self.at_target(target, error));
        }
        to.rename(written, target)
            .await
            .map_err(|error| self.at_target(target, error))
    }

    /// Opens the file to write for `target`: a temporary name next to it if the copy is atomic,
    /// or the target itself, emptied if `replace`.
    async fn create(
        &self,
        target: &B::Path,
        replace: bool,
    ) -> Result<(B::Writer, B::Path), Stop<L>> {
        let to = self.to.vfs;
        if !self.options.atomic {
            return match to.create_file(target, replace).await {
                Ok(writer) => Ok((writer, target.clone())),
                Err(error) => Err(self.at_target(target, error)),
            };
        }
        let (Some(dir), Some(name)) = (target.parent(), target.name()) else {
            let error = VfsError::Io(io::ErrorKind::InvalidInput.into());
            return Err(self.at_target(target, error));
        };
        loop {
            let temporary = dir.join_name(&temporary_name(name));
            match to.create_file(&temporary, false).await {
                Ok(writer) => return Ok((writer, temporary)),
                Err(VfsError::AlreadyExists(_)) => {}
                Err(error) => return Err(self.at_target(target, error)),
            }
        }
    }

    /// Moves the data of `item` from `reader` to `writer`, which writes `written`, and gives
    /// it the attributes of `item` if the copy preserves them.
    async fn transfer(
        &mut self,
        item: &Item<A::Path>,
        reader: &mut A::Reader,
        mut writer: B::Writer,
        written: &B::Path,
    ) -> Result<(), Stop<L>> {
        loop {
            if self.reporter.cancelled() {
                return Err(Stop::Aborted);
            }
            let chunk = reader
                .read()
                .await
                .map_err(|error| Stop::Failed((self.from.report)(&item.source), error))?;
            let Some(chunk) = chunk else { break };
            self.bytes_done += chunk.len() as u64;
            writer
                .write(chunk)
                .await
                .map_err(|error| self.at_target(written, error))?;
            self.progress(&item.source);
        }
        writer
            .finish()
            .await
            .map_err(|error| self.at_target(written, error))?;
        if self.options.preserve {
            self.set_attributes(written, &item.metadata)
                .await
                .map_err(|error| self.at_target(written, error))?;
        }
        Ok(())
    }

    async fn set_attributes(&self, path: &B::Path, metadata: &Metadata) -> Result<(), VfsError> {
        if let Some(mode) = metadata.permissions {
            self.to.vfs.set_permissions(path, mode).await?;
        }
        if let Some(modified) = metadata.modified {
            self.to.vfs.set_modified(path, modified).await?;
        }
        Ok(())
    }

    /// Gives a directory made by the copy the attributes of its source.
    async fn attributes(&mut self, dir: &B::Path, metadata: &Metadata) -> Result<(), Aborted> {
        loop {
            let Err(error) = self.set_attributes(dir, metadata).await else {
                return Ok(());
            };
            match self.reporter.ask((self.to.report)(dir), error).await {
                Decision::Retry => {}
                Decision::Skip | Decision::SkipAll => return Ok(()),
                Decision::Abort => return Err(Aborted),
            }
        }
    }
}

/// The bytes an entry adds to the total: those of files.
fn file_size<P>(item: &Item<P>) -> u64 {
    match item.metadata.kind {
        FileKind::File | FileKind::Unknown => item.metadata.size.unwrap_or(0),
        _ => 0,
    }
}

/// A hidden name for writing a copy of `name` before it gets its real one.
fn temporary_name(name: &[u8]) -> Vec<u8> {
    let number = TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let suffix = format!(".sftp-tui-{}-{number}", std::process::id());
    // Long names would leave no room for the suffix in a file system's limit of 255 bytes.
    let mut temporary = b".".to_vec();
    if name.len() + suffix.len() < 250 {
        temporary.extend_from_slice(name);
    }
    temporary.extend_from_slice(suffix.as_bytes());
    temporary
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use sftp_tui_vfs::{LocalFs, RemotePath, VfsPath};
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::testing;

    fn report<P: VfsPath>(path: &P) -> String {
        path.display()
    }

    /// What a copy reported.
    #[derive(Debug, Default)]
    struct Seen {
        failed: Vec<String>,
        bytes: Vec<(u64, u64)>,
        /// Each taken name: the target, and the sizes of the source and the target.
        exists: Vec<(String, Option<u64>, Option<u64>)>,
    }

    /// The answers a copy gets, in turn, and whether it is cancelled once data has moved.
    #[derive(Default)]
    struct Script<'a> {
        failures: &'a [Decision],
        conflicts: &'a [Conflict],
        cancel_midway: bool,
    }

    /// Copies with `answers` for failures; cancels once data has moved if `cancel_midway`.
    async fn run<A: Vfs, B: Vfs>(
        from: (&A, Vec<A::Path>),
        to: (&B, B::Path),
        options: CopyOptions,
        answers: &[Decision],
        cancel_midway: bool,
    ) -> (Outcome, Seen) {
        let script = Script {
            failures: answers,
            conflicts: &[],
            cancel_midway,
        };
        run_script(from, to, options, script).await
    }

    async fn run_script<A: Vfs, B: Vfs>(
        (from, sources): (&A, Vec<A::Path>),
        (to, target): (&B, B::Path),
        options: CopyOptions,
        script: Script<'_>,
    ) -> (Outcome, Seen) {
        let (events, incoming) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let mut reporter = Reporter::new(events, cancel.clone());
        let job = async {
            let from = Endpoint {
                vfs: from,
                report: &report::<A::Path>,
            };
            let to = Endpoint {
                vfs: to,
                report: &report::<B::Path>,
            };
            let outcome = copy(from, sources, to, target, options, &mut reporter).await;
            drop(reporter);
            outcome
        };
        tokio::join!(job, listen(incoming, &cancel, script))
    }

    /// Moves within `vfs`, with the answers of `script`.
    async fn run_move<V: Vfs>(
        vfs: &V,
        sources: Vec<V::Path>,
        target: V::Path,
        script: Script<'_>,
    ) -> (Outcome, Seen) {
        let (events, incoming) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let mut reporter = Reporter::new(events, cancel.clone());
        let job = async {
            let side = Endpoint {
                vfs,
                report: &report::<V::Path>,
            };
            let outcome = move_within(side, sources, target, BOTH, &mut reporter).await;
            drop(reporter);
            outcome
        };
        tokio::join!(job, listen(incoming, &cancel, script))
    }

    /// Takes the reports of a job and answers its questions from `script`.
    async fn listen(
        mut incoming: mpsc::UnboundedReceiver<Event<String>>,
        cancel: &CancellationToken,
        script: Script<'_>,
    ) -> Seen {
        let mut seen = Seen::default();
        let mut failures = script.failures.iter();
        let mut conflicts = script.conflicts.iter();
        while let Some(event) = incoming.recv().await {
            match event {
                Event::Scanning { .. } => {}
                Event::Progress(progress) => {
                    seen.bytes.push((progress.bytes_done, progress.bytes_total));
                    if script.cancel_midway && progress.bytes_done > 0 {
                        cancel.cancel();
                    }
                }
                Event::Failed { path, reply, .. } => {
                    seen.failed.push(path);
                    let answer = *failures.next().expect("an answer for each failure");
                    let _ = reply.send(answer);
                }
                Event::Exists {
                    target,
                    source_metadata,
                    target_metadata,
                    reply,
                    ..
                } => {
                    let sizes = (source_metadata.size, target_metadata.size);
                    seen.exists.push((target, sizes.0, sizes.1));
                    let answer = *conflicts.next().expect("an answer for each taken name");
                    let _ = reply.send(answer);
                }
            }
        }
        seen
    }

    const BOTH: CopyOptions = CopyOptions {
        preserve: true,
        atomic: true,
        remove_sources: false,
    };

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn set_modified(path: &Path, time: SystemTime) {
        let times = fs::FileTimes::new().set_modified(time);
        fs::File::open(path).unwrap().set_times(times).unwrap();
    }

    fn modified(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    /// `src/dir` with `a`, `sub/b`, a link to `a`, and a link to nothing; and `src/file`. Files
    /// and directories have times in whole seconds, which SFTP keeps.
    fn sources(root: &Path) -> PathBuf {
        let src = root.join("src");
        let dir = src.join("dir");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a"), "0123456789").unwrap();
        fs::write(dir.join("sub/b"), vec![7; 100_000]).unwrap();
        fs::write(src.join("file"), "f").unwrap();
        symlink("a", dir.join("link")).unwrap();
        symlink("missing", dir.join("dangling")).unwrap();
        fs::set_permissions(dir.join("a"), fs::Permissions::from_mode(0o640)).unwrap();
        fs::set_permissions(dir.join("sub"), fs::Permissions::from_mode(0o751)).unwrap();
        for (name, seconds) in [("dir/a", 1_000), ("dir/sub/b", 2_000), ("file", 3_000)] {
            set_modified(&src.join(name), at(seconds));
        }
        set_modified(&dir.join("sub"), at(4_000));
        set_modified(&dir, at(5_000));
        src
    }

    /// Checks a copy of `src/dir` at `dir`.
    fn check_dir(dir: &Path) {
        assert_eq!(fs::read_to_string(dir.join("a")).unwrap(), "0123456789");
        assert_eq!(fs::read(dir.join("sub/b")).unwrap(), vec![7; 100_000]);
        assert_eq!(fs::read_link(dir.join("link")).unwrap(), Path::new("a"));
        assert_eq!(
            fs::read_link(dir.join("dangling")).unwrap(),
            Path::new("missing")
        );
        assert_eq!(modified(&dir.join("a")), at(1_000));
        assert_eq!(modified(&dir.join("sub/b")), at(2_000));
        assert_eq!(modified(&dir.join("sub")), at(4_000));
        assert_eq!(modified(dir), at(5_000), "after what went into it");
        assert_eq!(mode(&dir.join("a")), 0o640);
        assert_eq!(mode(&dir.join("sub")), 0o751);
    }

    /// Names in the tree under `dir` that look temporary.
    fn leftovers(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().contains(".sftp-tui-") {
                found.push(path.clone());
            }
            if path.is_dir() && !path.is_symlink() {
                found.extend(leftovers(&path));
            }
        }
        found
    }

    #[tokio::test]
    async fn copies_trees_into_a_directory_with_links_and_attributes() {
        let root = tempfile::tempdir().unwrap();
        let src = sources(root.path());
        let dst = root.path().join("dst");
        fs::create_dir(&dst).unwrap();
        let sources = vec![src.join("dir"), src.join("file")];
        let (outcome, seen) = run(
            (&LocalFs, sources),
            (&LocalFs, dst.clone()),
            BOTH,
            &[],
            false,
        )
        .await;
        assert!(seen.failed.is_empty(), "{seen:?}");
        // dir, a, sub, b, link, dangling, and file.
        assert_eq!(
            outcome,
            Outcome {
                done: 7,
                skipped: 0,
                aborted: false
            }
        );
        check_dir(&dst.join("dir"));
        assert_eq!(modified(&dst.join("file")), at(3_000));
        assert_eq!(leftovers(&dst), Vec::<PathBuf>::new());
        let total = 10 + 100_000 + 1;
        assert!(
            seen.bytes.iter().all(|(_, of)| *of == total),
            "{:?}",
            seen.bytes
        );
        assert_eq!(seen.bytes.iter().map(|(done, _)| *done).max(), Some(total));
    }

    #[tokio::test]
    async fn one_source_takes_a_new_name_and_several_make_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let src = sources(root.path());
        let renamed = root.path().join("renamed");
        let plain = CopyOptions {
            preserve: false,
            atomic: false,
            remove_sources: false,
        };
        let one = vec![src.join("dir")];
        run(
            (&LocalFs, one),
            (&LocalFs, renamed.clone()),
            BOTH,
            &[],
            false,
        )
        .await;
        check_dir(&renamed);

        let new = root.path().join("new");
        let several = vec![src.join("dir/a"), src.join("file")];
        let (outcome, _) = run(
            (&LocalFs, several),
            (&LocalFs, new.clone()),
            plain,
            &[],
            false,
        )
        .await;
        assert_eq!(outcome.done, 2);
        assert_eq!(fs::read_to_string(new.join("file")).unwrap(), "f");
        assert_ne!(modified(&new.join("a")), at(1_000), "not preserved");
    }

    /// `dst/dir` with `a` and `sub/b` already there, older and newer than their sources.
    fn taken(root: &Path) -> (PathBuf, PathBuf) {
        let src = sources(root);
        let dst = root.join("dst");
        fs::create_dir_all(dst.join("dir/sub")).unwrap();
        fs::write(dst.join("dir/a"), "older").unwrap();
        fs::write(dst.join("dir/sub/b"), "newer").unwrap();
        set_modified(&dst.join("dir/a"), at(500));
        set_modified(&dst.join("dir/sub/b"), at(9_000));
        (src, dst)
    }

    async fn copy_taken(root: &Path, conflicts: &[Conflict]) -> (Outcome, Seen, PathBuf) {
        let (src, dst) = taken(root);
        let script = Script {
            conflicts,
            ..Script::default()
        };
        let sources = vec![src.join("dir")];
        let (outcome, seen) =
            run_script((&LocalFs, sources), (&LocalFs, dst.clone()), BOTH, script).await;
        (outcome, seen, dst.join("dir"))
    }

    #[tokio::test]
    async fn a_taken_name_asks_and_directories_merge() {
        let root = tempfile::tempdir().unwrap();
        let answers = [Conflict::Overwrite, Conflict::Skip];
        let (outcome, seen, dir) = copy_taken(root.path(), &answers).await;
        let a = dir.join("a").to_string_lossy().into_owned();
        assert_eq!(seen.exists[0], (a, Some(10), Some(5)), "with both sizes");
        assert_eq!(seen.exists.len(), 2);
        assert_eq!(fs::read_to_string(dir.join("a")).unwrap(), "0123456789");
        assert_eq!(fs::read_to_string(dir.join("sub/b")).unwrap(), "newer");
        // dir and sub (merged), a, link, and dangling; b stays.
        assert_eq!((outcome.done, outcome.skipped), (5, 1));
        assert_eq!(leftovers(&dir), Vec::<PathBuf>::new());
    }

    #[tokio::test]
    async fn answers_for_all_ask_once() {
        let root = tempfile::tempdir().unwrap();
        let (_, seen, dir) = copy_taken(root.path(), &[Conflict::OverwriteAll]).await;
        assert_eq!(seen.exists.len(), 1);
        assert_eq!(
            fs::read_to_string(dir.join("sub/b")).unwrap().len(),
            100_000
        );

        let root = tempfile::tempdir().unwrap();
        let (_, seen, dir) = copy_taken(root.path(), &[Conflict::SkipAll]).await;
        assert_eq!(seen.exists.len(), 1);
        assert_eq!(fs::read_to_string(dir.join("a")).unwrap(), "older");
        assert_eq!(fs::read_to_string(dir.join("sub/b")).unwrap(), "newer");

        let root = tempfile::tempdir().unwrap();
        let (_, seen, dir) = copy_taken(root.path(), &[Conflict::OverwriteOlder]).await;
        assert_eq!(seen.exists.len(), 1);
        assert_eq!(
            fs::read_to_string(dir.join("a")).unwrap(),
            "0123456789",
            "older"
        );
        assert_eq!(
            fs::read_to_string(dir.join("sub/b")).unwrap(),
            "newer",
            "newer"
        );

        let root = tempfile::tempdir().unwrap();
        let (outcome, _, dir) = copy_taken(root.path(), &[Conflict::Abort]).await;
        assert!(outcome.aborted);
        assert!(!dir.join("link").exists(), "stopped there");
    }

    #[tokio::test]
    async fn a_file_does_not_replace_a_directory_or_go_through_a_link() {
        for atomic in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let src = sources(root.path());
            let dst = root.path().join("dst");
            fs::create_dir_all(dst.join("dir/a")).unwrap();
            fs::write(root.path().join("elsewhere"), "keep").unwrap();
            fs::create_dir_all(dst.join("dir/sub")).unwrap();
            symlink(root.path().join("elsewhere"), dst.join("dir/sub/b")).unwrap();
            let options = CopyOptions {
                preserve: false,
                atomic,
                remove_sources: false,
            };
            let script = Script {
                failures: &[Decision::Skip],
                conflicts: &[Conflict::Overwrite],
                cancel_midway: false,
            };
            let sources = vec![src.join("dir")];
            let (_, seen) = run_script(
                (&LocalFs, sources),
                (&LocalFs, dst.clone()),
                options,
                script,
            )
            .await;
            assert_eq!(seen.failed.len(), 1, "a directory is in the way: {seen:?}");
            assert!(dst.join("dir/a").is_dir());
            let b = dst.join("dir/sub/b");
            assert!(!b.is_symlink(), "atomic: {atomic}");
            assert_eq!(fs::read(&b).unwrap().len(), 100_000);
            let elsewhere = fs::read_to_string(root.path().join("elsewhere")).unwrap();
            assert_eq!(elsewhere, "keep", "atomic: {atomic}");
        }
    }

    #[tokio::test]
    async fn a_cancelled_copy_leaves_no_part_of_a_file() {
        for atomic in [true, false] {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join("big"), vec![1; 8 * 1024 * 1024]).unwrap();
            let dst = root.path().join("dst");
            fs::create_dir(&dst).unwrap();
            let options = CopyOptions {
                preserve: true,
                atomic,
                remove_sources: false,
            };
            let sources = vec![root.path().join("big")];
            let (outcome, _) = run(
                (&LocalFs, sources),
                (&LocalFs, dst.clone()),
                options,
                &[],
                true,
            )
            .await;
            assert!(outcome.aborted, "atomic: {atomic}");
            let left: Vec<_> = fs::read_dir(&dst).unwrap().collect();
            assert!(left.is_empty(), "atomic: {atomic}: {left:?}");
        }
    }

    #[tokio::test]
    async fn unreadable_and_special_files_fail_alone() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("locked"), "x").unwrap();
        fs::set_permissions(src.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(src.join("locked")).is_ok() {
            // Root reads it anyway.
            return;
        }
        let status = std::process::Command::new("mkfifo")
            .arg(src.join("fifo"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(src.join("ok"), "ok").unwrap();
        let dst = root.path().join("dst");
        let answers = [Decision::Skip, Decision::Skip];
        let (outcome, seen) = run(
            (&LocalFs, vec![src.clone()]),
            (&LocalFs, dst.clone()),
            BOTH,
            &answers,
            false,
        )
        .await;
        assert_eq!(seen.failed.len(), 2, "{seen:?}");
        assert_eq!(fs::read_to_string(dst.join("ok")).unwrap(), "ok");
        assert!(!dst.join("locked").exists());
        assert!(!dst.join("fifo").exists());
        assert_eq!((outcome.done, outcome.skipped), (2, 2));
        assert_eq!(leftovers(&dst), Vec::<PathBuf>::new());
    }

    #[tokio::test]
    async fn copies_to_a_host_and_back() {
        let root = tempfile::tempdir().unwrap();
        let src = sources(root.path());
        let remote = root.path().join("remote");
        fs::create_dir(&remote).unwrap();
        let Some((_server, sftp)) = testing::sftp_server(&remote).await else {
            return;
        };
        let up = vec![src.join("dir"), src.join("file")];
        let (outcome, seen) = run(
            (&LocalFs, up),
            (&sftp, RemotePath::from("up")),
            BOTH,
            &[],
            false,
        )
        .await;
        assert!(seen.failed.is_empty(), "{seen:?}");
        assert_eq!(outcome.done, 7);
        check_dir(&remote.join("up/dir"));
        assert_eq!(leftovers(&remote), Vec::<PathBuf>::new());

        let back = root.path().join("back");
        let down = vec![RemotePath::from("up/dir")];
        let (outcome, seen) = run((&sftp, down), (&LocalFs, back.clone()), BOTH, &[], false).await;
        assert!(seen.failed.is_empty(), "{seen:?}");
        assert_eq!(outcome.done, 6);
        check_dir(&back);

        // Again, over what is there: renamed over it with posix-rename.
        let again = vec![src.join("dir")];
        let script = Script {
            conflicts: &[Conflict::OverwriteAll],
            ..Script::default()
        };
        let target = RemotePath::from("up");
        let (_, replaced) = run_script((&LocalFs, again), (&sftp, target), BOTH, script).await;
        assert!(replaced.failed.is_empty(), "{replaced:?}");
        assert_eq!(replaced.exists.len(), 1);
        // Directories that were there keep their attributes; what went into them changed them.
        let dir = remote.join("up/dir");
        assert_eq!(fs::read(dir.join("sub/b")).unwrap(), vec![7; 100_000]);
        assert_eq!(modified(&dir.join("a")), at(1_000));
        assert_eq!(fs::read_link(dir.join("link")).unwrap(), Path::new("a"));
        assert_eq!(leftovers(&remote), Vec::<PathBuf>::new());
        sftp.close().await.unwrap();
    }

    #[tokio::test]
    async fn a_copy_that_removes_its_sources_keeps_what_it_skips() {
        let root = tempfile::tempdir().unwrap();
        let (src, dst) = taken(root.path());
        let options = CopyOptions {
            remove_sources: true,
            ..BOTH
        };
        let script = Script {
            conflicts: &[Conflict::Skip, Conflict::Overwrite],
            ..Script::default()
        };
        let sources = vec![src.join("dir"), src.join("file")];
        let (outcome, seen) = run_script(
            (&LocalFs, sources),
            (&LocalFs, dst.clone()),
            options,
            script,
        )
        .await;
        assert!(seen.failed.is_empty(), "{seen:?}");
        assert!(!outcome.aborted);
        // `a` stays, and with it `dir`; the rest has moved.
        assert_eq!(fs::read_to_string(src.join("dir/a")).unwrap(), "0123456789");
        assert!(!src.join("dir/sub").exists(), "all of it moved");
        assert!(!src.join("dir/link").exists());
        assert!(!src.join("file").exists());
        assert_eq!(fs::read_to_string(dst.join("file")).unwrap(), "f");
        assert_eq!(fs::read(dst.join("dir/sub/b")).unwrap(), vec![7; 100_000]);
    }

    #[tokio::test]
    async fn moving_renames_within_a_file_system() {
        let root = tempfile::tempdir().unwrap();
        let src = sources(root.path());
        let dst = root.path().join("dst");
        fs::create_dir(&dst).unwrap();
        let moving = vec![src.join("dir"), src.join("file")];
        let (outcome, seen) = run_move(&LocalFs, moving, dst.clone(), Script::default()).await;
        assert!(seen.failed.is_empty() && seen.exists.is_empty(), "{seen:?}");
        assert_eq!((outcome.done, outcome.skipped), (2, 0), "two renames");
        check_dir(&dst.join("dir"));
        assert!(!src.join("dir").exists());

        // One source to a new name: a rename in place.
        let renamed = root.path().join("renamed");
        let (outcome, _) = run_move(
            &LocalFs,
            vec![dst.join("file")],
            renamed.clone(),
            Script::default(),
        )
        .await;
        assert_eq!(outcome.done, 1);
        assert_eq!(fs::read_to_string(&renamed).unwrap(), "f");
    }

    #[tokio::test]
    async fn moving_asks_about_taken_names_and_merges_directories() {
        let root = tempfile::tempdir().unwrap();
        let (src, dst) = taken(root.path());
        fs::write(dst.join("file"), "there").unwrap();
        // `file` is taken: Overwrite. `dir` merges by copying: `a` is skipped, `b` replaced.
        let script = Script {
            conflicts: &[Conflict::Overwrite, Conflict::Skip, Conflict::Overwrite],
            ..Script::default()
        };
        let moving = vec![src.join("file"), src.join("dir")];
        let (outcome, seen) = run_move(&LocalFs, moving, dst.clone(), script).await;
        assert!(seen.failed.is_empty(), "{seen:?}");
        assert_eq!(seen.exists.len(), 3);
        assert!(!outcome.aborted);
        assert_eq!(fs::read_to_string(dst.join("file")).unwrap(), "f");
        assert!(!src.join("file").exists());
        assert_eq!(fs::read_to_string(dst.join("dir/a")).unwrap(), "older");
        assert_eq!(
            fs::read_to_string(src.join("dir/a")).unwrap(),
            "0123456789",
            "stays"
        );
        assert_eq!(fs::read(dst.join("dir/sub/b")).unwrap(), vec![7; 100_000]);
        assert!(!src.join("dir/sub").exists());
    }

    /// The local file system, where every rename crosses file systems.
    struct OtherDevice;

    impl Vfs for OtherDevice {
        type Path = PathBuf;
        type Reader = sftp_tui_vfs::LocalReader;
        type Writer = sftp_tui_vfs::LocalWriter;

        async fn list_dir(&self, path: &PathBuf) -> Result<Vec<sftp_tui_vfs::DirEntry>, VfsError> {
            LocalFs.list_dir(path).await
        }
        async fn metadata(&self, path: &PathBuf) -> Result<Metadata, VfsError> {
            LocalFs.metadata(path).await
        }
        async fn symlink_metadata(&self, path: &PathBuf) -> Result<Metadata, VfsError> {
            LocalFs.symlink_metadata(path).await
        }
        async fn canonicalize(&self, path: &PathBuf) -> Result<PathBuf, VfsError> {
            LocalFs.canonicalize(path).await
        }
        async fn create_dir(&self, path: &PathBuf) -> Result<(), VfsError> {
            LocalFs.create_dir(path).await
        }
        async fn remove_file(&self, path: &PathBuf) -> Result<(), VfsError> {
            LocalFs.remove_file(path).await
        }
        async fn remove_dir(&self, path: &PathBuf) -> Result<(), VfsError> {
            LocalFs.remove_dir(path).await
        }
        async fn rename(&self, from: &PathBuf, to: &PathBuf) -> Result<(), VfsError> {
            // Copies rename their temporary files, which stay on one device.
            if from.to_string_lossy().contains(".sftp-tui-") {
                return LocalFs.rename(from, to).await;
            }
            Err(VfsError::Io(io::ErrorKind::CrossesDevices.into()))
        }
        async fn read_link(&self, path: &PathBuf) -> Result<Vec<u8>, VfsError> {
            LocalFs.read_link(path).await
        }
        async fn create_symlink(&self, target: &[u8], path: &PathBuf) -> Result<(), VfsError> {
            LocalFs.create_symlink(target, path).await
        }
        async fn set_permissions(&self, path: &PathBuf, mode: u32) -> Result<(), VfsError> {
            LocalFs.set_permissions(path, mode).await
        }
        async fn set_modified(&self, path: &PathBuf, time: SystemTime) -> Result<(), VfsError> {
            LocalFs.set_modified(path, time).await
        }
        async fn open_file(&self, path: &PathBuf) -> Result<Self::Reader, VfsError> {
            LocalFs.open_file(path).await
        }
        async fn create_file(
            &self,
            path: &PathBuf,
            replace: bool,
        ) -> Result<Self::Writer, VfsError> {
            LocalFs.create_file(path, replace).await
        }
    }

    #[tokio::test]
    async fn moving_across_file_systems_copies_and_removes() {
        let root = tempfile::tempdir().unwrap();
        let src = sources(root.path());
        let dst = root.path().join("dst");
        fs::create_dir(&dst).unwrap();
        let moving = vec![src.join("dir"), src.join("file")];
        let (outcome, seen) = run_move(&OtherDevice, moving, dst.clone(), Script::default()).await;
        assert!(seen.failed.is_empty(), "{seen:?}");
        // dir, a, sub, b, link, dangling, and file, copied one by one.
        assert_eq!(outcome.done, 7);
        check_dir(&dst.join("dir"));
        assert_eq!(fs::read_dir(&src).unwrap().count(), 0, "all of it moved");
    }

    #[tokio::test]
    async fn moves_on_a_host() {
        let root = tempfile::tempdir().unwrap();
        let src = sources(root.path());
        let Some((_server, sftp)) = testing::sftp_server(root.path()).await else {
            return;
        };
        fs::create_dir(root.path().join("dst")).unwrap();
        let moving = vec![RemotePath::from("src/dir"), RemotePath::from("src/file")];
        let target = RemotePath::from("dst");
        let (outcome, seen) = run_move(&sftp, moving, target, Script::default()).await;
        assert!(seen.failed.is_empty(), "{seen:?}");
        assert_eq!(outcome.done, 2, "two renames");
        check_dir(&root.path().join("dst/dir"));
        assert_eq!(fs::read_dir(&src).unwrap().count(), 0);
        sftp.close().await.unwrap();
    }
}
