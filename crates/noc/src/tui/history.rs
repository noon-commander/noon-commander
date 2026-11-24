//! The window of the command history, `q :` in a panel or on the command line (ADR 0019,
//! ADR 0072): the commands of the panel's host, or of all hosts, newest first, and below them
//! where the command under the cursor ran: its host and when, then its directory. `/` types in
//! its filter; Enter puts the command on the command line, without running it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use jiff::tz::TimeZone;
use noc_config::HistoryEntry;
use noc_locale::Formats;
use noc_scroll::scrollbar::Bar;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::cells::{self, Align, Cut, Marks};
use super::keymap::{Action, Mode, Resolved};
use super::mouse::Pointer;
use super::panel::home_text;
use super::picker::{Body, MenuLine, Pick, Picker};
use super::search::{Input, Kind};
use super::theme::Theme;
use super::window::{self, Colors, inner_width};
use crate::i18n::fl;

/// Rows of commands the window shows at most.
const MAX_ROWS: u16 = 12;

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
    /// The home directory, which local directories show as `~`.
    home: PathBuf,
    /// Shows the commands of every host, not only those of `here`.
    all: bool,
    /// Filters the commands of the hosts shown.
    picker: Picker,
}

impl HistoryWindow {
    /// A window on `history`, oldest first as `history.toml` keeps it, for a panel on `here`;
    /// `home` is the home directory, and the filter matches as `input` says, as it was last.
    pub(crate) fn new(
        history: &[HistoryEntry],
        here: Option<String>,
        home: PathBuf,
        input: Input,
    ) -> Self {
        let mut window = Self {
            entries: history.iter().rev().cloned().collect(),
            here,
            home,
            all: false,
            picker: Picker::new(input),
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

    /// The kind of its filter while that takes the keys.
    pub(crate) fn search(&self) -> Option<Kind> {
        self.picker.search()
    }

    /// How its filter matches now, and whether case counts.
    pub(crate) fn input(&self) -> Input {
        self.picker.input()
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
                self.all.toggle();
                self.picker.set_rows(self.texts());
                self.picker.focus_best();
                HistoryEvent::Pending
            }
            Pick::Other(Action::Delete) => self
                .chosen()
                .map_or(HistoryEvent::Pending, |entry| event(entry, true)),
            Pick::Pending | Pick::Filtered | Pick::Other(_) => HistoryEvent::Pending,
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

    /// The name of `host` in the window: its alias; `local` for this machine.
    fn host_name(host: Option<&str>) -> String {
        host.map_or_else(|| fl!("history-local"), str::to_owned)
    }

    /// The lines under the rows for `entry`, in `width` cells: the host it ran on, then its
    /// directory, which loses its start where it is too long. A local one shows the home
    /// directory as `~`, and so does a host's.
    fn footer(&self, entry: &HistoryEntry, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let ran = theme.glyphs.ran;
        let (icon, dir) = match &entry.host {
            None => (ran.here, home_text(Path::new(&entry.dir), &self.home)),
            Some(_) if entry.dir.is_empty() => (ran.host, "~".to_owned()),
            Some(_) => (ran.host, cells::sanitize(entry.dir.as_bytes())),
        };
        let name = cells::sanitize(Self::host_name(entry.host.as_deref()).as_bytes());
        let styles = (theme.dialog, theme.truncated_dialog);
        let marks = theme.glyphs.marks;
        let line = |icon: char, text: &str, cut: Cut| {
            let lead = format!("{icon} ");
            let room = width.saturating_sub(cells::width(&lead));
            let mut spans = vec![Span::styled(lead, styles.0)];
            let text = cells::fit_cut(text, room, Align::Left, cut);
            spans.extend(cells::spans(text, styles, marks));
            Line::from(spans)
        };
        vec![
            line(icon, &name, Cut::Middle),
            line(ran.dir, &dir, Cut::Start),
        ]
    }

    /// Draws the window centered over `area`, the panels: the filter and which hosts it shows,
    /// the commands, scrolled to the cursor, and where the command under the cursor ran, with
    /// when it ran in `when` on the line over that, in the color of the title.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        when: (&TimeZone, &Formats),
    ) {
        // From the fields, not `shown`, so that the picker can draw while this borrows them.
        let (all, here) = (self.all, &self.here);
        let scoped = self
            .entries
            .iter()
            .filter(|entry| all || entry.host == *here);
        let shown = self.picker.shown(scoped);
        let lines: Vec<MenuLine> = (0..shown.len()).map(MenuLine::Row).collect();
        let cursor = self.picker.cursor().min(shown.len().saturating_sub(1));
        let colors = Colors::plain(theme);
        let chosen = shown.get(cursor);
        let footer = chosen.map_or_else(Vec::new, |entry| {
            self.footer(entry, inner_width(window::WIDTH, area), theme)
        });
        let footer_label = chosen
            .map(|entry| ran_at(entry.time, when))
            .filter(|time| !time.is_empty())
            .map(|time| Line::styled(time, colors.title));
        let scope = if self.all {
            fl!("history-all-hosts")
        } else {
            fl!(
                "history-this-host",
                host = Self::host_name(self.here.as_deref())
            )
        };
        let message = if self.entries.is_empty() {
            Some(fl!("history-none"))
        } else if shown.is_empty() {
            Some(fl!("history-nothing"))
        } else {
            None
        };
        let title = fl!("history-title");
        let body = Body {
            title: &title,
            width: window::WIDTH,
            lines: &lines,
            max_lines: Some(MAX_ROWS),
            note: Some(Line::raw(scope)),
            message,
            footer,
            footer_label,
            ..Body::default()
        };
        let focused = colors.focused_style();
        self.picker.render(frame, area, theme, body, &mut |row| {
            let entry = shown[row.index];
            let mut lines = entry.command.lines();
            let first = cells::sanitize(lines.next().unwrap_or_default().as_bytes());
            let more = lines.count();
            let more = if more > 0 {
                format!(" {}", fl!("history-more-lines", count = more))
            } else {
                String::new()
            };
            // A cell on either side of the command.
            let room = row.width.saturating_sub(2 + cells::width(&more));
            let styles = if row.selected {
                (focused, theme.truncated_cursor)
            } else if entry.host != *here {
                (
                    theme.dialog.add_modifier(Modifier::DIM),
                    theme.truncated_dialog.remove_modifier(Modifier::DIM),
                )
            } else {
                (theme.dialog, theme.truncated_dialog)
            };
            let marks = theme.glyphs.marks;
            let gap = || Span::styled(" ", styles.0);
            let mut spans = vec![gap()];
            spans.extend(command_spans(&first, &more, room, styles, marks));
            spans.push(gap());
            Line::from(spans).style(styles.0)
        });
    }
}

/// The `first` line of a command in `room` cells, its middle cut where it is longer, in
/// `style`; where `more` lines follow it, the mark of `marks` after it, then `more`. Marks are
/// in `mark` over `style`.
fn command_spans(
    first: &str,
    more: &str,
    room: usize,
    (style, mark): (Style, Style),
    marks: Marks,
) -> Vec<Span<'static>> {
    let styles = (style, mark);
    if more.is_empty() {
        return cells::fit(first, room, Align::Left, styles, marks);
    }
    // The mark says that the command goes on in lines after its first.
    let text_room = room.saturating_sub(2);
    let mut spans = if cells::width(first) > text_room {
        cells::fit(first, text_room, Align::Left, styles, marks)
    } else {
        vec![Span::styled(first.to_owned(), style)]
    };
    spans.push(Span::styled(" ", style));
    spans.push(Span::styled(marks.after, style.patch(mark)));
    let mut spans = cells::fit_spans(spans, room, styles, marks);
    spans.push(Span::styled(more.to_owned(), style));
    spans
}

