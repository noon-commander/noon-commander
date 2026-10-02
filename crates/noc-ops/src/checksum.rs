//! Checksums of files and directory trees.

use std::fmt;
use std::mem;

use noc_vfs::{FileKind, FileReader as _, Metadata, Vfs, VfsError, VfsPath};
use sha2::Digest as _;
use tokio::task::{JoinError, JoinHandle};

use crate::copy::Endpoint;
use crate::job::{Decision, Event, Outcome, Progress, Reporter};

/// Data gathered before it goes to a hasher off the async thread: enough that the trip there
/// costs little next to the hashing.
const BATCH: usize = 256 * 1024;

/// A hash function for checksums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Algorithm {
    Sha256,
    Sha512,
    Sha1,
    Md5,
    Blake3,
}

impl Algorithm {
    /// Every algorithm, in the order a menu offers them.
    pub const ALL: [Self; 5] = [
        Self::Sha256,
        Self::Sha512,
        Self::Sha1,
        Self::Md5,
        Self::Blake3,
    ];

    /// Bytes of a digest.
    pub fn digest_len(self) -> usize {
        match self {
            Self::Sha256 | Self::Blake3 => 32,
            Self::Sha512 => 64,
            Self::Sha1 => 20,
            Self::Md5 => 16,
        }
    }
}

/// A running hash of one of the algorithms.
pub struct Hasher(State);

enum State {
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
    Sha1(sha1::Sha1),
    Md5(md5::Md5),
    // Large next to the others.
    Blake3(Box<blake3::Hasher>),
}

impl Hasher {
    pub fn new(algorithm: Algorithm) -> Self {
        Self(match algorithm {
            Algorithm::Sha256 => State::Sha256(sha2::Sha256::new()),
            Algorithm::Sha512 => State::Sha512(sha2::Sha512::new()),
            Algorithm::Sha1 => State::Sha1(sha1::Sha1::new()),
            Algorithm::Md5 => State::Md5(md5::Md5::new()),
            Algorithm::Blake3 => State::Blake3(Box::default()),
        })
    }

    /// The algorithm it hashes with.
    pub fn algorithm(&self) -> Algorithm {
        match self.0 {
            State::Sha256(_) => Algorithm::Sha256,
            State::Sha512(_) => Algorithm::Sha512,
            State::Sha1(_) => Algorithm::Sha1,
            State::Md5(_) => Algorithm::Md5,
            State::Blake3(_) => Algorithm::Blake3,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match &mut self.0 {
            State::Sha256(hasher) => hasher.update(data),
            State::Sha512(hasher) => hasher.update(data),
            State::Sha1(hasher) => hasher.update(data),
            State::Md5(hasher) => hasher.update(data),
            State::Blake3(hasher) => {
                hasher.update(data);
            }
        }
    }

    pub fn finalize(self) -> Vec<u8> {
        match self.0 {
            State::Sha256(hasher) => hasher.finalize().to_vec(),
            State::Sha512(hasher) => hasher.finalize().to_vec(),
            State::Sha1(hasher) => hasher.finalize().to_vec(),
            State::Md5(hasher) => hasher.finalize().to_vec(),
            State::Blake3(hasher) => hasher.finalize().as_bytes().to_vec(),
        }
    }
}

impl fmt::Debug for Hasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Hasher").field(&self.algorithm()).finish()
    }
}

/// The checksum of a file, or the lack of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sum<L> {
    /// The file, as its endpoint reports it.
    pub path: L,
    /// Its name relative to the directory the targets are in: a target's own name, then
    /// `/`-separated names below it for files found in a target directory.
    pub name: Vec<u8>,
    /// Bytes hashed, or the size the scan found if it was skipped.
    pub size: u64,
    /// `None` if it was skipped after a failure.
    pub digest: Option<Vec<u8>>,
}

/// A file to hash, found while scanning.
#[derive(Debug)]
struct File<P> {
    path: P,
    name: Vec<u8>,
    size: u64,
}

