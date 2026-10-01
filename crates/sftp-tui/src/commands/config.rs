//! `sftp-tui config`.

use std::path::Path;
use std::process::ExitCode;

use color_eyre::eyre::Result;
use sftp_tui_config::{Paths, write_default_config};

pub(super) fn init(path: &Path, force: bool) -> Result<ExitCode> {
    write_default_config(path, force)?;
    println!("wrote {}", path.display());
    Ok(ExitCode::SUCCESS)
}

pub(super) fn paths(paths: &Paths, config_file: &Path) -> ExitCode {
    let rows = [
        ("config file", config_file),
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
