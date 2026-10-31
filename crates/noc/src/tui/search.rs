//! How every field that searches matches what is typed (ADR 0030, ADR 0044, ADR 0046,
//! ADR 0047): quick search, the filters of the lists, the search of a tree, and the marking of
//! `g +` and `g -`. Each is a [`Kind`], which says the ways it matches, in the order Ctrl+s
//! Ctrl+s goes round them, the way it starts in for each of `ui.default_search_mode`, and the
//! cases it can take; an [`Input`] is one of them while it is open, with its way and whether case
//! counts. Glyphs at the end of the field say both, in every field. Each field keeps how it was
//! last until Noon Commander quits ([`Memory`]).

use std::collections::HashMap;

use noc_config::SearchMode;
pub(crate) use noc_pattern::Case;
use noc_pattern::{Normalized, Options, PatternError};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use regex::Regex;

use super::cells;
use super::dialog::{self, Field};
use super::fuzzy::Fuzzy;
use super::glyphs::{CaseModes, SearchModes};
use super::keymap::Action;
use super::pattern::Pattern;
use super::theme::Theme;

/// A way to match what is typed: the three of `ui.default_search_mode`, and shell patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Its glyph at the end of a field.
    pub(crate) fn glyph(self, glyphs: SearchModes) -> &'static str {
        match self {
            Self::Plain => glyphs.plain,
            Self::Fzf => glyphs.fzf,
            Self::Regex => glyphs.regex,
            Self::Glob => glyphs.glob,
        }
    }
}

/// The glyph of `case` at the end of a field.
pub(crate) fn case_glyph(case: Case, glyphs: CaseModes) -> &'static str {
    match case {
        Case::Smart => glyphs.smart,
        Case::Ignore => glyphs.ignore,
        Case::Match => glyphs.exact,
    }
}

/// Whether case counts in the order Ctrl+s c goes round them.
const CASES: [Case; 3] = [Case::Smart, Case::Ignore, Case::Match];

/// What is matched: names, or paths, where fzf has a word start after a `/`; or the files that
/// ripgrep or fd search, by globs that they match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Target {
    Names,
    Paths,
    Files,
}

/// What a key typed after Ctrl+s in a field that searches does to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Switch {
    /// The next way the field has, round.
    NextWay,
    /// This way, if the field has it.
    Way(Way),
    /// The next of smart case, case ignored, and case that the field has, round.
    NextCase,
    /// This case, if the field has it.
    Case(Case),
}

impl Switch {
    /// What `action` does to a field that searches, if it does anything.
    pub(crate) fn of(action: Action) -> Option<Self> {
        let switch = match action {
            Action::SearchMode => Self::NextWay,
            Action::SearchPlain => Self::Way(Way::Plain),
            Action::SearchFzf => Self::Way(Way::Fzf),
            Action::SearchRegex => Self::Way(Way::Regex),
            Action::SearchGlob => Self::Way(Way::Glob),
            Action::CaseMode => Self::NextCase,
            Action::CaseSmart => Self::Case(Case::Smart),
            Action::CaseIgnore => Self::Case(Case::Ignore),
            Action::CaseMatch => Self::Case(Case::Match),
            _ => return None,
        };
        Some(switch)
    }
}

/// A kind of field that searches: the ways it matches, what, and whether case counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Kind {
    /// In the order Ctrl+s Ctrl+s goes round them.
    ways: &'static [Way],
    /// The way it starts in when the one asked for is not among them.
    instead: Way,
    target: Target,
    /// In the order Ctrl+s c goes round them; it starts in the first.
    cases: &'static [Case],
}

impl Kind {
    /// Quick search in a panel: `/` and `?`.
    pub(crate) const QUICK_SEARCH: Self = Self {
        ways: &[Way::Plain, Way::Fzf, Way::Regex],
        instead: Way::Fzf,
        target: Target::Names,
        cases: &CASES,
    };
    /// The filter of a list of names: the location menu, the tab menu, the workspaces, the
    /// command history, the key hints, the checksums.
    pub(crate) const FILTER: Self = Self {
        ways: &[Way::Plain, Way::Fzf, Way::Regex],
        instead: Way::Fzf,
        target: Target::Names,
        cases: &CASES,
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
        cases: &CASES,
    };
    /// The globs of the files of the search of a tree, which ripgrep matches, each with smart
    /// case, case ignored, or case (`--iglob` or `--glob`).
    pub(crate) const FILES: Self = Self {
        ways: &[Way::Glob],
        instead: Way::Glob,
        target: Target::Files,
        cases: &CASES,
    };
    /// The same globs when fd searches names, whose `--exclude` always tells case apart.
    pub(crate) const FILES_FD: Self = Self {
        cases: &[Case::Match],
        ..Self::FILES
    };
    /// The pattern of `g +` and `g -`: shell patterns first, as mc's, then text and regular
    /// expressions.
    pub(crate) const MARK: Self = Self {
        ways: &[Way::Glob, Way::Plain, Way::Regex],
        instead: Way::Glob,
        target: Target::Names,
        cases: &CASES,
    };