/// Files found by [`Checksum::scan`], to pass to [`Checksum::hash`] with the same endpoint.
#[derive(Debug)]
pub struct Files<P> {
    files: Vec<File<P>>,
}

/// Why the job stopped early.
struct Aborted;

/// Why one attempt at a file did not work.
enum Stop {
    Failed(VfsError),
    /// Cancelled, or the hashing thread panicked.
    Aborted,
}

/// A hasher, or the blocking task that has it while it hashes.
enum Running {
    Ready(Box<Hasher>),
    Busy(JoinHandle<Box<Hasher>>),
}

impl Running {
    async fn ready(self) -> Result<Box<Hasher>, JoinError> {
        match self {
            Self::Ready(hasher) => Ok(hasher),
            Self::Busy(task) => task.await,
        }
    }
}

/// An entry still to look at while walking a target directory.
struct Pending<P> {
    path: P,
    name: Vec<u8>,
    metadata: Metadata,
}

/// Hashes files found under targets of one or more endpoints, with one total for progress:
/// every `scan` comes before the first `hash`.
pub struct Checksum<'r, L> {
    algorithm: Algorithm,
    reporter: &'r mut Reporter<L>,
    outcome: Outcome,
    sums: Vec<Sum<L>>,
    /// Files found by every scan so far.
    found: u64,
    items_total: u64,
    bytes_done: u64,
    bytes_total: u64,
    /// Bytes read, for speeds: unlike `bytes_done`, it neither jumps on a skip nor goes back
    /// on a retry.
    bytes_copied: u64,
}

impl<'r, L> Checksum<'r, L> {
    pub fn new(algorithm: Algorithm, reporter: &'r mut Reporter<L>) -> Self {
        Self {
            algorithm,
            reporter,
            outcome: Outcome::default(),
            sums: Vec::new(),
            found: 0,
            items_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            bytes_copied: 0,
        }
    }

    /// Finds the files under `targets`, adding them to the totals. Symlinks among the targets
    /// are followed; below them, symlinks to files are hashed and other symlinks left out, so
    /// that a walk never loops. Special files are left out and never opened: reading a FIFO
    /// would wait for a writer. Directories are walked in the order of their names.
    pub async fn scan<V: Vfs>(
        &mut self,
        side: &Endpoint<'_, V, L>,
        targets: Vec<V::Path>,
    ) -> Files<V::Path> {
        let mut files = Vec::new();
        if !self.outcome.aborted && self.walk(side, targets, &mut files).await.is_err() {
            self.outcome.aborted = true;
            files.clear();
        }
        Files { files }
    }

    /// Hashes `files`, in the order they were found. When reading one fails it asks the
    /// reporter what to do; a retry hashes the file from its start.
    pub async fn hash<V: Vfs>(&mut self, side: &Endpoint<'_, V, L>, files: Files<V::Path>) {
        if !self.outcome.aborted && self.hash_files(side, files).await.is_err() {
            self.outcome.aborted = true;
        }
    }

    /// How it ended, and the sums in order: those of the first `hash`, then the next.
    pub fn finish(self) -> (Outcome, Vec<Sum<L>>) {
        (self.outcome, self.sums)
    }

