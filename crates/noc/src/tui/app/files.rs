//! Going to a directory, making one, renaming, and marking by pattern.

use noc_vfs::Location;

use crate::i18n::fl;
use crate::tui::cd;
use crate::tui::cells::{self};
use crate::tui::describe::Reason;
use crate::tui::dialog::{Dialog, Problem};
use crate::tui::keymap::{Action, Resolved};
use crate::tui::panel::{Panel, child, location_text};
use crate::tui::search::Slot;
use crate::tui::tabs::PanelId;
use crate::tui::tasks::{Refused, Unmade};

use super::{
    Again, Answer, App, Change, Effect, Host, Open, PATTERN_DIALOG_WIDTH, PatternOptions, Purpose,
    Way, file_name, names_an_entry,
};

impl App {
    /// Opens Go to directory, `g d`, for the active panel, as mc's Quick cd.
    pub(super) fn ask_go_to(&mut self) {
        self.ask_go_to_in(self.shown(self.active), "");
    }

    /// Opens Go to directory for `panel`, with `text` typed.
    pub(super) fn ask_go_to_in(&mut self, panel: PanelId, text: &str) {
        let (title, prompt) = (fl!("goto-title"), fl!("goto-prompt"));
        let dialog = Dialog::form(&title, &prompt, text, &[]).with_completion();
        // First, as it may follow a dialog that just closed, ahead of others waiting.
        self.dialogs.push_front(Open {
            dialog,
            purpose: Purpose::GoToDir { panel },
        });
    }

    /// Sends `panel` where `cd <text>` leads from its directory. A path that is not there
    /// leaves the panel where it is and asks what to do, as any listing does.
    pub(super) fn cd(&mut self, panel: PanelId, text: &str) -> Vec<Effect> {
        let Some(tab) = self.tabs(panel.side).get(panel.tab) else {
            return Vec::new();
        };
        let here = tab.panel.location();
        let (dir, last_local) = self.last_local(&tab.panel);
        let Some(location) = cd::target(text, here, &self.home, &dir, tab.previous.as_ref()) else {
            return Vec::new();
        };
        if location == *here {
            return Vec::new();
        }
        let destination = if location == *last_local.location() {
            last_local
        } else {
            tab.panel.destination(location)
        };
        self.go(panel, destination)
    }

    /// Opens the dialog of F7 for the active panel, with an empty field.
    pub(super) fn ask_mkdir(&mut self) {
        let panel = self.shown(self.active);
        self.ask_mkdir_in(panel, "");
    }