/// When a command ran, `time` seconds after the Unix epoch, in the time zone and formats of
/// `when`, as every time shows; nothing where it is not known.
fn ran_at(time: i64, (tz, formats): (&TimeZone, &Formats)) -> String {
    let time = u64::try_from(time)
        .ok()
        .filter(|&seconds| seconds > 0)
        .and_then(|seconds| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds)));
    cells::mtime(time, tz, formats)
}

#[cfg(test)]
mod tests {
    use noc_config::SearchMode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::glyphs::Glyphs;

    /// 2026-11-10 09:05 UTC.
    const MORNING: i64 = 1_794_301_500;

    fn entry(command: &str, host: Option<&str>, dir: &str, time: i64) -> HistoryEntry {
        HistoryEntry {
            command: command.to_owned(),
            host: host.map(str::to_owned),
            dir: dir.to_owned(),
            time,
        }
    }

    /// Oldest first, as `history.toml` keeps them.
    fn history() -> Vec<HistoryEntry> {
        let minutes = |count: i64| MORNING + count * 60;
        vec![
            entry("make", None, "/home/me/src/noc", MORNING),
            entry(
                "tail -f log/production.log",
                Some("web"),
                "/var/www/app",
                minutes(282),
            ),
            entry(
                "for f in *.log; do\n  gzip \"$f\"\ndone",
                Some("web"),
                "/var/log",
                minutes(327),
            ),
            entry(
                "cargo test -p noc-ops",
                None,
                "/home/me/src/noc",
                minutes(365),
            ),
        ]
    }

    fn window(here: Option<&str>) -> HistoryWindow {
        HistoryWindow::new(
            &history(),
            here.map(str::to_owned),
            PathBuf::from("/home/me"),
            Input::new(Kind::FILTER, SearchMode::Fzf),
        )
    }

