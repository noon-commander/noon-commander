//! Reading the start of a file for the viewer, through any [`Vfs`].

use noc_vfs::{FileReader as _, Vfs, VfsError};

/// Bytes of a file the viewer reads.
pub const LIMIT: usize = 16 * 1024 * 1024;

/// The start of the file at `path`, up to [`LIMIT`] bytes, and whether there is more.
///
/// Cancel-safe: dropping the future closes the file.
pub async fn read_start<V: Vfs>(vfs: &V, path: &V::Path) -> Result<(Vec<u8>, bool), VfsError> {
    let mut reader = vfs.open_file(path).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.read().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() >= LIMIT {
            let more = bytes.len() > LIMIT || reader.read().await?.is_some();
            bytes.truncate(LIMIT);
            return Ok((bytes, more));
        }
    }
    Ok((bytes, false))
}

#[cfg(test)]
mod tests {
    use noc_vfs::LocalFs;

    use super::*;

    #[tokio::test]
    async fn reads_up_to_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let short = dir.path().join("short");
        std::fs::write(&short, b"hello\n").unwrap();
        assert_eq!(
            read_start(&LocalFs, &short).await.unwrap(),
            (b"hello\n".to_vec(), false)
        );

        let exact = dir.path().join("exact");
        std::fs::write(&exact, vec![b'x'; LIMIT]).unwrap();
        let (bytes, more) = read_start(&LocalFs, &exact).await.unwrap();
        assert_eq!(
            (bytes.len(), more),
            (LIMIT, false),
            "a file of the limit is whole"
        );

        let long = dir.path().join("long");
        std::fs::write(&long, vec![b'x'; LIMIT + 1]).unwrap();
        let (bytes, more) = read_start(&LocalFs, &long).await.unwrap();
        assert_eq!((bytes.len(), more), (LIMIT, true));

        assert!(matches!(
            read_start(&LocalFs, &dir.path().join("missing")).await,
            Err(VfsError::NotFound(_))
        ));
    }
}
