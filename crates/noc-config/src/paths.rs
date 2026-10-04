use std::ffi::OsString;
use std::fs::{self, DirBuilder, Metadata};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::{APP_NAME, ConfigError};

/// Directories used by Noon Commander: the XDG layout on every platform, including macOS.
///
/// An `XDG_*` variable counts only if it is set to an absolute path; otherwise the default
/// applies, as the XDG Base Directory specification requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Home directory of the current user.
    pub home: PathBuf,
    /// Settings, keymap, and themes: `$XDG_CONFIG_HOME/noc`, by default
    /// `~/.config/noc`.
    pub config_dir: PathBuf,
    /// Bookmarks and workspaces: `$XDG_DATA_HOME/noc`, by default `~/.local/share/noc`.
    pub data_dir: PathBuf,
    /// History, last directories, and logs: `$XDG_STATE_HOME/noc`, by default
    /// `~/.local/state/noc`.
    pub state_dir: PathBuf,
    /// Cached `ssh -G` results: `$XDG_CACHE_HOME/noc`, by default `~/.cache/noc`.
    pub cache_dir: PathBuf,
    /// Control sockets, the askpass socket, and temporary files: `$XDG_RUNTIME_DIR/noc`,
    /// else `$TMPDIR/noc-<uid>`, else `/tmp/noc-<uid>`. Call
    /// [`ensure_runtime_dir`](Self::ensure_runtime_dir) before using it.
    pub runtime_dir: PathBuf,
    uid: u32,
}

impl Paths {
    /// Resolves the directories for the current user from the process environment.
    ///
    /// Fails with [`ConfigError::NoHomeDir`] if [`std::env::home_dir`] finds no absolute home
    /// directory.
    pub fn from_env() -> Result<Self, ConfigError> {
        // A relative or empty home would put every path under the working directory.
        let home = std::env::home_dir()
            .filter(|home| home.is_absolute())
            .ok_or(ConfigError::NoHomeDir)?;
        let uid = rustix::process::getuid().as_raw();
        Ok(Self::resolve(&home, uid, &|name| std::env::var_os(name)))
    }

    /// Resolves the directories for `home` and `uid`; `env` looks up environment variables.
    ///
    /// Touches neither the process environment nor the file system.
    pub fn resolve(home: &Path, uid: u32, env: &dyn Fn(&str) -> Option<OsString>) -> Self {
        let absolute = |name: &str| {
            env(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let base_dir = |name: &str, default: &str| {
            absolute(name)
                .unwrap_or_else(|| home.join(default))
                .join(APP_NAME)
        };
        // Keep these names short: control socket paths must fit in `sun_path` (104 bytes on
        // macOS), and macOS's `$TMPDIR` is already long.
        let runtime_dir = match absolute("XDG_RUNTIME_DIR") {
            Some(dir) => dir.join(APP_NAME),
            None => absolute("TMPDIR")
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(format!("{APP_NAME}-{uid}")),
        };
        Self {
            home: home.to_path_buf(),
            config_dir: base_dir("XDG_CONFIG_HOME", ".config"),
            data_dir: base_dir("XDG_DATA_HOME", ".local/share"),
            state_dir: base_dir("XDG_STATE_HOME", ".local/state"),
            cache_dir: base_dir("XDG_CACHE_HOME", ".cache"),
            runtime_dir,
            uid,
        }
    }

    /// The settings file: `config.toml` in [`config_dir`](Self::config_dir).
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// The host settings next to `config_file`: `hosts.toml` in its directory, so that
    /// `--config` moves both.
    pub fn hosts_file(config_file: &Path) -> PathBuf {
        config_file.with_file_name("hosts.toml")
    }

    /// The saved workspaces: `workspaces.toml` in [`data_dir`](Self::data_dir).
    pub fn workspaces_file(&self) -> PathBuf {
        self.data_dir.join("workspaces.toml")
    }

    /// Creates [`runtime_dir`](Self::runtime_dir) with mode 0700 if it is missing, and checks
    /// that it is a private directory of the user: a real directory, not a symbolic link,
    /// owned by the user, and closed to group and others.
    ///
    /// Only the last component is created; the parent must exist. Fails with
    /// [`ConfigError::UnsafeRuntimeDir`] if the check fails.
    pub fn ensure_runtime_dir(&self) -> Result<(), ConfigError> {
        let dir = &self.runtime_dir;
        match DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            // Also the result of losing a creation race; the check below decides either way.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(ConfigError::Write {
                    path: dir.clone(),
                    source,
                });
            }
        }
        // `symlink_metadata`, because a link to a private directory is not one: whoever owns
        // the link can point it elsewhere.
        let metadata = fs::symlink_metadata(dir).map_err(|source| ConfigError::Read {
            path: dir.clone(),
            source,
        })?;
        check_private_dir(&metadata, self.uid).map_err(|reason| ConfigError::UnsafeRuntimeDir {
            path: dir.clone(),
            reason,
        })
    }
}

