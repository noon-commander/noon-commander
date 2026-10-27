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

/// How far back the time left looks: work this much older weighs a third as much, a minute
/// older an eighth. Long enough to ride out the ups and downs of a link or a disk, which the
/// speed shows, short enough to follow a change of pace within a minute or so.
const ETA_HORIZON: Duration = Duration::from_secs(30);

/// How far the time left shown may stray from the forecast, as a share of the forecast, and
/// only count down by the clock, a second each second: the forecast's own small ups and downs
/// keep within it.
const ETA_NEAR: f64 = 0.03;

/// Up to this far from the forecast, beyond [`ETA_NEAR`], the time left shown also moves a tenth
/// of the way to it each second, so that it does not stay off for long; beyond this, halfway.
const ETA_SLACK: f64 = 0.1;

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

/// Where a job was after some work, for its [`Forecast`].
#[derive(Debug, Clone, Copy)]
struct Point {
    /// The time it had worked.
    at: Duration,
    /// Entries done or skipped.
    entries: u64,
    /// Bytes done or skipped.
    done: u64,
    copied: u64,
}

impl Point {
    fn new(at: Duration, counts: &Counts) -> Self {
        Self {
            at,
            entries: counts.done,
            done: counts.bytes_done,
            copied: counts.bytes_copied,
        }
    }
}

/// Sums over stretches of a job's work, each weighing less the longer ago it ended, by
/// [`ETA_HORIZON`].
#[derive(Debug, Clone, Copy, Default)]
struct Sums {
    /// Seconds worked, bytes copied, and entries done, over stretches of at least [`SAMPLE`].
    time: f64,
    bytes: f64,
    entries: f64,
    /// Seconds and bytes of those stretches in which no entry was done: data streaming within
    /// entries, which tells what a byte costs.
    stream_time: f64,
    stream_bytes: f64,
    /// For the least squares of the seconds of a stretch on its bytes and its entries, over
    /// stretches that end as entries are done: how many (`rows`), and the products of bytes
    /// (`b`), entries (`n`), and seconds (`t`).
    rows: f64,
    bb: f64,
    bn: f64,
    nn: f64,
    bt: f64,
    nt: f64,
    tt: f64,
}

impl Sums {
    /// Adds the stretch of work from `from` to `to` to the pace, after which every stretch
    /// before weighs less. A stretch that skipped data is as a pause: its time went to entries
    /// that will not be counted, as will those left to skip.
    fn add(&mut self, from: Point, to: Point) {
        if skipped(from, to) {
            return;
        }
        let time = to.at.saturating_sub(from.at).as_secs_f64();
        let keep = (-time / ETA_HORIZON.as_secs_f64()).exp();
        for sum in [
            &mut self.time,
            &mut self.bytes,
            &mut self.entries,
            &mut self.stream_time,
            &mut self.stream_bytes,
            &mut self.rows,
            &mut self.bb,
            &mut self.bn,
            &mut self.nn,
            &mut self.bt,
            &mut self.nt,
            &mut self.tt,
        ] {
            *sum *= keep;
        }
        let bytes = float(to.copied.saturating_sub(from.copied));
        let entries = to.entries.saturating_sub(from.entries);
        self.time += time;
        self.bytes += bytes;
        self.entries += float(entries);
        if entries == 0 && bytes > 0.0 {
            self.stream_time += time;
            self.stream_bytes += bytes;
        }
    }

    /// Adds the stretch from `from` to `to`, which ends as entries are done, to what tells
    /// what an entry costs beside its bytes: a stretch for each entry, or for a few that have
    /// no data, so that big and small ones tell them apart.
    fn fit(&mut self, from: Point, to: Point) {
        if skipped(from, to) {
            return;
        }
        let time = to.at.saturating_sub(from.at).as_secs_f64();
        let bytes = float(to.copied.saturating_sub(from.copied));
        let entries = float(to.entries.saturating_sub(from.entries));
        self.rows += 1.0;
        self.bb += bytes * bytes;
        self.bn += bytes * entries;
        self.nn += entries * entries;
        self.bt += bytes * time;
        self.nt += entries * time;
        self.tt += time * time;
    }

