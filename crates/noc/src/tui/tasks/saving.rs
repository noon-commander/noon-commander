//! Writing the state files: config, workspaces, history, recent.

use std::path::PathBuf;

use noc_config::{
    Config, History, Recent, Workspaces, add_command, remove_command, remove_workspace,
    rename_workspace, save_config, save_workspace,
};
use noc_vfs::VfsPath as _;
use tokio::sync::mpsc;

use crate::tui::app::{HistoryChange, RecentChange, WorkspaceChange};
use crate::tui::describe;

use super::Done;

/// Starts the task that writes changes to the config file at `path`, one after another, and
/// reports each; returns where to send them.
pub(super) fn save_configs(
    path: PathBuf,
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<(Config, Config)> {
    let (sender, mut changes) = mpsc::unbounded_channel::<(Config, Config)>();
    tokio::spawn(async move {
        while let Some((old, new)) = changes.recv().await {
            let path = path.clone();
            let (old, new) = (Box::new(old), Box::new(new));
            let (from, to) = (old.clone(), new.clone());
            let saved = tokio::task::spawn_blocking(move || save_config(&path, &from, &to)).await;
            let result = match saved {
                Ok(result) => result.map_err(|error| describe::reason(&error)),
                Err(error) => Err(error.to_string().into()),
            };
            if done.send(Done::ConfigSaved { old, new, result }).is_err() {
                break;
            }
        }
    });
    sender
}

/// Starts the task that reads and changes `workspaces.toml` at `path`, in the order the
/// changes come, and reports each to `done`.
pub(super) fn change_workspaces(
    path: PathBuf,
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<WorkspaceChange> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<WorkspaceChange>();
    tokio::spawn(async move {
        while let Some(change) = receiver.recv().await {
            let path = path.clone();
            let asked = change.clone();
            let result = tokio::task::spawn_blocking(move || match asked {
                WorkspaceChange::Load => Workspaces::load(&path),
                WorkspaceChange::Save(workspace) => save_workspace(&path, &workspace),
                WorkspaceChange::Remove(name) => remove_workspace(&path, &name),
                WorkspaceChange::Rename { from, to } => rename_workspace(&path, &from, &to),
            })
            .await;
            let result = match result {
                Ok(result) => result.map_err(|error| describe::reason(&error)),
                Err(error) => Err(error.to_string().into()),
            };
            if done.send(Done::Workspaces { change, result }).is_err() {
                break;
            }
        }
    });
    sender
}

/// Starts the task that reads and changes `history.toml` at `path`, in the order the changes
/// come, and reports each to `done`.
pub(super) fn change_history(
    path: PathBuf,
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<HistoryChange> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<HistoryChange>();
    tokio::spawn(async move {
        while let Some(change) = receiver.recv().await {
            let path = path.clone();
            let asked = change.clone();
            let result = tokio::task::spawn_blocking(move || match asked {
                HistoryChange::Load => History::load(&path),
                HistoryChange::Add { entry, size } => add_command(&path, &entry, size),
                HistoryChange::Remove { host, command } => {
                    remove_command(&path, host.as_deref(), &command)
                }
            })
            .await;
            let result = match result {
                Ok(result) => result
                    .map(|history| history.commands)
                    .map_err(|error| describe::reason(&error)),
                Err(error) => Err(error.to_string().into()),
            };
            if done.send(Done::History { change, result }).is_err() {
                break;
            }
        }
    });
    sender
}

/// Starts the task that reads and writes `recent.toml` at `path`, in the order the changes
/// come, and reports what it read to `done`; a failed write only goes to the log.
pub(super) fn change_recent(
    path: PathBuf,
    done: mpsc::UnboundedSender<Done>,
) -> mpsc::UnboundedSender<RecentChange> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<RecentChange>();
    tokio::spawn(async move {
        while let Some(change) = receiver.recv().await {
            let path = path.clone();
            let result = tokio::task::spawn_blocking(move || match change {
                RecentChange::Load => Recent::load(&path).map(Some),
                RecentChange::Save(recent) => recent.save(&path).map(|()| None),
            })
            .await;
            let result = match result {
                Ok(Ok(None)) => continue,
                Ok(Ok(Some(recent))) => Ok(recent),
                Ok(Err(error)) => Err(describe::chain(&error)),
                Err(error) => Err(error.to_string()),
            };
            if let Err(reason) = &result {
                tracing::warn!(%reason, "cannot read or write the recent places");
            }
            if done.send(Done::Recent(result)).is_err() {
                break;
            }
        }
    });
    sender
}

/// Removes a temporary file, if it is there, in a task of its own.
pub(super) fn discard(path: PathBuf) {
    tokio::spawn(async move {
        if let Err(error) = tokio::fs::remove_file(&path).await {
            tracing::debug!(path = %path.display(), %error, "cannot remove");
        }
    });
}
