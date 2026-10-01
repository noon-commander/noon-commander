//! Configuration for sftp-tui: XDG paths, the TOML schema, and built-in defaults.
//!
//! [`Paths`] resolves the directories sftp-tui uses, [`Config`] is the schema of `config.toml`,
//! and [`DEFAULT_CONFIG`] is the commented template that `sftp-tui config init` writes.

mod config;
mod error;
mod paths;

pub use config::{
    Config, DEFAULT_CONFIG, DiscoveryConfig, HostConfig, SshConfig, write_default_config,
};
pub use error::ConfigError;
pub use paths::Paths;

/// The application name, also used for its directories.
pub const APP_NAME: &str = "sftp-tui";
