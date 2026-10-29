//! The terminal's own screen, the one the panels hide: commands of the command line write there
//! with the terminal handed over, and `g <` shows it (ADR 0019). Each command is shown after
//! its prompt, as a shell would, and its output ends on a line of its own.

use std::io::{self, Write as _};
use std::os::unix::process::ExitStatusExt as _;
use std::process::ExitStatus;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use crossterm::style::{Attribute, Color, Print, SetAttribute, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use futures_util::StreamExt as _;
use noc_config::Pause;
use noc_tools::ToolError;
use noc_tools::job::{Ending, Running, Waited};
use noc_tools::shell::Shell;
use rustix::process;
use tokio::process::Child;

use super::command::{Place, Run};
use super::glyphs;
use super::keymap::{Action, Context, KeyState, Keymap, Mode, Resolved};
use super::told::Told;
use super::{describe, tasks};
use crate::i18n::fl;

/// How a command that ran ended, for what follows it on the screen.
#[derive(Debug)]
pub(super) struct Ran {
    /// Its output stays until a key, after a line that asks for one.
    pub(super) waits: bool,
    /// `[exit 2]` for a command that failed, which stays on the screen.
    pub(super) mark: Option<String>,
}

/// Runs the shell command of `run`, with the terminal handed over, as the last argument of
/// its shell: locally, or on its host through ssh over the host's connection, after its
/// prompt. Then ends its output on a line of its own and, if it waits, asks for a key there,
/// in raw mode to read it; otherwise leaves the mark of a failure. Fails if it could not start.
pub(super) async fn run_command(
    run: Run,
    (keymap, told): (&Keymap, &mut Told),
) -> Result<Ran, String> {
    let Run {
        place,
        shell,
        command,
        prompt,
        pause,
    } = run;
    let mut out = io::stdout();
    echo(&mut out, &prompt, &command);
    let status = match place {
        Place::Local(dir) => {
            let shell = Shell::from_env(shell.words());
            let cannot_run = |error: ToolError| {
                let reason = match &error {
                    ToolError::Spawn { source, .. } => source.to_string(),
                    other => describe::chain(other),
                };
                let program = shell.program().display().to_string();
                fl!("command-error", program = program, reason = reason)
            };
            let mut running = shell.start(&dir, &command).map_err(cannot_run)?;
            finish(&mut running, (keymap, told), cannot_run).await?
        }
        Place::Remote { handle, dir } => {
            let mut ssh = tasks::remote_command(&handle, dir, shell, command).await?;
            let program = ssh.as_std().get_program().to_string_lossy().into_owned();
            let cannot_run = |error: io::Error| {
                let reason = error.to_string();
                fl!("command-error", program = program.clone(), reason = reason)
            };
            let mut ssh = ssh.spawn().map_err(cannot_run)?;
            tokio::select! {
                biased;
                ending = told.came() => pass_on_to_ssh(&mut ssh, ending).await,
                status = ssh.wait() => Some(status.map_err(cannot_run)?),
            }
        }
    };
    end_line(&mut out);
    // One that did not end in time, let go, or ssh, killed, shows nothing more.
    let Some(status) = status else {
        return Ok(Ran {
            waits: false,
            mark: None,
        });
    };
    let (ending, mark) = if let Some(code) = status.code().filter(|&code| code != 0) {
        (
            Some(fl!("command-exit-code", code = code)),
            Some(fl!("command-exit-mark", code = code)),
        )
    } else if let Some(signal) = status.signal() {
        (
            Some(fl!("command-signal", signal = signal)),
            Some(fl!("command-signal-mark", signal = signal)),
        )
    } else {
        (None, None)
    };
    // Told to end, Noon Commander ends, and nothing waits for a key.
    let waits = !told.has_come()
        && match pause {
            Pause::Always => true,
            Pause::OnError => mark.is_some(),
            Pause::Never => false,
        };
    if waits {
        let press = fl!("command-press-key");
        let text = ending.map_or(press.clone(), |ending| format!("{ending} {press}"));
        let _ = crossterm::queue!(
            out,
            SetAttribute(Attribute::Reverse),
            Print(format!(" {text} ")),
            SetAttribute(Attribute::Reset)
        );
        let _ = out.flush();
        crossterm::terminal::enable_raw_mode().map_err(|error| error.to_string())?;
    } else if let Some(mark) = &mark {
        print_mark(&mut out, mark, "\n");
    }
    Ok(Ran { waits, mark })
}

/// How long a program that was told to end, as Noon Commander was, has to end before Noon
/// Commander ends: its last output, and how it puts the terminal back, come before Noon
/// Commander's own.
const LAST_WORDS: Duration = Duration::from_secs(1);

/// Waits for `running`, a command of the command line or the editor of F4, to end, and asks
/// what next each time it is stopped, as by Ctrl+z. What ends Noon Commander meanwhile goes on
/// to it, as a shell passes SIGHUP on to its jobs; one that goes on after a while is let go,
/// and its status is `None`. `cannot` says why it could not be waited for.
pub(super) async fn finish(
    running: &mut Running,
    (keymap, told): (&Keymap, &mut Told),
    cannot: impl Fn(ToolError) -> String,
) -> Result<Option<ExitStatus>, String> {
    let mut out = io::stdout();
    loop {
        let waited = tokio::select! {
            biased;
            ending = told.came() => return Ok(pass_on(running, ending).await),
            waited = running.wait() => waited.map_err(&cannot)?,
        };
        match waited {
            Waited::Ended(status) => return Ok(Some(status)),
            Waited::Stopped => {
                let goes_on = goes_on(&mut out, (keymap, told), running.has_ended()).await?;
                // Told to end meanwhile, it hears so next.
                if told.has_come() {
                    continue;
                }
                // One that cannot go on ends, rather than stays stopped while it waits.
                if !(goes_on && running.resume().is_ok()) {
                    let _ = running.end();
                }
            }
        }
    }
}

/// Passes `ending` on to `running` and gives it a while to end; one that goes on is let go, as a
/// shell that ends leaves a job that took no SIGHUP, rather than killed.
async fn pass_on(running: &mut Running, ending: Ending) -> Option<ExitStatus> {
    let _ = running.pass_on(ending);
    let ended = async {
        loop {
            match running.wait().await {
                Ok(Waited::Ended(status)) => return Some(status),
                Ok(Waited::Stopped) => {}
                Err(_) => return None,
            }
        }
    };
    let status = tokio::time::timeout(LAST_WORDS, ended).await.ok().flatten();
    if status.is_none() {
        running.let_go();
    }
    status
}

/// Passes `ending` on to `ssh`, which ends its session then, so that the host ends the command
/// there, and gives it a while to end; then it is killed.
async fn pass_on_to_ssh(ssh: &mut Child, ending: Ending) -> Option<ExitStatus> {
    if let Some(pid) = ssh
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(process::Pid::from_raw)
    {
        let _ = process::kill_process(pid, ending.signal());
    }
    let status = tokio::time::timeout(LAST_WORDS, ssh.wait()).await;
    status.ok().and_then(Result::ok)
}

/// Asks whether a command that was stopped, as by Ctrl+z, goes on or ends: on a line of its
/// own, `[stopped]`, as the mark of a failure, then the keys of a dialog's question, which read
/// the answer: Confirm goes on, Cancel ends it, or kills it if it was `ended` before and went
/// on. The line goes after the answer, and the terminal is as it was before, for the command.
/// Without input to read, or told to end, it ends.
async fn goes_on(
    out: &mut impl io::Write,
    (keymap, told): (&Keymap, &mut Told),
    ended: bool,
) -> Result<bool, String> {
    let key = |action| keymap.key(Context::Dialog, action).unwrap_or_default();
    let (resume, end) = (key(Action::Confirm), key(Action::Cancel));
    let keys = if ended {
        fl!("command-stopped-kill-keys", resume = resume, end = end)
    } else {
        fl!("command-stopped-keys", resume = resume, end = end)
    };
    let Some(mut events) = told.events() else {
        return Ok(false);
    };
    end_line(out);
    print_mark(out, &fl!("command-stopped-mark"), &format!(" {keys}"));
    crossterm::terminal::enable_raw_mode().map_err(|error| error.to_string())?;
    let mut keys = KeyState::default();
    let mut answer = None;
    while answer.is_none() {
        let Some(key) = next_key(&mut events, told).await else {
            break;
        };
        let place = (Mode::Normal, Context::Dialog);
        for typed in keymap.feed(&mut keys, place, key, std::time::Instant::now()) {
            match typed.resolved {
                Resolved::Action(Action::Confirm) => answer = Some(true),
                Resolved::Action(Action::Cancel) => answer = Some(false),
                _ => {}
            }
        }
    }
    // The command reads the keys from here on.
    drop(events);
    let _ = crossterm::queue!(out, Print("\r"), Clear(ClearType::CurrentLine));
    let _ = out.flush();
    crossterm::terminal::disable_raw_mode().map_err(|error| error.to_string())?;
    Ok(answer.unwrap_or(false))
}

/// After the key a command waited for: the line that asked for it goes, and the mark of a
/// failure takes its place. In raw mode.
pub(super) fn after_key(ran: &Ran) {
    let mut out = io::stdout();
    let _ = crossterm::queue!(out, Print("\r"), Clear(ClearType::CurrentLine));
    match &ran.mark {
        Some(mark) => print_mark(&mut out, mark, "\r\n"),
        None => {
            let _ = out.flush();
        }
    }
}

/// A mark, of a failure, `[exit 2]`, or of a command that is stopped, bold and red in the
/// terminal's own palette, so that it stands out whatever the theme; then `after`, the line
/// break of the mode the terminal is in, or the rest of the line.
fn print_mark(out: &mut impl io::Write, mark: &str, after: &str) {
    let _ = crossterm::queue!(
        out,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Red),
        Print(mark),
        SetAttribute(Attribute::Reset),
        Print(after)
    );
    let _ = out.flush();
}

