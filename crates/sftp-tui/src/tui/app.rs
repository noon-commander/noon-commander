//! State and drawing of the whole screen.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::SystemTime;

use jiff::tz::TimeZone;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sftp_tui_vfs::Location;
use tokio_util::sync::CancellationToken;

use super::dialog::{Ask, Dialog, DialogEvent, Reply};
use super::keymap::{Action, Context, Keymap, Resolved};
use super::panel::{HostState, HostStatus, ListRequest, Listed, Panel};
use super::tasks::HostHandle;
use crate::i18n::fl;

/// One of the two panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

impl Side {
    fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

impl Side {
    const BOTH: [Self; 2] = [Self::Left, Self::Right];
}

/// Work the app asks the event loop to do.
#[derive(Debug)]
pub(crate) enum Effect {
    /// List a location and pass the result to [`App::listed`]. Remote locations go to the task
    /// of their host.
    List {
        side: Side,
        request: ListRequest,
        host: Option<HostHandle>,
    },
    /// Connect to a host and report to [`App::connected`] and [`App::closed`]; `stop` ends the
    /// attempt or the connection.
    Connect {
        host: String,
        connection: u64,
        stop: CancellationToken,
    },
}

/// A host that is connected or on its way. `connection` tells attempts apart, so that reports
/// about an earlier one are ignored.
#[derive(Debug)]
enum Host {
    Connecting {
        connection: u64,
        stop: CancellationToken,
    },
    Connected {
        connection: u64,
        stop: CancellationToken,
        handle: HostHandle,
    },
}

impl Host {
    fn connection(&self) -> u64 {
        match self {
            Self::Connecting { connection, .. } | Self::Connected { connection, .. } => *connection,
        }
    }

    fn stop(&self) {
        match self {
            Self::Connecting { stop, .. } | Self::Connected { stop, .. } => stop.cancel(),
        }
    }
}

/// A dialog on screen or waiting for its turn, and where its answer goes.
#[derive(Debug)]
struct Open {
    dialog: Dialog,
    /// `None` for a notice.
    reply: Option<Reply>,
}

/// What the TUI shows and whether it keeps running.
#[derive(Debug)]
pub(crate) struct App {
    left: Panel,
    right: Panel,
    active: Side,
    hosts: HashMap<String, Host>,
    connections: u64,
    /// Whether panels show names that start with a dot.
    show_hidden: bool,
    /// Hosts whose last attempt failed or whose connection was lost.
    failed: HashSet<String>,
    /// Addresses from `ssh -G` in this session.
    addresses: HashMap<String, String>,
    /// The first one is on screen and gets the keys; the others wait, so that a new prompt
    /// never takes the keys from a dialog in use.
    dialogs: VecDeque<Open>,
    quit: bool,
    redraw: bool,
}

impl App {
    /// Both panels on the local directory `start`, and the listings to request for them. From
    /// the virtual root, the local file system opens at `home`. `show_hidden` shows names that
    /// start with a dot.
    pub(crate) fn new(start: &Path, home: &Path, show_hidden: bool) -> (Self, Vec<Effect>) {
        let panel = || {
            let start = Location::Local(start.to_path_buf());
            Panel::new(start, home.to_path_buf(), show_hidden)
        };
        let (left, left_request) = panel();
        let (right, right_request) = panel();
        let mut app = Self {
            left,
            right,
            active: Side::Left,
            hosts: HashMap::new(),
            connections: 0,
            show_hidden,
            failed: HashSet::new(),
            addresses: HashMap::new(),
            dialogs: VecDeque::new(),
            quit: false,
            redraw: false,
        };
        let mut effects = app.route(Side::Left, left_request);
        effects.extend(app.route(Side::Right, right_request));
        (app, effects)
    }

    /// Whether the user asked to quit.
    pub(crate) fn quits(&self) -> bool {
        self.quit
    }

    /// Whether the whole screen must be drawn again; resets the request.
    pub(crate) fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// Where keys go now.
    pub(crate) fn context(&self) -> Context {
        if let Some(open) = self.dialogs.front() {
            open.dialog.context()
        } else if self.panel(self.active).shows_root() {
            Context::Root
        } else {
            Context::Panel
        }
    }

