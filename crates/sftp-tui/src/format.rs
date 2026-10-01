//! Text formatting for command-line output.

use std::time::{SystemTime, UNIX_EPOCH};

use sftp_tui_vfs::FileKind;

/// `ls -l`-style mode, e.g. `drwxr-xr-x`.
pub(crate) fn mode(kind: FileKind, permissions: Option<u32>) -> String {
    let type_char = match kind {
        FileKind::File => '-',
        FileKind::Dir => 'd',
        FileKind::Symlink => 'l',
        FileKind::Fifo => 'p',
        FileKind::Socket => 's',
        FileKind::BlockDevice => 'b',
        FileKind::CharDevice => 'c',
        FileKind::Unknown => '?',
    };
    let mut text = String::with_capacity(10);
    text.push(type_char);
    let Some(bits) = permissions else {
        text.push_str("?????????");
        return text;
    };
    let triads = [
        (0o400, 0o200, 0o100, 0o4000, 's'),
        (0o040, 0o020, 0o010, 0o2000, 's'),
        (0o004, 0o002, 0o001, 0o1000, 't'),
    ];
    for (read, write, execute, special, letter) in triads {
        text.push(if bits & read == 0 { '-' } else { 'r' });
        text.push(if bits & write == 0 { '-' } else { 'w' });
        text.push(match (bits & execute != 0, bits & special != 0) {
            (true, true) => letter,
            (false, true) => letter.to_ascii_uppercase(),
            (true, false) => 'x',
            (false, false) => '-',
        });
    }
    text
}

/// Human-readable size with binary units, e.g. `512`, `4.2K`, `17M`.
pub(crate) fn size(bytes: u64) -> String {
    const UNITS: [char; 6] = ['K', 'M', 'G', 'T', 'P', 'E'];
    if bytes < 1024 {
        return bytes.to_string();
    }
    let mut divisor: u64 = 1024;
    let mut unit = 0;
    while bytes / divisor >= 1024 && unit + 1 < UNITS.len() {
        divisor *= 1024;
        unit += 1;
    }
    let tenths = u128::from(bytes) * 10 / u128::from(divisor);
    if tenths < 100 {
        format!("{}.{}{}", tenths / 10, tenths % 10, UNITS[unit])
    } else {
        format!("{}{}", tenths / 10, UNITS[unit])
    }
}

/// `YYYY-MM-DD HH:MM` in UTC, or dashes for an unknown time.
pub(crate) fn time(time: Option<SystemTime>) -> String {
    let seconds = time
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_secs()).ok());
    let Some(seconds) = seconds else {
        return "-".repeat(16);
    };
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let minutes = seconds.rem_euclid(86_400) / 60;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        minutes / 60,
        minutes % 60
    )
}

/// Gregorian date of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn formats_modes() {
        assert_eq!(mode(FileKind::Dir, Some(0o755)), "drwxr-xr-x");
        assert_eq!(mode(FileKind::File, Some(0o644)), "-rw-r--r--");
        assert_eq!(mode(FileKind::File, Some(0o4755)), "-rwsr-xr-x");
        assert_eq!(mode(FileKind::Dir, Some(0o1777)), "drwxrwxrwt");
        assert_eq!(mode(FileKind::File, Some(0o2644)), "-rw-r-Sr--");
        assert_eq!(mode(FileKind::Symlink, None), "l?????????");
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(size(0), "0");
        assert_eq!(size(1023), "1023");
        assert_eq!(size(1024), "1.0K");
        assert_eq!(size(4300), "4.1K");
        assert_eq!(size(10 * 1024), "10K");
        assert_eq!(size(1024 * 1024 - 1), "1023K");
        assert_eq!(size(5 * 1024 * 1024 * 1024), "5.0G");
        assert_eq!(size(u64::MAX), "15E");
    }

    #[test]
    fn formats_times() {
        let at = |seconds| Some(UNIX_EPOCH + Duration::from_secs(seconds));
        assert_eq!(time(at(0)), "1970-01-01 00:00");
        assert_eq!(time(at(951_782_400)), "2000-02-29 00:00");
        assert_eq!(time(at(1_700_000_000)), "2023-11-14 22:13");
        assert_eq!(time(None), "----------------");
    }
}
