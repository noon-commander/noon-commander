use std::convert::Infallible;
use std::path::PathBuf;
use std::str::FromStr;

use crate::RemotePath;

/// A place a panel can show: a local directory, or a remote directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
    /// A path on the local file system. Relative paths, including the empty path, are relative
    /// to the current directory.
    Local(PathBuf),
    /// A path on a remote host. Relative paths, including the empty path, are relative to the
    /// remote home directory.
    Remote {
        /// The destination as given to `ssh`, usually a host alias from the ssh config.
        host: String,
        /// The path on the host.
        path: RemotePath,
    },
}

impl Location {
    /// Parses a command-line location, scp-style: `host:path` is remote when the text before
    /// the first ':' is non-empty and contains no '/'; anything else is a local path.
    /// `host:` (empty path) means the remote home directory (empty [`RemotePath`]).
    ///
    /// Write `./a:b` for a local file whose name contains a colon.
    pub fn parse(arg: &str) -> Self {
        match arg.split_once(':') {
            Some((host, path)) if !host.is_empty() && !host.contains('/') => Self::Remote {
                host: host.to_owned(),
                path: RemotePath::from(path),
            },
            _ => Self::Local(PathBuf::from(arg)),
        }
    }

    /// `..` semantics: the lexical parent on the same file system; `None` for a local `/`, a
    /// remote `/`, and remote paths without a parent (the empty path, which is the home
    /// directory).
    ///
    /// Like [`Path::parent`](std::path::Path::parent), the parent of a single relative component
    /// such as `a` is the empty path.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        match self {
            Self::Local(path) => path
                .parent()
                .map(|parent| Self::Local(parent.to_path_buf())),
            Self::Remote { host, path } => path.parent().map(|parent| Self::Remote {
                host: host.clone(),
                path: parent,
            }),
        }
    }

    /// Whether there is a [`parent`](Self::parent), without making it.
    pub fn has_parent(&self) -> bool {
        match self {
            Self::Local(path) => path.parent().is_some(),
            Self::Remote { path, .. } => path.as_bytes().iter().any(|&byte| byte != b'/'),
        }
    }
}

impl FromStr for Location {
    type Err = Infallible;

    /// Same as [`Location::parse`].
    fn from_str(arg: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(arg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(path: &str) -> Location {
        Location::Local(PathBuf::from(path))
    }

    fn remote(host: &str, path: &str) -> Location {
        Location::Remote {
            host: host.to_owned(),
            path: RemotePath::from(path),
        }
    }

    #[test]
    fn parses_remote_locations() {
        assert_eq!(Location::parse("host:/x"), remote("host", "/x"));
        assert_eq!(Location::parse("host:"), remote("host", ""));
        assert_eq!(Location::parse("host:rel"), remote("host", "rel"));
        assert_eq!(Location::parse("host:a:b"), remote("host", "a:b"));
        assert_eq!(Location::parse("user@host:/x"), remote("user@host", "/x"));
        assert_eq!(Location::parse("C:foo"), remote("C", "foo"));
    }

    #[test]
    fn parses_local_locations() {
        assert_eq!(Location::parse("/abs"), local("/abs"));
        assert_eq!(Location::parse("rel"), local("rel"));
        assert_eq!(Location::parse("./a:b"), local("./a:b"));
        assert_eq!(Location::parse("a/b:c"), local("a/b:c"));
        assert_eq!(Location::parse("/x:y"), local("/x:y"));
        assert_eq!(Location::parse(":foo"), local(":foo"));
        assert_eq!(Location::parse(""), local(""));
        assert_eq!("host:/x".parse(), Ok(remote("host", "/x")));
    }

    #[test]
    fn local_parents() {
        assert_eq!(local("/").parent(), None);
        assert_eq!(local("/a").parent(), Some(local("/")));
        assert_eq!(local("/a/b").parent(), Some(local("/a")));
        assert_eq!(local("/a/b/").parent(), Some(local("/a")));
        assert_eq!(local("a").parent(), Some(local("")));
        assert_eq!(local("").parent(), None);
        for path in ["/", "/a", "/a/b/", "a", ""] {
            assert_eq!(local(path).has_parent(), local(path).parent().is_some());
        }
    }

    #[test]
    fn remote_parents() {
        assert_eq!(remote("h", "/").parent(), None);
        assert_eq!(remote("h", "/a").parent(), Some(remote("h", "/")));
        assert_eq!(remote("h", "/a/b/").parent(), Some(remote("h", "/a")));
        assert_eq!(remote("h", "a/b").parent(), Some(remote("h", "a")));
        assert_eq!(remote("h", "a").parent(), Some(remote("h", "")));
        assert_eq!(remote("h", "").parent(), None);
        for path in ["/", "//", "/a", "/a/b/", "a", ""] {
            let location = remote("h", path);
            assert_eq!(
                location.has_parent(),
                location.parent().is_some(),
                "{path:?}"
            );
        }
    }
}
