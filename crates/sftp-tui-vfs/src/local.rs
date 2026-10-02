use std::fs;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{AtFlags, CWD, Timespec, Timestamps};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::{DirEntry, FileKind, FileReader, FileWriter, Metadata, Vfs, VfsError};

/// Bytes each read of a local file asks for.
const CHUNK: usize = 256 * 1024;

/// The local file system.
///
/// Relative paths are relative to the current directory; the empty path is the current
/// directory, like the empty [`RemotePath`](crate::RemotePath) is the remote home directory.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalFs;

/// A local file open for reading.
#[derive(Debug)]
pub struct LocalReader {
    file: tokio::fs::File,
    path: PathBuf,
}

impl FileReader for LocalReader {
    async fn read(&mut self) -> Result<Option<Vec<u8>>, VfsError> {
        let mut buffer = vec![0; CHUNK];
        let read = self
            .file
            .read(&mut buffer)
            .await
            .map_err(|err| VfsError::local(err, &self.path))?;
        if read == 0 {
            return Ok(None);
        }
        buffer.truncate(read);
        Ok(Some(buffer))
    }
}

/// A local file open for writing.
#[derive(Debug)]
pub struct LocalWriter {
    file: tokio::fs::File,
    path: PathBuf,
}

impl FileWriter for LocalWriter {
    async fn write(&mut self, data: Vec<u8>) -> Result<(), VfsError> {
        self.file
            .write_all(&data)
            .await
            .map_err(|err| VfsError::local(err, &self.path))
    }

    async fn finish(mut self) -> Result<(), VfsError> {
        self.file
            .flush()
            .await
            .map_err(|err| VfsError::local(err, &self.path))
    }
}

impl Vfs for LocalFs {
    type Path = PathBuf;
    type Reader = LocalReader;
    type Writer = LocalWriter;

    async fn list_dir(&self, path: &PathBuf) -> Result<Vec<DirEntry>, VfsError> {
        let path = path.clone();
        tokio::task::spawn_blocking(move || read_dir(&path))
            .await
            .map_err(VfsError::task_failed)?
    }

    async fn metadata(&self, path: &PathBuf) -> Result<Metadata, VfsError> {
        let metadata = tokio::fs::metadata(os_path(path))
            .await
            .map_err(|err| VfsError::local(err, path))?;
        Ok(convert(&metadata))
    }

    async fn symlink_metadata(&self, path: &PathBuf) -> Result<Metadata, VfsError> {
        let metadata = tokio::fs::symlink_metadata(os_path(path))
            .await
            .map_err(|err| VfsError::local(err, path))?;
        Ok(convert(&metadata))
    }

    async fn canonicalize(&self, path: &PathBuf) -> Result<PathBuf, VfsError> {
        tokio::fs::canonicalize(os_path(path))
            .await
            .map_err(|err| VfsError::local(err, path))
    }

    async fn create_dir(&self, path: &PathBuf) -> Result<(), VfsError> {
        tokio::fs::create_dir(path)
            .await
            .map_err(|err| VfsError::local(err, path))
    }

    async fn remove_file(&self, path: &PathBuf) -> Result<(), VfsError> {
        tokio::fs::remove_file(path)
            .await
            .map_err(|err| VfsError::local(err, path))
    }

    async fn remove_dir(&self, path: &PathBuf) -> Result<(), VfsError> {
        tokio::fs::remove_dir(path)
            .await
            .map_err(|err| VfsError::local(err, path))
    }

    async fn rename(&self, from: &PathBuf, to: &PathBuf) -> Result<(), VfsError> {
        tokio::fs::rename(from, to).await.map_err(|err| {
            // The name that is taken is the target; any other trouble is the source's.
            let path = if err.kind() == io::ErrorKind::AlreadyExists {
                to
            } else {
                from
            };
            VfsError::local(err, path)
        })
    }

    async fn set_permissions(&self, path: &PathBuf, mode: u32) -> Result<(), VfsError> {
        let permissions = fs::Permissions::from_mode(mode & 0o7777);
        tokio::fs::set_permissions(os_path(path), permissions)
            .await
            .map_err(|err| VfsError::local(err, path))
    }

