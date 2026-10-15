//! `noc debug version`: one fact per line, a key and then its value, for scripts and bug
//! reports; a value that cannot be found is `unknown`. The keys are an interface: keep them and
//! their order, and add new ones at the end. `noc --version` is clap's single line.

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal as _, Read as _, Write as _};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use noc_tools::editor::Editor;
use rustix::termios::{OptionalActions, Termios, tcgetattr, tcgetpgrp, tcsetattr};

use crate::about::{
    Build, Detected, Fact, Keyboard, Report, System, Terminal, editor_command, known,
};

/// Prints the report, with what zoxide, ripgrep, and fd said they are. Blocks while the
/// terminal is asked.
pub(super) fn print((zoxide, ripgrep, fd): (Detected, Detected, Detected)) -> ExitCode {
    // The terminal is in raw mode while it is asked, so the report is ready before printing.
    let answers = Answers::query();
    let report = Report {
        system: System::current(),
        terminal: Terminal::from_env(
            |name| std::env::var_os(name),
            answers.name,
            answers.keyboard,
        ),
        editor: editor_command(&Editor::from_env()),
        zoxide,
        ripgrep,
        fd,
        ..Report::new(Build::current())
    };
    let text = text(&report);
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// What the terminal answers to [`QUERY`].
#[derive(Debug, PartialEq, Eq)]
struct Answers {
    /// Its name and version, from XTVERSION.
    name: Option<String>,
    keyboard: Keyboard,
}

/// XTVERSION, the kitty keyboard flags, and primary device attributes. Every terminal answers
/// the last, and in order, so its answer ends the wait.
const QUERY: &[u8] = b"\x1b[>q\x1b[?u\x1b[c";
const TIMEOUT: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(10);

impl Answers {
    /// Asks the terminal on `/dev/tty`, so that it is asked even when stdout is a pipe, as in
    /// `noc debug version | pbcopy`. Not when stdin is not a terminal: then no one is there.
    fn query() -> Self {
        let unchecked = Self {
            name: None,
            keyboard: Keyboard::Unchecked,
        };
        if !std::io::stdin().is_terminal() {
            return unchecked;
        }
        let Ok(tty) = OpenOptions::new().read(true).write(true).open("/dev/tty") else {
            return unchecked;
        };
        // The terminal stops a background job that reads it.
        if tcgetpgrp(&tty).ok() != Some(rustix::process::getpgrp()) {
            return unchecked;
        }
        Self::parse(&ask(&tty).unwrap_or_default())
    }

    fn parse(reply: &[u8]) -> Self {
        let keyboard = if private_csi(reply, b'u') {
            Keyboard::Kitty
        } else if private_csi(reply, b'c') {
            Keyboard::Legacy
        } else {
            Keyboard::Unknown
        };
        Self {
            name: xtversion(reply),
            keyboard,
        }
    }
}

/// Writes [`QUERY`] and reads the answers, in raw mode so that they are not echoed.
fn ask(tty: &File) -> io::Result<Vec<u8>> {
    let _raw = Raw::new(tty)?;
    rustix::io::ioctl_fionbio(tty, true)?;
    let (mut writer, mut reader) = (tty, tty);
    writer.write_all(QUERY)?;
    let deadline = Instant::now() + TIMEOUT;
    let mut reply = Vec::new();
    let mut buffer = [0u8; 256];
    while !private_csi(&reply, b'c') && Instant::now() < deadline {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => reply.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(POLL),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(reply)
}

/// Raw mode on the terminal until dropped.
struct Raw<'a> {
    tty: &'a File,
    saved: Termios,
}

impl<'a> Raw<'a> {
    fn new(tty: &'a File) -> io::Result<Self> {
        let saved = tcgetattr(tty)?;
        let mut raw = saved.clone();
        raw.make_raw();
        tcsetattr(tty, OptionalActions::Now, &raw)?;
        Ok(Self { tty, saved })
    }
}

impl Drop for Raw<'_> {
    fn drop(&mut self) {
        let _ = tcsetattr(self.tty, OptionalActions::Now, &self.saved);
    }
}

