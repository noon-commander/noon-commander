//! The terminal user interface: two panels over the local file system and SFTP hosts.

mod app;
mod cd;
mod cells;
mod complete;
mod configuration;
mod decor;
mod describe;
mod dialog;
mod help;
mod jobs;
mod jump;
mod keymap;
mod menu;
mod panel;
mod pattern;
mod progress;
mod pulldown;
mod root;
mod scrollbar;
mod sums;
mod tabs;
mod tasks;
mod theme;

use std::io::{self, IsTerminal as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, bail};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt as _;
use jiff::tz::TimeZone;
use noc_tools::ToolError;
use noc_tools::editor::Editor;
use ratatui::DefaultTerminal;
use ratatui::backend::{Backend as _, ClearType};
use ratatui::widgets::Clear;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;

use app::App;
use keymap::KeyState;
use tasks::{Done, Tasks};

use crate::context::Context;
use crate::i18n::fl;

pub(crate) use root::read_volumes;

/// Whether `name` is a built-in theme, for `ui.theme`; and the names there are.
pub(crate) fn is_valid_theme(name: &str) -> bool {
    theme::Theme::by_name(name, theme::ColorDepth::TrueColor).is_some()
}

pub(crate) fn theme_names() -> &'static [&'static str] {
    theme::Theme::NAMES
}

/// Hands finished background work to the app, and runs what the app asks for next.
fn take_done(app: &mut App, tasks: &mut Tasks, done: Done) {
    match done {
        Done::Listed {
            panel,
            generation,
            result,
        } => app.listed(panel, generation, result),
        Done::Places { generation, result } => app.places(generation, result),
        Done::Created {
            panel,
            location,
            result,
        } => tasks.run(app.created(panel, &location, result)),
        Done::Job { id, event } => tasks.run(app.job_event(id, event)),
        Done::Read { id, result } => app.read(id, result),
        Done::Connected {
            host,
            connection,
            handle,
        } => tasks.run(app.connected(&host, connection, handle)),
        Done::Resolved { host, address } => app.resolved(host, address),
        Done::Ask(ask) => app.ask(ask),
        Done::Notice {
            id,
            context,
            message,
        } => app.notice(id, &context, &message),
        Done::PromptClosed { id } => app.prompt_closed(id),
        Done::Closed {
            host,
            connection,
            reason,
        } => tasks.run(app.closed(&host, connection, reason.as_deref())),
        Done::HostSaved(result) => tasks.run(app.host_saved(result)),
        Done::ConfigSaved(result) => app.config_saved(result),
        Done::Jumps { generation, result } => app.jumps(generation, result),
        Done::Names {
            generation,
            entries,
            hosts,
        } => app.names(generation, entries, &hosts),
        Done::Written { location, result } => tasks.run(app.written(&location, result)),
    }
}

/// How long a spinner shows each of its frames.
const SPINNER_FRAME: Duration = Duration::from_millis(150);

/// Runs the TUI with both panels on the local directory `start` until the user quits or the
/// process gets SIGTERM, SIGHUP, or SIGINT.
pub(crate) async fn run(context: Context, start: PathBuf) -> Result<()> {
    if !io::stdout().is_terminal() {
        bail!("the TUI needs a terminal; in scripts, use the subcommands (`noc --help`)");
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
    let (mut app, effects) = start_app(&context, &start, &tz);
    tasks.run(effects);
    let result = loop {
        if app.quits() {
            break Ok(());
        }
        if let Some(dir) = app.take_work_dir() {
            tasks.change_work_dir(dir);
        }
        if let Some(file) = app.take_edit() {
            // The editor gets every key. A stream's reader holds crossterm's input lock while it
            // waits, and a new stream takes that lock, so the old one goes first; the new one
            // reads nothing until it is polled.
            drop(events);
            let result = match suspend(&mut terminal) {
                Ok(()) => edit(&file).await,
                Err(error) => Err(error.to_string()),
            };
            events = EventStream::new();
            if let Err(error) = resume(&mut terminal) {
                break Err(error.into());
            }
            // Ctrl-C in the editor reached Noon Commander too; forget it.
            match signal(SignalKind::interrupt()) {
                Ok(fresh) => interrupt = fresh,
                Err(error) => break Err(error.into()),
            }
            keys = KeyState::default();
            tasks.run(app.edited(result));
            continue;
        }
        if app.take_redraw()
            && let Err(error) = repaint(&mut terminal)
        {
            break Err(error.into());
        }
        if let Some(text) = app.take_clipboard()
            && let Err(error) = copy_to_clipboard(&mut terminal, &text)
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
            Some(job) = done.recv() => {
                // Jobs report every entry; take all that waits, then draw once.
                take_done(&mut app, &mut tasks, job);
                while let Ok(job) = done.try_recv() {
                    take_done(&mut app, &mut tasks, job);
                }
            }
            _ = spinner.tick(), if app.animates() => app.tick(),
            _ = terminate.recv() => break Ok(()),
            _ = hangup.recv() => break Ok(()),
            _ = interrupt.recv() => break Ok(()),
        }
    };
    // The terminal comes back first, so a slow shutdown does not look like a hang.
    drop(terminal);
    drop(restore);
    app.stop_jobs();
    tasks.finish_jobs().await;
    app.disconnect_all();
    tasks.shutdown().await;
    result
}

