//! The terminal user interface: two panels over the local file system and SFTP hosts.

mod app;
mod keymap;

use std::io::{self, IsTerminal as _};
use std::time::Instant;

use color_eyre::eyre::{Result, bail};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt as _;
use ratatui::DefaultTerminal;
use ratatui::backend::{Backend as _, ClearType};
use ratatui::widgets::Clear;
use tokio::signal::unix::{SignalKind, signal};

use app::App;
use keymap::{KeyState, Keymap};

/// Runs the TUI until the user quits or the process gets SIGTERM, SIGHUP, or SIGINT.
pub(crate) async fn run() -> Result<()> {
    if !io::stdout().is_terminal() {
        bail!("the TUI needs a terminal; in scripts, use the subcommands (`sftp-tui --help`)");
    }
    // Raw mode turns Ctrl-C into a key, but these can still come from elsewhere, such as `kill`
    // or a closed terminal window; quitting through them restores the terminal.
    let mut terminate = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let _restore = Restore;
    let mut terminal = ratatui::try_init()?;
    let mut events = EventStream::new();
    let keymap = Keymap::mc();
    let mut keys = KeyState::default();
    let mut app = App::default();
    while !app.quits() {
        if app.take_redraw() {
            repaint(&mut terminal)?;
        }
        terminal.draw(|frame| app.render(frame, &keymap))?;
        let deadline = keys.deadline();
        let resolved = tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    keymap.feed(&mut keys, app.context(), key, Instant::now())
                }
                // Resizes and other events only need a redraw.
                Some(Ok(_)) => Vec::new(),
                Some(Err(error)) => return Err(error.into()),
                None => break,
            },
            () = sleep_until(deadline) => keymap.expire(&mut keys, Instant::now()),
            _ = terminate.recv() => break,
            _ = hangup.recv() => break,
            _ = interrupt.recv() => break,
        };
        for input in resolved {
            app.handle(input);
        }
    }
    Ok(())
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
