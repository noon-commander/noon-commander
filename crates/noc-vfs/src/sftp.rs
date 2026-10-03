use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::time::SystemTime;

use bytes::BytesMut;
use futures_util::stream::FuturesUnordered;
use futures_util::{StreamExt, TryStreamExt, stream};
use openssh_sftp_client::error::SftpErrorKind;
use openssh_sftp_client::file::{File, OpenOptions};
use openssh_sftp_client::metadata::{MetaData, MetaDataBuilder, Permissions, RawFileType};
use openssh_sftp_client::{Error, Sftp, SftpOptions, UnixTimeStamp};
use tokio::io::{AsyncRead, AsyncSeekExt as _, AsyncWrite};

use crate::files::{Fetch, ReadPipeline};
use crate::{
    DirEntry, FileKind, FileReader, FileWriter, Metadata, RemotePath, Space, Vfs, VfsError,
};

/// Symlink targets a listing resolves at a time, the number of requests `sftp(1)` keeps in
/// flight.
const MAX_PENDING_STATS: usize = 64;

/// Bytes each read or write request of a file carries: what every server takes, and what
/// `sftp(1)` asks for. A server that takes less answers reads short, which the reader asks
/// again for, and the client splits writes.
const CHUNK: u32 = 32 * 1024;
/// Bytes of a file that reads and writes keep in flight: 64 requests, as `sftp(1)` does.
const IN_FLIGHT: u32 = 64 * CHUNK;

/// A remote file open for reading, with several reads in flight.
pub struct SftpReader {
    pipeline: ReadPipeline,
}

impl std::fmt::Debug for SftpReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpReader").finish_non_exhaustive()
    }
}

impl FileReader for SftpReader {
    async fn read(&mut self) -> Result<Option<Vec<u8>>, VfsError> {
        self.pipeline.read().await
    }
}

/// A remote file open for writing, with several writes in flight.
#[derive(Debug)]
pub struct SftpWriter {
    file: File,
    path: RemotePath,
    /// Where the next write goes.
    offset: u64,
    /// Writes in flight, and their bytes.
    pending: FuturesUnordered<WriteFuture>,
    in_flight: u64,
}

type WriteFuture = futures_util::future::BoxFuture<'static, (u64, Result<(), Error>)>;

impl SftpWriter {
    /// Waits for one write in flight.
    async fn settle_one(&mut self) -> Result<(), VfsError> {
        if let Some((length, result)) = self.pending.next().await {
            self.in_flight -= length;
            result.map_err(|err| VfsError::remote(err, &self.path))?;
        }
        Ok(())
    }
}

impl FileWriter for SftpWriter {
    async fn write(&mut self, data: Vec<u8>) -> Result<(), VfsError> {
        for part in data.chunks(CHUNK as usize) {
            while self.in_flight >= u64::from(IN_FLIGHT) {
                self.settle_one().await?;
            }
            let mut file = self.file.clone();
            let offset = self.offset;
            let part = part.to_vec();
            let length = part.len() as u64;
            self.pending.push(Box::pin(async move {
                let result = match file.seek(io::SeekFrom::Start(offset)).await {
                    Ok(_) => file.write_all(&part).await,
                    Err(err) => Err(Error::IOError(err)),
                };
                (length, result)
            }));
            self.offset += length;
            self.in_flight += length;
        }
        Ok(())
    }

    async fn finish(mut self) -> Result<(), VfsError> {
        while !self.pending.is_empty() {
            self.settle_one().await?;
        }
        let path = self.path;
        self.file
            .close()
            .await
            .map_err(|err| VfsError::remote(err, &path))
    }
}

/// A file system on a remote host, reached through an SFTP channel.
///
/// Names must be valid UTF-8: the SFTP client refuses to send other paths, and a reply that
/// contains one ends the session.
#[derive(Debug)]
pub struct SftpFs {
    sftp: Sftp,
}

impl SftpFs {
    /// Wraps an established SFTP session.
    pub fn new(sftp: Sftp) -> Self {
        Self { sftp }
    }

    /// Starts an SFTP session over the pipes of an `ssh -s … sftp` process (or `sftp-server`).
    pub async fn from_pipes<W, R>(stdin: W, stdout: R) -> Result<Self, VfsError>
    where
        W: AsyncWrite + Send + 'static,
        R: AsyncRead + Send + 'static,
    {
        Sftp::new(stdin, stdout, SftpOptions::default())
            .await
            .map(Self::new)
            .map_err(VfsError::Sftp)
    }

