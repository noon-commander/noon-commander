//! The terminal user interface: two panels over the local file system and SFTP hosts.

mod app;
mod cd;
mod cells;
mod combo_box;
mod command;
mod complete;
mod configuration;
mod decor;
mod describe;
mod dialog;
mod edit;
mod find;
mod fuzzy;
mod glyphs;
mod help;
mod history;
mod jobs;
mod jump;
mod key_hints;
mod keymap;
mod label;
mod loading;
mod machine;
mod menu;
mod mouse;
mod output;
mod panel;
mod pattern;
mod picker;
mod places;
mod progress;
mod pulldown;
mod reading;
mod save;
mod scroll;
mod search;
mod sidebar;
mod spinner;
mod sums;
mod tab_menu;
mod tabs;
mod tasks;
mod theme;
mod told;
mod transfer;
mod which_key;
mod workspaces;
mod zoom;

use std::io::{self, IsTerminal as _};
use std::path::{Path, PathBuf};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, bail};
use crossterm::cursor::SetCursorStyle;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyEventKind, MouseEvent,
};
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use futures_util::StreamExt as _;
use jiff::tz::TimeZone;
use noc_locale::Formats;
use noc_tools::ToolError;
use noc_tools::editor::Editor;
use ratatui::backend::{Backend as _, ClearType};
use ratatui::widgets::Clear;
use ratatui::{DefaultTerminal, Frame};
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::mpsc;

use app::{App, Handover};
use help::{About, Files};
use keymap::{KeyState, Keyboard, Keymap, Mode};
use mouse::{Clicks, Press};
use tasks::{Done, Tasks};
use told::Told;

use crate::about::{self, Build, Report, System, Tool};
use crate::context::Context;
use crate::i18n::fl;

pub(crate) use places::read_volumes;
pub(crate) use theme::ColorDepth;

/// Whether `name` is a built-in theme, for `ui.theme`; and the names there are.
pub(crate) fn is_valid_theme(name: &str) -> bool {
    theme::Theme::by_name(name, theme::ColorDepth::TrueColor).is_some()
}

pub(crate) fn theme_names() -> &'static [&'static str] {
    theme::Theme::NAMES
}

/// Hands `first` to the app, and every result after it that waits in `done`, so that they draw
/// once: jobs report every entry. Whether the screen changes with any of them.
fn take_all_done(
    app: &mut App,
    tasks: &mut Tasks,
    done: &mut mpsc::UnboundedReceiver<Done>,
    first: Done,
) -> bool {
    let mut changes = take_done(app, tasks, first);
    while let Ok(next) = done.try_recv() {
        changes |= take_done(app, tasks, next);
    }
    changes
}

/// Hands finished background work to the app, and runs what the app asks for next. Whether
/// the screen changes with it: not with every report of a job in the background.
fn take_done(app: &mut App, tasks: &mut Tasks, done: Done) -> bool {
    match done {
        Done::Listed {
            panel,
            generation,
            result,
        } => tasks.run(app.listed(panel, generation, result)),
        Done::Places { generation, result } => app.places(generation, result),
        Done::Created {
            panel,
            location,
            result,
        } => tasks.run(app.created(panel, &location, result)),
        Done::Job { id, event } => {
            let (effects, changes) = app.job_report(id, event);
            tasks.run(effects);
            return changes;
        }
        Done::Read { id, result } => tasks.run(app.read(id, result)),
        Done::ReadProgress { id, progress } => app.read_progress(id, progress),
        Done::Detected(tool, detected) => app.detected(tool, detected),
        Done::Formats(formats) => {
            noc_locale::set_current(*formats);
            app.formats_changed();
        }
        Done::Connected {
            host,
            connection,
            handle,
        } => tasks.run(app.connected(&host, connection, handle)),
        Done::Resolved {
            host,
            address,
            user,
        } => app.resolved(host, address, &user),
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
        Done::Recent(result) => app.recent_loaded(result),
        Done::Jumps { generation, result } => app.jumps(generation, result),
        Done::Found { generation, found } => app.found(generation, found),
        Done::FindEnded { generation, result } => app.find_ended(generation, result),
        Done::OpenFailed { path, reason } => app.open_failed(&path, &reason),
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
    true
}

/// The time zone, from /etc/localtime, and the formats of the region, put to use, from the
/// environment and on macOS the settings of the system.
async fn region(context: &Context) -> Result<TimeZone> {
    let setting = context.config().ui.formats.clone();
    let (tz, formats) =
        tokio::task::spawn_blocking(move || (TimeZone::system(), Formats::detect(&setting)))
            .await?;
    noc_locale::set_current(formats);
    Ok(tz)
}

/// Debug builds only: how long results of background work are held back, so spinners stay
/// long enough to watch, how long windows take to grow or shrink, and whether windows show
/// their sizes.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DebugOptions {
    /// Every result (`--slow-spinners`).
    pub(crate) slow_spinners: Option<Duration>,
    /// A connection made or ended, on top of that (`--slow-connections`).
    pub(crate) slow_connections: Option<Duration>,
    /// A window growing or shrinking, from start to end (`--slow-windows`).
    pub(crate) slow_windows: Option<Duration>,
    /// Each window writes its size on its frame (`--show-sizes`).
    pub(crate) show_sizes: bool,
}

