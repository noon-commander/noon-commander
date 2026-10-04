//! Text for table cells: sizes and times here, and from `noc-text` terminal-safe names,
//! widths, truncation, and wrapping.

use std::time::SystemTime;

use jiff::Timestamp;
use jiff::tz::TimeZone;
pub(crate) use noc_text::{Align, fit, sanitize, split, width, wrap};

/// Width in cells of [`mtime`].
pub(crate) const MTIME_WIDTH: usize = 12;

/// Half a year in seconds: newer times show the time of day, older ones the year.
const SIX_MONTHS: i64 = 15_778_476;

/// `bytes` with a comma between each group of three digits, as mc shows the size of marked
/// files.
pub(crate) fn grouped(bytes: u64) -> String {
    let digits = bytes.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
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
    fn groups_digits_in_threes() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(10_012_345), "10,012,345");
        assert_eq!(grouped(u64::MAX), "18,446,744,073,709,551,615");
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
