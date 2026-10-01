//! The terminal user interface: two panels over the local file system and SFTP hosts.

mod app;
mod keymap;

use std::io::{self, IsTerminal as _};

use color_eyre::eyre::{Result, bail};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt as _;
use tokio::signal::unix::{SignalKind, signal};

use app::App;

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
    let mut app = App::default();
    while !app.quits() {
        terminal.draw(|frame| app.render(frame))?;
        tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    if let Some(action) = keymap::action(key) {
                        app.handle(action);
                    }
                }
                // Resizes and other events only need a redraw.
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
                None => break,
            },
            _ = terminate.recv() => break,
            _ = hangup.recv() => break,
            _ = interrupt.recv() => break,
        }
    }
    Ok(())
}

/// Leaves raw mode and the alternate screen however [`run`] ends. Panics are covered by the
/// hook that `ratatui::try_init` installs.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
