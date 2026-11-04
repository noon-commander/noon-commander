//! Where keys go: the mode, the context, counts, and what an action works on.

use std::time::Instant;

use crate::tui::command::CommandLine;
use crate::tui::dialog::Field;
use crate::tui::edit::Editor;
use crate::tui::find::FindWindow;
use crate::tui::history::HistoryWindow;
use crate::tui::jump::JumpMenu;
use crate::tui::key_hints::KeyHintsWindow;
use crate::tui::keymap::{self, Action, Context, Extra, KeyState, Mode, Resolved, Typed};
use crate::tui::menu::LocationMenu;
use crate::tui::search::{self};
use crate::tui::tab_menu::TabMenu;
use crate::tui::workspaces::WorkspacesWindow;
use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Completes, Effect, Front, MAX_REPEAT, Side, Viewing};

impl App {
    /// Where keys go now.
    pub(crate) fn context(&self) -> Context {
        if self.key_hints.is_some() {
            Context::KeyHints
        } else if let Some(open) = self.dialogs.front() {
            if self.lists(Completes::Field) && open.dialog.completes() {
                Context::Completion
            } else {
                open.dialog.context()
            }
        } else if self.viewer_over() {
            if self.viewing.as_ref().is_some_and(Viewing::window_shown) {
                Context::Dialog
            } else {
                Context::Viewer
            }
        } else if self.menu.is_some() {
            Context::LocationMenu
        } else if self.tab_menu.is_some() {
            Context::TabMenu
        } else if self.jump.is_some() {
            Context::Jump
        } else if self.workspaces_window.is_some() {
            Context::Workspaces
        } else if self.history_window.is_some() {
            Context::CommandHistory
        } else if let Some(results) = self.results.front() {
            results.window.context()
        } else if self.in_front().is_some() || self.jobs_list.is_some() {
            Context::Dialog
        } else if self.help.is_some() {
            Context::Help
        } else if let Some(configuration) = &self.configuration {
            configuration.context()
        } else if self.pulldown.is_some() {
            Context::PullDown
        } else if self.viewing.as_ref().is_some_and(Viewing::window_shown) {
            Context::Dialog
        } else if self.viewing.is_some() {
            Context::Viewer
        } else if let Some(window) = &self.find {
            window.context()
        } else if self.command_line.is_some() {
            if self.lists(Completes::CommandLine) {
                Context::CommandCompletion
            } else {
                Context::CommandLine
            }
        } else if self.panel(self.active).renaming() {
            Context::Rename
        } else if self.panel(self.active).searching() {
            Context::QuickSearch
        } else {
            Context::Panel
        }
    }

    /// Whether the list of completions for `target` shows.
    pub(super) fn lists(&self, target: Completes) -> bool {
        self.completion
            .as_ref()
            .is_some_and(|completing| completing.target == target && completing.choices.is_some())
    }

    /// The mode keys are typed in now, as vim's: that of the text field that has them, Visual
    /// mode while a panel selects rows, and else Normal mode.
    pub(crate) fn mode(&self) -> Mode {
        if let Some(window) = &self.key_hints {
            window.mode()
        } else if let Some(open) = self.dialogs.front() {
            if self.context() == Context::Completion {
                Mode::Insert
            } else {
                open.dialog.mode()
            }
        } else if self.viewer_over() {
            Mode::Normal
        } else if let Some(menu) = &self.menu {
            menu.mode()
        } else if let Some(menu) = &self.tab_menu {
            menu.mode()
        } else if let Some(jump) = &self.jump {
            jump.mode()
        } else if let Some(window) = &self.workspaces_window {
            window.mode()
        } else if let Some(window) = &self.history_window {
            window.mode()
        } else if let Some(results) = self.results.front() {
            results.window.mode()
        } else if self.in_front().is_some() || self.jobs_list.is_some() || self.help.is_some() {
            Mode::Normal
        } else if let Some(configuration) = &self.configuration {
            configuration.mode()
        } else if self.pulldown.is_some() || self.viewing.is_some() {
            Mode::Normal
        } else if let Some(window) = &self.find {
            window.mode()
        } else if let Some(line) = &self.command_line {
            line.mode()
        } else {
            let panel = self.panel(self.active);
            if let Some(field) = panel.rename_field_ref() {
                field.mode()
            } else if panel.searching() {
                Mode::Insert
            } else if panel.visual() {
                Mode::Visual
            } else {
                Mode::Normal
            }
        }
    }

