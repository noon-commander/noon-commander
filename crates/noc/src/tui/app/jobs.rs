//! Jobs: starting them, their events, and the list of them.

use std::collections::HashMap;
use std::time::Instant;

use noc_ops::{Decision, Resolution, Step};
use noc_vfs::{FileKind, Location};
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::describe::Reason;
use crate::tui::dialog::{Button, Dialog, Problem, Tone};
use crate::tui::jobs::{JobsEvent, Row};
use crate::tui::keymap::Resolved;
use crate::tui::panel::{child, location_text};
use crate::tui::progress::JobButton;
use crate::tui::tasks::{HostHandle, HostLink, JobEvent, Relink};

use super::{
    Again, App, Effect, Handover, Host, Job, JobKind, Open, Purpose, Shown, Sight, Then,
    files_and_dirs,
};

impl App {
    /// Gives a key to the job in front, if there is one: Abort stops it, Background sends it
    /// behind the panels.
    pub(super) fn handle_job(&mut self, input: Resolved) -> bool {
        let Some(job) = self.jobs.iter_mut().find(|job| !job.background) else {
            return false;
        };
        match job.view.handle(input) {
            Some(JobButton::Abort) => {
                let id = job.id;
                self.abort_job(id);
            }
            Some(JobButton::Background) => job.background = true,
            None => {}
        }
        true
    }

