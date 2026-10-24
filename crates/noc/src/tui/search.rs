//! How every field that searches matches what is typed (ADR 0030, ADR 0044): quick search, the
//! filters of the lists, the search of a tree, and the marking of `g +` and `g -`. Each is a
//! [`Kind`], which says the ways it matches, in the order Ctrl+s goes round them, and the way it
//! starts in for each of `ui.search_mode`; an [`Input`] is one of them while it is open. A glyph
//! in its prompt says which way, in every field.

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
use super::pattern::Pattern;
use super::theme::Theme;

/// A way to match what is typed: the three of `ui.search_mode`, and shell patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Way {
    /// The text anywhere.
    Plain,
    /// The characters in order, as fzf does, best first.
    Fzf,
    /// A regular expression, anywhere.
    Regex,
    /// A shell pattern, `*`, `?`, `[a-z]`, `{a,b}`, which the whole name matches, as mc's `+`.
    Glob,
}

impl From<SearchMode> for Way {
    fn from(mode: SearchMode) -> Self {
        match mode {
            SearchMode::Plain => Self::Plain,
            SearchMode::Fzf => Self::Fzf,
            SearchMode::Regex => Self::Regex,
        }
    }
}

impl Way {
    /// Its glyph in a prompt.
    pub(crate) fn glyph(self, glyphs: SearchModes) -> &'static str {
        match self {
            Self::Plain => glyphs.plain,
            Self::Fzf => glyphs.fzf,
            Self::Regex => glyphs.regex,
            Self::Glob => glyphs.glob,
        }
    }
}

/// What is matched: names, or paths, where fzf has a word start after a `/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Names,
    Paths,
}

/// Whether case counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Case {
    /// As the way has it: plain text and patterns ignore case; fzf and regular expressions count
    /// it once what is typed has a capital letter.
    Own,
    /// Always.
    Counts,
}

/// A kind of field that searches: the ways it matches, and what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kind {
    /// In the order Ctrl+s goes round them.
    ways: &'static [Way],
    /// The way it starts in when the one asked for is not among them.
    instead: Way,
    target: Target,
}

impl Kind {
    /// Quick search in a panel: `/` and `?`.
    pub(crate) const QUICK_SEARCH: Self = Self {
        ways: &[Way::Plain, Way::Fzf, Way::Regex],
        instead: Way::Fzf,
        target: Target::Names,
    };
    /// The filter of a list of names: the location menu, the tab menu, the workspaces, the
    /// command history, the key hints.
    pub(crate) const FILTER: Self = Self {
        ways: &[Way::Plain, Way::Fzf, Way::Regex],
        instead: Way::Fzf,
        target: Target::Names,
    };
    /// The filter of a list of paths: the zoxide window.
    pub(crate) const FILTER_PATHS: Self = Self {
        target: Target::Paths,
        ..Self::FILTER
    };
    /// The text of the search of a tree, `f /` and `f f`: ripgrep and fd match a pattern rather
    /// than rank names, so fzf is a regular expression there.
    pub(crate) const TREE: Self = Self {
        ways: &[Way::Plain, Way::Regex],
        instead: Way::Regex,
        target: Target::Names,
    };
    /// The pattern of `g +` and `g -`.
    pub(crate) const MARK: Self = Self {
        ways: &[Way::Glob],
        instead: Way::Glob,
        target: Target::Names,
    };

    /// The way it starts in when `way` is asked for.
    fn start(self, way: Way) -> Way {
        if self.ways.contains(&way) {
            way
        } else {
            self.instead
        }
    }

    /// The way after `way`, as Ctrl+s goes round.
    fn next(self, way: Way) -> Way {
        let at = self.ways.iter().position(|&each| each == way);
        at.and_then(|at| self.ways.get((at + 1) % self.ways.len()))
            .copied()
            .unwrap_or(self.instead)
    }
}

/// A field that searches, while it is open: its kind, and the way it matches now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Input {
    kind: Kind,
    way: Way,
}

impl Input {
    /// A field of `kind` that starts in `way`, `ui.search_mode` mostly; in the kind's own if it
    /// has no such way.
    pub(crate) fn new(kind: Kind, way: impl Into<Way>) -> Self {
        Self {
            kind,
            way: kind.start(way.into()),
        }
    }

    pub(crate) fn kind(self) -> Kind {
        self.kind
    }

    /// The way it matches now.
    pub(crate) fn way(self) -> Way {
        self.way
    }