/// Checks that `metadata` describes a directory that only `uid` can access.
fn check_private_dir(metadata: &Metadata, uid: u32) -> Result<(), String> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err("it is a symbolic link".to_owned());
    }
    if !file_type.is_dir() {
        return Err("it is not a directory".to_owned());
    }
    if metadata.uid() != uid {
        return Err(format!("it is owned by uid {}, not {uid}", metadata.uid()));
    }
    let mode = metadata.mode() & 0o7777;
    if mode & 0o077 != 0 {
        return Err(format!(
            "its mode {mode:04o} gives access to group or others"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs::{self, DirBuilder};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    use super::Paths;
    use crate::ConfigError;

    fn resolve(vars: &[(&str, &str)]) -> Paths {
        let env = |name: &str| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(*value))
        };
        Paths::resolve(Path::new("/home/u"), 501, &env)
    }

    #[test]
    fn defaults() {
        let paths = resolve(&[]);
        assert_eq!(paths.home.as_os_str(), "/home/u");
        assert_eq!(paths.config_dir.as_os_str(), "/home/u/.config/noc");
        assert_eq!(paths.data_dir.as_os_str(), "/home/u/.local/share/noc");
        assert_eq!(paths.state_dir.as_os_str(), "/home/u/.local/state/noc");
        assert_eq!(paths.cache_dir.as_os_str(), "/home/u/.cache/noc");
        assert_eq!(paths.runtime_dir.as_os_str(), "/tmp/noc-501");
        assert_eq!(
            paths.config_file().as_os_str(),
            "/home/u/.config/noc/config.toml"
        );
        assert_eq!(
            paths.workspaces_file().as_os_str(),
            "/home/u/.local/share/noc/workspaces.toml"
        );
    }

    #[test]
    fn each_xdg_variable_moves_only_its_directory() {
        let defaults = resolve(&[]);
        assert_eq!(
            resolve(&[("XDG_CONFIG_HOME", "/xdg/config")]),
            Paths {
                config_dir: PathBuf::from("/xdg/config/noc"),
                ..defaults.clone()
            }
        );
        assert_eq!(
            resolve(&[("XDG_DATA_HOME", "/xdg/data")]),
            Paths {
                data_dir: PathBuf::from("/xdg/data/noc"),
                ..defaults.clone()
            }
        );
        assert_eq!(
            resolve(&[("XDG_STATE_HOME", "/xdg/state")]),
            Paths {
                state_dir: PathBuf::from("/xdg/state/noc"),
                ..defaults.clone()
            }
        );
        assert_eq!(
            resolve(&[("XDG_CACHE_HOME", "/xdg/cache")]),
            Paths {
                cache_dir: PathBuf::from("/xdg/cache/noc"),
                ..defaults.clone()
            }
        );
        assert_eq!(
            resolve(&[("XDG_RUNTIME_DIR", "/run/user/501")]),
            Paths {
                runtime_dir: PathBuf::from("/run/user/501/noc"),
                ..defaults
            }
        );
    }

    #[test]
    fn the_hosts_file_is_next_to_the_config_file() {
        let paths = resolve(&[]);
        assert_eq!(
            Paths::hosts_file(&paths.config_file()).as_os_str(),
            "/home/u/.config/noc/hosts.toml"
        );
        assert_eq!(
            Paths::hosts_file(Path::new("/etc/noc/work.toml")).as_os_str(),
            "/etc/noc/hosts.toml"
        );
    }

    #[test]
    fn config_file_follows_xdg_config_home() {
        let paths = resolve(&[("XDG_CONFIG_HOME", "/xdg/config")]);
        assert_eq!(
            paths.config_file().as_os_str(),
            "/xdg/config/noc/config.toml"
        );
    }

    #[test]
    fn empty_and_relative_values_are_ignored() {
        let names = [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
            "XDG_RUNTIME_DIR",
            "TMPDIR",
        ];
        for value in ["", "relative", "./relative", "~/relative"] {
            let vars: Vec<_> = names.iter().map(|name| (*name, value)).collect();
            assert_eq!(resolve(&vars), resolve(&[]), "{value:?}");
        }
    }

    #[test]
    fn runtime_dir_prefers_xdg_runtime_dir() {
        let paths = resolve(&[("XDG_RUNTIME_DIR", "/run/user/501"), ("TMPDIR", "/var/tmp")]);
        assert_eq!(paths.runtime_dir.as_os_str(), "/run/user/501/noc");
    }

    #[test]
    fn runtime_dir_falls_back_to_tmpdir() {
        // macOS sets `TMPDIR` with a trailing slash.
        let paths = resolve(&[("TMPDIR", "/var/folders/ab/xyz/T/")]);
        assert_eq!(
            paths.runtime_dir.as_os_str(),
            "/var/folders/ab/xyz/T/noc-501"
        );
        let paths = resolve(&[("XDG_RUNTIME_DIR", ""), ("TMPDIR", "/var/tmp")]);
        assert_eq!(paths.runtime_dir.as_os_str(), "/var/tmp/noc-501");
    }

    #[test]
    fn runtime_dir_falls_back_to_tmp() {
        let paths = resolve(&[("XDG_RUNTIME_DIR", "run"), ("TMPDIR", "")]);
        assert_eq!(paths.runtime_dir.as_os_str(), "/tmp/noc-501");
    }

    fn current_uid() -> u32 {
        rustix::process::getuid().as_raw()
    }

    #[test]
    fn from_env_resolves_absolute_paths_for_the_current_user() {
        let paths = match Paths::from_env() {
            Ok(paths) => paths,
            // Some sandboxes run tests without a home directory.
            Err(ConfigError::NoHomeDir) => {
                assert!(!std::env::home_dir().is_some_and(|home| home.is_absolute()));
                return;
            }
            Err(error) => panic!("{error:?}"),
        };
        assert_eq!(paths.uid, current_uid());
        for dir in [
            &paths.home,
            &paths.config_dir,
            &paths.data_dir,
            &paths.state_dir,
            &paths.cache_dir,
            &paths.runtime_dir,
        ] {
            assert!(dir.is_absolute(), "{}", dir.display());
        }
    }

    fn with_runtime_dir(dir: PathBuf, uid: u32) -> Paths {
        Paths {
            runtime_dir: dir,
            ..Paths::resolve(Path::new("/home/u"), uid, &|_| None)
        }
    }

    fn assert_unsafe(paths: &Paths, expected_reason: &str) {
        match paths.ensure_runtime_dir() {
            Err(ConfigError::UnsafeRuntimeDir { path, reason }) => {
                assert_eq!(path, paths.runtime_dir);
                assert!(reason.contains(expected_reason), "{reason}");
            }
            other => panic!("expected an unsafe runtime directory, got {other:?}"),
        }
    }

    #[test]
    fn ensure_runtime_dir_creates_a_private_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = with_runtime_dir(tmp.path().join("noc-501"), current_uid());
        paths.ensure_runtime_dir().unwrap();
        let metadata = fs::symlink_metadata(&paths.runtime_dir).unwrap();
        assert!(metadata.is_dir());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        // The next start finds the directory it created.
        paths.ensure_runtime_dir().unwrap();
    }

    #[test]
    fn ensure_runtime_dir_accepts_an_existing_private_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("noc-501");
        DirBuilder::new().mode(0o700).create(&dir).unwrap();
        with_runtime_dir(dir, current_uid())
            .ensure_runtime_dir()
            .unwrap();
    }

    #[test]
    fn ensure_runtime_dir_rejects_group_or_other_access() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("noc-501");
        fs::create_dir(&dir).unwrap();
        for mode in [0o755, 0o750, 0o701] {
            fs::set_permissions(&dir, fs::Permissions::from_mode(mode)).unwrap();
            assert_unsafe(
                &with_runtime_dir(dir.clone(), current_uid()),
                &format!("mode {mode:04o}"),
            );
        }
    }

    #[test]
    fn ensure_runtime_dir_rejects_a_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        DirBuilder::new().mode(0o700).create(&target).unwrap();
        let link = tmp.path().join("noc-501");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_unsafe(&with_runtime_dir(link, current_uid()), "symbolic link");

        // A dangling link is not followed either.
        let dangling = tmp.path().join("noc-502");
        std::os::unix::fs::symlink(tmp.path().join("missing"), &dangling).unwrap();
        assert_unsafe(&with_runtime_dir(dangling, current_uid()), "symbolic link");
        assert!(!tmp.path().join("missing").exists());
    }

    #[test]
    fn ensure_runtime_dir_rejects_a_regular_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("noc-501");
        fs::write(&file, "").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        assert_unsafe(&with_runtime_dir(file, current_uid()), "not a directory");
    }

    #[test]
    fn ensure_runtime_dir_rejects_another_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("noc-501");
        DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let other_uid = current_uid().wrapping_add(1);
        assert_unsafe(&with_runtime_dir(dir, other_uid), "owned by uid");
    }

    #[test]
    fn ensure_runtime_dir_does_not_create_the_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = with_runtime_dir(tmp.path().join("missing/noc"), current_uid());
        match paths.ensure_runtime_dir() {
            Err(ConfigError::Write { path, source }) => {
                assert_eq!(path, paths.runtime_dir);
                assert_eq!(source.kind(), io::ErrorKind::NotFound);
            }
            other => panic!("expected a write error, got {other:?}"),
        }
        assert!(!tmp.path().join("missing").exists());
    }
}
