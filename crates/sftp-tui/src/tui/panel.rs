//! A panel that lists a directory of the local file system.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;
use std::time::SystemTime;

use jiff::tz::TimeZone;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Block;
use sftp_tui_vfs::{DirEntry, VfsError};

use super::cells::{self, Align, MTIME_WIDTH};
use super::keymap::Action;
use crate::i18n::fl;

/// Width of the size column, as in mc.
const SIZE_WIDTH: usize = 7;

/// A request to list `path` for a panel; the reply must carry `generation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListRequest {
    pub(crate) generation: u64,
    pub(crate) path: PathBuf,
}

/// Which row gets the cursor once a listing arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Focus {
    First,
    /// The entry with this name, or the first row if it is gone.
    Name(Vec<u8>),
}

#[derive(Debug)]
struct Pending {
    generation: u64,
    path: PathBuf,
    focus: Focus,
}

/// A row of the listing.
#[derive(Debug, Clone, Copy)]
enum Row<'a> {
    /// `..`, the parent directory.
    Parent,
    Entry(&'a DirEntry),
}

/// One directory listing with a cursor.
#[derive(Debug)]
pub(crate) struct Panel {
    /// The directory shown; a requested one replaces it once its listing arrives.
    path: PathBuf,
    /// Directories first, then by name.
    entries: Vec<DirEntry>,
    /// Row under the cursor; row 0 is `..` unless the directory is `/`.
    cursor: usize,
    /// First row on screen.
    offset: usize,
    /// Rows on screen at the last render.
    page: usize,
    generation: u64,
    pending: Option<Pending>,
    error: Option<String>,
}

impl Panel {
    /// A panel for `path`, and the request for its first listing.
    pub(crate) fn new(path: PathBuf) -> (Self, ListRequest) {
        let mut panel = Self {
            path: path.clone(),
            entries: Vec::new(),
            cursor: 0,
            offset: 0,
            page: 1,
            generation: 0,
            pending: None,
            error: None,
        };
        let request = panel.open(path, Focus::First);
        (panel, request)
    }

    fn open(&mut self, path: PathBuf, focus: Focus) -> ListRequest {
        self.generation += 1;
        self.error = None;
        self.pending = Some(Pending {
            generation: self.generation,
            path: path.clone(),
            focus,
        });
        ListRequest {
            generation: self.generation,
            path,
        }
    }

    /// Takes the reply to a [`ListRequest`]; replies to older requests are dropped. On error the
    /// panel keeps showing its directory and reports the error below the listing.
    pub(crate) fn listed(&mut self, generation: u64, result: Result<Vec<DirEntry>, VfsError>) {
        let Some(pending) = self
            .pending
            .take_if(|pending| pending.generation == generation)
        else {
            return;
        };
        match result {
            Ok(mut entries) => {
                entries.sort_by_cached_key(|entry| {
                    let name = String::from_utf8_lossy(&entry.name).to_lowercase();
                    (!entry.is_dir_like(), name, entry.name.clone())
                });
                self.path = pending.path;
                self.entries = entries;
                self.offset = 0;
                self.cursor = match pending.focus {
                    Focus::First => 0,
                    Focus::Name(name) => (0..self.rows())
                        .find(|&row| matches!(self.row(row), Some(Row::Entry(entry)) if entry.name == name))
                        .unwrap_or(0),
                };
            }
            Err(error) => {
                // The title shows the directory, so a subdirectory needs only its name.
                let shown = pending
                    .path
                    .strip_prefix(&self.path)
                    .ok()
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .unwrap_or(&pending.path);
                let path = cells::sanitize(shown.as_os_str().as_bytes());
                self.error = Some(fl!("panel-error", path = path, reason = describe(&error)));
            }
        }
    }

