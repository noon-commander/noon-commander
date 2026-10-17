//! How quick search and the filters of the lists match what is typed (ADR 0030): as it is, as
//! fzf does, or as a regular expression. Each search and each window starts with
//! `ui.search_mode`, and Ctrl+s switches its own, round; a glyph in its prompt, after the word,
//! says which.

use noc_config::SearchMode;
use noc_pattern::{Normalized, Options, PatternError};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use regex::Regex;

use super::cells;
use super::dialog::{self, Field};
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

/// Stands for what is typed while the message of a prompt is formatted, to find where the
/// message puts it: a noncharacter, which no message, glyph, or typed text has.
const TYPED: &str = "\u{FFFF}";

/// The message of a prompt around what is typed into it, such as `Search: …`: its text before
/// and after what is typed, wherever a translation puts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub(crate) before: String,
    pub(crate) after: String,
}

impl Prompt {
    /// The text that `message` makes of what is typed, split where it puts it; all of it
    /// before, if it puts it nowhere.
    pub(crate) fn new(message: impl FnOnce(String) -> String) -> Self {
        let text = message(TYPED.to_owned());
        match text.split_once(TYPED) {
            Some((before, after)) => Self {
                before: before.to_owned(),
                after: after.replace(TYPED, ""),
            },
            None => Self {
                before: text,
                after: String::new(),
            },
        }
    }

    /// The cells it takes without what is typed.
    pub(crate) fn width(&self) -> usize {
        cells::width(&self.before) + cells::width(&self.after)
    }

    /// The whole text, with `typed` in its place.
    pub(crate) fn with(&self, typed: &str) -> String {
        format!("{}{typed}{}", self.before, self.after)
    }
}

/// The glyph of a mode in a prompt, as it is drawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mark {
    pub(crate) glyph: &'static str,
    pub(crate) style: Style,
}

impl Mark {
    /// `prompt` with `typed` in it as spans in `style`, with the glyph in its own style where
    /// the message has it, before or after what is typed, which is never searched for it.
    pub(crate) fn spans(self, prompt: &Prompt, typed: &str, style: Style) -> Vec<Span<'static>> {
        let mut spans = Vec::with_capacity(5);
        let mut marked = false;
        for (part, message) in [
            (&prompt.before[..], true),
            (typed, false),
            (&prompt.after, true),
        ] {
            match part.find(self.glyph).filter(|_| message && !marked) {
                Some(at) => {
                    marked = true;
                    spans.push(Span::styled(part[..at].to_owned(), style));
                    spans.push(Span::styled(self.glyph, self.style));
                    spans.push(Span::styled(
                        part[at + self.glyph.len()..].to_owned(),
                        style,
                    ));
                }
                None => spans.push(Span::styled(part.to_owned(), style)),
            }
        }
        spans.retain(|span| !span.content.is_empty());
        spans
    }

    /// Draws `prompt` on the line `area` in `style`, the glyph marked, with `field` in its
    /// place in the colors of the text fields of dialogs, as wide as the prompt leaves room
    /// for. Returns the cursor's column in `area`. Where no room is left, the prompt and the
    /// text are cut as one.
    pub(crate) fn render_field(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        (prompt, field): (&Prompt, &Field),
        style: Style,
        theme: &Theme,
    ) -> u16 {
        let width = usize::from(area.width);
        let (before, after) = (cells::width(&prompt.before), cells::width(&prompt.after));
        let room = width.saturating_sub(before + after);
        let to_u16 = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
        if room == 0 {
            let (shown, column) = field.visible(0);
            let spans = cells::fit_spans(
                self.spans(prompt, &shown, style),
                width,
                (style, theme.truncated_dialog),
                theme.glyphs.marks,
            );
            frame.render_widget(Line::from(spans).style(style), area);
            return to_u16(before + column).min(area.width.saturating_sub(1));
        }
        let part = |text: &str, mark: bool| {
            let alone = Prompt {
                before: text.to_owned(),
                after: String::new(),
            };
            if mark {
                self.spans(&alone, "", style)
            } else {
                vec![Span::styled(text.to_owned(), style)]
            }
        };
        let in_before = prompt.before.contains(self.glyph);
        let (field_line, column) = dialog::field_line(field, room, theme);
        let field_area = Rect::new(area.x + to_u16(before), area.y, to_u16(room), 1);
        let after_area = Rect::new(field_area.right(), area.y, to_u16(after), 1);
        frame.render_widget(
            Line::from(part(&prompt.before, true)).style(style),
            Rect::new(area.x, area.y, to_u16(before), 1),
        );
        frame.render_widget(field_line, field_area);
        frame.render_widget(
            Line::from(part(&prompt.after, !in_before)).style(style),
            after_area,
        );
        to_u16(before + column)
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
            Self::Regex(noc_pattern::text(&noc_pattern::nfc(typed), options))
        } else {
            Self::Plain(noc_pattern::fold(typed).into_owned())
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
            Self::Plain(typed) => noc_pattern::fold(text)
                .contains(typed.as_str())
                .then_some(0),
            Self::Fuzzy(fuzzy) => fuzzy.score(text),
            Self::Regex(regex) => regex
                .as_ref()
                .ok()?
                .is_match(&noc_pattern::nfc(text))
                .then_some(0),
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
                let normalized = Normalized::nfc(text);
                let text = &normalized.text;
                let starts: Vec<usize> = text.char_indices().map(|(byte, _)| byte).collect();
                normalized.original(regex.find_iter(text).flat_map(|found| {
                    let first = starts.partition_point(|&byte| byte < found.start());
                    let end = starts.partition_point(|&byte| byte < found.end());
                    first..end
                }))
            }
            Self::Regex(Err(_)) => Vec::new(),
        }
    }
}

