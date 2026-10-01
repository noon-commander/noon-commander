//! A panel that lists the virtual root or a directory.

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
use sftp_tui_vfs::{DirEntry, Location, RemotePath};

use super::cells::{self, Align, MTIME_WIDTH};
use super::keymap::Action;
use super::root::RootHost;
use crate::i18n::fl;

/// Width of the size column, as in mc.
const SIZE_WIDTH: usize = 7;

/// Narrow panels drop columns to keep at least this many cells for names.
const MIN_NAME_WIDTH: usize = 8;

/// A request to list `location` for a panel; the reply must carry `generation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListRequest {
    pub(crate) generation: u64,
    pub(crate) location: Location,
}

/// The reply to a [`ListRequest`]: where the listing is from, which may be more exact than
/// the request (a host's start directory as a path), and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) location: Location,
    pub(crate) listing: Listing,
}

/// What a location holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Listing {
    /// The hosts of the virtual root, which shows the local file system before them.
    Root(Vec<RootHost>),
    /// The entries of a directory, in any order.
    Dir(Vec<DirEntry>),
}

/// What the app knows about a host beyond the listing of the virtual root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HostState {
    pub(crate) status: HostStatus,
    /// From `ssh -G` in this session, fresher than the cached address in the listing.
    pub(crate) address: Option<String>,
}

/// The connection state of a host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum HostStatus {
    #[default]
    Idle,
    Connecting,
    Connected,
    /// The last attempt failed, or the connection was lost.
    Failed,
}

impl HostStatus {
    /// The marker in front of the host's name.
    fn marker(self) -> char {
        match self {
            Self::Idle => '○',
            Self::Connecting => '◌',
            Self::Connected => '●',
            Self::Failed => '✗',
        }
    }
}

/// What a directory is sorted by. Directories always come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Name,
    Extension,
    Time,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sort {
    key: SortKey,
    descending: bool,
}

impl Sort {
    /// Sorting by `key`: a new key starts as Far does, newest and largest first, names A to
    /// Z; the same key again reverses the order.
    fn by(self, key: SortKey) -> Self {
        let descending = if key == self.key {
            !self.descending
        } else {
            matches!(key, SortKey::Time | SortKey::Size)
        };
        Self { key, descending }
    }

    fn arrow(self) -> char {
        if self.descending { '↓' } else { '↑' }
    }
}

/// Which row gets the cursor once a listing arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Focus {
    /// The first row: `..`, or the local file system in the virtual root.
    First,
    /// The entry with this name, or the first row if it is gone.
    Name(Vec<u8>),
    /// The host with this alias, or the first row if it is gone.
    Host(String),
}

impl Focus {
    fn matches(&self, row: Row<'_>) -> bool {
        match (self, row) {
            (Self::Name(name), Row::Entry(entry)) => entry.name == *name,
            (Self::Host(alias), Row::Host(host)) => host.alias == *alias,
            _ => false,
        }
    }
}

#[derive(Debug)]
struct Pending {
    generation: u64,
    location: Location,
    focus: Focus,
}

