//! How quick search and the filters of the lists match what is typed (ADR 0030): as it is, as
//! fzf does, or as a regular expression. Each search and each window starts with
//! `ui.search_mode`, and Ctrl+s switches its own, round; a glyph in its prompt, after the word,
//! says which.

use noc_config::SearchMode;
use noc_pattern::{Options, PatternError};
use ratatui::style::Style;
use ratatui::text::Span;
use regex::Regex;

use super::fuzzy::Fuzzy;
use super::glyphs::SearchModes;
use super::theme::Theme;

/// The mode after `mode`, as Ctrl+s goes round: plain, fzf, regex.
pub(crate) fn next(mode: SearchMode) -> SearchMode {
    match mode {
        SearchMode::Plain => SearchMode::Fzf,
        SearchMode::Fzf => SearchMode::Regex,
        SearchMode::Regex => SearchMode::Plain,
    }
}

/// The glyph of `mode` in a prompt.
pub(crate) fn glyph(mode: SearchMode, glyphs: SearchModes) -> &'static str {
    match mode {
        SearchMode::Plain => glyphs.plain,
        SearchMode::Fzf => glyphs.fzf,
        SearchMode::Regex => glyphs.regex,
    }
}

/// The glyph of `mode`, and its style in a prompt in `style`: the style of what is wrong if
/// `invalid`, a text that is no regular expression.
pub(crate) fn marked(mode: SearchMode, invalid: bool, style: Style, theme: &Theme) -> Mark {
    Mark {
        glyph: glyph(mode, theme.glyphs.search_modes),
        style: if invalid { theme.invalid(style) } else { style },
    }
}

/// The glyph of a mode in a prompt, as it is drawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mark {
    pub(crate) glyph: &'static str,
    pub(crate) style: Style,
}

impl Mark {
    /// `prompt` as spans in `style`, with the glyph in its own style where it is in `prefix`,
    /// the start of `prompt` that has the glyph but not what was typed.
    pub(crate) fn spans(self, prompt: &str, prefix: &str, style: Style) -> Vec<Span<'static>> {
        let Some(at) = prefix.find(self.glyph) else {
            return vec![Span::styled(prompt.to_owned(), style)];
        };
        let end = at + self.glyph.len();
        vec![
            Span::styled(prompt[..at].to_owned(), style),
            Span::styled(self.glyph, self.style),
            Span::styled(prompt[end..].to_owned(), style),
        ]
    }
}

/// What was typed, ready to match texts with in its mode.
#[derive(Debug)]
pub(crate) enum Matcher {
    /// The text in lower case, anywhere, ignoring case.
    Plain(String),
    Fuzzy(Fuzzy),
    /// A regular expression with smart case, or why it is none, which matches nothing.
    Regex(Result<Regex, PatternError>),
}

impl Matcher {
    /// For names, such as files or hosts.
    pub(crate) fn names(mode: SearchMode, typed: &str) -> Self {
        match mode {
            SearchMode::Fzf => Self::Fuzzy(Fuzzy::names(typed)),
            _ => Self::literal(mode, typed),
        }
    }

    /// For paths: fzf has a word start after a `/`.
    pub(crate) fn paths(mode: SearchMode, typed: &str) -> Self {
        match mode {
            SearchMode::Fzf => Self::Fuzzy(Fuzzy::paths(typed)),
            _ => Self::literal(mode, typed),
        }
    }

    fn literal(mode: SearchMode, typed: &str) -> Self {
        if mode == SearchMode::Regex {
            let options = Options {
                regex: true,
                match_case: false,
            };
            Self::Regex(noc_pattern::text(typed, options))
        } else {
            Self::Plain(typed.to_lowercase())
        }
    }

    /// Whether it ranks what matches, best first, as fzf does; the others keep their order.
    pub(crate) fn ranks(&self) -> bool {
        matches!(self, Self::Fuzzy(_))
    }

    /// Why what was typed is not a regular expression, in the regex mode.
    pub(crate) fn error(&self) -> Option<&PatternError> {
        match self {
            Self::Regex(Err(error)) => Some(error),
            _ => None,
        }
    }

    /// How well `text` matches, higher for better; `None` if it does not. Without ranking,
    /// every match scores the same.
    pub(crate) fn score(&mut self, text: &str) -> Option<u32> {
        match self {
            Self::Plain(typed) => text.to_lowercase().contains(typed.as_str()).then_some(0),
            Self::Fuzzy(fuzzy) => fuzzy.score(text),
            Self::Regex(regex) => regex.as_ref().ok()?.is_match(text).then_some(0),
        }
    }

