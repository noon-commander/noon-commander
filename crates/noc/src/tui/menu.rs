//! The location menu of `g l`, and of Ctrl+x `g l` for the other panel, as Far Manager's menu to
//! change drives, for one panel: groups of places under their headings, the home directory in
//! Favorites, the volumes in Mounts, and the hosts of the ssh config in SFTP. Mounts and SFTP put
//! those used last first and show the first ten, with a row that shows the rest; the filter finds
//! them all. Typing filters the menu; while the filter is empty, `1` … `9` and `0` open the first
//! ten places. The sizes of a volume that did not answer in time come later, a skeleton in their
//! place until then.

use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noc_config::{Recent, SearchMode};
use noc_scroll::scrollbar::Bar;
use noc_vfs::{LateAnswer, Location, Volume, VolumeKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::app::Side;
use super::cells::{self, Align, Cut};
use super::decor::Decor;
use super::keymap::{Action, Mode, Resolved};
use super::loading::Loading;
use super::mouse::Pointer;
use super::panel::{HostState, HostStatus, home_text};
use super::picker::{Body, MenuLine, Pick, Picker, ShownRow, Shows};
use super::places::{KnownHost, Places, volume_name, volume_of};
use super::search::{Input, Kind};
use super::spinner::{Shine, Spin, Wait};
use super::theme::Theme;
use super::window::{self, Colors};
use crate::i18n::fl;

/// Places of a group that show while the filter is empty, besides those always shown.
const SHOWN: usize = 10;

/// Cells of the skeleton on the first line of a card in place of what is still to come, the
/// address of a host or what a volume mounts: as many as an address such as `user@10.0.0.5`
/// takes.
const SKELETON: usize = 16;

/// How long the sizes of a volume may take, from when it was asked, before its row gives them
/// up and says that it does not respond.
pub(crate) const NO_ANSWER: Duration = Duration::from_secs(5);

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
    /// View the `ssh_config` file that defines a host, from the line of its `Host`.
    ViewConfig {
        file: PathBuf,
        line: usize,
    },
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
    /// The home directory, with the volume that holds it.
    Home(&'a Path, Option<&'a Volume>),
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
    contents: Contents,
    /// The listing; its lines are those with every group as it opens.
    loading: Loading,
    /// The wait for the sizes of the volumes that did not answer in time, whose shine crosses
    /// all their skeletons at once.
    sizes: Wait,
    /// The wait for the address of a host from `ssh -G`, whose shine crosses its skeleton.
    address: Wait,
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
    /// Where `ssh -G` said hosts connect to in this run, which the filter finds them by.
    addresses: HashMap<String, String>,
    /// Groups that show all their places.
    expanded: HashSet<Group>,
}

impl LocationMenu {
    /// A menu for the panel on `side`, which shows `current`, waiting for the listing
    /// `generation`. Its first row opens `home`.
    pub(crate) fn new(side: Side, current: Location, home: PathBuf, generation: u64) -> Self {
        let mut menu = Self {
            side,
            contents: Contents {
                current,
                home,
                places: Places::default(),
                recent: Recent::default(),
                connected: HashSet::new(),
                addresses: HashMap::new(),
                expanded: HashSet::new(),
            },
            loading: Loading::new(generation),
            sizes: Wait::default(),
            address: Wait::default(),
            picker: Picker::new(Input::new(Kind::FILTER, SearchMode::Plain)),
        };
        // The home directory, before the listing.
        menu.picker.set_rows_showing(menu.texts());
        menu
    }

    /// The same menu, whose filter matches as `input` says, as it was last.
    pub(crate) fn with_input(mut self, input: Input) -> Self {
        self.picker.set_input(input);
        self
    }

    /// The same menu, with the hosts and volumes in `recent` first in their groups.
    pub(crate) fn recent(mut self, recent: &Recent) -> Self {
        self.contents.recent = recent.clone();
        self
    }

    /// The same menu, whose skeleton has as many `lines` as the last menu had, if any.
    pub(crate) fn lines_like(mut self, lines: Option<u16>) -> Self {
        self.loading = self.loading.lines_like(lines);
        self
    }

    /// Lines of the last listing, for the skeleton of the next menu: with the filter empty and
    /// every group as it opens.
    pub(crate) fn lines_listed(&self) -> Option<u16> {
        self.loading.listed()
    }

    /// The panel the menu is for.
    pub(crate) fn side(&self) -> Side {
        self.side
    }

    /// Waits for the listing `generation` instead, for a reload. The rows stay until it comes.
    pub(crate) fn reload(&mut self, generation: u64) {
        self.loading.reload(generation);
    }

    /// The wait for a listing, the first or one asked for again, whose dots turn in front of
    /// the title.
    pub(crate) fn wait(&self) -> Option<&Wait> {
        self.loading.wait()
    }

    /// The wait for the sizes of volumes, while some are still to come at `at` and not given
    /// up for yet: its shine crosses their skeletons. No dots turn in the title for it.
    pub(crate) fn sizes_wait(&self, at: Instant) -> Option<&Wait> {
        (self.contents.places.waiting.values())
            .any(|since| at.saturating_duration_since(*since) < NO_ANSWER)
            .then_some(&self.sizes)
    }

    /// The wait for the address of the host under the cursor, while `ssh -G` has not told it,
    /// nor failed for it, in this run, as `told` says: its shine crosses the skeleton in the
    /// card.
    pub(crate) fn address_wait(&self, told: &dyn Fn(&str) -> bool) -> Option<&Wait> {
        match self.chosen()? {
            Item::Host(host) if !told(&host.alias) => Some(&self.address),
            Item::Home(..) | Item::Volume(_) | Item::Host(_) | Item::More(..) => None,
        }
    }

    /// When the next sizes still to come after `at` are given up for, so that their skeleton
    /// turns to the words that the volume does not respond even where nothing else draws the
    /// screen.
    pub(crate) fn gives_up(&self, at: Instant) -> Option<Instant> {
        (self.contents.places.waiting.values())
            .map(|since| *since + NO_ANSWER)
            .filter(|gives_up| *gives_up > at)
            .min()
    }

    /// Takes the answer of the volume at `mount_point`, listed without it: the volume as it is
    /// now, not one to show, or failed, which leaves it as one that does not respond. The cursor
    /// stays on its row.
    pub(crate) fn answered(&mut self, mount_point: &Path, answer: LateAnswer) {
        let places = &mut self.contents.places;
        let Some(index) =
            (places.volumes.iter()).position(|shown| shown.mount_point == mount_point)
        else {
            return;
        };
        places.waiting.remove(mount_point);
        match answer {
            LateAnswer::Volume(volume) => places.volumes[index] = volume,
            LateAnswer::Gone => {
                places.volumes.remove(index);
            }
            LateAnswer::Failed => {
                places.failed.insert(mount_point.to_path_buf());
            }
        }
        self.refresh();
    }

    /// Takes the hosts that are connected, connecting, or stalled, which always show. The
    /// cursor stays on its row.
    pub(crate) fn set_connected(&mut self, connected: HashSet<String>) {
        if self.contents.connected != connected {
            self.contents.connected = connected;
            self.refresh();
        }
    }

    /// Takes where `ssh -G` said hosts connect to in this run, which the filter finds them by.
    /// The cursor stays on its row.
    pub(crate) fn set_addresses(&mut self, addresses: HashMap<String, String>) {
        if self.contents.addresses != addresses {
            self.contents.addresses = addresses;
            self.refresh();
        }
    }

