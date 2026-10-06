//! `noc keymap`: the built-in keymaps.

use std::io::{IsTerminal as _, Write as _};
use std::process::ExitCode;

use clap::ColorChoice;
use color_eyre::eyre::{Result, eyre};

/// Prints the built-in keymaps `left` and `right` compared action by action.
pub(crate) fn diff(left: &str, right: &str, all: bool, color: ColorChoice) -> Result<ExitCode> {
    let color = match color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
                && std::env::var_os("TERM").is_none_or(|term| term != "dumb")
        }
    };
    let text = crate::tui::keymap_diff(left, right, all, color)
        .ok_or_else(|| eyre!("no such keymap: `{left}` or `{right}`"))?;
    // A closed pipe, as of `| head`, is no error.
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
    Ok(ExitCode::SUCCESS)
}
