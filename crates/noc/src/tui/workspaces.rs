//! Workspaces: the tabs of both panels, saved under a name in `workspaces.toml` (ADR 0017),
//! and their window, Alt-W or F9 → Workspace → Workspace list…, which saves, restores,
//! renames, and deletes them. Typing filters the window; while the filter is empty, `1` … `9`
//! and `0` restore the first ten rows.

use std::path::Path;

use noc_config::{Place, SavedTab, SortBy, Workspace, Workspaces};
use noc_vfs::{Location, RemotePath};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;

use super::cells::{self, Align};
use super::dialog::{Colors, draw_box};
use super::fuzzy::Fuzzy;
use super::keymap::{Action, Resolved};
use super::panel::{Destination, Panel, SortKey};
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the window gets, in cells, borders included.
const WIDTH: u16 = 60;
/// Rows the window shows at most.
const MAX_ROWS: u16 = 16;

/// What a key did in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspacesEvent {
    Pending,
    Closed,
    /// Ask for a name to save the tabs of both panels under, as a new workspace.
    Save,
    /// Replace the tabs of both panels with those of this workspace.
    Restore(String),
    /// Ask for a new name for this workspace.
    Rename(String),
    /// Ask whether to delete this workspace.
    Delete(String),
}

/// A saved workspace, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) name: String,
    /// The tabs of both panels together.
    pub(crate) tabs: usize,
}

impl Row {
    /// The rows of `workspaces`, in their order.
    pub(crate) fn all(workspaces: &Workspaces) -> Vec<Self> {
        workspaces
            .workspaces
            .iter()
            .map(|workspace| Self {
                name: workspace.name.clone(),
                tabs: workspace.left.len() + workspace.right.len(),
            })
            .collect()
    }
}

/// The window, with the workspaces as `workspaces.toml` held them last.
#[derive(Debug)]
pub(crate) struct WorkspacesWindow {
    rows: Vec<Row>,
    filter: String,
    /// The filter matches as fzf does: `ui.fuzzy_search`.
    fuzzy: bool,
    /// How well each row, shown or not, matches the filter, higher for better; `None` hides
    /// it.
    matches: Vec<Option<u32>>,
    /// The row under the cursor, among those the filter shows.
    cursor: usize,
    /// The workspace the cursor goes to once the rows come again, such as one just saved.
    wanted: Option<String>,
    /// First row on screen, and rows on screen at the last render.
    offset: usize,
    page: usize,
}

impl WorkspacesWindow {
    /// A window on `rows`, whose filter matches as fzf does if `fuzzy` (`ui.fuzzy_search`).
    pub(crate) fn new(rows: Vec<Row>, fuzzy: bool) -> Self {
        let mut window = Self {
            rows,
            filter: String::new(),
            fuzzy,
            matches: Vec::new(),
            cursor: 0,
            wanted: None,
            offset: 0,
            page: 1,
        };
        window.refilter();
        window
    }

    /// Puts the cursor on the workspace `name` once the rows come again with it.
    pub(crate) fn focus(&mut self, name: &str) {
        self.wanted = Some(name.to_owned());
    }

    /// Takes the workspaces again, after a change; the cursor goes to the workspace it was
    /// asked to, else stays on its workspace if it is still there, else on its row.
    pub(crate) fn set_rows(&mut self, rows: Vec<Row>) {
        let chosen = self
            .wanted
            .take()
            .or_else(|| self.chosen().map(|row| row.name.clone()));
        self.rows = rows;
        self.refilter();
        let shown = self.shown();
        self.cursor = chosen
            .and_then(|name| shown.iter().position(|row| row.name == name))
            .unwrap_or(self.cursor)
            .min(shown.len().saturating_sub(1));
    }