    async fn open_file(&self, path: &PathBuf) -> Result<LocalReader, VfsError> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|err| VfsError::local(err, path))?;
        Ok(LocalReader {
            file,
            path: path.clone(),
        })
    }

    async fn create_file(&self, path: &PathBuf, replace: bool) -> Result<LocalWriter, VfsError> {
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true);
        if replace {
            options.create(true).truncate(true);
        } else {
            options.create_new(true);
        }
        let file = options
            .open(path)
            .await
            .map_err(|err| VfsError::local(err, path))?;
        Ok(LocalWriter {
            file,
            path: path.clone(),
        })
    }

    async fn set_modified(&self, path: &PathBuf, time: SystemTime) -> Result<(), VfsError> {
        let target = path.clone();
        tokio::task::spawn_blocking(move || {
            let invalid = || io::Error::from(io::ErrorKind::InvalidInput);
            let times = Timestamps {
                last_access: timespec(SystemTime::now()).ok_or_else(invalid)?,
                last_modification: timespec(time).ok_or_else(invalid)?,
            };
            // By path: opening a FIFO to set its times would wait for a writer.
            rustix::fs::utimensat(CWD, os_path(&target), &times, AtFlags::empty())
                .map_err(io::Error::from)
        })
        .await
        .map_err(VfsError::task_failed)?
        .map_err(|err| VfsError::local(err, path))
    }
}

/// `time` as seconds and nanoseconds from the epoch, negative before it.
fn timespec(time: SystemTime) -> Option<Timespec> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => Timespec::try_from(after).ok(),
        Err(before) => {
            let zero = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            zero.checked_sub(Timespec::try_from(before.duration()).ok()?)
        }
    }
}

/// The path to pass to the OS, which rejects the empty path.
fn os_path(path: &Path) -> &Path {
    if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    }
}

fn read_dir(path: &Path) -> Result<Vec<DirEntry>, VfsError> {
    let error = |err| VfsError::local(err, path);
    let mut entries = Vec::new();
    for entry in fs::read_dir(os_path(path)).map_err(error)? {
        let entry = entry.map_err(error)?;
        // `DirEntry::metadata` does not follow symlinks.
        let metadata = match entry.metadata() {
            Ok(metadata) => convert(&metadata),
            // Removed after the directory was read.
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => Metadata::of_kind(entry.file_type().map_or(FileKind::Unknown, kind_of)),
        };
        let target_kind = if metadata.kind == FileKind::Symlink {
            fs::metadata(entry.path())
                .ok()
                .map(|target| kind_of(target.file_type()))
        } else {
            None
        };
        entries.push(DirEntry {
            name: entry.file_name().into_vec(),
            metadata,
            target_kind,
        });
    }
    Ok(entries)
}

fn convert(metadata: &fs::Metadata) -> Metadata {
    Metadata {
        kind: kind_of(metadata.file_type()),
        size: Some(metadata.len()),
        permissions: Some(metadata.mode() & 0o7777),
        modified: metadata.modified().ok(),
        uid: Some(metadata.uid()),
        gid: Some(metadata.gid()),
    }
}

