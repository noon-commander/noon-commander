use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ConfigError;
use crate::write::write_new;

/// Commented default configuration written by `noc config init`.
///
/// It parses to [`Config::default()`].
pub const DEFAULT_CONFIG: &str = include_str!("../assets/default-config.toml");

/// Settings from `config.toml`. Every section and field is optional.
///
/// Unknown keys are errors, so a typo does not silently fall back to a default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// `[ssh]`: how the system OpenSSH client is run.
    pub ssh: SshConfig,
    /// `[discovery]`: which hosts from `ssh_config` are listed.
    pub discovery: DiscoveryConfig,
    /// `[volumes]`: which mounted volumes the root lists.
    pub volumes: VolumesConfig,
    /// `[ui]`: how the TUI looks.
    pub ui: UiConfig,
    /// `[transfer]`: how files are copied.
    pub transfer: TransferConfig,
    /// `[zoxide]`: the directories zoxide ranks, for jumping to them.
    pub zoxide: ZoxideConfig,
}

/// The `[zoxide]` section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ZoxideConfig {
    /// The zoxide program: a name looked up in `PATH`, or a path. Default: `zoxide`.
    pub program: PathBuf,
    /// Whether local directories where the user did something, such as copy, delete, or view a
    /// file, are added to zoxide; merely passing through does not count. Nothing happens
    /// without zoxide. Default: `true`.
    pub record: bool,
}

impl Default for ZoxideConfig {
    fn default() -> Self {
        Self {
            program: PathBuf::from("zoxide"),
            record: true,
        }
    }
}

/// The `[transfer]` section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TransferConfig {
    /// Whether a copy is written under a hidden temporary name next to its target and renamed
    /// when complete, so that the target never holds part of a file; otherwise the target is
    /// written directly. Default: `true`.
    pub atomic_upload: bool,
    /// How many jobs run at once; later ones wait their turn. Editing (F4) never waits.
    /// Default: `2`.
    pub parallel_jobs: NonZeroUsize,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            atomic_upload: true,
            parallel_jobs: NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN),
        }
    }
}

/// The `[ui]` section. Values are checked by the TUI, which knows its languages.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
// Each bool is a switch of its own in `config.toml`, not a state.
#[allow(clippy::struct_excessive_bools)]
pub struct UiConfig {
    /// Interface language as a language tag such as `en-US`, or `auto` for the system locale.
    /// Default: `auto`.
    pub language: String,
    /// A built-in color theme: `mc-classic`, `terminal`, `noon-dark`, `noon-light`,
    /// `catppuccin-mocha`, or `catppuccin-latte`. Default: `mc-classic`.
    pub theme: String,
    /// The lines that frame panels and dialogs. Default: `double`.
    pub borders: Borders,
    /// Whether names get Nerd Font icons; otherwise mc's type markers (`/`, `*`, `@`, …).
    /// Needs a Nerd Font in the terminal. Default: `true`.
    pub icons: bool,
    /// Whether panels start out showing files whose names begin with a dot. Default: `true`.
    pub show_hidden: bool,
    /// Whether quick search, the location menu, and the zoxide window match what is typed as
    /// fzf does: its characters in order, not necessarily together, best matches first.
    /// Otherwise names must start with it in quick search and contain it in the location menu,
    /// and zoxide matches it as keywords. Default: `true`.
    pub fuzzy_search: bool,
    /// When the menu bar of F9 shows. Default: `on-demand`.
    pub menu_bar: MenuBar,
    /// Where a panel with more than one tab shows them. Default: `line`.
    pub tab_bar: TabBar,
    /// Whether the mouse clicks and scrolls; the terminal then selects text only with a
    /// modifier, such as Shift or Option. Default: `true`.
    pub mouse: bool,
    /// How far the mouse wheel scrolls. Default: `3` lines.
    pub wheel: Wheel,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            language: "auto".to_owned(),
            theme: "mc-classic".to_owned(),
            borders: Borders::default(),
            icons: true,
            show_hidden: true,
            fuzzy_search: true,
            menu_bar: MenuBar::default(),
            tab_bar: TabBar::default(),
            mouse: true,
            wheel: Wheel::default(),
        }
    }
}

