//! The window of the command history, Alt+h or Ctrl+r on the command line (ADR 0019): the
//! commands of the panel's host, or of all hosts, newest first, each with its host and
//! directory, and the whole command under the cursor below them. Typing filters it; Enter puts
//! the command on the command line, without running it.

use std::cell::RefCell;
use std::collections::HashMap;

use noc_config::HistoryEntry;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, draw_box, frame_around};
use super::fuzzy::Fuzzy;
use super::keymap::{Action, Resolved};
use super::mouse::{Drawn, Pointer};
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the window gets, in cells, borders included.
const WIDTH: u16 = 76;
/// Rows of commands the window shows at most.
const MAX_ROWS: u16 = 12;
/// Lines of the whole command under the cursor, below the rows, at most.
const PREVIEW_ROWS: usize = 4;
/// Widest the columns of hosts and directories get, in cells.
const HOST_WIDTH: usize = 12;
const DIR_WIDTH: usize = 18;

/// What a key did in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEvent {
    Pending,
    Closed,
    /// Put `command`, which ran on `host`, on the command line.
    Take {
        command: String,
        host: Option<String>,
    },
    /// Remove `command` on `host` from the history.
    Delete {
        command: String,
        host: Option<String>,
    },
}

/// The window, with the commands as `history.toml` held them last.
#[derive(Debug)]
pub(crate) struct HistoryWindow {
    /// Newest first.
    entries: Vec<HistoryEntry>,
    /// The host of the active panel; none for a local one.
    here: Option<String>,
    /// The labels of hosts that have one, by alias.
    labels: HashMap<String, String>,
    /// Shows the commands of every host, not only those of `here`.
    all: bool,
    filter: String,
    /// The filter matches as fzf does: `ui.fuzzy_search`.
    fuzzy: bool,
    /// How well each entry matches the filter, higher for better; `None` hides it.
    matches: Vec<Option<u32>>,
    /// The row under the cursor, among those shown.
    cursor: usize,
    /// First row on screen, and rows on screen at the last render.
    offset: usize,
    page: usize,
    /// Where the last render drew it, for the mouse.
    drawn: RefCell<Drawn>,
}

impl HistoryWindow {
    /// A window on `history`, oldest first as `history.toml` keeps it, for a panel on `here`;
    /// `labels` name hosts, and the filter matches as fzf does if `fuzzy`.
    pub(crate) fn new(
        history: &[HistoryEntry],
        here: Option<String>,
        labels: HashMap<String, String>,
        fuzzy: bool,
    ) -> Self {
        let mut window = Self {
            entries: history.iter().rev().cloned().collect(),
            here,
            labels,
            all: false,
            filter: String::new(),
            fuzzy,
            matches: Vec::new(),
            cursor: 0,
            offset: 0,
            page: 1,
            drawn: RefCell::default(),
        };
        window.refilter();
        window
    }

    /// Takes the commands again, after a change; the cursor stays on its row.
    pub(crate) fn set_history(&mut self, history: &[HistoryEntry]) {
        self.entries = history.iter().rev().cloned().collect();
        self.refilter();
        self.cursor = self.cursor.min(self.shown().len().saturating_sub(1));
    }

    /// Matches the entries of the hosts shown against the filter again: by command, ignoring
    /// case; `fuzzy`, as fzf matches a line.
    fn refilter(&mut self) {
        let mut fuzzy = Fuzzy::names(&self.filter);
        let filter = self.filter.to_lowercase();
        self.matches = self
            .entries
            .iter()
            .map(|entry| {
                if !self.all && entry.host != self.here {
                    None
                } else if self.fuzzy {
                    fuzzy.score(&entry.command)
                } else {
                    entry.command.to_lowercase().contains(&filter).then_some(0)
                }
            })
            .collect();
    }

    /// The entries shown, newest first.
    fn shown(&self) -> Vec<&HistoryEntry> {
        self.entries
            .iter()
            .zip(&self.matches)
            .filter_map(|(entry, score)| score.map(|_| entry))
            .collect()
    }

