//! A panel that lists the virtual root, the SFTP hosts, or a directory.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;
use std::time::SystemTime;

use jiff::tz::TimeZone;
use noc_vfs::{DirEntry, Location, RemotePath, Space, Volume};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;

use super::cells::{self, Align, MTIME_WIDTH};
use super::decor::Decor;
use super::keymap::Action;
use super::root::{RootHost, volume_name, volume_of};
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the size column, as in mc.
const SIZE_WIDTH: usize = 7;

/// Narrow panels drop columns to keep at least this many cells for names.
const MIN_NAME_WIDTH: usize = 8;

/// Cells of the free space and size of a volume, such as `212G`; the column is wider.
const VOLUME_SIZE_WIDTH: usize = 5;

/// A request to list `location` for a panel; the reply must carry `generation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListRequest {
    pub(crate) generation: u64,
    pub(crate) location: Location,
    /// Where a host opens, if the location is the host itself (the empty path) and the
    /// directory is still there: the last one shown on it, for `remember_dir`.
    pub(crate) resume: Option<RemotePath>,
}

/// The reply to a [`ListRequest`]: where the listing is from, which may be more exact than
/// the request (a host's start directory as a path), and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) location: Location,
    pub(crate) listing: Listing,
    /// The space of the file system that holds a directory; `None` for the virtual root and the
    /// hosts, and when unknown.
    pub(crate) space: Option<Space>,
}

/// What a location holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Listing {
    /// The virtual root: the home directory, the mounted volumes, the system volume first, then
    /// the hosts, which it shows as one row, with the connected ones again below it.
    Root {
        volumes: Vec<Volume>,
        hosts: Vec<RootHost>,
    },
    /// The SFTP hosts, in config order.
    Hosts(Vec<RootHost>),
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

/// What drawing a panel needs from the app.
pub(crate) struct View<'a> {
    /// What the app knows about a host.
    pub(crate) hosts: &'a dyn Fn(&str) -> HostState,
    pub(crate) decor: Decor,
    pub(crate) theme: &'a Theme,
    /// The title of the virtual root: the name of this machine.
    pub(crate) root_title: &'a str,
    /// Turns spinners.
    pub(crate) tick: u64,
    pub(crate) now: SystemTime,
    pub(crate) tz: &'a TimeZone,
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
    /// The first row: `..`, or the home directory in the virtual root.
    First,
    /// The entry with this name, or the first row if it is gone.
    Name(Vec<u8>),
    /// The entry with this name, or, if it is gone, the row it was on (or the last one), so
    /// that the cursor stays where it was when an entry is deleted.
    Near { name: Vec<u8>, row: usize },
    /// The host with this alias, or the first row if it is gone.
    Host(String),
    /// The volume mounted here, or the first row if it is gone.
    Volume(PathBuf),
    /// The row of the SFTP hosts in the virtual root.
    Sftp,
    /// The home directory in the virtual root.
    Home,
}

