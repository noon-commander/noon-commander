//! Names spelled otherwise. A file system that ignores case takes `Name` for `name`, and one
//! that ignores Unicode normalization `é` for `e` and a combining accent, as macOS's do by
//! default, so such a name leads to an entry that its directory lists under another.

use noc_vfs::{Metadata, Vfs, VfsError, VfsPath as _};

/// Whether `other`, whose metadata is `there`, leads to `entry`, whose metadata is `here`,
/// under a name spelled otherwise: its directory does not list it byte for byte, and it is
/// the same object, by device and inode, or, where the backend does not report them, as over
/// SFTP, an entry of the same directory with the same metadata.
pub(crate) async fn spelled_otherwise<V: Vfs>(
    vfs: &V,
    (entry, here): (&V::Path, &Metadata),
    (other, there): (&V::Path, &Metadata),
) -> Result<bool, VfsError> {
    let alike = if here.inode.is_some() && there.inode.is_some() {
        here.same_object(there)
    } else {
        other.parent() == entry.parent() && here.same_attributes(there)
    };
    Ok(alike && !is_listed(vfs, other).await?)
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
    use std::path::{Path, PathBuf};

    use noc_vfs::{LocalFs, RemotePath};

    use super::*;
    use crate::testing;

    /// Whether `b` is `a` spelled otherwise, on `vfs`.
    async fn respelled<V: Vfs>(vfs: &V, a: &V::Path, b: &V::Path) -> bool {
        let here = vfs.symlink_metadata(a).await.unwrap();
        let there = vfs.symlink_metadata(b).await.unwrap();
        spelled_otherwise(vfs, (a, &here), (b, &there))
            .await
            .unwrap()
    }

    /// Whether the file system of `dir` ignores case.
    fn ignores_case(dir: &Path) -> bool {
        fs::write(dir.join("probe"), "").unwrap();
        let ignores = dir.join("PROBE").exists();
        fs::remove_file(dir.join("probe")).unwrap();
        ignores
    }

    #[tokio::test]
    async fn another_case_is_the_entry_where_the_file_system_ignores_case() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        if !ignores_case(dir) {
            return;
        }
        fs::write(dir.join("name"), "x").unwrap();
        fs::write(dir.join("other"), "y").unwrap();
        fs::hard_link(dir.join("name"), dir.join("hard")).unwrap();
        let path = |name: &str| dir.join(name);
        assert!(respelled(&LocalFs, &path("name"), &path("Name")).await);
        assert!(respelled(&LocalFs, &path("name"), &path("NAME")).await);
        assert!(
            !respelled(&LocalFs, &path("name"), &path("name")).await,
            "listed: the same name"
        );
        assert!(
            !respelled(&LocalFs, &path("name"), &path("hard")).await,
            "listed: a hard link"
        );
        assert!(
            !respelled(&LocalFs, &path("name"), &path("OTHER")).await,
            "another entry"
        );
        // A directory spelled otherwise on the way: the same object.
        let upper: PathBuf = dir
            .parent()
            .unwrap()
            .join(dir.file_name().unwrap().to_string_lossy().to_uppercase());
        assert!(respelled(&LocalFs, &path("name"), &upper.join("Name")).await);
    }

    #[tokio::test]
    async fn another_case_over_sftp_is_an_entry_alike_in_the_same_directory() {
        let root = tempfile::tempdir().unwrap();
        if !ignores_case(root.path()) {
            return;
        }
        let Some((_server, sftp)) = testing::sftp_server(root.path()).await else {
            return;
        };
        fs::write(root.path().join("name"), "x").unwrap();
        fs::write(root.path().join("other"), "yy").unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        let path = RemotePath::from;
        assert!(respelled(&sftp, &path("name"), &path("Name")).await);
        assert!(!respelled(&sftp, &path("name"), &path("name")).await);
        assert!(
            !respelled(&sftp, &path("name"), &path("OTHER")).await,
            "not alike"
        );
        assert!(
            !respelled(&sftp, &path("name"), &path("dir/../Name")).await,
            "not in the same directory, as far as the paths tell"
        );
        sftp.close().await.unwrap();
    }
}
