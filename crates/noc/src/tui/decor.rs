//! What goes in front of names: Nerd Font icons, or mc's type markers without them, as
//! [`glyphs`](super::glyphs) has them.

use std::path::Path;

use noc_vfs::{DirEntry, FileKind, VolumeKind};

use super::glyphs::{Glyphs, SPINNER};
use super::panel::HostStatus;

/// Icons on (`ui.icons`) or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Decor {
    icons: bool,
}

impl Decor {
    pub(crate) fn new(icons: bool) -> Self {
        Self { icons }
    }

    /// Whether prefixes end in an icon rather than an mc marker.
    pub(crate) fn icons(self) -> bool {
        self.icons
    }

    fn glyphs(self) -> &'static Glyphs {
        Glyphs::of(self.icons)
    }

    /// The prefix of an entry's name: its icon and a space, or its mc marker.
    pub(crate) fn entry(self, entry: &DirEntry) -> String {
        let glyph = self.kind(entry);
        if self.icons {
            format!("{glyph} ")
        } else {
            glyph.to_string()
        }
    }

    /// The prefix of the `..` row.
    pub(crate) fn parent(self) -> &'static str {
        self.glyphs().parent
    }

    /// The prefix of the home directory in the location menu.
    pub(crate) fn home(self) -> &'static str {
        self.glyphs().home
    }

    /// The prefix of a volume in the location menu: a disk, a network drive, or mc's marker of
    /// block devices.
    pub(crate) fn volume(self, kind: VolumeKind) -> &'static str {
        match kind {
            VolumeKind::Network => self.glyphs().network_volume,
            VolumeKind::System | VolumeKind::Local => self.glyphs().volume,
        }
    }

    /// The prefix of a host, which tells its status in one cell, as every prefix takes one:
    /// with icons a server, a server with a cross after a failure, or the spinner while
    /// connecting, which `tick` turns; without them a circle, a dot, a cross, or the spinner.
    /// The theme colors it by the status.
    pub(crate) fn host(self, status: HostStatus, tick: u64) -> String {
        let hosts = self.glyphs().hosts;
        let marker = match status {
            HostStatus::Connecting => spinner(tick),
            HostStatus::Idle => hosts.idle,
            HostStatus::Connected => hosts.connected,
            HostStatus::Failed | HostStatus::Stalled => hosts.failed,
        };
        format!("{marker} ")
    }

    /// What tells the kind of a symlink's `target`, of `kind` (`None` when nothing is there):
    /// with icons, its icon and a space in front of it, devicons' for a file whose name it
    /// knows; without them, mc's marker after it where `ls -F` puts one, and `!` after nothing.
    pub(crate) fn target(self, kind: Option<FileKind>, target: &[u8]) -> (String, String) {
        let kinds = self.glyphs().kinds;
        if self.icons {
            let icon = match kind {
                None => kinds.broken_link,
                Some(FileKind::Dir) => kinds.dir,
                Some(FileKind::Symlink) => kinds.link,
                Some(FileKind::Fifo) => kinds.fifo,
                Some(FileKind::Socket) => kinds.socket,
                Some(FileKind::CharDevice) => kinds.char_device,
                Some(FileKind::BlockDevice) => kinds.block_device,
                Some(FileKind::File | FileKind::Unknown) => {
                    let name = target.rsplit(|&byte| byte == b'/').next().unwrap_or(target);
                    file_icon(name).unwrap_or(kinds.file)
                }
            };
            return (format!("{icon} "), String::new());
        }
        let marker = match kind {
            None => Some(kinds.broken_link),
            Some(FileKind::Dir) => Some(kinds.dir),
            Some(FileKind::Fifo) => Some(kinds.fifo),
            Some(FileKind::Socket) => Some(kinds.socket),
            Some(_) => None,
        };
        (String::new(), marker.map(String::from).unwrap_or_default())
    }

    /// The icon or the marker of an entry's kind; with icons, devicons' for a file it knows.
    fn kind(self, entry: &DirEntry) -> char {
        let kinds = self.glyphs().kinds;
        match entry.metadata.kind {
            FileKind::Dir => kinds.dir,
            FileKind::Symlink => match entry.target_kind {
                Some(FileKind::Dir) => kinds.link_to_dir,
                Some(_) => kinds.link,
                None => kinds.broken_link,
            },
            FileKind::Fifo => kinds.fifo,
            FileKind::Socket => kinds.socket,
            FileKind::CharDevice => kinds.char_device,
            FileKind::BlockDevice => kinds.block_device,
            FileKind::File | FileKind::Unknown if self.icons => {
                file_icon(&entry.name).unwrap_or(if is_executable(entry) {
                    kinds.executable
                } else {
                    kinds.file
                })
            }
            FileKind::File if is_executable(entry) => kinds.executable,
            FileKind::File | FileKind::Unknown => kinds.file,
        }
    }
}

