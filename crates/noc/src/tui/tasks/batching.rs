//! Batching the reports of a job before they reach the app.

use std::time::{Duration, Instant};

use noc_ops::Event;
use noc_vfs::{Location, VfsError};
use tokio::sync::mpsc;

use crate::tui::describe;
use crate::tui::progress::Counts;

use super::{Done, JobEvent, TakenName};

/// The least time between two batches of a job's reports of how far it is: a copy reports
/// every chunk it moves, thousands a second, and the UI takes a batch as it would take one
/// report. Shorter than the frames drawn for results of background work (`RESULTS_FRAME`), so
/// that a batch never waits for a frame.
pub(super) const BATCH: Duration = Duration::from_millis(20);

/// A job's reports of how far it is that wait to go to the UI together: of its counting, the
/// last, with the time of the first, and of its progress, each with the time the job made it.
pub(super) struct Waiting<P> {
    /// When the first report of counting that waits was made, and the entries found, as the
    /// last said. Reports of progress that came before it went before it waited, so it goes
    /// before those that wait with it.
    found: Option<(Instant, u64)>,
    reports: Vec<(Instant, Counts)>,
    /// The entry at hand in the last of them.
    current: Option<P>,
    /// When the last batch went.
    sent: Option<Instant>,
}

/// What goes to the UI at once of what a job reported: the entries found as it counts, with
/// the time of the first report of them, then its reports of progress, the last at its entry.
pub(super) type Batch<P> = (Option<(Instant, u64)>, Option<(P, Vec<(Instant, Counts)>)>);

impl<P> Waiting<P> {
    pub(super) fn new() -> Self {
        Self {
            found: None,
            reports: Vec::new(),
            current: None,
            sent: None,
        }
    }

    /// Takes a report made at `at`, at `current` with `counts`, and tells whether the batch
    /// goes now: the first after none went for [`BATCH`] goes at once.
    pub(super) fn take(&mut self, at: Instant, current: P, counts: Counts) -> bool {
        self.reports.push((at, counts));
        self.current = Some(current);
        self.free(at)
    }

    /// Takes a report of counting made at `at`, with `items` found, in place of one that
    /// waits, whose time it keeps, and tells whether the batch goes now. Reports of progress
    /// that wait must go first ([`progress`](Self::progress)).
    pub(super) fn count(&mut self, at: Instant, items: u64) -> bool {
        let first = self.found.map_or(at, |(first, _)| first);
        self.found = Some((first, items));
        self.free(at)
    }

    /// Whether reports of progress wait.
    pub(super) fn progress(&self) -> bool {
        self.current.is_some()
    }

    /// Whether a batch may go at `at`: none went for [`BATCH`].
    fn free(&self, at: Instant) -> bool {
        self.sent.is_none_or(|sent| at >= sent + BATCH)
    }

    /// When the reports that wait are due: [`BATCH`] after the last batch went.
    pub(super) fn due(&self) -> Option<Instant> {
        if self.found.is_none() && self.current.is_none() {
            return None;
        }
        self.sent.map(|sent| sent + BATCH)
    }

    /// The batch to send at `now`: what waits.
    pub(super) fn send(&mut self, now: Instant) -> Batch<P> {
        let found = self.found.take();
        let progress = self
            .current
            .take()
            .map(|current| (current, std::mem::take(&mut self.reports)));
        if found.is_some() || progress.is_some() {
            self.sent = Some(now);
        }
        (found, progress)
    }
}

/// Passes the reports of the job `id` on to `done`, with paths made locations by `location`
/// and errors in words. It runs in the job's task, so the time it takes a report is the time
/// the job made it. Reports of how far the job is go in batches, at most one each [`BATCH`],
/// with the last of its counting; those that wait go before anything else the job reports,
/// and before its end.
pub(super) async fn forward<P>(
    mut incoming: mpsc::UnboundedReceiver<Event<P>>,
    id: u64,
    done: &mpsc::UnboundedSender<Done>,
    location: impl Fn(P) -> Location,
) {
    let mut waiting = Waiting::new();
    let send = |waiting: &mut Waiting<P>, now| {
        let (found, progress) = waiting.send(now);
        if let Some((at, items)) = found {
            let event = JobEvent::Scanning { at, items };
            let _ = done.send(Done::Job { id, event });
        }
        if let Some((current, reports)) = progress {
            let current = location(current);
            let event = JobEvent::Progress { current, reports };
            let _ = done.send(Done::Job { id, event });
        }
    };
    loop {
        let event = tokio::select! {
            event = incoming.recv() => event,
            () = crate::tui::sleep_until(waiting.due()) => {
                send(&mut waiting, Instant::now());
                continue;
            }
        };
        let at = Instant::now();
        let event = match event {
            None => {
                send(&mut waiting, at);
                return;
            }
            Some(Event::Progress(progress)) => {
                let counts = Counts {
                    done: progress.items_done,
                    total: progress.items_total,
                    bytes_done: progress.bytes_done,
                    bytes_total: progress.bytes_total,
                    bytes_copied: progress.bytes_copied,
                };
                if waiting.take(at, progress.current, counts) {
                    send(&mut waiting, at);
                }
                continue;
            }
            Some(Event::Scanning { items }) => {
                // Counting after some work starts the window's speed and time left over: the
                // work goes first.
                if waiting.progress() {
                    send(&mut waiting, at);
                }
                if waiting.count(at, items) {
                    send(&mut waiting, at);
                }
                continue;
            }
            Some(Event::Failed {
                path,
                step,
                error,
                reply,
            }) => JobEvent::Failed {
                at,
                path: location(path),
                step,
                retry: !matches!(error, VfsError::SameFile(_)),
                reason: describe::vfs_reason(&error),
                reply,
            },
            Some(Event::Exists {
                source,
                target,
                source_metadata,
                target_metadata,
                free,
                reply,
            }) => {
                let taken = TakenName {
                    source: location(source),
                    target: location(target),
                    source_metadata,
                    target_metadata,
                    free,
                    reply,
                };
                JobEvent::Exists {
                    at,
                    taken: Box::new(taken),
                }
            }
        };
        send(&mut waiting, at);
        let _ = done.send(Done::Job { id, event });
    }
}
