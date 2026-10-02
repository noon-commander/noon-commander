//! The window of a running job: what it does, where it is, and a gauge.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, button_line, draw_box};
use super::keymap::{Action, Resolved};
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the window, borders included.
const WIDTH: u16 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// Waiting for other jobs to finish.
    Waiting,
    /// Counting what to do.
    Scanning { items: u64 },
    /// At `current`, `done` of `total` entries and `bytes` of their bytes.
    Working {
        current: String,
        done: u64,
        total: u64,
        bytes: (u64, u64),
    },
}

/// How far a job is, for its window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) done: u64,
    pub(crate) total: u64,
    /// Bytes done and in all; both zero for jobs that move no data.
    pub(crate) bytes_done: u64,
    pub(crate) bytes_total: u64,
}

/// A button of a job's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobButton {
    /// The job goes on behind the panels.
    Background,
    Abort,
}

/// The window of a job, with Background, the default, and Abort.
#[derive(Debug)]
pub(crate) struct JobView {
    title: String,
    /// What the job does, such as `Deleting`.
    doing: String,
    stage: Stage,
    aborting: bool,
    buttons: Vec<JobButton>,
    focus: usize,
}

impl JobView {
    pub(crate) fn new(title: String, doing: String) -> Self {
        Self {
            title,
            doing,
            stage: Stage::Scanning { items: 0 },
            aborting: false,
            buttons: vec![JobButton::Background, JobButton::Abort],
            focus: 0,
        }
    }

    /// Leaves Abort the only button, for a job that must stay in front.
    pub(crate) fn keep_in_front(&mut self) {
        self.buttons = vec![JobButton::Abort];
        self.focus = 0;
    }

    /// The job waits its turn.
    pub(crate) fn wait(&mut self) {
        self.stage = Stage::Waiting;
    }

    pub(crate) fn scanning(&mut self, items: u64) {
        self.stage = Stage::Scanning { items };
    }

    /// The job is at `current`, shown as given, with `counts` behind it.
    pub(crate) fn working(&mut self, current: String, counts: Counts) {
        self.stage = Stage::Working {
            current,
            done: counts.done,
            total: counts.total,
            bytes: (counts.bytes_done, counts.bytes_total),
        };
    }

    /// How far the job is, from 0 to 1: by bytes if it moves data, else by entries.
    pub(crate) fn ratio(&self) -> f64 {
        match self.stage {
            Stage::Waiting | Stage::Scanning { .. } => 0.0,
            Stage::Working {
                done,
                total,
                bytes: (_, 0),
                ..
            }
            | Stage::Working {
                bytes: (done, total),
                ..
            } => ratio(done, total),
        }
    }

    /// What the job does, such as `Copy`.
    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    /// How far the job is, in a word or a percentage, and the entry at hand: for the list of
    /// jobs.
    pub(crate) fn summary(&self) -> (String, String) {
        let state = match &self.stage {
            _ if self.aborting => fl!("jobs-state-aborting"),
            Stage::Waiting => fl!("jobs-state-waiting"),
            Stage::Scanning { .. } => fl!("jobs-state-counting"),
            Stage::Working { .. } => {
                // A ratio within 0 … 1.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let percent = (self.ratio() * 100.0).round() as u64;
                fl!("jobs-state-percent", percent = percent.to_string())
            }
        };
        let current = match &self.stage {
            Stage::Working { current, .. } => current.clone(),
            Stage::Waiting | Stage::Scanning { .. } => String::new(),
        };
        (state, current)
    }

    /// Takes a key: Enter or Space presses the focused button, Esc aborts, as in mc, and
    /// arrows and Tab move the focus.
    pub(crate) fn handle(&mut self, input: Resolved) -> Option<JobButton> {
        let Resolved::Action(action) = input else {
            return None;
        };
        let count = self.buttons.len();
        match action {
            Action::Cancel => Some(JobButton::Abort),
            Action::Confirm | Action::Toggle => Some(self.buttons[self.focus]),
            Action::NextField | Action::Right | Action::Down => {
                self.focus = (self.focus + 1) % count;
                None
            }
            Action::PrevField | Action::Left | Action::Up => {
                self.focus = (self.focus + count - 1) % count;
                None
            }
            _ => None,
        }
    }

    /// Shows that the job was asked to stop.
    pub(crate) fn abort(&mut self) {
        self.aborting = true;
    }

    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let colors = Colors::of(theme, false);
        // Borders, what it does, where, the gauge, the count, a blank line, Abort.
        let inner = draw_box(frame, area, (WIDTH, 8), &self.title, colors, theme.shadow);
        let width = usize::from(inner.width);
        // On a small screen, what fits.
        let mut put = |index: u16, line: Line<'static>| {
            if index < inner.height {
                let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
                frame.render_widget(line, row);
            }
        };
        let (doing, current, ratio, count) = match &self.stage {
            Stage::Waiting => (fl!("job-waiting"), String::new(), 0.0, String::new()),
            Stage::Scanning { items } => (
                fl!("job-scanning"),
                String::new(),
                0.0,
                fl!("job-found", items = items.to_string()),
            ),
            Stage::Working {
                current,
                done,
                total,
                bytes: (bytes_done, bytes_total),
            } => {
                let done_total = (done.to_string(), total.to_string());
                // Data takes the time, so the gauge follows the bytes, if there are any.
                let (ratio, count) = if *bytes_total > 0 {
                    let count = fl!(
                        "job-count-bytes",
                        done = done_total.0,
                        total = done_total.1,
                        bytes_done = cells::size(*bytes_done, 7),
                        bytes_total = cells::size(*bytes_total, 7)
                    );
                    (ratio(*bytes_done, *bytes_total), count)
                } else {
                    let count = fl!("job-count", done = done_total.0, total = done_total.1);
                    (ratio(*done, *total), count)
                };
                (self.doing.clone(), current.clone(), ratio, count)
            }
        };
        let doing = if self.aborting {
            fl!("job-aborting")
        } else {
            doing
        };
        put(0, Line::raw(cells::fit(&doing, width, Align::Left)));
        put(1, Line::raw(cells::fit(&current, width, Align::Left)));
        put(2, gauge(ratio, width, theme));
        put(3, Line::raw(cells::fit(&count, width, Align::Left)));
        let labels: Vec<String> = self
            .buttons
            .iter()
            .map(|button| match button {
                JobButton::Background => fl!("job-background"),
                JobButton::Abort => fl!("dialog-abort"),
            })
            .collect();
        put(5, button_line(&labels, 0, Some(self.focus), colors));
    }
}