    /// The mode and the context of the next key.
    pub(crate) fn place(&self) -> (Mode, Context) {
        (self.mode(), self.context())
    }

    /// Where the next key goes, for the keymap: the mode, the context, and the kind of the field
    /// that has the keys if it searches (ADR 0045, ADR 0046).
    pub(crate) fn key_place(&self) -> keymap::Place {
        let (mode, context) = self.place();
        keymap::Place {
            mode,
            context,
            search: self.search_focused(),
        }
    }

    /// The kind of the field that has the keys, if it searches: quick search, the filter of a
    /// list while it takes text, or such a field of a dialog.
    fn search_focused(&self) -> Option<search::Kind> {
        match self.context() {
            Context::QuickSearch => self.panel(self.active).search_kind(),
            Context::KeyHints => self.key_hints.as_ref()?.search(),
            Context::LocationMenu => self.menu.as_ref()?.search(),
            Context::TabMenu => self.tab_menu.as_ref()?.search(),
            Context::Jump => self.jump.as_ref()?.search(),
            Context::Workspaces => self.workspaces_window.as_ref()?.search(),
            Context::CommandHistory => self.history_window.as_ref()?.search(),
            Context::Checksums => self.results.front()?.window.search(),
            Context::DialogInput | Context::PathInput => {
                self.dialogs.front()?.dialog.search_focused()
            }
            Context::FindInput => self.find.as_ref()?.search_focused(),
            _ => None,
        }
    }

    /// The operator that a text field with the keys has waiting for its motion, as `showcmd`
    /// shows it.
    /// The editor of the text field that has the keys, if one does.
    pub(super) fn focused_editor(&self) -> Option<&Editor> {
        let field = match self.place().1 {
            Context::DialogInput | Context::PathInput | Context::Completion => {
                match (self.dialogs.front(), &self.configuration) {
                    (Some(open), _) => open.dialog.focused_field(),
                    (None, Some(configuration)) => configuration.field(),
                    (None, None) => None,
                }
            }
            Context::Rename => self.panel(self.active).rename_field_ref(),
            Context::FindInput => self.find.as_ref().and_then(FindWindow::focused_field),
            Context::CommandLine | Context::CommandCompletion => {
                return self.command_line.as_ref().map(CommandLine::editor);
            }
            _ => None,
        };
        field.map(Field::editor)
    }

    pub(super) fn pending_operator(&self) -> Option<String> {
        if self.key_hints.is_some() {
            return None;
        }
        if let Some(open) = self.dialogs.front() {
            return open.dialog.pending();
        }
        if let Some(configuration) = &self.configuration {
            return configuration.pending();
        }
        if self.front() == Front::Find {
            return self.find.as_ref().and_then(FindWindow::pending);
        }
        if let Some(line) = &self.command_line {
            return line.pending();
        }
        self.panel(self.active)
            .rename_field_ref()
            .and_then(Field::pending)
    }

    /// Takes a key press as the event loop gets it: to the keymap, in the mode and the context
    /// of the app, and what it resolves to, to the app. Alt and a key that stand for Esc and the
    /// key go as both, each in the mode and the context it finds, as Esc may leave Insert mode;
    /// on a password they do nothing, as the Esc would close its dialog and send the key to the
    /// panels.
    pub(crate) fn key(&mut self, keys: &mut KeyState, key: KeyEvent, now: Instant) -> Vec<Effect> {
        self.message = None;
        let split = if self.typing_secret() {
            None
        } else {
            self.keymap.alt_as_esc(keys, self.key_place(), key)
        };
        let (first, then) = match split {
            Some(plain) => (KeyEvent::from(KeyCode::Esc), Some(plain)),
            None => (key, None),
        };
        let mut effects = self.feed(keys, first, now);
        if let Some(plain) = then {
            effects.extend(self.feed(keys, plain, now));
        }
        effects
    }

    /// Settles a key sequence of `keys` whose time is up, as [`key`](Self::key) takes a key.
    pub(crate) fn expire_keys(&mut self, keys: &mut KeyState, now: Instant) -> Vec<Effect> {
        let typed = self.keymap.expire(keys, now);
        self.take_typed(keys, typed)
    }

    fn feed(&mut self, keys: &mut KeyState, key: KeyEvent, now: Instant) -> Vec<Effect> {
        keys.set_operator(self.pending_operator().is_some());
        let typed = self.keymap.feed(keys, self.key_place(), key, now);
        self.take_typed(keys, typed)
    }

