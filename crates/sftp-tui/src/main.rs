//! sftp-tui: a Midnight Commander-style SFTP file manager built on the system OpenSSH client.

use std::process::ExitCode;
use std::sync::LazyLock;

use clap::Parser;

/// Package version plus compile-time features, e.g. `0.1.0 (-forwarding)`.
static VERSION: LazyLock<String> = LazyLock::new(|| {
    let forwarding = if sftp_tui_ssh::policy::FORWARDING_ENABLED {
        '+'
    } else {
        '-'
    };
    format!("{} ({forwarding}forwarding)", env!("CARGO_PKG_VERSION"))
});

#[derive(Debug, Parser)]
#[command(version = VERSION.as_str(), about)]
struct Cli {}

fn main() -> ExitCode {
    let _cli = Cli::parse();
    eprintln!(
        "sftp-tui {}: the TUI is not implemented yet, see docs/roadmap.md",
        VERSION.as_str()
    );
    ExitCode::SUCCESS
}
