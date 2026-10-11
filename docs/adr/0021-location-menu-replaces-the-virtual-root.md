# 0021. The location menu replaces the virtual root

- Status: Accepted
- Date: 2026-10-12

## Context

[ADR 0006](0006-virtual-root-with-volumes-and-hosts.md) gave the panels a virtual root: `Home`,
the mounted volumes, a row `SFTP` that opened the list of hosts, and the hosts in use again. `..`
led there from a local `/`, and to the list of hosts from a remote `/` or the remote home. The
location menu of Alt+F1 and Alt+F2 listed the same places. In use, the root cost more than it
gave:

- It duplicated the menu. Going there took over a panel and its listing, and a keystroke or two
  more than the menu, which opens over the panel and leaves it where it is.
- It mixed places with directories. Its rows were not entries, so marks, `+`, `-`, and the
  F-keys had to be refused there or mean something else: F4 edited a host and F8 disconnected
  it. `..` meant the parent directory in one place and the list of places in another.
- A long ssh config made the list of hosts and the menu long, and the hosts in use and the
  volumes got lost among the others.
- Favorites, directories the user chooses, and plugins for other backends need a place. As rows
  of the root, each would be a new kind of row with its own meaning for every key.
- A lost connection sent the panels on the host back to the list of hosts, away from the
  directory the user worked in, and said why only in their status line.

## Decision

- **No virtual root and no list of hosts.** A `Location` is `Local(PathBuf)` or
  `Remote { host, path }`, so a panel always shows a real directory. A local `/` and a remote
  `/` have no `..` row, and going up from them does nothing; `Location::parent` returns `None`
  there.
- **The location menu is the one place to choose a volume or a host**: Alt+F1, Alt+F2, Ctrl+x 1,
  Ctrl+x 2, and F9 → Left or Right → Change location. Its rows are grouped under headings, in
  this order: Favorites, for now only `Home`, which opens the home directory, with the user's
  favorites to follow; Mounts, the volumes, the system volume first and the others by mount
  point; SFTP, the hosts of the ssh config in its order. Plugins for other backends will be a
  group of their own. A group without rows is not drawn.
- **The places used last come first** in Mounts and SFTP, then the others in their usual order.
  Each group shows its first ten places, a constant rather than a setting, and always the volume
  or host the panel is on and the hosts that are connected, connecting, or waiting for Retry.
  The rest are behind a last row, `N more…`, which the cursor and the mouse can select but which
  takes no digit; Enter on it shows the whole group until the menu closes. While the filter has
  text, it finds every place, hidden ones too, and the rows of more go away. While it is empty,
  `1` … `9` and `0` open the first ten places, skipping those rows.
- **`recent.toml`** in the XDG state directory, next to `history.toml`, keeps the hosts and
  volumes used last, most recent first, up to 50 of each. A host counts as used when a
  connection to it succeeds; a volume when a panel arrives in a directory on it in any way: the
  menu, Quick cd, zoxide, Enter, or a workspace. `noc-config` writes the file whole and
  atomically, in the background. Mount points that are not UTF-8 are not kept. The app reads
  the volumes once at start, in the background, to know which volume a panel is on.
- **F4 and F8 move to the menu**, keymap context `location_menu` in both presets: F4 edits the
  settings of the host under the cursor, F8 disconnects it or stops connecting to it, and Ctrl+r
  reads the volumes and hosts again. The `root` context is gone. F9 → Command → Edit host
  settings… and Disconnect host act on the host the active panel shows.
- **Panels stay on their host.** A failed connection, the first attempt or a lost connection,
  opens a dialog with Retry, the default, and Cancel. While it is open, the panels that wait for
  the host keep waiting, with a spinner in their status line. On a lost connection the panels on
  the host drop their entries at once and wait to read the same directory, with the cursor
  where it was. Retry connects again, and the waiting panels read their directories once it is
  connected. Cancel after a first attempt leaves the panel where it was, with the reason in its
  status line; after a lost connection, the panels stay on the host without entries until
  Ctrl+r, or Enter on `..`, connects again. F8 on a connected host leaves its panels the same
  way, without a dialog.
- **Workspaces** no longer write `root` or `sftp`; read, both mean `~`. A location that
  `workspaces.toml` cannot hold is saved as the nearest directory above it that it can, or else
  as `~`.

This supersedes ADR 0006 in part: the root, the list of hosts, and `..` above `/` are gone. Its
volumes stand: `noc-vfs::volumes` finds them without blocking (`/Volumes` without `nobrowse`
mounts on macOS, the rules for `/proc/self/mountinfo` on Linux), each size is awaited for at most
500 ms, and `[volumes] hide` leaves out mount points by pattern.

## Consequences

- A volume or host is Alt+F1 (or Alt+F2) and a digit away, as before, and one past the first
  ten of its group a few letters of its name. The way up from `/` to the volumes is gone, and
  with it the extra Enter on `SFTP`.
- Every row of a panel but `..` is an entry, so every panel key means the same everywhere, and
  the code that refused keys on the root is gone.
- The order of the menu follows use, so a digit may open another place than it did the day
  before. The cursor still starts on the panel's place, and the filter does not depend on the
  order.
- A new state file, `recent.toml`. Deleting it only forgets the order.
- Panels on a lost host no longer jump elsewhere: they stay on the directory and come back
  with Retry or Ctrl+r. A failed connection asks a question, which waits in the queue of
  dialogs, rather than leaving a line in the status bar.
- The terminal and tab titles no longer show the name of the machine, which only the root had.
- Groups without rows take no room, so Favorites gets more rows, and Plugins a group, as these
  features land, without changing the rest of the menu.
- `noc ls` without an argument still prints what the menu offers: the mount points of the
  volumes and the hosts as `host:`.
