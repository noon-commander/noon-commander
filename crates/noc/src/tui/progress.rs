//! The window of a running job: what it does, where it is, a gauge, and its time and speed.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, button_line, button_spots, draw_box, draw_separator, frame_around};
use super::keymap::{Action, Resolved};
use super::mouse::{Drawn, Pointer};
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the window, borders included.
const WIDTH: u16 = 60;
/// Work the speed needs behind it before it means anything.
const SETTLED: Duration = Duration::from_secs(1);
/// What stands for a time or a speed that is not known yet.
const UNKNOWN_TIME: &str = "-:--";
const UNKNOWN_SPEED: &str = "-";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// Waiting for other jobs to finish.
    Waiting,
    /// Counting what to do.
    Scanning { items: u64 },
    /// At `current`, with `counts` behind it.
    Working { current: String, counts: Counts },
}

/// How far a job is, for its window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) done: u64,
    pub(crate) total: u64,
    /// Bytes done and in all; both zero for jobs that move no data.
    pub(crate) bytes_done: u64,
    pub(crate) bytes_total: u64,
    /// Bytes read and written, which the speed follows.
    pub(crate) bytes_copied: u64,
}

/// The time a job has worked. It stands while the job waits for an answer, so that a question
/// left open lowers neither the speed nor the time left; mc counts that time too.
#[derive(Debug, Clone, Copy, Default)]
struct Stopwatch {
    /// Up to the last stop.
    spent: Duration,
    /// Since when it runs, if it does.
    since: Option<Instant>,
}

impl Stopwatch {
    fn start(&mut self, now: Instant) {
        self.since.get_or_insert(now);
    }

    fn stop(&mut self, now: Instant) {
        if let Some(since) = self.since.take() {
            self.spent += now.saturating_duration_since(since);
        }
    }

    fn running(&self) -> bool {
        self.since.is_some()
    }

    fn elapsed(&self, now: Instant) -> Duration {
        self.spent
            + self
                .since
                .map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }
}

/// The time and speed of a job at some moment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Timing {
    elapsed: Duration,
    /// Bytes per second on average, once there is enough behind it; for jobs that move data.
    speed: Option<f64>,
    /// The time left at that speed.
    left: Option<Duration>,
}

/// A button of a job's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobButton {
    /// The job goes on behind the panels.
    Background,
    Abort,
}

/// How far a job is, for the list of jobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Summary {
    /// In a word or a percentage.
    pub(crate) state: String,
    /// The time left, for jobs that move data, once it is known; otherwise empty.
    pub(crate) left: String,
    /// The entry at hand.
    pub(crate) current: String,
}

/// The window of a job, with Background, the default, and Abort.
#[derive(Debug)]
pub(crate) struct JobView {
    title: String,
    /// What the job does, such as `Deleting`.
    doing: String,
    stage: Stage,
    aborting: bool,
    /// The job waits for an answer to a question.
    asking: bool,
    clock: Stopwatch,
    buttons: Vec<JobButton>,
    focus: usize,
    /// Where the last render drew it, for the mouse.
    drawn: RefCell<Drawn>,
}