    async fn walk<V: Vfs>(
        &mut self,
        side: &Endpoint<'_, V, L>,
        targets: Vec<V::Path>,
        files: &mut Vec<File<V::Path>>,
    ) -> Result<(), Aborted> {
        for path in targets {
            let Some(metadata) = self.retry(side, &path, Vfs::metadata).await? else {
                continue;
            };
            let name = path.name().unwrap_or_default().to_vec();
            let mut pending = vec![Pending {
                path,
                name,
                metadata,
            }];
            while let Some(entry) = pending.pop() {
                if self.reporter.cancelled() {
                    return Err(Aborted);
                }
                match entry.metadata.kind {
                    FileKind::File => self.add(files, entry.path, entry.name, &entry.metadata),
                    // Servers that do not tell the kind in listings may tell it here.
                    FileKind::Symlink | FileKind::Unknown => {
                        if let Ok(metadata) = side.vfs.metadata(&entry.path).await
                            && matches!(metadata.kind, FileKind::File | FileKind::Unknown)
                        {
                            self.add(files, entry.path, entry.name, &metadata);
                        }
                    }
                    FileKind::Dir => {
                        let listed = self.retry(side, &entry.path, Vfs::list_dir).await?;
                        let Some(mut entries) = listed else { continue };
                        entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
                        // Reversed onto the stack, so that they come off in order.
                        for child in entries.into_iter().rev() {
                            let mut name = entry.name.clone();
                            if !name.is_empty() {
                                name.push(b'/');
                            }
                            name.extend_from_slice(&child.name);
                            pending.push(Pending {
                                path: entry.path.join_name(&child.name),
                                name,
                                metadata: child.metadata,
                            });
                        }
                        self.reporter.report(Event::Scanning { items: self.found });
                    }
                    FileKind::Fifo
                    | FileKind::Socket
                    | FileKind::BlockDevice
                    | FileKind::CharDevice => {}
                }
            }
        }
        self.reporter.report(Event::Scanning { items: self.found });
        Ok(())
    }

    /// Runs `operation` on `path` until it works, or the reporter says to skip it: then
    /// `None`, and it counts as one entry skipped.
    async fn retry<'v, 'p, V: Vfs, T, F>(
        &mut self,
        side: &Endpoint<'v, V, L>,
        path: &'p V::Path,
        operation: impl Fn(&'v V, &'p V::Path) -> F,
    ) -> Result<Option<T>, Aborted>
    where
        F: Future<Output = Result<T, VfsError>>,
    {
        loop {
            if self.reporter.cancelled() {
                return Err(Aborted);
            }
            match operation(side.vfs, path).await {
                Ok(value) => return Ok(Some(value)),
                Err(error) => match self.reporter.ask((side.report)(path), error).await {
                    Decision::Retry => {}
                    Decision::Skip | Decision::SkipAll => {
                        self.outcome.skipped += 1;
                        self.items_total += 1;
                        return Ok(None);
                    }
                    Decision::Abort => return Err(Aborted),
                },
            }
        }
    }

    fn add<P>(&mut self, files: &mut Vec<File<P>>, path: P, name: Vec<u8>, metadata: &Metadata) {
        let size = metadata.size.unwrap_or(0);
        self.found += 1;
        self.items_total += 1;
        self.bytes_total += size;
        files.push(File { path, name, size });
    }

    async fn hash_files<V: Vfs>(
        &mut self,
        side: &Endpoint<'_, V, L>,
        files: Files<V::Path>,
    ) -> Result<(), Aborted> {
        for file in files.files {
            let before = self.bytes_done;
            let hashed = loop {
                if self.reporter.cancelled() {
                    return Err(Aborted);
                }
                match self.hash_file(side, &file.path).await {
                    Ok(hashed) => break Some(hashed),
                    Err(Stop::Aborted) => return Err(Aborted),
                    Err(Stop::Failed(error)) => {
                        self.bytes_done = before;
                        match self.reporter.ask((side.report)(&file.path), error).await {
                            Decision::Retry => {}
                            Decision::Skip | Decision::SkipAll => break None,
                            Decision::Abort => return Err(Aborted),
                        }
                    }
                }
            };
            let (bytes, digest) = if let Some((bytes, digest)) = hashed {
                self.outcome.done += 1;
                (bytes, Some(digest))
            } else {
                self.outcome.skipped += 1;
                self.bytes_done = before + file.size;
                (file.size, None)
            };
            self.sums.push(Sum {
                path: (side.report)(&file.path),
                name: file.name,
                size: bytes,
                digest,
            });
        }
        Ok(())
    }

