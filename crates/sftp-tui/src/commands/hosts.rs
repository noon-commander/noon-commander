//! `sftp-tui hosts`.

use std::fmt::Write as _;
use std::process::ExitCode;

use color_eyre::eyre::Result;
use futures_util::StreamExt as _;
use sftp_tui_ssh::resolve::resolve;

use super::Context;

/// Parallel `ssh -G` processes for `--resolve`.
const RESOLVE_JOBS: usize = 4;

pub(super) async fn run(context: &Context, with_addresses: bool) -> Result<ExitCode> {
    let discovery = context.discover().await?;
    let hosts: Vec<_> = discovery.visible(&context.config.discovery.hide).collect();
    if hosts.is_empty() {
        eprintln!("no hosts found in ssh_config");
        return Ok(ExitCode::SUCCESS);
    }
    let addresses: Vec<String> = if with_addresses {
        futures_util::stream::iter(&hosts)
            .map(|host| async move {
                match resolve(&context.settings, &context.target(&host.alias)).await {
                    Ok(resolved) => resolved.address(),
                    Err(error) => format!("error: {error}"),
                }
            })
            .buffered(RESOLVE_JOBS)
            .collect()
            .await
    } else {
        vec![String::new(); hosts.len()]
    };
    let alias_width = hosts.iter().map(|host| host.alias.len()).max().unwrap_or(0);
    let address_width = addresses.iter().map(String::len).max().unwrap_or(0);
    for (host, address) in hosts.iter().zip(&addresses) {
        let mut line = format!("{:<alias_width$}", host.alias);
        if with_addresses {
            let _ = write!(line, "  {address:<address_width$}");
        }
        if let Some(label) = context
            .config
            .hosts
            .get(&host.alias)
            .and_then(|config| config.label.as_deref())
        {
            let _ = write!(line, "  {label}");
        }
        if !host.other_names.is_empty() {
            let _ = write!(line, "  (also {})", host.other_names.join(", "));
        }
        println!("{}", line.trim_end());
    }
    Ok(ExitCode::SUCCESS)
}
