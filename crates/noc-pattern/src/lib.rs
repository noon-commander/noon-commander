//! Patterns typed to match names: a regular expression, or text as it is, with smart case, as
//! fd and ripgrep match them. The search of a tree and quick search build them here, so that a
//! pattern matches the same names in both, with the regex engine fd and ripgrep use. Text is
//! compared in one Unicode form, and case folded where case does not count ([`fold`]).

mod fold;

pub use fold::{Normalized, fold, nfc};

/// Why a pattern is not a regular expression: the last line of what the regex engine says,
/// after the lines that show where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PatternError(String);

/// How a pattern matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// A regular expression; else the text as it is.
    pub regex: bool,
    /// Case always counts; else smart case: only once the pattern has a capital letter.
    pub match_case: bool,
}

/// `pattern` built to match text.
pub fn text(pattern: &str, options: Options) -> Result<regex::Regex, PatternError> {
    regex::RegexBuilder::new(&source(pattern, options))
        .case_insensitive(caseless(pattern, options))
        .build()
        .map_err(|error| invalid(&error))
}

/// `pattern` built to match bytes, such as names that are not UTF-8.
pub fn bytes(pattern: &str, options: Options) -> Result<regex::bytes::Regex, PatternError> {
    regex::bytes::RegexBuilder::new(&source(pattern, options))
        .case_insensitive(caseless(pattern, options))
        .build()
        .map_err(|error| invalid(&error))
}

fn source(pattern: &str, options: Options) -> String {
    if options.regex {
        pattern.to_owned()
    } else {
        regex::escape(pattern)
    }
}

fn caseless(pattern: &str, options: Options) -> bool {
    !options.match_case && !has_capital(pattern, options.regex)
}

/// Whether `pattern` has a capital letter, as smart case asks; in a regular expression, one not
/// escaped, as `\S` is a class rather than a letter.
pub fn has_capital(pattern: &str, regex: bool) -> bool {
    let mut escaped = false;
    pattern.chars().any(|c| {
        let literal = !escaped;
        escaped = regex && !escaped && c == '\\';
        literal && c.is_uppercase()
    })
}

fn invalid(error: &regex::Error) -> PatternError {
    PatternError(last_line(&error.to_string()))
}

/// The last line of a message that has text, trimmed.
fn last_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGEX: Options = Options {
        regex: true,
        match_case: false,
    };

    #[test]
    fn text_as_it_is_matches_with_smart_case() {
        let plain = Options::default();
        assert!(text("a.b", plain).unwrap().is_match("A.B"));
        assert!(!text("a.b", plain).unwrap().is_match("axb"));
        assert!(!text("Ab", plain).unwrap().is_match("ab"), "a capital");
        let exact = Options {
            match_case: true,
            ..plain
        };
        assert!(!text("ab", exact).unwrap().is_match("AB"));
        assert!(text("(", plain).is_ok());
    }

    #[test]
    fn a_regular_expression_matches_with_smart_case() {
        assert!(text("^a.b$", REGEX).unwrap().is_match("AxB"));
        assert!(!text("^A", REGEX).unwrap().is_match("abc"));
        assert!(
            text(r"\Sb", REGEX).unwrap().is_match("AB"),
            "\\S is no capital"
        );
        assert!(bytes("b$", REGEX).unwrap().is_match(b"\xffB"));
    }

    #[test]
    fn says_what_is_wrong_with_a_regular_expression() {
        assert_eq!(
            text("(", REGEX).unwrap_err().to_string(),
            "error: unclosed group"
        );
        assert_eq!(
            bytes("(", REGEX).unwrap_err().to_string(),
            "error: unclosed group"
        );
    }

    #[test]
    fn capitals_count_unless_escaped_in_a_regular_expression() {
        assert!(has_capital("aB", false));
        assert!(has_capital(r"\S", false));
        assert!(!has_capital(r"\S\W", true));
        assert!(has_capital(r"\\S", true), "an escaped backslash");
        assert!(!has_capital("abc", true));
    }
}