    /// The remote home directory: canonical form of `.`.
    pub async fn home(&self) -> Result<RemotePath, VfsError> {
        self.canonicalize(&RemotePath::from(".")).await
    }

    /// Closes the SFTP session. It waits for the readers and writers of its files to be
    /// dropped; drop them first.
    pub async fn close(self) -> Result<(), VfsError> {
        self.sftp.close().await.map_err(VfsError::Sftp)
    }

    /// The kind of the target of the symlink at `link`; `None` if it cannot be resolved.
    async fn target_kind(&self, link: &RemotePath) -> Result<Option<FileKind>, VfsError> {
        let mut fs = self.sftp.fs();
        match fs.metadata(link.as_path()).await {
            Ok(metadata) => Ok(Some(kind(metadata))),
            // Dangling, a loop, or an inaccessible target.
            Err(openssh_sftp_client::Error::SftpError(..)) => Ok(None),
            Err(err) => Err(VfsError::Sftp(err)),
        }
    }

    /// The error for a failure to give `path` a name: SFTP v3 has no code for a name that is
    /// taken, so a plain failure where something has that name means that.
    async fn naming_error(&self, err: Error, path: &RemotePath) -> VfsError {
        if matches!(err, Error::SftpError(SftpErrorKind::Failure, _))
            && self.symlink_metadata(path).await.is_ok()
        {
            return VfsError::AlreadyExists(path.display().into_owned());
        }
        VfsError::remote(err, path)
    }
}

impl Vfs for SftpFs {
    type Path = RemotePath;
    type Reader = SftpReader;
    type Writer = SftpWriter;

    async fn list_dir(&self, path: &RemotePath) -> Result<Vec<DirEntry>, VfsError> {
        let error = |err| VfsError::remote(err, path);
        let mut fs = self.sftp.fs();
        let dir_path = wire_path(path).to_owned();
        // If this future is dropped before the server replies, the task still receives the
        // handle and closes it, instead of leaking it on the server.
        let dir = tokio::spawn(async move { fs.open_dir(dir_path).await })
            .await
            .map_err(VfsError::task_failed)?
            .map_err(error)?;
        let listing: Vec<_> = dir.read_dir().try_collect().await.map_err(error)?;

        let mut entries = Vec::with_capacity(listing.len());
        let mut links = Vec::new();
        for entry in listing {
            let name = entry.filename().as_os_str().as_bytes();
            if !is_entry_name(name) {
                if !matches!(name, b"." | b"..") {
                    tracing::warn!(
                        name = %String::from_utf8_lossy(name),
                        "skipping an invalid name in a directory listing"
                    );
                }
                continue;
            }
            let metadata = convert(entry.metadata());
            if metadata.kind == FileKind::Symlink {
                links.push((entries.len(), path.join(name)));
            }
            entries.push(DirEntry {
                name: name.to_vec(),
                metadata,
                target_kind: None,
            });
        }

        let targets: Vec<_> = stream::iter(links)
            .map(|(index, link)| async move {
                let kind = self.target_kind(&link).await?;
                Ok::<_, VfsError>((index, kind))
            })
            .buffer_unordered(MAX_PENDING_STATS)
            .try_collect()
            .await?;
        for (index, kind) in targets {
            entries[index].target_kind = kind;
        }
        Ok(entries)
    }

    async fn metadata(&self, path: &RemotePath) -> Result<Metadata, VfsError> {
        let mut fs = self.sftp.fs();
        fs.metadata(wire_path(path))
            .await
            .map(convert)
            .map_err(|err| VfsError::remote(err, path))
    }

    async fn symlink_metadata(&self, path: &RemotePath) -> Result<Metadata, VfsError> {
        let mut fs = self.sftp.fs();
        fs.symlink_metadata(wire_path(path))
            .await
            .map(convert)
            .map_err(|err| VfsError::remote(err, path))
    }

    async fn space(&self, path: &RemotePath) -> Result<Option<Space>, VfsError> {
        if !self.sftp.support_statvfs() {
            return Ok(None);
        }
        let mut fs = self.sftp.fs();
        let stat = fs
            .statvfs(wire_path(path))
            .await
            .map_err(|err| VfsError::remote(err, path))?;
        let fragment = if stat.frsize == 0 {
            stat.bsize
        } else {
            stat.frsize
        };
        Ok(Some(Space {
            total: stat.blocks.saturating_mul(fragment),
            available: stat.bavail.saturating_mul(fragment),
        }))
    }

