//! The location menu of Alt-F1 and Alt-F2, as Far Manager's menu to change drives: the home
//! directory, the volumes, and the SFTP hosts, for one panel. Typing filters it; while the filter is empty,
//! `1` … `9` and `0` open the first ten rows.

use std::path::{Path, PathBuf};

use noc_vfs::{Location, Space, Volume};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::app::Side;
use super::cells::{self, Align};
use super::decor::Decor;
use super::dialog::{Colors, draw_box};
use super::keymap::{Action, Resolved};
use super::panel::{HostState, Listed, Listing};
use super::root::{RootHost, volume_name, volume_of};
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the menu gets, in cells, borders included.
const WIDTH: u16 = 48;
/// Cells of the free space of a volume, such as `212G`.
const FREE_WIDTH: usize = 5;

/// What a key did in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MenuEvent {
    Pending,
    Closed,
    /// Open this in the menu's panel.
    Open(Location),
    /// Close the connection to this host, or stop connecting to it.
    Disconnect(String),
    /// Read the volumes and hosts again.
    Reload,
}

/// A row that can be chosen.
#[derive(Debug, Clone, Copy)]
enum Item<'a> {
    /// The home directory, with the space of the volume that holds it.
    Home(&'a Path, Option<Space>),
    Volume(&'a Volume),
    Host(&'a RootHost),
}

/// A line of the menu: a row to choose, by its index among the rows the filter shows, or the
/// heading of the hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Item(usize),
    Hosts,
}

/// The menu of one panel. Its volumes and hosts come from a listing of the virtual root.
#[derive(Debug)]
pub(crate) struct LocationMenu {
    side: Side,
    /// Where the panel is: the row the cursor starts on.
    current: Location,
    /// The home directory, the first row.
    home: PathBuf,
    /// Of the listing the menu waits for; a reply to an older one is dropped.
    generation: u64,
    volumes: Vec<Volume>,
    hosts: Vec<RootHost>,
    /// `true` once the first listing arrived.
    loaded: bool,
    error: Option<String>,
    filter: String,
    /// The row under the cursor, among those the filter shows.
    cursor: usize,
    /// First line on screen, and lines on screen at the last render.
    offset: usize,
    page: usize,
}

impl LocationMenu {
    /// A menu for the panel on `side`, which shows `current`, waiting for the listing
    /// `generation`. Its first row opens `home`.
    pub(crate) fn new(side: Side, current: Location, home: PathBuf, generation: u64) -> Self {
        Self {
            side,
            current,
            home,
            generation,
            volumes: Vec::new(),
            hosts: Vec::new(),
            loaded: false,
            error: None,
            filter: String::new(),
            cursor: 0,
            offset: 0,
            page: 1,
        }
    }

    /// The panel the menu is for.
    pub(crate) fn side(&self) -> Side {
        self.side
    }

    /// Waits for the listing `generation` instead, for a reload.
    pub(crate) fn reload(&mut self, generation: u64) {
        self.generation = generation;
    }

    /// Takes a listing of the virtual root. The first one puts the cursor on where the panel is.
    pub(crate) fn listed(&mut self, generation: u64, result: Result<Listed, String>) {
        if generation != self.generation {
            return;
        }
        match result {
            Ok(Listed {
                listing: Listing::Root { volumes, hosts },
                ..
            }) => {
                let chosen = self.chosen().map(key);
                self.volumes = volumes;
                self.hosts = hosts;
                self.error = None;
                let items = self.items();
                let keep =
                    chosen.and_then(|chosen| items.iter().position(|item| key(*item) == chosen));
                self.cursor = match keep {
                    Some(row) if self.loaded => row,
                    _ => self.current_row(&items),
                };
                self.loaded = true;
            }
            Ok(_) => {}
            Err(reason) => self.error = Some(cells::sanitize(reason.as_bytes())),
        }
    }

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
                        Item::Host(_) => return None,
                    };
                    path.starts_with(top)
                        .then_some((top.as_os_str().len(), std::cmp::Reverse(row)))
                })
                .max()
                .map(|(_, std::cmp::Reverse(row))| row),
            Location::Remote { host, .. } => items
                .iter()
                .position(|item| matches!(item, Item::Host(shown) if shown.alias == *host)),
            Location::Sftp => items.iter().position(|item| matches!(item, Item::Host(_))),
            Location::Root => None,
        };
        position.unwrap_or(0)
    }

    /// The rows the filter shows: the home directory, the volumes, then the hosts.
    fn items(&self) -> Vec<Item<'_>> {
        let filter = self.filter.to_lowercase();
        let has = |text: &str| text.to_lowercase().contains(&filter);
        let home = (filter.is_empty()
            || has(&fl!("root-home"))
            || has(&self.home.to_string_lossy()))
        .then(|| {
            let space = volume_of(&self.volumes, &self.home).and_then(|volume| volume.space);
            Item::Home(&self.home, space)
        });
        let volumes = self
            .volumes
            .iter()
            .filter(|volume| {
                filter.is_empty()
                    || has(&volume_name(volume))
                    || has(&volume.mount_point.to_string_lossy())
            })
            .map(Item::Volume);
        let hosts = self
            .hosts
            .iter()
            .filter(|host| {
                filter.is_empty()
                    || has(&host.alias)
                    || host.label.as_deref().is_some_and(has)
                    || host.address.as_deref().is_some_and(has)
            })
            .map(Item::Host);
        home.into_iter().chain(volumes).chain(hosts).collect()
    }

    /// The row under the cursor.
    fn chosen(&self) -> Option<Item<'_>> {
        self.items().get(self.cursor).copied()
    }

    /// Where a row leads: the home directory, a volume's mount point, or a host's home
    /// directory.
    fn open(item: Item<'_>) -> Location {
        match item {
            Item::Home(home, _) => Location::Local(home.to_path_buf()),
            Item::Volume(volume) => Location::Local(volume.mount_point.clone()),
            Item::Host(host) => Location::Remote {
                host: host.alias.clone(),
                path: noc_vfs::RemotePath::from(""),
            },
        }
    }

    /// Takes a key: arrows move, Enter opens, a digit opens its row while the filter is empty,
    /// other characters filter, Backspace takes one back, F8 disconnects a host, and Esc
    /// closes the menu.
    pub(crate) fn handle(&mut self, input: Resolved) -> MenuEvent {
        let rows = self.items().len();
        let last = rows.saturating_sub(1);
        let page = self.page.max(1);
        match input {
            Resolved::Insert(c) if self.filter.is_empty() && c.is_ascii_digit() => {
                let digit = c.to_digit(10).map_or(0, |digit| digit as usize);
                let row = (digit + 9) % 10;
                return match self.items().get(row) {
                    Some(item) => MenuEvent::Open(Self::open(*item)),
                    None => MenuEvent::Pending,
                };
            }
            Resolved::Insert(c) => {
                self.filter.push(c);
                self.cursor = 0;
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
                    self.cursor = 0;
                }
                Action::Confirm => {
                    return match self.chosen() {
                        Some(item) => MenuEvent::Open(Self::open(item)),
                        None => MenuEvent::Pending,
                    };
                }
                Action::Disconnect => {
                    if let Some(Item::Host(host)) = self.chosen() {
                        return MenuEvent::Disconnect(host.alias.clone());
                    }
                }
                Action::Reload => return MenuEvent::Reload,
                Action::Cancel => return MenuEvent::Closed,
                _ => {}
            },
        }
        MenuEvent::Pending
    }

    /// The lines: the rows the filter shows, with a heading above the hosts.
    fn lines(items: &[Item<'_>]) -> Vec<Shown> {
        let mut lines = Vec::with_capacity(items.len() + 1);
        for (row, item) in items.iter().enumerate() {
            if matches!(item, Item::Host(_)) && !lines.contains(&Shown::Hosts) {
                lines.push(Shown::Hosts);
            }
            lines.push(Shown::Item(row));
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
        let (page, cursor, offset) = self.draw(frame, area, theme, decor, hosts, tick);
        (self.page, self.cursor, self.offset) = (page, cursor, offset);
    }

    /// Draws the menu; returns the lines on a page, and the cursor and the first line shown.
    fn draw(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        decor: Decor,
        hosts: &dyn Fn(&str) -> HostState,
        tick: u64,
    ) -> (usize, usize, usize) {
        let items = self.items();
        let lines = Self::lines(&items);
        let shown = u16::try_from(lines.len().max(1)).unwrap_or(u16::MAX);
        let colors = Colors::of(theme, false);
        let title = match self.side {
            Side::Left => fl!("menu-left"),
            Side::Right => fl!("menu-right"),
        };
        // Borders, the filter, the line under it, the rows.
        let size = (WIDTH, shown.saturating_add(4));
        let inner = draw_box(frame, area, size, &title, colors, theme);
        if inner.height < 3 || inner.width < 4 {
            return (self.page, self.cursor, self.offset);
        }
        let width = usize::from(inner.width);
        let line = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);

        let filter = fl!(
            "menu-filter",
            text = cells::sanitize(self.filter.as_bytes())
        );
        let filter = cells::fit(&filter, width, Align::Left);
        let column = u16::try_from(cells::width(filter.trim_end())).unwrap_or(0);
        frame.render_widget(Line::styled(filter, theme.dialog), line(0));
        frame.set_cursor_position(Position::new(
            (inner.x + column + 1).min(inner.right() - 1),
            inner.y,
        ));
        frame.render_widget(Line::styled("─".repeat(width), theme.dialog), line(1));

        let page = usize::from(inner.height - 2);
        let message = if !self.loaded {
            Some(self.error.clone().unwrap_or_else(|| fl!("panel-loading")))
        } else if items.is_empty() {
            Some(fl!("menu-nothing"))
        } else {
            None
        };
        if let Some(message) = message {
            let text = cells::fit(&message, width, Align::Left);
            frame.render_widget(Line::styled(text, theme.dialog), line(2));
            return (page, self.cursor, self.offset);
        }
        let cursor = self.cursor.min(items.len() - 1);
        let at = lines
            .iter()
            .position(|shown| *shown == Shown::Item(cursor))
            .unwrap_or(0);
        // The heading above the first host shows with it.
        let top = if at > 0 && lines[at - 1] == Shown::Hosts {
            at - 1
        } else {
            at
        };
        let offset = self.offset.min(top).max((at + 1).saturating_sub(page));

        let info_width = items
            .iter()
            .map(|item| cells::width(&info(*item, hosts)))
            .max()
            .unwrap_or(0)
            .min(width / 2);
        // A hotkey, a space, the name, a space, the information.
        let name_width = width.saturating_sub(info_width + 3);
        let hotkeys = self.filter.is_empty();
        let look = Look {
            theme,
            decor,
            hosts,
            tick,
            focused: colors.focused_style(),
        };
        for (index, shown) in lines.iter().skip(offset).take(page).enumerate() {
            let y = u16::try_from(index + 2).unwrap_or(u16::MAX);
            let text = match *shown {
                Shown::Hosts => {
                    let heading = format!("─ {} ", fl!("root-sftp"));
                    let rest = width.saturating_sub(cells::width(&heading));
                    Line::styled(format!("{heading}{}", "─".repeat(rest)), theme.dialog)
                }
                Shown::Item(row) => {
                    let hotkey = match row {
                        0..=9 if hotkeys => {
                            char::from_digit(u32::try_from((row + 1) % 10).unwrap_or(0), 10)
                                .unwrap_or(' ')
                        }
                        _ => ' ',
                    };
                    look.line(items[row], hotkey, row == cursor, (name_width, info_width))
                }
            };
            frame.render_widget(text, line(y));
        }
        (page, cursor, offset)
    }
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
    /// A row: its hotkey, its name with its prefix, and its information, in cells of `widths`.
    fn line(
        &self,
        item: Item<'_>,
        hotkey: char,
        selected: bool,
        (name_width, info_width): (usize, usize),
    ) -> Line<'static> {
        let (prefix, name, marker) = match item {
            Item::Home(..) => (self.decor.home().to_owned(), fl!("root-home"), None),
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
        };
        let name = cells::fit(&format!("{prefix}{name}"), name_width, Align::Left);
        let info = cells::fit(&info(item, self.hosts), info_width, Align::Right);
        if selected {
            return Line::styled(format!("{hotkey} {name} {info}"), self.focused);
        }
        let mut spans = vec![Span::raw(format!("{hotkey} "))];
        match marker {
            // The status marker of a host has a color of its own.
            Some(style) => {
                let split = name.chars().next().map_or(0, char::len_utf8);
                let (marker, rest) = name.split_at(split);
                spans.push(Span::styled(marker.to_owned(), style));
                spans.push(Span::raw(rest.to_owned()));
            }
            None => spans.push(Span::raw(name)),
        }
        spans.push(Span::raw(format!(" {info}")));
        Line::from(spans).style(self.theme.dialog)
    }
}

