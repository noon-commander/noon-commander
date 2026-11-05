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
            crate::tui::check_terminal()?;
            // Only the TUI logs: the subcommands have the terminal to say what they do.
            let (paths, config) = (context.paths.clone(), context.config());
            let log =
                tokio::task::spawn_blocking(move || crate::logging::start(&paths, &config.log))
                    .await?;
            crate::i18n::select(&context.config().ui.language);
            let start = std::env::current_dir().wrap_err("cannot read the current directory")?;
            let ran = Box::pin(crate::tui::run(
                context,
                start,
                &log.file,
                #[cfg(debug_assertions)]
                crate::tui::DebugOptions {
                    slow_spinners: cli.slow_spinners,
                    slow_connections: cli.slow_connections,
                    slow_windows: cli.slow_windows,
                    show_sizes: cli.show_sizes,
                },
            ))
            .await;
            // Ending the log waits for the last lines to reach the file.
            let _ = tokio::task::spawn_blocking(move || drop(log)).await;
            ran?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Config(ConfigCommand::Init { force })) => config::init(&config_path, force),
        Some(Command::Config(ConfigCommand::Paths)) => Ok(config::paths(&paths, &config_path)),
        Some(Command::Debug(command)) => debug::run(command, paths, &config_path).await,
    }
}
