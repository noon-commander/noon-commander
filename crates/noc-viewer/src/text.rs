//! A file's bytes as lines for the screen, and the cells of a line that show.

use unicode_width::UnicodeWidthChar as _;

/// Columns between tab stops.
const TAB: usize = 8;

/// The lines of `bytes`, as UTF-8 with invalid bytes replaced: `\r\n` ends a line too, tabs
/// go to the next stop, and what a terminal would act on is shown safely.
pub(crate) fn lines_of(bytes: &[u8]) -> Vec<String> {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let pieces = pieces(bytes, threads_for(bytes.len()));
    in_parallel(&pieces, |piece| lines_in(piece))
        .into_iter()
        .flatten()
        .collect()
}

/// The rows of all `lines`, as [`row_count`] counts them, on as many threads as they are worth.
pub(crate) fn rows_of_lines(lines: &[String], first: usize, rest: usize) -> usize {
    let bytes = lines.iter().map(String::len).sum();
    let threads = threads_for(bytes);
    let pieces: Vec<&[String]> = lines.chunks(lines.len().div_ceil(threads).max(1)).collect();
    let count = |piece: &&[String]| -> usize {
        piece.iter().map(|line| row_count(line, first, rest)).sum()
    };
    in_parallel(&pieces, count).into_iter().sum()
}

/// The fewest bytes worth a thread of their own.
const PIECE: usize = 1 << 20;

/// The threads to share the work on `bytes` bytes between: one for each [`PIECE`], at most one
/// for each processor.
fn threads_for(bytes: usize) -> usize {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    threads.min(bytes / PIECE).max(1)
}

