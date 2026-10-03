//! The pull-down menu of F9, as in mc: a bar of menus (Left, File, Command, Options, Right),
//! and the commands of the one that is open. Left and Right act on the panel drawn on that
//! side; the others do what their keys do. Each command shows the key that does the same, from
//! the keymap, and has a letter that runs it while its menu is open.
//!
//! As in Far Manager, F9 opens the bar alone, where each menu's letter opens it, and Esc in an
//! open menu goes back to the bar. The menu remembers where it was when it closed, and opens
//! there again.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use super::app::Side;
use super::cells::{self, Align};
use super::keymap::{Action, Resolved};
use super::theme::Theme;
use crate::i18n::fl;

/// Cells between a command's name and its key.
const KEY_GAP: usize = 2;

/// What a command does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    /// What the action does in the active panel, as its key does.
    Do(Action),
    /// What the action does in the panel on that side: sorting or reading it again.
    On(Side, Action),
    /// Opens the location menu of the panel on that side.
    Location(Side),
    /// Closes the connection of the host that the panel on that side shows.
    DisconnectPanel(Side),
    /// Opens the Configuration dialog.
    Configuration,
}

/// What the app says about a command now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Status {
    /// Whether it does something; the cursor skips the others.
    pub(crate) enabled: bool,
    /// Whether its mark shows: the sort order of the panel, or a setting that is on.
    pub(crate) checked: bool,
    /// The keys that do the same, such as `Ctrl-F3`.
    pub(crate) key: Option<String>,
}

/// What a key did in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullDownEvent {
    Pending,
    Closed,
    /// Close the menu and run this.
    Run(Command),
}

/// The mark in front of a command that has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    /// One of several, such as a sort order.
    Radio,
    /// A setting that is on or off.
    Check,
}

/// Text with the letter that an `&` marks, as the Fluent messages write it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Label {
    text: String,
    /// The index among the characters of `text`, and the letter in lower case.
    hotkey: Option<(usize, char)>,
}

impl Label {
    /// `&&` stands for `&`; the first other `&` marks the letter after it.
    fn parse(source: &str) -> Self {
        let mut text = String::new();
        let mut hotkey = None;
        let mut chars = source.chars().peekable();
        let mut index = 0;
        while let Some(c) = chars.next() {
            let c = match (c, chars.peek()) {
                ('&', Some('&')) => {
                    chars.next();
                    '&'
                }
                ('&', Some(&next)) if hotkey.is_none() => {
                    chars.next();
                    hotkey = next.to_lowercase().next().map(|lower| (index, lower));
                    next
                }
                _ => c,
            };
            text.push(c);
            index += 1;
        }
        Self { text, hotkey }
    }

