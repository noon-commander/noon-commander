//! The formats of macOS, as System Settings → General → Language & Region sets them, with the
//! user's own date format, separators of numbers, and 24-hour time, which the identifier of
//! the locale does not show: Foundation's formatters for the current locale apply them.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use jiff::civil::{Date, DateTime};
use objc2::rc::{Retained, autoreleasepool};
use objc2_foundation::{
    NSDate, NSDateFormatter, NSDateFormatterStyle, NSLocale, NSNumber, NSNumberFormatter,
    NSNumberFormatterStyle, NSString, NSTimeZone,
};

use super::{Dates, Numbers};

/// The identifier of the current locale, such as `en_US`, without the user's own settings.
pub(crate) fn identifier() -> String {
    autoreleasepool(|_| NSLocale::currentLocale().localeIdentifier().to_string())
}

/// Numbers as the system writes them: a formatter for each fraction and grouping asked for,
/// kept, as making one takes far longer than using it.
#[derive(Debug, Default)]
pub(crate) struct System {
    formatters: Mutex<HashMap<(usize, usize, bool), Retained<NSNumberFormatter>>>,
}

impl System {
    fn format(
        &self,
        number: &NSNumber,
        fraction: (usize, usize),
        grouping: bool,
    ) -> Option<String> {
        let mut formatters = self
            .formatters
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let formatter = formatters
            .entry((fraction.0, fraction.1, grouping))
            .or_insert_with(|| {
                let formatter = NSNumberFormatter::new();
                formatter.setLocale(Some(&NSLocale::currentLocale()));
                formatter.setNumberStyle(NSNumberFormatterStyle::DecimalStyle);
                formatter.setMinimumFractionDigits(fraction.0);
                formatter.setMaximumFractionDigits(fraction.1);
                formatter.setUsesGroupingSeparator(grouping);
                formatter
            });
        formatter
            .stringFromNumber(number)
            .map(|text| text.to_string())
    }
}

impl Numbers for System {
    fn integer(&self, value: u64, grouping: bool) -> String {
        autoreleasepool(|_| self.format(&NSNumber::new_u64(value), (0, 0), grouping))
            .unwrap_or_else(|| value.to_string())
    }

    fn decimal(&self, value: f64, fraction: (usize, usize), grouping: bool) -> String {
        autoreleasepool(|_| self.format(&NSNumber::new_f64(value), fraction, grouping))
            .unwrap_or_else(|| value.to_string())
    }
}

/// Dates and times as the system writes them in its short style: the user's own patterns,
/// with the year of the date's in two digits.
#[derive(Debug)]
pub(crate) struct SystemDates {
    date: Retained<NSDateFormatter>,
    time: Retained<NSDateFormatter>,
}

impl SystemDates {
    pub(crate) fn new() -> Self {
        autoreleasepool(|_| {
            let date = pattern(
                NSDateFormatterStyle::ShortStyle,
                NSDateFormatterStyle::NoStyle,
            );
            let time = pattern(
                NSDateFormatterStyle::NoStyle,
                NSDateFormatterStyle::ShortStyle,
            );
            Self {
                date: formatter(&short_year(&date)),
                time: formatter(&time),
            }
        })
    }
}

/// The pattern of the current locale for the styles of a date and of a time, with the user's
/// settings.
fn pattern(date: NSDateFormatterStyle, time: NSDateFormatterStyle) -> String {
    let formatter = NSDateFormatter::new();
    formatter.setLocale(Some(&NSLocale::currentLocale()));
    formatter.setDateStyle(date);
    formatter.setTimeStyle(time);
    formatter.dateFormat().to_string()
}

/// A formatter of `pattern` in the current locale, for times given in UTC: the civil times it
/// gets are already in the time zone they show.
fn formatter(pattern: &str) -> Retained<NSDateFormatter> {
    let formatter = NSDateFormatter::new();
    formatter.setLocale(Some(&NSLocale::currentLocale()));
    formatter.setTimeZone(Some(&NSTimeZone::timeZoneForSecondsFromGMT(0)));
    // After the locale and the styles, which would replace it.
    formatter.setDateFormat(Some(&NSString::from_str(pattern)));
    formatter
}