    /// Goes to the next way, as Ctrl+s does; whether it changed, which it does not in a kind of
    /// one way.
    pub(crate) fn switch(&mut self) -> bool {
        let before = self.way;
        self.way = self.kind.next(self.way);
        self.way != before
    }

    /// What `typed` matches, in the way it matches now.
    pub(crate) fn matcher(self, typed: &str, case: Case) -> Matcher {
        Matcher::new(self.way, self.kind.target, typed, case)
    }

    /// Whether it has more than one way, which Ctrl+s and a click on its glyph switch.
    pub(crate) fn switchable(self) -> bool {
        self.kind.ways.len() > 1
    }

    /// Whether `typed` matches nothing for being no regular expression, in the regex way.
    pub(crate) fn invalid(self, typed: &str) -> bool {
        self.way == Way::Regex && self.matcher(typed, Case::Own).error().is_some()
    }

    /// Its glyph.
    pub(crate) fn glyph(self, glyphs: SearchModes) -> &'static str {
        self.way.glyph(glyphs)
    }

    /// Its glyph, and its style at the end of a field in `style`: the style of what is wrong if
    /// `invalid`, a text that is no regular expression.
    pub(crate) fn mark(self, invalid: bool, style: Style, theme: &Theme) -> Mark {
        Mark {
            glyph: self.glyph(theme.glyphs.search_modes),
            style: if invalid { theme.invalid(style) } else { style },
        }
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

/// The glyph of a way at the end of a field, as it is drawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mark {
    pub(crate) glyph: &'static str,
    pub(crate) style: Style,
}

impl Mark {
    /// The cells it takes at the end of a field: the glyph, and a blank before it.
    pub(crate) fn width(self) -> usize {
        cells::width(self.glyph) + 1
    }

    /// Draws `field` on the line `area` in the colors of the fields of dialogs, with the glyph
    /// at its right end (ADR 0045). Returns the cursor's column in `area`, and where the glyph
    /// is, for the mouse; without room for both, the field shows alone.
    pub(crate) fn render_in_field(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        field: &Field,
        theme: &Theme,
    ) -> (u16, Option<Rect>) {
        let width = usize::from(area.width);
        let glyph = self.width();
        if width <= glyph {
            let (line, column) = dialog::field_line(field, width, theme);
            frame.render_widget(line, area);
            return (to_u16(column), None);
        }
        let room = to_u16(width - glyph);
        let (line, column) = dialog::field_line(field, usize::from(room), theme);
        frame.render_widget(
            line,
            Rect {
                width: room,
                ..area
            },
        );
        let at = Rect::new(area.x + room, area.y, to_u16(glyph), 1);
        let spans = vec![Span::raw(" "), Span::styled(self.glyph, self.style)];
        frame.render_widget(Line::from(spans).style(theme.dialog_input), at);
        let drawn = Rect {
            x: at.x + 1,
            width: at.width - 1,
            ..at
        };
        (to_u16(column), Some(drawn))
    }

    /// Draws `prompt` on the line `area` in `style`, with `field` in its place in the colors
    /// of the text fields of dialogs, as wide as the prompt leaves room for, and the glyph at
    /// the end of the field. Returns the cursor's column in `area`, and where the glyph is, for
    /// the mouse. Where no room is left, the prompt and the text are cut as one, without it.
    pub(crate) fn render_field(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        (prompt, field): (&Prompt, &Field),
        style: Style,
        theme: &Theme,
    ) -> (u16, Option<Rect>) {
        let width = usize::from(area.width);
        let (before, after) = (cells::width(&prompt.before), cells::width(&prompt.after));
        let room = width.saturating_sub(before + after);
        if room <= self.width() {
            let (shown, column) = field.visible(0);
            let spans = cells::fit_spans(
                vec![Span::styled(prompt.with(&shown), style)],
                width,
                (style, theme.truncated_dialog),
                theme.glyphs.marks,
            );
            frame.render_widget(Line::from(spans).style(style), area);
            return (
                to_u16(before + column).min(area.width.saturating_sub(1)),
                None,
            );
        }
        let field_area = Rect::new(area.x + to_u16(before), area.y, to_u16(room), 1);
        let after_area = Rect::new(field_area.right(), area.y, to_u16(after), 1);
        frame.render_widget(
            Line::styled(prompt.before.clone(), style),
            Rect::new(area.x, area.y, to_u16(before), 1),
        );
        let (column, glyph) = self.render_in_field(frame, field_area, field, theme);
        frame.render_widget(Line::styled(prompt.after.clone(), style), after_area);
        (to_u16(before) + column, glyph)
    }
}

