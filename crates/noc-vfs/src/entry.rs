use std::borrow::Cow;
use std::time::SystemTime;

/// The type of a file system object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link.
    Symlink,
    /// A named pipe.
    Fifo,
    /// A Unix domain socket.
    Socket,
    /// A block device.
    BlockDevice,
    /// A character device.
    CharDevice,
    /// The backend did not report the type.
    Unknown,
}

/// Metadata of a file system object; fields the backend did not report are `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// The type of the object.
    pub kind: FileKind,
    /// Size in bytes; for a symlink, the length of its target path.
    pub size: Option<u64>,
    /// Permission bits, masked with `0o7777`.
    pub permissions: Option<u32>,
    /// Time of the last modification; SFTP reports whole seconds.
    pub modified: Option<SystemTime>,
    /// User ID of the owner.
    pub uid: Option<u32>,
    /// Group ID of the owner.
    pub gid: Option<u32>,
}

impl Metadata {
    /// Metadata of which only the kind is known.
    pub(crate) fn of_kind(kind: FileKind) -> Self {
        Self {
            kind,
            size: None,
            permissions: None,
            modified: None,
            uid: None,
            gid: None,
        }
    }
}

/// An entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The file name as raw bytes: Unix and SFTP names need not be UTF-8.
    pub name: Vec<u8>,
    /// Metadata of the entry itself, not of a symlink's target.
    pub metadata: Metadata,
    /// For symlinks, the kind of the target; `None` if the link is dangling or not a symlink.
    pub target_kind: Option<FileKind>,
}

impl DirEntry {
    /// The name for display, with invalid UTF-8 replaced by `U+FFFD`.
    pub fn display_name(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.name)
    }

    /// Whether the entry is a directory or a symlink whose target is a directory.
    pub fn is_dir_like(&self) -> bool {
        match self.metadata.kind {
            FileKind::Dir => true,
            FileKind::Symlink => self.target_kind == Some(FileKind::Dir),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: FileKind, target_kind: Option<FileKind>) -> DirEntry {
        DirEntry {
            name: b"x".to_vec(),
            metadata: Metadata::of_kind(kind),
            target_kind,
        }
    }

    #[test]
    fn dir_like_entries() {
        assert!(entry(FileKind::Dir, None).is_dir_like());
        assert!(entry(FileKind::Symlink, Some(FileKind::Dir)).is_dir_like());
        assert!(!entry(FileKind::Symlink, Some(FileKind::File)).is_dir_like());
        assert!(!entry(FileKind::Symlink, None).is_dir_like());
        assert!(!entry(FileKind::File, None).is_dir_like());
        assert!(!entry(FileKind::File, Some(FileKind::Dir)).is_dir_like());
        assert!(!entry(FileKind::Unknown, None).is_dir_like());
    }

    #[test]
    fn display_name_is_lossy() {
        let mut entry = entry(FileKind::File, None);
        entry.name = b"caf\xe9.txt".to_vec();
        assert_eq!(entry.display_name(), "caf\u{fffd}.txt");
    }
}