    /// Takes what keys resolved to, after giving up the part of a command that the keymap gave
    /// up; then lets Visual mode go in the panels that no longer have the keys.
    fn take_typed(&mut self, keys: &mut KeyState, typed: Vec<Typed>) -> Vec<Effect> {
        if keys.take_dropped() {
            self.give_up_command();
        }
        let mut effects = Vec::new();
        for typed in typed {
            effects.extend(self.typed(typed));
        }
        self.settle_visual();
        effects
    }

    /// Whether keys go to the filter of a list, in Insert mode.
    pub(super) fn filtering(&self) -> bool {
        matches!(
            self.place(),
            (
                Mode::Insert,
                Context::LocationMenu
                    | Context::TabMenu
                    | Context::Jump
                    | Context::Workspaces
                    | Context::CommandHistory
                    | Context::KeyHints
                    | Context::Checksums
            )
        )
    }

    /// Whether keys go to a secret field, such as the password of a prompt from ssh.
    fn typing_secret(&self) -> bool {
        self.key_hints.is_none()
            && self
                .dialogs
                .front()
                .is_some_and(|open| open.dialog.secret_focused())
    }

    /// Gives up the part of a command that a text field has waiting, such as an operator, when
    /// the keymap gives up the command: after Esc, or a key that cannot go on with it.
    pub(super) fn give_up_command(&mut self) {
        // The key hints with the keys have no command of their own to give up; what waits under
        // them, such as an operator, waits for the command run from them.
        if self.key_hints.is_some() {
            return;
        }
        if let Some(open) = self.dialogs.front_mut() {
            open.dialog.give_up();
        }
        if let Some(configuration) = &mut self.configuration {
            configuration.give_up();
        }
        if let Some(window) = &mut self.find {
            window.give_up();
        }
        if let Some(line) = &mut self.command_line {
            line.give_up();
        }
        if let Some(field) = self.panel_mut(self.active).rename_field() {
            field.give_up();
        }
    }

    /// Ends Visual mode in the panels that do not have the keys: the other side, and the tabs
    /// that do not show.
    pub(super) fn settle_visual(&mut self) {
        let active = self.active;
        for side in [Side::Left, Side::Right] {
            let shown = self.tabs(side).index();
            for (index, tab) in self.tabs_mut(side).iter_mut().enumerate() {
                if side != active || index != shown {
                    tab.panel.end_visual();
                }
            }
        }
    }

    /// Takes keys resolved with what came with them: a count repeats a motion where keys
    /// move a cursor over rows; a text field, the commands that take a count as a number,
    /// such as `G`, and those that take a register or a character read them themselves. After
    /// Ctrl+x, the other panel has the keys for the command, and gives them back once it ends;
    /// the list of the keys after Ctrl+x has them already for the command run from it.
    pub(crate) fn typed(&mut self, typed: Typed) -> Vec<Effect> {
        let Typed { resolved, extra } = typed;
        if extra.other && self.returns.is_none() {
            self.returns = Some(self.active);
            self.active = self.active.other();
        }
        let times = match resolved {
            Resolved::Action(action) if self.repeats(action) => extra.times().min(MAX_REPEAT),
            Resolved::Action(Action::PageUp | Action::PageDown) => {
                if let Some(count) = extra.count {
                    self.count_pages(count);
                }
                1
            }
            _ => 1,
        };
        self.extra = extra;
        let place = self.place();
        let mut effects = Vec::new();
        for _ in 0..times {
            effects.extend(self.handle(resolved));
            // What the key left, such as Visual mode after Space, takes the rest no more.
            if self.place() != place {
                break;
            }
        }
        self.extra = Extra::default();
        self.come_back();
        effects
    }

    /// Gives the keys back to the panel that Ctrl+x took them from, once the command it ran in
    /// the other panel ends: when that panel has the keys in Normal mode again, with nothing in
    /// front of it and no program to hand the terminal to, as vim goes back to Insert mode
    /// after the command of Ctrl+o.
    pub(crate) fn come_back(&mut self) {
        if self.handover.is_none()
            && self.place() == (Mode::Normal, Context::Panel)
            && let Some(side) = self.returns.take()
        {
            self.active = side;
        }
    }

