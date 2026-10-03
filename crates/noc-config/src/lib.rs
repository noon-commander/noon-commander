//! Configuration for Noon Commander: XDG paths, the TOML schema, and built-in defaults.
//!
//! [`Paths`] resolves the directories Noon Commander uses, [`Config`] is the schema of
//! `config.toml`, [`Hosts`] the schema of `hosts.toml`, and [`DEFAULT_CONFIG`] and
//! [`DEFAULT_HOSTS`] are the commented templates that `noc config init` writes.
//! [`save_config`] and [`save_host`] change the files for the TUI, keeping their comments.

mod config;
mod edit;
mod error;
mod hosts;
mod paths;
mod write;

pub use config::{
    Borders, Config, DEFAULT_CONFIG, DiscoveryConfig, MenuBar, SshConfig, TabBar, TransferConfig,
    UiConfig, VolumesConfig, ZoxideConfig, write_default_config,
};
pub use edit::save_config;
pub use error::ConfigError;
pub use hosts::{
    DEFAULT_HOSTS, HostConfig, Hosts, SftpHost, local_dir, save_host, write_default_hosts,
};
pub use paths::Paths;

/// The application name, also used for its directories.
pub const APP_NAME: &str = "noc";
