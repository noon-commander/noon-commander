//! Hosts: connecting, listing, and their settings.

use std::sync::Arc;

use noc_config::{HostConfig, Hosts, SftpHost};
use noc_ops::Algorithm;
use noc_ssh::askpass::PromptKind;
use noc_vfs::Location;
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::configuration::{run_of, run_text};
use crate::tui::describe::Reason;
use crate::tui::dialog::{Ask, Button, Dialog, DialogEvent, Problem, Reply, Tone};
use crate::tui::keymap::Action;
use crate::tui::panel::{
    Destination, Failure, HostState, HostStatus, ListError, ListErrorKind, ListRequest, Listed,
};
use crate::tui::places::volume_of;
use crate::tui::tabs::PanelId;
use crate::tui::tasks::HostHandle;

use super::{
    Again, Answer, App, Effect, HOST_OTHER_DIR, HOST_RUN, HOST_START_DIR, Host, Open, Purpose,
    Side, Way, cd_text, names_an_entry, parse_expected, waits_for,
};

impl App {
    /// Opens the dialog of F4 on the host `name`: its settings from `hosts.toml`.
    pub(super) fn ask_edit_host(&mut self, name: String) {
        let old = self.host_settings.get(&name).cloned();
        let sftp = match &old {
            Some(HostConfig::Sftp(sftp)) => sftp.clone(),
            None => SftpHost::default(),
        };
        let current = [self.active, self.active.other()]
            .into_iter()
            .find_map(|side| match self.panel(side).location() {
                Location::Remote { host, path } if *host == name && !path.as_bytes().is_empty() => {
                    Some(String::from_utf8_lossy(path.as_bytes()).into_owned())
                }
                _ => None,
            });
        let text = |value: &Option<String>| value.clone().unwrap_or_default();
        let fields = [
            (fl!("host-label"), text(&sftp.label)),
            (fl!("host-start-dir"), text(&sftp.start_dir)),
            (fl!("host-other-dir"), text(&sftp.other_dir)),
            (
                fl!("host-run"),
                sftp.run.as_ref().map(run_text).unwrap_or_default(),
            ),
        ];
        let checks = [(fl!("host-remember-dir"), sftp.remember_dir)];
        let mut buttons = vec![Button::Ok, Button::Cancel];
        if current.is_some() {
            buttons.insert(1, Button::UseCurrent);
        }
        let title = fl!("host-edit-title", host = cells::sanitize(name.as_bytes()));
        let dialog = Dialog::fields(&title, &fields, &checks, buttons)
            .with_placeholder(HOST_RUN, &run_text(&self.config.shell.run));
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::EditHost { name, old, current },
        });
    }

    /// Whether the dialog in front stays open after `event`: Use Current fills in a field, and
    /// OK on host settings that are not valid, or on an expected checksum that is none, shows
    /// why over it.
    pub(super) fn keeps_open(&mut self, event: DialogEvent) -> bool {
        let Some(Open { dialog, purpose }) = self.dialogs.front_mut() else {
            return false;
        };
        let current = match purpose {
            Purpose::EditHost { current, .. } => current,
            // Without a name, OK does nothing; a rename takes a name in its directory alone.
            Purpose::ConflictRename { question } => {
                if event != DialogEvent::Pressed(Button::Ok) {
                    return false;
                }
                let text = dialog.text();
                if text.trim().is_empty() {
                    return true;
                }
                let renames = matches!(
                    question.purpose,
                    Purpose::Conflict {
                        answer: Answer::Rename { .. },
                        ..
                    }
                );
                if !renames || names_an_entry(text) {
                    return false;
                }
                let name = cells::sanitize(text.as_bytes());
                self.show_error_over(&fl!("rename-invalid", name = name));
                return true;
            }
            Purpose::Checksum { expect: true, .. } if event == DialogEvent::Pressed(Button::Ok) => {
                let typed = dialog.text().trim();
                if typed.is_empty() || parse_expected(typed, Algorithm::Sha256).is_some() {
                    return false;
                }
                let text = cells::sanitize(typed.as_bytes());
                self.show_error_over(&fl!("checksum-expected-invalid", text = text));
                return true;
            }
            _ => return false,
        };
        match event {
            DialogEvent::Pressed(Button::UseCurrent) => {
                if let Some(current) = current {
                    dialog.set_text(HOST_START_DIR, current);
                }
                true
            }
            DialogEvent::Pressed(Button::Ok) => {
                let other_dir = dialog.text_of(HOST_OTHER_DIR).trim();
                if !other_dir.is_empty() && noc_config::local_dir(other_dir, &self.home).is_none() {
                    self.show_error_over(&fl!("host-other-dir-invalid"));
                    return true;
                }
                let run = dialog.text_of(HOST_RUN).trim();
                if let Err(reason) = (!run.is_empty()).then(|| run_of(run)).transpose() {
                    self.show_error_over(&fl!("host-run-invalid", reason = reason));
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Takes the end of an [`Effect::SaveHost`]: the host settings now, which the location menu
    /// shows at once, or why they could not be saved.
    pub(crate) fn host_saved(
        &mut self,
        name: String,
        host: Option<HostConfig>,
        result: Result<Arc<Hosts>, Reason>,
    ) -> Vec<Effect> {
        match result {
            Ok(hosts) => {
                self.host_settings = hosts;
                self.reload_menu()
            }
            Err(reason) => {
                let problem = Problem::new(fl!("host-save-error"), reason.sanitized());
                self.show_problem_way(&problem, Way::Again(Again::SaveHost { name, host }));
                Vec::new()
            }
        }
    }

    /// Sends `panel` to `destination`.
    pub(super) fn go(&mut self, panel: PanelId, destination: Destination) -> Vec<Effect> {
        let Some(shown) = self.panel_of_mut(panel) else {
            return Vec::new();
        };
        let request = shown.go(destination);
        self.open(panel, request)
    }

    /// Sends a new request of `panel`. Opening a host sends the panel that shows on the other
    /// side to the host's `other_dir`, if it has one.
    pub(super) fn open(&mut self, panel: PanelId, request: ListRequest) -> Vec<Effect> {
        let other_dir = match &request.location {
            Location::Remote { host, path } if path.as_bytes().is_empty() => self
                .host_settings
                .get(host)
                .and_then(HostConfig::other_dir)
                .and_then(|dir| noc_config::local_dir(dir, &self.home)),
            _ => None,
        };
        let mut effects = self.route(panel, request);
        if let Some(dir) = other_dir {
            let other = panel.side.other();
            let request = self
                .panel_mut(other)
                .go(Destination::to(Location::Local(dir)));
            effects.extend(self.route(self.shown(other), request));
        }
        effects
    }

    /// Sends a panel's request where it can be answered. A host that is not connected gets
    /// connected first; its panels' requests go out once it is. Opening a host with
    /// `remember_dir` resumes its last directory.
    pub(super) fn route(&mut self, panel: PanelId, mut request: ListRequest) -> Vec<Effect> {
        if let Location::Remote { host, path } = &request.location
            && path.as_bytes().is_empty()
            && self
                .host_settings
                .get(host)
                .is_some_and(HostConfig::remember_dir)
        {
            request.resume = self.last_dirs.get(host).cloned();
        }
        let Location::Remote { host, .. } = &request.location else {
            return vec![Effect::List {
                panel,
                request,
                host: None,
            }];
        };
        match self.hosts.get(host) {
            Some(Host::Connected { handle, .. }) => {
                let host = Some(handle.clone());
                vec![Effect::List {
                    panel,
                    request,
                    host,
                }]
            }
            Some(Host::Connecting { .. } | Host::Stalled { .. }) => Vec::new(),
            None => {
                let host = host.clone();
                self.connect(&host)
            }
        }
    }

    /// Starts connecting to `host`, which is not connected.
    pub(super) fn connect(&mut self, host: &str) -> Vec<Effect> {
        self.failed.remove(host);
        self.connections += 1;
        let connection = self.connections;
        let stop = CancellationToken::new();
        let state = Host::Connecting {
            connection,
            stop: stop.clone(),
            declined: Vec::new(),
        };
        self.hosts.insert(host.to_owned(), state);
        vec![Effect::Connect {
            host: host.to_owned(),
            connection,
            stop,
        }]
    }

    /// Stops what the panel on `side` waits for. A connection attempt stops for every panel.
    pub(super) fn cancel(&mut self, side: Side) {
        let Some(request) = self.panel(side).pending_request() else {
            return;
        };
        if let Location::Remote { host, .. } = &request.location
            && let Some(state @ Host::Connecting { .. }) = self.hosts.get(host)
        {
            state.stop();
            let host = host.clone();
            self.hosts.remove(&host);
            for panel in self.panels_mut() {
                if waits_for(panel, &host) {
                    panel.cancel();
                }
            }
        }
        self.panel_mut(side).cancel();
    }

    /// The host that F4 and F8 act on: the one under the cursor of the location menu, else the
    /// one the active panel shows.
    pub(super) fn chosen_host(&self) -> Option<String> {
        if let Some(menu) = &self.menu {
            return menu.chosen_host().map(str::to_owned);
        }
        match self.panel(self.active).location() {
            Location::Remote { host, .. } => Some(host.clone()),
            Location::Local(_) => None,
        }
    }

    /// Closes the connection to the host that F8 acts on, or stops connecting to it.
    pub(super) fn disconnect_chosen(&mut self) -> Vec<Effect> {
        match self.chosen_host() {
            Some(host) => self.disconnect(&host),
            None => Vec::new(),
        }
    }

    /// Closes the connection to `host`, or stops connecting to it. Panels on that host stay
    /// there, with their entries toned down, until they read their directories again.
    pub(super) fn disconnect(&mut self, host: &str) -> Vec<Effect> {
        self.forget_pending(host);
        let effects = self.end_jobs_on(host);
        let Some(state) = self.hosts.remove(host) else {
            return effects;
        };
        state.stop();
        for panel in self.panels_mut() {
            if matches!(state, Host::Connected { .. }) {
                panel.drop_host(host);
            } else if waits_for(panel, host) {
                panel.cancel();
            }
        }
        self.sync_connected();
        effects
    }

    /// Takes the address and the user `ssh -G` gave for a host.
    pub(crate) fn resolved(&mut self, host: String, address: String, user: &str) {
        self.users
            .insert(host.clone(), cells::sanitize(user.as_bytes()));
        self.addresses.insert(host, address);
    }

    /// What the location menu and the panels show for a host.
    pub(super) fn host_state(&self, host: &str) -> HostState {
        let status = match self.hosts.get(host) {
            Some(Host::Connecting { .. }) => HostStatus::Connecting,
            Some(Host::Connected { .. }) => HostStatus::Connected,
            Some(Host::Stalled { .. }) => HostStatus::Stalled,
            None if self.failed.contains(host) => HostStatus::Failed,
            None => HostStatus::Idle,
        };
        HostState {
            status,
            address: self.addresses.get(host).cloned(),
        }
    }

    /// Takes the result of an [`Effect::List`]. A failure asks what to do, unless the panel
    /// went up past a directory that is not there either, on its way to one that is.
    pub(crate) fn listed(
        &mut self,
        panel: PanelId,
        generation: u64,
        result: Result<Listed, ListError>,
    ) -> Vec<Effect> {
        self.sync_connected();
        let Some(tab) = self.tab_mut(panel) else {
            return Vec::new();
        };
        let before = tab.panel.location().clone();
        let failure = tab.panel.listed(generation, result);
        let location = tab.panel.location().clone();
        if location != before {
            tab.previous = Some(before);
            // A new visit, which counts in zoxide again; a jump counted already.
            let arriving = tab.arriving.take();
            tab.noted =
                matches!(&location, Location::Local(path) if arriving.as_ref() == Some(path));
            if let Location::Local(path) = &location
                && let Some(volume) = volume_of(&self.volumes, path)
            {
                self.recent.use_volume(&volume.mount_point);
            }
        }
        if let Location::Remote { host, path } = location
            && !path.as_bytes().is_empty()
        {
            self.last_dirs.insert(host, path);
        }
        match failure {
            Some(failure) => self.list_failed(panel, failure),
            None => Vec::new(),
        }
    }

    /// Reports that `panel` could not read a directory, in a dialog with the ways out: OK,
    /// Retry, Up where it leads elsewhere, Go to. A directory that is not there on the way up
    /// is passed by. Each panel has one such dialog, the last failure's.
    fn list_failed(&mut self, panel: PanelId, failure: Failure) -> Vec<Effect> {
        if failure.climbing
            && failure.error.kind == ListErrorKind::NotFound
            && let Some(parent) = failure.parent
        {
            return self.climb(panel, parent);
        }
        let Some(shown) = self.panel_of(panel) else {
            return Vec::new();
        };
        let up = failure
            .parent
            .as_ref()
            .is_some_and(|parent| parent.location() != shown.location());
        let mut buttons = vec![Button::Retry];
        if up {
            buttons.push(Button::Up);
        }
        buttons.extend([Button::GoTo, Button::Cancel]);
        // A panel left with nothing to show goes up; any other tries again.
        let first = if up && failure.stranded {
            Button::Up
        } else {
            Button::Retry
        };
        let default = buttons
            .iter()
            .position(|&button| button == first)
            .unwrap_or_default();
        let reason = Reason {
            text: failure.error.reason.clone(),
            code: failure.error.code,
        };
        let problem = Problem::new(fl!("list-failed"), reason).at(failure.shown.clone());
        let dialog = Dialog::failure(&problem, buttons, default);
        let open = Open {
            dialog,
            purpose: Purpose::ListFailed { panel, failure },
        };
        let earlier = self.dialogs.iter_mut().find(|open| {
            matches!(open.purpose, Purpose::ListFailed { panel: failed, .. } if failed == panel)
        });
        match earlier {
            Some(earlier) => *earlier = open,
            None => self.dialogs.push_back(open),
        }
        Vec::new()
    }

    /// Takes the button that closed the dialog of [`App::list_failed`].
    pub(super) fn list_failed_closed(
        &mut self,
        panel: PanelId,
        failure: Failure,
        event: DialogEvent,
    ) -> Vec<Effect> {
        match event {
            DialogEvent::Pressed(Button::Retry) => self.go(panel, failure.destination),
            DialogEvent::Pressed(Button::Up) => match failure.parent {
                Some(parent) => self.climb(panel, parent),
                None => Vec::new(),
            },
            DialogEvent::Pressed(Button::GoTo) => {
                self.ask_go_to_in(panel, &cd_text(failure.destination.location()));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Sends `panel` up to `destination`, and on up past directories that are not there.
    fn climb(&mut self, panel: PanelId, destination: Destination) -> Vec<Effect> {
        let Some(shown) = self.panel_of_mut(panel) else {
            return Vec::new();
        };
        let request = shown.climb(destination);
        self.open(panel, request)
    }

    /// Asks what to do instead of `action` in the panel on `side`, if it needs the host of the
    /// panel and the host is not connected. The entries the panel kept can be looked through.
    pub(super) fn ask_connect(&mut self, side: Side, action: Action) -> bool {
        let panel = self.panel(side);
        if works_unconnected(action) || (works_offline(action) && panel.has_entries()) {
            return false;
        }
        let Some(host) = panel.not_connected() else {
            return false;
        };
        let message = fl!(
            "reconnect-not-connected",
            host = cells::sanitize(host.as_bytes())
        );
        let buttons = vec![Button::Connect, Button::Local, Button::Cancel];
        let dialog = Dialog::question(&fl!("reconnect-title"), &message, buttons, 0, Tone::Error);
        let panel = self.shown(side);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::NotConnected { panel },
        });
        true
    }

    /// Takes the button that closed the dialog of [`App::ask_connect`].
    pub(super) fn not_connected_closed(
        &mut self,
        panel: PanelId,
        event: DialogEvent,
    ) -> Vec<Effect> {
        let Some(shown) = self.panel_of(panel) else {
            return Vec::new();
        };
        let destination = match event {
            DialogEvent::Pressed(Button::Connect) => shown.here(),
            DialogEvent::Pressed(Button::Local) => shown
                .last_local()
                .unwrap_or_else(|| Destination::to(Location::Local(self.home.clone()))),
            _ => return Vec::new(),
        };
        self.go(panel, destination)
    }

    /// Takes the handle of a host that [`Effect::Connect`] connected, and sends the requests
    /// that waited for it.
    pub(crate) fn connected(
        &mut self,
        host: &str,
        connection: u64,
        handle: HostHandle,
    ) -> Vec<Effect> {
        let Some(state) = self.hosts.get_mut(host) else {
            return Vec::new();
        };
        if !matches!(state, Host::Connecting { connection: current, .. } if *current == connection)
        {
            return Vec::new();
        }
        let (Host::Connecting { stop, .. } | Host::Connected { stop, .. }) = state else {
            return Vec::new();
        };
        if let Some(relink) = self.links.get(host) {
            relink.connected(handle.clone());
        }
        *state = Host::Connected {
            connection,
            stop: stop.clone(),
            handle,
        };
        self.recent.use_host(host);
        self.resume_jobs();
        let mut effects = Vec::new();
        for id in self.panel_ids() {
            // A tab that a workspace restored hidden asks once it shows.
            if let Some(tab) = self.tabs(id.side).get(id.tab)
                && tab.deferred.is_none()
                && let Some(request) = tab.panel.pending_request()
                && waits_for(&tab.panel, host)
            {
                effects.extend(self.route(id, request));
            }
        }
        effects.extend(self.do_pending(host));
        effects
    }

    /// Takes the end of an [`Effect::Connect`]: a failed attempt, or a closed connection.
    /// `reason` is `None` if it was asked to stop.
    pub(crate) fn closed(
        &mut self,
        host: &str,
        connection: u64,
        reason: Option<&str>,
    ) -> Vec<Effect> {
        match self.hosts.get(host) {
            Some(state @ (Host::Connecting { .. } | Host::Connected { .. }))
                if state.connection() == connection => {}
            _ => return Vec::new(),
        }
        let Some(state) = self.hosts.remove(host) else {
            return Vec::new();
        };
        if reason.is_some() {
            self.failed.insert(host.to_owned());
        }
        // Jobs wait for a host that was lost, and end with one that was asked to stop.
        let mut effects = Vec::new();
        if let Some(reason) = reason {
            self.hosts
                .insert(host.to_owned(), Host::Stalled { connection });
            self.hold_jobs(host);
            let lost = matches!(state, Host::Connected { .. });
            if lost {
                for panel in self.panels_mut() {
                    panel.lose_host(host);
                }
            }
            self.ask_reconnect(host, reason, lost);
        } else {
            effects = self.end_jobs_on(host);
            for panel in self.panels_mut() {
                if matches!(state, Host::Connected { .. }) {
                    panel.drop_host(host);
                } else if waits_for(panel, host) {
                    panel.cancel();
                }
            }
        }
        self.sync_connected();
        effects
    }

    /// Asks whether to connect to `host` again, after the attempt failed for `reason` or, if
    /// `lost`, the connection was lost.
    fn ask_reconnect(&mut self, host: &str, reason: &str, lost: bool) {
        let shown = cells::sanitize(host.as_bytes());
        let why = cells::sanitize(reason.as_bytes());
        let head = if lost {
            fl!("reconnect-lost", host = shown)
        } else {
            fl!("reconnect-failed", host = shown)
        };
        let buttons = vec![Button::Retry, Button::Cancel];
        let problem = Problem::new(head, why);
        let dialog = Dialog::failure_titled(&fl!("reconnect-title"), &problem, buttons, 0);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Reconnect {
                host: host.to_owned(),
                reason: reason.to_owned(),
            },
        });
    }

    /// Tries `host` again, for the panels that wait for it, after Retry.
    pub(super) fn reconnect(&mut self, host: &str) -> Vec<Effect> {
        if !matches!(self.hosts.get(host), Some(Host::Stalled { .. })) {
            return Vec::new();
        }
        self.hosts.remove(host);
        let effects = self.connect(host);
        self.sync_connected();
        effects
    }

    /// Stops waiting for `host`, which could not be reached for `reason`, after Cancel: the
    /// panels that were to open it stay where they were; those on it stay there with their
    /// entries toned down, until a key in one asks to connect. The dialog said why already.
    pub(super) fn give_up(&mut self, host: &str, reason: &str) -> Vec<Effect> {
        if !matches!(self.hosts.get(host), Some(Host::Stalled { .. })) {
            return Vec::new();
        }
        self.hosts.remove(host);
        self.forget_pending(host);
        let effects = self.end_jobs_on(host);
        for panel in self.panels_mut() {
            let Some(request) = panel.pending_request() else {
                continue;
            };
            if !waits_for(panel, host) {
                continue;
            }
            if is_on(panel.location(), host) {
                panel.drop_host(host);
            } else {
                panel.listed(request.generation, Err(ListError::other(reason.to_owned())));
            }
        }
        self.sync_connected();
        effects
    }

    /// Queues a dialog for a question from ssh.
    pub(crate) fn ask(&mut self, ask: Ask) {
        let dialog = Dialog::prompt(&ask.context, &ask.message, ask.kind);
        // ssh takes a password, a code, or a server's question declined for an empty answer,
        // which fails and is asked again; a host key or a confirmation declined ends there.
        let stops = matches!(ask.kind, PromptKind::Secret | PromptKind::Question);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Ssh {
                id: ask.id,
                host: ask.context,
                stops,
                reply: Some(ask.reply),
            },
        });
    }

    /// Queues a dialog for information from ssh, whose Cancel stops connecting: ssh waits for
    /// something done outside, such as a touch of a security key, which the dialog cannot do.
    pub(crate) fn notice(&mut self, id: u64, context: &str, message: &str) {
        let dialog = Dialog::notice(context, message);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Ssh {
                id,
                host: context.to_owned(),
                stops: true,
                reply: None,
            },
        });
    }

    /// Sends ssh the answer to a prompt from connecting to `host`, if it has a `reply`.
    /// Declining one that `stops` stops the attempt, and leaves the prompt unanswered until it
    /// ends.
    pub(super) fn answer_ssh(
        &mut self,
        host: &str,
        stops: bool,
        reply: Option<Reply>,
        answer: Option<SecretString>,
    ) {
        match self.hosts.get_mut(host) {
            Some(Host::Connecting { stop, declined, .. }) if stops && answer.is_none() => {
                stop.cancel();
                declined.extend(reply);
            }
            _ => {
                if let Some(reply) = reply {
                    reply.send(answer);
                }
            }
        }
    }

    /// Closes the dialog of a prompt or notice that ssh no longer waits for.
    pub(crate) fn prompt_closed(&mut self, id: u64) {
        self.dialogs
            .retain(|open| !matches!(open.purpose, Purpose::Ssh { id: shown, .. } if shown == id));
    }

    /// Stops every connection and connection attempt, for quitting.
    pub(crate) fn disconnect_all(&mut self) {
        for (_, state) in self.hosts.drain() {
            state.stop();
        }
        self.close_viewer();
    }
}