    /// Whether a count repeats `action` where keys go now: a motion over rows, the previous
    /// tab, or `d d` on a row of a list, outside the text fields, whose editor reads the count. `n` and `N` take it
    /// themselves, as one jump, and marks, which stop at the end of the listing.
    fn repeats(&self, action: Action) -> bool {
        if self.counts_moves() {
            return false;
        }
        // As vim's `3dd`, which deletes three lines: rows of the lists that `d d` removes.
        let removes = matches!(
            (self.context(), action),
            (Context::TabMenu, Action::CloseTab)
                | (Context::LocationMenu, Action::Disconnect)
                | (
                    Context::Workspaces | Context::CommandHistory,
                    Action::Delete
                )
        );
        if removes {
            return true;
        }
        let text = matches!(
            self.context(),
            Context::Rename
                | Context::DialogInput
                | Context::PathInput
                | Context::Completion
                | Context::CommandLine
                | Context::CommandCompletion
                | Context::QuickSearch
        );
        // A count before Ctrl+f and Ctrl+b scrolls that many pages at once in the panels and
        // the lists, as in vim, rather than one at a time.
        let pages = matches!(
            self.context(),
            Context::Panel
                | Context::LocationMenu
                | Context::TabMenu
                | Context::Workspaces
                | Context::CommandHistory
                | Context::KeyHints
                | Context::Jump
        );
        if pages && matches!(action, Action::PageUp | Action::PageDown) {
            return false;
        }
        !text
            && matches!(
                action,
                Action::Up
                    | Action::Down
                    | Action::Left
                    | Action::Right
                    | Action::ScrollLeft
                    | Action::ScrollRight
                    | Action::ScrollHalfLeft
                    | Action::ScrollHalfRight
                    | Action::PageUp
                    | Action::PageDown
                    | Action::HalfPageUp
                    | Action::HalfPageDown
                    | Action::PrevTab
            )
    }

    /// Gives the count before Ctrl+f or Ctrl+b to the list in front, which scrolls that many
    /// pages at once, as vim's do. The panels read it themselves.
    fn count_pages(&mut self, pages: usize) {
        let picker = match self.context() {
            Context::LocationMenu => self.menu.as_mut().map(LocationMenu::picker_mut),
            Context::TabMenu => self.tab_menu.as_mut().map(TabMenu::picker_mut),
            Context::Workspaces => self
                .workspaces_window
                .as_mut()
                .map(WorkspacesWindow::picker_mut),
            Context::CommandHistory => self.history_window.as_mut().map(HistoryWindow::picker_mut),
            Context::KeyHints => self.key_hints.as_mut().map(KeyHintsWindow::picker_mut),
            Context::Jump => self.jump.as_mut().map(JumpMenu::picker_mut),
            _ => None,
        };
        if let Some(picker) = picker {
            picker.count_pages(pages);
        }
    }

    /// Whether a dialog, the search of a tree, or the configuration has the keys, which count
    /// their moves themselves, past their text fields too, where a count would go to the field.
    fn counts_moves(&self) -> bool {
        if self.key_hints.is_some() {
            return false;
        }
        if !self.dialogs.is_empty() || matches!(self.front(), Front::Find | Front::Results(_)) {
            return true;
        }
        let over = self.menu.is_some()
            || self.tab_menu.is_some()
            || self.jump.is_some()
            || self.workspaces_window.is_some()
            || self.history_window.is_some()
            || !self.results.is_empty()
            || self.in_front().is_some()
            || self.jobs_list.is_some()
            || self.help.is_some();
        !over && self.configuration.is_some()
    }

    /// Whether the app does something for `action` now; the F-key bar shows only those.
    pub(super) fn supports(&self, action: Action) -> bool {
        if self.context() == Context::Workspaces {
            // F6 and F8 rename and delete the workspace under the cursor.
            let chosen = self
                .workspaces_window
                .as_ref()
                .is_some_and(WorkspacesWindow::has_chosen);
            return action == Action::Cancel || chosen;
        }
        if action == Action::CloseTab {
            return self.tab_menu_closes();
        }
        match action {
            Action::Help
            | Action::Quit
            | Action::Redraw
            | Action::Disconnect
            | Action::Cancel
            | Action::PullDown
            | Action::PullDownLast
            | Action::Mkdir
            | Action::Delete
            | Action::Copy
            | Action::Duplicate
            | Action::Move
            | Action::Rename
            | Action::View
            | Action::Edit => true,
            Action::ToggleWrap => self.viewing.is_some(),
            Action::EditHost => self.chosen_host().is_some(),
            _ => false,
        }
    }
}
