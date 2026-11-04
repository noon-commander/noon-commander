//! Copying and moving, and the questions they ask.

use std::os::unix::ffi::OsStrExt as _;
use std::time::SystemTime;

use noc_ops::{Choice, Conflict, CopyOptions, Resolution, Rest};
use noc_vfs::{Location, Metadata};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::dialog::{Button, Dialog, DialogEvent, Problem, Tone};
use crate::tui::label::{self};
use crate::tui::panel::{self, child, location_text, stem_chars};
use crate::tui::tabs::PanelId;
use crate::tui::tasks::{TakenFile, TakenName};
use crate::tui::transfer::{self, Sources};

use super::{
    Again, Answer, App, Change, Effect, Job, JobKind, Open, Purpose, Way, file_name,
    files_and_dirs, remote_within,
};

impl App {
    /// Asks where F5 copies, or F6 moves, the marked entries of the active panel, or the one
    /// under the cursor, as mc does: the field opens with the other panel's location.
    pub(super) fn ask_transfer(&mut self, kind: transfer::Kind) {
        let panel = self.panel(self.active);
        let chosen = panel.chosen();
        let sources = match chosen.as_slice() {
            [] => return,
            [entry] => Sources::One(prompt_name(&entry.name)),
            many => {
                let (files, dirs) = files_and_dirs(many);
                Sources::Many { files, dirs }
            }
        };
        let names = chosen.iter().map(|entry| entry.name.clone()).collect();
        let other = self.panel(self.active.other()).location().clone();
        let text = location_text(&other);
        let field = (text.clone(), text.chars().count());
        self.open_transfer(kind, &sources, names, field, Some((text, other)));
    }

    /// Asks where Shift+F5 copies the entry under the cursor of the active panel, not the
    /// marked ones, as mc does: the field opens with its name, the cursor before its last
    /// extension, as when it is renamed in its row, so that the copy goes beside it under the
    /// name typed. A name that is not UTF-8 cannot be edited, which an error says: F5 copies it.
    pub(super) fn ask_duplicate(&mut self) {
        let Some(entry) = self.panel(self.active).entry_under_cursor() else {
            return;
        };
        let (name, dir_like) = (entry.name.clone(), entry.is_dir_like());
        let Ok(text) = std::str::from_utf8(&name) else {
            let name = cells::sanitize(&name);
            self.show_error(&fl!("duplicate-not-utf8", name = name));
            return;
        };
        let cursor = stem_chars(text, dir_like);
        // Alone, `~` is the home directory.
        let field = if text == "~" {
            ("\\~".to_owned(), cursor + 1)
        } else {
            (text.to_owned(), cursor)
        };
        let sources = Sources::One(prompt_name(&name));
        self.open_transfer(transfer::Kind::Copy, &sources, vec![name], field, None);
    }

    /// Opens the dialog of F5, F6, and Shift+F5 to do `kind` to `names` in the active panel's
    /// directory, which its prompt names as `sources`: a field that opens with a text and the
    /// cursor before its character of that number, and whether to preserve attributes, for a
    /// copy or a move alike. `offered` is the location that the text stands for while it stays
    /// as it was.
    fn open_transfer(
        &mut self,
        kind: transfer::Kind,
        sources: &Sources,
        names: Vec<Vec<u8>>,
        field: (String, usize),
        offered: Option<(String, Location)>,
    ) {
        let dir = self.panel(self.active).location().clone();
        let panel = self.shown(self.active);
        self.open_transfer_in((panel, dir), kind, sources, names, field, offered);
    }