impl Focus {
    fn matches(&self, row: Row<'_>) -> bool {
        match (self, row) {
            (Self::Name(name) | Self::Near { name, .. }, Row::Entry(entry)) => entry.name == *name,
            (Self::Host(alias), Row::Host(host)) => host.alias == *alias,
            (Self::Volume(path), Row::Volume(volume)) => volume.mount_point == *path,
            (Self::Sftp, Row::Sftp) | (Self::Home, Row::Home) => true,
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

/// Where a panel can go: a location, and the row to put the cursor on there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Destination {
    location: Location,
    focus: Focus,
}

impl Destination {
    /// `location`, with the cursor on its first row.
    pub(crate) fn to(location: Location) -> Self {
        Self {
            location,
            focus: Focus::First,
        }
    }
}

/// A row of the listing.
#[derive(Debug, Clone, Copy)]
enum Row<'a> {
    /// `..`: the parent directory, or the virtual root from `/` and from the hosts.
    Parent,
    Entry(&'a DirEntry),
    /// The home directory, first in the virtual root.
    Home,
    /// A mounted volume in the virtual root.
    Volume(&'a Volume),
    /// The SFTP hosts, as one row of the virtual root.
    Sftp,
    Host(&'a RootHost),
}

/// One listing with a cursor.
#[derive(Debug)]
pub(crate) struct Panel {
    /// What is shown; a requested location replaces it once its listing arrives.
    location: Location,
    /// Directories first, then as `sort` says; hosts in config order.
    listing: Listing,
    /// The space of the file system that holds the directory shown, on the bottom of the frame.
    space: Option<Space>,
    /// The entries of a directory listing that are shown, in order: hidden files may be left
    /// out. In the virtual root, the hosts shown below its row of hosts.
    shown: Vec<usize>,
    /// Hosts that are connected or connecting, which the virtual root shows again.
    connected: HashSet<String>,
    sort: Sort,
    show_hidden: bool,
    /// Names of the marked entries; only shown ones, and never `..`.
    marked: HashSet<Vec<u8>>,
    /// The home directory: the first row of the virtual root, and `~` in dialogs.
    home: PathBuf,
    /// Row under the cursor; row 0 is `..`, or the home directory in the virtual root.
    cursor: usize,
    /// First row on screen.
    offset: usize,
    /// Rows on screen at the last render.
    page: usize,
    generation: u64,
    pending: Option<Pending>,
    /// What quick search has matched so far, while it runs.
    search: Option<String>,
    error: Option<String>,
}

impl Panel {
    /// A panel for `location`, and the request for its first listing. The virtual root opens
    /// `home`, which `~` in dialogs stands for too; `show_hidden` shows names that start with a
    /// dot.
    pub(crate) fn new(location: Location, home: PathBuf, show_hidden: bool) -> (Self, ListRequest) {
        let listing = match location {
            Location::Root => Listing::Root {
                volumes: Vec::new(),
                hosts: Vec::new(),
            },
            Location::Sftp => Listing::Hosts(Vec::new()),
            Location::Local(_) | Location::Remote { .. } => Listing::Dir(Vec::new()),
        };
        let mut panel = Self {
            location: location.clone(),
            listing,
            space: None,
            shown: Vec::new(),
            connected: HashSet::new(),
            sort: Sort {
                key: SortKey::Name,
                descending: false,
            },
            show_hidden,
            marked: HashSet::new(),
            home,
            cursor: 0,
            offset: 0,
            page: 1,
            generation: 0,
            pending: None,
            search: None,
            error: None,
        };
        let request = panel.open(location, Focus::First);
        (panel, request)
    }

    fn open(&mut self, location: Location, focus: Focus) -> ListRequest {
        self.generation += 1;
        self.error = None;
        self.search = None;
        self.pending = Some(Pending {
            generation: self.generation,
            location: location.clone(),
            focus,
        });
        ListRequest {
            generation: self.generation,
            location,
            resume: None,
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
            Ok(Listed {
                location,
                listing,
                space,
            }) => {
                // Reading the same directory again keeps the marks on names still there.
                if location != self.location {
                    self.marked.clear();
                }
                self.location = location;
                self.listing = listing;
                self.space = space;
                self.arrange();
                self.offset = 0;
                let rows = self.rows();
                self.cursor = (0..rows)
                    .find(|&index| {
                        self.row(index)
                            .is_some_and(|row| pending.focus.matches(row))
                    })
                    .unwrap_or(match pending.focus {
                        Focus::Near { row, .. } => row.min(rows.saturating_sub(1)),
                        _ => 0,
                    });
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
            resume: None,
        })
    }

    /// Stops waiting for a listing; its reply will be dropped.
    pub(crate) fn cancel(&mut self) {
        self.pending = None;
    }

    /// Leaves `host` for the list of hosts, with the cursor on it and the `reason` of a lost
    /// connection below the listing, if the panel shows or waits for that host.
    pub(crate) fn leave_host(&mut self, host: &str, reason: Option<&str>) -> Option<ListRequest> {
        let on_host = |location: &Location| matches!(location, Location::Remote { host: shown, .. } if shown == host);
        let pending = self.pending.as_ref().map(|pending| &pending.location);
        if !on_host(&self.location) && !pending.is_some_and(on_host) {
            return None;
        }
        let request = self.open(Location::Sftp, Focus::Host(host.to_owned()));
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

    /// Takes the hosts that are connected or connecting, which the virtual root shows below
    /// its row of hosts. The cursor stays on its row's volume or host, or on its row.
    pub(crate) fn set_connected(&mut self, connected: HashSet<String>) {
        if self.connected != connected {
            self.connected = connected;
            self.rearrange();
        }
    }

    /// Sorts and filters a listing again, keeping the cursor on its entry, or on its row if the
    /// entry is no longer shown.
    fn rearrange(&mut self) {
        let focus = self.focus();
        self.arrange();
        let rows = self.rows();
        self.cursor = (0..rows)
            .find(|&row| self.row(row).is_some_and(|row| focus.matches(row)))
            .unwrap_or_else(|| self.cursor.min(rows.saturating_sub(1)));
    }

    /// Sorts a directory listing and picks the entries to show, or the connected hosts in the
    /// virtual root; entries that are not shown lose their marks.
    fn arrange(&mut self) {
        let entries = match &mut self.listing {
            Listing::Dir(entries) => entries,
            Listing::Root { hosts, .. } => {
                let connected = &self.connected;
                self.shown = (0..hosts.len())
                    .filter(|&index| connected.contains(&hosts[index].alias))
                    .collect();
                self.marked.clear();
                return;
            }
            Listing::Hosts(_) => {
                self.shown.clear();
                self.marked.clear();
                return;
            }
        };
        sort(entries, self.sort);
        let show_hidden = self.show_hidden;
        self.shown = (0..entries.len())
            .filter(|&index| show_hidden || !entries[index].name.starts_with(b"."))
            .collect();
        if !self.marked.is_empty() {
            let shown: HashSet<&[u8]> = self
                .shown
                .iter()
                .map(|&index| entries[index].name.as_slice())
                .collect();
            self.marked.retain(|name| shown.contains(name.as_slice()));
        }
    }

    /// Marks the entry under the cursor, or unmarks it; `..`, volumes, and hosts cannot be
    /// marked.
    fn toggle_mark(&mut self) {
        if let Some(Row::Entry(entry)) = self.row(self.cursor) {
            let name = entry.name.clone();
            if !self.marked.remove(&name) {
                self.marked.insert(name);
            }
        }
    }

    /// Inverts the marks on the shown entries that are not directories, as mc does by default.
    fn invert_marks(&mut self) {
        let Listing::Dir(entries) = &self.listing else {
            return;
        };
        for &index in &self.shown {
            let entry = &entries[index];
            if !entry.is_dir_like() && !self.marked.remove(&entry.name) {
                self.marked.insert(entry.name.clone());
            }
        }
    }

    /// Marks, or unmarks, the shown entries for which `test` holds.
    pub(crate) fn mark_where(&mut self, mark: bool, test: impl Fn(&DirEntry) -> bool) {
        let Listing::Dir(entries) = &self.listing else {
            return;
        };
        for &index in &self.shown {
            let entry = &entries[index];
            if !test(entry) {
                continue;
            }
            if mark {
                self.marked.insert(entry.name.clone());
            } else {
                self.marked.remove(&entry.name);
            }
        }
    }

    /// How many entries are marked, and the bytes in the files among them.
    fn marked_total(&self) -> (usize, u64) {
        let Listing::Dir(entries) = &self.listing else {
            return (0, 0);
        };
        let bytes = entries
            .iter()
            .filter(|entry| !entry.is_dir_like() && self.marked.contains(&entry.name))
            .filter_map(|entry| entry.metadata.size)
            .sum();
        (self.marked.len(), bytes)
    }

    /// Whether quick search runs.
    pub(crate) fn searching(&self) -> bool {
        self.search.is_some()
    }

    /// Starts quick search, or jumps to the next match while it runs.
    pub(crate) fn search_next(&mut self) {
        match &self.search {
            None => self.search = Some(String::new()),
            Some(text) => {
                let text = text.clone();
                if let Some(row) = self.find(&text, self.cursor + 1) {
                    self.cursor = row;
                }
            }
        }
    }

    /// Adds `c` to quick search, starting it if needed, and moves the cursor to the first match
    /// from where it is. As in mc, a character that nothing matches is dropped.
    pub(crate) fn search_type(&mut self, c: char) {
        let mut text = self.search.clone().unwrap_or_default();
        text.extend(c.to_lowercase());
        if let Some(row) = self.find(&text, self.cursor) {
            self.cursor = row;
            self.search = Some(text);
        }
    }

    /// Takes the last character off quick search; the cursor stays.
    pub(crate) fn search_back(&mut self) {
        if let Some(text) = &mut self.search {
            text.pop();
        }
    }

    pub(crate) fn end_search(&mut self) {
        self.search = None;
    }

    /// The first row from `start` on, round to the top, whose name starts with `text` (lower
    /// case). `..` never matches.
    fn find(&self, text: &str, start: usize) -> Option<usize> {
        let rows = self.rows();
        (start..rows).chain(0..start.min(rows)).find(|&index| {
            let name = match self.row(index) {
                Some(Row::Entry(entry)) => entry.display_name().to_lowercase(),
                Some(Row::Host(host)) => {
                    host.label.as_deref().unwrap_or(&host.alias).to_lowercase()
                }
                Some(Row::Volume(volume)) => volume_name(volume).to_lowercase(),
                Some(Row::Home) => fl!("root-home").to_lowercase(),
                Some(Row::Sftp) => fl!("root-sftp").to_lowercase(),
                Some(Row::Parent) | None => return false,
            };
            name.starts_with(text)
        })
    }

    /// Whether the panel shows the virtual root or the list of hosts, which hold no files.
    pub(crate) fn shows_root(&self) -> bool {
        self.location.is_virtual()
    }

    /// What the panel shows.
    pub(crate) fn location(&self) -> &Location {
        &self.location
    }

    /// The entry under the cursor; `None` on `..`, volumes, and hosts.
    pub(crate) fn entry_under_cursor(&self) -> Option<&DirEntry> {
        match self.row(self.cursor)? {
            Row::Entry(entry) => Some(entry),
            Row::Parent | Row::Home | Row::Volume(_) | Row::Sftp | Row::Host(_) => None,
        }
    }

    /// The name of the entry under the cursor; `None` on `..`, volumes, and hosts.
    pub(crate) fn name_under_cursor(&self) -> Option<&[u8]> {
        self.entry_under_cursor().map(|entry| entry.name.as_slice())
    }

    /// Where `text`, typed in a dialog, points from this directory: a name or a relative path,
    /// an absolute path, or `~` or `~/…` for the home directory (the remote one on a host);
    /// `\~` stands for a name that starts with `~`, as in mc. Trailing slashes are dropped.
    /// `None` in the virtual root and the list of hosts.
    pub(crate) fn resolve(&self, text: &str) -> Option<Location> {
        let trimmed = text.trim_end_matches('/');
        let text = if trimmed.is_empty() { text } else { trimmed };
        let home_relative = match text.strip_prefix('~') {
            Some("") => Some(""),
            Some(rest) => rest.strip_prefix('/'),
            None => None,
        };
        let text = text.strip_prefix("\\~").map_or(text, |_| &text[1..]);
        match (&self.location, home_relative) {
            (Location::Root | Location::Sftp, _) => None,
            (Location::Local(_), Some(rest)) => Some(Location::Local(self.home.join(rest))),
            (Location::Remote { host, .. }, Some(rest)) => Some(Location::Remote {
                host: host.clone(),
                // Relative remote paths start at the remote home directory.
                path: RemotePath::from(rest),
            }),
            (location, None) => child(location, text.as_bytes()),
        }
    }

    /// What an operation acts on: the marked entries in the order shown, or else the entry
    /// under the cursor. Nothing on `..`, volumes, and hosts.
    pub(crate) fn chosen(&self) -> Vec<&DirEntry> {
        let Listing::Dir(entries) = &self.listing else {
            return Vec::new();
        };
        if !self.marked.is_empty() {
            return self
                .shown
                .iter()
                .map(|&index| &entries[index])
                .filter(|entry| self.marked.contains(&entry.name))
                .collect();
        }
        match self.row(self.cursor) {
            Some(Row::Entry(entry)) => vec![entry],
            _ => Vec::new(),
        }
    }

    /// Reads the directory again with the cursor on `name`, such as a directory just made.
    pub(crate) fn reload_onto(&mut self, name: Vec<u8>) -> ListRequest {
        self.open(self.location.clone(), Focus::Name(name))
    }

    /// The alias of the host under the cursor, in the virtual root or the list of hosts.
    pub(crate) fn host_under_cursor(&self) -> Option<&str> {
        match self.row(self.cursor)? {
            Row::Host(host) => Some(&host.alias),
            Row::Parent | Row::Entry(_) | Row::Home | Row::Volume(_) | Row::Sftp => None,
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
            // As in mc, the cursor moves on even from `..`, which cannot be marked.
            Action::Mark => {
                self.toggle_mark();
                self.cursor = (self.cursor + 1).min(last);
            }
            Action::MarkUp => {
                self.toggle_mark();
                self.cursor = self.cursor.saturating_sub(1);
            }
            Action::InvertMarks => self.invert_marks(),
            Action::Enter => return self.row_destination().map(|to| self.go(to)),
            Action::Parent => return self.parent_destination().map(|to| self.go(to)),
            Action::SortByName => self.sort_by(SortKey::Name),
            Action::SortByExtension => self.sort_by(SortKey::Extension),
            Action::SortByTime => self.sort_by(SortKey::Time),
            Action::SortBySize => self.sort_by(SortKey::Size),
            Action::Reload => return Some(self.go(self.here())),
            _ => {}
        }
        None
    }

    fn sort_by(&mut self, key: SortKey) {
        self.sort = self.sort.by(key);
        self.rearrange();
    }

    /// Goes to `destination`; returns the listing to request.
    pub(crate) fn go(&mut self, destination: Destination) -> ListRequest {
        self.open(destination.location, destination.focus)
    }

    /// This location, with the cursor on the row it is on.
    pub(crate) fn here(&self) -> Destination {
        Destination {
            location: self.location.clone(),
            focus: self.focus(),
        }
    }

    /// What the cursor is on.
    fn focus(&self) -> Focus {
        match self.row(self.cursor) {
            Some(Row::Entry(entry)) => Focus::Near {
                name: entry.name.clone(),
                row: self.cursor,
            },
            Some(Row::Host(host)) => Focus::Host(host.alias.clone()),
            Some(Row::Volume(volume)) => Focus::Volume(volume.mount_point.clone()),
            Some(Row::Sftp) => Focus::Sftp,
            Some(Row::Home) => Focus::Home,
            Some(Row::Parent) | None => Focus::First,
        }
    }

    /// What Alt-O opens in the other panel, as in mc: the directory or host under the cursor,
    /// or, for a file, the parent directory with the cursor on this one. The cursor moves on
    /// to the next row.
    pub(crate) fn for_other_panel(&mut self) -> Option<Destination> {
        let destination = match self.row(self.cursor)? {
            Row::Entry(entry) if !entry.is_dir_like() => self.parent_destination(),
            _ => self.row_destination(),
        };
        self.cursor = (self.cursor + 1).min(self.rows().saturating_sub(1));
        destination
    }

    /// Where Enter on the row under the cursor leads: into a directory, the home directory, a
    /// volume, or a host, to the list of hosts, or up from `..`; nowhere from a file. Volumes
    /// open at their mount points.
    fn row_destination(&self) -> Option<Destination> {
        let location = match self.row(self.cursor)? {
            Row::Parent => return self.parent_destination(),
            Row::Entry(entry) if entry.is_dir_like() => child(&self.location, &entry.name)?,
            Row::Entry(_) => return None,
            Row::Home => Location::Local(self.home.clone()),
            Row::Volume(volume) => Location::Local(volume.mount_point.clone()),
            Row::Sftp => Location::Sftp,
            Row::Host(host) => Location::Remote {
                host: host.alias.clone(),
                path: RemotePath::from(""),
            },
        };
        Some(Destination {
            location,
            focus: Focus::First,
        })
    }

    /// The parent, with the cursor on the directory this is, on its host in the list of hosts,
    /// or in the virtual root on the system volume or the row of hosts. `None` in the virtual
    /// root.
    fn parent_destination(&self) -> Option<Destination> {
        let parent = self.location.parent();
        let focus = match (&self.location, &parent) {
            (Location::Root, _) => return None,
            (Location::Sftp, _) => Focus::Sftp,
            (Location::Local(_), Location::Root) => Focus::Volume(PathBuf::from("/")),
            (Location::Remote { host, .. }, Location::Sftp) => Focus::Host(host.clone()),
            (Location::Local(path), _) => path
                .file_name()
                .map_or(Focus::First, |name| Focus::Name(name.as_bytes().to_vec())),
            (Location::Remote { path, .. }, _) => path
                .file_name()
                .map_or(Focus::First, |name| Focus::Name(name.to_vec())),
        };
        Some(Destination {
            location: parent,
            focus,
        })
    }

    /// The rows: in the virtual root the home directory, the volumes, the row of hosts, and the
    /// connected hosts; elsewhere `..` and the hosts or the entries shown.
    fn rows(&self) -> usize {
        match &self.listing {
            Listing::Root { volumes, .. } => 1 + volumes.len() + 1 + self.shown.len(),
            Listing::Hosts(hosts) => 1 + hosts.len(),
            Listing::Dir(_) => 1 + self.shown.len(),
        }
    }

    fn row(&self, index: usize) -> Option<Row<'_>> {
        match &self.listing {
            Listing::Root { volumes, hosts } => {
                let Some(index) = index.checked_sub(1) else {
                    return Some(Row::Home);
                };
                if let Some(volume) = volumes.get(index) {
                    return Some(Row::Volume(volume));
                }
                match index - volumes.len() {
                    0 => Some(Row::Sftp),
                    shown => hosts.get(*self.shown.get(shown - 1)?).map(Row::Host),
                }
            }
            Listing::Hosts(hosts) => match index.checked_sub(1) {
                None => Some(Row::Parent),
                Some(index) => hosts.get(index).map(Row::Host),
            },
            Listing::Dir(entries) => match index.checked_sub(1) {
                None => Some(Row::Parent),
                Some(index) => entries.get(*self.shown.get(index)?).map(Row::Entry),
            },
        }
    }

    /// Draws the panel: the location in the frame, column headers, the rows, the size and number
    /// of marked entries on the line below them, and a status line with the name under the
    /// cursor, the loading state, or the last error.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        active: bool,
        view: &View<'_>,
    ) {
        let hosts = view.hosts;
        let theme = view.theme;
        let mut title = match self.location {
            Location::Root => cells::sanitize(view.root_title.as_bytes()),
            _ => location_text(&self.location),
        };
        let room = usize::from(area.width.saturating_sub(4));
        if cells::width(&title) > room {
            title = cells::fit(&title, room, Align::Left);
        }
        let title_style = if active {
            theme.panel_title_active
        } else {
            theme.panel_border
        };
        let title = Line::styled(format!(" {title} "), title_style);
        let block = Block::bordered()
            .border_type(theme.border_type())
            .title(title)
            .style(theme.panel)
            .border_style(theme.panel_border);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height < 3 || inner.width < 2 {
            return;
        }
        self.render_space(frame, area, theme);
        let width = usize::from(inner.width);
        let list_height = usize::from(inner.height - 3);
        self.page = list_height;
        self.scroll(list_height);

        let columns = match &self.listing {
            Listing::Dir(_) => Columns::Dir(DirColumns::for_width(width)),
            Listing::Root {
                volumes,
                hosts: listed,
            } => Columns::Root(RootColumns {
                home: volume_of(volumes, &self.home).and_then(|volume| volume.space),
                ..RootColumns::for_width(width, listed.len())
            }),
            Listing::Hosts(listed) => {
                let addresses = listed.iter().map(|host| address(host, hosts));
                Columns::Hosts(HostColumns::for_width(width, addresses))
            }
        };
        let line = |y: u16| Rect::new(inner.x, y, inner.width, 1);
        let header = Line::styled(columns.header(self.sort), theme.header);
        frame.render_widget(header, line(inner.y));
        for (screen_row, index) in (self.offset..self.rows()).take(list_height).enumerate() {
            let Some(row) = self.row(index) else { break };
            let mut text = columns.row(row, view);
            // The cursor and marks replace the colors of the row, as in mc.
            let marked = matches!(row, Row::Entry(entry) if self.marked.contains(&entry.name));
            let style = match (active && index == self.cursor, marked) {
                (true, true) => Some(theme.marked_cursor),
                (true, false) => Some(theme.cursor),
                (false, true) => Some(theme.marked),
                (false, false) => None,
            };
            if let Some(style) = style {
                for span in &mut text.spans {
                    span.style = Style::new();
                }
                text = text.style(style);
            }
            let y = inner.y + 1 + u16::try_from(screen_row).unwrap_or(u16::MAX);
            frame.render_widget(text, line(y));
        }

        self.render_separator(frame, area, inner.bottom() - 2, theme);
        let mut status_style = Style::new();
        let status = if let Some(text) = &self.search {
            status_style = theme.quick_search;
            let status = fl!("panel-search", text = cells::sanitize(text.as_bytes()));
            if active {
                let column = u16::try_from(cells::width(&status)).unwrap_or(u16::MAX);
                let x = inner.x.saturating_add(column).min(inner.right() - 1);
                frame.set_cursor_position((x, inner.bottom() - 1));
            }
            status
        } else if let Some(error) = &self.error {
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
                Some(Row::Volume(volume)) => Self::volume_status(volume),
                Some(Row::Home) => cells::sanitize(self.home.as_os_str().as_bytes()),
                Some(Row::Sftp) => fl!("root-sftp-status"),
                Some(Row::Host(host)) => cells::sanitize(host.alias.as_bytes()),
                None => String::new(),
            }
        };
        let status = cells::fit(&status, width, Align::Left);
        frame.render_widget(Line::styled(status, status_style), line(inner.bottom() - 1));
    }

    /// The status line on a volume: its mount point, and its file system.
    fn volume_status(volume: &Volume) -> String {
        let path = cells::sanitize(volume.mount_point.as_os_str().as_bytes());
        match &volume.fs_type {
            Some(fs_type) => format!("{path}  {}", cells::sanitize(fs_type.as_bytes())),
            None => path,
        }
    }

    /// The line between the listing and the status line, across the panel's frame, with the
    /// total of the marked entries in the middle.
    fn render_separator(&self, frame: &mut Frame<'_>, area: Rect, y: u16, theme: &Theme) {
        let inside = area.width.saturating_sub(2);
        let (left, right) = theme.tees();
        let separator = format!("{left}{}{right}", "─".repeat(usize::from(inside)));
        frame.render_widget(
            Line::styled(separator, theme.panel_border),
            Rect::new(area.x, y, area.width, 1),
        );
        let (count, bytes) = self.marked_total();
        if count == 0 {
            return;
        }
        let total = fl!("panel-marked", size = cells::grouped(bytes), count = count);
        let mut total = format!(" {total} ");
        if cells::width(&total) > usize::from(inside) {
            total = cells::fit(&total, usize::from(inside), Align::Left);
        }
        let total_width = u16::try_from(cells::width(&total)).unwrap_or(u16::MAX);
        let x = area.x + area.width.saturating_sub(total_width) / 2;
        frame.render_widget(
            // An underline would run along the frame.
            Line::styled(total, theme.marked.not_underlined()),
            Rect::new(x, y, total_width.min(inside), 1),
        );
    }

    /// The free space and size of the directory's file system, on the bottom of the frame at
    /// the right, as in mc; left out when unknown or when it does not fit.
    fn render_space(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        // Pseudo file systems such as devfs report nothing.
        let Some(space) = self.space.filter(|space| space.total > 0) else {
            return;
        };
        let percent = u128::from(space.available.min(space.total)) * 100 / u128::from(space.total);
        let text = fl!(
            "panel-space",
            free = cells::size(space.available, VOLUME_SIZE_WIDTH),
            total = cells::size(space.total, VOLUME_SIZE_WIDTH),
            percent = u64::try_from(percent).unwrap_or(100)
        );
        let text = format!(" {text} ");
        let width = u16::try_from(cells::width(&text)).unwrap_or(u16::MAX);
        // A corner and a line on either side.
        if width.saturating_add(4) > area.width {
            return;
        }
        let x = area.right() - 2 - width;
        frame.render_widget(
            Line::styled(text, theme.panel),
            Rect::new(x, area.bottom() - 1, width, 1),
        );
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

/// How rows become text: the columns of a directory, of the virtual root, or of the hosts.
#[derive(Debug, Clone, Copy)]
enum Columns {
    Dir(DirColumns),
    Root(RootColumns),
    Hosts(HostColumns),
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
                let (name, rest) = columns.join(&name, &size, &time, [Align::Center; 3]);
                name + &rest
            }
            Self::Root(columns) => {
                let (name, rest) = columns.join(
                    &fl!("panel-name"),
                    &fl!("root-free"),
                    &fl!("panel-size"),
                    [Align::Center; 2],
                );
                name + &rest
            }
            Self::Hosts(columns) => {
                let (name, rest) =
                    columns.join(&fl!("panel-name"), &fl!("root-address"), [Align::Center; 2]);
                name + &rest
            }
        }
    }

    /// A row, with the name in the color of its kind, as mc highlights files.
    fn row(self, row: Row<'_>, view: &View<'_>) -> Line<'static> {
        const DIR: [Align; 3] = [Align::Left, Align::Right, Align::Left];
        let (decor, theme) = (view.decor, view.theme);
        let styled = |(name, rest): (String, String), prefix: &str, style: Style| {
            let mut spans = icon_spans(name, prefix, style, view);
            spans.push(Span::raw(rest));
            Line::from(spans)
        };
        match (self, row) {
            (Self::Dir(columns), Row::Parent) => {
                let prefix = decor.parent();
                let name = format!("{prefix}..");
                styled(
                    columns.join(&name, &fl!("panel-up-dir"), "", DIR),
                    prefix,
                    theme.directory,
                )
            }
            (Self::Dir(columns), Row::Entry(entry)) => {
                let size = if entry.is_dir_like() {
                    fl!("panel-dir")
                } else {
                    entry
                        .metadata
                        .size
                        .map_or_else(String::new, |size| cells::size(size, SIZE_WIDTH))
                };
                let time = cells::mtime(entry.metadata.modified, view.now, view.tz);
                let prefix = decor.entry(entry);
                let name = format!("{prefix}{}", cells::sanitize(&entry.name));
                styled(
                    columns.join(&name, &size, &time, DIR),
                    &prefix,
                    theme.entry(entry),
                )
            }
            (Self::Root(columns), Row::Home) => {
                let prefix = decor.home();
                let name = format!("{prefix}{}", fl!("root-home"));
                let (free, size) = space_cells(columns.home);
                styled(
                    columns.join(&name, &free, &size, [Align::Left, Align::Right]),
                    prefix,
                    theme.directory,
                )
            }
            (Self::Root(columns), Row::Volume(volume)) => {
                let prefix = decor.volume(volume.kind);
                let name = format!("{prefix}{}", volume_name(volume));
                let (free, size) = space_cells(volume.space);
                styled(
                    columns.join(&name, &free, &size, [Align::Left, Align::Right]),
                    prefix,
                    theme.directory,
                )
            }
            (Self::Root(columns), Row::Sftp) => {
                let prefix = decor.sftp();
                let name = format!("{prefix}{}", fl!("root-sftp"));
                let count = fl!("root-sftp-hosts", count = columns.hosts);
                styled(
                    columns.join_wide(&name, &count, Align::Right),
                    prefix,
                    theme.directory,
                )
            }
            (Self::Root(columns), Row::Host(host)) => host_line(host, view, |name, address| {
                columns.join_wide(name, address, Align::Left)
            }),
            (Self::Hosts(columns), Row::Parent) => {
                let prefix = decor.parent();
                let name = format!("{prefix}..");
                styled(
                    columns.join(&name, &fl!("panel-up-dir"), [Align::Left; 2]),
                    prefix,
                    theme.directory,
                )
            }
            (Self::Hosts(columns), Row::Host(host)) => host_line(host, view, |name, address| {
                columns.join(name, address, [Align::Left; 2])
            }),
            // A listing has only rows of its own kind.
            (Self::Dir(_), Row::Home | Row::Volume(_) | Row::Sftp | Row::Host(_))
            | (Self::Root(_), Row::Parent | Row::Entry(_))
            | (Self::Hosts(_), Row::Entry(_) | Row::Home | Row::Volume(_) | Row::Sftp) => {
                Line::default()
            }
        }
    }
}

