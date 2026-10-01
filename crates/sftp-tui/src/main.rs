//! sftp-tui: a Midnight Commander-style SFTP file manager built on the system OpenSSH client.

mod cli;
mod commands;
mod format;
mod i18n;
mod logging;
mod prompt;
mod tty;

use std::io::Write as _;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser as _;
use sftp_tui_ssh::askpass::{Invocation, Outcome};

fn main() -> ExitCode {
    // ssh runs this binary as its askpass program (ADR 0003); that comes before anything else.
    if let Some(invocation) = Invocation::from_env() {
        return askpass(&invocation);
    }
    let cli = cli::Cli::parse();
    let _ = color_eyre::install();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Error: cannot start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(commands::run(cli));
    // Don't wait for blocking tasks, such as a scan of a slow file system.
    runtime.shutdown_timeout(Duration::from_millis(200));
    result.unwrap_or_else(|report| {
        eprintln!("error: {report}");
        for cause in report.chain().skip(1) {
            eprintln!("  caused by: {cause}");
        }
        ExitCode::FAILURE
    })
}

/// Forwards the prompt of ssh to the running sftp-tui and prints the answer for ssh.
fn askpass(invocation: &Invocation) -> ExitCode {
    match invocation.run() {
        Ok(Outcome::Answer(text)) => {
            let mut stdout = std::io::stdout().lock();
            let written = stdout
                .write_all(text.as_bytes())
                .and_then(|()| stdout.write_all(b"\n"))
                .and_then(|()| stdout.flush());
            if written.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        // ssh treats a failing askpass program as a declined prompt.
        Ok(Outcome::Cancelled) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("sftp-tui askpass: {error}");
            ExitCode::FAILURE
        }
    }
}