/// `work` done on each of `pieces`, each on a thread of its own if there are more than one.
fn in_parallel<P: Sync, T: Send>(pieces: &[P], work: impl Fn(&P) -> T + Sync) -> Vec<T> {
    if pieces.len() < 2 {
        return pieces.iter().map(&work).collect();
    }
    std::thread::scope(|scope| {
        let threads: Vec<_> = pieces
            .iter()
            .map(|piece| scope.spawn(|| work(piece)))
            .collect();
        threads
            .into_iter()
            .map(|thread| {
                thread
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

/// `bytes` in about `count` pieces of whole lines, without the line feeds between them.
fn pieces(bytes: &[u8], count: usize) -> Vec<&[u8]> {
    let mut pieces = Vec::with_capacity(count);
    let mut rest = bytes;
    for left in (1..count).rev() {
        let at = rest.len() / (left + 1);
        let Some(feed) = rest[at..].iter().position(|&byte| byte == b'\n') else {
            break;
        };
        pieces.push(&rest[..at + feed]);
        rest = &rest[at + feed + 1..];
    }
    pieces.push(rest);
    pieces
}

fn lines_in(bytes: &[u8]) -> Vec<String> {
    bytes.split(|&byte| byte == b'\n').map(line_of).collect()
}

/// One line of [`lines_of`], in one pass: a line feed is never part of a bad UTF-8 sequence, so
/// the bytes split into lines before they are decoded.
fn line_of(bytes: &[u8]) -> String {
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    let mut line = String::with_capacity(bytes.len());
    let mut column = 0;
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            push(&mut line, &mut column, c);
        }
        if !chunk.invalid().is_empty() {
            push(&mut line, &mut column, char::REPLACEMENT_CHARACTER);
        }
    }
    line
}

fn push(line: &mut String, column: &mut usize, c: char) {
    if c.is_ascii_graphic() || c == ' ' {
        line.push(c);
        *column += 1;
    } else if c == '\t' {
        let spaces = TAB - *column % TAB;
        line.extend(std::iter::repeat_n(' ', spaces));
        *column += spaces;
    } else {
        line.push(noc_text::safe(c));
        *column += c.width().unwrap_or(0);
    }
}

/// Where the rows of `line` start, in bytes, wrapped in a first row of at most `first` cells and
/// further rows of at most `rest`, broken anywhere, as mc wraps: at least one row, and at least
/// one character in each.
pub(crate) fn row_starts(line: &str, first: usize, rest: usize) -> Vec<usize> {
    let mut starts = vec![0];
    let (mut start, mut used, mut room) = (0, 0, first.max(1));
    for (index, c) in line.char_indices() {
        let char_width = c.width().unwrap_or(0);
        if used + char_width > room && index > start {
            starts.push(index);
            (start, used, room) = (index, 0, rest.max(1));
        }
        used += char_width;
    }
    starts
}

/// The row `row` of `line`, whose rows start at `starts`; empty past the last.
pub(crate) fn row_of<'a>(line: &'a str, starts: &[usize], row: usize) -> &'a str {
    let Some(&start) = starts.get(row) else {
        return "";
    };
    let end = starts.get(row + 1).copied().unwrap_or(line.len());
    &line[start..end]
}

/// How many rows [`row_starts`] breaks `line` into, without keeping where.
pub(crate) fn row_count(line: &str, first: usize, rest: usize) -> usize {
    let (first, rest) = (first.max(1), rest.max(1));
    // No cell takes less than a byte, and lines of [`lines_of`] in ASCII take one a byte.
    if line.len() <= first {
        return 1;
    }
    if line.is_ascii() {
        return 1 + (line.len() - first).div_ceil(rest);
    }
    let (mut count, mut used, mut room, mut empty) = (1, 0, first, true);
    for c in line.chars() {
        let char_width = c.width().unwrap_or(0);
        if used + char_width > room && !empty {
            count += 1;
            (used, room) = (0, rest.max(1));
        }
        used += char_width;
        empty = false;
    }
    count
}

/// The cells `line` takes, counted as [`cut`] counts them.
pub(crate) fn width(line: &str) -> usize {
    line.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Whether `line` takes more than `cells` cells; it stops counting there, however long the line.
pub(crate) fn wider_than(line: &str, cells: usize) -> bool {
    let mut used = 0;
    line.chars().any(|c| {
        used += c.width().unwrap_or(0);
        used > cells
    })
}

/// The cells `skip` … `skip + width` of `line`; a wide character cut in two shows as a space.
pub(crate) fn cut(line: &str, skip: usize, width: usize) -> String {
    let mut text = String::new();
    let (mut column, mut used) = (0, 0);
    for c in line.chars() {
        let char_width = c.width().unwrap_or(0);
        let start = column;
        column += char_width;
        if column <= skip {
            continue;
        }
        let shown = if start < skip {
            column - skip
        } else {
            char_width
        };
        if used + shown > width {
            break;
        }
        if shown < char_width {
            text.push_str(&" ".repeat(shown));
        } else {
            text.push(c);
        }
        used += shown;
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(line: &str, width: usize) -> Vec<&str> {
        rows_in(line, width, width)
    }

    fn rows_in(line: &str, first: usize, rest: usize) -> Vec<&str> {
        let starts = row_starts(line, first, rest);
        (0..starts.len())
            .map(|row| row_of(line, &starts, row))
            .collect()
    }

    #[test]
    fn makes_lines_safe_and_expands_tabs() {
        let lines = lines_of(b"a\tb\r\nwide\xe6\x96\x87\tx\n\x1b[2Jbad \xff\n\n");
        assert_eq!(
            lines,
            ["a       b", "wide文  x", "?[2Jbad \u{fffd}", ""],
            "one line per newline, the last ending none"
        );
        assert_eq!(lines_of(b""), [""]);
    }

    #[test]
    fn wraps_and_cuts_by_cells() {
        assert_eq!(rows("abcdefg", 3), ["abc", "def", "g"]);
        for (line, first, rest) in [
            ("abcdefg", 3, 3),
            ("", 3, 3),
            ("文文文", 5, 5),
            ("文", 1, 1),
            ("a\u{301}bc", 1, 1),
            ("abcdefg", 3, 2),
            ("abcdefgh", 4, 3),
            ("文文文文", 4, 3),
            ("文文", 2, 1),
            ("", 3, 2),
        ] {
            assert_eq!(
                row_count(line, first, rest),
                rows_in(line, first, rest).len(),
                "{line:?} in {first} then {rest}"
            );
        }
        assert_eq!(rows("", 3), [""]);
        assert_eq!(rows("文文文", 5), ["文文", "文"]);
        assert_eq!(
            rows("文", 1),
            ["文"],
            "a character wider than a row gets one"
        );
        assert_eq!(cut("abcdef", 2, 3), "cde");
        assert_eq!(
            cut("文文文", 1, 4),
            " 文",
            "a cut half is a space, and what does not fit is left out"
        );
        assert_eq!(cut("ab", 5, 3), "");
    }

    #[test]
    fn rows_after_the_first_take_their_own_width() {
        assert_eq!(rows_in("abcdefgh", 4, 3), ["abcd", "efg", "h"]);
        assert_eq!(rows_in("abcd", 4, 3), ["abcd"], "a full first row only");
        assert_eq!(
            rows_in("文文文", 4, 3),
            ["文文", "文"],
            "a wide character goes on to the next row whole"
        );
        assert_eq!(
            rows_in("文文", 2, 1),
            ["文", "文"],
            "a character wider than a row gets one"
        );
    }

    #[test]
    fn counts_cells_as_cut_does() {
        assert_eq!(width("a文\u{301}b"), 4);
        assert_eq!(width(""), 0);
        assert!(wider_than("abc", 2));
        assert!(!wider_than("abc", 3));
        assert!(wider_than("文", 1));
        assert!(!wider_than("", 0));
        assert!(!wider_than("\u{301}", 0), "nothing shows");
    }

    #[test]
    fn shares_long_text_between_threads_as_one_would_read_it() {
        let mut bytes = Vec::new();
        let mut n = 0_u32;
        while bytes.len() < 3 * PIECE {
            n += 1;
            bytes.extend_from_slice(match n % 4 {
                0 => b"\t\xe6\x96\x87x\xff\r\n",
                1 => b"\n",
                2 => b"a\x1bb\xe6\x96",
                _ => b"\x87 line\n",
            });
        }
        let one = lines_in(&bytes[..bytes.len() - 1]);
        assert_eq!(lines_of(&bytes), one);
        assert_eq!(
            pieces(b"ab\ncd\nef", 3),
            [b"ab", b"cd", b"ef"],
            "split at the line feed after a third"
        );
        assert_eq!(pieces(b"abc", 3), [b"abc"], "no line feed to split at");
        for (first, rest) in [(10, 8), (3, 1)] {
            let rows: usize = one.iter().map(|line| row_count(line, first, rest)).sum();
            assert_eq!(rows_of_lines(&one, first, rest), rows);
        }
    }
}