    /// What a byte and an entry cost, in proportion. A byte costs what it does while data
    /// streams within entries, and an entry what the stretches of entries took beyond their
    /// bytes. A job with no data streaming yet, whose files are all small, takes both from the
    /// least squares while the cost of an entry stands well clear of the noise; that of their
    /// bytes is mostly noise, and no less than nothing, until data streams. Else bytes alone
    /// count, as for files of one size.
    fn costs(&self) -> (f64, f64) {
        if self.stream_bytes > 0.0 && self.stream_time > 0.0 {
            let byte = self.stream_time / self.stream_bytes;
            let entry = if self.nn > 0.0 {
                ((self.nt - byte * self.bn) / self.nn).max(0.0)
            } else {
                0.0
            };
            return (byte, entry);
        }
        let det = self.bb * self.nn - self.bn * self.bn;
        if self.rows > 3.0 && det > 1e-3 * self.bb * self.nn {
            let byte = (self.bt * self.nn - self.bn * self.nt) / det;
            let entry = (self.bb * self.nt - self.bn * self.bt) / det;
            let squares = self.tt - 2.0 * (byte * self.bt + entry * self.nt)
                + byte * byte * self.bb
                + 2.0 * byte * entry * self.bn
                + entry * entry * self.nn;
            let noise = squares.max(0.0) / (self.rows - 2.0);
            // At least twice its standard error.
            if entry > 0.0 && entry * entry * det >= 4.0 * noise * self.bb {
                return (byte.max(0.0), entry);
            }
        }
        (1.0, 0.0)
    }
}

/// The time left of a job: the work left, bytes and entries, at the pace of the work behind
/// it over about [`ETA_HORIZON`] of it. An entry costs time as some bytes do, opening,
/// creating, setting attributes, and how many is learned as the job goes, so that many small
/// files left take longer than their bytes say.
#[derive(Debug, Default)]
struct Forecast {
    /// Where the last stretch of the pace ended.
    last: Option<Point>,
    /// Where the job was when it last had done more entries.
    finished: Option<Point>,
    /// When the job last reported, in the time it worked.
    seen: Option<Duration>,
    /// Time in which no report came beyond [`RECENT`]: the job stood still, and the time is
    /// left out, so that the pace is that of work when work goes on.
    still: Duration,
    sums: Sums,
}

impl Forecast {
    /// The job had `counts` behind it when it had worked `at`.
    fn note(&mut self, at: Duration, counts: &Counts) {
        if let Some(seen) = self.seen {
            self.still += at.saturating_sub(seen).saturating_sub(RECENT);
        }
        self.seen = Some(at);
        let point = Point::new(at.saturating_sub(self.still), counts);
        let (Some(last), Some(finished)) = (self.last, self.finished) else {
            self.last = Some(point);
            self.finished = Some(point);
            return;
        };
        if point.at >= last.at + SAMPLE {
            self.sums.add(last, point);
            self.last = Some(point);
        }
        if point.entries > finished.entries {
            self.sums.fit(finished, point);
            self.finished = Some(point);
        }
    }

    /// The time left at `at`, with `counts` behind the job: unknown once no report has come
    /// for [`RECENT`], as the speed then falls to nothing, or before any work has a pace.
    fn left(&self, at: Duration, counts: &Counts) -> Option<Duration> {
        if at.saturating_sub(self.seen?) >= RECENT {
            return None;
        }
        let sums = &self.sums;
        let bytes = float(counts.bytes_total.saturating_sub(counts.bytes_done));
        let entries = float(counts.total.saturating_sub(counts.done));
        let (byte, entry) = sums.costs();
        let done = byte * sums.bytes + entry * sums.entries;
        let seconds = if done > 0.0 {
            sums.time * (byte * bytes + entry * entries) / done
        } else if counts.bytes_done >= counts.bytes_total && sums.entries > 0.0 {
            // Entries with no data so far, and none left.
            sums.time * entries / sums.entries
        } else {
            return None;
        };
        Some(Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX))
    }
}

/// Whether data was skipped from `from` to `to`: entries skipped with it, and which of the
/// stretch's were is not known, so that none of them count.
fn skipped(from: Point, to: Point) -> bool {
    to.done.saturating_sub(from.done) > to.copied.saturating_sub(from.copied)
}

/// A count or a size as a float, for the sums of a [`Forecast`].
fn float(count: u64) -> f64 {
    // Counts and sizes stay far below 2^52, where `f64` would round them.
    #[allow(clippy::cast_precision_loss)]
    let float = count as f64;
    float
}

/// The time and speed of a job at some moment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Timing {
    elapsed: Duration,
    /// Bytes per second over the last [`RECENT`] of work, once there is enough behind it; for
    /// jobs that move data.
    speed: Option<f64>,
    /// The time left, counted down as [`count_down`] does.
    left: Option<Duration>,
}

/// The speed and the time left of a job, worked out for one second of its time as it shows.
#[derive(Debug, Clone, Copy)]
struct Held {
    second: u64,
    speed: Option<f64>,
    left: Option<Duration>,
}

/// What the window of a job shows at some moment, before it is put in words, apart from its
/// buttons and the shine over a gauge not known yet: the window draws the same as long as this
/// stays the same. It takes no formatting, so it can be made for every report.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Look {
    aborting: bool,
    stage: Seen,
}

/// The stage of a job as its window shows it.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Waiting,
    Scanning {
        items: u64,
    },
    Working {
        current: String,
        done: u64,
        total: u64,
        /// How far it is in whole percent, which the bar of the gauge follows too.
        percent: u64,
        /// The whole seconds that its time shows.
        second: u64,
        /// For jobs that move data.
        data: Option<Data>,
    },
}