    async fn canonicalize(&self, path: &RemotePath) -> Result<RemotePath, VfsError> {
        let mut fs = self.sftp.fs();
        let canonical = fs
            .canonicalize(wire_path(path))
            .await
            .map_err(|err| VfsError::remote(err, path))?;
        Ok(RemotePath::new(canonical.into_os_string().into_vec()))
    }

    async fn create_dir(&self, path: &RemotePath) -> Result<(), VfsError> {
        let result = self.sftp.fs().create_dir(wire_path(path)).await;
        match result {
            Ok(()) => Ok(()),
            Err(err) => Err(self.naming_error(err, path).await),
        }
    }

    async fn remove_file(&self, path: &RemotePath) -> Result<(), VfsError> {
        let mut fs = self.sftp.fs();
        fs.remove_file(wire_path(path))
            .await
            .map_err(|err| VfsError::remote(err, path))
    }

    async fn remove_dir(&self, path: &RemotePath) -> Result<(), VfsError> {
        let mut fs = self.sftp.fs();
        fs.remove_dir(wire_path(path))
            .await
            .map_err(|err| VfsError::remote(err, path))
    }

    async fn rename(&self, from: &RemotePath, to: &RemotePath) -> Result<(), VfsError> {
        let result = self.sftp.fs().rename(wire_path(from), wire_path(to)).await;
        match result {
            Ok(()) => Ok(()),
            Err(err @ Error::SftpError(SftpErrorKind::Failure, _)) => {
                Err(self.naming_error(err, to).await)
            }
            Err(err) => Err(VfsError::remote(err, from)),
        }
    }

    async fn read_link(&self, path: &RemotePath) -> Result<Vec<u8>, VfsError> {
        let mut fs = self.sftp.fs();
        fs.read_link(wire_path(path))
            .await
            .map(|target| target.into_os_string().into_vec())
            .map_err(|err| VfsError::remote(err, path))
    }

    async fn create_symlink(&self, target: &[u8], path: &RemotePath) -> Result<(), VfsError> {
        // The client sends only UTF-8 paths; others it would refuse anyway.
        let target = RemotePath::from(target);
        let result = self
            .sftp
            .fs()
            .symlink(target.as_path(), wire_path(path))
            .await;
        match result {
            Ok(()) => Ok(()),
            Err(err) => Err(self.naming_error(err, path).await),
        }
    }

    async fn set_permissions(&self, path: &RemotePath, mode: u32) -> Result<(), VfsError> {
        let bits = u16::try_from(mode & 0o7777).unwrap_or_else(|_| unreachable!());
        let mut fs = self.sftp.fs();
        fs.set_permissions(wire_path(path), Permissions::from(bits))
            .await
            .map_err(|err| VfsError::remote(err, path))
    }

    async fn open_file(&self, path: &RemotePath) -> Result<SftpReader, VfsError> {
        let mut options = self.sftp.options();
        options.read(true);
        let file = open(options, path)
            .await
            .map_err(|err| VfsError::remote(err, path))?;
        let fetch: Fetch = Box::new(move |offset, length| {
            let mut file = file.clone();
            Box::pin(async move {
                file.seek(io::SeekFrom::Start(offset))
                    .await
                    .map_err(VfsError::Io)?;
                let data = file
                    .read(length, BytesMut::with_capacity(length as usize))
                    .await
                    .map_err(VfsError::Sftp)?;
                Ok(data.map(Vec::from))
            })
        });
        let depth = usize::try_from(IN_FLIGHT / CHUNK).unwrap_or(1);
        Ok(SftpReader {
            pipeline: ReadPipeline::new(fetch, CHUNK, depth),
        })
    }

    async fn create_file(&self, path: &RemotePath, replace: bool) -> Result<SftpWriter, VfsError> {
        let mut options = self.sftp.options();
        options.write(true);
        if replace {
            options.create(true).truncate(true);
        } else {
            options.create_new(true);
        }
        let file = match open(options, path).await {
            Ok(file) => file,
            Err(err) => return Err(self.naming_error(err, path).await),
        };
        Ok(SftpWriter {
            file,
            path: path.clone(),
            offset: 0,
            pending: FuturesUnordered::new(),
            in_flight: 0,
        })
    }

