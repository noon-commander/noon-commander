//! Neutral formats: dates and times of ISO 8601, numbers as the C locale writes them.

use jiff::civil::{Date, DateTime};

use super::{Dates, Numbers};

#[derive(Debug)]
pub(crate) struct Iso;

impl Numbers for Iso {
    fn integer(&self, value: u64, _grouping: bool) -> String {
        value.to_string()
    }

    fn decimal(&self, value: f64, (min, max): (usize, usize), _grouping: bool) -> String {
        trim_fraction(format!("{value:.max$}"), '.', min)
    }
}

impl Dates for Iso {
    /// The whole year: ISO 8601 has no year of two digits, and `26-10-12` would read as a day
    /// first.
    fn date(&self, date: Date) -> String {
        format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
    }

    fn time(&self, at: DateTime) -> String {
        format!("{:02}:{:02}", at.hour(), at.minute())
    }
}

/// `text`, a number with `decimal` before its fraction, without the zeros at the end of the
/// fraction beyond `min` digits, and without the separator when no digit is left after it.
pub(crate) fn trim_fraction(mut text: String, decimal: char, min: usize) -> String {
    let Some(at) = text.rfind(decimal) else {
        return text;
    };
    let start = at + decimal.len_utf8();
    let mut end = text.len();
    while end > start + min && text[..end].ends_with('0') {
        end -= 1;
    }
    if end == start {
        end = at;
    }
    text.truncate(end);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_lose_their_last_zeros_down_to_the_least() {
        assert_eq!(trim_fraction("1.500".to_owned(), '.', 0), "1.5");
        assert_eq!(trim_fraction("1.000".to_owned(), '.', 0), "1");
        assert_eq!(trim_fraction("1.000".to_owned(), '.', 2), "1.00");
        assert_eq!(trim_fraction("1,50".to_owned(), ',', 1), "1,5");
        assert_eq!(trim_fraction("100".to_owned(), '.', 0), "100");
    }
}
