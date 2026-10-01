//! Directory trees shared by the tests of both backends.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::TempDir;

use crate::{DirEntry, FileKind, Vfs};

/// The names in [`tree`], sorted.
pub(crate) const TREE: [&str; 5] = ["dangling", "dir", "file.txt", "link-dir", "link-file"];

/// A directory with a file, a subdirectory, and symlinks to both and to nothing.
pub(crate) fn tree() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| dir.path().join(name);
    fs::write(path("file.txt"), "hello").unwrap();
    fs::set_permissions(path("file.txt"), fs::Permissions::from_mode(0o640)).unwrap();
    fs::create_dir(path("dir")).unwrap();
    fs::write(path("dir/inner.txt"), "inner").unwrap();
    symlink("dir", path("link-dir")).unwrap();
    symlink("file.txt", path("link-file")).unwrap();
    symlink("missing", path("dangling")).unwrap();
    dir
}

/// Checks a listing of [`tree`], whose canonical path is `root`.
pub(crate) fn check_tree(entries: &[DirEntry], root: &Path) {
    assert_eq!(sorted_names(entries), TREE);

    let local = fs::metadata(root.join("file.txt")).unwrap();
    let file = entry(entries, "file.txt");
    assert_eq!(file.metadata.kind, FileKind::File);
    assert_eq!(file.metadata.size, Some(5));
    assert_eq!(file.metadata.permissions, Some(0o640));
    assert_eq!(file.metadata.uid, Some(local.uid()));
    assert_eq!(file.metadata.gid, Some(local.gid()));
    assert_eq!(
        file.metadata.modified.map(seconds),
        Some(seconds(local.modified().unwrap()))
    );
    assert_eq!(file.target_kind, None);
    assert!(!file.is_dir_like());

    let dir = entry(entries, "dir");
    assert_eq!(dir.metadata.kind, FileKind::Dir);
    assert_eq!(dir.target_kind, None);
    assert!(dir.is_dir_like());

    let links = [
        ("link-dir", "dir", Some(FileKind::Dir)),
        ("link-file", "file.txt", Some(FileKind::File)),
        ("dangling", "missing", None),
    ];
    for (name, target, target_kind) in links {
        let link = entry(entries, name);
        assert_eq!(link.metadata.kind, FileKind::Symlink, "{name}");
        assert_eq!(link.metadata.size, Some(target.len() as u64), "{name}");
        assert_eq!(link.target_kind, target_kind, "{name}");
        assert_eq!(
            link.is_dir_like(),
            target_kind == Some(FileKind::Dir),
            "{name}"
        );
    }
}

/// A directory with a socket, a FIFO, and a symlink to a character device.
pub(crate) fn special_files() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    // The socket file outlives the listener.
    drop(UnixListener::bind(dir.path().join("socket")).unwrap());
    let status = Command::new("mkfifo")
        .arg(dir.path().join("fifo"))
        .status()
        .unwrap();
    assert!(status.success(), "mkfifo: {status}");
    symlink("/dev/null", dir.path().join("null")).unwrap();
    dir
}

/// Checks a listing of [`special_files`].
pub(crate) fn check_special_files(entries: &[DirEntry]) {
    assert_eq!(sorted_names(entries), ["fifo", "null", "socket"]);
    assert_eq!(entry(entries, "socket").metadata.kind, FileKind::Socket);
    assert_eq!(entry(entries, "fifo").metadata.kind, FileKind::Fifo);
    let null = entry(entries, "null");
    assert_eq!(null.metadata.kind, FileKind::Symlink);
    assert_eq!(null.target_kind, Some(FileKind::CharDevice));
    assert!(!null.is_dir_like());
}

/// A directory without permissions, removed again when the tests run as root, which ignores
/// permissions.
pub(crate) struct LockedDir {
    pub(crate) path: PathBuf,
}

impl LockedDir {
    pub(crate) fn new(parent: &Path) -> Option<Self> {
        let path = parent.join("locked");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&path).is_ok() {
            fs::remove_dir(&path).unwrap();
            return None;
        }
        Some(Self { path })
    }
}

impl Drop for LockedDir {
    fn drop(&mut self) {
        // Lets the temporary directory be removed.
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o755));
    }
}

/// The entry called `name`.
pub(crate) fn entry<'a>(entries: &'a [DirEntry], name: &str) -> &'a DirEntry {
    entries
        .iter()
        .find(|entry| entry.name == name.as_bytes())
        .unwrap_or_else(|| panic!("no entry {name:?} in {entries:#?}"))
}

/// The names in the listing of `path`, sorted.
pub(crate) async fn names<V: Vfs>(vfs: &V, path: &V::Path) -> Vec<String> {
    sorted_names(&vfs.list_dir(path).await.unwrap())
}

fn sorted_names(entries: &[DirEntry]) -> Vec<String> {
    let mut names: Vec<_> = entries
        .iter()
        .map(|entry| entry.display_name().into_owned())
        .collect();
    names.sort_unstable();
    names
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).unwrap().as_secs()
}
