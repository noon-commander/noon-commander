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
pub struct UiConfig {
    /// Interface language as a language tag such as `en-US`, or `auto` for the system locale.
    /// Default: `auto`.
    pub language: String,
    /// A built-in color theme: `mc-classic` or `terminal`. Default: `mc-classic`.
    pub theme: String,
    /// The lines that frame panels and dialogs. Default: `double`.
    pub borders: Borders,
    /// Whether names get Nerd Font icons; otherwise mc's type markers (`/`, `*`, `@`, …).
    /// Needs a Nerd Font in the terminal. Default: `true`.
    pub icons: bool,
    /// Whether panels start out showing files whose names begin with a dot. Default: `true`.
    pub show_hidden: bool,
    /// Whether typing in a panel starts quick search; otherwise only Ctrl-S and Alt-S do, as
    /// in mc. Default: `true`.
    pub type_to_search: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            language: "auto".to_owned(),
            theme: "mc-classic".to_owned(),
            borders: Borders::default(),
            icons: true,
            show_hidden: true,
            type_to_search: true,
        }
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

    /// Expands a leading `~` or `~/` to `home` in [`SshConfig::program`] and
    /// [`SshConfig::config_file`]. `~user` is left alone.
    pub fn expand_tilde(&mut self, home: &Path) {
        expand_tilde_in(&mut self.ssh.program, home);
        if let Some(config_file) = &mut self.ssh.config_file {
            expand_tilde_in(config_file, home);
        }
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
        Borders, Config, DEFAULT_CONFIG, DiscoveryConfig, SshConfig, TransferConfig, UiConfig,
        VolumesConfig,
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
        type_to_search = false

        [transfer]
        atomic_upload = false
        parallel_jobs = 4
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
                type_to_search: false,
            },
            transfer: TransferConfig {
                atomic_upload: false,
                parallel_jobs: NonZeroUsize::new(4).unwrap(),
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
        assert!(config.ui.type_to_search);
        assert!(config.transfer.atomic_upload);
        assert_eq!(config.transfer.parallel_jobs.get(), 2);
    }

    #[test]
    fn at_least_one_job_runs() {
        let error = toml::from_str::<Config>("[transfer]\nparallel_jobs = 0").unwrap_err();
        assert!(error.to_string().contains("nonzero"), "{error}");
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
        config.expand_tilde(Path::new("/home/u"));
        assert_eq!(config.ssh.program.as_os_str(), "/home/u/bin/ssh");
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
        assert!(table["ui"].get("type_to_search").is_some());
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