fn kind_of(file_type: fs::FileType) -> FileKind {
    if file_type.is_file() {
        FileKind::File
    } else if file_type.is_dir() {
        FileKind::Dir
    } else if file_type.is_symlink() {
        FileKind::Symlink
    } else if file_type.is_fifo() {
        FileKind::Fifo
    } else if file_type.is_socket() {
        FileKind::Socket
    } else if file_type.is_block_device() {
        FileKind::BlockDevice
    } else if file_type.is_char_device() {
        FileKind::CharDevice
    } else {
        FileKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    #[tokio::test]
    async fn lists_a_directory() {
        let dir = fixture::tree();
        let entries = LocalFs.list_dir(&dir.path().to_path_buf()).await.unwrap();
        fixture::check_tree(&entries, dir.path());

        let file = fixture::entry(&entries, "file.txt");
        let local = fs::metadata(dir.path().join("file.txt")).unwrap();
        assert_eq!(file.metadata.modified, Some(local.modified().unwrap()));

        for name in ["dir", "link-dir"] {
            let names = fixture::names(&LocalFs, &dir.path().join(name)).await;
            assert_eq!(names, ["inner.txt"], "{name}");
        }
    }

    #[tokio::test]
    async fn lists_special_files() {
        let dir = fixture::special_files();
        let entries = LocalFs.list_dir(&dir.path().to_path_buf()).await.unwrap();
        fixture::check_special_files(&entries);
    }

    #[tokio::test]
    async fn metadata_follows_symlinks() {
        let dir = fixture::tree();
        let path = |name: &str| dir.path().join(name);

        let file = LocalFs.metadata(&path("link-file")).await.unwrap();
        assert_eq!(file.kind, FileKind::File);
        assert_eq!(file.size, Some(5));
        assert_eq!(file.permissions, Some(0o640));
        let sub = LocalFs.metadata(&path("link-dir")).await.unwrap();
        assert_eq!(sub.kind, FileKind::Dir);

        let err = LocalFs.metadata(&path("dangling")).await.unwrap_err();
        assert!(
            matches!(&err, VfsError::NotFound(p) if *p == path("dangling").display().to_string()),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn changes_entries() {
        let dir = tempfile::tempdir().unwrap();
        fixture::check_changes(&LocalFs, dir.path(), |name| dir.path().join(name)).await;
    }

    #[tokio::test]
    async fn writes_and_reads_files() {
        let dir = tempfile::tempdir().unwrap();
        fixture::check_files(&LocalFs, dir.path(), |name| dir.path().join(name)).await;
    }

    #[tokio::test]
    async fn sets_attributes() {
        let dir = tempfile::tempdir().unwrap();
        fixture::check_attributes(&LocalFs, dir.path(), |name| dir.path().join(name)).await;

        // The nanoseconds, and times before 1970.
        let file = dir.path().join("file");
        for time in [
            UNIX_EPOCH + std::time::Duration::new(1_600_000_000, 123_456_789),
            UNIX_EPOCH - std::time::Duration::from_millis(1_500),
        ] {
            LocalFs.set_modified(&file, time).await.unwrap();
            assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), time);
        }
    }

    #[tokio::test]
    async fn symlink_metadata_does_not_follow() {
        let dir = fixture::tree();
        let path = |name: &str| dir.path().join(name);
        for name in ["link-dir", "link-file", "dangling"] {
            let link = LocalFs.symlink_metadata(&path(name)).await.unwrap();
            assert_eq!(link.kind, FileKind::Symlink, "{name}");
        }
        let file = LocalFs.symlink_metadata(&path("file.txt")).await.unwrap();
        assert_eq!(file.kind, FileKind::File);
    }

    #[tokio::test]
    async fn canonicalizes() {
        let dir = fixture::tree();
        let root = fs::canonicalize(dir.path()).unwrap();
        let canonical = LocalFs
            .canonicalize(&dir.path().join("link-dir/../file.txt"))
            .await
            .unwrap();
        assert_eq!(canonical, root.join("file.txt"));
        let canonical = LocalFs
            .canonicalize(&dir.path().join("link-dir"))
            .await
            .unwrap();
        assert_eq!(canonical, root.join("dir"));
    }

    #[tokio::test]
    async fn reports_missing_paths() {
        let dir = fixture::tree();
        let missing = dir.path().join("missing");
        let expected = missing.display().to_string();

        let err = LocalFs.list_dir(&missing).await.unwrap_err();
        assert!(
            matches!(&err, VfsError::NotFound(p) if *p == expected),
            "{err:?}"
        );
        let err = LocalFs.metadata(&missing).await.unwrap_err();
        assert!(
            matches!(&err, VfsError::NotFound(p) if *p == expected),
            "{err:?}"
        );
        let err = LocalFs.canonicalize(&missing).await.unwrap_err();
        assert!(
            matches!(&err, VfsError::NotFound(p) if *p == expected),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn reports_permission_denied() {
        let dir = tempfile::tempdir().unwrap();
        let Some(locked) = fixture::LockedDir::new(dir.path()) else {
            return;
        };
        let err = LocalFs.list_dir(&locked.path).await.unwrap_err();
        let expected = locked.path.display().to_string();
        assert!(
            matches!(&err, VfsError::PermissionDenied(p) if *p == expected),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn the_empty_path_is_the_current_directory() {
        let empty = PathBuf::new();
        let current = std::env::current_dir().unwrap();
        assert_eq!(
            LocalFs.canonicalize(&empty).await.unwrap(),
            fs::canonicalize(&current).unwrap()
        );
        assert_eq!(LocalFs.metadata(&empty).await.unwrap().kind, FileKind::Dir);
        assert_eq!(
            fixture::names(&LocalFs, &empty).await,
            fixture::names(&LocalFs, &current).await
        );
    }

    #[tokio::test]
    async fn listing_a_file_is_an_io_error() {
        let dir = fixture::tree();
        let err = LocalFs
            .list_dir(&dir.path().join("file.txt"))
            .await
            .unwrap_err();
        assert!(matches!(err, VfsError::Io(_)), "{err:?}");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn keeps_non_utf8_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(OsStr::from_bytes(b"caf\xe9")), "").unwrap();
        let entries = LocalFs.list_dir(&dir.path().to_path_buf()).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, b"caf\xe9");
        assert_eq!(entries[0].display_name(), "caf\u{fffd}");
    }
}