    /// Reads the file at `path` and hashes it off the async thread, a batch at a time, while
    /// the next batch is read; its size as read, and its digest.
    async fn hash_file<V: Vfs>(
        &mut self,
        side: &Endpoint<'_, V, L>,
        path: &V::Path,
    ) -> Result<(u64, Vec<u8>), Stop> {
        self.progress(side, path);
        let mut reader = side.vfs.open_file(path).await.map_err(Stop::Failed)?;
        let mut running = Running::Ready(Box::new(Hasher::new(self.algorithm)));
        let mut batch = Vec::new();
        let mut bytes = 0;
        loop {
            if self.reporter.cancelled() {
                return Err(Stop::Aborted);
            }
            let Some(chunk) = reader.read().await.map_err(Stop::Failed)? else {
                break;
            };
            let length = chunk.len() as u64;
            bytes += length;
            self.bytes_done += length;
            self.bytes_copied += length;
            if batch.is_empty() && chunk.len() >= BATCH {
                batch = chunk;
            } else {
                batch.reserve(BATCH.saturating_sub(batch.len()));
                batch.extend_from_slice(&chunk);
            }
            if batch.len() >= BATCH {
                let data = mem::take(&mut batch);
                let mut hasher = running.ready().await.map_err(|_| Stop::Aborted)?;
                running = Running::Busy(tokio::task::spawn_blocking(move || {
                    hasher.update(&data);
                    hasher
                }));
            }
            self.progress(side, path);
        }
        let mut hasher = running.ready().await.map_err(|_| Stop::Aborted)?;
        let digest = tokio::task::spawn_blocking(move || {
            hasher.update(&batch);
            hasher.finalize()
        })
        .await
        .map_err(|_| Stop::Aborted)?;
        Ok((bytes, digest))
    }

    fn progress<V: Vfs>(&self, side: &Endpoint<'_, V, L>, current: &V::Path) {
        self.reporter.report(Event::Progress(Progress {
            current: (side.report)(current),
            items_done: self.outcome.done + self.outcome.skipped,
            items_total: self.items_total,
            bytes_done: self.bytes_done,
            bytes_total: self.bytes_total,
            bytes_copied: self.bytes_copied,
        }));
    }
}