    /// Whether the app does something for `action` yet; the F-key bar shows only those.
    fn supports(action: Action) -> bool {
        matches!(
            action,
            Action::Quit | Action::Redraw | Action::Disconnect | Action::Cancel
        )
    }

    fn panel(&self, side: Side) -> &Panel {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn panel_mut(&mut self, side: Side) -> &mut Panel {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    pub(crate) fn handle(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(open) = self.dialogs.front_mut() {
            let answer = match open.dialog.handle(input) {
                DialogEvent::Pending => return Vec::new(),
                DialogEvent::Answer(text) => Some(text),
                DialogEvent::Decline | DialogEvent::Close => None,
            };
            if let Some(reply) = self.dialogs.pop_front().and_then(|open| open.reply) {
                reply.send(answer);
            }
            return Vec::new();
        }
        let Resolved::Action(action) = input else {
            return Vec::new();
        };
        match action {
            Action::Quit => self.quit = true,
            Action::Redraw => self.redraw = true,
            Action::SwitchPanel => self.active = self.active.other(),
            // As in mc, for both panels.
            Action::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                for side in Side::BOTH {
                    let show = self.show_hidden;
                    self.panel_mut(side).set_show_hidden(show);
                }
            }
            Action::Cancel => self.cancel(self.active),
            Action::Disconnect => return self.disconnect(self.active),
            _ => {
                let side = self.active;
                if let Some(request) = self.panel_mut(side).handle(action) {
                    return self.route(side, request);
                }
            }
        }
        Vec::new()
    }

    /// Sends a panel's request where it can be answered. A host that is not connected gets
    /// connected first; its panels' requests go out once it is.
    fn route(&mut self, side: Side, request: ListRequest) -> Vec<Effect> {
        let Location::Remote { host, .. } = &request.location else {
            return vec![Effect::List {
                side,
                request,
                host: None,
            }];
        };
        match self.hosts.get(host) {
            Some(Host::Connected { handle, .. }) => {
                let host = Some(handle.clone());
                vec![Effect::List {
                    side,
                    request,
                    host,
                }]
            }
            Some(Host::Connecting { .. }) => Vec::new(),
            None => {
                self.failed.remove(host);
                self.connections += 1;
                let connection = self.connections;
                let stop = CancellationToken::new();
                let host = host.clone();
                let state = Host::Connecting {
                    connection,
                    stop: stop.clone(),
                };
                self.hosts.insert(host.clone(), state);
                vec![Effect::Connect {
                    host,
                    connection,
                    stop,
                }]
            }
        }
    }

    /// Stops what the panel on `side` waits for. A connection attempt stops for every panel.
    fn cancel(&mut self, side: Side) {
        let Some(request) = self.panel(side).pending_request() else {
            return;
        };
        if let Location::Remote { host, .. } = &request.location
            && let Some(state @ Host::Connecting { .. }) = self.hosts.get(host)
        {
            state.stop();
            let host = host.clone();
            self.hosts.remove(&host);
            for side in Side::BOTH {
                if waits_for(self.panel(side), &host) {
                    self.panel_mut(side).cancel();
                }
            }
        }
        self.panel_mut(side).cancel();
    }

    /// Closes the connection to the host under the cursor of the panel on `side`, or stops
    /// connecting to it. Panels on that host go back to the root.
    fn disconnect(&mut self, side: Side) -> Vec<Effect> {
        let Some(host) = self.panel(side).host_under_cursor().map(str::to_owned) else {
            return Vec::new();
        };
        let Some(state) = self.hosts.remove(&host) else {
            return Vec::new();
        };
        state.stop();
        let mut effects = Vec::new();
        for side in Side::BOTH {
            let panel = self.panel_mut(side);
            if matches!(state, Host::Connecting { .. }) {
                if waits_for(panel, &host) {
                    panel.cancel();
                }
            } else if let Some(request) = panel.leave_host(&host, None) {
                effects.extend(self.route(side, request));
            }
        }
        effects
    }

    /// Takes the address `ssh -G` gave for a host.
    pub(crate) fn resolved(&mut self, host: String, address: String) {
        self.addresses.insert(host, address);
    }

    /// What the root shows for a host.
    fn host_state(&self, host: &str) -> HostState {
        let status = match self.hosts.get(host) {
            Some(Host::Connecting { .. }) => HostStatus::Connecting,
            Some(Host::Connected { .. }) => HostStatus::Connected,
            None if self.failed.contains(host) => HostStatus::Failed,
            None => HostStatus::Idle,
        };
        HostState {
            status,
            address: self.addresses.get(host).cloned(),
        }
    }

    /// Takes the result of an [`Effect::List`].
    pub(crate) fn listed(&mut self, side: Side, generation: u64, result: Result<Listed, String>) {
        self.panel_mut(side).listed(generation, result);
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
        let stop = match state {
            Host::Connecting { stop, .. } | Host::Connected { stop, .. } => stop.clone(),
        };
        *state = Host::Connected {
            connection,
            stop,
            handle,
        };
        let mut effects = Vec::new();
        for side in Side::BOTH {
            if let Some(request) = self.panel(side).pending_request()
                && waits_for(self.panel(side), host)
            {
                effects.extend(self.route(side, request));
            }
        }
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
        if self.hosts.get(host).map(Host::connection) != Some(connection) {
            return Vec::new();
        }
        let Some(state) = self.hosts.remove(host) else {
            return Vec::new();
        };
        if reason.is_some() {
            self.failed.insert(host.to_owned());
        }
        let mut effects = Vec::new();
        for side in Side::BOTH {
            let panel = self.panel_mut(side);
            match (&state, reason) {
                (Host::Connecting { .. }, Some(reason)) => {
                    if let Some(request) = panel.pending_request()
                        && waits_for(panel, host)
                    {
                        panel.listed(request.generation, Err(reason.to_owned()));
                    }
                }
                (Host::Connecting { .. }, None) => {
                    if waits_for(panel, host) {
                        panel.cancel();
                    }
                }
                (Host::Connected { .. }, _) => {
                    if let Some(request) = panel.leave_host(host, reason) {
                        effects.extend(self.route(side, request));
                    }
                }
            }
        }
        effects
    }

    /// Queues a dialog for a question from ssh.
    pub(crate) fn ask(&mut self, ask: Ask) {
        let dialog = Dialog::prompt(ask.id, &ask.context, &ask.message, ask.kind);
        self.dialogs.push_back(Open {
            dialog,
            reply: Some(ask.reply),
        });
    }

    /// Queues a dialog for information from ssh.
    pub(crate) fn notice(&mut self, id: u64, context: &str, message: &str) {
        let dialog = Dialog::notice(id, context, message);
        self.dialogs.push_back(Open {
            dialog,
            reply: None,
        });
    }

    /// Closes the dialog of a prompt or notice that ssh no longer waits for.
    pub(crate) fn prompt_closed(&mut self, id: u64) {
        self.dialogs.retain(|open| open.dialog.id() != id);
    }

    /// Stops every connection and connection attempt, for quitting.
    pub(crate) fn disconnect_all(&mut self) {
        for (_, state) in self.hosts.drain() {
            state.stop();
        }
    }

    /// Two panels side by side above the F-key bar.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        keymap: &Keymap,
        now: SystemTime,
        tz: &TimeZone,
    ) {
        let [panels, key_bar] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let [left, right] = Layout::horizontal([Constraint::Fill(1); 2]).areas(panels);
        let active = self.active;
        let states: HashMap<String, HostState> = self
            .hosts
            .keys()
            .chain(&self.failed)
            .chain(self.addresses.keys())
            .map(|host| (host.clone(), self.host_state(host)))
            .collect();
        let hosts = |host: &str| states.get(host).cloned().unwrap_or_default();
        self.left
            .render(frame, left, active == Side::Left, &hosts, now, tz);
        self.right
            .render(frame, right, active == Side::Right, &hosts, now, tz);
        self.render_fkeys(frame, key_bar, keymap);
        if let Some(open) = self.dialogs.front() {
            open.dialog.render(frame, panels);
        }
    }

