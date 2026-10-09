//! The pull-down menu of F9, as in mc: a bar of menus (Left, File, Command, Options,
//! Workspace, Right), and the commands of the one that is open. Left and Right act on the
//! panel drawn on that side; the others do what their keys do. Each command shows the key that
//! does the same, from the keymap, and has a letter that runs it while its menu is open. The
//! Workspace menu lists the saved workspaces too, the first ten with a digit; a menu taller
//! than the screen scrolls with the cursor.
//!
//! As in Far Manager, F9 opens the bar alone, at the menu of the active panel, where each menu's
//! letter opens it, and Esc in an open menu goes back to the bar; Shift+F9 opens it on the
//! command that ran last.

use std::cell::RefCell;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use super::app::Side;
use super::cells::{self, Align};
use super::keymap::{Action, Resolved};
use super::mouse::{Pointer, Press};
use super::theme::Theme;
use crate::i18n::fl;

/// Cells between a command's name and its key.
const KEY_GAP: usize = 2;

/// What a command does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    /// What the action does in the active panel, as its key does.
    Do(Action),
    /// What the action does in the panel on that side: sorting, reading it again, or its
    /// tabs.
    On(Side, Action),
    /// Opens the location menu of the panel on that side.
    Location(Side),
    /// Closes the connection of the host that the panel on that side shows.
    DisconnectPanel(Side),
    /// Opens the Configuration dialog.
    Configuration,
    /// Restores the saved workspace at this index in the Workspace menu.
    Workspace(usize),
}

/// What the app says about a command now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Status {
    /// Whether it does something; the cursor skips the others.
    pub(crate) enabled: bool,
    /// Whether its mark shows: the sort order of the panel, or a setting that is on.
    pub(crate) checked: bool,
    /// The keys that do the same, such as `Ctrl+F3`.
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

/// The open menu bar, the menu selected on it, and the command under the cursor in that menu.
#[derive(Debug)]
pub(crate) struct PullDown {
    menus: Vec<Menu>,
    /// The menu selected on the bar.
    selected: usize,
    /// Whether that menu is open below its title, or only the bar is.
    open: bool,
    /// The entry under the cursor in that menu.
    cursor: usize,
    /// Where the last render drew it, for the mouse.
    drawn: RefCell<Drawn>,
}

/// Where the menu bar and the open menu were drawn.
#[derive(Debug, Default)]
struct Drawn {
    /// The titles on the bar.
    titles: Vec<Rect>,
    /// The open menu with its frame.
    menu: Rect,
    /// Its rows shown, by the index of their entries.
    rows: Vec<(usize, Rect)>,
}

/// The titles of the menu bar, left to right.
fn titles() -> [Label; 6] {
    [
        fl!("pulldown-left"),
        fl!("pulldown-file"),
        fl!("pulldown-command"),
        fl!("pulldown-options"),
        fl!("pulldown-workspace"),
        fl!("pulldown-right"),
    ]
    .map(|title| Label::parse(&title))
}

/// Cells a workspace's name takes at most in the Workspace menu.
const WORKSPACE_NAME_WIDTH: usize = 40;

