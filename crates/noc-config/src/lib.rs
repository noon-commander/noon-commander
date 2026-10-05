//! Configuration for Noon Commander: XDG paths, the TOML schema, and built-in defaults.
//!
//! [`Paths`] resolves the directories Noon Commander uses, [`Config`] is the schema of
//! `config.toml`, [`Hosts`] the schema of `hosts.toml`, and [`DEFAULT_CONFIG`] and
//! [`DEFAULT_HOSTS`] are the commented templates that `noc config init` writes.
//! [`save_config`] and [`save_host`] change the files for the TUI, keeping their comments.
//! [`Workspaces`] is the schema of `workspaces.toml`, the saved tabs of both panels, which
//! [`save_workspace`], [`remove_workspace`], and [`rename_workspace`] write whole.
//! [`History`] is the schema of `history.toml`, the commands of the command line, which
//! [`add_command`] and [`remove_command`] write whole.

mod config;
mod edit;
mod error;
mod history;
mod hosts;
mod paths;
mod workspaces;
mod write;

pub use config::{
    Borders, Config, DEFAULT_CONFIG, DiscoveryConfig, MenuBar, ShellConfig, SshConfig, TabBar,
    TransferConfig, UiConfig, VolumesConfig, Wheel, ZoxideConfig, write_default_config,
};
pub use edit::save_config;
pub use error::ConfigError;
pub use history::{History, HistoryEntry, add_command, remove_command};
pub use hosts::{
    DEFAULT_HOSTS, HostConfig, Hosts, SftpHost, local_dir, save_host, write_default_hosts,
};
pub use paths::Paths;
pub use workspaces::{
    PanelSide, Place, SavedTab, SortBy, Workspace, Workspaces, remove_workspace, rename_workspace,
    save_workspace,
};

/// The application name, also used for its directories.
pub const APP_NAME: &str = "noc";
