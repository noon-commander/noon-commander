//! Reading a file for the viewer in parts, through any [`Vfs`].

use std::ops::ControlFlow;
use std::time::SystemTime;

use noc_vfs::{FileKind, FileReader as _, Vfs, VfsError};

use crate::text::lines_of;

/// A part of a file the viewer asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Want {
    /// Where the part starts.
    pub offset: u64,
    /// The bytes of the part, at most.
    pub length: u64,
    /// Whether the part was never read: a new part ends after its last line feed, unless the
    /// file ends with it or it has none; a known one is read as it was.
    pub new: bool,
}

/// What the file was like when a part of it was read: when it changes, the parts read before
/// no longer fit with the ones read after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// The size the file reported.
    pub size: Option<u64>,
    /// The time of its last change.
    pub modified: Option<SystemTime>,
}

/// How far the reading of a part is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The bytes read.
    pub read: u64,
    /// The bytes to read: the length of the part, or less when the file ends before.
    pub total: u64,
    /// The size of the file, when it reports one.
    pub size: Option<u64>,
}

/// A part of a file, read, and its lines made for the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// What was asked for.
    pub want: Want,
    /// The bytes read.
    pub(crate) len: u64,
    pub(crate) lines: Vec<String>,
    /// Whether the file ends with the part.
    pub end: bool,
    /// Whether the reading stopped before the part was whole.
    pub stopped: bool,
    /// The file as it was when the part was read.
    pub stamp: Stamp,
}

impl Part {
    /// The part of `bytes` read for `want`, its lines made at once: in a part of megabytes
    /// that takes long enough to be kept away from the screen.
    pub fn new(want: Want, bytes: &[u8], (end, stopped): (bool, bool), stamp: Stamp) -> Self {
        Self {
            want,
            len: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            lines: lines_of(bytes),
            end,
            stopped,
            stamp,
        }
    }

    /// Whether no byte was read.
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Reads the part of the file at `path` that `want` asks for. `report` hears how far it is
/// after each chunk, and stops the reading when it breaks: the part is then what was read, cut
/// as a new part is. Its lines are made on a thread for blocking work.
///
/// Cancel-safe: dropping the future closes the file; lines being made are thrown away.
pub async fn read_part<V: Vfs>(
    vfs: &V,
    path: &V::Path,
    want: Want,
    mut report: impl FnMut(Progress) -> ControlFlow<()>,
) -> Result<Part, VfsError> {
    let metadata = vfs.metadata(path).await?;
    let stamp = Stamp {
        size: metadata.size,
        modified: metadata.modified,
    };
    // Files of the system that make their text as they are read report no size.
    let size = metadata
        .size
        .filter(|&size| size > 0 && metadata.kind == FileKind::File);
    let total = size.map_or(want.length, |size| {
        want.length.min(size.saturating_sub(want.offset))
    });
    let limit = usize::try_from(want.length).unwrap_or(usize::MAX);
    let mut reader = vfs.open_file_at(path, want.offset).await?;
    let mut bytes = Vec::new();
    let (mut eof, mut stopped) = (false, false);
    while bytes.len() < limit {
        let Some(chunk) = reader.read().await? else {
            eof = true;
            break;
        };
        bytes.extend_from_slice(&chunk);
        let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX).min(total);
        if report(Progress { read, total, size }).is_break() {
            stopped = bytes.len() < limit;
            break;
        }
    }
    let over = bytes.len() > limit;
    bytes.truncate(limit);
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let end = !over && (eof || size.is_some_and(|size| want.offset + read >= size));
    bytes.truncate(whole_lines(&bytes, want, end));
    tokio::task::spawn_blocking(move || Part::new(want, &bytes, (end, stopped), stamp))
        .await
        .map_err(|error| VfsError::Io(std::io::Error::other(error)))
}