    /// The row to put the cursor on after the filter changed: the newest of those that match
    /// best.
    fn best_row(&self) -> usize {
        let shown: Vec<u32> = self.matches.iter().flatten().copied().collect();
        let best = shown.iter().max();
        shown
            .iter()
            .position(|score| Some(score) == best)
            .unwrap_or(0)
    }

    fn chosen(&self) -> Option<&HistoryEntry> {
        self.shown().get(self.cursor).copied()
    }

    /// Takes a key: arrows move, characters filter, Backspace takes one back, Tab switches
    /// between this host and all hosts, Enter takes the command, Delete removes it, and Esc
    /// closes the window.
    pub(crate) fn handle(&mut self, input: Resolved) -> HistoryEvent {
        let last = self.shown().len().saturating_sub(1);
        let page = self.page.max(1);
        match input {
            Resolved::Insert(c) => {
                self.filter.push(c);
                self.refilter();
                self.cursor = self.best_row();
            }
            Resolved::Action(action) => match action {
                Action::Up => self.cursor = self.cursor.saturating_sub(1),
                Action::Down => self.cursor = (self.cursor + 1).min(last),
                Action::PageUp => self.cursor = self.cursor.saturating_sub(page),
                Action::PageDown => self.cursor = (self.cursor + page).min(last),
                Action::Home => self.cursor = 0,
                Action::End => self.cursor = last,
                Action::Backspace => {
                    self.filter.pop();
                    self.refilter();
                    self.cursor = self.best_row();
                }
                Action::NextField => {
                    self.all = !self.all;
                    self.refilter();
                    self.cursor = self.best_row();
                }
                Action::Confirm => {
                    if let Some(entry) = self.chosen() {
                        return HistoryEvent::Take {
                            command: entry.command.clone(),
                            host: entry.host.clone(),
                        };
                    }
                }
                Action::Delete => {
                    if let Some(entry) = self.chosen() {
                        return HistoryEvent::Delete {
                            command: entry.command.clone(),
                            host: entry.host.clone(),
                        };
                    }
                }
                Action::Cancel => return HistoryEvent::Closed,
                _ => {}
            },
        }
        HistoryEvent::Pending
    }

