//! The zoxide window of `g z`: the directories zoxide ranks highest for the keywords typed,
//! best first, as `z foo bar` picks them in a shell, in the plain mode of `ui.search_mode`; or
//! all of them, filtered by what is typed, as fzf ranks them or as a regular expression in
//! zoxide's order. It opens in Insert mode, as `zi` does, and Esc goes to the rows, where `/`
//! types the keywords again; Enter opens one in the active panel.

use std::cell::RefCell;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use noc_config::SearchMode;
use noc_scroll::scrollbar::{self, Bar};
use noc_tools::zoxide::Scored;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};

use super::cells::{self, Align, Cut};
use super::dialog::{
    Colors, Field, draw_box, draw_separator, draw_title_dots, frame_around, right_frame,
};
use super::keymap::{Action, Mode, Resolved};
use super::mouse::{Drawn, Pointer};
use super::picker::{FULL_ROWS, Whole};
use super::scroll::Window;
use super::search_mode::{self, Matcher, Prompt};
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
    /// Of the query the window waits for; an answer to an older one is dropped.
    generation: u64,
    /// The keywords, a text field.
    keywords: Field,
    /// The keywords take what is typed, in Insert mode.
    typing: bool,
    /// How it matches the keywords: `ui.search_mode` until Ctrl+s switches it. In the plain
    /// mode it asks zoxide for each change of the keywords; in the others, once for every
    /// directory, which it filters itself.
    mode: SearchMode,
    /// Why the keywords are not a regular expression, in the regex mode.
    invalid: Option<String>,
    /// zoxide's answer, best first.
    found: Vec<Scored>,
    /// The rows: indices into `found` of the directories that match, best first.
    rows: Vec<usize>,
    /// `true` once the first answer arrived.
    loaded: bool,
    /// For the first answer.
    wait: Wait,
    error: Option<String>,
    /// The row under the cursor.
    cursor: usize,
    /// The rows on screen, as vim's window scrolls them, and how many at the last render.
    window: Window,
    page: usize,
    /// The count before the next Ctrl+f or Ctrl+b, which scroll that many pages at once.
    pages: usize,
    /// Where the last render drew it, for the mouse.
    drawn: RefCell<Drawn>,
    /// The scroll bar of the rows at the last render, on the right of the frame.
    bar: Option<Bar>,
    /// Whether `K` asks for the whole directory under the cursor under the rows.
    full: Whole,
}

impl JumpMenu {
    /// A window for a panel on `exclude`, if it is local, waiting for the query `generation`.
    /// It opens with the keywords taking what is typed, as `zi` does.
    pub(crate) fn new(home: PathBuf, exclude: Option<PathBuf>, generation: u64) -> Self {
        Self {
            home,
            exclude,
            generation,
            keywords: Field::plain(""),
            typing: true,
            mode: SearchMode::Plain,
            invalid: None,
            found: Vec::new(),
            rows: Vec::new(),
            loaded: false,
            wait: Wait::default(),
            error: None,
            cursor: 0,
            window: Window::default(),
            page: 1,
            pages: 1,
            drawn: RefCell::default(),
            bar: None,
            full: Whole::Cut,
        }
    }

    /// The same window, which matches in `mode` (`ui.search_mode`).
    pub(crate) fn search_mode(mut self, mode: SearchMode) -> Self {
        self.mode = mode;
        self
    }

    /// Whether the window filters zoxide's directories itself, rather than zoxide.
    fn filters(&self) -> bool {
        self.mode != SearchMode::Plain
    }

