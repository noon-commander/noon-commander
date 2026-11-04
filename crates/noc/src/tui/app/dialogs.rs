//! Dialogs: their keys, their answers, and the errors they show.

use noc_config::{HostConfig, SftpHost};
use noc_ops::{Decision, Later, Resolution};
use noc_vfs::Location;

use crate::i18n::fl;
use crate::tui::command::CommandLine;
use crate::tui::complete::{self, ChoicesEvent};
use crate::tui::configuration::run_of;
use crate::tui::dialog::{Button, Dialog, DialogEvent, Problem};
use crate::tui::keymap::{Action, Context, Resolved, Typed};
use crate::tui::panel::{self, location_text};
use crate::tui::transfer::{self};

use super::{
    Again, App, Change, Completes, Effect, HOST_LABEL, HOST_OTHER_DIR, HOST_RUN, HOST_START_DIR,
    Handover, Host, Open, Purpose, Way, choices_for,
};

impl App {
    /// Gives a key to the dialog in front, and does what it was for once it closes. In a path
    /// field, Tab completes, and the list of completions takes the keys while it shows.
    pub(super) fn handle_dialog(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(effects) = self.change_dir(Completes::Field, input) {
            return effects;
        }
        if self.choose(Completes::Field, input) {
            return Vec::new();
        }
        if input == Resolved::Action(Action::Complete) {
            return self.complete();
        }
        // Any other key makes a completion on its way too late.
        self.completion = None;
        let Some(open) = self.dialogs.front_mut() else {
            return Vec::new();
        };
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        let event = open.dialog.handle_typed(typed, &mut self.registers);
        self.dialog_event(event)
    }

    /// Gives a key to the list of completions for `target`, if it shows; whether the key was
    /// the list's. Any other key closes it, keeping the text, and goes on to the field.
    pub(super) fn choose(&mut self, target: Completes, input: Resolved) -> bool {
        let Some(choices) = choices_for(&mut self.completion, target) else {
            return false;
        };
        let event = match input {
            Resolved::Action(action) => choices.handle(action),
            Resolved::Insert(_) | Resolved::Hotkey(_) => ChoicesEvent::Passed,
        };
        self.take_choices(event)
    }

    /// Left and Right in the list of completions for `target`, as in vim's pop-up menu of the
    /// command line: Left completes again in the parent of the directory listed, and Right in
    /// the directory chosen; `None` where the key is not one of them, or Right has no
    /// directory to go into.
    pub(super) fn change_dir(&mut self, target: Completes, input: Resolved) -> Option<Vec<Effect>> {
        let up = match input {
            Resolved::Action(Action::Left) => true,
            Resolved::Action(Action::Right) => false,
            _ => return None,
        };
        let completing = self
            .completion
            .as_ref()
            .filter(|completing| completing.target == target && completing.choices.is_some())?;
        let before = self.before_cursor(target)?;
        let (lead, word) = before.split_at(completing.word.min(before.len()));
        let word = if up {
            complete::up_dir(word)
        } else {
            complete::down_dir(word)?
        };
        let before = format!("{lead}{word}");
        self.completion = None;
        self.put_before_cursor(target, &before);
        Some(match target {
            Completes::Field => self.complete(),
            Completes::CommandLine => self.complete_command(),
        })
    }

    /// Does what a key or a press did to the list of completions: puts a match in, or what
    /// was typed, and closes the list if it is done. Whether it was the list's.
    pub(super) fn take_choices(&mut self, event: ChoicesEvent) -> bool {
        let Some(completing) = &self.completion else {
            return false;
        };
        let ChoicesEvent::Show { text, close } = event else {
            self.completion = None;
            return false;
        };
        let target = completing.target;
        let before = match text {
            Some(text) => format!("{}{text}", completing.keep),
            None => completing.before.clone(),
        };
        if close {
            self.completion = None;
        }
        self.put_before_cursor(target, &before);
        true
    }

    /// The text before the cursor where a completion for `target` goes, if it can go there
    /// now: the path field of the dialog in front, or the command line with the keys.
    pub(super) fn before_cursor(&self, target: Completes) -> Option<&str> {
        match target {
            Completes::Field => self
                .dialogs
                .front()
                .filter(|open| open.dialog.completes())
                .map(|open| open.dialog.around_cursor().0),
            Completes::CommandLine => self
                .command_line
                .as_ref()
                .filter(|_| {
                    matches!(
                        self.context(),
                        Context::CommandLine | Context::CommandCompletion
                    )
                })
                .map(CommandLine::before_cursor),
        }
    }

