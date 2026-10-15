//! What the binary is, the system and the terminal it runs on, and the programs it runs: the
//! report that `noc debug version` prints and the About page of the help shows. Both list
//! [`Fact::ALL`], so a fact added here shows in both; the help names each in a `match` that a
//! new one does not compile without.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt as _;

use noc_tools::ToolError;
use noc_tools::editor::Editor;
use noc_tools::ripgrep::Ripgrep;
use noc_tools::zoxide::Zoxide;

use crate::tui::ColorDepth;

/// What was decided when the binary was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Build {
    pub(crate) version: &'static str,
    /// Cargo features and whether they are on.
    pub(crate) features: &'static [(&'static str, bool)],
    pub(crate) commit: &'static str,
    pub(crate) dirty: &'static str,
    pub(crate) commit_time: &'static str,
    pub(crate) target: &'static str,
    pub(crate) profile: &'static str,
    pub(crate) license: &'static str,
    pub(crate) repository: &'static str,
}

impl Build {
    pub(crate) fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            features: &[("forwarding", noc_ssh::policy::FORWARDING_ENABLED)],
            commit: env!("NOC_BUILD_COMMIT"),
            dirty: env!("NOC_BUILD_DIRTY"),
            commit_time: env!("NOC_BUILD_COMMIT_TIME"),
            target: env!("NOC_BUILD_TARGET"),
            profile: env!("NOC_BUILD_PROFILE"),
            license: env!("CARGO_PKG_LICENSE"),
            repository: env!("CARGO_PKG_REPOSITORY"),
        }
    }

    /// The features, each after `+` when it is on and `-` when it is off.
    pub(crate) fn features_text(&self) -> String {
        self.features
            .iter()
            .map(|&(name, on)| format!("{}{name}", if on { '+' } else { '-' }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What a fact of the report is about; the help shows each under a title of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Topic {
    /// The name and the version, which the help has in its first title.
    App,
    Build,
    System,
    Terminal,
    /// The programs other than ssh that Noon Commander runs.
    Tools,
}

/// A fact of the report. `noc debug version` prints them in the order of [`Fact::ALL`], one
/// per line after its [`key`](Self::key): the keys are an interface, so keep them and their
/// order, and add new ones at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fact {
    Name,
    Version,
    Features,
    Commit,
    Dirty,
    CommitTime,
    Target,
    Profile,
    Os,
    Kernel,
    Arch,
    Terminal,
    Term,
    Multiplexer,
    Colors,
    Keyboard,
    Editor,
    Zoxide,
    Ripgrep,
}

impl Fact {
    pub(crate) const ALL: [Self; 19] = [
        Self::Name,
        Self::Version,
        Self::Features,
        Self::Commit,
        Self::Dirty,
        Self::CommitTime,
        Self::Target,
        Self::Profile,
        Self::Os,
        Self::Kernel,
        Self::Arch,
        Self::Terminal,
        Self::Term,
        Self::Multiplexer,
        Self::Colors,
        Self::Keyboard,
        Self::Editor,
        Self::Zoxide,
        Self::Ripgrep,
    ];

    /// Its key in `noc debug version`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Version => "version",
            Self::Features => "features",
            Self::Commit => "commit",
            Self::Dirty => "dirty",
            Self::CommitTime => "commit-time",
            Self::Target => "target",
            Self::Profile => "profile",
            Self::Os => "os",
            Self::Kernel => "kernel",
            Self::Arch => "arch",
            Self::Terminal => "terminal",
            Self::Term => "term",
            Self::Multiplexer => "multiplexer",
            Self::Colors => "colors",
            Self::Keyboard => "keyboard",
            Self::Editor => "editor",
            Self::Zoxide => "zoxide",
            Self::Ripgrep => "ripgrep",
        }
    }

    pub(crate) fn topic(self) -> Topic {
        match self {
            Self::Name | Self::Version => Topic::App,
            Self::Features
            | Self::Commit
            | Self::Dirty
            | Self::CommitTime
            | Self::Target
            | Self::Profile => Topic::Build,
            Self::Os | Self::Kernel | Self::Arch => Topic::System,
            Self::Terminal | Self::Term | Self::Multiplexer | Self::Colors | Self::Keyboard => {
                Topic::Terminal
            }
            Self::Editor | Self::Zoxide | Self::Ripgrep => Topic::Tools,
        }
    }
}