/// Debug builds only: holds back each result of background work as `debug` says, from when it
/// comes. Connections have a queue of their own, so the results behind them do not wait for
/// them; each queue keeps its order.
#[cfg(debug_assertions)]
fn slow_down(
    done: mpsc::UnboundedSender<Done>,
    debug: DebugOptions,
) -> mpsc::UnboundedSender<Done> {
    if debug.slow_spinners.is_none() && debug.slow_connections.is_none() {
        return done;
    }
    let (sender, mut results) = mpsc::unbounded_channel::<Done>();
    let others = release(done.clone());
    let connections = release(done);
    tokio::spawn(async move {
        while let Some(result) = results.recv().await {
            let mut delay = debug.slow_spinners.unwrap_or_default();
            let queue = if matches!(result, Done::Connected { .. } | Done::Closed { .. }) {
                delay += debug.slow_connections.unwrap_or_default();
                &connections
            } else {
                &others
            };
            if queue
                .send((tokio::time::Instant::now() + delay, result))
                .is_err()
            {
                break;
            }
        }
    });
    sender
}

/// Debug builds only: sends each result to `done` at its time, in the order they come.
#[cfg(debug_assertions)]
fn release(
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<(tokio::time::Instant, Done)> {
    let (sender, mut held) = mpsc::unbounded_channel::<(tokio::time::Instant, Done)>();
    tokio::spawn(async move {
        while let Some((due, result)) = held.recv().await {
            tokio::time::sleep_until(due).await;
            if done.send(result).is_err() {
                break;
            }
        }
    });
    sender
}

/// Runs the TUI with both panels on the local directory `start` until the user quits or the
/// process gets SIGTERM, SIGHUP, or SIGINT. In debug builds, `debug` holds back the results of
/// background work, slows down windows, and has them show their sizes.
pub(crate) async fn run(
    context: Context,
    start: PathBuf,
    #[cfg(debug_assertions)] debug: DebugOptions,
) -> Result<()> {
    if !io::stdout().is_terminal() {
        bail!("the TUI needs a terminal; in scripts, use the subcommands (`noc --help`)");
    }
    // Raw mode turns Ctrl+c into a key, but these can still come from elsewhere, such as `kill`
    // or a closed terminal window; quitting through them restores the terminal.
    let mut told = Told::listen();
    let mut interrupt = signal(SignalKind::interrupt())?;
    let tz = region(&context).await?;
    // Reads the macOS version or the Linux distribution, for the About page of the help.
    let system = tokio::task::spawn_blocking(System::current).await?;
    let restore = Restore::default();
    let mut terminal = ratatui::try_init()?;
    release_mouse_on_panic();
    // A terminal that goes away before it answers leaves the reader that waits for the answer
    // spinning at the end of its input, for good (crossterm-rs/crossterm#793); it is left
    // behind, and `main` does not wait for it.
    let modes = tokio::select! {
        modes = Modes::start(&restore) => modes?,
        _ = told.came() => return Ok(()),
        _ = interrupt.recv() => return Ok(()),
    };
    let mut asked = Asked {
        modes,
        ..Asked::default()
    };
    let mut events = EventStream::new();
    let mut keys = KeyState::default();
    let mut clicks = Clicks::default();
    // The last event changed nothing, or results that wait for their frame, so the screen stays
    // as it is.
    let mut idle = false;
    let mut drawn = Instant::now();
    let mut pace = Pace::default();
    let context = Arc::new(context);
    let (done_tx, mut done) = mpsc::unbounded_channel();
    #[cfg(debug_assertions)]
    let done_tx = slow_down(done_tx, debug);
    let mut tasks = Tasks::start(Arc::clone(&context), done_tx).await;
    let (mut app, effects) = start_app(&context, &start, (&tz, system), asked.modes.keys());
    #[cfg(debug_assertions)]
    app.debug_windows(debug);
    tasks.run(effects);
    let result = loop {
        if app.quits() {
            break Ok(());
        }
        // The command of Ctrl+x may end without a key: by the mouse, or a program it ran.
        app.come_back();
        take_requests(&mut app, &mut tasks);
        if let Some(handover) = app.take_handover() {
            let tty = (&mut terminal, &mut asked, &mut told);
            let background = (&mut app, &mut tasks, &mut done);
            let Some((fresh, after)) = hand_over(tty, events, background, handover).await else {
                // Told to end while the terminal was handed over.
                break Ok(());
            };
            events = fresh;
            match after {
                Ok(fresh) => interrupt = fresh,
                Err(error) => break Err(error.into()),
            }
            keys = KeyState::default();
            continue;
        }
        if let Err(error) = take_terminal_requests(&mut terminal, &mut app, &mut asked) {
            break Err(error.into());
        }
        let now = Instant::now();
        take_hints(&mut app, &mut keys);
        if !std::mem::take(&mut idle) {
            if let Err(error) = draw_whole(&mut terminal, |frame| app.render(frame, now, &tz)) {
                break Err(error.into());
            }
            drawn = now;
            pace.drew();
        }
        let timer = key_timer(&app, &keys);
        let frame_due = app.frame_due();
        let spin_due = app.spin_due(drawn);
        tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    tasks.run(app.key(&mut keys, key, Instant::now()));
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
            () = sleep_until(timer) => key_time(&mut app, &mut tasks, &mut keys),
            Some(job) = done.recv() => {
                let changes = take_all_done(&mut app, &mut tasks, &mut done, job);
                idle = !(changes && pace.came(Instant::now()));
            }
            () = sleep_until(pace.due) => pace.release(Instant::now()),
            // A window growing or shrinking, a spinner, or a clock needs its next frame.
            () = sleep_until(frame_due) => {}
            () = sleep_until(spin_due) => {}
            _ = told.came() => break Ok(()),
            _ = interrupt.recv() => break Ok(()),
        }
    };
    shut_down((terminal, restore, &mut asked), &mut app, tasks).await;
    result
}

