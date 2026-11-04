//! What jobs share: their reports, the decisions they wait for, and their outcome.

use std::time::Duration;

use noc_vfs::{Code, Metadata, VfsError};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// How long a cancelled job may take to end: long enough to let go of what it was writing,
/// short of waiting for ever on a host that stopped answering.
const GRACE: Duration = Duration::from_secs(2);

/// Runs `work`, a job that stops once `cancel` is cancelled, to its end; or from then on for
/// [`GRACE`] more at most, `None` past it. The job is dropped then, which abandons what it
/// waited for: work on a host is cancel-safe, and the requests it sent go on without it.
pub async fn in_time<T>(cancel: &CancellationToken, work: impl Future<Output = T>) -> Option<T> {
    in_time_of(cancel, GRACE, work).await
}

async fn in_time_of<T>(
    cancel: &CancellationToken,
    grace: Duration,
    work: impl Future<Output = T>,
) -> Option<T> {
    let mut work = std::pin::pin!(work);
    tokio::select! {
        done = &mut work => return Some(done),
        () = cancel.cancelled() => {}
    }
    tokio::time::timeout(grace, work).await.ok()
}

/// What to do about an operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Try it again.
    Retry,
    /// Leave it and go on.
    Skip,
    /// Stop the job.
    Abort,
}

/// The answer to a failure: what to do about this one, and about the failures after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    pub this: Decision,
    pub rest: Later,
}

impl Resolution {
    /// `this` for this failure; the next one asks again.
    pub fn once(this: Decision) -> Self {
        Self {
            this,
            rest: Later::Ask,
        }
    }
}

/// What to do about the failures that come later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Later {
    /// Ask about each.
    Ask,
    /// Leave every one, without asking.
    SkipAll,
    /// Leave each with the [`Code`] of this failure, without asking, and ask about the others.
    /// A failure without a code leaves no others so.
    SkipSame,
}

/// What a job was doing to a path when an operation on it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Reading what the entry is: its kind, size, and times.
    Inspect,
    /// Listing the directory.
    List,
    /// Reading the file, or where the link points.
    Read,
    /// Making the directory, the file, or the link.
    Create,
    /// Writing the data of the file.
    Write,
    /// Setting its permissions or times.
    SetAttributes,
    /// Renaming or moving it within its file system.
    Rename,
    /// Removing it.
    Remove,
}

/// What to do about a target that is there already: about this one, and about the targets that
/// are there that come after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub this: Choice,
    pub rest: Rest,
}

impl Conflict {
    /// `this` for this target; the next one taken asks again.
    pub fn once(this: Choice) -> Self {
        Self {
            this,
            rest: Rest::Ask,
        }
    }
}

/// What to do about one target that is there already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Replace it.
    Overwrite,
    /// Leave it, and do not copy the source.
    Skip,
    /// Copy the source to this path instead, whole, on the target's file system; if that is
    /// taken too, the job asks again.
    Rename(Vec<u8>),
    /// Stop the job.
    Abort,
}

/// What to do about the targets that are there that come later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rest {
    /// Ask about each.
    Ask,
    /// What [`Choice`] did to this one: replace each, leave each, or give each a free name, in
    /// the directory the renamed one went to. After [`Choice::Abort`] nothing comes.
    Same,
    /// Replace each that is older than its source; leave the others. One without a time stays.
    OverwriteOlder,
    /// Replace each that is smaller than its source; leave the others.
    OverwriteSmaller,
    /// Replace each that is larger than its source; leave the others.
    OverwriteLarger,
    /// Replace each whose size is not its source's; leave the others.
    OverwriteSizeDiffers,
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
    /// The sizes of the files the job found as it counted, each taking what the file holds once
    /// it is read, so that `bytes_done` comes to it and never goes past it.
    pub bytes_total: u64,
    /// Bytes actually read and written so far, retried ones included and skipped ones not: it
    /// only grows, so speeds follow it rather than `bytes_done`. Like `bytes_done`, it counts a
    /// byte written once it has arrived, which on a host is once the server has taken it.
    pub bytes_copied: u64,
}

/// A report from a running job.
#[derive(Debug)]
pub enum Event<P> {
    /// The job counts what it has to do; `items` found so far.
    Scanning {
        items: u64,
    },
    Progress(Progress<P>),
    /// An operation on `path` failed at `step`; the job waits for an answer on `reply`.
    /// Dropping `reply` aborts the job.
    Failed {
        path: P,
        step: Step,
        error: VfsError,
        reply: oneshot::Sender<Resolution>,
    },
    /// The name of a copy is taken; the job waits for an answer on `reply`. Dropping `reply`
    /// aborts the job.
    Exists {
        source: P,
        target: P,
        /// What the source and the target are now, for their sizes and times.
        source_metadata: Metadata,
        target_metadata: Metadata,
        /// A name that is free in the target's directory, for [`Choice::Rename`] to offer.
        free: Vec<u8>,
        reply: oneshot::Sender<Conflict>,
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
/// stops the job between operations and while it waits for a decision or for data.
#[derive(Debug)]
pub struct Reporter<P> {
    events: mpsc::UnboundedSender<Event<P>>,
    cancel: CancellationToken,
    skip_all: bool,
    /// The codes of the failures to leave without asking.
    skip_codes: Vec<Code>,
}

impl<P> Reporter<P> {
    pub fn new(events: mpsc::UnboundedSender<Event<P>>, cancel: CancellationToken) -> Self {
        Self {
            events,
            cancel,
            skip_all: false,
            skip_codes: Vec::new(),
        }
    }

    /// Whether the job should stop: it was cancelled, or no one listens any more.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.is_cancelled() || self.events.is_closed()
    }

    pub(crate) fn report(&self, event: Event<P>) {
        let _ = self.events.send(event);
    }

    /// Waits for `work` unless the job is cancelled first, `None` then: data on its way over a
    /// slow link, or a host that stopped answering, would keep it waiting. What it waited for
    /// is abandoned, as work on a host is cancel-safe.
    pub(crate) async fn unless_cancelled<T>(&self, work: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => None,
            done = work => Some(done),
        }
    }

