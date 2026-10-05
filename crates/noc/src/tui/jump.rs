//! The zoxide window of Alt-Z: the directories zoxide ranks highest for the keywords typed,
//! best first, as `z foo bar` picks them in a shell; or, with `ui.fuzzy_search`, all of them,
//! filtered and ranked by what is typed as fzf does. Enter opens one in the active panel;
//! while nothing is typed, `1` … `9` and `0` open the first ten rows.

use std::cell::RefCell;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use noc_tools::zoxide::Scored;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;

use super::cells::{self, Align};
use super::dialog::{Colors, draw_box, frame_around};
use super::fuzzy::Fuzzy;
use super::keymap::{Action, Resolved};
use super::mouse::{Drawn, Pointer};
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
    keywords: String,
    /// The window asks zoxide once for every directory and filters them itself, as fzf does
    /// (`ui.fuzzy_search`), instead of asking zoxide for each change of the keywords.
    fuzzy: bool,
    /// zoxide's answer, best first.
    found: Vec<Scored>,
    /// The rows: indices into `found` of the directories that match, best first.
    rows: Vec<usize>,
    /// `true` once the first answer arrived.
    loaded: bool,
    error: Option<String>,
    /// The row under the cursor.
    cursor: usize,
    /// First row on screen, and rows on screen at the last render.
    offset: usize,
    page: usize,
    /// Where the last render drew it, for the mouse.
    drawn: RefCell<Drawn>,
}

impl JumpMenu {
    /// A window for a panel on `exclude`, if it is local, waiting for the query `generation`.
    pub(crate) fn new(home: PathBuf, exclude: Option<PathBuf>, generation: u64) -> Self {
        Self {
            home,
            exclude,
            generation,
            keywords: String::new(),
            fuzzy: false,
            found: Vec::new(),
            rows: Vec::new(),
            loaded: false,
            error: None,
            cursor: 0,
            offset: 0,
            page: 1,
            drawn: RefCell::default(),
        }
    }

    /// The same window, which filters zoxide's directories as fzf does if `fuzzy`
    /// (`ui.fuzzy_search`).
    pub(crate) fn fuzzy(mut self, fuzzy: bool) -> Self {
        self.fuzzy = fuzzy;
        self
    }