/// The part of `text`, held whole in memory rather than read from a file, that `want` asks
/// for, cut as a part of a file is. Its lines are made at once, on the thread that asks: for
/// text such as the lines of a checksum file, small next to a file of megabytes.
pub fn part_of(text: &[u8], want: Want) -> Part {
    let start = usize::try_from(want.offset)
        .unwrap_or(usize::MAX)
        .min(text.len());
    let limit = usize::try_from(want.length).unwrap_or(usize::MAX);
    let bytes = &text[start..start.saturating_add(limit).min(text.len())];
    let end = start + bytes.len() == text.len();
    let bytes = &bytes[..whole_lines(bytes, want, end)];
    let stamp = Stamp {
        size: Some(u64::try_from(text.len()).unwrap_or(u64::MAX)),
        modified: None,
    };
    Part::new(want, bytes, (end, false), stamp)
}

/// How many of `bytes`, read for `want`, the part keeps: a new part that is not the end ends
/// after its last line feed, unless it has none.
fn whole_lines(bytes: &[u8], want: Want, end: bool) -> usize {
    match bytes.iter().rposition(|&byte| byte == b'\n') {
        Some(last) if want.new && !end => last + 1,
        _ => bytes.len(),
    }
}

#[cfg(test)]
mod tests {
    use noc_vfs::LocalFs;

    use super::*;

    fn new(offset: u64, length: u64) -> Want {
        Want {
            offset,
            length,
            new: true,
        }
    }

    fn lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|&line| line.to_owned()).collect()
    }

    async fn read(path: &std::path::PathBuf, want: Want) -> Part {
        read_part(&LocalFs, path, want, |_| ControlFlow::Continue(()))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_new_part_ends_after_its_last_line_feed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"one\ntwo\nthree\n").unwrap();
        let part = read(&path, new(0, 10)).await;
        assert_eq!(
            (part.lines, part.len, part.end),
            (lines(&["one", "two"]), 8, false)
        );
        assert_eq!(part.stamp.size, Some(14));
        let part = read(&path, new(8, 10)).await;
        assert_eq!(
            (part.lines, part.len, part.end),
            (lines(&["three"]), 6, true)
        );
        let whole = read(&path, new(0, 14)).await;
        assert!(whole.end, "a file of the length is whole");

        let known = Want {
            new: false,
            ..new(0, 10)
        };
        assert_eq!(read(&path, known).await.lines, lines(&["one", "two", "th"]));
    }

    #[tokio::test]
    async fn a_line_longer_than_a_part_is_cut_and_the_last_need_not_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"abcdefgh\nij").unwrap();
        assert_eq!(read(&path, new(0, 4)).await.lines, lines(&["abcd"]));
        let last = read(&path, new(9, 4)).await;
        assert_eq!((last.lines, last.end), (lines(&["ij"]), true));
        let past = read(&path, new(20, 4)).await;
        assert!(past.is_empty() && past.end);
    }

    #[test]
    fn text_in_memory_is_cut_into_parts_as_a_file_is() {
        let text = b"one\ntwo\nthree\n";
        let part = part_of(text, new(0, 10));
        assert_eq!(
            (part.lines, part.len, part.end),
            (lines(&["one", "two"]), 8, false)
        );
        assert_eq!(part.stamp.size, Some(14));
        let last = part_of(text, new(8, 10));
        assert_eq!(
            (last.lines, last.len, last.end),
            (lines(&["three"]), 6, true)
        );
        let known = Want {
            new: false,
            ..new(0, 10)
        };
        assert_eq!(part_of(text, known).lines, lines(&["one", "two", "th"]));
        let beyond = part_of(text, new(20, 4));
        assert!(beyond.is_empty() && beyond.end);
        assert!(part_of(b"", new(0, 4)).end, "nothing is whole");
    }

    #[tokio::test]
    async fn stops_when_the_report_breaks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        // More than a chunk of the local reader.
        let text = "line\n".repeat(200_000);
        std::fs::write(&path, &text).unwrap();
        let mut reports = Vec::new();
        let part = read_part(&LocalFs, &path, new(0, 1 << 20), |progress| {
            reports.push(progress);
            ControlFlow::Break(())
        })
        .await
        .unwrap();
        assert!(part.stopped && !part.end);
        assert!(!part.is_empty() && part.len % 5 == 0, "whole lines");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].total, 1_000_000);
        assert_eq!(reports[0].size, Some(1_000_000));

        assert!(matches!(
            read_part(&LocalFs, &dir.path().join("missing"), new(0, 1), |_| {
                ControlFlow::Continue(())
            })
            .await,
            Err(VfsError::NotFound(_))
        ));
    }
}