impl Look {
    /// Only what the row of the job in the list of jobs shows.
    pub(crate) fn row(self) -> RowLook {
        let (phase, left, current) = match self.stage {
            Seen::Waiting => (Phase::Waiting, None, String::new()),
            Seen::Scanning { .. } => (Phase::Counting, None, String::new()),
            Seen::Working {
                current,
                percent,
                data,
                ..
            } => (
                Phase::Working { percent },
                data.and_then(|data| data.left),
                current,
            ),
        };
        RowLook {
            aborting: self.aborting,
            phase,
            left,
            current,
        }
    }
}

/// What the row of a job in the list of jobs shows, before it is put in words: less than its
/// window, so that a report which changes only the window does not draw the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowLook {
    aborting: bool,
    phase: Phase,
    /// The time left, for jobs that move data, once it is known.
    left: Option<Duration>,
    /// The entry at hand, while it works.
    current: String,
}

/// How far a job is, as its row in the list of jobs says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting,
    Counting,
    Working { percent: u64 },
}

/// What the window of a job that moves data shows of it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Data {
    done: Size,
    total: Size,
    speed: Option<f64>,
    left: Option<Duration>,
}

/// A size in bytes, equal to another that reads the same.
#[derive(Debug, Clone, Copy)]
struct Size(u64);

impl Size {
    fn key(self) -> (usize, usize, u64) {
        // Sizes stay far below 2^52, where `f64` would round them.
        #[allow(clippy::cast_precision_loss)]
        cells::short_size_key(self.0 as f64)
    }
}

impl PartialEq for Size {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
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
    forecast: Forecast,
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
            forecast: Forecast::default(),
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
        let at = self.clock.elapsed(now);
        self.samples.note(at, counts.bytes_copied);
        self.forecast.note(at, &counts);
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