/// Whether `location` is on `host`.
fn is_on(location: &Location, host: &str) -> bool {
    matches!(location, Location::Remote { host: shown, .. } if shown == host)
}

/// Whether `action` does its work in a panel on a host that is not connected, without asking
/// to connect: it leaves the panel, goes elsewhere, or does not touch its entries. Reload
/// connects by itself.
fn works_unconnected(action: Action) -> bool {
    matches!(
        action,
        Action::Quit
            | Action::Help
            | Action::KeyHints
            | Action::Redraw
            | Action::UserScreen
            | Action::Jobs
            | Action::Cancel
            | Action::NormalMode
            | Action::SwitchPanel
            | Action::FocusLeft
            | Action::FocusRight
            | Action::SwapPanels
            | Action::NewTab
            | Action::CloseTab
            | Action::NextTab
            | Action::PrevTab
            | Action::TabList
            | Action::LocationMenu
            | Action::OtherPanel
            | Action::PullDown
            | Action::PullDownLast
            | Action::Reload
            | Action::Disconnect
            | Action::EditHost
            | Action::Jump
            | Action::GoToDir
            | Action::JumpBack
            | Action::JumpForward
            | Action::SaveWorkspace
            | Action::Workspaces
            | Action::ToggleHidden
    )
}

/// Whether `action` only looks through the entries a panel kept from a host that is not
/// connected: it moves the cursor or scrolls, searches, sorts, marks, or selects.
fn works_offline(action: Action) -> bool {
    matches!(
        action,
        Action::Up
            | Action::Down
            | Action::PageUp
            | Action::PageDown
            | Action::HalfPageUp
            | Action::HalfPageDown
            | Action::Home
            | Action::End
            | Action::ScrollLeft
            | Action::ScrollRight
            | Action::ScrollHalfLeft
            | Action::ScrollHalfRight
            | Action::ScrollStart
            | Action::ScrollEnd
            | Action::ShowFull
            | Action::QuickSearch
            | Action::QuickSearchBack
            | Action::NextMatch
            | Action::PrevMatch
            | Action::SearchMode
            | Action::SearchPlain
            | Action::SearchFzf
            | Action::SearchRegex
            | Action::SearchGlob
            | Action::CaseMode
            | Action::CaseSmart
            | Action::CaseIgnore
            | Action::CaseMatch
            | Action::Abort
            | Action::SortByName
            | Action::SortByExtension
            | Action::SortByTime
            | Action::SortBySize
            | Action::Mark
            | Action::MarkUp
            | Action::InvertMarks
            | Action::Select
            | Action::Unselect
            | Action::VisualMode
            | Action::VisualLineMode
            | Action::OtherEnd
            | Action::Reselect
    )
}