/// Frame `tick` of the spinner shown while connecting.
pub(crate) fn spinner(tick: u64) -> char {
    SPINNER[usize::try_from(tick % 4).unwrap_or(0)]
}

/// The devicons glyph for a file name, if it knows the name or its extension.
fn file_icon(name: &[u8]) -> Option<char> {
    let name = std::str::from_utf8(name).ok()?;
    // devicons asks the file system whether a name it does not know is a directory. A path
    // with a NUL byte names nothing, so the answer is no without asking: drawing stays free of
    // I/O, and a remote name is never looked up as a local path.
    let path = format!("\0/{name}");
    let icon = devicons::icon_for_file(Path::new(&path), &None).icon;
    (icon != devicons::FileIcon::default().icon).then_some(icon)
}

fn is_executable(entry: &DirEntry) -> bool {
    entry
        .metadata
        .permissions
        .is_some_and(|bits| bits & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use noc_vfs::Metadata;

    use super::*;

    fn entry(
        name: &str,
        kind: FileKind,
        permissions: u32,
        target_kind: Option<FileKind>,
    ) -> DirEntry {
        DirEntry {
            name: name.as_bytes().to_vec(),
            metadata: Metadata {
                kind,
                size: Some(1),
                permissions: Some(permissions),
                modified: None,
                uid: None,
                gid: None,
            },
            target_kind,
            link_target: None,
        }
    }

    fn file(name: &str) -> DirEntry {
        entry(name, FileKind::File, 0o644, None)
    }

    #[test]
    fn markers_follow_mc() {
        let cases = [
            (entry("d", FileKind::Dir, 0o755, None), "/"),
            (
                entry("l", FileKind::Symlink, 0o777, Some(FileKind::Dir)),
                "~",
            ),
            (
                entry("l", FileKind::Symlink, 0o777, Some(FileKind::File)),
                "@",
            ),
            (entry("l", FileKind::Symlink, 0o777, None), "!"),
            (entry("run", FileKind::File, 0o755, None), "*"),
            (entry("p", FileKind::Fifo, 0o644, None), "|"),
            (entry("s", FileKind::Socket, 0o644, None), "="),
            (entry("c", FileKind::CharDevice, 0o644, None), "-"),
            (entry("b", FileKind::BlockDevice, 0o644, None), "+"),
            (file("notes.txt"), " "),
        ];
        let decor = Decor::new(false);
        for (entry, marker) in cases {
            assert_eq!(decor.entry(&entry), marker, "{}", entry.display_name());
        }
        assert_eq!(decor.parent(), "/");
    }

    #[test]
    fn icons_come_from_the_kind_then_devicons() {
        let decor = Decor::new(true);
        assert_eq!(decor.entry(&entry("d", FileKind::Dir, 0o755, None)), " ");
        assert_eq!(decor.entry(&file("main.rs")), " ");
        assert_eq!(decor.entry(&file("Cargo.toml")), " ", "by extension");
        assert_eq!(decor.entry(&file("no-such-kind.zzz")), " ");
        assert_eq!(
            decor.entry(&entry("run", FileKind::File, 0o755, None)),
            " "
        );
        assert_eq!(decor.entry(&file("caf\u{fffd}")), " ");
        let mut latin1 = file("x");
        latin1.name = b"caf\xe9.rs".to_vec();
        assert_eq!(decor.entry(&latin1), " ", "not UTF-8");
        assert_eq!(decor.parent(), " ");
    }

    #[test]
    fn the_target_of_a_link_has_the_icon_of_its_kind_or_a_marker_after_it() {
        let icons = Decor::new(true);
        let before = |kind, target: &str| {
            let (prefix, suffix) = icons.target(kind, target.as_bytes());
            assert_eq!(suffix, "", "{target}");
            prefix
        };
        let dir = entry("d", FileKind::Dir, 0o755, None);
        assert_eq!(before(Some(FileKind::Dir), "/opt"), icons.entry(&dir));
        assert_eq!(
            before(Some(FileKind::File), "../etc/Cargo.toml"),
            icons.entry(&file("Cargo.toml")),
            "by the last name of the path"
        );
        assert_eq!(
            before(Some(FileKind::File), "/usr/bin/zz"),
            icons.entry(&file("zz"))
        );
        let broken = entry("l", FileKind::Symlink, 0o777, None);
        assert_eq!(before(None, "missing"), icons.entry(&broken));
        let fifo = entry("p", FileKind::Fifo, 0o644, None);
        assert_eq!(before(Some(FileKind::Fifo), "/run/p"), icons.entry(&fifo));

        let plain = Decor::new(false);
        let after = |kind, target: &str| {
            let (prefix, suffix) = plain.target(kind, target.as_bytes());
            assert_eq!(prefix, "", "{target}");
            suffix
        };
        assert_eq!(after(Some(FileKind::Dir), "/opt"), "/");
        assert_eq!(after(None, "missing"), "!");
        assert_eq!(after(Some(FileKind::Fifo), "p"), "|");
        assert_eq!(after(Some(FileKind::Socket), "s"), "=");
        assert_eq!(after(Some(FileKind::File), "a.txt"), "", "as ls -F");
        assert_eq!(after(Some(FileKind::CharDevice), "/dev/null"), "");
    }

    #[test]
    fn file_names_are_never_looked_up_on_the_local_disk() {
        // The tests run in the crate directory, where `src` is a directory; a remote file of
        // that name is still a file.
        assert!(Path::new("src").is_dir());
        // Asked directly, devicons looks at the disk and answers with its folder.
        assert_eq!(devicons::icon_for_file(Path::new("src"), &None).icon, '');
        assert_eq!(file_icon(b"src"), None);
    }

    #[test]
    fn hosts_show_their_status() {
        let plain = Decor::new(false);
        assert_eq!(plain.host(HostStatus::Connected, 0), "● ");
        assert_eq!(plain.host(HostStatus::Idle, 0), "○ ");
        assert_eq!(plain.host(HostStatus::Failed, 0), "✗ ");
        assert_eq!(plain.host(HostStatus::Stalled, 0), "✗ ");
        let spun: Vec<String> = (0..5)
            .map(|tick| plain.host(HostStatus::Connecting, tick))
            .collect();
        assert_eq!(spun, ["| ", "/ ", "- ", "\\ ", "| "]);
        assert_eq!(plain.volume(VolumeKind::System), "+ ");
        assert_eq!(plain.home(), "~ ");
        // With icons, the icon itself tells the status, so every prefix is one cell wide.
        let icons = Decor::new(true);
        assert_eq!(icons.host(HostStatus::Idle, 0), "󰒋 ");
        assert_eq!(icons.host(HostStatus::Connected, 0), "󰒋 ");
        assert_eq!(icons.host(HostStatus::Failed, 0), "󰒑 ");
        assert_eq!(icons.host(HostStatus::Connecting, 1), "/ ");
        assert_eq!(icons.volume(VolumeKind::Local), "󰋊 ");
        assert_eq!(icons.volume(VolumeKind::Network), "󰣳 ");
        assert_eq!(icons.home(), "󰋜 ");
        for prefix in [icons.parent(), icons.home()] {
            assert_eq!(prefix.chars().count(), 2, "{prefix:?}");
        }
    }
}
