//! The mouse: clicks, the wheel, and the scroll bars.

use noc_config::Wheel;
use noc_scroll::Motion;
use noc_scroll::scrollbar::{self, Part};
use noc_viewer::Command as ViewerCommand;
use ratatui::layout::Position;

use crate::tui::combo_box::ComboBox;
use crate::tui::command::CommandLine;
use crate::tui::configuration::Configuration;
use crate::tui::find::FindWindow;
use crate::tui::help::Help;
use crate::tui::history::HistoryWindow;
use crate::tui::jobs::JobsList;
use crate::tui::jump::JumpMenu;
use crate::tui::key_hints::KeyHintsWindow;
use crate::tui::keymap::{Action, Resolved};
use crate::tui::menu::LocationMenu;
use crate::tui::mouse::{Pointer, Press, Scroll};
use crate::tui::pulldown::PullDown;
use crate::tui::tab_menu::TabMenu;
use crate::tui::workspaces::WorkspacesWindow;

use super::{App, Completes, Effect, Front, Scrolled, Side, Viewing, choices_for};

impl App {
    /// Takes a press of the mouse at a cell, as the last render drew the screen. What is in
    /// front takes it, as it takes keys.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Vec<Effect> {
        let effects = self.pointer_input(pointer);
        self.remember_searches();
        effects
    }

    fn pointer_input(&mut self, pointer: Pointer) -> Vec<Effect> {
        if !self.config.ui.mouse {
            return Vec::new();
        }
        if pointer.press == Press::Drag {
            return self.drag(pointer.at);
        }
        // Any other press lets go of the thumb, as the button that held it went up before.
        self.grab = None;
        let front = self.front();
        // A first click that closed a menu or a dialog, or opened one, leaves the second click
        // of a double click nothing to do.
        if pointer.press == Press::DoubleClick && self.clicked != Some(front) {
            return Vec::new();
        }
        let effects = self.route_pointer(pointer);
        self.clicked = (pointer.press == Press::Click && self.front() == front).then_some(front);
        self.settle_visual();
        effects
    }

    /// What is in front, in the order [`handle_over`](Self::handle_over) gives keys.
    pub(super) fn front(&self) -> Front {
        if self.key_hints.is_some() {
            Front::KeyHints
        } else if let Some(open) = self.dialogs.front() {
            Front::Dialog(open.dialog.id())
        } else if self.viewer_over() {
            if self.viewing.as_ref().is_some_and(Viewing::window_shown) {
                Front::Reading
            } else {
                Front::Viewer
            }
        } else if self.menu.is_some() {
            Front::Menu
        } else if self.tab_menu.is_some() {
            Front::TabMenu
        } else if self.jump.is_some() {
            Front::Jump
        } else if self.workspaces_window.is_some() {
            Front::Workspaces
        } else if self.history_window.is_some() {
            Front::History
        } else if !self.results.is_empty() {
            Front::Results(self.results.len())
        } else if self.in_front().is_some() {
            Front::Job
        } else if self.jobs_list.is_some() {
            Front::JobsList
        } else if self.help.is_some() {
            Front::Help
        } else if self.configuration.is_some() {
            Front::Configuration
        } else if self.pulldown.is_some() {
            Front::PullDown
        } else if self.viewing.as_ref().is_some_and(Viewing::window_shown) {
            Front::Reading
        } else if self.viewing.is_some() {
            Front::Viewer
        } else if self.find.is_some() {
            Front::Find
        } else {
            Front::Panels
        }
    }

    /// The F-key bar presses its keys, and the scroll bar of what is in front its parts; then
    /// what is in front takes the press: a dialog, the pull-down menu, the viewer, the menu
    /// bar that `ui.menu_bar` keeps, or the panels, where a click moves the cursor, a double
    /// click opens, a right click marks, and the wheel scrolls.
    fn route_pointer(&mut self, pointer: Pointer) -> Vec<Effect> {
        let Pointer { press, at } = pointer;
        if let Some(slot) = self.spots.fkeys.iter().position(|slot| slot.contains(at)) {
            // The second click of a double click would press the key again.
            let action = self.fkey_actions().into_iter().nth(slot).flatten();
            return match action {
                Some((action, _)) if press == Press::Click => self.handle(Resolved::Action(action)),
                _ => Vec::new(),
            };
        }
        if let Some(effects) = self.pointer_scroll_bar(pointer) {
            return effects;
        }
        match self.front() {
            Front::Dialog(_) => return self.pointer_dialog(pointer),
            Front::PullDown => return self.pointer_pulldown(pointer),
            Front::Viewer => return self.wheel_viewer(press),
            Front::Panels => {}
            _ => return self.pointer_window(pointer),
        }
        if self.command_line.is_some() {
            return self.pointer_command_line(pointer);
        }
        if self.panel(self.active).renaming() {
            return Vec::new();
        }
        // A click on a glyph of quick search switches how it matches, or whether case counts,
        // as the keys after Ctrl+s do.
        if let Some(action) = self.panel(self.active).search_action_at(at) {
            if press == Press::Click {
                return self.handle(Resolved::Action(action));
            }
            return Vec::new();
        }
        if let Some(index) = self
            .spots
            .menu_bar
            .iter()
            .position(|title| title.contains(at))
        {
            if press == Press::Click {
                self.open_pulldown(false);
                if let Some(mut pulldown) = self.pulldown.take() {
                    pulldown.open_menu(index, &|command| self.command_status(command));
                    self.pulldown = Some(pulldown);
                }
            }
            return Vec::new();
        }
        let Some(side) = self
            .spots
            .sides
            .iter()
            .find(|(_, area)| area.contains(at))
            .map(|(side, _)| *side)
        else {
            return Vec::new();
        };
        match press {
            Press::WheelUp | Press::WheelDown => {
                let step = self.config.ui.wheel;
                self.panel_mut(side).wheel(step, press == Press::WheelDown);
                Vec::new()
            }
            Press::Click | Press::DoubleClick | Press::RightClick => {
                self.click_panel(side, press, at)
            }
            Press::Drag => Vec::new(),
        }
    }

    /// The key that a step of the wheel stands for in windows and lists, and how many times:
    /// Up or Down by `ui.wheel` lines, or a page up or down.
    fn wheel_keys(&self, press: Press) -> Option<(Action, u8)> {
        let down = match press {
            Press::WheelUp => false,
            Press::WheelDown => true,
            Press::Click | Press::DoubleClick | Press::RightClick | Press::Drag => return None,
        };
        Some(match (self.config.ui.wheel, down) {
            (Wheel::Lines(count), false) => (Action::Up, count),
            (Wheel::Lines(count), true) => (Action::Down, count),
            (Wheel::Page, false) => (Action::PageUp, 1),
            (Wheel::Page, true) => (Action::PageDown, 1),
        })
    }

    /// Gives the keys of a step of the wheel to what is in front.
    fn wheel(&mut self, (action, count): (Action, u8)) -> Vec<Effect> {
        let mut effects = Vec::new();
        for _ in 0..count {
            effects.extend(self.handle(Resolved::Action(action)));
        }
        effects
    }

    /// Gives a press of the mouse to the window or menu in front. The wheel moves its cursor
    /// as Up and Down do, or scrolls the help; but not in a job's window or the viewer's
    /// reading, where they move between the buttons. A click does what the window says, often by the key it stands
    /// for.
    fn pointer_window(&mut self, pointer: Pointer) -> Vec<Effect> {
        let front = self.front();
        if let Some(keys) = self.wheel_keys(pointer.press) {
            let combo = self.open_combo_box().is_some();
            return match (front, &mut self.help) {
                (Front::Job | Front::Reading, _) => Vec::new(),
                // The wheel moves in the open list of a combo box, as in a menu.
                (Front::Find, _) if combo => self.wheel(keys),
                // The wheel scrolls the rows of the search, wherever the focus is.
                (Front::Find, _) => {
                    if let Some(window) = &mut self.find {
                        window.wheel(keys.0, usize::from(keys.1));
                    }
                    Vec::new()
                }
                // The wheel scrolls the help's text, wherever its keys go.
                (Front::Help, Some(help)) => {
                    let (action, count) = keys;
                    for _ in 0..count {
                        help.wheel(action);
                    }
                    Vec::new()
                }
                _ => self.wheel(keys),
            };
        }
        let action = match front {
            Front::KeyHints => self
                .key_hints
                .as_mut()
                .and_then(|window| window.pointer(pointer)),
            Front::Menu => self.menu.as_mut().and_then(|menu| menu.pointer(pointer)),
            Front::TabMenu => self
                .tab_menu
                .as_mut()
                .and_then(|menu| menu.pointer(pointer)),
            Front::Jump => self.jump.as_mut().and_then(|jump| jump.pointer(pointer)),
            Front::Workspaces => self
                .workspaces_window
                .as_mut()
                .and_then(|window| window.pointer(pointer)),
            Front::History => self
                .history_window
                .as_mut()
                .and_then(|window| window.pointer(pointer)),
            Front::Find => {
                let Some(window) = &mut self.find else {
                    return Vec::new();
                };
                let event = window.pointer(pointer);
                return self.find_event(event);
            }
            Front::Results(_) => self
                .results
                .front_mut()
                .and_then(|results| results.window.pointer(pointer)),
            Front::Job => self
                .jobs
                .iter_mut()
                .find(|job| !job.background)
                .and_then(|job| job.view.pointer(pointer)),
            Front::Reading => self
                .viewing
                .as_mut()
                .and_then(|viewing| viewing.reading.as_mut())
                .and_then(|reading| reading.pointer(pointer)),
            Front::JobsList => {
                let rows = self.job_rows();
                let list = self.jobs_list.as_mut();
                list.and_then(|list| list.pointer(pointer, &rows))
            }
            Front::Configuration => {
                let Some(configuration) = &mut self.configuration else {
                    return Vec::new();
                };
                let event = configuration.pointer(pointer);
                return self.configuration_event(event);
            }
            Front::Help => {
                if let Some(help) = &mut self.help {
                    help.pointer(pointer);
                }
                None
            }
            Front::Dialog(_) | Front::PullDown | Front::Viewer | Front::Panels => None,
        };
        action.map_or_else(Vec::new, |action| self.handle(Resolved::Action(action)))
    }

    /// Scrolls the viewer by a step of the wheel, if `press` is one, and reads the part it
    /// scrolls to.
    fn wheel_viewer(&mut self, press: Press) -> Vec<Effect> {
        let down = match press {
            Press::WheelUp => false,
            Press::WheelDown => true,
            Press::Click | Press::DoubleClick | Press::RightClick | Press::Drag => {
                return Vec::new();
            }
        };
        let (motion, count) = self.wheel_motion(down);
        let Some(viewing) = &mut self.viewing else {
            return Vec::new();
        };
        for _ in 0..count {
            viewing.viewer.handle(ViewerCommand::Scroll(motion));
        }
        self.read_more()
    }

    /// How far a step of the wheel up or down scrolls text, and how many times: by `ui.wheel`
    /// lines, or a page.
    fn wheel_motion(&self, down: bool) -> (Motion, u8) {
        match (self.config.ui.wheel, down) {
            (Wheel::Lines(count), false) => (Motion::Up, count),
            (Wheel::Lines(count), true) => (Motion::Down, count),
            (Wheel::Page, false) => (Motion::PageUp, 1),
            (Wheel::Page, true) => (Motion::PageDown, 1),
        }
    }

    /// The scroll bars of what is in front, as they were last drawn, with what each scrolls:
    /// the list of completions first, which shows over its field; the open list of a combo box
    /// alone, which has the mouse until it closes.
    pub(super) fn scroll_bars(&self) -> Vec<(Scrolled, scrollbar::Bar)> {
        if let Some(combo) = self.open_combo_box() {
            return combo
                .bar()
                .map(|bar| (Scrolled::ComboBox, bar))
                .into_iter()
                .collect();
        }
        let choices = |target: Completes| {
            let completing = self.completion.as_ref()?;
            let choices = completing.choices.as_ref()?;
            let bar = choices.bar().filter(|_| completing.target == target)?;
            Some((Scrolled::Choices, bar))
        };
        let one = |target: Scrolled, bar: Option<scrollbar::Bar>| bar.map(|bar| (target, bar));
        let bars = match self.front() {
            Front::KeyHints => {
                let bar = self.key_hints.as_ref().and_then(KeyHintsWindow::bar);
                vec![one(Scrolled::KeyHints, bar)]
            }
            Front::Dialog(_) => {
                let dialog = self.dialogs.front().and_then(|open| open.dialog.bar());
                vec![choices(Completes::Field), one(Scrolled::Dialog, dialog)]
            }
            Front::Menu => vec![one(
                Scrolled::Menu,
                self.menu.as_ref().and_then(LocationMenu::bar),
            )],
            Front::TabMenu => {
                let bar = self.tab_menu.as_ref().and_then(TabMenu::bar);
                vec![one(Scrolled::TabMenu, bar)]
            }
            Front::Jump => vec![one(
                Scrolled::Jump,
                self.jump.as_ref().and_then(JumpMenu::bar),
            )],
            Front::Workspaces => {
                let bar = self
                    .workspaces_window
                    .as_ref()
                    .and_then(WorkspacesWindow::bar);
                vec![one(Scrolled::Workspaces, bar)]
            }
            Front::History => {
                let bar = self.history_window.as_ref().and_then(HistoryWindow::bar);
                vec![one(Scrolled::History, bar)]
            }
            Front::Find => vec![one(
                Scrolled::Find,
                self.find.as_ref().and_then(FindWindow::bar),
            )],
            Front::Results(_) => {
                let bar = self
                    .results
                    .front()
                    .and_then(|results| results.window.bar());
                vec![one(Scrolled::Results, bar)]
            }
            Front::Job | Front::Reading => Vec::new(),
            Front::JobsList => {
                let bar = self.jobs_list.as_ref().and_then(JobsList::bar);
                vec![one(Scrolled::JobsList, bar)]
            }
            Front::Help => vec![one(Scrolled::Help, self.help.as_ref().and_then(Help::bar))],
            Front::Configuration => {
                let bar = self.configuration.as_ref().and_then(Configuration::bar);
                vec![one(Scrolled::Configuration, bar)]
            }
            Front::PullDown => {
                let bar = self.pulldown.as_ref().and_then(PullDown::bar);
                vec![one(Scrolled::PullDown, bar)]
            }
            Front::Viewer => {
                let bar = self
                    .viewing
                    .as_ref()
                    .and_then(|viewing| viewing.viewer.bar());
                vec![one(Scrolled::Viewer, bar)]
            }
            Front::Panels if self.command_line.is_some() => {
                let line = self.command_line.as_ref().and_then(CommandLine::bar);
                vec![
                    choices(Completes::CommandLine),
                    one(Scrolled::CommandLine, line),
                ]
            }
            Front::Panels if self.panel(self.active).renaming() => Vec::new(),
            Front::Panels => [Side::Left, Side::Right]
                .into_iter()
                .map(|side| one(Scrolled::Panel(side), self.panel(side).bar()))
                .collect(),
        };
        bars.into_iter().flatten().collect()
    }

    /// Scrolls `target` as a press on its scroll bar asks. The lists that the wheel moves by
    /// their keys take the keys of the arrows and the track too.
    fn scroll_target(&mut self, target: Scrolled, scroll: Scroll) -> Vec<Effect> {
        let keys = matches!(
            target,
            Scrolled::Menu
                | Scrolled::TabMenu
                | Scrolled::Jump
                | Scrolled::Workspaces
                | Scrolled::History
                | Scrolled::Find
                | Scrolled::KeyHints
                | Scrolled::Choices
                | Scrolled::ComboBox
        );
        if let (true, Scroll::By(motion)) = (keys, scroll) {
            let action = match motion {
                Motion::Up => Action::Up,
                Motion::Down => Action::Down,
                Motion::PageUp => Action::PageUp,
                Motion::PageDown => Action::PageDown,
                _ => return Vec::new(),
            };
            return self.handle(Resolved::Action(action));
        }
        if let (true, Scroll::To(top)) = (keys, scroll) {
            self.scroll_list_to(target, top);
            return Vec::new();
        }
        match target {
            Scrolled::Help => {
                if let Some(help) = &mut self.help {
                    help.scroll(scroll);
                }
            }
            Scrolled::Configuration => {
                if let Some(configuration) = &mut self.configuration {
                    let event = configuration.scroll(scroll);
                    return self.configuration_event(event);
                }
            }
            Scrolled::Viewer => {
                if let Some(viewing) = &mut self.viewing {
                    viewing.viewer.handle(match scroll {
                        Scroll::By(motion) => ViewerCommand::Scroll(motion),
                        Scroll::To(at) => ViewerCommand::Bar(at),
                    });
                    return self.read_more();
                }
            }
            Scrolled::Panel(side) => self.panel_mut(side).scroll_bar(scroll),
            Scrolled::PullDown => {
                if let Some(mut pulldown) = self.pulldown.take() {
                    pulldown.scroll(scroll, &|command| self.command_status(command));
                    self.pulldown = Some(pulldown);
                }
            }
            Scrolled::JobsList => {
                let rows = self.job_rows();
                if let Some(list) = &mut self.jobs_list {
                    list.scroll(scroll, &rows);
                }
            }
            Scrolled::Results => {
                if let Some(results) = self.results.front_mut() {
                    results.window.scroll(scroll);
                }
            }
            Scrolled::CommandLine => {
                if let Some(line) = &mut self.command_line {
                    line.scroll(scroll);
                }
            }
            Scrolled::Dialog => {
                if let Some(open) = self.dialogs.front_mut() {
                    open.dialog.scroll(scroll);
                }
            }
            Scrolled::Menu
            | Scrolled::TabMenu
            | Scrolled::Jump
            | Scrolled::Workspaces
            | Scrolled::History
            | Scrolled::Find
            | Scrolled::KeyHints
            | Scrolled::Choices
            | Scrolled::ComboBox => {}
        }
        Vec::new()
    }

    /// Shows the rows of the list `target` from `top`, where the thumb of its scroll bar was
    /// dragged.
    fn scroll_list_to(&mut self, target: Scrolled, top: usize) {
        match target {
            Scrolled::Menu => {
                if let Some(menu) = &mut self.menu {
                    menu.scroll_to(top);
                }
            }
            Scrolled::TabMenu => {
                if let Some(menu) = &mut self.tab_menu {
                    menu.scroll_to(top);
                }
            }
            Scrolled::Jump => {
                if let Some(jump) = &mut self.jump {
                    jump.scroll_to(top);
                }
            }
            Scrolled::Workspaces => {
                if let Some(window) = &mut self.workspaces_window {
                    window.scroll_to(top);
                }
            }
            Scrolled::History => {
                if let Some(window) = &mut self.history_window {
                    window.scroll_to(top);
                }
            }
            Scrolled::Find => {
                if let Some(window) = &mut self.find {
                    window.scroll_to(top);
                }
            }
            Scrolled::KeyHints => {
                if let Some(window) = &mut self.key_hints {
                    window.scroll_to(top);
                }
            }
            Scrolled::Choices => {
                let target = self.completion.as_ref().map(|completing| completing.target);
                if let Some(choices) =
                    target.and_then(|target| choices_for(&mut self.completion, target))
                {
                    choices.scroll_to(top);
                }
            }
            Scrolled::ComboBox => {
                if let Some(combo) = self.open_combo_box_mut() {
                    combo.scroll_to(top);
                }
            }
            _ => {}
        }
    }

    /// The combo box whose list is open in what is in front: a dialog, the search of a tree,
    /// or the Configuration dialog.
    fn open_combo_box(&self) -> Option<&ComboBox> {
        match self.front() {
            Front::Dialog(_) => self.dialogs.front()?.dialog.open_combo_box(),
            Front::Find => self.find.as_ref()?.open_combo_box(),
            Front::Configuration => self.configuration.as_ref()?.open_combo_box(),
            _ => None,
        }
    }

    fn open_combo_box_mut(&mut self) -> Option<&mut ComboBox> {
        match self.front() {
            Front::Dialog(_) => self.dialogs.front_mut()?.dialog.open_combo_box_mut(),
            Front::Find => self.find.as_mut()?.open_combo_box_mut(),
            Front::Configuration => self.configuration.as_mut()?.open_combo_box_mut(),
            _ => None,
        }
    }

    /// Takes a click on a scroll bar of what is in front, if it is on one: an arrow scrolls as
    /// a step of the wheel, the track a page towards the click, as in Far, and the thumb is
    /// held for dragging. The second click of a double click scrolls again.
    fn pointer_scroll_bar(&mut self, Pointer { press, at }: Pointer) -> Option<Vec<Effect>> {
        if !matches!(press, Press::Click | Press::DoubleClick) {
            return None;
        }
        let (target, bar) = self
            .scroll_bars()
            .into_iter()
            .find(|(_, bar)| bar.area().contains(at))?;
        let (motion, count) = match bar.part(at)? {
            Part::Up => self.wheel_motion(false),
            Part::Down => self.wheel_motion(true),
            Part::PageUp => (Motion::PageUp, 1),
            Part::PageDown => (Motion::PageDown, 1),
            Part::Thumb(row) => {
                self.grab = Some((target, row));
                return Some(Vec::new());
            }
        };
        let mut effects = Vec::new();
        for _ in 0..count {
            effects.extend(self.scroll_target(target, Scroll::By(motion)));
        }
        Some(effects)
    }

    /// Whether the left button holds the thumb of a scroll bar, which its drags move.
    pub(crate) fn dragging(&self) -> bool {
        self.grab.is_some()
    }

    /// Scrolls what holds the thumb to where it was dragged, at the row of `at`; nothing if no
    /// thumb is held, or its bar has gone.
    fn drag(&mut self, at: Position) -> Vec<Effect> {
        let Some((target, row)) = self.grab else {
            return Vec::new();
        };
        let bar = self
            .scroll_bars()
            .into_iter()
            .find_map(|(scrolled, bar)| (scrolled == target).then_some(bar));
        let Some(bar) = bar else {
            self.grab = None;
            return Vec::new();
        };
        self.scroll_target(target, Scroll::To(bar.dragged(row, at.y)))
    }

    /// Gives a press of the mouse to the dialog in front, and does what it was for if it
    /// closes; or to the list of completions under its field, which goes at a press outside
    /// it. The wheel scrolls a message that does not fit, and moves in the open list of a combo
    /// box as Up and Down do.
    fn pointer_dialog(&mut self, pointer: Pointer) -> Vec<Effect> {
        if let Some(effects) = self.pointer_choices(Completes::Field, pointer) {
            return effects;
        }
        self.completion = None;
        if let Some(keys) = self.wheel_keys(pointer.press)
            && self.open_combo_box().is_some()
        {
            return self.wheel(keys);
        }
        let down = match pointer.press {
            Press::WheelUp => Some(false),
            Press::WheelDown => Some(true),
            Press::Click | Press::DoubleClick | Press::RightClick | Press::Drag => None,
        };
        let (motion, count) = self.wheel_motion(down == Some(true));
        let Some(open) = self.dialogs.front_mut() else {
            return Vec::new();
        };
        if down.is_some() && open.dialog.bar().is_some() {
            for _ in 0..count {
                open.dialog.scroll(Scroll::By(motion));
            }
            return Vec::new();
        }
        let event = open.dialog.pointer(pointer);
        self.dialog_event(event)
    }

    /// Gives a press of the mouse to the list of completions over the command line, which goes
    /// at a press outside it; the line itself takes none.
    fn pointer_command_line(&mut self, pointer: Pointer) -> Vec<Effect> {
        if let Some(effects) = self.pointer_choices(Completes::CommandLine, pointer) {
            return effects;
        }
        if choices_for(&mut self.completion, Completes::CommandLine).is_some() {
            self.completion = None;
        }
        Vec::new()
    }

    /// Gives a press of the mouse to the list of completions for `target`, if it shows and the
    /// press is on it: the wheel moves in it as Up and Down do, a click puts a match in, and a
    /// double click keeps it, as Ctrl+y. `None` for a press elsewhere.
    fn pointer_choices(&mut self, target: Completes, pointer: Pointer) -> Option<Vec<Effect>> {
        let wheel = self.wheel_keys(pointer.press);
        let choices = choices_for(&mut self.completion, target)?;
        if !choices.contains(pointer.at) {
            return None;
        }
        if let Some(keys) = wheel {
            return Some(self.wheel(keys));
        }
        if let Some(event) = choices.pointer(pointer) {
            self.take_choices(event);
        }
        Some(Vec::new())
    }

    /// Gives a press of the mouse to the pull-down menu; a command closes it, then runs.
    fn pointer_pulldown(&mut self, pointer: Pointer) -> Vec<Effect> {
        let Some(mut pulldown) = self.pulldown.take() else {
            return Vec::new();
        };
        let event = pulldown.pointer(pointer, &|command| self.command_status(command));
        self.pulldown_event(pulldown, event)
    }

    /// A click on the panel on `side`, at `at`: on a tab, it shows that tab; on a row, it puts
    /// the cursor there, and opens the row on a double click or marks it on a right click.
    /// Either way the side gets the keys, and quick search ends, as at a key of its own.
    fn click_panel(&mut self, side: Side, press: Press, at: Position) -> Vec<Effect> {
        self.end_search(self.active);
        self.active = side;
        let tab = self
            .spots
            .tabs
            .iter()
            .find(|(tab_side, _, area)| *tab_side == side && area.contains(at))
            .map(|(_, index, _)| *index);
        if let Some(index) = tab {
            if press == Press::Click && self.tabs_mut(side).select(index) {
                return self.revealed(side);
            }
            return Vec::new();
        }
        let panel = self.panel_mut(side);
        match press {
            Press::RightClick => panel.mark_at(at),
            Press::DoubleClick if panel.click(at) => {
                return self.handle(Resolved::Action(Action::Enter));
            }
            _ => {
                panel.click(at);
            }
        }
        Vec::new()
    }
}
