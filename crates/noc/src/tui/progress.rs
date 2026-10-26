//! The window of a running job: what it does, where it is, a gauge, and its time and speed.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align, Cut};
use super::dialog::{
    Colors, MAX_WIDTH, Tone, button_line, button_spots, draw_box, draw_separator, frame_around,
    shine_bar,
};
use super::glyphs;
use super::keymap::{Action, Resolved};
use super::label::Label;
use super::mouse::{Drawn, Pointer};
use super::panel::short_size;
use super::spinner::{Shine, Spin, Wait};
use super::theme::Theme;
use crate::i18n::fl;

/// Work the speed needs behind it before it means anything.
const SETTLED: Duration = Duration::from_secs(1);

/// The last stretch of work that the speed and the time left follow: long enough to even out
/// chunks and small files, short enough that a slow start or a change of pace is soon behind.
const RECENT: Duration = Duration::from_secs(5);

/// The least time between two [`Samples`], so that [`RECENT`] holds a few dozen.
const SAMPLE: Duration = Duration::from_millis(250);

/// The seconds from which the time left goes by [`LEFT_STEP`], so that it does not jump to and
/// fro by a second or two.
const ROUGH_LEFT: u64 = 60;

/// The step of a time left over [`ROUGH_LEFT`], in seconds.
const LEFT_STEP: u64 = 5;

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

/// The bytes a job had copied at moments of the time it worked, back to the start of the last
/// [`RECENT`] of it. Times of work, not of the clock, so that a question left open is no gap.
#[derive(Debug, Default)]
struct Samples(VecDeque<(Duration, u64)>);

impl Samples {
    /// The job had copied `bytes` when it had worked `at`.
    fn note(&mut self, at: Duration, bytes: u64) {
        if self.0.back().is_none_or(|&(last, _)| at >= last + SAMPLE) {
            self.0.push_back((at, bytes));
        }
        // The last sample at or before the start of the stretch stays: the stretch starts there.
        while self.0.get(1).is_some_and(|&(next, _)| next + RECENT <= at) {
            self.0.pop_front();
        }
    }

    /// Bytes per second over the last [`RECENT`] of work up to `at`, with `bytes` copied by
    /// then, or since the first sample if the job has worked less. The stretch starts at a
    /// sample, so it is wider while reports are rare, and falls to nothing when they stop.
    fn speed(&self, at: Duration, bytes: u64) -> Option<f64> {
        let start = at.saturating_sub(RECENT);
        let &(since, from) = self
            .0
            .iter()
            .rev()
            .find(|(time, _)| *time <= start)
            .or_else(|| self.0.front())?;
        let span = at.saturating_sub(since);
        (!span.is_zero()).then(|| {
            // Sizes stay far below 2^52, where `f64` would round them.
            #[allow(clippy::cast_precision_loss)]
            let copied = bytes.saturating_sub(from) as f64;
            copied / span.as_secs_f64()
        })
    }
}

/// The time and speed of a job at some moment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Timing {
    elapsed: Duration,
    /// Bytes per second over the last [`RECENT`] of work, once there is enough behind it; for
    /// jobs that move data.
    speed: Option<f64>,
    /// The time left at that speed, while data moves, as [`rough_left`] gives it.
    left: Option<Duration>,
}

/// The speed and the time left of a job, worked out for one second of its time as it shows.
#[derive(Debug, Clone, Copy)]
struct Held {
    second: u64,
    speed: Option<f64>,
    left: Option<Duration>,
}

/// A button of a job's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobButton {
    /// The job goes on behind the panels.
    Background,
    Abort,
}

impl JobButton {
    /// Its name, with the letter that Alt and it press.
    fn label(self) -> Label {
        Label::parse(&match self {
            Self::Background => fl!("job-background"),
            Self::Abort => fl!("dialog-abort"),
        })
    }
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
    samples: Samples,
    /// The speed and the time left change with the second that the time shows, rather than
    /// with every frame.
    held: Cell<Option<Held>>,
    buttons: Vec<JobButton>,
    focus: usize,
    /// Where the last render drew it, for the mouse.
    drawn: RefCell<Drawn>,
    /// For the waiting and the counting, whose gauge the shine crosses.
    wait: Wait,
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
            samples: Samples::default(),
            held: Cell::default(),
            buttons: vec![JobButton::Background, JobButton::Abort],
            focus: 0,
            drawn: RefCell::default(),
            wait: Wait::default(),
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
        self.samples
            .note(self.clock.elapsed(now), counts.bytes_copied);
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

