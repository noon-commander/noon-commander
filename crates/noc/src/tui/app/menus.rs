//! The location menu, the pull-down menu, and the configuration.

use noc_config::{Config, Recent};
use noc_vfs::Location;

use crate::about::Tool;
use crate::i18n::fl;
use crate::tui::configuration::{ConfigEvent, Configuration};
use crate::tui::decor::Decor;
use crate::tui::describe::Reason;
use crate::tui::dialog::Problem;
use crate::tui::keymap::{Action, Context, Resolved, Typed};
use crate::tui::menu::{LocationMenu, MenuEvent};
use crate::tui::panel::Destination;
use crate::tui::places::Places;
use crate::tui::pulldown::{Command, PullDown, PullDownEvent, Status};
use crate::tui::search::Slot;
use crate::tui::theme::Theme;
use crate::tui::workspaces::{self};

use super::{Again, App, Effect, Side, Way, Whole, theme_of};

impl App {
    /// Opens the location menu for the panel on `side`, and lists what it offers.
    pub(super) fn open_menu(&mut self, side: Side) -> Vec<Effect> {
        self.menu_listings += 1;
        let generation = self.menu_listings;
        let current = self.panel(side).location().clone();
        let mut menu = LocationMenu::new(side, current, self.home.clone(), generation)
            .with_input(self.search_opens(Slot::LocationMenu))
            .recent(&self.recent)
            .lines_like(self.sizes.menu);
        menu.set_connected(self.hosts.keys().cloned().collect());
        self.menu = Some(menu);
        vec![Effect::ListPlaces { generation }]
    }

    /// Reads the volumes and hosts of the location menu again, if it is open.
    pub(super) fn reload_menu(&mut self) -> Vec<Effect> {
        let Some(menu) = &mut self.menu else {
            return Vec::new();
        };
        self.menu_listings += 1;
        let generation = self.menu_listings;
        menu.reload(generation);
        vec![Effect::ListPlaces { generation }]
    }

    /// Gives a key to the location menu: what it opens goes to its panel, and the keys stay with
    /// the active one, as after Ctrl+w f and Ctrl+w v.
    pub(super) fn handle_menu(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(menu) = &mut self.menu else {
            return Vec::new();
        };
        match menu.handle(input) {
            MenuEvent::Pending => Vec::new(),
            MenuEvent::Closed => {
                self.menu = None;
                Vec::new()
            }
            MenuEvent::Open(location) => {
                let side = menu.side();
                self.menu = None;
                self.go(self.shown(side), Destination::to(location))
            }
            MenuEvent::Disconnect(host) => self.disconnect(&host),
            MenuEvent::EditHost(host) => {
                self.ask_edit_host(host);
                Vec::new()
            }
            MenuEvent::Reload => self.reload_menu(),
        }
    }

    /// Opens the pull-down menu: the bar at the menu of the active panel, or with `last`, on
    /// the command that ran last.
    pub(super) fn open_pulldown(&mut self, last: bool) {
        let pulldown = PullDown::new(
            self.active,
            self.swapped,
            self.pulldown_last.filter(|_| last),
            &workspaces::names(&self.workspaces),
            self.theme.glyphs.marks,
            &|command| self.command_status(command),
        );
        self.pulldown = Some(pulldown);
    }

    /// Gives a key to the pull-down menu; a command closes it, then runs.
    pub(super) fn handle_pulldown(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(mut pulldown) = self.pulldown.take() else {
            return Vec::new();
        };
        let event = pulldown.handle(input, &|command| self.command_status(command));
        self.pulldown_event(pulldown, event)
    }

    /// Keeps the pull-down menu open after `event`, or closes it and runs its command.
    pub(super) fn pulldown_event(
        &mut self,
        pulldown: PullDown,
        event: PullDownEvent,
    ) -> Vec<Effect> {
        match event {
            PullDownEvent::Pending => {
                self.pulldown = Some(pulldown);
                Vec::new()
            }
            PullDownEvent::Closed => Vec::new(),
            PullDownEvent::Run(command) => {
                self.pulldown_last = Some(command);
                self.run(command)
            }
        }
    }

