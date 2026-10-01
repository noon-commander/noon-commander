//! The command-line subcommands.

mod config;
mod hosts;
mod ls;

use std::path::Path;
use std::process::ExitCode;

use color_eyre::eyre::{Result, WrapErr as _};
use sftp_tui_config::{Config, Paths};
use sftp_tui_ssh::discovery::{Discovery, DiscoveryOptions, DiscoveryWarning, discover};
use sftp_tui_ssh::{SshSettings, Target};

use crate::cli::{Cli, Command, ConfigCommand};

/// Exit code after Ctrl-C, as shells report a process killed by SIGINT.
const INTERRUPTED: u8 = 130;

pub(crate) async fn run(cli: Cli) -> Result<ExitCode> {
    let paths = Paths::from_env()?;
    let config_path = cli.config.unwrap_or_else(|| paths.config_file());
    match cli.command {
        None => {
            eprintln!(
                "sftp-tui: the TUI is not implemented yet; try `sftp-tui hosts` or \
                 `sftp-tui ls <host>:<path>`"
            );
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Config(ConfigCommand::Init { force })) => config::init(&config_path, force),
        Some(Command::Config(ConfigCommand::Paths)) => Ok(config::paths(&paths, &config_path)),
        Some(Command::Hosts { resolve }) => {
            let context = Context::load(paths, &config_path)?;
            hosts::run(&context, resolve).await
        }
        Some(Command::Ls { location }) => {
            let context = Context::load(paths, &config_path)?;
            ls::run(&context, location.as_deref()).await
        }
    }
}

/// What the subcommands that talk to ssh share.
struct Context {
    paths: Paths,
    config: Config,
    settings: SshSettings,
}

impl Context {
    fn load(paths: Paths, config_path: &Path) -> Result<Self> {
        crate::logging::init(&paths);
        let config = Config::load(config_path, &paths.home)?;
        sftp_tui_ssh::args::validate(&config.ssh.args)
            .wrap_err_with(|| format!("invalid `ssh.args` in {}", config_path.display()))?;
        let settings = SshSettings {
            program: config.ssh.program.clone(),
            config_file: config.ssh.config_file.clone(),
            args: config.ssh.args.clone(),
            multiplex: config.ssh.multiplex,
        };
        Ok(Self {
            paths,
            config,
            settings,
        })
    }

    /// The ssh target for a host alias, with its `hosts.<alias>.args`.
    fn target(&self, alias: &str) -> Target {
        let args = self
            .config
            .hosts
            .get(alias)
            .map(|host| host.args.clone())
            .unwrap_or_default();
        Target::new(alias).with_args(args)
    }

    fn discovery_options(&self) -> DiscoveryOptions {
        DiscoveryOptions::new(self.paths.home.clone(), self.config.ssh.config_file.clone())
    }

    /// Scans the `ssh_config` files and prints what was skipped.
    async fn discover(&self) -> Result<Discovery> {
        let options = self.discovery_options();
        let discovery = tokio::task::spawn_blocking(move || {
            discover(&options, &|name| std::env::var(name).ok())
        })
        .await?;
        for warning in &discovery.warnings {
            eprintln!("warning: {}", describe(warning));
        }
        Ok(discovery)
    }

    /// Visible host aliases in config order.
    async fn host_aliases(&self) -> Result<Vec<String>> {
        let discovery = self.discover().await?;
        Ok(discovery
            .visible(&self.config.discovery.hide)
            .map(|host| host.alias.clone())
            .collect())
    }
}

fn describe(warning: &DiscoveryWarning) -> String {
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