    async fn set_modified(&self, path: &RemotePath, time: SystemTime) -> Result<(), VfsError> {
        let stamp = |time| {
            UnixTimeStamp::new(time)
                .map_err(|err| VfsError::Io(io::Error::new(io::ErrorKind::InvalidInput, err)))
        };
        let times = MetaDataBuilder::new()
            .time(stamp(SystemTime::now())?, stamp(time)?)
            .create();
        let mut fs = self.sftp.fs();
        fs.set_metadata(wire_path(path), times)
            .await
            .map_err(|err| VfsError::remote(err, path))
    }
}

/// Opens a file in a task of its own, so that if the caller drops the future before the
/// server answers, the task still receives the handle and closes it, instead of leaking it
/// on the server.
async fn open(options: OpenOptions, path: &RemotePath) -> Result<File, Error> {
    let path = wire_path(path).to_owned();
    match tokio::spawn(async move { options.open(path).await }).await {
        Ok(result) => result,
        Err(err) => Err(Error::IOError(io::Error::other(err))),
    }
}

/// Whether a name from the server names an entry of the directory: not `.` or `..`, and nothing
/// that, joined to the directory's path, could point elsewhere (the empty name, `/`, NUL).
fn is_entry_name(name: &[u8]) -> bool {
    !matches!(name, b"" | b"." | b"..") && !name.contains(&b'/') && !name.contains(&0)
}

/// The path to send for `path`: servers know the home directory as `.`, not as the empty path.
fn wire_path(path: &RemotePath) -> &Path {
    if path.as_bytes().is_empty() {
        Path::new(".")
    } else {
        path.as_path()
    }
}

fn convert(metadata: MetaData) -> Metadata {
    Metadata {
        kind: kind(metadata),
        size: metadata.len(),
        permissions: metadata
            .permissions()
            .map(|permissions| permissions.as_raw().bits() & 0o7777),
        modified: metadata.modified().map(UnixTimeStamp::as_system_time),
        uid: metadata.uid(),
        gid: metadata.gid(),
    }
}

