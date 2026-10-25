//! The zoxide window of `g z`: the directories zoxide ranks highest for the keywords typed,
//! best first, as `z foo bar` picks them in a shell, in the plain search mode; or
//! all of them, filtered by what is typed, as fzf ranks them or as a regular expression in
//! zoxide's order. A list as the others, on [`Picker`]: it opens in Insert mode, as `zi` does,
//! and Esc goes to the rows, where `/` types the keywords again; Enter opens one in the active
//! panel.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use noc_config::SearchMode;
use noc_scroll::scrollbar::Bar;
use noc_tools::zoxide::Scored;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;

use super::cells::{self, Align, Cut};
use super::dialog::Colors;
use super::keymap::{Action, Mode, Resolved};
use super::loading::Loading;
use super::mouse::Pointer;
use super::picker::{Body, MenuLine, Pick, Picker, ShownRow};
use super::search::{Input, Kind};
use super::spinner::{Spin, Wait};
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the window gets, in cells, borders included: paths are long.
const WIDTH: u16 = 78;
/// Rows the window shows at most.
const MAX_ROWS: u16 = 16;

/// What a key did in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JumpEvent {
    Pending,
    Closed,
    /// The keywords changed: ask zoxide again.
    Query,
    /// Open this directory in the active panel.
    Open(PathBuf),
}

/// The window, with the answer to the last query it asked.
#[derive(Debug)]
pub(crate) struct JumpMenu {
    /// `~` stands for it.
    home: PathBuf,
    /// The local directory the panel shows, which zoxide leaves out.
    exclude: Option<PathBuf>,
    /// zoxide's answers; the rows listed are those without keywords, every directory.
    loading: Loading,
    /// zoxide's answer, best first.
    found: Vec<Scored>,
    /// The keywords: in the plain mode zoxide picks the directories by them; in the others the
    /// picker filters every directory, as it shows them, from `~`.
    picker: Picker,
}

impl JumpMenu {
    /// A window for a panel on `exclude`, if it is local, waiting for the query `generation`.
    /// It opens with the keywords taking what is typed, as `zi` does.
    pub(crate) fn new(home: PathBuf, exclude: Option<PathBuf>, generation: u64) -> Self {
        Self {
            home,
            exclude,
            loading: Loading::new(generation),
            found: Vec::new(),
            picker: Picker::new(Input::new(Kind::FILTER_PATHS, SearchMode::Plain)).plain_keywords(),
        }
    }

    /// The same window, which matches as `input` says, as it was last.
    pub(crate) fn with_input(mut self, input: Input) -> Self {
        self.picker.set_input(input);
        self
    }

    /// The same window, whose skeleton has as many `rows` as the last window had, if any.
    pub(crate) fn rows_like(mut self, rows: Option<u16>) -> Self {
        self.loading = self.loading.lines_like(rows);
        self
    }

    /// Rows of the last answer without keywords, for the skeleton of the next window.
    pub(crate) fn rows_listed(&self) -> Option<u16> {
        self.loading.listed()
    }

