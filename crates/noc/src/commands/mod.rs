//! The command-line subcommands.

mod config;
mod debug;

use std::process::ExitCode;

use color_eyre::eyre::{Result, WrapErr as _};
use noc_config::Paths;

use crate::cli::{Cli, Command, ConfigCommand};
use crate::context::Context;

pub(crate) async fn run(cli: Cli) -> Result<ExitCode> {
    let paths = Paths::from_env()?;
    let config_path = cli.config.unwrap_or_else(|| paths.config_file());
    match cli.command {
        None => {
            let context = Context::load(paths, &config_path)?;
            crate::i18n::select(&context.config().ui.language);
            let start = std::env::current_dir().wrap_err("cannot read the current directory")?;
            crate::tui::run(
                context,
                start,
                #[cfg(debug_assertions)]
                crate::tui::Slow {
                    spinners: cli.slow_spinners,
                    connections: cli.slow_connections,
                    windows: cli.slow_windows,
                },
            )
            .await?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Config(ConfigCommand::Init { force })) => config::init(&config_path, force),
        Some(Command::Config(ConfigCommand::Paths)) => Ok(config::paths(&paths, &config_path)),
        Some(Command::Debug(command)) => debug::run(command, paths, &config_path).await,
    }
}