    /// The keywords to ask zoxide for: those typed, separated by spaces; none, for every
    /// directory, if the window filters them itself.
    pub(crate) fn keywords(&self) -> Vec<String> {
        if self.filters() {
            return Vec::new();
        }
        self.keywords
            .text()
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    pub(crate) fn exclude(&self) -> Option<&Path> {
        self.exclude.as_deref()
    }

    /// Waits for the query `generation` instead.
    pub(crate) fn wait(&mut self, generation: u64) {
        self.generation = generation;
    }

    /// The wait for the first answer, whose dots turn in front of the title.
    pub(crate) fn first_wait(&self) -> Option<&Wait> {
        (!self.loaded).then_some(&self.wait)
    }

    /// Takes zoxide's answer to the query `generation`: directories best first, or why there
    /// are none. The cursor goes back to the best.
    pub(crate) fn found(&mut self, generation: u64, result: Result<Vec<Scored>, String>) {
        if generation != self.generation {
            return;
        }
        self.loaded = true;
        match result {
            Ok(found) => {
                self.found = found;
                self.error = None;
            }
            Err(reason) => {
                self.found.clear();
                self.error = Some(cells::sanitize(reason.as_bytes()));
            }
        }
        self.refilter();
    }

    /// Picks the rows again, with the cursor on the best: every directory zoxide found; if the
    /// window filters them itself, those whose paths match the keywords, as fzf ranks them,
    /// and those as good in zoxide's order.
    fn refilter(&mut self) {
        self.cursor = 0;
        self.window.reset();
        self.invalid = None;
        if !self.filters() {
            self.rows = (0..self.found.len()).collect();
            return;
        }
        let mut matcher = Matcher::paths(self.mode, self.keywords.text());
        self.invalid = matcher.error().map(ToString::to_string);
        let mut scored: Vec<(usize, u32)> = self
            .found
            .iter()
            .enumerate()
            .filter_map(|(index, found)| Some((index, matcher.score(&self.shown(&found.path))?)))
            .collect();
        scored.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
        self.rows = scored.into_iter().map(|(index, _)| index).collect();
    }

    /// The directory on `row`.
    fn row(&self, row: usize) -> Option<&Scored> {
        self.found.get(*self.rows.get(row)?)
    }

    /// The mode keys are typed in: Insert while the keywords take them.
    pub(crate) fn mode(&self) -> Mode {
        if self.typing {
            Mode::Insert
        } else {
            Mode::Normal
        }
    }

    /// Takes the count before Ctrl+f or Ctrl+b, which scroll that many pages at once, as vim's.
    pub(crate) fn count_pages(&mut self, pages: usize) {
        self.pages = pages.max(1);
    }

    /// Takes a key: Up, Down, the page keys, Home, and End move, Enter opens, and Esc closes
    /// the window. While the keywords take what is typed, as they do when the window opens,
    /// characters are keywords and the keys of text fields edit them, until Esc goes back to the
    /// rows; `/` types them again.
    pub(crate) fn handle(&mut self, input: Resolved) -> JumpEvent {
        let rows = self.rows.len();
        let last = rows.saturating_sub(1);
        let page = self.page.max(1);
        match input {
            Resolved::Insert(c) if self.typing => {
                self.keywords.insert(c);
                return self.changed();
            }
            Resolved::Insert(_) | Resolved::Hotkey(_) => {}
            Resolved::Action(action) => match action {
                Action::Up => self.cursor = self.cursor.saturating_sub(1),
                Action::Down => self.cursor = (self.cursor + 1).min(last),
                // As vim's Ctrl+b, Ctrl+f, Ctrl+u, and Ctrl+d, which scroll the rows.
                Action::PageUp | Action::PageDown => {
                    let down = action == Action::PageDown;
                    let pages = std::mem::replace(&mut self.pages, 1);
                    self.window.page(down, pages, &mut self.cursor, rows, page);
                }
                Action::HalfPageUp | Action::HalfPageDown => {
                    let down = action == Action::HalfPageDown;
                    self.window.half_page(down, &mut self.cursor, rows, page);
                }
                Action::Home => self.cursor = 0,
                Action::End => self.cursor = last,
                Action::QuickSearch => self.typing = true,
                Action::NormalMode => self.typing = false,
                Action::ShowFull => self.full = self.full.toggled(),
                Action::SearchMode => {
                    let before = self.filters();
                    self.mode = search_mode::next(self.mode);
                    // zoxide gives every directory, or what matches the keywords, anew.
                    if before != self.filters() {
                        return JumpEvent::Query;
                    }
                    self.refilter();
                }
                Action::Left
                | Action::Right
                | Action::TextStart
                | Action::TextEnd
                | Action::WordForward
                | Action::WordBackward
                | Action::Backspace
                | Action::DeleteChar
                | Action::DeleteToStart
                | Action::DeleteToEnd
                | Action::DeleteWord
                    if self.typing =>
                {
                    let before = self.keywords.text().to_owned();
                    self.keywords.edit(action);
                    if self.keywords.text() != before {
                        return self.changed();
                    }
                }
                Action::Confirm => {
                    if let Some(scored) = self.row(self.cursor) {
                        return JumpEvent::Open(scored.path.clone());
                    }
                }
                Action::Cancel => return JumpEvent::Closed,
                _ => {}
            },
        }
        JumpEvent::Pending
    }

    /// Takes a press of the mouse, where the window was drawn last: a click puts the cursor on
    /// a row, and returns the key that a double click on one, or a click outside the window,
    /// stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        let drawn = self.drawn.borrow().clone();
        drawn.menu_press(pointer, &mut self.cursor)
    }

