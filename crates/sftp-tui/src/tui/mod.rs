//! The terminal user interface: two panels over the local file system and SFTP hosts.

mod app;
mod cells;
mod keymap;
mod panel;
mod root;

use std::io::{self, IsTerminal as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use color_eyre::eyre::{Result, bail};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt as _;
use jiff::tz::TimeZone;
use ratatui::DefaultTerminal;
use ratatui::backend::{Backend as _, ClearType};
use ratatui::widgets::Clear;
use sftp_tui_vfs::{LocalFs, Location, Vfs as _, VfsError};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;

use app::{App, Effect, Side};
use keymap::{KeyState, Keymap};
use panel::Listing;

use crate::context::Context;

/// A finished background job.
enum Done {
    Listed {
        side: Side,
        generation: u64,
        result: Result<Listing, VfsError>,
    },
}

/// Runs the TUI with both panels on the local directory `start` until the user quits or the
/// process gets SIGTERM, SIGHUP, or SIGINT.
pub(crate) async fn run(context: Context, start: PathBuf) -> Result<()> {
    if !io::stdout().is_terminal() {
        bail!("the TUI needs a terminal; in scripts, use the subcommands (`sftp-tui --help`)");
    }
    // Raw mode turns Ctrl-C into a key, but these can still come from elsewhere, such as `kill`
    // or a closed terminal window; quitting through them restores the terminal.
    let mut terminate = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    // Reads /etc/localtime.
    let tz = tokio::task::spawn_blocking(TimeZone::system).await?;
    let _restore = Restore;
    let mut terminal = ratatui::try_init()?;
    let mut events = EventStream::new();
    let keymap = Keymap::mc();
    let mut keys = KeyState::default();
    let context = Arc::new(context);
    let (done_tx, mut done) = mpsc::unbounded_channel();
    let (mut app, effects) = App::new(&start, &context.paths.home);
    run_effects(effects, &context, &done_tx);
    while !app.quits() {
        if app.take_redraw() {
            repaint(&mut terminal)?;
        }
        let now = SystemTime::now();
        terminal.draw(|frame| app.render(frame, &keymap, now, &tz))?;
        let deadline = keys.deadline();
        tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    for input in keymap.feed(&mut keys, app.context(), key, Instant::now()) {
                        run_effects(app.handle(input), &context, &done_tx);
                    }
                }
                // Resizes and other events only need a redraw.
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
                None => break,
            },
            () = sleep_until(deadline) => {
                for input in keymap.expire(&mut keys, Instant::now()) {
                    run_effects(app.handle(input), &context, &done_tx);
                }
            }
            Some(job) = done.recv() => match job {
                Done::Listed { side, generation, result } => app.listed(side, generation, result),
            },
            _ = terminate.recv() => break,
            _ = hangup.recv() => break,
            _ = interrupt.recv() => break,
        }
    }
    Ok(())
}

/// Starts the work of `effects` in the background; results come back through `done`.
fn run_effects(effects: Vec<Effect>, context: &Arc<Context>, done: &mpsc::UnboundedSender<Done>) {
    for effect in effects {
        match effect {
            Effect::List { side, request } => {
                let context = Arc::clone(context);
                let done = done.clone();
                tokio::spawn(async move {
                    let result = list(context, request.location).await;
                    let _ = done.send(Done::Listed {
                        side,
                        generation: request.generation,
                        result,
                    });
                });
            }
        }
    }
}

/// Lists a location. The virtual root reads the ssh config each time, so a reload shows new
/// hosts.
async fn list(context: Arc<Context>, location: Location) -> Result<Listing, VfsError> {
    match location {
        Location::Root => tokio::task::spawn_blocking(move || root::read_hosts(&context))
            .await
            .map(Listing::Root)
            .map_err(|error| VfsError::Io(io::Error::other(error))),
        Location::Local(path) => LocalFs.list_dir(&path).await.map(Listing::Dir),
        Location::Remote { .. } => Err(VfsError::Io(io::ErrorKind::Unsupported.into())),
    }
}

/// Makes the next draw write every cell. Unlike `Terminal::clear`, this does not ask the
/// terminal for the cursor position, which fails where no answer comes.
fn repaint(terminal: &mut DefaultTerminal) -> io::Result<()> {
    terminal.backend_mut().clear_region(ClearType::All)?;
    terminal.draw(|frame| frame.render_widget(Clear, frame.area()))?;
    Ok(())
}

/// Completes at `deadline`; never without one.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}

/// Leaves raw mode and the alternate screen however [`run`] ends. Panics are covered by the
/// hook that `ratatui::try_init` installs.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