/// `done` of `total`, from 0 to 1.
fn ratio(done: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    // Counts and sizes stay far below 2^52, where `f64` would round them.
    #[allow(clippy::cast_precision_loss)]
    let ratio = done as f64 / total as f64;
    ratio.clamp(0.0, 1.0)
}

/// A bar of `width` cells, filled for `ratio`, and the percentage after it. Block characters
/// show in any theme, where colors alone would not.
fn gauge(ratio: f64, width: usize, theme: &Theme) -> Line<'static> {
    let percent = format!(" {:>3.0}%", ratio * 100.0);
    let bar = width.saturating_sub(percent.len());
    // `ratio` is within 0 … 1, and bars are a few dozen cells.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let filled = ((bar as f64 * ratio).round() as usize).min(bar);
    let text = format!("{}{}", "█".repeat(filled), "░".repeat(bar - filled));
    Line::from(vec![Span::styled(text, theme.gauge), Span::raw(percent)])
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn draw(view: &JobView) -> String {
        let mut terminal = Terminal::new(TestBackend::new(64, 10)).unwrap();
        terminal
            .draw(|frame| view.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn shows_counting_then_progress() {
        let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
        view.scanning(42);
        let text = draw(&view);
        assert!(text.contains("Counting"), "{text}");
        assert!(text.contains("42 found"), "{text}");

        let counts = Counts {
            done: 3,
            total: 12,
            ..Counts::default()
        };
        view.working("/srv/www/index.html".to_owned(), counts);
        insta::assert_snapshot!(draw(&view));

        // With bytes, the gauge follows them.
        let counts = Counts {
            done: 1,
            total: 2,
            bytes_done: 1536,
            bytes_total: 2048,
        };
        view.working("big".to_owned(), counts);
        let text = draw(&view);
        assert!(text.contains("1 of 2, 1536 of 2048 bytes"), "{text}");
        assert!(text.contains(" 75%"), "{text}");

        view.abort();
        assert!(draw(&view).contains("Aborting"));

        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        view.wait();
        assert!(draw(&view).contains("Waiting for other jobs"));
        assert!(view.ratio() < f64::EPSILON);
    }

    #[test]
    fn the_gauge_fills_by_ratio() {
        let theme = Theme::terminal();
        let text = |ratio| gauge(ratio, 15, &theme).to_string();
        assert_eq!(text(0.0), "░░░░░░░░░░   0%");
        assert_eq!(text(0.5), "█████░░░░░  50%");
        assert_eq!(text(1.0), "██████████ 100%");
    }

    #[test]
    fn enter_sends_to_the_background_and_esc_aborts() {
        let press = |view: &mut JobView, action| view.handle(Resolved::Action(action));
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        assert!(draw(&view).contains("[< Background >] [ Abort ]"));
        assert_eq!(
            press(&mut view, Action::Confirm),
            Some(JobButton::Background)
        );
        assert_eq!(press(&mut view, Action::Cancel), Some(JobButton::Abort));
        assert_eq!(press(&mut view, Action::Right), None);
        assert_eq!(press(&mut view, Action::Toggle), Some(JobButton::Abort));
        assert_eq!(press(&mut view, Action::PrevField), None);
        assert_eq!(
            press(&mut view, Action::Confirm),
            Some(JobButton::Background)
        );
        assert_eq!(view.handle(Resolved::Insert('a')), None);

        view.keep_in_front();
        assert!(!draw(&view).contains("Background"));
        for action in [Action::Cancel, Action::Confirm, Action::Toggle] {
            assert_eq!(press(&mut view, action), Some(JobButton::Abort));
        }
        assert_eq!(press(&mut view, Action::Right), None);
        assert_eq!(press(&mut view, Action::Confirm), Some(JobButton::Abort));
    }

    #[test]
    fn the_ratio_follows_bytes_or_entries() {
        let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
        assert!(view.ratio() < f64::EPSILON);
        let counts = Counts {
            done: 1,
            total: 4,
            ..Counts::default()
        };
        view.working(String::new(), counts);
        assert!((view.ratio() - 0.25).abs() < f64::EPSILON);
        let counts = Counts {
            done: 1,
            total: 4,
            bytes_done: 3,
            bytes_total: 4,
        };
        view.working(String::new(), counts);
        assert!((view.ratio() - 0.75).abs() < f64::EPSILON);
    }
}
