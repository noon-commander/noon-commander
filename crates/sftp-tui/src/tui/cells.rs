//! Text for table cells: terminal-safe names, widths, truncation, sizes, and times.

use std::time::SystemTime;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

/// Width in cells of [`mtime`].
pub(crate) const MTIME_WIDTH: usize = 12;

/// Half a year in seconds: newer times show the time of day, older ones the year.
const SIX_MONTHS: i64 = 15_778_476;

/// Where text goes in a cell wider than the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Center,
    Right,
}

/// A name made safe for the terminal: invalid UTF-8 is replaced, and control characters and
/// bidi controls, which could move the cursor or reorder the screen, are shown as `?`.
pub(crate) fn sanitize(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if is_unsafe(c) { '?' } else { c })
        .collect()
}

fn is_unsafe(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

/// Width of `text` in terminal cells.
pub(crate) fn width(text: &str) -> usize {
    text.width()
}

/// Fits `text` into exactly `width` cells: shorter text is padded; longer text loses its
/// middle, marked with `~` as mc does.
pub(crate) fn fit(text: &str, width: usize, align: Align) -> String {
    let text_width = text.width();
    if text_width <= width {
        let gap = width - text_width;
        let (left, right) = match align {
            Align::Left => (0, gap),
            Align::Center => (gap / 2, gap - gap / 2),
            Align::Right => (gap, 0),
        };
        return format!("{}{text}{}", " ".repeat(left), " ".repeat(right));
    }
    if width == 0 {
        return String::new();
    }
    let kept = width - 1;
    let (head, head_width) = take_width(text.chars(), kept - kept / 2);
    let (tail, tail_width) = take_width(text.chars().rev(), kept / 2);
    let tail: String = tail.chars().rev().collect();
    // A wide character that does not fit leaves a gap.
    let gap = width - 1 - head_width - tail_width;
    format!("{head}~{}{tail}", " ".repeat(gap))
}

/// The longest prefix of `chars` that fits into `limit` cells, and its width.
fn take_width(chars: impl Iterator<Item = char>, limit: usize) -> (String, usize) {
    let mut taken = String::new();
    let mut used = 0;
    for c in chars {
        let char_width = c.width().unwrap_or(0);
        if used + char_width > limit {
            break;
        }
        used += char_width;
        taken.push(c);
    }
    (taken, used)
}

/// A file size in at most `width` cells: the bytes when they fit, otherwise K, M, G, …
/// (powers of 1024, rounded), as mc shows it.
pub(crate) fn size(bytes: u64, width: usize) -> String {
    let plain = bytes.to_string();
    if plain.len() <= width {
        return plain;
    }
    let mut value = bytes;
    let mut text = plain;
    for unit in ['K', 'M', 'G', 'T', 'P', 'E'] {
        value = value / 1024 + u64::from(value % 1024 >= 512);
        text = format!("{value}{unit}");
        if text.len() <= width {
            break;
        }
    }
    text
}

/// A modification time as `ls -l` and mc show it: `Sep 30 12:34` for the last six months,
/// `Sep 30  2025` otherwise, spaces when unknown. [`MTIME_WIDTH`] cells.
pub(crate) fn mtime(time: Option<SystemTime>, now: SystemTime, tz: &TimeZone) -> String {
    let Some(stamp) = time.and_then(|time| Timestamp::try_from(time).ok()) else {
        return " ".repeat(MTIME_WIDTH);
    };
    let age = Timestamp::try_from(now).map_or(0, |now| now.as_second() - stamp.as_second());
    // Up to an hour in the future still counts as recent, for clocks that differ a little.
    let format = if (-3600..SIX_MONTHS).contains(&age) {
        "%b %e %H:%M"
    } else {
        "%b %e  %Y"
    };
    stamp.to_zoned(tz.clone()).strftime(format).to_string()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn sanitize_hides_what_could_drive_the_terminal() {
        assert_eq!(sanitize(b"notes.txt"), "notes.txt");
        assert_eq!(sanitize(b"evil\x1b]0;title\x07.txt"), "evil?]0;title?.txt");
        assert_eq!(sanitize(b"two\nlines\r\t"), "two?lines??");
        assert_eq!(sanitize("del\u{7f}c1\u{9b}".as_bytes()), "del?c1?");
        assert_eq!(sanitize("txt.\u{202e}exe".as_bytes()), "txt.?exe");
        assert_eq!(sanitize(b"latin1 \xe9t\xe9"), "latin1 \u{fffd}t\u{fffd}");
        assert_eq!(sanitize("Привет 文件".as_bytes()), "Привет 文件");
    }

    #[test]
    fn widths_count_cells() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("文件"), 4);
        assert_eq!(width("é"), 1);
    }

    #[test]
    fn fit_pads_short_text() {
        assert_eq!(fit("ab", 5, Align::Left), "ab   ");
        assert_eq!(fit("ab", 5, Align::Right), "   ab");
        assert_eq!(fit("ab", 5, Align::Center), " ab  ");
        assert_eq!(fit("abcde", 5, Align::Left), "abcde");
    }

    #[test]
    fn fit_cuts_the_middle_of_long_text() {
        assert_eq!(fit("verylongname.txt", 9, Align::Left), "very~.txt");
        assert_eq!(fit("abcdef", 4, Align::Right), "ab~f");
        assert_eq!(fit("abc", 1, Align::Left), "~");
        assert_eq!(fit("abc", 0, Align::Left), "");
        let wide = fit("文件文件文件", 6, Align::Left);
        assert_eq!(width(&wide), 6, "{wide:?}");
        assert_eq!(
            wide, "文~ 件",
            "a wide character never splits; its cell stays blank"
        );
    }

    #[test]
    fn sizes_fit_their_cells() {
        assert_eq!(size(0, 7), "0");
        assert_eq!(size(9_999_999, 7), "9999999");
        assert_eq!(size(10_000_000, 7), "9766K");
        assert_eq!(size(1_536, 3), "2K");
        assert_eq!(size(5 * 1024 * 1024 * 1024, 4), "5G");
        assert!(size(u64::MAX, 7).len() <= 7, "{}", size(u64::MAX, 7));
    }

    #[test]
    fn times_show_the_time_of_day_or_the_year() {
        let at = |seconds| UNIX_EPOCH + Duration::from_secs(seconds);
        // 2023-11-14 22:13:20 UTC
        let now = at(1_700_000_000);
        assert_eq!(mtime(Some(now), now, &TimeZone::UTC), "Nov 14 22:13");
        assert_eq!(
            mtime(Some(at(1_700_000_000 - 86_400 * 40)), now, &TimeZone::UTC),
            "Oct  5 22:13"
        );
        assert_eq!(
            mtime(Some(at(1_600_000_000)), now, &TimeZone::UTC),
            "Sep 13  2020"
        );
        assert_eq!(
            mtime(Some(at(1_700_000_000 + 86_400)), now, &TimeZone::UTC),
            "Nov 15  2023",
            "far in the future"
        );
        assert_eq!(mtime(None, now, &TimeZone::UTC), " ".repeat(MTIME_WIDTH));
        for time in [now, at(1_600_000_000)] {
            assert_eq!(width(&mtime(Some(time), now, &TimeZone::UTC)), MTIME_WIDTH);
        }
        let tokyo = TimeZone::get("Asia/Tokyo").unwrap();
        assert_eq!(mtime(Some(now), now, &tokyo), "Nov 15 07:13");
    }
}
