//! The command line, its history, and its prompt.

use std::time::SystemTime;

use noc_config::{HistoryEntry, HostConfig};
use noc_vfs::Location;
use ratatui::style::Style;
use ratatui::text::Span;

use crate::i18n::fl;
use crate::tui::cells::{self, Align};
use crate::tui::command::{self, CommandEvent, CommandLine, Place, Run};
use crate::tui::describe::Reason;
use crate::tui::dialog::Problem;
use crate::tui::history::{HistoryEvent, HistoryWindow};
use crate::tui::key_hints::KeyHintsEvent;
use crate::tui::keymap::{Action, Resolved, Typed};
use crate::tui::machine::{self};
use crate::tui::panel::{home_text, location_text};
use crate::tui::search::Slot;

use super::{Again, App, Completes, Effect, Handover, HistoryChange, Host, PROMPT_DIR_WIDTH, Way};

impl App {
    /// Opens the command line for `action`: `!` for a shell command, `:` for commands of Noon
    /// Commander, `q :` with the window of the history; in a panel on a local directory or on
    /// one of a connected host.
    pub(super) fn open_command_line(&mut self, action: Action) {
        let kind = if action == Action::Command {
            command::Kind::Noc
        } else {
            command::Kind::Shell
        };
        let opens = match self.panel(self.active).location() {
            Location::Local(_) => true,
            Location::Remote { host, .. } => {
                matches!(self.hosts.get(host), Some(Host::Connected { .. }))
            }
        };
        if opens {
            let line = CommandLine::new(kind);
            if action == Action::CommandHistory {
                self.command_line = Some(line.for_history());
                self.open_history();
            } else {
                self.command_line = Some(line);
            }
        }
    }

    /// Gives a key to the command line: Enter hands its command to the event loop, to run in
    /// the active panel's directory, Tab completes, and the list of completions takes the keys
    /// while it shows.
    pub(super) fn handle_command_line(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(effects) = self.change_dir(Completes::CommandLine, input) {
            return effects;
        }
        if self.choose(Completes::CommandLine, input) {
            return Vec::new();
        }
        if input == Resolved::Action(Action::Complete) {
            return self.complete_command();
        }
        // Any other key makes a completion on its way too late.
        self.completion = None;
        let Some(line) = &mut self.command_line else {
            return Vec::new();
        };
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        let event = line.handle_typed(typed, &mut self.registers);
        match event {
            CommandEvent::None => Vec::new(),
            CommandEvent::Close => {
                self.command_line = None;
                Vec::new()
            }
            CommandEvent::Older | CommandEvent::Newer => {
                let host = match self.panel(self.active).location() {
                    Location::Remote { host, .. } => Some(host.as_str()),
                    Location::Local(_) => None,
                };
                let commands: Vec<&str> = self
                    .history
                    .iter()
                    .rev()
                    .filter(|entry| entry.host.as_deref() == host)
                    .map(|entry| entry.command.as_str())
                    .collect();
                if let Some(line) = &mut self.command_line {
                    line.browse(event == CommandEvent::Older, &commands);
                }
                Vec::new()
            }
            CommandEvent::History => {
                self.open_history();
                Vec::new()
            }
            CommandEvent::Unknown(text) => {
                let text = cells::sanitize(text.as_bytes());
                self.show_error(&fl!("command-unknown", command = text));
                Vec::new()
            }
            CommandEvent::Run(command) => {
                self.command_line = None;
                let dir = self.panel(self.active).location().clone();
                let prompt = cells::text_of(&self.shell_prompt(u16::MAX));
                self.run_in(&dir, command, prompt)
            }
        }
    }