/// Whether `reply` has `ESC [ ? params end`, the form of the keyboard flags and of the device
/// attributes.
fn private_csi(reply: &[u8], end: u8) -> bool {
    (0..reply.len()).any(|start| {
        let Some(rest) = reply[start..].strip_prefix(b"\x1b[?") else {
            return false;
        };
        let params = rest
            .iter()
            .take_while(|&&byte| byte.is_ascii_digit() || byte == b';')
            .count();
        rest.get(params) == Some(&end)
    })
}

/// The XTVERSION answer, `ESC P > | text ESC \`; `kitty(0.35.2)` becomes `kitty 0.35.2`.
fn xtversion(reply: &[u8]) -> Option<String> {
    const START: &[u8] = b"\x1bP>|";
    let start = reply
        .windows(START.len())
        .position(|window| window == START)?
        + START.len();
    let rest = &reply[start..];
    let text = known(&rest[..rest.windows(2).position(|window| window == b"\x1b\\")?])?;
    Some(
        match text.strip_suffix(')').and_then(|text| text.split_once('(')) {
            Some((name, version)) => format!("{} {version}", name.trim_end()),
            None => text,
        },
    )
}

/// Each fact of `report` on a line of its own: its key, and its value.
fn text(report: &Report) -> String {
    let width = Fact::ALL
        .iter()
        .map(|fact| fact.key().len())
        .max()
        .unwrap_or(0)
        + 2;
    let mut text = String::new();
    for fact in Fact::ALL {
        let _ = writeln!(text, "{:<width$}{}", fact.key(), report.value(fact).text());
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;

    use noc_tools::editor::Editor;

    use super::{
        Answers, Build, Detected, Keyboard, Report, System, Terminal, editor_command, text,
    };
    use crate::tui::ColorDepth;

    fn env_with(vars: &[(&str, &str)], answers: Answers) -> Terminal {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|&(name, value)| (name.to_owned(), OsString::from(value)))
            .collect();
        Terminal::from_env(
            |name| vars.get(name).cloned(),
            answers.name,
            answers.keyboard,
        )
    }

    fn terminal(vars: &[(&str, &str)]) -> Terminal {
        let unchecked = Answers {
            name: None,
            keyboard: Keyboard::Unchecked,
        };
        env_with(vars, unchecked)
    }

    #[test]
    fn the_report_has_one_key_and_value_per_line() {
        let build = Build {
            version: "1.2.3",
            features: &[("forwarding", false), ("other", true)],
            commit: "0123456789ab",
            dirty: "no",
            commit_time: "2026-10-12T14:03:27Z",
            target: "aarch64-apple-darwin",
            profile: "release",
            license: "GPL-3.0-or-later",
            repository: "https://example.com/noc",
        };
        let system = System {
            os: Some("macOS 26.0".to_owned()),
            kernel: Some("Darwin 25.0.0".to_owned()),
            arch: None,
        };
        let answers = Answers::parse(b"\x1bP>|kitty(0.35.2)\x1b\\\x1b[?0u\x1b[?62;c");
        let terminal = env_with(
            &[
                ("TERM", "xterm-kitty"),
                ("TMUX", "/tmp/tmux-501/default,1,0"),
                ("COLORTERM", "truecolor"),
            ],
            answers,
        );
        let report = Report {
            build,
            system,
            terminal,
            editor: Some("nvim -p".to_owned()),
            zoxide: Detected::Found("zoxide 0.9.8".to_owned()),
            ripgrep: Detected::Found("ripgrep 15.2.0".to_owned()),
            fd: Detected::Found("fd 10.5.0".to_owned()),
        };
        insta::assert_snapshot!(text(&report));
        let missing = Report {
            zoxide: Detected::Missing,
            ripgrep: Detected::Missing,
            fd: Detected::Missing,
            ..report
        };
        assert!(
            text(&missing).ends_with(
                "zoxide       not found\nripgrep      not found\nfd           not found\n"
            ),
            "{}",
            text(&missing)
        );
    }

    #[test]
    fn the_answers_of_the_terminal_are_read() {
        let kitty = Answers::parse(b"\x1bP>|kitty(0.35.2)\x1b\\\x1b[?0u\x1b[?62;c");
        assert_eq!(kitty.name.as_deref(), Some("kitty 0.35.2"));
        assert_eq!(kitty.keyboard, Keyboard::Kitty);

        let wezterm = Answers::parse(b"\x1bP>|WezTerm 20240203-110809-5046fc22\x1b\\\x1b[?65;4c");
        assert_eq!(
            wezterm.name.as_deref(),
            Some("WezTerm 20240203-110809-5046fc22")
        );
        assert_eq!(wezterm.keyboard, Keyboard::Legacy);

        // Apple's Terminal answers only the device attributes.
        let apple = Answers::parse(b"\x1b[?1;2c");
        assert_eq!(apple.name, None);
        assert_eq!(apple.keyboard, Keyboard::Legacy);

        let silent = Answers::parse(b"");
        assert_eq!(silent.name, None);
        assert_eq!(silent.keyboard, Keyboard::Unknown);

        // Cut off before its end, as after the timeout.
        assert_eq!(Answers::parse(b"\x1bP>|kitty(0.3").name, None);
        assert_eq!(Answers::parse(b"\x1b[?62;").keyboard, Keyboard::Unknown);
    }

    #[test]
    fn the_terminal_comes_from_the_environment_where_it_does_not_answer() {
        let bare = terminal(&[]);
        assert_eq!(bare.program, None);
        assert_eq!(bare.term, None);
        assert_eq!(bare.multiplexer, "none");
        assert_eq!(bare.colors, ColorDepth::Indexed);
        assert_eq!(bare.keyboard, Keyboard::Unchecked);

        let program = terminal(&[("TERM_PROGRAM", "Apple_Terminal"), ("STY", "1.pts")]);
        assert_eq!(program.program.as_deref(), Some("Apple_Terminal"));
        assert_eq!(program.multiplexer, "screen");

        let versioned = terminal(&[
            ("TERM_PROGRAM", "WezTerm"),
            ("TERM_PROGRAM_VERSION", "20240203"),
        ]);
        assert_eq!(versioned.program.as_deref(), Some("WezTerm 20240203"));

        let kitty = terminal(&[("TERM", "xterm-kitty"), ("KITTY_WINDOW_ID", "1")]);
        assert_eq!(kitty.program.as_deref(), Some("kitty"));

        let answered = env_with(
            &[("TERM_PROGRAM", "tmux")],
            Answers::parse(b"\x1bP>|tmux 3.5a\x1b\\\x1b[?1;2c"),
        );
        assert_eq!(answered.program.as_deref(), Some("tmux 3.5a"));
    }

    #[test]
    fn the_editor_is_the_command_of_f4() {
        let editor = |vars: &[(&str, &str)]| {
            editor_command(&Editor::from_vars(|name| {
                vars.iter()
                    .find(|&&(key, _)| key == name)
                    .map(|&(_, value)| value.to_owned())
            }))
        };
        assert_eq!(
            editor(&[("VISUAL", "code  -w"), ("EDITOR", "nano")]).as_deref(),
            Some("code -w")
        );
        assert_eq!(editor(&[]).as_deref(), Some("vi"));
    }

    #[test]
    fn control_characters_from_the_environment_cannot_break_a_line() {
        let terminal = terminal(&[("TERM", "xterm\nkeyboard kitty\x1b[2J")]);
        assert_eq!(terminal.term.as_deref(), Some("xterm?keyboard kitty?[2J"));
    }
}