/// Gives the terminal back first, so a slow shutdown does not look like a hang; then stops the
/// jobs, which clean up before the connections go.
async fn shut_down(
    (terminal, restore, asked): (DefaultTerminal, Restore, &mut Asked),
    app: &mut App,
    mut tasks: Tasks,
) {
    let _ = asked.title.show(None);
    give_back(terminal);
    drop(restore);
    app.stop_jobs();
    tasks.finish_jobs().await;
    app.disconnect_all();
    tasks.shutdown().await;
}

/// Drops `terminal`, which shows the cursor again. Where the terminal went away, ratatui's drop
/// prints that it could not to stderr, the same terminal, and printing there panics; so then the
/// terminal goes without its drop.
fn give_back(mut terminal: DefaultTerminal) {
    if terminal.show_cursor().is_err() {
        std::mem::forget(terminal);
    }
}

/// The app with both panels on `start`, set up for this terminal and `system`, and the work it
/// asks for first: the saved workspaces, the history, the places used last, and the volumes,
/// which tell which one a panel is on.
fn start_app(
    context: &Context,
    start: &Path,
    (tz, system): (&TimeZone, System),
    keyboard: Keyboard,
) -> (App, Vec<app::Effect>) {
    let (mut app, mut effects) = App::new(start, &context.paths.home, &context.config());
    effects.push(app::Effect::Workspaces(app::WorkspaceChange::Load));
    effects.push(app::Effect::History(app::HistoryChange::Load));
    effects.push(app::Effect::Recent(app::RecentChange::Load));
    // No menu asks for this generation.
    effects.push(app::Effect::ListPlaces { generation: 0 });
    effects.extend([Tool::Zoxide, Tool::Ripgrep, Tool::Fd].map(app::Effect::Detect));
    app.set_color_depth(theme::ColorDepth::detect());
    app.set_keyboard(keyboard);
    app.set_time_zone(tz.clone());
    app.set_machine(machine::Machine::current());
    app.set_runtime_dir(context.paths.runtime_dir.clone());
    app.set_hosts(context.hosts());
    let paths = &context.paths;
    let keyboard_protocol = match keyboard {
        Keyboard::Kitty => about::Keyboard::Kitty,
        Keyboard::Legacy => about::Keyboard::Legacy,
    };
    app.set_about(About {
        report: Report {
            system,
            // Without the terminal's answer with its name, which the TUI does not ask for.
            terminal: about::Terminal::from_env(
                |name| std::env::var_os(name),
                None,
                keyboard_protocol,
            ),
            editor: about::editor_command(&Editor::from_env()),
            language: Some(crate::i18n::selected()),
            formats: Some(about::formats_text(&noc_locale::current())),
            ..Report::new(Build::current())
        },
        files: Some(Files {
            home: paths.home.clone(),
            config: context.config_file.clone(),
            hosts: context.hosts_file.clone(),
            workspaces: paths.workspaces_file(),
            history: paths.history_file(),
            recent: paths.recent_file(),
            log: paths.log_file(),
        }),
    });
    let path = std::env::var_os("PATH").unwrap_or_default();
    app.set_command_dirs(std::env::split_paths(&path).collect());
    (app, effects)
}

