//! A persistent cache of `ssh -G` results.
//!
//! `ssh -G` runs `Match exec` predicates, so it may only run lazily, on selection or connect.
//! The cache lets the virtual root show the addresses resolved in earlier runs right away.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _,
};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::{SshSettings, Target};
use crate::resolve::ResolvedHost;
use crate::runtime::random_hex;

/// Version of the cache file format.
const VERSION: u32 = 2;

/// What the cache is valid for: the state of the ssh config files and the ssh settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigStamp {
    program: String,
    config_file: Option<String>,
    args: Vec<String>,
    files: Vec<FileStamp>,
    dirs: Vec<DirStamp>,
}

impl ConfigStamp {
    /// Reads the state of `files`: for each one, its inode, modification time in nanoseconds,
    /// and size, or that it is missing; and the names in each one's directory, so that a new
    /// file in an included directory changes the stamp too. The program, config file, and
    /// arguments of `settings` are part of the stamp; the order of `files` is not.
    ///
    /// Blocking: call it from a blocking thread.
    pub fn read(files: &[PathBuf], settings: &SshSettings) -> Self {
        let paths: BTreeSet<&Path> = files.iter().map(PathBuf::as_path).collect();
        let files: Vec<FileStamp> = paths
            .iter()
            .map(|path| FileStamp {
                path: lossy(path),
                state: FileState::read(path),
            })
            .collect();
        // The directory of a device such as `/dev/null` says nothing about the configuration.
        let dirs: BTreeSet<&Path> = paths
            .iter()
            .zip(&files)
            .filter(|(_, file)| file.state != FileState::Special)
            .filter_map(|(path, _)| path.parent())
            .filter(|dir| !dir.as_os_str().is_empty())
            .collect();
        let dirs = dirs
            .into_iter()
            .map(|path| DirStamp {
                path: lossy(path),
                names: list_dir(path),
            })
            .collect();
        Self {
            program: lossy(&settings.program),
            config_file: settings.config_file.as_deref().map(lossy),
            args: settings.args.clone(),
            files,
            dirs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    path: String,
    state: FileState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FileState {
    Missing,
    /// A device, FIFO, or socket, which discovery reads as empty. Writes to `/dev/null` change
    /// its times.
    Special,
    /// The inode tells apart files with the same time and size, such as files in the Nix store,
    /// which all have the time 1 and which home-manager links `~/.ssh/config` to.
    Present {
        inode: u64,
        mtime_ns: i128,
        size: u64,
    },
}

impl FileState {
    fn read(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(metadata) if is_special(metadata.file_type()) => Self::Special,
            Ok(metadata) => Self::Present {
                inode: metadata.ino(),
                mtime_ns: i128::from(metadata.mtime()) * 1_000_000_000
                    + i128::from(metadata.mtime_nsec()),
                size: metadata.len(),
            },
            Err(_) => Self::Missing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DirStamp {
    path: String,
    /// `None` if the directory cannot be listed.
    names: Option<Vec<String>>,
}

/// The sorted names in `dir`, without sockets, FIFOs, and devices. Names rather than the
/// directory's modification time, because ssh creates and removes a `ControlPath` socket, often
/// in `~/.ssh`, for every multiplexed connection.
fn list_dir(dir: &Path) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if !entry.file_type().is_ok_and(is_special) {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort_unstable();
    Some(names)
}

fn is_special(kind: fs::FileType) -> bool {
    kind.is_socket() || kind.is_fifo() || kind.is_char_device() || kind.is_block_device()
}

fn lossy(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Address details of one host from `ssh -G`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedHost {
    pub user: String,
    pub hostname: String,
    pub port: u16,
    /// `None` when ssh connects directly.
    pub proxy_jump: Option<String>,
}

impl CachedHost {
    /// `user@hostname`, with `:port` unless it is 22, like [`ResolvedHost::address`].
    pub fn address(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.user, self.hostname)
        } else {
            format!("{}@{}:{}", self.user, self.hostname, self.port)
        }
    }
}

impl From<&ResolvedHost> for CachedHost {
    fn from(host: &ResolvedHost) -> Self {
        Self {
            user: host.user.clone(),
            hostname: host.hostname.clone(),
            port: host.port,
            proxy_jump: host.proxy_jump.clone(),
        }
    }
}

/// `ssh -G` results of earlier runs, stored as JSON in one file.
///
/// A cache is valid for one [`ConfigStamp`]: loaded for another one, it is empty. Entries can
/// still be stale, for example when the answer of a `Match exec` predicate changed, so they only
/// stand in until `ssh -G` runs again.
#[derive(Debug, Clone)]
pub struct ResolveCache {
    path: PathBuf,
    stamp: ConfigStamp,
    /// Sorted by destination, without duplicates.
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    destination: String,
    host: CachedHost,
}

#[derive(Serialize)]
struct CacheFileRef<'a> {
    version: u32,
    stamp: &'a ConfigStamp,
    entries: &'a [Entry],
}

#[derive(Deserialize)]
struct Header {
    version: u32,
}

#[derive(Deserialize)]
struct CacheFile {
    stamp: ConfigStamp,
    entries: Vec<Entry>,
}

/// Why a cache file is not used.
#[derive(Debug, thiserror::Error)]
enum Unusable {
    #[error("cannot read it: {0}")]
    Io(#[from] io::Error),
    #[error("not a regular file")]
    NotAFile,
    #[error("invalid contents: {0}")]
    Corrupt(#[from] serde_json::Error),
    #[error("format version {0}")]
    Version(u32),
    #[error("the ssh configuration changed")]
    Outdated,
}

impl ResolveCache {
    /// Loads the cache file at `path`. A missing, unreadable, or corrupt file, or one with
    /// another format version or another stamp, yields an empty cache for `stamp`; the reason
    /// is logged at debug level.
    ///
    /// Blocking: call it from a blocking thread.
    pub fn load(path: PathBuf, stamp: ConfigStamp) -> Self {
        let mut entries = read_entries(&path, &stamp).unwrap_or_else(|reason| {
            tracing::debug!(path = %path.display(), %reason, "starting with an empty ssh -G cache");
            Vec::new()
        });
        // Only a hand-edited file could be out of order.
        entries.sort_by(|a, b| a.destination.cmp(&b.destination));
        entries.dedup_by(|a, b| a.destination == b.destination);
        Self {
            path,
            stamp,
            entries,
        }
    }

    /// The entry for the destination of `target`.
    pub fn get(&self, target: &Target) -> Option<&CachedHost> {
        let index = self.position(target).ok()?;
        Some(&self.entries[index].host)
    }

    /// Adds or replaces the entry for `target`.
    pub fn insert(&mut self, target: &Target, host: CachedHost) {
        match self.position(target) {
            Ok(index) => self.entries[index].host = host,
            Err(index) => self.entries.insert(
                index,
                Entry {
                    destination: target.destination.clone(),
                    host,
                },
            ),
        }
    }

    fn position(&self, target: &Target) -> Result<usize, usize> {
        self.entries
            .binary_search_by(|entry| entry.destination.as_str().cmp(&target.destination))
    }

    /// Writes the cache file atomically: to a temporary file with mode 0600 in the same
    /// directory, which then replaces it. Creates the parent directory (mode 0700) if needed.
    ///
    /// Blocking: call it from a blocking thread.
    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        let bytes = serde_json::to_vec(&CacheFileRef {
            version: VERSION,
            stamp: &self.stamp,
            entries: &self.entries,
        })?;
        let mut temporary = self.path.clone().into_os_string();
        temporary.push(format!(".{}.tmp", random_hex(8)?));
        let temporary = PathBuf::from(temporary);
        let written = {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&bytes)
        };
        let result = written.and_then(|()| fs::rename(&temporary, &self.path));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn read_entries(path: &Path, stamp: &ConfigStamp) -> Result<Vec<Entry>, Unusable> {
    // Reading a FIFO could block forever.
    if !fs::metadata(path)?.is_file() {
        return Err(Unusable::NotAFile);
    }
    let bytes = fs::read(path)?;
    let Header { version } = serde_json::from_slice(&bytes)?;
    if version != VERSION {
        return Err(Unusable::Version(version));
    }
    let file: CacheFile = serde_json::from_slice(&bytes)?;
    if file.stamp != *stamp {
        return Err(Unusable::Outdated);
    }
    Ok(file.entries)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, SystemTime};

    use tempfile::TempDir;

    use super::*;

    struct Scratch(TempDir);

    impl Scratch {
        fn new() -> Self {
            Self(TempDir::new().unwrap())
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.0.path().join(relative)
        }

        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.path(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }
    }

    fn stamp(files: &[PathBuf]) -> ConfigStamp {
        ConfigStamp::read(files, &SshSettings::default())
    }

    fn host(user: &str, port: u16) -> CachedHost {
        CachedHost {
            user: user.to_owned(),
            hostname: "10.0.0.5".to_owned(),
            port,
            proxy_jump: None,
        }
    }

    fn target(destination: &str) -> Target {
        Target::new(destination)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn round_trips_through_the_file() {
        let scratch = Scratch::new();
        let config = scratch.write("ssh/config", "Host web\n");
        let stamp = stamp(&[config]);
        let path = scratch.path("cache/resolve.json");
        let web = target("web");
        let jump = target("jump");

        let mut cache = ResolveCache::load(path.clone(), stamp.clone());
        assert_eq!(cache.get(&web), None);
        cache.insert(&web, host("deploy", 22));
        cache.insert(&jump, host("deploy", 2222));
        cache.insert(&web, host("admin", 22));
        cache.save().unwrap();

        let loaded = ResolveCache::load(path, stamp);
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.get(&web), Some(&host("admin", 22)));
        assert_eq!(loaded.get(&jump), Some(&host("deploy", 2222)));
        assert_eq!(loaded.get(&target("db")), None);
    }

    #[test]
    fn finds_entries_inserted_in_any_order() {
        let scratch = Scratch::new();
        let mut cache = ResolveCache::load(scratch.path("resolve.json"), stamp(&[]));
        let targets = [
            target("m"),
            target("b2"),
            target("z"),
            target("b"),
            target("a"),
            target("b1"),
        ];
        for (port, target) in (1..).zip(&targets) {
            cache.insert(target, host("u", port));
        }
        for (port, target) in (1..).zip(&targets) {
            assert_eq!(cache.get(target), Some(&host("u", port)), "{target:?}");
        }
    }

    #[test]
    fn loads_hand_edited_files() {
        let scratch = Scratch::new();
        let path = scratch.path("resolve.json");
        let entry = |destination: &str, user: &str| Entry {
            destination: destination.to_owned(),
            host: host(user, 22),
        };
        let entries = [
            entry("web", "first"),
            entry("db", "db"),
            entry("web", "second"),
        ];
        let file = CacheFileRef {
            version: VERSION,
            stamp: &stamp(&[]),
            entries: &entries,
        };
        fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        let cache = ResolveCache::load(path, stamp(&[]));
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.get(&target("web")), Some(&host("first", 22)));
        assert_eq!(cache.get(&target("db")), Some(&host("db", 22)));
    }

    #[test]
    fn another_stamp_starts_empty() {
        let scratch = Scratch::new();
        let files = [scratch.write("ssh/config", "Host web\n")];
        let path = scratch.path("resolve.json");
        let mut cache = ResolveCache::load(path.clone(), stamp(&files));
        cache.insert(&target("web"), host("deploy", 22));
        cache.save().unwrap();

        let settings = SshSettings {
            args: vec!["-v".to_owned()],
            ..SshSettings::default()
        };
        let other_settings = ConfigStamp::read(&files, &settings);
        assert!(
            ResolveCache::load(path.clone(), other_settings)
                .entries
                .is_empty()
        );
        let other_files = stamp(&[]);
        assert!(
            ResolveCache::load(path.clone(), other_files)
                .entries
                .is_empty()
        );
        assert_eq!(ResolveCache::load(path, stamp(&files)).entries.len(), 1);
    }

    #[test]
    fn unusable_files_start_empty() {
        let scratch = Scratch::new();
        let path = scratch.path("resolve.json");
        let mut cache = ResolveCache::load(path.clone(), stamp(&[]));
        cache.insert(&target("web"), host("deploy", 22));
        cache.save().unwrap();
        let valid = fs::read(&path).unwrap();

        // Version 1 keyed entries by destination and per-host arguments.
        let mut other_version: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        assert_eq!(other_version["version"], 2);
        other_version["version"] = 1.into();
        other_version["entries"][0]["args"] = serde_json::json!([]);
        let unusable = [
            Vec::new(),
            b"not json".to_vec(),
            b"{}".to_vec(),
            b"{\"version\": 2}".to_vec(),
            valid[..valid.len() / 2].to_vec(),
            serde_json::to_vec(&other_version).unwrap(),
        ];
        for contents in unusable {
            fs::write(&path, &contents).unwrap();
            let cache = ResolveCache::load(path.clone(), stamp(&[]));
            assert!(
                cache.entries.is_empty(),
                "{}",
                String::from_utf8_lossy(&contents)
            );
        }

        // The next save replaces an unusable file.
        let mut cache = ResolveCache::load(path.clone(), stamp(&[]));
        cache.insert(&target("db"), host("admin", 22));
        cache.save().unwrap();
        let loaded = ResolveCache::load(path, stamp(&[]));
        assert_eq!(loaded.get(&target("db")), Some(&host("admin", 22)));
    }

    #[test]
    fn saves_privately_without_leftovers() {
        let scratch = Scratch::new();
        let dir = scratch.path("cache/noc");
        let path = dir.join("resolve.json");
        let mut cache = ResolveCache::load(path.clone(), stamp(&[]));
        cache.insert(&target("web"), host("deploy", 22));
        cache.save().unwrap();
        cache.save().unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&scratch.path("cache")), 0o700);
        assert_eq!(names(&dir), ["resolve.json"]);
    }

    #[test]
    fn failed_saves_leave_no_temporary_files() {
        let scratch = Scratch::new();
        let path = scratch.path("resolve.json");
        fs::create_dir(&path).unwrap();
        let mut cache = ResolveCache::load(path, stamp(&[]));
        assert!(cache.entries.is_empty());
        cache.insert(&target("web"), host("deploy", 22));
        assert!(cache.save().is_err());
        assert_eq!(names(scratch.0.path()), ["resolve.json"]);
    }

    #[test]
    fn equal_states_give_equal_stamps() {
        let scratch = Scratch::new();
        let config = scratch.write("ssh/config", "Include conf.d/*\n");
        let included = scratch.write("ssh/conf.d/web", "Host web\n");
        let missing = scratch.path("etc/ssh_config");
        let files = [config.clone(), included.clone(), missing.clone()];
        let first = stamp(&files);
        assert_eq!(stamp(&files), first);
        assert_eq!(stamp(&[missing, included, config.clone(), config]), first);

        let json = serde_json::to_string(&first).unwrap();
        assert_eq!(serde_json::from_str::<ConfigStamp>(&json).unwrap(), first);
    }

    #[test]
    fn stamp_follows_contents_and_times() {
        let scratch = Scratch::new();
        let config = scratch.write("ssh/config", "Host web\n");
        let files = [config.clone()];
        let before = stamp(&files);
        fs::write(&config, "Host web db\n").unwrap();
        assert_ne!(stamp(&files), before, "the contents changed");

        let before = stamp(&files);
        let file = fs::File::options().write(true).open(&config).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000))
            .unwrap();
        assert_ne!(stamp(&files), before, "the modification time changed");
    }

    #[test]
    fn stamp_follows_replaced_files() {
        let scratch = Scratch::new();
        let config = scratch.write("ssh/config", "Port 2222\n");
        let replacement = scratch.write("store/config", "Port 2223\n");
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        for path in [&config, &replacement] {
            let file = fs::File::options().write(true).open(path).unwrap();
            file.set_modified(time).unwrap();
        }
        let files = [config.clone()];
        let before = stamp(&files);
        fs::rename(&replacement, &config).unwrap();
        assert_ne!(stamp(&files), before);
    }

    #[test]
    fn stamp_follows_new_files() {
        let scratch = Scratch::new();
        let config = scratch.path("ssh/config");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        let files = [config];
        let before = stamp(&files);
        scratch.write("ssh/config", "Host web\n");
        assert_ne!(stamp(&files), before, "a missing file appeared");

        let files = [scratch.write("ssh/conf.d/web", "Host web\n")];
        let before = stamp(&files);
        scratch.write("ssh/conf.d/db", "Host db\n");
        assert_ne!(
            stamp(&files),
            before,
            "a file appeared next to a watched one"
        );
    }

    #[test]
    fn stamp_ignores_sockets_next_to_watched_files() {
        // Short, so the socket path fits into `sun_path`.
        let dir = tempfile::Builder::new()
            .prefix("st")
            .tempdir_in("/tmp")
            .unwrap();
        let config = dir.path().join("config");
        fs::write(&config, "Host web\n").unwrap();
        let files = [config];
        let before = stamp(&files);
        let _socket = UnixListener::bind(dir.path().join("cm-web")).unwrap();
        assert_eq!(stamp(&files), before);
    }

    #[test]
    fn special_files_have_no_times() {
        let files = [PathBuf::from("/dev/null")];
        let before = stamp(&files);
        assert_eq!(before.files[0].state, FileState::Special);
        assert_eq!(before.dirs, [], "/dev is not watched");
        fs::write("/dev/null", "x").unwrap();
        assert_eq!(stamp(&files), before);
    }

    #[test]
    fn stamp_covers_the_ssh_settings() {
        let base = ConfigStamp::read(&[], &SshSettings::default());
        let changed = [
            SshSettings {
                program: PathBuf::from("/opt/ssh"),
                ..SshSettings::default()
            },
            SshSettings {
                config_file: Some(PathBuf::from("/cfg")),
                ..SshSettings::default()
            },
            SshSettings {
                args: vec!["-v".to_owned()],
                ..SshSettings::default()
            },
        ];
        for settings in changed {
            assert_ne!(ConfigStamp::read(&[], &settings), base, "{settings:?}");
        }
        // `ssh -G` does not depend on it.
        let direct = SshSettings {
            multiplex: false,
            ..SshSettings::default()
        };
        assert_eq!(ConfigStamp::read(&[], &direct), base);
    }

    #[test]
    fn converts_resolved_hosts() {
        let resolved =
            ResolvedHost::parse("user deploy\nhostname 10.0.0.5\nport 2222\nproxyjump bastion\n")
                .unwrap();
        let cached = CachedHost::from(&resolved);
        assert_eq!(
            cached,
            CachedHost {
                proxy_jump: Some("bastion".to_owned()),
                ..host("deploy", 2222)
            }
        );
        assert_eq!(cached.address(), resolved.address());
        assert_eq!(host("deploy", 22).address(), "deploy@10.0.0.5");
    }
}