/// Workspace: save the tabs of both panels, open the list of the saved workspaces, then the
/// saved `workspaces`, which restore; the first ten have the digits `1` … `9` and `0`, and
/// the one restored or saved last a mark.
fn workspace_menu(title: Label, workspaces: &[String]) -> Menu {
    let item = |text: String, command, mark| {
        Entry::Item(Item {
            label: Label::parse(&text),
            command,
            mark,
        })
    };
    let mut entries = vec![
        item(
            fl!("pulldown-save-workspace"),
            Command::Do(Action::SaveWorkspace),
            Mark::None,
        ),
        item(
            fl!("pulldown-workspace-list"),
            Command::Do(Action::Workspaces),
            Mark::None,
        ),
    ];
    if !workspaces.is_empty() {
        entries.push(Entry::Separator);
    }
    for (index, name) in workspaces.iter().enumerate() {
        let name = cells::sanitize(name.as_bytes());
        let width = cells::width(&name).min(WORKSPACE_NAME_WIDTH);
        let name = cells::fit(&name, width, Align::Left).replace('&', "&&");
        let text = match index {
            0..=9 => format!("&{} {name}", (index + 1) % 10),
            _ => format!("  {name}"),
        };
        entries.push(item(text, Command::Workspace(index), Mark::Radio));
    }
    Menu { title, entries }
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
            Entry::Separator,
            item(
                fl!("pulldown-new-tab"),
                Command::On(side, Action::NewTab),
                Mark::None,
            ),
            item(
                fl!("pulldown-close-tab"),
                Command::On(side, Action::CloseTab),
                Mark::None,
            ),
            item(
                fl!("pulldown-tab-list"),
                Command::On(side, Action::TabList),
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
    /// The menu bar with the menu of the `active` panel selected and none open; or, given the
    /// command that ran `last`, its menu open on it, if the menus still have it. `swapped`
    /// panels are drawn on each other's sides, and Left and Right go with where they are drawn,
    /// so a command on a panel stays with that panel. The Workspace menu lists the saved
    /// `workspaces`.
    pub(crate) fn new(
        active: Side,
        swapped: bool,
        last: Option<Command>,
        workspaces: &[String],
        status: &dyn Fn(Command) -> Status,
    ) -> Self {
        let [left, file, command, options, workspace, right] = titles();
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
                    Some((fl!("pulldown-rename"), Action::Rename)),
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
                    Some((fl!("pulldown-quick-cd"), Action::QuickCd)),
                    Some((fl!("pulldown-jump"), Action::Jump)),
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
            workspace_menu(workspace, workspaces),
            panel_menu(right, on_right),
        ];
        let selected = if active == on_left {
            0
        } else {
            menus.len() - 1
        };
        let found = last.and_then(|last| {
            menus.iter().enumerate().find_map(|(index, menu)| {
                let entry = menu
                    .entries
                    .iter()
                    .position(|entry| matches!(entry, Entry::Item(item) if item.command == last))?;
                Some((index, entry))
            })
        });
        let mut pulldown = Self {
            menus,
            selected,
            open: false,
            cursor: 0,
            drawn: RefCell::default(),
        };
        if let Some((index, entry)) = found {
            pulldown.open_at(index, entry, status);
        }
        pulldown
    }

    fn menu(&self) -> &Menu {
        &self.menus[self.selected]
    }

    /// The first entry from `start`, forward to the end of the menu or back to its top, that is
    /// a command that runs now.
    fn next_enabled(
        &self,
        start: usize,
        forward: bool,
        status: &dyn Fn(Command) -> Status,
    ) -> Option<usize> {
        let menu = self.menu();
        let runs = |&index: &usize| {
            menu.item(index)
                .is_some_and(|item| status(item.command).enabled)
        };
        if forward {
            (start..menu.entries.len()).find(runs)
        } else {
            (0..=start.min(menu.entries.len().saturating_sub(1)))
                .rev()
                .find(runs)
        }
    }

    /// The command that runs now above the cursor, or below it.
    fn step(&self, forward: bool, status: &dyn Fn(Command) -> Status) -> Option<usize> {
        let cursor = self.cursor;
        if forward {
            self.next_enabled(cursor + 1, true, status)
        } else {
            self.next_enabled(cursor.checked_sub(1)?, false, status)
        }
    }

    /// Opens the menu `index` below its title, with the cursor on its `entry`, or on the
    /// nearest command that runs now, below it first.
    fn open_at(&mut self, index: usize, entry: usize, status: &dyn Fn(Command) -> Status) {
        self.selected = index;
        self.open = true;
        let nearest = self
            .next_enabled(entry, true, status)
            .or_else(|| self.next_enabled(entry, false, status));
        self.cursor = nearest.unwrap_or(entry);
    }

    /// Opens the menu `index` below its title, as a click on the title of an idle menu bar
    /// does.
    pub(crate) fn open_menu(&mut self, index: usize, status: &dyn Fn(Command) -> Status) {
        if index < self.menus.len() {
            self.open_at(index, 0, status);
        }
    }

    /// Takes a press of the mouse, where the menu was drawn last. A click on a title opens its
    /// menu, or closes the one open there; a click on a command runs it, if it runs now; the
    /// wheel moves the cursor in the open menu; and a click elsewhere closes the menu bar.
    pub(crate) fn pointer(
        &mut self,
        pointer: Pointer,
        status: &dyn Fn(Command) -> Status,
    ) -> PullDownEvent {
        let Pointer { press, at } = pointer;
        let (title, in_menu, row) = {
            let drawn = self.drawn.borrow();
            let title = drawn.titles.iter().position(|title| title.contains(at));
            let row = drawn.rows.iter().find(|(_, row)| row.contains(at));
            (title, drawn.menu.contains(at), row.map(|(index, _)| *index))
        };
        if let Some(index) = title {
            if press == Press::Click {
                if self.open && self.selected == index {
                    self.open = false;
                } else {
                    self.open_at(index, 0, status);
                }
            }
            return PullDownEvent::Pending;
        }
        if in_menu {
            return match press {
                // The wheel stops at the first command, where Up would leave the menu.
                Press::WheelUp | Press::WheelDown => {
                    if let Some(cursor) = self.step(press == Press::WheelDown, status) {
                        self.cursor = cursor;
                    }
                    PullDownEvent::Pending
                }
                Press::Click | Press::DoubleClick => {
                    let item = row.and_then(|index| Some((index, self.menu().item(index)?)));
                    match item {
                        Some((index, item)) if status(item.command).enabled => {
                            let command = item.command;
                            self.cursor = index;
                            PullDownEvent::Run(command)
                        }
                        _ => PullDownEvent::Pending,
                    }
                }
                Press::RightClick => PullDownEvent::Pending,
            };
        }
        match press {
            Press::Click | Press::RightClick => PullDownEvent::Closed,
            Press::DoubleClick | Press::WheelUp | Press::WheelDown => PullDownEvent::Pending,
        }
    }

    /// Selects the menu `index` on the bar, and opens it if a menu is open.
    fn select(&mut self, index: usize, status: &dyn Fn(Command) -> Status) {
        if self.open {
            self.open_at(index, 0, status);
        } else {
            self.selected = index;
        }
    }

    /// Takes a key. On the bar, Left and Right select the next menu, up to either end, and Enter,
    /// Down, or a menu's letter opens it; Up and Home do nothing there. In an open menu, Left
    /// and Right open the next menu; Up and Down move to the next command that runs now, and
    /// stop at the last one; Home goes to the first; Up or Home on the first goes back to the
    /// bar; Enter or a command's letter runs it; Esc goes back to the bar, and closes the menu
    /// bar from there.
    pub(crate) fn handle(
        &mut self,
        input: Resolved,
        status: &dyn Fn(Command) -> Status,
    ) -> PullDownEvent {
        let count = self.menus.len();
        let entries = self.menu().entries.len();
        let selected = self.selected;
        match input {
            Resolved::Insert(c) => {
                let lower = c.to_lowercase().next().unwrap_or(c);
                let has = |label: &Label| label.hotkey.is_some_and(|(_, key)| key == lower);
                if !self.open {
                    if let Some(index) = self.menus.iter().position(|menu| has(&menu.title)) {
                        self.open_at(index, 0, status);
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
                    self.cursor = index;
                    return PullDownEvent::Run(command);
                }
            }
            Resolved::Action(action) => match action {
                Action::Left => self.select(selected.saturating_sub(1), status),
                Action::Right => self.select((selected + 1).min(count - 1), status),
                Action::Down | Action::Confirm if !self.open => {
                    self.open_at(selected, 0, status);
                }
                Action::Up | Action::Home if !self.open => {}
                Action::Up | Action::Home if self.step(false, status).is_none() => {
                    self.open = false;
                }
                Action::Up => {
                    if let Some(cursor) = self.step(false, status) {
                        self.cursor = cursor;
                    }
                }
                Action::Down => {
                    if let Some(cursor) = self.step(true, status) {
                        self.cursor = cursor;
                    }
                }
                Action::Home => {
                    let cursor = self.next_enabled(0, true, status);
                    self.cursor = cursor.unwrap_or(self.cursor);
                }
                Action::End => {
                    if !self.open {
                        self.open_at(selected, 0, status);
                    }
                    let last = entries.saturating_sub(1);
                    let cursor = self.next_enabled(last, false, status);
                    self.cursor = cursor.unwrap_or(self.cursor);
                }
                Action::Confirm => {
                    if let Some(item) = self.menu().item(self.cursor)
                        && status(item.command).enabled
                    {
                        return PullDownEvent::Run(item.command);
                    }
                }
                Action::Cancel if self.open => self.open = false,
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
        let spots = render_bar(frame, bar, theme, &titles, Some(self.selected));
        let x = spots.get(self.selected).map_or(bar.x, |title| title.x + 1);
        *self.drawn.borrow_mut() = Drawn {
            titles: spots,
            ..Drawn::default()
        };
        if !self.open {
            return;
        }
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
        let mut drawn = self.drawn.borrow_mut();
        drawn.menu = outer;
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
        // A menu taller than the screen shows the rows down to the cursor.
        let page = usize::from(rows.height);
        let first = (self.cursor + 1).saturating_sub(page);
        for (line, (index, (entry, status))) in menu
            .entries
            .iter()
            .zip(&statuses)
            .enumerate()
            .skip(first)
            .take(page)
            .enumerate()
        {
            let Ok(line) = u16::try_from(line) else {
                break;
            };
            let y = rows.y + line;
            if let (Entry::Item(item), Some(status)) = (entry, status) {
                let line = item_line(item, status, index == self.cursor, room, theme, marks);
                let row = Rect::new(rows.x, y, rows.width, 1);
                frame.render_widget(line, row);
                drawn.rows.push((index, row));
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
/// Returns where each title is.
pub(crate) fn render_idle(frame: &mut Frame<'_>, area: Rect, theme: &Theme) -> Vec<Rect> {
    let titles = titles();
    let titles: Vec<&Label> = titles.iter().collect();
    render_bar(frame, area, theme, &titles, None)
}

/// Draws the bar: `titles` from the left, the one at `selected` set apart, with their letters
/// while a menu bar is open. Returns where each title is, with a cell on either side.
fn render_bar(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    titles: &[&Label],
    selected: Option<usize>,
) -> Vec<Rect> {
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
    let mut spots = Vec::with_capacity(titles.len());
    // As mc spaces them: two cells before the first title, and between titles.
    let mut x = area.x.saturating_add(1);
    for (index, title) in titles.iter().enumerate() {
        let width = u16::try_from(cells::width(&title.text) + 2).unwrap_or(u16::MAX);
        let room = area.right().saturating_sub(x);
        spots.push(Rect::new(x, area.y, width.min(room), 1));
        let shown = if selected == Some(index) {
            theme.menu_bar_selected
        } else {
            style
        };
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
    spots
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
                Command::Do(Action::Checksum) => Some("Ctrl+x #".to_owned()),
                _ => None,
            },
        }
    }

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    /// The menu bar at the `active` panel's menu, opened.
    fn open(active: Side) -> PullDown {
        let mut menu = PullDown::new(active, false, None, &[], &status);
        assert!(!menu.open, "the bar alone");
        menu.handle(action(Action::Down), &status);
        menu
    }

    fn chosen(menu: &PullDown) -> Option<Command> {
        menu.menu().item(menu.cursor).map(|item| item.command)
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
        let names: Vec<String> = (1..=10).map(|index| format!("w{index}")).collect();
        let menu = PullDown::new(Side::Left, false, None, &names, &status);
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
        assert_eq!(left.selected, 0);
        assert_eq!(chosen(&left), Some(Command::Location(Side::Left)));
        let right = open(Side::Right);
        assert_eq!(right.selected, 5);
        assert_eq!(chosen(&right), Some(Command::Location(Side::Right)));
        // Swapped, the right panel is drawn on the left, under Left.
        let mut swapped = PullDown::new(Side::Right, true, None, &[], &status);
        swapped.handle(action(Action::Confirm), &status);
        assert_eq!(swapped.selected, 0);
        assert_eq!(chosen(&swapped), Some(Command::Location(Side::Right)));
    }

    #[test]
    fn keys_move_along_the_bar_and_down_the_menu_past_what_cannot_run() {
        let mut menu = open(Side::Left);
        assert_eq!(
            menu.handle(action(Action::Left), &status),
            PullDownEvent::Pending
        );
        assert_eq!(menu.selected, 0, "Left stops at the first menu");
        assert!(menu.open);
        let mut right = open(Side::Right);
        right.handle(action(Action::Right), &status);
        assert_eq!(right.selected, 5, "Right stops at the last menu");
        menu.handle(action(Action::Right), &status);
        assert!(menu.open, "opens the next menu");
        assert_eq!(menu.menu().title.text, "File");
        assert_eq!(chosen(&menu), Some(Command::Do(Action::View)));
        for _ in 0..6 {
            menu.handle(action(Action::Down), &status);
        }
        assert_eq!(
            chosen(&menu),
            Some(Command::Do(Action::Select)),
            "past Delete and the separator"
        );
        menu.handle(action(Action::Up), &status);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::Mkdir)));
        menu.handle(action(Action::End), &status);
        menu.handle(action(Action::Down), &status);
        assert_eq!(
            chosen(&menu),
            Some(Command::Do(Action::Quit)),
            "Down stops at the last command"
        );
        menu.handle(action(Action::Home), &status);
        assert_eq!(
            menu.handle(action(Action::Up), &status),
            PullDownEvent::Pending
        );
        assert!(!menu.open, "Up on the first command goes back to the bar");
        assert_eq!(menu.menu().title.text, "File");
        menu.handle(action(Action::Up), &status);
        menu.handle(action(Action::Home), &status);
        assert!(!menu.open, "Up and Home on the bar open nothing");
        menu.handle(action(Action::Down), &status);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::View)));
        menu.handle(action(Action::End), &status);
        menu.handle(action(Action::Home), &status);
        assert_eq!(
            chosen(&menu),
            Some(Command::Do(Action::View)),
            "Home goes up"
        );
        menu.handle(action(Action::Home), &status);
        assert!(!menu.open, "Home on the first command goes back to the bar");
        menu.handle(action(Action::Down), &status);
        assert_eq!(
            menu.handle(action(Action::Confirm), &status),
            PullDownEvent::Run(Command::Do(Action::View))
        );
        assert_eq!(
            menu.handle(action(Action::Cancel), &status),
            PullDownEvent::Pending,
            "back to the bar"
        );
        assert!(!menu.open);
        assert_eq!(
            menu.handle(action(Action::Cancel), &status),
            PullDownEvent::Closed
        );
    }

    #[test]
    fn on_the_bar_keys_select_menus_and_letters_open_them() {
        let mut menu = PullDown::new(Side::Left, false, None, &[], &status);
        menu.handle(action(Action::Right), &status);
        assert_eq!((menu.selected, menu.open), (1, false));
        menu.handle(action(Action::Left), &status);
        menu.handle(action(Action::Left), &status);
        assert_eq!((menu.selected, menu.open), (0, false));
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
        assert!(menu.open);
        assert_eq!(chosen(&menu), Some(Command::Configuration));
        menu.handle(action(Action::Cancel), &status);
        menu.handle(action(Action::Confirm), &status);
        assert!(menu.open, "Enter opens the menu");
    }

    #[test]
    fn opens_on_the_command_that_ran_last() {
        // The active panel does not matter then.
        let swap = Some(Command::Do(Action::SwapPanels));
        let mut menu = PullDown::new(Side::Right, false, swap, &[], &status);
        assert_eq!(menu.menu().title.text, "Command");
        assert!(menu.open);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::SwapPanels)));
        menu.handle(action(Action::Left), &status);
        assert_eq!(
            chosen(&menu),
            Some(Command::Do(Action::View)),
            "another menu opens at its first command"
        );
        // One that cannot run now is passed by.
        let delete = Some(Command::Do(Action::Delete));
        let menu = PullDown::new(Side::Left, false, delete, &[], &status);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::Select)));
        // A command on a panel stays with the panel when the panels swap sides.
        let sort = Some(Command::On(Side::Left, Action::SortBySize));
        let menu = PullDown::new(Side::Left, true, sort, &[], &status);
        assert_eq!(menu.menu().title.text, "Right");
        assert_eq!(chosen(&menu), sort);
        // Without it, the bar alone at the active panel's menu: a workspace no longer saved.
        let gone = Some(Command::Workspace(3));
        let names = ["w1".to_owned()];
        let menu = PullDown::new(Side::Right, false, gone, &names, &status);
        assert_eq!((menu.selected, menu.open), (5, false));
        let menu = PullDown::new(Side::Left, false, None, &names, &status);
        assert_eq!((menu.selected, menu.open), (0, false));
    }

    #[test]
    fn end_on_the_bar_opens_the_menu_at_its_last_command() {
        let mut menu = open(Side::Left);
        menu.handle(action(Action::Down), &status);
        menu.handle(action(Action::Cancel), &status);
        menu.handle(action(Action::Right), &status);
        menu.handle(action(Action::Right), &status);
        menu.handle(action(Action::End), &status);
        assert!(menu.open);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::Redraw)));
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

    fn press(menu: &mut PullDown, press: Press, x: u16, y: u16) -> PullDownEvent {
        draw(menu, &Theme::terminal());
        let at = ratatui::layout::Position::new(x, y);
        menu.pointer(Pointer { press, at }, &status)
    }

    #[test]
    fn the_mouse_opens_menus_and_runs_commands() {
        // On the 64 columns of `draw`: ` File ` takes columns 10 … 15, and its menu's rows
        // start on line 2, View first.
        let mut menu = PullDown::new(Side::Left, false, None, &[], &status);
        assert_eq!(
            press(&mut menu, Press::Click, 12, 0),
            PullDownEvent::Pending
        );
        assert_eq!(menu.menu().title.text, "File");
        assert!(menu.open);
        assert_eq!(
            press(&mut menu, Press::Click, 14, 8),
            PullDownEvent::Pending,
            "Delete cannot run"
        );
        press(&mut menu, Press::WheelUp, 14, 8);
        assert!(menu.open, "the wheel stops at the first command");
        assert_eq!(chosen(&menu), Some(Command::Do(Action::View)));
        press(&mut menu, Press::WheelDown, 14, 8);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::Edit)));
        assert_eq!(
            press(&mut menu, Press::Click, 14, 7),
            PullDownEvent::Run(Command::Do(Action::Mkdir))
        );
        assert_eq!(chosen(&menu), Some(Command::Do(Action::Mkdir)));
        // The title of the open menu closes it, and another title opens its own.
        press(&mut menu, Press::Click, 12, 0);
        assert!(!menu.open);
        press(&mut menu, Press::Click, 21, 0);
        assert_eq!(menu.menu().title.text, "Command");
        assert!(menu.open);
        assert_eq!(
            press(&mut menu, Press::Click, 60, 15),
            PullDownEvent::Closed,
            "a click elsewhere closes it"
        );
    }

    #[test]
    fn draws_the_bar_and_the_open_menu() {
        let mut menu = open(Side::Left);
        menu.handle(action(Action::Right), &status);
        insta::assert_snapshot!(draw(&menu, &Theme::terminal()));
    }

    #[test]
    fn draws_the_bar_alone_with_the_letters_of_the_menus() {
        let menu = PullDown::new(Side::Left, false, None, &[], &status);
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

    /// The Workspace menu, open, with `count` workspaces named `w1`, `w2`, ….
    fn workspace_menu(count: usize) -> PullDown {
        let names: Vec<String> = (1..=count).map(|index| format!("w{index}")).collect();
        let mut menu = PullDown::new(Side::Left, false, None, &names, &status);
        menu.handle(Resolved::Insert('w'), &status);
        assert_eq!(menu.menu().title.text, "Workspace");
        menu
    }

    #[test]
    fn the_workspace_menu_saves_lists_and_restores_by_digit() {
        let mut menu = workspace_menu(12);
        assert_eq!(chosen(&menu), Some(Command::Do(Action::SaveWorkspace)));
        assert_eq!(
            menu.handle(Resolved::Insert('2'), &status),
            PullDownEvent::Run(Command::Workspace(1))
        );
        assert_eq!(
            menu.handle(Resolved::Insert('0'), &status),
            PullDownEvent::Run(Command::Workspace(9)),
            "0 for the tenth"
        );
        assert_eq!(
            menu.handle(Resolved::Insert('l'), &status),
            PullDownEvent::Run(Command::Do(Action::Workspaces))
        );
        let labels: Vec<&str> = menu
            .menu()
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item(item) => Some(item.label.text.as_str()),
                Entry::Separator => None,
            })
            .collect();
        assert_eq!(labels[2], "1 w1");
        assert_eq!(labels[11], "0 w10");
        assert_eq!(labels[12], "  w11", "no digit left");
        let empty = workspace_menu(0);
        assert_eq!(empty.menu().entries.len(), 2, "no separator without any");
    }

    #[test]
    fn a_workspace_menu_taller_than_the_screen_scrolls_to_the_cursor() {
        let mut menu = workspace_menu(30);
        menu.handle(action(Action::End), &status);
        assert_eq!(chosen(&menu), Some(Command::Workspace(29)));
        let text = draw(&menu, &Theme::terminal());
        assert!(
            text.contains("w30") && !text.contains("Save workspace"),
            "{text}"
        );
    }

    #[test]
    fn names_with_ampersands_keep_them_and_no_letter() {
        let names = ["R&D".to_owned()];
        let menu = PullDown::new(Side::Left, false, None, &names, &status);
        let menu = &menu.menus[4];
        let Some(Entry::Item(item)) = menu.entries.last() else {
            panic!("{menu:?}");
        };
        assert_eq!(item.label.text, "1 R&D");
        assert_eq!(item.label.hotkey, Some((0, '1')));
    }
}
