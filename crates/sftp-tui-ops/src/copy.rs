//! Copying files, symlinks, and directory trees between any two backends.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

use sftp_tui_vfs::{
    FileKind, FileReader as _, FileWriter as _, Metadata, Vfs, VfsError, VfsPath as _,
};

use crate::job::{Decision, Event, Outcome, Progress, Reporter};

/// How to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyOptions {
    /// Give copies the modification times and permission bits of their sources.
    pub preserve: bool,
    /// Write each file under a temporary name next to its target, then rename it to the
    /// target, so that the target never holds part of a file.
    pub atomic: bool,
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

/// Where the sources go.
enum Destination<Q> {
    /// Into this directory, under their own names.
    Into(Q),
    /// The only source, to this name in this directory.
    As { dir: Q, name: Vec<u8> },
}

/// Why one attempt at an entry did not work.
enum Stop<L> {
    Failed(L, VfsError),
    Cancelled,
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
    };
    let count = sources.len();
    if job.run(sources, target, count).await.is_err() {
        job.outcome.aborted = true;
    }
    job.outcome
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
        self.items_total = items.len() as u64;
        let mut targets: Vec<Option<B::Path>> = Vec::with_capacity(items.len());
        let mut made_dirs = Vec::new();
        for item in &items {
            if self.reporter.cancelled() {
                return Err(Aborted);
            }
            let target = match (item.parent, &destination) {
                (Some(parent), _) => targets[parent]
                    .as_ref()
                    .map(|dir| dir.join_name(&item.name)),
                (None, Destination::Into(dir)) => Some(dir.join_name(&item.name)),
                (None, Destination::As { dir, name }) => Some(dir.join_name(name)),
            };
            let size = file_size(item);
            let Some(target) = target.filter(|_| !item.skip) else {
                self.outcome.skipped += 1;
                self.bytes_done += size;
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
                    Ok(()) => break true,
                    Err(Stop::Cancelled) => return Err(Aborted),
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
                // Only directories have entries to go into them.
                targets.push((item.metadata.kind == FileKind::Dir).then_some(target));
            } else {
                self.outcome.skipped += 1;
                self.bytes_done = before + size;
                targets.push(None);
            }
        }
        // Writing into a directory changes its time, so directories get theirs last.
        if self.options.preserve {
            for (dir, metadata) in made_dirs.iter().rev() {
                self.attributes(dir, metadata).await?;
            }
        }
        Ok(())
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

    async fn copy_link(&self, item: &Item<A::Path>, target: &B::Path) -> Result<(), Stop<L>> {
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
            .map_err(|error| self.at_target(target, error))
    }

    /// Copies a file to `target`, which must not exist, through a temporary name if the copy
    /// is atomic. What it wrote goes again if it does not finish.
    async fn copy_file(&mut self, item: &Item<A::Path>, target: &B::Path) -> Result<(), Stop<L>> {
        let to = self.to.vfs;
        match to.symlink_metadata(target).await {
            Err(VfsError::NotFound(_)) => {}
            Ok(_) => {
                let error = VfsError::AlreadyExists(target.display());
                return Err(self.at_target(target, error));
            }
            Err(error) => return Err(self.at_target(target, error)),
        }
        let mut reader = self
            .from
            .vfs
            .open_file(&item.source)
            .await
            .map_err(|error| Stop::Failed((self.from.report)(&item.source), error))?;
        let (writer, written) = self.create(target).await?;
        let result = self.transfer(item, &mut reader, writer, &written).await;
        let result = match result {
            Ok(()) if self.options.atomic => to
                .rename(&written, target)
                .await
                .map_err(|error| self.at_target(target, error)),
            other => other,
        };
        if result.is_err() {
            // Best effort: what is left of it is no use, and an error here would hide the first.
            let _ = to.remove_file(&written).await;
        }
        result
    }

    /// Opens the file to write for `target`: a temporary name next to it if the copy is atomic,
    /// or the target itself.
    async fn create(&self, target: &B::Path) -> Result<(B::Writer, B::Path), Stop<L>> {
        let to = self.to.vfs;
        if !self.options.atomic {
            return match to.create_file(target, false).await {
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
                return Err(Stop::Cancelled);
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
    }

    /// Copies with `answers` for failures; cancels once data has moved if `cancel_midway`.
    async fn run<A: Vfs, B: Vfs>(
        (from, sources): (&A, Vec<A::Path>),
        (to, target): (&B, B::Path),
        options: CopyOptions,
        answers: &[Decision],
        cancel_midway: bool,
    ) -> (Outcome, Seen) {
        let (events, mut incoming) = mpsc::unbounded_channel();
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
        let listen = async {
            let mut seen = Seen::default();
            let mut answers = answers.iter();
            while let Some(event) = incoming.recv().await {
                match event {
                    Event::Scanning { .. } => {}
                    Event::Progress(progress) => {
                        seen.bytes.push((progress.bytes_done, progress.bytes_total));
                        if cancel_midway && progress.bytes_done > 0 {
                            cancel.cancel();
                        }
                    }
                    Event::Failed { path, reply, .. } => {
                        seen.failed.push(path);
                        let answer = *answers.next().expect("an answer for each failure");
                        let _ = reply.send(answer);
                    }
                }
            }
            seen
        };
        tokio::join!(job, listen)
    }

    const BOTH: CopyOptions = CopyOptions {
        preserve: true,
        atomic: true,
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

    #[tokio::test]
    async fn a_taken_name_fails_and_merges_directories() {
        let root = tempfile::tempdir().unwrap();
        let src = sources(root.path());
        let dst = root.path().join("dst");
        fs::create_dir_all(dst.join("dir")).unwrap();
        fs::write(dst.join("dir/a"), "mine").unwrap();
        let sources = vec![src.join("dir")];
        let (outcome, seen) = run(
            (&LocalFs, sources),
            (&LocalFs, dst.clone()),
            BOTH,
            &[Decision::Skip],
            false,
        )
        .await;
        assert_eq!(seen.failed, [dst.join("dir/a").to_string_lossy()]);
        assert_eq!(fs::read_to_string(dst.join("dir/a")).unwrap(), "mine");
        assert_eq!(fs::read(dst.join("dir/sub/b")).unwrap(), vec![7; 100_000]);
        // dir (merged), sub, b, link, and dangling; a stays.
        assert_eq!((outcome.done, outcome.skipped), (5, 1));
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
        sftp.close().await.unwrap();
    }
}
