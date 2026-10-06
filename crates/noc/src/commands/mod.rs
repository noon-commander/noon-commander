//! The command-line subcommands.

mod config;
mod hosts;
mod keymap;
mod ls;

use std::process::ExitCode;

use color_eyre::eyre::{Result, WrapErr as _};
use noc_config::Paths;
use noc_ssh::discovery::Discovery;

use crate::cli::{Cli, Command, ConfigCommand, KeymapCommand};
use crate::context::{Context, describe};

/// Exit code after Ctrl+c, as shells report a process killed by SIGINT.
const INTERRUPTED: u8 = 130;

pub(crate) async fn run(cli: Cli) -> Result<ExitCode> {
    let paths = Paths::from_env()?;
    let config_path = cli.config.unwrap_or_else(|| paths.config_file());
    match cli.command {
        None => {
            let context = Context::load(paths, &config_path)?;
            crate::i18n::select(&context.config().ui.language);
            let start = std::env::current_dir().wrap_err("cannot read the current directory")?;
            crate::tui::run(context, start).await?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Config(ConfigCommand::Init { force })) => config::init(&config_path, force),
        Some(Command::Config(ConfigCommand::Paths)) => Ok(config::paths(&paths, &config_path)),
        Some(Command::Hosts { resolve }) => {
            let context = Context::load(paths, &config_path)?;
            hosts::run(&context, resolve).await
        }
        Some(Command::Keymap(KeymapCommand::Diff {
            left,
            right,
            all,
            color,
        })) => keymap::diff(&left, &right, all, color),
        Some(Command::Ls { location }) => {
            let context = Context::load(paths, &config_path)?;
            ls::run(&context, location.as_deref()).await
        }
    }
}

/// Scans the `ssh_config` files and prints what was skipped.
async fn discover(context: &Context) -> Result<Discovery> {
    let scanner = context.clone();
    let discovery = tokio::task::spawn_blocking(move || scanner.scan()).await?;
    for warning in &discovery.warnings {
        eprintln!("warning: {}", describe(warning));
    }
    Ok(discovery)
}

/// Visible host aliases in config order.
async fn host_aliases(context: &Context) -> Result<Vec<String>> {
    let discovery = discover(context).await?;
    Ok(discovery
        .visible(&context.config().discovery.hide)
        .map(|host| host.alias.clone())
        .collect())
}
