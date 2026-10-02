//! Configuration for Noon Commander: XDG paths, the TOML schema, and built-in defaults.
//!
//! [`Paths`] resolves the directories Noon Commander uses, [`Config`] is the schema of
//! `config.toml`, and [`DEFAULT_CONFIG`] is the commented template that `noc config init` writes.

mod config;
mod error;
mod paths;

pub use config::{
    Borders, Config, DEFAULT_CONFIG, DiscoveryConfig, HostConfig, SshConfig, TransferConfig,
    UiConfig, VolumesConfig, write_default_config,
};
pub use error::ConfigError;
pub use paths::Paths;

/// The application name, also used for its directories.
pub const APP_NAME: &str = "noc";