/// How far a step of the mouse wheel scrolls: `ui.wheel`, a number of lines or `"page"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    /// So many lines, from 1 to [`Wheel::MAX_LINES`].
    Lines(u8),
    /// A page, as the keys `PageUp` and `PageDown` move.
    Page,
}

impl Wheel {
    /// The most lines a step scrolls.
    pub const MAX_LINES: u8 = 20;

    /// The value of `ui.wheel` in `text`: a number of lines, or `page`.
    pub fn parse(text: &str) -> Option<Self> {
        if text == "page" {
            return Some(Self::Page);
        }
        let lines = text.parse::<u8>().ok()?;
        (1..=Self::MAX_LINES)
            .contains(&lines)
            .then_some(Self::Lines(lines))
    }
}

impl Default for Wheel {
    fn default() -> Self {
        Self::Lines(3)
    }
}

impl std::fmt::Display for Wheel {
    /// As `config.toml` writes it, without quotes.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lines(lines) => write!(formatter, "{lines}"),
            Self::Page => formatter.write_str("page"),
        }
    }
}

impl Serialize for Wheel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Lines(lines) => serializer.serialize_u8(*lines),
            Self::Page => serializer.serialize_str("page"),
        }
    }
}

impl<'de> Deserialize<'de> for Wheel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = Wheel;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    formatter,
                    "a number of lines from 1 to {}, or \"page\"",
                    Wheel::MAX_LINES
                )
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Wheel, E> {
                u8::try_from(value)
                    .ok()
                    .filter(|lines| (1..=Wheel::MAX_LINES).contains(lines))
                    .map(Wheel::Lines)
                    .ok_or_else(|| E::invalid_value(serde::de::Unexpected::Signed(value), &self))
            }

            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Wheel, E> {
                i64::try_from(value)
                    .map_err(|_| E::invalid_value(serde::de::Unexpected::Unsigned(value), &self))
                    .and_then(|value| self.visit_i64(value))
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Wheel, E> {
                match value {
                    "page" => Ok(Wheel::Page),
                    _ => Err(E::invalid_value(serde::de::Unexpected::Str(value), &self)),
                }
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// The lines that frame panels and dialogs: `ui.borders`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Borders {
    /// `═`, `║`, `╔`, …, as mc draws them.
    #[default]
    Double,
    /// `─`, `│`, `┌`, ….
    Single,
}

/// When the menu bar of F9 shows: `ui.menu_bar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MenuBar {
    /// Only while a menu is open, over the top line of the panels, as in Far Manager.
    #[default]
    OnDemand,
    /// Always, above the panels, as in mc.
    Always,
}

/// Where a panel with more than one tab shows them: `ui.tab_bar`. A panel with one tab shows
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabBar {
    /// On a line of its own above the panel, as in Total Commander.
    #[default]
    Line,
    /// In the top line of the panel's frame, in place of its title; it takes no room.
    Frame,
}

/// The `[ssh]` section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SshConfig {
    /// The ssh program: a name looked up in `PATH`, or a path. Default: `ssh`.
    pub program: PathBuf,
    /// Replaces `~/.ssh/config` and `/etc/ssh/ssh_config`, like `ssh -F`, for connections and
    /// host discovery. Default: none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_file: Option<PathBuf>,
    /// Extra arguments for every ssh invocation. Default: none.
    pub args: Vec<String>,
    /// Whether to share one `ControlMaster` connection per host. Default: `true`.
    pub multiplex: bool,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            program: PathBuf::from("ssh"),
            config_file: None,
            args: Vec::new(),
            multiplex: true,
        }
    }
}

/// The `[discovery]` section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscoveryConfig {
    /// Host aliases to leave out of the host list, as patterns with `*` and `?`. Default:
    /// `github.com`, `gitlab.com`, and `bitbucket.org`.
    pub hide: Vec<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            hide: ["github.com", "gitlab.com", "bitbucket.org"]
                .map(String::from)
                .into(),
        }
    }
}

/// The `[volumes]` section.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct VolumesConfig {
    /// Mount points to leave out of the root, as patterns with `*` and `?`, such as
    /// `/Volumes/Backup*`. The system volume is always listed. Default: none.
    pub hide: Vec<String>,
}