    /// Takes a press of the mouse, where the window was drawn last: a click puts the cursor on
    /// a row, and returns the key that a double click on one, or a click outside the window,
    /// stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        let drawn = self.drawn.borrow().clone();
        drawn.menu_press(pointer, &mut self.cursor)
    }

    /// The name of `host` in the window: its label, else its alias; `local` for this machine.
    fn host_name(&self, host: Option<&str>) -> String {
        host.map_or_else(
            || fl!("history-local"),
            |host| {
                self.labels
                    .get(host)
                    .map_or(host, String::as_str)
                    .to_owned()
            },
        )
    }

    /// Draws the window centered over `area`, the panels: the filter and which hosts it shows,
    /// the commands, scrolled to the cursor, and the whole command under the cursor.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let (page, cursor, offset) = self.draw(frame, area, theme);
        (self.page, self.cursor, self.offset) = (page, cursor, offset);
    }

    /// The filter, with the cursor after it, and which hosts the window shows, on the first
    /// line of `inner`; a line under them.
    fn draw_filter(&self, frame: &mut Frame<'_>, inner: Rect, theme: &Theme) {
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let scope = if self.all {
            fl!("history-all-hosts")
        } else {
            fl!(
                "history-this-host",
                host = self.host_name(self.here.as_deref())
            )
        };
        let scope = cells::sanitize(scope.as_bytes());
        let filter = fl!(
            "history-filter",
            text = cells::sanitize(self.filter.as_bytes())
        );
        let column = u16::try_from(cells::width(&filter)).unwrap_or(u16::MAX);
        let room = width.saturating_sub(cells::width(&scope) + 1);
        let top = format!("{} {scope}", cells::fit(&filter, room, Align::Left));
        frame.render_widget(Line::styled(top, theme.dialog), line(0));
        frame.set_cursor_position(Position::new(
            inner.x.saturating_add(column).min(inner.right() - 1),
            inner.y,
        ));
        frame.render_widget(Line::styled("─".repeat(width), theme.dialog), line(1));
    }

    fn draw(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) -> (usize, usize, usize) {
        let shown = self.shown();
        let rows = u16::try_from(shown.len().max(1))
            .unwrap_or(u16::MAX)
            .min(MAX_ROWS);
        let preview = self.chosen().map_or_else(Vec::new, |entry| {
            let width = usize::from(WIDTH.min(area.width).saturating_sub(4));
            preview_rows(&entry.command, width)
        });
        let colors = Colors::of(theme, false);
        // Borders, the filter, the line under it, the rows, a line, the preview.
        let preview_rows = u16::try_from(preview.len()).unwrap_or(0);
        let size = (WIDTH, rows + 5 + preview_rows);
        let inner = draw_box(frame, area, size, &fl!("history-title"), colors, theme);
        let mut drawn = self.drawn.borrow_mut();
        *drawn = Drawn {
            frame: frame_around(inner),
            ..Drawn::default()
        };
        if inner.height < 4 || inner.width < 8 {
            return (self.page, self.cursor, self.offset);
        }
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);

        self.draw_filter(frame, inner, theme);

        let below = 1 + preview_rows;
        let page = usize::from(inner.height.saturating_sub(2 + below)).max(1);
        if shown.is_empty() {
            let message = if self.entries.is_empty() {
                fl!("history-none")
            } else {
                fl!("history-nothing")
            };
            for (index, text) in (2..inner.height).zip(cells::wrap(&message, width)) {
                frame.render_widget(Line::styled(text, theme.dialog), line(index));
            }
            return (page, self.cursor, self.offset);
        }
        let cursor = self.cursor.min(shown.len() - 1);
        let offset = self
            .offset
            .min(cursor)
            .max((cursor + 1).saturating_sub(page));
        let hosts: Vec<String> = shown
            .iter()
            .map(|entry| cells::sanitize(self.host_name(entry.host.as_deref()).as_bytes()))
            .collect();
        let host_width = hosts
            .iter()
            .map(|host| cells::width(host))
            .max()
            .unwrap_or(0)
            .min(HOST_WIDTH);
        let dir_width = DIR_WIDTH.min(width / 4);
        for (index, (row, entry)) in shown.iter().enumerate().skip(offset).take(page).enumerate() {
            let mut lines = entry.command.lines();
            let first = cells::sanitize(lines.next().unwrap_or_default().as_bytes());
            let more = lines.count();
            let more = if more > 0 {
                format!(" +{more}")
            } else {
                String::new()
            };
            let room = width.saturating_sub(host_width + dir_width + 4 + cells::width(&more));
            let first = if more.is_empty() {
                cells::fit(&first, room, Align::Left)
            } else {
                cells::fit(&format!("{first} …"), room, Align::Left)
            };
            let host = cells::fit(&hosts[row], host_width, Align::Left);
            let dir = cells::fit(
                &cells::sanitize(entry.dir.as_bytes()),
                dir_width,
                Align::Left,
            );
            let style = if row == cursor {
                colors.focused_style()
            } else if entry.host != self.here {
                theme.dialog.add_modifier(Modifier::DIM)
            } else {
                theme.dialog
            };
            let text =
                Line::from(vec![Span::raw(format!(" {host} {dir} {first}{more} "))]).style(style);
            let y = u16::try_from(index + 2).unwrap_or(u16::MAX);
            frame.render_widget(text, line(y));
            drawn.rows.push((row, line(y)));
        }
        let top = inner.height.saturating_sub(below);
        frame.render_widget(Line::styled("─".repeat(width), theme.dialog), line(top));
        for (index, text) in (top + 1..inner.height).zip(preview) {
            frame.render_widget(Line::styled(text, theme.dialog), line(index));
        }
        (page, cursor, offset)
    }
}

