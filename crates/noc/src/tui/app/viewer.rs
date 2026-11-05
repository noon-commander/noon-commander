//! The F3 viewer and the reads it waits for.

use noc_vfs::Location;
use noc_viewer::{
    Command as ViewerCommand, Part as ViewerPart, Progress as ReadProgress, Sizes as ViewerSizes,
    Taken, Viewer, part_of,
};
use tokio_util::sync::CancellationToken;

use crate::i18n::fl;
use crate::tui::cells::{self};
use crate::tui::describe::Reason;
use crate::tui::dialog::Problem;
use crate::tui::keymap::{Action, Resolved};
use crate::tui::panel::{child, location_text};
use crate::tui::reading::{ReadButton, Reading};
use crate::tui::spinner::Wait;

use super::{
    Again, App, Effect, Open, ReadRequest, Results, Source, Viewing, Way, sums_file, sums_name,
    viewer_command,
};

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
        self.view_file(location, &dir, None)
    }

    /// Views the file at `location`, in `dir`, from `line` if there is one.
    pub(super) fn view_file(
        &mut self,
        location: Location,
        dir: &Location,
        line: Option<u64>,
    ) -> Vec<Effect> {
        let host = self.handle_for(&location).ok().flatten();
        let mut viewer = Viewer::new(location_text(&location), self.viewer_sizes());
        if let Some(line) = line {
            let line = usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX);
            viewer.handle(ViewerCommand::Top(line));
        }
        self.open_viewer(viewer, Source::File { location, host });
        let mut effects = self.read_more();
        effects.extend(self.note(dir));
        effects
    }

    /// Views the file at `location` for `question`, about a taken name, which comes back in
    /// front once the viewer closes.
    pub(super) fn view_for(&mut self, location: Location, question: Open) -> Vec<Effect> {
        let host = self.handle_for(&location).ok().flatten();
        let viewer = Viewer::new(location_text(&location), self.viewer_sizes());
        self.open_viewer(viewer, Source::File { location, host });
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
        let viewer = Viewer::new(cells::sanitize(name.as_bytes()), self.viewer_sizes());
        let source = Source::Text {
            name,
            bytes,
            results: None,
        };
        self.open_viewer(viewer, source);
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
                self.message = Some(fl!("log-none", reason = reason));
                return Vec::new();
            }
            None => return Vec::new(),
        };
        let location = Location::Local(path);
        let mut viewer = Viewer::new(location_text(&location), self.viewer_sizes());
        viewer.handle(ViewerCommand::Top(usize::MAX));
        self.open_viewer(
            viewer,
            Source::File {
                location,
                host: None,
            },
        );
        // A question that the viewer it replaces held is among the dialogs again by now.
        let under = self.dialogs.iter().map(|open| open.dialog.id()).collect();
        if let Some(viewing) = &mut self.viewing {
            viewing.log = Some(under);
        }
        // The windows it covers go at once, rather than shrink over it.
        self.stage.settle();
        self.read_more()
    }

    /// Whether the viewer is in front of the windows: for a question about a taken name, or
    /// with the log.
    pub(super) fn viewer_over(&self) -> bool {
        self.viewing
            .as_ref()
            .is_some_and(|viewing| viewing.question.is_some() || viewing.log.is_some())
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
        let viewer = Viewer::new(cells::sanitize(name.as_bytes()), self.viewer_sizes());
        let results = Some(Box::new(results));
        self.open_viewer(
            viewer,
            Source::Text {
                name,
                bytes,
                results,
            },
        );
        self.read_more()
    }

    /// How much of a file the viewer reads at once, and holds: `viewer.part_mib` and
    /// `viewer.window_mib`.
    fn viewer_sizes(&self) -> ViewerSizes {
        let mebibytes = |count: std::num::NonZeroUsize| {
            u64::try_from(count.get())
                .unwrap_or(u64::MAX)
                .saturating_mul(1 << 20)
        };
        let viewer = &self.config.viewer;
        ViewerSizes {
            part: mebibytes(viewer.part_mib),
            window: mebibytes(viewer.window_mib),
        }
    }

    /// Shows `viewer` on `source`, in place of the viewer on screen, if any.
    fn open_viewer(&mut self, viewer: Viewer, source: Source) {
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
        });
    }

    /// Reads the part the viewer wants next, unless it reads one already. Text that the app
    /// holds gives every part it wants at once.
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
            return Vec::new();
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
                    self.message = Some(fl!("viewer-changed", path = path));
                }
            }
            Err(reason) => {
                let path = viewing.name();
                let reason = reason.sanitized();
                if viewing.viewer.loading() {
                    let asked = viewing.question.is_some();
                    let log = viewing.log.is_some();
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
                            self.show_problem_way(&problem, Way::Again(Again::View(location)));
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
            match reading.handle(input) {
                Some(ReadButton::Stop) => {
                    reading.stop();
                    viewing.viewer.stop();
                }
                Some(ReadButton::Close) => self.close_viewer(),
                None => {}
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
