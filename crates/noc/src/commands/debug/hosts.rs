//! `noc debug hosts`.

use std::fmt::Write as _;
use std::process::ExitCode;

use color_eyre::eyre::Result;
use futures_util::StreamExt as _;
use noc_ssh::Target;
use noc_ssh::resolve::resolve;

use crate::context::Context;

/// Parallel `ssh -G` processes.
const RESOLVE_JOBS: usize = 4;

/// Shows the hosts with where `ssh -G` says each connects to, run for every host now.
pub(super) async fn run(context: &Context) -> Result<ExitCode> {
    let discovery = super::discover(context).await?;
    let config = context.config();
    let hosts: Vec<_> = discovery.visible(&config.discovery.hide).collect();
    if hosts.is_empty() {
        eprintln!("no hosts found in ssh_config");
        return Ok(ExitCode::SUCCESS);
    }
    let addresses: Vec<String> = futures_util::stream::iter(&hosts)
        .map(|host| async move {
            let target = Target::new(host.alias.as_str());
            match resolve(&context.settings(), &target).await {
                Ok(resolved) => resolved.address(),
                Err(error) => format!("error: {error}"),
            }
        })
        .buffered(RESOLVE_JOBS)
        .collect()
        .await;

    let alias_width = hosts.iter().map(|host| host.alias.len()).max().unwrap_or(0);
    let address_width = addresses.iter().map(String::len).max().unwrap_or(0);
    for (host, address) in hosts.iter().zip(&addresses) {
        let mut line = format!("{:<alias_width$}", host.alias);
        let _ = write!(line, "  {address:<address_width$}");
        if let Some(label) = context.label(&host.alias) {
            let _ = write!(line, "  {label}");
        }
        if !host.other_names.is_empty() {
            let _ = write!(line, "  (also {})", host.other_names.join(", "));
        }
        println!("{}", line.trim_end());
    }
    Ok(ExitCode::SUCCESS)
}