    /// The characters of `text` that match, by their indexes, in order; each place it matches
    /// but in fzf's mode, which has one.
    pub(crate) fn positions(&mut self, text: &str) -> Vec<usize> {
        match self {
            Self::Plain(typed) => {
                let typed: Vec<char> = typed.chars().collect();
                text_positions(text, &typed)
            }
            Self::Fuzzy(fuzzy) => fuzzy.positions(text).unwrap_or_default(),
            Self::Regex(Ok(regex)) => {
                let starts: Vec<usize> = text.char_indices().map(|(byte, _)| byte).collect();
                let mut positions: Vec<usize> = regex
                    .find_iter(text)
                    .flat_map(|found| {
                        let first = starts.partition_point(|&byte| byte < found.start());
                        let end = starts.partition_point(|&byte| byte < found.end());
                        first..end
                    })
                    .collect();
                positions.dedup();
                positions
            }
            Self::Regex(Err(_)) => Vec::new(),
        }
    }
}

/// The characters of `name` that hold `text`, in lower case, ignoring case, each place it
/// does.
fn text_positions(name: &str, text: &[char]) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    // A character may lower to several, each with the one it came from.
    let lowered: Vec<(char, usize)> = name
        .chars()
        .enumerate()
        .flat_map(|(index, c)| c.to_lowercase().map(move |lower| (lower, index)))
        .collect();
    let mut positions = Vec::new();
    let mut at = 0;
    while at + text.len() <= lowered.len() {
        let window = &lowered[at..at + text.len()];
        if window.iter().map(|&(c, _)| c).eq(text.iter().copied()) {
            positions.extend(window.iter().map(|&(_, index)| index));
            at += text.len();
        } else {
            at += 1;
        }
    }
    positions.dedup();
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_r_goes_round_the_modes() {
        assert_eq!(next(SearchMode::Plain), SearchMode::Fzf);
        assert_eq!(next(SearchMode::Fzf), SearchMode::Regex);
        assert_eq!(next(SearchMode::Regex), SearchMode::Plain);
    }

    #[test]
    fn plain_text_matches_anywhere_ignoring_case() {
        let mut plain = Matcher::names(SearchMode::Plain, "Fi");
        assert_eq!(plain.score("profile"), Some(0));
        assert_eq!(plain.score("cfg"), None);
        assert_eq!(plain.positions("FileFI"), [0, 1, 4, 5]);
        assert!(!plain.ranks() && plain.error().is_none());
    }

    #[test]
    fn text_positions_are_each_place_the_text_is_ignoring_case() {
        let text = |text: &str| text.chars().collect::<Vec<_>>();
        assert_eq!(text_positions("ReadMe.md", &text("e")), [1, 5]);
        assert_eq!(text_positions("aaaa", &text("aa")), [0, 1, 2, 3]);
        assert_eq!(text_positions("aaa", &text("aa")), [0, 1]);
        assert_eq!(text_positions("ДОКУМЕНТ.txt", &text("кум")), [2, 3, 4]);
        assert_eq!(text_positions("abc", &text("x")), Vec::<usize>::new());
    }

    #[test]
    fn a_regular_expression_matches_with_smart_case() {
        let mut regex = Matcher::names(SearchMode::Regex, r"^.\.rs$");
        assert_eq!(regex.score("a.rs"), Some(0));
        assert_eq!(regex.score("ab.rs"), None);
        let mut smart = Matcher::names(SearchMode::Regex, "R");
        assert_eq!(smart.score("r"), None, "a capital");
        let mut any = Matcher::names(SearchMode::Regex, ".ö");
        assert_eq!(
            any.positions("föo.ö"),
            [0, 1, 3, 4],
            "characters, not bytes"
        );
        assert_eq!(
            Matcher::names(SearchMode::Regex, "^").positions("a"),
            [] as [usize; 0]
        );
    }

    #[test]
    fn what_is_not_a_regular_expression_matches_nothing() {
        let mut bad = Matcher::names(SearchMode::Regex, "(");
        assert_eq!(bad.score("("), None);
        assert_eq!(bad.positions("("), [] as [usize; 0]);
        assert_eq!(
            bad.error().map(ToString::to_string).as_deref(),
            Some("error: unclosed group")
        );
    }

    #[test]
    fn fzf_ranks_what_matches() {
        let mut fuzzy = Matcher::paths(SearchMode::Fzf, "cfg");
        assert!(fuzzy.ranks());
        assert!(fuzzy.score("config.rs").is_some());
        assert_eq!(fuzzy.positions("config.rs"), [0, 3, 5]);
    }
}