    /// The text in `style`, its letter in `hotkey` over it.
    fn spans(&self, style: Style, hotkey: Option<Style>) -> Vec<Span<'static>> {
        let Some(((at, _), hotkey)) = self.hotkey.zip(hotkey) else {
            return vec![Span::styled(self.text.clone(), style)];
        };
        let mut chars = self.text.chars();
        let head: String = chars.by_ref().take(at).collect();
        let letter: String = chars.by_ref().take(1).collect();
        let tail: String = chars.collect();
        vec![
            Span::styled(head, style),
            Span::styled(letter, style.patch(hotkey)),
            Span::styled(tail, style),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    label: Label,
    command: Command,
    mark: Mark,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Item(Item),
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Menu {
    title: Label,
    entries: Vec<Entry>,
}

impl Menu {
    fn item(&self, index: usize) -> Option<&Item> {
        match self.entries.get(index)? {
            Entry::Item(item) => Some(item),
            Entry::Separator => None,
        }
    }
}

/// The open menu bar, the menu selected on it, and the command under the cursor in each menu.
#[derive(Debug)]
pub(crate) struct PullDown {
    menus: Vec<Menu>,
    place: Place,
}

/// Where the pull-down menu is: kept when it closes, for F9 to open it there again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    /// The menu selected on the bar.
    selected: usize,
    /// Whether that menu is open below its title, or only the bar is.
    open: bool,
    /// The entry under the cursor in each menu.
    cursors: Vec<usize>,
}

/// The titles of the menu bar, left to right.
fn titles() -> [Label; 5] {
    [
        fl!("pulldown-left"),
        fl!("pulldown-file"),
        fl!("pulldown-command"),
        fl!("pulldown-options"),
        fl!("pulldown-right"),
    ]
    .map(|title| Label::parse(&title))
}

/// The menu of a panel: the panel on `side`, which is drawn under its title.
fn panel_menu(title: Label, side: Side) -> Menu {
    let item = |text: String, command, mark| {
        Entry::Item(Item {
            label: Label::parse(&text),
            command,
            mark,
        })
    };
    let sort = |text: String, action| item(text, Command::On(side, action), Mark::Radio);
    Menu {
        title,
        entries: vec![
            item(
                fl!("pulldown-location"),
                Command::Location(side),
                Mark::None,
            ),
            Entry::Separator,
            sort(fl!("pulldown-sort-name"), Action::SortByName),
            sort(fl!("pulldown-sort-extension"), Action::SortByExtension),
            sort(fl!("pulldown-sort-time"), Action::SortByTime),
            sort(fl!("pulldown-sort-size"), Action::SortBySize),
            Entry::Separator,
            item(
                fl!("pulldown-rescan"),
                Command::On(side, Action::Reload),
                Mark::None,
            ),
            item(
                fl!("pulldown-disconnect-panel"),
                Command::DisconnectPanel(side),
                Mark::None,
            ),
        ],
    }
}

/// A menu of commands that act as their keys do; `None` is a separator.
fn plain_menu(title: Label, items: Vec<Option<(String, Action)>>) -> Menu {
    let entries = items
        .into_iter()
        .map(|item| match item {
            Some((text, action)) => Entry::Item(Item {
                label: Label::parse(&text),
                command: Command::Do(action),
                mark: if action == Action::ToggleHidden {
                    Mark::Check
                } else {
                    Mark::None
                },
            }),
            None => Entry::Separator,
        })
        .collect();
    Menu { title, entries }
}

/// Options: the Configuration dialog, then settings that switch at once.
fn options_menu(title: Label) -> Menu {
    let mut menu = plain_menu(
        title,
        vec![None, Some((fl!("pulldown-hidden"), Action::ToggleHidden))],
    );
    menu.entries.insert(
        0,
        Entry::Item(Item {
            label: Label::parse(&fl!("pulldown-configuration")),
            command: Command::Configuration,
            mark: Mark::None,
        }),
    );
    menu
}

impl PullDown {
    /// The menu bar at `place`, where it was when it closed; the first time, with the menu of
    /// the `active` panel selected and none open. `swapped` panels are drawn on each other's
    /// sides, and Left and Right go with where they are drawn.
    pub(crate) fn new(
        active: Side,
        swapped: bool,
        place: Option<&Place>,
        status: &dyn Fn(Command) -> Status,
    ) -> Self {
        let [left, file, command, options, right] = titles();
        let (on_left, on_right) = if swapped {
            (Side::Right, Side::Left)
        } else {
            (Side::Left, Side::Right)
        };
        let menus = vec![
            panel_menu(left, on_left),
            plain_menu(
                file,
                vec![
                    Some((fl!("pulldown-view"), Action::View)),
                    Some((fl!("pulldown-edit"), Action::Edit)),
                    Some((fl!("pulldown-copy"), Action::Copy)),
                    Some((fl!("pulldown-move"), Action::Move)),
                    Some((fl!("pulldown-mkdir"), Action::Mkdir)),
                    Some((fl!("pulldown-delete"), Action::Delete)),
                    None,
                    Some((fl!("pulldown-select"), Action::Select)),
                    Some((fl!("pulldown-unselect"), Action::Unselect)),
                    Some((fl!("pulldown-invert"), Action::InvertMarks)),
                    None,
                    Some((fl!("pulldown-checksum"), Action::Checksum)),
                    None,
                    Some((fl!("pulldown-exit"), Action::Quit)),
                ],
            ),
            plain_menu(
                command,
                vec![
                    Some((fl!("pulldown-quick-search"), Action::QuickSearch)),
                    None,
                    Some((fl!("pulldown-swap"), Action::SwapPanels)),
                    Some((fl!("pulldown-other-open"), Action::OtherPanelOpen)),
                    Some((fl!("pulldown-other-sync"), Action::OtherPanelSync)),
                    None,
                    Some((fl!("pulldown-jobs"), Action::Jobs)),
                    None,
                    Some((fl!("pulldown-edit-host"), Action::EditHost)),
                    Some((fl!("pulldown-disconnect-host"), Action::Disconnect)),
                    None,
                    Some((fl!("pulldown-help"), Action::Help)),
                    Some((fl!("pulldown-redraw"), Action::Redraw)),
                ],
            ),
            options_menu(options),
            panel_menu(right, on_right),
        ];
        let selected = if active == on_left {
            0
        } else {
            menus.len() - 1
        };
        let place = place
            .filter(|place| place.selected < menus.len() && place.cursors.len() == menus.len())
            .cloned()
            .unwrap_or_else(|| Place {
                selected,
                open: false,
                cursors: vec![0; menus.len()],
            });
        let mut pulldown = Self { menus, place };
        if pulldown.place.open {
            pulldown.open(pulldown.place.selected, status);
        }
        pulldown
    }

