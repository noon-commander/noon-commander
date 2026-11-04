//! Copying files, symlinks, and directory trees between any two backends.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

use noc_vfs::{FileKind, FileReader as _, FileWriter as _, Metadata, Vfs, VfsError, VfsPath};

use crate::free_name::{free_name, numbered};
use crate::job::{Choice, Conflict, Decision, Event, Outcome, Progress, Reporter, Rest, Step};
use crate::spelling::spelled_otherwise;

/// How to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches, like the options of cp"
)]
pub struct CopyOptions {
    /// Give copies the modification times and permission bits of their sources.
    pub preserve: bool,
    /// Write each file under a temporary name next to its target, then rename it to the
    /// target, so that the target never holds part of a file.
    pub atomic: bool,
    /// Remove each source once all of it is copied, which moves it.
    pub remove_sources: bool,
    /// Replace what has a target's name without asking, as when a file goes back where it came
    /// from.
    pub overwrite: bool,
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
        // One source to its own name spelled otherwise: the rename gives it that spelling.
        if let [source] = sources.as_slice() {
            match self.respelled(source, &target).await? {
                None => {
                    self.outcome.skipped = 1;
                    return Ok(());
                }
                Some(true) => {
                    self.progress(source);
                    match self.rename_one(source, &target, true).await? {
                        Renamed::Done => self.outcome.done += 1,
                        Renamed::Skipped | Renamed::Copy(_) => self.outcome.skipped += 1,
                    }
                    return Ok(());
                }
                Some(false) => {}
            }
        }
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
            match self.rename_one(&source, &target, false).await? {
                Renamed::Done => self.outcome.done += 1,
                Renamed::Skipped => self.outcome.skipped += 1,
                Renamed::Copy(answered) => {
                    if let Some(answered) = answered {
                        self.answered.push((source.clone(), answered));
                    }
                    left.push(source);
                }
            }
        }
        if left.is_empty() {
            return Ok(());
        }
        // Counting is reported even where it lists no directory, as for files alone: the copy
        // that follows has totals of its own, and the renames tell nothing of its pace.
        self.reporter.report(Event::Scanning { items: 0 });
        let items = self.scan(left).await?;
        self.copy_items(&items, &destination).await
    }

    /// Copies within one file system: one source to its own name spelled otherwise is a
    /// failure, as a copy cannot go over its source.
    async fn run_copy(&mut self, sources: Vec<V::Path>, target: V::Path) -> Result<(), Aborted> {
        if let [source] = sources.as_slice() {
            loop {
                match self.respelled(source, &target).await? {
                    None => {
                        self.outcome.skipped = 1;
                        return Ok(());
                    }
                    Some(false) => break,
                    Some(true) => {
                        let error = VfsError::SameFile(target.display());
                        let path = (self.to.report)(&target);
                        match self.reporter.ask(path, Step::Create, error).await {
                            Decision::Retry => {}
                            Decision::Skip => {
                                self.outcome.skipped = 1;
                                return Ok(());
                            }
                            Decision::Abort => return Err(Aborted),
                        }
                    }
                }
            }
        }
        let count = sources.len();
        self.run(sources, target, count).await
    }

    /// Whether `target`, where the only source goes, is `source` under its name spelled
    /// otherwise; `None` if the source is skipped.
    async fn respelled(
        &mut self,
        source: &V::Path,
        target: &V::Path,
    ) -> Result<Option<bool>, Aborted> {
        let vfs = self.to.vfs;
        loop {
            let error = match vfs.symlink_metadata(target).await {
                Err(VfsError::NotFound(..)) => return Ok(Some(false)),
                Err(error) => error,
                Ok(there) => match vfs.symlink_metadata(source).await {
                    Err(error) => error,
                    Ok(here) => {
                        match spelled_otherwise(vfs, (source, &here), (target, &there)).await {
                            Ok(respelled) => return Ok(Some(respelled)),
                            Err(error) => error,
                        }
                    }
                },
            };
            let path = (self.from.report)(source);
            match self.reporter.ask(path, Step::Inspect, error).await {
                Decision::Retry => {}
                Decision::Skip => return Ok(None),
                Decision::Abort => return Err(Aborted),
            }
        }
    }

    async fn rename_one(
        &mut self,
        source: &V::Path,
        target: &V::Path,
        respelled: bool,
    ) -> Result<Renamed<V::Path>, Aborted> {
        loop {
            match self.try_rename(source, target, respelled).await {
                Ok(renamed) => return Ok(renamed),
                Err(Stop::Aborted) => return Err(Aborted),
                Err(Stop::Failed(path, step, error)) => {
                    match self.reporter.ask(path, step, error).await {
                        Decision::Retry => {}
                        Decision::Skip => return Ok(Renamed::Skipped),
                        Decision::Abort => return Err(Aborted),
                    }
                }
            }
        }
    }

    /// Renames `source` to `target`, asking first if the name is taken by something a rename
    /// would replace. If `respelled`, the target is the source under its name spelled
    /// otherwise, which the rename gives it: nothing is replaced, or copied.
    async fn try_rename(
        &mut self,
        source: &V::Path,
        target: &V::Path,
        respelled: bool,
    ) -> Result<Renamed<V::Path>, Stop<L>> {
        let vfs = self.to.vfs;
        if respelled {
            return vfs
                .rename(source, target)
                .await
                .map(|()| Renamed::Done)
                .map_err(|error| self.at_source(source, Step::Rename, error));
        }
        let metadata = vfs
            .symlink_metadata(source)
            .await
            .map_err(|error| self.at_source(source, Step::Inspect, error))?;
        let mut target = target.clone();
        let replace = match vfs.symlink_metadata(&target).await {
            Err(VfsError::NotFound(..)) => false,
            // A hard link, or a directory on the way spelled otherwise: a rename would do nothing.
            Ok(existing) if existing.same_object(&metadata) => {
                let error = VfsError::SameFile(target.display());
                return Err(self.at_target(&target, Step::Create, error));
            }
            Ok(existing) if existing.kind == FileKind::Dir && metadata.kind == FileKind::Dir => {
                return Ok(Renamed::Copy(None));
            }
            Ok(_) => {
                let item = Item {
                    source: source.clone(),
                    parent: None,
                    name: Vec::new(),
                    metadata,
                    skip: false,
                };
                match self.plan(&item, &mut target).await? {
                    Plan::New => false,
                    Plan::Skip => return Ok(Renamed::Skipped),
                    Plan::Replace(_) => true,
                }
            }
            Err(error) => return Err(self.at_target(&target, Step::Inspect, error)),
        };
        let target = &target;
        let renaming = |error| self.at_source(source, Step::Rename, error);
        let stop = match vfs.rename(source, target).await {
            Ok(()) => return Ok(Renamed::Done),
            Err(error) if error.is_cross_device() => {
                let answered = Answered {
                    target: target.clone(),
                    replace,
                };
                return Ok(Renamed::Copy(Some(answered)));
            }
            // Without posix-rename the target goes first.
            Err(VfsError::AlreadyExists(..)) if replace => match vfs.remove_file(target).await {
                Ok(()) => vfs.rename(source, target).await.err().map(renaming),
                Err(error) => Some(self.at_target(target, Step::Remove, error)),
            },
            Err(error) => Some(renaming(error)),
        };
        match stop {
            None => Ok(Renamed::Done),
            Some(stop) => Err(stop),
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

/// For each of `items`, the index of the last entry within it, at any depth, or its own if it
/// holds none. Each entry comes after the directory it is in, so once the copy is past that
/// index, all within the directory is done.
fn last_within<P>(items: &[Item<P>]) -> Vec<usize> {
    let mut last: Vec<usize> = (0..items.len()).collect();
    for index in (0..items.len()).rev() {
        if let Some(parent) = items[index].parent {
            last[parent] = last[parent].max(last[index]);
        }
    }
    last
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
    Failed(L, Step, VfsError),
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
#[derive(Debug, Clone)]
enum Policy<Q> {
    OverwriteAll,
    SkipAll,
    /// A free name in this directory.
    RenameAll(Q),
    OverwriteOlder,
    OverwriteSmaller,
    OverwriteLarger,
    OverwriteSizeDiffers,
}

impl<Q: VfsPath> Policy<Q> {
    /// What `rest` makes of the answer `this`, given for a target renamed to `renamed`, if it
    /// was.
    fn of(this: &Choice, rest: Rest, renamed: Option<&Q>) -> Option<Self> {
        match rest {
            Rest::Ask => None,
            Rest::Same => match this {
                Choice::Overwrite => Some(Self::OverwriteAll),
                Choice::Skip => Some(Self::SkipAll),
                Choice::Rename(_) => renamed.and_then(VfsPath::parent).map(Self::RenameAll),
                Choice::Abort => None,
            },
            Rest::OverwriteOlder => Some(Self::OverwriteOlder),
            Rest::OverwriteSmaller => Some(Self::OverwriteSmaller),
            Rest::OverwriteLarger => Some(Self::OverwriteLarger),
            Rest::OverwriteSizeDiffers => Some(Self::OverwriteSizeDiffers),
        }
    }
}

/// What to do about a taken name, after a question or by a policy.
enum Next<Q> {
    Plan(Plan),
    /// Go to this path instead.
    To(Q),
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
    let mut job = Job::new(from, to, options, reporter);
    let count = sources.len();
    if job.run(sources, target, count).await.is_err() {
        job.outcome.aborted = true;
    }
    job.outcome
}

/// Copies `sources` to `target` within one file system, as [`copy`] does. A copy never goes
/// over its source: one source to its own name spelled otherwise, which a file system that
/// ignores case or Unicode normalization takes for its name, is a failure, as is a target
/// that is a source by device and inode.
pub async fn copy_within<V: Vfs, L>(
    side: Endpoint<'_, V, L>,
    sources: Vec<V::Path>,
    target: V::Path,
    options: CopyOptions,
    reporter: &mut Reporter<L>,
) -> Outcome {
    let from = Endpoint {
        vfs: side.vfs,
        report: side.report,
    };
    let mut job = Job::new(from, side, options, reporter);
    if job.run_copy(sources, target).await.is_err() {
        job.outcome.aborted = true;
    }
    job.outcome
}

/// Moves `sources` to `target` within one file system, as [`copy`] places them: renames each
/// source where it can, asking first when its name is taken, and copies and removes it where
/// it cannot: into a directory of the same name, which merges, or across file systems.
/// Copies that remove their sources keep times and permissions, and the copy options
/// otherwise apply. One source to its own name spelled otherwise, which a file system that
/// ignores case or Unicode normalization takes for its name, takes that spelling.
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
    let from = Endpoint {
        vfs: side.vfs,
        report: side.report,
    };
    let mut job = Job::new(from, side, options, reporter);
    job.items_total = sources.len() as u64;
    let count = sources.len();
    if job.run_move(sources, target, count).await.is_err() {
        job.outcome.aborted = true;
    }
    job.outcome
}

/// What a rename did.
enum Renamed<Q> {
    Done,
    Skipped,
    /// It cannot rename this one; a copy has to move it, where the question about a taken name
    /// said, if it asked.
    Copy(Option<Answered<Q>>),
}

/// Where a source goes after the question about its taken name, which its copy does not ask
/// again: `target`, the name it had or a new one, over what is there if `replace`.
#[derive(Debug, Clone)]
struct Answered<Q> {
    target: Q,
    replace: bool,
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
    /// Bytes that arrived where they were written, for speeds: unlike `bytes_done`, it neither
    /// jumps on a skip nor goes back on a retry.
    bytes_copied: u64,
    policy: Option<Policy<B::Path>>,
    /// Sources a move could not rename, with what the question about their taken names said.
    answered: Vec<(A::Path, Answered<B::Path>)>,
    /// The target that the entry being copied may replace without asking, as answered.
    granted: Option<B::Path>,
    /// What a copy of a file that failed left behind, as it could not remove it either, as on
    /// a host whose connection was lost: the next copy of a file replaces it if it is its
    /// target, else removes it first.
    leftover: Option<B::Path>,
}

impl<'a, 'r, A: Vfs, B: Vfs, L> Job<'a, 'r, A, B, L> {
    fn new(
        from: Endpoint<'a, A, L>,
        to: Endpoint<'a, B, L>,
        options: CopyOptions,
        reporter: &'r mut Reporter<L>,
    ) -> Self {
        Self {
            from,
            to,
            options,
            reporter,
            outcome: Outcome::default(),
            items_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            bytes_copied: 0,
            policy: options.overwrite.then_some(Policy::OverwriteAll),
            answered: Vec::new(),
            granted: None,
            leftover: None,
        }
    }

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
    /// all of it is copied. A directory is finished once all in it is done: the copy gets the
    /// attributes of its source, and a move removes the source.
    async fn copy_items(
        &mut self,
        items: &[Item<A::Path>],
        destination: &Destination<B::Path>,
    ) -> Result<(), Aborted> {
        self.items_total = self.outcome.done + self.outcome.skipped + items.len() as u64;
        let last = last_within(items);
        let mut targets: Vec<Option<B::Path>> = Vec::with_capacity(items.len());
        // Sources that stay: not copied, or holding something that was not.
        let mut kept = vec![false; items.len()];
        // Directories this copy made, rather than went into.
        let mut made = vec![false; items.len()];
        for (index, item) in items.iter().enumerate() {
            if self.reporter.cancelled() {
                return Err(Aborted);
            }
            self.granted = None;
            let target = match item.parent {
                Some(parent) => targets[parent]
                    .as_ref()
                    .map(|dir| dir.join_name(&item.name)),
                None => Some(
                    self.answered_target(item)
                        .unwrap_or_else(|| destination.path(&item.name)),
                ),
            };
            let size = file_size(item);
            let Some(target) = target.filter(|_| !item.skip) else {
                self.outcome.skipped += 1;
                self.bytes_done += size;
                keep(&mut kept, items, index);
                targets.push(None);
                self.finish_dirs(items, index, &last, (&targets, &made), &mut kept)
                    .await?;
                continue;
            };
            self.progress(&item.source);
            let (before, total) = (self.bytes_done, self.bytes_total);
            let copied = loop {
                let attempt = match item.metadata.kind {
                    FileKind::Dir => self.copy_dir(item, &target).await.map(|made_now| {
                        made[index] = made_now;
                        true
                    }),
                    FileKind::Symlink => self.copy_link(item, &target).await,
                    FileKind::File | FileKind::Unknown => self.copy_file(item, &target).await,
                    FileKind::Fifo
                    | FileKind::Socket
                    | FileKind::BlockDevice
                    | FileKind::CharDevice => Err(self.at_source(
                        &item.source,
                        Step::Read,
                        VfsError::Io(io::ErrorKind::Unsupported.into()),
                    )),
                };
                match attempt {
                    Ok(copied) => break copied,
                    Err(Stop::Aborted) => return Err(Aborted),
                    Err(Stop::Failed(path, step, error)) => {
                        (self.bytes_done, self.bytes_total) = (before, total);
                        match self.reporter.ask(path, step, error).await {
                            Decision::Retry => {}
                            Decision::Skip => break false,
                            Decision::Abort => return Err(Aborted),
                        }
                    }
                }
            };
            if copied {
                self.outcome.done += 1;
                // A file that shrank since it was counted held less than the total said; one
                // that grew has added to it as its bytes arrived.
                let moved = self.bytes_done - before;
                self.bytes_total -= size.saturating_sub(moved);
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
            self.finish_dirs(items, index, &last, (&targets, &made), &mut kept)
                .await?;
        }
        Ok(())
    }

    /// Where a source that a move could not rename goes, as the question about its taken name
    /// said; the target it may then replace without asking again.
    fn answered_target(&mut self, item: &Item<A::Path>) -> Option<B::Path> {
        let index = self
            .answered
            .iter()
            .position(|(source, _)| *source == item.source)?;
        let (_, answered) = self.answered.swap_remove(index);
        self.granted = answered.replace.then(|| answered.target.clone());
        Some(answered.target)
    }

    /// Finishes the directories whose `last` entry is `items[index]`, now done: the deepest
    /// first, each empty of anything to come by its turn. Writing into a directory changes its
    /// time, so a copy that `made` it gives it the attributes of its source only then, at its
    /// `targets`; a move removes the source, unless it is `kept`. So this work is done among the
    /// entries, where the time left of the job counts it, and an aborted copy keeps it.
    async fn finish_dirs(
        &mut self,
        items: &[Item<A::Path>],
        index: usize,
        last: &[usize],
        (targets, made): (&[Option<B::Path>], &[bool]),
        kept: &mut [bool],
    ) -> Result<(), Aborted> {
        let mut dir = if items[index].metadata.kind == FileKind::Dir {
            Some(index)
        } else {
            items[index].parent
        };
        while let Some(at) = dir.filter(|&at| last[at] == index) {
            let item = &items[at];
            if self.options.preserve
                && made[at]
                && let Some(target) = &targets[at]
            {
                self.attributes(target, &item.metadata).await?;
            }
            if self.options.remove_sources && !kept[at] && !self.remove_source(item).await? {
                keep(kept, items, at);
            }
            dir = item.parent;
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
                Ok(()) | Err(VfsError::NotFound(..)) => return Ok(true),
                Err(error) => error,
            };
            let path = (self.from.report)(&item.source);
            match self.reporter.ask(path, Step::Remove, error).await {
                Decision::Retry => {}
                Decision::Skip => return Ok(false),
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
            let (step, error) = match to.metadata(&target).await {
                Ok(metadata) if metadata.kind == FileKind::Dir => {
                    return Ok(Some(Destination::Into(target)));
                }
                Ok(_) | Err(VfsError::NotFound(..)) if count == 1 => {
                    return Ok(Some(match (target.parent(), target.name()) {
                        (Some(dir), Some(name)) => Destination::As {
                            name: name.to_vec(),
                            dir,
                        },
                        _ => Destination::Into(target),
                    }));
                }
                Err(VfsError::NotFound(..)) => match to.create_dir(&target).await {
                    Ok(()) => return Ok(Some(Destination::Into(target))),
                    Err(error) => (Step::Create, error),
                },
                Ok(_) => (
                    Step::Create,
                    VfsError::AlreadyExists(target.display(), None),
                ),
                Err(error) => (Step::Inspect, error),
            };
            match self
                .reporter
                .ask((self.to.report)(&target), step, error)
                .await
            {
                Decision::Retry => {}
                Decision::Skip => return Ok(None),
                Decision::Abort => return Err(Aborted),
            }
        }
    }

    /// Lists the trees under `sources`, parents before what they hold, and counts their
    /// bytes, reporting how many entries it has found after each source and each directory.
    /// Listings report symlinks without following them.
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
                    Err(error) => {
                        let path = (self.from.report)(&source);
                        match self.reporter.ask(path, Step::Inspect, error).await {
                            Decision::Retry => {}
                            Decision::Skip => break None,
                            Decision::Abort => return Err(Aborted),
                        }
                    }
                }
            };
            let Some(metadata) = metadata else {
                self.outcome.skipped += 1;
                continue;
            };
            let name = source.name().unwrap_or_default().to_vec();
            self.add(&mut items, &mut dirs, source, None, name, metadata);
            self.reporter.report(Event::Scanning {
                items: items.len() as u64,
            });
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
                            match self.reporter.ask(path, Step::List, error).await {
                                Decision::Retry => {}
                                Decision::Skip => {
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
            bytes_copied: self.bytes_copied,
        }));
    }

    fn at_source(&self, path: &A::Path, step: Step, error: VfsError) -> Stop<L> {
        Stop::Failed((self.from.report)(path), step, error)
    }

    fn at_target(&self, path: &B::Path, step: Step, error: VfsError) -> Stop<L> {
        Stop::Failed((self.to.report)(path), step, error)
    }

    /// Makes the directory `target` for `item`, or goes into one that is there, unless that is
    /// the source itself; `true` if it made it.
    async fn copy_dir(&self, item: &Item<A::Path>, target: &B::Path) -> Result<bool, Stop<L>> {
        let to = self.to.vfs;
        match to.create_dir(target).await {
            Ok(()) => Ok(true),
            Err(VfsError::AlreadyExists(name, code)) => match to.metadata(target).await {
                Ok(metadata) if metadata.same_object(&item.metadata) => {
                    Err(self.at_target(target, Step::Create, VfsError::SameFile(name)))
                }
                Ok(metadata) if metadata.kind == FileKind::Dir => Ok(false),
                _ => {
                    let error = VfsError::AlreadyExists(name, code);
                    Err(self.at_target(target, Step::Create, error))
                }
            },
            Err(error) => Err(self.at_target(target, Step::Create, error)),
        }
    }

    /// What to do about the name `target` of a copy of `item`: free, or taken by something
    /// that the answer to a question, or an earlier answer for it or for all, says to replace or
    /// keep, or to leave for another name, which `target` becomes. A directory there is a failure: a
    /// file does not replace one; so is the source itself, which a copy would empty.
    async fn plan(&mut self, item: &Item<A::Path>, target: &mut B::Path) -> Result<Plan, Stop<L>> {
        loop {
            let existing = match self.to.vfs.symlink_metadata(target).await {
                Ok(existing) => existing,
                Err(VfsError::NotFound(..)) => return Ok(Plan::New),
                Err(error) => return Err(self.at_target(target, Step::Inspect, error)),
            };
            if existing.same_object(&item.metadata) {
                let error = VfsError::SameFile(target.display());
                return Err(self.at_target(target, Step::Create, error));
            }
            if existing.kind == FileKind::Dir {
                let error = VfsError::AlreadyExists(target.display(), None);
                return Err(self.at_target(target, Step::Create, error));
            }
            if self.granted.as_ref() == Some(&*target) {
                return Ok(Plan::Replace(existing.kind));
            }
            let next = match self.policy.clone() {
                Some(policy) => self.by_policy(policy, item, target, &existing).await?,
                None => self.ask_taken(item, target, &existing).await?,
            };
            match next {
                Next::Plan(plan) => return Ok(plan),
                Next::To(path) => *target = path,
            }
        }
    }

    /// Asks what to do about `existing`, the entry at `target` that a copy of `item` would
    /// replace, and keeps the answer for the rest.
    async fn ask_taken(
        &mut self,
        item: &Item<A::Path>,
        target: &B::Path,
        existing: &Metadata,
    ) -> Result<Next<B::Path>, Stop<L>> {
        let name = target.name().unwrap_or_default();
        let free = match target.parent() {
            Some(dir) => free_name(self.to.vfs, &dir, name).await.ok(),
            None => None,
        };
        let free = free
            .as_ref()
            .and_then(VfsPath::name)
            .map_or_else(|| numbered(name, 2), <[u8]>::to_vec);
        let source = (self.from.report)(&item.source);
        let reported = (self.to.report)(target);
        let (source_metadata, target_metadata) = (item.metadata.clone(), existing.clone());
        let answer = self
            .reporter
            .question(|reply| Event::Exists {
                source,
                target: reported,
                source_metadata,
                target_metadata,
                free,
                reply,
            })
            .await;
        let Conflict { this, rest } = answer.unwrap_or(Conflict::once(Choice::Abort));
        let renamed = match &this {
            Choice::Rename(path) => Some(B::Path::from_bytes(path)),
            _ => None,
        };
        self.policy = Policy::of(&this, rest, renamed.as_ref());
        match (this, renamed) {
            (Choice::Overwrite, _) => Ok(Next::Plan(Plan::Replace(existing.kind))),
            (Choice::Skip, _) => Ok(Next::Plan(Plan::Skip)),
            (Choice::Rename(_), Some(path)) => Ok(Next::To(path)),
            (Choice::Rename(_) | Choice::Abort, _) => Err(Stop::Aborted),
        }
    }

    /// What an earlier answer for all says to do about `existing`, the entry at `target` that a
    /// copy of `item` would replace.
    async fn by_policy(
        &self,
        policy: Policy<B::Path>,
        item: &Item<A::Path>,
        target: &B::Path,
        existing: &Metadata,
    ) -> Result<Next<B::Path>, Stop<L>> {
        let (there, source) = (existing, &item.metadata);
        let replace = match policy {
            Policy::OverwriteAll => true,
            Policy::SkipAll => false,
            Policy::RenameAll(dir) => {
                let name = target.name().unwrap_or_default();
                return match free_name(self.to.vfs, &dir, name).await {
                    Ok(path) => Ok(Next::To(path)),
                    Err(error) => Err(self.at_target(target, Step::Inspect, error)),
                };
            }
            // Unknown times keep the target.
            Policy::OverwriteOlder => matches!(
                (there.modified, source.modified),
                (Some(there), Some(source)) if there < source
            ),
            Policy::OverwriteSmaller => matches!(
                (there.size, source.size),
                (Some(there), Some(source)) if there < source
            ),
            Policy::OverwriteLarger => matches!(
                (there.size, source.size),
                (Some(there), Some(source)) if there > source
            ),
            Policy::OverwriteSizeDiffers => matches!(
                (there.size, source.size),
                (Some(there), Some(source)) if there != source
            ),
        };
        Ok(Next::Plan(if replace {
            Plan::Replace(existing.kind)
        } else {
            Plan::Skip
        }))
    }

    /// Copies a symlink, as a symlink; `false` if the name is taken and stays.
    async fn copy_link(&mut self, item: &Item<A::Path>, target: &B::Path) -> Result<bool, Stop<L>> {
        let mut target = target.clone();
        let plan = self.plan(item, &mut target).await?;
        let target = &target;
        match plan {
            Plan::New => {}
            Plan::Skip => return Ok(false),
            Plan::Replace(_) => self
                .to
                .vfs
                .remove_file(target)
                .await
                .map_err(|error| self.at_target(target, Step::Remove, error))?,
        }
        let link = self
            .from
            .vfs
            .read_link(&item.source)
            .await
            .map_err(|error| self.at_source(&item.source, Step::Read, error))?;
        self.to
            .vfs
            .create_symlink(&link, target)
            .await
            .map_err(|error| self.at_target(target, Step::Create, error))?;
        Ok(true)
    }

    /// Copies a file to `target`, through a temporary name if the copy is atomic; `false` if
    /// the name is taken and stays. What it wrote goes again if it does not finish.
    async fn copy_file(&mut self, item: &Item<A::Path>, target: &B::Path) -> Result<bool, Stop<L>> {
        let to = self.to.vfs;
        let mut target = target.clone();
        if let Some(leftover) = self.leftover.take() {
            if leftover == target {
                self.granted = Some(leftover);
            } else {
                let _ = to.remove_file(&leftover).await;
            }
        }
        let plan = self.plan(item, &mut target).await?;
        let target = &target;
        let replace = match plan {
            Plan::New => false,
            Plan::Skip => return Ok(false),
            // Written directly, a file would go through a symlink to whatever it points to.
            Plan::Replace(FileKind::Symlink) if !self.options.atomic => {
                to.remove_file(target)
                    .await
                    .map_err(|error| self.at_target(target, Step::Remove, error))?;
                false
            }
            Plan::Replace(_) => true,
        };
        let mut reader = self
            .from
            .vfs
            .open_file(&item.source)
            .await
            .map_err(|error| self.at_source(&item.source, Step::Read, error))?;
        let (writer, written) = self.create(target, replace).await?;
        let result = self
            .transfer(item, &mut reader, writer, (&written, target))
            .await;
        let result = match result {
            Ok(()) if self.options.atomic => self.rename_over(&written, target, replace).await,
            other => other,
        };
        if result.is_err() && to.remove_file(&written).await.is_err() {
            // What is left of it is no use, and an error here would hide the first.
            self.leftover = Some(written);
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
        match to.rename(written, target).await {
            Ok(()) => return Ok(()),
            Err(VfsError::AlreadyExists(..)) if replace => {
                if let Err(error) = to.remove_file(target).await {
                    return Err(self.at_target(target, Step::Remove, error));
                }
            }
            Err(error) => return Err(self.at_target(target, Step::Rename, error)),
        }
        to.rename(written, target)
            .await
            .map_err(|error| self.at_target(target, Step::Rename, error))
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
                Err(error) => Err(self.at_target(target, Step::Create, error)),
            };
        }
        let (Some(dir), Some(name)) = (target.parent(), target.name()) else {
            let error = VfsError::Io(io::ErrorKind::InvalidInput.into());
            return Err(self.at_target(target, Step::Create, error));
        };
        loop {
            let temporary = dir.join_name(&temporary_name(name));
            let created = self
                .reporter
                .unless_cancelled(to.create_file(&temporary, false))
                .await;
            match created {
                // On a host the file is still made once asked for, so it goes again after.
                None => {
                    let _ = to.remove_file(&temporary).await;
                    return Err(Stop::Aborted);
                }
                Some(Ok(writer)) => return Ok((writer, temporary)),
                Some(Err(VfsError::AlreadyExists(..))) => {}
                Some(Err(error)) => return Err(self.at_target(target, Step::Create, error)),
            }
        }
    }

    /// Moves the data of `item` from `reader` to `writer`, which writes `written` for `target`,
    /// and gives it the attributes of `item` if the copy preserves them. Bytes count as they
    /// arrive, which on a host is when the server has taken them. Failures name `target`, not
    /// a temporary name the user never sees. A cancel stops the waits for data.
    async fn transfer(
        &mut self,
        item: &Item<A::Path>,
        reader: &mut A::Reader,
        mut writer: B::Writer,
        (written, target): (&B::Path, &B::Path),
    ) -> Result<(), Stop<L>> {
        let mut counted = 0;
        loop {
            if self.reporter.cancelled() {
                return Err(Stop::Aborted);
            }
            let chunk = self
                .reporter
                .unless_cancelled(reader.read())
                .await
                .ok_or(Stop::Aborted)?
                .map_err(|error| self.at_source(&item.source, Step::Read, error))?;
            let Some(chunk) = chunk else { break };
            let wrote = self.reporter.unless_cancelled(writer.write(chunk)).await;
            // What the server took before a write it refused still counts.
            self.arrived(item, &writer, &mut counted);
            wrote
                .ok_or(Stop::Aborted)?
                .map_err(|error| self.at_target(target, Step::Write, error))?;
        }
        loop {
            let more = self.reporter.unless_cancelled(writer.settle()).await;
            self.arrived(item, &writer, &mut counted);
            let more = more
                .ok_or(Stop::Aborted)?
                .map_err(|error| self.at_target(target, Step::Write, error))?;
            if !more {
                break;
            }
            if self.reporter.cancelled() {
                return Err(Stop::Aborted);
            }
        }
        self.reporter
            .unless_cancelled(writer.finish())
            .await
            .ok_or(Stop::Aborted)?
            .map_err(|error| self.at_target(target, Step::Write, error))?;
        if self.options.preserve {
            self.reporter
                .unless_cancelled(self.set_attributes(written, &item.metadata))
                .await
                .ok_or(Stop::Aborted)?
                .map_err(|error| self.at_target(target, Step::SetAttributes, error))?;
        }
        Ok(())
    }

    /// Counts what has arrived through `writer` beyond the `counted` bytes of `item`, and
    /// reports it if there is any. Bytes beyond the size the file had when it was counted add to
    /// the total, so that the bytes done never go past it.
    fn arrived(&mut self, item: &Item<A::Path>, writer: &B::Writer, counted: &mut u64) {
        let new = writer.written().saturating_sub(*counted);
        if new > 0 {
            let size = file_size(item);
            let beyond = |bytes: u64| bytes.saturating_sub(size);
            self.bytes_total += beyond(*counted + new) - beyond(*counted);
            *counted += new;
            self.bytes_done += new;
            self.bytes_copied += new;
            self.progress(&item.source);
        }
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
            match self
                .reporter
                .ask((self.to.report)(dir), Step::SetAttributes, error)
                .await
            {
                Decision::Retry => {}
                Decision::Skip => return Ok(()),
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
    let suffix = format!(".noc-{}-{number}", std::process::id());
    // Long names would leave no room for the suffix in a file system's limit of 255 bytes.
    let mut temporary = b".".to_vec();
    if name.len() + suffix.len() < 250 {
        temporary.extend_from_slice(name);
    }
    temporary.extend_from_slice(suffix.as_bytes());
    temporary
}

#[cfg(test)]
mod tests;
