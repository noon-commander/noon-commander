//! Command-line interface.

use std::path::PathBuf;
#[cfg(debug_assertions)]
use std::time::Duration;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about)]
pub(crate) struct Cli {
    /// Use this configuration file instead of `~/.config/noc/config.toml`.
    #[arg(short, long, global = true, value_name = "FILE")]
    pub(crate) config: Option<PathBuf>,

    /// Debug builds only: hold back every result of background work, such as a directory
    /// read, by this many seconds, so spinners stay long enough to watch.
    #[cfg(debug_assertions)]
    #[arg(long, value_name = "SECONDS", value_parser = seconds)]
    pub(crate) slow_spinners: Option<Duration>,

    /// Debug builds only: hold back a connection made or ended by this many seconds more, so
    /// its spinners stay long enough to watch, while other results come as they are.
    #[cfg(debug_assertions)]
    #[arg(long, value_name = "SECONDS", value_parser = seconds)]
    pub(crate) slow_connections: Option<Duration>,

    /// Debug builds only: have a window take this many seconds to grow as it opens and to
    /// shrink as it closes, so each of its sizes can be watched.
    #[cfg(debug_assertions)]
    #[arg(long, value_name = "SECONDS", value_parser = seconds)]
    pub(crate) slow_windows: Option<Duration>,

    /// Debug builds only: write the size of each window, frame included, as `WIDTHxHEIGHT` on
    /// the bottom line of its frame.
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub(crate) show_sizes: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[cfg(debug_assertions)]
fn seconds(text: &str) -> Result<Duration, String> {
    let seconds: f64 = text.parse().map_err(|error| format!("{error}"))?;
    Duration::try_from_secs_f64(seconds).map_err(|error| format!("{error}"))
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Manage the configuration file.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Look into the build, the system, the ssh configuration, and connections.
    #[command(subcommand)]
    Debug(DebugCommand),
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

#[derive(Debug, Subcommand)]
pub(crate) enum DebugCommand {
    /// Print the version, build, system, terminal, editor, and zoxide, one `key value` line each.
    Version,
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
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }

    #[cfg(debug_assertions)]
    #[test]
    fn slow_modes_take_seconds() {
        use clap::Parser as _;

        let cli = Cli::try_parse_from(["noc", "--slow-spinners", "1.5"]).unwrap();
        assert_eq!(
            cli.slow_spinners,
            Some(std::time::Duration::from_millis(1500))
        );
        assert!(Cli::try_parse_from(["noc", "--slow-spinners", "-1"]).is_err());
        let cli = Cli::try_parse_from(["noc", "--slow-connections", "5"]).unwrap();
        assert_eq!(
            cli.slow_connections,
            Some(std::time::Duration::from_secs(5))
        );
        let cli = Cli::try_parse_from(["noc", "--slow-windows", "2"]).unwrap();
        assert_eq!(cli.slow_windows, Some(std::time::Duration::from_secs(2)));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn show_sizes_is_a_switch() {
        use clap::Parser as _;

        assert!(!Cli::try_parse_from(["noc"]).unwrap().show_sizes);
        assert!(
            Cli::try_parse_from(["noc", "--show-sizes"])
                .unwrap()
                .show_sizes
        );
        assert!(Cli::try_parse_from(["noc", "--show-sizes", "yes"]).is_err());
    }
}