    /// Where the menu is now, to open it there again.
    pub(crate) fn place(&self) -> &Place {
        &self.place
    }

    fn menu(&self) -> &Menu {
        &self.menus[self.place.selected]
    }

    fn cursor(&self) -> usize {
        self.place.cursors[self.place.selected]
    }

    fn set_cursor(&mut self, cursor: usize) {
        self.place.cursors[self.place.selected] = cursor;
    }

    /// The first entry from `start`, forward or back round the menu, that is a command that
    /// runs now.
    fn next_enabled(
        &self,
        start: usize,
        forward: bool,
        status: &dyn Fn(Command) -> Status,
    ) -> Option<usize> {
        let menu = self.menu();
        let count = menu.entries.len();
        (0..count)
            .map(|step| {
                if forward {
                    (start + step) % count
                } else {
                    (start + count - step % count) % count
                }
            })
            .find(|&index| {
                menu.item(index)
                    .is_some_and(|item| status(item.command).enabled)
            })
    }

    /// Opens the menu `index` below its title, with the cursor where it was in that menu, or
    /// on the next command that runs now.
    fn open(&mut self, index: usize, status: &dyn Fn(Command) -> Status) {
        self.place.selected = index;
        self.place.open = true;
        let cursor = self.cursor();
        self.set_cursor(self.next_enabled(cursor, true, status).unwrap_or(cursor));
    }

    /// Selects the menu `index` on the bar, and opens it if a menu is open.
    fn select(&mut self, index: usize, status: &dyn Fn(Command) -> Status) {
        if self.place.open {
            self.open(index, status);
        } else {
            self.place.selected = index;
        }
    }