/// Runs the editor on `file`, at `line` if there is one, with the terminal handed over, as a
/// job that asks what next when Ctrl+z stops it, as a command of the command line does; and
/// tells whether the file changed (its size or time), or why the editor could not run.
async fn edit(
    file: &Path,
    line: Option<u64>,
    (keymap, told): (&Keymap, &mut Told),
) -> Result<bool, String> {
    let stamp = |metadata: std::fs::Metadata| (metadata.len(), metadata.modified().ok());
    let before = tokio::fs::metadata(file)
        .await
        .map(stamp)
        .map_err(|error| error.to_string())?;
    let editor = Editor::from_env();
    let cannot_run = |error: ToolError| {
        let reason = match &error {
            ToolError::Spawn { source, .. } => source.to_string(),
            other => describe::chain(other),
        };
        fl!(
            "edit-cannot-run",
            program = editor.program().display().to_string(),
            reason = reason
        )
    };
    let mut running = editor.start(file, line).map_err(cannot_run)?;
    output::finish(&mut running, (keymap, told), cannot_run).await?;
    let after = tokio::fs::metadata(file)
        .await
        .map(stamp)
        .map_err(|error| error.to_string())?;
    Ok(before != after)
}

/// Hands the terminal to the program of `handover`, takes it back, runs what the app does
/// next, and returns a new stream of SIGINT, since Ctrl+c in the program reached Noon
/// Commander too, with a new stream of events in place of `events`. The program gets the mouse
/// and the terminal's own title, and may set one of its own; the next turn of the event loop
/// takes both again. `None` if Noon Commander was `told` to end meanwhile: then no new stream
/// of events is made (`Told::events`).
///
/// Meanwhile the app takes the reports of how far its jobs are from `done` as they come,
/// without drawing them: a copy makes thousands a second, which would pile up until the
/// terminal came back, and then all be counted at once. The rest of what background work sends
/// waits, in the order it came, until the app has taken the program's end.
async fn hand_over(
    (terminal, asked, told): (&mut DefaultTerminal, &mut Asked, &mut Told),
    events: EventStream,
    (app, tasks, done): (&mut App, &mut Tasks, &mut mpsc::UnboundedReceiver<Done>),
    handover: Handover,
) -> Option<(EventStream, io::Result<Signal>)> {
    // The program gets every key. A stream's reader holds crossterm's input lock while it
    // waits, and a new stream takes that lock, so the old one goes first; the new one reads
    // nothing until it is polled.
    drop(events);
    asked.mouse = false;
    // Leaving the modes gives the cursor its own shape back.
    asked.bar = None;
    let suspended = asked
        .title
        .show(None)
        .and_then(|()| suspend(terminal, asked.modes))
        .map_err(|error| error.to_string());
    let keymap = app.keymap().clone();
    let mut held = Vec::new();
    let mut take = |result: Done| match result {
        Done::Job { id, event } if event.is_progress() => {
            tasks.run(app.job_event(id, event));
            None
        }
        other => Some(other),
    };
    let (events, effects) = match handover {
        Handover::Edit { file, line } => {
            let result = match suspended {
                Ok(()) => {
                    let editor = pin!(edit(&file, line, (&keymap, told)));
                    meanwhile(editor, done, &mut held, &mut take).await
                }
                Err(error) => Err(error),
            };
            (told.events()?, app.edited(result))
        }
        Handover::Run(run) => {
            let result = match suspended {
                Ok(()) => {
                    let command = pin!(output::run_command(run, (&keymap, told)));
                    meanwhile(command, done, &mut held, &mut take).await
                }
                Err(error) => Err(error),
            };
            let mut events = told.events()?;
            if let Ok(ran) = &result
                && ran.waits
            {
                let key = pin!(output::wait_for_key(&mut events, told));
                meanwhile(key, done, &mut held, &mut take).await;
                output::after_key(ran);
            }
            (events, app.ran(result.map(drop)))
        }
        Handover::UserScreen => {
            let mut events = told.events()?;
            if suspended.is_ok() && crossterm::terminal::enable_raw_mode().is_ok() {
                // The keys that go back are read as the panels read them, with the kitty keyboard
                // protocol, which tells Ctrl+[ and Ctrl+m from other keys.
                let _ = asked.modes.keyboard(true);
                let back = pin!(output::wait_to_go_back(&mut events, &keymap, told));
                meanwhile(back, done, &mut held, &mut take).await;
                let _ = asked.modes.keyboard(false);
            }
            (events, Vec::new())
        }
    };
    if told.has_come() {
        return None;
    }
    let after = resume(terminal, asked.modes).and_then(|()| signal(SignalKind::interrupt()));
    if after.is_ok() {
        tasks.run(effects);
        for result in held {
            take_done(app, tasks, result);
        }
    }
    Some((events, after))
}

