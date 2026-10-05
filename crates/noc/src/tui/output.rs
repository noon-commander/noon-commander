//! The terminal's own screen, the one the panels hide: commands of the command line write there
//! with the terminal handed over, and Ctrl+o shows it (ADR 0019). Each command is shown after
//! its prompt, as a shell would, and its output ends on a line of its own.

use std::io::{self, Write as _};
use std::os::unix::process::ExitStatusExt as _;

use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::style::{Attribute, Color, Print, SetAttribute, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use futures_util::StreamExt as _;
use noc_config::Pause;
use noc_tools::ToolError;
use noc_tools::shell::Shell;

use super::command::{Place, Run};
use super::keymap::{Action, Context, KeyState, Keymap, Resolved};
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

/// Runs the shell command of `run`, with the terminal handed over: locally in `$SHELL`, or on
/// its host through ssh over the host's connection, after its prompt. Then ends its output on
/// a line of its own and, if it waits, asks for a key there, in raw mode to read it; otherwise
/// leaves the mark of a failure. Fails if it could not start.
pub(super) async fn run_command(run: Run) -> Result<Ran, String> {
    let Run {
        place,
        command,
        prompt,
        pause,
    } = run;
    let mut out = io::stdout();
    echo(&mut out, &prompt, &command);
    let status = match place {
        Place::Local(dir) => {
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
        Place::Remote { handle, dir } => {
            let mut ssh = tasks::remote_command(&handle, dir, command).await?;
            let program = ssh.as_std().get_program().to_string_lossy().into_owned();
            ssh.status().await.map_err(|error| {
                let reason = error.to_string();
                fl!("command-error", program = program, reason = reason)
            })?
        }
    };
    end_line(&mut out);
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
    let waits = match pause {
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

/// The mark of a failure, `[exit 2]`, bold and red in the terminal's own palette, so that it
/// stands out whatever the theme; then `end`, the line break of the mode the terminal is in.
fn print_mark(out: &mut impl io::Write, mark: &str, end: &str) {
    let _ = crossterm::queue!(
        out,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Red),
        Print(mark),
        SetAttribute(Attribute::Reset),
        Print(end)
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
        Print("⏎"),
        SetAttribute(Attribute::Reset),
        Print(" ".repeat(usize::from(columns.saturating_sub(1)))),
        Print("\r"),
        Clear(ClearType::UntilNewLine)
    );
    let _ = out.flush();
}

/// Waits for a key press; or for the end of the input, or an error, which the event loop meets
/// again.
pub(super) async fn wait_for_key(events: &mut EventStream) {
    while let Some(Ok(event)) = events.next().await {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            return;
        }
    }
}

/// Waits, while the terminal's screen shows, for a key that `keymap` binds to going back to
/// the panels there: Ctrl+o or Esc in the mc preset. Other keys do nothing.
pub(super) async fn wait_to_go_back(events: &mut EventStream, keymap: &Keymap) {
    let mut keys = KeyState::default();
    while let Some(Ok(event)) = events.next().await {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            let now = std::time::Instant::now();
            let resolved = keymap.feed(&mut keys, Context::UserScreen, key, now);
            if resolved.contains(&Resolved::Action(Action::Cancel)) {
                return;
            }
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
