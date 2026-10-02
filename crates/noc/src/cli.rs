//! Command-line interface.

use std::path::PathBuf;
use std::sync::LazyLock;

use clap::{Parser, Subcommand};

/// Package version plus compile-time features, e.g. `0.1.0 (-forwarding)`.
static VERSION: LazyLock<String> = LazyLock::new(|| {
    let forwarding = if noc_ssh::policy::FORWARDING_ENABLED {
        '+'
    } else {
        '-'
    };
    format!("{} ({forwarding}forwarding)", env!("CARGO_PKG_VERSION"))
});

#[derive(Debug, Parser)]
#[command(version = VERSION.as_str(), about)]
pub(crate) struct Cli {
    /// Use this configuration file instead of `~/.config/noc/config.toml`.
    #[arg(short, long, global = true, value_name = "FILE")]
    pub(crate) config: Option<PathBuf>,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// List the hosts from `ssh_config`.
    Hosts {
        /// Run `ssh -G` for every host to show and cache where it connects to. This also runs
        /// the commands of `Match exec` lines in `ssh_config`. Without it, addresses come from
        /// the cache of earlier runs, as long as the ssh configuration has not changed.
        #[arg(long)]
        resolve: bool,
    },
    /// List a directory.
    Ls {
        /// A local path, or `host:path` for a remote one; `host:` is the remote home
        /// directory. Without it, lists the virtual root: the local file system and the hosts.
        location: Option<String>,
    },
    /// Manage the configuration file.
    #[command(subcommand)]
    Config(ConfigCommand),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Write a commented configuration file with the default settings.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Show the files and directories Noon Commander uses.
    Paths,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }
}
