//! Reading a file for the viewer in parts, through any [`Vfs`].

use std::ops::{ControlFlow, Range};
use std::time::SystemTime;

use noc_vfs::{FileKind, FileReader as _, Vfs, VfsError};

use crate::text::lines_of;

/// A part of a file the viewer asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Want {
    /// Where the part starts.
    pub offset: u64,
    /// The bytes of the part, at most, unless a line goes on past them.
    pub length: u64,
    /// The bytes of a new part at most when it has no line feed in `length` bytes: it then
    /// goes on to the first, so that a line longer than a part stays whole, and is cut only
    /// here. No less than `length`.
    pub most: u64,
    /// Whether the part was never read: a new part ends after its last line feed, unless the
    /// file ends with it or it has none; a known one is read as it was.
    pub new: bool,
    /// Whether the file was not read up to `offset`, as after a jump, so that a line may be
    /// under way there: the part then starts after the first line feed from the byte before
    /// `offset` on, at the start of a line, unless nothing follows that line feed.
    pub after_feed: bool,
    /// Whether the part is read ahead of the screen, which waits for nothing: its reading shows
    /// no window.
    pub ahead: bool,
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
    /// Where it starts: where it was asked for, or after the line feed it was to start after.
    pub(crate) offset: u64,
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
    /// The part of `bytes` read for `want`, from where it asks on, its lines made at once: in
    /// a part of megabytes that takes long enough to be kept away from the screen.
    pub fn new(want: Want, bytes: &[u8], ends: (bool, bool), stamp: Stamp) -> Self {
        Self::at(want.offset, want, bytes, ends, stamp)
    }

    /// The part of `bytes`, which start at `offset`, read for `want`.
    fn at(
        offset: u64,
        want: Want,
        bytes: &[u8],
        (end, stopped): (bool, bool),
        stamp: Stamp,
    ) -> Self {
        Self {
            want,
            offset,
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
    let start = start_of(want);
    let before = usize::try_from(want.offset - start).unwrap_or(usize::MAX);
    let bytes_of = |length: u64| {
        usize::try_from(length)
            .unwrap_or(usize::MAX)
            .saturating_add(before)
    };
    let (limit, most) = (bytes_of(want.length), bytes_of(want.most.max(want.length)));
    let mut reader = vfs.open_file_at(path, start).await?;
    let mut bytes = Vec::new();
    let (mut eof, mut stopped) = (false, false);
    // Whether a line feed came that the part can end after; without one, a new part reads on.
    let mut feed = !want.new;
    let wanted = |read: usize, feed: bool| read < if feed { limit } else { most };
    while wanted(bytes.len(), feed) {
        let Some(chunk) = reader.read().await? else {
            eof = true;
            break;
        };
        let known = bytes.len().max(before);
        bytes.extend_from_slice(&chunk);
        feed = feed || bytes.get(known..).is_some_and(|new| new.contains(&b'\n'));
        let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX).min(total);
        if report(Progress { read, total, size }).is_break() {
            stopped = wanted(bytes.len(), feed);
            break;
        }
    }
    let kept = taken(&bytes, before, limit, most, want.new);
    let over = bytes.len() > kept;
    bytes.truncate(kept);
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let end = !over && (eof || size.is_some_and(|size| start + read >= size));
    let (offset, kept) = cut(want, start, &bytes, end);
    tokio::task::spawn_blocking(move || Part::at(offset, want, &bytes[kept], (end, stopped), stamp))
        .await
        .map_err(|error| VfsError::Io(std::io::Error::other(error)))
}

/// The part of `text`, held whole in memory rather than read from a file, that `want` asks
/// for, cut as a part of a file is. Its lines are made at once, on the thread that asks: for
/// text such as the lines of a checksum file, small next to a file of megabytes.
pub fn part_of(text: &[u8], want: Want) -> Part {
    let first = start_of(want);
    let start = usize::try_from(first).unwrap_or(usize::MAX).min(text.len());
    let before = usize::try_from(want.offset - first).unwrap_or(usize::MAX);
    let bytes_of = |length: u64| {
        usize::try_from(length)
            .unwrap_or(usize::MAX)
            .saturating_add(before)
    };
    let (limit, most) = (bytes_of(want.length), bytes_of(want.most.max(want.length)));
    let rest = &text[start..];
    let bytes = &rest[..taken(rest, before, limit, most, want.new)];
    let end = start + bytes.len() == text.len();
    let (offset, kept) = cut(want, first, bytes, end);
    let stamp = Stamp {
        size: Some(u64::try_from(text.len()).unwrap_or(u64::MAX)),
        modified: None,
    };
    Part::at(offset, want, &bytes[kept], (end, false), stamp)
}