/// A row of the listing.
#[derive(Debug, Clone, Copy)]
enum Row<'a> {
    /// `..`: the parent directory, or the virtual root from `/`.
    Parent,
    Entry(&'a DirEntry),
    /// The local file system, first in the virtual root.
    Local,
    Host(&'a RootHost),
}

/// One listing with a cursor.
#[derive(Debug)]
pub(crate) struct Panel {
    /// What is shown; a requested location replaces it once its listing arrives.
    location: Location,
    /// Directories first, then as `sort` says; hosts in config order.
    listing: Listing,
    /// The entries of a directory listing that are shown, in order: hidden files may be left
    /// out.
    shown: Vec<usize>,
    sort: Sort,
    show_hidden: bool,
    /// Where the local file system opens from the virtual root.
    home: PathBuf,
    /// Row under the cursor; row 0 is `..`, or the local file system in the virtual root.
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
    /// A panel for `location`, and the request for its first listing. From the virtual root,
    /// the local file system opens at `home`. `show_hidden` shows names that start with a dot.
    pub(crate) fn new(location: Location, home: PathBuf, show_hidden: bool) -> (Self, ListRequest) {
        let listing = match location {
            Location::Root => Listing::Root(Vec::new()),
            Location::Local(_) | Location::Remote { .. } => Listing::Dir(Vec::new()),
        };
        let mut panel = Self {
            location: location.clone(),
            listing,
            shown: Vec::new(),
            sort: Sort {
                key: SortKey::Name,
                descending: false,
            },
            show_hidden,
            home,
            cursor: 0,
            offset: 0,
            page: 1,
            generation: 0,
            pending: None,
            error: None,
        };
        let request = panel.open(location, Focus::First);
        (panel, request)
    }

    fn open(&mut self, location: Location, focus: Focus) -> ListRequest {
        self.generation += 1;
        self.error = None;
        self.pending = Some(Pending {
            generation: self.generation,
            location: location.clone(),
            focus,
        });
        ListRequest {
            generation: self.generation,
            location,
        }
    }

    /// Takes the reply to a [`ListRequest`]; replies to older requests are dropped. On error the
    /// panel keeps showing what it showed and reports `reason` below the listing.
    pub(crate) fn listed(&mut self, generation: u64, result: Result<Listed, String>) {
        let Some(pending) = self
            .pending
            .take_if(|pending| pending.generation == generation)
        else {
            return;
        };
        match result {
            Ok(Listed { location, listing }) => {
                self.location = location;
                self.listing = listing;
                self.arrange();
                self.offset = 0;
                self.cursor = (0..self.rows())
                    .find(|&index| {
                        self.row(index)
                            .is_some_and(|row| pending.focus.matches(row))
                    })
                    .unwrap_or(0);
            }
            Err(reason) => {
                let shown = match (&pending.location, &self.location) {
                    // The title shows the directory, so a subdirectory needs only its name.
                    (Location::Local(path), Location::Local(current)) => path
                        .strip_prefix(current)
                        .ok()
                        .filter(|relative| !relative.as_os_str().is_empty())
                        .map(|relative| cells::sanitize(relative.as_os_str().as_bytes())),
                    _ => None,
                };
                let shown = shown.unwrap_or_else(|| location_text(&pending.location));
                // The reason may quote ssh, which may quote the server.
                let reason = cells::sanitize(reason.as_bytes());
                self.error = Some(fl!("panel-error", path = shown, reason = reason));
            }
        }
    }

    /// The request the panel waits for, if any.
    pub(crate) fn pending_request(&self) -> Option<ListRequest> {
        self.pending.as_ref().map(|pending| ListRequest {
            generation: pending.generation,
            location: pending.location.clone(),
        })
    }

    /// Stops waiting for a listing; its reply will be dropped.
    pub(crate) fn cancel(&mut self) {
        self.pending = None;
    }

    /// Leaves `host` for the virtual root, with the cursor on it and the `reason` of a lost
    /// connection below the listing, if the panel shows or waits for that host.
    pub(crate) fn leave_host(&mut self, host: &str, reason: Option<&str>) -> Option<ListRequest> {
        let on_host = |location: &Location| matches!(location, Location::Remote { host: shown, .. } if shown == host);
        let pending = self.pending.as_ref().map(|pending| &pending.location);
        if !on_host(&self.location) && !pending.is_some_and(on_host) {
            return None;
        }
        let request = self.open(Location::Root, Focus::Host(host.to_owned()));
        if let Some(reason) = reason {
            let reason = cells::sanitize(reason.as_bytes());
            let host = cells::sanitize(host.as_bytes());
            self.error = Some(fl!("panel-host-lost", host = host, reason = reason));
        }
        Some(request)
    }

    /// Shows or hides names that start with a dot; the cursor stays on its entry if it can.
    pub(crate) fn set_show_hidden(&mut self, show: bool) {
        if self.show_hidden != show {
            self.show_hidden = show;
            self.rearrange();
        }
    }

    /// Sorts and filters a directory listing again, keeping the cursor on its entry, or on its
    /// row if the entry is no longer shown.
    fn rearrange(&mut self) {
        let name = match self.row(self.cursor) {
            Some(Row::Entry(entry)) => Some(entry.name.clone()),
            _ => None,
        };
        self.arrange();
        if let Some(name) = name
            && let Some(row) = (0..self.rows())
                .find(|&row| matches!(self.row(row), Some(Row::Entry(entry)) if entry.name == name))
        {
            self.cursor = row;
        }
    }

    /// Sorts a directory listing and picks the entries to show.
    fn arrange(&mut self) {
        let Listing::Dir(entries) = &mut self.listing else {
            self.shown.clear();
            return;
        };
        sort(entries, self.sort);
        let show_hidden = self.show_hidden;
        self.shown = (0..entries.len())
            .filter(|&index| show_hidden || !entries[index].name.starts_with(b"."))
            .collect();
    }

    /// Whether the panel shows the virtual root.
    pub(crate) fn shows_root(&self) -> bool {
        self.location == Location::Root
    }

    /// The alias of the host under the cursor in the virtual root.
    pub(crate) fn host_under_cursor(&self) -> Option<&str> {
        match self.row(self.cursor)? {
            Row::Host(host) => Some(&host.alias),
            Row::Parent | Row::Entry(_) | Row::Local => None,
        }
    }

    /// Moves the cursor or opens a directory or host. Returns the listing to request, if any.
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
                    let location = child(&self.location, &entry.name)?;
                    return Some(self.open(location, Focus::First));
                }
                Row::Entry(_) => {}
                Row::Local => {
                    let location = Location::Local(self.home.clone());
                    return Some(self.open(location, Focus::First));
                }
                Row::Host(host) => {
                    let location = Location::Remote {
                        host: host.alias.clone(),
                        path: RemotePath::from(""),
                    };
                    return Some(self.open(location, Focus::First));
                }
            },
            Action::Parent => return self.open_parent(),
            Action::SortByName => self.sort_by(SortKey::Name),
            Action::SortByExtension => self.sort_by(SortKey::Extension),
            Action::SortByTime => self.sort_by(SortKey::Time),
            Action::SortBySize => self.sort_by(SortKey::Size),
            Action::Reload => {
                let focus = match self.row(self.cursor) {
                    Some(Row::Entry(entry)) => Focus::Name(entry.name.clone()),
                    Some(Row::Host(host)) => Focus::Host(host.alias.clone()),
                    _ => Focus::First,
                };
                return Some(self.open(self.location.clone(), focus));
            }
            _ => {}
        }
        None
    }

    fn sort_by(&mut self, key: SortKey) {
        self.sort = self.sort.by(key);
        self.rearrange();
    }

    /// Opens the parent with the cursor on the directory it came from, or on its host or the
    /// local file system in the virtual root.
    fn open_parent(&mut self) -> Option<ListRequest> {
        let parent = self.location.parent();
        let focus = match (&self.location, &parent) {
            (Location::Root, _) => return None,
            (Location::Local(_), Location::Root) => Focus::First,
            (Location::Remote { host, .. }, Location::Root) => Focus::Host(host.clone()),
            (Location::Local(path), _) => path
                .file_name()
                .map_or(Focus::First, |name| Focus::Name(name.as_bytes().to_vec())),
            (Location::Remote { path, .. }, _) => path
                .file_name()
                .map_or(Focus::First, |name| Focus::Name(name.to_vec())),
        };
        Some(self.open(parent, focus))
    }

    /// Both kinds of listing have one row before their entries: `..` or the local file system.
    fn rows(&self) -> usize {
        1 + match &self.listing {
            Listing::Root(hosts) => hosts.len(),
            Listing::Dir(_) => self.shown.len(),
        }
    }

    fn row(&self, index: usize) -> Option<Row<'_>> {
        match (&self.listing, index.checked_sub(1)) {
            (Listing::Root(_), None) => Some(Row::Local),
            (Listing::Root(hosts), Some(index)) => hosts.get(index).map(Row::Host),
            (Listing::Dir(_), None) => Some(Row::Parent),
            (Listing::Dir(entries), Some(index)) => {
                let index = *self.shown.get(index)?;
                entries.get(index).map(Row::Entry)
            }
        }
    }

    /// Draws the panel: the location in the frame, column headers, the rows, and a status line
    /// with the name under the cursor, the loading state, or the last error. `hosts` tells what
    /// the app knows about a host.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        active: bool,
        hosts: &dyn Fn(&str) -> HostState,
        now: SystemTime,
        tz: &TimeZone,
    ) {
        let reversed = Style::new().reversed();
        let mut title = location_text(&self.location);
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
        let width = usize::from(inner.width);
        let list_height = usize::from(inner.height - 3);
        self.page = list_height;
        self.scroll(list_height);

        let columns = match &self.listing {
            Listing::Dir(_) => Columns::Dir(DirColumns::for_width(width)),
            Listing::Root(listed) => {
                let addresses = listed.iter().map(|host| address(host, hosts));
                Columns::Root(RootColumns::for_width(width, addresses))
            }
        };
        let line = |y: u16| Rect::new(inner.x, y, inner.width, 1);
        frame.render_widget(Line::raw(columns.header(self.sort)), line(inner.y));
        for (screen_row, index) in (self.offset..self.rows()).take(list_height).enumerate() {
            let Some(row) = self.row(index) else { break };
            let style = if active && index == self.cursor {
                reversed
            } else {
                Style::new()
            };
            let y = inner.y + 1 + u16::try_from(screen_row).unwrap_or(u16::MAX);
            let text = columns.row(row, hosts, now, tz);
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
        } else if let Some(pending) = &self.pending {
            match &pending.location {
                Location::Remote { host, .. } if hosts(host).status == HostStatus::Connecting => {
                    fl!("panel-connecting", host = cells::sanitize(host.as_bytes()))
                }
                _ => fl!("panel-loading"),
            }
        } else {
            match self.row(self.cursor) {
                Some(Row::Parent) => "..".to_owned(),
                Some(Row::Entry(entry)) => cells::sanitize(&entry.name),
                // Where it opens, and the alias that a label stands for.
                Some(Row::Local) => cells::sanitize(self.home.as_os_str().as_bytes()),
                Some(Row::Host(host)) => cells::sanitize(host.alias.as_bytes()),
                None => String::new(),
            }
        };
        let status = cells::fit(&status, width, Align::Left);
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

/// How rows become text: the columns of a directory or of the virtual root.
#[derive(Debug, Clone, Copy)]
enum Columns {
    Dir(DirColumns),
    Root(RootColumns),
}

impl Columns {
    /// The column titles; an arrow marks the one a directory is sorted by, and its direction.
    fn header(self, sort: Sort) -> String {
        match self {
            Self::Dir(columns) => {
                let mut titles = [fl!("panel-name"), fl!("panel-size"), fl!("panel-time")];
                let (column, title) = match sort.key {
                    SortKey::Name => (0, fl!("panel-name")),
                    SortKey::Extension => (0, fl!("panel-name-by-extension")),
                    SortKey::Size => (1, fl!("panel-size")),
                    SortKey::Time => (2, fl!("panel-time")),
                };
                titles[column] = format!("{}{title}", sort.arrow());
                let [name, size, time] = titles;
                columns.join(&name, &size, &time, [Align::Center; 3])
            }
            Self::Root(columns) => {
                columns.join(&fl!("panel-name"), &fl!("root-address"), [Align::Center; 2])
            }
        }
    }

    fn row(
        self,
        row: Row<'_>,
        hosts: &dyn Fn(&str) -> HostState,
        now: SystemTime,
        tz: &TimeZone,
    ) -> String {
        const DIR: [Align; 3] = [Align::Left, Align::Right, Align::Left];
        match (self, row) {
            (Self::Dir(columns), Row::Parent) => columns.join("..", &fl!("panel-up-dir"), "", DIR),
            (Self::Dir(columns), Row::Entry(entry)) => {
                let size = if entry.is_dir_like() {
                    fl!("panel-dir")
                } else {
                    entry
                        .metadata
                        .size
                        .map_or_else(String::new, |size| cells::size(size, SIZE_WIDTH))
                };
                let time = cells::mtime(entry.metadata.modified, now, tz);
                columns.join(&cells::sanitize(&entry.name), &size, &time, DIR)
            }
            // Without a marker, but in line with the hosts.
            (Self::Root(columns), Row::Local) => {
                columns.join(&format!("  {}", fl!("root-local")), "~", [Align::Left; 2])
            }
            (Self::Root(columns), Row::Host(host)) => {
                let name = host.label.as_deref().unwrap_or(&host.alias);
                let name = cells::sanitize(name.as_bytes());
                let marker = hosts(&host.alias).status.marker();
                columns.join(
                    &format!("{marker} {name}"),
                    &address(host, hosts),
                    [Align::Left; 2],
                )
            }
            // A listing has only rows of its own kind.
            (Self::Dir(_), Row::Local | Row::Host(_))
            | (Self::Root(_), Row::Parent | Row::Entry(_)) => String::new(),
        }
    }
}

/// Column widths of a directory: the name takes what the size and time columns leave, and
/// narrow panels drop the time, then the size.
#[derive(Debug, Clone, Copy)]
struct DirColumns {
    name: usize,
    size: bool,
    time: bool,
}

impl DirColumns {
    fn for_width(width: usize) -> Self {
        let full = width.saturating_sub(SIZE_WIDTH + MTIME_WIDTH + 2);
        if full >= MIN_NAME_WIDTH {
            return Self {
                name: full,
                size: true,
                time: true,
            };
        }
        let sized = width.saturating_sub(SIZE_WIDTH + 1);
        if sized >= MIN_NAME_WIDTH {
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

    fn join(self, name: &str, size: &str, time: &str, align: [Align; 3]) -> String {
        let mut text = cells::fit(name, self.name, align[0]);
        if self.size {
            text.push('│');
            text.push_str(&cells::fit(size, SIZE_WIDTH, align[1]));
        }
        if self.time {
            text.push('│');
            text.push_str(&cells::fit(time, MTIME_WIDTH, align[2]));
        }
        text
    }
}

/// Column widths of the virtual root: the address column fits the longest address, up to half
/// the width, and the name takes the rest; narrow panels drop the address.
#[derive(Debug, Clone, Copy)]
struct RootColumns {
    name: usize,
    address: Option<usize>,
}

impl RootColumns {
    fn for_width(width: usize, addresses: impl Iterator<Item = String>) -> Self {
        let address = addresses
            .map(|address| cells::width(&address))
            .chain([cells::width(&fl!("root-address"))])
            .max()
            .unwrap_or(0)
            .min(width / 2);
        let name = width.saturating_sub(address + 1);
        if name >= MIN_NAME_WIDTH {
            Self {
                name,
                address: Some(address),
            }
        } else {
            Self {
                name: width,
                address: None,
            }
        }
    }

    fn join(self, name: &str, address: &str, align: [Align; 2]) -> String {
        let mut text = cells::fit(name, self.name, align[0]);
        if let Some(width) = self.address {
            text.push('│');
            text.push_str(&cells::fit(address, width, align[1]));
        }
        text
    }
}

/// Sorts a directory: directories first, then by `sort`; ties by name ignoring case, then by
/// the bytes of the name.
fn sort(entries: &mut Vec<DirEntry>, sort: Sort) {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Value {
        Name,
        Extension(String),
        Time(Option<SystemTime>),
        Size(u64),
    }
    let mut keyed: Vec<(bool, Value, String, DirEntry)> = std::mem::take(entries)
        .into_iter()
        .map(|entry| {
            let name = String::from_utf8_lossy(&entry.name).to_lowercase();
            let value = match sort.key {
                SortKey::Name => Value::Name,
                SortKey::Extension => Value::Extension(extension(&name).to_owned()),
                SortKey::Time => Value::Time(entry.metadata.modified),
                SortKey::Size => Value::Size(entry.metadata.size.unwrap_or(0)),
            };
            (!entry.is_dir_like(), value, name, entry)
        })
        .collect();
    keyed.sort_by(|a, b| {
        let order = (&a.1, &a.2, &a.3.name).cmp(&(&b.1, &b.2, &b.3.name));
        a.0.cmp(&b.0).then(if sort.descending {
            order.reverse()
        } else {
            order
        })
    });
    *entries = keyed.into_iter().map(|(.., entry)| entry).collect();
}

/// What follows the last dot of a name, unless the dot starts it.
fn extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot + 1..],
        _ => "",
    }
}