    /// Matches the rows against the filter again: by name, ignoring case; `fuzzy`, as fzf
    /// matches a line.
    fn refilter(&mut self) {
        self.matches = if self.fuzzy {
            let mut fuzzy = Fuzzy::names(&self.filter);
            self.rows.iter().map(|row| fuzzy.score(&row.name)).collect()
        } else {
            let filter = self.filter.to_lowercase();
            self.rows
                .iter()
                .map(|row| row.name.to_lowercase().contains(&filter).then_some(0))
                .collect()
        };
    }

    /// The rows the filter shows, in their order.
    fn shown(&self) -> Vec<&Row> {
        self.rows
            .iter()
            .zip(&self.matches)
            .filter_map(|(row, score)| score.map(|_| row))
            .collect()
    }

    /// The row to put the cursor on after the filter changed: the first of those that match
    /// best.
    fn best_row(&self) -> usize {
        let shown: Vec<u32> = self.matches.iter().flatten().copied().collect();
        let best = shown.iter().max();
        shown
            .iter()
            .position(|score| Some(score) == best)
            .unwrap_or(0)
    }

    fn chosen(&self) -> Option<&Row> {
        self.shown().get(self.cursor).copied()
    }

    /// Whether a workspace is under the cursor, for F6 and F8.
    pub(crate) fn has_chosen(&self) -> bool {
        self.chosen().is_some()
    }

    /// Takes a key: arrows move, Enter restores, a digit restores its row while the filter is
    /// empty, other characters filter, Backspace takes one back, Insert saves the tabs as a new
    /// workspace, F6 renames, F8 deletes, and Esc closes the window.
    pub(crate) fn handle(&mut self, input: Resolved) -> WorkspacesEvent {
        let last = self.shown().len().saturating_sub(1);
        let page = self.page.max(1);
        let chosen = |window: &Self, event: fn(String) -> WorkspacesEvent| {
            window
                .chosen()
                .map_or(WorkspacesEvent::Pending, |row| event(row.name.clone()))
        };
        match input {
            Resolved::Insert(c) if self.filter.is_empty() && c.is_ascii_digit() => {
                let digit = c.to_digit(10).map_or(0, |digit| digit as usize);
                return match self.shown().get((digit + 9) % 10) {
                    Some(row) => WorkspacesEvent::Restore(row.name.clone()),
                    None => WorkspacesEvent::Pending,
                };
            }
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
                Action::Confirm => return chosen(self, WorkspacesEvent::Restore),
                Action::SaveWorkspace => return WorkspacesEvent::Save,
                Action::Move => return chosen(self, WorkspacesEvent::Rename),
                Action::Delete => return chosen(self, WorkspacesEvent::Delete),
                Action::Cancel => return WorkspacesEvent::Closed,
                _ => {}
            },
        }
        WorkspacesEvent::Pending
    }

    /// Draws the window centered over `area`, the panels: the filter, then the workspaces,
    /// scrolled to the cursor.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let (page, cursor, offset) = self.draw(frame, area, theme);
        (self.page, self.cursor, self.offset) = (page, cursor, offset);
    }

    fn draw(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) -> (usize, usize, usize) {
        let shown = self.shown();
        let rows = u16::try_from(shown.len().max(1))
            .unwrap_or(u16::MAX)
            .min(MAX_ROWS);
        let colors = Colors::of(theme, false);
        // Borders, the filter, the line under it, the rows.
        let size = (WIDTH, rows.saturating_add(4));
        let inner = draw_box(frame, area, size, &fl!("workspaces-title"), colors, theme);
        if inner.height < 3 || inner.width < 4 {
            return (self.page, self.cursor, self.offset);
        }
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);

        let filter = fl!(
            "workspaces-filter",
            text = cells::sanitize(self.filter.as_bytes())
        );
        // Right after the text, spaces typed last included.
        let column = u16::try_from(cells::width(&filter)).unwrap_or(u16::MAX);
        let filter = cells::fit(&filter, width, Align::Left);
        frame.render_widget(Line::styled(filter, theme.dialog), line(0));
        frame.set_cursor_position(Position::new(
            inner.x.saturating_add(column).min(inner.right() - 1),
            inner.y,
        ));
        frame.render_widget(Line::styled("─".repeat(width), theme.dialog), line(1));

        let page = usize::from(inner.height - 2);
        let message = if self.rows.is_empty() {
            Some(fl!("workspaces-none"))
        } else if shown.is_empty() {
            Some(fl!("workspaces-nothing"))
        } else {
            None
        };
        if let Some(message) = message {
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
        let counts: Vec<String> = shown
            .iter()
            .map(|row| fl!("workspaces-tabs", count = row.tabs))
            .collect();
        let count_width = counts
            .iter()
            .map(|count| cells::width(count))
            .max()
            .unwrap_or(0);
        // A hotkey, a space, the name, a space, the tabs.
        let name_width = width.saturating_sub(count_width + 3);
        let hotkeys = self.filter.is_empty();
        for (index, (row, shown)) in shown.iter().enumerate().skip(offset).take(page).enumerate() {
            let hotkey = match row {
                0..=9 if hotkeys => {
                    char::from_digit(u32::try_from((row + 1) % 10).unwrap_or(0), 10).unwrap_or(' ')
                }
                _ => ' ',
            };
            let name = cells::fit(
                &cells::sanitize(shown.name.as_bytes()),
                name_width,
                Align::Left,
            );
            let count = cells::fit(&counts[row], count_width, Align::Right);
            let style = if row == cursor {
                colors.focused_style()
            } else {
                theme.dialog
            };
            let text = Line::styled(format!("{hotkey} {name} {count}"), style);
            let y = u16::try_from(index + 2).unwrap_or(u16::MAX);
            frame.render_widget(text, line(y));
        }
        (page, cursor, offset)
    }
}