    /// The keywords to ask zoxide for: those typed, separated by spaces; none, for every
    /// directory, if the window filters them itself.
    pub(crate) fn keywords(&self) -> Vec<String> {
        if self.picker.filters() {
            return Vec::new();
        }
        self.picker
            .text()
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    pub(crate) fn exclude(&self) -> Option<&Path> {
        self.exclude.as_deref()
    }

    /// Waits for the query `generation` instead. The rows stay until its answer comes.
    pub(crate) fn ask(&mut self, generation: u64) {
        self.loading.ask(generation);
    }

    /// The wait for an answer, whose dots turn in front of the title.
    pub(crate) fn wait(&self) -> Option<&Wait> {
        self.loading.wait()
    }

    /// Takes zoxide's answer to the query `generation`: directories best first, or why there
    /// are none. The cursor goes back to the best.
    pub(crate) fn found(&mut self, generation: u64, result: Result<Vec<Scored>, String>) {
        if !self.loading.answers(generation) {
            return;
        }
        match result {
            Ok(found) => {
                let listed = self.picker.text().is_empty().then_some(found.len());
                self.found = found;
                self.loading.came(listed);
            }
            Err(reason) => {
                self.found.clear();
                self.loading.failed(&reason);
            }
        }
        let texts = (self.found.iter())
            .map(|scored| vec![self.path_shown(&scored.path)])
            .collect();
        self.picker.set_rows(texts);
        self.picker.focus_best();
    }

    /// The mode keys are typed in: Insert while the keywords take them.
    pub(crate) fn mode(&self) -> Mode {
        self.picker.mode()
    }

    /// The kind of its filter while that takes the keys.
    pub(crate) fn search(&self) -> Option<Kind> {
        self.picker.search()
    }

    /// How its filter matches now, and whether case counts.
    pub(crate) fn input(&self) -> Input {
        self.picker.input()
    }

    /// The rows, for the count before a page key.
    pub(crate) fn picker_mut(&mut self) -> &mut Picker {
        &mut self.picker
    }

    /// Takes a key: the picker's, whose filter is the keywords. Enter opens the directory under
    /// the cursor.
    pub(crate) fn handle(&mut self, input: Resolved) -> JumpEvent {
        let filtered = self.picker.filters();
        match self.picker.handle(input) {
            Pick::Chosen(row) => match self.picker.shown(&self.found).get(row) {
                Some(scored) => JumpEvent::Open(scored.path.clone()),
                None => JumpEvent::Pending,
            },
            Pick::Closed => JumpEvent::Closed,
            // zoxide picks the directories by the keywords, or gives every one anew.
            Pick::Filtered if !filtered || !self.picker.filters() => JumpEvent::Query,
            Pick::Pending | Pick::Filtered | Pick::Other(_) => JumpEvent::Pending,
        }
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.picker.bar()
    }

    /// Shows the rows from `top`, where the thumb of the scroll bar was dragged.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        self.picker.scroll_to(top);
    }

