//! The formats of the user's region (ADR 0035): numbers, dates, and times as the system shows
//! them, and names in the order of its language, with the numbers in them by value.
//!
//! `ui.formats` picks them: `auto` follows the system, `iso` is neutral, and a language tag
//! names the formats of a region in CLDR. No language or region is special here: every format
//! comes from the system or from CLDR's data.
//!
//! The formats in use are global, as the language of the messages is, so that the numbers in
//! messages ([`fluent`]) and those drawn without one agree. Tests never change them: they see
//! [`Formats::iso`], and give other formats to the functions that take them.

use std::cmp::Ordering;
use std::fmt;
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use fluent_bundle::FluentValue;
use intl_memoizer::concurrent::IntlLangMemoizer;
use jiff::civil::{Date, DateTime};
use unicode_width::UnicodeWidthStr;

mod detect;
mod icu;
mod iso;
#[cfg(target_os = "macos")]
mod system;

pub use detect::is_valid;

/// Where a part of the formats comes from, as Help → About tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Neutral: ISO 8601 dates and times, numbers as the C locale writes them, names in the
    /// root order of Unicode.
    Iso,
    /// A language tag in `ui.formats`.
    Setting(String),
    /// A variable of the environment, such as `LC_TIME`, and the locale it names.
    Environment {
        variable: &'static str,
        locale: String,
    },
    /// The settings of the system, with the identifier of its locale.
    System(String),
}

/// Numbers in the formats of a region.
trait Numbers: fmt::Debug + Send + Sync {
    /// `value`, grouped by thousands if `grouping`.
    fn integer(&self, value: u64, grouping: bool) -> String;

    /// `value` rounded to `fraction.1` digits after the decimal separator, without zeros at the
    /// end beyond `fraction.0`, grouped if `grouping`.
    fn decimal(&self, value: f64, fraction: (usize, usize), grouping: bool) -> String;
}

/// Dates and times in the formats of a region, all in digits.
trait Dates: fmt::Debug + Send + Sync {
    /// The year, the month, and the day, in the short form of the region, with the last two
    /// digits of the year.
    fn date(&self, date: Date) -> String;

    /// The hour and the minute.
    fn time(&self, at: DateTime) -> String;
}

/// The formats of a region: how numbers, dates, and times are written, and how names are
/// ordered.
#[derive(Debug)]
pub struct Formats {
    numbers: Box<dyn Numbers>,
    numbers_from: Origin,
    dates: Box<dyn Dates>,
    dates_from: Origin,
    names: icu::Names,
    names_from: Origin,
    /// The most cells of [`Self::date_time`].
    width: usize,
}

impl Formats {
    /// Neutral formats: `2025-10-17 14:05`, `1234567.5`, names in Unicode's root order.
    pub fn iso() -> Self {
        Self::new(
            (Box::new(iso::Iso), Origin::Iso),
            (Box::new(iso::Iso), Origin::Iso),
            (icu::Names::new(None), Origin::Iso),
        )
    }

    /// The formats `setting` names, as `ui.formats` holds it: `auto` for the system's, `iso`,
    /// or a language tag. What cannot be made falls back to [`Self::iso`]. It reads the
    /// environment and asks the system, so it blocks.
    pub fn detect(setting: &str) -> Self {
        detect::detect(setting)
    }

    fn new(
        (numbers, numbers_from): (Box<dyn Numbers>, Origin),
        (dates, dates_from): (Box<dyn Dates>, Origin),
        (names, names_from): (icu::Names, Origin),
    ) -> Self {
        let mut formats = Self {
            numbers,
            numbers_from,
            dates,
            dates_from,
            names,
            names_from,
            width: 0,
        };
        formats.width = formats.measure();
        formats
    }

    /// Where the formats of numbers come from.
    pub fn numbers_from(&self) -> &Origin {
        &self.numbers_from
    }

    /// Where the formats of dates and times come from.
    pub fn dates_from(&self) -> &Origin {
        &self.dates_from
    }

    /// Where the order of names comes from.
    pub fn names_from(&self) -> &Origin {
        &self.names_from
    }

