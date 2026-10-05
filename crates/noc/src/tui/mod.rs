//! The terminal user interface: two panels over the local file system and SFTP hosts.

mod app;
mod cd;
mod cells;
mod command;
mod complete;
mod configuration;
mod decor;
mod describe;
mod dialog;
mod fuzzy;
mod help;
mod jobs;
mod jump;
mod keymap;
mod menu;
mod mouse;
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
mod workspaces;

use std::io::{self, IsTerminal as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, bail};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyEventKind, MouseEvent,
};
use futures_util::StreamExt as _;
use jiff::tz::TimeZone;
use noc_tools::ToolError;
use noc_tools::editor::Editor;
use noc_tools::shell::Shell;
use ratatui::DefaultTerminal;
use ratatui::backend::{Backend as _, ClearType};
use ratatui::widgets::Clear;
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::mpsc;

use app::App;
use keymap::KeyState;
use mouse::Clicks;
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
        Done::Workspaces { changed, result } => app.workspaces_changed(changed, result),
        Done::Jumps { generation, result } => app.jumps(generation, result),
        Done::Names {
            generation,
            entries,
            hosts,
        } => app.names(generation, entries, &hosts),
        Done::Written { location, result } => tasks.run(app.written(&location, result)),
        Done::Renamed {
            panel,
            from,
            to,
            result,
        } => tasks.run(app.entry_renamed(panel, &from, &to, result)),
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
    let restore = Restore::default();
    let mut terminal = ratatui::try_init()?;
    release_mouse_on_panic();
    let modes = Modes::start(&restore).await?;
    let mut events = EventStream::new();
    let mut spinner = tokio::time::interval(SPINNER_FRAME);
    spinner.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut keys = KeyState::default();
    let mut clicks = Clicks::default();
    // The mouse is captured; `ui.mouse` may change while the app runs.
    let mut mouse = false;
    // The last event changed nothing, so the screen stays as it is.
    let mut idle = false;
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
        if let Some(handover) = Handover::take(&mut app) {
            // The next turn captures the mouse again.
            mouse = false;
            let (fresh, after) =
                hand_over((&mut terminal, modes), events, &mut app, handover).await;
            events = fresh;
            let effects = match after {
                Ok((effects, fresh)) => {
                    interrupt = fresh;
                    effects
                }
                Err(error) => break Err(error.into()),
            };
            keys = KeyState::default();
            tasks.run(effects);
            continue;
        }
        if let Err(error) = take_terminal_requests(&mut terminal, &mut app, &mut mouse) {
            break Err(error.into());
        }
        let now = SystemTime::now();
        if !std::mem::take(&mut idle)
            && let Err(error) = terminal.draw(|frame| app.render(frame, now, &tz))
        {
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
                Some(Ok(Event::Paste(text))) => tasks.run(app.paste(&text)),
                Some(Ok(Event::Mouse(event))) => {
                    idle = !take_mouse(&mut app, &mut tasks, &mut clicks, &mut keys, event);
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
/// it asks for first, the saved workspaces among it.
fn start_app(context: &Context, start: &Path, tz: &TimeZone) -> (App, Vec<app::Effect>) {
    let (mut app, mut effects) = App::new(start, &context.paths.home, &context.config());
    effects.push(app::Effect::Workspaces(app::WorkspaceChange::Load));
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

/// A program that gets the terminal, with the panels hidden.
enum Handover {
    /// The editor of F4, on a local file.
    Edit(PathBuf),
    /// The editor on the command line's text, written to `file`.
    EditCommand { file: PathBuf, text: String },
    /// A shell command of the command line.
    Run(command::Run),
}

impl Handover {
    /// What the app asks to hand the terminal to now, if anything.
    fn take(app: &mut App) -> Option<Self> {
        if let Some(file) = app.take_edit() {
            Some(Self::Edit(file))
        } else if let Some((file, text)) = app.take_command_edit() {
            Some(Self::EditCommand { file, text })
        } else {
            app.take_run().map(Self::Run)
        }
    }
}

/// Runs the editor on `text`, in `file`, which goes afterwards, and returns what it left
/// there, or why the editor could not run.
async fn edit_command(file: &Path, text: &str) -> Result<String, String> {
    let failed = |reason: String| fl!("command-edit-error", reason = reason);
    let mut contents = text.to_owned();
    contents.push('\n');
    tokio::fs::write(file, contents)
        .await
        .map_err(|error| failed(error.to_string()))?;
    let editor = Editor::from_env();
    let edited = match editor.edit(file).await {
        Ok(_) => tokio::fs::read(file)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|error| failed(error.to_string())),
        Err(error) => {
            let reason = match &error {
                ToolError::Spawn { source, .. } => source.to_string(),
                other => describe::chain(other),
            };
            let program = editor.program().display().to_string();
            Err(failed(fl!(
                "edit-cannot-run",
                program = program,
                reason = reason
            )))
        }
    };
    let _ = tokio::fs::remove_file(file).await;
    edited
}

/// Hands the terminal to the program of `handover`, takes it back, and returns what the app
/// does next and a new stream of SIGINT, since Ctrl-C in the program reached Noon Commander
/// too, with a new stream of events in place of `events`.
async fn hand_over(
    (terminal, modes): (&mut DefaultTerminal, Modes),
    events: EventStream,
    app: &mut App,
    handover: Handover,
) -> (EventStream, io::Result<(Vec<app::Effect>, Signal)>) {
    // The program gets every key. A stream's reader holds crossterm's input lock while it
    // waits, and a new stream takes that lock, so the old one goes first; the new one reads
    // nothing until it is polled.
    drop(events);
    let suspended = suspend(terminal, modes).map_err(|error| error.to_string());
    let (events, effects) = match handover {
        Handover::Edit(file) => {
            let result = match suspended {
                Ok(()) => edit(&file).await,
                Err(error) => Err(error),
            };
            (EventStream::new(), app.edited(result))
        }
        Handover::EditCommand { file, text } => {
            let result = match suspended {
                Ok(()) => edit_command(&file, &text).await,
                Err(error) => Err(error),
            };
            app.command_edited(result);
            (EventStream::new(), Vec::new())
        }
        Handover::Run(run) => {
            let result = match suspended {
                Ok(()) => run_command(run).await,
                Err(error) => Err(error),
            };
            let mut events = EventStream::new();
            // What the command printed stays on screen until a key.
            if result.is_ok() {
                wait_for_key(&mut events).await;
            }
            (events, app.ran(result))
        }
    };
    let after = resume(terminal, modes).and_then(|()| signal(SignalKind::interrupt()));
    (events, after.map(|interrupt| (effects, interrupt)))
}

/// Runs the shell command of `run`, with the terminal handed over: locally in `$SHELL`, or on
/// its host through ssh over the host's connection. Then says how it ended if it failed, asks
/// for a key, and puts the terminal in raw mode to read it. Fails if it could not start.
async fn run_command(run: command::Run) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::process::ExitStatusExt as _;

    let command::Run { place, command } = run;
    let status = match place {
        command::Place::Local(dir) => {
            let shell = Shell::from_env();
            shell.run(&dir, &command).await.map_err(|error| {
                let reason = match &error {
                    ToolError::Spawn { source, .. } => source.to_string(),
                    other => describe::chain(other),
                };
                let program = shell.program().display().to_string();
                fl!("command-error", program = program, reason = reason)
            })?
        }
        command::Place::Remote { handle, dir } => {
            let mut ssh = tasks::remote_command(&handle, dir, command).await?;
            let program = ssh.as_std().get_program().to_string_lossy().into_owned();
            ssh.status().await.map_err(|error| {
                let reason = error.to_string();
                fl!("command-error", program = program, reason = reason)
            })?
        }
    };
    let mut out = io::stdout();
    let ending = if let Some(code) = status.code().filter(|&code| code != 0) {
        Some(fl!("command-exit-code", code = code))
    } else {
        status
            .signal()
            .map(|signal| fl!("command-signal", signal = signal))
    };
    // The panels are hidden: this goes where the command printed, as a shell would say it.
    if let Some(ending) = ending {
        let _ = writeln!(out, "{ending}");
    }
    let _ = write!(out, "{}", fl!("command-press-key"));
    let _ = out.flush();
    crossterm::terminal::enable_raw_mode().map_err(|error| error.to_string())
}

/// Waits for a key press; or for the end of the input, or an error, which the event loop meets
/// again.
async fn wait_for_key(events: &mut EventStream) {
    while let Some(Ok(event)) = events.next().await {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            return;
        }
    }
}

