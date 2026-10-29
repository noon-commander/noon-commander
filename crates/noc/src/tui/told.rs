//! What ends Noon Commander other than the user: SIGTERM, SIGHUP, and the terminal going away,
//! which need not come with SIGHUP. The kernel sends SIGHUP to the leader of the terminal's
//! session alone, and the leader passes it on, unless it ignores it, as `nohup`, `trap '' HUP`,
//! or `sudo` may. Without the end of the terminal heard, crossterm's reader spins at its end of
//! input, holding the lock of the input that a new stream of events waits for, and nothing
//! ends Noon Commander (crossterm-rs/crossterm#793).

use std::io::{self, IsTerminal as _};
use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd};

use crossterm::event::EventStream;
use futures_util::FutureExt as _;
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};
use noc_tools::job::Ending;
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::watch;

/// Listens for what ends Noon Commander, both in the event loop and while the terminal is handed
/// over, where the waits for a key end with it, and a program that runs hears it.
pub(super) struct Told {
    hangup: Option<Signal>,
    terminate: Option<Signal>,
    /// Says when the terminal went away, if stdin is the terminal.
    terminal: Option<watch::Receiver<bool>>,
    /// What came first.
    came: Option<Ending>,
}

impl Told {
    /// Listens from now on.
    pub(super) fn listen() -> Self {
        let stdin = io::stdin();
        let terminal = stdin
            .is_terminal()
            .then(|| stdin.as_fd().try_clone_to_owned());
        Self {
            hangup: signal(SignalKind::hangup()).ok(),
            terminate: signal(SignalKind::terminate()).ok(),
            terminal: terminal.and_then(Result::ok).and_then(watch_end),
            came: None,
        }
    }

    /// What came, once one of them came; never, if none can be heard. A terminal that went away
    /// is a hangup, as the kernel tells the leader of its session.
    pub(super) async fn came(&mut self) -> Ending {
        let Self {
            hangup,
            terminate,
            terminal,
            came,
        } = self;
        if let Some(ending) = came {
            return *ending;
        }
        let ending = tokio::select! {
            () = heard(hangup) => Ending::Hangup,
            () = heard(terminate) => Ending::Terminate,
            () = gone(terminal) => Ending::Hangup,
        };
        *came = Some(ending);
        ending
    }

    /// Whether one of them came.
    pub(super) fn has_come(&mut self) -> bool {
        self.came().now_or_never().is_some()
    }

    /// A new stream of events, unless one of them came: a terminal that went away leaves the
    /// reader of the last stream spinning at the end of its input with the lock that a new one
    /// would wait for, forever.
    pub(super) fn events(&mut self) -> Option<EventStream> {
        (!self.has_come()).then(EventStream::new)
    }
}

/// Watches `terminal` for its end in a thread of its own: epoll and kqueue tell when its input is
/// closed, as when its other end closed, which poll(2) on macOS does not; tokio's `AsyncFd`, which
/// would need no thread, takes `unsafe`. A key wakes the thread once, and stays unread. `None` if
/// it cannot be watched.
fn watch_end(terminal: OwnedFd) -> Option<watch::Receiver<bool>> {
    let mut poll = Poll::new().ok()?;
    let fd = terminal.as_raw_fd();
    poll.registry()
        .register(&mut SourceFd(&fd), Token(0), Interest::READABLE)
        .ok()?;
    let (gone, receiver) = watch::channel(false);
    let thread = std::thread::Builder::new().name("terminal".to_owned());
    thread
        .spawn(move || {
            // Open while it is watched.
            let _terminal = terminal;
            let mut events = Events::with_capacity(4);
            // Until no one listens.
            while !gone.is_closed() {
                match poll.poll(&mut events, None) {
                    Ok(()) if events.iter().any(mio::event::Event::is_read_closed) => {
                        let _ = gone.send(true);
                        return;
                    }
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => return,
                }
            }
        })
        .ok()?;
    Some(receiver)
}

async fn heard(signal: &mut Option<Signal>) {
    if let Some(signal) = signal
        && signal.recv().await.is_some()
    {
        return;
    }
    std::future::pending::<()>().await;
}

/// Ends once the terminal went away; never, if that cannot be told.
async fn gone(terminal: &mut Option<watch::Receiver<bool>>) {
    if let Some(terminal) = terminal
        && terminal.wait_for(|gone| *gone).await.is_ok()
    {
        return;
    }
    std::future::pending::<()>().await;
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn a_hangup_ends_the_wait() {
        // Without the terminal of the tests, if they have one.
        let mut told = Told {
            hangup: signal(SignalKind::hangup()).ok(),
            terminate: None,
            terminal: None,
            came: None,
        };
        let pid = rustix::process::getpid();
        rustix::process::kill_process(pid, rustix::process::Signal::HUP).unwrap();
        let came = tokio::time::timeout(Duration::from_secs(5), told.came()).await;
        assert_eq!(came, Ok(Ending::Hangup));
        assert!(told.has_come());
        assert!(told.events().is_none());
    }

    #[tokio::test]
    async fn a_terminal_that_goes_away_ends_the_wait_and_its_keys_do_not() {
        let (mut other_end, ours) = UnixStream::pair().unwrap();
        // Without signals, which the other test sends to the whole process.
        let mut told = Told {
            hangup: None,
            terminate: None,
            terminal: watch_end(OwnedFd::from(ours)),
            came: None,
        };
        other_end.write_all(b"x").unwrap();
        let keys = tokio::time::timeout(Duration::from_millis(200), told.came()).await;
        assert!(keys.is_err(), "a key ended it");
        assert!(!told.has_come());
        drop(other_end);
        let came = tokio::time::timeout(Duration::from_secs(5), told.came()).await;
        assert_eq!(came, Ok(Ending::Hangup));
        assert!(told.has_come());
    }
}