    /// Takes the places of the listing `generation`. The first one puts the cursor on where
    /// the panel is.
    pub(crate) fn listed(&mut self, generation: u64, result: Result<Places, String>) {
        if !self.loading.answers(generation) {
            return;
        }
        match result {
            Ok(places) => {
                // A shine of its own for sizes that come again after the last had come.
                if self.contents.places.waiting.is_empty() && !places.waiting.is_empty() {
                    self.sizes = Wait::default();
                }
                self.contents.places = places;
                let expanded = std::mem::take(&mut self.contents.expanded);
                let items: Vec<Item<'_>> = (self.contents.rows().into_iter())
                    .filter(|(_, shows)| *shows != Shows::Filtered)
                    .map(|(item, _)| item)
                    .collect();
                let lines = Self::lines(&items, false).len();
                self.contents.expanded = expanded;
                let first = !self.loading.loaded();
                self.loading.came(Some(lines));
                if first {
                    self.picker.set_rows_showing(self.texts());
                    let row = self.contents.current_row(&self.items());
                    self.picker.set_cursor(row);
                } else {
                    self.refresh();
                }
            }
            Err(reason) => self.loading.failed(&reason),
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
            .map(|(item, shows)| (texts(item, &self.contents.addresses), shows))
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

    /// The `ssh_config` file that defines the host under the cursor, and the line of its
    /// `Host`, from 1.
    pub(crate) fn chosen_config(&self) -> Option<(&Path, usize)> {
        match self.chosen()? {
            Item::Host(host) => Some((&host.file, host.line)),
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

    /// The kind of its filter while that takes the keys.
    pub(crate) fn search(&self) -> Option<Kind> {
        self.picker.search()
    }

    /// How its filter matches now, and whether case counts.
    pub(crate) fn input(&self) -> Input {
        self.picker.input()
    }

    /// Takes a key: the picker's, and F3, which views the `ssh_config` of a host, F4, which
    /// edits the settings of a host, F8, which disconnects it, and R, which reads the volumes
    /// and hosts again. Enter on the row of the rest of a group shows them.
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
            Pick::Other(Action::View) => match self.chosen_config() {
                Some((file, line)) => MenuEvent::ViewConfig {
                    file: file.to_path_buf(),
                    line,
                },
                None => MenuEvent::Pending,
            },
            Pick::Other(Action::Reload) => MenuEvent::Reload,
            Pick::Pending | Pick::Filtered | Pick::Other(_) => MenuEvent::Pending,
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

    /// The lines: the rows the filter shows, with the heading of each group above its first;
    /// without headings while they are `ranked`, best first, out of their groups.
    fn lines(items: &[Item<'_>], ranked: bool) -> Vec<MenuLine> {
        let mut lines = Vec::with_capacity(items.len() + 3);
        let mut group = None;
        for (row, item) in items.iter().enumerate() {
            if !ranked && group != Some(item.group()) {
                group = Some(item.group());
                lines.push(MenuLine::Heading(item.group().heading()));
            }
            lines.push(MenuLine::Row(row));
        }
        lines
    }

    /// Draws the menu centered over `area`, the panels: the filter, then the rows, scrolled to
    /// the cursor.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        decor: Decor,
        hosts: &dyn Fn(&str) -> HostState,
        spin: &Spin,
    ) {
        let rows = self.contents.rows();
        let items = self.picker.shown(rows.into_iter().map(|(item, _)| item));
        let lines = Self::lines(&items, self.picker.ranked());
        let title = fl!("menu-title");
        let message = (self.loading.error())
            .filter(|_| !self.loading.loaded())
            .map(str::to_owned);
        let note = self.note(theme);
        let now = spin.at();
        let Places {
            waiting, failed, ..
        } = &self.contents.places;
        // One whose answer failed after it hung has not answered either.
        let asked = |mount_point: &Path| {
            let ago = waiting
                .get(mount_point)
                .map(|since| now.saturating_duration_since(*since));
            ago.or_else(|| failed.contains(mount_point).then_some(NO_ANSWER))
        };
        let sizes: Vec<Option<Sizes>> = items.iter().map(|item| sizes(*item, &asked)).collect();
        let columns = SizeColumns::of(sizes.iter().flatten());
        let infos: Vec<Info> = (sizes.iter())
            .map(|sizes| info(sizes.as_ref(), columns))
            .collect();
        let widest = infos.iter().map(Info::width).max().unwrap_or(0);
        let look = Look {
            theme,
            decor,
            hosts,
            dots: spin.dots(),
            shine: spin.shine(&self.sizes),
            address_shine: spin.shine(&self.address),
            focused: Colors::plain(theme).focused_style(),
        };
        // No card while the skeleton stands for the rows: it takes the lines of the card too.
        let cursor = self.picker.cursor().min(items.len().saturating_sub(1));
        let chosen = (items.get(cursor).copied()).filter(|_| self.loading.skeleton().is_none());
        let (footer, footer_label) = chosen.map_or((Vec::new(), None), |item| {
            let hidden = match item {
                Item::More(group, _) => self.contents.hidden(group),
                Item::Home(..) | Item::Volume(_) | Item::Host(_) => Vec::new(),
            };
            let room = (
                window::inner_width(window::WIDTH, area),
                &*self.contents.home,
            );
            let sizes = sizes.get(cursor).and_then(Option::as_ref);
            let (lines, label) = look.card(item, sizes, &hidden, room);
            let label = label.map(|label| Line::styled(label, Colors::plain(theme).title));
            (lines, label)
        });
        let body = Body {
            title: &title,
            width: window::WIDTH,
            lines: &lines,
            note,
            message,
            loading: Some((&self.loading, spin)),
            footer,
            footer_label,
            has_full: true,
            ..Body::default()
        };
        self.picker.render(frame, area, theme, body, &mut |row| {
            let info_width = widest.min(row.width / 2);
            // The name, a space, the information.
            let name_width = row.width.saturating_sub(info_width + 1);
            look.line(
                items[row.index],
                &infos[row.index],
                row,
                (name_width, info_width),
            )
        });
    }

    /// Right of the filter once the rows show: why listing them again failed.
    fn note(&self, theme: &Theme) -> Option<Line<'static>> {
        let reason = self.loading.error().filter(|_| self.loading.loaded())?;
        let text = fl!("menu-reload-failed", reason = reason);
        Some(Line::styled(
            text,
            theme.dialog.patch(theme.dialog_error_text),
        ))
    }
}

impl Contents {
    /// The names of the places of `group` that its row of the rest stands for, in their order.
    fn hidden(&self, group: Group) -> Vec<String> {
        (self.rows().into_iter())
            .filter(|(item, shows)| *shows == Shows::Filtered && item.group() == group)
            .filter_map(|(item, _)| match item {
                Item::Volume(volume) => Some(volume_name(volume)),
                Item::Host(host) => Some(cells::sanitize(host.alias.as_bytes())),
                Item::Home(..) | Item::More(..) => None,
            })
            .collect()
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
        let home = volume_of(&self.places.volumes, &self.home);
        let mut rows = vec![(Item::Home(&self.home, home), Shows::Always)];
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

/// `line` after `lead`.
fn after(lead: Span<'static>, line: Line<'static>) -> Line<'static> {
    let mut spans = vec![lead];
    spans.extend(line.spans);
    Line::from(spans)
}

/// How rows are drawn.
struct Look<'a> {
    theme: &'a Theme,
    decor: Decor,
    hosts: &'a dyn Fn(&str) -> HostState,
    /// The frame of the spinner of hosts that connect.
    dots: char,
    /// The shine over the skeletons of sizes still to come, and of what volumes mount.
    shine: Option<Shine<'a>>,
    /// The shine over the skeleton of the address of a host still to come.
    address_shine: Option<Shine<'a>>,
    /// The row under the cursor.
    focused: Style,
}

impl<'a> Look<'a> {
    /// A row: its name with its prefix, and its `info`, in cells of `widths`.
    fn line(
        &self,
        item: Item<'_>,
        info: &Info,
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
                (
                    self.decor.host(status, self.dots),
                    cells::sanitize(host.alias.as_bytes()),
                    Some(self.theme.dialog_host_status(status)),
                )
            }
            Item::More(_, count) => (self.decor.more(), fl!("menu-more", count = count), None),
        };
        let styles = if selected {
            (self.focused, self.theme.truncated_cursor)
        } else if let Item::More(..) = item {
            self.theme.dialog_more()
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
        // The words in place of sizes have colors of their own, but under the cursor.
        let words = |style: Style| {
            if selected {
                styles
            } else {
                (styles.0.patch(style), styles.1)
            }
        };
        match info {
            Info::Text(text) => {
                spans.extend(cells::fit(text, info_width, Align::Right, styles, marks));
            }
            Info::NotResponding(text) => {
                let styles = words(self.theme.dialog_warning_text);
                spans.extend(cells::fit(text, info_width, Align::Right, styles, marks));
            }
            Info::Unknown(text) => {
                let styles = words(self.theme.disabled);
                spans.extend(cells::fit(text, info_width, Align::Right, styles, marks));
            }
            Info::Waiting(width) => {
                let width = (*width).min(info_width);
                spans.push(Span::styled(" ".repeat(info_width - width), styles.0));
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                let skeleton = window::cell_skeleton(width, styles.0, self.theme, self.shine);
                spans.extend(skeleton);
            }
        }
        Line::from(spans).style(styles.0)
    }

    /// The card of `item` under the rows, in `width` cells: its two lines, what it is and how
    /// it is, and what goes on the line over them. `sizes` are those of the volume of a volume
    /// or of the home directory, `hidden` the names of the places a row of the rest stands for;
    /// `home` shortens the path of the ssh config. The first line starts with the icon of the
    /// row, in its colors, then says what the place is, a path or an address, and against its
    /// right end where it comes from: the volume of the home directory, what a volume mounts, the
    /// `Host` line of a host. The second stands under its text.
    fn card(
        &self,
        item: Item<'_>,
        sizes: Option<&Sizes>,
        hidden: &[String],
        (width, home): (usize, &Path),
    ) -> (Vec<Line<'static>>, Option<String>) {
        let dialog = self.theme.dialog;
        let text = |text: &str, cut: Cut, width: usize| {
            let styles = (dialog, self.theme.truncated_dialog);
            cells::fitted(
                text,
                width,
                (Align::Left, cut),
                styles,
                self.theme.glyphs.marks,
            )
        };
        // The icon in `style`, as many blanks for the line under it, and the cells after them.
        let lead = |icon: String, style: Style| {
            let room = width.saturating_sub(cells::width(&icon));
            let indent = Span::styled(" ".repeat(width - room), dialog);
            (Span::styled(icon, style), indent, room)
        };
        match item {
            Item::Home(path, volume) => {
                let label = volume.map(volume_label);
                let path = cells::sanitize(path.as_os_str().as_bytes());
                let start = End::Text(path, dialog, Cut::Start);
                let end = volume.map_or(End::Nothing, |volume| {
                    End::Text(volume_name(volume), dialog, Cut::Start)
                });
                let (icon, indent, room) = lead(self.decor.home().to_owned(), dialog);
                let lines = vec![
                    after(icon, self.first_line(start, end, room)),
                    after(indent, self.sizes_line(sizes, room)),
                ];
                (lines, label)
            }
            Item::Volume(volume) => {
                let label = volume_label(volume);
                let mount_point = cells::sanitize(volume.mount_point.as_os_str().as_bytes());
                let start = End::Text(mount_point, dialog, Cut::Start);
                let icon = self.decor.volume(volume.kind).to_owned();
                let (icon, indent, room) = lead(icon, dialog);
                let lines = vec![
                    after(
                        icon,
                        self.first_line(start, self.source(volume, sizes), room),
                    ),
                    after(indent, self.sizes_line(sizes, room)),
                ];
                (lines, Some(label))
            }
            Item::Host(host) => {
                let state = (self.hosts)(&host.alias);
                let file = home_text(&host.file, home);
                let defined = fl!("menu-card-defined", file = file, line = host.line);
                let end = End::Text(defined, dialog, Cut::Start);
                let status = dialog.patch(self.theme.dialog_host_status(state.status));
                let (icon, indent, room) = lead(self.decor.host(state.status, self.dots), status);
                let lines = vec![
                    after(icon, self.first_line(self.reach(&state), end, room)),
                    self.status_line(&state, indent, width),
                ];
                (lines, Some(fl!("menu-card-sftp")))
            }
            Item::More(..) => {
                let (icon, indent, room) = lead(self.decor.more(), dialog);
                let (first, second) = two_lines(hidden, room);
                let lines = vec![
                    after(icon, text(&first, Cut::End, room)),
                    after(indent, text(&second, Cut::End, room)),
                ];
                (lines, None)
            }
        }
    }

    /// The first line of a card, in `width` cells: `start`, what the place is, and `end`, where
    /// it comes from, against the right end, which takes what the start leaves, but at least half
    /// the line.
    fn first_line(&self, start: End<'_>, end: End<'_>, width: usize) -> Line<'static> {
        let (start_width, end_width) = ends(width, start.wanted(), end.wanted());
        let mut spans = self.end_spans(start, start_width, Align::Left);
        spans.push(Span::styled(
            " ".repeat(width - start_width - end_width),
            self.theme.dialog,
        ));
        spans.extend(self.end_spans(end, end_width, Align::Right));
        Line::from(spans)
    }

    /// `end` in `width` cells, against the side `align` says.
    fn end_spans(&self, end: End<'_>, width: usize, align: Align) -> Vec<Span<'static>> {
        let dialog = self.theme.dialog;
        match end {
            End::Text(text, style, cut) => {
                let fitted = cells::fit_cut(&text, width, align, cut);
                let styles = (style, self.theme.truncated_dialog);
                cells::spans(fitted, styles, self.theme.glyphs.marks)
            }
            End::Skeleton(shine) => {
                let skeleton = SKELETON.min(width);
                let cells = u16::try_from(skeleton).unwrap_or(u16::MAX);
                let mut spans = window::cell_skeleton(cells, dialog, self.theme, shine);
                let pad = Span::styled(" ".repeat(width - skeleton), dialog);
                match align {
                    Align::Right => spans.insert(0, pad),
                    Align::Left | Align::Center => spans.push(pad),
                }
                spans
            }
            End::Nothing => vec![Span::styled(" ".repeat(width), dialog)],
        }
    }

    /// How to reach a host as `state` has it: its address and the host ssh jumps through, from
    /// this run's `ssh -G`, else that it failed, in the colors of errors, why going to the log
    /// alone; else a skeleton, as `ssh -G` runs for every host the cursor rests on: no text that
    /// would show for the moment before it starts. The address loses its middle.
    fn reach(&self, state: &HostState) -> End<'a> {
        let dialog = self.theme.dialog;
        if let Some(address) = &state.address {
            let address = cells::sanitize(address.as_bytes());
            let text = match &state.jump {
                Some(jump) => fl!("menu-card-via", address = address, jump = jump.as_str()),
                None => address,
            };
            End::Text(text, dialog, Cut::Middle)
        } else if state.unresolved {
            let style = dialog.patch(self.theme.dialog_error_text);
            End::Text(fl!("menu-card-not-resolved"), style, Cut::End)
        } else {
            End::Skeleton(self.address_shine)
        }
    }

    /// What `volume` mounts, as `sizes` tell how it answered: a skeleton while its answer is
    /// to come; `source unknown` in the colors of warnings once it has not answered, and faint
    /// where it answered without. A device or a share loses its start.
    fn source(&self, volume: &Volume, sizes: Option<&Sizes>) -> End<'a> {
        let dialog = self.theme.dialog;
        if let Some(source) = &volume.source {
            return End::Text(cells::sanitize(source.as_bytes()), dialog, Cut::Start);
        }
        let style = match sizes {
            Some(Sizes::Waiting) => return End::Skeleton(self.shine),
            Some(Sizes::NotResponding) => self.theme.dialog_warning_text,
            Some(Sizes::Known { .. } | Sizes::Unknown) | None => self.theme.disabled,
        };
        End::Text(
            fl!("menu-card-source-unknown"),
            dialog.patch(style),
            Cut::End,
        )
    }

