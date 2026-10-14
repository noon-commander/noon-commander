//! `noc debug`: what bug reports need, and the ssh layer without the TUI.

mod hosts;
mod ls;
mod version;

use std::path::Path;
use std::process::ExitCode;

use color_eyre::eyre::Result;
use noc_config::Paths;
use noc_ssh::discovery::Discovery;

use crate::cli::DebugCommand;
use crate::context::{Context, describe};

pub(super) async fn run(
    command: DebugCommand,
    paths: Paths,
    config_path: &Path,
) -> Result<ExitCode> {
    match command {
        // It waits for the terminal's answers.
        DebugCommand::Version => Ok(tokio::task::spawn_blocking(version::print).await?),
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
