//! `noc debug`: what bug reports need, and the ssh layer without the TUI.

mod hosts;
mod ls;
mod version;

use std::path::Path;
use std::process::ExitCode;

use color_eyre::eyre::Result;
use noc_config::Paths;
use noc_ssh::discovery::Discovery;
use noc_tools::ripgrep::Ripgrep;
use noc_tools::zoxide::Zoxide;

use crate::cli::DebugCommand;
use crate::context::{Context, describe};

pub(super) async fn run(
    command: DebugCommand,
    paths: Paths,
    config_path: &Path,
) -> Result<ExitCode> {
    match command {
        DebugCommand::Version => {
            // zoxide and ripgrep as the settings name them, or by their names if they cannot be
            // read.
            let config = noc_config::Config::load(config_path, &paths.home).unwrap_or_default();
            let zoxide = Zoxide::new(config.zoxide.program);
            let ripgrep = Ripgrep::new(config.search.program);
            let tools = tokio::join!(
                crate::about::detect_zoxide(&zoxide),
                crate::about::detect_ripgrep(&ripgrep)
            );
            // It waits for the terminal's answers.
            Ok(tokio::task::spawn_blocking(move || version::print(tools)).await?)
        }
        DebugCommand::Hosts { resolve } => {
            let context = Context::load(paths, config_path)?;
            hosts::run(&context, resolve).await
        }
        DebugCommand::Ls { location } => {
            let context = Context::load(paths, config_path)?;
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