    /// Opens the dialog of F5, F6, and Shift+F5 as [`App::open_transfer`] does, for `panel` on
    /// `dir`.
    pub(super) fn open_transfer_in(
        &mut self,
        (panel, dir): (PanelId, Location),
        kind: transfer::Kind,
        sources: &Sources,
        names: Vec<Vec<u8>>,
        (text, cursor): (String, usize),
        offered: Option<(String, Location)>,
    ) {
        let preserve = self.copy_choices.preserve;
        let dialog = transfer::dialog(kind, sources, (&text, cursor), preserve);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Transfer {
                kind,
                panel,
                dir,
                names,
                sources: sources.clone(),
                offered,
            },
        });
    }

    /// Where a typed target points: `host:path` on a host the app knows, or a path from the
    /// directory of `panel`.
    pub(super) fn resolve_target(&self, panel: PanelId, text: &str) -> Option<Location> {
        if text.is_empty() {
            return None;
        }
        if let Location::Remote { host, path } = Location::parse(text)
            && self.hosts.contains_key(&host)
        {
            return Some(Location::Remote { host, path });
        }
        Some(self.panel_of(panel)?.resolve(text))
    }

    /// Starts copying or moving `names` from `dir` to `target`, and shows its progress; a
    /// target that is one of the sources, or in one, is an error.
    pub(super) fn start_transfer(
        &mut self,
        kind: transfer::Kind,
        dir: Location,
        names: &[Vec<u8>],
        target: Location,
        typed: Change,
    ) -> Vec<Effect> {
        let sources: Vec<Location> = names.iter().map(|name| child(&dir, name)).collect();
        let error = |reason: String| Problem::new(kind.error(), reason).at(location_text(&target));
        if target == dir || sources.contains(&target) {
            self.show_problem_way(&error(fl!("transfer-same")), Way::Change(typed));
            return Vec::new();
        }
        if let Some(source) = sources.iter().find(|source| within(source, &target)) {
            let reason = fl!("transfer-into-itself", path = location_text(source));
            self.show_problem_way(&error(reason), Way::Change(typed));
            return Vec::new();
        }
        let (Ok(from), Ok(to)) = (self.link_for(&dir), self.link_for(&target)) else {
            let away = if self.link_for(&dir).is_err() {
                &dir
            } else {
                &target
            };
            let away = away.clone();
            let again = Again::Transfer {
                kind,
                dir: dir.clone(),
                names: names.to_vec(),
                target: target.clone(),
                typed,
            };
            self.show_not_connected(kind.error(), &away, again);
            return Vec::new();
        };
        // The source, and the target if a panel shows it.
        let mut noted = self.note(&dir);
        noted.extend(self.note(&target));
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        // Into the target, or to it as a new name in its parent; a move empties the source.
        let changes = [Some(target.clone()), target.parent(), Some(dir)]
            .into_iter()
            .flatten()
            .collect();
        let job = Job::new(id, JobKind::Transfer(kind), changes, cancel.clone());
        let options = CopyOptions {
            preserve: self.copy_choices.preserve,
            atomic: self.copy_choices.atomic,
            remove_sources: kind == transfer::Kind::Move,
            overwrite: false,
        };
        let effect = Effect::Copy {
            id,
            sources,
            target,
            hosts: (from, to),
            options,
            cancel,
        };
        let mut effects = self.launch(job, effect);
        effects.extend(noted);
        effects
    }

    /// Asks what to do about `asked`, a taken name in the copy job `id` (ADR 0060), with Skip as
    /// the default. A job of one entry has no Skip, which would do what Abort does, and Abort is
    /// the default; at its last entry, no combo box asks about the names taken after it.
    pub(super) fn ask_conflict(&mut self, id: u64, asked: TakenName) {
        let (single, more) = self
            .jobs
            .iter()
            .find(|job| job.id == id)
            .map_or((false, true), |job| (job.view.single(), job.view.more()));
        let mut buttons = vec![Button::Overwrite];
        if !single {
            buttons.push(Button::Skip);
        }
        buttons.extend([Button::Rename, Button::Abort]);
        let default = if single { buttons.len() - 1 } else { 1 };
        let source = &asked.source_metadata;
        let new = self.file_text(source.size, source.modified);
        let mut dialog = self.taken_dialog(
            &asked.target,
            &new,
            &asked.target_metadata,
            buttons,
            default,
        );
        if more {
            let rests = [
                fl!("copy-exists-ask"),
                fl!("copy-exists-same"),
                fl!("copy-exists-older"),
                fl!("copy-exists-smaller"),
                fl!("copy-exists-larger"),
                fl!("copy-exists-size-differs"),
            ];
            dialog = dialog
                .with_line()
                .with_combo_box(&fl!("copy-exists-rest"), &rests, 0);
        }
        let answer = Answer::Job {
            job: id,
            source: asked.source,
            reply: asked.reply,
        };
        self.dialogs.push_back(Open {
            dialog: dialog.with_focus_on_default(),
            purpose: Purpose::Conflict {
                answer,
                target: asked.target,
                free: asked.free,
            },
        });
    }

    /// Asks what to do about `taken`, the file at `target`, where a rename or a save, as
    /// `answer` says, was to put a file of `new`, its size and time: the question of a job about
    /// a taken name, for one file, with Overwrite, Rename…, and Cancel, the default.
    pub(super) fn ask_taken(
        &mut self,
        answer: Answer,
        target: Location,
        new: (Option<u64>, Option<SystemTime>),
        taken: TakenFile,
    ) {
        let new = self.file_text(new.0, new.1);
        let buttons = vec![Button::Overwrite, Button::Rename, Button::Cancel];
        let dialog = self
            .taken_dialog(&target, &new, &taken.target, buttons, 2)
            .with_focus_on_default();
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Conflict {
                answer,
                target,
                free: taken.free,
            },
        });
    }

    /// The question about the taken name `target`, as a warning: its path; then, between lines,
    /// the line of the new file, which says `new`, and that of the file there, `existing`, each
    /// with its size and time, and each viewing its file; then `buttons`, of which `default`
    /// is the default.
    fn taken_dialog(
        &self,
        target: &Location,
        new: &str,
        existing: &Metadata,
        buttons: Vec<Button>,
        default: usize,
    ) -> Dialog {
        let title = fl!("copy-exists-title");
        let existing = self.file_text(existing.size, existing.modified);
        Dialog::question(
            &title,
            &location_text(target),
            buttons,
            default,
            Tone::Warning,
        )
        .with_line()
        .with_item(&fl!("copy-exists-new"), new)
        .with_item(&fl!("copy-exists-existing"), &existing)
    }

    /// The size and the time of a file, as the question about a taken name shows them. An
    /// unknown time leaves its cells blank, so that the sizes line up.
    fn file_text(&self, size: Option<u64>, modified: Option<SystemTime>) -> String {
        let formats = noc_locale::current();
        let mut time = cells::mtime(modified, &self.tz, &formats);
        if time.is_empty() {
            time = " ".repeat(formats.time_width());
        }
        fl!("copy-exists-file", size = size, time = time)
    }

    /// Takes a press in the question about a taken name in front that leaves it waiting: an item
    /// views its file, over the windows, and Rename… asks for a new name; the question comes
    /// back once they close. `None` for any other.
    pub(super) fn set_question_aside(&mut self, event: DialogEvent) -> Option<Vec<Effect>> {
        let Purpose::Conflict {
            answer,
            target,
            free,
        } = &self.dialogs.front()?.purpose
        else {
            return None;
        };
        match event {
            DialogEvent::Opened(index) => {
                let new = match answer {
                    Answer::Job { source, .. } => Ok(source.clone()),
                    Answer::Rename { from, .. } => Ok(from.clone()),
                    Answer::Save { bytes } => Err(bytes.clone()),
                };
                let target = target.clone();
                let question = self.dialogs.pop_front()?;
                Some(match (index, new) {
                    (0, Ok(location)) => self.view_for(location, question),
                    (0, Err(bytes)) => {
                        let name = file_name(&target).unwrap_or_default();
                        let name = String::from_utf8_lossy(&name).into_owned();
                        self.view_text_for(name, bytes, question)
                    }
                    _ => self.view_for(target, question),
                })
            }
            DialogEvent::Pressed(Button::Rename) => {
                // A name that is not UTF-8 cannot be edited; one is typed in its place.
                let name = String::from_utf8(free.clone()).unwrap_or_default();
                let cursor = panel::stem_chars(&name, false);
                let (title, prompt) = (fl!("copy-rename-title"), fl!("copy-rename-prompt"));
                let dialog = Dialog::form(&title, &prompt, &name, &[]).with_cursor(cursor);
                let question = Box::new(self.dialogs.pop_front()?);
                self.dialogs.push_front(Open {
                    dialog,
                    purpose: Purpose::ConflictRename { question },
                });
                Some(Vec::new())
            }
            _ => None,
        }
    }

    /// Does what the question about the taken name `target`, `dialog`, was for, once `event`
    /// closed it: a job hears the answer; a rename or a save goes over the file there after
    /// Overwrite, and does nothing after Cancel.
    pub(super) fn conflict_answered(
        &mut self,
        answer: Answer,
        target: Location,
        event: DialogEvent,
        dialog: &Dialog,
    ) -> Vec<Effect> {
        let overwrite = event == DialogEvent::Pressed(Button::Overwrite);
        match answer {
            Answer::Job { job, reply, .. } => {
                self.answered(job);
                let _ = reply.send(conflict_of(event, dialog));
            }
            Answer::Rename { panel, from } if overwrite => {
                return self.rename_entry(panel, from, target, true);
            }
            Answer::Save { bytes } if overwrite => return self.write_file(target, bytes, true),
            Answer::Rename { .. } | Answer::Save { .. } => {}
        }
        Vec::new()
    }

    /// Answers `question`, a [`Purpose::Conflict`], with Rename… to `text`, typed from the
    /// directory of the file there: a job copies there, and reads the directory it names again
    /// once it ends; a rename or a save goes there, and asks again if that name is taken too.
    pub(super) fn rename_conflict(&mut self, question: Open, text: &str) -> Vec<Effect> {
        let Purpose::Conflict { answer, target, .. } = question.purpose else {
            return Vec::new();
        };
        let dir = target.parent().unwrap_or(target);
        let location = panel::resolve(&dir, &self.home, text);
        match answer {
            Answer::Job { job, reply, .. } => {
                let path = match &location {
                    Location::Local(path) => path.as_os_str().as_bytes().to_vec(),
                    Location::Remote { path, .. } => path.as_bytes().to_vec(),
                };
                if let Some(dir) = location.parent()
                    && let Some(job) = self.jobs.iter_mut().find(|found| found.id == job)
                    && !job.changes.contains(&dir)
                {
                    job.changes.push(dir);
                }
                self.answered(job);
                let conflict = Conflict {
                    this: Choice::Rename(path),
                    rest: rest_of(&question.dialog),
                };
                let _ = reply.send(conflict);
                Vec::new()
            }
            Answer::Rename { panel, from } => self.rename_entry(panel, from, location, false),
            Answer::Save { bytes } => self.write_file(location, bytes, false),
        }
    }

    /// Asks what to do about `problem`, a failure in the job `id`, which waits for `reply`:
    /// Retry, the default, unless it would fail again, as `retry` says; Skip, unless the entry
    /// is the `single` one, where it would do what Abort does; and Abort. Where `more` entries
    /// come after it, a combo box asks about the failures after it too.
    pub(super) fn ask_failure(
        &mut self,
        id: u64,
        problem: &Problem,
        (retry, single, more): (bool, bool, bool),
        reply: oneshot::Sender<Resolution>,
    ) {
        let mut buttons = Vec::new();
        if retry {
            buttons.push(Button::Retry);
        }
        if !single {
            buttons.push(Button::Skip);
        }
        buttons.push(Button::Abort);
        let mut dialog = Dialog::failure(problem, buttons, 0);
        if more {
            let mut rests = vec![fl!("copy-exists-ask"), fl!("failure-skip-all")];
            if problem.code().is_some() {
                rests.push(fl!("failure-skip-same"));
            }
            dialog = dialog
                .with_line()
                .with_combo_box(&fl!("copy-exists-rest"), &rests, 0);
        }
        self.dialogs.push_back(Open {
            dialog: dialog.with_focus_on_default(),
            purpose: Purpose::Failure { job: id, reply },
        });
    }
}

