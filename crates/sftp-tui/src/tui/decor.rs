//! What goes in front of names: Nerd Font icons, or mc's type markers without them.

use std::path::Path;

use sftp_tui_vfs::{DirEntry, FileKind};

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

    /// The prefix of an entry's name: its icon and a space, or its mc marker.
    pub(crate) fn entry(self, entry: &DirEntry) -> String {
        if self.icons {
            format!("{} ", icon(entry))
        } else {
            marker(entry).to_string()
        }
    }

    /// The prefix of the `..` row.
    pub(crate) fn parent(self) -> &'static str {
        if self.icons { " " } else { "/" }
    }

    /// The prefix of the local file system in the virtual root: no status, in line with the
    /// hosts.
    pub(crate) fn local(self) -> &'static str {
        if self.icons { "  󰌢 " } else { "  " }
    }

    /// The prefix of a host in the virtual root: its status, and with icons a server. `tick`
    /// turns the spinner while connecting.
    pub(crate) fn host(self, status: HostStatus, tick: u64) -> String {
        let status = match status {
            HostStatus::Idle => '○',
            HostStatus::Connecting => spinner(tick),
            HostStatus::Connected => '●',
            HostStatus::Failed => '✗',
        };
        if self.icons {
            format!("{status} 󰒋 ")
        } else {
            format!("{status} ")
        }
    }
}

/// Frame `tick` of the spinner shown while connecting.
pub(crate) fn spinner(tick: u64) -> char {
    ['|', '/', '-', '\\'][usize::try_from(tick % 4).unwrap_or(0)]
}

/// mc's type marker: `/` directory, `~` link to a directory, `@` link, `!` broken link,
/// `*` executable, `|` FIFO, `=` socket, `-` character device, `+` block device.
fn marker(entry: &DirEntry) -> char {
    match entry.metadata.kind {
        FileKind::Dir => '/',
        FileKind::Symlink => match entry.target_kind {
            Some(FileKind::Dir) => '~',
            Some(_) => '@',
            None => '!',
        },
        FileKind::Fifo => '|',
        FileKind::Socket => '=',
        FileKind::CharDevice => '-',
        FileKind::BlockDevice => '+',
        FileKind::File if is_executable(entry) => '*',
        FileKind::File | FileKind::Unknown => ' ',
    }
}

fn icon(entry: &DirEntry) -> char {
    match entry.metadata.kind {
        FileKind::Dir => '',
        FileKind::Symlink => match entry.target_kind {
            Some(FileKind::Dir) => '',
            Some(_) => '',
            None => '',
        },
        FileKind::Fifo => '󰟥',
        FileKind::Socket => '󰐧',
        FileKind::CharDevice | FileKind::BlockDevice => '󰋊',
        FileKind::File | FileKind::Unknown => {
            file_icon(&entry.name).unwrap_or(if is_executable(entry) { '' } else { '' })
        }
    }
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
    use sftp_tui_vfs::Metadata;

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
        let spun: Vec<String> = (0..5)
            .map(|tick| plain.host(HostStatus::Connecting, tick))
            .collect();
        assert_eq!(spun, ["| ", "/ ", "- ", "\\ ", "| "]);
        assert_eq!(plain.local(), "  ");
        let icons = Decor::new(true);
        assert_eq!(icons.host(HostStatus::Connected, 0), "● 󰒋 ");
        assert_eq!(icons.local(), "  󰌢 ");
    }
}
