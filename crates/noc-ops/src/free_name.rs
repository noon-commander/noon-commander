//! Free names for copies whose names are taken: the name with a number, joined to it as the
//! words of the name are joined, so that it needs no more quoting in a shell than it did.

use noc_vfs::{Vfs, VfsError, VfsPath};

/// The first of `name`, then `name` with 2, 3, and so on, that nothing has in `dir`.
pub(crate) async fn free_name<V: Vfs>(
    vfs: &V,
    dir: &V::Path,
    name: &[u8],
) -> Result<V::Path, VfsError> {
    let mut path = dir.join_name(name);
    let mut number = 2;
    loop {
        match vfs.symlink_metadata(&path).await {
            Err(VfsError::NotFound(_)) => return Ok(path),
            Ok(_) => {}
            Err(error) => return Err(error),
        }
        path = dir.join_name(&numbered(name, number));
        number += 1;
    }
}

/// `name` with `number` before its extension, after the last of a space, `_`, and `-` in what
/// comes before it, or `_` if it has none: `My Report 2.pdf`, `build-log-2.txt`,
/// `desktop_2.ini`. A number already there stays, as it may be a year or a version.
pub(crate) fn numbered(name: &[u8], number: u64) -> Vec<u8> {
    let (stem, extension) = split(name);
    let separator = stem
        .iter()
        .rev()
        .find(|&&byte| matches!(byte, b' ' | b'_' | b'-'))
        .copied()
        .unwrap_or(b'_');
    [stem, &[separator], number.to_string().as_bytes(), extension].concat()
}

/// `name` cut before its extension: from its last dot, or from `.tar` before it, which goes
/// with what compresses the archive. A dot that starts the name starts no extension.
fn split(name: &[u8]) -> (&[u8], &[u8]) {
    let dot = |part: &[u8]| {
        part.iter()
            .rposition(|&byte| byte == b'.')
            .filter(|&at| at > 0)
    };
    let Some(last) = dot(name) else {
        return (name, &[]);
    };
    if let Some(before) = dot(&name[..last])
        && name[before..last].eq_ignore_ascii_case(b".tar")
    {
        return name.split_at(before);
    }
    name.split_at(last)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use noc_vfs::{LocalFs, RemotePath};

    use super::*;
    use crate::testing;

    fn second(name: &str) -> String {
        String::from_utf8(numbered(name.as_bytes(), 2)).unwrap()
    }

    #[test]
    fn the_number_joins_the_name_as_its_words_are_joined() {
        assert_eq!(second("My Report.pdf"), "My Report 2.pdf");
        assert_eq!(second("build-log.txt"), "build-log-2.txt");
        assert_eq!(second("my-app_v1.tar.gz"), "my-app_v1_2.tar.gz");
        assert_eq!(second("Backup.TAR.ZST"), "Backup_2.TAR.ZST");
        assert_eq!(second("desktop.ini"), "desktop_2.ini");
        assert_eq!(second("Makefile"), "Makefile_2");
        assert_eq!(second("my photo-final.jpg"), "my photo-final-2.jpg");
    }

    #[test]
    fn a_dot_that_starts_the_name_starts_no_extension() {
        assert_eq!(second(".bashrc"), ".bashrc_2");
        assert_eq!(second(".config.toml"), ".config_2.toml");
        assert_eq!(second(".tar.gz"), ".tar_2.gz");
    }

    #[test]
    fn a_number_already_there_stays() {
        assert_eq!(second("photo_2024.jpg"), "photo_2024_2.jpg");
        assert_eq!(second("report_2.txt"), "report_2_2.txt");
        assert_eq!(
            String::from_utf8(numbered(b"a.txt", 12)).unwrap(),
            "a_12.txt"
        );
    }

    #[tokio::test]
    async fn the_first_free_name_counts_from_two() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_path_buf();
        let free = free_name(&LocalFs, &dir, b"a.txt").await.unwrap();
        assert_eq!(free, dir.join("a.txt"), "the name itself, while free");
        fs::write(dir.join("a.txt"), "").unwrap();
        fs::write(dir.join("a_2.txt"), "").unwrap();
        let free = free_name(&LocalFs, &dir, b"a.txt").await.unwrap();
        assert_eq!(free, dir.join("a_3.txt"));
    }

    #[tokio::test]
    async fn finds_a_free_name_over_sftp() {
        let root = tempfile::tempdir().unwrap();
        let Some((_server, sftp)) = testing::sftp_server(root.path()).await else {
            return;
        };
        fs::write(root.path().join("My Report.pdf"), "").unwrap();
        let free = free_name(&sftp, &RemotePath::default(), b"My Report.pdf")
            .await
            .unwrap();
        assert_eq!(free, RemotePath::from("My Report 2.pdf"));
    }
}
