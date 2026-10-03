//! Quick cd, Alt-C, as in mc: a path typed as for `cd` in a shell, read from the directory of
//! the active panel.

use std::path::{Component, Path, PathBuf};

use noc_vfs::{Location, RemotePath};

/// Where `cd <text>` leads from `here`, as a shell reads it, without asking the file system:
///
/// - `-` is `previous`, the directory the panel showed before;
/// - `host:path` is on that host, scp-style: the text before the first `:` is not empty and
///   has no `/` (write `./a:b` for a local name with a colon); `host:` alone, `host:~` and
///   `host:~/…` start at its home directory, a relative path too;
/// - `~` and `~/…` are the home directory, `home` locally and the remote one on a host;
/// - `/…` is absolute on the panel's side: local, or on its host;
/// - anything else is relative to `here`; from the volumes and hosts, to `home`.
///
/// `.` and `..` are resolved by name, as a shell's `cd` does by default. `None` for empty
/// text, and for `-` without a directory before.
pub(crate) fn target(
    text: &str,
    here: &Location,
    home: &Path,
    previous: Option<&Location>,
) -> Option<Location> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text == "-" {
        return previous.cloned();
    }
    if let Location::Remote { host, path } = Location::parse(text) {
        return Some(remote(&host, &RemotePath::default(), path.as_bytes()));
    }
    match here {
        Location::Remote { host, path } => Some(remote(host, path, text.as_bytes())),
        Location::Local(dir) => Some(Location::Local(local(dir, home, text))),
        Location::Root | Location::Sftp => Some(Location::Local(local(home, home, text))),
    }
}

/// `text` from the local directory `dir`.
fn local(dir: &Path, home: &Path, text: &str) -> PathBuf {
    let (base, rest) = match text.strip_prefix('~') {
        Some("") => (home, ""),
        Some(rest) if rest.starts_with('/') => (home, &rest[1..]),
        _ => (dir, text),
    };
    let mut path = base.to_path_buf();
    for component in Path::new(rest).components() {
        match component {
            Component::RootDir => path = PathBuf::from("/"),
            Component::ParentDir => {
                path.pop();
            }
            Component::Normal(name) => path.push(name),
            Component::CurDir | Component::Prefix(_) => {}
        }
    }
    path
}

/// `text` from the directory `dir` on `host`; the empty path is its home directory.
fn remote(host: &str, dir: &RemotePath, text: &[u8]) -> Location {
    let (mut parts, rest): (Vec<&[u8]>, &[u8]) = match text {
        b"~" => (Vec::new(), b""),
        _ if text.starts_with(b"~/") => (Vec::new(), &text[2..]),
        _ if text.starts_with(b"/") => (Vec::new(), text),
        _ => (components(dir.as_bytes()), text),
    };
    let absolute = if text.starts_with(b"/") {
        true
    } else {
        text != b"~" && !text.starts_with(b"~/") && dir.is_absolute()
    };
    for part in components(rest) {
        match part {
            b"." => {}
            // Above the start of a relative path, the server finds out where that is.
            b".." if parts.last().is_none_or(|last| *last == b"..") && !absolute => {
                parts.push(part);
            }
            b".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    let mut path = Vec::new();
    if absolute {
        path.push(b'/');
    }
    path.extend_from_slice(&parts.join(&b'/'));
    Location::Remote {
        host: host.to_owned(),
        path: RemotePath::from(path),
    }
}

/// The names in a path, without empty ones.
fn components(path: &[u8]) -> Vec<&[u8]> {
    path.split(|&byte| byte == b'/')
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/me";

    fn local_at(path: &str) -> Location {
        Location::Local(PathBuf::from(path))
    }

    fn remote_at(host: &str, path: &str) -> Location {
        Location::Remote {
            host: host.to_owned(),
            path: RemotePath::from(path),
        }
    }

    fn cd(text: &str, here: &Location) -> Option<Location> {
        target(text, here, Path::new(HOME), None)
    }

    #[test]
    fn local_paths_read_as_a_shell_reads_them() {
        let here = local_at("/srv/www");
        for (text, expected) in [
            ("logs", "/srv/www/logs"),
            ("  logs/ ", "/srv/www/logs"),
            ("./a/./b", "/srv/www/a/b"),
            ("..", "/srv"),
            ("../..", "/"),
            ("../../../..", "/"),
            ("../api/v1", "/srv/api/v1"),
            ("/etc", "/etc"),
            ("/etc/../var//log", "/var/log"),
            ("~", HOME),
            ("~/src/..", HOME),
            ("~/src", "/home/me/src"),
            ("~user", "/srv/www/~user"),
            ("./a:b", "/srv/www/a:b"),
            ("dir with space", "/srv/www/dir with space"),
        ] {
            assert_eq!(cd(text, &here), Some(local_at(expected)), "{text}");
        }
        assert_eq!(cd("", &here), None);
        assert_eq!(cd("   ", &here), None);
    }

    #[test]
    fn host_prefixes_open_hosts_scp_style() {
        let here = local_at("/srv");
        assert_eq!(cd("web:", &here), Some(remote_at("web", "")));
        assert_eq!(
            cd("web:/var/log", &here),
            Some(remote_at("web", "/var/log"))
        );
        assert_eq!(cd("web:~", &here), Some(remote_at("web", "")));
        assert_eq!(cd("web:~/app", &here), Some(remote_at("web", "app")));
        assert_eq!(cd("web:app/../lib", &here), Some(remote_at("web", "lib")));
        assert_eq!(
            cd("a/b:c", &here),
            Some(local_at("/srv/a/b:c")),
            "a slash before the colon is local"
        );
    }

    #[test]
    fn on_a_host_paths_stay_on_it() {
        let here = remote_at("web", "/var/www");
        assert_eq!(cd("../log", &here), Some(remote_at("web", "/var/log")));
        assert_eq!(cd("/etc", &here), Some(remote_at("web", "/etc")));
        assert_eq!(cd("../../..", &here), Some(remote_at("web", "/")));
        assert_eq!(cd("~", &here), Some(remote_at("web", "")));
        assert_eq!(cd("~/app", &here), Some(remote_at("web", "app")));
        assert_eq!(cd("db:/srv", &here), Some(remote_at("db", "/srv")));
        // From the remote home directory, before the server says where it is.
        let home = remote_at("web", "");
        assert_eq!(cd("app", &home), Some(remote_at("web", "app")));
        assert_eq!(cd("..", &home), Some(remote_at("web", "..")));
        assert_eq!(cd("../../x/..", &home), Some(remote_at("web", "../..")));
    }

    #[test]
    fn from_the_volumes_and_hosts_paths_start_at_home() {
        assert_eq!(cd("src", &Location::Root), Some(local_at("/home/me/src")));
        assert_eq!(cd("/tmp", &Location::Sftp), Some(local_at("/tmp")));
        assert_eq!(cd("web:", &Location::Root), Some(remote_at("web", "")));
    }

    #[test]
    fn a_dash_goes_back() {
        let here = local_at("/srv");
        let before = remote_at("web", "/var");
        assert_eq!(
            target("-", &here, Path::new(HOME), Some(&before)),
            Some(before)
        );
        assert_eq!(cd("-", &here), None);
        assert_eq!(cd("-x", &here), Some(local_at("/srv/-x")));
    }
}