    /// Replaces the text before the cursor where a completion for `target` goes with `before`.
    pub(super) fn put_before_cursor(&mut self, target: Completes, before: &str) {
        match target {
            Completes::Field => {
                if let Some(open) = self.dialogs.front_mut() {
                    open.dialog.replace_before_cursor(before);
                }
            }
            Completes::CommandLine => {
                if let Some(line) = &mut self.command_line {
                    line.replace_before_cursor(before);
                }
            }
        }
    }

    /// Does what the dialog in front was for, if `event` closes it.
    pub(super) fn dialog_event(&mut self, event: DialogEvent) -> Vec<Effect> {
        if let Some(effects) = self.set_question_aside(event) {
            return effects;
        }
        if matches!(event, DialogEvent::Pending | DialogEvent::Opened(_)) || self.keeps_open(event)
        {
            return Vec::new();
        }
        match self.dialogs.pop_front() {
            Some(Open { dialog, purpose }) => self.dialog_closed(&dialog, purpose, event),
            None => Vec::new(),
        }
    }

    /// Does what a dialog was for, once `event` closed it.
    fn dialog_closed(
        &mut self,
        dialog: &Dialog,
        purpose: Purpose,
        event: DialogEvent,
    ) -> Vec<Effect> {
        let ok = event == DialogEvent::Pressed(Button::Ok);
        match purpose {
            Purpose::Ssh { reply, .. } => {
                if let Some(reply) = reply {
                    reply.send(dialog.answer(event));
                }
            }
            Purpose::Pattern { panel, mark } if ok => self.mark_matching(panel, mark, dialog),
            Purpose::Mkdir { panel } if ok && !dialog.text().is_empty() => {
                let resolved = self.panel_of(panel).map(|p| p.resolve(dialog.text()));
                if let Some(location) = resolved {
                    return self.create_dir(panel, location);
                }
            }
            Purpose::Delete { dir, names } if event == DialogEvent::Pressed(Button::Yes) => {
                return self.start_delete(dir, &names);
            }
            Purpose::Transfer {
                kind,
                panel,
                dir,
                names,
                sources,
                offered,
            } if ok => {
                self.copy_choices.preserve = transfer::preserve(dialog);
                let target = match offered {
                    Some((text, location)) if text == dialog.text() => Some(location),
                    _ => self.resolve_target(panel, dialog.text()),
                };
                if let Some(target) = target {
                    let typed = Change::Transfer {
                        kind,
                        panel,
                        dir: dir.clone(),
                        names: names.clone(),
                        sources,
                        text: dialog.text().to_owned(),
                    };
                    return self.start_transfer(kind, dir, &names, target, typed);
                }
            }
            Purpose::Failure { job, reply } => {
                self.answered(job);
                let _ = reply.send(resolution_of(event, dialog));
            }
            Purpose::Conflict { answer, target, .. } => {
                return self.conflict_answered(answer, target, event, dialog);
            }
            Purpose::ConflictRename { question } => {
                if ok {
                    return self.rename_conflict(*question, dialog.text());
                }
                self.dialogs.push_front(*question);
            }
            Purpose::EditHost { name, old, .. } if ok => {
                return save_host(name, old.as_ref(), dialog);
            }
            Purpose::Checksum {
                dir,
                names,
                other,
                expect,
            } if ok => return self.start_checksum(dialog, dir, &names, other, expect),
            Purpose::SaveFile { dir, bytes } if ok && !dialog.text().is_empty() => {
                let location = panel::resolve(&dir, &self.home, dialog.text());
                return self.write_file(location, bytes, false);
            }
            Purpose::GoToDir { panel } if ok => return self.cd(panel, dialog.text()),
            Purpose::Workspace(question) => {
                return self.workspace_dialog_closed(dialog.text(), question, event);
            }
            Purpose::Reconnect { host, reason } => {
                if event == DialogEvent::Pressed(Button::Retry) {
                    return self.reconnect(&host);
                }
                return self.give_up(&host, &reason);
            }
            Purpose::ListFailed { panel, failure } => {
                return self.list_failed_closed(panel, failure, event);
            }
            Purpose::NotConnected { panel } => return self.not_connected_closed(panel, event),
            Purpose::Quit => self.quit = event == DialogEvent::Pressed(Button::Yes),
            Purpose::EditHost { .. }
            | Purpose::GoToDir { .. }
            | Purpose::Checksum { .. }
            | Purpose::SaveFile { .. }
            | Purpose::Pattern { .. }
            | Purpose::Mkdir { .. }
            | Purpose::Delete { .. }
            | Purpose::Transfer { .. }
            | Purpose::Info
            | Purpose::InfoOver => {}
            Purpose::Problem { way } => return self.problem_closed(way, event),
        }
        Vec::new()
    }