/// How many of `bytes`, which start `before` bytes before the part, a part of `limit` bytes
/// takes: a `new` one whose first line goes on past them goes on to its line feed, for `most`
/// bytes at most.
fn taken(bytes: &[u8], before: usize, limit: usize, most: usize, new: bool) -> usize {
    let bytes = &bytes[..bytes.len().min(most)];
    if !new || bytes.len() <= limit {
        return bytes.len().min(limit);
    }
    let feed = bytes.iter().skip(before).position(|&byte| byte == b'\n');
    match feed.map(|feed| before + feed) {
        Some(feed) if feed >= limit => feed + 1,
        Some(_) => limit,
        None => bytes.len(),
    }
}

/// Where the reading for `want` starts: a byte before the part that is to start after a line
/// feed, so that a line feed right before it counts.
fn start_of(want: Want) -> u64 {
    want.offset - u64::from(want.after_feed && want.offset > 0)
}

/// The part that `want` asks for of `bytes`, read from `start` on: where it starts in the
/// file, and which of the bytes it keeps. One that starts after a line feed starts after the
/// first, unless nothing follows it: then, as without any, where it was asked for.
fn cut(want: Want, start: u64, bytes: &[u8], end: bool) -> (u64, Range<usize>) {
    let before = usize::try_from(want.offset.saturating_sub(start)).unwrap_or(usize::MAX);
    let skip = match bytes.iter().position(|&byte| byte == b'\n') {
        Some(feed) if before > 0 && feed + 1 < bytes.len() => feed + 1,
        _ => before.min(bytes.len()),
    };
    let kept = whole_lines(&bytes[skip..], want, end);
    let offset = start + u64::try_from(skip).unwrap_or(u64::MAX);
    (offset, skip..skip + kept)
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
            most: length,
            new: true,
            after_feed: false,
            ahead: false,
        }
    }

    /// A part that starts after a line feed.
    fn jump(offset: u64, length: u64) -> Want {
        Want {
            after_feed: true,
            ..new(offset, length)
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

    #[tokio::test]
    async fn a_line_that_goes_on_past_a_part_stays_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        let text = b"abcdefgh\nij\nkl";
        std::fs::write(&path, text).unwrap();
        let most = |most, want| Want { most, ..want };
        for part in [
            read(&path, most(100, new(0, 4))).await,
            part_of(text, most(100, new(0, 4))),
        ] {
            assert_eq!(
                (part.lines, part.len, part.end),
                (lines(&["abcdefgh"]), 9, false),
                "on to its line feed"
            );
        }
        for part in [
            read(&path, most(6, new(0, 4))).await,
            part_of(text, most(6, new(0, 4))),
        ] {
            assert_eq!(part.lines, lines(&["abcdef"]), "no further than it may");
        }
        let part = read(&path, most(100, new(0, 10))).await;
        assert_eq!(part.len, 9, "a part with a line feed ends after it");
        let part = read(&path, most(100, new(12, 1))).await;
        assert_eq!((part.lines, part.end), (lines(&["kl"]), true), "to the end");
        let known = Want {
            new: false,
            ..most(100, new(0, 4))
        };
        assert_eq!(
            read(&path, known).await.lines,
            lines(&["abcd"]),
            "a known part is read as it was"
        );
        // The line feed before a part that starts after one is not its own.
        let part = read(&path, most(100, jump(9, 1))).await;
        assert_eq!((part.offset, part.lines), (9, lines(&["ij"])));
    }

    #[tokio::test]
    async fn a_part_from_the_middle_of_a_file_starts_after_a_line_feed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"one\ntwo\nthree\nfour\n").unwrap();
        let part = read(&path, jump(5, 14)).await;
        assert_eq!(
            (part.offset, part.lines, part.end),
            (8, lines(&["three", "four"]), true),
            "in the middle of a line: from the next one on"
        );
        let part = read(&path, jump(8, 6)).await;
        assert_eq!(
            (part.offset, part.lines, part.len),
            (8, lines(&["three"]), 6),
            "the line feed before it counts"
        );
        let before = Want {
            new: false,
            ..jump(5, 9)
        };
        let part = read(&path, before).await;
        assert_eq!((part.offset, part.len), (8, 6), "it ends where asked");
        let part = read(&path, jump(9, 3)).await;
        assert_eq!(
            (part.offset, part.lines),
            (9, lines(&["hre"])),
            "no line feed: where it was asked for"
        );
        let part = read(&path, jump(16, 3)).await;
        assert_eq!(
            (part.offset, part.lines, part.end),
            (16, lines(&["ur"]), true),
            "nothing after the line feed: where it was asked for"
        );
        let part = read(&path, jump(0, 8)).await;
        assert_eq!(
            (part.offset, part.lines),
            (0, lines(&["one", "two"])),
            "no byte before the start of the file"
        );
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
        let after = part_of(text, jump(5, 9));
        assert_eq!(
            (after.offset, after.lines, after.end),
            (8, lines(&["three"]), true),
            "from the middle of a line"
        );
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
            Err(VfsError::NotFound(..))
        ));
    }
}