    /// Compares names as their language orders them, with the numbers in them by value:
    /// `file2` before `file10`.
    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        self.names.compare(a, b)
    }

    /// The bytes that order `name` as [`Self::compare`] does: for sorting many names, as they
    /// compare far faster than names do.
    pub fn sort_key(&self, name: &str) -> Vec<u8> {
        self.names.key(name)
    }

    /// `value` with the separator of thousands of the region, such as `1,234,567`.
    pub fn integer(&self, value: u64) -> String {
        clean(self.numbers.integer(value, true))
    }

    /// `value` with exactly `fraction` digits after the decimal separator of the region, such
    /// as `1.5` or `1,5`, and the separator of thousands if `grouping`.
    pub fn decimal(&self, value: f64, fraction: usize, grouping: bool) -> String {
        clean(self.numbers.decimal(value, (fraction, fraction), grouping))
    }

    /// A number of a message: whole numbers without a fraction unless `min` asks for one,
    /// others with up to `max`, or 3, digits after the decimal separator.
    pub fn number(
        &self,
        value: f64,
        (min, max): (Option<usize>, Option<usize>),
        grouping: bool,
    ) -> String {
        let min = min.unwrap_or(0);
        let max = max.unwrap_or(3).max(min);
        let whole = value.fract() == 0.0 && min == 0;
        // Whole numbers that fit print exactly, as integers.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let text = if whole && (0.0..9.007_199_254_740_992e15).contains(&value) {
            self.numbers.integer(value as u64, grouping)
        } else {
            self.numbers.decimal(value, (min, max), grouping)
        };
        clean(text)
    }

    /// The year, the month, and the day of `date`, the year in two digits but in `iso`.
    pub fn date(&self, date: Date) -> String {
        clean(self.dates.date(date))
    }

    /// The hour and the minute of `at`.
    pub fn time(&self, at: DateTime) -> String {
        clean(self.dates.time(at))
    }

    /// `at` as every time shows: [`Self::date`] and [`Self::time`].
    pub fn date_time(&self, at: DateTime) -> String {
        format!("{} {}", self.date(at.date()), self.time(at))
    }

    /// The most cells that [`Self::date_time`] takes, for a column of times.
    pub fn time_width(&self) -> usize {
        self.width
    }

    /// The widest [`Self::date_time`], tried on days and hours whose digits and names of the
    /// half of the day differ in length.
    fn measure(&self) -> usize {
        let mut width = 0;
        for month in 1..=12 {
            for day in [1, 28] {
                let Ok(date) = Date::new(2026, month, day) else {
                    continue;
                };
                for hour in [0, 9, 11, 12, 21, 23] {
                    width = width.max(cells(&self.date_time(date.at(hour, 59, 0, 0))));
                }
            }
        }
        width
    }
}

/// The cells `text` takes in a terminal.
fn cells(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// `text` without the marks of the direction of text that some formats have, which terminals
/// do not use.
fn clean(text: String) -> String {
    if !text.chars().any(is_direction) {
        return text;
    }
    text.chars().filter(|c| !is_direction(*c)).collect()
}

fn is_direction(c: char) -> bool {
    matches!(
        c,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

/// The formats in use, [`Formats::iso`] until [`set_current`] sets others.
static CURRENT: LazyLock<RwLock<Arc<Formats>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Formats::iso())));

/// The formats in use.
pub fn current() -> Arc<Formats> {
    CURRENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Uses `formats` from now on, in messages too.
pub fn set_current(formats: Formats) {
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(formats);
}

/// Writes the numbers of Fluent messages in the [`current`] formats; a formatter for
/// `FluentBundle::set_formatter`. Other values are left to Fluent.
pub fn fluent(value: &FluentValue<'_>, _: &IntlLangMemoizer) -> Option<String> {
    let FluentValue::Number(number) = value else {
        return None;
    };
    let options = &number.options;
    Some(current().number(
        number.value,
        (
            options.minimum_fraction_digits,
            options.maximum_fraction_digits,
        ),
        options.use_grouping,
    ))
}

#[cfg(test)]
mod tests {
    use fluent_bundle::concurrent::FluentBundle;
    use fluent_bundle::{FluentArgs, FluentResource};

    use super::*;

    #[test]
    fn iso_formats_are_neutral() {
        let iso = Formats::iso();
        assert_eq!(iso.integer(1_234_567), "1234567");
        assert_eq!(iso.decimal(1.46, 1, true), "1.5");
        assert_eq!(iso.decimal(2.0, 1, false), "2.0");
        let at = jiff::civil::date(2025, 3, 7).at(9, 5, 0, 0);
        assert_eq!(iso.date(at.date()), "2025-03-07");
        assert_eq!(iso.date_time(at), "2025-03-07 09:05");
        assert_eq!(iso.time_width(), 16);
        assert_eq!(iso.numbers_from(), &Origin::Iso);
    }

    #[test]
    fn numbers_of_messages_keep_their_fraction() {
        let iso = Formats::iso();
        assert_eq!(iso.number(3.0, (None, None), true), "3");
        assert_eq!(iso.number(3.0, (Some(2), None), true), "3.00");
        assert_eq!(iso.number(2.5, (None, None), true), "2.5");
        assert_eq!(iso.number(1.0 / 3.0, (None, None), true), "0.333");
        assert_eq!(iso.number(1.0 / 3.0, (None, Some(1)), true), "0.3");
        assert_eq!(iso.number(-4.0, (None, None), true), "-4");
    }

    #[test]
    fn fluent_writes_numbers_in_the_current_formats() {
        let resource = FluentResource::try_new("n = { $n } and { $s }".to_owned()).unwrap();
        let mut bundle = FluentBundle::new_concurrent(vec!["en-US".parse().unwrap()]);
        bundle.set_use_isolating(false);
        bundle.add_resource(resource).unwrap();
        bundle.set_formatter(Some(fluent));
        let mut args = FluentArgs::new();
        args.set("n", 1_234_567_u64);
        args.set("s", "text");
        let message = bundle.get_message("n").unwrap();
        let mut errors = Vec::new();
        let text = bundle.format_pattern(message.value().unwrap(), Some(&args), &mut errors);
        assert_eq!(text, "1234567 and text");
        assert_eq!(errors.len(), 0, "{errors:?}");
    }

    #[test]
    fn marks_of_direction_are_left_out() {
        assert_eq!(clean("\u{200f}12\u{061c}/3".to_owned()), "12/3");
        assert_eq!(clean("1 2".to_owned()), "1 2");
    }
}