    /// The keywords to ask zoxide for: those typed, separated by spaces; none, for every
    /// directory, if the window filters them itself.
    pub(crate) fn keywords(&self) -> Vec<String> {
        if self.fuzzy {
            return Vec::new();
        }
        self.keywords
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
    /// window filters them itself, those that match the keywords as fzf matches a path, best
    /// first, and those as good in zoxide's order.
    fn refilter(&mut self) {
        self.cursor = 0;
        self.offset = 0;
        if !self.fuzzy {
            self.rows = (0..self.found.len()).collect();
            return;
        }
        let mut fuzzy = Fuzzy::paths(&self.keywords);
        let mut scored: Vec<(usize, u32)> = self
            .found
            .iter()
            .enumerate()
            .filter_map(|(index, found)| Some((index, fuzzy.score(&self.shown(&found.path))?)))
            .collect();
        scored.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
        self.rows = scored.into_iter().map(|(index, _)| index).collect();
    }

    /// The directory on `row`.
    fn row(&self, row: usize) -> Option<&Scored> {
        self.found.get(*self.rows.get(row)?)
    }

    /// Takes a key: arrows move, Enter opens, a digit opens its row while nothing is typed,
    /// other characters are keywords, Backspace takes one back, and Esc closes the window.
    pub(crate) fn handle(&mut self, input: Resolved) -> JumpEvent {
        let last = self.rows.len().saturating_sub(1);
        let page = self.page.max(1);
        match input {
            Resolved::Insert(c) if self.keywords.is_empty() && c.is_ascii_digit() => {
                let digit = c.to_digit(10).map_or(0, |digit| digit as usize);
                return match self.row((digit + 9) % 10) {
                    Some(scored) => JumpEvent::Open(scored.path.clone()),
                    None => JumpEvent::Pending,
                };
            }
            Resolved::Insert(c) => {
                self.keywords.push(c);
                return self.changed();
            }
            Resolved::Action(action) => match action {
                Action::Up => self.cursor = self.cursor.saturating_sub(1),
                Action::Down => self.cursor = (self.cursor + 1).min(last),
                Action::PageUp => self.cursor = self.cursor.saturating_sub(page),
                Action::PageDown => self.cursor = (self.cursor + page).min(last),
                Action::Home => self.cursor = 0,
                Action::End => self.cursor = last,
                Action::Backspace => {
                    if self.keywords.pop().is_some() {
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
        if self.fuzzy {
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
    /// scrolled to the cursor.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let (page, cursor, offset) = self.draw(frame, area, theme);
        (self.page, self.cursor, self.offset) = (page, cursor, offset);
    }

    fn draw(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) -> (usize, usize, usize) {
        let rows = u16::try_from(self.rows.len().max(1))
            .unwrap_or(u16::MAX)
            .min(MAX_ROWS);
        let colors = Colors::of(theme, false);
        // Borders, the keywords, the line under them, the rows.
        let size = (WIDTH, rows.saturating_add(4));
        let inner = draw_box(frame, area, size, &fl!("jump-title"), colors, theme);
        let mut drawn = self.drawn.borrow_mut();
        *drawn = Drawn {
            frame: frame_around(inner),
            ..Drawn::default()
        };
        if inner.height < 3 || inner.width < 4 {
            return (self.page, self.cursor, self.offset);
        }
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);

        let prompt = fl!(
            "jump-keywords",
            text = cells::sanitize(self.keywords.as_bytes())
        );
        // Right after the text, spaces typed last included.
        let column = u16::try_from(cells::width(&prompt)).unwrap_or(u16::MAX);
        let prompt = cells::fit(&prompt, width, Align::Left);
        frame.render_widget(Line::styled(prompt, theme.dialog), line(0));
        frame.set_cursor_position(Position::new(
            inner.x.saturating_add(column).min(inner.right() - 1),
            inner.y,
        ));
        frame.render_widget(Line::styled("─".repeat(width), theme.dialog), line(1));

        let page = usize::from(inner.height - 2);
        let message = if !self.loaded {
            Some(fl!("panel-loading"))
        } else if let Some(error) = &self.error {
            Some(error.clone())
        } else if self.rows.is_empty() {
            Some(fl!("jump-nothing"))
        } else {
            None
        };
        if let Some(message) = message {
            for (index, text) in (2..inner.height).zip(cells::wrap(&message, width)) {
                frame.render_widget(Line::styled(text, theme.dialog), line(index));
            }
            return (page, self.cursor, self.offset);
        }
        let cursor = self.cursor.min(self.rows.len() - 1);
        let offset = self
            .offset
            .min(cursor)
            .max((cursor + 1).saturating_sub(page));
        let found: Vec<&Scored> = self.rows.iter().map(|&index| &self.found[index]).collect();
        let scores: Vec<String> = found
            .iter()
            .map(|scored| format!("{:.1}", scored.score))
            .collect();
        let score_width = scores.iter().map(String::len).max().unwrap_or(0);
        // A hotkey, a space, the directory, a space, the score.
        let path_width = width.saturating_sub(score_width + 3);
        let hotkeys = self.keywords.is_empty();
        for (index, (row, scored)) in found.iter().enumerate().skip(offset).take(page).enumerate() {
            let hotkey = match row {
                0..=9 if hotkeys => {
                    char::from_digit(u32::try_from((row + 1) % 10).unwrap_or(0), 10).unwrap_or(' ')
                }
                _ => ' ',
            };
            let path = cells::fit(&self.shown(&scored.path), path_width, Align::Left);
            let score = cells::fit(&scores[row], score_width, Align::Right);
            let style = if row == cursor {
                colors.focused_style()
            } else {
                theme.dialog
            };
            let text = Line::styled(format!("{hotkey} {path} {score}"), style);
            let y = u16::try_from(index + 2).unwrap_or(u16::MAX);
            frame.render_widget(text, line(y));
            drawn.rows.push((row, line(y)));
        }
        (page, cursor, offset)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

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
        terminal
            .draw(|frame| menu.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn typing_asks_again_and_digits_open_rows_while_nothing_is_typed() {
        let mut menu = menu();
        assert_eq!(
            menu.handle(Resolved::Insert('2')),
            JumpEvent::Open(PathBuf::from("/srv/www"))
        );
        assert_eq!(menu.handle(Resolved::Insert('9')), JumpEvent::Pending);
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
        let mut menu = JumpMenu::new(PathBuf::from("/home/me"), None, 1).fuzzy(true);
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
        assert!(draw(&mut menu, 10).contains("Loading…"));
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
            terminal
                .draw(|frame| menu.render(frame, frame.area(), &Theme::terminal()))
                .unwrap();
            terminal.get_cursor_position().unwrap()
        };
        // The frame is in column 0, a blank in column 1, and `Jump to: ` is 9 cells.
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
}
