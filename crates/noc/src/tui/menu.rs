//! The location menu of Alt+F1 and Alt+F2, as Far Manager's menu to change drives, for one
//! panel: groups of places under their headings, the home directory in Favorites, the volumes
//! in Mounts, and the hosts of the ssh config in SFTP. Mounts and SFTP put those used last
//! first and show the first ten, with a row that shows the rest; the filter finds them all.
//! Typing filters the menu; while the filter is empty, `1` … `9` and `0` open the first ten
//! places.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use noc_config::{Recent, SearchMode};
use noc_scroll::scrollbar::Bar;
use noc_vfs::{Location, Space, Volume};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::app::Side;
use super::cells::{self, Align};
use super::decor::Decor;
use super::dialog::Colors;
use super::keymap::{Action, Mode, Resolved};
use super::mouse::Pointer;
use super::panel::HostState;
use super::picker::{Body, MenuLine, Pick, Picker, ShownRow, Shows};
use super::places::{KnownHost, Places, volume_name, volume_of};
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the menu gets, in cells, borders included.
const WIDTH: u16 = 48;
/// Places of a group that show while the filter is empty, besides those always shown.
const SHOWN: usize = 10;

/// What a key did in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MenuEvent {
    Pending,
    Closed,
    /// Open this in the menu's panel.
    Open(Location),
    /// Close the connection to this host, or stop connecting to it.
    Disconnect(String),
    /// Edit the settings of this host.
    EditHost(String),
    /// Read the volumes and hosts again.
    Reload,
}

/// A group of places, under a heading of its own, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Group {
    Favorites,
    Mounts,
    Sftp,
}

impl Group {
    fn heading(self) -> String {
        match self {
            Self::Favorites => fl!("menu-favorites"),
            Self::Mounts => fl!("menu-mounts"),
            Self::Sftp => fl!("menu-sftp"),
        }
    }
}

