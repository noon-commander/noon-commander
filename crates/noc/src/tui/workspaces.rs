//! Workspaces: the tabs of both panels, saved under a name in `workspaces.toml` (ADR 0017),
//! and their window, t w or F9 → Workspace → Workspace list…, which saves, restores,
//! renames, and deletes them. Typing filters the window; while the filter is empty, `1` … `9`
//! and `0` restore the first ten rows.

use std::path::Path;

use noc_config::{PanelSide, Place, SavedTab, SearchMode, SortBy, Workspace, Workspaces};
use noc_scroll::scrollbar::Bar;
use noc_vfs::{Location, RemotePath};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::cells::{self, Align, Cut};
use super::dialog::Colors;
use super::glyphs;
use super::keymap::{Action, Mode, Resolved};
use super::mouse::Pointer;
use super::panel::{Destination, Panel, SortKey};
use super::picker::{Body, MenuLine, Pick, Picker};
use super::search::Kind;
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the window gets, in cells, borders included.
const WIDTH: u16 = 60;
/// Rows the window shows at most.
const MAX_ROWS: u16 = 16;
/// Lines of tabs each column under the rows shows at most; the last says how many more there
/// are on a side with more.
const FOOTER_TABS: usize = 4;

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
    pub(crate) left: SideTabs,
    pub(crate) right: SideTabs,
    /// The side whose panel has the keys.
    pub(crate) active: PanelSide,
}

/// The tabs of a side of a workspace, as the window shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SideTabs {
    /// Where each tab is, terminal-safe.
    pub(crate) places: Vec<String>,
    /// The index of the tab that shows.
    pub(crate) current: usize,
}

impl SideTabs {
    fn of(tabs: &[SavedTab]) -> Self {
        Self {
            places: tabs
                .iter()
                .map(|tab| cells::sanitize(tab.place.to_string().as_bytes()))
                .collect(),
            current: current_of(tabs),
        }
    }
}

impl Row {
    /// The rows of `workspaces`, in their order.
    pub(crate) fn all(workspaces: &Workspaces) -> Vec<Self> {
        workspaces
            .workspaces
            .iter()
            .map(|workspace| Self {
                name: workspace.name.clone(),
                left: SideTabs::of(&workspace.left),
                right: SideTabs::of(&workspace.right),
                active: workspace.active,
            })
            .collect()
    }