    /// How the space of a volume is, in `width` cells: its free space and its size, a skeleton
    /// while they are still to come, that it does not respond, in the colors of warnings, or,
    /// faint, that it tells none.
    fn sizes_line(&self, sizes: Option<&Sizes>, width: usize) -> Line<'static> {
        let dialog = self.theme.dialog;
        let (text, style) = match sizes {
            Some(Sizes::Known {
                free,
                total,
                percent,
            }) => {
                let (free, total, percent) = (free.clone(), total.clone(), *percent);
                let text = fl!(
                    "menu-card-space",
                    free = free,
                    total = total,
                    percent = percent
                );
                (text, dialog)
            }
            Some(Sizes::Waiting) => {
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                return window::status_skeleton(width, dialog, self.theme, self.shine);
            }
            Some(Sizes::NotResponding) => (
                fl!("menu-card-not-responding"),
                dialog.patch(self.theme.dialog_warning_text),
            ),
            Some(Sizes::Unknown) | None => (
                fl!("menu-card-size-unknown"),
                dialog.patch(self.theme.disabled),
            ),
        };
        let styles = (style, self.theme.truncated_dialog);
        let fit = (Align::Left, Cut::End);
        cells::fitted(&text, width, fit, styles, self.theme.glyphs.marks)
    }

    /// How a host is, in `width` cells, after `indent`: not connected; connected; connecting; or
    /// why its last attempt failed, in the colors of errors.
    fn status_line(&self, state: &HostState, indent: Span<'static>, width: usize) -> Line<'static> {
        let dialog = self.theme.dialog;
        let (text, style) = match state.status {
            HostStatus::Idle => (fl!("menu-card-not-connected"), dialog),
            HostStatus::Connected => (fl!("menu-card-connected"), dialog),
            HostStatus::Connecting => (fl!("menu-card-connecting"), dialog),
            HostStatus::Failed | HostStatus::Stalled => {
                let reason = state.reason.clone();
                (
                    reason.unwrap_or_else(|| fl!("menu-card-failed")),
                    dialog.patch(self.theme.dialog_error_text),
                )
            }
        };
        let room = width.saturating_sub(cells::width(&indent.content));
        let mut spans = vec![indent];
        let fitted = cells::fit_cut(&text, room, Align::Left, Cut::End);
        let styles = (style, self.theme.truncated_dialog);
        spans.extend(cells::spans(fitted, styles, self.theme.glyphs.marks));
        Line::from(spans)
    }
}