    /// Takes a key. On the bar, Left and Right select the next menu, round the bar, and Enter,
    /// Up, Down, or a menu's letter opens it. In an open menu, Left and Right open the next
    /// menu; Up and Down move to the next command that runs now, round the menu; Enter or a
    /// command's letter runs it; Esc goes back to the bar, and closes the menu bar from there.
    pub(crate) fn handle(
        &mut self,
        input: Resolved,
        status: &dyn Fn(Command) -> Status,
    ) -> PullDownEvent {
        let count = self.menus.len();
        let entries = self.menu().entries.len();
        let selected = self.place.selected;
        match input {
            Resolved::Insert(c) => {
                let lower = c.to_lowercase().next().unwrap_or(c);
                let has = |label: &Label| label.hotkey.is_some_and(|(_, key)| key == lower);
                if !self.place.open {
                    if let Some(index) = self.menus.iter().position(|menu| has(&menu.title)) {
                        self.open(index, status);
                    }
                    return PullDownEvent::Pending;
                }
                let found = self
                    .menu()
                    .entries
                    .iter()
                    .enumerate()
                    .find_map(|(index, entry)| match entry {
                        Entry::Item(item) if has(&item.label) && status(item.command).enabled => {
                            Some((index, item.command))
                        }
                        Entry::Item(_) | Entry::Separator => None,
                    });
                if let Some((index, command)) = found {
                    self.set_cursor(index);
                    return PullDownEvent::Run(command);
                }
            }
            Resolved::Action(action) => match action {
                Action::Left => self.select((selected + count - 1) % count, status),
                Action::Right => self.select((selected + 1) % count, status),
                Action::Up | Action::Down | Action::Confirm if !self.place.open => {
                    self.open(selected, status);
                }
                Action::Up => {
                    let start = (self.cursor() + entries - 1) % entries;
                    let cursor = self.next_enabled(start, false, status);
                    self.set_cursor(cursor.unwrap_or(self.cursor()));
                }
                Action::Down => {
                    let start = (self.cursor() + 1) % entries;
                    let cursor = self.next_enabled(start, true, status);
                    self.set_cursor(cursor.unwrap_or(self.cursor()));
                }
                Action::Home => {
                    self.place.open = true;
                    let cursor = self.next_enabled(0, true, status);
                    self.set_cursor(cursor.unwrap_or(self.cursor()));
                }
                Action::End => {
                    self.place.open = true;
                    let last = entries.saturating_sub(1);
                    let cursor = self.next_enabled(last, false, status);
                    self.set_cursor(cursor.unwrap_or(self.cursor()));
                }
                Action::Confirm => {
                    if let Some(item) = self.menu().item(self.cursor())
                        && status(item.command).enabled
                    {
                        return PullDownEvent::Run(item.command);
                    }
                }
                Action::Cancel if self.place.open => self.place.open = false,
                Action::Cancel => return PullDownEvent::Closed,
                _ => {}
            },
        }
        PullDownEvent::Pending
    }

    /// Draws the bar on `bar` with the selected menu's title set apart, and that menu below it
    /// if it is open, within `screen`. `icons` picks the marks: `•` and `✓`, or `*` and `x` as
    /// mc's.
    pub(crate) fn render(
        &self,
        frame: &mut Frame<'_>,
        (bar, screen): (Rect, Rect),
        theme: &Theme,
        icons: bool,
        status: &dyn Fn(Command) -> Status,
    ) {
        let titles: Vec<&Label> = self.menus.iter().map(|menu| &menu.title).collect();
        let starts = render_bar(frame, bar, theme, &titles, Some(self.place.selected));
        if !self.place.open {
            return;
        }
        let x = starts.get(self.place.selected).copied().unwrap_or(bar.x);
        let below = Rect::new(
            screen.x,
            bar.bottom(),
            screen.width,
            screen.bottom().saturating_sub(bar.bottom()),
        );
        self.render_menu(frame, x, below, theme, icons, status);
    }

