//! Renaming one entry in its directory.

use noc_vfs::{FileKind, Vfs, VfsError, VfsPath as _};

/// Why [`rename`] left everything as it was.
#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    /// Another entry, of this kind, has the new name.
    #[error("the name is taken")]
    Exists(FileKind),
    #[error(transparent)]
    Vfs(#[from] VfsError),
}

/// Renames `from` to `to`, a new name in the same directory. A name that another entry has is
/// [`RenameError::Exists`] unless `replace`, and a directory is never replaced. A name that
/// differs only in case or Unicode normalization, on a file system that does not tell them
/// apart, is the entry itself, which takes the new spelling.
pub async fn rename<V: Vfs>(
    vfs: &V,
    from: &V::Path,
    to: &V::Path,
    replace: bool,
) -> Result<(), RenameError> {
    let source = vfs.symlink_metadata(from).await?;
    let taken = match vfs.symlink_metadata(to).await {
        Ok(existing) if is_listed(vfs, to).await? => Some(existing.kind),
        // Not there, or another spelling of `from` itself.
        Err(VfsError::NotFound(_)) | Ok(_) => None,
        Err(error) => return Err(error.into()),
    };
    match taken {
        Some(kind) if !replace || kind == FileKind::Dir => return Err(RenameError::Exists(kind)),
        // No rename puts a directory in a file's place.
        Some(_) if source.kind == FileKind::Dir => vfs.remove_file(to).await?,
        Some(_) | None => {}
    }
    match vfs.rename(from, to).await {
        // Without posix-rename the target goes first.
        Err(VfsError::AlreadyExists(_)) if taken.is_some() => {
            vfs.remove_file(to).await?;
            vfs.rename(from, to).await?;
        }
        result => result?,
    }
    Ok(())
}

/// Whether the directory of `path` lists its name byte for byte, not only a name that the file
/// system takes for the same.
async fn is_listed<V: Vfs>(vfs: &V, path: &V::Path) -> Result<bool, VfsError> {
    let (Some(dir), Some(name)) = (path.parent(), path.name()) else {
        return Ok(true);
    };
    let entries = vfs.list_dir(&dir).await?;
    Ok(entries.iter().any(|entry| entry.name == name))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use noc_vfs::{LocalFs, RemotePath};

    use super::*;
    use crate::testing;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn renames_to_a_free_name() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::create_dir(dir.join("d")).unwrap();
        rename(&LocalFs, &dir.join("a.txt"), &dir.join("b.txt"), false)
            .await
            .unwrap();
        rename(&LocalFs, &dir.join("d"), &dir.join("e"), false)
            .await
            .unwrap();
        assert_eq!(names(dir), ["b.txt", "e"]);
        assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "a");
    }

    #[tokio::test]
    async fn a_taken_name_needs_replace_and_a_directory_stays() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        fs::write(dir.join("a"), "a").unwrap();
        fs::write(dir.join("b"), "b").unwrap();
        fs::create_dir(dir.join("d")).unwrap();
        let taken = rename(&LocalFs, &dir.join("a"), &dir.join("b"), false).await;
        assert!(matches!(taken, Err(RenameError::Exists(FileKind::File))));
        let taken = rename(&LocalFs, &dir.join("a"), &dir.join("d"), true).await;
        assert!(matches!(taken, Err(RenameError::Exists(FileKind::Dir))));
        assert_eq!(names(dir), ["a", "b", "d"]);

        rename(&LocalFs, &dir.join("a"), &dir.join("b"), true)
            .await
            .unwrap();
        assert_eq!(names(dir), ["b", "d"]);
        assert_eq!(fs::read_to_string(dir.join("b")).unwrap(), "a");

        // A directory takes the place of a file too.
        rename(&LocalFs, &dir.join("d"), &dir.join("b"), true)
            .await
            .unwrap();
        assert_eq!(names(dir), ["b"]);
        assert!(dir.join("b").is_dir());
    }

    #[tokio::test]
    async fn a_new_case_renames_the_entry_itself() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        fs::write(dir.join("name"), "x").unwrap();
        if dir.join("NAME").exists() {
            // The file system ignores case: `NAME` is `name`.
            rename(&LocalFs, &dir.join("name"), &dir.join("Name"), false)
                .await
                .unwrap();
            assert_eq!(names(dir), ["Name"]);
        } else {
            // It tells them apart: `Name` is another file.
            fs::write(dir.join("Name"), "y").unwrap();
            let taken = rename(&LocalFs, &dir.join("name"), &dir.join("Name"), false).await;
            assert!(matches!(taken, Err(RenameError::Exists(FileKind::File))));
        }
    }

    #[tokio::test]
    async fn renames_over_sftp() {
        let root = tempfile::tempdir().unwrap();
        let Some((_server, sftp)) = testing::sftp_server(root.path()).await else {
            return;
        };
        fs::write(root.path().join("a"), "a").unwrap();
        fs::write(root.path().join("b"), "b").unwrap();
        let (a, b, c) = (
            RemotePath::from("a"),
            RemotePath::from("b"),
            RemotePath::from("c"),
        );
        let taken = rename(&sftp, &a, &b, false).await;
        assert!(matches!(taken, Err(RenameError::Exists(FileKind::File))));
        rename(&sftp, &a, &c, false).await.unwrap();
        rename(&sftp, &c, &b, true).await.unwrap();
        assert_eq!(names(root.path()), ["b"]);
        assert_eq!(fs::read_to_string(root.path().join("b")).unwrap(), "a");
    }
}
