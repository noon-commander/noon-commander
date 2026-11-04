//! The tabs of a panel and the tab menu.

use crate::tui::keymap::{Action, Resolved};
use crate::tui::search::Slot;
use crate::tui::tab_menu::{TabMenu, TabMenuEvent};
use crate::tui::tabs::{self, PanelId, Tab};

use super::{App, Effect, Side};

impl App {
    /// Does what a tab action does on `side`.
    pub(super) fn tab_action(&mut self, side: Side, action: Action) -> Vec<Effect> {
        match action {
            Action::NewTab => return self.new_tab(side),
            Action::CloseTab => {
                if self.tabs_mut(side).close().is_some() {
                    return self.revealed(side);
                }
            }
            Action::NextTab | Action::PrevTab => {
                self.end_search(side);
                if self.tabs_mut(side).step(action == Action::NextTab) {
                    return self.revealed(side);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// Opens a tab on `side` after the one that shows, on the same location, and shows it.
    fn new_tab(&mut self, side: Side) -> Vec<Effect> {
        self.end_search(side);
        let (panel, request) = self.panel(side).duplicate();
        self.last_tab += 1;
        let id = PanelId {
            side,
            tab: self.last_tab,
        };
        self.tabs_mut(side).push(Tab::new(id, panel));
        request.map_or_else(Vec::new, |request| self.route(id, request))
    }

    /// The tab that shows on `side` now asks for its first listing if a workspace restored it
    /// hidden, or reads its location again if it changed while hidden.
    pub(super) fn revealed(&mut self, side: Side) -> Vec<Effect> {
        let tab = self.tabs_mut(side).active_mut();
        let id = tab.id;
        if let Some(request) = tab.deferred.take()
            && tab
                .panel
                .pending_request()
                .is_some_and(|pending| pending.generation == request.generation)
        {
            tab.stale = false;
            return self.route(id, request);
        }
        if !std::mem::take(&mut tab.stale) {
            return Vec::new();
        }
        let here = tab.panel.here();
        let request = tab.panel.go(here);
        self.route(id, request)
    }

    /// Opens the tab menu for the panel on `side`, which lists where its tabs are.
    pub(super) fn open_tab_menu(&mut self, side: Side) {
        let titles = self.tab_titles(side);
        let input = self.search_opens(Slot::TabMenu);
        let current = self.tabs(side).index();
        self.tab_menu = Some(TabMenu::new(side, titles, current, input));
    }

    /// Where the tabs on `side` are, as the tab menu lists them.
    fn tab_titles(&self, side: Side) -> Vec<String> {
        self.tabs(side)
            .iter()
            .map(|tab| tabs::title(tab.panel.location(), &self.home))
            .collect()
    }

    /// Gives a key to the tab menu: the tab it shows is its panel's, and the keys stay with the
    /// active one, as after the location menu. A tab it closes goes as `t c` closes it; the menu
    /// stays.
    pub(super) fn handle_tab_menu(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(menu) = &mut self.tab_menu else {
            return Vec::new();
        };
        match menu.handle(input) {
            TabMenuEvent::Pending => Vec::new(),
            TabMenuEvent::Closed => {
                self.tab_menu = None;
                Vec::new()
            }
            TabMenuEvent::Show(index) => {
                let side = menu.side();
                self.tab_menu = None;
                self.end_search(side);
                if self.tabs_mut(side).select(index) {
                    return self.revealed(side);
                }
                Vec::new()
            }
            TabMenuEvent::Close(index) => {
                let side = menu.side();
                let shown = index == self.tabs(side).index();
                if shown {
                    self.end_search(side);
                }
                if self.tabs_mut(side).close_at(index).is_none() {
                    return Vec::new();
                }
                let titles = self.tab_titles(side);
                if let Some(menu) = &mut self.tab_menu {
                    menu.set_titles(titles);
                }
                if shown {
                    return self.revealed(side);
                }
                Vec::new()
            }
        }
    }

    /// Whether the tab menu is open on a panel with more than one tab, of which F8 closes one.
    pub(super) fn tab_menu_closes(&self) -> bool {
        self.tab_menu
            .as_ref()
            .is_some_and(|menu| self.tabs(menu.side()).len() > 1)
    }
}