impl JobView {
    pub(crate) fn new(title: String, doing: String) -> Self {
        Self {
            title,
            doing,
            stage: Stage::Scanning { items: 0 },
            aborting: false,
            asking: false,
            clock: Stopwatch::default(),
            buttons: vec![JobButton::Background, JobButton::Abort],
            focus: 0,
            drawn: RefCell::default(),
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

    /// The job is at `current`, shown as given, with `counts` behind it. Its clock starts with
    /// the first of these, after the waiting and the counting.
    pub(crate) fn working(&mut self, current: String, counts: Counts, now: Instant) {
        self.stage = Stage::Working { current, counts };
        if !self.asking {
            self.clock.start(now);
        }
    }

    /// The job asked a question and waits for the answer: its clock stands, however long the
    /// question stays open or waits behind other dialogs.
    pub(crate) fn ask(&mut self, now: Instant) {
        self.asking = true;
        self.clock.stop(now);
    }

    /// The question was answered, and the job goes on.
    pub(crate) fn answered(&mut self, now: Instant) {
        self.asking = false;
        if matches!(self.stage, Stage::Working { .. }) {
            self.clock.start(now);
        }
    }

    /// Whether its time runs, so that what shows it needs drawing as time passes.
    pub(crate) fn ticking(&self) -> bool {
        self.clock.running()
    }

    /// How far the job is, from 0 to 1: by bytes if it moves data, else by entries.
    pub(crate) fn ratio(&self) -> f64 {
        match &self.stage {
            Stage::Waiting | Stage::Scanning { .. } => 0.0,
            Stage::Working { counts, .. } if counts.bytes_total > 0 => {
                ratio(counts.bytes_done, counts.bytes_total)
            }
            Stage::Working { counts, .. } => ratio(counts.done, counts.total),
        }
    }

    /// The time and speed at `now`, while the job works.
    fn timing(&self, now: Instant) -> Option<Timing> {
        let Stage::Working { counts, .. } = &self.stage else {
            return None;
        };
        let elapsed = self.clock.elapsed(now);
        let speed =
            (counts.bytes_total > 0 && counts.bytes_copied > 0 && elapsed >= SETTLED).then(|| {
                // Sizes stay far below 2^52, where `f64` would round them.
                #[allow(clippy::cast_precision_loss)]
                let copied = counts.bytes_copied as f64;
                copied / elapsed.as_secs_f64()
            });
        let left = speed.map(|speed| {
            #[allow(clippy::cast_precision_loss)]
            let left = counts.bytes_total.saturating_sub(counts.bytes_done) as f64;
            Duration::try_from_secs_f64(left / speed).unwrap_or(Duration::MAX)
        });
        Some(Timing {
            elapsed,
            speed,
            left,
        })
    }

    /// What the job does, such as `Copy`.
    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    /// How far the job is at `now`, for the list of jobs.
    pub(crate) fn summary(&self, now: Instant) -> Summary {
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
        let left = self
            .timing(now)
            .and_then(|timing| timing.left)
            .map(|left| fl!("jobs-left", left = duration(left)))
            .unwrap_or_default();
        let current = match &self.stage {
            Stage::Working { current, .. } => current.clone(),
            Stage::Waiting | Stage::Scanning { .. } => String::new(),
        };
        Summary {
            state,
            left,
            current,
        }
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

    /// Takes a press of the mouse, where the window was drawn last: a click on a button
    /// returns Enter, which presses it.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        let drawn = self.drawn.borrow().clone();
        drawn.button_press(pointer, &mut self.focus)
    }

    /// Shows that the job was asked to stop.
    pub(crate) fn abort(&mut self) {
        self.aborting = true;
    }

    /// Draws the window as it is at `now`.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme, now: Instant) {
        let colors = Colors::of(theme, false);
        // Borders, what it does, where, the gauge, the count, the time, a line, the buttons.
        let inner = draw_box(frame, area, (WIDTH, 9), &self.title, colors, theme);
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
            Stage::Working { current, counts } => {
                let done_total = (counts.done.to_string(), counts.total.to_string());
                // Data takes the time, so the gauge follows the bytes, if there are any.
                let count = if counts.bytes_total > 0 {
                    fl!(
                        "job-count-bytes",
                        done = done_total.0,
                        total = done_total.1,
                        bytes_done = cells::size(counts.bytes_done, 7),
                        bytes_total = cells::size(counts.bytes_total, 7)
                    )
                } else {
                    fl!("job-count", done = done_total.0, total = done_total.1)
                };
                (self.doing.clone(), current.clone(), self.ratio(), count)
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
        let time = self.time_line(now);
        put(4, Line::raw(cells::fit(&time, width, Align::Left)));
        let labels: Vec<String> = self
            .buttons
            .iter()
            .map(|button| match button {
                JobButton::Background => fl!("job-background"),
                JobButton::Abort => fl!("dialog-abort"),
            })
            .collect();
        let buttons = button_line(&labels, 0, Some(self.focus), colors);
        let row = Rect::new(inner.x, inner.y + 6, inner.width, 1);
        *self.drawn.borrow_mut() = Drawn {
            frame: frame_around(inner),
            rows: Vec::new(),
            buttons: if inner.height > 6 {
                button_spots(&buttons, row)
            } else {
                Vec::new()
            },
        };
        put(6, buttons);
        if inner.height > 6 {
            draw_separator(frame, inner, inner.y + 5, colors, theme);
        }
    }

    /// The time the job has worked, and for jobs that move data the time left and the speed.
    fn time_line(&self, now: Instant) -> String {
        let Some(timing) = self.timing(now) else {
            return String::new();
        };
        let elapsed = duration(timing.elapsed);
        let moves_data =
            matches!(&self.stage, Stage::Working { counts, .. } if counts.bytes_total > 0);
        if !moves_data {
            return fl!("job-elapsed", elapsed = elapsed);
        }
        let left = timing
            .left
            .map_or_else(|| UNKNOWN_TIME.to_owned(), duration);
        let speed = timing.speed.map_or_else(
            || UNKNOWN_SPEED.to_owned(),
            |speed| fl!("job-speed", size = speed_size(speed)),
        );
        fl!("job-timing", elapsed = elapsed, left = left, speed = speed)
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

/// A duration in whole seconds, as `M:SS`, or `H:MM:SS` from an hour on.
fn duration(duration: Duration) -> String {
    let seconds = duration.as_secs() + u64::from(duration.subsec_millis() >= 500);
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Bytes per second as a size: in powers of 1024, as sizes in panels, with a decimal below
/// 100 so that slow links still show change: `512B`, `1.5M`, `480K`.
fn speed_size(bytes: f64) -> String {
    let mut value = bytes.max(0.0);
    let mut unit = "B";
    for next in ["K", "M", "G", "T", "P"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    if unit == "B" || value >= 99.95 {
        format!("{value:.0}{unit}")
    } else {
        format!("{value:.1}{unit}")
    }
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

    fn draw_at(view: &JobView, now: Instant) -> String {
        let mut terminal = Terminal::new(TestBackend::new(64, 11)).unwrap();
        terminal
            .draw(|frame| view.render(frame, frame.area(), &Theme::terminal(), now))
            .unwrap();
        terminal.backend().to_string()
    }

    fn draw(view: &JobView) -> String {
        draw_at(view, Instant::now())
    }

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    /// A copy of 100 MiB, with `done` and `copied` MiB of it.
    fn copying(done: u64, copied: u64) -> Counts {
        const MIB: u64 = 1024 * 1024;
        Counts {
            done: 1,
            total: 2,
            bytes_done: done * MIB,
            bytes_total: 100 * MIB,
            bytes_copied: copied * MIB,
        }
    }

    #[test]
    fn shows_counting_then_progress() {
        let start = Instant::now();
        let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
        view.scanning(42);
        let text = draw_at(&view, start);
        assert!(text.contains("Counting"), "{text}");
        assert!(text.contains("42 found"), "{text}");
        assert!(!text.contains("Time"), "{text}");

        let counts = Counts {
            done: 3,
            total: 12,
            ..Counts::default()
        };
        view.working("/srv/www/index.html".to_owned(), counts, start);
        insta::assert_snapshot!(draw_at(&view, start + secs(75)));

        // With bytes, the gauge follows them.
        let counts = Counts {
            done: 1,
            total: 2,
            bytes_done: 1536,
            bytes_total: 2048,
            bytes_copied: 1536,
        };
        view.working("big".to_owned(), counts, start);
        let text = draw(&view);
        assert!(text.contains("1 of 2, 1536 of 2048 bytes"), "{text}");
        assert!(text.contains(" 75%"), "{text}");

        view.abort();
        assert!(draw(&view).contains("Aborting"));

        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        view.wait();
        assert!(draw(&view).contains("Waiting for other jobs"));
        assert!(view.ratio() < f64::EPSILON);
        assert!(!view.ticking(), "nothing to time yet");
    }

    #[test]
    fn a_copy_shows_its_time_speed_and_time_left() {
        let start = Instant::now();
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        view.working("big".to_owned(), copying(0, 0), start);
        let text = draw_at(&view, start + Duration::from_millis(400));
        assert!(text.contains("Time 0:00   ETA -:--   -"), "{text}");

        view.working("big".to_owned(), copying(20, 20), start + secs(10));
        insta::assert_snapshot!(draw_at(&view, start + secs(10)));
        let summary = view.summary(start + secs(10));
        assert_eq!(summary.left, "ETA 0:40");
    }

    #[test]
    fn time_waiting_for_an_answer_does_not_count() {
        let start = Instant::now();
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        view.working("a".to_owned(), copying(10, 10), start);
        // A question after 5 seconds stays open for an hour.
        view.ask(start + secs(5));
        assert!(!view.ticking());
        // Reports that were on their way change nothing.
        view.working("a".to_owned(), copying(10, 10), start + secs(6));
        assert!(!view.ticking());
        let open = start + secs(3605);
        let timing = view.timing(open).unwrap();
        assert_eq!(timing.elapsed, secs(5));
        view.answered(open);
        assert!(view.ticking());

        let timing = view.timing(open + secs(5)).unwrap();
        assert_eq!(timing.elapsed, secs(10));
        let speed = timing.speed.unwrap();
        assert!((speed - 1024.0 * 1024.0).abs() < 1.0, "1 MiB/s: {speed}");
        assert_eq!(timing.left.map(|left| left.as_secs()), Some(90));
        let text = draw_at(&view, open + secs(5));
        assert!(text.contains("Time 0:10   ETA 1:30   1.0M/s"), "{text}");
    }

    #[test]
    fn skipped_and_retried_bytes_do_not_bend_the_speed() {
        let start = Instant::now();
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        // 10 MiB in 10 seconds, then 80 MiB skipped at once, then a retry from the start of a
        // file: the speed follows the bytes that moved.
        view.working("a".to_owned(), copying(10, 10), start);
        view.working("b".to_owned(), copying(90, 10), start);
        let timing = view.timing(start + secs(10)).unwrap();
        assert!((timing.speed.unwrap() - 1024.0 * 1024.0).abs() < 1.0);
        assert_eq!(timing.left.map(|left| left.as_secs()), Some(10));
        view.working("b".to_owned(), copying(85, 15), start);
        let timing = view.timing(start + secs(10)).unwrap();
        assert!((timing.speed.unwrap() - 1.5 * 1024.0 * 1024.0).abs() < 1.0);
        assert_eq!(timing.left.map(|left| left.as_secs()), Some(10));
    }

    #[test]
    fn a_question_before_the_work_does_not_start_the_clock() {
        let start = Instant::now();
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        view.ask(start);
        view.answered(start + secs(5));
        assert!(!view.ticking(), "still counting");
        view.working("a".to_owned(), copying(0, 0), start + secs(7));
        assert_eq!(view.timing(start + secs(9)).unwrap().elapsed, secs(2));
    }

    #[test]
    fn jobs_without_data_show_only_their_time() {
        let start = Instant::now();
        let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
        let counts = Counts {
            done: 1,
            total: 4,
            ..Counts::default()
        };
        view.working("a".to_owned(), counts, start);
        let text = draw_at(&view, start + secs(3_725));
        assert!(text.contains("Time 1:02:05"), "{text}");
        assert!(!text.contains("ETA"), "{text}");
        assert_eq!(view.summary(start + secs(10)).left, "");
    }

    #[test]
    fn durations_and_speeds_read_short() {
        assert_eq!(duration(Duration::ZERO), "0:00");
        assert_eq!(duration(Duration::from_millis(59_600)), "1:00");
        assert_eq!(duration(secs(754)), "12:34");
        assert_eq!(duration(secs(36_000)), "10:00:00");
        assert_eq!(speed_size(512.0), "512B");
        assert_eq!(speed_size(1536.0), "1.5K");
        assert_eq!(speed_size(480.0 * 1024.0), "480K");
        assert_eq!(speed_size(12.34 * 1024.0 * 1024.0), "12.3M");
        assert_eq!(speed_size(99.97 * 1024.0), "100K");
        assert_eq!(speed_size(3.0 * 1024.0 * 1024.0 * 1024.0), "3.0G");
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
        let now = Instant::now();
        let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
        assert!(view.ratio() < f64::EPSILON);
        let counts = Counts {
            done: 1,
            total: 4,
            ..Counts::default()
        };
        view.working(String::new(), counts, now);
        assert!((view.ratio() - 0.25).abs() < f64::EPSILON);
        let counts = Counts {
            done: 1,
            total: 4,
            bytes_done: 3,
            bytes_total: 4,
            bytes_copied: 3,
        };
        view.working(String::new(), counts, now);
        assert!((view.ratio() - 0.75).abs() < f64::EPSILON);
    }
}
