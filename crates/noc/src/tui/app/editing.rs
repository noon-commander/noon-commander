//! Editing files with the editor and opening them externally.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use noc_ops::CopyOptions;
use noc_vfs::Location;
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::describe::Reason;
use crate::tui::dialog::{Button, Dialog, Problem};
use crate::tui::panel::{child, location_text, short_size};
use crate::tui::transfer::{self};
use crate::tui::window::Tone;

use super::{
    Again, App, CopyRequest, Editing, Effect, Handover, Job, JobKind, Message, Open, Purpose,
    Sniffed, Sniffing, Then, Way,
};

impl App {
    /// Edits the file under the cursor of the active panel with F4: a local one where it is, a
    /// remote one as a local copy, which goes back if it changes; after a question if it looks
    /// binary or is large.
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
        self.ask_edit(dir, &name, None)
    }

    /// Edits the file `name` in `dir`, at `line` if there is one, once its first bytes and its
    /// size are read, which tell whether it asks first. On a host that is not connected, it
    /// says so at once.
    pub(super) fn ask_edit(
        &mut self,
        dir: Location,
        name: &[u8],
        line: Option<u64>,
    ) -> Vec<Effect> {
        let location = child(&dir, name);
        let Ok(host) = self.handle_for(&location) else {
            return self.edit_file(dir, name, line);
        };
        self.last_job += 1;
        let id = self.last_job;
        self.sniffing = Some(Sniffing {
            id,
            dir,
            name: name.to_vec(),
            line,
        });
        vec![Effect::Sniff { id, location, host }]
    }

    /// Takes what [`Effect::Sniff`] `id` read of the file that F4 is to edit, and edits it,
    /// after a question if that is to come first. A file that could not be read goes to the
    /// editor, or to the copy for it, all the same, which tell why.
    pub(crate) fn sniffed(&mut self, id: u64, result: Result<Sniffed, Reason>) -> Vec<Effect> {
        let Some(Sniffing {
            dir, name, line, ..
        }) = self.sniffing.take_if(|sniffing| sniffing.id == id)
        else {
            return Vec::new();
        };
        match result {
            Ok(sniffed) => self.edit_sniffed(dir, name, line, sniffed),
            Err(reason) => {
                let reason = reason.text;
                tracing::info!(%reason, "cannot tell whether a file to edit is binary");
                self.edit_file(dir, &name, line)
            }
        }
    }

    /// Edits the file `name` in `dir`, at `line`, as `sniffed` tells it is: one that looks
    /// binary, or one larger than `edit.large_mib`, after a question whose default is Cancel.
    pub(super) fn edit_sniffed(
        &mut self,
        dir: Location,
        name: Vec<u8>,
        line: Option<u64>,
        sniffed: Sniffed,
    ) -> Vec<Effect> {
        let limit = self.config.edit.large_mib.saturating_mul(1 << 20);
        let large = sniffed.size.filter(|&size| limit > 0 && size > limit);
        let shown = cells::sanitize(&name);
        let mut message = match (sniffed.binary, large) {
            (false, None) => return self.edit_file(dir, &name, line),
            (true, None) => fl!("edit-ask-binary", name = shown),
            (false, Some(size)) => fl!("edit-ask-large", name = shown, size = short_size(size)),
            (true, Some(size)) => {
                fl!(
                    "edit-ask-binary-large",
                    name = shown,
                    size = short_size(size)
                )
            }
        };
        if let Location::Remote { host, .. } = &dir {
            let host = cells::sanitize(host.as_bytes());
            message.push('\n');
            message.push_str(&fl!("edit-ask-remote", host = host));
        }
        let buttons = vec![Button::Edit, Button::Cancel];
        let dialog = Dialog::question(&fl!("edit-ask-title"), &message, buttons, 1, Tone::Warning);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::EditAnyway { dir, name, line },
        });
        Vec::new()
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
        let file = copy_path(&self.runtime_dir, id, host, name);
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
        let make_dir = file.parent().map(Path::to_path_buf);
        vec![Effect::Copy(CopyRequest {
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
            make_dir,
            cancel,
        })]
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
            self.message = Some(Message::Warning(fl!("message-open-remote")));
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

    /// Ends the edit of `editing`: panels on a local file's directory read it again, and so
    /// does the viewer on the file if it `changed`; the copy of a remote file goes back if it
    /// `changed`, and goes away if not.
    pub(super) fn finish_edit(&mut self, editing: Editing, changed: bool) -> Vec<Effect> {
        let Some(remote) = editing.remote else {
            let mut effects = self.reload(&editing.dir);
            if changed {
                effects.extend(self.reread(&Location::Local(editing.file)));
            }
            return effects;
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
        vec![Effect::Copy(CopyRequest {
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
            make_dir: None,
            cancel,
        })]
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

/// Where F4 copies the file `name` of `host` for job `id`: `edit-PID-N-host/name` in
/// `runtime_dir`. The file keeps its own name, by which editors know its kind, in a directory
/// of its own, which names the host.
pub(super) fn copy_path(runtime_dir: &Path, id: u64, host: &str, name: &[u8]) -> PathBuf {
    let mut dir = format!("edit-{}-{id}-", std::process::id());
    let host = host.replace('/', "_");
    let mut end = host.len().min(NAME_MAX.saturating_sub(dir.len()));
    while !host.is_char_boundary(end) {
        end -= 1;
    }
    dir.push_str(&host[..end]);
    runtime_dir
        .join(dir)
        .join(std::ffi::OsStr::from_bytes(name))
}

/// The longest name of a file, in bytes, on the file systems of macOS and Linux.
const NAME_MAX: usize = 255;
