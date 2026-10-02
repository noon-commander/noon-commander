use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use futures_util::future::join_all;

/// A mounted file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub mount_point: PathBuf,
    /// The file system's label: on macOS the volume name, on Linux from `/dev/disk/by-label`.
    pub label: Option<String>,
    /// Such as `apfs`, `ext4`, or `smbfs`; `None` when unknown.
    pub fs_type: Option<String>,
    pub kind: VolumeKind,
    /// `None` when the file system did not answer in time or could not tell.
    pub space: Option<Space>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeKind {
    /// The file system mounted at `/`.
    System,
    /// Another local file system.
    Local,
    /// A network file system (NFS, SMB, AFP, sshfs, …).
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    /// Bytes in total.
    pub total: u64,
    /// Bytes an unprivileged user can still write (`f_bavail`).
    pub available: u64,
}

/// The volumes: the system volume first, then the others by mount point.
///
/// Never fails: problems are logged and the list holds what could be found; `/` is always
/// present. Each file system is asked for its details in parallel, and each answer is awaited
/// at most `timeout`. A file system that does not answer in time, or whose earlier query is
/// still pending (a dead network mount can block forever), stays in the list without
/// [`Volume::space`]; on macOS it is also assumed to be a network file system of unknown type.
pub async fn volumes(timeout: Duration) -> Vec<Volume> {
    #[cfg(target_os = "macos")]
    let mut volumes = macos::volumes(timeout).await;
    #[cfg(not(target_os = "macos"))]
    let mut volumes = {
        let candidates = match tokio::task::spawn_blocking(candidates).await {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::warn!(%error, "listing mount points failed");
                Vec::new()
            }
        };
        statvfs_volumes(candidates, timeout).await
    };
    finish(&mut volumes);
    volumes
}

/// Adds `/` if it is missing and sorts: the system volume first, then by mount point.
fn finish(volumes: &mut Vec<Volume>) {
    if !volumes
        .iter()
        .any(|volume| volume.mount_point == Path::new("/"))
    {
        volumes.push(Volume {
            mount_point: PathBuf::from("/"),
            label: None,
            fs_type: None,
            kind: VolumeKind::System,
            space: None,
        });
    }
    volumes.sort_by(|a, b| {
        (a.kind != VolumeKind::System)
            .cmp(&(b.kind != VolumeKind::System))
            .then_with(|| a.mount_point.cmp(&b.mount_point))
    });
}

/// Mount points whose probe has not returned yet, possibly long after it timed out.
static PROBING: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// An entry in [`PROBING`], removed on drop.
struct InFlight(PathBuf);

impl InFlight {
    fn claim(path: &Path) -> Option<Self> {
        let mut probing = PROBING.lock().unwrap_or_else(PoisonError::into_inner);
        probing
            .insert(path.to_path_buf())
            .then(|| Self(path.to_path_buf()))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        PROBING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// Runs `probe` on `path` on a blocking thread, waiting at most `timeout`. `None` when it timed
/// out, when an earlier probe of `path` is still running, or when it panicked.
async fn probe<T, F>(path: PathBuf, timeout: Duration, probe: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce(&Path) -> T + Send + 'static,
{
    let Some(in_flight) = InFlight::claim(&path) else {
        tracing::debug!(path = %path.display(), "previous file system query still pending");
        return None;
    };
    let task = tokio::task::spawn_blocking(move || {
        let result = probe(&in_flight.0);
        drop(in_flight);
        result
    });
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(result)) => Some(result),
        Ok(Err(error)) => {
            tracing::warn!(path = %path.display(), %error, "file system query failed");
            None
        }
        Err(_) => {
            tracing::warn!(path = %path.display(), "file system query timed out");
            None
        }
    }
}

/// A mount point to probe with `statvfs`, used everywhere but on macOS.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    mount_point: PathBuf,
    label: Option<String>,
    fs_type: Option<String>,
    kind: VolumeKind,
}

