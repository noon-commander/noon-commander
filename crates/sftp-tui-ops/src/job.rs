//! What jobs share: their reports, the decisions they wait for, and their outcome.

use sftp_tui_vfs::VfsError;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// What to do about an operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Try it again.
    Retry,
    /// Leave it and go on.
    Skip,
    /// Leave it and every later one that fails, without asking again.
    SkipAll,
    /// Stop the job.
    Abort,
}

/// How far a job is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress<P> {
    /// What the job works on now.
    pub current: P,
    /// Entries done or skipped so far.
    pub items_done: u64,
    /// Entries the job found to work on.
    pub items_total: u64,
    /// Bytes of files done or skipped so far; zero for jobs that do not move data.
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// A report from a running job.
#[derive(Debug)]
pub enum Event<P> {
    /// The job counts what it has to do; `items` found so far.
    Scanning {
        items: u64,
    },
    Progress(Progress<P>),
    /// An operation on `path` failed; the job waits for an answer on `reply`. Dropping `reply`
    /// aborts the job.
    Failed {
        path: P,
        error: VfsError,
        reply: oneshot::Sender<Decision>,
    },
}

/// How a job ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Entries done.
    pub done: u64,
    /// Entries left as they were: skipped, or kept because something in them was.
    pub skipped: u64,
    /// Stopped before the end, by [`Decision::Abort`] or cancellation.
    pub aborted: bool,
}

/// A job's line to whoever runs it: reports go out, decisions come back, and cancellation
/// stops the job between operations and while it waits for a decision.
#[derive(Debug)]
pub struct Reporter<P> {
    events: mpsc::UnboundedSender<Event<P>>,
    cancel: CancellationToken,
    skip_all: bool,
}

impl<P> Reporter<P> {
    pub fn new(events: mpsc::UnboundedSender<Event<P>>, cancel: CancellationToken) -> Self {
        Self {
            events,
            cancel,
            skip_all: false,
        }
    }

    /// Whether the job should stop: it was cancelled, or no one listens any more.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.is_cancelled() || self.events.is_closed()
    }

    pub(crate) fn report(&self, event: Event<P>) {
        let _ = self.events.send(event);
    }

    /// Asks what to do about `error` on `path`. After [`Decision::SkipAll`] every failure is
    /// skipped without asking; cancellation, and a reply that never comes, abort.
    pub(crate) async fn ask(&mut self, path: P, error: VfsError) -> Decision {
        if self.skip_all {
            return Decision::Skip;
        }
        let (reply, answer) = oneshot::channel();
        if self
            .events
            .send(Event::Failed { path, error, reply })
            .is_err()
        {
            return Decision::Abort;
        }
        let decision = tokio::select! {
            answer = answer => answer.unwrap_or(Decision::Abort),
            () = self.cancel.cancelled() => Decision::Abort,
        };
        match decision {
            Decision::SkipAll => {
                self.skip_all = true;
                Decision::Skip
            }
            other => other,
        }
    }
}