    /// Takes a press of the mouse, where the window was drawn last: a click puts the cursor on
    /// a row, and returns the key that a double click on one, or a click outside the window,
    /// stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        self.picker.pointer(pointer)
    }

    /// A directory as the window shows it: under the home directory, from `~`.
    fn path_shown(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~/{}", cells::sanitize(rest.as_os_str().as_bytes())),
            Err(_) => cells::sanitize(path.as_os_str().as_bytes()),
        }
    }

    /// Draws the window centered over `area`, the panels: the keywords, then the directories
    /// with their scores, scrolled to the cursor, and the whole directory under the cursor below
    /// them.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme, spin: &Spin) {
        let found = self.picker.shown(&self.found);
        let paths: Vec<String> = (found.iter())
            .map(|scored| self.path_shown(&scored.path))
            .collect();
        let formats = noc_locale::current();
        let scores: Vec<String> = (found.iter())
            .map(|scored| formats.decimal(scored.score, 1, true))
            .collect();
        let score_width = (scores.iter())
            .map(|score| cells::width(score))
            .max()
            .unwrap_or(0);
        let lines: Vec<MenuLine> = (0..found.len()).map(MenuLine::Row).collect();
        let title = fl!("jump-title");
        let body = Body {
            title: &title,
            width: WIDTH,
            lines: &lines,
            max_lines: Some(MAX_ROWS),
            prompt: Some(|text| fl!("jump-keywords", text = text)),
            message: self.loading.error().map(str::to_owned),
            nothing: Some(fl!("jump-nothing")),
            loading: Some((&self.loading, spin)),
            full: paths.get(self.picker.cursor()).cloned(),
            ..Body::default()
        };
        let focused = Colors::of(theme, false).focused_style();
        let marks = theme.glyphs.marks;
        self.picker.render(frame, area, theme, body, &mut |row| {
            let ShownRow {
                index,
                selected,
                width,
            } = row;
            let styles = if selected {
                (focused, theme.truncated_cursor)
            } else {
                (theme.dialog, theme.truncated_dialog)
            };
            // The directory, a space, the score.
            let path_width = width.saturating_sub(score_width + 1);
            let fit = (Align::Left, Cut::Start);
            let mut line = cells::fitted(&paths[index], path_width, fit, styles, marks);
            line.spans.push(Span::styled(" ", styles.0));
            let score = cells::fit(&scores[index], score_width, Align::Right, styles, marks);
            line.spans.extend(score);
            line
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::glyphs::Glyphs;
    use crate::tui::picker::FULL_ROWS;
    use crate::tui::spinner;

    fn scored(score: f64, path: &str) -> Scored {
        Scored {
            score,
            path: PathBuf::from(path),
        }
    }

    fn found() -> Vec<Scored> {
        vec![
            scored(56.0, "/home/me/src/noc"),
            scored(12.5, "/srv/www"),
            scored(4.0, "/home/me"),
            scored(0.3, "/home/me/Downloads/a b"),
        ]
    }

    fn menu() -> JumpMenu {
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1);
        menu.found(1, Ok(found()));
        menu
    }

    fn press(menu: &mut JumpMenu, action: Action) -> JumpEvent {
        menu.handle(Resolved::Action(action))
    }

    fn draw(menu: &mut JumpMenu, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(70, height)).unwrap();
        // Drawn again once what it waits for shows, with a still spinner.
        let start = Instant::now();
        for at in [start, start + spinner::DELAY] {
            let spin = Spin::new(at, start, false, Glyphs::PLAIN.spinners);
            terminal
                .draw(|frame| menu.render(frame, frame.area(), &Theme::terminal(), &spin))
                .unwrap();
        }
        terminal.backend().to_string()
    }

    #[test]
    fn opens_typing_keywords_and_typing_asks_again() {
        let mut menu = menu();
        assert_eq!(menu.mode(), Mode::Insert);
        press(&mut menu, Action::NormalMode);
        assert_eq!(menu.mode(), Mode::Normal, "Esc goes to the rows");
        assert_eq!(menu.handle(Resolved::Insert('2')), JumpEvent::Pending);
        press(&mut menu, Action::QuickSearch);
        assert_eq!(menu.mode(), Mode::Insert, "/ types the keywords again");
        assert_eq!(menu.handle(Resolved::Insert('s')), JumpEvent::Query);
        assert_eq!(menu.handle(Resolved::Insert(' ')), JumpEvent::Query);
        assert_eq!(menu.handle(Resolved::Insert('2')), JumpEvent::Query);
        assert_eq!(menu.keywords(), ["s", "2"]);
        assert_eq!(press(&mut menu, Action::Backspace), JumpEvent::Query);
        assert_eq!(press(&mut menu, Action::Backspace), JumpEvent::Query);
        assert_eq!(press(&mut menu, Action::Backspace), JumpEvent::Query);
        assert_eq!(press(&mut menu, Action::Backspace), JumpEvent::Pending);
        assert_eq!(menu.keywords(), [] as [String; 0]);
        assert_eq!(press(&mut menu, Action::Cancel), JumpEvent::Closed);
    }

    #[test]
    fn the_keywords_are_a_text_field_and_only_edits_ask_again() {
        let mut menu = menu();
        for c in "src".chars() {
            menu.handle(Resolved::Insert(c));
        }
        assert_eq!(press(&mut menu, Action::Left), JumpEvent::Pending);
        assert_eq!(press(&mut menu, Action::Left), JumpEvent::Pending);
        assert_eq!(menu.handle(Resolved::Insert(' ')), JumpEvent::Query);
        assert_eq!(menu.keywords(), ["s", "rc"]);
        assert_eq!(press(&mut menu, Action::DeleteToEnd), JumpEvent::Query);
        assert_eq!(menu.keywords(), ["s"]);
        // At the end, the character before, as on vim's command line.
        assert_eq!(press(&mut menu, Action::DeleteChar), JumpEvent::Query);
        assert_eq!(menu.keywords(), ["s"]);
        assert_eq!(press(&mut menu, Action::DeleteToStart), JumpEvent::Query);
        assert_eq!(menu.keywords(), [] as [String; 0]);
        // Home goes to the first row, as in the other lists.
        press(&mut menu, Action::End);
        press(&mut menu, Action::Home);
        assert_eq!(
            press(&mut menu, Action::Confirm),
            JumpEvent::Open(PathBuf::from("/home/me/src/noc"))
        );
    }

    #[test]
    fn its_thumb_scrolls_the_rows_and_the_cursor_stays_on_screen() {
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1);
        let many = (0..30).map(|n| scored(f64::from(30 - n), &format!("/d/{n:02}")));
        menu.found(1, Ok(many.collect()));
        let top = draw(&mut menu, 16);
        assert!(top.contains('▲') && top.contains('█'), "{top}");
        let bar = menu.bar().unwrap();
        menu.scroll_to(bar.dragged(0, u16::MAX));
        let end = draw(&mut menu, 16);
        assert!(end.contains("/d/29") && !end.contains("/d/00"), "{end}");
        press(&mut menu, Action::NormalMode);
        let JumpEvent::Open(path) = press(&mut menu, Action::Confirm) else {
            panic!("expected a directory");
        };
        assert!(
            end.contains(&*path.to_string_lossy()),
            "on the screen: {path:?}"
        );
    }

    #[test]
    fn enter_opens_the_row_under_the_cursor() {
        let mut menu = menu();
        assert_eq!(
            press(&mut menu, Action::Confirm),
            JumpEvent::Open(PathBuf::from("/home/me/src/noc"))
        );
        press(&mut menu, Action::End);
        press(&mut menu, Action::Up);
        assert_eq!(
            press(&mut menu, Action::Confirm),
            JumpEvent::Open(PathBuf::from("/home/me"))
        );
        menu.found(1, Ok(Vec::new()));
        assert_eq!(press(&mut menu, Action::Confirm), JumpEvent::Pending);
    }

    #[test]
    fn fuzzy_keywords_filter_without_asking_zoxide() {
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1)
            .with_input(Input::new(Kind::FILTER_PATHS, SearchMode::Fzf));
        menu.found(1, Ok(found()));
        assert_eq!(menu.keywords(), [] as [String; 0]);
        for c in "dl".chars() {
            assert_eq!(menu.handle(Resolved::Insert(c)), JumpEvent::Pending);
        }
        assert_eq!(menu.keywords(), [] as [String; 0], "zoxide gets none");
        assert_eq!(
            press(&mut menu, Action::Confirm),
            JumpEvent::Open(PathBuf::from("/home/me/Downloads/a b"))
        );
        assert_eq!(press(&mut menu, Action::Backspace), JumpEvent::Pending);
        assert_eq!(press(&mut menu, Action::Backspace), JumpEvent::Pending);
        assert_eq!(menu.picker.len(), 4, "every directory again");
        menu.handle(Resolved::Insert('~'));
        assert_eq!(
            menu.picker.len(),
            3,
            "the paths as the window shows them, from ~"
        );
        assert_eq!(
            press(&mut menu, Action::Confirm),
            JumpEvent::Open(PathBuf::from("/home/me/src/noc")),
            "as good: in zoxide's order"
        );
    }

    #[test]
    fn a_regular_expression_filters_in_zoxides_order_and_alt_r_switches_the_mode() {
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1)
            .with_input(Input::new(Kind::FILTER_PATHS, SearchMode::Regex));
        menu.found(1, Ok(found()));
        for c in "^~/[sD]".chars() {
            assert_eq!(menu.handle(Resolved::Insert(c)), JumpEvent::Pending);
        }
        assert_eq!(menu.keywords(), [] as [String; 0], "zoxide gets none");
        assert_eq!(menu.picker.shown(0..4), [0, 3], "zoxide's order");
        menu.handle(Resolved::Insert('('));
        assert_eq!(menu.picker.len(), 0);
        assert!(draw(&mut menu, 10).contains("Not a regular expression: error: unclosed group"));
        assert_eq!(
            press(&mut menu, Action::SearchMode),
            JumpEvent::Query,
            "zoxide matches"
        );
        assert_eq!(menu.keywords(), ["^~/[sD]("]);
        assert_eq!(
            press(&mut menu, Action::SearchMode),
            JumpEvent::Query,
            "every directory"
        );
        assert_eq!(menu.keywords(), [] as [String; 0]);
        menu.ask(2);
        menu.found(2, Ok(found()));
        press(&mut menu, Action::DeleteToStart);
        assert_eq!(menu.picker.len(), 4);
        assert_eq!(
            press(&mut menu, Action::SearchMode),
            JumpEvent::Pending,
            "fzf to regex"
        );
        assert_eq!(menu.keywords(), [] as [String; 0], "the window filters");
    }

    #[test]
    fn stale_answers_are_dropped_and_new_ones_start_at_the_best() {
        let mut menu = menu();
        press(&mut menu, Action::Down);
        menu.ask(2);
        menu.found(1, Ok(Vec::new()));
        assert_eq!(menu.found.len(), 4);
        menu.found(2, Ok(found()[1..].to_vec()));
        assert_eq!(
            press(&mut menu, Action::Confirm),
            JumpEvent::Open(PathBuf::from("/srv/www"))
        );
    }

    #[test]
    fn says_why_it_lists_nothing() {
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1);
        let text = draw(&mut menu, 10);
        assert!(text.contains("═ … zoxide ═"), "{text}");
        let bars = text.lines().filter(|line| line.contains("---")).count();
        assert_eq!(bars, 1, "a skeleton of the rows to come: {text}");
        menu.found(1, Ok(Vec::new()));
        assert!(draw(&mut menu, 10).contains("zoxide knows no directory"));
        menu.found(1, Err("zoxide is not installed".to_owned()));
        assert!(draw(&mut menu, 10).contains("zoxide is not installed"));
    }

    #[test]
    fn a_window_waits_with_as_many_rows_as_the_last_one_had_without_keywords() {
        let mut first = menu();
        assert_eq!(first.rows_listed(), Some(4));
        first.handle(Resolved::Insert('s'));
        first.ask(2);
        first.found(2, Ok(found()[1..2].to_vec()));
        assert_eq!(first.rows_listed(), Some(4), "keywords pick only some");

        let mut next =
            JumpMenu::new(PathBuf::from("/home/me"), None, 1).rows_like(first.rows_listed());
        let text = draw(&mut next, 20);
        let bars = text.lines().filter(|line| line.contains("---")).count();
        let full = usize::from(FULL_ROWS + 1);
        assert_eq!(
            bars,
            4 + full,
            "the rows, and the whole row under them: {text}"
        );
        next.found(1, Ok(found()));
        let rows = draw(&mut next, 20);
        let lines = |text: &str| text.lines().filter(|line| !line.trim().is_empty()).count();
        assert_eq!(lines(&rows), lines(&text), "the box stays as tall: {rows}");
    }

    #[test]
    fn scrolls_to_the_cursor() {
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1);
        let many = (0..30)
            .map(|index| scored(f64::from(30 - index), &format!("/d/{index}")))
            .collect();
        menu.found(1, Ok(many));
        press(&mut menu, Action::End);
        let text = draw(&mut menu, 12);
        assert!(text.contains("/d/29") && !text.contains("/d/0 "), "{text}");
    }

    #[test]
    fn the_cursor_stands_right_after_the_keywords() {
        let mut menu = menu();
        let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
        let mut cursor_after = |menu: &mut JumpMenu, text: &str| {
            for c in text.chars() {
                menu.handle(Resolved::Insert(c));
            }
            let now = Instant::now();
            let spin = Spin::new(now, now, false, Glyphs::PLAIN.spinners);
            terminal
                .draw(|frame| menu.render(frame, frame.area(), &Theme::terminal(), &spin))
                .unwrap();
            terminal.get_cursor_position().unwrap()
        };
        // The frame is in column 0, a blank in column 1, and `Jump to: ` 9; the glyph of the
        // mode ends the field.
        let empty = cursor_after(&mut menu, "");
        assert_eq!(empty.x, 2 + 9);
        assert_eq!(cursor_after(&mut menu, "src").x, empty.x + 3);
        assert_eq!(
            cursor_after(&mut menu, " ").x,
            empty.x + 4,
            "after a space too"
        );
    }

    #[test]
    fn draws_directories_from_home_with_their_scores() {
        let mut menu = menu();
        insta::assert_snapshot!(draw(&mut menu, 12));
    }

    #[test]
    fn shows_the_whole_directory_under_the_cursor_under_the_rows() {
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1);
        let long = "/srv/www/clients/acme-corporation-international/deploy/releases/2026-10-15";
        menu.found(1, Ok(vec![scored(9.0, long), scored(1.0, "/srv")]));
        insta::assert_snapshot!(draw(&mut menu, 12));
    }
}
