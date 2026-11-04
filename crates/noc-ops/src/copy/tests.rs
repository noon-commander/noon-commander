use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use noc_vfs::{LocalFs, RemotePath, VfsPath};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::job::Resolution;
use crate::testing;

fn report<P: VfsPath>(path: &P) -> String {
    path.display()
}

/// `this`, and the same for every later taken name.
fn all(this: Choice) -> Conflict {
    Conflict {
        this,
        rest: Rest::Same,
    }
}

/// Renames to `path`, and asks about the next.
fn rename_to(path: &Path) -> Conflict {
    Conflict::once(Choice::Rename(path.as_os_str().as_bytes().to_vec()))
}

/// What a copy reported.
#[derive(Debug, Default)]
struct Seen {
    failed: Vec<String>,
    /// The step of each failure.
    steps: Vec<Step>,
    /// `bytes_copied` as each failure came.
    copied_at_failures: Vec<u64>,
    bytes: Vec<(u64, u64)>,
    /// The largest `bytes_copied` reported.
    copied: u64,
    /// Each taken name: the target, and the sizes of the source and the target.
    exists: Vec<(String, Option<u64>, Option<u64>)>,
    /// The free name each question offered.
    free: Vec<String>,
    /// Each report of counting: how many reports of progress came before it, and how many
    /// entries it had found.
    scanned: Vec<(usize, u64)>,
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

/// Copies within `vfs`, with the answers of `script`.
async fn run_within<V: Vfs>(
    vfs: &V,
    sources: Vec<V::Path>,
    target: V::Path,
    options: CopyOptions,
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
        let outcome = copy_within(side, sources, target, options, &mut reporter).await;
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
            Event::Scanning { items } => seen.scanned.push((seen.bytes.len(), items)),
            Event::Progress(progress) => {
                seen.bytes.push((progress.bytes_done, progress.bytes_total));
                assert!(progress.bytes_done <= progress.bytes_total, "{progress:?}");
                assert!(progress.bytes_copied >= seen.copied, "it never goes back");
                seen.copied = progress.bytes_copied;
                if script.cancel_midway && progress.bytes_done > 0 {
                    cancel.cancel();
                }
            }
            Event::Failed {
                path, step, reply, ..
            } => {
                seen.failed.push(path);
                seen.steps.push(step);
                seen.copied_at_failures.push(seen.copied);
                let answer = *failures.next().expect("an answer for each failure");
                let _ = reply.send(Resolution::once(answer));
            }
            Event::Exists {
                target,
                source_metadata,
                target_metadata,
                free,
                reply,
                ..
            } => {
                let sizes = (source_metadata.size, target_metadata.size);
                seen.exists.push((target, sizes.0, sizes.1));
                seen.free.push(String::from_utf8(free).unwrap());
                let answer = conflicts
                    .next()
                    .expect("an answer for each taken name")
                    .clone();
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
    overwrite: false,
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
        if path.to_string_lossy().contains(".noc-") {
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
    assert_eq!(seen.copied, total);
}

#[tokio::test]
async fn an_aborted_copy_keeps_the_attributes_of_the_directories_it_finished() {
    let root = tempfile::tempdir().unwrap();
    let src = sources(root.path());
    let dst = root.path().join("dst");
    // A file does not replace a directory: Abort, after `dir` is copied.
    fs::create_dir_all(dst.join("file")).unwrap();
    let sources = vec![src.join("dir"), src.join("file")];
    let (outcome, seen) = run(
        (&LocalFs, sources),
        (&LocalFs, dst.clone()),
        BOTH,
        &[Decision::Abort],
        false,
    )
    .await;
    assert!(outcome.aborted);
    assert_eq!(seen.failed, [report(&dst.join("file"))]);
    assert_eq!(seen.steps, [Step::Create]);
    check_dir(&dst.join("dir"));
}

#[tokio::test]
async fn a_move_finishes_each_directory_once_all_in_it_has_moved() {
    let root = tempfile::tempdir().unwrap();
    let src = sources(root.path());
    let dst = root.path().join("dst");
    fs::create_dir(&dst).unwrap();
    let (events, mut incoming) = mpsc::unbounded_channel();
    let mut reporter = Reporter::new(events, CancellationToken::new());
    let job = async {
        let side = Endpoint {
            vfs: &OtherDevice::default(),
            report: &report::<PathBuf>,
        };
        let moving = vec![src.join("dir"), src.join("file")];
        let outcome = move_within(side, moving, dst.clone(), BOTH, &mut reporter).await;
        drop(reporter);
        outcome
    };
    // As `file` starts to be copied, after the renames, `dir` is done with.
    let watch = async {
        let mut found = None;
        while let Some(event) = incoming.recv().await {
            if let Event::Progress(progress) = event
                && progress.bytes_total > 0
                && progress.current.ends_with("/file")
                && found.is_none()
            {
                found = Some((src.join("dir").exists(), modified(&dst.join("dir"))));
            }
        }
        found
    };
    let (outcome, found) = tokio::join!(job, watch);
    assert_eq!((outcome.done, outcome.aborted), (7, false));
    assert_eq!(found, Some((false, at(5_000))), "removed, with its time");
    check_dir(&dst.join("dir"));
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
        overwrite: false,
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
    let answers = [
        Conflict::once(Choice::Overwrite),
        Conflict::once(Choice::Skip),
    ];
    let (outcome, seen, dir) = copy_taken(root.path(), &answers).await;
    let a = dir.join("a").to_string_lossy().into_owned();
    assert_eq!(seen.exists[0], (a, Some(10), Some(5)), "with both sizes");
    assert_eq!(seen.exists.len(), 2);
    assert_eq!(fs::read_to_string(dir.join("a")).unwrap(), "0123456789");
    assert_eq!(fs::read_to_string(dir.join("sub/b")).unwrap(), "newer");
    // dir and sub (merged), a, link, and dangling; b stays.
    assert_eq!((outcome.done, outcome.skipped), (5, 1));
    assert_eq!(leftovers(&dir), Vec::<PathBuf>::new());
    assert_eq!(seen.copied, 10, "only a moved data");
}

#[tokio::test]
async fn answers_for_all_ask_once() {
    let root = tempfile::tempdir().unwrap();
    let (_, seen, dir) = copy_taken(root.path(), &[all(Choice::Overwrite)]).await;
    assert_eq!(seen.exists.len(), 1);
    assert_eq!(
        fs::read_to_string(dir.join("sub/b")).unwrap().len(),
        100_000
    );

    let root = tempfile::tempdir().unwrap();
    let (_, seen, dir) = copy_taken(root.path(), &[all(Choice::Skip)]).await;
    assert_eq!(seen.exists.len(), 1);
    assert_eq!(fs::read_to_string(dir.join("a")).unwrap(), "older");
    assert_eq!(fs::read_to_string(dir.join("sub/b")).unwrap(), "newer");

    // The sources go in the order given, but what a directory holds goes in the order of its
    // listing, which differs between file systems: tmpfs lists the newest first.
    let root = tempfile::tempdir().unwrap();
    let (src, dst) = taken(root.path());
    let script = Script {
        conflicts: &[Conflict::once(Choice::Abort)],
        ..Script::default()
    };
    let sources = vec![src.join("dir/a"), src.join("dir/link")];
    let dir = dst.join("dir");
    let (outcome, seen) =
        run_script((&LocalFs, sources), (&LocalFs, dir.clone()), BOTH, script).await;
    assert!(outcome.aborted);
    assert_eq!(seen.exists.len(), 1);
    assert_eq!(fs::read_to_string(dir.join("a")).unwrap(), "older");
    assert!(!dir.join("link").exists(), "stopped there");
}

/// `src` and `dst` in `root`, each with `files`: a name, and what the file holds there.
fn both_sides(root: &Path, files: &[(&str, &str, &str)]) -> (PathBuf, PathBuf) {
    let (src, dst) = (root.join("src"), root.join("dst"));
    fs::create_dir(&src).unwrap();
    fs::create_dir(&dst).unwrap();
    for (name, source, target) in files {
        fs::write(src.join(name), source).unwrap();
        if !target.is_empty() {
            fs::write(dst.join(name), target).unwrap();
        }
    }
    (src, dst)
}

/// Copies `asked`, then files whose targets are older, newer, smaller, and larger than their
/// sources, into a directory that has them all; skips the first, with `rest` for the others.
/// The names of those replaced.
async fn replaced_by(rest: Rest) -> Vec<&'static str> {
    let root = tempfile::tempdir().unwrap();
    let files = [
        ("asked", "s", "t", 1, 1),
        ("older", "s", "t", 2, 1),
        ("newer", "s", "t", 1, 2),
        ("smaller", "ss", "t", 1, 1),
        ("larger", "s", "tt", 1, 1),
    ];
    let contents: Vec<_> = files.iter().map(|file| (file.0, file.1, file.2)).collect();
    let (src, dst) = both_sides(root.path(), &contents);
    for (name, _, _, source, target) in files {
        set_modified(&src.join(name), at(source * 1_000));
        set_modified(&dst.join(name), at(target * 1_000));
    }
    let script = Script {
        conflicts: &[Conflict {
            this: Choice::Skip,
            rest,
        }],
        ..Script::default()
    };
    let sources = files.iter().map(|file| src.join(file.0)).collect();
    let (_, seen) = run_script((&LocalFs, sources), (&LocalFs, dst.clone()), BOTH, script).await;
    assert_eq!(seen.exists.len(), 1, "{rest:?}");
    files
        .iter()
        .map(|file| file.0)
        .filter(|name| fs::read_to_string(dst.join(name)).unwrap().starts_with('s'))
        .collect()
}

#[tokio::test]
async fn the_rest_are_replaced_by_their_times_or_sizes() {
    assert_eq!(replaced_by(Rest::OverwriteOlder).await, ["older"]);
    assert_eq!(replaced_by(Rest::OverwriteSmaller).await, ["smaller"]);
    assert_eq!(replaced_by(Rest::OverwriteLarger).await, ["larger"]);
    let differ = replaced_by(Rest::OverwriteSizeDiffers).await;
    assert_eq!(differ, ["smaller", "larger"]);
    assert_eq!(replaced_by(Rest::Same).await, Vec::<&str>::new());
}

#[tokio::test]
async fn rename_copies_to_another_name_and_asks_again_if_it_is_taken() {
    let root = tempfile::tempdir().unwrap();
    let files = [("a.txt", "new", "old"), ("a_2.txt", "", "taken")];
    let (src, dst) = both_sides(root.path(), &files);
    let script = Script {
        conflicts: &[
            rename_to(&dst.join("a_2.txt")),
            rename_to(&dst.join("b.txt")),
        ],
        ..Script::default()
    };
    let sources = vec![src.join("a.txt")];
    let (outcome, seen) =
        run_script((&LocalFs, sources), (&LocalFs, dst.clone()), BOTH, script).await;
    assert_eq!((outcome.done, outcome.skipped), (1, 0));
    let asked: Vec<_> = seen.exists.iter().map(|asked| asked.0.as_str()).collect();
    let (a, a_2) = (dst.join("a.txt"), dst.join("a_2.txt"));
    assert_eq!(asked, [a.to_str().unwrap(), a_2.to_str().unwrap()]);
    assert_eq!(seen.free, ["a_3.txt", "a_2_2.txt"], "free next to each");
    assert_eq!(fs::read_to_string(a).unwrap(), "old");
    assert_eq!(fs::read_to_string(a_2).unwrap(), "taken");
    assert_eq!(fs::read_to_string(dst.join("b.txt")).unwrap(), "new");
    assert_eq!(leftovers(&dst), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn renaming_for_all_gives_the_rest_free_names_where_the_first_went() {
    let root = tempfile::tempdir().unwrap();
    let files = [("a", "new", "old"), ("b", "new", "old"), ("c", "new", "")];
    let (src, dst) = both_sides(root.path(), &files);
    let other = dst.join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("b"), "other").unwrap();
    let path = other.join("a").as_os_str().as_bytes().to_vec();
    let script = Script {
        conflicts: &[all(Choice::Rename(path))],
        ..Script::default()
    };
    let sources = ["a", "b", "c"].map(|name| src.join(name)).to_vec();
    let (outcome, seen) =
        run_script((&LocalFs, sources), (&LocalFs, dst.clone()), BOTH, script).await;
    assert_eq!((outcome.done, outcome.skipped), (3, 0));
    assert_eq!(seen.exists.len(), 1);
    let read = |path: PathBuf| fs::read_to_string(path).unwrap();
    assert_eq!(read(other.join("a")), "new");
    assert_eq!(read(other.join("b")), "other");
    assert_eq!(read(other.join("b_2")), "new");
    assert_eq!(
        (read(dst.join("a")), read(dst.join("b"))),
        ("old".into(), "old".into())
    );
    assert_eq!(read(dst.join("c")), "new", "a free name stays");
}

#[tokio::test]
async fn a_move_renames_to_another_name() {
    let root = tempfile::tempdir().unwrap();
    let (src, dst) = both_sides(root.path(), &[("a", "new", "old")]);
    let script = Script {
        conflicts: &[rename_to(&dst.join("a_2"))],
        ..Script::default()
    };
    let (outcome, seen) = run_move(&LocalFs, vec![src.join("a")], dst.clone(), script).await;
    assert_eq!(outcome.done, 1);
    assert_eq!(seen.free, ["a_2"]);
    assert!(!src.join("a").exists());
    assert_eq!(fs::read_to_string(dst.join("a")).unwrap(), "old");
    assert_eq!(fs::read_to_string(dst.join("a_2")).unwrap(), "new");
}

#[tokio::test]
async fn a_move_that_copies_across_devices_asks_once() {
    for (rename, name, there) in [(true, "a_2", "old"), (false, "a", "new")] {
        let root = tempfile::tempdir().unwrap();
        let (src, dst) = both_sides(root.path(), &[("a", "new", "old")]);
        let answer = if rename {
            rename_to(&dst.join("a_2"))
        } else {
            Conflict::once(Choice::Overwrite)
        };
        let script = Script {
            conflicts: &[answer],
            ..Script::default()
        };
        let device = OtherDevice::default();
        let (outcome, seen) = run_move(&device, vec![src.join("a")], dst.clone(), script).await;
        assert_eq!(outcome.done, 1, "{seen:?}");
        assert_eq!(seen.exists.len(), 1, "the copy asks no more");
        assert!(!src.join("a").exists());
        assert_eq!(fs::read_to_string(dst.join(name)).unwrap(), "new");
        assert_eq!(fs::read_to_string(dst.join("a")).unwrap(), there);
    }
}

#[tokio::test]
async fn rename_copies_to_another_name_on_a_host() {
    let root = tempfile::tempdir().unwrap();
    let (src, dst) = both_sides(root.path(), &[("My Report.pdf", "new", "old")]);
    let Some((_server, sftp)) = testing::sftp_server(root.path()).await else {
        return;
    };
    let script = Script {
        conflicts: &[Conflict::once(Choice::Rename(
            b"dst/My Report 2.pdf".to_vec(),
        ))],
        ..Script::default()
    };
    let sources = vec![src.join("My Report.pdf")];
    let target = (&sftp, RemotePath::from("dst"));
    let (outcome, seen) = run_script((&LocalFs, sources), target, BOTH, script).await;
    assert_eq!(outcome.done, 1, "{seen:?}");
    assert_eq!(seen.free, ["My Report 2.pdf"]);
    let read = |name| fs::read_to_string(dst.join(name)).unwrap();
    assert_eq!(
        (read("My Report.pdf"), read("My Report 2.pdf")),
        ("old".into(), "new".into())
    );
    sftp.close().await.unwrap();
}

#[tokio::test]
async fn overwrite_replaces_without_asking() {
    let root = tempfile::tempdir().unwrap();
    let (src, dst) = taken(root.path());
    let options = CopyOptions {
        overwrite: true,
        ..BOTH
    };
    let sources = vec![src.join("dir/a")];
    let target = dst.join("dir/a");
    let (outcome, seen) = run_script(
        (&LocalFs, sources),
        (&LocalFs, target.clone()),
        options,
        Script::default(),
    )
    .await;
    assert_eq!(seen.exists.len(), 0);
    assert_eq!((outcome.done, outcome.skipped), (1, 0));
    assert_eq!(fs::read_to_string(&target).unwrap(), "0123456789");
    assert_eq!(leftovers(&dst.join("dir")), Vec::<PathBuf>::new());
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
            overwrite: false,
        };
        let script = Script {
            failures: &[Decision::Skip],
            conflicts: &[Conflict::once(Choice::Overwrite)],
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
            overwrite: false,
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
    assert!(
        seen.failed.contains(&report(&src.join("locked"))),
        "{seen:?}"
    );
    assert!(seen.failed.contains(&report(&src.join("fifo"))), "{seen:?}");
    assert_eq!(seen.steps, [Step::Read, Step::Read], "of the sources");
    assert_eq!(fs::read_to_string(dst.join("ok")).unwrap(), "ok");
    assert!(!dst.join("locked").exists());
    assert!(!dst.join("fifo").exists());
    assert_eq!((outcome.done, outcome.skipped), (2, 2));
    assert_eq!(leftovers(&dst), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn an_unlisted_directory_and_a_locked_target_name_their_steps() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir_all(src.join("hidden")).unwrap();
    fs::write(src.join("file"), "f").unwrap();
    fs::set_permissions(src.join("hidden"), fs::Permissions::from_mode(0o000)).unwrap();
    let dst = root.path().join("dst");
    fs::create_dir(&dst).unwrap();
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o555)).unwrap();
    let unlock = || {
        fs::set_permissions(src.join("hidden"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&dst, fs::Permissions::from_mode(0o755)).unwrap();
    };
    if fs::read_dir(src.join("hidden")).is_ok() {
        // Root reads and writes it anyway.
        unlock();
        return;
    }
    let sources = vec![src.join("hidden"), src.join("file")];
    let answers = [Decision::Skip, Decision::Skip];
    let (outcome, seen) = run(
        (&LocalFs, sources),
        (&LocalFs, dst.clone()),
        BOTH,
        &answers,
        false,
    )
    .await;
    unlock();
    // The directory that could not be listed is skipped before anything goes to the target.
    let failed = [report(&src.join("hidden")), report(&dst.join("file"))];
    assert_eq!(seen.failed, failed, "{seen:?}");
    assert_eq!(seen.steps, [Step::List, Step::Create]);
    assert_eq!((outcome.done, outcome.skipped), (0, 2));
}

#[tokio::test]
async fn what_a_host_took_before_it_refused_counts() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("file");
    fs::write(&src, vec![7; 300_000]).unwrap();
    let remote = root.path().join("remote");
    fs::create_dir(&remote).unwrap();
    // A host that takes 64 KiB of a file, or 32, and refuses the rest.
    let Some((_server, sftp)) = testing::sftp_server_with_file_limit(&remote, 64).await else {
        return;
    };
    let mut probe = sftp.create_file(&"probe".into(), false).await.unwrap();
    probe.write(vec![7; 300_000]).await.unwrap();
    while probe.settle().await.unwrap_or(true) {}
    drop(probe);
    let taken = fs::metadata(remote.join("probe")).unwrap().len();
    fs::remove_file(remote.join("probe")).unwrap();
    assert!(taken > 0 && taken < 300_000, "{taken}");

    let answers = [Decision::Retry, Decision::Skip];
    let target = (&sftp, RemotePath::from("file"));
    let (outcome, seen) = run((&LocalFs, vec![src]), target, BOTH, &answers, false).await;
    assert_eq!(outcome.skipped, 1);
    // Written under a temporary name, the file failed under its own.
    assert_eq!(seen.failed, ["file", "file"]);
    assert_eq!(seen.steps, [Step::Write, Step::Write]);
    assert_eq!(seen.copied_at_failures, [taken, 2 * taken], "{seen:?}");
    assert_eq!(seen.copied, 2 * taken);
    assert_eq!(leftovers(&remote), Vec::<PathBuf>::new());
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
    assert_eq!(seen.copied, 100_011, "all of it, as the host took it");
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
        conflicts: &[all(Choice::Overwrite)],
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
        conflicts: &[
            Conflict::once(Choice::Skip),
            Conflict::once(Choice::Overwrite),
        ],
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
        conflicts: &[
            Conflict::once(Choice::Overwrite),
            Conflict::once(Choice::Skip),
            Conflict::once(Choice::Overwrite),
        ],
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

/// The local file system, where every rename crosses file systems, and what is written
/// arrives late, as on a host; or, with a `stall`, as on a host that stops answering.
#[derive(Default)]
struct OtherDevice {
    stall: Option<Stall>,
}

/// Where a host stops answering, and the job it cancels as it does.
#[derive(Debug, Clone)]
struct Stall {
    at: Stage,
    cancel: CancellationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Create,
    Read,
    Write,
    Settle,
    Finish,
    Attributes,
}

/// Never ends at the `stage` where `stall` stops answering, once it has cancelled its job.
async fn stall_at(stall: Option<&Stall>, stage: Stage) {
    if let Some(stall) = stall.filter(|stall| stall.at == stage) {
        stall.cancel.cancel();
        std::future::pending::<()>().await;
    }
}

/// A local file whose writes arrive only as the job waits for them, 64 KiB at a time.
#[derive(Debug)]
struct Late {
    writer: noc_vfs::LocalWriter,
    arrived: u64,
    stall: Option<Stall>,
}

impl noc_vfs::FileWriter for Late {
    async fn write(&mut self, data: Vec<u8>) -> Result<(), VfsError> {
        if self.writer.written() > 0 {
            stall_at(self.stall.as_ref(), Stage::Write).await;
        }
        self.writer.write(data).await
    }

    fn written(&self) -> u64 {
        self.arrived
    }

    async fn settle(&mut self) -> Result<bool, VfsError> {
        stall_at(self.stall.as_ref(), Stage::Settle).await;
        // As a server's answer, it comes after others have had their turn.
        tokio::task::yield_now().await;
        let left = self.writer.written() - self.arrived;
        self.arrived += left.min(64 * 1024);
        Ok(left > 0)
    }

    async fn finish(self) -> Result<(), VfsError> {
        stall_at(self.stall.as_ref(), Stage::Finish).await;
        self.writer.finish().await
    }
}

/// A local file read as from a host, whose answers may stop after the first.
#[derive(Debug)]
struct Remote {
    reader: noc_vfs::LocalReader,
    read: bool,
    stall: Option<Stall>,
}

impl noc_vfs::FileReader for Remote {
    async fn read(&mut self) -> Result<Option<Vec<u8>>, VfsError> {
        if self.read {
            stall_at(self.stall.as_ref(), Stage::Read).await;
        }
        self.read = true;
        self.reader.read().await
    }
}

impl Vfs for OtherDevice {
    type Path = PathBuf;
    type Reader = Remote;
    type Writer = Late;

    async fn list_dir(&self, path: &PathBuf) -> Result<Vec<noc_vfs::DirEntry>, VfsError> {
        LocalFs.list_dir(path).await
    }
    async fn metadata(&self, path: &PathBuf) -> Result<Metadata, VfsError> {
        LocalFs.metadata(path).await
    }
    async fn symlink_metadata(&self, path: &PathBuf) -> Result<Metadata, VfsError> {
        LocalFs.symlink_metadata(path).await
    }
    async fn space(&self, path: &PathBuf) -> Result<Option<noc_vfs::Space>, VfsError> {
        LocalFs.space(path).await
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
        if from.to_string_lossy().contains(".noc-") {
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
        stall_at(self.stall.as_ref(), Stage::Attributes).await;
        LocalFs.set_permissions(path, mode).await
    }
    async fn set_modified(&self, path: &PathBuf, time: SystemTime) -> Result<(), VfsError> {
        LocalFs.set_modified(path, time).await
    }
    async fn open_file_at(&self, path: &PathBuf, offset: u64) -> Result<Self::Reader, VfsError> {
        Ok(Remote {
            reader: LocalFs.open_file_at(path, offset).await?,
            read: false,
            stall: self.stall.clone(),
        })
    }
    async fn create_file(&self, path: &PathBuf, replace: bool) -> Result<Self::Writer, VfsError> {
        // Made, as a host makes it once asked, whether or not its answer comes.
        let writer = LocalFs.create_file(path, replace).await?;
        stall_at(self.stall.as_ref(), Stage::Create).await;
        Ok(Late {
            writer,
            arrived: 0,
            stall: self.stall.clone(),
        })
    }
}

/// The local file system as a host whose connection is lost while the first file is
/// written, and back once the job has tried to clean up: the write fails, and so does the
/// removal of what it left.
#[derive(Debug, Default)]
struct Lost {
    /// Whether the connection was lost already, and whether it is lost now.
    state: std::sync::Arc<std::sync::Mutex<(bool, bool)>>,
}

/// A local file whose first write loses the connection, if `lost` is the state of one
/// still there.
#[derive(Debug)]
struct Cut {
    writer: noc_vfs::LocalWriter,
    lost: Option<std::sync::Arc<std::sync::Mutex<(bool, bool)>>>,
}

impl noc_vfs::FileWriter for Cut {
    async fn write(&mut self, data: Vec<u8>) -> Result<(), VfsError> {
        self.writer.write(data).await?;
        match self.lost.take() {
            Some(lost) => {
                *lost.lock().unwrap() = (true, true);
                Err(VfsError::Io(io::ErrorKind::BrokenPipe.into()))
            }
            None => Ok(()),
        }
    }

    fn written(&self) -> u64 {
        self.writer.written()
    }

    async fn settle(&mut self) -> Result<bool, VfsError> {
        self.writer.settle().await
    }

    async fn finish(self) -> Result<(), VfsError> {
        self.writer.finish().await
    }
}

impl Vfs for Lost {
    type Path = PathBuf;
    type Reader = noc_vfs::LocalReader;
    type Writer = Cut;

    async fn list_dir(&self, path: &PathBuf) -> Result<Vec<noc_vfs::DirEntry>, VfsError> {
        LocalFs.list_dir(path).await
    }
    async fn metadata(&self, path: &PathBuf) -> Result<Metadata, VfsError> {
        LocalFs.metadata(path).await
    }
    async fn symlink_metadata(&self, path: &PathBuf) -> Result<Metadata, VfsError> {
        LocalFs.symlink_metadata(path).await
    }
    async fn space(&self, path: &PathBuf) -> Result<Option<noc_vfs::Space>, VfsError> {
        LocalFs.space(path).await
    }
    async fn canonicalize(&self, path: &PathBuf) -> Result<PathBuf, VfsError> {
        LocalFs.canonicalize(path).await
    }
    async fn create_dir(&self, path: &PathBuf) -> Result<(), VfsError> {
        LocalFs.create_dir(path).await
    }
    async fn remove_file(&self, path: &PathBuf) -> Result<(), VfsError> {
        // The first removal after the loss fails, and the connection is back then.
        let down = std::mem::replace(&mut self.state.lock().unwrap().1, false);
        if down {
            return Err(VfsError::Io(io::ErrorKind::BrokenPipe.into()));
        }
        LocalFs.remove_file(path).await
    }
    async fn remove_dir(&self, path: &PathBuf) -> Result<(), VfsError> {
        LocalFs.remove_dir(path).await
    }
    async fn rename(&self, from: &PathBuf, to: &PathBuf) -> Result<(), VfsError> {
        LocalFs.rename(from, to).await
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
    async fn open_file_at(&self, path: &PathBuf, offset: u64) -> Result<Self::Reader, VfsError> {
        LocalFs.open_file_at(path, offset).await
    }
    async fn create_file(&self, path: &PathBuf, replace: bool) -> Result<Self::Writer, VfsError> {
        let writer = LocalFs.create_file(path, replace).await?;
        let lost = !self.state.lock().unwrap().0;
        Ok(Cut {
            writer,
            lost: lost.then(|| std::sync::Arc::clone(&self.state)),
        })
    }
}

#[tokio::test]
async fn a_retry_after_a_lost_connection_replaces_what_the_failed_copy_left() {
    for atomic in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("file");
        fs::write(&src, vec![7; 600_000]).unwrap();
        let dst = root.path().join("dst");
        fs::create_dir(&dst).unwrap();
        let options = CopyOptions { atomic, ..BOTH };
        let from = (&LocalFs, vec![src]);
        let (outcome, seen) = run(
            from,
            (&Lost::default(), dst.clone()),
            options,
            &[Decision::Retry],
            false,
        )
        .await;
        assert_eq!(seen.steps, [Step::Write], "atomic: {atomic}");
        // Without a question about the name the first try took.
        assert!(seen.exists.is_empty(), "atomic: {atomic}");
        assert_eq!(outcome.done, 1, "atomic: {atomic}");
        assert_eq!(fs::read(dst.join("file")).unwrap(), vec![7; 600_000]);
        let names: Vec<_> = fs::read_dir(&dst)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["file"], "nothing else is left, atomic: {atomic}");
    }
}

#[tokio::test]
async fn bytes_count_as_they_arrive() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("file");
    fs::write(&src, vec![7; 300_000]).unwrap();
    let dst = root.path().join("dst");
    fs::create_dir(&dst).unwrap();
    let from = (&LocalFs, vec![src]);
    let (outcome, seen) = run(
        from,
        (&OtherDevice::default(), dst.clone()),
        BOTH,
        &[],
        false,
    )
    .await;
    assert_eq!(outcome.done, 1);
    // Read and written 256 KiB at a time, it arrives 64 KiB at a time once all is written.
    let done: Vec<u64> = seen.bytes.iter().map(|(done, _)| *done).collect();
    assert_eq!(done, [0, 65_536, 131_072, 196_608, 262_144, 300_000]);
    assert_eq!(seen.copied, 300_000);
    assert_eq!(fs::read(dst.join("file")).unwrap(), vec![7; 300_000]);
}

#[tokio::test]
async fn the_total_follows_files_that_changed_since_they_were_counted() {
    // While the copy asks about the taken name of `a`, a file it has counted grows and
    // another shrinks: the total takes what each holds, so that the bytes done never go
    // past it, and come to it.
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir(&src).unwrap();
    let names = ["a", "grows", "shrinks", "z"];
    for (name, size) in names.into_iter().zip([10, 1_000, 200_000, 3]) {
        fs::write(src.join(name), vec![7; size]).unwrap();
    }
    let dst = root.path().join("dst");
    fs::create_dir(&dst).unwrap();
    fs::write(dst.join("a"), "taken").unwrap();
    let (events, mut incoming) = mpsc::unbounded_channel();
    let mut reporter = Reporter::new(events, CancellationToken::new());
    let job = async {
        let side = Endpoint {
            vfs: &LocalFs,
            report: &report::<PathBuf>,
        };
        let sources = names.map(|name| src.join(name)).to_vec();
        let outcome = copy_within(side, sources, dst.clone(), BOTH, &mut reporter).await;
        drop(reporter);
        outcome
    };
    let watch = async {
        let mut bytes = Vec::new();
        while let Some(event) = incoming.recv().await {
            match event {
                Event::Progress(progress) => {
                    bytes.push((progress.bytes_done, progress.bytes_total));
                }
                Event::Exists { reply, .. } => {
                    fs::write(src.join("grows"), vec![7; 300_000]).unwrap();
                    fs::write(src.join("shrinks"), vec![7; 5]).unwrap();
                    let _ = reply.send(Conflict::once(Choice::Overwrite));
                }
                Event::Scanning { .. } => {}
                Event::Failed { path, .. } => panic!("{path} failed"),
            }
        }
        bytes
    };
    let (outcome, bytes) = tokio::join!(job, watch);
    assert_eq!((outcome.done, outcome.aborted), (4, false));
    assert_eq!(bytes.first(), Some(&(0, 201_013)), "as counted");
    assert!(bytes.iter().all(|(done, total)| done <= total), "{bytes:?}");
    // As `z` starts, then once it is done.
    let end = &bytes[bytes.len() - 2..];
    assert_eq!(end, [(300_015, 300_018), (300_018, 300_018)]);
    assert_eq!(fs::read(dst.join("grows")).unwrap().len(), 300_000);
}

/// Copies `source` to `target` with `cancel`, within the while a cancelled job has.
async fn cancelled_copy<A, B>(
    (from, source): (&A, PathBuf),
    (to, target): (&B, PathBuf),
    cancel: &CancellationToken,
) -> Option<Outcome>
where
    A: Vfs<Path = PathBuf>,
    B: Vfs<Path = PathBuf>,
{
    let (events, incoming) = mpsc::unbounded_channel();
    let mut reporter = Reporter::new(events, cancel.clone());
    let job = async {
        let from = Endpoint {
            vfs: from,
            report: &report::<PathBuf>,
        };
        let to = Endpoint {
            vfs: to,
            report: &report::<PathBuf>,
        };
        let outcome = copy(from, vec![source], to, target, BOTH, &mut reporter).await;
        drop(reporter);
        outcome
    };
    let job = crate::in_time(cancel, job);
    let (ended, _) = tokio::join!(job, listen(incoming, cancel, Script::default()));
    ended
}

#[tokio::test]
async fn a_cancel_stops_each_wait_on_a_host_and_takes_away_the_copy() {
    use Stage::{Attributes, Create, Finish, Read, Settle, Write};
    for at in [Create, Read, Write, Settle, Finish, Attributes] {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("file");
        fs::write(&src, vec![7; 600_000]).unwrap();
        let dst = root.path().join("dst");
        fs::create_dir(&dst).unwrap();
        // The host stops answering there and the copy is cancelled: it ends in time, with
        // no part of the file left, under its name or a temporary one.
        let cancel = CancellationToken::new();
        let stall = Some(Stall {
            at,
            cancel: cancel.clone(),
        });
        let host = OtherDevice { stall };
        let ended = if at == Read {
            cancelled_copy((&host, src), (&LocalFs, dst.clone()), &cancel).await
        } else {
            cancelled_copy((&LocalFs, src), (&host, dst.clone()), &cancel).await
        };
        assert!(
            ended.is_some_and(|outcome| outcome.aborted),
            "{at:?}: {ended:?}"
        );
        let left: Vec<_> = fs::read_dir(&dst).unwrap().collect();
        assert!(left.is_empty(), "{at:?}: {left:?}");
    }
}

#[tokio::test]
async fn a_cancel_stops_the_wait_for_what_is_on_its_way() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("file");
    fs::write(&src, vec![7; 300_000]).unwrap();
    let dst = root.path().join("dst");
    fs::create_dir(&dst).unwrap();
    // Cancelled once the first 64 KiB have arrived, with all of it written.
    let from = (&LocalFs, vec![src]);
    let (outcome, seen) = run(
        from,
        (&OtherDevice::default(), dst.clone()),
        BOTH,
        &[],
        true,
    )
    .await;
    assert!(outcome.aborted);
    assert!(seen.copied < 300_000, "{seen:?}");
    assert!(
        fs::read_dir(&dst).unwrap().next().is_none(),
        "no part of it"
    );
}

#[tokio::test]
async fn moving_across_file_systems_copies_and_removes() {
    let root = tempfile::tempdir().unwrap();
    let src = sources(root.path());
    let dst = root.path().join("dst");
    fs::create_dir(&dst).unwrap();
    let moving = vec![src.join("dir"), src.join("file")];
    let (outcome, seen) = run_move(
        &OtherDevice::default(),
        moving,
        dst.clone(),
        Script::default(),
    )
    .await;
    assert!(seen.failed.is_empty(), "{seen:?}");
    // dir, a, sub, b, link, dangling, and file, copied one by one.
    assert_eq!(outcome.done, 7);
    check_dir(&dst.join("dir"));
    assert_eq!(fs::read_dir(&src).unwrap().count(), 0, "all of it moved");
}

#[tokio::test]
async fn a_move_counts_what_it_could_not_rename_before_it_copies() {
    // Files alone, with no directory to list: the counting is reported all the same, so that
    // the window starts the speed and the time left of the copy over, rather than take the
    // tries to rename for work; and each file counts as it is found, rather than none until
    // the copy, as each takes a round trip on a host.
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("one"), "1").unwrap();
    fs::write(src.join("two"), "22").unwrap();
    let dst = root.path().join("dst");
    fs::create_dir(&dst).unwrap();
    let moving = vec![src.join("one"), src.join("two")];
    let (outcome, seen) = run_move(
        &OtherDevice::default(),
        moving,
        dst.clone(),
        Script::default(),
    )
    .await;
    assert_eq!(outcome.done, 2, "{seen:?}");
    // A report as it tries each rename, with no data; then the copy, with its own.
    let copying = seen.bytes.iter().position(|&(_, total)| total > 0);
    assert_eq!(copying, Some(2), "{seen:?}");
    assert_eq!(seen.scanned, [(2, 0), (2, 1), (2, 2)], "{seen:?}");
    assert_eq!(fs::read_to_string(dst.join("two")).unwrap(), "22");
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

/// Whether the file system of `dir` ignores case.
fn ignores_case(dir: &Path) -> bool {
    fs::write(dir.join("probe"), "").unwrap();
    let ignores = dir.join("PROBE").exists();
    fs::remove_file(dir.join("probe")).unwrap();
    ignores
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Writing straight into the target, which empties it first: what would lose a source.
const PLAIN: CopyOptions = CopyOptions {
    preserve: false,
    atomic: false,
    remove_sources: false,
    overwrite: false,
};

const ABORT: &[Conflict] = &[Conflict {
    this: Choice::Abort,
    rest: Rest::Ask,
}];

/// Skips each failure; a copy over its source would ask about a taken name, and abort.
fn skipping(failures: &[Decision]) -> Script<'_> {
    Script {
        failures,
        conflicts: ABORT,
        cancel_midway: false,
    }
}

#[tokio::test]
async fn a_copy_does_not_go_over_a_hard_link_of_its_source() {
    let root = tempfile::tempdir().unwrap();
    let src = sources(root.path());
    fs::hard_link(src.join("file"), src.join("hard")).unwrap();
    let script = skipping(&[Decision::Skip]);
    let one = vec![src.join("file")];
    let (outcome, seen) = run_within(&LocalFs, one, src.join("hard"), PLAIN, script).await;
    assert_eq!(
        seen.failed,
        [src.join("hard").to_string_lossy()],
        "{seen:?}"
    );
    assert_eq!(seen.steps, [Step::Create]);
    assert!(seen.exists.is_empty(), "{seen:?}");
    assert_eq!((outcome.done, outcome.skipped), (0, 1));
    assert_eq!(fs::read_to_string(src.join("file")).unwrap(), "f");
}

#[tokio::test]
async fn a_copy_does_not_go_over_its_source_spelled_otherwise() {
    let root = tempfile::tempdir().unwrap();
    if !ignores_case(root.path()) {
        return;
    }
    let src = sources(root.path());
    let upper = root.path().join("SRC");
    // One source to its own name in another case: a file, and a directory, which would go
    // into itself.
    for (name, other) in [("file", "FILE"), ("dir", "Dir")] {
        let script = skipping(&[Decision::Skip]);
        let one = vec![src.join(name)];
        let (outcome, seen) = run_within(&LocalFs, one, src.join(other), PLAIN, script).await;
        assert_eq!(seen.failed.len(), 1, "{name}: {seen:?}");
        assert_eq!((outcome.done, outcome.skipped), (0, 1), "{name}");
    }
    // Into their own directory spelled otherwise: each of them is its own target.
    let script = skipping(&[Decision::Skip, Decision::Skip]);
    let both = vec![src.join("dir"), src.join("file")];
    let (outcome, seen) = run_within(&LocalFs, both, upper, PLAIN, script).await;
    assert_eq!(seen.failed.len(), 2, "once each: {seen:?}");
    assert!(seen.exists.is_empty(), "{seen:?}");
    assert_eq!(outcome.done, 0);
    assert_eq!(names(&src), ["dir", "file"]);
    assert_eq!(fs::read_to_string(src.join("file")).unwrap(), "f");
    check_dir(&src.join("dir"));
    assert!(!src.join("dir/dir").exists());
}

#[tokio::test]
async fn a_move_to_its_own_name_spelled_otherwise_takes_that_spelling() {
    let root = tempfile::tempdir().unwrap();
    if !ignores_case(root.path()) {
        return;
    }
    let src = sources(root.path());
    for (name, other) in [("file", "File"), ("dir", "DIR")] {
        let one = vec![src.join(name)];
        let (outcome, seen) = run_move(&LocalFs, one, src.join(other), Script::default()).await;
        assert!(seen.failed.is_empty() && seen.exists.is_empty(), "{seen:?}");
        assert_eq!(outcome.done, 1, "{name}");
    }
    assert_eq!(names(&src), ["DIR", "File"]);
    assert_eq!(fs::read_to_string(src.join("File")).unwrap(), "f");
    check_dir(&src.join("DIR"));

    // Into its own directory spelled otherwise, it stays where it is.
    let script = skipping(&[Decision::Skip]);
    let one = vec![src.join("File")];
    let (outcome, seen) = run_move(&LocalFs, one, root.path().join("SRC"), script).await;
    assert_eq!(seen.failed.len(), 1, "{seen:?}");
    assert_eq!((outcome.done, outcome.skipped), (0, 1));
    assert_eq!(fs::read_to_string(src.join("File")).unwrap(), "f");
}

#[tokio::test]
async fn on_a_host_a_copy_does_not_go_over_its_source_spelled_otherwise() {
    let root = tempfile::tempdir().unwrap();
    if !ignores_case(root.path()) {
        return;
    }
    let src = sources(root.path());
    let Some((_server, sftp)) = testing::sftp_server(root.path()).await else {
        return;
    };
    let path = RemotePath::from;
    let script = skipping(&[Decision::Skip]);
    let one = vec![path("src/file")];
    let (outcome, seen) = run_within(&sftp, one, path("src/FILE"), PLAIN, script).await;
    assert_eq!(seen.failed, ["src/FILE"], "{seen:?}");
    assert_eq!((outcome.done, outcome.skipped), (0, 1));
    assert_eq!(fs::read_to_string(src.join("file")).unwrap(), "f");

    let one = vec![path("src/file")];
    let (outcome, seen) = run_move(&sftp, one, path("src/File"), Script::default()).await;
    assert!(seen.failed.is_empty() && seen.exists.is_empty(), "{seen:?}");
    assert_eq!(outcome.done, 1);
    assert_eq!(names(&src), ["File", "dir"]);
    sftp.close().await.unwrap();
}