    /// Moves the cursor or opens a directory. Returns the listing to request, if any.
    pub(crate) fn handle(&mut self, action: Action) -> Option<ListRequest> {
        let last = self.rows().saturating_sub(1);
        let page = self.page.max(1);
        match action {
            Action::Up => self.cursor = self.cursor.saturating_sub(1),
            Action::Down => self.cursor = (self.cursor + 1).min(last),
            Action::PageUp => self.cursor = self.cursor.saturating_sub(page),
            Action::PageDown => self.cursor = (self.cursor + page).min(last),
            Action::Home => self.cursor = 0,
            Action::End => self.cursor = last,
            Action::Enter => match self.row(self.cursor)? {
                Row::Parent => return self.open_parent(),
                Row::Entry(entry) if entry.is_dir_like() => {
                    let path = self.path.join(OsStr::from_bytes(&entry.name));
                    return Some(self.open(path, Focus::First));
                }
                Row::Entry(_) => {}
            },
            Action::Parent => return self.open_parent(),
            Action::Reload => {
                let focus = match self.row(self.cursor) {
                    Some(Row::Entry(entry)) => Focus::Name(entry.name.clone()),
                    _ => Focus::First,
                };
                return Some(self.open(self.path.clone(), focus));
            }
            _ => {}
        }
        None
    }

    /// Opens the parent directory with the cursor on the directory it came from.
    fn open_parent(&mut self) -> Option<ListRequest> {
        let parent = self.path.parent()?.to_path_buf();
        let focus = self
            .path
            .file_name()
            .map_or(Focus::First, |name| Focus::Name(name.as_bytes().to_vec()));
        Some(self.open(parent, focus))
    }

    fn has_parent(&self) -> bool {
        self.path.parent().is_some()
    }

    fn rows(&self) -> usize {
        self.entries.len() + usize::from(self.has_parent())
    }

    fn row(&self, index: usize) -> Option<Row<'_>> {
        match index.checked_sub(usize::from(self.has_parent())) {
            None => Some(Row::Parent),
            Some(index) => self.entries.get(index).map(Row::Entry),
        }
    }

    /// Draws the panel: the directory in the frame, column headers, the rows, and a status
    /// line with the name under the cursor, the loading state, or the last error.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        active: bool,
        now: SystemTime,
        tz: &TimeZone,
    ) {
        let reversed = Style::new().reversed();
        let mut title = cells::sanitize(self.path.as_os_str().as_bytes());
        let room = usize::from(area.width.saturating_sub(4));
        if cells::width(&title) > room {
            title = cells::fit(&title, room, Align::Left);
        }
        let title_style = if active { reversed } else { Style::new() };
        let title = Line::styled(format!(" {title} "), title_style);
        let block = Block::bordered().title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height < 3 || inner.width < 2 {
            return;
        }
        let columns = Columns::for_width(usize::from(inner.width));
        let list_height = usize::from(inner.height - 3);
        self.page = list_height;
        self.scroll(list_height);

        let line = |y: u16| Rect::new(inner.x, y, inner.width, 1);
        let header = columns.line(
            &fl!("panel-name"),
            &fl!("panel-size"),
            &fl!("panel-time"),
            Align::Center,
        );
        frame.render_widget(Line::raw(header), line(inner.y));
        for (screen_row, index) in (self.offset..self.rows()).take(list_height).enumerate() {
            let Some(row) = self.row(index) else { break };
            let (name, size, time) = match row {
                Row::Parent => ("..".to_owned(), fl!("panel-up-dir"), String::new()),
                Row::Entry(entry) => (
                    cells::sanitize(&entry.name),
                    if entry.is_dir_like() {
                        fl!("panel-dir")
                    } else {
                        entry
                            .metadata
                            .size
                            .map_or_else(String::new, |size| cells::size(size, SIZE_WIDTH))
                    },
                    cells::mtime(entry.metadata.modified, now, tz),
                ),
            };
            let text = columns.row(&name, &size, &time);
            let style = if active && index == self.cursor {
                reversed
            } else {
                Style::new()
            };
            let y = inner.y + 1 + u16::try_from(screen_row).unwrap_or(u16::MAX);
            frame.render_widget(Line::styled(text, style), line(y));
        }

        let separator_y = inner.bottom() - 2;
        let separator = format!(
            "├{}┤",
            "─".repeat(usize::from(area.width.saturating_sub(2)))
        );
        frame.render_widget(
            Line::raw(separator),
            Rect::new(area.x, separator_y, area.width, 1),
        );
        let status = if let Some(error) = &self.error {
            error.clone()
        } else if self.pending.is_some() {
            fl!("panel-loading")
        } else {
            match self.row(self.cursor) {
                Some(Row::Parent) => "..".to_owned(),
                Some(Row::Entry(entry)) => cells::sanitize(&entry.name),
                None => String::new(),
            }
        };
        let status = cells::fit(&status, usize::from(inner.width), Align::Left);
        frame.render_widget(Line::raw(status), line(inner.bottom() - 1));
    }

    /// Keeps the cursor within the rows and on screen.
    fn scroll(&mut self, height: usize) {
        self.cursor = self.cursor.min(self.rows().saturating_sub(1));
        let height = height.max(1);
        self.offset = self.offset.min(self.rows().saturating_sub(height));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }
    }
}

