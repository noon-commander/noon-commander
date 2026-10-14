//! What `noc -V` and `noc --version` print. `-V` prints `noc` and the version, as clap does.
//! `--version` prints one fact per line, a key and then its value, for scripts and bug reports;
//! a value that cannot be found is `unknown`. The keys are an interface: keep them and their
//! order, and add new ones at the end.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal as _, Read as _, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use noc_tools::editor::Editor;
use rustix::termios::{OptionalActions, Termios, tcgetattr, tcgetpgrp, tcsetattr};

use crate::tui::ColorDepth;

const UNKNOWN: &str = "unknown";

/// Prints the version: the long report when `long`, else one line.
pub(crate) fn print(long: bool) -> ExitCode {
    // The terminal is in raw mode while it is asked, so the report is ready before printing.
    let text = if long {
        let editor = editor_command(&Editor::from_env());
        report(
            &Build::current(),
            &System::current(),
            &Terminal::current(),
            editor.as_deref(),
        )
    } else {
        format!("noc {}\n", env!("CARGO_PKG_VERSION"))
    };
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// What was decided when the binary was built.
struct Build {
    version: &'static str,
    /// Cargo features and whether they are on.
    features: &'static [(&'static str, bool)],
    commit: &'static str,
    dirty: &'static str,
    commit_time: &'static str,
    target: &'static str,
    profile: &'static str,
}

impl Build {
    fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            features: &[("forwarding", noc_ssh::policy::FORWARDING_ENABLED)],
            commit: env!("NOC_BUILD_COMMIT"),
            dirty: env!("NOC_BUILD_DIRTY"),
            commit_time: env!("NOC_BUILD_COMMIT_TIME"),
            target: env!("NOC_BUILD_TARGET"),
            profile: env!("NOC_BUILD_PROFILE"),
        }
    }
}

/// The system the binary runs on.
struct System {
    /// The distribution, or macOS and its version.
    os: Option<String>,
    kernel: Option<String>,
    arch: Option<String>,
}

impl System {
    fn current() -> Self {
        let uname = rustix::system::uname();
        let sysname = known(uname.sysname().to_bytes());
        let release = known(uname.release().to_bytes());
        let kernel = match (sysname, release) {
            (Some(sysname), Some(release)) => Some(format!("{sysname} {release}")),
            (sysname, _) => sysname,
        };
        Self {
            os: os_name(),
            kernel,
            arch: known(uname.machine().to_bytes()),
        }
    }
}

/// The macOS version, which `uname` does not give, or the Linux distribution.
fn os_name() -> Option<String> {
    if cfg!(target_os = "macos") {
        let plist =
            std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
        let name = plist_string(&plist, "ProductName")?;
        let version = plist_string(&plist, "ProductVersion")?;
        Some(format!("{name} {version}"))
    } else {
        ["/etc/os-release", "/usr/lib/os-release"]
            .into_iter()
            .find_map(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| os_release_name(&text))
    }
}

/// The `<string>` after `<key>key</key>` in a property list.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let tag = format!("<key>{key}</key>");
    let rest = plist[plist.find(&tag)? + tag.len()..].trim_start();
    let rest = rest.strip_prefix("<string>")?;
    known(&rest.as_bytes()[..rest.find("</string>")?])
}

/// `PRETTY_NAME` from `os-release`, or `NAME` where it is missing.
fn os_release_name(text: &str) -> Option<String> {
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
            .and_then(|value| known(unquote(value).as_bytes()))
    };
    value("PRETTY_NAME").or_else(|| value("NAME"))
}

/// An `os-release` value without its quotes and backslashes, as a shell would read it.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.to_owned();
    }
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    let mut text = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => text.extend(chars.next()),
            c => text.push(c),
        }
    }
    text
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
    /// `noc --version | pbcopy`. Not when stdin is not a terminal: then no one is there.
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
    let features = build
        .features
        .iter()
        .map(|&(name, on)| format!("{}{name}", if on { '+' } else { '-' }))
        .collect::<Vec<_>>()
        .join(" ");
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

/// Text from the system, made safe to print on one line; `None` when empty.
fn known(bytes: &[u8]) -> Option<String> {
    let text = noc_text::sanitize(bytes);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;

    use noc_tools::editor::Editor;

    use super::{
        Answers, Build, ColorDepth, Keyboard, System, Terminal, editor_command, os_release_name,
        plist_string, report,
    };

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

    #[test]
    fn macos_version_comes_from_the_property_list() {
        let plist = "<dict>\n\t<key>ProductName</key>\n\t<string>macOS</string>\n\t\
                     <key>ProductVersion</key>\n\t<string>26.0.1</string>\n</dict>";
        assert_eq!(plist_string(plist, "ProductName").as_deref(), Some("macOS"));
        assert_eq!(
            plist_string(plist, "ProductVersion").as_deref(),
            Some("26.0.1")
        );
        assert_eq!(plist_string(plist, "ProductBuildVersion"), None);
        assert_eq!(
            plist_string("<key>ProductName</key><true/>", "ProductName"),
            None
        );
        assert_eq!(plist_string("", "ProductName"), None);
    }

    #[test]
    fn linux_distribution_comes_from_os_release() {
        let text = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n";
        assert_eq!(os_release_name(text).as_deref(), Some("Ubuntu 24.04.1 LTS"));
        assert_eq!(os_release_name("NAME=Arch\n").as_deref(), Some("Arch"));
        assert_eq!(
            os_release_name("PRETTY_NAME=\"Say \\\"hi\\\"\"\n").as_deref(),
            Some("Say \"hi\"")
        );
        assert_eq!(
            os_release_name("PRETTY_NAME='C:\\\\'\n").as_deref(),
            Some("C:\\\\")
        );
        assert_eq!(
            os_release_name("PRETTY_NAME=\"\"\nNAME=Foo"),
            Some("Foo".to_owned())
        );
        assert_eq!(os_release_name(""), None);
    }
}