    /// The way it starts in when `way` is asked for.
    fn start(self, way: Way) -> Way {
        if self.ways.contains(&way) {
            way
        } else {
            self.instead
        }
    }

    /// The way after `way`, as Ctrl+s Ctrl+s goes round.
    fn next(self, way: Way) -> Way {
        let at = self.ways.iter().position(|&each| each == way);
        at.and_then(|at| self.ways.get((at + 1) % self.ways.len()))
            .copied()
            .unwrap_or(self.instead)
    }

    /// The case after `case`, as Ctrl+s c goes round.
    fn next_case(self, case: Case) -> Case {
        let at = self.cases.iter().position(|&each| each == case);
        at.and_then(|at| self.cases.get((at + 1) % self.cases.len()))
            .copied()
            .unwrap_or(self.cases[0])
    }

    /// Whether `action` does something in a field of this kind: going to a way or a case it
    /// has, or to the next if it has more than one; every other action does.
    pub(crate) fn allows(self, action: Action) -> bool {
        match Switch::of(action) {
            Some(Switch::NextWay) => self.ways.len() > 1,
            Some(Switch::Way(way)) => self.ways.contains(&way),
            Some(Switch::NextCase) => self.cases.len() > 1,
            Some(Switch::Case(case)) => self.cases.contains(&case),
            None => true,
        }
    }
}

/// A field that searches, while it is open: its kind, the way it matches now, and whether case
/// counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Input {
    kind: Kind,
    way: Way,
    case: Case,
}

impl Input {
    /// A field of `kind` that starts in `way`, `ui.default_search_mode` mostly, or in the kind's
    /// own if it has no such way; with its first case, smart case but where it has only one.
    pub(crate) fn new(kind: Kind, way: impl Into<Way>) -> Self {
        Self {
            kind,
            way: kind.start(way.into()),
            case: kind.cases[0],
        }
    }

    /// The same field, with `case` if it has it.
    pub(crate) fn with_case(mut self, case: Case) -> Self {
        self.switch(Switch::Case(case));
        self
    }

    pub(crate) fn kind(self) -> Kind {
        self.kind
    }

    /// The way it matches now.
    pub(crate) fn way(self) -> Way {
        self.way
    }

    /// Whether case counts now.
    pub(crate) fn case(self) -> Case {
        self.case
    }

    /// Does what `switch` says, as the keys after Ctrl+s do; whether the way or the case
    /// changed. A way or a case the kind has not changes nothing.
    pub(crate) fn switch(&mut self, switch: Switch) -> bool {
        let before = *self;
        match switch {
            Switch::NextWay => self.way = self.kind.next(self.way),
            Switch::Way(way) if self.kind.ways.contains(&way) => self.way = way,
            Switch::NextCase => self.case = self.kind.next_case(self.case),
            Switch::Case(case) if self.kind.cases.contains(&case) => self.case = case,
            Switch::Way(_) | Switch::Case(_) => {}
        }
        *self != before
    }

    /// What `typed` matches, in the way it matches now, with its case.
    pub(crate) fn matcher(self, typed: &str) -> Matcher {
        Matcher::new(self.way, self.kind.target, typed, self.case)
    }

    /// Whether it has more than one way, which Ctrl+s Ctrl+s and a click on its glyph switch.
    pub(crate) fn switchable(self) -> bool {
        self.kind.ways.len() > 1
    }

    /// Whether `typed` is wrong: no regular expression in the regex way, or no globs of files.
    pub(crate) fn invalid(self, typed: &str) -> bool {
        match (self.kind.target, self.way) {
            (Target::Files, _) => !noc_tools::globs::valid(typed),
            (_, Way::Regex) => self.matcher(typed).error().is_some(),
            _ => false,
        }
    }