/// A row.
#[derive(Debug, Clone, Copy)]
enum Item<'a> {
    /// The home directory, with the space of the volume that holds it.
    Home(&'a Path, Option<Space>),
    Volume(&'a Volume),
    Host(&'a KnownHost),
    /// The row that shows the places of a group that do not show yet: how many there are.
    More(Group, usize),
}

impl Item<'_> {
    fn group(self) -> Group {
        match self {
            Self::Home(..) => Group::Favorites,
            Self::Volume(_) => Group::Mounts,
            Self::Host(_) => Group::Sftp,
            Self::More(group, _) => group,
        }
    }
}

/// The menu of one panel. Its volumes and hosts come from [`Places`].
#[derive(Debug)]
pub(crate) struct LocationMenu {
    side: Side,
    /// Where the panel is drawn, which the title names: the other side after Ctrl+w x.
    drawn: Side,
    /// Of the listing the menu waits for; a reply to an older one is dropped.
    generation: u64,
    contents: Contents,
    /// `true` once the first listing arrived.
    loaded: bool,
    error: Option<String>,
    picker: Picker,
}

/// What the rows of the menu come from.
#[derive(Debug)]
struct Contents {
    /// Where the panel is: the row the cursor starts on, which always shows.
    current: Location,
    /// The home directory, the first row.
    home: PathBuf,
    places: Places,
    /// The hosts and volumes used last, which come first in their groups.
    recent: Recent,
    /// Hosts that are connected, connecting, or stalled, which always show.
    connected: HashSet<String>,
    /// Groups that show all their places.
    expanded: HashSet<Group>,
}

impl LocationMenu {
    /// A menu for the panel on `side`, which shows `current`, waiting for the listing
    /// `generation`. Its first row opens `home`.
    pub(crate) fn new(side: Side, current: Location, home: PathBuf, generation: u64) -> Self {
        let mut menu = Self {
            side,
            drawn: side,
            generation,
            contents: Contents {
                current,
                home,
                places: Places::default(),
                recent: Recent::default(),
                connected: HashSet::new(),
                expanded: HashSet::new(),
            },
            loaded: false,
            error: None,
            picker: Picker::new(SearchMode::Plain),
        };
        // The home directory, before the listing.
        menu.picker.set_rows_showing(menu.texts());
        menu
    }

    /// The same menu, whose filter matches in `mode` (`ui.search_mode`).
    pub(crate) fn search_mode(mut self, mode: SearchMode) -> Self {
        self.picker.set_mode(mode);
        self
    }

    /// The same menu, with the hosts and volumes in `recent` first in their groups.
    pub(crate) fn recent(mut self, recent: &Recent) -> Self {
        self.contents.recent = recent.clone();
        self
    }

    /// The same menu, for a panel drawn on `drawn`, which the title names.
    pub(crate) fn drawn(mut self, drawn: Side) -> Self {
        self.drawn = drawn;
        self
    }

    /// The panel the menu is for.
    pub(crate) fn side(&self) -> Side {
        self.side
    }

    /// Waits for the listing `generation` instead, for a reload.
    pub(crate) fn reload(&mut self, generation: u64) {
        self.generation = generation;
    }

    /// Takes the hosts that are connected, connecting, or stalled, which always show. The
    /// cursor stays on its row.
    pub(crate) fn set_connected(&mut self, connected: HashSet<String>) {
        if self.contents.connected != connected {
            self.contents.connected = connected;
            self.refresh();
        }
    }

    /// Takes the places of the listing `generation`. The first one puts the cursor on where
    /// the panel is.
    pub(crate) fn listed(&mut self, generation: u64, result: Result<Places, String>) {
        if generation != self.generation {
            return;
        }
        match result {
            Ok(places) => {
                self.contents.places = places;
                self.error = None;
                if self.loaded {
                    self.refresh();
                } else {
                    self.picker.set_rows_showing(self.texts());
                    let row = self.contents.current_row(&self.items());
                    self.picker.set_cursor(row);
                    self.loaded = true;
                }
            }
            Err(reason) => self.error = Some(cells::sanitize(reason.as_bytes())),
        }
    }

    /// Takes the rows again, with the cursor on its row if it still shows.
    fn refresh(&mut self) {
        let chosen = self.chosen().map(key);
        self.picker.set_rows_showing(self.texts());
        let items = self.items();
        if let Some(row) =
            chosen.and_then(|chosen| items.iter().position(|item| key(*item) == chosen))
        {
            self.picker.set_cursor(row);
        }
    }

    /// Shows every place of `group`; the cursor stays on its row, the first of those that did
    /// not show.
    fn expand(&mut self, group: Group) {
        self.contents.expanded.insert(group);
        self.picker.set_rows_showing(self.texts());
    }

    /// What the filter finds each row by, and when it shows.
    fn texts(&self) -> Vec<(Vec<String>, Shows)> {
        self.contents
            .rows()
            .into_iter()
            .map(|(item, shows)| (texts(item), shows))
            .collect()
    }

    /// The rows the filter shows, in their order.
    fn items(&self) -> Vec<Item<'_>> {
        self.picker
            .shown(self.contents.rows().into_iter().map(|(item, _)| item))
    }

    /// The row under the cursor.
    fn chosen(&self) -> Option<Item<'_>> {
        self.items().get(self.picker.cursor()).copied()
    }

    /// The alias of the host under the cursor.
    pub(crate) fn chosen_host(&self) -> Option<&str> {
        match self.chosen()? {
            Item::Host(host) => Some(&host.alias),
            Item::Home(..) | Item::Volume(_) | Item::More(..) => None,
        }
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    /// The rows, for the count before a page key.
    pub(crate) fn picker_mut(&mut self) -> &mut Picker {
        &mut self.picker
    }

    pub(crate) fn mode(&self) -> Mode {
        self.picker.mode()
    }

    /// Takes a key: the picker's, and F4, which edits the settings of a host, F8, which
    /// disconnects it, and R, which reads the volumes and hosts again. Enter on the row of the
    /// rest of a group shows them.
    pub(crate) fn handle(&mut self, input: Resolved) -> MenuEvent {
        match self.picker.handle(input) {
            Pick::Chosen(row) => {
                let chosen = self.items().get(row).map(|item| match item {
                    Item::More(group, _) => Err(*group),
                    Item::Home(home, _) => Ok(Location::Local(home.to_path_buf())),
                    Item::Volume(volume) => Ok(Location::Local(volume.mount_point.clone())),
                    Item::Host(host) => Ok(Location::Remote {
                        host: host.alias.clone(),
                        path: noc_vfs::RemotePath::from(""),
                    }),
                });
                match chosen {
                    Some(Ok(location)) => MenuEvent::Open(location),
                    Some(Err(group)) => {
                        self.expand(group);
                        MenuEvent::Pending
                    }
                    None => MenuEvent::Pending,
                }
            }
            Pick::Closed => MenuEvent::Closed,
            Pick::Other(Action::Disconnect) => match self.chosen_host() {
                Some(host) => MenuEvent::Disconnect(host.to_owned()),
                None => MenuEvent::Pending,
            },
            Pick::Other(Action::EditHost) => match self.chosen_host() {
                Some(host) => MenuEvent::EditHost(host.to_owned()),
                None => MenuEvent::Pending,
            },
            Pick::Other(Action::Reload) => MenuEvent::Reload,
            Pick::Pending | Pick::Other(_) => MenuEvent::Pending,
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

    /// Takes a press of the mouse, where the menu was drawn last: a click puts the cursor on
    /// a row, and returns the key that a double click on one, or a click outside the menu,
    /// stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        self.picker.pointer(pointer)
    }

    /// The lines: the rows the filter shows, with the heading of each group above its first.
    fn lines(items: &[Item<'_>]) -> Vec<MenuLine> {
        let mut lines = Vec::with_capacity(items.len() + 3);
        let mut group = None;
        for (row, item) in items.iter().enumerate() {
            if group != Some(item.group()) {
                group = Some(item.group());
                lines.push(MenuLine::Heading(item.group().heading()));
            }
            lines.push(MenuLine::Row(row));
        }
        lines
    }

    /// Draws the menu over `area`, the panel it is for: the filter, then the rows, scrolled to
    /// the cursor.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        decor: Decor,
        hosts: &dyn Fn(&str) -> HostState,
        tick: u64,
    ) {
        let rows = self.contents.rows();
        let items = self.picker.shown(rows.into_iter().map(|(item, _)| item));
        let lines = Self::lines(&items);
        let title = match self.drawn {
            Side::Left => fl!("menu-left"),
            Side::Right => fl!("menu-right"),
        };
        let message =
            (!self.loaded).then(|| self.error.clone().unwrap_or_else(|| fl!("panel-loading")));
        let widest = items
            .iter()
            .map(|item| cells::width(&info(*item, hosts)))
            .max()
            .unwrap_or(0);
        let look = Look {
            theme,
            decor,
            hosts,
            tick,
            focused: Colors::of(theme, false).focused_style(),
        };
        let full = items.get(self.picker.cursor()).map(|item| {
            let texts = texts(*item).join("  ");
            cells::sanitize(texts.as_bytes())
        });
        let body = Body {
            title: &title,
            width: WIDTH,
            lines: &lines,
            message,
            full,
            ..Body::default()
        };
        self.picker.render(frame, area, theme, body, &mut |row| {
            let info_width = widest.min(row.width / 2);
            // The name, a space, the information.
            let name_width = row.width.saturating_sub(info_width + 1);
            look.line(items[row.index], row, (name_width, info_width))
        });
    }
}

impl Contents {
    /// The row of the home directory or the volume that holds the panel's directory, whichever
    /// is nearer, or of its host.
    fn current_row(&self, items: &[Item<'_>]) -> usize {
        let position = match &self.current {
            Location::Local(path) => items
                .iter()
                .enumerate()
                .filter_map(|(row, item)| {
                    let top = match item {
                        Item::Home(home, _) => *home,
                        Item::Volume(volume) => &volume.mount_point,
                        Item::Host(_) | Item::More(..) => return None,
                    };
                    path.starts_with(top)
                        .then_some((top.as_os_str().len(), std::cmp::Reverse(row)))
                })
                .max()
                .map(|(_, std::cmp::Reverse(row))| row),
            Location::Remote { host, .. } => items
                .iter()
                .position(|item| matches!(item, Item::Host(shown) if shown.alias == *host)),
        };
        position.unwrap_or(0)
    }

    /// Every row, shown or not, and when it shows: the home directory, then the volumes, then
    /// the hosts, those used last first; past the first ten of a group that is not expanded,
    /// only those always shown, then a row for the rest.
    fn rows(&self) -> Vec<(Item<'_>, Shows)> {
        let space = volume_of(&self.places.volumes, &self.home).and_then(|volume| volume.space);
        let mut rows = vec![(Item::Home(&self.home, space), Shows::Always)];
        let here = match &self.current {
            Location::Local(path) => volume_of(&self.places.volumes, path),
            Location::Remote { .. } => None,
        };
        let volumes = by_recent(&self.places.volumes, |volume| {
            self.recent
                .volumes
                .iter()
                .position(|used| *used == volume.mount_point)
        });
        let pinned =
            |volume: &Volume| here.is_some_and(|here| here.mount_point == volume.mount_point);
        self.group(&mut rows, Group::Mounts, volumes, pinned, Item::Volume);
        let hosts = by_recent(&self.places.hosts, |host| {
            self.recent
                .hosts
                .iter()
                .position(|used| *used == host.alias)
        });
        let pinned = |host: &KnownHost| {
            self.connected.contains(&host.alias)
                || matches!(&self.current, Location::Remote { host: shown, .. } if *shown == host.alias)
        };
        self.group(&mut rows, Group::Sftp, hosts, pinned, Item::Host);
        rows
    }

    /// Adds the rows of `group`, its `places` in order: the first ten, and those `pinned`,
    /// always; the others only while the filter has text, with a row for them while it has
    /// none, unless the group is expanded.
    fn group<'a, T>(
        &self,
        rows: &mut Vec<(Item<'a>, Shows)>,
        group: Group,
        places: Vec<&'a T>,
        pinned: impl Fn(&T) -> bool,
        item: impl Fn(&'a T) -> Item<'a>,
    ) {
        let (first, hidden): (Vec<_>, Vec<_>) = places
            .into_iter()
            .enumerate()
            .partition(|(index, place)| *index < SHOWN || pinned(place));
        rows.extend(
            first
                .into_iter()
                .map(|(_, place)| (item(place), Shows::Always)),
        );
        if hidden.is_empty() {
            return;
        }
        let shows = if self.expanded.contains(&group) {
            Shows::Always
        } else {
            Shows::Filtered
        };
        let more = hidden.len();
        rows.extend(hidden.into_iter().map(|(_, place)| (item(place), shows)));
        if shows == Shows::Filtered {
            rows.push((Item::More(group, more), Shows::Unfiltered));
        }
    }
}

/// `places` in their order, those with a `rank` (lower for used later) first, by it.
fn by_recent<T>(places: &[T], rank: impl Fn(&T) -> Option<usize>) -> Vec<&T> {
    let mut places: Vec<&T> = places.iter().collect();
    places.sort_by_key(|place| rank(place).map_or((1, 0), |rank| (0, rank)));
    places
}

/// How rows are drawn.
struct Look<'a> {
    theme: &'a Theme,
    decor: Decor,
    hosts: &'a dyn Fn(&str) -> HostState,
    tick: u64,
    /// The row under the cursor.
    focused: Style,
}

impl Look<'_> {
    /// A row: its name with its prefix, and its information, in cells of `widths`.
    fn line(
        &self,
        item: Item<'_>,
        ShownRow { selected, .. }: ShownRow,
        (name_width, info_width): (usize, usize),
    ) -> Line<'static> {
        let (prefix, name, marker) = match item {
            Item::Home(..) => (self.decor.home().to_owned(), fl!("menu-home"), None),
            Item::Volume(volume) => (
                self.decor.volume(volume.kind).to_owned(),
                volume_name(volume),
                None,
            ),
            Item::Host(host) => {
                let status = (self.hosts)(&host.alias).status;
                let name = host.label.as_deref().unwrap_or(&host.alias);
                (
                    self.decor.host(status, self.tick),
                    cells::sanitize(name.as_bytes()),
                    Some(self.theme.dialog_host_status(status)),
                )
            }
            Item::More(_, count) => (String::new(), fl!("menu-more", count = count), None),
        };
        let styles = if selected {
            (self.focused, self.theme.truncated_cursor)
        } else {
            (self.theme.dialog, self.theme.truncated_dialog)
        };
        let marks = self.theme.glyphs.marks;
        let name = format!("{prefix}{name}");
        let mut spans = cells::fit(&name, name_width, Align::Left, styles, marks);
        // The status marker of a host has a color of its own.
        if let (Some(style), false) = (marker, selected)
            && let Some(first) = spans.first_mut()
        {
            let split = first.content.chars().next().map_or(0, char::len_utf8);
            let (marker, rest) = first.content.split_at(split);
            let (marker, rest) = (marker.to_owned(), rest.to_owned());
            *first = Span::styled(rest, styles.0);
            spans.insert(0, Span::styled(marker, styles.0.patch(style)));
        }
        spans.push(Span::styled(" ", styles.0));
        let info = info(item, self.hosts);
        spans.extend(cells::fit(&info, info_width, Align::Right, styles, marks));
        Line::from(spans).style(styles.0)
    }
}

