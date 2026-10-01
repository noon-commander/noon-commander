use std::borrow::Cow;
use std::ffi::OsStr;
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// A path on a remote host: raw bytes separated by `/`. May be relative (to the remote home).
///
/// SFTP v3 does not define an encoding for names, so paths are bytes and are displayed lossily.
/// The empty path (the default) is the remote home directory.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RemotePath(Vec<u8>);

impl RemotePath {
    /// A path from raw bytes.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// The root directory, `/`.
    pub fn root() -> Self {
        Self(b"/".to_vec())
    }

    /// The raw bytes of the path.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The raw bytes of the path.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Whether the path starts at `/`.
    pub fn is_absolute(&self) -> bool {
        self.0.starts_with(b"/")
    }

    /// Appends `name` with a single `/` in between. Like [`Path::join`], an absolute `name`
    /// replaces the path; an empty `name` leaves it unchanged.
    #[must_use]
    pub fn join(&self, name: &[u8]) -> Self {
        if self.0.is_empty() || name.starts_with(b"/") {
            return Self(name.to_vec());
        }
        if name.is_empty() {
            return self.clone();
        }
        let mut bytes = Vec::with_capacity(self.0.len() + 1 + name.len());
        bytes.extend_from_slice(&self.0);
        if !bytes.ends_with(b"/") {
            bytes.push(b'/');
        }
        bytes.extend_from_slice(name);
        Self(bytes)
    }

    /// The path without its last component, ignoring trailing slashes: `/a/b` → `/a`,
    /// `/a` → `/`, `a` → the empty path. `None` for `/` and the empty path.
    pub fn parent(&self) -> Option<Self> {
        let path = trim_trailing_slashes(&self.0);
        if path.is_empty() {
            return None;
        }
        let parent = match path.iter().rposition(|&b| b == b'/') {
            None => Self::default(),
            Some(slash) => match trim_trailing_slashes(&path[..slash]) {
                [] => Self::root(),
                head => Self(head.to_vec()),
            },
        };
        Some(parent)
    }

    /// The last component, ignoring trailing slashes. `None` for `/` and the empty path.
    pub fn file_name(&self) -> Option<&[u8]> {
        let path = trim_trailing_slashes(&self.0);
        if path.is_empty() {
            return None;
        }
        let start = path
            .iter()
            .rposition(|&b| b == b'/')
            .map_or(0, |slash| slash + 1);
        Some(&path[start..])
    }

    /// The path for display, with invalid UTF-8 replaced by `U+FFFD`.
    pub fn display(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    /// The same bytes as a [`Path`], for APIs that take one. It is still a remote path.
    pub fn as_path(&self) -> &Path {
        Path::new(OsStr::from_bytes(&self.0))
    }
}

fn trim_trailing_slashes(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|&b| b != b'/')
        .map_or(0, |last| last + 1);
    &bytes[..end]
}

impl fmt::Debug for RemotePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.display(), f)
    }
}

impl fmt::Display for RemotePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.display(), f)
    }
}

impl From<&str> for RemotePath {
    fn from(path: &str) -> Self {
        Self::new(path)
    }
}

impl From<String> for RemotePath {
    fn from(path: String) -> Self {
        Self::new(path)
    }
}

impl From<&[u8]> for RemotePath {
    fn from(path: &[u8]) -> Self {
        Self::new(path)
    }
}

impl From<Vec<u8>> for RemotePath {
    fn from(path: Vec<u8>) -> Self {
        Self::new(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> RemotePath {
        RemotePath::from(s)
    }

    fn parent(s: &str) -> Option<String> {
        path(s).parent().map(|p| p.display().into_owned())
    }

    fn file_name(s: &str) -> Option<Vec<u8>> {
        path(s).file_name().map(<[u8]>::to_vec)
    }

    #[test]
    fn root_and_empty() {
        let root = RemotePath::root();
        assert_eq!(root.as_bytes(), b"/");
        assert!(root.is_absolute());
        assert_eq!(root.parent(), None);
        assert_eq!(root.file_name(), None);

        let home = RemotePath::default();
        assert!(home.as_bytes().is_empty());
        assert!(!home.is_absolute());
        assert_eq!(home.parent(), None);
        assert_eq!(home.file_name(), None);
    }

    #[test]
    fn absolute_and_relative() {
        assert!(path("/a").is_absolute());
        assert!(!path("a/b").is_absolute());
        assert!(!path("./a").is_absolute());
    }

    #[test]
    fn join() {
        assert_eq!(path("/").join(b"a"), path("/a"));
        assert_eq!(path("").join(b"a"), path("a"));
        assert_eq!(path("/a").join(b"b"), path("/a/b"));
        assert_eq!(path("/a/").join(b"b"), path("/a/b"));
        assert_eq!(path("a").join(b"b"), path("a/b"));
        assert_eq!(path("/a").join(b"/b"), path("/b"));
        assert_eq!(path("/a").join(b""), path("/a"));
        assert_eq!(path("/a").join(b"b\xff"), RemotePath::new(b"/a/b\xff"));
    }

    #[test]
    fn parents() {
        assert_eq!(parent("/a/b").as_deref(), Some("/a"));
        assert_eq!(parent("/a").as_deref(), Some("/"));
        assert_eq!(parent("/"), None);
        assert_eq!(parent("//"), None);
        assert_eq!(parent("a/b").as_deref(), Some("a"));
        assert_eq!(parent("a").as_deref(), Some(""));
        assert_eq!(parent(""), None);
        assert_eq!(parent("/a/b/").as_deref(), Some("/a"));
        assert_eq!(parent("/a//b").as_deref(), Some("/a"));
        assert_eq!(parent("//a").as_deref(), Some("/"));
        assert_eq!(parent("a/").as_deref(), Some(""));
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name("/a/b"), Some(b"b".to_vec()));
        assert_eq!(file_name("/a/b/"), Some(b"b".to_vec()));
        assert_eq!(file_name("/a"), Some(b"a".to_vec()));
        assert_eq!(file_name("a"), Some(b"a".to_vec()));
        assert_eq!(file_name("a/b"), Some(b"b".to_vec()));
        assert_eq!(file_name("/"), None);
        assert_eq!(file_name("//"), None);
        assert_eq!(file_name(""), None);
    }

    #[test]
    fn non_utf8_is_displayed_lossily() {
        let path = RemotePath::new(b"/caf\xe9".to_vec());
        assert_eq!(path.display(), "/caf\u{fffd}");
        assert_eq!(path.to_string(), "/caf\u{fffd}");
        assert_eq!(format!("{path:?}"), "\"/caf\u{fffd}\"");
        assert_eq!(path.as_path().as_os_str().as_bytes(), b"/caf\xe9");
        assert_eq!(path.file_name(), Some(&b"caf\xe9"[..]));
    }

    #[test]
    fn conversions() {
        assert_eq!(RemotePath::from("/a").as_bytes(), b"/a");
        assert_eq!(RemotePath::from(String::from("/a")).as_bytes(), b"/a");
        assert_eq!(RemotePath::from(&b"/a"[..]).as_bytes(), b"/a");
        assert_eq!(RemotePath::from(b"/a".to_vec()).into_bytes(), b"/a");
        assert_eq!(path("/a b").as_path(), Path::new("/a b"));
    }
}
