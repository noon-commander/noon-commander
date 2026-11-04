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

/// How far the time left shown may stray from the forecast smoothed, as a share of it, and only
/// count down by the clock, a second each second: what is left of the forecast's ups and downs
/// keeps within it.
const ETA_NEAR: f64 = 0.01;

/// Beyond this far from the forecast, as a share of it, the time left shown moves halfway to it
/// each second, so that a lasting change shows within a few seconds without a jump.
const ETA_SLACK: f64 = 0.1;

/// More than this many times as long as the forecast, or as short, the time left shown goes to
/// it at once: halving the way from hours to the minutes a forecast has learned took seconds of
/// showing hours.
const ETA_JUMP: f64 = 2.0;

/// How long the time left shown takes to follow the forecast, as a share of the time left, up
/// to [`ETA_SMOOTH_MAX`]: long enough to even out the forecast's ups and downs, short enough
/// that one still settling, early in a short job, is soon followed. Following a forecast that
/// stands still, it lags by about as much, which with [`ETA_NEAR`] must keep within
/// [`ETA_SLACK`]: beyond, it would move halfway up, lag again, and go up again and again.
const ETA_SMOOTH: f64 = ETA_SLACK - ETA_NEAR;

/// The longest the time left shown takes to follow the forecast.
const ETA_SMOOTH_MAX: Duration = Duration::from_secs(30);

/// How many forecasts, a second of work apart at least, must agree before the time left shows:
/// early in a job the forecast knows little of its pace, and may be off three times over.
const ETA_SETTLE: usize = 3;

/// How far those forecasts may differ, each less the time since it was made: the longest at most
/// this many times the shortest.
const ETA_AGREE: f64 = 1.25;

/// The time left shows after this much work that tells the pace, whether or not its forecasts
/// agree: neither skipping nor standing still, which tell none, however long they take.
const ETA_SETTLE_MAX: Duration = ETA_HORIZON;

/// A report made this much before the speed and the time left of a second were worked out
/// comes late, as one that waited while the event loop was held up, by a terminal slow to take
/// a frame: they knew nothing of it, and of what came after it, so they are worked out again.
/// Far longer than a report takes to reach the window.
const LATE: Duration = Duration::from_millis(250);

/// How many times as large as the entries done those left may be on average, while the work
/// behind has not told what a byte costs, for the time left to show: small files of one size,
/// then a big one, showed the big one at their own pace, hours for minutes, until its data
/// streamed. Once the entry under way has moved this many times the bytes of the largest entry
/// done, its data streaming tells what a byte costs, rather than the entries done: their bytes
/// may go at another pace, as into a cache that a big file fills, or over a link whose window
/// holds a small file whole.
const AHEAD: f64 = 10.0;

/// How far back the pauses between a job's reports tell how long it may go without one and
/// still work: a link that sends a chunk every few seconds, a disk that takes its time to
/// write what it holds back, every so often.
const PAUSES: Duration = Duration::from_secs(60);

/// The fewest entries that the share of the work skipped looks back over, beside [`RECENT`] of
/// work: files of a few MiB over a link take seconds each, and a handful of them tell little.
const SKIPS_SPAN: u64 = 30;

/// While more than this share of the recent work was skipped, the time left is unknown: what is
/// left to skip counts as work, so the forecast would be three times too long or more.
const SKIPPING: f64 = 2.0 / 3.0;

/// Once less than this share of the recent work is skipped, the time left shows again; in
/// between it stays as it was, so that it does not come and go while the share keeps near
/// either.
const SKIPPING_ENDS: f64 = 1.0 / 3.0;

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
    /// The report came as the job started an entry, those before it done: the entry opens
    /// after it, before any of its data moves. Or the job asked a question after it, and may
    /// open the entry again once answered.
    starts: bool,
}

impl Point {
    fn new(at: Duration, counts: &Counts) -> Self {
        Self {
            at,
            entries: counts.done,
            done: counts.bytes_done,
            copied: counts.bytes_copied,
            starts: false,
        }
    }
}