fn to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// What was typed, ready to match texts with in its way.
#[derive(Debug)]
pub(crate) enum Matcher {
    /// The text in NFC, anywhere; case folded if `fold`, as it ignores case then.
    Plain {
        typed: String,
        fold: bool,
    },
    Fuzzy(Fuzzy),
    /// A regular expression, or why it is none, which matches nothing.
    Regex(Result<Regex, PatternError>),
    /// A shell pattern, which the whole name matches; in NFC, and case folded if `fold`.
    Glob {
        pattern: Pattern,
        fold: bool,
    },
}

impl Matcher {
    /// `typed`, to match `target` in `way`, with `case`.
    pub(crate) fn new(way: Way, target: Target, typed: &str, case: Case) -> Self {
        let counts = case == Case::Counts;
        match way {
            Way::Plain => Self::Plain {
                typed: form(typed, !counts),
                fold: !counts,
            },
            Way::Fzf => Self::Fuzzy(Fuzzy::new(typed, target == Target::Paths, counts)),
            Way::Regex => {
                let options = Options {
                    regex: true,
                    match_case: counts,
                };
                Self::Regex(noc_pattern::text(&noc_pattern::nfc(typed), options))
            }
            Way::Glob => Self::Glob {
                pattern: Pattern::new(&form(typed, !counts)),
                fold: !counts,
            },
        }
    }

    /// Whether it ranks what matches, best first, as fzf does; the others keep their order.
    pub(crate) fn ranks(&self) -> bool {
        matches!(self, Self::Fuzzy(_))
    }

    /// Why what was typed is not a regular expression, in the regex way.
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
            Self::Plain { typed, fold } => form(text, *fold).contains(typed.as_str()).then_some(0),
            Self::Fuzzy(fuzzy) => fuzzy.score(text),
            Self::Regex(regex) => regex
                .as_ref()
                .ok()?
                .is_match(&noc_pattern::nfc(text))
                .then_some(0),
            Self::Glob { pattern, fold } => pattern.matches(&form(text, *fold)).then_some(0),
        }
    }

    /// The characters of `text` that match, by their indexes, in order; each place it matches
    /// but in fzf's way, which has one. A pattern marks none, as the whole name matches it.
    pub(crate) fn positions(&mut self, text: &str) -> Vec<usize> {
        match self {
            Self::Plain { typed, fold } => {
                let typed: Vec<char> = typed.chars().collect();
                text_positions(text, &typed, *fold)
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
            Self::Regex(Err(_)) | Self::Glob { .. } => Vec::new(),
        }
    }
}

/// `text` in NFC, and case folded if `fold`.
fn form(text: &str, fold: bool) -> String {
    if fold {
        noc_pattern::fold(text).into_owned()
    } else {
        noc_pattern::nfc(text).into_owned()
    }
}