/// One end of the first line of a card.
enum End<'a> {
    /// Text in a style, which loses its start, its middle, or its end where it is too long.
    Text(String, Style, Cut),
    /// What is still to come: a skeleton of [`SKELETON`] cells at most, with its shine.
    Skeleton(Option<Shine<'a>>),
    Nothing,
}

impl End<'_> {
    /// The cells it would take.
    fn wanted(&self) -> usize {
        match self {
            Self::Text(text, ..) => cells::width(text),
            Self::Skeleton(_) => SKELETON,
            Self::Nothing => 0,
        }
    }
}

/// What a volume is, on the line over its card, and over the card of the home directory on it:
/// the type of its file system and its kind, or its kind alone where the type is not known.
fn volume_label(volume: &Volume) -> String {
    let kind = match volume.kind {
        VolumeKind::System => fl!("menu-card-system"),
        VolumeKind::Local => fl!("menu-card-local"),
        VolumeKind::Network => fl!("menu-card-network"),
    };
    match &volume.fs_type {
        Some(fs_type) => fl!(
            "menu-card-both",
            first = cells::sanitize(fs_type.as_bytes()),
            second = kind
        ),
        None => kind,
    }
}

/// How many cells of a line of `width` its start and its end take, for an end of `wanted`
/// cells after a start of `lead`: the end what the start leaves after a blank, but at least half
/// the line, so that a long start does not crowd it out; the start the rest but the blank.
fn ends(width: usize, lead: usize, wanted: usize) -> (usize, usize) {
    if wanted == 0 {
        return (width, 0);
    }
    let end = wanted.min(width.saturating_sub(lead + 1).max(width / 2));
    (width.saturating_sub(end + 1), end)
}

/// `names` over two lines of `width` cells, between separators: as many whole names as fit on
/// the first, at least one, and the rest on the second, which loses its end where it is too
/// long.
fn two_lines(names: &[String], width: usize) -> (String, String) {
    let separator = fl!("menu-card-separator");
    let after_last = separator.trim_end();
    let mut first = String::new();
    let mut taken = 0;
    for name in names {
        let joined = if taken == 0 {
            name.clone()
        } else {
            format!("{first}{separator}{name}")
        };
        let more = taken + 1 < names.len();
        let shown = cells::width(&joined) + if more { cells::width(after_last) } else { 0 };
        if taken > 0 && shown > width {
            break;
        }
        first = joined;
        taken += 1;
    }
    if taken < names.len() {
        first.push_str(after_last);
    }
    (first, names[taken..].join(&separator))
}

/// What the filter finds a row by; a host also by its address in `addresses`.
fn texts(item: Item<'_>, addresses: &HashMap<String, String>) -> Vec<String> {
    match item {
        Item::Home(home, _) => vec![fl!("menu-home"), home.to_string_lossy().into_owned()],
        Item::Volume(volume) => vec![
            volume_name(volume),
            volume.mount_point.to_string_lossy().into_owned(),
        ],
        Item::Host(host) => [Some(&host.alias), addresses.get(&host.alias)]
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

/// The free space and the size of the volume of a row, as far as they are known.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Sizes {
    /// As the row shows them, and the share free, in percent.
    Known {
        free: String,
        total: String,
        percent: u64,
    },
    /// Still to come from a volume that did not answer in time.
    Waiting,
    /// Given up for: the volume did not answer in [`NO_ANSWER`], or its answer failed.
    NotResponding,
    /// The volume told none.
    Unknown,
}

/// The sizes of the volume of a row, a volume or the home directory; none for a host or the row
/// of the rest of a group. A volume whose mount point was `asked` so long ago, and has not
/// answered, has them still to come, or given up for.
fn sizes(item: Item<'_>, asked: &dyn Fn(&Path) -> Option<Duration>) -> Option<Sizes> {
    let volume = match item {
        Item::Home(_, volume) => volume,
        Item::Volume(volume) => Some(volume),
        Item::Host(_) | Item::More(..) => return None,
    };
    // Pseudo file systems report no size.
    let space = (volume.and_then(|volume| volume.space)).filter(|space| space.total > 0);
    let ago = volume.and_then(|volume| asked(&volume.mount_point));
    Some(match (space, ago) {
        (Some(space), _) => {
            let free = u128::from(space.available.min(space.total)) * 100;
            Sizes::Known {
                free: super::panel::short_size(space.available),
                total: super::panel::short_size(space.total),
                percent: u64::try_from(free / u128::from(space.total)).unwrap_or(100),
            }
        }
        (None, Some(ago)) if ago < NO_ANSWER => Sizes::Waiting,
        (None, Some(_)) => Sizes::NotResponding,
        (None, None) => Sizes::Unknown,
    })
}

/// The cells of the free space and of the size of the rows, each of which takes as many, so
/// that the slashes between them line up: as many as the widest sizes known, or as many as
/// sizes take at most while none is known but some are still to come.
#[derive(Debug, Clone, Copy, Default)]
struct SizeColumns {
    free: usize,
    total: usize,
}

impl SizeColumns {
    fn of<'a>(sizes: impl Iterator<Item = &'a Sizes>) -> Self {
        let mut columns = Self::default();
        let (mut known, mut waiting) = (false, false);
        for sizes in sizes {
            match sizes {
                Sizes::Known { free, total, .. } => {
                    known = true;
                    columns.free = columns.free.max(cells::width(free));
                    columns.total = columns.total.max(cells::width(total));
                }
                Sizes::Waiting => waiting = true,
                Sizes::NotResponding | Sizes::Unknown => {}
            }
        }
        if waiting && !known {
            let widest = cells::short_size_width(&noc_locale::current());
            columns = Self {
                free: widest,
                total: widest,
            };
        }
        columns
    }

    /// Cells of the sizes of a row, the slash between them too.
    fn width(self) -> usize {
        let between = fl!("menu-space", free = "", total = "");
        self.free + cells::width(&between) + self.total
    }

    /// `free` and `total` in their columns, with the slash between them.
    fn text(self, free: &str, total: &str) -> String {
        let right = |text: &str, width: usize| {
            let pad = width.saturating_sub(cells::width(text));
            format!("{}{text}", " ".repeat(pad))
        };
        fl!(
            "menu-space",
            free = right(free, self.free),
            total = right(total, self.total)
        )
    }
}

/// What a row shows right of its name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Info {
    Text(String),
    /// A skeleton of so many cells, where the sizes of a volume are still to come.
    Waiting(usize),
    /// In place of the sizes of a volume: that it does not respond, which warns.
    NotResponding(String),
    /// In place of the sizes of a volume: that it tells none, faint.
    Unknown(String),
}

impl Info {
    fn width(&self) -> usize {
        match self {
            Self::Text(text) | Self::NotResponding(text) | Self::Unknown(text) => {
                cells::width(text)
            }
            Self::Waiting(width) => *width,
        }
    }
}

