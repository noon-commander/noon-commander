//! What the subcommands and the TUI share: directories, settings, and the ssh configuration.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use color_eyre::eyre::{Result, WrapErr as _, bail};
use noc_config::{Config, ConfigError, Hosts, Paths};
use noc_ssh::discovery::{Discovery, DiscoveryOptions, DiscoveryWarning, discover};
use noc_ssh::{ConfigStamp, ResolveCache, SshSettings};
use noc_tools::zoxide::Zoxide;

/// The loaded configuration.
#[derive(Debug)]
pub(crate) struct Context {
    pub(crate) paths: Paths,
    /// The settings, which the Configuration dialog changes while the TUI runs.
    config: RwLock<Arc<Config>>,
    /// `config.toml`, or the file of `--config`, which the Configuration dialog changes.
    pub(crate) config_file: PathBuf,
    /// `hosts.toml`, next to the config file.
    pub(crate) hosts_file: PathBuf,
    /// The settings from `hosts_file`, which the TUI changes while it runs.
    hosts: RwLock<Arc<Hosts>>,
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Self {
            paths: self.paths.clone(),
            config: RwLock::new(self.config()),
            config_file: self.config_file.clone(),
            hosts_file: self.hosts_file.clone(),
            hosts: RwLock::new(self.hosts()),
        }
    }
}

impl Context {
    /// Starts logging, then reads and validates the config file at `config_path` and the
    /// host settings next to it.
    pub(crate) fn load(paths: Paths, config_path: &Path) -> Result<Self> {
        crate::logging::init(&paths);
        let config = Config::load(config_path, &paths.home)?;
        noc_ssh::args::validate(&config.ssh.args)
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
        let hosts = Hosts::load(&Paths::hosts_file(config_path))?;
        Ok(Self::new(paths, config, config_path.to_path_buf(), hosts))
    }

    /// A context for a config from `config_file` that is already valid; the host settings
    /// are from `hosts.toml` next to it.
    pub(crate) fn new(paths: Paths, config: Config, config_file: PathBuf, hosts: Hosts) -> Self {
        Self {
            paths,
            config: RwLock::new(Arc::new(config)),
            hosts_file: Paths::hosts_file(&config_file),
            config_file,
            hosts: RwLock::new(Arc::new(hosts)),
        }
    }

    /// The settings as they are now.
    pub(crate) fn config(&self) -> Arc<Config> {
        // The lock guards a pointer swap, which cannot leave it half done.
        Arc::clone(&self.config.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Uses `config`, which is valid and has `~` expanded, from now on: new connections, `ssh
    /// -G`, and listings of the hosts and the volumes follow it.
    pub(crate) fn set_config(&self, config: Config) {
        *self.config.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(config);
    }

    /// How ssh is run now.
    pub(crate) fn settings(&self) -> SshSettings {
        let ssh = &self.config().ssh;
        SshSettings {
            program: ssh.program.clone(),
            config_file: ssh.config_file.clone(),
            args: ssh.args.clone(),
            multiplex: ssh.multiplex,
            // The working directory follows the active panel; ssh must not hold it.
            work_dir: Some(self.paths.home.clone()),
        }
    }

    /// How zoxide is run now.
    pub(crate) fn zoxide(&self) -> Zoxide {
        Zoxide::new(self.config().zoxide.program.clone())
    }

    /// The host settings as they are now.
    pub(crate) fn hosts(&self) -> Arc<Hosts> {
        // The lock guards a pointer swap, which cannot leave it half done.
        Arc::clone(&self.hosts.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Reads `hosts_file` again and uses what it says from now on.
    ///
    /// Blocking: call it from a blocking thread.
    pub(crate) fn reload_hosts(&self) -> Result<(), ConfigError> {
        let hosts = Arc::new(Hosts::load(&self.hosts_file)?);
        *self.hosts.write().unwrap_or_else(PoisonError::into_inner) = hosts;
        Ok(())
    }

    /// The label of a host from `hosts.toml`.
    pub(crate) fn label(&self, alias: &str) -> Option<String> {
        self.hosts().get(alias)?.label().map(str::to_owned)
    }

    pub(crate) fn discovery_options(&self) -> DiscoveryOptions {
        DiscoveryOptions::new(
            self.paths.home.clone(),
            self.config().ssh.config_file.clone(),
        )
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
        ResolveCache::load(path, ConfigStamp::read(&files, &self.settings()))
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn a_new_config_takes_over_for_ssh_and_listings() {
        let paths = Paths::resolve(Path::new("/home/me"), 501, &|_| None);
        let context = Context::new(
            paths,
            Config::default(),
            PathBuf::from("/home/me/.config/noc/config.toml"),
            Hosts::default(),
        );
        assert_eq!(context.settings().program, Path::new("ssh"));
        assert_eq!(
            context.hosts_file,
            Path::new("/home/me/.config/noc/hosts.toml")
        );
        let mut config = Config::default();
        config.ssh.program = PathBuf::from("/opt/bin/ssh");
        config.ssh.multiplex = false;
        config.volumes.hide = vec!["/mnt/*".to_owned()];
        context.set_config(config);
        let settings = context.settings();
        assert_eq!(settings.program, Path::new("/opt/bin/ssh"));
        assert!(!settings.multiplex);
        assert_eq!(context.config().volumes.hide, ["/mnt/*"]);
        assert_eq!(context.clone().config().volumes.hide, ["/mnt/*"]);
    }
}
