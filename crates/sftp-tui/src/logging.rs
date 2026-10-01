//! Logging to a file in the XDG state directory; the terminal belongs to the UI.

use std::fs::{self, OpenOptions};
use std::sync::Mutex;

use sftp_tui_config::Paths;
use tracing_subscriber::EnvFilter;

/// Environment variable with the log filter, e.g. `debug` or `sftp_tui_ssh=trace`.
const FILTER_ENV: &str = "SFTP_TUI_LOG";

/// Logs to `<state dir>/sftp-tui.log`. Without a writable state directory, nothing is logged.
pub(crate) fn init(paths: &Paths) {
    let filter = EnvFilter::try_from_env(FILTER_ENV).unwrap_or_else(|_| EnvFilter::new("info"));
    if fs::create_dir_all(&paths.state_dir).is_err() {
        return;
    }
    let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.state_dir.join("sftp-tui.log"))
    else {
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init();
}
