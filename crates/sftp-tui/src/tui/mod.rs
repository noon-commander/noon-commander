//! The terminal user interface: two panels over the local file system and SFTP hosts.

mod app;
mod cells;
mod decor;
mod describe;
mod dialog;
mod help;
mod keymap;
mod panel;
mod pattern;
mod root;
mod tasks;
mod theme;

use std::io::{self, IsTerminal as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, bail};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt as _;
use jiff::tz::TimeZone;
use ratatui::DefaultTerminal;
use ratatui::backend::{Backend as _, ClearType};
use ratatui::widgets::Clear;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;

use app::App;
use keymap::KeyState;
use tasks::{Done, Tasks};

use crate::context::Context;

/// Whether `name` is a built-in theme, for `ui.theme`; and the names there are.
pub(crate) fn is_valid_theme(name: &str) -> bool {
    theme::Theme::by_name(name).is_some()
}

pub(crate) fn theme_names() -> &'static [&'static str] {
    theme::Theme::NAMES
}

/// How long a spinner shows each of its frames.
const SPINNER_FRAME: Duration = Duration::from_millis(150);

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
    let restore = Restore;
    let mut terminal = ratatui::try_init()?;
    let mut events = EventStream::new();
    let mut spinner = tokio::time::interval(SPINNER_FRAME);
    spinner.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut keys = KeyState::default();
    let context = Arc::new(context);
    let (done_tx, mut done) = mpsc::unbounded_channel();
    let mut tasks = Tasks::start(Arc::clone(&context), done_tx).await;
    let (mut app, effects) = App::new(&start, &context.paths.home, &context.config.ui);
    tasks.run(effects);
    let result = loop {
        if app.quits() {
            break Ok(());
        }
        if app.take_redraw()
            && let Err(error) = repaint(&mut terminal)
        {
            break Err(error.into());
        }
        let now = SystemTime::now();
        if let Err(error) = terminal.draw(|frame| app.render(frame, now, &tz)) {
            break Err(error.into());
        }
        let deadline = keys.deadline();
        tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    let context = app.context();
                    for input in app.keymap().feed(&mut keys, context, key, Instant::now()) {
                        tasks.run(app.handle(input));
                    }
                }
                // Resizes and other events only need a redraw.
                Some(Ok(_)) => {}
                Some(Err(error)) => break Err(error.into()),
                None => break Ok(()),
            },
            () = sleep_until(deadline) => {
                for input in app.keymap().expire(&mut keys, Instant::now()) {
                    tasks.run(app.handle(input));
                }
            }
            Some(job) = done.recv() => match job {
                Done::Listed { side, generation, result } => app.listed(side, generation, result),
                Done::Connected { host, connection, handle } => {
                    tasks.run(app.connected(&host, connection, handle));
                }
                Done::Resolved { host, address } => app.resolved(host, address),
                Done::Ask(ask) => app.ask(ask),
                Done::Notice { id, context, message } => app.notice(id, &context, &message),
                Done::PromptClosed { id } => app.prompt_closed(id),
                Done::Closed { host, connection, reason } => {
                    tasks.run(app.closed(&host, connection, reason.as_deref()));
                }
            },
            _ = spinner.tick(), if app.animates() => app.tick(),
            _ = terminate.recv() => break Ok(()),
            _ = hangup.recv() => break Ok(()),
            _ = interrupt.recv() => break Ok(()),
        }
    };
    // The terminal comes back first, so a slow shutdown does not look like a hang.
    drop(terminal);
    drop(restore);
    app.disconnect_all();
    tasks.shutdown().await;
    result
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
