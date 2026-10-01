//! What the subcommands and the TUI share: directories, settings, and the ssh configuration.

use std::path::Path;

use color_eyre::eyre::{Result, WrapErr as _, bail};
use sftp_tui_config::{Config, Paths};
use sftp_tui_ssh::discovery::{Discovery, DiscoveryOptions, DiscoveryWarning, discover};
use sftp_tui_ssh::{ConfigStamp, ResolveCache, SshSettings, Target};

/// The loaded configuration.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    pub(crate) paths: Paths,
    pub(crate) config: Config,
    pub(crate) settings: SshSettings,
}

impl Context {
    /// Starts logging, then reads and validates the config file at `config_path`.
    pub(crate) fn load(paths: Paths, config_path: &Path) -> Result<Self> {
        crate::logging::init(&paths);
        let config = Config::load(config_path, &paths.home)?;
        sftp_tui_ssh::args::validate(&config.ssh.args)
            .wrap_err_with(|| format!("invalid `ssh.args` in {}", config_path.display()))?;
        if !crate::i18n::is_valid_language(&config.ui.language) {
            bail!(
                "invalid `ui.language` in {}: `{}` is not `auto` or a language tag such as \
                 `en-US`",
                config_path.display(),
                config.ui.language
            );
        }
        if !crate::tui::is_valid_theme(&config.ui.theme) {
            bail!(
                "invalid `ui.theme` in {}: `{}` is not one of {}",
                config_path.display(),
                config.ui.theme,
                crate::tui::theme_names().join(", ")
            );
        }
        Ok(Self::new(paths, config))
    }

    /// A context for a config that is already valid.
    pub(crate) fn new(paths: Paths, config: Config) -> Self {
        let settings = SshSettings {
            program: config.ssh.program.clone(),
            config_file: config.ssh.config_file.clone(),
            args: config.ssh.args.clone(),
            multiplex: config.ssh.multiplex,
        };
        Self {
            paths,
            config,
            settings,
        }
    }

    /// The ssh target for a host alias, with its `hosts.<alias>.args`.
    pub(crate) fn target(&self, alias: &str) -> Target {
        let args = self
            .config
            .hosts
            .get(alias)
            .map(|host| host.args.clone())
            .unwrap_or_default();
        Target::new(alias).with_args(args)
    }

    /// `hosts.<alias>.label`.
    pub(crate) fn label(&self, alias: &str) -> Option<&str> {
        self.config.hosts.get(alias)?.label.as_deref()
    }

    pub(crate) fn discovery_options(&self) -> DiscoveryOptions {
        DiscoveryOptions::new(self.paths.home.clone(), self.config.ssh.config_file.clone())
    }

    /// Scans the `ssh_config` files for hosts.
    ///
    /// Blocking: call it from a blocking thread.
    pub(crate) fn scan(&self) -> Discovery {
        discover(&self.discovery_options(), &|name| std::env::var(name).ok())
    }

    /// The `ssh -G` cache, valid for the files `discovery` read and for the ssh settings.
    ///
    /// Blocking: call it from a blocking thread.
    pub(crate) fn load_cache(&self, discovery: &Discovery) -> ResolveCache {
        let mut files = discovery.files.clone();
        files.extend(self.discovery_options().root_files());
        let path = self.paths.cache_dir.join("resolve.json");
        ResolveCache::load(path, ConfigStamp::read(&files, &self.settings))
    }
}

/// A discovery warning in words, for messages and logs.
pub(crate) fn describe(warning: &DiscoveryWarning) -> String {
    match warning {
        DiscoveryWarning::Unreadable { file, error } => {
            format!("cannot read {}: {error}", file.display())
        }
        DiscoveryWarning::IncludeWithTokens { file, line, path } => format!(
            "{}:{line}: skipped `Include {path}`: ssh expands % tokens only for a given host",
            file.display()
        ),
        DiscoveryWarning::UndefinedVariable { file, line, name } => format!(
            "{}:{line}: skipped an Include: the environment variable `{name}` is not set",
            file.display()
        ),
        DiscoveryWarning::IncludeTooDeep { file, line } => {
            format!("{}:{line}: Include is nested too deeply", file.display())
        }
    }
}