    /// The keywords changed: the window filters zoxide's directories again, or asks zoxide.
    fn changed(&mut self) -> JumpEvent {
        if self.filters() {
            self.refilter();
            JumpEvent::Pending
        } else {
            JumpEvent::Query
        }
    }

    /// A directory as the window shows it: under the home directory, from `~`.
    fn shown(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~/{}", cells::sanitize(rest.as_os_str().as_bytes())),
            Err(_) => cells::sanitize(path.as_os_str().as_bytes()),
        }
    }

    /// Draws the window centered over `area`, the panels: the keywords, then the directories,
    /// scrolled to the cursor, and the whole directory under the cursor below them if `K` asks
    /// for it.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme, spin: &Spin) {
        let (page, cursor, window, bar) = self.draw(frame, area, theme, spin);
        (self.page, self.cursor, self.window, self.bar) = (page, cursor, window, bar);
    }

    /// The keywords, a text field in the colors of those of dialogs, with the glyph of the mode
    /// before them, at `area`, with the cursor while they take text.
    fn draw_keywords(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let mark = search_mode::marked(self.mode, self.invalid.is_some(), theme.dialog, theme);
        let prompt = Prompt::new(|text| fl!("jump-keywords", mode = mark.glyph, text = text));
        let field = (&prompt, &self.keywords);
        let column = mark.render_field(frame, area, field, theme.dialog, theme);
        if self.typing {
            frame.set_cursor_position(Position::new(
                area.x.saturating_add(column).min(area.right() - 1),
                area.y,
            ));
        }
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// Shows the rows from `top`, where the thumb of the scroll bar was dragged; the cursor
    /// moves only to stay on screen.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        let rows = self.rows.len();
        self.window
            .scroll_to(top, &mut self.cursor, rows, self.page.max(1));
    }

    fn draw(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        spin: &Spin,
    ) -> (usize, usize, Window, Option<Bar>) {
        let rows = u16::try_from(self.rows.len().max(1))
            .unwrap_or(u16::MAX)
            .min(MAX_ROWS);
        let colors = Colors::of(theme, false);
        // A line between the rows and the whole directory under the cursor.
        let below = if self.full == Whole::Shown && !self.rows.is_empty() {
            FULL_ROWS + 1
        } else {
            0
        };
        // Borders, the keywords, the line under them, the rows.
        let size = (WIDTH, rows.saturating_add(4).saturating_add(below));
        let title = fl!("jump-title");
        let inner = draw_box(frame, area, size, &title, colors, theme);
        if let Some(dots) = self.first_wait().and_then(|wait| spin.wait_dots(wait)) {
            draw_title_dots(frame, inner, &title, (dots, theme.busy_dialog), colors);
        }
        let mut drawn = self.drawn.borrow_mut();
        *drawn = Drawn {
            frame: frame_around(inner),
            ..Drawn::default()
        };
        if inner.height < 3 || inner.width < 4 {
            return (self.page, self.cursor, self.window, None);
        }
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);

        self.draw_keywords(frame, line(0), theme);
        draw_separator(frame, inner, inner.y + 1, colors, theme);

        let page = usize::from(inner.height.saturating_sub(2 + below)).max(1);
        if !self.loaded {
            return (page, self.cursor, self.window, None);
        }
        let message = if let Some(error) = &self.error {
            Some(error.clone())
        } else if let Some(reason) = &self.invalid {
            Some(fl!(
                "picker-not-regex",
                reason = cells::sanitize(reason.as_bytes())
            ))
        } else if self.rows.is_empty() {
            Some(fl!("jump-nothing"))
        } else {
            None
        };
        if let Some(message) = message {
            for (index, text) in (2..inner.height).zip(cells::wrap(&message, width)) {
                frame.render_widget(Line::styled(text, theme.dialog), line(index));
            }
            return (page, self.cursor, self.window, None);
        }
        let (mut window, mut cursor) = (self.window, self.cursor);
        window.follow(&mut cursor, self.rows.len(), page);
        let offset = window.top();
        let height = u16::try_from(page).unwrap_or(u16::MAX);
        let bar = right_frame(inner, inner.y + 2, height);
        let position = (self.rows.len(), page, offset);
        let bar = scrollbar::render(frame, bar, position, theme.dialog, theme.glyphs.scroll_bar);
        let found: Vec<&Scored> = self.rows.iter().map(|&index| &self.found[index]).collect();
        let formats = noc_locale::current();
        let scores: Vec<String> = found
            .iter()
            .map(|scored| formats.decimal(scored.score, 1, true))
            .collect();
        let score_width = scores
            .iter()
            .map(|score| cells::width(score))
            .max()
            .unwrap_or(0);
        // The directory, a space, the score.
        let path_width = width.saturating_sub(score_width + 1);
        for (index, (row, scored)) in found.iter().enumerate().skip(offset).take(page).enumerate() {
            let styles = if row == cursor {
                (colors.focused_style(), theme.truncated_cursor)
            } else {
                (theme.dialog, theme.truncated_dialog)
            };
            let fit = (Align::Left, Cut::Start);
            let mut text = cells::fitted(
                &self.shown(&scored.path),
                path_width,
                fit,
                styles,
                theme.glyphs.marks,
            );
            text.spans.push(Span::styled(" ", styles.0));
            let score = &scores[row];
            let score = cells::fit(score, score_width, Align::Right, styles, theme.glyphs.marks);
            text.spans.extend(score);
            let y = u16::try_from(index + 2).unwrap_or(u16::MAX);
            frame.render_widget(text, line(y));
            drawn.rows.push((row, line(y)));
        }
        if below > 0
            && let Some(scored) = found.get(cursor)
        {
            draw_whole(frame, inner, &self.shown(&scored.path), theme);
        }
        (page, cursor, window, bar)
    }
}

