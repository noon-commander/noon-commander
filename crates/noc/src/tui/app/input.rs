//! Giving an action to what has the keys.

use crate::tui::find::{FindMode, FindWindow};
use crate::tui::history::HistoryWindow;
use crate::tui::jobs::JobsList;
use crate::tui::jump::JumpMenu;
use crate::tui::key_hints::KeyHintsWindow;
use crate::tui::keymap::{Action, Context, Resolved};
use crate::tui::menu::LocationMenu;
use crate::tui::search::{Input, Slot};
use crate::tui::tab_menu::TabMenu;
use crate::tui::transfer::{self};
use crate::tui::workspaces::WorkspacesWindow;

use super::{App, Effect, Handover, Purpose, Whole};

impl App {
    /// Gives a key to what is over the panels, front first, if anything is.
    fn handle_over(&mut self, input: Resolved) -> Option<Vec<Effect>> {
        if self.key_hints.is_some() {
            return Some(self.handle_key_hints(input));
        }
        if !self.dialogs.is_empty() {
            return Some(self.handle_dialog(input));
        }
        if self.viewer_over() {
            return Some(self.handle_viewer(input));
        }
        if self.menu.is_some() {
            return Some(self.handle_menu(input));
        }
        if self.tab_menu.is_some() {
            return Some(self.handle_tab_menu(input));
        }
        if self.jump.is_some() {
            return Some(self.handle_jump(input));
        }
        if self.workspaces_window.is_some() {
            return Some(self.handle_workspaces(input));
        }
        if self.history_window.is_some() {
            return Some(self.handle_history(input));
        }
        if let Some(effects) = self.handle_results(input) {
            return Some(effects);
        }
        if self.handle_job(input) || self.handle_jobs_list(input) {
            return Some(Vec::new());
        }
        if let Some(help) = &mut self.help {
            if help.handle(input) {
                self.help_spot = help.spot();
                self.help = None;
            }
            return Some(Vec::new());
        }
        if self.configuration.is_some() {
            return Some(self.handle_configuration(input));
        }
        if self.pulldown.is_some() {
            return Some(self.handle_pulldown(input));
        }
        if self.viewing.is_some() {
            return Some(self.handle_viewer(input));
        }
        if self.find.is_some() {
            return Some(self.handle_find(input));
        }
        None
    }

    /// Takes what a key, or keys, resolved to; the fields that search then keep how they match.
    pub(crate) fn handle(&mut self, input: Resolved) -> Vec<Effect> {
        let effects = self.handle_input(input);
        self.remember_searches();
        effects
    }

    /// How the field of `slot` opens: as it was last, else in `ui.default_search_mode`.
    pub(super) fn search_opens(&self, slot: Slot) -> Input {
        self.searches
            .input(slot, self.config.ui.default_search_mode)
    }

    /// Keeps how each field that searches and is open matches now, and whether case counts,
    /// which it opens with again (ADR 0047): after each key or press, so that a field keeps
    /// what was switched in it however it closes.
    pub(super) fn remember_searches(&mut self) {
        for slot in Slot::ALL {
            let input = match slot {
                Slot::QuickSearch => self.panel(self.active).search_input(),
                Slot::LocationMenu => self.menu.as_ref().map(LocationMenu::input),
                Slot::TabMenu => self.tab_menu.as_ref().map(TabMenu::input),
                Slot::Workspaces => self.workspaces_window.as_ref().map(WorkspacesWindow::input),
                Slot::CommandHistory => self.history_window.as_ref().map(HistoryWindow::input),
                Slot::KeyHints => self.key_hints.as_ref().map(KeyHintsWindow::input),
                Slot::Jump => self.jump.as_ref().map(JumpMenu::input),
                Slot::FindText => self.find.as_ref().map(FindWindow::text_input),
                Slot::FindFiles => self.find.as_ref().map(FindWindow::files_input),
                Slot::Checksums => self
                    .results
                    .front()
                    .and_then(|results| results.window.input()),
                Slot::Mark => self
                    .dialogs
                    .iter()
                    .find(|open| matches!(open.purpose, Purpose::Pattern { .. }))
                    .and_then(|open| open.dialog.search(0)),
            };
            if let Some(input) = input {
                self.searches.keep(slot, input);
            }
        }
    }