impl Config {
    /// Loads the configuration from `path`; a missing file yields the defaults.
    ///
    /// A leading `~` is expanded to `home`, see [`expand_tilde`](Self::expand_tilde).
    pub fn load(path: &Path, home: &Path) -> Result<Self, ConfigError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let mut config = Self::from_toml(&text, path)?;
        config.expand_tilde(home);
        Ok(config)
    }

    /// Parses TOML text; `origin` is only used in errors. Does not expand `~`.
    pub fn from_toml(text: &str, origin: &Path) -> Result<Self, ConfigError> {
        // Older versions kept the hosts here; say where they went rather than that the key is
        // unknown.
        if toml::from_str::<toml::Table>(text).is_ok_and(|table| table.contains_key("hosts")) {
            return Err(ConfigError::HostsMoved {
                path: origin.to_path_buf(),
            });
        }
        toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: origin.to_path_buf(),
            source: Box::new(source),
        })
    }

    /// Expands a leading `~` or `~/` to `home` in [`SshConfig::program`],
    /// [`SshConfig::config_file`], and [`ZoxideConfig::program`]. `~user` is left alone.
    pub fn expand_tilde(&mut self, home: &Path) {
        expand_tilde_in(&mut self.ssh.program, home);
        if let Some(config_file) = &mut self.ssh.config_file {
            expand_tilde_in(config_file, home);
        }
        expand_tilde_in(&mut self.zoxide.program, home);
    }
}

fn expand_tilde_in(path: &mut PathBuf, home: &Path) {
    // Matches whole components, so `~user` and `~foo/bar` do not count.
    let Ok(rest) = path.strip_prefix("~") else {
        return;
    };
    let expanded = if rest.as_os_str().is_empty() {
        home.to_path_buf()
    } else {
        home.join(rest)
    };
    *path = expanded;
}