/// The free space and size of a volume as the root shows them, or `?` when unknown.
fn space_cells(space: Option<Space>) -> (String, String) {
    space.map_or_else(
        || ("?".to_owned(), "?".to_owned()),
        |space| {
            (
                cells::size(space.available, VOLUME_SIZE_WIDTH),
                cells::size(space.total, VOLUME_SIZE_WIDTH),
            )
        },
    )
}

/// The row of a host: its status marker in a color of its own, then its name, and its address
/// in the cells `join` makes.
fn host_line(
    host: &RootHost,
    view: &View<'_>,
    join: impl FnOnce(&str, &str) -> (String, String),
) -> Line<'static> {
    let name = host.label.as_deref().unwrap_or(&host.alias);
    let name = cells::sanitize(name.as_bytes());
    let status = (view.hosts)(&host.alias).status;
    let prefix = view.decor.host(status, view.tick);
    let (name, rest) = join(&format!("{prefix}{name}"), &address(host, view.hosts));
    // The icon or marker that leads the name cell tells the status by its color.
    let marker_len = name.chars().next().map_or(0, char::len_utf8);
    let (marker, name) = name.split_at(marker_len);
    Line::from(vec![
        Span::styled(marker.to_owned(), view.theme.host_status(status)),
        Span::raw(name.to_owned()),
        Span::styled(rest, view.theme.address),
    ])
}