/// The first [`PREVIEW_ROWS`] rows of `command` in `width` cells: its lines, cut where they
/// are too long and going on in the next row, their indentation kept.
fn preview_rows(command: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in command.lines() {
        let mut rest = cells::sanitize(line.as_bytes());
        loop {
            if cells::width(&rest) <= width {
                rows.push(rest);
                break;
            }
            let (head, tail) = cells::split(&rest, width);
            rows.push(head);
            rest = tail;
        }
        if rows.len() >= PREVIEW_ROWS {
            break;
        }
    }
    rows.truncate(PREVIEW_ROWS);
    rows
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn entry(command: &str, host: Option<&str>, dir: &str) -> HistoryEntry {
        HistoryEntry {
            command: command.to_owned(),
            host: host.map(str::to_owned),
            dir: dir.to_owned(),
            time: 0,
        }
    }

    /// Oldest first, as `history.toml` keeps them.
    fn history() -> Vec<HistoryEntry> {
        vec![
            entry("make", None, "/home/me/src/noc"),
            entry("tail -f log/production.log", Some("web"), "/var/www/app"),
            entry(
                "for f in *.log; do\n  gzip \"$f\"\ndone",
                Some("web"),
                "/var/log",
            ),
            entry("cargo test -p noc-ops", None, "/home/me/src/noc"),
        ]
    }

    fn window(here: Option<&str>) -> HistoryWindow {
        let labels = HashMap::from([("web".to_owned(), "Site".to_owned())]);
        HistoryWindow::new(&history(), here.map(str::to_owned), labels, true)
    }

    fn press(window: &mut HistoryWindow, action: Action) -> HistoryEvent {
        window.handle(Resolved::Action(action))
    }

    fn commands(window: &HistoryWindow) -> Vec<&str> {
        window
            .shown()
            .iter()
            .map(|entry| entry.command.as_str())
            .collect()
    }

    fn draw(window: &mut HistoryWindow) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn shows_this_host_newest_first_and_tab_shows_all() {
        let mut window = window(Some("web"));
        assert_eq!(
            commands(&window),
            [
                "for f in *.log; do\n  gzip \"$f\"\ndone",
                "tail -f log/production.log"
            ]
        );
        press(&mut window, Action::NextField);
        assert_eq!(commands(&window).len(), 4);
        assert_eq!(commands(&window)[0], "cargo test -p noc-ops");
        press(&mut window, Action::NextField);
        assert_eq!(commands(&window).len(), 2);
    }

    #[test]
    fn typing_filters_and_enter_takes_the_command_with_its_host() {
        let mut window = window(None);
        press(&mut window, Action::NextField);
        for c in "tail".chars() {
            window.handle(Resolved::Insert(c));
        }
        assert_eq!(
            press(&mut window, Action::Confirm),
            HistoryEvent::Take {
                command: "tail -f log/production.log".to_owned(),
                host: Some("web".to_owned()),
            }
        );
        assert_eq!(
            press(&mut window, Action::Delete),
            HistoryEvent::Delete {
                command: "tail -f log/production.log".to_owned(),
                host: Some("web".to_owned()),
            }
        );
        window.set_history(&history()[..1]);
        assert_eq!(press(&mut window, Action::Confirm), HistoryEvent::Pending);
        assert_eq!(press(&mut window, Action::Cancel), HistoryEvent::Closed);
    }

    #[test]
    fn the_preview_keeps_indentation_and_cuts_long_lines() {
        assert_eq!(
            preview_rows("for f in *; do\n  gzip \"$f\"\ndone", 20),
            ["for f in *; do", "  gzip \"$f\"", "done"]
        );
        assert_eq!(preview_rows("abcdefgh", 3), ["abc", "def", "gh"]);
        assert_eq!(preview_rows("1\n2\n3\n4\n5", 3).len(), PREVIEW_ROWS);
    }

    #[test]
    fn draws_hosts_directories_and_the_whole_command_under_the_cursor() {
        let mut window = window(Some("web"));
        press(&mut window, Action::NextField);
        press(&mut window, Action::Down);
        insta::assert_snapshot!(draw(&mut window));
    }
}