    fn handle_input(&mut self, input: Resolved) -> Vec<Effect> {
        self.sync_connected();
        if let Some(effects) = self.handle_everywhere(input) {
            return effects;
        }
        if let Some(effects) = self.handle_over(input) {
            return effects;
        }
        if self.command_line.is_some() {
            return self.handle_command_line(input);
        }
        if self.panel(self.active).renaming() {
            return self.handle_rename(input);
        }
        let Some(action) = self.search_key(input) else {
            return Vec::new();
        };
        if self.ask_connect(self.active, action) {
            return Vec::new();
        }
        if let Some(effects) = self.handle_vim(action) {
            return effects;
        }
        match action {
            Action::QuickSearch => self.quick_search(),
            Action::QuickSearchBack => {
                let (count, input) = (self.extra.times(), self.search_opens(Slot::QuickSearch));
                self.panel_mut(self.active).search_up(count, input);
            }
            Action::NextMatch | Action::PrevMatch => self.search_again(action == Action::NextMatch),
            Action::Shell | Action::Command | Action::CommandHistory => {
                self.open_command_line(action);
            }
            Action::Quit => self.ask_quit(),
            Action::Jobs => self.jobs_list = Some(JobsList::default()),
            Action::Help => self.help = Some(self.help_screen()),
            Action::SwitchPanel => self.active = self.active.other(),
            Action::FocusLeft | Action::FocusRight => {
                self.active = self.drawn_on(action == Action::FocusLeft);
            }
            _ if action.of_tabs() => return self.tab_action(self.active, action),
            Action::TabList => self.open_tab_menu(self.active),
            // The active panel moves to the other side and stays active, as in mc.
            Action::SwapPanels => self.swapped = !self.swapped,
            Action::OtherPanelOpen => {
                if let Some(destination) = self.panel_mut(self.active).for_other_panel() {
                    return self.go(self.shown(self.active.other()), destination);
                }
            }
            Action::OtherPanelSync => {
                let here = self.panel(self.active).here();
                return self.go(self.shown(self.active.other()), here);
            }
            // As in mc, for both panels, and in every tab.
            Action::ToggleHidden => {
                self.config.ui.show_hidden = !self.config.ui.show_hidden;
                let show = self.config.ui.show_hidden;
                for panel in self.panels_mut() {
                    panel.set_show_hidden(show);
                }
            }
            Action::Mkdir => self.ask_mkdir(),
            Action::Checksum => self.ask_checksum(),
            Action::Delete => self.ask_delete(),
            Action::View => return self.view(),
            Action::Edit => return self.edit(),
            Action::SystemOpen => return self.system_open(),
            Action::Copy => self.ask_transfer(transfer::Kind::Copy),
            Action::Duplicate => self.ask_duplicate(),
            Action::Move => self.ask_transfer(transfer::Kind::Move),
            Action::Rename => self.start_rename(),
            Action::Select | Action::Unselect => self.ask_pattern(action == Action::Select),
            // Esc closes the whole name below the panels when it waits for no listing.
            Action::Cancel if self.panel(self.active).pending_request().is_none() => {
                self.whole = Whole::Cut;
            }
            Action::Cancel => self.cancel(self.active),
            Action::Disconnect => return self.disconnect_chosen(),
            Action::EditHost => {
                if let Some(host) = self.chosen_host() {
                    self.ask_edit_host(host);
                }
            }
            Action::LocationMenu => return self.open_menu(self.active),
            Action::Jump => return self.open_jump(),
            Action::FindFile => return self.open_find(FindMode::Text),
            Action::FindNames => return self.open_find(FindMode::Names),
            Action::GoToDir => self.ask_go_to(),
            Action::SaveWorkspace => self.ask_save_workspace(true),
            Action::Workspaces => self.open_workspaces(),
            Action::PullDown => self.open_pulldown(false),
            Action::PullDownLast => self.open_pulldown(true),
            _ => {
                let id = self.shown(self.active);
                if let Some(request) = self.panel_mut(id.side).handle(action) {
                    return self.open(id, request);
                }
            }
        }
        Vec::new()
    }

    /// Does what vim's keys do in a panel that mc's have no likes of: Visual mode, the jump
    /// list, and the commands that take a count as a number; `None` for the other keys.
    fn handle_vim(&mut self, action: Action) -> Option<Vec<Effect>> {
        if self.panel(self.active).visual()
            && let Some(effects) = self.handle_visual(action)
        {
            return Some(effects);
        }
        let count = self.extra.count;
        let id = self.shown(self.active);
        match action {
            Action::VisualMode | Action::VisualLineMode => {
                let line = action == Action::VisualLineMode;
                self.panel_mut(id.side).start_visual(count, line);
            }
            Action::Reselect => self.panel_mut(id.side).reselect(),
            Action::JumpBack | Action::JumpForward => {
                let times = count.unwrap_or(1);
                let panel = self.panel_mut(id.side);
                let request = if action == Action::JumpBack {
                    panel.jump_back(times)
                } else {
                    panel.jump_forward(times)
                };
                if let Some(request) = request {
                    return Some(self.open(id, request));
                }
            }
            // With a count, the row of that number, as vim's `G` and `g g` go to the line.
            Action::Home | Action::End if count.is_some() => {
                self.panel_mut(id.side).go_to_row(count.unwrap_or(1));
            }
            Action::PageUp | Action::PageDown if count.is_some() => {
                let down = action == Action::PageDown;
                self.panel_mut(id.side).page_by(down, count.unwrap_or(1));
            }
            Action::Mark | Action::MarkUp if count.is_some() => {
                let down = action == Action::Mark;
                self.panel_mut(id.side).mark_rows(down, count.unwrap_or(1));
            }
            // With a count, the tab of that number, as vim's `g t`.
            Action::NextTab if count.is_some() => {
                self.end_search(id.side);
                if self.tabs_mut(id.side).select(count.unwrap_or(1) - 1) {
                    return Some(self.revealed(id.side));
                }
            }
            _ => return None,
        }
        Some(Vec::new())
    }

