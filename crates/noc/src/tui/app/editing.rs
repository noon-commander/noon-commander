//! Editing files with the editor and opening them externally.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use noc_ops::CopyOptions;
use noc_vfs::Location;
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::describe::Reason;
use crate::tui::dialog::Problem;
use crate::tui::panel::{child, location_text};
use crate::tui::transfer::{self};

use super::{Again, App, Editing, Effect, Handover, Job, JobKind, Then, Way};

impl App {
    /// Edits the file under the cursor of the active panel with F4: a local one where it is, a
    /// remote one as a local copy, which goes back if it changes.
    pub(super) fn edit(&mut self) -> Vec<Effect> {
        let panel = self.panel(self.active);
        let dir = panel.location().clone();
        let Some(name) = panel
            .entry_under_cursor()
            .filter(|entry| !entry.is_dir_like())
            .map(|entry| entry.name.clone())
        else {
            return Vec::new();
        };
        self.edit_file(dir, &name, None)
    }

    /// Edits the file `name` in `dir`, at `line` if there is one: a local one where it is, a
    /// remote one as a local copy, which goes back if it changes.
    pub(super) fn edit_file(
        &mut self,
        dir: Location,
        name: &[u8],
        line: Option<u64>,
    ) -> Vec<Effect> {
        let location = child(&dir, name);
        let Location::Remote { host, .. } = &location else {
            if let Location::Local(path) = location {
                let noted = self.note(&dir);
                self.editing = Some(Editing {
                    file: path.clone(),
                    remote: None,
                    dir,
                    line,
                });
                self.handover = Some(Handover::Edit { file: path, line });
                return noted;
            }
            return Vec::new();
        };
        let on = host.clone();
        let Ok(Some(handle)) = self.link_for(&location) else {
            let again = Again::EditFile {
                dir: dir.clone(),
                name: name.to_vec(),
                line,
            };
            self.show_not_connected(fl!("edit-error"), &location, again);
            return Vec::new();
        };
        self.last_job += 1;
        let id = self.last_job;
        // Its own name last, so that the editor knows what kind of file it is.
        let mut temporary = format!("edit-{}-{id}-", std::process::id()).into_bytes();
        temporary.extend_from_slice(name);
        let file = self
            .runtime_dir
            .join(std::ffi::OsStr::from_bytes(&temporary));
        let cancel = CancellationToken::new();
        let mut job = Job::new(
            id,
            JobKind::Transfer(transfer::Kind::Copy),
            Vec::new(),
            cancel.clone(),
        )
        .then(Then::Edit);
        job.hosts.push(on);
        self.jobs.push(job);
        self.editing = Some(Editing {
            file: file.clone(),
            remote: Some(location.clone()),
            dir,
            line,
        });
        vec![Effect::Copy {
            id,
            sources: vec![location],
            target: Location::Local(file),
            hosts: (Some(handle), None),
            options: CopyOptions {
                preserve: true,
                atomic: false,
                remove_sources: false,
                overwrite: true,
            },
            cancel,
        }]
    }

    /// Opens the marked entries of the active panel, or the one under the cursor, with the
    /// programs of the desktop, as vim's `gx`. On a host, the line of the mode says that only
    /// local files open so.
    pub(super) fn system_open(&mut self) -> Vec<Effect> {
        let panel = self.panel(self.active);
        let dir = panel.location().clone();
        let chosen = panel.chosen();
        if chosen.is_empty() {
            return Vec::new();
        }
        let Location::Local(here) = &dir else {
            self.message = Some(fl!("message-open-remote"));
            return Vec::new();
        };
        let paths = chosen
            .iter()
            .map(|entry| here.join(std::ffi::OsStr::from_bytes(&entry.name)))
            .collect();
        let mut effects = vec![Effect::Open(paths)];
        effects.extend(self.note(&dir));
        effects
    }

    /// Takes the failure of an [`Effect::Open`]: the path that did not open, and why.
    pub(crate) fn open_failed(&mut self, path: &Path, reason: Reason) {
        let path = location_text(&Location::Local(path.to_path_buf()));
        let reason = reason.sanitized();
        self.show_problem(&Problem::new(fl!("open-error"), reason).at(path));
    }

    /// Takes the end of the editor: whether the file changed, or why it could not run. A
    /// changed copy of a remote file goes back.
    pub(crate) fn edited(&mut self, result: Result<bool, Reason>) -> Vec<Effect> {
        let Some(editing) = self.editing.take() else {
            return Vec::new();
        };
        match result {
            Ok(changed) => self.finish_edit(editing, changed),
            Err(reason) => {
                let path = editing.remote.as_ref().map_or_else(
                    || location_text(&Location::Local(editing.file.clone())),
                    location_text,
                );
                let problem = Problem::new(fl!("edit-error"), reason.sanitized()).at(path);
                self.show_problem_way(&problem, Way::Again(Again::Edit(editing)));
                Vec::new()
            }
        }
    }

    /// Ends the edit of `editing`: panels on a local file's directory read it again; the copy of
    /// a remote file goes back if it `changed`, and goes away if not.
    pub(super) fn finish_edit(&mut self, editing: Editing, changed: bool) -> Vec<Effect> {
        let Some(remote) = editing.remote else {
            return self.reload(&editing.dir);
        };
        if !changed {
            return vec![Effect::Discard(editing.file)];
        }
        self.send_back(editing.file, remote, editing.dir)
    }

    /// Copies `copy`, the edited copy of `remote`, back over it, and removes it then; panels on
    /// `dir` read it again.
    pub(super) fn send_back(
        &mut self,
        copy: PathBuf,
        remote: Location,
        dir: Location,
    ) -> Vec<Effect> {
        let Ok(Some(handle)) = self.link_for(&remote) else {
            self.keep_edit(copy, remote, dir, &fl!("error-connection-closed"));
            return Vec::new();
        };
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        let then = Then::Discard {
            copy: copy.clone(),
            remote: remote.clone(),
            dir: dir.clone(),
        };
        let job = Job::new(
            id,
            JobKind::Transfer(transfer::Kind::Copy),
            vec![dir],
            cancel.clone(),
        )
        .then(then);
        self.jobs.push(job);
        vec![Effect::Copy {
            id,
            sources: vec![Location::Local(copy)],
            target: remote,
            hosts: (None, Some(handle)),
            options: CopyOptions {
                preserve: true,
                atomic: self.copy_choices.atomic,
                remove_sources: false,
                overwrite: true,
            },
            cancel,
        }]
    }

    /// Says that `copy`, the edited copy of `remote`, could not go back, `why`, and where it
    /// stays; Retry sends it again.
    pub(super) fn keep_edit(&mut self, copy: PathBuf, remote: Location, dir: Location, why: &str) {
        let shown = cells::sanitize(copy.as_os_str().as_bytes());
        let reason = [why.to_owned(), fl!("edit-kept-copy", copy = shown)].join("\n");
        let problem = Problem::new(fl!("edit-kept"), reason).at(location_text(&remote));
        let again = Again::SendBack { copy, remote, dir };
        self.show_problem_way(&problem, Way::Again(again));
    }
}