/// The characters of `name` that hold `text`, in NFC and folded as [`noc_pattern::fold`]
/// folds it if `fold`, each place it does.
fn text_positions(name: &str, text: &[char], fold: bool) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    // A character may fold to several, and several in NFD to one: each stands for its own.
    let normalized = if fold {
        Normalized::folded(name)
    } else {
        Normalized::nfc(name)
    };
    let chars: Vec<char> = normalized.text.chars().collect();
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
    normalized.original(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(way: Way, typed: &str) -> Matcher {
        Matcher::new(way, Target::Names, typed, Case::Own)
    }

    /// The ways a kind starts in for each of `ui.search_mode`: plain, fzf, regex.
    fn starts(kind: Kind) -> [Way; 3] {
        [SearchMode::Plain, SearchMode::Fzf, SearchMode::Regex]
            .map(|mode| Input::new(kind, mode).way())
    }

    /// The ways Ctrl+s goes round from the one it starts in, back to it.
    fn round(kind: Kind, way: Way) -> Vec<Way> {
        let mut input = Input::new(kind, way);
        let mut ways = vec![input.way()];
        while input.switch() && input.way() != way {
            ways.push(input.way());
        }
        ways
    }

    #[test]
    fn each_kind_of_search_has_its_ways() {
        use Way::{Fzf, Glob, Plain, Regex};

        let kinds = [
            (
                "quick search",
                Kind::QUICK_SEARCH,
                &[Plain, Fzf, Regex][..],
                [Plain, Fzf, Regex],
            ),
            (
                "filters",
                Kind::FILTER,
                &[Plain, Fzf, Regex],
                [Plain, Fzf, Regex],
            ),
            (
                "filters of paths",
                Kind::FILTER_PATHS,
                &[Plain, Fzf, Regex],
                [Plain, Fzf, Regex],
            ),
            (
                "search of a tree",
                Kind::TREE,
                &[Plain, Regex],
                [Plain, Regex, Regex],
            ),
            ("g + and g -", Kind::MARK, &[Glob], [Glob, Glob, Glob]),
        ];
        for (name, kind, ways, start) in kinds {
            assert_eq!(kind.ways, ways, "{name}");
            assert_eq!(
                starts(kind),
                start,
                "{name}: what ui.search_mode starts it in"
            );
            assert_eq!(round(kind, ways[0]), ways, "{name}: Ctrl+s goes round them");
        }
        assert_eq!(Kind::FILTER_PATHS.target, Target::Paths);
    }

    #[test]
    fn ctrl_s_changes_nothing_in_a_kind_of_one_way() {
        let mut input = Input::new(Kind::MARK, Way::Regex);
        assert_eq!(input.way(), Way::Glob, "a way it has not");
        assert!(!input.switch());
        assert_eq!(input.way(), Way::Glob);
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
    fn the_field_of_a_prompt_has_the_colors_of_the_fields_of_dialogs_and_ends_with_the_glyph() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let theme = Theme::terminal();
        let mark = Input::new(Kind::FILTER, Way::Regex).mark(false, theme.dialog_input, &theme);
        let prompt = Prompt::new(|text| format!("Filter: {text} !"));
        let before = cells::width(&prompt.before);
        let field = Field::plain("ab");
        let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
        let mut drawn = (0, None);
        terminal
            .draw(|frame| {
                let area = frame.area();
                drawn = mark.render_field(frame, area, (&prompt, &field), theme.dialog, &theme);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let bg = |x: usize| buffer[(u16::try_from(x).unwrap(), 0)].bg;
        let (column, glyph) = drawn;
        assert_eq!(usize::from(column), before + 2, "after what is typed");
        assert_eq!(bg(0), theme.dialog.bg.unwrap_or_default());
        for x in before..28 {
            assert_eq!(bg(x), theme.dialog_input.bg.unwrap_or_default(), "cell {x}");
        }
        let glyph = glyph.unwrap();
        assert_eq!((glyph.x, glyph.width), (26, 2), "at the end of the field");
        let at = |x: u16| buffer[(x, 0)].symbol().to_owned();
        assert_eq!(at(26) + &at(27), mark.glyph);
        assert_eq!(at(25), " ", "a blank before it");
        assert_eq!(
            bg(29),
            theme.dialog.bg.unwrap_or_default(),
            "the prompt after it"
        );
        assert_eq!(buffer[(29, 0)].symbol(), "!");
    }

    #[test]
    fn a_field_too_narrow_for_its_glyph_shows_alone() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let theme = Theme::terminal();
        let mark = Input::new(Kind::FILTER, Way::Regex).mark(false, theme.dialog_input, &theme);
        let mut terminal = Terminal::new(TestBackend::new(3, 1)).unwrap();
        let mut glyph = Some(Rect::default());
        terminal
            .draw(|frame| {
                (_, glyph) = mark.render_in_field(frame, frame.area(), &Field::plain("ab"), &theme);
            })
            .unwrap();
        assert_eq!(glyph, None);
    }

    #[test]
    fn only_a_regular_expression_is_invalid() {
        assert!(Input::new(Kind::FILTER, Way::Regex).invalid("("));
        assert!(!Input::new(Kind::FILTER, Way::Regex).invalid("a"));
        assert!(!Input::new(Kind::FILTER, Way::Plain).invalid("("));
        assert!(!Input::new(Kind::MARK, Way::Glob).invalid("["));
        assert!(Input::new(Kind::FILTER, Way::Fzf).switchable());
        assert!(!Input::new(Kind::MARK, Way::Glob).switchable());
    }

    #[test]
    fn plain_text_matches_anywhere_ignoring_case() {
        let mut plain = names(Way::Plain, "Fi");
        assert_eq!(plain.score("profile"), Some(0));
        assert_eq!(plain.score("cfg"), None);
        assert_eq!(plain.positions("FileFI"), [0, 1, 4, 5]);
        assert!(!plain.ranks() && plain.error().is_none());
    }

    #[test]
    fn text_positions_are_each_place_the_text_is_ignoring_case() {
        let text = |text: &str| text.chars().collect::<Vec<_>>();
        assert_eq!(text_positions("ReadMe.md", &text("e"), true), [1, 5]);
        assert_eq!(text_positions("aaaa", &text("aa"), true), [0, 1, 2, 3]);
        assert_eq!(text_positions("aaa", &text("aa"), true), [0, 1]);
        assert_eq!(
            text_positions("ДОКУМЕНТ.txt", &text("кум"), true),
            [2, 3, 4]
        );
        assert_eq!(text_positions("abc", &text("x"), true), Vec::<usize>::new());
    }

    #[test]
    fn plain_text_matches_either_unicode_form_and_case_by_folding() {
        // A name in NFD, as macOS keeps it, and the text typed in NFC.
        let mut plain = names(Way::Plain, "Caf\u{e9}");
        assert_eq!(plain.score("my cafe\u{301}.txt"), Some(0));
        assert_eq!(plain.positions("my cafe\u{301}.txt"), [3, 4, 5, 6, 7]);
        let mut sharp = names(Way::Plain, "strasse");
        assert_eq!(sharp.score("Stra\u{df}e"), Some(0));
        assert_eq!(sharp.positions("Stra\u{df}e"), [0, 1, 2, 3, 4, 5]);
        let mut sigma = names(Way::Plain, "\u{3c3}\u{3b1}\u{3c2}");
        assert_eq!(sigma.score("\u{3a3}\u{391}\u{3a3}"), Some(0));
        assert_eq!(
            sigma.positions("\u{3a3}\u{391}\u{3a3}"),
            [0, 1, 2],
            "every sigma"
        );
        let mut regex = names(Way::Regex, "f\u{e9}$");
        assert_eq!(regex.score("cafe\u{301}"), Some(0));
        assert_eq!(regex.positions("cafe\u{301}"), [2, 3, 4]);
    }

    #[test]
    fn a_regular_expression_matches_with_smart_case() {
        let mut regex = names(Way::Regex, r"^.\.rs$");
        assert_eq!(regex.score("a.rs"), Some(0));
        assert_eq!(regex.score("ab.rs"), None);
        let mut smart = names(Way::Regex, "R");
        assert_eq!(smart.score("r"), None, "a capital");
        let mut any = names(Way::Regex, ".ö");
        assert_eq!(
            any.positions("föo.ö"),
            [0, 1, 3, 4],
            "characters, not bytes"
        );
        assert_eq!(names(Way::Regex, "^").positions("a"), [] as [usize; 0]);
    }

    #[test]
    fn what_is_not_a_regular_expression_matches_nothing() {
        let mut bad = names(Way::Regex, "(");
        assert_eq!(bad.score("("), None);
        assert_eq!(bad.positions("("), [] as [usize; 0]);
        assert_eq!(
            bad.error().map(ToString::to_string).as_deref(),
            Some("error: unclosed group")
        );
    }

    #[test]
    fn fzf_ranks_what_matches() {
        let mut fuzzy = Matcher::new(Way::Fzf, Target::Paths, "cfg", Case::Own);
        assert!(fuzzy.ranks());
        assert!(fuzzy.score("config.rs").is_some());
        assert_eq!(fuzzy.positions("config.rs"), [0, 3, 5]);
    }

    #[test]
    fn a_pattern_matches_the_whole_name_in_either_form_ignoring_case_by_folding() {
        let mut glob = names(Way::Glob, "*.RS");
        assert_eq!(glob.score("main.rs"), Some(0));
        assert_eq!(glob.score("main.rs.bak"), None, "the whole name");
        assert_eq!(glob.positions("main.rs"), [] as [usize; 0]);
        assert!(!glob.ranks() && glob.error().is_none());
        let mut nfd = names(Way::Glob, "caf\u{e9}*");
        assert_eq!(nfd.score("Cafe\u{301}.txt"), Some(0));
        let mut counts = Matcher::new(Way::Glob, Target::Names, "*.RS", Case::Counts);
        assert_eq!(counts.score("main.rs"), None);
        assert_eq!(counts.score("MAIN.RS"), Some(0));
    }

    #[test]
    fn case_counts_in_every_way_when_asked() {
        let counts = |way, typed| Matcher::new(way, Target::Names, typed, Case::Counts);
        for way in [Way::Plain, Way::Fzf, Way::Regex] {
            assert_eq!(counts(way, "read").score("README"), None, "{way:?}");
            assert!(counts(way, "READ").score("README").is_some(), "{way:?}");
        }
        let mut plain = counts(Way::Plain, "caf\u{e9}");
        assert_eq!(
            plain.positions("my cafe\u{301}"),
            [3, 4, 5, 6, 7],
            "either form"
        );
    }
}
