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
mod history;
mod jobs;
mod jump;
mod keymap;
mod menu;
mod mouse;
mod output;
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
use ratatui::DefaultTerminal;
use ratatui::backend::{Backend as _, ClearType};
use ratatui::widgets::Clear;
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::mpsc;

use app::{App, Handover};
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

/// Whether `name` is a built-in keymap, for `ui.keymap`; and the names there are.
pub(crate) fn is_valid_keymap(name: &str) -> bool {
    keymap::Keymap::by_name(name).is_some()
}

pub(crate) fn keymap_names() -> &'static [&'static str] {
    keymap::Keymap::NAMES
}

/// The built-in keymaps `left` and `right` compared action by action, for `noc keymap diff`;
/// `None` if either is not one. `all` shows the actions they bind alike too, and `color` paints
/// the differences.
pub(crate) fn keymap_diff(left: &str, right: &str, all: bool, color: bool) -> Option<String> {
    keymap::Diff::by_names(left, right).map(|diff| diff.render(all, color))
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
        Done::History(result) => app.history_changed(result),
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
    // Raw mode turns Ctrl+c into a key, but these can still come from elsewhere, such as `kill`
    // or a closed terminal window; quitting through them restores the terminal.
    let mut terminate = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    // Reads /etc/localtime.
    let tz = tokio::task::spawn_blocking(TimeZone::system).await?;
    let restore = Restore::default();
    let mut terminal = ratatui::try_init()?;
    release_mouse_on_panic();
    let mut asked = Asked {
        modes: Modes::start(&restore).await?,
        ..Asked::default()
    };
    let mut events = EventStream::new();
    let mut spinner = tokio::time::interval(SPINNER_FRAME);
    spinner.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut keys = KeyState::default();
    let mut clicks = Clicks::default();
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
        if let Some(handover) = app.take_handover() {
            let (fresh, after) =
                hand_over((&mut terminal, &mut asked), events, &mut app, handover).await;
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
        if let Err(error) = take_terminal_requests(&mut terminal, &mut app, &mut asked) {
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
    let _ = asked.title.show(None);
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
    effects.push(app::Effect::History(app::HistoryChange::Load));
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
/// does next and a new stream of SIGINT, since Ctrl+c in the program reached Noon Commander
/// too, with a new stream of events in place of `events`. The program gets the mouse and the
/// terminal's own title, and may set one of its own; the next turn of the event loop takes
/// both again.
async fn hand_over(
    (terminal, asked): (&mut DefaultTerminal, &mut Asked),
    events: EventStream,
    app: &mut App,
    handover: Handover,
) -> (EventStream, io::Result<(Vec<app::Effect>, Signal)>) {
    // The program gets every key. A stream's reader holds crossterm's input lock while it
    // waits, and a new stream takes that lock, so the old one goes first; the new one reads
    // nothing until it is polled.
    drop(events);
    asked.mouse = false;
    let suspended = asked
        .title
        .show(None)
        .and_then(|()| suspend(terminal, asked.modes))
        .map_err(|error| error.to_string());
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
                Ok(()) => output::run_command(run).await,
                Err(error) => Err(error),
            };
            let mut events = EventStream::new();
            if let Ok(ran) = &result
                && ran.waits
            {
                output::wait_for_key(&mut events).await;
                output::after_key(ran);
            }
            (events, app.ran(result.map(drop)))
        }
        Handover::UserScreen => {
            let mut events = EventStream::new();
            if suspended.is_ok() && crossterm::terminal::enable_raw_mode().is_ok() {
                output::wait_to_go_back(&mut events, app.keymap()).await;
            }
            (events, Vec::new())
        }
    };
    let after = resume(terminal, asked.modes).and_then(|()| signal(SignalKind::interrupt()));
    (events, after.map(|interrupt| (effects, interrupt)))
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
/// kitty keyboard protocol, which tells Shift+Enter from Enter (ADR 0019).
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

/// What the TUI has asked of the terminal beyond the screen; `ui.mouse` and
/// `ui.terminal_title` may change while the app runs.
#[derive(Debug, Default)]
struct Asked {
    modes: Modes,
    /// The mouse is captured.
    mouse: bool,
    title: Title,
}

/// Does what the app asks of the terminal: a full redraw, text for the clipboard, capturing
/// the mouse or releasing it, as `ui.mouse` says, and the window's title, as
/// `ui.terminal_title` says.
fn take_terminal_requests(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    asked: &mut Asked,
) -> io::Result<()> {
    if app.take_redraw() {
        repaint(terminal)?;
    }
    if let Some(text) = app.take_clipboard() {
        copy_to_clipboard(terminal, &text)?;
    }
    if app.mouse() != asked.mouse {
        asked.mouse = app.mouse();
        capture_mouse(asked.mouse)?;
    }
    asked.title.show(app.terminal_title())
}

/// The title that Noon Commander gave the terminal's window or tab, if it gave one.
///
/// Before its first title it saves the terminal's own on the terminal's stack of titles (xterm's
/// `CSI 22 t`, which most terminals have), and takes it back from there (`CSI 23 t`) when it
/// stops titling, so the shell gets its title back. A terminal without the stack ignores both,
/// and keeps the last title until something else sets one.
#[derive(Debug, Default)]
struct Title {
    shown: Option<String>,
}

impl Title {
    const PUSH: &str = "\x1b[22;0t";
    const POP: &str = "\x1b[23;0t";

    /// Titles the window with `title`, or gives the terminal its own title back for `None`.
    fn show(&mut self, title: Option<String>) -> io::Result<()> {
        self.write(&mut io::stdout(), title)
    }

    /// Writes to `out` what changes the title to `title`. The title is terminal-safe text,
    /// which holds no control character that could end the sequence.
    fn write(&mut self, out: &mut impl io::Write, title: Option<String>) -> io::Result<()> {
        use crossterm::style::Print;
        use crossterm::terminal::SetTitle;

        if self.shown == title {
            return Ok(());
        }
        match (&self.shown, &title) {
            (None, Some(text)) => crossterm::execute!(out, Print(Self::PUSH), SetTitle(text))?,
            (Some(_), Some(text)) => crossterm::execute!(out, SetTitle(text))?,
            (Some(_), None) => crossterm::execute!(out, Print(Self::POP))?,
            (None, None) => {}
        }
        self.shown = title;
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn written(title: &mut Title, text: Option<&str>) -> String {
        let mut out = Vec::new();
        title.write(&mut out, text.map(str::to_owned)).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn the_title_saves_the_terminals_own_and_gives_it_back() {
        let mut title = Title::default();
        assert_eq!(written(&mut title, None), "", "nothing to give back");
        assert_eq!(
            written(&mut title, Some("~ — noc")),
            "\x1b[22;0t\x1b]0;~ — noc\x07"
        );
        assert_eq!(written(&mut title, Some("~ — noc")), "", "unchanged");
        assert_eq!(
            written(&mut title, Some("~/src — noc")),
            "\x1b]0;~/src — noc\x07",
            "saved once"
        );
        assert_eq!(written(&mut title, None), "\x1b[23;0t");
        assert_eq!(written(&mut title, None), "", "given back once");
    }
}