    /// Runs a command of the pull-down menu.
    pub(super) fn run(&mut self, command: Command) -> Vec<Effect> {
        match command {
            Command::Do(action) => self.handle(Resolved::Action(action)),
            Command::On(
                side,
                action @ (Action::NewTab | Action::CloseTab | Action::NextTab | Action::PrevTab),
            ) => self.tab_action(side, action),
            Command::On(side, action) if self.ask_connect(side, action) => Vec::new(),
            Command::On(side, action) => match self.panel_mut(side).handle(action) {
                Some(request) => self.open(self.shown(side), request),
                None => Vec::new(),
            },
            Command::Location(side) => self.open_menu(side),
            Command::Tabs(side) => {
                self.open_tab_menu(side);
                Vec::new()
            }
            Command::DisconnectPanel(side) => match self.host_of(side) {
                Some(host) => self.disconnect(&host),
                None => Vec::new(),
            },
            Command::Configuration => {
                let icons = self.decor.icons();
                let configuration =
                    Configuration::new(&self.config, &self.home, Theme::NAMES, icons);
                self.configuration = Some(configuration);
                Vec::new()
            }
            Command::Workspace(index) => match self.workspaces.workspaces.get(index) {
                Some(workspace) => {
                    let name = workspace.name.clone();
                    self.restore_workspace(&name)
                }
                None => Vec::new(),
            },
        }
    }

    /// Gives a key to the Configuration dialog, which applies each change as it is made.
    pub(super) fn handle_configuration(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(configuration) = &mut self.configuration else {
            return Vec::new();
        };
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        let event = configuration.handle_typed(typed, &mut self.registers);
        self.configuration_event(event)
    }

    /// Closes the Configuration dialog if `event` closes it, and applies the change it made.
    pub(super) fn configuration_event(&mut self, event: ConfigEvent) -> Vec<Effect> {
        if event.closed {
            self.configuration = None;
        }
        match event.change {
            Some((old, new)) => self.apply_config(old, new),
            None => Vec::new(),
        }
    }

    /// Uses the settings `new`, as `config.toml` writes them, from now on, but the language,
    /// which the next start reads; and writes to the config file those that differ from `old`.
    pub(super) fn apply_config(&mut self, old: Config, new: Config) -> Vec<Effect> {
        if old == new {
            return Vec::new();
        }
        let mut config = new.clone();
        config.expand_tilde(&self.home);
        let ui = &config.ui;
        // The dialog offers only the built-in themes, and the one that `ui.theme` names.
        self.theme = theme_of(ui, self.color_depth);
        self.decor = Decor::new(ui.icons);
        if ui.show_hidden != self.config.ui.show_hidden {
            let show = ui.show_hidden;
            for panel in self.panels_mut() {
                panel.set_show_hidden(show);
            }
        }
        if ui.natural_sort != self.config.ui.natural_sort {
            let natural = ui.natural_sort;
            for panel in self.panels_mut() {
                panel.set_natural_sort(natural);
            }
        }
        let formats = (old.ui.formats != new.ui.formats).then(|| new.ui.formats.clone());
        let log = (old.log != new.log).then(|| new.log.clone());
        self.copy_choices.atomic = config.transfer.atomic_upload;
        self.parallel_jobs = config.transfer.parallel_jobs.get();
        self.config = config.clone();
        // A program changed: what it is shows on the About page.
        let changed = [
            (Tool::Zoxide, old.zoxide.program != new.zoxide.program),
            (Tool::Ripgrep, old.search.program != new.search.program),
            (Tool::Fd, old.search.fd != new.search.fd),
        ];
        let mut effects = vec![Effect::SaveConfig {
            old: Box::new(old),
            new: Box::new(new),
            config: Box::new(config),
        }];
        effects.extend(
            changed
                .into_iter()
                .filter(|&(_, changed)| changed)
                .map(|(tool, _)| Effect::Detect(tool)),
        );
        effects.extend(formats.map(Effect::Formats));
        effects.extend(log.map(Effect::Log));
        effects.extend(self.start_queued());
        effects
    }

    /// Writes dates and numbers, and sorts names, in the formats in use from now on, which
    /// [`Effect::Formats`] made.
    pub(crate) fn formats_changed(&mut self) {
        for panel in self.panels_mut() {
            panel.sort_again();
        }
        self.about.report.formats = Some(crate::about::formats_text(&noc_locale::current()));
        self.refresh_help();
    }

    /// Takes the result of an [`Effect::SaveConfig`].
    pub(crate) fn config_saved(
        &mut self,
        old: Box<Config>,
        new: Box<Config>,
        result: Result<(), Reason>,
    ) {
        if let Err(reason) = result {
            let problem = Problem::new(fl!("config-save-error"), reason.sanitized());
            self.show_problem_way(&problem, Way::Again(Again::SaveConfig { old, new }));
        }
    }