    /// Its glyphs, and their style at the end of a field in `style`: each that a click does not
    /// switch, as the field has nothing else, in the style of what cannot be used; the way's in
    /// the style of what is wrong if `invalid`, a text that is no regular expression or no globs.
    pub(crate) fn mark(self, invalid: bool, style: Style, theme: &Theme) -> Mark {
        let (switches, cases) = (self.switchable(), self.kind.cases.len() > 1);
        let usable = |usable: bool| {
            if usable {
                style
            } else {
                style.patch(theme.disabled)
            }
        };
        let way_style = usable(switches);
        Mark {
            case: case_glyph(self.case, theme.glyphs.case_modes),
            way: self.way.glyph(theme.glyphs.search_modes),
            style,
            case_style: usable(cases),
            way_style: if invalid {
                theme.invalid(way_style)
            } else {
                way_style
            },
            switches,
            cases,
        }
    }
}

/// A field that searches, of those that keep how they were last apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Slot {
    /// Quick search, `/` and `?`, in either panel.
    QuickSearch,
    LocationMenu,
    TabMenu,
    Workspaces,
    CommandHistory,
    KeyHints,
    /// The zoxide window.
    Jump,
    /// The text of the search of a tree, in either of its modes.
    FindText,
    /// Its globs, as ripgrep takes them.
    FindFiles,
    /// The pattern of `g +` and `g -`.
    Mark,
    /// The filter of the window of checksums.
    Checksums,
}

impl Slot {
    /// Every slot.
    pub(crate) const ALL: [Self; 11] = [
        Self::QuickSearch,
        Self::LocationMenu,
        Self::TabMenu,
        Self::Workspaces,
        Self::CommandHistory,
        Self::KeyHints,
        Self::Jump,
        Self::FindText,
        Self::FindFiles,
        Self::Mark,
        Self::Checksums,
    ];

    /// The kind of its field.
    pub(crate) fn kind(self) -> Kind {
        match self {
            Self::QuickSearch => Kind::QUICK_SEARCH,
            Self::LocationMenu
            | Self::TabMenu
            | Self::Workspaces
            | Self::CommandHistory
            | Self::KeyHints
            | Self::Checksums => Kind::FILTER,
            Self::Jump => Kind::FILTER_PATHS,
            Self::FindText => Kind::TREE,
            Self::FindFiles => Kind::FILES,
            Self::Mark => Kind::MARK,
        }
    }
}

/// How each field that searches was last, its way and its case, until Noon Commander quits
/// (ADR 0047): the next time it opens, it opens so. One not yet open opens in
/// `ui.default_search_mode`, with smart case.
#[derive(Debug, Clone, Default)]
pub(crate) struct Memory(HashMap<Slot, Input>);

impl Memory {
    /// How the field of `slot` opens: as it was last, else in `mode`.
    pub(crate) fn input(&self, slot: Slot, mode: SearchMode) -> Input {
        self.0
            .get(&slot)
            .copied()
            .unwrap_or_else(|| Input::new(slot.kind(), mode))
    }