/// What identifies a row across listings.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Home,
    Volume(PathBuf),
    Host(String),
}

fn key(item: Item<'_>) -> Key {
    match item {
        Item::Home(..) => Key::Home,
        Item::Volume(volume) => Key::Volume(volume.mount_point.clone()),
        Item::Host(host) => Key::Host(host.alias.clone()),
    }
}

/// Right of a row: the free space of the home directory or a volume, or the address of a host.
fn info(item: Item<'_>, hosts: &dyn Fn(&str) -> HostState) -> String {
    let free = |space: Option<Space>| {
        space.map_or_else(
            || "?".to_owned(),
            |space| cells::size(space.available, FREE_WIDTH),
        )
    };
    match item {
        Item::Home(_, space) => free(space),
        Item::Volume(volume) => free(volume.space),
        Item::Host(host) => {
            let address = hosts(&host.alias).address.or_else(|| host.address.clone());
            cells::sanitize(address.unwrap_or_default().as_bytes())
        }
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

    fn host(alias: &str, label: Option<&str>, address: Option<&str>) -> RootHost {
        RootHost {
            alias: alias.to_owned(),
            label: label.map(str::to_owned),
            address: address.map(str::to_owned),
        }
    }

    fn listing() -> Listed {
        Listed {
            location: Location::Root,
            listing: Listing::Root {
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
            },
        }
    }

    /// A menu for the left panel on `current`, with its listing.
    fn menu_at(current: Location) -> LocationMenu {
        let mut menu = LocationMenu::new(Side::Left, current, PathBuf::from("/Users/me"), 1);
        menu.listed(1, Ok(listing()));
        menu
    }

    fn typed(menu: &mut LocationMenu, text: &str) {
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
        assert_eq!(at(Location::Sftp), Some(Key::Host("web".to_owned())));
        assert_eq!(at(Location::Root), Some(Key::Home));
    }

    #[test]
    fn enter_and_digits_open_home_volumes_and_hosts() {
        let mut menu = menu_at(Location::Root);
        assert_eq!(
            press(&mut menu, Action::Confirm),
            MenuEvent::Open(Location::Local(PathBuf::from("/Users/me")))
        );
        assert_eq!(
            menu.handle(Resolved::Insert('2')),
            MenuEvent::Open(Location::Local(PathBuf::from("/"))),
            "volumes open at their mount points"
        );
        assert_eq!(
            menu.handle(Resolved::Insert('3')),
            MenuEvent::Open(Location::Local(PathBuf::from("/Volumes/USB")))
        );
        assert_eq!(
            menu.handle(Resolved::Insert('6')),
            MenuEvent::Open(remote("db"))
        );
        assert_eq!(
            menu.handle(Resolved::Insert('9')),
            MenuEvent::Pending,
            "no ninth row"
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
        let mut menu = menu_at(Location::Root);
        typed(&mut menu, "pro");
        let keys: Vec<Key> = menu.items().into_iter().map(key).collect();
        assert_eq!(
            keys,
            [Key::Host("web".to_owned()), Key::Host("proxy".to_owned())],
            "labelled Prod, and proxy"
        );
        typed(&mut menu, "x");
        assert_eq!(menu.chosen().map(key), Some(Key::Host("proxy".to_owned())));
        // A digit is text once the filter has some.
        press(&mut menu, Action::Backspace);
        press(&mut menu, Action::Backspace);
        press(&mut menu, Action::Backspace);
        press(&mut menu, Action::Backspace);
        typed(&mut menu, "volumes/u");
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Volume(PathBuf::from("/Volumes/USB")))
        );
        typed(&mut menu, "1");
        assert!(menu.items().is_empty());
        let mut home = menu_at(Location::Root);
        typed(&mut home, "hom");
        assert_eq!(home.chosen().map(key), Some(Key::Home));
        assert_eq!(press(&mut menu, Action::Confirm), MenuEvent::Pending);
        let text = draw(&mut menu, 12);
        assert!(text.contains("Nothing matches"), "{text}");
    }

    #[test]
    fn f8_disconnects_hosts_only() {
        let mut menu = menu_at(remote("web"));
        assert_eq!(
            press(&mut menu, Action::Disconnect),
            MenuEvent::Disconnect("web".to_owned())
        );
        press(&mut menu, Action::Home);
        assert_eq!(press(&mut menu, Action::Disconnect), MenuEvent::Pending);
    }

    #[test]
    fn a_reload_keeps_the_cursor_on_its_row_and_stale_replies_are_dropped() {
        let mut menu = menu_at(remote("db"));
        menu.reload(2);
        let mut reordered = listing();
        if let Listing::Root { hosts, .. } = &mut reordered.listing {
            hosts.reverse();
        }
        menu.listed(
            1,
            Ok(Listed {
                location: Location::Root,
                listing: Listing::Hosts(Vec::new()),
            }),
        );
        menu.listed(2, Ok(reordered));
        assert_eq!(menu.chosen().map(key), Some(Key::Host("db".to_owned())));
        assert_eq!(menu.items().len(), 7);
    }

    #[test]
    fn host_icons_show_on_the_gray_of_mc_classic() {
        use ratatui::style::Color;

        let mut menu = menu_at(Location::Root);
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
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
        // Rows 9 … 11 hold the hosts, below the cursor on Home; their icons are in column 5.
        let colors = |y: u16| (buffer[(5, y)].fg, buffer[(5, y)].bg);
        assert_eq!(colors(9), (Color::Green, Color::Gray), "connected");
        assert_eq!(
            colors(10),
            (Color::Black, Color::Gray),
            "idle: the dialog's own color"
        );
        assert_eq!(colors(11), (Color::Red, Color::Gray), "failed");
        assert_eq!(buffer[(5, 10)].symbol(), "󰒋");
    }

    #[test]
    fn draws_volumes_then_hosts_with_hotkeys() {
        let mut menu = menu_at(remote("web"));
        insta::assert_snapshot!(draw(&mut menu, 14));
        let mut loading =
            LocationMenu::new(Side::Right, Location::Root, PathBuf::from("/Users/me"), 1);
        let text = draw(&mut loading, 10);
        assert!(
            text.contains("Right") && text.contains("Loading…"),
            "{text}"
        );
    }
}