/// The value of a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Text(String),
    /// It could not be found: `unknown`.
    Unknown,
    /// A program that is not there: `not found`.
    Missing,
}

impl Value {
    /// As `noc debug version` prints it.
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Unknown => UNKNOWN,
            Self::Missing => "not found",
        }
    }
}

/// What `build.rs` gives for what it could not find.
const UNKNOWN: &str = "unknown";

/// The report: everything [`Fact::ALL`] tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub(crate) build: Build,
    pub(crate) system: System,
    pub(crate) terminal: Terminal,
    /// The command of F4.
    pub(crate) editor: Option<String>,
    pub(crate) zoxide: Detected,
    /// The ripgrep of `g /`, for local directories.
    pub(crate) ripgrep: Detected,
}

impl Report {
    /// What is known of the build alone: the rest is unknown until it is found.
    pub(crate) fn new(build: Build) -> Self {
        Self {
            build,
            system: System::default(),
            terminal: Terminal::default(),
            editor: None,
            zoxide: Detected::Unchecked,
            ripgrep: Detected::Unchecked,
        }
    }

    pub(crate) fn value(&self, fact: Fact) -> Value {
        let build = &self.build;
        let text = |text: &str| {
            if text == UNKNOWN {
                Value::Unknown
            } else {
                Value::Text(text.to_owned())
            }
        };
        let known = |value: &Option<String>| value.as_deref().map_or(Value::Unknown, text);
        let terminal = &self.terminal;
        match fact {
            Fact::Name => text("Noon Commander"),
            Fact::Version => text(build.version),
            Fact::Features => text(&build.features_text()),
            Fact::Commit => text(build.commit),
            Fact::Dirty => text(build.dirty),
            Fact::CommitTime => text(build.commit_time),
            Fact::Target => text(build.target),
            Fact::Profile => text(build.profile),
            Fact::Os => known(&self.system.os),
            Fact::Kernel => known(&self.system.kernel),
            Fact::Arch => known(&self.system.arch),
            Fact::Terminal => known(&terminal.program),
            Fact::Term => known(&terminal.term),
            Fact::Multiplexer => text(terminal.multiplexer),
            Fact::Colors => text(match terminal.colors {
                ColorDepth::TrueColor => "truecolor",
                ColorDepth::Indexed => "256",
            }),
            Fact::Keyboard => match terminal.keyboard {
                Keyboard::Unknown => Value::Unknown,
                keyboard => text(keyboard.name()),
            },
            Fact::Editor => known(&self.editor),
            Fact::Zoxide => self.zoxide.value(),
            Fact::Ripgrep => self.ripgrep.value(),
        }
    }
}

/// What the terminal says of itself, and what it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Terminal {
    /// The name the terminal answers with, else `TERM_PROGRAM` and `TERM_PROGRAM_VERSION`,
    /// else a variable that only one terminal sets.
    pub(crate) program: Option<String>,
    pub(crate) term: Option<String>,
    pub(crate) multiplexer: &'static str,
    pub(crate) colors: ColorDepth,
    pub(crate) keyboard: Keyboard,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            program: None,
            term: None,
            multiplexer: UNKNOWN,
            colors: ColorDepth::Indexed,
            keyboard: Keyboard::Unknown,
        }
    }
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
    /// The terminal as the variables of `var` tell it, named `name` where it answered with
    /// its name, with `keyboard` as it answered.
    pub(crate) fn from_env(
        var: impl Fn(&str) -> Option<OsString>,
        name: Option<String>,
        keyboard: Keyboard,
    ) -> Self {
        let text = |name: &str| var(name).and_then(|value| known(value.as_bytes()));
        let program = name.or_else(
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
            keyboard,
        }
    }
}