    /// The lines of tabs its column under the rows needs: those of its side with more.
    fn tab_lines(&self) -> usize {
        self.left.places.len().max(self.right.places.len())
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
    /// A window on `rows`, whose filter matches in `mode` (`ui.search_mode`).
    pub(crate) fn new(rows: Vec<Row>, mode: SearchMode) -> Self {
        let mut window = Self {
            rows,
            wanted: None,
            picker: Picker::new(mode),
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

    /// The kind of its filter while that takes the keys.
    pub(crate) fn search(&self) -> Option<Kind> {
        self.picker.search()
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
            Pick::Pending | Pick::Filtered | Pick::Other(_) => WorkspacesEvent::Pending,
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
    /// scrolled to the cursor, each with how many tabs each panel has; under them, a column
    /// for each panel with where the tabs of the one under the cursor are.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let shown = self.picker.shown(&self.rows);
        let lines: Vec<MenuLine> = (0..shown.len()).map(MenuLine::Row).collect();
        let formats = noc_locale::current();
        let integer = |count: usize| formats.integer(u64::try_from(count).unwrap_or(u64::MAX));
        let counts: Vec<(String, String)> = shown
            .iter()
            .map(|row| {
                (
                    integer(row.left.places.len()),
                    integer(row.right.places.len()),
                )
            })
            .collect();
        let widest = |side: fn(&(String, String)) -> &String| {
            counts
                .iter()
                .map(|count| cells::width(side(count)))
                .max()
                .unwrap_or(0)
        };
        let count_widths = (widest(|count| &count.0), widest(|count| &count.1));
        let separator = format!(" {} ", glyphs::COLUMN);
        let count_width = count_widths.0 + cells::width(&separator) + count_widths.1;
        // The room inside the box the picker draws.
        let width = usize::from(WIDTH.min(area.width).saturating_sub(4));
        // As many for every workspace, so that the box keeps its height as the cursor moves.
        let tab_lines = (self.rows.iter().map(Row::tab_lines).max())
            .unwrap_or(0)
            .min(FOOTER_TABS);
        let cursor = self.picker.cursor().min(shown.len().saturating_sub(1));
        let (footer, footer_column) = shown.get(cursor).map_or((Vec::new(), None), |row| {
            let (lines, column) = footer(row, (width, tab_lines), theme);
            (lines, Some(column))
        });
        let title = fl!("workspaces-title");
        let body = Body {
            title: &title,
            width: WIDTH,
            lines: &lines,
            max_lines: Some(MAX_ROWS),
            message: self.rows.is_empty().then(|| fl!("workspaces-none")),
            footer,
            footer_column,
            ..Body::default()
        };
        let focused = Colors::of(theme, false).focused_style();
        self.picker.render(frame, area, theme, body, &mut |row| {
            // The name, a space, the tabs of the left panel, a line, those of the right one.
            let name_width = row.width.saturating_sub(count_width + 1);
            let styles = if row.selected {
                (focused, theme.truncated_cursor)
            } else {
                (theme.dialog, theme.truncated_dialog)
            };
            let name = cells::sanitize(shown[row.index].name.as_bytes());
            let fit = (Align::Left, Cut::End);
            let mut line = cells::fitted(&name, name_width, fit, styles, theme.glyphs.marks);
            let count = |count: &str, width| {
                cells::fit(count, width, Align::Right, styles, theme.glyphs.marks)
            };
            let (left, right) = &counts[row.index];
            line.spans.push(Span::styled(" ", styles.0));
            line.spans.extend(count(left, count_widths.0));
            line.spans.push(Span::styled(separator.clone(), styles.0));
            line.spans.extend(count(right, count_widths.1));
            line
        });
    }
}

/// The lines under the rows for `row`, in `width` cells: a column for each panel, headed by its
/// side, with `tabs` lines of where its tabs are under the heading; the heading of the side with
/// the keys, and the tab that shows on each side, stand out. And the cell, from the left, of the
/// line between the columns, which the picker draws.
fn footer(row: &Row, (width, tabs): (usize, usize), theme: &Theme) -> (Vec<Line<'static>>, u16) {
    // A blank cell on either side of the line, as between the text and the frame.
    let column = width.saturating_sub(1) / 2;
    let widths = (column.saturating_sub(1), width.saturating_sub(column + 2));
    let heading = |text: String, side: PanelSide| {
        let style = if side == row.active {
            theme.dialog_title.add_modifier(Modifier::BOLD)
        } else {
            theme.dialog
        };
        (text, style)
    };
    let left = column_lines(
        heading(fl!("workspaces-left"), PanelSide::Left),
        &row.left,
        (widths.0, tabs),
        theme,
    );
    let right = column_lines(
        heading(fl!("workspaces-right"), PanelSide::Right),
        &row.right,
        (widths.1, tabs),
        theme,
    );
    let lines = left
        .into_iter()
        .zip(right)
        .map(|(mut spans, right)| {
            spans.push(Span::styled(" ".repeat(3), theme.dialog));
            spans.extend(right);
            Line::from(spans)
        })
        .collect();
    (lines, u16::try_from(column).unwrap_or(u16::MAX))
}

/// A column under the rows, `width` cells across: its `heading`, then where the tabs of a side
/// are, in `lines` lines; the last of them says how many more there are if they do not fit.
fn column_lines(
    (heading, style): (String, Style),
    tabs: &SideTabs,
    (width, lines): (usize, usize),
    theme: &Theme,
) -> Vec<Vec<Span<'static>>> {
    let fit = |text: &str, cut: Cut, style: Style| {
        let fitted = cells::fit_cut(text, width, Align::Left, cut);
        cells::spans(fitted, (style, theme.truncated_dialog), theme.glyphs.marks)
    };
    let count = tabs.places.len();
    let shown = if count > lines {
        lines.saturating_sub(1)
    } else {
        count
    };
    let mut column = vec![fit(&heading, Cut::End, style)];
    for (index, place) in tabs.places.iter().take(shown).enumerate() {
        let style = if index == tabs.current {
            theme.dialog.add_modifier(Modifier::BOLD)
        } else {
            theme.dialog
        };
        // The end of a path says most about it.
        column.push(fit(place, Cut::Start, style));
    }
    if shown < count {
        let more: usize = count - shown;
        let more = fl!("workspaces-more", count = more);
        column.push(fit(&more, Cut::End, theme.dialog));
    }
    column.resize(lines + 1, fit("", Cut::End, theme.dialog));
    column
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

    /// A workspace with `left` tabs on the left, the last of them showing, and `right` on the
    /// right, the first showing; the right panel has the keys.
    fn row(name: &str, left: usize, right: usize) -> Row {
        let places = [
            "~/src/noc",
            "~/Downloads",
            "/var/log",
            "web:/var/www",
            "/tmp",
        ];
        let tabs = |places: Vec<&str>, current| SideTabs {
            places: places.into_iter().map(str::to_owned).collect(),
            current,
        };
        Row {
            name: name.to_owned(),
            left: tabs(
                places.iter().copied().cycle().take(left).collect(),
                left - 1,
            ),
            right: tabs(
                places.iter().rev().copied().cycle().take(right).collect(),
                0,
            ),
            active: PanelSide::Right,
        }
    }

    fn rows(names: &[&str]) -> Vec<Row> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| row(name, index + 1, 1 + index % 2))
            .collect()
    }