/// Hands the terminal over to another program as the shell has it: with the cursor and the
/// mouse, on the main screen, and out of raw mode.
fn suspend(terminal: &mut DefaultTerminal, modes: Modes) -> io::Result<()> {
    modes.leave()?;
    capture_mouse(false)?;
    terminal.show_cursor()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()
}

/// Takes the terminal back after a program had it: raw mode, the alternate screen, and a
/// full redraw; the next draw hides the cursor.
fn resume(terminal: &mut DefaultTerminal, modes: Modes) -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    modes.enter()?;
    repaint(terminal)
}

/// What the TUI asks of the terminal while it has it: bracketed paste, so that pasted text
/// arrives whole and never as keys that run something; and, where the terminal speaks it, the
/// kitty keyboard protocol, which tells Shift-Enter from Enter (ADR 0019).
#[derive(Debug, Clone, Copy, Default)]
struct Modes {
    keyboard: bool,
}

impl Modes {
    /// Asks the terminal whether it speaks the kitty keyboard protocol, through crossterm's
    /// reader, before a stream of events takes it; then puts it in the modes, which `restore`
    /// turns off again.
    async fn start(restore: &Restore) -> Result<Self> {
        let keyboard = tokio::task::spawn_blocking(|| {
            crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
        })
        .await?;
        let modes = Self { keyboard };
        modes.enter()?;
        restore.modes.set(modes);
        Ok(modes)
    }

