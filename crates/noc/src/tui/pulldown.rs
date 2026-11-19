//! The pull-down menu of F9: a bar of menus (File, View, Go, Search, Tools, Window, Help), and
//! the commands of the one that is open. Every command acts on the active panel, as its key does;
//! Ctrl+x F9 acts on the other panel. Each command shows the key that does the same, from the
//! keymap, and has a letter that, typed with Alt, runs it while its menu is open; the plain
//! letters move, as `h`, `j`, `k`, and `l`. A menu taller than the screen scrolls with the
//! cursor.
//!
//! As in Far Manager, F9 opens the bar alone, at File, where Alt and each menu's letter open it,
//! and Esc in an open menu goes back to the bar; Shift+F9 opens it on the command that ran last.

use std::cell::{Cell, RefCell};

use noc_scroll::Motion;
use noc_scroll::scrollbar::{self, Bar};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use super::cells::{self, Align};
use super::glyphs;
use super::keymap::{Action, Resolved};
use super::label::Label;
use super::mouse::{Pointer, Press, Scroll};
use super::theme::Theme;
use super::window;
use crate::i18n::fl;

/// Cells between a command's name and its key.
const KEY_GAP: usize = 2;

/// What the app says about a command now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Status {
    /// Whether it does something; the cursor skips the others.
    pub(crate) enabled: bool,
    /// Whether its mark shows: the sort order of the panel, or a setting that is on.
    pub(crate) checked: bool,
    /// The keys that do the same, such as `o n`.
    pub(crate) key: Option<String>,
}

/// What a key did in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullDownEvent {
    Pending,
    Closed,
    /// Close the menu and do this in the active panel, as its key does.
    Run(Action),
}

/// The mark in front of a command that has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    /// One of several, such as a sort order: the dot of a radio button while it is the one taken.
    Choice,
    /// A setting that is on or off: the check of a check box while it is on.
    Check,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    label: Label,
    command: Action,
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
    /// The first entry shown of a menu taller than the screen, and how many show, at the last
    /// render.
    offset: Cell<usize>,
    page: Cell<usize>,
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
    /// Its scroll bar, on the right of its frame.
    bar: Option<Bar>,
}

/// The titles of the menu bar, left to right.
fn titles() -> [Label; 7] {
    [
        fl!("pulldown-file"),
        fl!("pulldown-view"),
        fl!("pulldown-go"),
        fl!("pulldown-search"),
        fl!("pulldown-tools"),
        fl!("pulldown-window"),
        fl!("pulldown-help-menu"),
    ]
    .map(|title| Label::parse(&title))
}

/// A menu of commands that act as their keys do; `None` is a separator.
fn plain_menu(title: Label, items: Vec<Option<(String, Action)>>) -> Menu {
    let entries = items
        .into_iter()
        .map(|item| match item {
            Some((text, action)) => Entry::Item(Item {
                label: Label::parse(&text),
                command: action,
                mark: match action {
                    Action::SortByName
                    | Action::SortByExtension
                    | Action::SortByTime
                    | Action::SortBySize => Mark::Choice,
                    Action::ToggleHidden | Action::ShowFull => Mark::Check,
                    _ => Mark::None,
                },
            }),
            None => Entry::Separator,
        })
        .collect();
    Menu { title, entries }
}

/// The File menu, under `title`.
fn file_menu(title: Label) -> Menu {
    plain_menu(
        title,
        vec![
            Some((fl!("pulldown-view-file"), Action::View)),
            Some((fl!("pulldown-edit"), Action::Edit)),
            Some((fl!("pulldown-system-open"), Action::SystemOpen)),
            Some((fl!("pulldown-checksum"), Action::Checksum)),
            None,
            Some((fl!("pulldown-copy"), Action::Copy)),
            Some((fl!("pulldown-duplicate"), Action::Duplicate)),
            Some((fl!("pulldown-move"), Action::Move)),
            Some((fl!("pulldown-rename"), Action::Rename)),
            Some((fl!("pulldown-mkdir"), Action::Mkdir)),
            Some((fl!("pulldown-delete"), Action::Delete)),
            None,
            Some((fl!("pulldown-select"), Action::Select)),
            Some((fl!("pulldown-unselect"), Action::Unselect)),
            Some((fl!("pulldown-invert"), Action::InvertMarks)),
            None,
            Some((fl!("pulldown-exit"), Action::Quit)),
        ],
    )
}

