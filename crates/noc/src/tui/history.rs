//! The window of the command history, `q :` in a panel or on the command line (ADR 0019): the
//! commands of the panel's host, or of all hosts, newest first, each with its host and
//! directory, and the whole command under the cursor below them. `/` types in its filter; Enter
//! puts the command on the command line, without running it.

use std::collections::HashMap;

use noc_config::HistoryEntry;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::Colors;
use super::keymap::{Action, Mode, Resolved};
use super::mouse::Pointer;
use super::picker::{Body, MenuLine, Pick, Picker};
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
    /// Filters the commands of the hosts shown.
    picker: Picker,
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
            picker: Picker::new(fuzzy),
        };
        window.picker.set_rows(window.texts());
        window
    }

    /// Takes the commands again, after a change; the cursor stays on its row.
    pub(crate) fn set_history(&mut self, history: &[HistoryEntry]) {
        self.entries = history.iter().rev().cloned().collect();
        self.picker.set_rows(self.texts());
        let last = self.picker.len().saturating_sub(1);
        self.picker.set_cursor(self.picker.cursor().min(last));
    }

    /// The entries of the hosts shown, newest first, whether the filter shows them or not.
    fn scoped(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries
            .iter()
            .filter(|entry| self.all || entry.host == self.here)
    }

    /// What the filter finds each entry of the hosts shown by: its command.
    fn texts(&self) -> Vec<Vec<String>> {
        self.scoped()
            .map(|entry| vec![entry.command.clone()])
            .collect()
    }

    /// The entries shown, newest first.
    fn shown(&self) -> Vec<&HistoryEntry> {
        self.picker.shown(self.scoped())
    }

    fn chosen(&self) -> Option<&HistoryEntry> {
        self.shown().get(self.picker.cursor()).copied()
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    /// The rows, for the count before a page key.
    pub(crate) fn picker_mut(&mut self) -> &mut Picker {
        &mut self.picker
    }

    pub(crate) fn mode(&self) -> Mode {
        self.picker.mode()
    }

    /// Takes a key: the picker's, where Enter takes the command, and Tab, which switches
    /// between this host and all hosts, and d d and F8, which remove the command.
    pub(crate) fn handle(&mut self, input: Resolved) -> HistoryEvent {
        let event = |entry: &HistoryEntry, delete: bool| {
            let (command, host) = (entry.command.clone(), entry.host.clone());
            if delete {
                HistoryEvent::Delete { command, host }
            } else {
                HistoryEvent::Take { command, host }
            }
        };
        match self.picker.handle(input) {
            Pick::Chosen(row) => self
                .shown()
                .get(row)
                .map_or(HistoryEvent::Pending, |entry| event(entry, false)),
            Pick::Closed => HistoryEvent::Closed,
            Pick::Other(Action::NextField) => {
                self.all = !self.all;
                self.picker.set_rows(self.texts());
                self.picker.focus_best();
                HistoryEvent::Pending
            }
            Pick::Other(Action::Delete) => self
                .chosen()
                .map_or(HistoryEvent::Pending, |entry| event(entry, true)),
            Pick::Pending | Pick::Other(_) => HistoryEvent::Pending,
        }
    }

    /// Takes a press of the mouse, where the window was drawn last: a click puts the cursor on
    /// a row, and returns the key that a double click on one, or a click outside the window,
    /// stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        self.picker.pointer(pointer)
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
        // From the fields, not `shown`, so that the picker can draw while this borrows them.
        let (all, here) = (self.all, &self.here);
        let scoped = self
            .entries
            .iter()
            .filter(|entry| all || entry.host == *here);
        let shown = self.picker.shown(scoped);
        let lines: Vec<MenuLine> = (0..shown.len()).map(MenuLine::Row).collect();
        let cursor = self.picker.cursor().min(shown.len().saturating_sub(1));
        let footer = shown.get(cursor).map_or_else(Vec::new, |entry| {
            let width = usize::from(WIDTH.min(area.width).saturating_sub(4));
            preview_rows(&entry.command, width)
        });
        let scope = if self.all {
            fl!("history-all-hosts")
        } else {
            fl!(
                "history-this-host",
                host = self.host_name(self.here.as_deref())
            )
        };
        let message = if self.entries.is_empty() {
            Some(fl!("history-none"))
        } else if shown.is_empty() {
            Some(fl!("history-nothing"))
        } else {
            None
        };
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
        let title = fl!("history-title");
        let body = Body {
            title: &title,
            width: WIDTH,
            lines: &lines,
            max_lines: Some(MAX_ROWS),
            note: Some(scope),
            message,
            footer,
        };
        let focused = Colors::of(theme, false).focused_style();
        self.picker.render(frame, area, theme, body, &mut |row| {
            let entry = shown[row.index];
            let dir_width = DIR_WIDTH.min(row.width / 4);
            let mut lines = entry.command.lines();
            let first = cells::sanitize(lines.next().unwrap_or_default().as_bytes());
            let more = lines.count();
            let more = if more > 0 {
                format!(" +{more}")
            } else {
                String::new()
            };
            let room = row
                .width
                .saturating_sub(host_width + dir_width + 4 + cells::width(&more));
            let first = if more.is_empty() {
                cells::fit(&first, room, Align::Left)
            } else {
                cells::fit(&format!("{first} …"), room, Align::Left)
            };
            let host = cells::fit(&hosts[row.index], host_width, Align::Left);
            let dir = cells::fit(
                &cells::sanitize(entry.dir.as_bytes()),
                dir_width,
                Align::Left,
            );
            let style = if row.selected {
                focused
            } else if entry.host != *here {
                theme.dialog.add_modifier(Modifier::DIM)
            } else {
                theme.dialog
            };
            Line::from(vec![Span::raw(format!(" {host} {dir} {first}{more} "))]).style(style)
        });
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
        press(&mut window, Action::QuickSearch);
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