    fn enter(self) -> io::Result<()> {
        use crossterm::event::{
            EnableBracketedPaste, KeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
        };

        crossterm::execute!(io::stdout(), EnableBracketedPaste)?;
        if self.keyboard {
            let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
            crossterm::execute!(io::stdout(), PushKeyboardEnhancementFlags(flags))?;
        }
        Ok(())
    }

    fn leave(self) -> io::Result<()> {
        use crossterm::event::{DisableBracketedPaste, PopKeyboardEnhancementFlags};

        if self.keyboard {
            crossterm::execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
        }
        crossterm::execute!(io::stdout(), DisableBracketedPaste)
    }
}

/// Does what the app asks of the terminal: a full redraw, text for the clipboard, and
/// capturing the mouse or releasing it, as `ui.mouse` says; `mouse` is whether it is
/// captured.
fn take_terminal_requests(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    mouse: &mut bool,
) -> io::Result<()> {
    if app.take_redraw() {
        repaint(terminal)?;
    }
    if let Some(text) = app.take_clipboard() {
        copy_to_clipboard(terminal, &text)?;
    }
    if app.mouse() != *mouse {
        *mouse = app.mouse();
        capture_mouse(*mouse)?;
    }
    Ok(())
}

/// Hands `event` to the app if it is a press; whether it was. Moves of the mouse come all the
/// time and do nothing. A press ends a key sequence.
fn take_mouse(
    app: &mut App,
    tasks: &mut Tasks,
    clicks: &mut Clicks,
    keys: &mut KeyState,
    event: MouseEvent,
) -> bool {
    let Some(pointer) = clicks.pointer(event, Instant::now()) else {
        return false;
    };
    *keys = KeyState::default();
    tasks.run(app.pointer(pointer));
    true
}

/// Releases the mouse and the modes on a panic: the hook of `ratatui::try_init`, which this
/// one calls, restores the terminal but leaves them on. Both are harmless to turn off when they
/// are not on.
fn release_mouse_on_panic() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = capture_mouse(false);
        let _ = Modes { keyboard: true }.leave();
        hook(info);
    }));
}

/// Has the terminal report the mouse, or stop.
fn capture_mouse(on: bool) -> io::Result<()> {
    if on {
        crossterm::execute!(io::stdout(), EnableMouseCapture)
    } else {
        crossterm::execute!(io::stdout(), DisableMouseCapture)
    }
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

/// Releases the mouse and leaves raw mode and the alternate screen however [`run`] ends.
/// Panics are covered by the hook that `ratatui::try_init` installs, and the one around it.
#[derive(Default)]
struct Restore {
    /// The modes the terminal is in.
    modes: std::cell::Cell<Modes>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = capture_mouse(false);
        let _ = self.modes.get().leave();
        ratatui::restore();
    }
}