/// Waits for `work` while it hands what comes in `done` to `take`, which gives back what has
/// to wait: that goes to `held`, in the order it came. What comes as `work` ends stays in
/// `done`. `work` is pinned where it was made, so that this future does not hold a copy of it.
async fn meanwhile<T, R>(
    mut work: Pin<&mut impl Future<Output = T>>,
    done: &mut mpsc::UnboundedReceiver<R>,
    held: &mut Vec<R>,
    mut take: impl FnMut(R) -> Option<R>,
) -> T {
    loop {
        tokio::select! {
            biased;
            out = &mut work => return out,
            Some(result) = done.recv() => held.extend(take(result)),
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
/// kitty keyboard protocol, which tells Shift+Enter from Enter and reports Alt with a symbol
/// typed with Shift, such as Alt++, as that symbol (ADR 0019).
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

    /// How the terminal sends keys while the TUI has it.
    fn keys(self) -> Keyboard {
        if self.keyboard {
            Keyboard::Kitty
        } else {
            Keyboard::Legacy
        }
    }

    fn enter(self) -> io::Result<()> {
        crossterm::execute!(io::stdout(), crossterm::event::EnableBracketedPaste)?;
        self.keyboard(true)
    }

    /// Leaves the modes, and gives the cursor its shape back; each even if another fails.
    fn leave(self) -> io::Result<()> {
        let keyboard = self.keyboard(false);
        let paste = crossterm::execute!(io::stdout(), crossterm::event::DisableBracketedPaste);
        let cursor = crossterm::execute!(io::stdout(), SetCursorStyle::DefaultUserShape);
        keyboard.and(paste).and(cursor)
    }

    /// Turns the kitty keyboard protocol on or off, where the terminal speaks it. Kitty keeps
    /// one stack of its modes for each screen, so each `true` takes a `false` on the same one.
    fn keyboard(self, on: bool) -> io::Result<()> {
        use crossterm::event::{
            KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
        };

        if !self.keyboard {
            return Ok(());
        }
        if on {
            // Without alternate keys, Alt+* arrives as Alt+Shift+8, which stands for Esc and a
            // key that does nothing, rather than Esc and `*`.
            let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;
            crossterm::execute!(io::stdout(), PushKeyboardEnhancementFlags(flags))
        } else {
            crossterm::execute!(io::stdout(), PopKeyboardEnhancementFlags)
        }
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
    /// The cursor is a bar, for Insert mode, or a block, for the others, as asked last; `None`
    /// while it has the terminal's own shape.
    bar: Option<bool>,
}

/// Does what the app asks of the tasks between events: the work directory of the process, which
/// follows the active panel, and the places used last, to save.
fn take_requests(app: &mut App, tasks: &mut Tasks) {
    if let Some(dir) = app.take_work_dir() {
        tasks.change_work_dir(dir);
    }
    if let Some(recent) = app.take_recent() {
        tasks.run(vec![app::Effect::Recent(app::RecentChange::Save(recent))]);
    }
}

/// Does what the app asks of the terminal: a full redraw, text for the clipboard, capturing
/// the mouse or releasing it, as `ui.mouse` says, the window's title, as `ui.terminal_title`
/// says, and the shape of the cursor, a bar in Insert mode and a block in the others, as vim
/// has it in terminals that can.
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
    let bar = app.mode() == Mode::Insert;
    if asked.bar != Some(bar) {
        asked.bar = Some(bar);
        let style = if bar {
            SetCursorStyle::SteadyBar
        } else {
            SetCursorStyle::SteadyBlock
        };
        crossterm::execute!(io::stdout(), style)?;
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

/// Hands `event` to the app if it is a press, or a drag of the thumb of a scroll bar; whether
/// it was. Moves of the mouse come all the time and do nothing, and so do drags of anything
/// else. A press ends a key sequence.
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
    if pointer.press == Press::Drag && !app.dragging() {
        return false;
    }
    *keys = KeyState::default();
    tasks.run(app.pointer(pointer));
    true
}

/// Releases the mouse and the modes on a panic: the hook of `ratatui::try_init`, which this
/// one calls, restores the terminal but leaves them on. A panic in the middle of a frame would
/// also leave the terminal holding back what it shows, until it gives up waiting. All are
/// harmless to turn off when they are not on.
fn release_mouse_on_panic() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(io::stdout(), EndSynchronizedUpdate);
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

/// Draws a frame between the marks of synchronized output, so that the terminal shows it whole.
/// Without them the cursor of a field shows on its way through the cells that change, such as
/// those a shine crosses every 40 ms, before it is put back. Terminals that do not know the
/// marks ignore them. The first mark is only queued: it leaves with the frame, in one write.
fn draw_whole(
    terminal: &mut DefaultTerminal,
    render: impl FnOnce(&mut Frame<'_>),
) -> io::Result<()> {
    crossterm::queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
    let drawn = terminal.draw(render).map(drop);
    crossterm::execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
    drawn
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

/// Shows the key hints that the app asked for, and has it draw those that `keys` has, and what
/// its pending command has typed.
fn take_hints(app: &mut App, keys: &mut KeyState) {
    if let Some(place) = app.take_hints_asked() {
        app.keymap().show_hints(keys, place);
    }
    app.set_hints(keys);
}

/// When the hints of the pending key sequence are due, while `ui.which_key` is on.
fn hints_due(app: &App, keys: &KeyState) -> Option<Instant> {
    app.hints_delay()
        .and_then(|delay| app.keymap().hints_due(keys, delay))
}

/// When the pending key sequence needs [`key_time`]: when its hints are due, for one that
/// waits for them and then for its next key; else when it times out.
fn key_timer(app: &App, keys: &KeyState) -> Option<Instant> {
    hints_due(app, keys).or_else(|| keys.deadline())
}

/// Shows the hints of the pending key sequence if they are due; else settles the sequence if
/// it timed out.
fn key_time(app: &mut App, tasks: &mut Tasks, keys: &mut KeyState) {
    let now = Instant::now();
    if hints_due(app, keys).is_some_and(|due| due <= now)
        && let Some(place) = keys.place()
    {
        app.keymap().show_hints(keys, place);
        return;
    }
    tasks.run(app.expire_keys(keys, now));
}

/// Completes at `deadline`; never without one.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}

/// The shortest time between two frames drawn for results of background work, which can come
/// faster than frames are drawn: a copy reports every chunk it moves, hundreds a second.
const RESULTS_FRAME: Duration = Duration::from_millis(40);

/// Draws results of background work at most once each [`RESULTS_FRAME`]. Keys and the mouse
/// draw at once, and so do results after a quiet spell, so neither waits behind a busy job.
#[derive(Debug, Default)]
struct Pace {
    /// When results last had a frame of their own.
    drawn: Option<Instant>,
    /// When the results that came since get theirs, if some wait.
    due: Option<Instant>,
}

impl Pace {
    /// Takes results that came at `now`: whether they draw now; if not, they wait for `due`.
    fn came(&mut self, now: Instant) -> bool {
        if self.due.is_some() {
            return false;
        }
        match self.drawn.map(|drawn| drawn + RESULTS_FRAME) {
            Some(due) if now < due => {
                self.due = Some(due);
                false
            }
            _ => {
                self.drawn = Some(now);
                true
            }
        }
    }

    /// The results that wait are due at `now`, and get their frame.
    fn release(&mut self, now: Instant) {
        self.due = None;
        self.drawn = Some(now);
    }

    /// A frame was drawn, which shows the results that waited.
    fn drew(&mut self) {
        self.due = None;
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
        // Not `ratatui::restore`, which prints why it failed to stderr, the terminal: printing
        // to one that went away panics.
        let _ = ratatui::try_restore();
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

    #[tokio::test]
    async fn results_are_taken_while_a_program_has_the_terminal() {
        let (sender, mut done) = mpsc::unbounded_channel();
        let (taken, mut heard) = mpsc::unbounded_channel();
        let program = async {
            for result in 0..7 {
                sender.send(result).unwrap();
            }
            // It goes on only once they were taken: as they came, not after it.
            for result in [0, 2, 4, 6] {
                assert_eq!(heard.recv().await, Some(result));
            }
            sender.send(7).unwrap();
            "ended"
        };
        let take = |result: u32| {
            if result % 2 == 1 {
                return Some(result);
            }
            taken.send(result).unwrap();
            None
        };
        let mut held = Vec::new();
        let program = pin!(program);
        let ended = meanwhile(program, &mut done, &mut held, take);
        let ended = tokio::time::timeout(Duration::from_secs(10), ended).await;
        assert_eq!(ended, Ok("ended"));
        assert_eq!(held, [1, 3, 5], "the others wait, in order");
        assert_eq!(done.try_recv(), Ok(7), "and what came as it ended");
    }

    #[test]
    fn results_draw_at_most_once_a_frame() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut pace = Pace::default();
        assert!(pace.came(at(0)), "the first results draw at once");
        assert!(!pace.came(at(3)));
        assert_eq!(pace.due, Some(at(40)));
        assert!(!pace.came(at(30)), "more results wait for the same frame");
        assert_eq!(pace.due, Some(at(40)));
        pace.release(at(40));
        assert_eq!(pace.due, None);
        assert!(
            !pace.came(at(45)),
            "and the next ones a whole frame after it"
        );
        assert_eq!(pace.due, Some(at(80)));
    }

    #[test]
    fn results_after_a_quiet_spell_or_a_key_draw_at_once() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut pace = Pace::default();
        assert!(pace.came(at(0)));
        assert!(!pace.came(at(10)));
        pace.drew();
        assert_eq!(
            pace.due, None,
            "a key's frame shows the results that waited"
        );
        assert!(pace.came(at(100)), "after a quiet spell");
    }
}