/// The seven menus on the bar, in order.
fn menus() -> Vec<Menu> {
    let [file, view, go, search, tools, window, help] = titles();
    vec![
        file_menu(file),
        plain_menu(
            view,
            vec![
                Some((fl!("pulldown-sort-name"), Action::SortByName)),
                Some((fl!("pulldown-sort-extension"), Action::SortByExtension)),
                Some((fl!("pulldown-sort-time"), Action::SortByTime)),
                Some((fl!("pulldown-sort-size"), Action::SortBySize)),
                None,
                Some((fl!("pulldown-hidden"), Action::ToggleHidden)),
                Some((fl!("pulldown-full"), Action::ShowFull)),
                None,
                Some((fl!("pulldown-rescan"), Action::Reload)),
                Some((fl!("pulldown-redraw"), Action::Redraw)),
            ],
        ),
        plain_menu(
            go,
            vec![
                Some((fl!("pulldown-location"), Action::LocationMenu)),
                Some((fl!("pulldown-edit-host"), Action::EditHost)),
                Some((fl!("pulldown-disconnect-host"), Action::Disconnect)),
                None,
                Some((fl!("pulldown-goto"), Action::GoToDir)),
                Some((fl!("pulldown-jump"), Action::Jump)),
                None,
                Some((fl!("pulldown-jump-back"), Action::JumpBack)),
                Some((fl!("pulldown-jump-forward"), Action::JumpForward)),
                None,
                Some((fl!("pulldown-other-open"), Action::OtherPanelOpen)),
                Some((fl!("pulldown-other-sync"), Action::OtherPanelSync)),
            ],
        ),
        plain_menu(
            search,
            vec![
                Some((fl!("pulldown-quick-search"), Action::QuickSearch)),
                Some((fl!("pulldown-next-match"), Action::NextMatch)),
                Some((fl!("pulldown-prev-match"), Action::PrevMatch)),
                None,
                Some((fl!("pulldown-find"), Action::FindFile)),
                Some((fl!("pulldown-find-names"), Action::FindNames)),
            ],
        ),
        plain_menu(
            tools,
            vec![
                Some((fl!("pulldown-shell"), Action::Shell)),
                Some((fl!("pulldown-command-history"), Action::CommandHistory)),
                Some((fl!("pulldown-user-screen"), Action::UserScreen)),
                None,
                Some((fl!("pulldown-jobs"), Action::Jobs)),
                None,
                Some((fl!("pulldown-configuration"), Action::Configuration)),
            ],
        ),
        plain_menu(
            window,
            vec![
                Some((fl!("pulldown-swap"), Action::SwapPanels)),
                None,
                Some((fl!("pulldown-new-tab"), Action::NewTab)),
                Some((fl!("pulldown-close-tab"), Action::CloseTab)),
                Some((fl!("pulldown-next-tab"), Action::NextTab)),
                Some((fl!("pulldown-prev-tab"), Action::PrevTab)),
                Some((fl!("pulldown-tab-list"), Action::TabList)),
                None,
                Some((fl!("pulldown-save-workspace"), Action::SaveWorkspace)),
                Some((fl!("pulldown-workspace-list"), Action::Workspaces)),
            ],
        ),
        plain_menu(
            help,
            vec![
                Some((fl!("pulldown-help"), Action::Help)),
                Some((fl!("pulldown-key-hints"), Action::KeyHints)),
                None,
                Some((fl!("pulldown-view-log"), Action::ViewLog)),
            ],
        ),
    ]
}

