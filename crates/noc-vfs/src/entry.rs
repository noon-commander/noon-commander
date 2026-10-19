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
    /// The device and inode numbers, the same for every path to one object: a name spelled
    /// otherwise, where the file system ignores case, or a hard link. SFTP does not report
    /// them.
    pub inode: Option<(u64, u64)>,
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
            inode: None,
        }
    }

    /// Whether `self` and `other` are of one object, by their device and inode numbers;
    /// `false` where the backend does not report them.
    pub fn same_object(&self, other: &Self) -> bool {
        matches!((self.inode, other.inode), (Some(this), Some(that)) if this == that)
    }

    /// Whether `self` and `other` tell the same of their objects, but for the device and inode
    /// numbers.
    pub fn same_attributes(&self, other: &Self) -> bool {
        (
            self.kind,
            self.size,
            self.permissions,
            self.modified,
            self.uid,
            self.gid,
        ) == (
            other.kind,
            other.size,
            other.permissions,
            other.modified,
            other.uid,
            other.gid,
        )
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
    /// For symlinks, the target as stored in the link; `None` if it cannot be read or not a
    /// symlink.
    pub link_target: Option<Vec<u8>>,
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
            link_target: None,
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
    fn objects_by_their_inodes_and_attributes_without() {
        let file = |inode| Metadata {
            size: Some(1),
            inode,
            ..Metadata::of_kind(FileKind::File)
        };
        assert!(file(Some((1, 2))).same_object(&file(Some((1, 2)))));
        assert!(!file(Some((1, 2))).same_object(&file(Some((3, 2)))));
        assert!(
            !file(None).same_object(&file(None)),
            "unknown is not the same"
        );
        assert!(file(Some((1, 2))).same_attributes(&file(None)));
        let bigger = Metadata {
            size: Some(2),
            ..file(None)
        };
        assert!(!bigger.same_attributes(&file(None)));
    }

    #[test]
    fn display_name_is_lossy() {
        let mut entry = entry(FileKind::File, None);
        entry.name = b"caf\xe9.txt".to_vec();
        assert_eq!(entry.display_name(), "caf\u{fffd}.txt");
    }
}