/// Shows `command` after `prompt`, bold, as a shell shows what was typed; its lines after the
/// first after `> `.
fn echo(out: &mut impl io::Write, prompt: &str, command: &str) {
    let _ = crossterm::queue!(out, SetAttribute(Attribute::Bold));
    for (number, line) in command.split('\n').enumerate() {
        let lead = if number == 0 { prompt } else { "> " };
        let _ = crossterm::queue!(out, Print(format!("{lead}{line}\n")));
    }
    let _ = crossterm::queue!(out, SetAttribute(Attribute::Reset));
    let _ = out.flush();
}

/// Ends the output on a line of its own, as zsh does, without asking the terminal where the
/// cursor is: a dim `⏎`, then spaces up to the last column, then the start of the line,
/// cleared. If the cursor was at the start of a line, the mark is cleared again; otherwise the
/// spaces go on to the next line and the mark stays, saying that the output had no line break
/// at its end.
fn end_line(out: &mut impl io::Write) {
    let columns = crossterm::terminal::size().map_or(80, |(columns, _)| columns);
    let _ = crossterm::queue!(
        out,
        SetAttribute(Attribute::Dim),
        Print(glyphs::NO_NEWLINE),
        SetAttribute(Attribute::Reset),
        Print(" ".repeat(usize::from(columns.saturating_sub(1)))),
        Print("\r"),
        Clear(ClearType::UntilNewLine)
    );
    let _ = out.flush();
}