impl PullDown {
    /// The menu bar at File with none open; or, given the command that ran `last`, its menu open
    /// on it.
    pub(crate) fn new(last: Option<Action>, status: &dyn Fn(Action) -> Status) -> Self {
        let menus = menus();
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
            selected: 0,
            open: false,
            cursor: 0,
            offset: Cell::new(0),
            page: Cell::new(1),
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
        status: &dyn Fn(Action) -> Status,
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
    fn step(&self, forward: bool, status: &dyn Fn(Action) -> Status) -> Option<usize> {
        let cursor = self.cursor;
        if forward {
            self.next_enabled(cursor + 1, true, status)
        } else {
            self.next_enabled(cursor.checked_sub(1)?, false, status)
        }
    }

    /// The command that runs now above the cursor, or below it, round the menu: past the last
    /// to the first, and back.
    fn step_round(&self, forward: bool, status: &dyn Fn(Action) -> Status) -> Option<usize> {
        self.step(forward, status).or_else(|| {
            if forward {
                self.next_enabled(0, true, status)
            } else {
                let last = self.menu().entries.len().saturating_sub(1);
                self.next_enabled(last, false, status)
            }
        })
    }

    /// Opens the menu `index` below its title, with the cursor on its `entry`, or on the
    /// nearest command that runs now, below it first.
    fn open_at(&mut self, index: usize, entry: usize, status: &dyn Fn(Action) -> Status) {
        self.selected = index;
        self.open = true;
        let nearest = self
            .next_enabled(entry, true, status)
            .or_else(|| self.next_enabled(entry, false, status));
        self.cursor = nearest.unwrap_or(entry);
    }

    /// Opens the menu `index` below its title, as a click on the title of an idle menu bar
    /// does.
    pub(crate) fn open_menu(&mut self, index: usize, status: &dyn Fn(Action) -> Status) {
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
        status: &dyn Fn(Action) -> Status,
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
                // The wheel stops at the ends of the menu, where Up and Down go round it.
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
                Press::RightClick | Press::Drag => PullDownEvent::Pending,
            };
        }
        match press {
            Press::Click | Press::RightClick => PullDownEvent::Closed,
            Press::DoubleClick | Press::WheelUp | Press::WheelDown | Press::Drag => {
                PullDownEvent::Pending
            }
        }
    }

    /// The scroll bar of the open menu at the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.drawn.borrow().bar
    }

    /// Moves the cursor in the open menu as a press on its scroll bar asks: an arrow to the
    /// next command that runs now, as the wheel, stopping at the ends; the track a page; the
    /// thumb onto the screen scrolled to where it was dragged.
    pub(crate) fn scroll(&mut self, scroll: Scroll, status: &dyn Fn(Action) -> Status) {
        if !self.open {
            return;
        }
        let page = self.page.get().max(1);
        let near = |entry: usize, forward: bool| {
            self.next_enabled(entry, forward, status)
                .or_else(|| self.next_enabled(entry, !forward, status))
        };
        let cursor = match scroll {
            Scroll::By(Motion::Up) => self.step(false, status),
            Scroll::By(Motion::Down) => self.step(true, status),
            Scroll::By(Motion::PageUp) => near(self.cursor.saturating_sub(page), false),
            Scroll::By(Motion::PageDown) => {
                let last = self.menu().entries.len().saturating_sub(1);
                near((self.cursor + page).min(last), true)
            }
            Scroll::By(_) => None,
            Scroll::To(top) => {
                self.offset.set(top);
                if self.cursor < top {
                    near(top, true)
                } else if self.cursor >= top + page {
                    near(top + page - 1, false)
                } else {
                    None
                }
            }
        };
        self.cursor = cursor.unwrap_or(self.cursor);
    }

    /// Selects the menu `index` on the bar, and opens it if a menu is open.
    fn select(&mut self, index: usize, status: &dyn Fn(Action) -> Status) {
        if self.open {
            self.open_at(index, 0, status);
        } else {
            self.selected = index;
        }
    }

    /// Takes a key. On the bar, Left and Right select the next menu, round the bar, and Enter,
    /// Down, or a menu's letter (with Alt, which the keymap takes off) opens it, End on its last
    /// command; Up and Home do nothing there. In an open menu, Left and Right open the next menu;
    /// Up and Down move to the next command that runs now, round the menu; Home goes to the
    /// first, and on the first goes back to the bar; Enter or a command's letter runs it; Esc
    /// goes back to the bar, and closes the menu bar from there.
    pub(crate) fn handle(
        &mut self,
        input: Resolved,
        status: &dyn Fn(Action) -> Status,
    ) -> PullDownEvent {
        let count = self.menus.len();
        let entries = self.menu().entries.len();
        let selected = self.selected;
        match input {
            Resolved::Hotkey(c) => {
                let has = |label: &Label| label.has(c);
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
            Resolved::Insert(_) => {}
            Resolved::Action(action) => match action {
                Action::Left => self.select((selected + count - 1) % count, status),
                Action::Right => self.select((selected + 1) % count, status),
                Action::Down | Action::Confirm if !self.open => {
                    self.open_at(selected, 0, status);
                }
                Action::Up | Action::Home if !self.open => {}
                Action::Up | Action::Down => {
                    if let Some(cursor) = self.step_round(action == Action::Down, status) {
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
    /// if it is open, within `screen`, with the marks of the theme's glyphs.
    pub(crate) fn render(
        &self,
        frame: &mut Frame<'_>,
        (bar, screen): (Rect, Rect),
        theme: &Theme,
        status: &dyn Fn(Action) -> Status,
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
        self.render_menu(frame, x, below, theme, status);
    }

    /// The open menu, framed, from the column `x` at the top of `area`.
    fn render_menu(
        &self,
        frame: &mut Frame<'_>,
        x: u16,
        area: Rect,
        theme: &Theme,
        status: &dyn Fn(Action) -> Status,
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
        // As in mc, the frame starts a cell left of the title, and stays on screen.
        let x = x
            .saturating_sub(1)
            .max(area.x)
            .min(area.right().saturating_sub(width));
        let outer = Rect::new(x, area.y, width, height);
        if !window::fits(outer) {
            return;
        }
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
        // A menu taller than the screen scrolls to the cursor.
        let page = usize::from(rows.height);
        let total = menu.entries.len();
        let first = self
            .offset
            .get()
            .min(self.cursor)
            .max((self.cursor + 1).saturating_sub(page))
            .min(total.saturating_sub(page));
        self.offset.set(first);
        self.page.set(page);
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
                let line = item_line(item, status, index == self.cursor, room, theme);
                let row = Rect::new(rows.x, y, rows.width, 1);
                frame.render_widget(line, row);
                drawn.rows.push((index, row));
            } else {
                let line = format!("{left_tee}{}{right_tee}", glyphs::LINE.repeat(room));
                let row = Rect::new(outer.x, y, outer.width, 1);
                frame.render_widget(Line::styled(line, theme.menu), row);
            }
        }
        let bar = Rect::new(outer.right() - 1, rows.y, 1, rows.height);
        let position = (total, page, first);
        drawn.bar = scrollbar::render(frame, bar, position, theme.menu, theme.glyphs.scroll_bar);
    }
}

/// A command in `room` cells: its mark, the dot of a radio button while it is the one taken or
/// the check of a check box while it is on, in the color of titles as in dialogs, but on the
/// cursor and where the command cannot run now; its name with its letter, and its keys on the
/// right.
fn item_line(
    item: &Item,
    status: &Status,
    selected: bool,
    room: usize,
    theme: &Theme,
) -> Line<'static> {
    let style = match (selected, status.enabled) {
        (true, _) => theme.menu_selected,
        (false, true) => theme.menu,
        (false, false) => theme.menu.patch(theme.disabled),
    };
    let mark = match (item.mark, status.checked) {
        (Mark::Choice, true) => theme.glyphs.choice,
        (Mark::Check, true) => theme.glyphs.check,
        _ => ' ',
    };
    let mark_style = if selected || !status.enabled {
        style
    } else {
        style.patch(theme.dialog_symbol)
    };
    let truncated = if selected {
        theme.truncated_cursor
    } else {
        theme.truncated_menu
    };
    let hotkey = if selected {
        theme.menu_hotkey_selected
    } else {
        theme.menu_hotkey
    };
    let hotkey = status.enabled.then_some(hotkey);
    let mut spans = vec![
        Span::styled(" ", style),
        Span::styled(mark.to_string(), mark_style),
        Span::styled(" ", style),
    ];
    spans.extend(item.label.spans(style, hotkey, truncated));
    let key = status.key.as_deref().unwrap_or_default();
    let used = 3 + cells::width(&item.label.text);
    let rest = room.saturating_sub(used + 1);
    // In the colors of keys, but on the cursor and where the command cannot run now.
    let key_style = if selected || !status.enabled {
        style
    } else {
        theme.menu.patch(theme.menu_keys)
    };
    let styles = (key_style, truncated);
    spans.extend(cells::fit(
        key,
        rest,
        Align::Right,
        styles,
        theme.glyphs.marks,
    ));
    spans.push(Span::styled(" ", style));
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
    let hotkey = selected.map(|_| theme.menu_hotkey_selected);
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
        let (shown, truncated) = if selected == Some(index) {
            (theme.menu_bar_selected, theme.truncated_cursor)
        } else {
            (style, theme.truncated_bar)
        };
        if room >= width {
            let mut spans = vec![Span::styled(" ", shown)];
            spans.extend(title.spans(shown, hotkey, truncated));
            spans.push(Span::styled(" ", shown));
            frame.render_widget(Line::from(spans), Rect::new(x, area.y, width, 1));
        } else if room > 0 {
            let text = format!(" {} ", title.text);
            let styles = (shown, truncated);
            let fitted = cells::fit(
                &text,
                usize::from(room),
                Align::Left,
                styles,
                theme.glyphs.marks,
            );
            frame.render_widget(Line::from(fitted), Rect::new(x, area.y, room, 1));
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
    use crate::tui::glyphs::Glyphs;
    use crate::tui::keymap::{Context, Keymap};

    /// Every command runs but Delete, the panel sorts by name, and only View and Select group
    /// show a key, the keymap's.
    fn status(command: Action) -> Status {
        Status {
            enabled: command != Action::Delete,
            checked: command == Action::SortByName,
            key: match command {
                Action::View | Action::Select => Keymap::default().key(Context::Panel, command),
                _ => None,
            },
        }
    }

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    /// The menu bar at File, opened.
    fn open() -> PullDown {
        let mut menu = PullDown::new(None, &status);
        assert!(!menu.open, "the bar alone");
        menu.handle(action(Action::Down), &status);
        menu
    }

    fn chosen(menu: &PullDown) -> Option<Action> {
        menu.menu().item(menu.cursor).map(|item| item.command)
    }

    #[test]
    fn r_renames_in_place_as_it_does_in_the_panels() {
        let letter = |letter| open().handle(Resolved::Hotkey(letter), &status);
        assert_eq!(letter('r'), PullDownEvent::Run(Action::Rename));
        assert_eq!(letter('n'), PullDownEvent::Run(Action::Move));
    }

    #[test]
    fn every_command_has_a_letter_of_its_own_in_its_menu() {
        let menu = PullDown::new(None, &status);
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
    fn opens_at_file_on_the_bar_alone() {
        let menu = open();
        assert_eq!(menu.selected, 0);
        assert_eq!(menu.menu().title.text, "File");
        assert_eq!(chosen(&menu), Some(Action::View));
    }

    #[test]
    fn keys_move_round_the_bar_and_the_menu_past_what_cannot_run() {
        let mut menu = open();
        assert_eq!(
            menu.handle(action(Action::Left), &status),
            PullDownEvent::Pending
        );
        assert_eq!(menu.selected, 6, "round the bar");
        assert_eq!(menu.menu().title.text, "Help");
        assert!(menu.open, "opens the next menu");
        menu.handle(action(Action::Right), &status);
        assert_eq!(menu.selected, 0, "round the bar");
        assert_eq!(menu.menu().title.text, "File");
        assert_eq!(chosen(&menu), Some(Action::View));
        for _ in 0..9 {
            menu.handle(action(Action::Down), &status);
        }
        assert_eq!(
            chosen(&menu),
            Some(Action::Select),
            "past Delete and the separator"
        );
        menu.handle(action(Action::Up), &status);
        assert_eq!(chosen(&menu), Some(Action::Mkdir));
        menu.handle(action(Action::End), &status);
        assert_eq!(chosen(&menu), Some(Action::Quit));
        menu.handle(action(Action::Down), &status);
        assert_eq!(
            chosen(&menu),
            Some(Action::View),
            "Down on the last command goes round to the first"
        );
        assert_eq!(
            menu.handle(action(Action::Up), &status),
            PullDownEvent::Pending
        );
        assert_eq!(
            chosen(&menu),
            Some(Action::Quit),
            "Up on the first command goes round to the last"
        );
        assert!(menu.open);
        menu.handle(action(Action::Cancel), &status);
        assert_eq!(menu.menu().title.text, "File");
        menu.handle(action(Action::Up), &status);
        menu.handle(action(Action::Home), &status);
        assert!(!menu.open, "Up and Home on the bar open nothing");
        menu.handle(action(Action::Down), &status);
        assert_eq!(chosen(&menu), Some(Action::View));
        menu.handle(action(Action::End), &status);
        menu.handle(action(Action::Home), &status);
        assert_eq!(chosen(&menu), Some(Action::View), "Home goes up");
        menu.handle(action(Action::Home), &status);
        assert!(menu.open, "Home on the first command stays, as vim's g g");
        assert_eq!(chosen(&menu), Some(Action::View));
        assert_eq!(
            menu.handle(action(Action::Confirm), &status),
            PullDownEvent::Run(Action::View)
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
        let mut menu = PullDown::new(None, &status);
        menu.handle(action(Action::Right), &status);
        assert_eq!((menu.selected, menu.open), (1, false));
        menu.handle(action(Action::Left), &status);
        menu.handle(action(Action::Left), &status);
        assert_eq!((menu.selected, menu.open), (6, false));
        assert_eq!(
            menu.handle(Resolved::Hotkey('x'), &status),
            PullDownEvent::Pending,
            "Exit is in a menu that is not open"
        );
        assert_eq!(
            menu.handle(Resolved::Hotkey('T'), &status),
            PullDownEvent::Pending
        );
        assert_eq!(menu.menu().title.text, "Tools");
        assert!(menu.open);
        assert_eq!(chosen(&menu), Some(Action::Shell));
        menu.handle(action(Action::Cancel), &status);
        menu.handle(action(Action::Confirm), &status);
        assert!(menu.open, "Enter opens the menu");
    }

    #[test]
    fn opens_on_the_command_that_ran_last() {
        let hidden = Some(Action::ToggleHidden);
        let mut menu = PullDown::new(hidden, &status);
        assert_eq!(menu.menu().title.text, "View");
        assert!(menu.open);
        assert_eq!(chosen(&menu), Some(Action::ToggleHidden));
        menu.handle(action(Action::Left), &status);
        assert_eq!(
            chosen(&menu),
            Some(Action::View),
            "another menu opens at its first command"
        );
        // One that cannot run now is passed by.
        let delete = Some(Action::Delete);
        let menu = PullDown::new(delete, &status);
        assert_eq!(chosen(&menu), Some(Action::Select));
    }

    #[test]
    fn end_on_the_bar_opens_the_menu_at_its_last_command() {
        let mut menu = PullDown::new(None, &status);
        menu.handle(action(Action::Left), &status);
        menu.handle(action(Action::End), &status);
        assert!(menu.open);
        assert_eq!(chosen(&menu), Some(Action::ViewLog));
    }

    #[test]
    fn letters_run_their_commands_in_the_open_menu() {
        let mut menu = open();
        assert_eq!(
            menu.handle(Resolved::Hotkey('d'), &status),
            PullDownEvent::Pending,
            "Delete cannot run"
        );
        assert_eq!(
            menu.handle(Resolved::Hotkey('q'), &status),
            PullDownEvent::Pending,
            "Quick search is in another menu"
        );
        assert_eq!(
            menu.handle(Resolved::Hotkey('x'), &status),
            PullDownEvent::Run(Action::Quit)
        );
        menu.handle(action(Action::Right), &status);
        assert_eq!(
            menu.handle(Resolved::Hotkey('S'), &status),
            PullDownEvent::Run(Action::SortBySize)
        );
    }

    fn draw(menu: &PullDown, theme: &Theme) -> String {
        draw_in(menu, theme, 20)
    }

    /// On a screen `height` lines tall.
    fn draw_in(menu: &PullDown, theme: &Theme, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(72, height)).unwrap();
        terminal
            .draw(|frame| {
                let screen = frame.area();
                let bar = Rect::new(0, 0, screen.width, 1);
                menu.render(frame, (bar, screen), theme, &status);
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
        // On the 72 columns of `draw`: ` File ` takes columns 1 … 6, ` View ` 10 … 15, and File's
        // rows start on line 2, View first.
        let mut menu = PullDown::new(None, &status);
        assert_eq!(press(&mut menu, Press::Click, 3, 0), PullDownEvent::Pending);
        assert_eq!(menu.menu().title.text, "File");
        assert!(menu.open);
        assert_eq!(
            press(&mut menu, Press::Click, 5, 12),
            PullDownEvent::Pending,
            "Delete cannot run"
        );
        press(&mut menu, Press::WheelUp, 5, 12);
        assert!(menu.open, "the wheel stops at the first command");
        assert_eq!(chosen(&menu), Some(Action::View));
        press(&mut menu, Press::WheelDown, 5, 12);
        assert_eq!(chosen(&menu), Some(Action::Edit));
        assert_eq!(
            press(&mut menu, Press::Click, 5, 11),
            PullDownEvent::Run(Action::Mkdir)
        );
        assert_eq!(chosen(&menu), Some(Action::Mkdir));
        // The title of the open menu closes it, and another title opens its own.
        press(&mut menu, Press::Click, 3, 0);
        assert!(!menu.open);
        press(&mut menu, Press::Click, 12, 0);
        assert_eq!(menu.menu().title.text, "View");
        assert!(menu.open);
        assert_eq!(
            press(&mut menu, Press::Click, 60, 15),
            PullDownEvent::Closed,
            "a click elsewhere closes it"
        );
    }

    #[test]
    fn draws_the_bar_and_the_open_menu() {
        let menu = open();
        insta::assert_snapshot!(draw(&menu, &Theme::terminal()));
    }

    #[test]
    fn draws_the_bar_alone_with_the_letters_of_the_menus() {
        let menu = PullDown::new(None, &status);
        let theme = Theme::mc_classic();
        let text = draw(&menu, &theme);
        assert!(!text.contains('╔'), "{text}");
        let mut terminal = Terminal::new(TestBackend::new(72, 3)).unwrap();
        terminal
            .draw(|frame| {
                let bar = Rect::new(0, 0, frame.area().width, 1);
                menu.render(frame, (bar, frame.area()), &theme, &status);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let at = |x: u16| buffer[(x, 0)].clone();
        // `  File     View`: F and V are the letters.
        assert_eq!(at(2).symbol(), "F");
        assert_eq!(at(2).fg, ratatui::style::Color::LightYellow);
        assert_ne!(at(3).fg, ratatui::style::Color::LightYellow);
        assert_eq!(at(11).symbol(), "V");
        assert_eq!(at(11).fg, ratatui::style::Color::LightYellow);
    }

    #[test]
    fn marks_the_sort_order_and_stays_on_screen() {
        let mut menu = PullDown::new(None, &status);
        menu.handle(Resolved::Hotkey('v'), &status);
        let text = draw(&menu, &Theme::mc_classic());
        let chosen = format!("{} Sort by name", Glyphs::PLAIN.choice);
        assert!(text.contains(&chosen), "{text}");
        menu.handle(action(Action::Cancel), &status);
        menu.handle(Resolved::Hotkey('h'), &status);
        let text = draw(&menu, &Theme::mc_classic());
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[1].trim_end().trim_end_matches('"').ends_with('╗'),
            "the frame reaches the right edge: {text}"
        );
    }

    /// File on a screen 10 lines tall: 7 of its 17 rows show.
    fn draw_short(menu: &PullDown) -> String {
        draw_in(menu, &Theme::terminal(), 10)
    }

    #[test]
    fn a_menu_taller_than_the_screen_scrolls_to_the_cursor() {
        let mut menu = open();
        menu.handle(action(Action::End), &status);
        assert_eq!(chosen(&menu), Some(Action::Quit));
        let text = draw_short(&menu);
        assert!(
            text.contains("Exit") && !text.contains("Open externally"),
            "{text}"
        );
    }

    #[test]
    fn the_scroll_bar_of_a_tall_menu_moves_the_cursor_onto_the_screen() {
        let mut menu = open();
        draw_short(&menu);
        let bar = menu.bar().unwrap();
        let first = menu.cursor;
        menu.scroll(Scroll::By(Motion::Up), &status);
        assert_eq!(
            menu.cursor, first,
            "the arrow stops at the first, as the wheel"
        );
        menu.scroll(Scroll::By(Motion::Down), &status);
        assert!(menu.cursor > first);
        menu.scroll(Scroll::To(bar.dragged(0, u16::MAX)), &status);
        let end = draw_short(&menu);
        assert!(
            end.contains("Exit") && !end.contains("Open externally"),
            "{end}"
        );
        assert!(menu.cursor >= menu.offset.get(), "on the screen");
        assert_eq!(
            chosen(&menu),
            Some(Action::Select),
            "past Delete, which cannot run"
        );
        menu.scroll(Scroll::To(0), &status);
        assert!(draw_short(&menu).contains("Open externally"));
        let cursor = menu.cursor;
        menu.scroll(Scroll::By(Motion::PageDown), &status);
        assert!(menu.cursor >= cursor + menu.page.get() - 1, "a page on");
    }

    #[test]
    fn keys_take_the_color_of_keys_but_on_the_cursor_and_where_disabled() {
        let menu = PullDown::new(None, &status);
        let Some(Entry::Item(item)) = menu.menus[0].entries.first() else {
            panic!("{menu:?}");
        };
        let theme = Theme::mc_classic();
        let key_style = |selected: bool, enabled: bool| {
            let status = Status {
                enabled,
                checked: false,
                key: Some("o n".to_owned()),
            };
            let line = item_line(item, &status, selected, 40, &theme);
            line.spans
                .iter()
                .find(|span| span.content.contains("o n"))
                .map(|span| span.style)
                .unwrap()
        };
        assert_eq!(key_style(false, true), theme.menu.patch(theme.menu_keys));
        assert_eq!(key_style(true, true), theme.menu_selected);
        assert_eq!(key_style(false, false), theme.menu.patch(theme.disabled));
    }

    #[test]
    fn marks_take_the_color_of_titles_but_on_the_cursor_and_where_disabled() {
        let menu = PullDown::new(None, &status);
        let item = |menu_index: usize, index: usize| {
            let Some(Entry::Item(item)) = menu.menus[menu_index].entries.get(index) else {
                panic!("{menu:?}");
            };
            item
        };
        let theme = Theme::mc_classic();
        let glyphs = theme.glyphs;
        // Sort by name, one among others, and Show hidden files, on or off.
        for (item, kind, glyph) in [
            (item(1, 0), Mark::Choice, glyphs.choice),
            (item(1, 5), Mark::Check, glyphs.check),
        ] {
            assert_eq!(item.mark, kind, "{:?}", item.label.text);
            let mark = |selected: bool, enabled: bool, checked: bool| {
                let status = Status {
                    enabled,
                    checked,
                    key: None,
                };
                let line = item_line(item, &status, selected, 40, &theme);
                (line.spans[1].content.to_string(), line.spans[1].style)
            };
            let glyph = glyph.to_string();
            assert_eq!(
                mark(false, true, true),
                (glyph.clone(), theme.menu.patch(theme.dialog_symbol))
            );
            assert_eq!(mark(true, true, true), (glyph.clone(), theme.menu_selected));
            assert_eq!(
                mark(false, false, true),
                (glyph, theme.menu.patch(theme.disabled))
            );
            assert_eq!(mark(false, true, false).0, " ", "neither taken nor on");
        }
    }
}