    /// Runs `command` of the command line in `dir`, with the shell of the host if it is on one,
    /// after `prompt`; and keeps it in the history.
    pub(super) fn run_in(
        &mut self,
        dir: &Location,
        command: String,
        prompt: String,
    ) -> Vec<Effect> {
        let mut shell = &self.config.shell.run;
        let place = match dir {
            Location::Local(path) => Place::Local(path.clone()),
            Location::Remote { host, path } => {
                let Some(Host::Connected { handle, .. }) = self.hosts.get(host) else {
                    let again = Again::RunIn {
                        dir: dir.clone(),
                        command,
                        prompt,
                    };
                    self.show_not_connected(fl!("command-failed"), dir, again);
                    return Vec::new();
                };
                if let Some(run) = self.host_settings.get(host).and_then(HostConfig::run) {
                    shell = run;
                }
                Place::Remote {
                    handle: handle.clone(),
                    dir: path.clone(),
                }
            }
        };
        let shell = shell.clone();
        let mut effects = self.remember(dir, &command);
        self.handover = Some(Handover::Run(Run {
            place,
            shell,
            command,
            prompt,
            pause: self.config.shell.pause,
        }));
        effects.extend(self.note(dir));
        effects
    }

    /// Opens the window of the command history over the command line, if it is open.
    fn open_history(&mut self) {
        if self.command_line.is_none() {
            return;
        }
        let here = match self.panel(self.active).location() {
            Location::Remote { host, .. } => Some(host.clone()),
            Location::Local(_) => None,
        };
        let labels = self
            .history
            .iter()
            .filter_map(|entry| entry.host.as_deref())
            .filter_map(|host| {
                let label = self.host_settings.get(host).and_then(HostConfig::label)?;
                Some((host.to_owned(), label.to_owned()))
            })
            .collect();
        let input = self.search_opens(Slot::CommandHistory);
        self.history_window = Some(HistoryWindow::new(&self.history, here, labels, input));
    }

    /// Gives a key to the window of the command history: a command it takes goes on the command
    /// line, which shows whether it ran on another host; `d d` and F8 remove one.
    pub(super) fn handle_history(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.history_window else {
            return Vec::new();
        };
        match window.handle(input) {
            HistoryEvent::Pending => Vec::new(),
            HistoryEvent::Closed => {
                self.history_window = None;
                if self.command_line.as_ref().is_some_and(CommandLine::alone) {
                    self.command_line = None;
                }
                Vec::new()
            }
            HistoryEvent::Take { command, host } => {
                self.history_window = None;
                let here = match self.panel(self.active).location() {
                    Location::Remote { host, .. } => Some(host.as_str()),
                    Location::Local(_) => None,
                };
                let foreign = host.as_deref() != here;
                if let Some(line) = &mut self.command_line {
                    line.take(&command, foreign);
                }
                Vec::new()
            }
            HistoryEvent::Delete { command, host } => {
                self.history
                    .retain(|entry| !entry.is(host.as_deref(), &command));
                window.set_history(&self.history);
                vec![Effect::History(HistoryChange::Remove { host, command })]
            }
        }
    }

    /// Gives a key to the key hints with the keys: a command run from them runs where the keys
    /// went before they opened, with the count and the register typed before them.
    pub(super) fn handle_key_hints(&mut self, input: Resolved) -> Vec<Effect> {
        let Some(window) = &mut self.key_hints else {
            return Vec::new();
        };
        match window.handle(input) {
            KeyHintsEvent::Pending => Vec::new(),
            KeyHintsEvent::Closed => {
                self.key_hints = None;
                Vec::new()
            }
            KeyHintsEvent::Run(typed) => {
                self.key_hints = None;
                let extra = self.extra;
                let effects = self.typed(typed);
                self.extra = extra;
                effects
            }
        }
    }