    /// Sends the question `event` makes of a reply, and waits for the answer; `None` if the job
    /// is cancelled or the answer never comes.
    pub(crate) async fn question<T>(
        &self,
        event: impl FnOnce(oneshot::Sender<T>) -> Event<P>,
    ) -> Option<T> {
        let (reply, answer) = oneshot::channel();
        self.events.send(event(reply)).ok()?;
        tokio::select! {
            answer = answer => answer.ok(),
            () = self.cancel.cancelled() => None,
        }
    }

    /// Asks what to do about `error` on `path` at `step`, unless an earlier answer said to skip
    /// it: after [`Later::SkipAll`] every failure, after [`Later::SkipSame`] each with that
    /// code. Cancellation, and a reply that never comes, abort.
    pub(crate) async fn ask(&mut self, path: P, step: Step, error: VfsError) -> Decision {
        let code = error.code();
        if self.skip_all || code.is_some_and(|code| self.skip_codes.contains(&code)) {
            return Decision::Skip;
        }
        let answer = self
            .question(|reply| Event::Failed {
                path,
                step,
                error,
                reply,
            })
            .await
            .unwrap_or(Resolution::once(Decision::Abort));
        match (answer.rest, code) {
            (Later::SkipAll, _) => self.skip_all = true,
            (Later::SkipSame, Some(code)) => self.skip_codes.push(code),
            (Later::Ask | Later::SkipSame, _) => {}
        }
        answer.this
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use super::*;

    const SHORT: Duration = Duration::from_millis(50);

    async fn after<T>(time: Duration, value: T) -> T {
        tokio::time::sleep(time).await;
        value
    }

    #[tokio::test]
    async fn a_cancelled_job_has_a_while_to_end_and_no_more() {
        let cancel = CancellationToken::new();
        // Not cancelled, it runs to its end, however long after the while that takes.
        assert_eq!(
            in_time_of(&cancel, SHORT, after(SHORT * 3, 1)).await,
            Some(1)
        );
        cancel.cancel();
        // Cancelled, it ends within the while, or is let go.
        assert_eq!(
            in_time_of(&cancel, SHORT, after(SHORT / 5, 2)).await,
            Some(2)
        );
        let stuck = in_time_of(&cancel, SHORT, pending::<u8>());
        let ended = tokio::time::timeout(SHORT * 20, stuck).await;
        assert_eq!(ended, Ok(None), "let go");
    }

    #[tokio::test]
    async fn a_cancel_stops_a_wait() {
        let (events, _incoming) = mpsc::unbounded_channel::<Event<String>>();
        let cancel = CancellationToken::new();
        let reporter = Reporter::new(events, cancel.clone());
        assert_eq!(reporter.unless_cancelled(after(SHORT, 1)).await, Some(1));
        let stopping = async {
            tokio::time::sleep(SHORT).await;
            cancel.cancel();
        };
        let (waited, ()) = tokio::join!(reporter.unless_cancelled(pending::<u8>()), stopping);
        assert_eq!(waited, None);
        // Once cancelled, what is ready already counts for nothing either.
        assert_eq!(reporter.unless_cancelled(async { 3 }).await, None);
    }

    #[tokio::test]
    async fn the_rest_of_the_failures_get_what_an_answer_said() {
        let (events, mut incoming) = mpsc::unbounded_channel::<Event<u8>>();
        let mut reporter = Reporter::new(events, CancellationToken::new());
        let denied = || VfsError::PermissionDenied("x".to_owned(), Some(Code::Os(13)));
        let full = || VfsError::Io(std::io::Error::from_raw_os_error(28));
        let uncoded = || VfsError::SameFile("x".to_owned());
        let answers = [
            Resolution {
                this: Decision::Skip,
                rest: Later::SkipSame,
            },
            Resolution::once(Decision::Retry),
            Resolution {
                this: Decision::Skip,
                rest: Later::SkipSame,
            },
            Resolution {
                this: Decision::Skip,
                rest: Later::SkipAll,
            },
        ];
        let listen = async {
            let mut asked = Vec::new();
            for answer in answers {
                let Some(Event::Failed { path, reply, .. }) = incoming.recv().await else {
                    panic!("expected a failure");
                };
                asked.push(path);
                reply.send(answer).unwrap();
            }
            asked
        };
        let job = async {
            let mut decisions = Vec::new();
            let failures = [
                (1, denied()),
                (2, denied()),
                (3, full()),
                (4, denied()),
                (5, uncoded()),
                (6, uncoded()),
                (7, full()),
            ];
            for (path, error) in failures {
                decisions.push(reporter.ask(path, Step::Read, error).await);
            }
            decisions
        };
        let (asked, decisions) = tokio::join!(listen, job);
        // The second denial is skipped unasked; a failure without a code skips no others.
        assert_eq!(asked, [1, 3, 5, 6]);
        let (retry, skip) = (Decision::Retry, Decision::Skip);
        assert_eq!(decisions, [skip, skip, retry, skip, skip, skip, skip]);
    }
}
