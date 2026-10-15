//! Workspaces: the tabs of both panels, saved under a name in `workspaces.toml` (ADR 0017),
//! and their window, t w or F9 → Workspace → Workspace list…, which saves, restores,
//! renames, and deletes them. Typing filters the window; while the filter is empty, `1` … `9`
//! and `0` restore the first ten rows.

use std::path::Path;

use noc_config::{Place, SavedTab, SortBy, Workspace, Workspaces};
use noc_scroll::scrollbar::Bar;
use noc_vfs::{Location, RemotePath};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;

use super::cells::{self, Align, Cut};
use super::dialog::Colors;
use super::keymap::{Action, Mode, Resolved};
use super::mouse::Pointer;
use super::panel::{Destination, Panel, SortKey};
use super::picker::{Body, MenuLine, Pick, Picker};
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
    /// Where the tabs are, those of the left panel first.
    pub(crate) places: Vec<String>,
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
                places: workspace
                    .left
                    .iter()
                    .chain(&workspace.right)
                    .map(|tab| tab.place.to_string())
                    .collect(),
            })
            .collect()
    }
}

/// The window, with the workspaces as `workspaces.toml` held them last.
#[derive(Debug)]
pub(crate) struct WorkspacesWindow {
    rows: Vec<Row>,
    /// The workspace the cursor goes to once the rows come again, such as one just saved.
    wanted: Option<String>,
    /// Filters the rows by name.
    picker: Picker,
}

impl WorkspacesWindow {
    /// A window on `rows`, whose filter matches as fzf does if `fuzzy` (`ui.fuzzy_search`).
    pub(crate) fn new(rows: Vec<Row>, fuzzy: bool) -> Self {
        let mut window = Self {
            rows,
            wanted: None,
            picker: Picker::new(fuzzy),
        };
        window.picker.set_rows(window.texts());
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
        self.picker.set_rows(self.texts());
        let shown = self.shown();
        let cursor = chosen
            .and_then(|name| shown.iter().position(|row| row.name == name))
            .unwrap_or(self.picker.cursor())
            .min(shown.len().saturating_sub(1));
        self.picker.set_cursor(cursor);
    }

    /// What the filter finds each workspace by: its name.
    fn texts(&self) -> Vec<Vec<String>> {
        self.rows.iter().map(|row| vec![row.name.clone()]).collect()
    }

    /// The rows the filter shows, in their order.
    fn shown(&self) -> Vec<&Row> {
        self.picker.shown(&self.rows)
    }

    fn chosen(&self) -> Option<&Row> {
        self.shown().get(self.picker.cursor()).copied()
    }

