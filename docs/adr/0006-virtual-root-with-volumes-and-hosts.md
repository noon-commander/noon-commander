# 0006. A virtual root of volumes and a list of hosts

- Status: Accepted
- Date: 2026-10-02

## Context

The virtual root was made for an SFTP-only tool: one `[Local]` row that opened the home
directory, then every host from the ssh config. Now that Noon Commander is a full file manager
([ADR 0005](0005-rename-to-noon-commander.md)), the root has to show the mounted volumes
(external disks, disk images, network shares) as well. More backends may follow behind the `Vfs`
trait, and long ssh configs would push the volumes off the screen. Far Manager users also expect
a quick menu to change a panel's drive (Alt+F1, Alt+F2) that lists the same places.

Finding the volumes and how big they are touches the file systems themselves: `statfs` on a dead
NFS or SMB mount can block in the kernel forever.

## Decision

- The root lists `Home` first, which opens the home directory on macOS and Linux alike, then
  the mounted volumes, the system volume (`/`) on top, named by the file system's label or else
  its mount point. Every volume opens at its mount point, the system volume at `/` too: a row
  that looks like `/` and opened the home directory, as `[Local]` did, would be the only one
  that does not go where it says, and on Linux with a separate `/home` it would lead onto
  another volume. The home directory has a row of its own instead, with the space of the volume
  that holds it. No row is called "Local": the name means nothing on a
  machine with several disks.
- The hosts from the ssh config are one row of the root, `SFTP`, which opens like a directory:
  `Location::Sftp`, with `..` back to the root. Hosts that are connected or connecting show
  again below that row, so the servers in use stay one keystroke away. Plugins for other
  backends will be rows of the root of the same kind.
- `..` follows paths, as in mc: from `/Volumes/USB` to `/Volumes`, from a local `/` to the root,
  from a remote `/` to the list of hosts.
- `noc-vfs::volumes` lists the volumes without blocking: on macOS `/` and the entries of
  `/Volumes`, leaving out those marked `nobrowse`; on Linux `/proc/self/mountinfo`, leaving out
  pseudo and system file systems. Each volume's size and free space comes from a blocking call
  on a thread of its own, awaited for at most 500 ms; a volume that does not answer is listed
  without them and is not asked again until the earlier call returns. `[volumes] hide` leaves
  out mount points by pattern.
- Alt+F1 and Alt+F2 (and Ctrl+x 1, Ctrl+x 2, for terminals without them) open a location menu
  over the left or right panel: `Home`, the volumes, then every host, with `1` … `0` as hotkeys while
  its filter is empty and typing to filter. It reads the same listing as the root.

## Consequences

- The root shows volumes on both platforms with no new dependencies and no `unsafe`; what
  macOS's Finder hides stays hidden.
- Reaching a host that is not connected takes one more Enter in the panel; the location menu
  and the connected hosts in the root make up for it.
- A dead network mount makes a listing of the root wait up to 500 ms, once; after that it is
  skipped until it answers.
- Volumes are read when the root or the menu is listed (and on Ctrl+r), not watched; a disk
  that is plugged in shows up on the next listing.
- The F9 pull-down menu offers the location menu too, as Left and Right → Change location.
