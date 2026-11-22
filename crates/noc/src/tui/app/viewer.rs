//! The F3 viewer and the reads it waits for.

use noc_scroll::Motion;
use noc_vfs::{DirEntry, Location};
use noc_viewer::{
    Checked, Command as ViewerCommand, Part as ViewerPart, Progress as ReadProgress,
    Sizes as ViewerSizes, Stamp, Taken, Viewer, part_of,
};
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::describe::Reason;
use crate::tui::dialog::Problem;
use crate::tui::find::FindEvent;
use crate::tui::keymap::{Action, Resolved};
use crate::tui::panel::{self, child, location_text};
use crate::tui::reading::Reading;
use crate::tui::spinner::Wait;

use super::{
    Again, App, Effect, Message, Neighbours, Open, PollRequest, ReadRequest, Results, Source, Step,
    Viewing, Way, file_name, sums_file, sums_name, viewer_command,
};

/// A file of no more bytes is read whole once its first part shows, and the end of a file no
/// further from what was read is read up to rather than jumped to: as much as the viewer read
/// before it showed anything, when its parts were that large, so that such files still have
/// all their lines counted.
const READ_AHEAD: u64 = 16 << 20;

impl App {
    /// Views the file under the cursor of the active panel, as F3 does in mc; a directory
    /// opens as with Enter.
    pub(super) fn view(&mut self) -> Vec<Effect> {
        let side = self.active;
        let panel = self.panel(side);
        let file = panel
            .entry_under_cursor()
            .filter(|entry| !entry.is_dir_like())
            .map(|entry| child(panel.location(), &entry.name));
        let Some(location) = file else {
            if let Some(request) = self.panel_mut(side).handle(Action::Enter) {
                return self.open(self.shown(side), request);
            }
            return Vec::new();
        };
        let dir = self.panel(side).location().clone();
        let neighbours = Neighbours::Panel(self.shown(side));
        self.view_file(location, &dir, None, neighbours)
    }

