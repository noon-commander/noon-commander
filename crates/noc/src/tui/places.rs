//! What the virtual root lists: the mounted volumes and the hosts.

use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;
use std::time::Duration;

use noc_ssh::pattern::wildcard_match;
use noc_ssh::{CachedHost, Target};
use noc_vfs::{Volume, VolumeKind};

use super::cells;
use crate::context::{Context, describe};

/// How long a volume may take to say how big it is; a dead network mount never does.
pub(crate) const VOLUME_TIMEOUT: Duration = Duration::from_millis(500);

/// A host in the virtual root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootHost {
    /// The alias from `ssh_config`.
    pub(crate) alias: String,
    /// The host's label from `hosts.toml`.
    pub(crate) label: Option<String>,
    /// `user@hostname[:port]` from an earlier `ssh -G`, if cached.
    pub(crate) address: Option<String>,
}

/// The hosts that `discovery.hide` does not hide, in config order, with the addresses cached by
/// earlier runs. Discovery warnings go to the log.
///
/// Blocking: reads the ssh config files and the cache.
pub(crate) fn read_hosts(context: &Context) -> Vec<RootHost> {
    let discovery = context.scan();
    for warning in &discovery.warnings {
        tracing::warn!("{}", describe(warning));
    }
    let cache = context.load_cache(&discovery);
    discovery
        .visible(&context.config().discovery.hide)
        .map(|host| RootHost {
            alias: host.alias.clone(),
            label: context.label(&host.alias),
            address: cache
                .get(&Target::new(host.alias.as_str()))
                .map(CachedHost::address),
        })
        .collect()
}

/// Taken while the volumes are read. A second reader waits for the first, rather than finding
/// every volume busy and taking it for a dead network mount.
static READING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The mounted volumes that `volumes.hide` does not hide. The system volume always stays.
pub(crate) async fn read_volumes(hide: &[String]) -> Vec<Volume> {
    let reading = READING.lock().await;
    let mut volumes = noc_vfs::volumes(VOLUME_TIMEOUT).await;
    drop(reading);
    volumes.retain(|volume| {
        let path = volume.mount_point.to_string_lossy();
        volume.kind == VolumeKind::System
            || !hide.iter().any(|pattern| wildcard_match(pattern, &path))
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

/// Stores what `ssh -G` said about `alias` in the cache, for the next listing and run.
///
/// Blocking: reads the ssh config files and writes the cache.
pub(crate) fn remember(context: &Context, alias: &str, host: CachedHost) -> io::Result<()> {
    let mut cache = context.load_cache(&context.scan());
    cache.insert(&Target::new(alias), host);
    cache.save()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    use noc_config::{Config, HostConfig, Hosts, Paths, SftpHost};

    use super::*;

    #[test]
    fn reads_visible_hosts_with_labels_and_cached_addresses() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let ssh_dir = dir.path().join("ssh");
        fs::create_dir_all(&ssh_dir).unwrap();
        let ssh_config = ssh_dir.join("config");
        fs::write(&ssh_config, "Host web\nHost db\nHost github.com\nHost *\n").unwrap();
        let cache_home = dir.path().join("cache");
        let env = move |name: &str| (name == "XDG_CACHE_HOME").then(|| OsString::from(&cache_home));
        let mut config = Config::default();
        config.ssh.config_file = Some(ssh_config);
        config.discovery.hide = vec!["github.*".to_owned()];
        let mut hosts = Hosts::default();
        hosts.hosts.insert(
            "web".to_owned(),
            HostConfig::Sftp(SftpHost {
                label: Some("Prod".to_owned()),
                ..SftpHost::default()
            }),
        );
        let paths = Paths::resolve(&home, 501, &env);
        let config_file = dir.path().join("config.toml");
        let context = Context::new(paths, config, config_file, hosts);

        let db = CachedHost {
            user: "admin".to_owned(),
            hostname: "10.0.0.5".to_owned(),
            port: 2222,
            proxy_jump: None,
        };
        remember(&context, "db", db).unwrap();

        assert_eq!(
            read_hosts(&context),
            [
                RootHost {
                    alias: "web".to_owned(),
                    label: Some("Prod".to_owned()),
                    address: None,
                },
                RootHost {
                    alias: "db".to_owned(),
                    label: None,
                    address: Some("admin@10.0.0.5:2222".to_owned()),
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