    /// Keeps `command`, run in `dir`, as the newest in the history, here and in `history.toml`;
    /// not if it starts with a space, as bash's `HISTCONTROL=ignorespace`, or while
    /// `shell.history_size` is 0.
    fn remember(&mut self, dir: &Location, command: &str) -> Vec<Effect> {
        let size = self.config.shell.history_size;
        if size == 0 || command.starts_with(' ') {
            return Vec::new();
        }
        let (host, dir) = match dir {
            Location::Local(path) => (None, path.to_string_lossy().into_owned()),
            Location::Remote { host, path } => (
                Some(host.clone()),
                String::from_utf8_lossy(path.as_bytes()).into_owned(),
            ),
        };
        let time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
            });
        let entry = HistoryEntry {
            command: command.to_owned(),
            host,
            dir,
            time,
        };
        self.history
            .retain(|kept| !kept.is(entry.host.as_deref(), &entry.command));
        self.history.push(entry.clone());
        let excess = self.history.len().saturating_sub(size);
        self.history.drain(..excess);
        vec![Effect::History(HistoryChange::Add { entry, size })]
    }

    /// Takes the commands as `history.toml` holds them after a change, or why it could not
    /// be read or written.
    pub(crate) fn history_changed(
        &mut self,
        change: HistoryChange,
        result: Result<Vec<HistoryEntry>, Reason>,
    ) {
        match result {
            Ok(history) => {
                self.history = history;
                if let Some(window) = &mut self.history_window {
                    window.set_history(&self.history);
                }
            }
            Err(reason) => {
                let problem = Problem::new(fl!("history-error"), reason.sanitized());
                self.show_problem_way(&problem, Way::Again(Again::History(change)));
            }
        }
    }

    /// Takes the end of a shell command, or why the shell could not run; both panels read
    /// their directories again, as in mc, since the command may have changed anything.
    pub(crate) fn ran(&mut self, result: Result<(), (Problem, Run)>) -> Vec<Effect> {
        if let Err((problem, run)) = result {
            self.show_problem_way(&problem.sanitized(), Way::Again(Again::Run(run)));
        }
        let mut dirs: Vec<Location> = Vec::new();
        for side in [self.active, self.active.other()] {
            let dir = self.panel(side).location().clone();
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        dirs.iter().flat_map(|dir| self.reload(dir)).collect()
    }

    /// What starts the command line: `:` for commands of Noon Commander; for a shell command,
    /// the [`shell_prompt`](Self::shell_prompt).
    pub(super) fn command_prompt(&self, width: u16) -> Vec<Span<'static>> {
        let Some(line) = &self.command_line else {
            return Vec::new();
        };
        if line.kind() == command::Kind::Noc {
            return vec![Span::raw(":")];
        }
        self.shell_prompt(width)
    }

    /// The prompt of a shell command in the active panel, as a shell's: `user@host:dir`, with
    /// `~` for the home directory and the host's label if it has one, and `$`, or `#` where the
    /// user is root. In a third of `width` at most: the middle of the directory cut first, then
    /// the user left out, then the directory.
    pub(super) fn shell_prompt(&self, width: u16) -> Vec<Span<'static>> {
        let room = usize::from(width / 3).max(1);
        let dir = match self.panel(self.active).location() {
            Location::Local(path) => Some(home_text(path, &self.home)),
            Location::Remote { path, .. } => {
                (!path.as_bytes().is_empty()).then(|| cells::sanitize(path.as_bytes()))
            }
        };
        let sign = if self.active_root() { " # " } else { " $ " };
        let mut spans = match (self.active_who(), dir) {
            (Some(who), dir) => self.who_prompt(who, dir.as_deref(), room),
            (None, Some(dir)) => self.prompt_dir(&dir, room),
            (None, None) => Vec::new(),
        };
        spans.push(Span::raw(sign));
        spans
    }

    /// `user@host:dir` in `room` cells, as [`shell_prompt`](Self::shell_prompt) cuts it.
    fn who_prompt(
        &self,
        (user, host): (Option<String>, String),
        dir: Option<&str>,
        room: usize,
    ) -> Vec<Span<'static>> {
        let style = self.who_style();
        let whole = user.map(|user| format!("{user}@{host}"));
        for who in whole.iter().chain([&host]) {
            let width = cells::width(who);
            match dir {
                None if width <= room => return vec![Span::styled(who.clone(), style)],
                Some(dir) => {
                    let left = room.saturating_sub(width + 1);
                    if left > 0 && left >= cells::width(dir).min(PROMPT_DIR_WIDTH) {
                        let mut spans = vec![Span::styled(who.clone(), style), Span::raw(":")];
                        spans.extend(self.prompt_dir(dir, left));
                        return spans;
                    }
                }
                None => {}
            }
        }
        let styles = (style, self.theme.truncated);
        let room = room.min(cells::width(&host));
        cells::fit(&host, room, Align::Left, styles, self.theme.glyphs.marks)
    }

    /// `dir` in `room` cells, its middle cut if it is wider.
    fn prompt_dir(&self, dir: &str, room: usize) -> Vec<Span<'static>> {
        if cells::width(dir) > room {
            let styles = (Style::new(), self.theme.truncated);
            cells::fit(dir, room, Align::Left, styles, self.theme.glyphs.marks)
        } else {
            vec![Span::raw(dir.to_owned())]
        }
    }

    /// Who is where in the active panel: the user, if known, and the machine's name or the
    /// host's label or alias, terminal-safe; `None` in a local panel while the machine has no
    /// name.
    fn active_who(&self) -> Option<(Option<String>, String)> {
        match self.panel(self.active).location() {
            Location::Local(_) => {
                let host = self.machine.host.clone()?;
                Some((self.machine.user.clone(), host))
            }
            Location::Remote { host, .. } => Some((
                self.users.get(host).cloned(),
                cells::sanitize(self.host_label(host).as_bytes()),
            )),
        }
    }

    /// The user is root in the active panel, as [`root_at`](Self::root_at) tells.
    fn active_root(&self) -> bool {
        self.root_at(self.panel(self.active).location())
    }

    /// The user is root at `location`: a local one while Noon Commander runs as root, on a
    /// host as the user that `ssh -G` gave.
    pub(super) fn root_at(&self, location: &Location) -> bool {
        match location {
            Location::Local(_) => self.machine.root,
            Location::Remote { host, .. } => self
                .users
                .get(host)
                .is_some_and(|user| user == machine::ROOT),
        }
    }

    /// The style of `user@host` in the prompt and on the line of the mode.
    fn who_style(&self) -> Style {
        if self.active_root() {
            self.theme.root
        } else {
            Style::new()
        }
    }

    /// On the line of the mode, left of what is typed: the glyph of root while Noon Commander
    /// runs as root, whichever panel is active, and `user@host` of the active panel.
    pub(super) fn mode_who(&self) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        if self.machine.root {
            let glyph = self.theme.glyphs.root.to_string();
            spans.push(Span::styled(glyph, self.theme.root));
        }
        if let Some((user, host)) = self.active_who() {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            let who = user.map_or_else(|| host.clone(), |user| format!("{user}@{host}"));
            spans.push(Span::styled(who, self.who_style()));
        }
        spans
    }

    /// The title of the terminal's window or tab while `ui.terminal_title` is on: where the
    /// active panel is, as the prompt of a shell command shows it, after `[root]` while Noon
    /// Commander runs as root or the active panel is on a host as root.
    pub(crate) fn terminal_title(&self) -> Option<String> {
        if !self.config.ui.terminal_title {
            return None;
        }
        let place = self.active_place();
        Some(if self.machine.root || self.active_root() {
            fl!("terminal-title-root", place = place)
        } else {
            fl!("terminal-title", place = place)
        })
    }

    /// The directory of the active panel, with `~` for the home directory, or `host:path` with
    /// the host's label if it has one.
    fn active_place(&self) -> String {
        match self.panel(self.active).location() {
            Location::Local(path) => home_text(path, &self.home),
            Location::Remote { host, path } => location_text(&Location::Remote {
                host: self.host_label(host).to_owned(),
                path: path.clone(),
            }),
        }
    }

    /// The label of `host` from `hosts.toml`, or else its alias.
    fn host_label<'a>(&'a self, host: &'a str) -> &'a str {
        self.host_settings
            .get(host)
            .and_then(HostConfig::label)
            .unwrap_or(host)
    }
}
