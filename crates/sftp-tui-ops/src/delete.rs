//! Deleting files and directory trees.

use sftp_tui_vfs::{FileKind, Vfs, VfsError, VfsPath as _};

use crate::job::{Decision, Event, Outcome, Progress, Reporter};

/// An entry to remove, found while counting.
struct Item<P> {
    path: P,
    dir: bool,
    /// The directory it is in, unless it is a target.
    parent: Option<usize>,
    /// Stays: it could not be read, or something in it stays.
    kept: bool,
}

/// Why counting or removing stopped early.
struct Aborted;

/// Deletes `targets`: files, symlinks (never what they point to), and directories with
/// everything in them. It counts the entries first, so that progress has a total, then removes
/// the deepest ones first. When an operation fails it asks `reporter` what to do; a directory
/// that keeps an entry is left alone. Entries that are already gone count as deleted.
pub async fn delete<V: Vfs>(
    vfs: &V,
    targets: Vec<V::Path>,
    reporter: &mut Reporter<V::Path>,
) -> Outcome {
    let mut outcome = Outcome::default();
    let mut items = Vec::new();
    if scan(vfs, targets, reporter, &mut items, &mut outcome)
        .await
        .is_err()
        || remove(vfs, reporter, &mut items, &mut outcome)
            .await
            .is_err()
    {
        outcome.aborted = true;
    }
    outcome
}

/// Lists the trees under `targets` into `items`, parents before what they hold. Symlinks are
/// entries of their own: listings report them without following them.
async fn scan<V: Vfs>(
    vfs: &V,
    targets: Vec<V::Path>,
    reporter: &mut Reporter<V::Path>,
    items: &mut Vec<Item<V::Path>>,
    outcome: &mut Outcome,
) -> Result<(), Aborted> {
    let mut dirs = Vec::new();
    for path in targets {
        let metadata = loop {
            if reporter.cancelled() {
                return Err(Aborted);
            }
            match vfs.symlink_metadata(&path).await {
                Ok(metadata) => break Some(metadata),
                Err(VfsError::NotFound(_)) => break None,
                Err(error) => match reporter.ask(path.clone(), error).await {
                    Decision::Retry => {}
                    Decision::Skip | Decision::SkipAll => {
                        outcome.skipped += 1;
                        break None;
                    }
                    Decision::Abort => return Err(Aborted),
                },
            }
        };
        let Some(metadata) = metadata else { continue };
        let dir = metadata.kind == FileKind::Dir;
        if dir {
            dirs.push(items.len());
        }
        items.push(Item {
            path,
            dir,
            parent: None,
            kept: false,
        });
        while let Some(index) = dirs.pop() {
            let entries = loop {
                if reporter.cancelled() {
                    return Err(Aborted);
                }
                match vfs.list_dir(&items[index].path).await {
                    Ok(entries) => break entries,
                    // Gone already; removing it will find that out.
                    Err(VfsError::NotFound(_)) => break Vec::new(),
                    Err(error) => match reporter.ask(items[index].path.clone(), error).await {
                        Decision::Retry => {}
                        Decision::Skip | Decision::SkipAll => {
                            items[index].kept = true;
                            break Vec::new();
                        }
                        Decision::Abort => return Err(Aborted),
                    },
                }
            };
            for entry in entries {
                let dir = entry.metadata.kind == FileKind::Dir;
                if dir {
                    dirs.push(items.len());
                }
                items.push(Item {
                    path: items[index].path.join_name(&entry.name),
                    dir,
                    parent: Some(index),
                    kept: false,
                });
            }
            reporter.report(Event::Scanning {
                items: items.len() as u64,
            });
        }
    }
    Ok(())
}