/// Right of a row: the `sizes` of the volume of the home directory or of a volume, in
/// `columns`; nothing for a host, whose address is in its card.
fn info(sizes: Option<&Sizes>, columns: SizeColumns) -> Info {
    match sizes {
        Some(Sizes::Known { free, total, .. }) => Info::Text(columns.text(free, total)),
        Some(Sizes::Waiting) => Info::Waiting(columns.width()),
        Some(Sizes::NotResponding) => Info::NotResponding(fl!("menu-space-not-responding")),
        Some(Sizes::Unknown) => Info::Unknown(fl!("menu-space-unknown")),
        None => Info::Text(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use noc_vfs::{Space, VolumeKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::glyphs::Glyphs;
    use crate::tui::panel::HostStatus;
    use crate::tui::picker::FULL_ROWS;
    use crate::tui::spinner;

    fn volume(path: &str, label: Option<&str>, kind: VolumeKind) -> Volume {
        Volume {
            mount_point: PathBuf::from(path),
            label: label.map(str::to_owned),
            fs_type: Some("apfs".to_owned()),
            source: None,
            kind,
            space: Some(Space {
                total: 994 << 30,
                available: 212 << 30,
            }),
        }
    }

    fn host(alias: &str) -> KnownHost {
        KnownHost {
            alias: alias.to_owned(),
            file: PathBuf::from("/Users/me/.ssh/config"),
            line: 1,
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
            hosts: vec![host("web"), host("db"), host("proxy")],
            ..Places::default()
        }
    }

    /// `count` hosts, `h01` … in config order.
    fn many_hosts(count: usize) -> Places {
        Places {
            volumes: vec![volume("/", None, VolumeKind::System)],
            hosts: (1..=count)
                .map(|index| host(&format!("h{index:02}")))
                .collect(),
            ..Places::default()
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

    /// Spinners that stand still.
    fn still() -> Spin {
        let now = Instant::now();
        Spin::new(now, now, false, Glyphs::PLAIN.spinners)
    }

    fn draw(menu: &mut LocationMenu, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(50, height)).unwrap();
        let hosts = |alias: &str| HostState {
            status: if alias == "web" {
                HostStatus::Connected
            } else {
                HostStatus::Idle
            },
            address: (alias == "web").then(|| "deploy@10.0.0.5".to_owned()),
            ..HostState::default()
        };
        // Drawn again once what it waits for shows, with a still spinner.
        let start = Instant::now();
        for at in [start, start + spinner::DELAY] {
            let spin = Spin::new(at, start, false, Glyphs::PLAIN.spinners);
            terminal
                .draw(|frame| {
                    menu.render(
                        frame,
                        frame.area(),
                        &Theme::terminal(),
                        Decor::new(false),
                        &hosts,
                        &spin,
                    );
                })
                .unwrap();
        }
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
        press(&mut menu, Action::NormalMode);
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
    fn typing_filters_by_name_mount_point_and_address() {
        let mut menu = menu_at(home());
        typed(&mut menu, "pro");
        assert_eq!(keys(&menu), [Key::Host("proxy".to_owned())]);
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
        let mut at_home = menu_at(home());
        typed(&mut at_home, "hom");
        assert_eq!(at_home.chosen().map(key), Some(Key::Home));
        assert_eq!(press(&mut menu, Action::Confirm), MenuEvent::Pending);
        let text = draw(&mut menu, 12);
        assert!(text.contains("Nothing matches"), "{text}");

        let mut menu = menu_at(home());
        typed(&mut menu, "admin@10");
        assert!(menu.items().is_empty(), "no address until ssh -G tells it");
        menu.set_addresses([("db".to_owned(), "admin@10.0.0.7".to_owned())].into());
        let told = "the address ssh -G told in this run";
        assert_eq!(keys(&menu), [Key::Host("db".to_owned())], "{told}");
    }

    #[test]
    fn a_fuzzy_filter_ranks_the_places_best_first_out_of_their_groups() {
        let mut menu = LocationMenu::new(Side::Left, home(), PathBuf::from("/Users/me"), 1)
            .with_input(Input::new(Kind::FILTER, SearchMode::Fzf));
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
        assert!(draw(&mut menu, 20).contains("Mounts"), "in their groups");
        typed(&mut menu, "sh");
        assert_eq!(
            keys(&menu),
            [
                Key::Volume(PathBuf::from("/Volumes/share")),
                Key::Volume(PathBuf::from("/"))
            ],
            "the start of a word, in share, beats the end of one, in Macintosh HD"
        );
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Volume(PathBuf::from("/Volumes/share"))),
            "on the best"
        );
        let text = draw(&mut menu, 20);
        assert!(!text.contains("Mounts"), "no headings: {text}");
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
    fn f3_views_the_ssh_config_of_a_host_from_its_host_line() {
        let mut places = places();
        places.hosts[1].file = PathBuf::from("/Users/me/.ssh/conf.d/db");
        places.hosts[1].line = 12;
        let mut menu = menu_with(remote("db"), &Recent::default(), places);
        assert_eq!(
            press(&mut menu, Action::View),
            MenuEvent::ViewConfig {
                file: PathBuf::from("/Users/me/.ssh/conf.d/db"),
                line: 12,
            }
        );
        press(&mut menu, Action::Home);
        assert_eq!(menu.chosen_config(), None);
        assert_eq!(press(&mut menu, Action::View), MenuEvent::Pending);
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
        let more = format!("║ {} 4 more…", Glyphs::PLAIN.more);
        assert!(text.contains(&more), "in the column of the icons: {text}");
        assert_eq!(press(&mut menu, Action::Confirm), MenuEvent::Pending);
        assert_eq!(hosts_shown(&menu).len(), 14, "Enter shows them");
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Host("h10".to_owned())),
            "the first of them"
        );
    }

    #[test]
    fn the_row_of_the_rest_is_faint_but_under_the_cursor() {
        let mut menu = menu_with(home(), &Recent::default(), many_hosts(14));
        let theme = Theme::mc_classic();
        let mut terminal = Terminal::new(TestBackend::new(50, 30)).unwrap();
        let hosts = |_: &str| HostState::default();
        let mut style_of = |menu: &mut LocationMenu, text: &str| {
            terminal
                .draw(|frame| {
                    let decor = Decor::new(false);
                    menu.render(frame, frame.area(), &theme, decor, &hosts, &still());
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let at = crate::tui::mouse::find(buffer, text);
            buffer[(at.x, at.y)].style()
        };
        let more = style_of(&mut menu, "4 more…");
        let host = style_of(&mut menu, "h09");
        assert_eq!(more.fg, theme.dialog_more().0.fg, "faint");
        assert_ne!(more.fg, host.fg, "apart from the places");
        press(&mut menu, Action::End);
        let more = style_of(&mut menu, "4 more…");
        let focused = Colors::plain(&theme).focused_style();
        assert_eq!((more.fg, more.bg), (focused.fg, focused.bg), "the cursor's");
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
            ..Places::default()
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
        let mut terminal = Terminal::new(TestBackend::new(50, 19)).unwrap();
        let hosts = |alias: &str| HostState {
            status: match alias {
                "web" => HostStatus::Connected,
                "proxy" => HostStatus::Failed,
                _ => HostStatus::Idle,
            },
            ..HostState::default()
        };
        terminal
            .draw(|frame| {
                let theme = Theme::mc_classic();
                menu.render(
                    frame,
                    frame.area(),
                    &theme,
                    Decor::new(true),
                    &hosts,
                    &still(),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        // The hosts follow web below their heading; their icons are in column 4.
        let web = (0..buffer.area.height)
            .find(|&y| {
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                row.contains(" web ")
            })
            .unwrap();
        let colors = |y: u16| (buffer[(4, y)].fg, buffer[(4, y)].bg);
        assert_eq!(colors(web), (Color::Green, Color::Gray), "connected");
        assert_eq!(
            colors(web + 1),
            (Color::Black, Color::Gray),
            "idle: the dialog's own color"
        );
        assert_eq!(colors(web + 2), (Color::Red, Color::Gray), "failed");
        assert_eq!(buffer[(4, web + 1)].symbol(), "󰒋");
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
                    menu.render(
                        frame,
                        frame.area(),
                        &theme,
                        Decor::new(false),
                        &hosts,
                        &still(),
                    );
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
        insta::assert_snapshot!(draw(&mut menu, 19));
        let mut loading = LocationMenu::new(Side::Right, home(), PathBuf::from("/Users/me"), 1);
        let text = draw(&mut loading, 10);
        let title = text
            .lines()
            .find(|line| line.contains("Location"))
            .unwrap_or_default();
        assert!(title.contains("═ … Location ═"), "{text}");
        loading.listed(1, Ok(places()));
        let listed = draw(&mut loading, 10);
        let still = listed
            .lines()
            .find(|line| line.contains("Location"))
            .unwrap_or_default();
        let column = |line: &str| line.find(" Location ").map(|at| line[..at].chars().count());
        assert_eq!(column(title), column(still), "the title stays in place");
    }

    #[test]
    fn volumes_show_their_free_space_and_size_with_the_slashes_in_a_column() {
        let sized = |path: &str, label: &str, available: u64, total: u64| Volume {
            space: Some(Space { total, available }),
            ..volume(path, Some(label), VolumeKind::Local)
        };
        let places = Places {
            volumes: vec![
                sized("/", "Macintosh HD", 212 << 30, 994 << 30),
                sized("/Volumes/USB", "USB", 12 << 30, 64 << 30),
                sized("/Volumes/Big", "Big", 1536 << 30, 20 << 40),
                Volume {
                    space: None,
                    ..volume("/Volumes/share", Some("share"), VolumeKind::Network)
                },
                sized("/Volumes/pseudo", "pseudo", 0, 0),
            ],
            hosts: vec![host("web")],
            ..Places::default()
        };
        let mut menu = menu_with(home(), &Recent::default(), places);
        let text = draw(&mut menu, 22);
        let row = |name: &str| {
            (text.lines())
                .find(|line| line.contains(&format!(" {name} ")))
                .unwrap_or_default()
        };
        let slash = |line: &str| line.find(" / ").map(|at| line[..at].chars().count());
        assert!(row("Home").contains("  212G /  994G ║"), "{text}");
        assert!(row("USB").contains(" 12.0G / 64.0G ║"), "{text}");
        assert!(row("Big").contains("  1.5T / 20.0T ║"), "{text}");
        let column = slash(row("Home"));
        for name in ["Macintosh HD", "USB", "Big"] {
            assert_eq!(slash(row(name)), column, "{name}: {text}");
        }
        for name in ["share", "pseudo"] {
            assert!(row(name).contains(" size unknown ║"), "{name}: {text}");
        }

        press(&mut menu, Action::End);
        let told = |_: &str| HostState {
            address: Some("deploy@10.0.0.5".to_owned()),
            ..HostState::default()
        };
        let (_, buffer) = card_in(&mut menu, &told, &Theme::terminal(), Decor::new(false));
        let lines = (0..buffer.area.height).filter(|&y| {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            line.contains("deploy@10.0.0.5")
        });
        assert_eq!(lines.count(), 1, "a host's address is in its card alone");
    }

    /// Places with the volumes `/`, which has answered, and `/Volumes/NAS`, which was asked
    /// `ago` and has not, and no hosts.
    fn waiting(ago: Duration) -> Places {
        let since = Instant::now().checked_sub(ago).unwrap_or_else(Instant::now);
        Places {
            volumes: vec![
                volume("/", Some("Macintosh HD"), VolumeKind::System),
                Volume {
                    space: None,
                    ..volume("/Volumes/NAS", Some("NAS"), VolumeKind::Network)
                },
            ],
            waiting: [(PathBuf::from("/Volumes/NAS"), since)].into(),
            ..Places::default()
        }
    }

    #[test]
    fn a_volume_still_to_answer_shows_a_skeleton_as_wide_as_the_sizes_known() {
        let mut menu = menu_with(home(), &Recent::default(), waiting(Duration::ZERO));
        insta::assert_snapshot!(draw(&mut menu, 12));
        let at = Instant::now();
        assert!(menu.sizes_wait(at).is_some(), "the shine crosses it");
        let gives_up = menu.gives_up(at).expect("given up for in time");
        assert!(gives_up > at && gives_up <= at + NO_ANSWER);
        assert!(menu.wait().is_none(), "no dots in the title");
    }

    #[test]
    fn a_skeleton_with_no_sizes_known_is_as_wide_as_sizes_take_at_most() {
        let mut places = waiting(Duration::ZERO);
        places.volumes.remove(0);
        let mut menu = LocationMenu::new(Side::Left, home(), PathBuf::from("/nowhere"), 1)
            .recent(&Recent::default());
        menu.listed(1, Ok(places));
        let text = draw(&mut menu, 12);
        let widest = cells::short_size_width(&noc_locale::current());
        let skeleton = format!(" {} ║", "-".repeat(2 * widest + 3));
        let nas = (text.lines())
            .find(|line| line.contains(" NAS "))
            .unwrap_or_default();
        assert!(nas.contains(&skeleton), "{text}");
        let home = (text.lines())
            .find(|line| line.contains(" Home "))
            .unwrap_or_default();
        assert!(home.contains(" size unknown ║"), "{text}");
    }

    #[test]
    fn sizes_that_do_not_come_in_time_are_given_up_for_and_come_later_still() {
        let mut menu = menu_with(home(), &Recent::default(), waiting(NO_ANSWER));
        let text = draw(&mut menu, 12);
        let nas = (text.lines())
            .find(|line| line.contains(" NAS "))
            .unwrap_or_default();
        assert!(nas.contains(" not responding ║"), "{text}");
        assert!(!nas.contains("-----"), "{text}");
        let at = Instant::now();
        assert!(menu.sizes_wait(at).is_none(), "no shine any more");
        assert_eq!(menu.gives_up(at), None);

        press(&mut menu, Action::End);
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Volume(PathBuf::from("/Volumes/NAS")))
        );
        let answered = volume("/Volumes/NAS", Some("NAS"), VolumeKind::Network);
        menu.answered(Path::new("/Volumes/NAS"), LateAnswer::Volume(answered));
        let text = draw(&mut menu, 12);
        let nas = (text.lines())
            .find(|line| line.contains(" NAS "))
            .unwrap_or_default();
        assert!(nas.contains(" 212G / 994G ║"), "{text}");
        assert_eq!(
            menu.chosen().map(key),
            Some(Key::Volume(PathBuf::from("/Volumes/NAS"))),
            "the cursor stays"
        );
        assert!(menu.contents.places.waiting.is_empty());
    }

    #[test]
    fn a_volume_that_turns_out_not_to_be_one_goes_and_others_answer_nothing() {
        let mut menu = menu_with(home(), &Recent::default(), waiting(Duration::ZERO));
        menu.answered(Path::new("/Volumes/gone"), LateAnswer::Gone);
        assert_eq!(menu.items().len(), 3, "not listed: nothing changes");
        menu.answered(Path::new("/Volumes/NAS"), LateAnswer::Gone);
        assert_eq!(keys(&menu), [Key::Home, Key::Volume(PathBuf::from("/"))]);
        assert!(menu.sizes_wait(Instant::now()).is_none());
    }

    #[test]
    fn a_volume_whose_answer_fails_after_it_hung_does_not_respond_at_once() {
        let mut menu = menu_with(home(), &Recent::default(), waiting(Duration::ZERO));
        menu.answered(Path::new("/Volumes/NAS"), LateAnswer::Failed);
        assert!(menu.contents.places.waiting.is_empty());
        let at = Instant::now();
        assert!(menu.sizes_wait(at).is_none(), "no shine");
        assert_eq!(menu.gives_up(at), None, "nothing to give up for");
        let text = draw(&mut menu, 12);
        let nas = (text.lines())
            .find(|line| line.contains(" NAS "))
            .unwrap_or_default();
        assert!(nas.contains(" not responding ║"), "before 5 s: {text}");
        press(&mut menu, Action::End);
        let [_, _, space] = card(&mut menu, &idle);
        assert!(space.starts_with("║   Not responding "), "{space}");
    }

    #[test]
    fn what_a_volume_does_not_tell_has_colors_of_its_own_but_under_the_cursor() {
        let mut places = waiting(NO_ANSWER);
        places.volumes.push(Volume {
            space: None,
            ..volume("/Volumes/share", Some("share"), VolumeKind::Network)
        });
        let mut menu = menu_with(home(), &Recent::default(), places);
        let theme = Theme::mc_classic();
        let draw_in = |menu: &mut LocationMenu| {
            let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
            terminal
                .draw(|frame| {
                    let decor = Decor::new(false);
                    menu.render(frame, frame.area(), &theme, decor, &idle, &still());
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let fg = |buffer: &ratatui::buffer::Buffer, text: &str| {
            let at = crate::tui::mouse::find(buffer, text);
            Some(buffer[(at.x, at.y)].fg)
        };
        let warning = theme.dialog_warning_text.fg;
        let buffer = draw_in(&mut menu);
        assert_eq!(fg(&buffer, "not responding"), warning);
        assert_eq!(fg(&buffer, "size unknown"), theme.disabled.fg);
        press(&mut menu, Action::End);
        let buffer = draw_in(&mut menu);
        let cursor = Colors::plain(&theme).focused_style().fg;
        assert_eq!(fg(&buffer, "size unknown"), cursor, "under the cursor");
        assert_eq!(
            fg(&buffer, "Size unknown"),
            theme.disabled.fg,
            "faint in the card"
        );
        assert_eq!(
            fg(&buffer, "source unknown"),
            theme.disabled.fg,
            "it answered without"
        );
        press(&mut menu, Action::Up);
        let buffer = draw_in(&mut menu);
        assert_eq!(
            fg(&buffer, "Not responding"),
            warning,
            "a warning in the card"
        );
        assert_eq!(fg(&buffer, "source unknown"), warning, "it did not answer");
    }

    #[test]
    fn home_waits_with_the_volume_that_holds_it() {
        let mut places = waiting(Duration::ZERO);
        places.volumes[1].mount_point = PathBuf::from("/Users");
        places.waiting = [(PathBuf::from("/Users"), Instant::now())].into();
        let mut menu = menu_with(home(), &Recent::default(), places);
        let text = draw(&mut menu, 12);
        let home = (text.lines())
            .find(|line| line.contains(" Home "))
            .unwrap_or_default();
        assert!(home.contains(" ----------- ║"), "{text}");
    }

    /// The line over the card and its two lines, drawn 60 cells wide with `hosts`, trimmed.
    fn card(menu: &mut LocationMenu, hosts: &dyn Fn(&str) -> HostState) -> [String; 3] {
        card_in(menu, hosts, &Theme::terminal(), Decor::new(false)).0
    }

    /// [`card`], drawn in `theme` with `decor`, and the buffer it was drawn in.
    fn card_in(
        menu: &mut LocationMenu,
        hosts: &dyn Fn(&str) -> HostState,
        theme: &Theme,
        decor: Decor,
    ) -> ([String; 3], ratatui::buffer::Buffer) {
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| menu.render(frame, frame.area(), theme, decor, hosts, &still()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let lines: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                let line: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                line.trim().to_owned()
            })
            .collect();
        let bottom = (lines.iter().rposition(|line| line.starts_with('╚'))).unwrap_or(3);
        let card = [3, 2, 1].map(|above| lines[bottom - above].clone());
        (card, buffer)
    }

    fn idle(_: &str) -> HostState {
        HostState::default()
    }

    #[test]
    fn a_card_starts_with_the_icon_of_its_row_and_its_second_line_stands_under_its_text() {
        use ratatui::style::Color;

        let mut menu = menu_with(remote("web"), &Recent::default(), places());
        let theme = Theme::mc_classic().with_glyphs(&Glyphs::NERD);
        let connected = |_: &str| HostState {
            status: HostStatus::Connected,
            address: Some("deploy@10.0.0.5".to_owned()),
            ..HostState::default()
        };
        let ([_, reach, how], buffer) = card_in(&mut menu, &connected, &theme, Decor::new(true));
        assert!(reach.starts_with("║ 󰒋 deploy@10.0.0.5 "), "{reach}");
        assert!(how.starts_with("║   Connected "), "{how}");
        let at = crate::tui::mouse::find(&buffer, "󰒋 deploy");
        assert_eq!(
            buffer[(at.x, at.y)].fg,
            Color::Green,
            "in the color of its status"
        );

        let ([_, _, how], _) = card_in(&mut menu, &idle, &theme, Decor::new(true));
        assert!(how.starts_with("║   Not connected "), "{how}");

        press(&mut menu, Action::Home);
        let ([_, path, space], _) = card_in(&mut menu, &idle, &theme, Decor::new(true));
        assert!(path.starts_with("║ 󰋜 /Users/me "), "{path}");
        assert!(space.starts_with("║   212G free of 994G "), "{space}");
        press(&mut menu, Action::Down);
        let ([_, mounted, _], _) = card_in(&mut menu, &idle, &theme, Decor::new(true));
        assert!(mounted.starts_with("║ 󰋊 / "), "{mounted}");
    }

    #[test]
    fn the_card_of_a_volume_says_where_it_is_what_it_mounts_and_how_much_is_free() {
        let usb = Volume {
            source: Some("/dev/disk4s1".to_owned()),
            space: Some(Space {
                total: 64 << 30,
                available: 12 << 30,
            }),
            ..volume("/Volumes/USB", Some("USB"), VolumeKind::Local)
        };
        let share = Volume {
            fs_type: Some("smbfs".to_owned()),
            source: Some("//me@nas/share".to_owned()),
            space: None,
            ..volume("/Volumes/share", Some("share"), VolumeKind::Network)
        };
        let system = volume("/", Some("Macintosh HD"), VolumeKind::System);
        let places = Places {
            volumes: vec![system, usb, share],
            ..Places::default()
        };
        let at = Location::Local(PathBuf::from("/Volumes/USB/photos"));
        let mut menu = menu_with(at, &Recent::default(), places);
        let [label, mounted, space] = card(&mut menu, &idle);
        assert!(label.ends_with(" apfs · local ─╢"), "{label}");
        assert!(mounted.starts_with("║ + /Volumes/USB "), "{mounted}");
        assert!(
            mounted.ends_with(" /dev/disk4s1 ║"),
            "at the right end: {mounted}"
        );
        assert!(
            space.starts_with("║   12.0G free of 64.0G (18%) "),
            "{space}"
        );

        press(&mut menu, Action::Down);
        let [label, mounted, space] = card(&mut menu, &idle);
        assert!(label.ends_with(" smbfs · network ─╢"), "{label}");
        assert!(mounted.starts_with("║ + /Volumes/share "), "{mounted}");
        assert!(mounted.ends_with(" //me@nas/share ║"), "{mounted}");
        assert!(space.starts_with("║   Size unknown "), "{space}");

        press(&mut menu, Action::Home);
        let [label, path, space] = card(&mut menu, &idle);
        assert!(
            label.ends_with(" apfs · system ─╢"),
            "as the card of its volume: {label}"
        );
        assert!(path.starts_with("║ ~ /Users/me "), "{path}");
        assert!(
            path.ends_with(" Macintosh HD ║"),
            "the name of its volume: {path}"
        );
        assert!(space.starts_with("║   212G free of 994G (21%) "), "{space}");
    }

    #[test]
    fn the_card_of_a_volume_still_to_answer_waits_then_gives_up() {
        let mut places = waiting(Duration::ZERO);
        places.volumes[1].fs_type = None;
        let mut menu = menu_with(home(), &Recent::default(), places);
        press(&mut menu, Action::End);
        let [label, mounted, space] = card(&mut menu, &idle);
        assert!(
            label.ends_with(" network ─╢"),
            "its type is still to come: {label}"
        );
        assert!(mounted.starts_with("║ + /Volumes/NAS "), "{mounted}");
        let skeleton = format!(" {} ║", "-".repeat(SKELETON));
        assert!(mounted.ends_with(&skeleton), "what it mounts: {mounted}");
        assert!(space.starts_with("║   ------"), "a skeleton: {space}");

        let mut menu = menu_with(home(), &Recent::default(), waiting(NO_ANSWER));
        press(&mut menu, Action::End);
        let [_, mounted, space] = card(&mut menu, &idle);
        assert!(mounted.ends_with(" source unknown ║"), "{mounted}");
        assert!(space.starts_with("║   Not responding "), "{space}");
    }

    #[test]
    fn the_card_of_a_host_says_how_to_reach_it_where_it_is_defined_and_how_it_is() {
        let mut defined = places();
        defined.hosts[0].line = 12;
        defined.hosts[1].file = PathBuf::from("/etc/ssh/ssh_config");
        defined.hosts[1].line = 40;
        let mut menu = menu_with(remote("web"), &Recent::default(), defined);
        let state = |alias: &str| match alias {
            "web" => HostState {
                status: HostStatus::Connected,
                address: Some("deploy@10.0.0.5".to_owned()),
                jump: Some("bastion".to_owned()),
                ..HostState::default()
            },
            "db" => HostState {
                status: HostStatus::Failed,
                reason: Some("Permission denied (publickey).".to_owned()),
                ..HostState::default()
            },
            _ => HostState {
                address: Some("root@proxy.example.org".to_owned()),
                ..HostState::default()
            },
        };
        let [label, reach, how] = card(&mut menu, &state);
        assert!(label.ends_with(" sftp ─╢"), "its kind: {label}");
        assert!(
            reach.starts_with("║ ● deploy@10.0.0.5 via bastion "),
            "{reach}"
        );
        assert!(
            reach.ends_with(" ~/.ssh/config:12 ║"),
            "at the right end: {reach}"
        );
        assert!(
            !reach.contains("web"),
            "no alias, which the row shows: {reach}"
        );
        assert_eq!(how.trim_end_matches(['║', ' ']), "║   Connected");

        press(&mut menu, Action::Down);
        let [label, reach, how] = card(&mut menu, &state);
        assert!(label.ends_with(" sftp ─╢"), "{label}");
        let skeleton = format!("║ ✗ {} ", "-".repeat(SKELETON));
        assert!(reach.starts_with(&skeleton), "{reach}");
        assert!(reach.ends_with(" /etc/ssh/ssh_config:40 ║"), "{reach}");
        assert!(
            how.starts_with("║   Permission denied (publickey). "),
            "{how}"
        );

        press(&mut menu, Action::Down);
        let [_, reach, how] = card(&mut menu, &state);
        assert!(reach.starts_with("║ ○ root@proxy.example.org "), "{reach}");
        assert!(reach.ends_with(" ~/.ssh/config:1 ║"), "{reach}");
        assert_eq!(how.trim_end_matches(['║', ' ']), "║   Not connected");
    }

    #[test]
    fn the_end_of_a_line_takes_what_its_start_leaves_but_at_least_half() {
        assert_eq!(ends(40, 3, 15), (24, 15), "as long as it is");
        assert_eq!(ends(40, 30, 35), (19, 20), "half against a long start");
        assert_eq!(ends(40, 3, 50), (3, 36), "the rest after a short one");
        assert_eq!(ends(40, 3, 0), (40, 0), "nothing at the end");
    }

    #[test]
    fn why_a_host_failed_and_that_ssh_g_did_are_in_the_colors_of_errors_not_crossed_out() {
        let failed = |_: &str| HostState {
            status: HostStatus::Failed,
            reason: Some("Connection refused".to_owned()),
            unresolved: true,
            ..HostState::default()
        };
        let theme = Theme::mc_classic();
        for text in ["Connection refused", "ssh -G failed"] {
            let mut menu = menu_with(remote("db"), &Recent::default(), places());
            let mut terminal = Terminal::new(TestBackend::new(70, 30)).unwrap();
            terminal
                .draw(|frame| {
                    let decor = Decor::new(false);
                    menu.render(frame, frame.area(), &theme, decor, &failed, &still());
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let at = crate::tui::mouse::find(buffer, text);
            let cell = &buffer[(at.x, at.y)];
            assert_eq!(Some(cell.fg), theme.dialog_error_text.fg, "{text}");
            assert_ne!(
                Some(cell.fg),
                theme.dialog.fg,
                "apart from the dialog's: {text}"
            );
            let crossed = cell
                .modifier
                .contains(ratatui::style::Modifier::CROSSED_OUT);
            assert!(!crossed, "nothing typed wrong: {text}");
        }
    }

    #[test]
    fn the_card_of_a_host_takes_its_address_from_ssh_g_and_waits_for_it() {
        let mut menu = menu_with(remote("web"), &Recent::default(), places());
        let fresh = |_: &str| HostState {
            address: Some("deploy@10.0.0.9".to_owned()),
            jump: Some("gate".to_owned()),
            ..HostState::default()
        };
        let [_, reach, _] = card(&mut menu, &fresh);
        let fresh = "║ ○ deploy@10.0.0.9 via gate ";
        assert!(reach.starts_with(fresh), "this run's: {reach}");
        assert!(
            menu.address_wait(&|_| true).is_none(),
            "no skeleton to shine over once ssh -G told it"
        );

        press(&mut menu, Action::Down);
        let [_, reach, _] = card(&mut menu, &idle);
        let skeleton = "from the start, before ssh -G runs: no text in between";
        assert!(reach.starts_with("║ ○ ------"), "{skeleton}: {reach}");
        assert!(menu.address_wait(&|_| false).is_some());
        let failed = |_: &str| HostState {
            unresolved: true,
            ..HostState::default()
        };
        let [_, reach, _] = card(&mut menu, &failed);
        assert!(
            reach.starts_with("║ ○ ssh -G failed "),
            "why is in the log: {reach}"
        );
    }

    #[test]
    fn the_card_of_the_row_of_the_rest_names_what_it_stands_for() {
        let mut menu = menu_with(home(), &Recent::default(), many_hosts(14));
        press(&mut menu, Action::End);
        let [label, names, empty] = card(&mut menu, &idle);
        assert!(label.ends_with("──╢") && !label.contains(':'), "{label}");
        assert!(names.starts_with("║ ▾ h11, h12, h13, h14 "), "{names}");
        assert_eq!(empty.trim_matches(['║', ' ']), "");

        let mut menu = menu_with(home(), &Recent::default(), many_hosts(60));
        press(&mut menu, Action::End);
        let [_, first, second] = card(&mut menu, &idle);
        let first = first.trim_end_matches(['║', ' ']);
        assert!(first.starts_with("║ ▾ h11, h12, "), "{first}");
        assert!(first.ends_with(','), "whole names, then the rest: {first}");
        let last = first.rsplit(' ').next().unwrap_or_default();
        let last: u32 = last.trim_matches(['h', ',']).parse().unwrap();
        let under = format!("║   h{}, ", last + 1);
        assert!(second.starts_with(&under), "under the names: {second}");
        assert!(second.ends_with("» ║"), "the end cut short: {second}");
    }

    #[test]
    fn names_go_over_two_lines_whole() {
        let names = ["alpha", "beta", "gamma", "delta"].map(str::to_owned);
        let lines = |width| two_lines(&names, width);
        let both = |first: &str, second: &str| (first.to_owned(), second.to_owned());
        assert_eq!(lines(40), both("alpha, beta, gamma, delta", ""));
        assert_eq!(lines(13), both("alpha, beta,", "gamma, delta"));
        assert_eq!(
            lines(3),
            both("alpha,", "beta, gamma, delta"),
            "one at least"
        );
        assert_eq!(two_lines(&[], 10), both("", ""));
    }

    #[test]
    fn the_box_keeps_its_height_whatever_the_card_is_of() {
        let mut menu = menu_with(home(), &Recent::default(), many_hosts(14));
        let height = |menu: &mut LocationMenu| {
            (draw(menu, 40).lines())
                .filter(|line| line.contains('║') || line.contains('╟'))
                .count()
        };
        let first = height(&mut menu);
        for action in [Action::Down, Action::Down, Action::End] {
            press(&mut menu, action);
            assert_eq!(height(&mut menu), first, "after {action:?}");
        }
    }

    #[test]
    fn a_menu_waits_in_a_box_as_tall_as_the_last_one_had() {
        // The lines inside the frame, those that join it too.
        let rows = |text: &str| {
            (text.lines())
                .filter(|line| line.contains('║') || line.contains('╟'))
                .count()
        };
        let mut first = LocationMenu::new(Side::Left, home(), PathBuf::from("/Users/me"), 1);
        assert_eq!(first.lines_listed(), None);
        let text = draw(&mut first, 30);
        assert_eq!(rows(&text), 3, "the filter, a line, one row: {text}");
        first.listed(1, Ok(places()));
        let listed = rows(&draw(&mut first, 30));
        assert!(listed > 3);
        let lines = first.lines_listed();
        // The filter and the line under it; a line and the whole row under the cursor.
        let around = 2 + usize::from(FULL_ROWS + 1);
        assert_eq!(lines.map(usize::from), Some(listed - around));

        let mut next =
            LocationMenu::new(Side::Left, home(), PathBuf::from("/Users/me"), 2).lines_like(lines);
        let text = draw(&mut next, 30);
        assert_eq!(rows(&text), listed, "{text}");
        assert!(
            text.lines().filter(|line| line.contains("--")).count() > 1,
            "{text}"
        );
    }

    #[test]
    fn a_menu_waiting_for_its_rows_shows_a_skeleton_of_them() {
        let mut menu = LocationMenu::new(Side::Right, home(), PathBuf::from("/Users/me"), 1);
        insta::assert_snapshot!(draw(&mut menu, 10));
    }

    #[test]
    fn listing_again_keeps_the_rows_and_turns_the_dots_of_the_title() {
        let mut menu = menu_at(home());
        assert!(menu.wait().is_none());

        menu.reload(2);
        assert!(menu.wait().is_some());
        let text = draw(&mut menu, 16);
        assert!(text.contains("═ … Location ═"), "{text}");
        assert!(text.contains("● web"), "the rows stay: {text}");

        menu.listed(1, Ok(places()));
        assert!(menu.wait().is_some(), "an older listing changes nothing");
        menu.listed(2, Ok(places()));
        assert!(menu.wait().is_none());
        assert!(!draw(&mut menu, 16).contains("… Location"));

        menu.reload(3);
        menu.listed(3, Err("no ssh config".to_owned()));
        assert!(menu.wait().is_none());
        let text = draw(&mut menu, 16);
        assert!(text.contains("Not listed again: no"), "{text}");
        assert!(text.contains("● web"), "the rows stay: {text}");

        menu.reload(4);
        assert!(!draw(&mut menu, 16).contains("Not listed again"));
    }

    #[test]
    fn listing_again_before_the_first_listing_waits_in_place_of_the_rows() {
        let mut menu = LocationMenu::new(Side::Left, home(), PathBuf::from("/Users/me"), 1);
        menu.listed(1, Err("no ssh config".to_owned()));
        assert!(menu.wait().is_none());
        menu.reload(2);
        assert!(menu.wait().is_some());
        let text = draw(&mut menu, 10);
        assert!(text.contains("═ … Location ═"), "{text}");
        assert!(!text.contains("no ssh config"), "{text}");
    }
}