/// A tab as a workspace saves it: where `panel` is, or goes, its sort order, and what is
/// under its cursor; `current` if it is the tab that shows on its side. A location that
/// `workspaces.toml` cannot hold, such as a path that is not UTF-8, is saved as the nearest
/// directory above it that it can hold.
pub(crate) fn saved_tab(panel: &Panel, current: bool, home: &Path) -> SavedTab {
    let (sort, descending) = panel.sort_order();
    let (mut location, cursor) = panel.heading();
    let mut cursor = cursor.and_then(|name| String::from_utf8(name).ok());
    let place = loop {
        if let Some(place) = place_of(&location, home) {
            break place;
        }
        location = location.parent();
        cursor = None;
    };
    SavedTab {
        place,
        sort: match sort {
            SortKey::Name => SortBy::Name,
            SortKey::Extension => SortBy::Extension,
            SortKey::Time => SortBy::Time,
            SortKey::Size => SortBy::Size,
        },
        descending,
        cursor,
        current,
    }
}

/// How `workspaces.toml` writes `location`, if it can.
fn place_of(location: &Location, home: &Path) -> Option<Place> {
    match location {
        Location::Root => Some(Place::Root),
        Location::Sftp => Some(Place::Sftp),
        Location::Local(path) => Place::local(path, home),
        Location::Remote { host, path } => {
            let place = Place::Remote {
                host: host.clone(),
                path: std::str::from_utf8(path.as_bytes()).ok()?.to_owned(),
            };
            // A host with a `:` or a `/` would read back as something else.
            (Place::parse(&place.to_string()).as_ref() == Some(&place)).then_some(place)
        }
    }
}