    /// Opens the dialog of F7 for `panel`, with `text` in its field.
    pub(super) fn ask_mkdir_in(&mut self, panel: PanelId, text: &str) {
        let (title, prompt) = (fl!("mkdir-title"), fl!("mkdir-prompt"));
        let dialog = Dialog::form(&title, &prompt, text, &[]).with_completion();
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Mkdir { panel },
        });
    }

    /// Makes the directory at `location` for `panel`, in the background; that counts as work
    /// in the panel's directory.
    pub(super) fn create_dir(&mut self, panel: PanelId, location: Location) -> Vec<Effect> {
        let noted = match self.panel_of(panel) {
            Some(shown) => {
                let dir = shown.location().clone();
                self.note(&dir)
            }
            None => Vec::new(),
        };
        self.begin_work(panel, &location);
        let host = match &location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Some(handle.clone()),
                _ => None,
            },
            Location::Local(_) => None,
        };
        let mut effects = vec![Effect::CreateDir {
            panel,
            location,
            host,
        }];
        effects.extend(noted);
        effects
    }

    /// Takes the result of an [`Effect::CreateDir`]: panels on the directory it is in read it
    /// again, the one that asked with the cursor on it, and hidden tabs once they show; an
    /// error shows in a dialog.
    pub(crate) fn created(
        &mut self,
        panel: PanelId,
        location: &Location,
        result: Result<(), Unmade>,
    ) -> Vec<Effect> {
        let problem = |reason: Reason| {
            Problem::new(fl!("mkdir-error"), reason.sanitized()).at(location_text(location))
        };
        let effects = match result {
            Ok(()) => self.show_new(panel, location),
            Err(Unmade::Taken(reason)) => {
                let text = self.typed_in(panel, location);
                self.show_problem_way(&problem(reason), Way::Change(Change::Mkdir { panel, text }));
                Vec::new()
            }
            Err(Unmade::Failed(reason)) => {
                let again = Again::CreateDir {
                    panel,
                    location: location.clone(),
                };
                self.show_problem_way(&problem(reason), Way::Again(again));
                Vec::new()
            }
        };
        self.end_work(panel, location);
        effects
    }

    /// The text that names `location` from the directory of `panel`, as F7 takes it: its name
    /// alone where it is in that directory, its path otherwise.
    fn typed_in(&self, panel: PanelId, location: &Location) -> String {
        let here = self.panel_of(panel).map(Panel::location);
        let name = file_name(location).filter(|_| location.parent().as_ref() == here);
        match (name, location) {
            (Some(name), _) => String::from_utf8_lossy(&name).into_owned(),
            (None, Location::Local(path)) => path.display().to_string(),
            (None, Location::Remote { path, .. }) => path.display().into_owned(),
        }
    }

    /// `panel` waits for work that makes `location`, with dots on its title, as for a listing.
    fn begin_work(&mut self, panel: PanelId, location: &Location) {
        if let Some(tab) = self.tab_mut(panel) {
            tab.panel.begin_work(location.clone());
        }
    }

    /// `panel` no longer waits for the work that made `location`; called after the listing its
    /// result asks for, so that the dots go on until that comes.
    fn end_work(&mut self, panel: PanelId, location: &Location) {
        if let Some(tab) = self.tab_mut(panel) {
            tab.panel.end_work(location);
        }
    }

    /// Panels on the directory that holds the new `location` read it again, `panel` with the
    /// cursor on it, and hidden tabs once they show.
    fn show_new(&mut self, panel: PanelId, location: &Location) -> Vec<Effect> {
        let parent = location.parent();
        let name = file_name(location);
        let mut effects = Vec::new();
        for id in self.panel_ids() {
            let shown = self.is_shown(id);
            let Some(tab) = self.tab_mut(id) else {
                continue;
            };
            if Some(tab.panel.location()) != parent.as_ref() {
                continue;
            }
            let request = match &name {
                Some(name) if id == panel => tab.panel.reload_onto(name.clone()),
                _ if !shown => {
                    tab.stale = true;
                    continue;
                }
                _ => {
                    let here = tab.panel.here();
                    tab.panel.go(here)
                }
            };
            effects.extend(self.route(id, request));
        }
        effects
    }

    /// Starts renaming the entry under the cursor of the active panel in its row, as Shift+F6;
    /// a name that is not UTF-8 cannot be edited, which an error says.
    pub(super) fn start_rename(&mut self) {
        let panel = self.panel_mut(self.active);
        if panel.start_rename() {
            return;
        }
        let invalid = panel
            .name_under_cursor()
            .filter(|name| std::str::from_utf8(name).is_err())
            .map(cells::sanitize);
        if let Some(name) = invalid {
            self.show_error(&fl!("rename-not-utf8", name = name));
        }
    }

    /// Gives a key to the field of the entry renamed in the active panel: Enter renames it,
    /// Esc in Normal mode leaves it as it was, and other keys edit the name in vim's modes.
    pub(super) fn handle_rename(&mut self, input: Resolved) -> Vec<Effect> {
        let extra = self.extra;
        let mut registers = std::mem::take(&mut self.registers);
        let panel = self.panel_mut(self.active);
        let action = match (input, panel.rename_field()) {
            (Resolved::Insert(c), Some(field)) => {
                field.insert(c);
                None
            }
            (Resolved::Action(action), Some(field)) => {
                (!field.handle(action, extra, &mut registers)).then_some(action)
            }
            (Resolved::Hotkey(_), _) | (_, None) => None,
        };
        self.registers = registers;
        match action {
            Some(Action::Confirm) => return self.confirm_rename(),
            Some(Action::Cancel) => {
                self.panel_mut(self.active).end_rename();
            }
            // A key that is not the field's is the one command after Ctrl+o.
            Some(_) => {
                if let Some(field) = self.panel_mut(self.active).rename_field() {
                    field.give_up();
                }
            }
            None => {}
        }
        Vec::new()
    }

    /// Renames the entry renamed in the active panel to the name in its field. An empty or
    /// unchanged name leaves it as it was; one that cannot name an entry in the directory
    /// shows an error, and the field stays for another try.
    fn confirm_rename(&mut self) -> Vec<Effect> {
        let id = self.shown(self.active);
        let Some(text) = self
            .panel_mut(self.active)
            .rename_field()
            .map(|field| field.text().to_owned())
        else {
            return Vec::new();
        };
        if !names_an_entry(&text) {
            let name = cells::sanitize(text.as_bytes());
            self.show_error(&fl!("rename-invalid", name = name));
            return Vec::new();
        }
        let panel = self.panel_mut(self.active);
        let Some((name, _)) = panel.end_rename() else {
            return Vec::new();
        };
        if text.is_empty() || text.as_bytes() == name {
            return Vec::new();
        }
        let dir = panel.location().clone();
        self.rename_entry(id, child(&dir, &name), child(&dir, text.as_bytes()), false)
    }

    /// Renames `from` to `to` for `panel`, in the background, over a file there if `replace`;
    /// that counts as work in the directory.
    pub(super) fn rename_entry(
        &mut self,
        panel: PanelId,
        from: Location,
        to: Location,
        replace: bool,
    ) -> Vec<Effect> {
        let Ok(host) = self.handle_for(&from) else {
            let again = Again::Rename {
                panel,
                from: from.clone(),
                to,
            };
            self.show_not_connected(fl!("rename-error"), &from, again);
            return Vec::new();
        };
        let noted = from.parent().map_or_else(Vec::new, |dir| self.note(&dir));
        self.begin_work(panel, &to);
        let mut effects = vec![Effect::Rename {
            panel,
            from,
            to,
            replace,
            host,
        }];
        effects.extend(noted);
        effects
    }

    /// Takes the result of an [`Effect::Rename`]: panels on the directory read it again, the
    /// one that renamed with the cursor on the new name. A file with that name asks what to do,
    /// as a copy does; an error shows in a dialog.
    pub(crate) fn entry_renamed(
        &mut self,
        panel: PanelId,
        from: &Location,
        to: &Location,
        result: Result<(), Refused>,
    ) -> Vec<Effect> {
        let effects = self.take_renamed(panel, from, to, result);
        self.end_work(panel, to);
        effects
    }

    fn take_renamed(
        &mut self,
        panel: PanelId,
        from: &Location,
        to: &Location,
        result: Result<(), Refused>,
    ) -> Vec<Effect> {
        match result {
            Ok(()) => self.show_new(panel, to),
            Err(Refused::Taken(taken)) => {
                let new = taken
                    .source
                    .as_ref()
                    .map_or((None, None), |source| (source.size, source.modified));
                let answer = Answer::Rename {
                    panel,
                    from: from.clone(),
                };
                self.ask_taken(answer, to.clone(), new, *taken);
                Vec::new()
            }
            Err(Refused::Failed(reason)) => {
                let reason = reason.sanitized();
                let problem = Problem::new(fl!("rename-error"), reason).at(location_text(from));
                let name = file_name(from).unwrap_or_default();
                let text = String::from_utf8_lossy(&file_name(to).unwrap_or_default()).into_owned();
                let change = Change::Rename { panel, name, text };
                self.show_problem_way(&problem, Way::Change(change));
                Vec::new()
            }
        }
    }

    /// Opens the dialog of `g +` (`mark`) or `g -` for the active panel, as mc's `+`
    /// and `-`: a pattern, and whether it applies to files only and whether case counts.
    pub(super) fn ask_pattern(&mut self, mark: bool) {
        let title = if mark {
            fl!("pattern-select")
        } else {
            fl!("pattern-unselect")
        };
        let options = &self.pattern_options;
        let checks = [(fl!("pattern-files-only"), options.files_only)];
        let dialog = Dialog::form(&title, &fl!("pattern-text"), &options.pattern, &checks)
            .with_width(PATTERN_DIALOG_WIDTH)
            .with_search(0, self.search_opens(Slot::Mark));
        let panel = self.shown(self.active);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Pattern { panel, mark },
        });
    }

    /// Marks, or unmarks, what the dialog of `g +` or `g -` asked for. An empty pattern does
    /// nothing.
    pub(super) fn mark_matching(&mut self, panel: PanelId, mark: bool, dialog: &Dialog) {
        if dialog.text().is_empty() {
            return;
        }
        let options = PatternOptions {
            pattern: dialog.text().to_owned(),
            files_only: dialog.checked(0),
        };
        let input = dialog
            .search(0)
            .unwrap_or_else(|| self.search_opens(Slot::Mark));
        let mut matcher = input.matcher(&options.pattern);
        if let Some(panel) = self.panel_of_mut(panel) {
            panel.mark_where(mark, |entry| {
                !(options.files_only && entry.is_dir_like())
                    && matcher.score(&entry.display_name()).is_some()
            });
        }
        self.pattern_options = options;
    }
}