/// The address of a host, terminal-safe: from `ssh -G` in this session, else the cached one.
fn address(host: &RootHost, hosts: &dyn Fn(&str) -> HostState) -> String {
    let address = hosts(&host.alias).address.or_else(|| host.address.clone());
    cells::sanitize(address.unwrap_or_default().as_bytes())
}

/// The directory `name` in `location`.
fn child(location: &Location, name: &[u8]) -> Option<Location> {
    match location {
        Location::Root => None,
        Location::Local(path) => Some(Location::Local(path.join(OsStr::from_bytes(name)))),
        Location::Remote { host, path } => Some(Location::Remote {
            host: host.clone(),
            path: path.join(name),
        }),
    }
}

/// A location for the title and messages: a local path, `host:path`, only `host` for the
/// remote home directory, or the title of the virtual root.
fn location_text(location: &Location) -> String {
    match location {
        Location::Root => fl!("root-title"),
        Location::Local(path) => cells::sanitize(path.as_os_str().as_bytes()),
        Location::Remote { host, path } => {
            let mut text = host.as_bytes().to_vec();
            if !path.as_bytes().is_empty() {
                text.push(b':');
                text.extend_from_slice(path.as_bytes());
            }
            cells::sanitize(&text)
        }
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

    const HOME: &str = "/home/me";

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

    fn host(alias: &str, label: Option<&str>, address: Option<&str>) -> RootHost {
        RootHost {
            alias: alias.to_owned(),
            label: label.map(str::to_owned),
            address: address.map(str::to_owned),
        }
    }

    /// Hosts in config order, which the root keeps.
    fn hosts() -> Vec<RootHost> {
        vec![
            host("web", Some("Prod"), Some("deploy@10.0.0.5")),
            host("db", None, None),
            host("staging", None, Some("ubuntu@stg.example.org:2222")),
        ]
    }

    fn local(path: &str) -> Location {
        Location::Local(PathBuf::from(path))
    }

    fn remote(host: &str, path: &str) -> Location {
        Location::Remote {
            host: host.to_owned(),
            path: RemotePath::from(path),
        }
    }

    /// Answers `request` with `listing` from the location it asked for.
    fn answer(panel: &mut Panel, request: &ListRequest, listing: Listing) {
        let location = request.location.clone();
        panel.listed(request.generation, Ok(Listed { location, listing }));
    }

    /// A panel on `location` whose first listing arrived.
    fn loaded_at(location: Location, listing: Listing) -> Panel {
        let (mut panel, request) = Panel::new(location, PathBuf::from(HOME), true);
        answer(&mut panel, &request, listing);
        panel
    }

    fn loaded(path: &str, entries: Vec<DirEntry>) -> Panel {
        loaded_at(local(path), Listing::Dir(entries))
    }

    fn root() -> Panel {
        loaded_at(Location::Root, Listing::Root(hosts()))
    }

    fn names(panel: &Panel) -> Vec<String> {
        (0..panel.rows())
            .map(|row| match panel.row(row) {
                Some(Row::Parent) => "..".to_owned(),
                Some(Row::Entry(entry)) => entry.display_name().into_owned(),
                Some(Row::Local) => "<local>".to_owned(),
                Some(Row::Host(host)) => host.alias.clone(),
                None => unreachable!(),
            })
            .collect()
    }

    fn under_cursor(panel: &Panel) -> String {
        names(panel)[panel.cursor].clone()
    }

    fn render(
        panel: &mut Panel,
        width: u16,
        height: u16,
        active: bool,
        hosts: &dyn Fn(&str) -> HostState,
    ) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(NOW);
        terminal
            .draw(|frame| {
                let area = frame.area();
                panel.render(frame, area, active, hosts, now, &TimeZone::UTC);
            })
            .unwrap();
        terminal.backend().clone()
    }

    fn draw(panel: &mut Panel, width: u16, height: u16, active: bool) -> TestBackend {
        render(panel, width, height, active, &|_| HostState::default())
    }

    fn draw_connecting(panel: &mut Panel, width: u16, height: u16) -> TestBackend {
        let connecting = |_: &str| HostState {
            status: HostStatus::Connecting,
            address: None,
        };
        render(panel, width, height, true, &connecting)
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
    }

    /// Entries whose names, extensions, times, and sizes all sort differently.
    fn varied() -> Vec<DirEntry> {
        let at = |seconds: u64| Some(UNIX_EPOCH + Duration::from_secs(NOW - 1000 + seconds));
        let file = |name: &str, size: u64, seconds: u64| DirEntry {
            metadata: Metadata {
                modified: at(seconds),
                ..entry(name, FileKind::File, size).metadata
            },
            ..entry(name, FileKind::File, size)
        };
        let dir = |name: &str, seconds: u64| DirEntry {
            metadata: Metadata {
                modified: at(seconds),
                ..entry(name, FileKind::Dir, 4096).metadata
            },
            ..entry(name, FileKind::Dir, 4096)
        };
        vec![
            file("c.txt", 20, 2),
            dir("dir1", 0),
            file("b.md", 10, 1),
            file("Z", 5, 5),
            dir("Adir", 4),
            file("a.txt", 30, 3),
            file(".env", 1, 6),
        ]
    }

    #[test]
    fn sorts_by_name_extension_time_and_size_and_reverses() {
        let mut panel = loaded("/srv", varied());
        let order = |panel: &Panel| names(panel)[1..].join(" ");
        assert_eq!(order(&panel), "Adir dir1 .env a.txt b.md c.txt Z");
        let header = |panel: &mut Panel| draw(panel, 60, 6, true).to_string();
        assert!(header(&mut panel).contains("↑Name"));

        panel.handle(Action::End);
        assert_eq!(panel.handle(Action::SortByExtension), None);
        assert_eq!(order(&panel), "Adir dir1 .env Z b.md a.txt c.txt");
        assert_eq!(under_cursor(&panel), "Z", "the cursor stays on its entry");
        assert!(header(&mut panel).contains("↑Name, by extension"));
        panel.handle(Action::SortByExtension);
        assert_eq!(order(&panel), "dir1 Adir c.txt a.txt b.md Z .env");

        panel.handle(Action::SortByTime);
        assert_eq!(
            order(&panel),
            "Adir dir1 .env Z a.txt c.txt b.md",
            "newest first"
        );
        assert!(header(&mut panel).contains("↓Modify time"));
        panel.handle(Action::SortBySize);
        assert_eq!(
            order(&panel),
            "dir1 Adir a.txt c.txt b.md Z .env",
            "largest first"
        );
        assert!(header(&mut panel).contains("↓Size"));
        panel.handle(Action::SortBySize);
        assert_eq!(order(&panel), "Adir dir1 .env Z b.md c.txt a.txt");
        panel.handle(Action::SortByName);
        assert_eq!(order(&panel), "Adir dir1 .env a.txt b.md c.txt Z");

        // The order holds for the next directory, and the root keeps the config order.
        panel.handle(Action::SortByTime);
        let request = panel.handle(Action::Reload).unwrap();
        answer(&mut panel, &request, Listing::Dir(varied()));
        assert_eq!(order(&panel), "Adir dir1 .env Z a.txt c.txt b.md");
        let mut root = root();
        root.handle(Action::SortBySize);
        assert_eq!(names(&root), ["<local>", "web", "db", "staging"]);
    }

    #[test]
    fn hidden_files_come_and_go_and_the_cursor_stays() {
        let (mut panel, request) = Panel::new(local("/srv"), PathBuf::from(HOME), false);
        answer(&mut panel, &request, Listing::Dir(varied()));
        assert_eq!(names(&panel)[1..].join(" "), "Adir dir1 a.txt b.md c.txt Z");
        panel.handle(Action::End);
        panel.set_show_hidden(true);
        assert_eq!(names(&panel).len(), 8);
        assert_eq!(under_cursor(&panel), "Z", "still on its entry");

        panel.handle(Action::Home);
        for _ in 0..3 {
            panel.handle(Action::Down);
        }
        assert_eq!(under_cursor(&panel), ".env");
        panel.set_show_hidden(false);
        assert_eq!(
            under_cursor(&panel),
            "a.txt",
            "the entry is gone; the row stays"
        );
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

        let mut empty = loaded("/srv", Vec::new());
        for action in [Action::Down, Action::End, Action::PageDown] {
            assert_eq!(empty.handle(action), None);
            assert_eq!(empty.cursor, 0);
        }
    }

    #[test]
    fn enters_directories_and_links_to_them_but_not_files() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, local("/srv/alpha-link"));
        assert_eq!(request.generation, 2);
        let inside = vec![entry("inside", FileKind::File, 1)];
        answer(&mut panel, &request, Listing::Dir(inside));
        assert_eq!(panel.location, local("/srv/alpha-link"));
        assert_eq!(names(&panel), ["..", "inside"]);

        panel.handle(Action::End);
        assert_eq!(panel.handle(Action::Enter), None, "a file does not open");
    }

    #[test]
    fn going_up_puts_the_cursor_on_the_directory_left() {
        let mut panel = loaded("/srv/bin", vec![entry("tool", FileKind::File, 1)]);
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, local("/srv"));
        answer(&mut panel, &request, Listing::Dir(listing()));
        assert_eq!(under_cursor(&panel), "bin");

        // Enter on `..` does the same.
        panel.handle(Action::Home);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, local("/"));
        let srv = vec![entry("srv", FileKind::Dir, 1)];
        answer(&mut panel, &request, Listing::Dir(srv));
        assert_eq!(under_cursor(&panel), "srv");
    }

    #[test]
    fn above_slash_is_the_root_with_local_first_and_hosts_in_config_order() {
        let mut panel = loaded("/", vec![entry("srv", FileKind::Dir, 1)]);
        assert_eq!(names(&panel), ["..", "srv"]);
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, Location::Root);
        answer(&mut panel, &request, Listing::Root(hosts()));
        assert_eq!(names(&panel), ["<local>", "web", "db", "staging"]);
        assert_eq!(under_cursor(&panel), "<local>", "the file system just left");
        assert_eq!(panel.handle(Action::Parent), None, "the root is the top");

        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(
            request.location,
            local(HOME),
            "the local file system opens at home"
        );
    }

    #[test]
    fn hosts_open_their_home_directory_and_lead_back_to_themselves() {
        let mut panel = root();
        panel.handle(Action::End);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, remote("staging", ""));
        let refused = "deploy@stg: Permission denied (publickey).".to_owned();
        panel.listed(request.generation, Err(refused));
        assert_eq!(panel.location, Location::Root);
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open staging: deploy@stg: Permission denied (publickey).")
        );

        // The reply names the start directory, which `..` then leads up from.
        let request = panel.handle(Action::Enter).unwrap();
        let home = remote("staging", "/home/ubuntu");
        let reply = Listed {
            location: home.clone(),
            listing: Listing::Dir(Vec::new()),
        };
        panel.listed(request.generation, Ok(reply));
        assert_eq!(panel.location, home);
        assert!(
            draw(&mut panel, 40, 6, true)
                .to_string()
                .contains("staging:/home/ubuntu")
        );
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, remote("staging", "/home"));

        let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(Vec::new()));
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, remote("db", "/"));
        answer(&mut panel, &request, Listing::Dir(listing()));
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, Location::Root);
        answer(&mut panel, &request, Listing::Root(hosts()));
        assert_eq!(under_cursor(&panel), "db");
    }

    #[test]
    fn reload_keeps_the_cursor_on_its_entry() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        let request = panel.handle(Action::Reload).unwrap();
        assert_eq!(request.location, local("/srv"));
        let mut changed = listing();
        changed.push(entry("new.txt", FileKind::File, 1));
        answer(&mut panel, &request, Listing::Dir(changed));
        assert_eq!(under_cursor(&panel), "zeta.txt");

        let request = panel.handle(Action::Reload).unwrap();
        let other = vec![entry("other", FileKind::File, 1)];
        answer(&mut panel, &request, Listing::Dir(other));
        assert_eq!(panel.cursor, 0, "the entry is gone");

        let mut root = root();
        root.handle(Action::Down);
        root.handle(Action::Down);
        let request = root.handle(Action::Reload).unwrap();
        assert_eq!(request.location, Location::Root);
        let mut reordered = hosts();
        reordered.reverse();
        answer(&mut root, &request, Listing::Root(reordered));
        assert_eq!(under_cursor(&root), "db");
    }

    #[test]
    fn drops_stale_replies() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let first = panel.handle(Action::Enter).unwrap();
        panel.handle(Action::Home);
        let second = panel.handle(Action::Enter).unwrap();
        let stale = vec![entry("stale", FileKind::File, 1)];
        answer(&mut panel, &first, Listing::Dir(stale));
        assert_eq!(
            panel.location,
            local("/srv"),
            "an older request was answered"
        );
        answer(&mut panel, &second, Listing::Dir(listing()));
        assert_eq!(panel.location, local("/"));
        answer(&mut panel, &second, Listing::Dir(Vec::new()));
        assert_eq!(panel.rows(), 1 + listing().len(), "a reply counts once");
    }

    #[test]
    fn a_failed_listing_keeps_the_directory_and_reports_why() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        panel.listed(request.generation, Err("permission denied".to_owned()));
        assert_eq!(panel.location, local("/srv"));
        assert_eq!(under_cursor(&panel), "alpha-link");
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open alpha-link: permission denied")
        );
        // The parent is not inside the directory shown, so it gets its full path.
        let request = panel.handle(Action::Parent).unwrap();
        panel.listed(
            request.generation,
            Err("no such file or directory".to_owned()),
        );
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open /: no such file or directory")
        );
        // The next request clears it.
        panel.handle(Action::Reload);
        assert_eq!(panel.error, None);

        let request = panel.handle(Action::Reload).unwrap();
        panel.listed(request.generation, Err("bad\x1b[2Jthing".to_owned()));
        assert_eq!(
            panel.error.as_deref(),
            Some("Cannot open /srv: bad?[2Jthing"),
            "reasons are shown terminal-safe"
        );
    }

    #[test]
    fn a_cancelled_request_is_forgotten() {
        let mut panel = root();
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(panel.pending_request(), Some(request.clone()));
        let status = draw(&mut panel, 40, 6, true).to_string();
        assert!(status.contains("Loading…"), "{status}");
        let status = draw_connecting(&mut panel, 40, 6).to_string();
        assert!(status.contains("Connecting to web…"), "{status}");

        panel.cancel();
        assert_eq!(panel.pending_request(), None);
        answer(&mut panel, &request, Listing::Dir(listing()));
        assert_eq!(panel.location, Location::Root, "the late reply is dropped");
        assert_eq!(under_cursor(&panel), "web");
    }

    #[test]
    fn a_lost_host_sends_its_panels_back_to_the_root() {
        let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
        assert_eq!(panel.leave_host("web", Some("gone")), None, "another host");
        let request = panel.leave_host("db", Some("Connection reset")).unwrap();
        assert_eq!(request.location, Location::Root);
        answer(&mut panel, &request, Listing::Root(hosts()));
        assert_eq!(under_cursor(&panel), "db");
        assert_eq!(
            panel.error.as_deref(),
            Some("Lost the connection to db: Connection reset")
        );

        // A panel that was about to open the host goes back as well.
        let mut panel = root();
        panel.handle(Action::Down);
        panel.handle(Action::Enter);
        assert!(panel.leave_host("web", None).is_some());
        assert_eq!(panel.error, None, "a disconnect needs no reason");
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
    fn draws_the_root_with_labels_and_cached_addresses() {
        let mut panel = root();
        panel.handle(Action::Down);
        assert_eq!(panel.host_under_cursor(), Some("web"));
        insta::assert_snapshot!(draw(&mut panel, 50, 9, true));

        let narrow = draw(&mut panel, 20, 9, true).to_string();
        assert!(!narrow.contains("deploy"), "{narrow}");
        assert!(narrow.contains("Prod"), "{narrow}");
    }

    #[test]
    fn the_root_marks_connection_states_and_prefers_fresh_addresses() {
        let mut panel = root();
        assert!(panel.shows_root());
        assert_eq!(panel.host_under_cursor(), None, "[Local]");
        let hosts = |alias: &str| match alias {
            "web" => HostState {
                status: HostStatus::Connected,
                address: Some("deploy@10.0.0.9".to_owned()),
            },
            "db" => HostState {
                status: HostStatus::Failed,
                address: None,
            },
            _ => HostState {
                status: HostStatus::Connecting,
                address: None,
            },
        };
        insta::assert_snapshot!(render(&mut panel, 50, 9, false, &hosts));
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
