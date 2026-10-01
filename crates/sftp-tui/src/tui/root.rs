//! The hosts of the virtual root.

use sftp_tui_ssh::CachedHost;

use crate::context::{Context, describe};

/// A host in the virtual root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootHost {
    /// The alias from `ssh_config`.
    pub(crate) alias: String,
    /// `hosts.<alias>.label`.
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
        .visible(&context.config.discovery.hide)
        .map(|host| RootHost {
            alias: host.alias.clone(),
            label: context.label(&host.alias).map(str::to_owned),
            address: cache
                .get(&context.target(&host.alias))
                .map(CachedHost::address),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use sftp_tui_config::{Config, HostConfig, Paths};

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
        config.hosts.insert(
            "web".to_owned(),
            HostConfig {
                label: Some("Prod".to_owned()),
                ..HostConfig::default()
            },
        );
        let context = Context::new(Paths::resolve(&home, 501, &env), config);

        let mut cache = context.load_cache(&context.scan());
        cache.insert(
            &context.target("db"),
            CachedHost {
                user: "admin".to_owned(),
                hostname: "10.0.0.5".to_owned(),
                port: 2222,
                proxy_jump: None,
            },
        );
        cache.save().unwrap();

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
}
