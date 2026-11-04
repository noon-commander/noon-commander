//! Completing paths, hosts, and commands in fields.

use std::path::PathBuf;

use noc_vfs::Location;

use crate::tui::cd;
use crate::tui::complete::{self, Candidate, Choices, Kind, Offer, Outcome, Quote};
use crate::tui::panel::{Destination, Panel};

use super::{App, Completes, Completing, Effect, FieldCompletion, FieldHosts, Host, Purpose};

impl App {
    /// What the path field of a dialog for `purpose` completes: from which directory, what it
    /// takes, and which hosts; and where `:` starts, for Go to directory only.
    fn completion_of(&self, purpose: &Purpose) -> Option<FieldCompletion> {
        let (panel, offer, hosts) = match purpose {
            Purpose::GoToDir { panel } => (*panel, Offer::Dirs, FieldHosts::All),
            Purpose::Transfer { panel, .. } => (*panel, Offer::Any, FieldHosts::Connected),
            Purpose::Mkdir { panel } => (*panel, Offer::Dirs, FieldHosts::None),
            Purpose::SaveFile { dir, .. } => {
                return Some(FieldCompletion {
                    here: dir.clone(),
                    last_local: None,
                    offer: Offer::Any,
                    hosts: FieldHosts::None,
                });
            }
            _ => return None,
        };
        let shown = self.panel_of(panel)?;
        let last_local =
            matches!(purpose, Purpose::GoToDir { .. }).then(|| self.last_local(shown).0);
        Some(FieldCompletion {
            here: shown.location().clone(),
            last_local,
            offer,
            hosts,
        })
    }

    /// Where `:` leads in Go to directory of `panel`: its directory if it is local, else the
    /// last local one it showed, else the home directory; as a path, and as where to go with
    /// the cursor where it was.
    pub(super) fn last_local(&self, panel: &Panel) -> (PathBuf, Destination) {
        let destination = match panel.location() {
            Location::Local(_) => panel.here(),
            Location::Remote { .. } => panel
                .last_local()
                .unwrap_or_else(|| Destination::to(Location::Local(self.home.clone()))),
        };
        let dir = match destination.location() {
            Location::Local(dir) => dir.clone(),
            Location::Remote { .. } => self.home.clone(),
        };
        (dir, destination)
    }

    /// Completes the path before the cursor in the field of the dialog in front: lists the
    /// directory it is in, read as that dialog reads its text, and the hosts it may name.
    pub(super) fn complete(&mut self) -> Vec<Effect> {
        let Some(open) = self.dialogs.front() else {
            return Vec::new();
        };
        if !open.dialog.completes() {
            return Vec::new();
        }
        let Some(FieldCompletion {
            here,
            last_local,
            offer,
            hosts,
        }) = self.completion_of(&open.purpose)
        else {
            return Vec::new();
        };
        let Some(open) = self.dialogs.front_mut() else {
            return Vec::new();
        };
        let before = open.dialog.around_cursor().0.to_owned();
        if before == "~" {
            open.dialog.replace_before_cursor("~/");
            return Vec::new();
        }
        let connected: Vec<String> = self
            .hosts
            .iter()
            .filter(|(_, state)| matches!(state, Host::Connected { .. }))
            .map(|(host, _)| host.clone())
            .collect();
        let is_host = |host: &str| match hosts {
            FieldHosts::All => true,
            FieldHosts::Connected => connected.iter().any(|known| known == host),
            FieldHosts::None => false,
        };
        let split = complete::split(&before, last_local.is_some(), is_host);
        let word = match before.split_once(':') {
            Some(("", _)) if last_local.is_some() => 1,
            Some((host, _)) if !host.is_empty() && !host.contains('/') && is_host(host) => {
                host.len() + 1
            }
            _ => 0,
        };
        let dir = cd::directory(
            &split.dir,
            &here,
            &self.home,
            last_local.as_deref(),
            is_host,
        );
        // A host that is not connected has nothing to list.
        let host = self.handle_for(&dir).unwrap_or(None);
        let list_hosts = split.hosts && hosts == FieldHosts::All;
        let generation = self.start_completion(Completing {
            generation: 0,
            target: Completes::Field,
            before,
            keep: split.dir,
            word,
            prefix: split.prefix,
            offer,
            quote: Quote::Plain,
            hosts: if split.hosts { hosts } else { FieldHosts::None },
            connected,
            choices: None,
        });
        vec![Effect::ListNames {
            generation,
            dir: Some(dir),
            host,
            hosts: list_hosts,
        }]
    }