fn format(formatter: &NSDateFormatter, at: DateTime) -> String {
    let seconds = at
        .to_zoned(jiff::tz::TimeZone::UTC)
        .map_or(0, |zoned| zoned.timestamp().as_second());
    // Seconds since 1970 fit an `f64` exactly for any date a file has.
    #[allow(clippy::cast_precision_loss)]
    let seconds = seconds as f64;
    autoreleasepool(|_| {
        let date = NSDate::dateWithTimeIntervalSince1970(seconds);
        formatter.stringFromDate(&date).to_string()
    })
}

impl Dates for SystemDates {
    fn date(&self, date: Date) -> String {
        format(&self.date, date.at(12, 0, 0, 0))
    }

    fn time(&self, at: DateTime) -> String {
        format(&self.time, at)
    }
}

/// A pattern of Unicode's date formats with its year in two digits: `dd.MM.y` → `dd.MM.yy`,
/// `y-MM-dd` → `yy-MM-dd`.
pub(crate) fn short_year(pattern: &str) -> String {
    parts(pattern)
        .iter()
        .map(|part| if part.is_year() { "yy" } else { part.text() })
        .collect()
}

/// A part of a pattern: a field, a run of one letter such as `dd`, or the text between fields,
/// quoted text among it as it is written.
#[derive(Debug, PartialEq, Eq)]
enum Part<'a> {
    Field(&'a str),
    Text(&'a str),
}

impl<'a> Part<'a> {
    fn is_year(&self) -> bool {
        matches!(self, Self::Field(field) if field.starts_with(['y', 'Y', 'u', 'U', 'r']))
    }

    fn text(&self) -> &'a str {
        match self {
            Self::Field(text) | Self::Text(text) => text,
        }
    }
}

fn parts(pattern: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut chars = pattern.char_indices().peekable();
    let mut text_start = None;
    while let Some((at, c)) = chars.next() {
        if c.is_ascii_alphabetic() {
            if let Some(start) = text_start.take() {
                parts.push(Part::Text(&pattern[start..at]));
            }
            let mut end = at + 1;
            while let Some(&(next, same)) = chars.peek()
                && same == c
            {
                end = next + 1;
                chars.next();
            }
            parts.push(Part::Field(&pattern[at..end]));
        } else {
            text_start.get_or_insert(at);
            if c == '\'' {
                // Quoted text, up to the closing quote.
                for (_, quoted) in chars.by_ref() {
                    if quoted == '\'' {
                        break;
                    }
                }
            }
        }
    }
    if let Some(start) = text_start {
        parts.push(Part::Text(&pattern[start..]));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_year_takes_two_digits() {
        assert_eq!(short_year("dd.MM.y"), "dd.MM.yy");
        assert_eq!(short_year("M/d/yy"), "M/d/yy");
        assert_eq!(short_year("y-MM-dd"), "yy-MM-dd");
        assert_eq!(short_year("d. M. yyyy"), "d. M. yy");
        assert_eq!(short_year("MM/dd"), "MM/dd", "no year");
        assert_eq!(
            short_year("dd 'de y' MM 'de' y"),
            "dd 'de y' MM 'de' yy",
            "quoted text"
        );
    }

    #[test]
    fn the_formats_of_the_system_write_something() {
        let numbers = System::default();
        assert_ne!(numbers.integer(1_234_567, true), "");
        assert!(numbers.integer(1_234_567, false).contains("1234567"));
        assert_ne!(numbers.decimal(1.46, (1, 1), false), "");
        let dates = SystemDates::new();
        let at = jiff::civil::date(2025, 10, 17).at(14, 5, 0, 0);
        let date = dates.date(at.date());
        assert!(
            date.contains("17") && date.contains("25") && !date.contains("2025"),
            "{date}"
        );
        assert!(dates.time(at).contains("05"));
        assert_ne!(identifier(), "");
    }
}