/// Writes [`DEFAULT_CONFIG`] to `path`, creating parent directories.
///
/// Fails with [`ConfigError::AlreadyExists`] if `path` exists, unless `force` is set.
pub fn write_default_config(path: &Path, force: bool) -> Result<(), ConfigError> {
    write_new(path, DEFAULT_CONFIG, force)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::num::NonZeroUsize;
    use std::path::{Path, PathBuf};

    use super::{
        Borders, Config, DEFAULT_CONFIG, DiscoveryConfig, MenuBar, SshConfig, TabBar,
        TransferConfig, UiConfig, VolumesConfig, Wheel, ZoxideConfig,
    };
    use crate::{ConfigError, write_default_config};

    const ORIGIN: &str = "/cfg/noc/config.toml";

    fn parse(text: &str) -> Config {
        Config::from_toml(text, Path::new(ORIGIN)).unwrap()
    }

    fn assert_parse_error(text: &str) {
        match Config::from_toml(text, Path::new(ORIGIN)) {
            Err(ConfigError::Parse { path, .. }) => assert_eq!(path, Path::new(ORIGIN)),
            other => panic!("expected a parse error for {text:?}, got {other:?}"),
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    const FULL: &str = r#"
        [ssh]
        program = "/opt/homebrew/bin/ssh"
        config_file = "/etc/noc/ssh_config"
        args = ["-o", "ServerAliveInterval=15"]
        multiplex = false

        [discovery]
        hide = ["*.internal", "git?"]

        [volumes]
        hide = ["/Volumes/Backup*"]

        [ui]
        language = "de-DE"
        theme = "terminal"
        borders = "single"
        icons = false
        show_hidden = false
        fuzzy_search = false
        menu_bar = "always"
        tab_bar = "frame"
        mouse = false
        wheel = "page"

        [transfer]
        atomic_upload = false
        parallel_jobs = 4

        [zoxide]
        program = "/opt/homebrew/bin/zoxide"
        record = false
    "#;

    fn full() -> Config {
        Config {
            ssh: SshConfig {
                program: PathBuf::from("/opt/homebrew/bin/ssh"),
                config_file: Some(PathBuf::from("/etc/noc/ssh_config")),
                args: strings(&["-o", "ServerAliveInterval=15"]),
                multiplex: false,
            },
            discovery: DiscoveryConfig {
                hide: strings(&["*.internal", "git?"]),
            },
            volumes: VolumesConfig {
                hide: strings(&["/Volumes/Backup*"]),
            },
            ui: UiConfig {
                language: "de-DE".to_owned(),
                theme: "terminal".to_owned(),
                borders: Borders::Single,
                icons: false,
                show_hidden: false,
                fuzzy_search: false,
                menu_bar: MenuBar::Always,
                tab_bar: TabBar::Frame,
                mouse: false,
                wheel: Wheel::Page,
            },
            transfer: TransferConfig {
                atomic_upload: false,
                parallel_jobs: NonZeroUsize::new(4).unwrap(),
            },
            zoxide: ZoxideConfig {
                program: PathBuf::from("/opt/homebrew/bin/zoxide"),
                record: false,
            },
        }
    }

    #[test]
    fn defaults() {
        let config = Config::default();
        assert_eq!(config.ssh.program, Path::new("ssh"));
        assert_eq!(config.ssh.config_file, None);
        assert_eq!(config.ssh.args, [] as [String; 0]);
        assert!(config.ssh.multiplex);
        assert_eq!(
            config.discovery.hide,
            ["github.com", "gitlab.com", "bitbucket.org"]
        );
        assert_eq!(config.ui.language, "auto");
        assert_eq!(config.ui.theme, "mc-classic");
        assert_eq!(config.ui.borders, Borders::Double);
        assert!(config.ui.icons);
        assert!(config.ui.show_hidden);
        assert!(config.ui.fuzzy_search);
        assert_eq!(config.ui.menu_bar, MenuBar::OnDemand);
        assert_eq!(config.ui.tab_bar, TabBar::Line);
        assert!(config.ui.mouse);
        assert_eq!(config.ui.wheel, Wheel::Lines(3));
        assert!(config.transfer.atomic_upload);
        assert_eq!(config.transfer.parallel_jobs.get(), 2);
        assert_eq!(config.zoxide.program, Path::new("zoxide"));
        assert!(config.zoxide.record);
    }

    #[test]
    fn at_least_one_job_runs() {
        let error = toml::from_str::<Config>("[transfer]\nparallel_jobs = 0").unwrap_err();
        assert!(error.to_string().contains("nonzero"), "{error}");
    }

    #[test]
    fn the_wheel_scrolls_lines_or_a_page() {
        let wheel = |text: &str| parse(&format!("[ui]\nwheel = {text}")).ui.wheel;
        assert_eq!(wheel("1"), Wheel::Lines(1));
        assert_eq!(wheel("20"), Wheel::Lines(20));
        assert_eq!(wheel("\"page\""), Wheel::Page);
        for text in ["0", "21", "-1", "\"lines\"", "2.5", "true"] {
            assert_parse_error(&format!("[ui]\nwheel = {text}"));
        }
        assert_eq!(Wheel::parse("page"), Some(Wheel::Page));
        assert_eq!(Wheel::parse("5"), Some(Wheel::Lines(5)));
        assert_eq!(Wheel::parse("0"), None);
        assert_eq!(Wheel::Lines(7).to_string(), "7");
        assert_eq!(Wheel::Page.to_string(), "page");
    }

    #[test]
    fn empty_text_yields_the_defaults() {
        assert_eq!(parse(""), Config::default());
        assert_eq!(parse("# only a comment\n"), Config::default());
        assert_eq!(parse("[ssh]\n[discovery]\n"), Config::default());
    }

    #[test]
    fn full_example() {
        assert_eq!(parse(FULL), full());
    }

    #[test]
    fn missing_fields_keep_their_defaults() {
        let config = parse("[ssh]\nargs = [\"-v\"]\n");
        assert_eq!(
            config.ssh,
            SshConfig {
                args: strings(&["-v"]),
                ..SshConfig::default()
            }
        );
        assert_eq!(config.discovery, DiscoveryConfig::default());
    }

    #[test]
    fn an_empty_hide_list_shows_every_host() {
        assert_eq!(
            parse("[discovery]\nhide = []\n").discovery.hide,
            [] as [String; 0]
        );
    }

    #[test]
    fn unknown_keys_are_parse_errors() {
        for text in [
            "colour = true",
            "[unknown]\nkey = true",
            "[ssh]\nprogramm = \"ssh\"",
            "[discovery]\nshow = []",
        ] {
            assert_parse_error(text);
        }
    }

    #[test]
    fn wrong_types_are_parse_errors() {
        for text in [
            "ssh = \"ssh\"",
            "[ssh]\nprogram = 1",
            "[ssh]\nargs = \"-v\"",
            "[ssh]\nargs = [1]",
            "[ssh]\nmultiplex = \"yes\"",
            "[discovery]\nhide = \"github.com\"",
        ] {
            assert_parse_error(text);
        }
    }

    #[test]
    fn invalid_toml_is_a_parse_error() {
        assert_parse_error("[ssh");
        assert_parse_error("[ssh]\nmultiplex = true\nmultiplex = false");
    }

    #[test]
    fn hosts_moved_to_their_own_file() {
        for text in ["[hosts.web]\nlabel = \"Web\"", "[hosts]\n"] {
            match Config::from_toml(text, Path::new(ORIGIN)) {
                Err(ConfigError::HostsMoved { path }) => assert_eq!(path, Path::new(ORIGIN)),
                other => panic!("expected HostsMoved for {text:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn expand_tilde_expands_program_and_config_file() {
        let mut config = full();
        config.ssh.program = PathBuf::from("~/bin/ssh");
        config.ssh.config_file = Some(PathBuf::from("~/.ssh/work_config"));
        config.zoxide.program = PathBuf::from("~/.cargo/bin/zoxide");
        config.expand_tilde(Path::new("/home/u"));
        assert_eq!(config.ssh.program.as_os_str(), "/home/u/bin/ssh");
        assert_eq!(
            config.zoxide.program.as_os_str(),
            "/home/u/.cargo/bin/zoxide"
        );
        assert_eq!(
            config.ssh.config_file.as_deref().map(Path::as_os_str),
            Some(OsStr::new("/home/u/.ssh/work_config"))
        );
        // Nothing else changes.
        assert_eq!(
            config,
            Config {
                ssh: SshConfig {
                    program: PathBuf::from("/home/u/bin/ssh"),
                    config_file: Some(PathBuf::from("/home/u/.ssh/work_config")),
                    ..full().ssh
                },
                zoxide: ZoxideConfig {
                    program: PathBuf::from("/home/u/.cargo/bin/zoxide"),
                    ..full().zoxide
                },
                ..full()
            }
        );
    }

    #[test]
    fn expand_tilde_only_touches_a_leading_tilde_component() {
        for (input, expected) in [
            ("~", "/home/u"),
            ("~/", "/home/u"),
            ("~/bin/ssh", "/home/u/bin/ssh"),
            ("ssh", "ssh"),
            ("/usr/bin/ssh", "/usr/bin/ssh"),
            ("~user/bin/ssh", "~user/bin/ssh"),
            ("~~/ssh", "~~/ssh"),
            ("bin/~/ssh", "bin/~/ssh"),
            ("/~/ssh", "/~/ssh"),
        ] {
            let mut config = Config::default();
            config.ssh.program = PathBuf::from(input);
            config.ssh.config_file = Some(PathBuf::from(input));
            config.expand_tilde(Path::new("/home/u"));
            assert_eq!(config.ssh.program.as_os_str(), expected, "{input}");
            assert_eq!(
                config.ssh.config_file.as_deref().map(Path::as_os_str),
                Some(OsStr::new(expected)),
                "{input}"
            );
        }
    }

    #[test]
    fn expand_tilde_keeps_a_missing_config_file() {
        let mut config = Config::default();
        config.expand_tilde(Path::new("/home/u"));
        assert_eq!(config, Config::default());
    }

    #[test]
    fn default_config_parses_to_the_defaults() {
        assert_eq!(parse(DEFAULT_CONFIG), Config::default());
        // The defaults are spelled out, not just implied by missing keys.
        let table: toml::Table = toml::from_str(DEFAULT_CONFIG).unwrap();
        for key in ["program", "args", "multiplex"] {
            assert!(table["ssh"].get(key).is_some(), "ssh.{key}");
        }
        assert!(table["discovery"].get("hide").is_some());
        assert!(table["volumes"].get("hide").is_some());
        assert!(table["ui"].get("language").is_some());
        assert!(table["ui"].get("theme").is_some());
        assert!(table["ui"].get("borders").is_some());
        assert!(table["ui"].get("icons").is_some());
        assert!(table["ui"].get("show_hidden").is_some());
        assert!(table["ui"].get("fuzzy_search").is_some());
        assert!(table["ui"].get("menu_bar").is_some());
        assert!(table["ui"].get("tab_bar").is_some());
        assert!(table["ui"].get("mouse").is_some());
        assert!(table["ui"].get("wheel").is_some());
        assert!(table["zoxide"].get("program").is_some());
        assert!(table["zoxide"].get("record").is_some());
    }

    #[test]
    fn default_config_examples_are_valid() {
        // Uncomments `# key = value` and `# [table]` lines, as a user would.
        let uncommented: Vec<&str> = DEFAULT_CONFIG
            .lines()
            .map(|line| match line.strip_prefix("# ") {
                Some(rest)
                    if rest.starts_with('[')
                        || rest.split_once(" = ").is_some_and(|(key, _)| {
                            key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                        }) =>
                {
                    rest
                }
                _ => line,
            })
            .collect();
        let config = parse(&uncommented.join("\n"));
        assert!(config.ssh.config_file.is_some());
        assert_eq!(
            Config {
                ssh: SshConfig {
                    config_file: None,
                    ..config.ssh
                },
                ..config
            },
            Config::default()
        );
    }

    #[test]
    fn serialization_round_trips() {
        for config in [Config::default(), full()] {
            let text = toml::to_string(&config).unwrap();
            assert_eq!(parse(&text), config, "{text}");
        }
    }

    #[test]
    fn load_of_a_missing_file_yields_the_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("missing/config.toml");
        assert_eq!(
            Config::load(&path, Path::new("/home/u")).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn load_parses_the_file_and_expands_tilde() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        fs::write(
            &path,
            "[ssh]\nprogram = \"~/bin/ssh\"\nconfig_file = \"~/.ssh/work_config\"\n",
        )
        .unwrap();
        let config = Config::load(&path, Path::new("/home/u")).unwrap();
        assert_eq!(config.ssh.program.as_os_str(), "/home/u/bin/ssh");
        assert_eq!(
            config.ssh.config_file.as_deref(),
            Some(Path::new("/home/u/.ssh/work_config"))
        );
    }

    #[test]
    fn load_reports_errors_with_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        // A directory can be opened but not read.
        match Config::load(tmp.path(), Path::new("/home/u")) {
            Err(ConfigError::Read { path, .. }) => assert_eq!(path, tmp.path()),
            other => panic!("expected a read error, got {other:?}"),
        }
        let file = tmp.path().join("config.toml");
        fs::write(&file, "[ssh]\nmultiplex = \"yes\"\n").unwrap();
        match Config::load(&file, Path::new("/home/u")) {
            Err(ConfigError::Parse { path, .. }) => assert_eq!(path, file),
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[test]
    fn write_default_config_creates_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config/noc/config.toml");
        write_default_config(&path, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);
        assert_eq!(
            Config::load(&path, Path::new("/home/u")).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn write_default_config_refuses_to_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        fs::write(&path, "# mine\n").unwrap();
        match write_default_config(&path, false) {
            Err(ConfigError::AlreadyExists { path: existing }) => assert_eq!(existing, path),
            other => panic!("expected AlreadyExists, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), "# mine\n");
    }

    #[test]
    fn write_default_config_overwrites_with_force() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        write_default_config(&path, true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);
        // Longer than the template, so a missing truncation would leave a tail behind.
        fs::write(&path, "#".repeat(DEFAULT_CONFIG.len() * 2)).unwrap();
        write_default_config(&path, true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);
    }

    #[test]
    fn write_default_config_reports_unwritable_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("file");
        fs::write(&blocker, "").unwrap();
        match write_default_config(&blocker.join("config.toml"), false) {
            Err(ConfigError::Write { path, .. }) => assert_eq!(path, blocker),
            other => panic!("expected a write error, got {other:?}"),
        }
    }
}