/// A panel for `saved`, with its sort order, and the request for its listing, which puts the
/// cursor where it was.
pub(crate) fn restored_panel(
    saved: &SavedTab,
    home: &Path,
    show_hidden: bool,
) -> (Panel, super::panel::ListRequest) {
    let location = match &saved.place {
        Place::Root => Location::Root,
        Place::Sftp => Location::Sftp,
        // `workspaces.toml` holds only places that start with / or ~.
        place @ Place::Local(_) => place
            .local_dir(home)
            .map_or(Location::Root, Location::Local),
        Place::Remote { host, path } => Location::Remote {
            host: host.clone(),
            path: RemotePath::from(path.as_str()),
        },
    };
    let destination = match &saved.cursor {
        Some(name) => Destination::onto(location, name),
        None => Destination::to(location),
    };
    let (mut panel, request) = Panel::at(destination, home.to_path_buf(), show_hidden);
    let key = match saved.sort {
        SortBy::Name => SortKey::Name,
        SortBy::Extension => SortKey::Extension,
        SortBy::Time => SortKey::Time,
        SortBy::Size => SortKey::Size,
    };
    panel.set_sort_order(key, saved.descending);
    (panel, request)
}

/// The index of the tab that shows among the saved `tabs` of a side: the current one, else
/// the first.
pub(crate) fn current_of(tabs: &[SavedTab]) -> usize {
    tabs.iter().position(|tab| tab.current).unwrap_or(0)
}