    /// Its wait while it waits its turn or counts, whose shine crosses the gauge.
    pub(crate) fn shining(&self) -> Option<&Wait> {
        (!self.aborting && !self.measured()).then_some(&self.wait)
    }

    /// Whether how far it is is known: not while it waits its turn or counts.
    fn measured(&self) -> bool {
        matches!(self.stage, Stage::Working { .. })
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

    /// The time and speed at `now`, while the job works. The speed and the time left are those
    /// of the first call in the second that the time shows.
    fn timing(&self, now: Instant) -> Option<Timing> {
        let Stage::Working { counts, .. } = &self.stage else {
            return None;
        };
        let elapsed = self.clock.elapsed(now);
        let second = whole_seconds(elapsed);
        let held = match self.held.get() {
            Some(held) if held.second == second => held,
            _ => {
                let held = self.measure(counts, elapsed, second);
                self.held.set(Some(held));
                held
            }
        };
        Some(Timing {
            elapsed,
            speed: held.speed,
            left: held.left,
        })
    }

    /// The speed and the time left after `elapsed` of work, in its `second`, with `counts`
    /// behind it.
    fn measure(&self, counts: &Counts, elapsed: Duration, second: u64) -> Held {
        let speed = if counts.bytes_total > 0 && elapsed >= SETTLED {
            self.samples.speed(elapsed, counts.bytes_copied)
        } else {
            None
        };
        let left = speed.filter(|speed| *speed > 0.0).map(|speed| {
            #[allow(clippy::cast_precision_loss)]
            let left = counts.bytes_total.saturating_sub(counts.bytes_done) as f64;
            rough_left(Duration::try_from_secs_f64(left / speed).unwrap_or(Duration::MAX))
        });
        Held {
            second,
            speed,
            left,
        }
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
                fl!("jobs-state-percent", percent = percent)
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

    /// Takes a key: Enter or Space presses the focused button, Esc aborts, as in mc, arrows
    /// and Tab move the focus, and Alt and a button's letter press it.
    pub(crate) fn handle(&mut self, input: Resolved) -> Option<JobButton> {
        let action = match input {
            Resolved::Action(action) => action,
            Resolved::Hotkey(c) => {
                return self
                    .buttons
                    .iter()
                    .copied()
                    .find(|button| button.label().has(c));
            }
            Resolved::Insert(_) => return None,
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

    /// Draws the window as it is at `now`; until how far the job is is known, its gauge is a
    /// skeleton, which the shine of `spin` crosses while it is [`shining`](Self::shining).
    pub(crate) fn render(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        (now, spin): (Instant, &Spin),
    ) {
        let colors = Colors::of(theme, Tone::Plain);
        // Borders, what it does, where, the gauge, the count, the time, a line, the buttons.
        let inner = draw_box(frame, area, (MAX_WIDTH, 9), &self.title, colors, theme);
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
            &Stage::Scanning { items } => (
                fl!("job-scanning"),
                String::new(),
                0.0,
                fl!("job-found", items = items),
            ),
            Stage::Working { current, counts } => {
                let (done, total) = (counts.done, counts.total);
                // Data takes the time, so the gauge follows the bytes, if there are any.
                let count = if counts.bytes_total > 0 {
                    fl!(
                        "job-count-bytes",
                        done = done,
                        total = total,
                        bytes_done = short_size(counts.bytes_done),
                        bytes_total = short_size(counts.bytes_total)
                    )
                } else {
                    fl!("job-count", done = done, total = total)
                };
                (self.doing.clone(), current.clone(), self.ratio(), count)
            }
        };
        let doing = if self.aborting {
            fl!("job-aborting")
        } else {
            doing
        };
        let text = |text: &str| {
            let styles = colors.body_styles();
            Line::from(cells::fit(
                text,
                width,
                Align::Left,
                styles,
                theme.glyphs.marks,
            ))
        };
        put(0, text(&doing));
        put(1, text(&current));
        if self.measured() {
            put(2, gauge(ratio, width, theme));
        } else {
            // A stop leaves it still: nothing more is to come.
            let shine = self.shining().and_then(|wait| spin.shine(wait));
            put(2, unknown_gauge(inner.width, shine, theme));
        }
        put(3, text(&count));
        put(4, text(&self.time_line(now)));
        let labels: Vec<Label> = self.buttons.iter().map(|button| button.label()).collect();
        let buttons = button_line(&labels, 0, Some(self.focus), colors);
        let row = Rect::new(inner.x, inner.y + 6, inner.width, 1);
        *self.drawn.borrow_mut() = Drawn {
            frame: frame_around(inner),
            rows: Vec::new(),
            buttons: if inner.height > 6 {
                button_spots(&labels, 0, row)
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
            .map_or_else(|| fl!("job-unknown-time"), duration);
        let speed = timing.speed.map_or_else(
            || fl!("job-unknown-speed"),
            |speed| fl!("job-speed", size = speed_size(speed)),
        );
        fl!("job-timing", elapsed = elapsed, left = left, speed = speed)
    }
}

/// `done` of `total`, from 0 to 1.
pub(crate) fn ratio(done: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    // Counts and sizes stay far below 2^52, where `f64` would round them.
    #[allow(clippy::cast_precision_loss)]
    let ratio = done as f64 / total as f64;
    ratio.clamp(0.0, 1.0)
}

/// The whole seconds of `duration`, to the nearest, as [`duration`] shows them.
fn whole_seconds(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_millis() >= 500))
}

/// A time left in whole seconds, and over [`ROUGH_LEFT`] to the nearest [`LEFT_STEP`].
fn rough_left(left: Duration) -> Duration {
    let seconds = whole_seconds(left);
    if seconds <= ROUGH_LEFT {
        return Duration::from_secs(seconds);
    }
    Duration::from_secs(seconds.saturating_add(LEFT_STEP / 2) / LEFT_STEP * LEFT_STEP)
}

/// A duration in whole seconds, as `M:SS`, or `H:MM:SS` from an hour on.
fn duration(duration: Duration) -> String {
    let seconds = whole_seconds(duration);
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Bytes per second as a size: `512B`, `1.5M`, `480K`.
fn speed_size(bytes: f64) -> String {
    cells::short_size(bytes, &noc_locale::current())
}

/// A bar of `width` cells, filled for `ratio`, and the percentage after it. Block characters
/// show in any theme, where colors alone would not.
pub(crate) fn gauge(ratio: f64, width: usize, theme: &Theme) -> Line<'static> {
    // `ratio` is within 0 … 1, and bars are a few dozen cells.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let (percent, filled) = {
        let percent = (ratio * 100.0).round() as u64;
        // As wide as the widest, so that the bar keeps its length.
        let widest = cells::width(&fl!("job-percent", percent = 100));
        let percent = cells::fit_cut(
            &fl!("job-percent", percent = percent),
            widest,
            Align::Right,
            Cut::End,
        );
        let percent = format!(" {}", percent.head);
        let bar = width.saturating_sub(cells::width(&percent));
        (percent, ((bar as f64 * ratio).round() as usize).min(bar))
    };
    let bar = width.saturating_sub(cells::width(&percent));
    let done = glyphs::GAUGE_DONE.repeat(filled);
    let text = format!("{done}{}", glyphs::GAUGE_LEFT.repeat(bar - filled));
    Line::from(vec![Span::styled(text, theme.gauge), Span::raw(percent)])
}

/// The gauge of a job that does not know yet how far it is: a faint bar of `width` cells, with
/// no percentage, over which a `shine`, if any, the one of the skeleton of rows, runs to and fro.
pub(crate) fn unknown_gauge(width: u16, shine: Option<Shine>, theme: &Theme) -> Line<'static> {
    let middle = shine.map(|shine| i32::from(shine.bounce(width)));
    Line::from(shine_bar(
        width,
        middle,
        glyphs::GAUGE_UNKNOWN,
        theme.dialog,
    ))
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    use super::*;
    use crate::tui::glyphs::Glyphs;
    use crate::tui::spinner;

    fn draw_at(view: &JobView, now: Instant) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 11)).unwrap();
        terminal
            .draw(|frame| {
                let spin = Spin::new(now, now, false, Glyphs::PLAIN.spinners);
                view.render(frame, frame.area(), &Theme::terminal(), (now, &spin));
            })
            .unwrap();
        terminal.backend().to_string()
    }

    /// The title line of the window drawn at `now` with turning spinners, the gauge line, and
    /// which cells of the gauge are dim.
    fn busy_lines(view: &JobView, now: Instant) -> (String, String, Vec<bool>) {
        let mut terminal = Terminal::new(TestBackend::new(80, 11)).unwrap();
        terminal
            .draw(|frame| {
                let spin = Spin::new(now, now, true, Glyphs::PLAIN.spinners);
                view.render(frame, frame.area(), &Theme::terminal(), (now, &spin));
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let line = |y: u16| -> String {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim()
                .to_owned()
        };
        // The gauge, inside the frame and its margin.
        let dim = (4..76)
            .map(|x| buffer[(x, 4)].modifier.contains(Modifier::DIM))
            .collect();
        (line(1), line(4), dim)
    }

    #[test]
    fn until_how_far_it_is_is_known_the_gauge_is_a_skeleton_that_a_shine_crosses() {
        let start = Instant::now();
        let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
        view.scanning(42);
        let (title, gauge, dim) = busy_lines(&view, start);
        assert!(title.contains("═ Delete ═"), "{title}");
        assert!(gauge.contains("▒▒▒▒▒▒") && !gauge.contains('%'), "{gauge}");
        assert!(dim.iter().all(|dim| *dim), "{dim:?}");
        // The shine starts with the delay of every spinner, and no dots turn in the title.
        let (title, _, dim) = busy_lines(&view, start + spinner::DELAY + secs(1) / 2);
        assert!(title.contains("═ Delete ═"), "{title}");
        assert_eq!(dim.iter().filter(|dim| !**dim).count(), 9, "{dim:?}");
        // It turns at the end of the bar of 72 cells and runs back: eight cells after the last.
        let back = start + spinner::DELAY + Duration::from_millis(40) * 79;
        let (_, _, dim) = busy_lines(&view, back);
        let lit: Vec<usize> = (0..dim.len()).filter(|x| !dim[*x]).collect();
        assert_eq!(lit, (59..=67).collect::<Vec<_>>());
        // A stop leaves it still.
        view.abort();
        let (_, _, dim) = busy_lines(&view, start + spinner::DELAY + secs(1) / 2);
        assert!(dim.iter().all(|dim| *dim), "{dim:?}");
        // Once it is known, the gauge fills, with its percentage.
        view.working("/srv/a".to_owned(), Counts::default(), start);
        let (_, gauge, _) = busy_lines(&view, start + secs(1));
        assert!(gauge.contains('%'), "{gauge}");
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
        assert!(text.contains("1 of 2, 1.5K of 2.0K"), "{text}");
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
        view.working("a".to_owned(), copying(0, 0), start);
        view.working("a".to_owned(), copying(5, 5), start + secs(5));
        // A question after 5 seconds stays open for an hour.
        view.ask(start + secs(5));
        assert!(!view.ticking());
        // Reports that were on their way change nothing.
        view.working("a".to_owned(), copying(5, 5), start + secs(6));
        assert!(!view.ticking());
        let open = start + secs(3605);
        let timing = view.timing(open).unwrap();
        assert_eq!(timing.elapsed, secs(5));
        view.answered(open);
        assert!(view.ticking());

        view.working("a".to_owned(), copying(10, 10), open + secs(5));
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
        // 1 MiB a second for 10 seconds, then 80 MiB skipped at once, then a retry that takes
        // 6 MiB back to the start of a file: the speed follows the bytes that moved.
        for second in 0..=10 {
            view.working(
                "a".to_owned(),
                copying(second, second),
                start + secs(second),
            );
        }
        view.working("b".to_owned(), copying(90, 10), start + secs(10));
        let timing = view.timing(start + secs(10)).unwrap();
        assert!((timing.speed.unwrap() - 1024.0 * 1024.0).abs() < 1.0);
        assert_eq!(timing.left.map(|left| left.as_secs()), Some(10));
        view.working("c".to_owned(), copying(85, 11), start + secs(11));
        let timing = view.timing(start + secs(11)).unwrap();
        assert!((timing.speed.unwrap() - 1024.0 * 1024.0).abs() < 1.0);
        assert_eq!(timing.left.map(|left| left.as_secs()), Some(15));
    }

    #[test]
    fn the_speed_and_the_time_left_change_with_the_second_shown() {
        let start = Instant::now();
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        for second in 0..=10 {
            view.working(
                "a".to_owned(),
                copying(second, second),
                start + secs(second),
            );
        }
        let shown = view.timing(start + secs(10)).unwrap();
        // A burst of 3 MiB within the second that shows changes neither.
        let burst = start + Duration::from_millis(10_300);
        view.working("a".to_owned(), copying(13, 13), burst);
        let timing = view.timing(start + Duration::from_millis(10_400)).unwrap();
        assert_eq!((timing.speed, timing.left), (shown.speed, shown.left));
        assert_eq!(timing.elapsed, Duration::from_millis(10_400));
        // The time shows 0:11 from 10.5 seconds on, and they follow.
        let timing = view.timing(start + Duration::from_millis(10_500)).unwrap();
        let speed = timing.speed.unwrap();
        assert!(speed > 1.4 * 1024.0 * 1024.0, "{speed}");
        assert_ne!(timing.left, shown.left);
    }

    #[test]
    fn a_long_time_left_goes_by_five_seconds() {
        let left = |millis: u64| rough_left(Duration::from_millis(millis)).as_secs();
        assert_eq!(left(41_400), 41);
        assert_eq!(left(59_600), 60);
        assert_eq!(left(62_400), 60);
        assert_eq!(left(62_600), 65);
        assert_eq!(left(92_000), 90);
        assert_eq!(left(3_723_000), 3_725);
        assert_eq!(rough_left(Duration::MAX).as_secs() % 5, 0);
    }

    #[test]
    fn a_slow_start_is_soon_behind_the_speed() {
        let start = Instant::now();
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        // 1 MiB a second for 10 seconds, then 5 MiB a second, reported every second.
        let copied = |second: u64| {
            if second <= 10 {
                second
            } else {
                10 + (second - 10) * 5
            }
        };
        for second in 0..=20 {
            let counts = copying(copied(second), copied(second));
            view.working("a".to_owned(), counts, start + secs(second));
        }
        // Averaged since the start, it would be 3 MiB a second, with 13 seconds left.
        let timing = view.timing(start + secs(20)).unwrap();
        let speed = timing.speed.unwrap();
        assert!(
            (speed - 5.0 * 1024.0 * 1024.0).abs() < 1.0,
            "5 MiB/s: {speed}"
        );
        assert_eq!(timing.left.map(|left| left.as_secs()), Some(8));
    }

    #[test]
    fn the_speed_falls_while_no_data_moves() {
        let start = Instant::now();
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        for second in 0..=10 {
            view.working(
                "a".to_owned(),
                copying(second, second),
                start + secs(second),
            );
        }
        // Then nothing comes: the seconds that moved data leave the stretch one by one.
        let timing = view.timing(start + secs(12)).unwrap();
        let speed = timing.speed.unwrap();
        assert!(
            (speed - 0.6 * 1024.0 * 1024.0).abs() < 1.0,
            "0.6 MiB/s: {speed}"
        );
        assert_eq!(timing.left.map(|left| left.as_secs()), Some(150));
        let timing = view.timing(start + secs(15)).unwrap();
        assert!(timing.speed.unwrap() < f64::EPSILON);
        assert_eq!(timing.left, None);
        let text = draw_at(&view, start + secs(15));
        assert!(text.contains("Time 0:15   ETA -:--   0B/s"), "{text}");
        assert_eq!(view.summary(start + secs(15)).left, "");
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
        assert_eq!(speed_size(1536.0), "1.5K");
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
        assert_eq!(view.handle(Resolved::Hotkey('a')), Some(JobButton::Abort));
        assert_eq!(
            view.handle(Resolved::Hotkey('B')),
            Some(JobButton::Background)
        );
        assert_eq!(view.handle(Resolved::Hotkey('x')), None);

        view.keep_in_front();
        assert_eq!(view.handle(Resolved::Hotkey('b')), None, "no Background");
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
