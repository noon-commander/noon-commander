//! `noc ls`.

use std::process::ExitCode;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr as _};
use noc_ssh::askpass::AskpassServer;
use noc_ssh::version::check_version;
use noc_ssh::{Session, SftpChannel, SshError, cleanup_stale};
use noc_vfs::{DirEntry, FileKind, LocalFs, Location, RemotePath, RootEntry, SftpFs, Vfs};
use tokio_util::sync::CancellationToken;

use super::INTERRUPTED;
use crate::context::Context;
use crate::format;

pub(super) async fn run(context: &Context, location: Option<&str>) -> Result<ExitCode> {
    match location.map_or(Location::Root, Location::parse) {
        Location::Root => {
            for entry in noc_vfs::root_entries(super::host_aliases(context).await?) {
                match entry {
                    RootEntry::Local => println!("[local]"),
                    RootEntry::Host { alias } => println!("{alias}"),
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Location::Local(path) => {
            let entries = LocalFs
                .list_dir(&path)
                .await
                .wrap_err_with(|| format!("cannot list {}", path.display()))?;
            print_entries(entries);
            Ok(ExitCode::SUCCESS)
        }
        Location::Remote { host, path } => remote(context, &host, path).await,
    }
}

async fn remote(context: &Context, host: &str, path: RemotePath) -> Result<ExitCode> {
    let version = check_version(&context.settings).await?;
    tracing::info!(%version, "using OpenSSH");
    let runtime_dir = &context.paths.runtime_dir;
    context.paths.ensure_runtime_dir()?;
    cleanup_stale(runtime_dir, &context.settings).await;
    let program = std::env::current_exe().wrap_err("cannot locate the noc executable")?;
    let (askpass, events) =
        AskpassServer::bind(runtime_dir, program).wrap_err("cannot start the askpass bridge")?;
    let prompts = tokio::spawn(crate::prompt::answer_on_tty(events));

    let cancel = CancellationToken::new();
    let work = list_remote(context, host, path, &askpass, &cancel);
    tokio::pin!(work);
    let result = tokio::select! {
        result = &mut work => result.map(Some),
        _ = tokio::signal::ctrl_c() => {
            cancel.cancel();
            // Give ssh the chance to shut down cleanly.
            let _ = tokio::time::timeout(Duration::from_secs(5), work).await;
            Ok(None)
        }
    };
    prompts.abort();
    match result {
        Ok(Some((path, entries))) => {
            tracing::debug!(path = %path, count = entries.len(), "listed");
            print_entries(entries);
            Ok(ExitCode::SUCCESS)
        }
        Ok(None) => {
            eprintln!("interrupted");
            Ok(ExitCode::from(INTERRUPTED))
        }
        Err(error) => Err(error),
    }
}

async fn list_remote(
    context: &Context,
    host: &str,
    path: RemotePath,
    askpass: &AskpassServer,
    cancel: &CancellationToken,
) -> Result<(RemotePath, Vec<DirEntry>)> {
    let session = Session::connect(
        &context.settings,
        &context.target(host),
        &context.paths.runtime_dir,
        Some(askpass.env(host)),
        cancel,
    )
    .await
    .wrap_err_with(|| format!("cannot connect to {host}"))?;
    let listing = async {
        let SftpChannel {
            stdin,
            stdout,
            process,
        } = session.open_sftp()?;
        let fs = SftpFs::from_pipes(stdin, stdout)
            .await
            .wrap_err("cannot start the SFTP session")?;
        let path = if path.as_bytes().is_empty() {
            fs.home().await?
        } else {
            path
        };
        let entries = fs
            .list_dir(&path)
            .await
            .wrap_err_with(|| format!("cannot list {host}:{path}"))?;
        fs.close().await?;
        process.finish().await;
        Ok((path, entries))
    };
    let result = tokio::select! {
        result = listing => result,
        () = cancel.cancelled() => Err(SshError::Cancelled.into()),
    };
    session.close().await;
    result
}

fn print_entries(mut entries: Vec<DirEntry>) {
    entries.sort_by(|a, b| {
        b.is_dir_like()
            .cmp(&a.is_dir_like())
            .then_with(|| a.name.cmp(&b.name))
    });
    let sizes: Vec<String> = entries
        .iter()
        .map(|entry| match (entry.metadata.kind, entry.metadata.size) {
            (FileKind::File, Some(size)) => format::size(size),
            _ => "-".to_owned(),
        })
        .collect();
    let width = sizes.iter().map(String::len).max().unwrap_or(0);
    for (entry, size) in entries.iter().zip(&sizes) {
        println!(
            "{}  {size:>width$}  {}  {}{}",
            format::mode(entry.metadata.kind, entry.metadata.permissions),
            format::time(entry.metadata.modified),
            entry.display_name(),
            suffix(entry),
        );
    }
}

/// `ls -F`-style marker.
fn suffix(entry: &DirEntry) -> &'static str {
    match entry.metadata.kind {
        FileKind::Dir => "/",
        FileKind::Symlink => "@",
        FileKind::Fifo => "|",
        FileKind::Socket => "=",
        FileKind::File
            if entry
                .metadata
                .permissions
                .is_some_and(|bits| bits & 0o111 != 0) =>
        {
            "*"
        }
        FileKind::File | FileKind::BlockDevice | FileKind::CharDevice | FileKind::Unknown => "",
    }
}