/// The rows of the pull-down menu's Workspace menu: the names of `workspaces`, in order.
pub(crate) fn names(workspaces: &Workspaces) -> Vec<String> {
    workspaces
        .workspaces
        .iter()
        .map(|workspace: &Workspace| workspace.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn rows(names: &[&str]) -> Vec<Row> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| Row {
                name: (*name).to_owned(),
                tabs: index + 2,
            })
            .collect()
    }

    fn window() -> WorkspacesWindow {
        WorkspacesWindow::new(rows(&["noon", "deploy", "photos", "Notes"]), false)
    }

    fn press(window: &mut WorkspacesWindow, action: Action) -> WorkspacesEvent {
        window.handle(Resolved::Action(action))
    }

    fn typed(window: &mut WorkspacesWindow, text: &str) {
        for c in text.chars() {
            assert_eq!(window.handle(Resolved::Insert(c)), WorkspacesEvent::Pending);
        }
    }

    fn draw(window: &mut WorkspacesWindow, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(64, height)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn enter_digits_f6_and_f8_act_on_their_row() {
        let mut window = window();
        let restore = |name: &str| WorkspacesEvent::Restore(name.to_owned());
        assert_eq!(press(&mut window, Action::Confirm), restore("noon"));
        assert_eq!(window.handle(Resolved::Insert('3')), restore("photos"));
        assert_eq!(
            window.handle(Resolved::Insert('9')),
            WorkspacesEvent::Pending,
            "no ninth row"
        );
        press(&mut window, Action::Down);
        assert_eq!(
            press(&mut window, Action::Move),
            WorkspacesEvent::Rename("deploy".to_owned())
        );
        assert_eq!(
            press(&mut window, Action::Delete),
            WorkspacesEvent::Delete("deploy".to_owned())
        );
        assert_eq!(press(&mut window, Action::Cancel), WorkspacesEvent::Closed);
        let mut empty = WorkspacesWindow::new(Vec::new(), false);
        assert_eq!(press(&mut empty, Action::Confirm), WorkspacesEvent::Pending);
        assert_eq!(press(&mut empty, Action::Delete), WorkspacesEvent::Pending);
        assert_eq!(
            press(&mut empty, Action::SaveWorkspace),
            WorkspacesEvent::Save,
            "Insert saves with or without workspaces"
        );
    }

    #[test]
    fn typing_filters_by_name_and_digits_are_text_then() {
        let mut window = window();
        typed(&mut window, "o");
        let names: Vec<&str> = window.shown().iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["noon", "deploy", "photos", "Notes"]);
        typed(&mut window, "t");
        assert_eq!(
            press(&mut window, Action::Confirm),
            WorkspacesEvent::Restore("photos".to_owned())
        );
        typed(&mut window, "1");
        assert_eq!(window.shown().len(), 0);
        assert!(draw(&mut window, 10).contains("Nothing matches"));
        press(&mut window, Action::Backspace);
        press(&mut window, Action::Backspace);
        assert_eq!(
            press(&mut window, Action::Confirm),
            WorkspacesEvent::Restore("noon".to_owned()),
            "the first that matches best"
        );
    }

    #[test]
    fn a_fuzzy_filter_puts_the_cursor_on_the_best() {
        let mut window = WorkspacesWindow::new(rows(&["photos", "deploy"]), true);
        typed(&mut window, "dpl");
        assert_eq!(
            press(&mut window, Action::Confirm),
            WorkspacesEvent::Restore("deploy".to_owned())
        );
    }

    #[test]
    fn new_rows_keep_the_cursor_on_its_workspace() {
        let mut window = window();
        press(&mut window, Action::End);
        window.set_rows(rows(&["Notes", "noon"]));
        assert_eq!(
            press(&mut window, Action::Confirm),
            WorkspacesEvent::Restore("Notes".to_owned())
        );
        window.set_rows(rows(&["noon"]));
        assert_eq!(
            press(&mut window, Action::Confirm),
            WorkspacesEvent::Restore("noon".to_owned()),
            "gone: the row that is left"
        );
        // One just saved gets the cursor.
        window.focus("photos");
        window.set_rows(rows(&["noon", "deploy", "photos"]));
        assert_eq!(
            press(&mut window, Action::Confirm),
            WorkspacesEvent::Restore("photos".to_owned())
        );
    }

    #[test]
    fn says_how_to_save_one_when_there_are_none() {
        let mut window = WorkspacesWindow::new(Vec::new(), false);
        assert!(draw(&mut window, 10).contains("Insert"));
    }

    #[test]
    fn draws_names_with_hotkeys_and_their_tabs() {
        let mut window = window();
        insta::assert_snapshot!(draw(&mut window, 12));
    }

    #[test]
    fn locations_are_saved_as_workspaces_toml_can_hold_them() {
        let home = Path::new("/home/me");
        let remote = |host: &str, path: &[u8]| Location::Remote {
            host: host.to_owned(),
            path: RemotePath::new(path),
        };
        let local = |path: &str| Location::Local(PathBuf::from(path));
        assert_eq!(place_of(&Location::Root, home), Some(Place::Root));
        assert_eq!(place_of(&Location::Sftp, home), Some(Place::Sftp));
        assert_eq!(
            place_of(&local("/home/me/src"), home),
            Some(Place::Local("~/src".to_owned()))
        );
        assert_eq!(
            place_of(&remote("web", b"/srv"), home),
            Some(Place::Remote {
                host: "web".to_owned(),
                path: "/srv".to_owned()
            })
        );
        assert_eq!(place_of(&remote("web", b"/\xff"), home), None);
        assert_eq!(place_of(&remote("a:b", b"/srv"), home), None);
    }

    #[test]
    fn a_tab_comes_back_where_it_was_with_its_sort_order() {
        let home = Path::new("/home/me");
        let (mut panel, _) = Panel::new(
            Location::Local(PathBuf::from("/home/me/src")),
            home.to_path_buf(),
            false,
        );
        panel.set_sort_order(SortKey::Time, true);
        let saved = saved_tab(&panel, true, home);
        assert_eq!(saved.place, Place::Local("~/src".to_owned()));
        assert_eq!(
            (saved.sort, saved.descending, saved.current),
            (SortBy::Time, true, true)
        );
        let (restored, request) = restored_panel(&saved, home, false);
        assert_eq!(
            request.location,
            Location::Local(PathBuf::from("/home/me/src"))
        );
        assert_eq!(restored.sort_order(), (SortKey::Time, true));
        // A directory whose name is not UTF-8 is saved as the one above it.
        let odd = Location::Local(PathBuf::from(<std::ffi::OsStr as OsStrExt>::from_bytes(
            b"/home/me/\xff",
        )));
        let (panel, _) = Panel::new(odd, home.to_path_buf(), false);
        assert_eq!(
            saved_tab(&panel, false, home).place,
            Place::Local("~".to_owned())
        );
    }
}
