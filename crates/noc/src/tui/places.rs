//! What the location menu lists: the mounted volumes and the hosts.

use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use noc_ssh::pattern::wildcard_match;
use noc_vfs::{Late, Volume, VolumeKind, Volumes};

use super::cells;
use super::spinner;
use crate::context::{Context, describe};

/// How long a volume may take to say how big it is; a dead network mount never does.
pub(crate) const VOLUME_TIMEOUT: Duration = Duration::from_millis(500);

/// How long the location menu waits for the volumes before it shows its rows, with the sizes
/// of those that did not answer still to come: as long as a wait goes before its spinner shows,
/// so that volumes that answer at once show no skeleton.
pub(crate) const FIRST_ANSWER: Duration = spinner::DELAY;

/// The places of the location menu, as read at one time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Places {
    /// The system volume first, then by mount point.
    pub(crate) volumes: Vec<Volume>,
    /// In config order.
    pub(crate) hosts: Vec<KnownHost>,
    /// The mount points of the volumes that did not answer in time, whose answers are still to
    /// come, and when they were asked.
    pub(crate) waiting: HashMap<PathBuf, Instant>,
    /// The mount points of the volumes whose answers failed after they hung: they have not
    /// answered.
    pub(crate) failed: HashSet<PathBuf>,
}

/// A host of the ssh config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnownHost {
    /// The alias from `ssh_config`.
    pub(crate) alias: String,
    /// The file of its `Host` line, and the line, from 1.
    pub(crate) file: PathBuf,
    pub(crate) line: usize,
}

/// The hosts that `discovery.hide` does not hide, in config order. Discovery warnings go to the
/// log.
///
/// Blocking: reads the ssh config files.
pub(crate) fn read_hosts(context: &Context) -> Vec<KnownHost> {
    let discovery = context.scan();
    for warning in &discovery.warnings {
        tracing::warn!("{}", describe(warning));
    }
    discovery
        .visible(&context.config().discovery.hide)
        .map(|host| KnownHost {
            alias: host.alias.clone(),
            file: host.file.clone(),
            line: host.line,
        })
        .collect()
}

/// The volumes and the hosts, after at most `first` for the volumes, with the answers of those
/// that did not answer by then. They are read each time, so a reload shows what changed.
pub(crate) async fn read_places(
    context: Arc<Context>,
    first: Duration,
) -> (Result<Places, String>, Vec<Late>) {
    let hide = context.config().volumes.hide.clone();
    let hosts = tokio::task::spawn_blocking(move || read_hosts(&context));
    let (volumes, hosts) = tokio::join!(read_volumes_within(&hide, first), hosts);
    let hosts = match hosts {
        Ok(hosts) => hosts,
        Err(error) => return (Err(error.to_string()), Vec::new()),
    };
    let waiting = (volumes.late.iter())
        .map(|late| (late.mount_point().to_path_buf(), late.since()))
        .collect();
    let places = Places {
        volumes: volumes.volumes,
        hosts,
        waiting,
        failed: HashSet::new(),
    };
    (Ok(places), volumes.late)
}

/// The mounted volumes that `volumes.hide` does not hide. The system volume always stays.
pub(crate) async fn read_volumes(hide: &[String]) -> Vec<Volume> {
    read_volumes_within(hide, VOLUME_TIMEOUT).await.volumes
}

/// The volumes of [`read_volumes`] after at most `first`, with the answers still to come.
/// Readers at the same time wait for the same answers: a volume is never asked again while it
/// has not answered.
async fn read_volumes_within(hide: &[String], first: Duration) -> Volumes {
    let mut volumes = noc_vfs::volumes_within(first).await;
    volumes.volumes.retain(|volume| {
        let path = volume.mount_point.to_string_lossy();
        volume.kind == VolumeKind::System
            || !hide.iter().any(|pattern| wildcard_match(pattern, &path))
    });
    let shown = &volumes.volumes;
    (volumes.late).retain(|late| {
        shown
            .iter()
            .any(|volume| volume.mount_point == late.mount_point())
    });
    volumes
}

/// The volume that holds `path`: the one with the longest mount point it is under.
pub(crate) fn volume_of<'a>(volumes: &'a [Volume], path: &Path) -> Option<&'a Volume> {
    volumes
        .iter()
        .filter(|volume| path.starts_with(&volume.mount_point))
        .max_by_key(|volume| volume.mount_point.as_os_str().len())
}

/// The name of a volume, terminal-safe: its label, or else its mount point.
pub(crate) fn volume_name(volume: &Volume) -> String {
    match &volume.label {
        Some(label) => cells::sanitize(label.as_bytes()),
        None => cells::sanitize(volume.mount_point.as_os_str().as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use noc_config::{Config, Hosts, Paths};

    use super::*;

    #[test]
    fn reads_visible_hosts_where_they_are_defined() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let ssh_dir = dir.path().join("ssh");
        fs::create_dir_all(&ssh_dir).unwrap();
        let ssh_config = ssh_dir.join("config");
        fs::write(&ssh_config, "Host web\nHost db\nHost github.com\nHost *\n").unwrap();
        let mut config = Config::default();
        config.ssh.config_file = Some(ssh_config.clone());
        config.discovery.hide = vec!["github.*".to_owned()];
        let paths = Paths::resolve(&home, 501, &|_| None);
        let config_file = dir.path().join("config.toml");
        let context = Context::new(paths, config, config_file, Hosts::default());

        assert_eq!(
            read_hosts(&context),
            [
                KnownHost {
                    alias: "web".to_owned(),
                    file: ssh_config.clone(),
                    line: 1,
                },
                KnownHost {
                    alias: "db".to_owned(),
                    file: ssh_config,
                    line: 2,
                },
            ]
        );
    }

    #[test]
    fn volumes_are_named_by_label_or_mount_point() {
        let mut volume = Volume {
            mount_point: PathBuf::from("/mnt/usb"),
            label: Some("My\x1bDisk".to_owned()),
            fs_type: None,
            source: None,
            kind: VolumeKind::Local,
            space: None,
        };
        assert_eq!(volume_name(&volume), "My?Disk", "terminal-safe");
        volume.label = None;
        assert_eq!(volume_name(&volume), "/mnt/usb");
    }

    #[test]
    fn a_path_is_on_the_volume_with_the_longest_mount_point_above_it() {
        let volume = |path: &str| Volume {
            mount_point: PathBuf::from(path),
            label: None,
            fs_type: None,
            source: None,
            kind: VolumeKind::Local,
            space: None,
        };
        let volumes = [volume("/"), volume("/home"), volume("/homes")];
        let on = |path: &str| volume_of(&volumes, Path::new(path)).map(|v| v.mount_point.clone());
        assert_eq!(on("/home/me"), Some(PathBuf::from("/home")));
        assert_eq!(
            on("/homework"),
            Some(PathBuf::from("/")),
            "whole components"
        );
        assert_eq!(on("/"), Some(PathBuf::from("/")));
        assert_eq!(volume_of(&volumes[1..], Path::new("/srv")), None);
    }

    #[tokio::test]
    async fn hidden_volumes_leave_the_system_volume() {
        let volumes = read_volumes(&["*".to_owned()]).await;
        assert!(
            volumes
                .iter()
                .all(|volume| volume.kind == VolumeKind::System),
            "{volumes:?}"
        );
        assert_eq!(volumes.len(), 1);
    }
}