/// Column widths of the listing: the name takes what the size and time columns leave, and
/// narrow panels drop the time, then the size.
#[derive(Debug, Clone, Copy)]
struct Columns {
    name: usize,
    size: bool,
    time: bool,
}

impl Columns {
    const MIN_NAME: usize = 8;

    fn for_width(width: usize) -> Self {
        let full = width.saturating_sub(SIZE_WIDTH + MTIME_WIDTH + 2);
        if full >= Self::MIN_NAME {
            return Self {
                name: full,
                size: true,
                time: true,
            };
        }
        let sized = width.saturating_sub(SIZE_WIDTH + 1);
        if sized >= Self::MIN_NAME {
            return Self {
                name: sized,
                size: true,
                time: false,
            };
        }
        Self {
            name: width,
            size: false,
            time: false,
        }
    }

    fn line(self, name: &str, size: &str, time: &str, align: Align) -> String {
        let mut text = cells::fit(name, self.name, align);
        if self.size {
            text.push('│');
            text.push_str(&cells::fit(size, SIZE_WIDTH, align));
        }
        if self.time {
            text.push('│');
            text.push_str(&cells::fit(time, MTIME_WIDTH, align));
        }
        text
    }

    fn row(self, name: &str, size: &str, time: &str) -> String {
        let mut text = cells::fit(name, self.name, Align::Left);
        if self.size {
            text.push('│');
            text.push_str(&cells::fit(size, SIZE_WIDTH, Align::Right));
        }
        if self.time {
            text.push('│');
            text.push_str(&cells::fit(time, MTIME_WIDTH, Align::Left));
        }
        text
    }
}