/// What a job had done, and skipped of it, at some moment of its work, for the share of its
/// recent work that it skipped.
#[derive(Debug, Clone, Copy)]
struct Tally {
    at: Duration,
    entries: u64,
    copied: u64,
    /// Bytes and entries skipped by then.
    skipped: u64,
    skipped_entries: u64,
}

/// Below this, a sum that fades and gets nothing new tells nothing: work some three hours old.
/// Left to fade further, sums stick among the smallest floats, alike, and their ratios turn to
/// one.
const FADED: f64 = 1e-100;

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
        self.forget();
        let bytes = float(to.copied.saturating_sub(from.copied));
        let entries = to.entries.saturating_sub(from.entries);
        self.time += time;
        self.bytes += bytes;
        self.entries += float(entries);
        // A stretch that starts as an entry starts holds its opening.
        if entries == 0 && bytes > 0.0 && !from.starts {
            self.stream_time += time;
            self.stream_bytes += bytes;
        }
    }

    /// Lets go of what has faded below [`FADED`]: each group of sums at once, so that what is
    /// left keeps its ratios.
    fn forget(&mut self) {
        for sum in [&mut self.bytes, &mut self.entries] {
            if *sum < FADED {
                *sum = 0.0;
            }
        }
        if self.stream_time < FADED || self.stream_bytes < FADED {
            self.stream_time = 0.0;
            self.stream_bytes = 0.0;
        }
        if self.rows < FADED {
            let fit = Sums {
                time: self.time,
                bytes: self.bytes,
                entries: self.entries,
                stream_time: self.stream_time,
                stream_bytes: self.stream_bytes,
                ..Sums::default()
            };
            *self = fit;
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

    /// What a byte and an entry cost, in proportion.
    ///
    /// The stretches between finished entries tell both by least squares, wherever the time of
    /// an entry falls among its reports, even after the last of them, as where reports run ahead
    /// of the data, while each stands well clear of the noise. Else a byte costs what it does
    /// while data streams within entries, and an entry what the stretches of entries took beyond
    /// their bytes. Else, with no data streaming yet, as among small files only, the least
    /// squares still tell the cost of an entry if it stands clear; that of their bytes is mostly
    /// noise then, and no less than nothing. Else bytes alone count, as for files of one size.
    fn costs(&self) -> (f64, f64) {
        let fitted = self.fitted();
        if let Some(((byte, entry), (true, true))) = fitted {
            return (byte, entry);
        }
        if let Some((byte, entry)) = self.streamed() {
            return (byte, entry.max(0.0));
        }
        if let Some(((byte, entry), (_, true))) = fitted {
            return (byte.max(0.0), entry);
        }
        (1.0, 0.0)
    }

    /// What a byte and an entry cost by the data streaming within entries: a byte what it
    /// takes there, an entry what the stretches between finished entries took beyond their
    /// bytes at that cost, which is less than nothing where their bytes went faster. Nothing
    /// before any data has streamed.
    fn streamed(&self) -> Option<(f64, f64)> {
        if self.stream_bytes <= 0.0 || self.stream_time <= 0.0 {
            return None;
        }
        let byte = self.stream_time / self.stream_bytes;
        let entry = if self.nn > 0.0 {
            (self.nt - byte * self.bn) / self.nn
        } else {
            0.0
        };
        Some((byte, entry))
    }

    /// The least squares of the time of the stretches between finished entries on their bytes
    /// and their entries: what a byte and an entry cost, and whether each is above nothing by
    /// at least twice its standard error. Nothing while the bytes and the entries of the
    /// stretches go too much alike to tell apart, or there are too few.
    fn fitted(&self) -> Option<((f64, f64), (bool, bool))> {
        let det = self.bb * self.nn - self.bn * self.bn;
        if self.rows <= 3.0 || det <= 1e-3 * self.bb * self.nn {
            return None;
        }
        let byte = (self.bt * self.nn - self.bn * self.nt) / det;
        let entry = (self.bb * self.nt - self.bn * self.bt) / det;
        let squares = self.tt - 2.0 * (byte * self.bt + entry * self.nt)
            + byte * byte * self.bb
            + 2.0 * byte * entry * self.bn
            + entry * entry * self.nn;
        let noise = squares.max(0.0) / (self.rows - 2.0);
        // The variance of a cost is the noise times its own sum of squares, over `det`.
        let clear = |cost: f64, sum: f64| cost > 0.0 && cost * cost * det >= 4.0 * noise * sum;
        Some(((byte, entry), (clear(byte, self.nn), clear(entry, self.bb))))
    }
}

/// The time left of a job: the work left, bytes and entries, at the pace of the work behind
/// it over about [`ETA_HORIZON`] of it. An entry costs time as some bytes do, opening,
/// creating, setting attributes, and how many is learned as the job goes, so that many small
/// files left take longer than their bytes say. It changes for defects only (ADR 0056).
#[derive(Debug, Default)]
struct Forecast {
    /// Where the job was when it first reported, or reported again after counting.
    first: Option<Point>,
    /// The time of the stretches that count for the pace: work, neither skipping nor standing
    /// still.
    paced: Duration,
    /// Where the last stretch of the pace ended.
    last: Option<Point>,
    /// Where the job was when it last had done more entries.
    finished: Option<Point>,
    /// When the job last reported, in the time it worked, and with how many entries done.
    seen: Option<Duration>,
    entries: u64,
    /// The pauses between reports of over half [`RECENT`] within the last [`PAUSES`] of work:
    /// when each ended, and how long it was.
    pauses: VecDeque<(Duration, Duration)>,
    /// Time in which the job stood still, beyond the pauses it takes as it works: it is left
    /// out, so that the pace is that of work when work goes on.
    still: Duration,
    sums: Sums,
    /// The last [`ETA_SETTLE`] forecasts, a second of work apart at least, with the time each
    /// was made, until they agree.
    settling: [Option<(Duration, Duration)>; ETA_SETTLE],
    /// They have agreed once, so that the time left shows.
    settled: bool,
    /// The least and the most bytes an entry had, of the finished ones with data.
    sizes: Option<(f64, f64)>,
    /// The work behind has told what a byte costs once: data streamed within an entry, or
    /// entries with data of sizes at least twice apart. Then the time left shows whatever the
    /// sizes of the entries left.
    told: bool,
    /// Bytes copied, as the last report said.
    copied: u64,
    /// The job asked about a failure after a report with these entries done and bytes copied:
    /// the try that failed, and what it does once answered, such as opening the entry again
    /// for Retry, tell nothing of its pace. Stretches count again after one that ends with more
    /// of either.
    failure: Option<(u64, u64)>,
    /// The job asked about a failure within the entry under way, whose cost it does not tell.
    failed_entry: bool,
    /// Where the job was at its last report.
    reported: Option<Point>,
    skips: Skips,
}

/// What a job skipped, for the share of its recent work that it skipped.
#[derive(Debug, Default)]
struct Skips {
    /// Bytes and entries skipped so far, by the stretches between reports that skipped data.
    bytes: u64,
    entries: u64,
    /// What the job had done and skipped as entries were done, back over [`RECENT`] of work
    /// and [`SKIPS_SPAN`] entries at least.
    tallies: VecDeque<Tally>,
    /// More than [`SKIPPING`] of the recent work was skipped, and not yet less than
    /// [`SKIPPING_ENDS`] since: the time left is unknown.
    skipping: bool,
}

impl Skips {
    /// Counts what the job skipped from `from` to `to`, two reports one after the other
    /// between which it [`skipped`] data.
    fn skipped(&mut self, from: Point, to: Point) {
        self.bytes += to.done.saturating_sub(from.done) - to.copied.saturating_sub(from.copied);
        self.entries += to.entries.saturating_sub(from.entries);
    }

    /// What the job had done and skipped at `point`.
    fn tally(&self, point: Point) -> Tally {
        Tally {
            at: point.at,
            entries: point.entries,
            copied: point.copied,
            skipped: self.bytes,
            skipped_entries: self.entries,
        }
    }

    /// Takes what the job had done and skipped at `point`, the last report, once it has done
    /// more entries a [`SAMPLE`] after the last tally, and lets go of those beyond [`RECENT`] of
    /// work and [`SKIPS_SPAN`] entries. Then it is [`skipping`](Self::skipping) while more than
    /// [`SKIPPING`] of the work since the first tally was skipped, at the costs of `sums`, and
    /// once the tallies go back that far, until less than [`SKIPPING_ENDS`] is: over fewer, as
    /// early in a job, a share tells little, and a first file skipped would keep the time left
    /// unknown.
    fn take(&mut self, point: Point, sums: &Sums) {
        if self
            .tallies
            .back()
            .is_none_or(|back| point.entries > back.entries && point.at >= back.at + SAMPLE)
        {
            self.tallies.push_back(self.tally(point));
        }
        let far = |tally: &Tally| {
            tally.at + RECENT <= point.at && tally.entries + SKIPS_SPAN <= point.entries
        };
        while self.tallies.get(1).is_some_and(far) {
            self.tallies.pop_front();
        }
        let full = self.tallies.front().is_some_and(far);
        let share = self.share(point, sums);
        self.skipping = if self.skipping && full {
            share >= SKIPPING_ENDS
        } else {
            share > SKIPPING
        };
    }

    /// The share of the work from the first tally to `point`, the last report, that the job
    /// skipped: bytes and entries, each at its cost in `sums`, skipped over those copied or
    /// skipped.
    fn share(&self, point: Point, sums: &Sums) -> f64 {
        let Some(from) = self.tallies.front() else {
            return 0.0;
        };
        if self.bytes == from.skipped {
            return 0.0;
        }
        let skipped = float(self.bytes - from.skipped);
        let skipped_entries = float(self.entries - from.skipped_entries);
        let copied = float(point.copied.saturating_sub(from.copied));
        let entries = float(point.entries.saturating_sub(from.entries));
        let (byte, entry) = sums.costs();
        let all = byte * (copied + skipped) + entry * entries;
        if all > 0.0 {
            (byte * skipped + entry * skipped_entries) / all
        } else {
            0.0
        }
    }
}

impl Forecast {
    /// The job had `counts` behind it when it had worked `at`.
    fn note(&mut self, at: Duration, counts: &Counts) {
        let mut point = Point::new(at, counts);
        // A pause around a failure is not one the job takes as it works.
        if let Some(seen) = self.seen.filter(|_| self.failure.is_none()) {
            let pause = at.saturating_sub(seen);
            let (usual, stall) = self.pace_of_pauses(at);
            if pause >= stall {
                self.still += pause.saturating_sub(usual);
            }
            while self
                .pauses
                .front()
                .is_some_and(|&(end, _)| end + PAUSES < at)
            {
                self.pauses.pop_front();
            }
            if pause > RECENT / 2 {
                self.pauses.push_back((at, pause));
            }
        }
        // A job's first report comes as it starts its first entry.
        point.starts = self.seen.is_none() || counts.done > self.entries;
        self.seen = Some(at);
        self.entries = counts.done;
        self.copied = counts.bytes_copied;
        point.at = at.saturating_sub(self.still);
        let (Some(last), Some(finished), Some(reported)) =
            (self.last, self.finished, self.reported)
        else {
            self.first = Some(point);
            self.last = Some(point);
            self.finished = Some(point);
            self.reported = Some(point);
            self.skips.take(point, &self.sums);
            return;
        };
        // Skipped since the last report: only that is left out, not the work before it within
        // the stretch, so that skipped files among copied ones, many in a quarter of a second
        // on a fast disk, still leave a pace.
        let skip = skipped(reported, point);
        if skip {
            self.skips.skipped(reported, point);
        }
        if skip || point.at >= last.at + SAMPLE {
            if let Some((entries, copied)) = self.failure {
                // As a pause, which the work behind does not fade over. A report with nothing
                // new, as a job may send before it tries again, does not end it.
                if point.entries > entries || point.copied > copied {
                    self.failure = None;
                }
            } else {
                let end = if skip { reported } else { point };
                if end.at > last.at {
                    if !skipped(last, end) {
                        self.paced += end.at.saturating_sub(last.at);
                    }
                    self.sums.add(last, end);
                }
            }
            self.last = Some(point);
        }
        self.reported = Some(point);
        if point.entries > finished.entries {
            if !std::mem::take(&mut self.failed_entry) {
                self.sums.fit(finished, point);
                self.sized(finished, point);
            }
            self.finished = Some(point);
        }
        self.told |= self.sums.stream_bytes > 0.0
            || self.sizes.is_some_and(|(least, most)| most >= 2.0 * least);
        self.skips.take(point, &self.sums);
        self.settle(at, counts);
    }

    /// The job asked a question: what it does once answered, such as opening the entry again
    /// for Retry, comes with no report first, so the stretch now under way is no data streaming.
    fn asked(&mut self) {
        if let Some(last) = &mut self.last {
            last.starts = true;
        }
    }

    /// The job asked what to do about a failure: since its last report it tried and failed,
    /// and once answered it may try again, as for Retry. Neither is the pace of its work.
    fn failed(&mut self) {
        if self.last.is_some() {
            self.failure = Some((self.entries, self.copied));
            self.failed_entry = true;
        }
    }

    /// Takes the size of the entries done from `from` to `to`, if they had data.
    fn sized(&mut self, from: Point, to: Point) {
        if skipped(from, to) {
            return;
        }
        let bytes = float(to.copied.saturating_sub(from.copied));
        let size = bytes / float(to.entries - from.entries);
        if size > 0.0 {
            let (least, most) = self.sizes.unwrap_or((size, size));
            self.sizes = Some((least.min(size), most.max(size)));
        }
    }

    /// Takes the forecast at `at`, with `counts` behind the job, a second of work after the last
    /// one taken, until the last [`ETA_SETTLE`] of them agree.
    fn settle(&mut self, at: Duration, counts: &Counts) {
        let taken = self.settling[ETA_SETTLE - 1];
        if self.settled || taken.is_some_and(|(then, _)| at < then + Duration::from_secs(1)) {
            return;
        }
        let Some(left) = self.left(at, counts) else {
            return;
        };
        self.settling.rotate_left(1);
        self.settling[ETA_SETTLE - 1] = Some((at, left));
        self.settled = agree(&self.settling, at);
    }

    /// Whether the time left shows: its forecasts have agreed, or the job has done
    /// [`ETA_SETTLE_MAX`] of work that tells the pace.
    fn shows(&self) -> bool {
        self.settled || self.paced >= ETA_SETTLE_MAX
    }

    /// When the job first reported, or reported again after counting, in the time it worked.
    fn began(&self) -> Option<Duration> {
        self.first.map(|first| first.at)
    }

    /// Whether the entries left, with `counts` behind the job, are on average more than
    /// [`AHEAD`] times as large as those it has finished since its first report, so that what
    /// their bytes cost cannot come from those. Not before it has finished any data. Those done
    /// before do not count: a move counts the entries it renamed, which have no data.
    fn bigger_ahead(&self, counts: &Counts) -> bool {
        let (Some(first), Some(finished)) = (self.first, self.finished) else {
            return false;
        };
        let left = counts.total.saturating_sub(finished.entries);
        let entries = finished.entries.saturating_sub(first.entries);
        let done = finished.done.saturating_sub(first.done);
        if entries == 0 || done == 0 || left == 0 {
            return false;
        }
        let ahead = float(counts.bytes_total.saturating_sub(finished.done)) / float(left);
        let behind = float(done) / float(entries);
        ahead > AHEAD * behind
    }

    /// What a byte and an entry left cost, with `counts` behind the job, and an entry behind,
    /// which weighs the work behind for its pace: as the sums say, but by the data streaming
    /// once the entry under way has moved more than [`AHEAD`] times the bytes of the largest
    /// entry finished: the entries finished tell nothing of what its bytes cost, and it tells
    /// them nothing until it ends. An entry behind then costs what it took beyond its bytes at
    /// that cost, less than nothing where their bytes went faster than those streaming: at
    /// none, as an entry left costs then, small files that took the time of their bytes
    /// weighed as many times what they took, and the time left came out as many times shorter.
    fn costs(&self, counts: &Counts) -> (f64, f64, f64) {
        let beyond = match (self.finished, self.sizes) {
            (Some(finished), Some((_, most))) => {
                float(counts.bytes_copied.saturating_sub(finished.copied)) > AHEAD * most
            }
            _ => false,
        };
        if beyond && let Some((byte, entry)) = self.sums.streamed() {
            return (byte, entry.max(0.0), entry);
        }
        let (byte, entry) = self.sums.costs();
        (byte, entry, entry)
    }

    /// The pauses the job takes as it works, at `at`: the longest of the last [`PAUSES`], and
    /// how long one may be before the job stands still: twice that, or [`RECENT`] at least.
    fn pace_of_pauses(&self, at: Duration) -> (Duration, Duration) {
        let usual = self
            .pauses
            .iter()
            .filter(|&&(end, _)| end + PAUSES >= at)
            .map(|&(_, pause)| pause)
            .max()
            .unwrap_or_default();
        (usual, (usual * 2).max(RECENT))
    }

    /// The time left at `at`, with `counts` behind the job: unknown once no report has come
    /// for longer than the job pauses as it works, five seconds at least, as the speed falls
    /// to nothing then; before any work has a pace; while the entries left are far larger than
    /// those done and the work has not told what a byte costs; or while most of the recent work
    /// was skipped, as what is left to skip counts as work.
    fn left(&self, at: Duration, counts: &Counts) -> Option<Duration> {
        let (_, stall) = self.pace_of_pauses(at);
        if at.saturating_sub(self.seen?) >= stall {
            return None;
        }
        if self.skips.skipping || !self.told && self.bigger_ahead(counts) {
            return None;
        }
        let sums = &self.sums;
        let bytes = float(counts.bytes_total.saturating_sub(counts.bytes_done));
        let entries = float(counts.total.saturating_sub(counts.done));
        let (byte, entry, behind) = self.costs(counts);
        let mut done = byte * sums.bytes + behind * sums.entries;
        if done <= 0.0 {
            done = byte * sums.bytes + entry * sums.entries;
        }
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

/// Whether the forecasts `taken`, each with the time it was made, agree at `at`: all are there
/// and, each less the time since, the longest is at most [`ETA_AGREE`] times the shortest.
fn agree(taken: &[Option<(Duration, Duration)>], at: Duration) -> bool {
    let (mut shortest, mut longest) = (f64::INFINITY, 0.0_f64);
    for &taken in taken {
        let Some((then, left)) = taken else {
            return false;
        };
        let now = left.as_secs_f64() - at.saturating_sub(then).as_secs_f64();
        (shortest, longest) = (shortest.min(now), longest.max(now));
    }
    shortest > 0.0 && longest <= ETA_AGREE * shortest
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
    /// The time the job had worked when they were worked out.
    at: Duration,
    speed: Option<f64>,
    left: Option<Left>,
    /// The time left of an earlier second that `left` counted down from.
    before: Option<(u64, Left)>,
    /// A report made well before they were worked out came after, so they are worked out
    /// again.
    late: bool,
}

/// The time left of a job in some second, as [`count_down`] works it out.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Left {
    /// As it shows, with its fraction of a second.
    shown: Duration,
    /// The forecast smoothed.
    smooth: Duration,
}

/// What the window of a job shows at some moment, before it is put in words, apart from its
/// buttons and the shine over a gauge not known yet: the window draws the same as long as this
/// stays the same. It takes no formatting, so it can be made for every report.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Look {
    aborting: bool,
    /// The host it waits for, whose connection was lost.
    away: Option<String>,
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
            away: self.away.is_some(),
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
    away: bool,
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
    /// The host it waits for, whose connection was lost.
    away: Option<String>,
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
            away: None,
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

    /// The job counts what to do, `items` so far, as it has since `now`. Counting after some
    /// work, as a move does before it copies what it could not rename, is no work of what
    /// follows, which has totals of its own: the speed and the time left start over, and the
    /// clock stands while it counts, as it does before the first report.
    pub(crate) fn scanning(&mut self, items: u64, now: Instant) {
        if matches!(self.stage, Stage::Working { .. }) {
            self.clock.stop(now);
            self.samples = Samples::default();
            self.forecast = Forecast::default();
            self.held.set(None);
        }
        self.stage = Stage::Scanning { items };
    }

    /// The job is at `current`, shown as given, with `counts` behind it, as it was at `now`,
    /// when it made the report, however long ago. Its clock starts with the first of these,
    /// after the waiting and the counting.
    pub(crate) fn working(&mut self, current: String, counts: Counts, now: Instant) {
        self.stage = Stage::Working { current, counts };
        self.passed(counts, now);
    }

    /// The job had `counts` behind it at `now`, as a report said that a later one of the same
    /// batch follows: it counts for the speed and the time left, as [`working`](Self::working)
    /// does, but the window shows the last.
    pub(crate) fn passed(&mut self, counts: Counts, now: Instant) {
        if !self.asking {
            self.clock.start(now);
        }
        let at = self.clock.elapsed(now);
        if let Some(held) = self.held.get().filter(|held| held.at > at + LATE) {
            self.held.set(Some(Held { late: true, ..held }));
        }
        self.samples.note(at, counts.bytes_copied);
        self.forecast.note(at, &counts);
    }

    /// The job asked a question at `now` and waits for the answer: its clock stands, however
    /// long the question stays open or waits behind other dialogs.
    pub(crate) fn ask(&mut self, now: Instant) {
        self.asking = true;
        self.clock.stop(now);
        self.forecast.asked();
    }

    /// The job asked at `now` what to do about a failure, as [`ask`](Self::ask) does: what it
    /// did since its last report, and does once answered until its work goes on, is no pace of
    /// that work for the time left.
    pub(crate) fn failed(&mut self, now: Instant) {
        self.ask(now);
        self.forecast.failed();
    }

    /// The question was answered, and the job goes on.
    pub(crate) fn answered(&mut self, now: Instant) {
        self.asking = false;
        if matches!(self.stage, Stage::Working { .. }) {
            self.clock.start(now);
        }
    }

    /// Whether the job has entries after the one it is at, as far as it knows: while it counts,
    /// it may.
    pub(crate) fn more(&self) -> bool {
        match &self.stage {
            Stage::Working { counts, .. } => counts.done + 1 < counts.total,
            Stage::Waiting | Stage::Scanning { .. } => true,
        }
    }

    /// Whether the job works on one entry alone, which it counted.
    pub(crate) fn single(&self) -> bool {
        matches!(&self.stage, Stage::Working { counts, .. } if counts.total <= 1)
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
    /// the time left are those of the first call in the second that the time shows, or the
    /// first since a report taken late (`working`).
    fn timing_of(&self, counts: &Counts, now: Instant) -> Timing {
        let elapsed = self.clock.elapsed(now);
        let second = whole_seconds(elapsed);
        let held = match self.held.get() {
            Some(held) if held.second == second && !held.late => held,
            _ => {
                let held = self.measure(counts, elapsed, second);
                self.held.set(Some(held));
                held
            }
        };
        Timing {
            elapsed,
            speed: held.speed,
            left: held.left.map(|left| left.shown),
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
            away: self.away.clone(),
            stage,
        }
    }

    /// The speed and the time left after `elapsed` of work, in its `second`, with `counts`
    /// behind it. The time left is unknown until its forecasts have agreed, or the job has done
    /// [`ETA_SETTLE_MAX`] of work that tells the pace ([`Forecast::shows`]). It counts down from
    /// that of an earlier second, the one it counted down from if worked out again within the
    /// same second.
    fn measure(&self, counts: &Counts, elapsed: Duration, second: u64) -> Held {
        let before = match self.held.get() {
            Some(held) if held.second == second => held.before,
            Some(held) => held.left.map(|left| (held.second, left)),
            None => None,
        };
        // Since the job first reported, or reported again after counting.
        let worked = elapsed.saturating_sub(self.forecast.began().unwrap_or_default());
        let (speed, left) = if counts.bytes_total > 0 && worked >= SETTLED {
            let speed = self.samples.speed(elapsed, counts.bytes_copied);
            let shows = self.forecast.shows();
            let left = self
                .forecast
                .left(elapsed, counts)
                .filter(|_| shows)
                .map(|forecast| count_down(before, second, forecast));
            (speed, left)
        } else {
            (None, None)
        };
        Held {
            second,
            at: elapsed,
            speed,
            left,
            before,
            late: false,
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
            _ if row.away => fl!("jobs-state-away"),
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

    /// The job waits for `host`, shown as given, whose connection was lost; `None` once it
    /// goes on.
    pub(crate) fn away(&mut self, host: Option<String>) {
        self.away = host;
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
        let doing = match &look.away {
            _ if look.aborting => fl!("job-aborting"),
            Some(host) => fl!("job-away", host = host.as_str()),
            None => doing,
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

/// The time left to show in `second` by the `forecast`, after the one shown `before`, in an
/// earlier second.
///
/// The time at which the forecast says the job ends is smoothed, each second taking the
/// [`ETA_SMOOTH`] part of the time left as long to follow: a forecast that is right says the
/// same end every second, so its ups and downs even out and it does not lag behind. The one
/// shown before, less the seconds since, shows while it keeps within [`ETA_NEAR`] of the
/// forecast smoothed; else the nearer edge of that band, so that the time left counts down by
/// the clock, follows the forecast without going up and down, and holds still where the
/// forecast does. Beyond [`ETA_SLACK`] from the forecast, it moves halfway there for each second
/// since, and the smoothing starts over from it. The forecast itself without one before, once
/// the count is down to nothing, or [`ETA_JUMP`] times as far from it, either way. It keeps its
/// fraction of a second, which shows rounded.
fn count_down(before: Option<(u64, Left)>, second: u64, forecast: Duration) -> Left {
    let fresh = Left {
        shown: forecast,
        smooth: forecast,
    };
    let Some((then, before)) = before else {
        return fresh;
    };
    let passed = second.saturating_sub(then);
    let counted = before.shown.saturating_sub(Duration::from_secs(passed));
    if counted.is_zero() {
        return fresh;
    }
    let times = i32::try_from(passed).unwrap_or(i32::MAX);
    let (from, to) = (counted.as_secs_f64(), forecast.as_secs_f64());
    if from > ETA_JUMP * to || to > ETA_JUMP * from {
        return fresh;
    }
    if (to - from).abs() > ETA_SLACK * to {
        let left = seconds(to + (from - to) * 0.5_f64.powi(times));
        return Left {
            shown: left,
            smooth: left,
        };
    }
    let lasting = (ETA_SMOOTH * to).clamp(1.0, ETA_SMOOTH_MAX.as_secs_f64());
    let smoothed = before.smooth.as_secs_f64() - float(passed);
    let smooth = to + (smoothed - to) * (1.0 - 1.0 / lasting).powi(times);
    let band = ETA_NEAR * smooth.max(0.0);
    Left {
        shown: seconds(from.clamp(smooth - band, smooth + band)),
        smooth: seconds(smooth),
    }
}

/// A time of so many seconds, none if less, and the longest there is if more.
fn seconds(seconds: f64) -> Duration {
    Duration::try_from_secs_f64(seconds.max(0.0)).unwrap_or(Duration::MAX)
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
mod tests;