    /// When the second that its time shows changes next after `now`, and with it the speed
    /// and the time left, while its time runs.
    pub(crate) fn next_tick(&self, now: Instant) -> Option<Instant> {
        self.ticking().then(|| {
            let elapsed = self.clock.elapsed(now);
            let next = Duration::from_secs(whole_seconds(elapsed))
                .saturating_add(Duration::from_millis(500));
            now + next.saturating_sub(elapsed)
        })
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

    /// The time and speed at `now`, while the job works.
    #[cfg(test)]
    fn timing(&self, now: Instant) -> Option<Timing> {
        match &self.stage {
            Stage::Working { counts, .. } => Some(self.timing_of(counts, now)),
            Stage::Waiting | Stage::Scanning { .. } => None,
        }
    }

    /// The time and speed at `now` of the job at work with `counts` behind it. The speed and
    /// the time left are those of the first call in the second that the time shows.
    fn timing_of(&self, counts: &Counts, now: Instant) -> Timing {
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
        Timing {
            elapsed,
            speed: held.speed,
            left: held.left,
        }
    }

    /// What its window shows at `now`.
    pub(crate) fn look(&self, now: Instant) -> Look {
        let stage = match &self.stage {
            Stage::Waiting => Seen::Waiting,
            &Stage::Scanning { items } => Seen::Scanning { items },
            Stage::Working { current, counts } => {
                let timing = self.timing_of(counts, now);
                // Data takes the time, so the gauge follows the bytes, if there are any.
                let data = (counts.bytes_total > 0).then_some(Data {
                    done: Size(counts.bytes_done),
                    total: Size(counts.bytes_total),
                    speed: timing.speed,
                    // As it reads.
                    left: timing
                        .left
                        .map(|left| Duration::from_secs(whole_seconds(left))),
                });
                // A ratio within 0 … 1.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let percent = (self.ratio() * 100.0).round() as u64;
                Seen::Working {
                    current: current.clone(),
                    done: counts.done,
                    total: counts.total,
                    percent,
                    second: whole_seconds(timing.elapsed),
                    data,
                }
            }
        };
        Look {
            aborting: self.aborting,
            stage,
        }
    }

    /// The speed and the time left after `elapsed` of work, in its `second`, with `counts`
    /// behind it.
    fn measure(&self, counts: &Counts, elapsed: Duration, second: u64) -> Held {
        let (speed, left) = if counts.bytes_total > 0 && elapsed >= SETTLED {
            let speed = self.samples.speed(elapsed, counts.bytes_copied);
            let shown = self
                .held
                .get()
                .and_then(|held| Some((held.second, held.left?)));
            let left = self
                .forecast
                .left(elapsed, counts)
                .map(|forecast| count_down(shown, second, forecast));
            (speed, left)
        } else {
            (None, None)
        };
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
        let row = self.look(now).row();
        let state = match row.phase {
            _ if row.aborting => fl!("jobs-state-aborting"),
            Phase::Waiting => fl!("jobs-state-waiting"),
            Phase::Counting => fl!("jobs-state-counting"),
            Phase::Working { percent } => fl!("jobs-state-percent", percent = percent),
        };
        let left = row
            .left
            .map(|left| fl!("jobs-left", left = duration(left)))
            .unwrap_or_default();
        Summary {
            state,
            left,
            current: row.current,
        }
    }

    /// Whether it moves data, so that its row in the list of jobs shows the time left, which
    /// changes as time passes.
    pub(crate) fn moves_data(&self) -> bool {
        matches!(&self.stage, Stage::Working { counts, .. } if counts.bytes_total > 0)
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
        let look = self.look(now);
        let (doing, current, percent, count, time) = match &look.stage {
            Seen::Waiting => (fl!("job-waiting"), "", None, String::new(), String::new()),
            &Seen::Scanning { items } => (
                fl!("job-scanning"),
                "",
                None,
                fl!("job-found", items = items),
                String::new(),
            ),
            &Seen::Working {
                ref current,
                done,
                total,
                percent,
                second,
                data,
            } => {
                let count = match data {
                    Some(data) => fl!(
                        "job-count-bytes",
                        done = done,
                        total = total,
                        bytes_done = short_size(data.done.0),
                        bytes_total = short_size(data.total.0)
                    ),
                    None => fl!("job-count", done = done, total = total),
                };
                let time = time_line(second, data.as_ref());
                (
                    self.doing.clone(),
                    current.as_str(),
                    Some(percent),
                    count,
                    time,
                )
            }
        };
        let doing = if look.aborting {
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
        put(1, text(current));
        if let Some(percent) = percent {
            // At most 100.
            #[allow(clippy::cast_precision_loss)]
            let ratio = percent as f64 / 100.0;
            put(2, gauge(ratio, width, theme));
        } else {
            // A stop leaves it still: nothing more is to come.
            let shine = self.shining().and_then(|wait| spin.shine(wait));
            put(2, unknown_gauge(inner.width, shine, theme));
        }
        put(3, text(&count));
        put(4, text(&time));
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
}

/// The `second`s a job has worked, and for one that moves `data` the time left and the speed.
fn time_line(second: u64, data: Option<&Data>) -> String {
    let elapsed = duration(Duration::from_secs(second));
    let Some(data) = data else {
        return fl!("job-elapsed", elapsed = elapsed);
    };
    let left = data.left.map_or_else(|| fl!("job-unknown-time"), duration);
    let speed = data.speed.map_or_else(
        || fl!("job-unknown-speed"),
        |speed| fl!("job-speed", size = speed_size(speed)),
    );
    fl!("job-timing", elapsed = elapsed, left = left, speed = speed)
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

/// The time left to show in `second` by the `forecast`: the one `shown` in an earlier second,
/// less the seconds since, while that keeps within [`ETA_NEAR`] of the forecast; within
/// [`ETA_SLACK`], a tenth closer to the forecast for each second since; beyond, halfway closer.
/// The forecast itself without one shown, or once the count is down to nothing. It keeps its
/// fraction of a second, which shows rounded, so that a count shows a second less each second
/// and a tenth of a small gap is not lost to the rounding.
fn count_down(shown: Option<(u64, Duration)>, second: u64, forecast: Duration) -> Duration {
    let Some((then, shown)) = shown else {
        return forecast;
    };
    let passed = second.saturating_sub(then);
    let counted = shown.saturating_sub(Duration::from_secs(passed));
    if counted.is_zero() {
        return forecast;
    }
    let (from, to) = (counted.as_secs_f64(), forecast.as_secs_f64());
    let off = (to - from).abs();
    let keep: f64 = if off <= ETA_NEAR * to {
        return counted;
    } else if off <= ETA_SLACK * to {
        0.9
    } else {
        0.5
    };
    let rest = keep.powi(i32::try_from(passed).unwrap_or(i32::MAX));
    Duration::try_from_secs_f64(to + (from - to) * rest).unwrap_or(Duration::MAX)
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

        for second in 1..=10 {
            let counts = copying(2 * second, 2 * second);
            view.working("big".to_owned(), counts, start + secs(second));
        }
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
        assert_eq!(timing.left.map(whole_seconds), Some(90));
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
        assert_eq!(timing.left.map(whole_seconds), Some(10));
        view.working("c".to_owned(), copying(85, 11), start + secs(11));
        let timing = view.timing(start + secs(11)).unwrap();
        assert!((timing.speed.unwrap() - 1024.0 * 1024.0).abs() < 1.0);
        let forecast = view.forecast.left(secs(11), &copying(85, 11));
        assert_eq!(forecast.map(whole_seconds), Some(15));
        // The time left shown moves there from the 9 seconds counted down, halfway at once.
        assert_eq!(timing.left.map(whole_seconds), Some(12));
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
    fn the_look_changes_only_with_what_the_window_shows() {
        const MIB: u64 = 1024 * 1024;
        /// Reports `done` entries and `bytes` of 100 MiB at `now`, and looks.
        fn report(view: &mut JobView, (done, bytes): (u64, u64), now: Instant) -> Look {
            let counts = Counts {
                done,
                total: 2,
                bytes_done: bytes,
                bytes_total: 100 * MIB,
                bytes_copied: bytes,
            };
            view.working("a".to_owned(), counts, now);
            view.look(now)
        }
        let start = Instant::now();
        let at = |millis| start + Duration::from_millis(millis);
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        let shown = report(&mut view, (0, 20 * MIB), at(0));
        let look = report(&mut view, (0, 20 * MIB + 1_000), at(100));
        assert_eq!(look, shown, "still 20.0M, 20%, 0:00");
        let look = report(&mut view, (0, 20 * MIB + 200 * 1024), at(200));
        assert_ne!(look, shown, "20.2M");
        let shown = look;
        assert_ne!(
            report(&mut view, (1, 20 * MIB + 200 * 1024), at(300)),
            shown,
            "1 of 2"
        );
        let shown = view.look(at(300));
        assert_eq!(view.look(at(499)), shown);
        assert_ne!(view.look(at(500)), shown, "0:01");
    }

    #[test]
    fn the_time_draws_again_when_the_second_it_shows_changes() {
        let start = Instant::now();
        let at = |millis| start + Duration::from_millis(millis);
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        assert_eq!(view.next_tick(start), None, "nothing to time yet");
        view.working("a".to_owned(), copying(0, 0), start);
        assert_eq!(view.next_tick(at(200)), Some(at(500)));
        assert_eq!(view.next_tick(at(500)), Some(at(1_500)));
        assert_eq!(view.next_tick(at(1_499)), Some(at(1_500)));
        view.ask(at(2_000));
        assert_eq!(view.next_tick(at(9_000)), None, "it stands");
    }

    #[test]
    fn the_time_left_counts_down_by_the_clock_near_the_forecast() {
        let left = |shown: Option<(u64, u64)>, second, forecast_millis| {
            let shown = shown.map(|(then, left)| (then, secs(left)));
            whole_seconds(count_down(
                shown,
                second,
                Duration::from_millis(forecast_millis),
            ))
        };
        // The first is the forecast, in whole seconds.
        assert_eq!(left(None, 10, 99_600), 100);
        // Within 3% of it, a second less each second.
        assert_eq!(left(Some((10, 100)), 11, 101_000), 99);
        assert_eq!(left(Some((10, 100)), 11, 97_000), 99);
        assert_eq!(left(Some((10, 100)), 13, 99_000), 97);
        // Within a tenth, a tenth closer too each second, up or down.
        assert_eq!(left(Some((10, 100)), 11, 106_000), 100);
        assert_eq!(left(Some((10, 100)), 11, 92_000), 98);
        assert_eq!(left(Some((10, 100)), 13, 105_000), 99);
        // Beyond, halfway there each second.
        assert_eq!(left(Some((10, 100)), 11, 151_000), 125);
        assert_eq!(left(Some((10, 100)), 11, 39_000), 69);
        assert_eq!(left(Some((10, 100)), 12, 178_000), 158);
        // Counted down to nothing, the forecast.
        assert_eq!(left(Some((10, 2)), 12, 30_000), 30);
        assert_eq!(left(Some((10, 0)), 11, 30_000), 30);
        // Nothing overflows.
        let far = count_down(Some((0, Duration::MAX)), 1, Duration::MAX);
        assert!(far > secs(u64::MAX / 2));
    }

    #[test]
    fn a_time_left_shown_too_long_comes_down_to_the_forecast() {
        // 10:00 shown, where 9:15 are left and the forecast says so from then on.
        let mut shown = secs(600);
        let mut steps = Vec::new();
        for second in 11..=60 {
            let forecast = secs(555 - (second - 10));
            let next = count_down(Some((second - 1, shown)), second, forecast);
            steps.push(whole_seconds(shown) - whole_seconds(next));
            shown = next;
            // Within half a minute it keeps within 3% of the forecast.
            if second >= 40 {
                let off = shown.abs_diff(forecast).as_secs_f64();
                assert!(off <= forecast.as_secs_f64() * 0.03, "{second}: {shown:?}");
            }
        }
        // Down all the way, a few seconds at a time at first, and then by the clock, but for
        // a step now and then as what is left of the gap grows beyond 3% of what is left.
        assert_eq!(steps[0], 5, "10:00, then 9:55");
        assert!(steps.iter().all(|step| (1..=6).contains(step)), "{steps:?}");
        let ticks = steps[10..].iter().filter(|step| **step == 1).count();
        assert!(ticks >= 36, "{steps:?}");
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
        let timing = view.timing(start + secs(20)).unwrap();
        let speed = timing.speed.unwrap();
        assert!(
            (speed - 5.0 * 1024.0 * 1024.0).abs() < 1.0,
            "5 MiB/s: {speed}"
        );
        // The time left looks further back: between the 8 seconds of the last pace and the 13
        // of the average since the start.
        let left = timing.left.unwrap().as_secs();
        assert!((9..13).contains(&left), "{left}");
    }

    #[test]
    fn while_no_data_moves_the_speed_falls_and_the_time_left_is_unknown() {
        let start = Instant::now();
        let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
        for second in 0..=10 {
            view.working(
                "a".to_owned(),
                copying(second, second),
                start + secs(second),
            );
        }
        let before = view.timing(start + secs(10)).unwrap().left.unwrap();
        assert_eq!(before.as_secs(), 90);
        // Then nothing comes: the seconds that moved data leave the speed's stretch one by one,
        // while the time left keeps to the pace behind it and counts down.
        let timing = view.timing(start + secs(12)).unwrap();
        let speed = timing.speed.unwrap();
        assert!(
            (speed - 0.6 * 1024.0 * 1024.0).abs() < 1.0,
            "0.6 MiB/s: {speed}"
        );
        assert_eq!(timing.left.map(whole_seconds), Some(88));
        // Five seconds without a report: the speed is nothing, and the time left unknown.
        let timing = view.timing(start + secs(15)).unwrap();
        assert!(timing.speed.unwrap() < f64::EPSILON);
        assert_eq!(timing.left, None);
        let text = draw_at(&view, start + secs(15));
        assert!(text.contains("Time 0:15   ETA -:--   0B/s"), "{text}");
        assert_eq!(view.summary(start + secs(15)).left, "");
    }

    const MIB: u64 = 1024 * 1024;

    /// A copy at work, reported as a job reports: after each chunk, a quarter of a second of
    /// data at most, and after each entry.
    struct Run {
        view: JobView,
        start: Instant,
        /// Seconds worked.
        at: f64,
        counts: Counts,
    }

    impl Run {
        fn new(total: u64, bytes_total: u64) -> Self {
            let mut run = Self {
                view: JobView::new("Copy".to_owned(), "Copying".to_owned()),
                start: Instant::now(),
                at: 0.0,
                counts: Counts {
                    total,
                    bytes_total,
                    ..Counts::default()
                },
            };
            run.report();
            run
        }

        fn now(&self) -> Instant {
            self.start + Duration::from_secs_f64(self.at)
        }

        fn report(&mut self) {
            let now = self.now();
            self.view.working("f".to_owned(), self.counts, now);
        }

        /// Moves `bytes` at `speed` bytes a second, within an entry.
        fn stream(&mut self, bytes: u64, speed: u64) {
            let mut left = bytes;
            while left > 0 {
                let chunk = left.min(speed / 4);
                self.at += float(chunk) / float(speed);
                self.counts.bytes_done += chunk;
                self.counts.bytes_copied += chunk;
                left -= chunk;
                self.report();
            }
        }

        /// Copies a file of `size` bytes at `speed` bytes a second, after `opening` seconds.
        fn copy(&mut self, size: u64, speed: u64, opening: f64) {
            self.at += opening;
            self.stream(size, speed);
            self.counts.done += 1;
            self.report();
        }

        /// Skips a file of `size` bytes that is there already, finding it out in `seconds`, and
        /// reports, as a job does before each entry.
        fn skip(&mut self, size: u64, seconds: f64) {
            self.at += seconds;
            self.counts.done += 1;
            self.counts.bytes_done += size;
            self.report();
        }

        /// The time left in seconds, as the forecast has it, before it is rounded.
        fn left(&self) -> f64 {
            self.left_at(self.at).unwrap()
        }

        /// The time left in seconds when the job has worked `at` seconds, if it is known.
        fn left_at(&self, at: f64) -> Option<f64> {
            let at = self
                .view
                .clock
                .elapsed(self.start + Duration::from_secs_f64(at));
            let left = self.view.forecast.left(at, &self.counts)?;
            Some(left.as_secs_f64())
        }
    }

    /// Numbers that look random, the same each run.
    struct Noise(u64);

    impl Noise {
        /// 53 bits.
        fn bits(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 11
        }

        /// From 0 to 1.
        fn next(&mut self) -> f64 {
            float(self.bits()) / float(1 << 53)
        }

        /// From 0 up to `limit`.
        fn below(&mut self, limit: u64) -> u64 {
            self.bits() % limit
        }

        /// `value`, give or take a share of it as large as `spread`.
        fn around(&mut self, value: f64, spread: f64) -> f64 {
            value * (1.0 + spread * (2.0 * self.next() - 1.0))
        }
    }

    #[test]
    fn what_bytes_and_entries_cost() {
        let point = |millis: u64, entries: u64, bytes: u64| Point {
            at: Duration::from_millis(millis),
            entries,
            done: bytes,
            copied: bytes,
        };
        // 10 MiB streaming in a second within an entry, then three entries without data, a
        // tenth of a second each.
        let mut sums = Sums::default();
        sums.add(point(0, 0, 0), point(1_000, 0, 10 * MIB));
        for n in 0..3 {
            let (from, to) = (1_000 + 100 * n, 1_100 + 100 * n);
            sums.fit(point(from, n, 10 * MIB), point(to, n + 1, 10 * MIB));
        }
        let (byte, entry) = sums.costs();
        assert!((byte * float(10 * MIB) - 1.0).abs() < 1e-9, "{byte}");
        assert!((entry - 0.1).abs() < 1e-9, "{entry}");

        // With no data streaming, files of one size tell bytes and entries nothing apart.
        let mut sums = Sums::default();
        for n in 0..10 {
            sums.fit(
                point(100 * n, n, 1024 * n),
                point(100 * (n + 1), n + 1, 1024 * (n + 1)),
            );
        }
        let (byte, entry) = sums.costs();
        assert!((byte - 1.0).abs() < f64::EPSILON && entry.abs() < f64::EPSILON);

        // Files of up to 30 KiB, each opened in a tenth of a second give or take a third: the
        // entries clearly cost their tenth, the bytes nothing that shows.
        let mut noise = Noise(7);
        let mut sums = Sums::default();
        let (mut at, mut bytes) = (0, 0);
        for n in 0..500 {
            let size = noise.below(30 * 1024);
            let took = Duration::from_secs_f64(noise.around(0.1, 0.3));
            let to = Point {
                at: Duration::from_millis(at) + took,
                ..point(0, n + 1, bytes + size)
            };
            sums.fit(point(at, n, bytes), to);
            at = u64::try_from(to.at.as_millis()).unwrap();
            bytes += size;
        }
        let (byte, entry) = sums.costs();
        assert!(byte >= 0.0 && byte * float(30 * 1024) < 0.01, "{byte}");
        assert!((entry - 0.1).abs() < 0.01, "{entry}");
    }

    #[test]
    fn after_a_stall_the_time_left_keeps_the_pace_of_the_work() {
        let mut run = Run::new(1, 4_000 * MIB);
        run.stream(600 * MIB, 10 * MIB);
        let pace = run.left();
        // Nothing comes for a minute and a half: four seconds on, the time left is known …
        let stalled = run.at;
        assert!(run.left_at(stalled + 4.0).is_some());
        // … and from five on it is not.
        for seconds in [5.0, 30.0, 90.0] {
            assert_eq!(run.left_at(stalled + seconds), None);
        }
        run.at += 90.0;
        // The work goes on at the same pace: of the stall, five seconds count at most.
        run.stream(10 * MIB, 10 * MIB);
        let even = float(run.counts.bytes_total - run.counts.bytes_done) / float(10 * MIB);
        let left = run.left();
        assert!(
            left > even && left < even * 1.25,
            "{left} for {even}, {pace} before"
        );
        // Those five fade as the others: a minute later they are all but gone.
        run.stream(600 * MIB, 10 * MIB);
        let even = float(run.counts.bytes_total - run.counts.bytes_done) / float(10 * MIB);
        let left = run.left();
        assert!((left - even).abs() < even * 0.05, "{left} for {even}");
    }

    #[test]
    fn skipping_entries_one_by_one_is_no_stall() {
        // Files of 64 KiB and 4 MiB by turns, then Skip all over 5,000 files of 1 KiB, each
        // found out in 20 ms as over a link, then 1,000 files of 1 KiB.
        let mut run = Run::new(6_200, 100 * (64 * 1024 + 4 * MIB) + 6_000 * 1024);
        for _ in 0..100 {
            run.copy(64 * 1024, 10 * MIB, 0.05);
            run.copy(4 * MIB, 10 * MIB, 0.05);
        }
        let before = run.left();
        for _ in 0..5_000 {
            run.skip(1024, 0.02);
            // The skipped ones leave the work, and their time does not slow the pace.
            assert!(run.left() < before, "{} after {before}", run.left());
        }
        let left = run.left();
        let then = run.at;
        for _ in 0..1_000 {
            run.copy(1024, 10 * MIB, 0.05);
        }
        let took = run.at - then;
        assert!((left - took).abs() < took * 0.2, "{left} for {took}");
    }

    #[test]
    fn a_big_file_after_tiny_ones_never_looks_done() {
        // 2,000 files of up to 30 KiB, each opened in a tenth of a second give or take a
        // third, then a file of 4 GiB at 100 MiB a second: the bytes of the small files cost
        // nothing beside the noise of opening them.
        for seed in 0..10 {
            let mut noise = Noise(seed);
            let sizes: Vec<u64> = (0..2_000).map(|_| noise.below(30 * 1024)).collect();
            let big = 4_096 * MIB;
            let mut run = Run::new(2_001, sizes.iter().sum::<u64>() + big);
            for size in sizes {
                let opening = noise.around(0.1, 0.3);
                run.copy(size, 100 * MIB, opening);
            }
            run.at += 0.1;
            let start = run.at;
            let took = float(big) / float(100 * MIB);
            for part in 1..=8 {
                run.stream(big / 8, 100 * MIB);
                let truth = took - (run.at - start);
                let left = run.left();
                // Never as if it took no time; and right from the second second on.
                assert!(left > truth * 0.8, "seed {seed}: {left} for {truth}");
                if part > 1 {
                    assert!(left < truth * 1.2 + 1.0, "seed {seed}: {left} for {truth}");
                }
            }
        }
    }

    #[test]
    fn the_time_left_rides_out_a_pace_that_comes_and_goes() {
        // 2 and 8 MiB a second, by turns, through one file of 2,000 MiB.
        let mut run = Run::new(1, 2_000 * MIB);
        let pace = |second: u64| {
            if second.is_multiple_of(2) {
                2 * MIB
            } else {
                8 * MIB
            }
        };
        for second in 0..60 {
            run.stream(pace(second), pace(second));
        }
        let mut speeds = Vec::new();
        let mut shown = Vec::new();
        for second in 60..70 {
            run.stream(pace(second), pace(second));
            let timing = run.view.timing(run.now()).unwrap();
            speeds.push(timing.speed.unwrap() / float(MIB));
            shown.push(whole_seconds(timing.left.unwrap()));
            // The time left keeps within 3% of what 5 MiB a second leaves …
            let left = run.left();
            let bytes = run.counts.bytes_total - run.counts.bytes_done;
            let even = float(bytes) / float(5 * MIB);
            assert!((left - even).abs() < even * 0.03, "{left} for {even}");
        }
        // … while the speed of the last five seconds swings by a fifth either way.
        let (slow, fast) = speeds
            .iter()
            .fold((f64::MAX, 0.0_f64), |(slow, fast), speed| {
                (slow.min(*speed), fast.max(*speed))
            });
        assert!(slow < 4.5 && fast > 5.5, "{speeds:?}");
        // The time left shown goes down by the clock.
        for pair in shown.windows(2) {
            assert_eq!(pair[0] - pair[1], 1, "{shown:?}");
        }
    }

    #[test]
    fn the_time_left_follows_a_new_pace_within_a_minute() {
        let mut run = Run::new(1, 4_000 * MIB);
        run.stream(600 * MIB, 10 * MIB);
        // Half as fast from now on.
        run.stream(150 * MIB, 5 * MIB);
        let left = run.left();
        let (old, new) = (3_250.0 / 10.0, 3_250.0 / 5.0);
        assert!(left > old * 1.2 && left < new, "{left}");
        run.stream(150 * MIB, 5 * MIB);
        let left = run.left();
        let new = 3_100.0 / 5.0;
        assert!((left - new).abs() < new * 0.15, "{left}");
    }

    #[test]
    fn many_small_files_left_take_longer_than_their_bytes_say() {
        // A twentieth of a second to open each entry, then 10 MiB a second: files of 64 KiB
        // and of 4 MiB by turns, then 1,000 files of 1 KiB, which take some 50 seconds.
        let mut run = Run::new(1_200, 100 * (64 * 1024 + 4 * MIB) + 1_000 * 1024);
        for _ in 0..100 {
            run.copy(64 * 1024, 10 * MIB, 0.05);
            run.copy(4 * MIB, 10 * MIB, 0.05);
        }
        let left = run.left();
        let then = run.at;
        for _ in 0..1_000 {
            run.copy(1024, 10 * MIB, 0.05);
        }
        let took = run.at - then;
        assert!((left - took).abs() < took * 0.2, "{left} for {took}");
    }

    #[test]
    fn entries_skipped_at_once_do_not_make_the_rest_look_quick() {
        let mut run = Run::new(6_200, 100 * (64 * 1024 + 4 * MIB) + 6_000 * 1024);
        for _ in 0..100 {
            run.copy(64 * 1024, 10 * MIB, 0.05);
            run.copy(4 * MIB, 10 * MIB, 0.05);
        }
        // Skip all: 5,000 files of 1 KiB that are there already, in one report.
        run.counts.done += 5_000;
        run.counts.bytes_done += 5_000 * 1024;
        run.report();
        for _ in 0..20 {
            run.copy(1024, 10 * MIB, 0.05);
        }
        let left = run.left();
        let then = run.at;
        for _ in 0..980 {
            run.copy(1024, 10 * MIB, 0.05);
        }
        let took = run.at - then;
        assert!((left - took).abs() < took * 0.2, "{left} for {took}");
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
