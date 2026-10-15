//! `noc debug version`: one fact per line, a key and then its value, for scripts and bug
//! reports; a value that cannot be found is `unknown`. The keys are an interface: keep them and
//! their order, and add new ones at the end. `noc --version` is clap's single line.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal as _, Read as _, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use noc_tools::editor::Editor;
use rustix::termios::{OptionalActions, Termios, tcgetattr, tcgetpgrp, tcsetattr};

use crate::about::{Build, System, known};
use crate::tui::ColorDepth;

const UNKNOWN: &str = "unknown";

/// Prints the report. Blocks while the terminal is asked.
pub(super) fn print() -> ExitCode {
    // The terminal is in raw mode while it is asked, so the report is ready before printing.
    let editor = editor_command(&Editor::from_env());
    let text = report(
        &Build::current(),
        &System::current(),
        &Terminal::current(),
        editor.as_deref(),
    );
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// What the terminal says of itself, and what it answers.
struct Terminal {
    /// The name the terminal answers with, else `TERM_PROGRAM` and `TERM_PROGRAM_VERSION`,
    /// else a variable that only one terminal sets.
    program: Option<String>,
    term: Option<String>,
    multiplexer: &'static str,
    colors: ColorDepth,
    keyboard: Keyboard,
}

/// Variables by which terminals that set no `TERM_PROGRAM` can be told.
const TERMINAL_VARS: &[(&str, &str)] = &[
    ("KITTY_WINDOW_ID", "kitty"),
    ("ALACRITTY_WINDOW_ID", "Alacritty"),
    ("GHOSTTY_RESOURCES_DIR", "ghostty"),
    ("WEZTERM_PANE", "WezTerm"),
    ("KONSOLE_VERSION", "Konsole"),
];

impl Terminal {
    fn current() -> Self {
        Self::from_env(|name| std::env::var_os(name), Answers::query())
    }

    fn from_env(var: impl Fn(&str) -> Option<OsString>, answers: Answers) -> Self {
        let text = |name: &str| var(name).and_then(|value| known(value.as_bytes()));
        let program =
            answers.name.or_else(
                || match (text("TERM_PROGRAM"), text("TERM_PROGRAM_VERSION")) {
                    (Some(program), Some(version)) => Some(format!("{program} {version}")),
                    (Some(program), None) => Some(program),
                    (None, _) => TERMINAL_VARS
                        .iter()
                        .find(|(name, _)| var(name).is_some())
                        .map(|(_, terminal)| (*terminal).to_owned()),
                },
            );
        let multiplexer = [("TMUX", "tmux"), ("STY", "screen"), ("ZELLIJ", "zellij")]
            .into_iter()
            .find(|(name, _)| var(name).is_some())
            .map_or("none", |(_, multiplexer)| multiplexer);
        Self {
            program,
            term: text("TERM"),
            multiplexer,
            colors: ColorDepth::from_colorterm(var("COLORTERM").as_deref()),
            keyboard: answers.keyboard,
        }
    }
}

/// Whether the terminal speaks the kitty keyboard protocol, which the TUI turns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keyboard {
    Kitty,
    Legacy,
    /// The terminal gave no answer that could be read.
    Unknown,
    /// stdin is not a terminal, or noc runs in the background, so it was not asked.
    Unchecked,
}

impl Keyboard {
    fn name(self) -> &'static str {
        match self {
            Self::Kitty => "kitty",
            Self::Legacy => "legacy",
            Self::Unknown => UNKNOWN,
            Self::Unchecked => "unchecked",
        }
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

/// The command of F4, as `noc_tools` picks it.
fn editor_command(editor: &Editor) -> Option<String> {
    let mut words = vec![editor.program().to_string_lossy().into_owned()];
    words.extend(editor.args().iter().cloned());
    known(words.join(" ").as_bytes())
}

fn report(build: &Build, system: &System, terminal: &Terminal, editor: Option<&str>) -> String {
    let features = build.features_text();
    let colors = match terminal.colors {
        ColorDepth::TrueColor => "truecolor",
        ColorDepth::Indexed => "256",
    };
    let or_unknown = |value: &Option<String>| value.as_deref().unwrap_or(UNKNOWN).to_owned();
    let fields = [
        ("name", "Noon Commander".to_owned()),
        ("version", build.version.to_owned()),
        ("features", features),
        ("commit", build.commit.to_owned()),
        ("dirty", build.dirty.to_owned()),
        ("commit-time", build.commit_time.to_owned()),
        ("target", build.target.to_owned()),
        ("profile", build.profile.to_owned()),
        ("os", or_unknown(&system.os)),
        ("kernel", or_unknown(&system.kernel)),
        ("arch", or_unknown(&system.arch)),
        ("terminal", or_unknown(&terminal.program)),
        ("term", or_unknown(&terminal.term)),
        ("multiplexer", terminal.multiplexer.to_owned()),
        ("colors", colors.to_owned()),
        ("keyboard", terminal.keyboard.name().to_owned()),
        ("editor", editor.unwrap_or(UNKNOWN).to_owned()),
    ];
    let width = fields.iter().map(|(key, _)| key.len()).max().unwrap_or(0) + 2;
    let mut text = String::new();
    for (key, value) in fields {
        let _ = writeln!(text, "{key:<width$}{value}");
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;

    use noc_tools::editor::Editor;

    use super::{Answers, Build, ColorDepth, Keyboard, System, Terminal, editor_command, report};

    fn env_with(vars: &[(&str, &str)], answers: Answers) -> Terminal {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|&(name, value)| (name.to_owned(), OsString::from(value)))
            .collect();
        Terminal::from_env(|name| vars.get(name).cloned(), answers)
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
        insta::assert_snapshot!(report(&build, &system, &terminal, Some("nvim -p")));
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
