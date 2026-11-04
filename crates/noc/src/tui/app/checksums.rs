//! Checksum jobs and their results.

use noc_ops::Algorithm;
use noc_vfs::Location;
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::dialog::{Button, Dialog};
use crate::tui::keymap::{Resolved, Typed};
use crate::tui::label::{self, Label};
use crate::tui::panel::{child, location_text};
use crate::tui::save::{self, Save};
use crate::tui::search::Slot;
use crate::tui::sums::{Mark, SumRow, SumsEvent, SumsWindow, Verdict};
use crate::tui::tasks::Refused;

use super::{
    Again, Answer, App, Effect, Hashing, Job, JobKind, Open, Purpose, Results, Way, files_and_dirs,
    hex, parse_expected, sums_file, sums_name,
};

impl App {
    /// Opens the checksum dialog for the marked entries of the active panel, or the one under
    /// the cursor: a combo box of the algorithm, the last one chosen; a field for the checksum
    /// one file should have, disabled for anything else; and, if the other panel's cursor is on
    /// a file, a check box to compare the two, checked if they have the same name.
    pub(super) fn ask_checksum(&mut self) {
        let panel = self.panel(self.active);
        let chosen = panel.chosen();
        let single_file = match chosen.as_slice() {
            [entry] => !entry.is_dir_like(),
            _ => false,
        };
        let message = match chosen.as_slice() {
            [] => return,
            [entry] if single_file => fl!("checksum-one", name = cells::sanitize(&entry.name)),
            [entry] => fl!("checksum-directory", name = cells::sanitize(&entry.name)),
            many => {
                let (files, dirs) = files_and_dirs(many);
                fl!("checksum-many", files = files, dirs = dirs)
            }
        };
        let names: Vec<Vec<u8>> = chosen.iter().map(|entry| entry.name.clone()).collect();
        let dir = panel.location().clone();
        let other_panel = self.panel(self.active.other());
        let other = other_panel
            .entry_under_cursor()
            .filter(|entry| single_file && !entry.is_dir_like())
            .map(|entry| {
                let location = child(other_panel.location(), &entry.name);
                (location, names.first() == Some(&entry.name))
            })
            .filter(|(location, _)| child(&dir, &names[0]) != *location);
        let checks: Vec<(String, bool)> = other
            .iter()
            .map(|(location, same)| {
                let path = label::escape(&location_text(location));
                (fl!("checksum-compare", path = path), *same)
            })
            .collect();
        let algorithms: Vec<String> = Algorithm::ALL.iter().map(|&a| algorithm_name(a)).collect();
        let chosen = Algorithm::ALL
            .iter()
            .position(|&a| a == self.algorithm)
            .unwrap_or(0);
        let buttons = vec![Button::Ok, Button::Cancel];
        let title = fl!("checksum-title");
        let placeholder = if single_file {
            fl!("checksum-expected-placeholder")
        } else {
            fl!("checksum-expected-single")
        };
        let mut dialog = Dialog::fields(&title, &[], &[], buttons)
            .with_message(&message)
            .with_combo_box(&fl!("checksum-algorithm"), &algorithms, chosen)
            .with_separator()
            .with_field(&fl!("checksum-expected"), "")
            .with_placeholder(0, &placeholder);
        for (label, on) in &checks {
            dialog = dialog.with_check(label, *on);
        }
        if !single_file {
            dialog = dialog.with_disabled(0);
        }
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Checksum {
                dir,
                names,
                other: other.map(|(location, _)| location),
                expect: single_file,
            },
        });
    }

    /// Starts the checksum job that the dialog of [`Self::ask_checksum`] asked for. An expected
    /// checksum of another length picks the algorithm that has it.
    pub(super) fn start_checksum(
        &mut self,
        dialog: &Dialog,
        dir: Location,
        names: &[Vec<u8>],
        other: Option<Location>,
        expect: bool,
    ) -> Vec<Effect> {
        let mut algorithm = Algorithm::ALL
            .get(dialog.chosen(0))
            .copied()
            .unwrap_or(Algorithm::Sha256);
        let typed = if expect { dialog.text().trim() } else { "" };
        let expected = if typed.is_empty() {
            None
        } else if let Some((hex, by_length)) = parse_expected(typed, algorithm) {
            algorithm = by_length;
            Some(hex)
        } else {
            let text = cells::sanitize(typed.as_bytes());
            self.show_error(&fl!("checksum-expected-invalid", text = text));
            return Vec::new();
        };
        self.algorithm = algorithm;
        let compare = other.is_some() && dialog.checked(0);
        let mut groups = vec![names.iter().map(|name| child(&dir, name)).collect()];
        if compare {
            groups.extend(other.map(|location| vec![location]));
        }
        self.hash(dir, groups, (algorithm, expected), compare)
    }

    /// Starts the checksum job of the `groups` of files, each on one host or local, with
    /// `algorithm`, against `expected`, comparing the two groups if `compare`; their checksums
    /// may be saved in `dir`.
    pub(super) fn hash(
        &mut self,
        dir: Location,
        groups: Vec<Vec<Location>>,
        (algorithm, expected): (Algorithm, Option<String>),
        compare: bool,
    ) -> Vec<Effect> {
        let mut targets = Vec::new();
        let mut hosts = Vec::new();
        for group in &groups {
            let Some(first) = group.first() else { continue };
            let Ok(handle) = self.link_for(first) else {
                let first = first.clone();
                let again = Again::Checksum {
                    dir,
                    groups,
                    algorithm,
                    expected,
                    compare,
                };
                self.show_not_connected(fl!("checksum-error"), &first, again);
                return Vec::new();
            };
            if let Location::Remote { host, .. } = first {
                hosts.push(host.clone());
            }
            targets.push((group.clone(), handle));
        }
        let noted = self.note(&dir);
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        let mut job = Job::new(id, JobKind::Checksum, Vec::new(), cancel.clone());
        job.hosts = hosts;
        job.hashing = Some(Hashing {
            algorithm,
            expected,
            compare,
            dir: (!compare).then_some(dir),
            sums: None,
        });
        let effect = Effect::Checksum {
            id,
            targets,
            algorithm,
            cancel,
        };
        let mut effects = self.launch(job, effect);
        effects.extend(noted);
        effects
    }

    /// Opens the window with the checksums of the job `id`; an aborted job has none to show,
    /// and a job that found no files says so.
    pub(super) fn show_sums(&mut self, id: u64, hashing: Hashing) {
        let Some(sums) = hashing.sums else {
            return;
        };
        if sums.is_empty() {
            self.dialogs.push_back(Open {
                dialog: Dialog::warning(&fl!("checksum-title"), &fl!("checksum-no-files")),
                purpose: Purpose::Info,
            });
            return;
        }
        let hexes: Vec<Option<String>> = sums
            .iter()
            .map(|sum| sum.digest.as_deref().map(hex))
            .collect();
        let compared = hashing.compare && sums.len() == 2;
        let same = compared && hexes[0].is_some() && hexes[0] == hexes[1];
        let rows: Vec<SumRow> = sums
            .iter()
            .zip(&hexes)
            .map(|(sum, hex)| {
                let mark = match (hex, &hashing.expected) {
                    (None, _) => Mark::Skipped,
                    (Some(hex), Some(expected)) if hex == expected => Mark::Match,
                    (Some(_), Some(_)) => Mark::Mismatch,
                    (Some(_), None) if compared && same => Mark::Match,
                    (Some(_), None) if compared => Mark::Mismatch,
                    (Some(_), None) => Mark::None,
                };
                let name = if compared {
                    location_text(&sum.path)
                } else {
                    cells::sanitize(&sum.name)
                };
                SumRow {
                    name,
                    hex: hex.clone(),
                    mark,
                }
            })
            .collect();
        let verdict = if compared {
            Some(if same {
                Verdict {
                    text: fl!("checksum-same"),
                    good: true,
                }
            } else {
                Verdict {
                    text: fl!("checksum-different"),
                    good: false,
                }
            })
        } else {
            hashing.expected.as_ref().map(|expected| {
                if hexes[0].as_ref() == Some(expected) {
                    Verdict {
                        text: fl!("checksum-matches"),
                        good: true,
                    }
                } else {
                    Verdict {
                        text: fl!("checksum-differs"),
                        good: false,
                    }
                }
            })
        };
        let mut buttons = vec![Button::ToClipboard];
        if rows.len() > 1 {
            buttons.push(Button::AllToClipboard);
        }
        // Two files compared are on screen whole.
        if rows.len() > 1 && !compared {
            buttons.push(Button::View);
        }
        if hashing.dir.is_some() {
            buttons.push(Button::Save);
        }
        buttons.push(Button::Ok);
        let lines = sums
            .into_iter()
            .zip(hexes)
            .map(|(sum, hex)| {
                let name = if compared {
                    location_text(&sum.path).into_bytes()
                } else {
                    sum.name
                };
                (name, hex)
            })
            .collect();
        let title = Label::parse(&algorithm_name(hashing.algorithm)).text;
        let input = self.search_opens(Slot::Checksums);
        self.results.push_back(Results {
            id,
            window: SumsWindow::new(&title, rows, verdict, buttons, input),
            algorithm: hashing.algorithm,
            dir: hashing.dir,
            lines,
        });
    }

    /// Gives a key to the window of checksums in front, if there is one: Copy puts a checksum
    /// on the clipboard, View shows them all in the viewer, in its place, and Save asks where
    /// to save them all.
    pub(super) fn handle_results(&mut self, input: Resolved) -> Option<Vec<Effect>> {
        let results = self.results.front_mut()?;
        let typed = Typed {
            resolved: input,
            extra: self.extra,
        };
        match results.window.handle(typed) {
            SumsEvent::Pending => {}
            SumsEvent::Copy(row) => {
                if let Some((_, Some(hex))) = results.lines.get(row) {
                    self.clipboard = Some(hex.clone());
                }
            }
            SumsEvent::CopyAll => {
                let text = sums_file(&results.lines);
                self.clipboard = Some(String::from_utf8_lossy(&text).into_owned());
            }
            SumsEvent::View => {
                if let Some(results) = self.results.pop_front() {
                    return Some(self.view_sums(results));
                }
            }
            SumsEvent::Save => {
                if let Some(dir) = results.dir.clone() {
                    let save = Save {
                        title: fl!("checksum-save-title"),
                        dir,
                        name: sums_name(results),
                        bytes: sums_file(&results.lines),
                    };
                    self.ask_save(save);
                }
            }
            SumsEvent::Closed => {
                self.results.pop_front();
            }
        }
        Some(Vec::new())
    }

    /// Opens the dialog that saves the file of `save` (ADR 0059).
    pub(super) fn ask_save(&mut self, save: Save) {
        let dialog = save.dialog();
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::SaveFile {
                dir: save.dir,
                bytes: save.bytes,
            },
        });
    }

    /// Writes `bytes` to the file at `location`, over what is there if `replace`.
    pub(super) fn write_file(
        &mut self,
        location: Location,
        bytes: Vec<u8>,
        replace: bool,
    ) -> Vec<Effect> {
        let Ok(host) = self.handle_for(&location) else {
            let again = Again::Write {
                location: location.clone(),
                bytes,
            };
            self.show_not_connected(fl!("save-error"), &location, again);
            return Vec::new();
        };
        self.saving.insert(location.clone(), bytes.clone());
        vec![Effect::WriteFile {
            location,
            bytes,
            replace,
            host,
        }]
    }

    /// Takes the end of an [`Effect::WriteFile`]: panels on its directory read it again, where
    /// the file shows; a taken name asks what to do, as a copy does.
    pub(crate) fn written(
        &mut self,
        location: &Location,
        result: Result<(), Refused>,
    ) -> Vec<Effect> {
        let Some(bytes) = self.saving.remove(location) else {
            return Vec::new();
        };
        match result {
            Ok(()) => location
                .parent()
                .map_or_else(Vec::new, |dir| self.reload(&dir)),
            Err(Refused::Taken(taken)) => {
                let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                let answer = Answer::Save { bytes };
                self.ask_taken(answer, location.clone(), (Some(size), None), *taken);
                Vec::new()
            }
            Err(Refused::Failed(reason)) => {
                let problem = save::error(location_text(location), reason.sanitized());
                let again = Again::Write {
                    location: location.clone(),
                    bytes,
                };
                self.show_problem_way(&problem, Way::Again(again));
                Vec::new()
            }
        }
    }
}

/// The name of `algorithm`, as the checksum dialog and window show it.
fn algorithm_name(algorithm: Algorithm) -> String {
    match algorithm {
        Algorithm::Sha256 => fl!("checksum-sha256"),
        Algorithm::Sha512 => fl!("checksum-sha512"),
        Algorithm::Sha1 => fl!("checksum-sha1"),
        Algorithm::Md5 => fl!("checksum-md5"),
        Algorithm::Blake3 => fl!("checksum-blake3"),
    }
}
