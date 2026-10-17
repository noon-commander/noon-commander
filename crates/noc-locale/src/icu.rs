//! The formats of a locale from CLDR's data, through ICU4X: for a language tag in the setting,
//! for the variables of the environment, and the order of names everywhere.

use std::cmp::Ordering;

use icu_calendar::Gregorian;
use icu_collator::options::CollatorOptions;
use icu_collator::preferences::CollationNumericOrdering;
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_datetime::fieldsets::{T, YMD};
use icu_datetime::input::{Date as IcuDate, Time};
use icu_datetime::{
    DateTimeFormatterPreferences, FixedCalendarDateTimeFormatter, NoCalendarFormatter,
};
use icu_decimal::DecimalFormatter;
use icu_decimal::input::{Decimal, FloatPrecision};
use icu_decimal::options::{DecimalFormatterOptions, GroupingStrategy};
use icu_locale_core::Locale;
use jiff::civil::{Date, DateTime};

use super::{Dates, Numbers};

/// A locale from a language tag, or from a POSIX locale such as `xx_YY.UTF-8@variant`, whose
/// codeset and modifier ICU4X has no use for; `None` for `C` and `POSIX`, which are neutral,
/// and for what is no locale.
pub(crate) fn locale(name: &str) -> Option<Locale> {
    let name = name.split(['.', '@']).next().unwrap_or_default();
    if name.is_empty() || name == "C" || name == "POSIX" {
        return None;
    }
    Locale::try_from_str(&name.replace('_', "-")).ok()
}

/// Numbers of a locale.
#[derive(Debug)]
pub(crate) struct IcuNumbers {
    grouped: DecimalFormatter,
    plain: DecimalFormatter,
}

impl IcuNumbers {
    pub(crate) fn new(locale: &Locale) -> Option<Self> {
        Some(Self {
            grouped: DecimalFormatter::try_new(locale.into(), DecimalFormatterOptions::default())
                .ok()?,
            plain: DecimalFormatter::try_new(locale.into(), GroupingStrategy::Never.into()).ok()?,
        })
    }

    fn formatter(&self, grouping: bool) -> &DecimalFormatter {
        if grouping { &self.grouped } else { &self.plain }
    }
}

impl Numbers for IcuNumbers {
    fn integer(&self, value: u64, grouping: bool) -> String {
        self.formatter(grouping)
            .format(&Decimal::from(value))
            .to_string()
    }

    fn decimal(&self, value: f64, (min, max): (usize, usize), grouping: bool) -> String {
        let digits = |count: usize| i16::try_from(count).unwrap_or(i16::MAX).saturating_neg();
        let Ok(mut decimal) = Decimal::try_from_f64(value, FloatPrecision::Magnitude(digits(max)))
        else {
            return value.to_string();
        };
        decimal.trim_end();
        decimal.pad_end(digits(min));
        self.formatter(grouping).format(&decimal).to_string()
    }
}

/// Dates and times of a locale, in digits: the short year, month, and day, whose year has two
/// digits where the locale's short form has them, from about 1950 to 2049, and hour and minute
/// in the locale's hour cycle.
#[derive(Debug)]
pub(crate) struct IcuDates {
    date: FixedCalendarDateTimeFormatter<Gregorian, YMD>,
    time: NoCalendarFormatter<T>,
}

impl IcuDates {
    pub(crate) fn new(locale: &Locale) -> Option<Self> {
        let preferences = DateTimeFormatterPreferences::from(locale);
        Some(Self {
            date: FixedCalendarDateTimeFormatter::try_new(preferences, YMD::short()).ok()?,
            time: NoCalendarFormatter::try_new(preferences, T::hm()).ok()?,
        })
    }
}

/// `date` as ICU4X takes it.
fn icu_date(date: Date) -> Option<IcuDate<Gregorian>> {
    let month = u8::try_from(date.month()).ok()?;
    let day = u8::try_from(date.day()).ok()?;
    IcuDate::try_new_gregorian(i32::from(date.year()), month, day).ok()
}

