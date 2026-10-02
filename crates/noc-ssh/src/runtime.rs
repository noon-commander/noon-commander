//! Files in the runtime directory: control sockets and askpass sockets.
//!
//! Names carry the owner's pid (`cm-<pid>-<hex>`, `ap-<pid>-<hex>`) so that a later instance
//! can tell leftovers of a crashed instance from sockets of a running one.

use std::fmt::Write as _;
use std::io;
use std::path::Path;
use std::time::Duration;

use rustix::process::{Pid, test_kill_process};

use crate::command::{SshSettings, control_command};

/// Prefix of control socket names.
pub(crate) const CONTROL_PREFIX: &str = "cm";
/// Prefix of askpass socket names.
pub(crate) const ASKPASS_PREFIX: &str = "ap";

/// Size of `sockaddr_un::sun_path`, including the terminating NUL.
const SUN_PATH_LEN: usize = if cfg!(target_os = "linux") { 108 } else { 104 };

/// Characters ssh appends to the control path for its temporary socket (`.` + 16 random).
const SSH_TEMP_SUFFIX_LEN: usize = 17;

/// Longest control path ssh can still bind on this platform.
pub(crate) const MAX_CONTROL_PATH_LEN: usize = SUN_PATH_LEN - 1 - SSH_TEMP_SUFFIX_LEN;

/// Longest path for a socket we bind ourselves.
pub(crate) const MAX_SOCKET_PATH_LEN: usize = SUN_PATH_LEN - 1;

/// A fresh socket name for this process, e.g. `cm-4242-9f3a01bc`.
pub(crate) fn socket_name(prefix: &str) -> io::Result<String> {
    let random = getrandom::u32()
        .map_err(|error| io::Error::other(format!("random number generator: {error}")))?;
    Ok(format!("{prefix}-{}-{random:08x}", std::process::id()))
}

/// `count` random bytes as lowercase hex.
pub(crate) fn random_hex(count: usize) -> io::Result<String> {
    let mut bytes = vec![0; count];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(format!("random number generator: {error}")))?;
    let mut hex = String::with_capacity(count * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

fn owner_pid(name: &str) -> Option<(&str, i32)> {
    let mut parts = name.splitn(3, '-');
    let prefix = parts.next()?;
    let pid = parts.next()?.parse().ok()?;
    parts.next()?;
    Some((prefix, pid))
}

fn is_running(pid: i32) -> bool {
    let Some(pid) = Pid::from_raw(pid) else {
        return false;
    };
    // EPERM means the process exists but belongs to someone else.
    !matches!(test_kill_process(pid), Err(rustix::io::Errno::SRCH))
}

/// Cleans up after Noon Commander instances that are no longer running: asks orphaned masters to
/// exit and removes their sockets. Sockets of running instances are left alone.
pub async fn cleanup_stale(runtime_dir: &Path, settings: &SshSettings) {
    let Ok(mut entries) = tokio::fs::read_dir(runtime_dir).await else {
        return;
    };
    let own_pid = std::process::id();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let Some((prefix, pid)) = name.to_str().and_then(owner_pid) else {
            continue;
        };
        if u32::try_from(pid) == Ok(own_pid) || is_running(pid) {
            continue;
        }
        let path = entry.path();
        if prefix == CONTROL_PREFIX {
            let mut command = control_command(settings, &path, "exit");
            let _ = tokio::time::timeout(Duration::from_secs(5), command.status()).await;
        } else if prefix != ASKPASS_PREFIX {
            continue;
        }
        tracing::debug!(path = %path.display(), "removing stale socket");
        let _ = tokio::fs::remove_file(&path).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_names_carry_the_pid() {
        let name = socket_name(CONTROL_PREFIX).unwrap();
        let (prefix, pid) = owner_pid(&name).unwrap();
        assert_eq!(prefix, CONTROL_PREFIX);
        assert_eq!(u32::try_from(pid).unwrap(), std::process::id());
        assert_eq!(name.len(), "cm-".len() + pid.to_string().len() + 1 + 8);
        assert_ne!(name, socket_name(CONTROL_PREFIX).unwrap());
    }

    #[test]
    fn parses_owner_pids() {
        assert_eq!(owner_pid("cm-42-0badf00d"), Some(("cm", 42)));
        assert_eq!(owner_pid("ap-7-00000000"), Some(("ap", 7)));
        assert_eq!(owner_pid("cm-x-0badf00d"), None);
        assert_eq!(owner_pid("cm-42"), None);
        assert_eq!(owner_pid("README"), None);
    }

    #[test]
    fn random_hex_has_the_requested_length() {
        let hex = random_hex(16).unwrap();
        assert_eq!(hex.len(), 32);
        assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn detects_running_processes() {
        assert!(is_running(i32::try_from(std::process::id()).unwrap()));
        assert!(!is_running(0));
    }

    #[tokio::test]
    async fn removes_only_stale_askpass_sockets() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join(socket_name(ASKPASS_PREFIX).unwrap());
        let stale = dir.path().join("ap-999999999-00000000");
        let unrelated = dir.path().join("notes.txt");
        for path in [&live, &stale, &unrelated] {
            std::fs::write(path, b"").unwrap();
        }
        cleanup_stale(dir.path(), &SshSettings::default()).await;
        assert!(live.exists());
        assert!(!stale.exists());
        assert!(unrelated.exists());
    }
}