    /// The F-key bar: ten equal slots, each the key number and the label of its action.
    fn render_fkeys(&self, frame: &mut Frame<'_>, area: Rect, keymap: &Keymap) {
        let slots = Layout::horizontal([Constraint::Fill(1); 10]).split(area);
        let actions = keymap.fkeys(self.context());
        for (number, (slot, action)) in (1..).zip(slots.iter().zip(actions)) {
            let label = action
                .filter(|action| Self::supports(*action))
                .and_then(fkey_label)
                .unwrap_or_default();
            let line = Line::from(vec![
                Span::raw(number.to_string()),
                Span::styled(label, Style::new().reversed()),
            ]);
            frame.render_widget(line, *slot);
        }
    }
}

/// Whether `panel` waits for a listing on `host`.
fn waits_for(panel: &Panel, host: &str) -> bool {
    panel.pending_request().is_some_and(
        |request| matches!(&request.location, Location::Remote { host: wanted, .. } if wanted == host),
    )
}

/// The label of an action in the F-key bar.
fn fkey_label(action: Action) -> Option<String> {
    match action {
        Action::Quit => Some(fl!("fkey-quit")),
        Action::Cancel => Some(fl!("fkey-cancel")),
        Action::Disconnect => Some(fl!("fkey-disconnect")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    use std::sync::mpsc;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use secrecy::ExposeSecret as _;
    use sftp_tui_ssh::askpass::PromptKind;
    use sftp_tui_vfs::{DirEntry, FileKind, Metadata, RemotePath};

    use super::super::panel::Listing;
    use super::super::root::RootHost;
    use super::*;

    fn dir(name: &str) -> DirEntry {
        DirEntry {
            name: name.as_bytes().to_vec(),
            metadata: Metadata {
                kind: FileKind::Dir,
                size: Some(4096),
                permissions: Some(0o755),
                modified: Some(UNIX_EPOCH + Duration::from_secs(1_699_990_000)),
                uid: None,
                gid: None,
            },
            target_kind: None,
        }
    }

    fn local(path: &str) -> Location {
        Location::Local(PathBuf::from(path))
    }

    fn remote(host: &str, path: &str) -> Location {
        Location::Remote {
            host: host.to_owned(),
            path: RemotePath::from(path),
        }
    }

    fn action(action: Action) -> Resolved {
        Resolved::Action(action)
    }

    /// The only effect.
    fn one(effects: Vec<Effect>) -> Effect {
        let [effect] = <[Effect; 1]>::try_from(effects).unwrap();
        effect
    }

    /// Answers every listing in `effects` with `listing`, from where it was asked for.
    fn answer(app: &mut App, effects: Vec<Effect>, listing: &Listing) {
        for effect in effects {
            let Effect::List { side, request, .. } = effect else {
                panic!("expected a listing, got {effect:?}");
            };
            let location = request.location;
            let listing = listing.clone();
            app.listed(side, request.generation, Ok(Listed { location, listing }));
        }
    }

    /// An app on `/srv` whose first listings arrived.
    fn loaded() -> App {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), true);
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir("left"), dir("right")]),
        );
        app
    }

    /// An app with both panels on the virtual root, which lists `web` and `db`.
    fn at_root() -> App {
        let (mut app, effects) = App::new(Path::new("/"), Path::new("/home/me"), true);
        answer(&mut app, effects, &Listing::Dir(Vec::new()));
        let hosts = ["web", "db"].map(|alias| RootHost {
            alias: alias.to_owned(),
            label: None,
            address: None,
        });
        for side in Side::BOTH {
            app.active = side;
            let effects = app.handle(action(Action::Parent));
            answer(&mut app, effects, &Listing::Root(hosts.to_vec()));
        }
        app.active = Side::Left;
        app
    }

    /// Opens the host `rows` rows below `[Local]` in the panel on `side`.
    fn enter_host(app: &mut App, side: Side, rows: usize) -> Vec<Effect> {
        app.active = side;
        for _ in 0..rows {
            app.handle(action(Action::Down));
        }
        app.handle(action(Action::Enter))
    }

    /// Wide enough for whole status lines.
    fn screen(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(160, 8)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, &Keymap::mc(), now, &TimeZone::UTC))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn lists_both_panels_at_start() {
        let (_, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), true);
        let sides: Vec<Side> = effects
            .iter()
            .map(|effect| match effect {
                Effect::List {
                    side, host: None, ..
                } => *side,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(sides, [Side::Left, Side::Right]);
    }

    #[test]
    fn keys_go_to_the_active_panel_and_tab_switches_it() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let Effect::List { side, request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        assert_eq!((side, request.location), (Side::Left, local("/srv/left")));

        assert!(app.handle(action(Action::SwitchPanel)).is_empty());
        app.handle(action(Action::End));
        let Effect::List { side, request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        assert_eq!((side, request.location), (Side::Right, local("/srv/right")));

        app.handle(action(Action::SwitchPanel));
        assert_eq!(app.active, Side::Left);
    }

    #[test]
    fn replies_reach_their_own_panel() {
        let mut app = loaded();
        app.handle(action(Action::Down));
        let Effect::List { side, request, .. } = one(app.handle(action(Action::Enter))) else {
            panic!("expected a listing");
        };
        let reply = |listing| {
            let location = request.location.clone();
            Ok(Listed { location, listing })
        };
        app.listed(
            side.other(),
            request.generation,
            reply(Listing::Dir(Vec::new())),
        );
        app.listed(
            side,
            request.generation,
            reply(Listing::Dir(vec![dir("deeper")])),
        );
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        terminal
            .draw(|frame| app.render(frame, &Keymap::mc(), now, &TimeZone::UTC))
            .unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn a_host_connects_once_and_then_lists_for_every_panel_that_waits() {
        let mut app = at_root();
        let Effect::Connect {
            host, connection, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        assert_eq!(host, "web");
        assert!(
            enter_host(&mut app, Side::Right, 1).is_empty(),
            "already on its way"
        );
        let text = screen(&mut app);
        assert_eq!(text.matches("Connecting to web…").count(), 2, "{text}");

        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        let sides: Vec<_> = effects
            .iter()
            .map(|effect| match effect {
                Effect::List {
                    side,
                    request,
                    host: Some(_),
                } => {
                    assert_eq!(request.location, remote("web", ""));
                    *side
                }
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(sides, Side::BOTH);
        assert!(!screen(&mut app).contains("Connecting"));

        // A connected host lists at once.
        let effects = enter_host(&mut app, Side::Left, 0);
        assert!(matches!(&effects[..], [Effect::List { host: Some(_), .. }]));
    }

    #[test]
    fn a_failed_connection_is_reported_and_can_be_tried_again() {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
            panic!("expected a connection");
        };
        let refused = "deploy@db: Permission denied (publickey).";
        assert!(app.closed("db", connection, Some(refused)).is_empty());
        let text = screen(&mut app);
        assert!(
            text.contains("Cannot open db: deploy@db: Permission denied (publickey)."),
            "{text}"
        );

        let Effect::Connect {
            connection: again, ..
        } = one(enter_host(&mut app, Side::Left, 0))
        else {
            panic!("expected a new connection");
        };
        assert_ne!(again, connection);
    }

    #[test]
    fn cancel_stops_a_connection_attempt_for_every_panel() {
        let mut app = at_root();
        let Effect::Connect {
            connection, stop, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        enter_host(&mut app, Side::Right, 1);
        app.handle(action(Action::Cancel));
        assert!(stop.is_cancelled());
        assert!(!screen(&mut app).contains("Connecting"));
        for side in Side::BOTH {
            assert_eq!(app.panel(side).pending_request(), None);
        }

        // Reports of the stopped attempt change nothing.
        let (handle, _requests) = HostHandle::channel();
        assert!(app.connected("web", connection, handle).is_empty());
        assert!(app.closed("web", connection, None).is_empty());
        assert!(app.hosts.is_empty());
        // Nothing to stop does nothing.
        assert!(app.handle(action(Action::Cancel)).is_empty());
    }

    #[test]
    fn a_lost_connection_sends_its_panels_back_to_the_root() {
        let mut app = at_root();
        let Effect::Connect {
            connection, stop, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
        assert!(screen(&mut app).contains("www"));

        let effects = app.closed("web", connection, Some("Broken pipe"));
        let [Effect::List { side, request, .. }] = &effects[..] else {
            panic!("expected the root, got {effects:?}");
        };
        assert_eq!((*side, &request.location), (Side::Left, &Location::Root));
        let text = screen(&mut app);
        assert!(
            text.contains("Lost the connection to web: Broken pipe"),
            "{text}"
        );
        assert!(!stop.is_cancelled(), "it ended on its own");
    }

    #[test]
    fn the_root_disconnects_the_host_under_the_cursor() {
        let mut app = at_root();
        assert_eq!(app.context(), Context::Root);
        assert!(screen(&mut app).contains("8Disconn"));
        let Effect::Connect {
            connection, stop, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        let (handle, _requests) = HostHandle::channel();
        let effects = app.connected("web", connection, handle);
        answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
        assert_eq!(app.context(), Context::Panel, "the left panel is on web");
        assert!(!screen(&mut app).contains("Disconn"));
        assert_eq!(app.host_state("web").status, HostStatus::Connected);

        // From the other panel's root.
        app.active = Side::Right;
        app.handle(action(Action::Down));
        let Effect::List {
            side,
            request,
            host: None,
        } = one(app.handle(action(Action::Disconnect)))
        else {
            panic!("expected the root for the left panel");
        };
        assert_eq!((side, &request.location), (Side::Left, &Location::Root));
        assert!(stop.is_cancelled());
        assert_eq!(app.host_state("web").status, HostStatus::Idle);
        assert!(
            app.closed("web", connection, None).is_empty(),
            "already gone"
        );
        assert!(
            !screen(&mut app).contains("Lost"),
            "asked for, so no reason"
        );
        // Nothing to close, or not a host.
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        app.handle(action(Action::Home));
        assert!(app.handle(action(Action::Disconnect)).is_empty());
    }

    #[test]
    fn disconnect_stops_a_connection_attempt() {
        let mut app = at_root();
        let Effect::Connect { stop, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
            panic!("expected a connection");
        };
        assert_eq!(app.host_state("db").status, HostStatus::Connecting);
        assert!(app.handle(action(Action::Disconnect)).is_empty());
        assert!(stop.is_cancelled());
        assert_eq!(app.panel(Side::Left).pending_request(), None);
        assert_eq!(app.host_state("db").status, HostStatus::Idle);
    }

    #[test]
    fn the_root_shows_failures_and_fresh_addresses() {
        let mut app = at_root();
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        app.resolved("web".to_owned(), "deploy@10.0.0.9".to_owned());
        app.closed("web", connection, Some("Connection refused"));
        assert_eq!(
            app.host_state("web"),
            HostState {
                status: HostStatus::Failed,
                address: Some("deploy@10.0.0.9".to_owned()),
            }
        );
        let text = screen(&mut app);
        assert!(text.contains("✗ web"), "{text}");
        assert!(text.contains("deploy@10.0.0.9"), "{text}");

        // A new attempt clears the failure.
        enter_host(&mut app, Side::Left, 0);
        assert_eq!(app.host_state("web").status, HostStatus::Connecting);
    }

    /// A question from ssh, and where its answer arrives.
    fn ask(id: u64, kind: PromptKind, message: &str) -> (Ask, mpsc::Receiver<Option<String>>) {
        let (sender, answers) = mpsc::channel();
        let reply = Reply::new(move |answer| {
            let answer = answer.map(|text| text.expose_secret().to_owned());
            let _ = sender.send(answer);
        });
        let ask = Ask {
            id,
            context: "web".to_owned(),
            message: message.to_owned(),
            kind,
            reply,
        };
        (ask, answers)
    }

    #[test]
    fn prompts_wait_their_turn_and_take_the_keys() {
        let mut app = at_root();
        let (password, password_answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
        let (host_key, host_key_answer) = ask(2, PromptKind::HostKey, "Continue (yes/no)? ");
        app.ask(password);
        app.ask(host_key);
        assert_eq!(app.context(), Context::DialogInput);
        let text = screen(&mut app);
        assert!(text.contains("deploy@web's password:"), "{text}");
        assert!(!text.contains("Continue"), "the second waits: {text}");
        assert!(text.contains("10Cancel"), "{text}");

        for c in "s3cret".chars() {
            app.handle(Resolved::Insert(c));
        }
        assert!(screen(&mut app).contains("[******"));
        app.handle(action(Action::Confirm));
        assert_eq!(password_answer.try_recv(), Ok(Some("s3cret".to_owned())));

        assert_eq!(app.context(), Context::Dialog);
        assert!(screen(&mut app).contains("Continue (yes/no)?"));
        // Panel keys do nothing while a dialog is open; F10 closes the dialog, not the app.
        app.handle(action(Action::Down));
        app.handle(action(Action::Cancel));
        assert!(!app.quits());
        assert_eq!(host_key_answer.try_recv(), Ok(None));
        assert_eq!(app.context(), Context::Root);
        assert_eq!(
            app.panel(Side::Left).host_under_cursor(),
            None,
            "still on [Local]"
        );
    }

    #[test]
    fn dialogs_close_when_ssh_stops_waiting() {
        let mut app = at_root();
        let (first, first_answer) = ask(1, PromptKind::Secret, "first");
        let (second, second_answer) = ask(2, PromptKind::Confirm, "second");
        app.ask(first);
        app.ask(second);
        app.notice(3, "web", "Confirm user presence for key ED25519-SK");
        app.prompt_closed(2);
        app.prompt_closed(1);
        assert!(first_answer.try_recv().is_err(), "nobody to answer");
        assert!(second_answer.try_recv().is_err());
        assert!(screen(&mut app).contains("Confirm user presence"));
        app.handle(action(Action::Confirm));
        assert_eq!(app.context(), Context::Root, "OK dismisses a notice");

        app.notice(4, "web", "Touch your key");
        app.prompt_closed(4);
        assert_eq!(app.context(), Context::Root);
    }

    #[test]
    fn quitting_stops_every_connection() {
        let mut app = at_root();
        let stops: Vec<CancellationToken> = [(Side::Left, 1), (Side::Right, 2)]
            .into_iter()
            .map(|(side, rows)| match &enter_host(&mut app, side, rows)[..] {
                [Effect::Connect { stop, .. }] => stop.clone(),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        app.disconnect_all();
        assert!(stops.iter().all(CancellationToken::is_cancelled));
        assert!(app.hosts.is_empty());
    }

    #[test]
    fn hidden_files_switch_in_both_panels() {
        let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), false);
        answer(
            &mut app,
            effects,
            &Listing::Dir(vec![dir(".git"), dir("src")]),
        );
        assert!(!screen(&mut app).contains(".git"));
        app.handle(action(Action::ToggleHidden));
        assert_eq!(
            screen(&mut app).matches(".git").count(),
            2,
            "in both panels"
        );
        app.handle(action(Action::ToggleHidden));
        assert!(!screen(&mut app).contains(".git"));
    }

    #[test]
    fn handles_quit_redraw_and_text() {
        let mut app = loaded();
        assert!(app.handle(Resolved::Insert('q')).is_empty());
        assert!(!app.quits());
        assert!(!app.take_redraw());
        app.handle(action(Action::Redraw));
        assert!(app.take_redraw());
        assert!(!app.take_redraw(), "a redraw is requested once");
        app.handle(action(Action::Quit));
        assert!(app.quits());
    }

    #[test]
    fn every_supported_f_key_action_has_a_label() {
        let keymap = Keymap::mc();
        for action in keymap.fkeys(Context::Panel).into_iter().flatten() {
            if App::supports(action) {
                assert!(fkey_label(action).is_some(), "{action:?}");
            }
        }
    }
}