    /// Shows `message` in an error dialog, after the dialogs open before it.
    pub(super) fn show_error(&mut self, message: &str) {
        self.dialogs.push_back(Open {
            dialog: Dialog::error(message),
            purpose: Purpose::Info,
        });
    }

    /// Shows `message` in an error dialog over the dialog in front, which stays open, and in
    /// sight, under it.
    pub(super) fn show_error_over(&mut self, message: &str) {
        self.dialogs.push_front(Open {
            dialog: Dialog::error(message),
            purpose: Purpose::InfoOver,
        });
    }

    /// Shows `problem` in an error dialog, after the dialogs open before it.
    pub(super) fn show_problem(&mut self, problem: &Problem) {
        self.dialogs.push_back(Open {
            dialog: Dialog::problem(problem),
            purpose: Purpose::Info,
        });
    }

    /// Shows `problem` in an error dialog over the dialog in front, which stays open, and in
    /// sight, under it.
    pub(super) fn show_problem_over(&mut self, problem: &Problem) {
        self.dialogs.push_front(Open {
            dialog: Dialog::problem(problem),
            purpose: Purpose::InfoOver,
        });
    }

    /// Shows `problem` in an error dialog, after the dialogs open before it, with `way` on
    /// from it first, the default, and Cancel (ADR 0063). Where a host that `again` needs is
    /// not connected, Connect takes the place of Retry.
    pub(super) fn show_problem_way(&mut self, problem: &Problem, way: Way) {
        let first = match &way {
            Way::Again(again) if self.host_away(again).is_some() => Button::Connect,
            Way::Again(_) => Button::Retry,
            Way::Change(_) => Button::Change,
        };
        self.dialogs.push_back(Open {
            dialog: Dialog::failure(problem, vec![first, Button::Cancel], 0),
            purpose: Purpose::Problem { way },
        });
    }

    /// Says that `head` cannot be done at `location`, as its host is not connected, with
    /// Connect, which connects and does `again` then.
    pub(super) fn show_not_connected(&mut self, head: String, location: &Location, again: Again) {
        let problem =
            Problem::new(head, fl!("error-connection-closed")).at(location_text(location));
        self.show_problem_way(&problem, Way::Again(again));
    }

    /// A host that `again` needs and that is not connected.
    fn host_away(&self, again: &Again) -> Option<String> {
        again
            .hosts()
            .into_iter()
            .find(|host| !matches!(self.hosts.get(*host), Some(Host::Connected { .. })))
            .map(str::to_owned)
    }

    /// Whether `host` is connected, or on its way.
    fn reachable(&self, host: &str) -> bool {
        matches!(
            self.hosts.get(host),
            Some(Host::Connected { .. } | Host::Connecting { .. })
        )
    }

    /// Connects to `host` unless it is connected or on its way; a host that waits for an
    /// answer to whether to try again tries again.
    fn connect_to(&mut self, host: &str) -> Vec<Effect> {
        if self.reachable(host) {
            return Vec::new();
        }
        self.hosts.remove(host);
        let effects = self.connect(host);
        self.sync_connected();
        effects
    }

    /// Does `again` once the hosts it needs are connected: at once if they are, else after
    /// connecting to them.
    fn connect_and_do(&mut self, again: Again) -> Vec<Effect> {
        let away: Vec<String> = again
            .hosts()
            .into_iter()
            .filter(|host| !matches!(self.hosts.get(*host), Some(Host::Connected { .. })))
            .map(str::to_owned)
            .collect();
        if away.is_empty() {
            return self.again(again);
        }
        self.pending.push(again);
        away.iter().flat_map(|host| self.connect_to(host)).collect()
    }

    /// Does what waited for `host`, and for no other host that is not connected yet.
    pub(super) fn do_pending(&mut self, host: &str) -> Vec<Effect> {
        let (ready, waiting): (Vec<Again>, Vec<Again>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|again| again.hosts().contains(&host) && self.host_away(again).is_none());
        self.pending = waiting;
        ready
            .into_iter()
            .flat_map(|again| self.again(again))
            .collect()
    }

    /// Forgets what waited for `host`, which will not connect.
    pub(super) fn forget_pending(&mut self, host: &str) {
        self.pending.retain(|again| !again.hosts().contains(&host));
    }

