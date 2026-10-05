//! `noc config`.

use std::path::Path;
use std::process::ExitCode;

use color_eyre::eyre::Result;
use noc_config::{ConfigError, Paths, write_default_config, write_default_hosts};

/// Writes the config file, and the hosts file next to it unless it exists; `force` overwrites
/// both.
pub(super) fn init(path: &Path, force: bool) -> Result<ExitCode> {
    write_default_config(path, force)?;
    println!("wrote {}", path.display());
    let hosts = Paths::hosts_file(path);
    match write_default_hosts(&hosts, force) {
        Ok(()) => println!("wrote {}", hosts.display()),
        Err(ConfigError::AlreadyExists { .. }) => println!("kept {}", hosts.display()),
        Err(error) => return Err(error.into()),
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn paths(paths: &Paths, config_file: &Path) -> ExitCode {
    let hosts_file = Paths::hosts_file(config_file);
    let workspaces_file = paths.workspaces_file();
    let history_file = paths.history_file();
    let rows = [
        ("config file", config_file),
        ("hosts file", &hosts_file),
        ("workspaces", &workspaces_file),
        ("history", &history_file),
        ("config", &paths.config_dir),
        ("data", &paths.data_dir),
        ("state, logs", &paths.state_dir),
        ("cache", &paths.cache_dir),
        ("runtime", &paths.runtime_dir),
    ];
    for (name, path) in rows {
        println!("{name:<12} {}", path.display());
    }
    ExitCode::SUCCESS
}