    /// Keeps how the field of `slot` is now, if it is of the slot's kind.
    pub(crate) fn keep(&mut self, slot: Slot, input: Input) {
        if input.kind == slot.kind() {
            self.0.insert(slot, input);
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

/// The glyphs at the end of a field that searches, as they are drawn: whether case counts,
/// then how it matches (ADR 0045, ADR 0046). A blank goes before them, and one after each, which
/// kitty draws a Nerd Font icon over rather than spill it on what comes next.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mark {
    pub(crate) case: &'static str,
    pub(crate) way: &'static str,
    /// The style of the field.
    style: Style,
    /// The style of the case's glyph: the field's, or that of what cannot be used where a click
    /// does not switch it.
    case_style: Style,
    /// The style of the way's glyph: the field's, that of what cannot be used, or that of what
    /// is wrong, which shows over it.
    way_style: Style,
    /// Whether a click on the way's glyph switches it.
    switches: bool,
    /// Whether a click on the case's glyph switches it.
    cases: bool,
}

impl Mark {
    /// The cells it takes at the end of a field: the blank before the glyphs, and each glyph
    /// with its blank.
    pub(crate) fn width(self) -> usize {
        cells::width(self.case) + cells::width(self.way) + 3
    }

    /// Its spans, from the blank before the glyphs.
    pub(crate) fn spans(self) -> Vec<Span<'static>> {
        vec![
            Span::styled(" ", self.style),
            Span::styled(self.case, self.case_style),
            Span::styled(" ", self.style),
            Span::styled(self.way, self.way_style),
            Span::styled(" ", self.style),
        ]
    }

    /// Where its glyphs are once drawn from column `x` of line `y`: each with the blank after
    /// it, which kitty draws it over.
    pub(crate) fn drawn_at(self, x: u16, y: u16) -> GlyphSpots {
        let case = to_u16(cells::width(self.case) + 1);
        let way = to_u16(cells::width(self.way) + 1);
        let first = x.saturating_add(1);
        GlyphSpots {
            left: x,
            case: self.cases.then(|| Rect::new(first, y, case, 1)),
            way: self
                .switches
                .then(|| Rect::new(first.saturating_add(case), y, way, 1)),
        }
    }

    /// Draws `field` on the line `area` in the colors of the fields of dialogs, with the glyphs
    /// at its right end. Returns the cursor's column in `area`, and where the glyphs are, for
    /// the mouse; without room for both, the field shows alone.
    pub(crate) fn render_in_field(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        field: &Field,
        theme: &Theme,
    ) -> (u16, Option<GlyphSpots>) {
        let width = usize::from(area.width);
        let glyphs = self.width();
        if width <= glyphs {
            let (line, column) = dialog::field_line(field, width, theme);
            frame.render_widget(line, area);
            return (to_u16(column), None);
        }
        let room = to_u16(width - glyphs);
        let (line, column) = dialog::field_line(field, usize::from(room), theme);
        frame.render_widget(
            line,
            Rect {
                width: room,
                ..area
            },
        );
        let at = Rect::new(area.x + room, area.y, to_u16(glyphs), 1);
        frame.render_widget(Line::from(self.spans()).style(theme.dialog_input), at);
        (to_u16(column), Some(self.drawn_at(at.x, at.y)))
    }

    /// Draws `prompt` on the line `area` in `style`, with `field` in its place in the colors
    /// of the text fields of dialogs, as wide as the prompt leaves room for, and the glyphs at
    /// the end of the field. Returns the cursor's column in `area`, and where the glyphs are,
    /// for the mouse. Where no room is left, the prompt and the text are cut as one, without
    /// them.
    pub(crate) fn render_field(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        (prompt, field): (&Prompt, &Field),
        style: Style,
        theme: &Theme,
    ) -> (u16, Option<GlyphSpots>) {
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
        let (column, drawn) = self.render_in_field(frame, field_area, field, theme);
        frame.render_widget(Line::styled(prompt.after.clone(), style), after_area);
        (to_u16(before) + column, drawn)
    }
}

/// Where the glyphs at the end of a field that searches were drawn, for the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GlyphSpots {
    /// The blank before them, where the field before them ends.
    left: u16,
    /// `None` if the field has one case.
    case: Option<Rect>,
    /// `None` if the field has one way.
    way: Option<Rect>,
}

impl GlyphSpots {
    /// What a click at `at` stands for: on the glyph of the case, Ctrl+s c; on that of the way,
    /// Ctrl+s Ctrl+s.
    pub(crate) fn action_at(self, at: Position) -> Option<Action> {
        if self.case.is_some_and(|case| case.contains(at)) {
            Some(Action::CaseMode)
        } else if self.way.is_some_and(|way| way.contains(at)) {
            Some(Action::SearchMode)
        } else {
            None
        }
    }

