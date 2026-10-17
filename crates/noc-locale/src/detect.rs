//! Which formats `ui.formats` names, and where `auto` finds them: in the variable of the
//! environment that POSIX gives each part, under `LC_ALL`; then, on macOS, in the settings of
//! the system; then in `LANG`.

use icu_locale_core::Locale;

use super::icu::{self, IcuDates, IcuNumbers, Names};
#[cfg(target_os = "macos")]
use super::system;
use super::{Dates, Formats, Numbers, Origin, iso::Iso};

/// Whether `setting` is valid for `ui.formats`: `auto`, `iso`, or a language tag.
pub fn is_valid(setting: &str) -> bool {
    matches!(setting, "auto" | "iso") || Locale::try_from_str(setting).is_ok()
}

pub(crate) fn detect(setting: &str) -> Formats {
    match setting {
        "auto" => auto(&|name| std::env::var(name).ok()),
        "iso" => Formats::iso(),
        tag => match Locale::try_from_str(tag) {
            Ok(locale) => from(&[Source::Locale(
                Box::new(locale),
                Origin::Setting(tag.to_owned()),
            )]),
            Err(error) => {
                tracing::warn!(%error, tag, "ui.formats is no language tag; using iso");
                Formats::iso()
            }
        },
    }
}

/// Where the formats of a part come from.
#[derive(Debug, Clone)]
enum Source {
    Iso,
    Locale(Box<Locale>, Origin),
    #[cfg(target_os = "macos")]
    System,
}

/// The formats of the system, as the variables of the environment, which `var` reads, and on
/// macOS the settings tell them.
fn auto(var: &dyn Fn(&str) -> Option<String>) -> Formats {
    from(&[
        source(var, "LC_NUMERIC"),
        source(var, "LC_TIME"),
        source(var, "LC_COLLATE"),
    ])
}

/// Where the part of `own`, a variable such as `LC_TIME`, comes from.
fn source(var: &dyn Fn(&str) -> Option<String>, own: &'static str) -> Source {
    let set = |variable: &'static str| {
        var(variable)
            .filter(|value| !value.is_empty())
            .map(|value| posix(variable, &value))
    };
    if let Some(source) = set("LC_ALL").or_else(|| set(own)) {
        return source;
    }
    // Terminals on macOS set `LANG` from the language, not the region.
    #[cfg(target_os = "macos")]
    return Source::System;
    #[cfg(not(target_os = "macos"))]
    set("LANG").unwrap_or(Source::Iso)
}

/// The source that a POSIX locale in `variable` names.
fn posix(variable: &'static str, value: &str) -> Source {
    match icu::locale(value) {
        Some(locale) => Source::Locale(
            Box::new(locale),
            Origin::Environment {
                variable,
                locale: value.to_owned(),
            },
        ),
        None => Source::Iso,
    }
}

/// The formats of numbers, dates, and names from their sources, in that order; a part whose
/// formats cannot be made is neutral.
fn from(sources: &[Source]) -> Formats {
    let source = |index: usize| sources.get(index).or(sources.first()).cloned();
    let (numbers, numbers_from) = numbers(source(0).unwrap_or(Source::Iso));
    let (dates, dates_from) = dates(source(1).unwrap_or(Source::Iso));
    let (names, names_from) = names(source(2).unwrap_or(Source::Iso));
    Formats::new(
        (numbers, numbers_from),
        (dates, dates_from),
        (names, names_from),
    )
}

fn numbers(source: Source) -> (Box<dyn Numbers>, Origin) {
    match source {
        Source::Locale(locale, origin) => match IcuNumbers::new(&locale) {
            Some(numbers) => (Box::new(numbers), origin),
            None => (Box::new(Iso), Origin::Iso),
        },
        #[cfg(target_os = "macos")]
        Source::System => (
            Box::new(system::System::default()),
            Origin::System(system::identifier()),
        ),
        Source::Iso => (Box::new(Iso), Origin::Iso),
    }
}

fn dates(source: Source) -> (Box<dyn Dates>, Origin) {
    match source {
        Source::Locale(locale, origin) => match IcuDates::new(&locale) {
            Some(dates) => (Box::new(dates), origin),
            None => (Box::new(Iso), Origin::Iso),
        },
        #[cfg(target_os = "macos")]
        Source::System => (
            Box::new(system::SystemDates::new()),
            Origin::System(system::identifier()),
        ),
        Source::Iso => (Box::new(Iso), Origin::Iso),
    }
}

fn names(source: Source) -> (Names, Origin) {
    match source {
        Source::Locale(locale, origin) => (Names::new(Some(&locale)), origin),
        #[cfg(target_os = "macos")]
        Source::System => {
            let identifier = system::identifier();
            let locale = icu::locale(&identifier);
            (Names::new(locale.as_ref()), Origin::System(identifier))
        }
        Source::Iso => (Names::new(None), Origin::Iso),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn settings_are_auto_iso_or_tags() {
        assert!(is_valid("auto"));
        assert!(is_valid("iso"));
        assert!(is_valid("und"));
        assert!(is_valid("und-ZZ-u-hc-h23"));
        assert!(!is_valid("not a tag"));
        assert!(!is_valid(""));
    }

    #[test]
    fn a_tag_names_every_part() {
        let formats = detect("und-u-hc-h23");
        let origin = Origin::Setting("und-u-hc-h23".to_owned());
        assert_eq!(formats.numbers_from(), &origin);
        assert_eq!(formats.dates_from(), &origin);
        assert_eq!(formats.names_from(), &origin);
        assert_eq!(detect("iso").dates_from(), &Origin::Iso);
        assert_eq!(detect("not a tag").dates_from(), &Origin::Iso);
    }

    #[test]
    fn lc_all_goes_before_the_variable_of_each_part() {
        let vars = [("LC_ALL", "und.UTF-8"), ("LC_TIME", "C"), ("LANG", "C")];
        let formats = auto(&env(&vars));
        let origin = Origin::Environment {
            variable: "LC_ALL",
            locale: "und.UTF-8".to_owned(),
        };
        assert_eq!(formats.dates_from(), &origin);
        assert_eq!(formats.numbers_from(), &origin);
    }

    #[test]
    fn each_part_has_its_own_variable() {
        let vars = [
            ("LC_TIME", "und"),
            ("LC_NUMERIC", "POSIX"),
            ("LC_COLLATE", "C.UTF-8"),
        ];
        let formats = auto(&env(&vars));
        assert_eq!(
            formats.dates_from(),
            &Origin::Environment {
                variable: "LC_TIME",
                locale: "und".to_owned()
            }
        );
        assert_eq!(formats.numbers_from(), &Origin::Iso, "POSIX is neutral");
        assert_eq!(formats.names_from(), &Origin::Iso);
    }

    #[test]
    fn without_variables_the_system_or_lang_decides() {
        let formats = auto(&env(&[("LANG", "und")]));
        #[cfg(target_os = "macos")]
        assert!(matches!(formats.dates_from(), Origin::System(_)));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            formats.dates_from(),
            &Origin::Environment {
                variable: "LANG",
                locale: "und".to_owned()
            }
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(auto(&env(&[])).dates_from(), &Origin::Iso);
    }
}