    /// The open menu, framed, from the column `x` at the top of `area`.
    fn render_menu(
        &self,
        frame: &mut Frame<'_>,
        x: u16,
        area: Rect,
        theme: &Theme,
        icons: bool,
        status: &dyn Fn(Command) -> Status,
    ) {
        let menu = self.menu();
        let statuses: Vec<Option<Status>> = menu
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Item(item) => Some(status(item.command)),
                Entry::Separator => None,
            })
            .collect();
        let label_width = menu
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item(item) => Some(cells::width(&item.label.text)),
                Entry::Separator => None,
            })
            .max()
            .unwrap_or(0);
        let key_width = statuses
            .iter()
            .flatten()
            .filter_map(|status| status.key.as_deref().map(cells::width))
            .max()
            .unwrap_or(0);
        let keys = if key_width > 0 {
            KEY_GAP + key_width
        } else {
            0
        };
        // A space, the mark and a space, the name, the keys, a space; the frame.
        let inner = 1 + 2 + label_width + keys + 1;
        let width = u16::try_from(inner + 2).unwrap_or(u16::MAX).min(area.width);
        let height = u16::try_from(menu.entries.len() + 2)
            .unwrap_or(u16::MAX)
            .min(area.height);
        if width < 4 || height < 3 {
            return;
        }
        // As in mc, the frame starts a cell left of the title, and stays on screen.
        let x = x
            .saturating_sub(1)
            .max(area.x)
            .min(area.right().saturating_sub(width));
        let outer = Rect::new(x, area.y, width, height);
        frame.render_widget(Clear, outer);
        if let Some(shadow) = theme.shadow {
            let right = Rect::new(outer.right(), outer.y + 1, 2, outer.height);
            let below = Rect::new(outer.x + 2, outer.bottom(), outer.width, 1);
            for rect in [right, below] {
                frame
                    .buffer_mut()
                    .set_style(rect.intersection(area), shadow);
            }
        }
        let block = Block::bordered()
            .border_type(theme.border_type())
            .style(theme.menu);
        let rows = block.inner(outer);
        frame.render_widget(block, outer);
        let room = usize::from(rows.width);
        let (left_tee, right_tee) = theme.tees();
        let marks = if icons { ('•', '✓') } else { ('*', 'x') };
        for (index, (entry, status)) in menu.entries.iter().zip(&statuses).enumerate() {
            let Ok(offset) = u16::try_from(index) else {
                break;
            };
            if offset >= rows.height {
                break;
            }
            let y = rows.y + offset;
            if let (Entry::Item(item), Some(status)) = (entry, status) {
                let line = item_line(item, status, index == self.cursor(), room, theme, marks);
                frame.render_widget(line, Rect::new(rows.x, y, rows.width, 1));
            } else {
                let line = format!("{left_tee}{}{right_tee}", "─".repeat(room));
                let row = Rect::new(outer.x, y, outer.width, 1);
                frame.render_widget(Line::styled(line, theme.menu), row);
            }
        }
    }
}

/// A command in `room` cells: its mark, from `marks` for radio buttons and check boxes, its
/// name with its letter, and its keys on the right.
fn item_line(
    item: &Item,
    status: &Status,
    selected: bool,
    room: usize,
    theme: &Theme,
    (radio, check): (char, char),
) -> Line<'static> {
    let style = match (selected, status.enabled) {
        (true, _) => theme.menu_selected,
        (false, true) => theme.menu,
        (false, false) => theme.menu.patch(theme.menu_disabled),
    };
    let mark = match (item.mark, status.checked) {
        (Mark::Radio, true) => radio,
        (Mark::Check, true) => check,
        _ => ' ',
    };
    let hotkey = status.enabled.then_some(theme.menu_hotkey);
    let mut spans = vec![Span::styled(format!(" {mark} "), style)];
    spans.extend(item.label.spans(style, hotkey));
    let key = status.key.as_deref().unwrap_or_default();
    let used = 3 + cells::width(&item.label.text);
    let rest = room.saturating_sub(used + 1);
    spans.push(Span::styled(
        format!("{} ", cells::fit(key, rest, Align::Right)),
        style,
    ));
    Line::from(spans)
}

/// Draws the menu bar of F9 on `area` while no menu is open, as `ui.menu_bar` keeps it.
pub(crate) fn render_idle(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let titles = titles();
    let titles: Vec<&Label> = titles.iter().collect();
    render_bar(frame, area, theme, &titles, None);
}