/// Waits for a key press; or for the end of the input, or an error, which the event loop meets
/// again; or for Noon Commander to be told to end.
pub(super) async fn wait_for_key(events: &mut EventStream, told: &mut Told) {
    next_key(events, told).await;
}

/// Waits, while the terminal's screen shows, for a key that `keymap` binds to going back to
/// the panels there, such as `g <` or Esc. Other keys do nothing.
pub(super) async fn wait_to_go_back(events: &mut EventStream, keymap: &Keymap, told: &mut Told) {
    let mut keys = KeyState::default();
    while let Some(key) = next_key(events, told).await {
        let now = std::time::Instant::now();
        let place = (Mode::Normal, Context::UserScreen);
        let typed = keymap.feed(&mut keys, place, key, now);
        if typed
            .iter()
            .any(|typed| typed.resolved == Resolved::Action(Action::Cancel))
        {
            return;
        }
    }
}

/// The next key pressed; `None` once the input ends or fails, or Noon Commander is told to end.
async fn next_key(events: &mut EventStream, told: &mut Told) -> Option<KeyEvent> {
    loop {
        tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => return Some(key),
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return None,
            },
            _ = told.came() => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echoes_the_command_after_its_prompt_in_bold() {
        let mut out = Vec::new();
        echo(&mut out, "~/src $ ", "make \\\n  test");
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "\u{1b}[1m~/src $ make \\\n>   test\n\u{1b}[0m");
    }

    #[test]
    fn the_mark_of_a_failure_is_bold_and_red() {
        let mut out = Vec::new();
        print_mark(&mut out, "[exit 2]", "\r\n");
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "\u{1b}[1m\u{1b}[38;5;9m[exit 2]\u{1b}[0m\r\n");
    }

    #[test]
    fn ends_the_output_on_a_line_of_its_own() {
        let mut out = Vec::new();
        end_line(&mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("\u{1b}[2m⏎\u{1b}[0m "), "{text:?}");
        assert!(text.ends_with(" \r\u{1b}[K"), "{text:?}");
    }
}