/// Removes `items` from the last to the first, so that a directory goes after what it holds.
async fn remove<V: Vfs>(
    vfs: &V,
    reporter: &mut Reporter<V::Path>,
    items: &mut [Item<V::Path>],
    outcome: &mut Outcome,
) -> Result<(), Aborted> {
    let total = items.len() as u64 + outcome.skipped;
    for index in (0..items.len()).rev() {
        if reporter.cancelled() {
            return Err(Aborted);
        }
        let item = &items[index];
        let mut removed = !item.kept;
        if removed {
            reporter.report(Event::Progress(Progress {
                current: item.path.clone(),
                items_done: outcome.done + outcome.skipped,
                items_total: total,
            }));
            loop {
                let result = if item.dir {
                    vfs.remove_dir(&item.path).await
                } else {
                    vfs.remove_file(&item.path).await
                };
                match result {
                    Ok(()) | Err(VfsError::NotFound(_)) => break,
                    Err(error) => match reporter.ask(item.path.clone(), error).await {
                        Decision::Retry => {}
                        Decision::Skip | Decision::SkipAll => {
                            removed = false;
                            break;
                        }
                        Decision::Abort => return Err(Aborted),
                    },
                }
            }
        }
        if removed {
            outcome.done += 1;
        } else {
            outcome.skipped += 1;
            if let Some(parent) = item.parent {
                items[parent].kept = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    use std::path::{Path, PathBuf};
    use std::process::Stdio;

    use sftp_tui_vfs::{LocalFs, RemotePath, SftpFs};
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::*;

    /// Deletes `targets` with `vfs`, answering failures with `answers` in turn, and returns
    /// the outcome, the paths that failed, and the totals that progress reported.
    async fn run<V: Vfs>(
        vfs: &V,
        targets: Vec<V::Path>,
        answers: &[Decision],
        cancel: CancellationToken,
    ) -> (Outcome, Vec<V::Path>, Vec<u64>) {
        let (events, mut incoming) = mpsc::unbounded_channel();
        let mut reporter = Reporter::new(events, cancel);
        let job = async {
            let outcome = delete(vfs, targets, &mut reporter).await;
            drop(reporter);
            outcome
        };
        let listen = async {
            let (mut failed, mut totals) = (Vec::new(), Vec::new());
            let mut answers = answers.iter();
            while let Some(event) = incoming.recv().await {
                match event {
                    Event::Scanning { .. } => {}
                    Event::Progress(progress) => totals.push(progress.items_total),
                    Event::Failed { path, reply, .. } => {
                        failed.push(path);
                        let answer = *answers.next().expect("an answer for each failure");
                        let _ = reply.send(answer);
                    }
                }
            }
            (failed, totals)
        };
        let (outcome, (failed, totals)) = tokio::join!(job, listen);
        (outcome, failed, totals)
    }

    /// `dir/` with `a`, `sub/b`, `sub/deeper/c`, a link to a directory outside, and a link
    /// to nothing; the outside directory holds `keep`.
    fn tree(root: &Path) -> PathBuf {
        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep"), "keep").unwrap();
        let dir = root.join("dir");
        fs::create_dir_all(dir.join("sub/deeper")).unwrap();
        fs::write(dir.join("a"), "a").unwrap();
        fs::write(dir.join("sub/b"), "b").unwrap();
        fs::write(dir.join("sub/deeper/c"), "c").unwrap();
        symlink(&outside, dir.join("sub/out")).unwrap();
        symlink("missing", dir.join("dangling")).unwrap();
        dir
    }

    /// Makes `dir` read-only, so that nothing in it can be removed, unless the tests run as
    /// root, which ignores that: then `false`.
    fn lock(dir: &Path) -> bool {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
        let probe = dir.join(".probe");
        if fs::write(&probe, "").is_ok() {
            fs::remove_file(probe).unwrap();
            unlock(dir);
            return false;
        }
        true
    }

    fn unlock(dir: &Path) {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[tokio::test]
    async fn deletes_trees_without_following_links() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        fs::write(root.path().join("file"), "f").unwrap();
        let targets = vec![
            dir.clone(),
            root.path().join("file"),
            root.path().join("gone"),
        ];
        let (outcome, failed, totals) = run(&LocalFs, targets, &[], CancellationToken::new()).await;
        assert_eq!(failed, Vec::<PathBuf>::new());
        // dir, a, dangling, sub, b, out, deeper, c, and file.
        assert_eq!(
            outcome,
            Outcome {
                done: 9,
                skipped: 0,
                aborted: false
            }
        );
        assert!(totals.iter().all(|total| *total == 9), "{totals:?}");
        assert_eq!(totals.len(), 9);
        assert!(!dir.exists());
        assert!(!root.path().join("file").exists());
        let keep = fs::read_to_string(root.path().join("outside/keep")).unwrap();
        assert_eq!(keep, "keep", "the link went, not its target");
    }

    #[tokio::test]
    async fn a_failure_keeps_the_directories_around_it() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let deeper = dir.join("sub/deeper");
        if !lock(&deeper) {
            return;
        }
        let (outcome, failed, _) = run(
            &LocalFs,
            vec![dir.clone()],
            &[Decision::Skip],
            CancellationToken::new(),
        )
        .await;
        unlock(&deeper);
        assert_eq!(failed, [deeper.join("c")]);
        // c, deeper, sub, and dir stay.
        assert_eq!(
            outcome,
            Outcome {
                done: 4,
                skipped: 4,
                aborted: false
            }
        );
        assert!(deeper.join("c").exists());
        assert!(!dir.join("a").exists());
        assert!(!dir.join("sub/b").exists());
    }

    #[tokio::test]
    async fn retry_tries_again_and_skip_all_asks_no_more() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let sub = dir.join("sub");
        if !lock(&sub) {
            return;
        }
        // Retry fails again while `sub` is locked; then skip that and everything after.
        let answers = [Decision::Retry, Decision::SkipAll];
        let (outcome, failed, _) = run(
            &LocalFs,
            vec![dir.clone()],
            &answers,
            CancellationToken::new(),
        )
        .await;
        unlock(&sub);
        assert_eq!(failed.len(), 2, "{failed:?}");
        assert_eq!(failed[0], failed[1], "the same entry again");
        // c goes, as `deeper` is not locked; b, out, and deeper stay, and so do sub and dir.
        assert_eq!(
            outcome,
            Outcome {
                done: 3,
                skipped: 5,
                aborted: false
            }
        );
        assert!(!dir.join("a").exists());
        assert!(!dir.join("sub/deeper/c").exists());
        assert!(dir.join("sub/b").exists());
    }

    #[tokio::test]
    async fn abort_and_cancel_stop_the_job() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let deeper = dir.join("sub/deeper");
        if !lock(&deeper) {
            return;
        }
        let (outcome, _, _) = run(
            &LocalFs,
            vec![dir.clone()],
            &[Decision::Abort],
            CancellationToken::new(),
        )
        .await;
        unlock(&deeper);
        assert!(outcome.aborted);
        assert!(dir.join("a").exists(), "stopped before the rest");

        let cancel = CancellationToken::new();
        cancel.cancel();
        let (outcome, _, _) = run(&LocalFs, vec![dir.clone()], &[], cancel).await;
        assert_eq!(
            outcome,
            Outcome {
                done: 0,
                skipped: 0,
                aborted: true
            }
        );
        assert!(dir.join("sub/deeper/c").exists());
    }

    #[tokio::test]
    async fn cancelling_stops_waiting_for_a_decision() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let deeper = dir.join("sub/deeper");
        if !lock(&deeper) {
            return;
        }
        let (events, mut incoming) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let mut reporter = Reporter::new(events, cancel.clone());
        let job = delete(&LocalFs, vec![dir.clone()], &mut reporter);
        let listen = async {
            let mut kept = Vec::new();
            while let Some(event) = incoming.recv().await {
                if let Event::Failed { reply, .. } = event {
                    // Keep the reply, so that only cancelling can end the wait.
                    kept.push(reply);
                    cancel.cancel();
                }
                if cancel.is_cancelled() {
                    break;
                }
            }
            kept
        };
        let (outcome, _kept) = tokio::join!(job, listen);
        unlock(&deeper);
        assert!(outcome.aborted);
    }

    /// A local `sftp-server` that starts in `dir`, if there is one.
    async fn sftp_server(dir: &Path) -> Option<(tokio::process::Child, SftpFs)> {
        let program = ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server"]
            .into_iter()
            .map(Path::new)
            .find(|path| path.exists())?;
        let mut child = tokio::process::Command::new(program)
            .arg("-e")
            .arg("-d")
            .arg(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let fs = SftpFs::from_pipes(stdin, stdout).await.unwrap();
        Some((child, fs))
    }

    #[tokio::test]
    async fn deletes_remote_trees_without_following_links() {
        let root = tempfile::tempdir().unwrap();
        let dir = tree(root.path());
        let Some((_server, fs)) = sftp_server(root.path()).await else {
            return;
        };
        let targets = vec![RemotePath::from("dir")];
        let (outcome, failed, _) = run(&fs, targets, &[], CancellationToken::new()).await;
        assert_eq!(failed, Vec::<RemotePath>::new());
        assert_eq!(outcome.done, 8);
        assert!(!dir.exists());
        assert!(root.path().join("outside/keep").exists());
        fs.close().await.unwrap();
    }
}