/// A name cell in `style`, with the icon of its `prefix` in a toned-down `style`. A cell cut so
/// short that the prefix lost its end, and mc's markers, are drawn whole in `style`.
fn icon_spans(name: String, prefix: &str, style: Style, view: &View<'_>) -> Vec<Span<'static>> {
    match name.strip_prefix(prefix) {
        Some(rest) if view.decor.icons() && !prefix.is_empty() => vec![
            Span::styled(prefix.to_owned(), Theme::icon(style)),
            Span::styled(rest.to_owned(), style),
        ],
        _ => vec![Span::styled(name, style)],
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

    /// The name cell, and the other cells with their separators.
    fn join(self, name: &str, size: &str, time: &str, align: [Align; 3]) -> (String, String) {
        let mut rest = String::new();
        if self.size {
            rest.push('│');
            rest.push_str(&cells::fit(size, SIZE_WIDTH, align[1]));
        }
        if self.time {
            rest.push('│');
            rest.push_str(&cells::fit(time, MTIME_WIDTH, align[2]));
        }
        (cells::fit(name, self.name, align[0]), rest)
    }
}

/// Column widths of the virtual root: the free space and the size of volumes, whose cells the
/// other rows take together for what they show, and the name; narrow panels drop the numbers.
#[derive(Debug, Clone, Copy)]
struct RootColumns {
    name: usize,
    numbers: bool,
    /// How many hosts the row of hosts stands for.
    hosts: usize,
    /// The space of the volume that holds the home directory.
    home: Option<Space>,
}

impl RootColumns {
    fn for_width(width: usize, hosts: usize) -> Self {
        let name = width.saturating_sub(2 * SIZE_WIDTH + 2);
        let numbers = name >= MIN_NAME_WIDTH;
        Self {
            name: if numbers { name } else { width },
            numbers,
            hosts,
            home: None,
        }
    }

    /// The name cell, and the free space and size cells with their separators.
    fn join(self, name: &str, free: &str, size: &str, align: [Align; 2]) -> (String, String) {
        let mut rest = String::new();
        if self.numbers {
            rest.push('│');
            rest.push_str(&cells::fit(free, SIZE_WIDTH, align[1]));
            rest.push('│');
            rest.push_str(&cells::fit(size, SIZE_WIDTH, align[1]));
        }
        (cells::fit(name, self.name, align[0]), rest)
    }

    /// The name cell, and `text` across the cells of the numbers.
    fn join_wide(self, name: &str, text: &str, align: Align) -> (String, String) {
        let mut rest = String::new();
        if self.numbers {
            rest.push('│');
            rest.push_str(&cells::fit(text, 2 * SIZE_WIDTH + 1, align));
        }
        (cells::fit(name, self.name, Align::Left), rest)
    }
}

/// Column widths of the list of hosts: the address column fits the longest address, up to half
/// the width, and the name takes the rest; narrow panels drop the address.
#[derive(Debug, Clone, Copy)]
struct HostColumns {
    name: usize,
    address: Option<usize>,
}

impl HostColumns {
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

    /// The name cell, and the address cell with its separator.
    fn join(self, name: &str, address: &str, align: [Align; 2]) -> (String, String) {
        let mut rest = String::new();
        if let Some(width) = self.address {
            rest.push('│');
            rest.push_str(&cells::fit(address, width, align[1]));
        }
        (cells::fit(name, self.name, align[0]), rest)
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

/// The entry `name` in `location`; `name` may be a path, relative or absolute.
pub(crate) fn child(location: &Location, name: &[u8]) -> Option<Location> {
    match location {
        Location::Root | Location::Sftp => None,
        Location::Local(path) => Some(Location::Local(path.join(OsStr::from_bytes(name)))),
        Location::Remote { host, path } => Some(Location::Remote {
            host: host.clone(),
            path: path.join(name),
        }),
    }
}

/// A location for the title and messages: a local path, `host:path`, only `host` for the
/// remote home directory, or the title of the virtual root or of the hosts.
pub(crate) fn location_text(location: &Location) -> String {
    match location {
        Location::Root => fl!("root-title"),
        Location::Sftp => fl!("root-sftp"),
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

    use noc_vfs::{FileKind, Metadata, VolumeKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;

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

    fn volume(path: &str, label: &str, kind: VolumeKind, space: Option<Space>) -> Volume {
        Volume {
            mount_point: PathBuf::from(path),
            label: Some(label.to_owned()),
            fs_type: Some("apfs".to_owned()),
            kind,
            space,
        }
    }

    /// The system volume, a local one, and a network one that did not say how big it is.
    fn volumes() -> Vec<Volume> {
        let space = Some(Space {
            total: 994 << 30,
            available: 212 << 30,
        });
        vec![
            volume("/", "Macintosh HD", VolumeKind::System, space),
            volume("/Volumes/USB", "USB", VolumeKind::Local, space),
            volume("/Volumes/share", "share", VolumeKind::Network, None),
        ]
    }

    fn root_listing(hosts: Vec<RootHost>) -> Listing {
        Listing::Root {
            volumes: volumes(),
            hosts,
        }
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
        panel.listed(
            request.generation,
            Ok(Listed {
                location,
                listing,
                space: None,
            }),
        );
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
        loaded_at(Location::Root, root_listing(hosts()))
    }

    /// The list of hosts.
    fn sftp() -> Panel {
        loaded_at(Location::Sftp, Listing::Hosts(hosts()))
    }

    fn connected(aliases: &[&str]) -> HashSet<String> {
        aliases.iter().map(|alias| (*alias).to_owned()).collect()
    }

    fn names(panel: &Panel) -> Vec<String> {
        (0..panel.rows())
            .map(|row| match panel.row(row) {
                Some(Row::Parent) => "..".to_owned(),
                Some(Row::Entry(entry)) => entry.display_name().into_owned(),
                Some(Row::Volume(volume)) => volume_name(volume),
                Some(Row::Sftp) => "<sftp>".to_owned(),
                Some(Row::Home) => "<home>".to_owned(),
                Some(Row::Host(host)) => host.alias.clone(),
                None => unreachable!(),
            })
            .collect()
    }

    fn under_cursor(panel: &Panel) -> String {
        names(panel)[panel.cursor].clone()
    }

    /// Draws `panel` with mc's markers, or icons with `decor`, in the `terminal` theme.
    fn render_with(
        panel: &mut Panel,
        (width, height): (u16, u16),
        active: bool,
        hosts: &dyn Fn(&str) -> HostState,
        decor: Decor,
    ) -> Terminal<TestBackend> {
        render_themed(
            panel,
            (width, height),
            active,
            hosts,
            decor,
            &Theme::terminal(),
        )
    }

    fn render_themed(
        panel: &mut Panel,
        (width, height): (u16, u16),
        active: bool,
        hosts: &dyn Fn(&str) -> HostState,
        decor: Decor,
        theme: &Theme,
    ) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let view = View {
            hosts,
            decor,
            theme,
            root_title: "alex-mbp",
            tick: 0,
            now: UNIX_EPOCH + Duration::from_secs(NOW),
            tz: &TimeZone::UTC,
        };
        terminal
            .draw(|frame| panel.render(frame, frame.area(), active, &view))
            .unwrap();
        terminal
    }

    fn render(
        panel: &mut Panel,
        width: u16,
        height: u16,
        active: bool,
        hosts: &dyn Fn(&str) -> HostState,
    ) -> TestBackend {
        let terminal = render_with(panel, (width, height), active, hosts, Decor::new(false));
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

        // The order holds for the next directory, and the hosts keep the config order.
        panel.handle(Action::SortByTime);
        let request = panel.handle(Action::Reload).unwrap();
        answer(&mut panel, &request, Listing::Dir(varied()));
        assert_eq!(order(&panel), "Adir dir1 .env Z a.txt c.txt b.md");
        let mut hosts = sftp();
        hosts.handle(Action::SortBySize);
        assert_eq!(names(&hosts), ["..", "web", "db", "staging"]);
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
    fn quick_search_finds_name_prefixes_from_the_cursor() {
        let mut panel = loaded("/srv", listing());
        panel.search_type('b');
        assert!(panel.searching());
        assert_eq!(under_cursor(&panel), "Beta", "case does not matter");
        panel.search_type('I');
        assert_eq!(under_cursor(&panel), "bin");
        panel.search_type('x');
        assert_eq!(panel.search.as_deref(), Some("bi"), "a miss is dropped");
        panel.search_back();
        assert_eq!(under_cursor(&panel), "bin", "the cursor stays");
        panel.search_next();
        assert_eq!(under_cursor(&panel), "Beta", "round to the top");
        panel.search_type('e');
        let hosts = |_: &str| HostState::default();
        let mut terminal = render_with(&mut panel, (40, 6), true, &hosts, Decor::new(false));
        assert!(terminal.backend().to_string().contains("║Search: be"));
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            Position::new(11, 4),
            "after the text"
        );

        panel.end_search();
        assert!(!panel.searching());
        // A character that starts no match starts no search.
        panel.search_type('q');
        assert!(!panel.searching());
        // Opening another directory ends a search.
        panel.search_type('b');
        panel.handle(Action::Reload);
        assert!(!panel.searching());
    }

    #[test]
    fn quick_search_finds_hosts_by_the_name_shown() {
        let mut root = root();
        root.search_type('u');
        assert_eq!(under_cursor(&root), "USB", "volumes by their labels");
        root.end_search();
        root.search_type('s');
        assert_eq!(under_cursor(&root), "share");
        root.search_type('f');
        assert_eq!(under_cursor(&root), "<sftp>");

        let mut panel = sftp();
        panel.search_type('p');
        assert_eq!(under_cursor(&panel), "web", "labelled Prod");
        panel.search_next();
        assert_eq!(under_cursor(&panel), "web", "the only match");
        panel.end_search();
        panel.search_type('s');
        assert_eq!(under_cursor(&panel), "staging");
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
    fn above_slash_is_the_root_with_the_volumes_and_the_hosts() {
        let mut panel = loaded("/", vec![entry("srv", FileKind::Dir, 1)]);
        assert_eq!(names(&panel), ["..", "srv"]);
        let request = panel.handle(Action::Parent).unwrap();
        assert_eq!(request.location, Location::Root);
        answer(&mut panel, &request, root_listing(hosts()));
        assert_eq!(
            names(&panel),
            ["<home>", "Macintosh HD", "USB", "share", "<sftp>"]
        );
        assert_eq!(
            under_cursor(&panel),
            "Macintosh HD",
            "the file system just left"
        );
        assert_eq!(panel.handle(Action::Parent), None, "the root is the top");

        panel.handle(Action::Home);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, local(HOME), "the first row opens home");
        panel.cancel();
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(
            request.location,
            local("/"),
            "volumes open at their mount points"
        );
        // `..` and Enter come back to where the panel left.
        answer(&mut panel, &request, Listing::Dir(listing()));
        let request = panel.handle(Action::Parent).unwrap();
        answer(&mut panel, &request, root_listing(hosts()));
        assert_eq!(under_cursor(&panel), "Macintosh HD");
        panel.handle(Action::Down);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, local("/Volumes/USB"));
        panel.cancel();

        // The hosts open as a directory of their own, with `..` back to their row.
        panel.handle(Action::End);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, Location::Sftp);
        answer(&mut panel, &request, Listing::Hosts(hosts()));
        assert_eq!(names(&panel), ["..", "web", "db", "staging"]);
        assert_eq!(panel.cursor, 0);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, Location::Root);
        answer(&mut panel, &request, root_listing(hosts()));
        assert_eq!(under_cursor(&panel), "<sftp>");
    }

    #[test]
    fn the_root_shows_connected_hosts_again_and_the_cursor_stays_on_its_row() {
        let mut panel = root();
        panel.set_connected(connected(&["staging"]));
        assert_eq!(
            names(&panel),
            [
                "<home>",
                "Macintosh HD",
                "USB",
                "share",
                "<sftp>",
                "staging"
            ]
        );
        panel.handle(Action::End);
        panel.set_connected(connected(&["web", "staging"]));
        assert_eq!(
            names(&panel),
            [
                "<home>",
                "Macintosh HD",
                "USB",
                "share",
                "<sftp>",
                "web",
                "staging"
            ],
            "in config order"
        );
        assert_eq!(
            under_cursor(&panel),
            "staging",
            "the cursor stays on its host"
        );
        assert_eq!(panel.host_under_cursor(), Some("staging"));
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, remote("staging", ""));
        panel.cancel();

        panel.set_connected(connected(&["web"]));
        assert_eq!(
            under_cursor(&panel),
            "web",
            "its host is gone; the last row"
        );
        panel.set_connected(HashSet::new());
        let _ = draw(&mut panel, 40, 10, true);
        assert_eq!(under_cursor(&panel), "<sftp>");

        // A host that connects while the list of hosts is shown changes nothing there.
        let mut hosts = sftp();
        hosts.set_connected(connected(&["web"]));
        assert_eq!(names(&hosts), ["..", "web", "db", "staging"]);
    }

    #[test]
    fn hosts_open_their_home_directory_and_lead_back_to_themselves() {
        let mut panel = sftp();
        panel.handle(Action::End);
        let request = panel.handle(Action::Enter).unwrap();
        assert_eq!(request.location, remote("staging", ""));
        let refused = "deploy@stg: Permission denied (publickey).".to_owned();
        panel.listed(request.generation, Err(refused));
        assert_eq!(panel.location, Location::Sftp);
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
            space: None,
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
        assert_eq!(request.location, Location::Sftp);
        answer(&mut panel, &request, Listing::Hosts(hosts()));
        assert_eq!(under_cursor(&panel), "db");
    }

    #[test]
    fn alt_o_picks_what_the_other_panel_opens_and_moves_on() {
        let mut panel = loaded("/srv", listing());
        let up = Destination {
            location: local("/"),
            focus: Focus::Name(b"srv".to_vec()),
        };
        assert_eq!(
            panel.for_other_panel(),
            Some(up.clone()),
            "`..`: the parent"
        );
        assert_eq!(under_cursor(&panel), "alpha-link", "the cursor moves on");
        let into = Destination {
            location: local("/srv/alpha-link"),
            focus: Focus::First,
        };
        assert_eq!(panel.for_other_panel(), Some(into));
        panel.handle(Action::End);
        assert_eq!(
            panel.for_other_panel(),
            Some(up),
            "a file: the parent, as in mc"
        );
        assert_eq!(under_cursor(&panel), "zeta.txt", "the last row stays");

        let mut root = root();
        let location = |to: Option<Destination>| to.map(|to| to.location);
        assert_eq!(location(root.for_other_panel()), Some(local(HOME)));
        assert_eq!(location(root.for_other_panel()), Some(local("/")));
        let mut hosts = sftp();
        hosts.handle(Action::Down);
        assert_eq!(location(hosts.for_other_panel()), Some(remote("web", "")));
    }

    #[test]
    fn here_is_the_location_and_the_row_under_the_cursor() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        let here = Destination {
            location: local("/srv"),
            focus: Focus::Near {
                name: b"zeta.txt".to_vec(),
                row: 6,
            },
        };
        assert_eq!(panel.here(), here);

        let mut other = loaded("/tmp", Vec::new());
        let request = other.go(here);
        answer(&mut other, &request, Listing::Dir(listing()));
        assert_eq!(under_cursor(&other), "zeta.txt");

        let mut root = root();
        assert_eq!(root.here().focus, Focus::Home);
        root.handle(Action::Down);
        root.handle(Action::Down);
        let here = Destination {
            location: Location::Root,
            focus: Focus::Volume(PathBuf::from("/Volumes/USB")),
        };
        assert_eq!(root.here(), here);
        root.handle(Action::End);
        assert_eq!(root.here().focus, Focus::Sftp);
        let mut hosts = sftp();
        hosts.handle(Action::Down);
        assert_eq!(hosts.here().focus, Focus::Host("web".to_owned()));
    }

    fn marked(panel: &Panel) -> Vec<String> {
        let mut names: Vec<String> = panel
            .marked
            .iter()
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn marks_entries_but_not_dot_dot_and_moves_on() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::Mark);
        assert_eq!(under_cursor(&panel), "alpha-link", "moves on from `..`");
        assert_eq!(panel.marked_total(), (0, 0), "but `..` is not marked");
        panel.handle(Action::Mark);
        panel.handle(Action::End);
        panel.handle(Action::Mark);
        assert_eq!(under_cursor(&panel), "zeta.txt", "the last row stays");
        assert_eq!(
            panel.marked_total(),
            (2, 12_345),
            "directories count, but not their size"
        );
        panel.handle(Action::MarkUp);
        assert_eq!(under_cursor(&panel), "Alpha.md");
        assert_eq!(marked(&panel), ["alpha-link"]);

        panel.handle(Action::InvertMarks);
        assert_eq!(
            marked(&panel),
            [".hidden", "Alpha.md", "alpha-link", "zeta.txt"],
            "files only"
        );
        assert_eq!(panel.marked_total(), (4, 10_012_346));

        let mut hosts = sftp();
        hosts.handle(Action::Mark);
        hosts.handle(Action::Mark);
        hosts.handle(Action::InvertMarks);
        assert_eq!(under_cursor(&hosts), "db");
        assert!(hosts.marked.is_empty(), "hosts cannot be marked");
        let mut root = root();
        root.handle(Action::Mark);
        root.handle(Action::Mark);
        root.handle(Action::InvertMarks);
        assert_eq!(under_cursor(&root), "USB");
        assert!(root.marked.is_empty(), "nor volumes or home");
    }

    #[test]
    fn marks_stay_while_the_directory_does() {
        let mut panel = loaded("/srv", listing());
        panel.handle(Action::InvertMarks);
        panel.handle(Action::SortBySize);
        assert_eq!(marked(&panel), [".hidden", "Alpha.md", "zeta.txt"]);
        panel.set_show_hidden(false);
        assert_eq!(
            marked(&panel),
            ["Alpha.md", "zeta.txt"],
            "hidden ones lose them"
        );
        panel.set_show_hidden(true);
        assert_eq!(marked(&panel), ["Alpha.md", "zeta.txt"]);

        let request = panel.handle(Action::Reload).unwrap();
        let mut changed = listing();
        changed.retain(|entry| entry.name != b"zeta.txt");
        answer(&mut panel, &request, Listing::Dir(changed));
        assert_eq!(marked(&panel), ["Alpha.md"], "names still there keep them");

        panel.handle(Action::Home);
        let request = panel.handle(Action::Enter).unwrap();
        answer(&mut panel, &request, Listing::Dir(listing()));
        assert!(panel.marked.is_empty(), "another directory starts unmarked");
    }

    #[test]
    fn resolves_what_dialogs_name() {
        let mut panel = loaded("/srv", listing());
        assert_eq!(panel.name_under_cursor(), None, "`..`");
        panel.handle(Action::End);
        assert_eq!(panel.name_under_cursor(), Some(&b"zeta.txt"[..]));
        let resolve = |text: &str| panel.resolve(text).unwrap();
        assert_eq!(resolve("new"), local("/srv/new"));
        assert_eq!(resolve("a/b//"), local("/srv/a/b"));
        assert_eq!(resolve("/tmp/x"), local("/tmp/x"));
        assert_eq!(resolve("/"), local("/"));
        assert_eq!(resolve("~"), local(HOME));
        assert_eq!(resolve("~/x"), local("/home/me/x"));
        assert_eq!(resolve("~x"), local("/srv/~x"), "not the home directory");
        assert_eq!(
            resolve("\\~/x"),
            local("/srv/~/x"),
            "a name that starts with ~"
        );

        let remote_panel = loaded_at(remote("db", "/srv"), Listing::Dir(Vec::new()));
        let resolve = |text: &str| remote_panel.resolve(text).unwrap();
        assert_eq!(resolve("new"), remote("db", "/srv/new"));
        assert_eq!(resolve("/abs"), remote("db", "/abs"));
        assert_eq!(resolve("~/x"), remote("db", "x"), "from the remote home");
        assert_eq!(resolve("~"), remote("db", ""));

        assert_eq!(root().resolve("x"), None);
        assert_eq!(sftp().resolve("x"), None);
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
        assert_eq!(
            panel.cursor, 1,
            "the entry is gone: its row, or the last one"
        );

        let mut hosts = sftp();
        hosts.handle(Action::Down);
        hosts.handle(Action::Down);
        let request = hosts.handle(Action::Reload).unwrap();
        assert_eq!(request.location, Location::Sftp);
        let mut reordered = self::hosts();
        reordered.reverse();
        answer(&mut hosts, &request, Listing::Hosts(reordered));
        assert_eq!(under_cursor(&hosts), "db");

        let mut root = root();
        root.handle(Action::Down);
        root.handle(Action::Down);
        let request = root.handle(Action::Reload).unwrap();
        let mut fewer = volumes();
        fewer.remove(0);
        let listing = Listing::Root {
            volumes: fewer,
            hosts: self::hosts(),
        };
        answer(&mut root, &request, listing);
        assert_eq!(under_cursor(&root), "USB", "on its volume");
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
        let mut panel = sftp();
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
        assert_eq!(panel.location, Location::Sftp, "the late reply is dropped");
        assert_eq!(under_cursor(&panel), "web");
    }

    #[test]
    fn a_lost_host_sends_its_panels_back_to_the_list_of_hosts() {
        let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
        assert_eq!(panel.leave_host("web", Some("gone")), None, "another host");
        let request = panel.leave_host("db", Some("Connection reset")).unwrap();
        assert_eq!(request.location, Location::Sftp);
        answer(&mut panel, &request, Listing::Hosts(hosts()));
        assert_eq!(under_cursor(&panel), "db");
        assert_eq!(
            panel.error.as_deref(),
            Some("Lost the connection to db: Connection reset")
        );

        // A panel that was about to open the host goes back as well.
        let mut panel = sftp();
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
    fn the_frame_shows_the_space_of_the_file_system_while_it_fits() {
        let (mut panel, request) = Panel::new(local("/srv"), PathBuf::from(HOME), true);
        let space = Space {
            total: 500 << 30,
            available: 123 << 30,
        };
        let reply = Listed {
            location: local("/srv"),
            listing: Listing::Dir(Vec::new()),
            space: Some(space),
        };
        panel.listed(request.generation, Ok(reply));
        insta::assert_snapshot!(draw(&mut panel, 30, 6, true));
        let bottom = |panel: &mut Panel, width| {
            let backend = draw(panel, width, 6, true);
            let buffer = backend.buffer();
            (0..width)
                .map(|x| buffer[(x, 5)].symbol().to_owned())
                .collect::<String>()
        };
        assert!(bottom(&mut panel, 23).contains("123G / 500G (24%)"));
        assert!(!bottom(&mut panel, 22).contains("123G"), "too narrow");

        // A failed reload keeps it; the next listing replaces it.
        let request = panel.reload_onto(Vec::new());
        panel.listed(request.generation, Err("gone".to_owned()));
        assert_eq!(panel.space, Some(space));
        let request = panel.reload_onto(Vec::new());
        answer(&mut panel, &request, Listing::Dir(Vec::new()));
        assert_eq!(panel.space, None);
        assert!(!bottom(&mut panel, 30).contains('G'));

        // A file system that reports nothing shows nothing.
        panel.space = Some(Space {
            total: 0,
            available: 0,
        });
        assert!(!bottom(&mut panel, 30).contains('0'));
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
    fn marked_rows_and_their_total_are_yellow() {
        use ratatui::style::{Color, Modifier};

        let mut panel = loaded("/srv", listing());
        panel.handle(Action::End);
        panel.handle(Action::MarkUp);
        panel.handle(Action::Mark);
        let hosts = |_: &str| HostState::default();
        let theme = Theme::mc_classic();
        let terminal = render_themed(
            &mut panel,
            (40, 12),
            true,
            &hosts,
            Decor::new(false),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        // Rows: frame, header, `..`, alpha-link, Beta, bin, .hidden, Alpha.md, zeta.txt.
        assert_eq!(colors(1, 7), (Color::LightYellow, Color::Blue), "marked");
        let underlined = |x: u16, y: u16| buffer[(x, y)].modifier.contains(Modifier::UNDERLINED);
        assert!(underlined(1, 7), "marked, and underlined");
        assert!(underlined(1, 8), "under the cursor too");
        assert!(!underlined(1, 6), "not the others");
        assert!(
            buffer[(1, 4)].modifier.contains(Modifier::BOLD),
            "a directory is bold"
        );
        assert_eq!(
            colors(30, 7),
            (Color::LightYellow, Color::Blue),
            "all of it"
        );
        assert_eq!(
            colors(1, 8),
            (Color::LightYellow, Color::Cyan),
            "marked, under the cursor"
        );
        let separator: String = (0..40).map(|x| buffer[(x, 9)].symbol()).collect();
        assert_eq!(separator, "╟────── 10,012,345 B in 2 files ───────╢");
        assert_eq!(colors(7, 9), (Color::LightYellow, Color::Blue));
        assert_eq!(colors(6, 9), (Color::Gray, Color::Blue));
    }

    #[test]
    fn single_borders_draw_single_lines() {
        let mut panel = loaded("/srv", listing());
        let hosts = |_: &str| HostState::default();
        let theme = Theme::terminal().with_borders(noc_config::Borders::Single);
        let terminal = render_themed(
            &mut panel,
            (40, 12),
            true,
            &hosts,
            Decor::new(false),
            &theme,
        );
        let text = terminal.backend().to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("\"┌"), "{text}");
        assert!(lines[9].starts_with("\"├────"), "{text}");
        assert!(lines[11].starts_with("\"└"), "{text}");
        assert!(!text.contains(['═', '║', '╟']), "{text}");
    }

    #[test]
    fn mc_classic_colors_panels_as_mc_does() {
        use ratatui::style::Color;

        let mut panel = loaded("/srv", varied());
        panel.handle(Action::Down);
        let hosts = |_: &str| HostState::default();
        let theme = Theme::mc_classic();
        let terminal = render_themed(
            &mut panel,
            (40, 12),
            true,
            &hosts,
            Decor::new(false),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        // Rows: frame, header, `..`, Adir (cursor), dir1, .env, a.txt, b.md.
        assert_eq!(colors(0, 0), (Color::Gray, Color::Blue), "frame");
        assert_eq!(colors(1, 1), (Color::LightYellow, Color::Blue), "header");
        assert_eq!(colors(1, 3), (Color::Black, Color::Cyan), "the cursor row");
        assert_eq!(colors(30, 3), (Color::Black, Color::Cyan), "all of it");
        assert_eq!(colors(1, 4), (Color::White, Color::Blue), "a directory");
        assert_eq!(colors(1, 6), (Color::Gray, Color::Blue), "a file");
        assert_eq!(colors(30, 4), (Color::Gray, Color::Blue), "other columns");
        assert_eq!(
            colors(2, 0),
            (Color::Black, Color::Cyan),
            "the active title"
        );

        let mut hosts = sftp();
        let connected = |_: &str| HostState {
            status: HostStatus::Connected,
            address: None,
        };
        let terminal = render_themed(
            &mut hosts,
            (40, 8),
            false,
            &connected,
            Decor::new(false),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(1, 3)].fg,
            Color::LightGreen,
            "the marker of a connected host"
        );
        assert_eq!(buffer[(3, 3)].fg, Color::Gray, "its name");
    }

    #[test]
    fn draws_icons_with_ui_icons() {
        let mut panel = loaded("/srv", listing());
        let hosts = |_: &str| HostState::default();
        let terminal = render_with(&mut panel, (40, 10), true, &hosts, Decor::new(true));
        insta::assert_snapshot!(terminal.backend());
        let mut root = root();
        root.set_connected(connected(&["web"]));
        let online = |_: &str| HostState {
            status: HostStatus::Connected,
            address: None,
        };
        let terminal = render_with(&mut root, (40, 11), true, &online, Decor::new(true));
        insta::assert_snapshot!("draws_icons_in_the_root", terminal.backend());
    }

    #[test]
    fn icons_are_dimmed_names_in_their_colors() {
        use ratatui::style::{Color, Modifier};

        let mut panel = loaded("/srv", varied());
        let hosts = |_: &str| HostState::default();
        let theme = Theme::mc_classic();
        let terminal = render_themed(
            &mut panel,
            (40, 12),
            false,
            &hosts,
            Decor::new(true),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let look = |x: u16, y: u16| {
            let cell = &buffer[(x, y)];
            (cell.fg, cell.modifier.contains(Modifier::DIM))
        };
        // Rows: frame, header, `..`, Adir, dir1, .env, a.txt, b.md.
        assert_eq!(look(1, 2), (Color::White, true), "the icon of `..`");
        assert_eq!(look(1, 3), (Color::White, true), "the icon of a directory");
        assert_eq!(look(3, 3), (Color::White, false), "its name");
        assert!(
            !buffer[(1, 3)].modifier.contains(Modifier::BOLD),
            "not bold"
        );
        assert!(
            buffer[(3, 3)].modifier.contains(Modifier::BOLD),
            "unlike the name"
        );
        assert_eq!(look(1, 6), (Color::Gray, true), "the icon of a file");
        assert_eq!(look(3, 6), (Color::Gray, false), "its name");

        let mut root = root();
        let connected = |_: &str| HostState {
            status: HostStatus::Connected,
            address: None,
        };
        let terminal = render_themed(
            &mut root,
            (40, 8),
            false,
            &connected,
            Decor::new(true),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let look = |x: u16, y: u16| {
            let cell = &buffer[(x, y)];
            (cell.fg, cell.modifier.contains(Modifier::DIM))
        };
        // Rows: frame, header, home, the system volume; their icons all in the first column.
        assert_eq!(look(1, 2), (Color::White, true), "the icon of home");
        assert_eq!(look(1, 3), (Color::White, true), "the icon of a volume");
        assert_eq!(look(3, 3), (Color::White, false), "its name");
        let mut hosts = sftp();
        let terminal = render_themed(
            &mut hosts,
            (40, 8),
            false,
            &connected,
            Decor::new(true),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        let look = |x: u16, y: u16| {
            let cell = &buffer[(x, y)];
            (cell.fg, cell.modifier.contains(Modifier::DIM))
        };
        // Rows: frame, header, `..`, the first host, in line with `..`.
        assert_eq!(look(1, 2), (Color::White, true), "the icon of `..`");
        assert_eq!(
            look(1, 3),
            (Color::LightGreen, false),
            "the icon of the host, in the color of its status"
        );
        assert_eq!(look(3, 3), (Color::Gray, false), "its name");
    }

    #[test]
    fn draws_the_root_with_volumes_and_connected_hosts() {
        let mut panel = root();
        panel.set_connected(connected(&["web"]));
        panel.handle(Action::Down);
        assert_eq!(panel.host_under_cursor(), None, "a volume");
        let online = |alias: &str| HostState {
            status: if alias == "web" {
                HostStatus::Connected
            } else {
                HostStatus::Idle
            },
            address: None,
        };
        insta::assert_snapshot!(render(&mut panel, 50, 11, true, &online));
        panel.handle(Action::End);
        let text = render(&mut panel, 50, 10, true, &online).to_string();
        assert!(
            text.contains("║web"),
            "the alias of the label on the status line: {text}"
        );
        panel.handle(Action::Up);
        let text = draw(&mut panel, 50, 10, true).to_string();
        assert!(text.contains("Hosts from the ssh config"), "{text}");

        let narrow = draw(&mut panel, 20, 10, true).to_string();
        assert!(!narrow.contains("212G"), "{narrow}");
        assert!(narrow.contains("USB"), "{narrow}");
    }

    #[test]
    fn draws_the_hosts_with_labels_and_cached_addresses() {
        let mut panel = sftp();
        panel.handle(Action::Down);
        assert_eq!(panel.host_under_cursor(), Some("web"));
        insta::assert_snapshot!(draw(&mut panel, 50, 9, true));

        let narrow = draw(&mut panel, 20, 9, true).to_string();
        assert!(!narrow.contains("deploy"), "{narrow}");
        assert!(narrow.contains("Prod"), "{narrow}");
    }

    #[test]
    fn the_hosts_show_connection_states_and_prefer_fresh_addresses() {
        let mut panel = sftp();
        assert!(panel.shows_root());
        assert_eq!(panel.host_under_cursor(), None, "`..`");
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
