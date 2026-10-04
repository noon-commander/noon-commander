//! Matching what is typed as fzf does, for quick search, the location menu, and the zoxide
//! window while `ui.fuzzy_search` is on (ADR 0016).
//!
//! The characters typed must come in a text in order, not necessarily together; runs of them,
//! and those that start a word (after a space, a `/`, or punctuation, or a capital after a
//! small letter), score higher. Words separated by spaces must all match, in any order. As in
//! fzf's extended search, `'word` matches the word as it is, `^word` at the start, `word$` at
//! the end, and `!word` only texts without it; `\` before a space or one of these takes it as
//! it is. Case counts only once the text has a capital letter, and letters with accents match
//! those without.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// What was typed, ready to score texts with.
#[derive(Debug)]
pub(crate) struct Fuzzy {
    pattern: Pattern,
    matcher: Matcher,
    /// Room for the characters of a text that is not ASCII.
    chars: Vec<char>,
}

impl Fuzzy {
    /// For names, such as files or hosts.
    pub(crate) fn names(typed: &str) -> Self {
        Self::new(typed, Config::DEFAULT)
    }

    /// For paths: after a `/` starts a word, as fzf's `--scheme=path` has it.
    pub(crate) fn paths(typed: &str) -> Self {
        Self::new(typed, Config::DEFAULT.match_paths())
    }

    fn new(typed: &str, config: Config) -> Self {
        Self {
            pattern: Pattern::parse(typed, CaseMatching::Smart, Normalization::Smart),
            matcher: Matcher::new(config),
            chars: Vec::new(),
        }
    }

    /// How well `text` matches, higher for better; `None` if it does not.
    pub(crate) fn score(&mut self, text: &str) -> Option<u32> {
        let text = Utf32Str::new(text, &mut self.chars);
        self.pattern.score(text, &mut self.matcher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(typed: &str, texts: &[&str]) -> Vec<String> {
        let mut fuzzy = Fuzzy::names(typed);
        texts
            .iter()
            .filter(|text| fuzzy.score(text).is_some())
            .map(|text| (*text).to_owned())
            .collect()
    }

    /// `texts` that match, best first; equal ones keep their order.
    fn ranked(typed: &str, texts: &[&str]) -> Vec<String> {
        let mut fuzzy = Fuzzy::names(typed);
        let mut scored: Vec<(u32, &str)> = texts
            .iter()
            .filter_map(|text| Some((fuzzy.score(text)?, *text)))
            .collect();
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored
            .into_iter()
            .map(|(_, text)| text.to_owned())
            .collect()
    }

    #[test]
    fn characters_match_in_order_but_not_necessarily_together() {
        let names = ["Cargo.toml", "config.rs", "crates", "src", "cfg"];
        assert_eq!(matches("cfg", &names), ["config.rs", "cfg"]);
        assert_eq!(matches("gfc", &names), [] as [String; 0]);
        assert_eq!(matches("", &names), names);
    }

    #[test]
    fn runs_and_the_starts_of_words_rank_higher() {
        assert_eq!(
            ranked("rdm", &["random.rs", "README.md", "a-rdm"]),
            ["a-rdm", "README.md", "random.rs"]
        );
        assert_eq!(
            ranked("map", &["heatmap.rs", "my-app.rs", "map.rs"]),
            ["map.rs", "my-app.rs", "heatmap.rs"],
            "the starts of words count for more than a run inside one"
        );
    }

    #[test]
    fn case_counts_only_with_a_capital_letter() {
        let names = ["readme", "README"];
        assert_eq!(matches("rea", &names), names);
        assert_eq!(matches("REA", &names), ["README"]);
        assert_eq!(matches("cafe", &["Café"]), ["Café"], "accents are ignored");
    }

    #[test]
    fn words_must_all_match_and_extended_search_works() {
        let names = ["main.rs", "domain.rs", "main.c", "README.md"];
        assert_eq!(matches("rs main", &names), ["main.rs", "domain.rs"]);
        assert_eq!(matches("^main", &names), ["main.rs", "main.c"]);
        assert_eq!(matches(".rs$", &names), ["main.rs", "domain.rs"]);
        assert_eq!(matches("!rs", &names), ["main.c", "README.md"]);
        assert_eq!(matches("'ain.", &names), ["main.rs", "domain.rs", "main.c"]);
        assert_eq!(matches("'mr", &names), [] as [String; 0], "as it is");
    }

    #[test]
    fn spaces_and_lone_operators_match_everything() {
        let names = ["a", "b c", "!^'"];
        for typed in ["", " ", "!", "^", "'", "$"] {
            assert_eq!(matches(typed, &names), names, "{typed:?}");
        }
    }

    #[test]
    fn paths_rank_whole_directories_first() {
        let mut fuzzy = Fuzzy::paths("src");
        let scripts = fuzzy.score("~/srv/scripts/c");
        let src = fuzzy.score("~/work/src");
        assert!(scripts.is_some() && src > scripts, "{src:?} {scripts:?}");
    }
}
