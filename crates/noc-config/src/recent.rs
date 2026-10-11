use std::borrow::Borrow;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

use crate::ConfigError;
use crate::hosts::read;
use crate::write::write_atomic;

/// The top of `recent.toml`, before the lists.
const HEADER: &str = "\
# Hosts and volumes that Noon Commander used last, most recent first, for its location menu.
# Noon Commander writes this file again on each change; comments are not kept. It is safe to
# delete.

";

/// How many entries each list of [`Recent`] keeps; older ones drop off.
pub const RECENT_MAX: usize = 50;

/// `recent.toml`: the SFTP hosts and local volumes used last, most recent first.
///
/// Unknown keys and types are errors, so a typo does not silently fall back to a default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recent {
    /// Aliases of SFTP hosts.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Mount points of local volumes. Those that are not UTF-8 are not written.
    #[serde(default, serialize_with = "utf8_paths")]
    pub volumes: Vec<PathBuf>,
}

impl Recent {
    /// Loads `recent.toml` from `path`; a missing file yields empty lists.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match read(path)? {
            Some(text) => toml::from_str(&text).map_err(|source| ConfigError::Parse {
                path: path.to_path_buf(),
                source: Box::new(source),
            }),
            None => Ok(Self::default()),
        }
    }

    /// Writes the lists to the file at `path`, whole: comments in it are not kept. It is
    /// replaced atomically, through a symbolic link if it is one.
    ///
    /// Blocking: call it from a blocking thread.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = self.to_text().map_err(|error| ConfigError::Write {
            path: path.to_path_buf(),
            source: io::Error::other(error),
        })?;
        write_atomic(path, text.as_bytes())
    }

    /// Makes the host `alias` the most recent. Returns whether the list changed: not if it
    /// already was.
    pub fn use_host(&mut self, alias: &str) -> bool {
        move_to_front(&mut self.hosts, alias)
    }

    /// Makes the volume at `mount_point` the most recent. Returns whether the list changed: not
    /// if it already was, nor if `mount_point` is not UTF-8, since TOML cannot hold it.
    pub fn use_volume(&mut self, mount_point: &Path) -> bool {
        mount_point.to_str().is_some() && move_to_front(&mut self.volumes, mount_point)
    }

    /// The text of `recent.toml`: the header, then the lists.
    fn to_text(&self) -> Result<String, toml::ser::Error> {
        Ok(format!("{HEADER}{}", toml::to_string(self)?))
    }
}

/// Moves `item` to the front of `list`, or inserts it there, and keeps the first
/// [`RECENT_MAX`]. Returns whether `list` changed.
fn move_to_front<T, Q>(list: &mut Vec<T>, item: &Q) -> bool
where
    T: Borrow<Q>,
    Q: PartialEq + ToOwned<Owned = T> + ?Sized,
{
    if list.first().is_some_and(|first| first.borrow() == item) {
        return false;
    }
    list.retain(|kept| kept.borrow() != item);
    list.insert(0, item.to_owned());
    list.truncate(RECENT_MAX);
    true
}

/// Serializes the UTF-8 paths of `paths`, leaving out the others, which serde would reject.
fn utf8_paths<S: Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(paths.iter().filter_map(|path| path.to_str()))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn non_utf8() -> PathBuf {
        PathBuf::from(OsStr::from_bytes(b"/Volumes/\xffUSB"))
    }

    #[test]
    fn a_missing_file_has_empty_lists() {
        let dir = tempfile::tempdir().unwrap();
        let recent = Recent::load(&dir.path().join("recent.toml")).unwrap();
        assert_eq!(recent, Recent::default());
    }

    #[test]
    fn reads_back_what_it_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/recent.toml");
        let recent = Recent {
            hosts: vec!["web".to_owned(), "db".to_owned()],
            volumes: vec![PathBuf::from("/Volumes/USB"), PathBuf::from("/")],
        };
        recent.save(&path).unwrap();
        assert_eq!(Recent::load(&path).unwrap(), recent);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Hosts and volumes"), "{text}");
        assert!(text.contains("\nhosts = [\"web\", \"db\"]\n"), "{text}");
        assert!(
            text.contains("\nvolumes = [\"/Volumes/USB\", \"/\"]\n"),
            "{text}"
        );
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn an_unknown_key_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.toml");
        std::fs::write(&path, "host = [\"web\"]\n").unwrap();
        assert!(matches!(
            Recent::load(&path),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn using_moves_to_the_front() {
        let mut recent = Recent::default();
        assert!(recent.use_host("web"));
        assert!(recent.use_host("db"));
        assert!(recent.use_host("web"));
        assert_eq!(recent.hosts, ["web", "db"]);
        assert!(!recent.use_host("web"), "it already is the most recent");
        assert_eq!(recent.hosts, ["web", "db"]);

        assert!(recent.use_volume(Path::new("/")));
        assert!(recent.use_volume(Path::new("/Volumes/USB")));
        assert!(!recent.use_volume(Path::new("/Volumes/USB")));
        assert!(recent.use_volume(Path::new("/")));
        assert_eq!(
            recent.volumes,
            [PathBuf::from("/"), PathBuf::from("/Volumes/USB")]
        );
    }

    #[test]
    fn keeps_the_most_recent() {
        let mut recent = Recent::default();
        for number in 0..RECENT_MAX + 5 {
            assert!(recent.use_host(&format!("host{number}")));
        }
        assert_eq!(recent.hosts.len(), RECENT_MAX);
        assert_eq!(recent.hosts[0], format!("host{}", RECENT_MAX + 4));
        assert_eq!(recent.hosts[RECENT_MAX - 1], "host5");
    }

    #[test]
    fn a_volume_that_is_not_utf8_is_left_out() {
        let mut recent = Recent::default();
        assert!(recent.use_volume(Path::new("/")));
        assert!(!recent.use_volume(&non_utf8()));
        assert_eq!(recent.volumes, [PathBuf::from("/")]);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.toml");
        recent.volumes.insert(0, non_utf8());
        recent.save(&path).unwrap();
        assert_eq!(Recent::load(&path).unwrap().volumes, [PathBuf::from("/")]);
    }
}