/// Why a directory could not be listed, for the status line.
fn describe(error: &VfsError) -> String {
    match error {
        VfsError::NotFound(_) => fl!("error-not-found"),
        VfsError::PermissionDenied(_) => fl!("error-permission-denied"),
        VfsError::Io(error) => error.to_string(),
        VfsError::Sftp(error) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use sftp_tui_vfs::{FileKind, Metadata};

    use super::*;

    /// 2023-11-14 22:13:20 UTC.
    const NOW: u64 = 1_700_000_000;

    fn entry(name: &str, kind: FileKind, size: u64) -> DirEntry {
        DirEntry {
            name: name.as_bytes().to_vec(),
            metadata: Metadata {
                kind,
                size: Some(size),
                permissions: Some(0o644),
                modified: Some(UNIX_EPOCH + Duration::from_secs(NOW - 3600)),
                uid: Some(501),
                gid: Some(20),
            },
            target_kind: None,
        }
    }

    fn link_to_dir(name: &str) -> DirEntry {
        DirEntry {
            target_kind: Some(FileKind::Dir),
            ..entry(name, FileKind::Symlink, 4)
        }
    }

    fn listing() -> Vec<DirEntry> {
        vec![
            entry("zeta.txt", FileKind::File, 12_345),
            entry("Beta", FileKind::Dir, 4096),
            entry(".hidden", FileKind::File, 1),
            link_to_dir("alpha-link"),
            entry("Alpha.md", FileKind::File, 10_000_000),
            entry("bin", FileKind::Dir, 4096),
        ]
    }

    /// A panel on `path` whose first listing arrived.
    fn loaded(path: &str, entries: Vec<DirEntry>) -> Panel {
        let (mut panel, request) = Panel::new(PathBuf::from(path));
        panel.listed(request.generation, Ok(entries));
        panel
    }

    fn names(panel: &Panel) -> Vec<String> {
        (0..panel.rows())
            .map(|row| match panel.row(row) {
                Some(Row::Parent) => "..".to_owned(),
                Some(Row::Entry(entry)) => entry.display_name().into_owned(),
                None => unreachable!(),
            })
            .collect()
    }

    fn under_cursor(panel: &Panel) -> String {
        names(panel)[panel.cursor].clone()
    }

    fn draw(panel: &mut Panel, width: u16, height: u16, active: bool) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(NOW);
        terminal
            .draw(|frame| panel.render(frame, frame.area(), active, now, &TimeZone::UTC))
            .unwrap();
        terminal.backend().clone()
    }

    #[test]
    fn lists_directories_first_then_names() {
        let panel = loaded("/srv", listing());
        assert_eq!(
            names(&panel),
            [
                "..",
                "alpha-link",
                "Beta",
                "bin",
                ".hidden",
                "Alpha.md",
                "zeta.txt"
            ]
        );
        assert_eq!(panel.cursor, 0);
        let root = loaded("/", listing());
        assert_eq!(names(&root)[0], "alpha-link", "no `..` in /");
    }

    #[test]
    fn moves_the_cursor_within_the_rows() {
        let mut panel = loaded("/srv", listing());
        panel.page = 3;
        assert_eq!(panel.handle(Action::Up), None);
        assert_eq!(panel.cursor, 0);
        panel.handle(Action::Down);
        assert_eq!(under_cursor(&panel), "alpha-link");
        panel.handle(Action::PageDown);
        assert_eq!(under_cursor(&panel), ".hidden");
        panel.handle(Action::PageDown);
        assert_eq!(under_cursor(&panel), "zeta.txt", "stops at the last row");
        panel.handle(Action::PageUp);
        assert_eq!(under_cursor(&panel), "bin");
        panel.handle(Action::Home);
        assert_eq!(panel.cursor, 0);
        panel.handle(Action::End);
        assert_eq!(under_cursor(&panel), "zeta.txt");

        let mut empty = loaded("/", Vec::new());
        for action in [Action::Down, Action::End, Action::PageDown, Action::Enter] {
            assert_eq!(empty.handle(action), None);
            assert_eq!(empty.cursor, 0);
        }
    }

    #[test]
    fn enters_directories_and_links_to_them_but_not_files() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.path, PathBuf::from("/srv/alpha-link"));
        assert_eq!(request.generation, 2);
        panel.listed(
            request.generation,
            Ok(vec![entry("inside", FileKind::File, 1)]),
        );
        assert_eq!(panel.path, PathBuf::from("/srv/alpha-link"));
        assert_eq!(names(&panel), ["..", "inside"]);

        panel.handle(Action::End);
        assert_eq!(panel.handle(Action::Enter), None, "a file does not open");
    }

    #[test]
    fn going_up_puts_the_cursor_on_the_directory_left() {
        let mut panel = loaded("/srv/bin", vec![entry("tool", FileKind::File, 1)]);
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.path, PathBuf::from("/srv"));
        panel.listed(request.generation, Ok(listing()));
        assert_eq!(under_cursor(&panel), "bin");

        // Enter on `..` does the same.
        panel.handle(Action::Home);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.path, PathBuf::from("/"));
        panel.listed(request.generation, Ok(vec![entry("srv", FileKind::Dir, 1)]));
        assert_eq!(under_cursor(&panel), "srv");
        assert_eq!(panel.handle(Action::Parent), None, "/ has no parent");
    }

    #[test]
    fn reload_keeps_the_cursor_on_its_entry() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        let request = panel.handle(Action::Reload).unwrap();
        assert_eq!(request.path, PathBuf::from("/srv"));
        let mut changed = listing();
        changed.push(entry("new.txt", FileKind::File, 1));
        panel.listed(request.generation, Ok(changed));
        assert_eq!(under_cursor(&panel), "zeta.txt");

        let request = panel.handle(Action::Reload).unwrap();
        panel.listed(
            request.generation,
            Ok(vec![entry("other", FileKind::File, 1)]),
        );
        assert_eq!(panel.cursor, 0, "the entry is gone");
    }

    #[test]
    fn drops_stale_replies() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let first = panel.handle(Action::Enter).unwrap();
        panel.handle(Action::Home);
        let second = panel.handle(Action::Enter).unwrap();
        panel.listed(
            first.generation,
            Ok(vec![entry("stale", FileKind::File, 1)]),
        );
        assert_eq!(
            panel.path,
            PathBuf::from("/srv"),
            "an older request was answered"
        );
        panel.listed(second.generation, Ok(listing()));
        assert_eq!(panel.path, PathBuf::from("/"));
        panel.listed(second.generation, Ok(Vec::new()));
        assert_eq!(panel.rows(), listing().len(), "a reply counts once");
    }

    #[test]
    fn a_failed_listing_keeps_the_directory_and_reports_why() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        let denied = VfsError::PermissionDenied("/srv/alpha-link".to_owned());
        panel.listed(request.generation, Err(denied));
        assert_eq!(panel.path, PathBuf::from("/srv"));
        assert_eq!(under_cursor(&panel), "alpha-link");
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open alpha-link: permission denied")
        );
        // The parent is not inside the directory shown, so it gets its full path.
        let request = panel.handle(Action::Parent).unwrap();
        panel.listed(request.generation, Err(VfsError::NotFound("/".to_owned())));
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open /: no such file or directory")
        );
        // The next request clears it.
        panel.handle(Action::Reload);
        assert_eq!(panel.error, None);
    }

    #[test]
    fn draws_the_listing() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        insta::assert_snapshot!(draw(&mut panel, 50, 12, true));
    }

    #[test]
    fn narrow_panels_drop_columns_and_unsafe_names_are_shown_safely() {
        let mut panel = loaded(
            "/a/very/long/path/that/does/not/fit",
            vec![entry("bad\x1b[2Jname", FileKind::File, 3)],
        );
        panel.handle(Action::End);
        insta::assert_snapshot!(draw(&mut panel, 24, 7, true));
    }

    #[test]
    fn the_cursor_shows_only_in_the_active_panel_and_stays_on_screen() {
        let many: Vec<DirEntry> = (0..30)
            .map(|index| entry(&format!("file{index:02}"), FileKind::File, 1))
            .collect();
        let mut panel = loaded("/srv", many);
        panel.handle(Action::End);
        let backend = draw(&mut panel, 40, 10, false);
        assert!(panel.offset > 0, "scrolled to the cursor");
        let text = backend.to_string();
        assert!(text.contains("file29"), "{text}");
        let reversed = backend
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.modifier.contains(ratatui::style::Modifier::REVERSED))
            .count();
        assert_eq!(
            reversed, 0,
            "an inactive panel shows no cursor or title highlight"
        );
    }
}