/// The app with both panels on `start`, set up for this terminal and machine, and the work
/// it asks for first.
fn start_app(context: &Context, start: &Path, tz: &TimeZone) -> (App, Vec<app::Effect>) {
    let (mut app, effects) = App::new(start, &context.paths.home, &context.config());
    app.set_color_depth(theme::ColorDepth::detect());
    app.set_time_zone(tz.clone());
    if let Some(name) = host_name() {
        app.set_root_title(name);
    }
    app.set_runtime_dir(context.paths.runtime_dir.clone());
    app.set_hosts(context.hosts());
    (app, effects)
}

/// The name of this machine without its domain, for the title of the virtual root.
fn host_name() -> Option<String> {
    let uname = rustix::system::uname();
    let name = uname.nodename().to_string_lossy();
    let name = name.split('.').next().unwrap_or_default();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Runs the editor on `file`, with the terminal handed over, and tells whether the file changed
/// (its size or time), or why the editor could not run.
async fn edit(file: &Path) -> Result<bool, String> {
    let stamp = |metadata: std::fs::Metadata| (metadata.len(), metadata.modified().ok());
    let before = tokio::fs::metadata(file)
        .await
        .map(stamp)
        .map_err(|error| error.to_string())?;
    let editor = Editor::from_env();
    if let Err(error) = editor.edit(file).await {
        let reason = match &error {
            ToolError::Spawn { source, .. } => source.to_string(),
            other => describe::chain(other),
        };
        return Err(fl!(
            "edit-cannot-run",
            program = editor.program().display().to_string(),
            reason = reason
        ));
    }
    let after = tokio::fs::metadata(file)
        .await
        .map(stamp)
        .map_err(|error| error.to_string())?;
    Ok(before != after)
}

/// Hands the terminal over to another program as the shell has it: with the cursor, on the
/// main screen, and out of raw mode.
fn suspend(terminal: &mut DefaultTerminal) -> io::Result<()> {
    terminal.show_cursor()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()
}

/// Takes the terminal back after a program had it: raw mode, the alternate screen, and a
/// full redraw; the next draw hides the cursor.
fn resume(terminal: &mut DefaultTerminal) -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    repaint(terminal)
}

/// Makes the next draw write every cell. Unlike `Terminal::clear`, this does not ask the
/// terminal for the cursor position, which fails where no answer comes.
fn repaint(terminal: &mut DefaultTerminal) -> io::Result<()> {
    terminal.backend_mut().clear_region(ClearType::All)?;
    terminal.draw(|frame| frame.render_widget(Clear, frame.area()))?;
    Ok(())
}

/// Asks the terminal to put `text` on the system clipboard with OSC 52 (ADR 0008). Nothing
/// tells whether it did: terminals that do not know the sequence, or do not allow it, ignore
/// it.
fn copy_to_clipboard(terminal: &mut DefaultTerminal, text: &str) -> io::Result<()> {
    use crossterm::clipboard::CopyToClipboard;

    crossterm::execute!(
        terminal.backend_mut(),
        CopyToClipboard::to_clipboard_from(text)
    )
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