/// What the filter finds a row by.
fn texts(item: Item<'_>) -> Vec<String> {
    match item {
        Item::Home(home, _) => vec![fl!("menu-home"), home.to_string_lossy().into_owned()],
        Item::Volume(volume) => vec![
            volume_name(volume),
            volume.mount_point.to_string_lossy().into_owned(),
        ],
        Item::Host(host) => [
            Some(&host.alias),
            host.label.as_ref(),
            host.address.as_ref(),
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect(),
        Item::More(..) => Vec::new(),
    }
}

/// What identifies a row across listings.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Home,
    Volume(PathBuf),
    Host(String),
    More(Group),
}

fn key(item: Item<'_>) -> Key {
    match item {
        Item::Home(..) => Key::Home,
        Item::Volume(volume) => Key::Volume(volume.mount_point.clone()),
        Item::Host(host) => Key::Host(host.alias.clone()),
        Item::More(group, _) => Key::More(group),
    }
}

/// Right of a row: the free space of the home directory or a volume, or the address of a host.
fn info(item: Item<'_>, hosts: &dyn Fn(&str) -> HostState) -> String {
    let free = |space: Option<Space>| {
        space.map_or_else(
            || fl!("menu-free-unknown"),
            |space| super::panel::short_size(space.available),
        )
    };
    match item {
        Item::Home(_, space) => free(space),
        Item::Volume(volume) => free(volume.space),
        Item::Host(host) => {
            let address = hosts(&host.alias).address.or_else(|| host.address.clone());
            cells::sanitize(address.unwrap_or_default().as_bytes())
        }
        Item::More(..) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use noc_vfs::VolumeKind;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::panel::HostStatus;

    fn volume(path: &str, label: Option<&str>, kind: VolumeKind) -> Volume {
        Volume {
            mount_point: PathBuf::from(path),
            label: label.map(str::to_owned),
            fs_type: Some("apfs".to_owned()),
            kind,
            space: Some(Space {
                total: 994 << 30,
                available: 212 << 30,
            }),
        }
    }

    fn host(alias: &str, label: Option<&str>, address: Option<&str>) -> KnownHost {
        KnownHost {
            alias: alias.to_owned(),
            label: label.map(str::to_owned),
            address: address.map(str::to_owned),
        }
    }

    fn places() -> Places {
        Places {
            volumes: vec![
                volume("/", Some("Macintosh HD"), VolumeKind::System),
                volume("/Volumes/USB", Some("USB"), VolumeKind::Local),
                Volume {
                    space: None,
                    ..volume("/Volumes/share", Some("share"), VolumeKind::Network)
                },
            ],
            hosts: vec![
                host("web", Some("Prod"), Some("deploy@10.0.0.5")),
                host("db", None, None),
                host("proxy", None, Some("root@proxy.example.org")),
            ],
        }
    }

    /// `count` hosts, `h01` … in config order.
    fn many_hosts(count: usize) -> Places {
        Places {
            volumes: vec![volume("/", None, VolumeKind::System)],
            hosts: (1..=count)
                .map(|index| host(&format!("h{index:02}"), None, None))
                .collect(),
        }
    }

    /// A menu for the left panel on `current`, with `places`.
    fn menu_with(current: Location, recent: &Recent, places: Places) -> LocationMenu {
        let mut menu =
            LocationMenu::new(Side::Left, current, PathBuf::from("/Users/me"), 1).recent(recent);
        menu.listed(1, Ok(places));
        menu
    }

    fn menu_at(current: Location) -> LocationMenu {
        menu_with(current, &Recent::default(), places())
    }

    fn home() -> Location {
        Location::Local(PathBuf::from("/Users/me"))
    }

    fn typed(menu: &mut LocationMenu, text: &str) {
        menu.handle(Resolved::Action(Action::QuickSearch));
        for c in text.chars() {
            assert_eq!(menu.handle(Resolved::Insert(c)), MenuEvent::Pending);
        }
    }

    fn press(menu: &mut LocationMenu, action: Action) -> MenuEvent {
        menu.handle(Resolved::Action(action))
    }

    fn remote(host: &str) -> Location {
        Location::Remote {
            host: host.to_owned(),
            path: noc_vfs::RemotePath::from(""),
        }
    }

    fn keys(menu: &LocationMenu) -> Vec<Key> {
        menu.items().into_iter().map(key).collect()
    }

    fn hosts_shown(menu: &LocationMenu) -> Vec<String> {
        menu.items()
            .into_iter()
            .filter_map(|item| match item {
                Item::Host(host) => Some(host.alias.clone()),
                Item::More(_, count) => Some(format!("+{count}")),
                Item::Home(..) | Item::Volume(_) => None,
            })
            .collect()
    }

    fn draw(menu: &mut LocationMenu, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(50, height)).unwrap();
        let hosts = |alias: &str| HostState {
            status: if alias == "web" {
                HostStatus::Connected
            } else {
                HostStatus::Idle
            },
            address: None,
        };
        terminal
            .draw(|frame| {
                menu.render(
                    frame,
                    frame.area(),
                    &Theme::terminal(),
                    Decor::new(false),
                    &hosts,
                    0,
                );
            })
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn starts_on_where_the_panel_is() {
        let at = |current: Location| {
            let menu = menu_at(current);
            menu.chosen().map(key)
        };
        let local = |path: &str| Location::Local(PathBuf::from(path));
        assert_eq!(
            at(local("/Volumes/USB/photos")),
            Some(Key::Volume(PathBuf::from("/Volumes/USB")))
        );
        assert_eq!(at(local("/Users/me/src")), Some(Key::Home), "nearer than /");
        assert_eq!(at(local("/Users")), Some(Key::Volume(PathBuf::from("/"))));
        assert_eq!(at(remote("db")), Some(Key::Host("db".to_owned())));
        assert_eq!(at(remote("gone")), Some(Key::Home), "a host not listed");
    }

    #[test]
    fn enter_opens_home_volumes_and_hosts() {
        let mut menu = menu_at(home());
        assert_eq!(
            press(&mut menu, Action::Confirm),
            MenuEvent::Open(Location::Local(PathBuf::from("/Users/me")))
        );
        press(&mut menu, Action::Down);
        assert_eq!(
            press(&mut menu, Action::Confirm),
            MenuEvent::Open(Location::Local(PathBuf::from("/"))),
            "volumes open at their mount points"
        );
        press(&mut menu, Action::Down);
        assert_eq!(
            press(&mut menu, Action::Confirm),
            MenuEvent::Open(Location::Local(PathBuf::from("/Volumes/USB")))
        );
        assert_eq!(
            menu.handle(Resolved::Insert('6')),
            MenuEvent::Pending,
            "no digits, and no filter in Normal mode"
        );
        press(&mut menu, Action::End);
        assert_eq!(
            press(&mut menu, Action::Confirm),
            MenuEvent::Open(remote("proxy"))
        );
        assert_eq!(press(&mut menu, Action::Cancel), MenuEvent::Closed);
        assert_eq!(press(&mut menu, Action::Reload), MenuEvent::Reload);
    }

    #[test]
    fn typing_filters_by_name_alias_mount_point_and_address() {
        let mut menu = menu_at(home());
        typed(&mut menu, "pro");
        assert_eq!(
            keys(&menu),
            [Key::Host("web".to_owned()), Key::Host("proxy".to_owned())],
            "labelled Prod, and proxy"
        );
        typed(&mut menu, "x");
        assert_eq!(menu.chosen().map(key), Some(Key::Host("proxy".to_owned())));
        // A digit is text once the filter has some.
        for _ in 0..4 {
            press(&mut menu, Action::Backspace);
        }
        typed(&mut menu, "volumes/u");
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Volume(PathBuf::from("/Volumes/USB")))
        );
        typed(&mut menu, "1");
        assert!(menu.items().is_empty());
        let mut home = menu_at(home());
        typed(&mut home, "hom");
        assert_eq!(home.chosen().map(key), Some(Key::Home));
        assert_eq!(press(&mut menu, Action::Confirm), MenuEvent::Pending);
        let text = draw(&mut menu, 12);
        assert!(text.contains("Nothing matches"), "{text}");
    }

    #[test]
    fn a_fuzzy_filter_keeps_the_order_and_puts_the_cursor_on_the_best() {
        let mut menu = LocationMenu::new(Side::Left, home(), PathBuf::from("/Users/me"), 1)
            .search_mode(SearchMode::Fzf);
        menu.listed(1, Ok(places()));
        typed(&mut menu, "pxy");
        assert_eq!(
            keys(&menu),
            [Key::Host("proxy".to_owned())],
            "the letters in order, not together"
        );
        for _ in 0..3 {
            press(&mut menu, Action::Backspace);
        }
        typed(&mut menu, "sh");
        assert_eq!(
            keys(&menu),
            [
                Key::Volume(PathBuf::from("/")),
                Key::Volume(PathBuf::from("/Volumes/share"))
            ],
            "Macintosh HD, and share"
        );
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Volume(PathBuf::from("/Volumes/share"))),
            "the start of a word beats the end of one"
        );
        typed(&mut menu, " !hd");
        assert_eq!(
            keys(&menu),
            [Key::Volume(PathBuf::from("/Volumes/share"))],
            "fzf's extended search"
        );
    }

    #[test]
    fn f4_and_f8_edit_and_disconnect_hosts_only() {
        let mut menu = menu_at(remote("web"));
        assert_eq!(menu.chosen_host(), Some("web"));
        assert_eq!(
            press(&mut menu, Action::EditHost),
            MenuEvent::EditHost("web".to_owned())
        );
        assert_eq!(
            press(&mut menu, Action::Disconnect),
            MenuEvent::Disconnect("web".to_owned())
        );
        press(&mut menu, Action::Home);
        assert_eq!(menu.chosen_host(), None);
        assert_eq!(press(&mut menu, Action::Disconnect), MenuEvent::Pending);
        assert_eq!(press(&mut menu, Action::EditHost), MenuEvent::Pending);
    }

    #[test]
    fn a_reload_keeps_the_cursor_on_its_row_and_stale_replies_are_dropped() {
        let mut menu = menu_at(remote("db"));
        menu.reload(2);
        let mut reordered = places();
        reordered.hosts.reverse();
        menu.listed(1, Ok(Places::default()));
        menu.listed(2, Ok(reordered));
        assert_eq!(menu.chosen().map(key), Some(Key::Host("db".to_owned())));
        assert_eq!(menu.items().len(), 7);
    }

    #[test]
    fn groups_put_the_places_used_last_first() {
        let recent = Recent {
            hosts: vec!["proxy".to_owned(), "gone".to_owned(), "db".to_owned()],
            volumes: vec![PathBuf::from("/Volumes/share")],
        };
        let menu = menu_with(home(), &recent, places());
        assert_eq!(
            keys(&menu),
            [
                Key::Home,
                Key::Volume(PathBuf::from("/Volumes/share")),
                Key::Volume(PathBuf::from("/")),
                Key::Volume(PathBuf::from("/Volumes/USB")),
                Key::Host("proxy".to_owned()),
                Key::Host("db".to_owned()),
                Key::Host("web".to_owned()),
            ],
            "the others in their order"
        );
    }

    #[test]
    fn past_ten_places_a_row_shows_the_rest_which_the_filter_finds_anyway() {
        let recent = Recent {
            hosts: vec!["h12".to_owned()],
            ..Recent::default()
        };
        let mut menu = menu_with(home(), &recent, many_hosts(14));
        let first: Vec<String> = [
            "h12", "h01", "h02", "h03", "h04", "h05", "h06", "h07", "h08",
        ]
        .iter()
        .map(|alias| (*alias).to_owned())
        .chain(["h09".to_owned(), "+4".to_owned()])
        .collect();
        assert_eq!(hosts_shown(&menu), first);

        typed(&mut menu, "h1");
        assert_eq!(
            hosts_shown(&menu),
            ["h12", "h10", "h11", "h13", "h14"],
            "the filter finds the rest"
        );
        press(&mut menu, Action::Backspace);
        press(&mut menu, Action::Backspace);

        press(&mut menu, Action::End);
        assert_eq!(menu.chosen().map(key), Some(Key::More(Group::Sftp)));
        assert_eq!(menu.chosen_host(), None);
        let text = draw(&mut menu, 20);
        assert!(text.contains("║ 4 more…"), "{text}");
        assert_eq!(press(&mut menu, Action::Confirm), MenuEvent::Pending);
        assert_eq!(hosts_shown(&menu).len(), 14, "Enter shows them");
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Host("h10".to_owned())),
            "the first of them"
        );
    }

    #[test]
    fn hosts_in_use_and_the_panels_place_always_show() {
        let mut menu = menu_with(remote("h13"), &Recent::default(), many_hosts(14));
        assert_eq!(menu.chosen_host(), Some("h13"));
        menu.set_connected(["h14".to_owned()].into());
        let shown = hosts_shown(&menu);
        assert_eq!(shown.len(), 13, "{shown:?}");
        assert_eq!(&shown[10..], ["h13", "h14", "+2"]);
        assert_eq!(menu.chosen_host(), Some("h13"), "the cursor stays");

        let volumes = Places {
            volumes: (0..12)
                .map(|index| volume(&format!("/Volumes/v{index:02}"), None, VolumeKind::Local))
                .collect(),
            hosts: Vec::new(),
        };
        let here = Location::Local(PathBuf::from("/Volumes/v11/photos"));
        let menu = menu_with(here, &Recent::default(), volumes);
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Volume(PathBuf::from("/Volumes/v11")))
        );
        assert_eq!(
            menu.items().len(),
            1 + 11 + 1,
            "home, ten and its own, the rest"
        );
    }

    #[test]
    fn host_icons_show_on_the_gray_of_mc_classic() {
        use ratatui::style::Color;

        let mut menu = menu_at(home());
        let mut terminal = Terminal::new(TestBackend::new(50, 16)).unwrap();
        let hosts = |alias: &str| HostState {
            status: match alias {
                "web" => HostStatus::Connected,
                "proxy" => HostStatus::Failed,
                _ => HostStatus::Idle,
            },
            address: None,
        };
        terminal
            .draw(|frame| {
                let theme = Theme::mc_classic();
                menu.render(frame, frame.area(), &theme, Decor::new(true), &hosts, 0);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        // Rows 11 … 13 hold the hosts, below their heading; their icons are in column 3.
        let colors = |y: u16| (buffer[(3, y)].fg, buffer[(3, y)].bg);
        assert_eq!(colors(11), (Color::Green, Color::Gray), "connected");
        assert_eq!(
            colors(12),
            (Color::Black, Color::Gray),
            "idle: the dialog's own color"
        );
        assert_eq!(colors(13), (Color::Red, Color::Gray), "failed");
        assert_eq!(buffer[(3, 12)].symbol(), "󰒋");
    }

    #[test]
    fn the_cursor_stands_right_after_the_filter() {
        let mut menu = menu_at(home());
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        let hosts = |_: &str| HostState::default();
        let mut cursor_after = |menu: &mut LocationMenu, text: &str| {
            typed(menu, text);
            terminal
                .draw(|frame| {
                    let theme = Theme::terminal();
                    menu.render(frame, frame.area(), &theme, Decor::new(false), &hosts, 0);
                })
                .unwrap();
            terminal.get_cursor_position().unwrap()
        };
        let empty = cursor_after(&mut menu, "");
        let typed = cursor_after(&mut menu, "pr");
        assert_eq!(typed.x, empty.x + 2);
        assert_eq!(
            cursor_after(&mut menu, " ").x,
            empty.x + 3,
            "after a space too"
        );
    }

    #[test]
    fn draws_groups_under_their_headings() {
        let mut menu = menu_at(remote("web"));
        insta::assert_snapshot!(draw(&mut menu, 16));
        let mut loading = LocationMenu::new(Side::Right, home(), PathBuf::from("/Users/me"), 1);
        let text = draw(&mut loading, 10);
        assert!(
            text.contains("Right") && text.contains("Loading…"),
            "{text}"
        );
    }
}
