//! The zoxide window and the search through files.

use std::path::Path;
use std::time::Duration;

use noc_tools::zoxide::Scored;
use noc_vfs::Location;
use tokio_util::sync::CancellationToken;

use crate::tui::find::{FindEvent, FindMode, FindSummary, FindWindow, Finding};
use crate::tui::jump::{JumpEvent, JumpMenu};
use crate::tui::keymap::{Resolved, Typed};
use crate::tui::panel::{Destination, child, home_text, location_text};
use crate::tui::search::Slot;

use super::{App, Effect, FIND_DELAY, FindRequest, Side};

impl App {
    /// Opens the zoxide window for the active panel, and asks zoxide for its best
    /// directories; in a search mode but the plain one, for all of them, which the window
    /// filters itself.
    pub(super) fn open_jump(&mut self) -> Vec<Effect> {
        let exclude = match self.panel(self.active).location() {
            Location::Local(path) => Some(path.clone()),
            Location::Remote { .. } => None,
        };
        let jump = JumpMenu::new(self.home.clone(), exclude, 0)
            .with_input(self.search_opens(Slot::Jump))
            .rows_like(self.sizes.jump);
        self.jump = Some(jump);
        self.query_jump()
    }

    /// Asks zoxide for the directories that match the keywords of the zoxide window.
    fn query_jump(&mut self) -> Vec<Effect> {
        let Some(jump) = &mut self.jump else {
            return Vec::new();
        };
        self.jump_queries += 1;
        let generation = self.jump_queries;
        jump.ask(generation);
        vec![Effect::ZoxideQuery {
            generation,
            keywords: jump.keywords(),
            exclude: jump.exclude().map(Path::to_path_buf),
        }]
    }

    /// Gives a key to the zoxide window: the directory it opens goes to the active panel, and
    /// counts in zoxide, as `z` counts it.
    pub(super) fn handle_jump(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(jump) = &mut self.jump else {
            return Vec::new();
        };
        match jump.handle(input) {
            JumpEvent::Pending => Vec::new(),
            JumpEvent::Closed => {
                self.jump = None;
                Vec::new()
            }
            JumpEvent::Query => self.query_jump(),
            JumpEvent::Open(path) => {
                self.jump = None;
                let id = self.shown(self.active);
                let record = self.config.zoxide.record;
                if let Some(tab) = self.tab_mut(id) {
                    tab.arriving = record.then(|| path.clone());
                }
                let mut effects = self.go(id, Destination::to(Location::Local(path.clone())));
                if record {
                    effects.push(Effect::ZoxideAdd(path));
                }
                effects
            }
        }
    }

    /// Takes the result of an [`Effect::ZoxideQuery`].
    pub(crate) fn jumps(&mut self, generation: u64, result: Result<Vec<Scored>, String>) {
        if let Some(jump) = &mut self.jump {
            jump.found(generation, result);
            self.sizes.jump = jump.rows_listed().or(self.sizes.jump);
        }
    }

    /// Opens the dialog of the search of the files under the active panel's directory in `mode`
    /// (`f /` and F9 → Command → Find file… for text, `f f` and Find files and dirs… for
    /// names), as far as `[search]` lets it go, with the hidden files if the panels show them,
    /// and lists the files at once.
    pub(super) fn open_find(&mut self, mode: FindMode) -> Vec<Effect> {
        let location = self.panel(self.active).location().clone();
        let title = match &location {
            Location::Local(path) => home_text(path, &self.home),
            Location::Remote { .. } => location_text(&location),
        };
        let search = &self.config.search;
        self.find = Some(FindWindow::new(
            (location, title),
            mode,
            (search.max_results.get(), search.max_per_file.get()),
            self.config.ui.show_hidden,
            (
                self.search_opens(Slot::FindText),
                self.search_opens(Slot::FindFiles),
            ),
            self.decor,
        ));
        self.search_tree(Duration::ZERO)
    }

    /// Starts the search of the dialog of the search of a tree again, after `delay`; the one
    /// before stops.
    pub(super) fn search_tree(&mut self, delay: Duration) -> Vec<Effect> {
        let Some(window) = &self.find else {
            return Vec::new();
        };
        let location = window.root().clone();
        // A host that is not connected has no session to search with: the window says so.
        let host = self.handle_for(&location).ok().flatten();
        self.finds += 1;
        let generation = self.finds;
        let cancel = CancellationToken::new();
        let Some(window) = &mut self.find else {
            return Vec::new();
        };
        window.restart(generation, cancel.clone());
        vec![Effect::Find(FindRequest {
            generation,
            location,
            host,
            query: window.query(),
            max_results: window.max_results(),
            delay,
            cancel,
        })]
    }

    /// Gives a key to the dialog of the search of a tree.
    pub(super) fn handle_find(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.find else {
            return Vec::new();
        };
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        let event = window.handle(typed, &mut self.registers);
        self.find_event(event)
    }

    /// Does what `event` of the dialog of the search of a tree says: a change searches again,
    /// Go to goes to the file in the active panel, which puts where it was in the jump list, or
    /// into a directory found by name, and View and Edit view and edit a file at the line, the
    /// dialog kept. Closing it stops the search.
    pub(super) fn find_event(&mut self, event: FindEvent) -> Vec<Effect> {
        match event {
            FindEvent::Pending => Vec::new(),
            FindEvent::Search => self.search_tree(FIND_DELAY),
            FindEvent::Closed => {
                self.find = None;
                Vec::new()
            }
            FindEvent::Go { location, name } => {
                self.find = None;
                let id = self.shown(self.active);
                let panel = self.panel_mut(id.side);
                // Another location goes in the jump list on the way; so does a row of this one.
                if *panel.location() == location {
                    panel.remember();
                }
                self.go(id, Destination::onto_entry(location, &name))
            }
            FindEvent::Open { location } => {
                self.find = None;
                let id = self.shown(self.active);
                self.go(id, Destination::to(location))
            }
            FindEvent::View {
                location,
                name,
                line,
            } => self.view_file(child(&location, &name), &location, line),
            FindEvent::Edit {
                location,
                name,
                line,
            } => self.edit_file(location, &name, line),
        }
    }

    /// Takes what the search `generation` of [`Effect::Find`] found.
    pub(crate) fn found(&mut self, generation: u64, found: Vec<Finding>) {
        if let Some(window) = &mut self.find {
            window.found(generation, found);
        }
    }

    /// Takes how the search `generation` of [`Effect::Find`] ended, or why it could not
    /// search.
    pub(crate) fn find_ended(&mut self, generation: u64, result: Result<FindSummary, String>) {
        if let Some(window) = &mut self.find {
            window.finished(generation, result);
        }
    }

    /// The user did something in `dir`, such as copy from it: a local directory that a panel
    /// shows goes to zoxide, once a visit (`zoxide.record`). Directories that no panel shows,
    /// such as a target typed in a dialog, and passing through, do not count.
    pub(super) fn note(&mut self, dir: &Location) -> Vec<Effect> {
        let Location::Local(path) = dir else {
            return Vec::new();
        };
        if !self.config.zoxide.record {
            return Vec::new();
        }
        let mut counted = false;
        for side in Side::BOTH {
            let tab = self.tabs_mut(side).active_mut();
            if tab.panel.location() == dir && !tab.noted {
                tab.noted = true;
                counted = true;
            }
        }
        if !counted {
            return Vec::new();
        }
        vec![Effect::ZoxideAdd(path.clone())]
    }
}