#[cfg(target_os = "linux")]
fn candidates() -> Vec<Candidate> {
    linux::candidates()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn candidates() -> Vec<Candidate> {
    vec![Candidate {
        mount_point: PathBuf::from("/"),
        label: None,
        fs_type: None,
        kind: VolumeKind::System,
    }]
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
async fn statvfs_volumes(candidates: Vec<Candidate>, timeout: Duration) -> Vec<Volume> {
    join_all(candidates.into_iter().map(|candidate| async move {
        let space = match probe(candidate.mount_point.clone(), timeout, statvfs_space).await {
            Some(Ok(space)) => Some(space),
            Some(Err(error)) => {
                tracing::warn!(
                    path = %candidate.mount_point.display(),
                    %error,
                    "statvfs failed"
                );
                None
            }
            None => None,
        };
        Volume {
            mount_point: candidate.mount_point,
            label: candidate.label,
            fs_type: candidate.fs_type,
            kind: candidate.kind,
            space,
        }
    }))
    .await
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn statvfs_space(path: &Path) -> io::Result<Space> {
    let stat = rustix::fs::statvfs(path)?;
    let fragment = if stat.f_frsize == 0 {
        stat.f_bsize
    } else {
        stat.f_frsize
    };
    Ok(Space {
        total: stat.f_blocks.saturating_mul(fragment),
        available: stat.f_bavail.saturating_mul(fragment),
    })
}

/// Mount discovery from `/proc/self/mountinfo`. Only plain std code, so that it is compiled
/// and tested on every platform even though only Linux uses it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux {
    use std::collections::{HashMap, HashSet};
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    use std::path::{Path, PathBuf};

    use super::{Candidate, VolumeKind};

    const NETWORK_FS: &[&str] = &[
        "9p",
        "afs",
        "ceph",
        "cifs",
        "coda",
        "davfs",
        "fuse.glusterfs",
        "fuse.rclone",
        "fuse.s3fs",
        "fuse.sshfs",
        "glusterfs",
        "lustre",
        "ncpfs",
        "nfs",
        "nfs4",
        "smb3",
        "smbfs",
    ];

    const PSEUDO_FS: &[&str] = &[
        "autofs",
        "binfmt_misc",
        "bpf",
        "cgroup",
        "cgroup2",
        "configfs",
        "debugfs",
        "devpts",
        "devtmpfs",
        "efivarfs",
        "fuse.gvfsd-fuse",
        "fuse.lxcfs",
        "fuse.portal",
        "fusectl",
        "hugetlbfs",
        "mqueue",
        "nsfs",
        "overlay",
        "proc",
        "pstore",
        "ramfs",
        "rpc_pipefs",
        "securityfs",
        "selinuxfs",
        "squashfs",
        "sysfs",
        "tmpfs",
        "tracefs",
    ];

    const HIDDEN_PREFIXES: &[&str] = &[
        "/boot",
        "/dev",
        "/efi",
        "/proc",
        "/run",
        "/snap",
        "/sys",
        "/var/lib/containers",
        "/var/lib/docker",
        "/var/lib/snapd",
        "/var/snap",
    ];

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct MountInfo {
        pub(super) mount_point: PathBuf,
        pub(super) fs_type: String,
        pub(super) source: PathBuf,
    }

    pub(super) fn candidates() -> Vec<Candidate> {
        let mountinfo = match fs::read("/proc/self/mountinfo") {
            Ok(mountinfo) => mountinfo,
            Err(error) => {
                tracing::warn!(%error, "reading /proc/self/mountinfo failed");
                return Vec::new();
            }
        };
        let labels = read_labels(Path::new("/dev/disk/by-label"));
        to_candidates(parse_mountinfo(&mountinfo), &labels)
    }

    /// The mounts to show, with their labels from `labels` (canonical device path → label).
    pub(super) fn to_candidates(
        mounts: Vec<MountInfo>,
        labels: &HashMap<PathBuf, String>,
    ) -> Vec<Candidate> {
        last_per_mount_point(mounts)
            .into_iter()
            .filter(keep)
            .map(|mount| {
                let label = if is_device(&mount.source) {
                    fs::canonicalize(&mount.source)
                        .ok()
                        .and_then(|device| labels.get(&device).cloned())
                } else {
                    None
                };
                let kind = if mount.mount_point == Path::new("/") {
                    VolumeKind::System
                } else if is_network(&mount.fs_type) {
                    VolumeKind::Network
                } else {
                    VolumeKind::Local
                };
                Candidate {
                    mount_point: mount.mount_point,
                    label,
                    fs_type: Some(mount.fs_type),
                    kind,
                }
            })
            .collect()
    }

    /// Maps the canonical paths of the devices linked from `dir` to the link names (labels).
    pub(super) fn read_labels(dir: &Path) -> HashMap<PathBuf, String> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::debug!(dir = %dir.display(), %error, "no disk labels");
                return HashMap::new();
            }
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let device = fs::canonicalize(entry.path()).ok()?;
                Some((device, unescape_label(&entry.file_name().to_string_lossy())))
            })
            .collect()
    }

    pub(super) fn parse_mountinfo(text: &[u8]) -> Vec<MountInfo> {
        text.split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .filter_map(parse_line)
            .collect()
    }

    /// `id parent major:minor root mount-point options [optional…] - fs-type source super-options`
    fn parse_line(line: &[u8]) -> Option<MountInfo> {
        let mut fields = line.split(|&byte| byte == b' ');
        let mount_point = fields.nth(4)?;
        let mut tail = fields.skip_while(|field| *field != b"-");
        tail.next()?;
        let fs_type = tail.next()?;
        let source = tail.next()?;
        Some(MountInfo {
            mount_point: PathBuf::from(OsString::from_vec(unescape_octal(mount_point))),
            fs_type: String::from_utf8_lossy(&unescape_octal(fs_type)).into_owned(),
            source: PathBuf::from(OsString::from_vec(unescape_octal(source))),
        })
    }

    /// Undoes the kernel's `\NNN` (octal) escaping of space, tab, newline, and backslash.
    pub(super) fn unescape_octal(field: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(field.len());
        let mut i = 0;
        while i < field.len() {
            if field[i] == b'\\' {
                let byte = field.get(i + 1..i + 4).and_then(|digits| {
                    digits
                        .iter()
                        .try_fold(0_u16, |value, &digit| {
                            (b'0'..=b'7')
                                .contains(&digit)
                                .then(|| value * 8 + u16::from(digit - b'0'))
                        })
                        .and_then(|value| u8::try_from(value).ok())
                });
                if let Some(byte) = byte {
                    out.push(byte);
                    i += 4;
                    continue;
                }
            }
            out.push(field[i]);
            i += 1;
        }
        out
    }

    /// Undoes udev's `\xHH` escaping in `/dev/disk/by-label` names.
    pub(super) fn unescape_label(name: &str) -> String {
        let bytes = name.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'x') {
                let byte = bytes
                    .get(i + 2..i + 4)
                    .filter(|digits| digits.iter().all(u8::is_ascii_hexdigit))
                    .and_then(|digits| std::str::from_utf8(digits).ok())
                    .and_then(|digits| u8::from_str_radix(digits, 16).ok());
                if let Some(byte) = byte {
                    out.push(byte);
                    i += 4;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Of mounts stacked on the same mount point, only the last one is visible.
    pub(super) fn last_per_mount_point(mounts: Vec<MountInfo>) -> Vec<MountInfo> {
        let mut seen = HashSet::new();
        let mut last: Vec<MountInfo> = mounts
            .into_iter()
            .rev()
            .filter(|mount| seen.insert(mount.mount_point.clone()))
            .collect();
        last.reverse();
        last
    }

    /// `/`, network file systems, and file systems on devices, except system ones.
    pub(super) fn keep(mount: &MountInfo) -> bool {
        let path = &mount.mount_point;
        if path == Path::new("/") {
            return true;
        }
        let hidden = HIDDEN_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
            && !path.starts_with("/run/media");
        if hidden || PSEUDO_FS.contains(&mount.fs_type.as_str()) {
            return false;
        }
        is_network(&mount.fs_type) || is_device(&mount.source)
    }

    pub(super) fn is_network(fs_type: &str) -> bool {
        NETWORK_FS.contains(&fs_type)
    }

    fn is_device(source: &Path) -> bool {
        source.as_os_str().as_bytes().starts_with(b"/dev/")
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{OsString, c_char};
    use std::fs;
    use std::io;
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use futures_util::future::join_all;

    use super::{Space, Volume, VolumeKind, probe};

    const MNT_LOCAL: u32 = 0x0000_1000;
    /// Set on mounts that are not meant to be browsed, such as Time Machine local snapshots.
    const MNT_DONTBROWSE: u32 = 0x0010_0000;

    pub(super) async fn volumes(timeout: Duration) -> Vec<Volume> {
        let candidates =
            match tokio::task::spawn_blocking(|| candidates(Path::new("/Volumes"))).await {
                Ok(candidates) => candidates,
                Err(error) => {
                    tracing::warn!(%error, "listing /Volumes failed");
                    vec![(PathBuf::from("/"), None)]
                }
            };
        let volumes = join_all(candidates.into_iter().map(|(path, label)| async move {
            let stat = probe(path.clone(), timeout, stat).await;
            to_volume(path, label, stat)
        }))
        .await;
        volumes.into_iter().flatten().collect()
    }

    /// `/` and the entries of `dir` (`/Volumes`) that are not symlinks, with their labels. A
    /// symlink to `/` in `dir` names the system volume.
    ///
    /// Only reads `dir` itself: the entry types come from the directory listing, so mounted file
    /// systems are not touched.
    pub(super) fn candidates(dir: &Path) -> Vec<(PathBuf, Option<String>)> {
        let mut root_label = None;
        let mut others = Vec::new();
        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.filter_map(Result::ok) {
                    let Ok(file_type) = entry.file_type() else {
                        continue;
                    };
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if file_type.is_symlink() {
                        if root_label.is_none()
                            && fs::read_link(entry.path())
                                .is_ok_and(|target| target == Path::new("/"))
                        {
                            root_label = Some(name);
                        }
                    } else if file_type.is_dir() {
                        others.push((entry.path(), Some(name)));
                    }
                }
            }
            Err(error) => tracing::warn!(dir = %dir.display(), %error, "reading volumes failed"),
        }
        std::iter::once((PathBuf::from("/"), root_label))
            .chain(others)
            .collect()
    }

    #[derive(Debug)]
    pub(super) struct Stat {
        mount_on: PathBuf,
        fs_type: String,
        flags: u32,
        space: Space,
    }

    pub(super) fn stat(path: &Path) -> io::Result<Stat> {
        let stat = rustix::fs::statfs(path)?;
        let block = u64::from(stat.f_bsize);
        Ok(Stat {
            mount_on: PathBuf::from(OsString::from_vec(c_bytes(&stat.f_mntonname))),
            fs_type: String::from_utf8_lossy(&c_bytes(&stat.f_fstypename)).into_owned(),
            flags: stat.f_flags,
            space: Space {
                total: stat.f_blocks.saturating_mul(block),
                available: stat.f_bavail.saturating_mul(block),
            },
        })
    }

    fn c_bytes(chars: &[c_char]) -> Vec<u8> {
        chars
            .iter()
            .map(|&c| c.to_ne_bytes()[0])
            .take_while(|&byte| byte != 0)
            .collect()
    }

    /// The volume at `path`, or `None` when it is not one to show. `stat` is `None` when the
    /// probe did not finish.
    pub(super) fn to_volume(
        path: PathBuf,
        label: Option<String>,
        stat: Option<io::Result<Stat>>,
    ) -> Option<Volume> {
        let is_root = path == Path::new("/");
        let system_or = |kind| if is_root { VolumeKind::System } else { kind };
        match stat {
            // Only network file systems hang.
            None => Some(Volume {
                kind: system_or(VolumeKind::Network),
                mount_point: path,
                label,
                fs_type: None,
                space: None,
            }),
            Some(Err(error)) => {
                tracing::warn!(path = %path.display(), %error, "statfs failed");
                is_root.then_some(Volume {
                    mount_point: path,
                    label,
                    fs_type: None,
                    kind: VolumeKind::System,
                    space: None,
                })
            }
            Some(Ok(stat)) => {
                if !is_root && (stat.mount_on != path || stat.flags & MNT_DONTBROWSE != 0) {
                    return None;
                }
                let kind = if stat.flags & MNT_LOCAL == 0 {
                    VolumeKind::Network
                } else {
                    VolumeKind::Local
                };
                Some(Volume {
                    kind: system_or(kind),
                    mount_point: path,
                    label,
                    fs_type: Some(stat.fs_type),
                    space: Some(stat.space),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::linux::{
        MountInfo, keep, last_per_mount_point, parse_mountinfo, read_labels, to_candidates,
        unescape_label, unescape_octal,
    };
    use super::*;

    fn mount(mount_point: &str, fs_type: &str, source: &str) -> MountInfo {
        MountInfo {
            mount_point: PathBuf::from(mount_point),
            fs_type: fs_type.to_owned(),
            source: PathBuf::from(source),
        }
    }

    fn volume(mount_point: &str, kind: VolumeKind) -> Volume {
        Volume {
            mount_point: PathBuf::from(mount_point),
            label: None,
            fs_type: None,
            kind,
            space: None,
        }
    }

    #[test]
    fn parses_mountinfo() {
        let text = b"22 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw\n\
            30 22 0:5 / /proc rw,nosuid - proc proc rw\n\
            40 22 8:17 / /media/My\\040Disk rw master:3 shared:4 - vfat /dev/sdb1 rw\n\
            41 22 0:50 / /mnt/a\\134b\\011c rw - fuse.sshfs me@host:/srv rw\n\
            garbage line\n";
        assert_eq!(
            parse_mountinfo(text),
            [
                mount("/", "ext4", "/dev/sda2"),
                mount("/proc", "proc", "proc"),
                mount("/media/My Disk", "vfat", "/dev/sdb1"),
                mount("/mnt/a\\b\tc", "fuse.sshfs", "me@host:/srv"),
            ]
        );
    }

    #[test]
    fn unescapes_octal() {
        assert_eq!(unescape_octal(b"a\\040b\\012"), b"a b\n");
        assert_eq!(unescape_octal(b"\\134\\134"), b"\\\\");
        assert_eq!(unescape_octal(b"\\09x\\4"), b"\\09x\\4");
        assert_eq!(unescape_octal(b"\\777"), b"\\777");
    }

    #[test]
    fn unescapes_labels() {
        assert_eq!(unescape_label("My\\x20Disk"), "My Disk");
        assert_eq!(unescape_label("a\\x2fb\\x5c"), "a/b\\");
        assert_eq!(unescape_label("\\xzz\\x2"), "\\xzz\\x2");
        assert_eq!(unescape_label("plain"), "plain");
    }

    #[test]
    fn last_mount_wins() {
        let mounts = vec![
            mount("/", "ext4", "/dev/sda2"),
            mount("/mnt", "ext4", "/dev/sdb1"),
            mount("/home", "ext4", "/dev/sda3"),
            mount("/mnt", "tmpfs", "tmpfs"),
        ];
        assert_eq!(
            last_per_mount_point(mounts),
            [
                mount("/", "ext4", "/dev/sda2"),
                mount("/home", "ext4", "/dev/sda3"),
                mount("/mnt", "tmpfs", "tmpfs"),
            ]
        );
    }

    #[test]
    fn keeps_root_devices_and_network_mounts() {
        assert!(keep(&mount("/", "overlay", "overlay")));
        assert!(keep(&mount("/home", "ext4", "/dev/sda3")));
        assert!(keep(&mount("/run/media/me/USB", "vfat", "/dev/sdb1")));
        assert!(keep(&mount("/mnt/nas", "nfs4", "nas:/export")));
        assert!(keep(&mount("/mnt/share", "cifs", "//nas/share")));
        assert!(keep(&mount("/home/me/remote", "fuse.sshfs", "me@host:")));
    }

    #[test]
    fn drops_system_mounts() {
        assert!(!keep(&mount("/proc", "proc", "proc")));
        assert!(!keep(&mount("/tmp", "tmpfs", "tmpfs")));
        assert!(!keep(&mount("/snap/core/1", "squashfs", "/dev/loop0")));
        assert!(!keep(&mount("/boot", "ext4", "/dev/sda1")));
        assert!(!keep(&mount("/boot/efi", "vfat", "/dev/sda1")));
        assert!(!keep(&mount("/efi", "vfat", "/dev/sda1")));
        assert!(!keep(&mount(
            "/run/user/1000/gvfs",
            "fuse.gvfsd-fuse",
            "gvfsd-fuse"
        )));
        assert!(!keep(&mount("/run/foo", "ext4", "/dev/sdc1")));
        assert!(!keep(&mount(
            "/var/lib/docker/overlay2/x",
            "ext4",
            "/dev/sda2"
        )));
        assert!(!keep(&mount("/mnt/loop", "squashfs", "/dev/loop1")));
        assert!(!keep(&mount("/srv/zfs", "zfs", "pool/srv")));
        assert!(!keep(&mount("/devices", "ext4", "LABEL=x")));
    }

    #[test]
    fn hidden_prefixes_match_whole_components() {
        assert!(keep(&mount("/devdata", "ext4", "/dev/sdc1")));
        assert!(keep(&mount("/running", "ext4", "/dev/sdc1")));
    }

    #[test]
    fn reads_escaped_labels() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let devices = dir.path().join("dev");
        let by_label = dir.path().join("by-label");
        std::fs::create_dir(&devices)?;
        std::fs::create_dir(&by_label)?;
        std::fs::write(devices.join("sdb1"), "")?;
        std::os::unix::fs::symlink("../dev/sdb1", by_label.join("My\\x20Disk"))?;
        std::os::unix::fs::symlink("../dev/missing", by_label.join("Gone"))?;

        let labels = read_labels(&by_label);
        let device = std::fs::canonicalize(devices.join("sdb1"))?;
        assert_eq!(labels, HashMap::from([(device, "My Disk".to_owned())]));
        assert!(read_labels(&dir.path().join("nope")).is_empty());
        Ok(())
    }

    #[test]
    fn builds_candidates() -> io::Result<()> {
        // Sources are canonicalized before the label lookup, so they must exist: `/dev/null`
        // stands in for a labelled device.
        let labels = HashMap::from([(std::fs::canonicalize("/dev/null")?, "USB".to_owned())]);
        let mounts = vec![
            mount("/", "ext4", "/dev/noc-test-missing"),
            mount("/proc", "proc", "proc"),
            mount("/mnt/nas", "nfs", "nas:/x"),
            mount("/media/usb", "vfat", "/dev/null"),
        ];
        let candidate = |path: &str, label: Option<&str>, fs_type: &str, kind| Candidate {
            mount_point: PathBuf::from(path),
            label: label.map(str::to_owned),
            fs_type: Some(fs_type.to_owned()),
            kind,
        };
        assert_eq!(
            to_candidates(mounts, &labels),
            [
                candidate("/", None, "ext4", VolumeKind::System),
                candidate("/mnt/nas", None, "nfs", VolumeKind::Network),
                candidate("/media/usb", Some("USB"), "vfat", VolumeKind::Local),
            ]
        );
        Ok(())
    }

    #[test]
    fn finish_adds_root_and_sorts() {
        let mut volumes = vec![
            volume("/mnt/b", VolumeKind::Network),
            volume("/mnt/a", VolumeKind::Local),
        ];
        finish(&mut volumes);
        assert_eq!(
            volumes,
            [
                volume("/", VolumeKind::System),
                volume("/mnt/a", VolumeKind::Local),
                volume("/mnt/b", VolumeKind::Network),
            ]
        );

        let mut volumes = vec![
            volume("/Volumes/USB", VolumeKind::Local),
            volume("/", VolumeKind::System),
            volume("/Volumes/NAS", VolumeKind::Network),
        ];
        finish(&mut volumes);
        assert_eq!(
            volumes,
            [
                volume("/", VolumeKind::System),
                volume("/Volumes/NAS", VolumeKind::Network),
                volume("/Volumes/USB", VolumeKind::Local),
            ]
        );
    }

    #[tokio::test]
    async fn statvfs_reports_space() -> io::Result<()> {
        // Not `/`: a concurrent test probing it would find it pending.
        let dir = tempfile::tempdir()?;
        let volumes = statvfs_volumes(
            vec![Candidate {
                mount_point: dir.path().to_path_buf(),
                label: Some("Root".to_owned()),
                fs_type: None,
                kind: VolumeKind::System,
            }],
            Duration::from_secs(5),
        )
        .await;
        let [volume] = volumes.as_slice() else {
            panic!("expected one volume: {volumes:?}");
        };
        let space = volume.space.expect("space of the temporary directory");
        assert!(space.total > 0 && space.available <= space.total);
        assert_eq!(volume.label.as_deref(), Some("Root"));
        Ok(())
    }

    #[tokio::test]
    async fn pending_probe_is_not_repeated() {
        let path = PathBuf::from("/noc-test/pending-probe");
        let (release, released) = std::sync::mpsc::channel::<()>();
        let first = probe(path.clone(), Duration::from_millis(20), move |_| {
            released.recv().ok();
        })
        .await;
        assert!(first.is_none(), "the first probe should time out");

        let called = Arc::new(AtomicBool::new(false));
        let second = probe(path.clone(), Duration::from_secs(5), {
            let called = Arc::clone(&called);
            move |_| called.store(true, Ordering::SeqCst)
        })
        .await;
        assert!(second.is_none());
        assert!(
            !called.load(Ordering::SeqCst),
            "a pending path must not be probed"
        );

        release.send(()).expect("release the first probe");
        for _ in 0..500 {
            if !PROBING
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(&path)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(probe(path, Duration::from_secs(5), |_| 7).await, Some(7));
    }

    #[cfg(target_os = "macos")]
    mod macos {
        use super::super::macos::{candidates, stat, to_volume};
        use super::*;

        #[tokio::test]
        async fn lists_the_system_volume_first() {
            let volumes = volumes(Duration::from_secs(5)).await;
            let root = volumes.first().expect("at least /");
            assert_eq!(root.mount_point, Path::new("/"));
            assert_eq!(root.kind, VolumeKind::System);
            assert!(root.space.is_some());
            assert!(root.label.is_some(), "/Volumes should link to /");
            assert_eq!(
                volumes
                    .iter()
                    .filter(|volume| volume.kind == VolumeKind::System)
                    .count(),
                1
            );
        }

        #[test]
        fn plain_directory_is_not_a_volume() -> io::Result<()> {
            let dir = tempfile::tempdir()?;
            let path = dir.path().to_path_buf();
            assert_eq!(to_volume(path.clone(), None, Some(stat(&path))), None);
            Ok(())
        }

        #[test]
        fn timed_out_volume_is_kept_as_network() {
            let path = PathBuf::from("/Volumes/NAS");
            assert_eq!(
                to_volume(path, Some("NAS".to_owned()), None),
                Some(Volume {
                    mount_point: PathBuf::from("/Volumes/NAS"),
                    label: Some("NAS".to_owned()),
                    fs_type: None,
                    kind: VolumeKind::Network,
                    space: None,
                })
            );
        }

        #[test]
        fn candidates_skip_symlinks_and_name_the_root() -> io::Result<()> {
            let dir = tempfile::tempdir()?;
            std::os::unix::fs::symlink("/", dir.path().join("Macintosh HD"))?;
            std::os::unix::fs::symlink("/tmp", dir.path().join("Elsewhere"))?;
            std::fs::create_dir(dir.path().join("USB"))?;
            std::fs::write(dir.path().join("file"), "")?;
            assert_eq!(
                candidates(dir.path()),
                [
                    (PathBuf::from("/"), Some("Macintosh HD".to_owned())),
                    (dir.path().join("USB"), Some("USB".to_owned())),
                ]
            );
            Ok(())
        }

        #[test]
        fn missing_volumes_directory_still_yields_root() {
            assert_eq!(
                candidates(Path::new("/noc-test/no-such-dir")),
                [(PathBuf::from("/"), None)]
            );
        }
    }
}