    /// The host that the panel on `side` shows, if it is connected or connecting.
    fn host_of(&self, side: Side) -> Option<String> {
        match self.panel(side).location() {
            Location::Remote { host, .. } if self.hosts.contains_key(host) => Some(host.clone()),
            _ => None,
        }
    }

    /// The key that does `action` in the panel on `side`, for the pull-down menu: its own in the
    /// active panel, and Ctrl+x and it in the other.
    fn key_on(&self, side: Side, action: Action) -> Option<String> {
        if side == self.active {
            self.keymap.key(Context::Panel, action)
        } else {
            self.keymap.other_key(Context::Panel, action)
        }
    }

    /// Whether a command of the pull-down menu runs now, its mark, and its key.
    pub(super) fn command_status(&self, command: Command) -> Status {
        let context = Context::Panel;
        let active = self.panel(self.active);
        match command {
            Command::Do(action) => Status {
                enabled: match action {
                    Action::Checksum => !active.chosen().is_empty(),
                    Action::SystemOpen => {
                        matches!(active.location(), Location::Local(_))
                            && !active.chosen().is_empty()
                    }
                    Action::Disconnect => self
                        .chosen_host()
                        .is_some_and(|host| self.hosts.contains_key(&host)),
                    Action::Select
                    | Action::Unselect
                    | Action::InvertMarks
                    | Action::QuickSearch
                    | Action::FindFile
                    | Action::FindNames
                    | Action::Jump
                    | Action::GoToDir
                    | Action::SwapPanels
                    | Action::OtherPanelOpen
                    | Action::OtherPanelSync
                    | Action::Jobs
                    | Action::SaveWorkspace
                    | Action::Workspaces
                    | Action::ToggleHidden
                    | Action::ShowFull => true,
                    _ => self.supports(action),
                },
                checked: match action {
                    Action::ToggleHidden => self.config.ui.show_hidden,
                    Action::ShowFull => self.whole == Whole::Shown,
                    _ => false,
                },
                key: self.keymap.key(context, action),
            },
            Command::On(side, action) => Status {
                enabled: match action {
                    Action::CloseTab | Action::NextTab | Action::PrevTab => {
                        self.tabs(side).len() > 1
                    }
                    _ => true,
                },
                checked: self.panel(side).sort_action() == action,
                key: self.key_on(side, action),
            },
            Command::Location(side) => Status {
                enabled: true,
                checked: false,
                key: self.key_on(side, Action::LocationMenu),
            },
            Command::Tabs(side) => Status {
                enabled: true,
                checked: false,
                key: self.key_on(side, Action::TabList),
            },
            Command::DisconnectPanel(side) => Status {
                enabled: self.host_of(side).is_some(),
                ..Status::default()
            },
            Command::Configuration => Status {
                enabled: true,
                ..Status::default()
            },
            Command::Workspace(index) => Status {
                enabled: true,
                checked: self
                    .workspaces
                    .workspaces
                    .get(index)
                    .is_some_and(|workspace| self.workspace.as_ref() == Some(&workspace.name)),
                key: None,
            },
        }
    }

    /// Takes the result of an [`Effect::ListPlaces`]: the volumes, which tell which one a
    /// panel arrives on, and, for the location menu, the hosts.
    pub(crate) fn places(&mut self, generation: u64, result: Result<Places, String>) {
        if let Ok(places) = &result {
            self.volumes.clone_from(&places.volumes);
        }
        if let Some(menu) = &mut self.menu {
            menu.listed(generation, result);
            self.sizes.menu = menu.lines_listed().or(self.sizes.menu);
        }
    }

    /// Hosts that are connected, connecting, or stalled, which the location menu always shows.
    pub(super) fn sync_connected(&mut self) {
        if let Some(menu) = &mut self.menu {
            menu.set_connected(self.hosts.keys().cloned().collect());
        }
    }

    /// Takes the hosts and volumes as `recent.toml` held them; those used since come first.
    pub(crate) fn recent_loaded(&mut self, result: Result<Recent, String>) {
        let Ok(loaded) = result else {
            return;
        };
        let mut recent = loaded.clone();
        for host in self.recent.hosts.iter().rev() {
            recent.use_host(host);
        }
        for volume in self.recent.volumes.iter().rev() {
            recent.use_volume(volume);
        }
        self.recent = recent;
        self.recent_written = loaded;
    }

    /// The hosts and volumes used last, if they changed since the last call, for
    /// `recent.toml`.
    pub(crate) fn take_recent(&mut self) -> Option<Recent> {
        if self.recent == self.recent_written {
            return None;
        }
        self.recent_written.clone_from(&self.recent);
        Some(self.recent.clone())
    }
}