    /// Waits for the listing of `completing`, under a new generation, which it returns.
    fn start_completion(&mut self, completing: Completing) -> u64 {
        self.completions += 1;
        let generation = self.completions;
        self.completion = Some(Completing {
            generation,
            ..completing
        });
        generation
    }

    /// Completes the word before the cursor on the command line, as vim's `:!` does: in a
    /// local panel, the first word as a command of `$PATH`, unless it names a directory, and
    /// the others as files from the panel's directory, where the command runs; on a host, every
    /// word as files there, through its session. Names go in escaped for the shell.
    pub(super) fn complete_command(&mut self) -> Vec<Effect> {
        let Some(line) = &mut self.command_line else {
            return Vec::new();
        };
        let Some((lead, command)) = line.command_before_cursor() else {
            return Vec::new();
        };
        let word = complete::word(command);
        let keep = format!("{lead}{}", word.keep);
        let start = lead.len() + word.start;
        let before = line.before_cursor().to_owned();
        if word.dir.is_empty() && &before[keep.len()..] == "~" {
            line.replace_before_cursor(&format!("{keep}~/"));
            return Vec::new();
        }
        let here = self.panel(self.active).location().clone();
        let commands = word.first && word.dir.is_empty() && matches!(here, Location::Local(_));
        let generation = self.start_completion(Completing {
            generation: 0,
            target: Completes::CommandLine,
            before,
            keep,
            word: start,
            prefix: word.prefix.clone(),
            offer: Offer::Any,
            quote: Quote::Shell,
            hosts: FieldHosts::None,
            connected: Vec::new(),
            choices: None,
        });
        if commands {
            return vec![Effect::ListCommands {
                generation,
                dirs: self.command_dirs.clone(),
                prefix: word.prefix,
                with_dirs: false,
            }];
        }
        let dir = cd::directory(&word.dir, &here, &self.home, None, |_| false);
        // A command named by its path, as vim's `:!` completes it: directories and programs.
        if word.first
            && let Location::Local(path) = &dir
        {
            return vec![Effect::ListCommands {
                generation,
                dirs: vec![path.clone()],
                prefix: word.prefix,
                with_dirs: true,
            }];
        }
        // The line opens on a host only while it is connected.
        let host = self.handle_for(&dir).unwrap_or(None);
        vec![Effect::ListNames {
            generation,
            dir: Some(dir),
            host,
            hosts: false,
        }]
    }

    /// Takes the result of an [`Effect::ListNames`] or [`Effect::ListCommands`]: one match
    /// goes in the field; of several, the first goes in, and a list of them shows.
    pub(crate) fn names(
        &mut self,
        generation: u64,
        entries: Result<Vec<(Vec<u8>, bool)>, String>,
        hosts: &[String],
    ) {
        let Some(completing) = &self.completion else {
            return;
        };
        if completing.generation != generation || completing.choices.is_some() {
            return;
        }
        if self.before_cursor(completing.target) != Some(completing.before.as_str()) {
            self.completion = None;
            return;
        }
        let entries = entries.unwrap_or_else(|reason| {
            tracing::debug!(%reason, "cannot list names to complete");
            Vec::new()
        });
        let mut candidates = complete::from_names(entries, completing.offer);
        let aliases = match completing.hosts {
            FieldHosts::All => hosts,
            FieldHosts::Connected => &completing.connected[..],
            FieldHosts::None => &[],
        };
        candidates.extend(aliases.iter().map(|alias| Candidate {
            name: alias.clone(),
            kind: Kind::Host,
        }));
        let (target, quote) = (completing.target, completing.quote);
        let keep = completing.keep.clone();
        let text = match complete::complete(&completing.prefix, candidates) {
            Outcome::Nothing => None,
            Outcome::One(candidate) => Some(candidate.typed(quote)),
            Outcome::List(items) => {
                let choices = Choices::new(items, quote);
                let first = choices.current();
                if let Some(completing) = &mut self.completion {
                    completing.choices = Some(choices);
                }
                first
            }
        };
        if !self.lists(target) {
            self.completion = None;
        }
        if let Some(text) = text {
            self.put_before_cursor(target, &format!("{keep}{text}"));
        }
    }
}