    /// The first column of the glyphs, where the field before them ends.
    pub(crate) fn left(self) -> u16 {
        self.left
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
        // Text and patterns have no escapes that look like capitals.
        let fold = !case.counts(typed, false);
        match way {
            Way::Plain => Self::Plain {
                typed: form(typed, fold),
                fold,
            },
            Way::Fzf => Self::Fuzzy(Fuzzy::new(typed, target == Target::Paths, case)),
            Way::Regex => {
                let options = Options { regex: true, case };
                Self::Regex(noc_pattern::text(&noc_pattern::nfc(typed), options))
            }
            Way::Glob => Self::Glob {
                pattern: Pattern::new(&form(typed, fold)),
                fold,
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
        Matcher::new(way, Target::Names, typed, Case::Smart)
    }

    /// The ways a kind starts in for each of `ui.default_search_mode`: plain, fzf, regex.
    fn starts(kind: Kind) -> [Way; 3] {
        [SearchMode::Plain, SearchMode::Fzf, SearchMode::Regex]
            .map(|mode| Input::new(kind, mode).way())
    }

    /// The ways Ctrl+s Ctrl+s goes round from the one it starts in, back to it.
    fn round(kind: Kind, way: Way) -> Vec<Way> {
        let mut input = Input::new(kind, way);
        let mut ways = vec![input.way()];
        while input.switch(Switch::NextWay) && input.way() != way {
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
            (
                "g + and g -",
                Kind::MARK,
                &[Glob, Plain, Regex],
                [Plain, Glob, Regex],
            ),
            (
                "the globs of files",
                Kind::FILES,
                &[Glob],
                [Glob, Glob, Glob],
            ),
            (
                "the globs for fd",
                Kind::FILES_FD,
                &[Glob],
                [Glob, Glob, Glob],
            ),
        ];
        for (name, kind, ways, start) in kinds {
            assert_eq!(kind.ways, ways, "{name}");
            assert_eq!(
                starts(kind),
                start,
                "{name}: what ui.default_search_mode starts it in"
            );
            assert_eq!(
                round(kind, ways[0]),
                ways,
                "{name}: Ctrl+s Ctrl+s goes round them"
            );
        }
        assert_eq!(Kind::FILTER_PATHS.target, Target::Paths);
    }

    #[test]
    fn the_keys_after_ctrl_s_go_to_a_way_the_field_has_and_switch_case() {
        let mut input = Input::new(Kind::TREE, Way::Fzf);
        assert_eq!(input.way(), Way::Regex, "a way it has not");
        assert_eq!(input.case(), Case::Smart, "smart case to start");
        assert!(!input.switch(Switch::Way(Way::Glob)), "nor goes to");
        assert!(!input.switch(Switch::Way(Way::Regex)), "the way it has");
        assert!(input.switch(Switch::Way(Way::Plain)));
        assert_eq!(input.way(), Way::Plain);
        let cases: Vec<Case> = (0..3)
            .map(|_| {
                input.switch(Switch::NextCase);
                input.case()
            })
            .collect();
        assert_eq!(cases, [Case::Ignore, Case::Match, Case::Smart], "round");
        assert!(input.switch(Switch::Case(Case::Match)));
        assert!(!input.switch(Switch::Case(Case::Match)));
        assert_eq!(input.way(), Way::Plain, "the way stays");
        assert_eq!(
            Input::new(Kind::MARK, Way::Glob)
                .with_case(Case::Ignore)
                .case(),
            Case::Ignore
        );
    }

    #[test]
    fn a_glyph_that_a_click_does_not_switch_shows_that_it_cannot_be_used() {
        let theme = Theme::terminal();
        let field = theme.dialog_input;
        let disabled = field.patch(theme.disabled);
        let styles = |input: Input, invalid: bool| {
            let spans = input.mark(invalid, field, &theme).spans();
            (spans[1].style, spans[3].style)
        };
        assert_eq!(
            styles(Input::new(Kind::FILTER, Way::Fzf), false),
            (field, field),
            "both switch"
        );
        assert_eq!(
            styles(Input::new(Kind::FILES, Way::Glob), false),
            (field, disabled),
            "one way"
        );
        assert_eq!(
            styles(Input::new(Kind::FILES_FD, Way::Glob), false),
            (disabled, disabled),
            "one case too"
        );
        assert_eq!(
            styles(Input::new(Kind::FILES, Way::Glob), true).1,
            theme.invalid(disabled),
            "what is wrong shows over it"
        );
        assert_eq!(theme.invalid(disabled).fg, theme.invalid(field).fg);
    }

    #[test]
    fn a_field_for_fd_tells_case_apart_alone() {
        let mut fd = Input::new(Kind::FILES_FD, Way::Glob);
        assert_eq!(fd.case(), Case::Match);
        assert!(!fd.switch(Switch::NextCase));
        assert!(!fd.switch(Switch::Case(Case::Ignore)));
        assert_eq!(fd.with_case(Case::Smart).case(), Case::Match);
        for action in [
            Action::CaseMode,
            Action::CaseSmart,
            Action::CaseIgnore,
            Action::SearchMode,
        ] {
            assert!(!Kind::FILES_FD.allows(action), "{action:?}");
        }
        assert!(Kind::FILES.allows(Action::CaseIgnore));
        assert!(!Kind::FILES.allows(Action::SearchMode), "one way");
    }

    #[test]
    fn each_field_opens_as_it_was_last_else_in_the_default() {
        let mut memory = Memory::default();
        let fresh = memory.input(Slot::Mark, SearchMode::Plain);
        assert_eq!((fresh.way(), fresh.case()), (Way::Plain, Case::Smart));
        assert_eq!(
            memory.input(Slot::Mark, SearchMode::Fzf).way(),
            Way::Glob,
            "no fzf"
        );
        assert_eq!(
            memory.input(Slot::FindText, SearchMode::Fzf).way(),
            Way::Regex
        );
        let mut regex = Input::new(Kind::FILTER, Way::Regex);
        regex.switch(Switch::Case(Case::Match));
        memory.keep(Slot::TabMenu, regex);
        assert_eq!(
            memory.input(Slot::TabMenu, SearchMode::Plain),
            regex,
            "whatever the default"
        );
        assert_eq!(
            memory.input(Slot::Workspaces, SearchMode::Plain).way(),
            Way::Plain,
            "each field its own"
        );
        memory.keep(Slot::QuickSearch, Input::new(Kind::MARK, Way::Glob));
        assert_eq!(
            memory.input(Slot::QuickSearch, SearchMode::Plain).way(),
            Way::Plain,
            "a field of another kind is not kept"
        );
    }

    #[test]
    fn a_kind_allows_the_ways_it_has() {
        assert!(Kind::TREE.allows(Action::SearchRegex));
        assert!(!Kind::TREE.allows(Action::SearchFzf));
        assert!(!Kind::FILTER.allows(Action::SearchGlob));
        assert!(Kind::MARK.allows(Action::SearchGlob));
        let one = Kind {
            ways: &[Way::Glob],
            ..Kind::MARK
        };
        assert!(!one.allows(Action::SearchMode), "no next way");
        for action in [
            Action::SearchMode,
            Action::CaseMode,
            Action::CaseIgnore,
            Action::Up,
        ] {
            assert!(Kind::FILTER.allows(action), "{action:?}");
        }
        assert_eq!(Switch::of(Action::Up), None);
        assert_eq!(
            Switch::of(Action::CaseMatch),
            Some(Switch::Case(Case::Match))
        );
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
    fn the_field_of_a_prompt_has_the_colors_of_the_fields_of_dialogs_and_ends_with_the_glyphs() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let theme = Theme::terminal();
        let input = Input::new(Kind::FILTER, Way::Regex);
        let mark = input.mark(false, theme.dialog_input, &theme);
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
        let (column, glyphs) = drawn;
        assert_eq!(usize::from(column), before + 2, "after what is typed");
        assert_eq!(bg(0), theme.dialog.bg.unwrap_or_default());
        for x in before..28 {
            assert_eq!(bg(x), theme.dialog_input.bg.unwrap_or_default(), "cell {x}");
        }
        let line: String = (0..30)
            .map(|x| buffer[(x, 0)].symbol().to_owned())
            .collect();
        assert!(
            line.ends_with(" Aa .*  !"),
            "a blank before each glyph and after: {line}"
        );
        let glyphs = glyphs.unwrap();
        assert_eq!(glyphs.left(), 21);
        assert_eq!(
            glyphs.action_at(Position::new(22, 0)),
            Some(Action::CaseMode)
        );
        assert_eq!(
            glyphs.action_at(Position::new(24, 0)),
            Some(Action::CaseMode),
            "its blank"
        );
        assert_eq!(
            glyphs.action_at(Position::new(25, 0)),
            Some(Action::SearchMode)
        );
        assert_eq!(
            glyphs.action_at(Position::new(27, 0)),
            Some(Action::SearchMode)
        );
        assert_eq!(glyphs.action_at(Position::new(21, 0)), None);
        assert_eq!(glyphs.action_at(Position::new(28, 0)), None);
        assert_eq!(
            bg(29),
            theme.dialog.bg.unwrap_or_default(),
            "the prompt after it"
        );
    }

    #[test]
    fn a_field_too_narrow_for_its_glyphs_shows_alone() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let theme = Theme::terminal();
        let mark = Input::new(Kind::FILTER, Way::Regex).mark(false, theme.dialog_input, &theme);
        let mut terminal = Terminal::new(TestBackend::new(7, 1)).unwrap();
        let mut glyphs = Some(GlyphSpots {
            left: 0,
            case: None,
            way: None,
        });
        terminal
            .draw(|frame| {
                (_, glyphs) =
                    mark.render_in_field(frame, frame.area(), &Field::plain("ab"), &theme);
            })
            .unwrap();
        assert_eq!(glyphs, None);
    }

    #[test]
    fn only_a_regular_expression_is_invalid() {
        assert!(Input::new(Kind::FILTER, Way::Regex).invalid("("));
        assert!(!Input::new(Kind::FILTER, Way::Regex).invalid("a"));
        assert!(!Input::new(Kind::FILTER, Way::Plain).invalid("("));
        assert!(
            !Input::new(Kind::MARK, Way::Glob).invalid("["),
            "taken as it is"
        );
        let files = Input::new(Kind::FILES, Way::Glob);
        assert!(files.invalid("*.rs ["), "globs of files are globset's");
        assert!(!files.invalid("*.rs !target"));
        assert!(Input::new(Kind::FILTER, Way::Fzf).switchable());
    }

    #[test]
    fn plain_text_matches_anywhere_with_smart_case() {
        let mut plain = names(Way::Plain, "fi");
        assert_eq!(plain.score("proFile"), Some(0));
        assert_eq!(plain.score("cfg"), None);
        assert_eq!(plain.positions("FileFI"), [0, 1, 4, 5]);
        assert!(!plain.ranks() && plain.error().is_none());
        let mut capital = names(Way::Plain, "Fi");
        assert_eq!(capital.score("profile"), None, "a capital");
        assert_eq!(capital.positions("FileFI"), [0, 1]);
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
        let mut plain = names(Way::Plain, "caf\u{e9}");
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
        let mut fuzzy = Matcher::new(Way::Fzf, Target::Paths, "cfg", Case::Smart);
        assert!(fuzzy.ranks());
        assert!(fuzzy.score("config.rs").is_some());
        assert_eq!(fuzzy.positions("config.rs"), [0, 3, 5]);
    }

    #[test]
    fn a_pattern_matches_the_whole_name_in_either_form_with_smart_case() {
        let mut glob = names(Way::Glob, "*.rs");
        assert_eq!(glob.score("main.RS"), Some(0));
        assert_eq!(glob.score("main.rs.bak"), None, "the whole name");
        assert_eq!(glob.positions("main.rs"), [] as [usize; 0]);
        assert!(!glob.ranks() && glob.error().is_none());
        let mut nfd = names(Way::Glob, "caf\u{e9}*");
        assert_eq!(nfd.score("Cafe\u{301}.txt"), Some(0));
        let mut capital = names(Way::Glob, "*.RS");
        assert_eq!(capital.score("main.rs"), None, "a capital");
        assert_eq!(capital.score("MAIN.RS"), Some(0));
    }

    #[test]
    fn every_way_matches_with_smart_case_case_ignored_or_case() {
        let with = |case, way, typed: &str| {
            let typed = if way == Way::Glob {
                format!("{typed}*")
            } else {
                typed.to_owned()
            };
            Matcher::new(way, Target::Names, &typed, case)
        };
        for way in [Way::Plain, Way::Fzf, Way::Regex, Way::Glob] {
            assert!(
                with(Case::Smart, way, "read").score("README").is_some(),
                "{way:?}"
            );
            assert_eq!(
                with(Case::Smart, way, "Read").score("README"),
                None,
                "{way:?}"
            );
            assert!(
                with(Case::Ignore, way, "Read").score("README").is_some(),
                "{way:?}"
            );
            assert_eq!(
                with(Case::Match, way, "read").score("README"),
                None,
                "{way:?}"
            );
            assert!(
                with(Case::Match, way, "READ").score("README").is_some(),
                "{way:?}"
            );
        }
        let counts = |way, typed| Matcher::new(way, Target::Names, typed, Case::Match);
        let mut plain = counts(Way::Plain, "caf\u{e9}");
        assert_eq!(
            plain.positions("my cafe\u{301}"),
            [3, 4, 5, 6, 7],
            "either form"
        );
    }
}