/// Hashes the files under `targets` on one endpoint: [`Checksum::scan`] then
/// [`Checksum::hash`].
pub async fn checksum<V: Vfs, L>(
    side: Endpoint<'_, V, L>,
    targets: Vec<V::Path>,
    algorithm: Algorithm,
    reporter: &mut Reporter<L>,
) -> (Outcome, Vec<Sum<L>>) {
    let mut job = Checksum::new(algorithm, reporter);
    let files = job.scan(&side, targets).await;
    job.hash(&side, files).await;
    job.finish()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use noc_vfs::{LocalFs, RemotePath};
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::testing;

    fn report<P: VfsPath>(path: &P) -> String {
        path.display()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
    }

    fn digest(algorithm: Algorithm, data: &[u8]) -> Vec<u8> {
        let mut hasher = Hasher::new(algorithm);
        hasher.update(data);
        hasher.finalize()
    }

    /// What a job reported.
    #[derive(Debug, Default)]
    struct Seen {
        failed: Vec<String>,
        progress: Vec<Progress<String>>,
        scanned: u64,
    }

    /// Takes the reports of a job and answers its failures with `answers` in turn.
    async fn listen(
        mut incoming: mpsc::UnboundedReceiver<Event<String>>,
        answers: &[Decision],
    ) -> Seen {
        let mut seen = Seen::default();
        let mut answers = answers.iter();
        while let Some(event) = incoming.recv().await {
            match event {
                Event::Scanning { items } => {
                    assert!(items >= seen.scanned, "it never goes back");
                    seen.scanned = items;
                }
                Event::Progress(progress) => {
                    if let Some(last) = seen.progress.last() {
                        assert!(
                            progress.bytes_copied >= last.bytes_copied,
                            "it never goes back"
                        );
                    }
                    assert!(progress.bytes_done <= progress.bytes_total, "{progress:?}");
                    seen.progress.push(progress);
                }
                Event::Failed { path, reply, .. } => {
                    seen.failed.push(path);
                    let answer = *answers.next().expect("an answer for each failure");
                    let _ = reply.send(answer);
                }
                Event::Exists { .. } => panic!("checksums take no names"),
            }
        }
        seen
    }

    /// Hashes `targets` on `vfs` with SHA-256, answering failures with `answers`.
    async fn run<V: Vfs>(
        vfs: &V,
        targets: Vec<V::Path>,
        answers: &[Decision],
        cancel: CancellationToken,
    ) -> (Outcome, Vec<Sum<String>>, Seen) {
        let (events, incoming) = mpsc::unbounded_channel();
        let mut reporter = Reporter::new(events, cancel);
        let job = async {
            let side = Endpoint {
                vfs,
                report: &report::<V::Path>,
            };
            let result = checksum(side, targets, Algorithm::Sha256, &mut reporter).await;
            drop(reporter);
            result
        };
        let ((outcome, sums), seen) = tokio::join!(job, listen(incoming, answers));
        (outcome, sums, seen)
    }

    fn names(sums: &[Sum<String>]) -> Vec<String> {
        sums.iter()
            .map(|sum| String::from_utf8_lossy(&sum.name).into_owned())
            .collect()
    }

    /// `dir/` with `b`, `a/x`, `a/sub/y`, a link to `a/x`, a link to `a`, a dangling link,
    /// and a FIFO; and `file` next to it.
    fn tree(root: &Path) -> PathBuf {
        let dir = root.join("dir");
        fs::create_dir_all(dir.join("a/sub")).unwrap();
        fs::write(dir.join("b"), "bee").unwrap();
        fs::write(dir.join("a/x"), "ex").unwrap();
        fs::write(dir.join("a/sub/y"), "why").unwrap();
        symlink("a/x", dir.join("link-file")).unwrap();
        symlink("a", dir.join("link-dir")).unwrap();
        symlink("missing", dir.join("dangling")).unwrap();
        let status = Command::new("mkfifo")
            .arg(dir.join("fifo"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(root.join("file"), "abc").unwrap();
        dir
    }

    #[test]
    fn known_vectors() {
        let expected = [
            (
                Algorithm::Sha256,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                Algorithm::Sha512,
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
                 2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
            ),
            (Algorithm::Sha1, "a9993e364706816aba3e25717850c26c9cd0d89d"),
            (Algorithm::Md5, "900150983cd24fb0d6963f7d28e17f72"),
            (
                Algorithm::Blake3,
                "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
            ),
        ];
        assert_eq!(expected.map(|(algorithm, _)| algorithm), Algorithm::ALL);
        for (algorithm, hash) in expected {
            let digest = digest(algorithm, b"abc");
            assert_eq!(hex(&digest), hash, "{algorithm:?}");
            assert_eq!(digest.len(), algorithm.digest_len(), "{algorithm:?}");
        }
    }

    #[test]
    fn chunks_hash_like_the_whole() {
        let data: Vec<u8> = (0..3 * 1024 * 1024 + 17)
            .map(|i: u32| (i % 251) as u8)
            .collect();
        for algorithm in Algorithm::ALL {
            let mut hasher = Hasher::new(algorithm);
            for chunk in data.chunks(32 * 1024 + 7) {
                hasher.update(chunk);
            }
            assert_eq!(hasher.finalize(), digest(algorithm, &data), "{algorithm:?}");
        }
    }

    #[tokio::test]
    async fn hashes_files_in_order_and_leaves_out_the_rest() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let targets = vec![dir.clone(), root.path().join("file")];
        let (outcome, sums, seen) = run(&LocalFs, targets, &[], CancellationToken::new()).await;
        assert_eq!(seen.failed, Vec::<String>::new());
        assert_eq!(
            names(&sums),
            ["dir/a/sub/y", "dir/a/x", "dir/b", "dir/link-file", "file"]
        );
        assert_eq!(
            outcome,
            Outcome {
                done: 5,
                skipped: 0,
                aborted: false
            }
        );
        let contents = ["why", "ex", "bee", "ex", "abc"];
        for (sum, contents) in sums.iter().zip(contents) {
            assert_eq!(sum.size, contents.len() as u64);
            let expected = digest(Algorithm::Sha256, contents.as_bytes());
            assert_eq!(sum.digest.as_deref(), Some(&expected[..]));
        }
        assert_eq!(sums[0].path, report(&dir.join("a/sub/y")));
        assert_eq!(seen.scanned, 5);
        let last = seen.progress.last().unwrap();
        assert!(
            seen.progress
                .iter()
                .all(|progress| progress.items_total == 5 && progress.bytes_total == 13)
        );
        assert_eq!((last.bytes_done, last.bytes_copied), (13, 13));
    }

    #[tokio::test]
    async fn skips_retries_and_aborts_unreadable_files() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let locked = dir.join("b");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&locked).is_ok() {
            // Root reads it anyway.
            return;
        }
        let failed = report(&locked);

        let (outcome, sums, seen) = run(
            &LocalFs,
            vec![dir.clone()],
            &[Decision::Skip],
            CancellationToken::new(),
        )
        .await;
        assert_eq!(seen.failed, std::slice::from_ref(&failed));
        assert_eq!(
            outcome,
            Outcome {
                done: 3,
                skipped: 1,
                aborted: false
            }
        );
        assert_eq!(sums.len(), 4);
        assert_eq!((sums[2].size, sums[2].digest.clone()), (3, None));
        let last = seen.progress.last().unwrap();
        assert_eq!((last.bytes_done, last.bytes_total), (10, 10));
        assert_eq!(last.bytes_copied, 7, "the skipped file was never read");

        let answers = [Decision::Retry, Decision::Skip];
        let (outcome, _, seen) = run(
            &LocalFs,
            vec![dir.clone()],
            &answers,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(seen.failed, [failed.clone(), failed]);
        assert_eq!((outcome.done, outcome.skipped), (3, 1));

        let (outcome, sums, _) = run(
            &LocalFs,
            vec![dir.clone()],
            &[Decision::Abort],
            CancellationToken::new(),
        )
        .await;
        assert!(outcome.aborted);
        assert_eq!(names(&sums), ["dir/a/sub/y", "dir/a/x"], "stopped there");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[tokio::test]
    async fn an_unreadable_directory_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let locked = dir.join("a");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&locked).is_ok() {
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let (outcome, sums, seen) = run(
            &LocalFs,
            vec![dir.clone()],
            &[Decision::Skip],
            CancellationToken::new(),
        )
        .await;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        // The link to `a/x` cannot be followed either, and goes without a word.
        assert_eq!(seen.failed, [report(&locked)]);
        assert_eq!(names(&sums), ["dir/b"]);
        assert_eq!(
            outcome,
            Outcome {
                done: 1,
                skipped: 1,
                aborted: false
            }
        );
        assert!(
            seen.progress
                .iter()
                .all(|progress| progress.items_total == 2)
        );
    }

    #[test]
    fn jobs_can_run_on_other_threads() {
        fn send<T: Send>(_: T) {}
        let (events, _incoming) = mpsc::unbounded_channel();
        let mut reporter = Reporter::<String>::new(events, CancellationToken::new());
        let side = Endpoint {
            vfs: &LocalFs,
            report: &report::<PathBuf>,
        };
        send(checksum(side, Vec::new(), Algorithm::Blake3, &mut reporter));
        send(Hasher::new(Algorithm::Blake3));
    }

    #[tokio::test]
    async fn a_missing_target_is_a_failure() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        let (outcome, sums, seen) = run(
            &LocalFs,
            vec![missing.clone()],
            &[Decision::Skip],
            CancellationToken::new(),
        )
        .await;
        assert_eq!(seen.failed, [report(&missing)]);
        assert_eq!(
            outcome,
            Outcome {
                done: 0,
                skipped: 1,
                aborted: false
            }
        );
        assert_eq!(sums, []);
    }

    #[tokio::test]
    async fn cancelling_before_the_start_hashes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (outcome, sums, seen) = run(&LocalFs, vec![dir], &[], cancel).await;
        assert_eq!(
            outcome,
            Outcome {
                done: 0,
                skipped: 0,
                aborted: true
            }
        );
        assert_eq!(sums, []);
        assert_eq!(seen.progress, Vec::new());
    }

    #[tokio::test]
    async fn hashes_large_files_in_batches() {
        let root = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..5 * 1024 * 1024 + 3)
            .map(|i: u32| (i % 253) as u8)
            .collect();
        fs::write(root.path().join("big"), &data).unwrap();
        let (outcome, sums, seen) = run(
            &LocalFs,
            vec![root.path().join("big")],
            &[],
            CancellationToken::new(),
        )
        .await;
        assert_eq!(outcome.done, 1);
        assert_eq!(sums[0].digest, Some(digest(Algorithm::Sha256, &data)));
        assert_eq!(names(&sums), ["big"]);
        assert!(seen.progress.len() > 2, "progress after each chunk");
    }

    #[tokio::test]
    async fn scans_both_sides_before_hashing() {
        let local = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        tree(local.path());
        tree(remote.path());
        fs::write(remote.path().join("file"), "abcd").unwrap();
        let Some((_server, sftp)) = testing::sftp_server(remote.path()).await else {
            return;
        };
        let (events, incoming) = mpsc::unbounded_channel();
        let mut reporter = Reporter::new(events, CancellationToken::new());
        let job = async {
            let left = Endpoint {
                vfs: &LocalFs,
                report: &report::<PathBuf>,
            };
            let right = Endpoint {
                vfs: &sftp,
                report: &report::<RemotePath>,
            };
            let mut job = Checksum::new(Algorithm::Md5, &mut reporter);
            let left_files = job.scan(&left, vec![local.path().join("file")]).await;
            let right_files = job.scan(&right, vec![RemotePath::from("file")]).await;
            job.hash(&left, left_files).await;
            job.hash(&right, right_files).await;
            let result = job.finish();
            drop(reporter);
            result
        };
        let ((outcome, sums), seen) = tokio::join!(job, listen(incoming, &[]));
        assert_eq!((outcome.done, outcome.aborted), (2, false));
        assert_eq!(names(&sums), ["file", "file"]);
        assert_eq!(sums[1].path, "file");
        assert_eq!(sums[0].digest, Some(digest(Algorithm::Md5, b"abc")));
        assert_eq!(sums[1].digest, Some(digest(Algorithm::Md5, b"abcd")));
        assert!(
            seen.progress
                .iter()
                .all(|progress| progress.items_total == 2 && progress.bytes_total == 7)
        );
        sftp.close().await.unwrap();
    }

    #[tokio::test]
    async fn hashes_remote_trees() {
        let root = tempfile::tempdir().unwrap();
        tree(root.path());
        let data: Vec<u8> = (0..700 * 1024).map(|i: u32| (i % 241) as u8).collect();
        fs::write(root.path().join("dir/a/big"), &data).unwrap();
        let Some((_server, sftp)) = testing::sftp_server(root.path()).await else {
            return;
        };
        let targets = vec![RemotePath::from("dir")];
        let (outcome, sums, seen) = run(&sftp, targets, &[], CancellationToken::new()).await;
        assert_eq!(seen.failed, Vec::<String>::new());
        assert_eq!(outcome.done, 5);
        assert_eq!(
            names(&sums),
            [
                "dir/a/big",
                "dir/a/sub/y",
                "dir/a/x",
                "dir/b",
                "dir/link-file"
            ]
        );
        assert_eq!(sums[0].digest, Some(digest(Algorithm::Sha256, &data)));
        assert_eq!(sums[4].digest, Some(digest(Algorithm::Sha256, b"ex")));
        assert_eq!(sums[1].path, "dir/a/sub/y");
        sftp.close().await.unwrap();
    }
}