/// The characters of `name` that hold `text`, folded as [`noc_pattern::fold`] folds it, each
/// place it does, ignoring case and Unicode form.
fn text_positions(name: &str, text: &[char]) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    // A character may fold to several, and several in NFD to one: each stands for its own.
    let folded = Normalized::folded(name);
    let chars: Vec<char> = folded.text.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at + text.len() <= chars.len() {
        if chars[at..at + text.len()] == *text {
            found.extend(at..at + text.len());
            at += text.len();
        } else {
            at += 1;
        }
    }
    folded.original(found)
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
    fn a_prompt_finds_what_is_typed_wherever_the_message_puts_it() {
        let last = Prompt::new(|text| format!("Search *: {text}"));
        assert_eq!(
            (last.before.as_str(), last.after.as_str()),
            ("Search *: ", "")
        );
        assert_eq!(last.width(), 10);
        let first = Prompt::new(|text| format!("{text} :* search"));
        assert_eq!(
            (first.before.as_str(), first.after.as_str()),
            ("", " :* search")
        );
        assert_eq!(first.with("ab"), "ab :* search");
        let none = Prompt::new(|_| "Search".to_owned());
        assert_eq!(none.with("ab"), "Searchab");
    }

    #[test]
    fn the_glyph_is_marked_in_the_message_and_never_in_what_is_typed() {
        let mark = Mark {
            glyph: "*",
            style: Style::new().bold(),
        };
        let text = |spans: &[Span<'_>]| -> Vec<(String, bool)> {
            spans
                .iter()
                .map(|span| (span.content.to_string(), span.style == mark.style))
                .collect()
        };
        let owned = |text: &str, marked| (text.to_owned(), marked);
        let after = Prompt::new(|text| format!("{text} — *"));
        assert_eq!(
            text(&mark.spans(&after, "a*é", Style::new())),
            [owned("a*é", false), owned(" — ", false), owned("*", true)],
            "typed text before the glyph, with a wide character, as a translation may put it"
        );
        let before = Prompt::new(|text| format!("* {text}"));
        assert_eq!(
            text(&mark.spans(&before, "*", Style::new())),
            [owned("*", true), owned(" ", false), owned("*", false)]
        );
    }

    #[test]
    fn the_field_of_a_prompt_has_the_colors_of_the_fields_of_dialogs() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let theme = Theme::terminal();
        let mark = marked(SearchMode::Fzf, false, theme.dialog, &theme);
        let prompt = Prompt::new(|text| format!("Filter {}: {text} !", mark.glyph));
        let before = cells::width(&prompt.before);
        let field = Field::plain("ab");
        let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
        let mut column = 0;
        terminal
            .draw(|frame| {
                let area = frame.area();
                column = mark.render_field(frame, area, (&prompt, &field), theme.dialog, &theme);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let bg = |x: usize| buffer[(u16::try_from(x).unwrap(), 0)].bg;
        assert_eq!(usize::from(column), before + 2, "after what is typed");
        assert_eq!(bg(0), theme.dialog.bg.unwrap_or_default());
        for x in before..28 {
            assert_eq!(bg(x), theme.dialog_input.bg.unwrap_or_default(), "cell {x}");
        }
        assert_eq!(
            bg(29),
            theme.dialog.bg.unwrap_or_default(),
            "the prompt after it"
        );
        assert_eq!(buffer[(28, 0)].symbol(), " ");
        assert_eq!(buffer[(29, 0)].symbol(), "!");
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
    fn plain_text_matches_either_unicode_form_and_case_by_folding() {
        // A name in NFD, as macOS keeps it, and the text typed in NFC.
        let mut plain = Matcher::names(SearchMode::Plain, "Caf\u{e9}");
        assert_eq!(plain.score("my cafe\u{301}.txt"), Some(0));
        assert_eq!(plain.positions("my cafe\u{301}.txt"), [3, 4, 5, 6, 7]);
        let mut sharp = Matcher::names(SearchMode::Plain, "strasse");
        assert_eq!(sharp.score("Stra\u{df}e"), Some(0));
        assert_eq!(sharp.positions("Stra\u{df}e"), [0, 1, 2, 3, 4, 5]);
        let mut sigma = Matcher::names(SearchMode::Plain, "\u{3c3}\u{3b1}\u{3c2}");
        assert_eq!(sigma.score("\u{3a3}\u{391}\u{3a3}"), Some(0));
        assert_eq!(
            sigma.positions("\u{3a3}\u{391}\u{3a3}"),
            [0, 1, 2],
            "every sigma"
        );
        let mut regex = Matcher::names(SearchMode::Regex, "f\u{e9}$");
        assert_eq!(regex.score("cafe\u{301}"), Some(0));
        assert_eq!(regex.positions("cafe\u{301}"), [2, 3, 4]);
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