/// Whether the terminal speaks the kitty keyboard protocol, which the TUI turns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keyboard {
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

/// The command of F4, as `noc_tools` picks it.
pub(crate) fn editor_command(editor: &Editor) -> Option<String> {
    let mut words = vec![editor.program().to_string_lossy().into_owned()];
    words.extend(editor.args().iter().cloned());
    known(words.join(" ").as_bytes())
}

/// Whether a program is there, and what it says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Detected {
    /// It says it is this, such as `zoxide 0.9.8`.
    Found(String),
    Missing,
    /// Not asked yet, or it did not say.
    Unchecked,
}

impl Detected {
    /// As the report has it.
    fn value(&self) -> Value {
        match self {
            Self::Found(version) => Value::Text(version.clone()),
            Self::Missing => Value::Missing,
            Self::Unchecked => Value::Unknown,
        }
    }

    /// What `answer`, the version `program` gave or why it gave none, says of it.
    fn of(program: &str, answer: Result<String, ToolError>) -> Self {
        match answer {
            Ok(version) => known(version.as_bytes()).map_or(Self::Unchecked, Self::Found),
            Err(error) if error.is_not_found() => Self::Missing,
            Err(error) => {
                tracing::info!("{program} does not say what it is: {error}");
                Self::Unchecked
            }
        }
    }
}

/// Asks `zoxide` what it is.
pub(crate) async fn detect_zoxide(zoxide: &Zoxide) -> Detected {
    Detected::of("zoxide", zoxide.version().await)
}

/// Asks `rg` what it is.
pub(crate) async fn detect_ripgrep(ripgrep: &Ripgrep) -> Detected {
    Detected::of("rg", ripgrep.version().await)
}

/// The system the binary runs on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct System {
    /// The distribution, or macOS and its version.
    pub(crate) os: Option<String>,
    pub(crate) kernel: Option<String>,
    pub(crate) arch: Option<String>,
}

impl System {
    /// Reads files on macOS and Linux: call it where blocking is allowed.
    pub(crate) fn current() -> Self {
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

/// Text from the system, made safe to print on one line; `None` when empty.
pub(crate) fn known(bytes: &[u8]) -> Option<String> {
    let text = noc_text::sanitize(bytes);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Fact, os_release_name, plist_string};

    #[test]
    fn every_fact_is_listed_once_with_a_key_of_its_own() {
        for (index, fact) in Fact::ALL.iter().enumerate() {
            assert_eq!(
                Fact::ALL.iter().position(|other| other == fact),
                Some(index)
            );
            let key = fact.key();
            assert_eq!(
                Fact::ALL.iter().filter(|other| other.key() == key).count(),
                1,
                "{key}"
            );
        }
        // A new fact does not compile without its place here, and then needs it in `ALL`.
        let place = |fact: Fact| match fact {
            Fact::Name => 0,
            Fact::Version => 1,
            Fact::Features => 2,
            Fact::Commit => 3,
            Fact::Dirty => 4,
            Fact::CommitTime => 5,
            Fact::Target => 6,
            Fact::Profile => 7,
            Fact::Os => 8,
            Fact::Kernel => 9,
            Fact::Arch => 10,
            Fact::Terminal => 11,
            Fact::Term => 12,
            Fact::Multiplexer => 13,
            Fact::Colors => 14,
            Fact::Keyboard => 15,
            Fact::Editor => 16,
            Fact::Zoxide => 17,
            Fact::Ripgrep => 18,
        };
        for (index, fact) in Fact::ALL.into_iter().enumerate() {
            assert_eq!(place(fact), index, "{fact:?}");
        }
        assert_eq!(
            place(Fact::Ripgrep) + 1,
            Fact::ALL.len(),
            "the last is last"
        );
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