    /// Views the file at `location`, in `dir`, from `line` if there is one, among
    /// `neighbours`.
    pub(super) fn view_file(
        &mut self,
        location: Location,
        dir: &Location,
        line: Option<u64>,
        neighbours: Neighbours,
    ) -> Vec<Effect> {
        let host = self.handle_for(&location).ok().flatten();
        let sizes = self.viewer_sizes(Some(&location));
        let mut viewer = Viewer::new(location_text(&location), sizes);
        if let Some(line) = line {
            let line = usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX);
            viewer.handle(ViewerCommand::Top(line));
        }
        self.open_viewer(viewer, Source::File { location, host }, neighbours);
        let mut effects = self.read_more();
        effects.extend(self.note(dir));
        effects
    }

    /// Views the file at `location`, in `dir`, from `line`, in place of the one the viewer
    /// shows, which is among the same `neighbours`: `] f` and `[ f`. Its lines wrap, or are cut,
    /// as they were.
    fn view_beside(
        &mut self,
        location: Location,
        dir: &Location,
        line: Option<u64>,
        neighbours: Neighbours,
    ) -> Vec<Effect> {
        let wraps = self
            .viewing
            .as_ref()
            .is_none_or(|viewing| viewing.viewer.wraps());
        let effects = self.view_file(location, dir, line, neighbours);
        if let Some(viewing) = &mut self.viewing
            && viewing.viewer.wraps() != wraps
        {
            viewing.viewer.handle(ViewerCommand::ToggleWrap);
        }
        effects
    }

    /// Views the file at `location` for `question`, about a taken name, which comes back in
    /// front once the viewer closes.
    pub(super) fn view_for(&mut self, location: Location, question: Open) -> Vec<Effect> {
        let host = self.handle_for(&location).ok().flatten();
        let sizes = self.viewer_sizes(Some(&location));
        let viewer = Viewer::new(location_text(&location), sizes);
        self.open_viewer(viewer, Source::File { location, host }, Neighbours::Alone);
        if let Some(viewing) = &mut self.viewing {
            viewing.question = Some(Box::new(question));
        }
        self.read_more()
    }

    /// Views `bytes`, the file a save would write, under `name`, for `question`, about a taken
    /// name, which comes back in front once the viewer closes.
    pub(super) fn view_text_for(
        &mut self,
        name: String,
        bytes: Vec<u8>,
        question: Open,
    ) -> Vec<Effect> {
        let viewer = Viewer::new(cells::sanitize(name.as_bytes()), self.viewer_sizes(None));
        let source = Source::Text {
            name,
            bytes,
            results: None,
        };
        self.open_viewer(viewer, source, Neighbours::Alone);
        if let Some(viewing) = &mut self.viewing {
            viewing.question = Some(Box::new(question));
        }
        self.read_more()
    }

    /// Views the log of this run from its end, in front of the windows and the dialogs, which
    /// are there again once it closes; a dialog that comes in front of them later stands in
    /// front of it. Without a log, says why there is none.
    pub(super) fn view_log(&mut self) -> Vec<Effect> {
        let log = self.about.files.as_ref().map(|files| &files.log);
        let path = match log {
            Some(Ok(path)) => path.clone(),
            Some(Err(reason)) => {
                let reason = cells::sanitize(reason.as_bytes());
                self.message = Some(Message::Warning(fl!("log-none", reason = reason)));
                return Vec::new();
            }
            None => return Vec::new(),
        };
        let location = Location::Local(path);
        let sizes = self.viewer_sizes(Some(&location));
        let mut viewer = Viewer::new(location_text(&location), sizes);
        viewer.handle(ViewerCommand::Scroll(Motion::End));
        self.open_viewer(
            viewer,
            Source::File {
                location,
                host: None,
            },
            Neighbours::Dir,
        );
        // A question that the viewer it replaces held is among the dialogs again by now.
        let under = self.dialogs.iter().map(|open| open.dialog.id()).collect();
        if let Some(viewing) = &mut self.viewing {
            viewing.log = Some(under);
        }
        // The windows it covers go at once, rather than shrink over it.
        self.frames.stage.settle();
        self.read_more()
    }

    /// Whether the viewer is in front of the windows: for a question about a taken name, with
    /// the log, or over the location menu.
    pub(super) fn viewer_over(&self) -> bool {
        self.viewing.as_ref().is_some_and(|viewing| {
            viewing.question.is_some() || viewing.log.is_some() || viewing.menu
        })
    }

    /// The dialog in front, unless the log covers it: the log stands in front of the dialogs
    /// that were open when it opened.
    pub(super) fn dialog_in_front(&self) -> Option<&Open> {
        let open = self.dialogs.front()?;
        let covered = self
            .viewing
            .as_ref()
            .and_then(|viewing| viewing.log.as_ref())
            .is_some_and(|under| under.contains(&open.dialog.id()));
        (!covered).then_some(open)
    }

    /// Views the checksums of the window `results` as the lines of a file of them, under the
    /// name Save offers; the window goes back in front once the viewer closes (ADR 0058).
    pub(super) fn view_sums(&mut self, results: Results) -> Vec<Effect> {
        let name = sums_name(&results);
        let bytes = sums_file(&results.lines);
        let viewer = Viewer::new(cells::sanitize(name.as_bytes()), self.viewer_sizes(None));
        let results = Some(Box::new(results));
        self.open_viewer(
            viewer,
            Source::Text {
                name,
                bytes,
                results,
            },
            Neighbours::Alone,
        );
        self.read_more()
    }

    /// How much of the file at `location`, or of text the app holds, the viewer reads at once,
    /// and holds: `viewer.part_mib` and `viewer.window_mib`; and how much it reads ahead of the
    /// screen. On a host, where every part read is sent, a part is `viewer.host_part_kib`, and
    /// it reads nothing ahead.
    fn viewer_sizes(&self, location: Option<&Location>) -> ViewerSizes {
        let bytes = |count: std::num::NonZeroUsize, unit: u64| {
            u64::try_from(count.get())
                .unwrap_or(u64::MAX)
                .saturating_mul(unit)
        };
        let (kibibyte, mebibyte) = (1 << 10, 1 << 20);
        let viewer = &self.config.viewer;
        let (part, ahead) = match location {
            Some(Location::Remote { .. }) => (bytes(viewer.host_part_kib, kibibyte), 0),
            _ => (bytes(viewer.part_mib, mebibyte), READ_AHEAD),
        };
        ViewerSizes {
            part,
            window: bytes(viewer.window_mib, mebibyte),
            ahead,
        }
    }

    /// Shows `viewer` on `source`, among `neighbours`, in place of the viewer on screen, if
    /// any; in front of the location menu if that is open, which is not drawn under a viewer.
    fn open_viewer(&mut self, viewer: Viewer, source: Source, neighbours: Neighbours) {
        self.close_viewer();
        self.last_job += 1;
        self.viewing = Some(Viewing {
            id: self.last_job,
            viewer,
            wait: Wait::default(),
            source,
            reading: None,
            finished: None,
            cancel: CancellationToken::new(),
            question: None,
            log: None,
            menu: self.menu.is_some(),
            neighbours,
            stepping: None,
            polling: false,
        });
    }

    /// Reads the part the viewer wants next, unless it reads one already; while it follows the
    /// file and wants none, looks at the file again a moment later. Text that the app holds
    /// gives every part it wants at once.
    pub(super) fn read_more(&mut self) -> Vec<Effect> {
        let Some(viewing) = &mut self.viewing else {
            return Vec::new();
        };
        if viewing.reading.is_some() {
            return Vec::new();
        }
        let (location, host) = match &viewing.source {
            Source::File { location, host } => (location, host),
            Source::Text { bytes, .. } => {
                // A part it takes and wants again would come round for ever.
                let mut last = None;
                while let Some(want) = viewing.viewer.want().filter(|want| Some(*want) != last) {
                    viewing.viewer.take(part_of(bytes, want));
                    last = Some(want);
                }
                return Vec::new();
            }
        };
        let Some(want) = viewing.viewer.want() else {
            if !viewing.viewer.following() || viewing.polling {
                return Vec::new();
            }
            viewing.polling = true;
            return vec![Effect::Poll(PollRequest {
                id: viewing.id,
                location: location.clone(),
                host: host.clone(),
                cancel: viewing.cancel.clone(),
            })];
        };
        let stop = CancellationToken::new();
        viewing.reading = Some(Reading::new(want, stop.clone()));
        vec![Effect::Read(ReadRequest {
            id: viewing.id,
            location: location.clone(),
            host: host.clone(),
            want,
            stop,
            cancel: viewing.cancel.clone(),
        })]
    }

    /// Takes the part [`Effect::Read`] read for the viewer `id`, and reads the next one it
    /// wants. A file that changed is read again from the start. An error closes the viewer
    /// while it has nothing to show; after that, the viewer keeps what it holds.
    pub(crate) fn read(&mut self, id: u64, result: Result<ViewerPart, Reason>) -> Vec<Effect> {
        let Some(viewing) = self.viewing.as_mut().filter(|viewing| viewing.id == id) else {
            return Vec::new();
        };
        let reading = viewing.reading.take();
        match result {
            Ok(part) => {
                if !part.stopped {
                    viewing.finished = reading.and_then(Reading::finish);
                }
                if viewing.viewer.take(part) == Taken::Changed {
                    let path = viewing.name();
                    self.message = Some(Message::Warning(fl!("viewer-changed", path = path)));
                }
            }
            Err(reason) => {
                let path = viewing.name();
                let reason = reason.sanitized();
                if viewing.viewer.loading() {
                    let asked = viewing.question.is_some();
                    let log = viewing.log.is_some();
                    let neighbours = viewing.neighbours;
                    let location = match &viewing.source {
                        Source::File { location, .. } => Some(location.clone()),
                        Source::Text { .. } => None,
                    };
                    self.close_viewer();
                    let problem = Problem::new(fl!("viewer-error"), reason).at(path);
                    // Over the question, which is in sight under it, and whose line views the
                    // file again.
                    match (asked, location) {
                        (true, _) => self.show_problem_over(&problem),
                        (false, Some(_)) if log => {
                            self.show_problem_way(&problem, Way::Again(Again::ViewLog));
                        }
                        (false, Some(location)) => {
                            let again = Again::View(location, neighbours);
                            self.show_problem_way(&problem, Way::Again(again));
                        }
                        (false, None) => self.show_problem(&problem),
                    }
                } else {
                    viewing.viewer.stop();
                    let problem = Problem::new(fl!("viewer-read-error"), reason).at(path);
                    self.show_problem_way(&problem, Way::Again(Again::ReadMore));
                }
                return Vec::new();
            }
        }
        self.read_more()
    }

    /// Shows how far [`Effect::Read`] is with the part of the viewer `id`.
    pub(crate) fn read_progress(&mut self, id: u64, progress: ReadProgress) {
        if let Some(reading) = self
            .viewing
            .as_mut()
            .filter(|viewing| viewing.id == id)
            .and_then(|viewing| viewing.reading.as_mut())
        {
            reading.progress(progress);
        }
    }

    /// Takes a key for the viewer, or for the window of the part it reads while it shows: F1
    /// shows the help over it. A move may ask for another part.
    pub(super) fn handle_viewer(&mut self, input: Resolved) -> Vec<Effect> {
        if let Some(viewing) = &mut self.viewing
            && viewing.window_shown()
            && let Some(reading) = &mut viewing.reading
        {
            if reading.handle(input) {
                reading.stop();
                viewing.viewer.stop();
            }
            return Vec::new();
        }
        let Resolved::Action(action) = input else {
            return Vec::new();
        };
        match action {
            // The help would open under it.
            Action::Help if self.viewer_over() => {}
            Action::Help => {
                self.help = Some(self.help_screen());
            }
            Action::Quit | Action::Cancel => self.close_viewer(),
            Action::Edit => return self.edit_viewed(),
            Action::SystemOpen => return self.open_viewed(),
            Action::Reload => return self.reload_viewed(),
            Action::NextFile | Action::PrevFile => {
                return self.view_neighbour(action == Action::NextFile);
            }
            Action::Follow => return self.follow_viewed(),
            action => {
                if let Some(viewing) = &mut self.viewing
                    && let Some(command) = viewer_command(action)
                {
                    viewing.viewer.handle(command);
                    return self.read_more();
                }
            }
        }
        Vec::new()
    }

    /// Edits the file of the viewer with F4, at the line on top, once its start is read: after
    /// a question if it looks binary or is large. The viewer stays, and reads the file again
    /// once the editor changed it.
    fn edit_viewed(&mut self) -> Vec<Effect> {
        let Some((location, sniffed, line)) = self.viewing.as_ref().and_then(Viewing::to_edit)
        else {
            return Vec::new();
        };
        let (Some(dir), Some(name)) = (location.parent(), file_name(&location)) else {
            return Vec::new();
        };
        self.edit_sniffed(dir, name, line, sniffed)
    }

    /// Opens the file of the viewer with the programs of the desktop, as `g x` does in the
    /// panels; on a host, the line of the mode says that only local files open so.
    fn open_viewed(&mut self) -> Vec<Effect> {
        let Some(Viewing {
            source: Source::File { location, .. },
            ..
        }) = &self.viewing
        else {
            return Vec::new();
        };
        match location {
            Location::Local(path) => vec![Effect::Open(vec![path.clone()])],
            Location::Remote { .. } => {
                self.message = Some(Message::Warning(fl!("message-open-remote")));
                Vec::new()
            }
        }
    }

    /// Reads the file of the viewer again, up to the line on top, as `R` reads a directory
    /// again in the panels.
    fn reload_viewed(&mut self) -> Vec<Effect> {
        if let Some(viewing) = &mut self.viewing
            && matches!(viewing.source, Source::File { .. })
        {
            viewing.viewer.reload();
        }
        self.read_more()
    }

    /// Reads the file at `location` again in the viewer, if it shows it: the editor changed it.
    pub(super) fn reread(&mut self, location: &Location) -> Vec<Effect> {
        let shows = self.viewing.as_ref().is_some_and(|viewing| {
            matches!(&viewing.source, Source::File { location: shown, .. } if shown == location)
        });
        if shows {
            return self.reload_viewed();
        }
        Vec::new()
    }

    /// Follows the end of the file of the viewer as it grows, as `tail -f` and less's `F`; or
    /// stops.
    fn follow_viewed(&mut self) -> Vec<Effect> {
        if let Some(viewing) = &mut self.viewing
            && matches!(viewing.source, Source::File { .. })
        {
            let on = !viewing.viewer.following();
            viewing.viewer.follow(on);
        }
        self.read_more()
    }

    /// Takes how the file of the viewer `id`, which it follows, is now: what was added to it is
    /// read, and a file that changed otherwise is read again. A file that cannot be looked at
    /// stops the following, and the line of the mode says why.
    pub(crate) fn polled(&mut self, id: u64, result: Result<Stamp, Reason>) -> Vec<Effect> {
        let Some(viewing) = self.viewing.as_mut().filter(|viewing| viewing.id == id) else {
            return Vec::new();
        };
        viewing.polling = false;
        if !viewing.viewer.following() {
            return Vec::new();
        }
        let path = viewing.name();
        match result {
            Ok(stamp) => {
                if viewing.viewer.check(stamp) == Checked::Changed {
                    self.message = Some(Message::Warning(fl!("viewer-changed", path = path)));
                }
            }
            Err(reason) => {
                viewing.viewer.follow(false);
                let reason = reason.sanitized().text;
                self.message = Some(Message::Warning(fl!(
                    "viewer-follow-stopped",
                    path = path,
                    reason = reason
                )));
                return Vec::new();
            }
        }
        self.read_more()
    }

    /// Views the file a count of files after the one the viewer shows, or before it, among its
    /// neighbours: the files of the panel it was viewed from, whose cursor goes along; those the
    /// search of a tree found, likewise; or those of its directory, which it lists first. No
    /// further than the last or the first.
    fn view_neighbour(&mut self, forward: bool) -> Vec<Effect> {
        let count = self.extra.times();
        let Some(Viewing {
            source: Source::File { location, .. },
            neighbours,
            ..
        }) = &self.viewing
        else {
            return Vec::new();
        };
        let (location, neighbours) = (location.clone(), *neighbours);
        match neighbours {
            Neighbours::Panel(id) => {
                let Some(name) = file_name(&location) else {
                    return Vec::new();
                };
                let Some(panel) = self.panel_of_mut(id) else {
                    return Vec::new();
                };
                let Some(next) = panel.file_beside(&name, forward, count) else {
                    return Vec::new();
                };
                panel.put_cursor_on(&next);
                let dir = panel.location().clone();
                self.view_beside(child(&dir, &next), &dir, None, neighbours)
            }
            Neighbours::Found => {
                let event = self
                    .find
                    .as_mut()
                    .and_then(|find| find.step_file(forward, count));
                let Some(FindEvent::View {
                    location: dir,
                    name,
                    line,
                }) = event
                else {
                    return Vec::new();
                };
                self.view_beside(child(&dir, &name), &dir, line, neighbours)
            }
            Neighbours::Dir => {
                let Some(dir) = location.parent() else {
                    return Vec::new();
                };
                let Ok(host) = self.handle_for(&dir) else {
                    return Vec::new();
                };
                self.last_job += 1;
                let id = self.last_job;
                if let Some(viewing) = &mut self.viewing {
                    viewing.stepping = Some(Step { id, forward, count });
                }
                vec![Effect::ListSiblings { id, dir, host }]
            }
            Neighbours::Alone => Vec::new(),
        }
    }

    /// Takes the entries of the directory of the file of the viewer, listed for the step `id` of
    /// `] f` or `[ f`, and views the file it goes to there, in the order of the active panel.
    pub(crate) fn siblings(
        &mut self,
        id: u64,
        result: Result<Vec<DirEntry>, Reason>,
    ) -> Vec<Effect> {
        let Some(viewing) = &mut self.viewing else {
            return Vec::new();
        };
        let Some(step) = viewing.stepping.take_if(|step| step.id == id) else {
            return Vec::new();
        };
        let Source::File { location, .. } = &viewing.source else {
            return Vec::new();
        };
        let location = location.clone();
        let (Some(dir), Some(name)) = (location.parent(), file_name(&location)) else {
            return Vec::new();
        };
        let entries = match result {
            Ok(entries) => entries,
            Err(reason) => {
                let (path, reason) = (location_text(&dir), reason.sanitized().text);
                self.message = Some(Message::Warning(fl!(
                    "viewer-list-error",
                    path = path,
                    reason = reason
                )));
                return Vec::new();
            }
        };
        let files = panel::files_in_order(entries, self.panel(self.active));
        let Some(next) = panel::beside(&files, &name, step.forward, step.count) else {
            return Vec::new();
        };
        self.view_beside(child(&dir, &next), &dir, None, Neighbours::Dir)
    }

    /// Closes the viewer; the window of checksums whose lines it showed, or the question whose
    /// file it showed, goes back in front.
    pub(super) fn close_viewer(&mut self) {
        if let Some(viewing) = self.viewing.take() {
            viewing.cancel.cancel();
            if let Source::Text {
                results: Some(results),
                ..
            } = viewing.source
            {
                self.results.push_front(*results);
            }
            if let Some(question) = viewing.question {
                self.dialogs.push_front(*question);
            }
        }
    }
}