    /// Does what the keys do wherever they go, before what is in front takes them; `None` for
    /// the other keys.
    fn handle_everywhere(&mut self, input: Resolved) -> Option<Vec<Effect>> {
        let Resolved::Action(action) = input else {
            return None;
        };
        match action {
            // The hints show their keys over what is in front, which stays open; again while
            // they show, they take the keys, as a list.
            Action::KeyHints => {
                let place = self.key_place();
                match self.hints.take() {
                    Some(hints) if hints.place == place => {
                        let input = self.search_opens(Slot::KeyHints);
                        let window = KeyHintsWindow::new(&self.keymap, &hints, self.extra, input);
                        self.key_hints = Some(window);
                    }
                    _ => self.hints_asked = Some(place),
                }
            }
            // The panels alone: the lists always show their row under the cursor whole.
            Action::ShowFull if self.context() == Context::Panel => {
                self.whole = self.whole.toggled();
            }
            // So does the output of commands, and what is in front is there again afterwards;
            // as a command of its own, it gives up an operator that waits, and is the one
            // command after Ctrl+o.
            Action::UserScreen => {
                self.give_up_command();
                self.handover = Some(Handover::UserScreen);
            }
            // A redraw leaves what is in front as it was too; quick search goes on after it.
            Action::Redraw => {
                self.give_up_command();
                self.redraw = true;
            }
            // As in a text field, Ctrl+r types the text of a register in the filter of a list.
            Action::PasteRegister if self.filtering() => {
                let text = self.registers.text(self.extra.char).unwrap_or_default();
                let text: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
                return Some(
                    text.into_iter()
                        .flat_map(|c| self.handle(Resolved::Insert(c)))
                        .collect(),
                );
            }
            _ => return None,
        }
        Some(Vec::new())
    }

    /// Takes a key while a panel selects rows in Visual mode: the motions extend the
    /// selection, Space marks it, and the commands of files act on what it selects, which they
    /// find marked. The keys to another panel or tab keep it; any other command, such as an
    /// F-key or one of the pull-down menu, ends Visual mode and runs as in Normal mode: `None`
    /// then.
    fn handle_visual(&mut self, action: Action) -> Option<Vec<Effect>> {
        let id = self.shown(self.active);
        let count = self.extra.count;
        let panel = self.panel_mut(id.side);
        match action {
            Action::NormalMode => panel.end_visual(),
            Action::VisualMode | Action::VisualLineMode => {
                panel.switch_visual(action == Action::VisualLineMode);
            }
            Action::OtherEnd => panel.other_end(),
            Action::Mark => panel.mark_visual(false),
            Action::Copy
            | Action::Move
            | Action::Delete
            | Action::Checksum
            | Action::SystemOpen => {
                panel.mark_visual(true);
                return Some(self.handle(Resolved::Action(action)));
            }
            Action::Home | Action::End if count.is_some() => {
                panel.go_to_row(count.unwrap_or(1));
            }
            Action::PageUp | Action::PageDown if count.is_some() => {
                panel.page_by(action == Action::PageDown, count.unwrap_or(1));
            }
            Action::Up
            | Action::Down
            | Action::PageUp
            | Action::PageDown
            | Action::HalfPageUp
            | Action::HalfPageDown
            | Action::Home
            | Action::End => {
                if let Some(request) = panel.handle(action) {
                    return Some(self.open(id, request));
                }
            }
            // The panel or the tab left keeps its Visual mode, and has it again with the keys
            // (ADR 0066).
            Action::SwitchPanel
            | Action::FocusLeft
            | Action::FocusRight
            | Action::GoToTab
            | Action::LastTab => return None,
            _ => {
                panel.end_visual();
                return None;
            }
        }
        Some(Vec::new())
    }
}