    /// Whether a workspace is under the cursor, for F6 and F8.
    pub(crate) fn has_chosen(&self) -> bool {
        self.chosen().is_some()
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    /// The rows, for the count before a page key.
    pub(crate) fn picker_mut(&mut self) -> &mut Picker {
        &mut self.picker
    }

    pub(crate) fn mode(&self) -> Mode {
        self.picker.mode()
    }

    /// Takes a key: the picker's, where Enter restores a workspace, and a, which saves the
    /// tabs as a new workspace, r and F6, which rename, and d d and F8, which delete.
    pub(crate) fn handle(&mut self, input: Resolved) -> WorkspacesEvent {
        let chosen = |window: &Self, event: fn(String) -> WorkspacesEvent| {
            window
                .chosen()
                .map_or(WorkspacesEvent::Pending, |row| event(row.name.clone()))
        };
        match self.picker.handle(input) {
            Pick::Chosen(row) => match self.shown().get(row) {
                Some(row) => WorkspacesEvent::Restore(row.name.clone()),
                None => WorkspacesEvent::Pending,
            },
            Pick::Closed => WorkspacesEvent::Closed,
            Pick::Other(Action::SaveWorkspace) => WorkspacesEvent::Save,
            Pick::Other(Action::Move) => chosen(self, WorkspacesEvent::Rename),
            Pick::Other(Action::Delete) => chosen(self, WorkspacesEvent::Delete),
            Pick::Pending | Pick::Other(_) => WorkspacesEvent::Pending,
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

    /// Draws the window centered over `area`, the panels: the filter, then the workspaces,
    /// scrolled to the cursor.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let shown = self.picker.shown(&self.rows);
        let lines: Vec<MenuLine> = (0..shown.len()).map(MenuLine::Row).collect();
        let counts: Vec<String> = shown
            .iter()
            .map(|row| fl!("workspaces-tabs", count = row.tabs))
            .collect();
        let count_width = counts
            .iter()
            .map(|count| cells::width(count))
            .max()
            .unwrap_or(0);
        let title = fl!("workspaces-title");
        let body = Body {
            title: &title,
            width: WIDTH,
            lines: &lines,
            max_lines: Some(MAX_ROWS),
            message: self.rows.is_empty().then(|| fl!("workspaces-none")),
            full: shown.get(self.picker.cursor()).map(|row| {
                let places = row.places.join("  ");
                cells::sanitize(places.as_bytes())
            }),
            ..Body::default()
        };
        let focused = Colors::of(theme, false).focused_style();
        self.picker.render(frame, area, theme, body, &mut |row| {
            // The name, a space, the tabs.
            let name_width = row.width.saturating_sub(count_width + 1);
            let (style, mark) = if row.selected {
                (focused, theme.truncated_cursor)
            } else {
                (theme.dialog, theme.truncated_dialog)
            };
            let name = cells::sanitize(shown[row.index].name.as_bytes());
            let fit = (Align::Left, Cut::End);
            let mut line = cells::fitted(&name, name_width, fit, (style, mark), theme.glyphs.marks);
            let count = cells::fit(
                &counts[row.index],
                count_width,
                Align::Right,
                theme.glyphs.marks,
            );
            line.spans.push(Span::styled(format!(" {count}"), style));
            line
        });
    }
}

/// A tab as a workspace saves it: where `panel` is, or goes, its sort order, and what is
/// under its cursor; `current` if it is the tab that shows on its side. A location that
/// `workspaces.toml` cannot hold, such as a path that is not UTF-8, is saved as the nearest
/// directory above it that it can hold, or else as the home directory.
pub(crate) fn saved_tab(panel: &Panel, current: bool, home: &Path) -> SavedTab {
    let (sort, descending) = panel.sort_order();
    let (mut location, cursor) = panel.heading();
    let mut cursor = cursor.and_then(|name| String::from_utf8(name).ok());
    let place = loop {
        if let Some(place) = place_of(&location, home) {
            break place;
        }
        cursor = None;
        match location.parent() {
            Some(parent) => location = parent,
            None => break Place::Local("~".to_owned()),
        }
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
        // `workspaces.toml` holds only places that start with / or ~.
        place @ Place::Local(_) => {
            Location::Local(place.local_dir(home).unwrap_or_else(|| home.to_path_buf()))
        }
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
                places: vec!["~/src/noc".to_owned(); index + 2],
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
        window.handle(Resolved::Action(Action::QuickSearch));
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
    fn enter_f6_and_f8_act_on_their_row() {
        let mut window = window();
        let restore = |name: &str| WorkspacesEvent::Restore(name.to_owned());
        assert_eq!(press(&mut window, Action::Confirm), restore("noon"));
        assert_eq!(
            window.handle(Resolved::Insert('3')),
            WorkspacesEvent::Pending,
            "no digits"
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
            "a saves with or without workspaces"
        );
    }

    #[test]
    fn the_filter_finds_names_and_its_digits_are_text() {
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
        assert!(draw(&mut window, 10).contains("a here"));
    }

    #[test]
    fn draws_names_and_their_tabs() {
        let mut window = window();
        insta::assert_snapshot!(draw(&mut window, 12));
    }

    #[test]
    fn k_shows_where_the_tabs_under_the_cursor_are_under_the_rows() {
        let mut window = window();
        press(&mut window, Action::ShowFull);
        insta::assert_snapshot!(draw(&mut window, 14));
    }

    #[test]
    fn locations_are_saved_as_workspaces_toml_can_hold_them() {
        let home = Path::new("/home/me");
        let remote = |host: &str, path: &[u8]| Location::Remote {
            host: host.to_owned(),
            path: RemotePath::new(path),
        };
        let local = |path: &str| Location::Local(PathBuf::from(path));
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