    /// Does what the error dialog of `problem` offered, once `event` closed it: Retry, Connect,
    /// or Change… as `way` says; Cancel and Esc do nothing, but end an edit.
    fn problem_closed(&mut self, way: Way, event: DialogEvent) -> Vec<Effect> {
        match (way, event) {
            (Way::Again(again), DialogEvent::Pressed(Button::Retry)) => self.again(again),
            (Way::Again(again), DialogEvent::Pressed(Button::Connect)) => {
                self.connect_and_do(again)
            }
            (Way::Change(change), DialogEvent::Pressed(Button::Change)) => {
                self.change(change);
                Vec::new()
            }
            (Way::Again(Again::Edit(editing)), _) => self.finish_edit(editing, false),
            _ => Vec::new(),
        }
    }

    /// Does `again` again.
    fn again(&mut self, again: Again) -> Vec<Effect> {
        match again {
            Again::Delete { dir, names } => self.start_delete(dir, &names),
            Again::Transfer {
                kind,
                dir,
                names,
                target,
                typed,
            } => self.start_transfer(kind, dir, &names, target, typed),
            Again::Checksum {
                dir,
                groups,
                algorithm,
                expected,
                compare,
            } => self.hash(dir, groups, (algorithm, expected), compare),
            Again::EditFile { dir, name, line } => self.edit_file(dir, &name, line),
            Again::RunIn {
                dir,
                command,
                prompt,
            } => self.run_in(&dir, command, prompt),
            Again::CreateDir { panel, location } => self.create_dir(panel, location),
            Again::Rename { panel, from, to } => self.rename_entry(panel, from, to, false),
            Again::Write { location, bytes } => self.write_file(location, bytes, false),
            Again::View(location) => {
                let dir = location.parent().unwrap_or_else(|| location.clone());
                self.view_file(location, &dir, None)
            }
            Again::ReadMore => {
                if let Some(viewing) = &mut self.viewing {
                    viewing.viewer.resume();
                }
                self.read_more()
            }
            Again::Edit(editing) => {
                self.handover = Some(Handover::Edit {
                    file: editing.file.clone(),
                    line: editing.line,
                });
                self.editing = Some(editing);
                Vec::new()
            }
            Again::SendBack { copy, remote, dir } => self.send_back(copy, remote, dir),
            Again::Run(run) => {
                self.handover = Some(Handover::Run(run));
                Vec::new()
            }
            Again::SaveConfig { old, new } => vec![Effect::SaveConfig {
                old,
                new,
                config: Box::new(self.config.clone()),
            }],
            Again::SaveHost { name, host } => vec![Effect::SaveHost { name, host }],
            Again::Workspaces(change) => vec![Effect::Workspaces(change)],
            Again::History(change) => vec![Effect::History(change)],
        }
    }

    /// Opens the dialog of `change` again, with what was typed in it.
    fn change(&mut self, change: Change) {
        match change {
            Change::Rename { panel, name, text } => {
                if self.shown(panel.side) != panel {
                    return;
                }
                self.active = panel.side;
                self.panel_mut(panel.side).rename_again(&name, &text);
            }
            Change::Transfer {
                kind,
                panel,
                dir,
                names,
                sources,
                text,
            } => {
                let field = (text.clone(), text.chars().count());
                self.open_transfer_in((panel, dir), kind, &sources, names, field, None);
            }
            Change::Mkdir { panel, text } => self.ask_mkdir_in(panel, &text),
        }
    }
}

fn resolution_of(event: DialogEvent, dialog: &Dialog) -> Resolution {
    const LATER: [Later; 3] = [Later::Ask, Later::SkipAll, Later::SkipSame];
    let this = match event {
        DialogEvent::Pressed(Button::Skip) => Decision::Skip,
        DialogEvent::Pressed(Button::Retry) => Decision::Retry,
        _ => Decision::Abort,
    };
    Resolution {
        this,
        rest: LATER.get(dialog.chosen(0)).copied().unwrap_or(Later::Ask),
    }
}

/// Saves what the dialog of F4 on the host `name` holds, if it changed anything.
fn save_host(name: String, old: Option<&HostConfig>, dialog: &Dialog) -> Vec<Effect> {
    let text = |index: usize| {
        let text = dialog.text_of(index).trim();
        (!text.is_empty()).then(|| text.to_owned())
    };
    let host = HostConfig::Sftp(SftpHost {
        label: text(HOST_LABEL),
        start_dir: text(HOST_START_DIR),
        other_dir: text(HOST_OTHER_DIR),
        remember_dir: dialog.checked(0),
        // OK checked it.
        run: text(HOST_RUN).and_then(|run| run_of(&run).ok()),
    });
    let unchanged = match old {
        Some(old) => *old == host,
        None => host.is_default(),
    };
    if unchanged {
        return Vec::new();
    }
    let host = (!host.is_default()).then_some(host);
    vec![Effect::SaveHost { name, host }]
}