fn kind(metadata: MetaData) -> FileKind {
    let Some(file_type) = metadata.file_type() else {
        return FileKind::Unknown;
    };
    match file_type.as_raw() {
        RawFileType::RegularFile => FileKind::File,
        RawFileType::Directory => FileKind::Dir,
        RawFileType::Symlink => FileKind::Symlink,
        RawFileType::FIFO => FileKind::Fifo,
        RawFileType::Socket => FileKind::Socket,
        RawFileType::BlockDevice => FileKind::BlockDevice,
        RawFileType::CharacterDevice => FileKind::CharDevice,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Stdio;
    use std::time::{Duration, SystemTime};

    use openssh_sftp_client::metadata::{MetaDataBuilder, Permissions};
    use tokio::process::{Child, ChildStdout, Command};

    use super::*;
    use crate::{LocalFs, fixture};

    /// `$SFTP_SERVER`, or the first `sftp-server` found in the usual places.
    fn sftp_server() -> Option<PathBuf> {
        if let Some(program) = std::env::var_os("SFTP_SERVER").filter(|p| !p.is_empty()) {
            return Some(program.into());
        }
        let found = [
            "/usr/libexec/sftp-server",
            "/usr/lib/openssh/sftp-server",
            "/usr/libexec/openssh/sftp-server",
            "/usr/lib/ssh/sftp-server",
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists());
        if found.is_none() {
            eprintln!("note: sftp-server not found, skipping; set SFTP_SERVER to its path");
        }
        found
    }

    /// A local `sftp-server` that starts in `dir`, and a session with it over pipes.
    struct Server {
        child: Child,
        fs: SftpFs,
    }

    impl Server {
        async fn start(dir: &Path) -> Option<Self> {
            let mut command = Command::new(sftp_server()?);
            command.arg("-e").arg("-d").arg(dir);
            Some(Self::connect(command).await)
        }

        /// Like [`Server::start`], but the server may have at most `limit` open files.
        async fn start_with_fd_limit(dir: &Path, limit: u32) -> Option<Self> {
            let mut command = Command::new("/bin/sh");
            command
                .arg("-c")
                .arg(r#"ulimit -n "$1" && exec "$2" -e -d "$3""#)
                .arg("sh")
                .arg(limit.to_string())
                .arg(sftp_server()?)
                .arg(dir);
            Some(Self::connect(command).await)
        }

        async fn connect(mut command: Command) -> Self {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let stdin = child.stdin.take().unwrap();
            let stdout = child.stdout.take().unwrap();
            let fs = SftpFs::from_pipes(stdin, stdout).await.unwrap();
            Self { child, fs }
        }

        /// Closes the session; the server exits when its input ends.
        async fn stop(self) {
            let Self { mut child, fs } = self;
            fs.close().await.unwrap();
            let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
                .await
                .unwrap()
                .unwrap();
            assert!(status.success(), "{status}");
        }
    }

    fn remote(path: &Path) -> RemotePath {
        RemotePath::new(path.as_os_str().as_bytes())
    }

    #[tokio::test]
    async fn lists_a_directory() {
        let dir = fixture::tree();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        let root = std::fs::canonicalize(dir.path()).unwrap();

        let entries = server.fs.list_dir(&remote(&root)).await.unwrap();
        fixture::check_tree(&entries, &root);
        let entries = server.fs.list_dir(&RemotePath::default()).await.unwrap();
        fixture::check_tree(&entries, &root);
        let entries = server.fs.list_dir(&remote(dir.path())).await.unwrap();
        fixture::check_tree(&entries, &root);

        for path in ["dir", "link-dir", "./link-dir/"] {
            assert_eq!(
                fixture::names(&server.fs, &RemotePath::from(path)).await,
                ["inner.txt"],
                "{path}"
            );
        }
        server.stop().await;
    }

    #[tokio::test]
    async fn lists_special_files() {
        let dir = fixture::special_files();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        let entries = server.fs.list_dir(&RemotePath::default()).await.unwrap();
        fixture::check_special_files(&entries);
        server.stop().await;
    }

    #[tokio::test]
    async fn metadata_follows_symlinks() {
        let dir = fixture::tree();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        let root = std::fs::canonicalize(dir.path()).unwrap();

        let file = server.fs.metadata(&"link-file".into()).await.unwrap();
        assert_eq!(file.kind, FileKind::File);
        assert_eq!(file.size, Some(5));
        assert_eq!(file.permissions, Some(0o640));
        let sub = server.fs.metadata(&"link-dir".into()).await.unwrap();
        assert_eq!(sub.kind, FileKind::Dir);
        let home = server.fs.metadata(&RemotePath::default()).await.unwrap();
        assert_eq!(home.kind, FileKind::Dir);
        let sub = server
            .fs
            .metadata(&remote(&root.join("dir")))
            .await
            .unwrap();
        assert_eq!(sub.kind, FileKind::Dir);

        let err = server.fs.metadata(&"dangling".into()).await.unwrap_err();
        assert!(
            matches!(&err, VfsError::NotFound(p) if p == "dangling"),
            "{err:?}"
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn changes_entries() {
        let dir = tempfile::tempdir().unwrap();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        assert!(server.fs.sftp.support_posix_rename(), "OpenSSH has it");
        fixture::check_changes(&server.fs, dir.path(), |name: &str| RemotePath::from(name)).await;
        server.stop().await;
    }

    #[tokio::test]
    async fn makes_and_reads_links() {
        let dir = tempfile::tempdir().unwrap();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        fixture::check_links(&server.fs, dir.path(), |name: &str| RemotePath::from(name)).await;
        server.stop().await;
    }

    #[tokio::test]
    async fn writes_and_reads_files() {
        let dir = tempfile::tempdir().unwrap();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        fixture::check_files(&server.fs, dir.path(), |name: &str| RemotePath::from(name)).await;
        server.stop().await;
    }

    #[tokio::test]
    async fn dropped_files_close_their_handles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big"), fixture::pattern(4 * 1024 * 1024)).unwrap();
        // Every open file holds a descriptor in the server, so leaked handles soon make
        // `open` fail for good.
        let Some(server) = Server::start_with_fd_limit(dir.path(), 16).await else {
            return;
        };
        let big = RemotePath::from("big");
        for round in 0..40 {
            // Dropped while opening, after a read with more in flight, and while writing.
            let mut opening = Box::pin(server.fs.open_file(&big));
            assert!(futures_util::poll!(opening.as_mut()).is_pending());
            drop(opening);
            let mut reader = server.fs.open_file(&big).await.unwrap();
            assert!(reader.read().await.unwrap().is_some());
            drop(reader);
            let name = RemotePath::from(format!("new-{round}").as_str());
            let mut writer = server.fs.create_file(&name, false).await.unwrap();
            writer.write(vec![1; 1024 * 1024]).await.unwrap();
            drop(writer);
        }
        let mut attempts = 0;
        loop {
            match server.fs.open_file(&big).await {
                Ok(_) => break,
                Err(err) if attempts == 100 => panic!("handles leaked: {err:?}"),
                Err(_) => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
        server.stop().await;
    }

    #[tokio::test]
    async fn sets_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        fixture::check_attributes(&server.fs, dir.path(), |name: &str| RemotePath::from(name))
            .await;
        let before_1970 = SystemTime::UNIX_EPOCH - Duration::from_secs(1);
        let err = server
            .fs
            .set_modified(&"file".into(), before_1970)
            .await
            .unwrap_err();
        assert!(matches!(&err, VfsError::Io(e) if e.kind() == io::ErrorKind::InvalidInput));
        server.stop().await;
    }

    #[tokio::test]
    async fn symlink_metadata_does_not_follow() {
        let dir = fixture::tree();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        for name in ["link-dir", "link-file", "dangling"] {
            let link = server.fs.symlink_metadata(&name.into()).await.unwrap();
            assert_eq!(link.kind, FileKind::Symlink, "{name}");
        }
        let file = server
            .fs
            .symlink_metadata(&"file.txt".into())
            .await
            .unwrap();
        assert_eq!(file.kind, FileKind::File);
        server.stop().await;
    }

    #[tokio::test]
    async fn home_is_the_start_directory() {
        let dir = fixture::tree();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        let root = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(server.fs.home().await.unwrap(), remote(&root));
        server.stop().await;
    }

    #[tokio::test]
    async fn tells_the_space_of_the_file_system() {
        let dir = fixture::tree();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        let local = LocalFs.space(&dir.path().to_path_buf()).await.unwrap();
        let space = server.fs.space(&remote(&dir.path().join("dir"))).await;
        let space = space.unwrap().expect("sftp-server has statvfs@openssh.com");
        let local = local.expect("the local space");
        assert_eq!(space.total, local.total, "the same file system");
        assert!(space.available <= space.total, "{space:?}");
        assert!(server.fs.space(&"".into()).await.unwrap().is_some(), "home");

        let err = server.fs.space(&"missing".into()).await.unwrap_err();
        assert!(matches!(err, VfsError::NotFound(_)), "{err:?}");
        server.stop().await;
    }

    #[tokio::test]
    async fn canonicalizes() {
        let dir = fixture::tree();
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let canonicalize = async |path: &str| server.fs.canonicalize(&path.into()).await;

        assert_eq!(canonicalize("").await.unwrap(), remote(&root));
        assert_eq!(
            canonicalize("link-dir").await.unwrap(),
            remote(&root.join("dir"))
        );
        assert_eq!(
            canonicalize("dir/../link-file").await.unwrap(),
            remote(&root.join("file.txt"))
        );
        assert_eq!(
            server.fs.canonicalize(&remote(dir.path())).await.unwrap(),
            remote(&root)
        );
        // Unlike the local file system, sftp-server accepts a missing last component.
        assert_eq!(
            canonicalize("missing").await.unwrap(),
            remote(&root.join("missing"))
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn reports_errors() {
        let dir = fixture::tree();
        let locked = fixture::LockedDir::new(dir.path());
        let Some(server) = Server::start(dir.path()).await else {
            return;
        };

        let err = server.fs.list_dir(&"missing".into()).await.unwrap_err();
        assert!(
            matches!(&err, VfsError::NotFound(p) if p == "missing"),
            "{err:?}"
        );
        let err = server.fs.metadata(&"missing".into()).await.unwrap_err();
        assert!(
            matches!(&err, VfsError::NotFound(p) if p == "missing"),
            "{err:?}"
        );
        // sftp-server reports ENOTDIR as "no such file".
        let err = server.fs.list_dir(&"file.txt".into()).await.unwrap_err();
        assert!(
            matches!(&err, VfsError::NotFound(p) if p == "file.txt"),
            "{err:?}"
        );
        // ENAMETOOLONG is a "bad message".
        let long = RemotePath::new("a".repeat(300));
        let err = server.fs.list_dir(&long).await.unwrap_err();
        assert!(
            matches!(
                err,
                VfsError::Sftp(openssh_sftp_client::Error::SftpError(..))
            ),
            "{err:?}"
        );
        if let Some(locked) = &locked {
            let path = remote(&locked.path);
            let err = server.fs.list_dir(&path).await.unwrap_err();
            assert!(
                matches!(&err, VfsError::PermissionDenied(p) if *p == path.display()),
                "{err:?}"
            );
        }

        // The session survives errors.
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let mut expected = fixture::TREE.to_vec();
        if locked.is_some() {
            expected.push("locked");
            expected.sort_unstable();
        }
        assert_eq!(fixture::names(&server.fs, &remote(&root)).await, expected);
        server.stop().await;
    }

    #[tokio::test]
    async fn dropped_listings_close_their_handles() {
        let dir = fixture::tree();
        // Every open directory handle holds a descriptor in the server, so leaked handles soon
        // make `opendir` fail for good.
        let Some(server) = Server::start_with_fd_limit(dir.path(), 16).await else {
            return;
        };
        let home = RemotePath::default();

        // Dropped before the server opens the directory.
        for _ in 0..50 {
            let mut listing = Box::pin(server.fs.list_dir(&home));
            assert!(futures_util::poll!(listing.as_mut()).is_pending());
        }
        // Dropped at various later points.
        for micros in [10, 100, 1000, 10_000] {
            let listing = server.fs.list_dir(&home);
            let _ = tokio::time::timeout(Duration::from_micros(micros), listing).await;
        }

        // Handles of dropped listings are closed after the server opened them.
        let mut attempts = 0;
        let entries = loop {
            match server.fs.list_dir(&home).await {
                Ok(entries) => break entries,
                Err(err) if attempts == 100 => panic!("handles leaked: {err:?}"),
                Err(_) => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        };
        let root = std::fs::canonicalize(dir.path()).unwrap();
        fixture::check_tree(&entries, &root);
        server.stop().await;
    }

    #[test]
    fn converts_partial_metadata() {
        let empty = convert(MetaDataBuilder::new().create());
        assert_eq!(empty, Metadata::of_kind(FileKind::Unknown));

        let mtime = UnixTimeStamp::from_raw(1_000_000).unwrap();
        let full = convert(
            MetaDataBuilder::new()
                .len(42)
                .id((501, 20))
                .permissions(Permissions::from(0o4755))
                .time(UnixTimeStamp::unix_epoch(), mtime)
                .create(),
        );
        assert_eq!(
            full,
            Metadata {
                // The builder cannot set the file type.
                kind: FileKind::Unknown,
                size: Some(42),
                permissions: Some(0o4755),
                modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)),
                uid: Some(501),
                gid: Some(20),
            }
        );
    }

    #[test]
    fn sends_the_home_directory_as_dot() {
        assert_eq!(wire_path(&RemotePath::default()), Path::new("."));
        assert_eq!(wire_path(&"a".into()), Path::new("a"));
        assert_eq!(wire_path(&RemotePath::root()), Path::new("/"));
    }

    #[test]
    fn keeps_only_names_of_entries() {
        for name in [&b"a"[..], b"...", b".hidden", b"a b", b"caf\xc3\xa9"] {
            assert!(is_entry_name(name), "{name:?}");
        }
        for name in [
            &b""[..],
            b".",
            b"..",
            b"/",
            b"../x",
            b"a/b",
            b"/etc",
            b"a\0b",
        ] {
            assert!(!is_entry_name(name), "{name:?}");
        }
    }

    #[test]
    fn futures_are_send() {
        fn assert_send<T: Send>(_: T) {}
        fn check(fs: SftpFs, stdin: tokio::process::ChildStdin, stdout: ChildStdout) {
            assert_send(SftpFs::from_pipes(stdin, stdout));
            assert_send(fs.home());
            assert_send(fs.list_dir(&RemotePath::root()));
            assert_send(fs.create_dir(&RemotePath::root()));
            assert_send(fs.rename(&RemotePath::root(), &RemotePath::root()));
            assert_send(fs.set_modified(&RemotePath::root(), SystemTime::now()));
            assert_send(fs.close());
        }
        let _ = check;
    }
}