    /// Gives a key to the list of jobs, if it is open: Show brings a job to the front, in place
    /// of the list.
    pub(super) fn handle_jobs_list(&mut self, input: Resolved) -> bool {
        let rows = self.job_rows();
        let Some(list) = &mut self.jobs_list else {
            return false;
        };
        match list.handle(input, &rows) {
            JobsEvent::Pending => {}
            JobsEvent::Show(id) => {
                if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                    job.background = false;
                }
                self.jobs_list = None;
            }
            JobsEvent::Abort(id) => self.abort_job(id),
            JobsEvent::Closed => self.jobs_list = None,
        }
        true
    }

    /// Stops the job `id`; one that waits goes at once, as it has not started.
    fn abort_job(&mut self, id: u64) {
        let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return;
        };
        if job.queued.is_some() {
            self.end_job(id);
        } else {
            job.cancel.cancel();
            job.view.abort();
        }
    }

    /// The jobs, as the list shows them.
    pub(super) fn job_rows(&self) -> Vec<Row> {
        let now = Instant::now();
        self.jobs
            .iter()
            .map(|job| {
                let summary = job.view.summary(now);
                Row {
                    id: job.id,
                    title: job.view.title().to_owned(),
                    state: summary.state,
                    left: summary.left,
                    current: summary.current,
                }
            })
            .collect()
    }

    /// The job `id` has the answer to its question and goes on, with its clock.
    pub(super) fn answered(&mut self, id: u64) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
            job.view.answered(Instant::now());
        }
    }

    /// The job in front, in its window.
    pub(super) fn in_front(&self) -> Option<&Job> {
        self.jobs.iter().find(|job| !job.background)
    }

    /// How much the screen shows of `job`: its window while it is in front, else its row while
    /// the list of jobs is open, else only its share of the corner.
    pub(super) fn sight(&self, job: &Job) -> Sight {
        if !job.background {
            Sight::Window
        } else if self.jobs_list.is_some() {
            Sight::Row
        } else {
            Sight::Corner
        }
    }

    /// Asks before F8 deletes the marked entries of the active panel, or the one under the
    /// cursor, in red with Yes as the default, as mc does.
    pub(super) fn ask_delete(&mut self) {
        let panel = self.panel(self.active);
        let chosen = panel.chosen();
        let message = match chosen.as_slice() {
            [] => return,
            [entry] => {
                let name = cells::sanitize(&entry.name);
                if entry.metadata.kind == FileKind::Dir {
                    fl!("delete-directory", name = name)
                } else {
                    fl!("delete-file", name = name)
                }
            }
            many => {
                let (files, dirs) = files_and_dirs(many);
                fl!("delete-many", files = files, dirs = dirs)
            }
        };
        let names = chosen.iter().map(|entry| entry.name.clone()).collect();
        let dir = panel.location().clone();
        let buttons = vec![Button::Yes, Button::No];
        let dialog = Dialog::question(&fl!("delete-title"), &message, buttons, 0, Tone::Warning);
        self.dialogs.push_back(Open {
            dialog,
            purpose: Purpose::Delete { dir, names },
        });
    }

    /// Starts deleting `names` in `dir`, and shows its progress.
    pub(super) fn start_delete(&mut self, dir: Location, names: &[Vec<u8>]) -> Vec<Effect> {
        let Ok(host) = self.link_for(&dir) else {
            let again = Again::Delete {
                dir: dir.clone(),
                names: names.to_vec(),
            };
            self.show_not_connected(fl!("delete-error"), &dir, again);
            return Vec::new();
        };
        let targets = names.iter().map(|name| child(&dir, name)).collect();
        let noted = self.note(&dir);
        self.last_job += 1;
        let id = self.last_job;
        let cancel = CancellationToken::new();
        let job = Job::new(id, JobKind::Delete, vec![dir], cancel.clone());
        let effect = Effect::Delete {
            id,
            targets,
            host,
            cancel,
        };
        let mut effects = self.launch(job, effect);
        effects.extend(noted);
        effects
    }

    /// Starts `job` with `effect`, or lets it wait while as many jobs run as
    /// `transfer.parallel_jobs` allows. The jobs of F4 never wait.
    pub(super) fn launch(&mut self, mut job: Job, effect: Effect) -> Vec<Effect> {
        let running = self.jobs.iter().filter(|job| job.queued.is_none()).count();
        if job.then.is_some() || running < self.parallel_jobs {
            self.jobs.push(job);
            return vec![effect];
        }
        job.view.wait();
        job.queued = Some(effect);
        self.jobs.push(job);
        Vec::new()
    }

    /// Starts the jobs that wait, the oldest first, as far as `transfer.parallel_jobs` allows.
    pub(super) fn start_queued(&mut self) -> Vec<Effect> {
        let mut running = self.jobs.iter().filter(|job| job.queued.is_none()).count();
        let mut effects = Vec::new();
        for job in &mut self.jobs {
            if running >= self.parallel_jobs {
                break;
            }
            if let Some(effect) = job.queued.take() {
                job.view.scanning(0, Instant::now());
                effects.push(effect);
                running += 1;
            }
        }
        effects
    }

    /// Takes a report from the job `id`, as [`App::job_event`] does, and tells whether the
    /// screen changes with it. A job reports every chunk it moves, which come in batches up to
    /// 50 a second, while its window, or its row in the list of jobs, changes only when what it
    /// shows does, and the corner shows only how far the jobs in the background are together,
    /// in percent: the screen draws when one of these changes, not for every batch.
    pub(crate) fn job_report(&mut self, id: u64, event: JobEvent) -> (Vec<Effect>, bool) {
        if !event.is_progress() {
            return (self.job_event(id, event), true);
        }
        let now = Instant::now();
        let shown = self.job_shown(id, now);
        let corner = self.corner();
        let effects = self.job_event(id, event);
        let changes = self.job_shown(id, now) != shown || self.corner() != corner;
        (effects, changes)
    }

    /// What the screen shows at `now` of the job `id` beyond its share of the corner.
    fn job_shown(&self, id: u64, now: Instant) -> Option<Shown> {
        let job = self.jobs.iter().find(|job| job.id == id)?;
        match self.sight(job) {
            Sight::Window => Some(Shown::Window(job.view.look(now))),
            Sight::Row => Some(Shown::Row(job.view.look(now).row())),
            Sight::Corner => None,
        }
    }

    /// Takes a report from the job `id`: progress for its window, a failure for a dialog
    /// that waits for an answer, or its end.
    pub(crate) fn job_event(&mut self, id: u64, event: JobEvent) -> Vec<Effect> {
        let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return Vec::new();
        };
        match event {
            JobEvent::Scanning { at, items } => job.view.scanning(items, at),
            JobEvent::Progress { current, reports } => {
                if let Some((&(at, counts), before)) = reports.split_last() {
                    for &(at, counts) in before {
                        job.view.passed(counts, at);
                    }
                    job.view.working(location_text(&current), counts, at);
                }
            }
            JobEvent::Exists { at, taken } => {
                job.view.ask(at);
                self.ask_conflict(id, *taken);
            }
            JobEvent::Failed {
                at,
                path,
                step,
                reason,
                retry,
                reply,
            } => {
                job.view.failed(at);
                if let Some(host) = away_of(&self.hosts, &job.hosts) {
                    job.view.away(Some(cells::sanitize(host.as_bytes())));
                    job.held = Some(reply);
                    return Vec::new();
                }
                let reason = Reason {
                    text: cells::sanitize(reason.text.as_bytes()),
                    ..reason
                };
                let problem = Problem::new(step_text(step), reason).at(location_text(&path));
                let (single, more) = (job.view.single(), job.view.more());
                self.ask_failure(id, &problem, (retry, single, more), reply);
            }
            JobEvent::Sums(sums) => {
                if let Some(hashing) = &mut job.hashing {
                    hashing.sums = Some(sums);
                }
            }
            JobEvent::Finished { complete } => {
                if let Some(mut job) = self.end_job(id) {
                    if let Some(hashing) = job.hashing.take() {
                        self.show_sums(id, hashing);
                    }
                    let mut effects = job
                        .then
                        .map_or_else(Vec::new, |then| self.follow_up(then, complete));
                    for (index, dir) in job.changes.iter().enumerate() {
                        if !job.changes[..index].contains(dir) {
                            effects.extend(self.reload(dir));
                        }
                    }
                    effects.extend(self.start_queued());
                    return effects;
                }
            }
        }
        Vec::new()
    }

    /// Takes the job `id` away, with the questions it asked.
    fn end_job(&mut self, id: u64) -> Option<Job> {
        let index = self.jobs.iter().position(|job| job.id == id)?;
        let job = self.jobs.remove(index);
        self.dialogs.retain(|open| !open.asks_for(job.id));
        if let Some(viewing) = &mut self.viewing
            && viewing
                .question
                .as_ref()
                .is_some_and(|open| open.asks_for(job.id))
        {
            viewing.question = None;
        }
        Some(job)
    }

    /// Lets the jobs on `host`, whose connection is lost, wait for its next one: a failure one
    /// of them asks about meanwhile, or asked about already, waits too, for Retry then.
    pub(super) fn hold_jobs(&mut self, host: &str) {
        let shown = cells::sanitize(host.as_bytes());
        for job in &mut self.jobs {
            if !job.hosts.iter().any(|on| on == host) {
                continue;
            }
            job.view.away(Some(shown.clone()));
            let id = job.id;
            let asked = self.dialogs.iter().position(
                |open| matches!(&open.purpose, Purpose::Failure { job, .. } if *job == id),
            );
            if let Some(Open {
                purpose: Purpose::Failure { reply, .. },
                ..
            }) = asked.and_then(|index| self.dialogs.remove(index))
            {
                job.held = Some(reply);
            }
        }
    }

    /// Lets the jobs whose hosts are all connected again go on, trying again what failed as
    /// one was lost.
    pub(super) fn resume_jobs(&mut self) {
        let now = Instant::now();
        for job in &mut self.jobs {
            if away_of(&self.hosts, &job.hosts).is_some() {
                continue;
            }
            job.view.away(None);
            if let Some(reply) = job.held.take() {
                job.view.answered(now);
                let _ = reply.send(Resolution::once(Decision::Retry));
            }
        }
    }

    /// Ends the jobs on `host`, which the user gave up or disconnected: they stop, and what
    /// follows a job of F4 hears that it did not end.
    pub(super) fn end_jobs_on(&mut self, host: &str) -> Vec<Effect> {
        let lost: Vec<u64> = self
            .jobs
            .iter()
            .filter(|job| job.hosts.iter().any(|on| on == host))
            .map(|job| job.id)
            .collect();
        let mut effects = Vec::new();
        for id in lost {
            let Some(job) = self.end_job(id) else {
                continue;
            };
            job.cancel.cancel();
            if let Some(then) = job.then {
                effects.extend(self.follow_up(then, false));
            }
        }
        effects.extend(self.start_queued());
        effects
    }

    /// Does what follows a job of F4 once it ends, `complete` if it did all it was asked: opens
    /// the editor on a copy that came, or removes a copy that went back. A partial copy goes;
    /// an edited one that did not go back stays, and the user hears where.
    fn follow_up(&mut self, then: Then, complete: bool) -> Vec<Effect> {
        match (then, complete) {
            (Then::Edit, true) => {
                self.handover = self.editing.as_ref().map(|editing| Handover::Edit {
                    file: editing.file.clone(),
                    line: editing.line,
                });
                Vec::new()
            }
            (Then::Edit, false) => self
                .editing
                .take()
                .map(|editing| Effect::Discard(editing.file))
                .into_iter()
                .collect(),
            (Then::Discard { copy, .. }, true) => vec![Effect::Discard(copy)],
            (Then::Discard { copy, remote, dir }, false) => {
                let why = fl!("edit-kept-stopped");
                self.keep_edit(copy, remote, dir, &why);
                Vec::new()
            }
        }
    }

    /// Reads `dir` again in the panels that show it, with their cursors where they were;
    /// hidden tabs read it once they show.
    pub(super) fn reload(&mut self, dir: &Location) -> Vec<Effect> {
        let mut effects = Vec::new();
        for id in self.panel_ids() {
            let shown = self.is_shown(id);
            let Some(tab) = self.tab_mut(id) else {
                continue;
            };
            if tab.panel.location() != dir {
                continue;
            }
            if shown {
                let here = tab.panel.here();
                let request = tab.panel.go(here);
                effects.extend(self.route(id, request));
            } else {
                tab.stale = true;
            }
        }
        effects
    }

    /// How a job reaches the host of `location`, if it is remote; `Err` if the host is not
    /// connected.
    pub(super) fn link_for(&mut self, location: &Location) -> Result<Option<HostLink>, ()> {
        let Location::Remote { host, .. } = location else {
            return Ok(None);
        };
        let Some(Host::Connected { handle, .. }) = self.hosts.get(host) else {
            return Err(());
        };
        let relink = self
            .links
            .entry(host.clone())
            .or_insert_with(|| Relink::new(handle.clone()));
        Ok(Some(relink.link()))
    }

    /// How to reach `location`: `None` here, the handle of its host if that is connected,
    /// and an error if it is not.
    pub(super) fn handle_for(&self, location: &Location) -> Result<Option<HostHandle>, ()> {
        match location {
            Location::Remote { host, .. } => match self.hosts.get(host) {
                Some(Host::Connected { handle, .. }) => Ok(Some(handle.clone())),
                _ => Err(()),
            },
            Location::Local(_) => Ok(None),
        }
    }

    /// Stops every job, for quitting; they clean up before the connections go.
    pub(crate) fn stop_jobs(&self) {
        for job in &self.jobs {
            job.cancel.cancel();
        }
    }
}

/// What a job could not do, over the path it could not do it to.
fn step_text(step: Step) -> String {
    match step {
        Step::Inspect => fl!("step-inspect"),
        Step::List => fl!("step-list"),
        Step::Read => fl!("step-read"),
        Step::Create => fl!("step-create"),
        Step::Write => fl!("step-write"),
        Step::SetAttributes => fl!("step-set-attributes"),
        Step::Rename => fl!("step-rename"),
        Step::Remove => fl!("step-remove"),
    }
}

/// What a job does after a failure, `dialog`, closed by `event`: about this one by the button
/// pressed, about the later ones by its combo box, if it has one.
/// The first of `on`, the hosts of a job, that is not connected among `hosts`.
fn away_of<'a>(hosts: &HashMap<String, Host>, on: &'a [String]) -> Option<&'a String> {
    on.iter()
        .find(|host| !matches!(hosts.get(*host), Some(Host::Connected { .. })))
}