impl Dates for IcuDates {
    fn date(&self, date: Date) -> String {
        icu_date(date).map_or_else(String::new, |date| self.date.format(&date).to_string())
    }

    fn time(&self, at: DateTime) -> String {
        let hour = u8::try_from(at.hour()).unwrap_or_default();
        let minute = u8::try_from(at.minute()).unwrap_or_default();
        Time::try_new(hour, minute, 0, 0).map_or_else(
            |_| String::new(),
            |time| self.time.format(&time).to_string(),
        )
    }
}

/// The order of names in a language, with the numbers in them by value.
#[derive(Debug)]
pub(crate) struct Names(Option<CollatorBorrowed<'static>>);

impl Names {
    /// For `locale`, or Unicode's root order without one.
    pub(crate) fn new(locale: Option<&Locale>) -> Self {
        let mut preferences =
            locale.map_or_else(CollatorPreferences::default, CollatorPreferences::from);
        preferences.numeric_ordering = Some(CollationNumericOrdering::True);
        Self(Collator::try_new(preferences, CollatorOptions::default()).ok())
    }

    pub(crate) fn compare(&self, a: &str, b: &str) -> Ordering {
        match &self.0 {
            Some(collator) => collator.compare(a, b),
            None => a.cmp(b),
        }
    }

    /// The bytes that order `name` as [`Self::compare`] does, which compare faster.
    pub(crate) fn key(&self, name: &str) -> Vec<u8> {
        let Some(collator) = &self.0 else {
            return name.as_bytes().to_vec();
        };
        let mut key = Vec::new();
        let Ok(()) = collator.write_sort_key_to(name, &mut key);
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_locales_lose_their_codeset_and_modifier() {
        assert_eq!(locale("und"), Some(Locale::UNKNOWN));
        assert_eq!(locale("und_ZZ.UTF-8@x"), locale("und-ZZ"));
        assert!(locale("und-ZZ").is_some());
        assert_eq!(locale("C"), None);
        assert_eq!(locale("C.UTF-8"), None);
        assert_eq!(locale("POSIX"), None);
        assert_eq!(locale(""), None);
        assert_eq!(locale("not a locale"), None);
    }

    #[test]
    fn root_numbers_round_and_group() {
        let numbers = IcuNumbers::new(&Locale::UNKNOWN).unwrap();
        assert_eq!(numbers.integer(1_234_567, false), "1234567");
        assert_eq!(numbers.decimal(1.46, (1, 1), false), "1.5");
        assert_eq!(numbers.decimal(1.5, (0, 3), false), "1.5");
        assert_eq!(numbers.decimal(2.0, (0, 3), false), "2");
        assert_eq!(numbers.decimal(2.0, (2, 2), false), "2.00");
        assert_ne!(numbers.integer(1_234_567, true), "1234567", "grouped");
    }

    #[test]
    fn root_dates_are_digits() {
        let dates = IcuDates::new(&Locale::UNKNOWN).unwrap();
        let at = jiff::civil::date(2025, 3, 7).at(9, 5, 0, 0);
        for text in [dates.date(at.date()), dates.time(at)] {
            assert_ne!(text, "");
            assert!(!text.chars().any(char::is_alphabetic), "{text}");
        }
        assert!(dates.date(at.date()).contains("25"));
    }

    #[test]
    fn names_compare_numbers_by_value() {
        let names = Names::new(None);
        assert_eq!(names.compare("file2", "file10"), Ordering::Less);
        assert_eq!(names.compare("a", "B"), Ordering::Less);
        assert_eq!(
            names.compare("e\u{301}", "\u{e9}"),
            Ordering::Equal,
            "NFD and NFC"
        );
        assert_eq!(names.compare("\u{e9}", "f"), Ordering::Less);
        let mut sorted = ["file10", "B", "file2", "a", "\u{e9}"];
        sorted.sort_by_key(|name| names.key(name));
        assert_eq!(sorted, ["a", "B", "\u{e9}", "file2", "file10"]);
    }
}
