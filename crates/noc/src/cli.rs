//! Command-line interface.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

// clap builds its version text on every run; ours asks the terminal, so only when wanted.
#[derive(Debug, Parser)]
#[command(about, disable_version_flag = true)]
pub(crate) struct Cli {
    /// Use this configuration file instead of `~/.config/noc/config.toml`.
    #[arg(short, long, global = true, value_name = "FILE")]
    pub(crate) config: Option<PathBuf>,

    /// Print version.
    #[arg(short = 'V')]
    pub(crate) short_version: bool,

    /// Print version, build, system, and terminal details, one `key value` line each.
    #[arg(long = "version")]
    pub(crate) long_version: bool,

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
        /// directory. Without it, lists what the location menu offers: the mount points of the
        /// volumes, and the hosts as `host:`.
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
    use clap::{CommandFactory, Parser as _};

    use super::Cli;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn short_and_long_version_are_different_flags() {
        let short = Cli::try_parse_from(["noc", "-V"]).unwrap();
        assert!(short.short_version && !short.long_version);
        let long = Cli::try_parse_from(["noc", "--version"]).unwrap();
        assert!(long.long_version && !long.short_version);
    }
}