/// `path`, whole, in the last lines of `inner`, below a line across it that joins the frame.
fn draw_whole(frame: &mut Frame<'_>, inner: Rect, path: &str, theme: &Theme) {
    let width = usize::from(inner.width);
    let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
    let top = inner.height.saturating_sub(FULL_ROWS + 1);
    draw_separator(frame, inner, inner.y + top, Colors::of(theme, false), theme);
    let styles = (theme.dialog, theme.truncated_dialog);
    let size = (width, usize::from(FULL_ROWS));
    let rows = cells::whole(path, size, Some('/'), styles, theme.glyphs.marks);
    for (index, text) in (top + 1..inner.height).zip(rows) {
        frame.render_widget(text, line(index));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::glyphs::Glyphs;
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
        let mut menu =
            JumpMenu::new(PathBuf::from("/home/me"), None, 1).search_mode(SearchMode::Fzf);
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
        assert_eq!(menu.rows.len(), 4, "every directory again");
        menu.handle(Resolved::Insert('~'));
        assert_eq!(
            menu.rows.len(),
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
        let mut menu =
            JumpMenu::new(PathBuf::from("/home/me"), None, 1).search_mode(SearchMode::Regex);
        menu.found(1, Ok(found()));
        for c in "^~/[sD]".chars() {
            assert_eq!(menu.handle(Resolved::Insert(c)), JumpEvent::Pending);
        }
        assert_eq!(menu.keywords(), [] as [String; 0], "zoxide gets none");
        assert_eq!(menu.rows, [0, 3], "zoxide's order");
        menu.handle(Resolved::Insert('('));
        assert_eq!(menu.rows, [] as [usize; 0]);
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
        menu.wait(2);
        menu.found(2, Ok(found()));
        press(&mut menu, Action::DeleteToStart);
        assert_eq!(menu.rows.len(), 4);
        assert_eq!(
            press(&mut menu, Action::SearchMode),
            JumpEvent::Pending,
            "fzf to regex"
        );
        assert_eq!(menu.mode, SearchMode::Regex);
    }

    #[test]
    fn stale_answers_are_dropped_and_new_ones_start_at_the_best() {
        let mut menu = menu();
        press(&mut menu, Action::Down);
        menu.wait(2);
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
        menu.found(1, Ok(Vec::new()));
        assert!(draw(&mut menu, 10).contains("zoxide knows no directory"));
        menu.found(1, Err("zoxide is not installed".to_owned()));
        assert!(draw(&mut menu, 10).contains("zoxide is not installed"));
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
        // The frame is in column 0, a blank in column 1, the glyph of the mode and a blank 2
        // cells, and `Jump to: ` 9.
        let empty = cursor_after(&mut menu, "");
        assert_eq!(empty.x, 2 + 2 + 9);
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
    fn k_shows_the_whole_directory_under_the_cursor_under_the_rows() {
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1);
        let long = "/srv/www/clients/acme-corporation-international/deploy/releases/2026-10-15";
        menu.found(1, Ok(vec![scored(9.0, long), scored(1.0, "/srv")]));
        menu.handle(Resolved::Action(Action::ShowFull));
        insta::assert_snapshot!(draw(&mut menu, 12));
    }
}