/// The answer of `dialog`, the question about a taken name, closed by `event`.
fn conflict_of(event: DialogEvent, dialog: &Dialog) -> Conflict {
    let this = match event {
        DialogEvent::Pressed(Button::Overwrite) => Choice::Overwrite,
        DialogEvent::Pressed(Button::Skip) => Choice::Skip,
        _ => Choice::Abort,
    };
    Conflict {
        this,
        rest: rest_of(dialog),
    }
}

/// What the question about a taken name, `dialog`, says to do about the names taken after it,
/// by its combo box.
fn rest_of(dialog: &Dialog) -> Rest {
    const RESTS: [Rest; 6] = [
        Rest::Ask,
        Rest::Same,
        Rest::OverwriteOlder,
        Rest::OverwriteSmaller,
        Rest::OverwriteLarger,
        Rest::OverwriteSizeDiffers,
    ];
    RESTS.get(dialog.chosen(0)).copied().unwrap_or(Rest::Ask)
}

/// Whether `inner` is `outer` or in it, on the same file system.
fn within(outer: &Location, inner: &Location) -> bool {
    match (outer, inner) {
        (Location::Local(outer), Location::Local(inner)) => inner.starts_with(outer),
        (
            Location::Remote {
                host: outer_host,
                path: outer,
            },
            Location::Remote {
                host: inner_host,
                path: inner,
            },
        ) => outer_host == inner_host && remote_within(outer, inner),
        _ => false,
    }
}

/// A name for the prompt of a dialog, where `&` marks the letter of the field; a name's is a
/// character.
fn prompt_name(name: &[u8]) -> String {
    label::escape(&cells::sanitize(name))
}
