//! `sftp-tui hosts`.

use std::fmt::Write as _;
use std::process::ExitCode;

use color_eyre::eyre::Result;
use futures_util::StreamExt as _;
use sftp_tui_ssh::resolve::resolve;
use sftp_tui_ssh::{CachedHost, ConfigStamp, ResolveCache};

use super::Context;

/// Parallel `ssh -G` processes for `--resolve`.
const RESOLVE_JOBS: usize = 4;

/// Shows the hosts with the addresses cached by earlier runs; `refresh` runs `ssh -G` for every
/// host first and updates the cache.
pub(super) async fn run(context: &Context, refresh: bool) -> Result<ExitCode> {
    let discovery = context.discover().await?;
    let hosts: Vec<_> = discovery.visible(&context.config.discovery.hide).collect();
    if hosts.is_empty() {
        eprintln!("no hosts found in ssh_config");
        return Ok(ExitCode::SUCCESS);
    }
    let mut files = discovery.files.clone();
    files.extend(context.discovery_options().root_files());
    let settings = context.settings.clone();
    let path = context.paths.cache_dir.join("resolve.json");
    let mut cache = tokio::task::spawn_blocking(move || {
        ResolveCache::load(path, ConfigStamp::read(&files, &settings))
    })
    .await?;

    let addresses: Vec<String> = if refresh {
        let results: Vec<_> = futures_util::stream::iter(&hosts)
            .map(|host| async move {
                let target = context.target(&host.alias);
                let result = resolve(&context.settings, &target).await;
                (target, result)
            })
            .buffered(RESOLVE_JOBS)
            .collect()
            .await;
        let addresses = results
            .into_iter()
            .map(|(target, result)| match result {
                Ok(resolved) => {
                    cache.insert(&target, CachedHost::from(&resolved));
                    resolved.address()
                }
                Err(error) => format!("error: {error}"),
            })
            .collect();
        let saved = tokio::task::spawn_blocking(move || cache.save()).await?;
        if let Err(error) = saved {
            tracing::warn!(%error, "cannot save the ssh -G cache");
        }
        addresses
    } else {
        hosts
            .iter()
            .map(|host| {
                cache
                    .get(&context.target(&host.alias))
                    .map(CachedHost::address)
                    .unwrap_or_default()
            })
            .collect()
    };

    let alias_width = hosts.iter().map(|host| host.alias.len()).max().unwrap_or(0);
    let address_width = addresses.iter().map(String::len).max().unwrap_or(0);
    for (host, address) in hosts.iter().zip(&addresses) {
        let mut line = format!("{:<alias_width$}", host.alias);
        if address_width > 0 {
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