    /// The text of the lines under the rows for `entry`, in `width` cells.
    fn footer(window: &HistoryWindow, entry: &HistoryEntry, width: usize) -> Vec<String> {
        window
            .footer(entry, width, &Theme::terminal())
            .iter()
            .map(|line| cells::text_of(&line.spans))
            .collect()
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

    fn draw(window: &mut HistoryWindow, glyphs: &'static Glyphs) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
        let theme = Theme::terminal().with_glyphs(glyphs);
        let when = (&TimeZone::UTC, &Formats::iso());
        terminal
            .draw(|frame| window.render(frame, frame.area(), &theme, when))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn a_command_of_several_lines_ends_its_first_with_a_mark() {
        let marks = crate::tui::glyphs::Glyphs::PLAIN.marks;
        let (style, mark) = (Style::new(), Style::new().bold());
        let shown = |first, more, room| -> Vec<(String, Style)> {
            command_spans(first, more, room, (style, mark), marks)
                .into_iter()
                .filter(|span| !span.content.is_empty())
                .map(|span| (span.content.into_owned(), span.style))
                .collect()
        };
        let owned = |text: &str, style| (text.to_owned(), style);
        assert_eq!(
            shown("for f", " +2", 9),
            [
                owned("for f", style),
                owned(" ", style),
                owned("»", mark),
                owned("  ", style),
                owned(" +2", style)
            ]
        );
        assert_eq!(
            shown("for f in *.log", " +2", 9),
            [
                owned("for", style),
                owned("»", mark),
                owned("log", style),
                owned(" ", style),
                owned("»", mark),
                owned(" +2", style)
            ],
            "cut in its middle, the mark of the lines after it kept"
        );
        assert_eq!(shown("ls", "", 4), [owned("ls  ", style)]);
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
    fn the_footer_names_the_host_and_shows_the_directory_s_end() {
        let window = window(None);
        assert_eq!(
            footer(&window, &entry("ls", Some("db"), "", MORNING), 20),
            [
                format!("≡ db{}", " ".repeat(16)),
                format!("› ~{}", " ".repeat(17))
            ],
            "a host's home directory is ~, and its alias names a host without a label"
        );
        let deep = entry("ls", None, "/home/me/a/very/deep/tree/of/dirs", MORNING);
        assert_eq!(
            footer(&window, &deep, 23),
            [
                format!("⌂ local{}", " ".repeat(16)),
                "› «ry/deep/tree/of/dirs".to_owned()
            ],
            "the directory keeps its end"
        );
    }

    #[test]
    fn when_it_ran_is_on_the_line_over_the_footer_in_the_color_of_the_title() {
        let theme = Theme::mc_classic().with_glyphs(&Glyphs::PLAIN);
        let when = (&TimeZone::UTC, &Formats::iso());
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
        let mut window = window(None);
        terminal
            .draw(|frame| window.render(frame, frame.area(), &theme, when))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let time = "2026-11-10 15:10";
        let width = u16::try_from(time.len()).unwrap();
        let (x, y) = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width - width).map(move |x| (x, y)))
            .find(|&(x, y)| {
                let cells = (x..x + width).map(|x| buffer[(x, y)].symbol());
                cells.collect::<String>() == time
            })
            .unwrap();
        let line = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol());
        assert!(line.collect::<String>().contains('╟'), "on the line");
        for x in x..x + width {
            let cell = &buffer[(x, y)];
            assert_eq!(cell.fg, theme.dialog_title.fg.unwrap());
            assert!(!cell.modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn an_unknown_time_leaves_the_line_over_the_footer_bare() {
        let mut window = HistoryWindow::new(
            &[entry("ls", None, "/tmp", 0)],
            None,
            PathBuf::from("/home/me"),
            Input::new(Kind::FILTER, SearchMode::Fzf),
        );
        let drawn = draw(&mut window, &Glyphs::PLAIN);
        let lines: Vec<&str> = drawn.lines().filter(|line| line.contains('╟')).collect();
        assert_eq!(lines.len(), 2, "{drawn}");
        assert!(!lines[1].contains(|c: char| c.is_ascii_digit()), "{drawn}");
    }

    #[test]
    fn draws_the_commands_and_where_the_one_under_the_cursor_ran() {
        let mut window = window(Some("web"));
        press(&mut window, Action::NextField);
        press(&mut window, Action::Down);
        insta::assert_snapshot!(draw(&mut window, &Glyphs::PLAIN));
    }

    #[test]
    fn draws_this_machine_and_its_home_directory_with_icons() {
        let mut window = window(None);
        insta::assert_snapshot!(draw(&mut window, &Glyphs::NERD));
    }
}
