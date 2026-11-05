//! Saving and restoring workspaces.

use noc_config::{PanelSide, Workspace, Workspaces};

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::describe::Reason;
use crate::tui::dialog::{Button, Dialog, DialogEvent, Problem};
use crate::tui::keymap::Resolved;
use crate::tui::label::{self};
use crate::tui::search::Slot;
use crate::tui::tabs::{PanelId, Tab, Tabs};
use crate::tui::window::Tone;
use crate::tui::workspaces::{self, WorkspacesEvent, WorkspacesWindow};

use super::{Again, App, Effect, Open, Purpose, Side, Way, WorkspaceChange, WorkspaceQuestion};

impl App {
    /// Asks for a name, and saves the tabs of both panels under it; it offers the name of the
    /// workspace restored or saved last if `again`, else none, for a new one.
    pub(super) fn ask_save_workspace(&mut self, again: bool) {
        let name = match &self.workspace {
            Some(name) if again => name.clone(),
            _ => String::new(),
        };
        let (title, prompt) = (fl!("workspaces-save-title"), fl!("workspaces-save-prompt"));
        let dialog = Dialog::form(&title, &prompt, &name, &[]);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Workspace(WorkspaceQuestion::Save),
        });
    }

    /// Saves the tabs of both panels under `name`, the text of the dialog of t W. A name
    /// that another workspace has asks first; that of the workspace restored or saved last
    /// does not, as saving it again is what the dialog offers.
    fn save_workspace_as(&mut self, name: &str) -> Vec<Effect> {
        let name = name.trim();
        if name.is_empty() {
            return Vec::new();
        }
        let workspace = self.workspace_named(name.to_owned());
        if self.workspaces.get(name).is_some() && self.workspace.as_deref() != Some(name) {
            let message = fl!(
                "workspaces-replace",
                name = cells::sanitize(name.as_bytes())
            );
            let buttons = vec![Button::Yes, Button::No];
            let title = fl!("workspaces-replace-title");
            let dialog = Dialog::question(&title, &message, buttons, 1, Tone::Warning);
            self.dialogs.push_back(Open {
                dialog,
                purpose: Purpose::Workspace(WorkspaceQuestion::Replace { workspace }),
            });
            return Vec::new();
        }
        self.save_workspace(workspace)
    }

    fn save_workspace(&mut self, workspace: Workspace) -> Vec<Effect> {
        self.workspace = Some(workspace.name.clone());
        if let Some(window) = &mut self.workspaces_window {
            window.focus(&workspace.name);
        }
        vec![Effect::Workspaces(WorkspaceChange::Save(workspace))]
    }

    /// The tabs of both panels, as a workspace named `name`.
    fn workspace_named(&self, name: String) -> Workspace {
        let saved = |side: Side| {
            let tabs = self.tabs(side);
            // One tab needs no mark of the one that shows.
            let several = tabs.len() > 1;
            tabs.iter()
                .enumerate()
                .map(|(index, tab)| {
                    let current = several && index == tabs.index();
                    workspaces::saved_tab(&tab.panel, current, &self.home)
                })
                .collect()
        };
        Workspace {
            name,
            active: match self.active {
                Side::Left => PanelSide::Left,
                Side::Right => PanelSide::Right,
            },
            left: saved(Side::Left),
            right: saved(Side::Right),
        }
    }

    /// Replaces the tabs of both panels with those of the workspace `name`. The tabs that show
    /// list their locations at once, the others once they show; the tabs that close take
    /// their marks with them.
    pub(super) fn restore_workspace(&mut self, name: &str) -> Vec<Effect> {
        let Some(workspace) = self.workspaces.get(name).cloned() else {
            let name = cells::sanitize(name.as_bytes());
            self.show_error(&fl!("workspaces-gone", name = name));
            return Vec::new();
        };
        let show_hidden = self.config.ui.show_hidden;
        let mut effects = Vec::new();
        for (side, saved) in [
            (Side::Left, &workspace.left),
            (Side::Right, &workspace.right),
        ] {
            let current = workspaces::current_of(saved);
            let mut tabs = Vec::with_capacity(saved.len());
            let mut first = None;
            for (index, saved) in saved.iter().enumerate() {
                let (mut panel, request) =
                    workspaces::restored_panel(saved, &self.home, show_hidden);
                panel.set_natural_sort(self.config.ui.natural_sort);
                self.last_tab += 1;
                let id = PanelId {
                    side,
                    tab: self.last_tab,
                };
                let mut tab = Tab::new(id, panel);
                if index == current {
                    first = Some((id, request));
                } else {
                    // Should the request be lost on the way, the tab reads its location then.
                    tab.stale = true;
                    tab.deferred = Some(request);
                }
                tabs.push(tab);
            }
            // `workspaces.toml` holds no side without tabs.
            if let Some(tabs) = Tabs::of(tabs, current) {
                *self.tabs_mut(side) = tabs;
            }
            if let Some((id, request)) = first {
                effects.extend(self.route(id, request));
            }
        }
        self.active = match workspace.active {
            PanelSide::Left => Side::Left,
            PanelSide::Right => Side::Right,
        };
        self.workspace = Some(workspace.name);
        self.sync_connected();
        effects
    }

    /// Opens the window of the saved workspaces.
    pub(super) fn open_workspaces(&mut self) {
        let rows = workspaces::Row::all(&self.workspaces);
        let window = WorkspacesWindow::new(rows, self.search_opens(Slot::Workspaces));
        self.workspaces_window = Some(window);
    }

    /// Gives a key to the window of the saved workspaces: a restored one closes it; saving,
    /// renaming, and deleting ask in a dialog over it.
    pub(super) fn handle_workspaces(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.workspaces_window else {
            return Vec::new();
        };
        match window.handle(input) {
            WorkspacesEvent::Pending => {}
            WorkspacesEvent::Closed => self.workspaces_window = None,
            WorkspacesEvent::Save => self.ask_save_workspace(false),
            WorkspacesEvent::Restore(name) => {
                self.workspaces_window = None;
                return self.restore_workspace(&name);
            }
            WorkspacesEvent::Rename(from) => {
                let shown = label::escape(&cells::sanitize(from.as_bytes()));
                let title = fl!("workspaces-rename-title");
                let prompt = fl!("workspaces-rename-prompt", name = shown);
                let dialog = Dialog::form(&title, &prompt, &from, &[]);
                self.dialogs.push_back(Open {
                    dialog,
                    purpose: Purpose::Workspace(WorkspaceQuestion::Rename { from }),
                });
            }
            WorkspacesEvent::Delete(name) => {
                let shown = cells::sanitize(name.as_bytes());
                let message = fl!("workspaces-delete", name = shown);
                let buttons = vec![Button::Yes, Button::No];
                let title = fl!("workspaces-delete-title");
                let dialog = Dialog::question(&title, &message, buttons, 0, Tone::Warning);
                self.dialogs.push_back(Open {
                    dialog,
                    purpose: Purpose::Workspace(WorkspaceQuestion::Delete { name }),
                });
            }
        }
        Vec::new()
    }

    /// Renames the workspace `from` to `to`, the text of the dialog of F6; a name that another
    /// workspace has asks first.
    fn rename_workspace(&mut self, from: String, to: &str) -> Vec<Effect> {
        let to = to.trim();
        if to.is_empty() || to == from {
            return Vec::new();
        }
        if self.workspaces.get(to).is_some() {
            let message = fl!(
                "workspaces-rename-replace",
                name = cells::sanitize(to.as_bytes())
            );
            let buttons = vec![Button::Yes, Button::No];
            let title = fl!("workspaces-rename-title");
            let dialog = Dialog::question(&title, &message, buttons, 1, Tone::Warning);
            let to = to.to_owned();
            self.dialogs.push_back(Open {
                dialog,
                purpose: Purpose::Workspace(WorkspaceQuestion::RenameOver { from, to }),
            });
            return Vec::new();
        }
        self.renamed(from, to.to_owned())
    }

    fn renamed(&mut self, from: String, to: String) -> Vec<Effect> {
        if self.workspace.as_ref() == Some(&from) {
            self.workspace = Some(to.clone());
        } else if self.workspace.as_ref() == Some(&to) {
            self.workspace = None;
        }
        vec![Effect::Workspaces(WorkspaceChange::Rename { from, to })]
    }

    fn delete_workspace(&mut self, name: String) -> Vec<Effect> {
        if self.workspace.as_ref() == Some(&name) {
            self.workspace = None;
        }
        vec![Effect::Workspaces(WorkspaceChange::Remove(name))]
    }

    /// Does what a dialog about workspaces was for, once `event` closed it with `text` in its
    /// field.
    pub(super) fn workspace_dialog_closed(
        &mut self,
        text: &str,
        question: WorkspaceQuestion,
        event: DialogEvent,
    ) -> Vec<Effect> {
        let ok = event == DialogEvent::Pressed(Button::Ok);
        let yes = event == DialogEvent::Pressed(Button::Yes);
        match question {
            WorkspaceQuestion::Save if ok => self.save_workspace_as(text),
            WorkspaceQuestion::Replace { workspace } if yes => self.save_workspace(workspace),
            WorkspaceQuestion::Rename { from } if ok => self.rename_workspace(from, text),
            WorkspaceQuestion::RenameOver { from, to } if yes => self.renamed(from, to),
            WorkspaceQuestion::Delete { name } if yes => self.delete_workspace(name),
            _ => Vec::new(),
        }
    }

    /// Takes the end of an [`Effect::Workspaces`]: the workspaces as `workspaces.toml` holds
    /// them now, or why it could not be read, or changed if `changed`.
    pub(crate) fn workspaces_changed(
        &mut self,
        change: WorkspaceChange,
        result: Result<Workspaces, Reason>,
    ) {
        match result {
            Ok(workspaces) => {
                self.workspaces = workspaces;
                if let Some(window) = &mut self.workspaces_window {
                    window.set_rows(workspaces::Row::all(&self.workspaces));
                }
            }
            Err(reason) => {
                let reason = reason.sanitized();
                let head = if change == WorkspaceChange::Load {
                    fl!("workspaces-load-error")
                } else {
                    fl!("workspaces-save-error")
                };
                let again = Again::Workspaces(change);
                self.show_problem_way(&Problem::new(head, reason), Way::Again(again));
            }
        }
    }
}