/// Draws the bar: `titles` from the left, the one at `selected` set apart, with their letters
/// while a menu bar is open. Returns the column each title starts at.
fn render_bar(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    titles: &[&Label],
    selected: Option<usize>,
) -> Vec<u16> {
    if area.height == 0 {
        return Vec::new();
    }
    let style = if selected.is_some() {
        theme.menu_bar
    } else {
        theme.menu_bar_inactive
    };
    let hotkey = selected.map(|_| theme.menu_hotkey);
    let row = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(
        Line::styled(" ".repeat(usize::from(area.width)), style),
        row,
    );
    let mut starts = Vec::with_capacity(titles.len());
    // As mc spaces them: two cells before the first title, and between titles.
    let mut x = area.x.saturating_add(1);
    for (index, title) in titles.iter().enumerate() {
        let width = u16::try_from(cells::width(&title.text) + 2).unwrap_or(u16::MAX);
        starts.push(x + 1);
        let shown = if selected == Some(index) {
            theme.menu_bar_selected
        } else {
            style
        };
        let room = area.right().saturating_sub(x);
        if room >= width {
            let mut spans = vec![Span::styled(" ", shown)];
            spans.extend(title.spans(shown, hotkey));
            spans.push(Span::styled(" ", shown));
            frame.render_widget(Line::from(spans), Rect::new(x, area.y, width, 1));
        } else if room > 0 {
            let text = format!(" {} ", title.text);
            let fitted = cells::fit(&text, usize::from(room), Align::Left);
            frame.render_widget(Line::styled(fitted, shown), Rect::new(x, area.y, room, 1));
        }
        x = x.saturating_add(width).saturating_add(3);
    }
    starts
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    /// Every command runs and shows no key, but Delete, which cannot run, and the panels'
    /// sort orders, by name.
    fn status(command: Command) -> Status {
        Status {
            enabled: command != Command::Do(Action::Delete),
            checked: matches!(command, Command::On(_, Action::SortByName)),
            key: match command {
                Command::Do(Action::View) => Some("F3".to_owned()),
                Command::Do(Action::Checksum) => Some("Ctrl-x #".to_owned()),
                _ => None,
            },
        }
    }

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    /// The menu bar at the `active` panel's menu, opened.
    fn open(active: Side) -> PullDown {
        let mut menu = PullDown::new(active, false, None, &status);
        assert!(!menu.place.open, "the bar alone");
        menu.handle(action(Action::Down), &status);
        menu
    }

    fn chosen(menu: &PullDown) -> Option<Command> {
        menu.menu().item(menu.cursor()).map(|item| item.command)
    }

    #[test]
    fn labels_mark_their_letter() {
        assert_eq!(
            Label::parse("Sort by si&ze"),
            Label {
                text: "Sort by size".to_owned(),
                hotkey: Some((10, 'z'))
            }
        );
        assert_eq!(Label::parse("&View").hotkey, Some((0, 'v')));
        assert_eq!(
            Label::parse("R&&D &Notes"),
            Label {
                text: "R&D Notes".to_owned(),
                hotkey: Some((4, 'n'))
            }
        );
        assert_eq!(Label::parse("Plain").hotkey, None);
    }

    #[test]
    fn every_command_has_a_letter_of_its_own_in_its_menu() {
        let menu = PullDown::new(Side::Left, false, None, &status);
        let titles: HashSet<_> = menu
            .menus
            .iter()
            .filter_map(|each| each.title.hotkey.map(|(_, letter)| letter))
            .collect();
        assert_eq!(titles.len(), menu.menus.len(), "a letter for each menu");
        for each in &menu.menus {
            let mut letters = HashSet::new();
            for entry in &each.entries {
                if let Entry::Item(item) = entry {
                    let (_, letter) = item.label.hotkey.unwrap_or_else(|| {
                        panic!("{:?} in {} has no letter", item.label.text, each.title.text)
                    });
                    assert!(
                        letters.insert(letter),
                        "{letter} twice in {}",
                        each.title.text
                    );
                }
            }
        }
    }

    #[test]
    fn opens_at_the_active_panel_and_follows_where_panels_are_drawn() {
        let left = open(Side::Left);
        assert_eq!(left.place.selected, 0);
        assert_eq!(chosen(&left), Some(Command::Location(Side::Left)));
        let right = open(Side::Right);
        assert_eq!(right.place.selected, 4);
        assert_eq!(chosen(&right), Some(Command::Location(Side::Right)));
        // Swapped, the right panel is drawn on the left, under Left.
        let mut swapped = PullDown::new(Side::Right, true, None, &status);
        swapped.handle(action(Action::Confirm), &status);
        assert_eq!(swapped.place.selected, 0);
        assert_eq!(chosen(&swapped), Some(Command::Location(Side::Right)));
    }

    #[test]
    fn keys_move_round_the_bar_and_the_menu_past_what_cannot_run() {
        let mut menu = open(Side::Left);
        assert_eq!(
            menu.handle(action(Action::Left), &status),
            PullDownEvent::Pending
        );
        assert_eq!(menu.place.selected, 4, "round the bar");
        assert!(menu.place.open, "opens the next menu");
        menu.handle(action(Action::Right), &status);
        menu.handle(action(Action::Right), &status);
        assert_eq!(menu.menu().title.text, "File");
        assert_eq!(chosen(&menu), Some(Command::Do(Action::View)));
        for _ in 0..5 {
            menu.handle(action(Action::Down), &status);
        }
        assert_eq!(
            chosen(&menu),
            Some(Command::Do(Action::Select)),
            "past Delete and the separator"
        );
        menu.handle(action(Action::Up), &status);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::Mkdir)));
        menu.handle(action(Action::Home), &status);
        menu.handle(action(Action::Up), &status);
        assert_eq!(
            chosen(&menu),
            Some(Command::Do(Action::Quit)),
            "round the menu"
        );
        menu.handle(action(Action::Home), &status);
        assert_eq!(
            menu.handle(action(Action::Confirm), &status),
            PullDownEvent::Run(Command::Do(Action::View))
        );
        assert_eq!(
            menu.handle(action(Action::Cancel), &status),
            PullDownEvent::Pending,
            "back to the bar"
        );
        assert!(!menu.place.open);
        assert_eq!(
            menu.handle(action(Action::Cancel), &status),
            PullDownEvent::Closed
        );
    }

    #[test]
    fn on_the_bar_keys_select_menus_and_letters_open_them() {
        let mut menu = PullDown::new(Side::Left, false, None, &status);
        menu.handle(action(Action::Right), &status);
        assert_eq!((menu.place.selected, menu.place.open), (1, false));
        menu.handle(action(Action::Left), &status);
        menu.handle(action(Action::Left), &status);
        assert_eq!((menu.place.selected, menu.place.open), (4, false));
        assert_eq!(
            menu.handle(Resolved::Insert('x'), &status),
            PullDownEvent::Pending,
            "Exit is in a menu that is not open"
        );
        assert_eq!(
            menu.handle(Resolved::Insert('O'), &status),
            PullDownEvent::Pending
        );
        assert_eq!(menu.menu().title.text, "Options");
        assert!(menu.place.open);
        assert_eq!(chosen(&menu), Some(Command::Configuration));
        menu.handle(action(Action::Cancel), &status);
        menu.handle(action(Action::Confirm), &status);
        assert!(menu.place.open, "Enter opens the menu");
    }

    #[test]
    fn opens_again_where_it_closed() {
        let mut menu = open(Side::Left);
        menu.handle(action(Action::Right), &status);
        menu.handle(action(Action::Down), &status);
        menu.handle(action(Action::Down), &status);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::Copy)));
        menu.handle(action(Action::Right), &status);
        menu.handle(action(Action::Down), &status);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::SwapPanels)));
        assert_eq!(
            menu.handle(action(Action::Confirm), &status),
            PullDownEvent::Run(Command::Do(Action::SwapPanels))
        );
        // The active panel does not matter once the menu has a place.
        let mut again = PullDown::new(Side::Right, false, Some(menu.place()), &status);
        assert_eq!(again.menu().title.text, "Command");
        assert!(again.place.open);
        assert_eq!(chosen(&again), Some(Command::Do(Action::SwapPanels)));
        again.handle(action(Action::Left), &status);
        assert_eq!(
            chosen(&again),
            Some(Command::Do(Action::Copy)),
            "each menu keeps its command"
        );
        // A letter puts the cursor on its command; one that cannot run now is passed by.
        again.handle(action(Action::Up), &status);
        assert_eq!(chosen(&again), Some(Command::Do(Action::Edit)));
        again.handle(Resolved::Insert('m'), &status);
        assert_eq!(chosen(&again), Some(Command::Do(Action::Mkdir)));
        let mut place = again.place().clone();
        place.cursors[1] = 5;
        let third = PullDown::new(Side::Left, false, Some(&place), &status);
        assert_eq!(chosen(&third), Some(Command::Do(Action::Select)));
        // Closed from the bar, it opens on the bar.
        again.handle(action(Action::Cancel), &status);
        again.handle(action(Action::Cancel), &status);
        let bar = PullDown::new(Side::Left, false, Some(again.place()), &status);
        assert_eq!((bar.place.selected, bar.place.open), (1, false));
    }

    #[test]
    fn letters_run_their_commands_in_the_open_menu() {
        let mut menu = open(Side::Left);
        assert_eq!(
            menu.handle(Resolved::Insert('Z'), &status),
            PullDownEvent::Run(Command::On(Side::Left, Action::SortBySize))
        );
        menu.handle(action(Action::Right), &status);
        assert_eq!(
            menu.handle(Resolved::Insert('d'), &status),
            PullDownEvent::Pending,
            "Delete cannot run"
        );
        assert_eq!(
            menu.handle(Resolved::Insert('q'), &status),
            PullDownEvent::Pending,
            "Quick search is in another menu"
        );
        assert_eq!(
            menu.handle(Resolved::Insert('x'), &status),
            PullDownEvent::Run(Command::Do(Action::Quit))
        );
    }

    fn draw(menu: &PullDown, theme: &Theme) -> String {
        let mut terminal = Terminal::new(TestBackend::new(64, 20)).unwrap();
        terminal
            .draw(|frame| {
                let screen = frame.area();
                let bar = Rect::new(0, 0, screen.width, 1);
                menu.render(frame, (bar, screen), theme, false, &status);
            })
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn draws_the_bar_and_the_open_menu() {
        let mut menu = open(Side::Left);
        menu.handle(action(Action::Right), &status);
        insta::assert_snapshot!(draw(&menu, &Theme::terminal()));
    }

    #[test]
    fn draws_the_bar_alone_with_the_letters_of_the_menus() {
        let menu = PullDown::new(Side::Left, false, None, &status);
        let theme = Theme::mc_classic();
        let text = draw(&menu, &theme);
        assert!(!text.contains('╔'), "{text}");
        let mut terminal = Terminal::new(TestBackend::new(64, 3)).unwrap();
        terminal
            .draw(|frame| {
                let bar = Rect::new(0, 0, frame.area().width, 1);
                menu.render(frame, (bar, frame.area()), &theme, false, &status);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let at = |x: u16| buffer[(x, 0)].clone();
        // `  Left     File`: L and F are the letters.
        assert_eq!(at(2).symbol(), "L");
        assert_eq!(at(2).fg, ratatui::style::Color::LightYellow);
        assert_ne!(at(3).fg, ratatui::style::Color::LightYellow);
        assert_eq!(at(11).symbol(), "F");
        assert_eq!(at(11).fg, ratatui::style::Color::LightYellow);
    }

    #[test]
    fn marks_the_sort_order_and_stays_on_screen() {
        let menu = open(Side::Right);
        let text = draw(&menu, &Theme::mc_classic());
        assert!(text.contains("* Sort by name"), "{text}");
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[1].trim_end().trim_end_matches('"').ends_with('╗'),
            "the frame reaches the right edge: {text}"
        );
    }
}