    fn window() -> WorkspacesWindow {
        WorkspacesWindow::new(
            rows(&["noon", "deploy", "photos", "Notes"]),
            SearchMode::Plain,
        )
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
        draw_with(window, height, &Theme::terminal())
    }

    fn draw_with(window: &mut WorkspacesWindow, height: u16, theme: &Theme) -> String {
        let mut terminal = Terminal::new(TestBackend::new(64, height)).unwrap();
        terminal
            .draw(|frame| window.render(frame, frame.area(), theme))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn enter_f6_and_f8_act_on_their_row() {
        let mut window = window();
        let restore = |name: &str| WorkspacesEvent::Restore(name.to_owned());
        assert_eq!(press(&mut window, Action::Confirm), restore("noon"));
        press(&mut window, Action::NormalMode);
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
        let mut empty = WorkspacesWindow::new(Vec::new(), SearchMode::Plain);
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
        let mut window = WorkspacesWindow::new(rows(&["photos", "deploy"]), SearchMode::Fzf);
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
        let mut window = WorkspacesWindow::new(Vec::new(), SearchMode::Plain);
        assert!(draw(&mut window, 10).contains("a here"));
    }

    #[test]
    fn draws_names_and_their_tabs_and_where_those_under_the_cursor_are() {
        let mut window = window();
        press(&mut window, Action::End);
        insta::assert_snapshot!(draw(&mut window, 16));
    }

    #[test]
    fn a_side_with_more_tabs_than_fit_says_how_many_more() {
        let mut window = WorkspacesWindow::new(vec![row("many", 7, 1)], SearchMode::Plain);
        let text = draw(&mut window, 16);
        assert!(text.contains("7 │ 1"), "{text}");
        assert!(
            text.contains("/var/log") && text.contains("4 more…"),
            "{text}"
        );
        assert!(
            !text.contains("web:/var/www"),
            "the fourth does not show: {text}"
        );
    }

    #[test]
    fn the_line_between_the_columns_joins_the_frame() {
        let mut window = window();
        let double = draw(&mut window, 16);
        assert!(double.contains('┬') && double.contains('╧'), "{double}");
        let single = Theme::terminal().with_borders(noc_config::Borders::Single);
        let single = draw_with(&mut window, 16, &single);
        assert!(single.contains('┬') && single.contains('┴'), "{single}");
        assert!(!single.contains('╧'), "{single}");
    }

    #[test]
    fn the_box_keeps_its_height_as_the_cursor_moves() {
        let mut window = window();
        let height = |text: &str| text.lines().filter(|line| line.contains('║')).count();
        let first = height(&draw(&mut window, 20));
        press(&mut window, Action::End);
        assert_eq!(height(&draw(&mut window, 20)), first);
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
