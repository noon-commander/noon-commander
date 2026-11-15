# 0075. The location menu shows a card of the place under the cursor

- Status: Accepted; amended by
  [ADR 0076](0076-ssh-g-for-the-host-under-the-cursor-of-the-location-menu.md),
  [ADR 0079](0079-ssh-g-results-are-not-cached.md)
- Date: 2026-11-15
- Amends: [ADR 0041](0041-lists-always-show-the-whole-row.md),
  [ADR 0021](0021-location-menu-replaces-the-virtual-root.md)

## Context

Under its rows the location menu showed the whole row under the cursor in two lines
([ADR 0041](0041-lists-always-show-the-whole-row.md)): `Home  /Users/me`, `USB  /Volumes/USB`,
`web  Prod  deploy@10.0.0.5`. Most of it was in the row already. What the menu knows of a place
and the row has no room for showed nowhere: what a volume mounts, the type of its file system and
the share of it free; where a host is defined, the host ssh jumps through to reach it, why its last
attempt failed, how many tabs are on it, and where opening it goes. The reason a connection failed
was said once, in the dialog that asked to retry, and was gone after Cancel.

## Decision

- **A card in place of the whole row**, as the command history has
  ([ADR 0072](0072-the-command-history-shows-where-a-command-ran.md)): on the line over it, at its
  right end, as a title on the frame in the color of the window's title but not bold
  (`Body.footer_label`), what kind of place it is or where it is defined; under it two lines, what
  it is, then how it is. Every card has two lines, so the box keeps its height as the cursor
  moves, and the skeleton of the rows takes them while the rows are to come, as it took those of
  the whole row (`Body.has_full`).
- **The icon of the row in front of the first line**, in the row's colors: the home, the disk or
  the network drive of a volume, the server of a host in the color of its status, its spinner while
  it connects, the chevron of the row of the rest; mc's markers without icons. The second line
  stands under the text of the first, as the history's second stands under its first, or, where it
  says where opening a host goes, after a folder, the history's (`glyphs.ran.dir`). A first version
  put the host's marker in front of its second line and none in front of the first: the server
  stood in front of a directory, and the host had no icon.
- **The home directory**: the volume that holds it and the type of its file system over the card;
  its path, which loses its start where it is too long; then its space.
- **A volume**: the type of its file system and its kind, `system`, `local`, or `network`, over
  the card; what it mounts and where, as `mount` says it, `/dev/disk4s1 on /Volumes/USB`, which
  loses its middle; then its space: `12.0G free of 64.0G (18%)`, the share free, as the frame of a
  panel tells it; a skeleton while it is still to come
  ([ADR 0074](0074-the-location-menu-shows-its-rows-before-the-volumes-answer.md));
  `Did not answer in 5 s` once given up for; `Tells no free space` for a volume that tells none.
  What a volume mounts comes with it from `noc-vfs` (`Volume::source`): on macOS from the same
  `statfs` as its space, so it is still to come with it; on Linux from `/proc/self/mountinfo`. A
  password in the user part of a URL there, as `//me:secret@nas/share`, is left out in `noc-vfs`,
  so that it never reaches the screen.
- **A host**: the file and the line of its `Host` line in the ssh config over the card, with `~`
  for the home directory; its alias and its address, with the host ssh jumps through,
  `web · deploy@10.0.0.5 via bastion`, or a skeleton until `ssh -G` tells them
  ([ADR 0076](0076-ssh-g-for-the-host-under-the-cursor-of-the-location-menu.md)); then
  `Connected` with the tabs of both panels on it; `Connecting`; why its last attempt failed or its
  connection was lost, on one line, in the colors of errors, until a new attempt starts; or, not
  connected, after a folder, where opening it goes: the last directory shown on it with
  `remember_dir`, else its `start_dir`, else the remote home, and the other panel's `other_dir`.
- **The row of the rest of a group**: the names of the places it stands for.
- Since when a host is connected, and when it was last, do not show: a time shows with its whole
  date ([ADR 0035](0035-formats-of-the-region.md)), too long for what it tells here, and the second
  would need `recent.toml` to keep times.

## Consequences

- Everything the whole row showed is in the card: the whole name of a place, its mount point or
  path, its alias, label, and address.
- The menu keeps its height; nothing new is asked of a volume or a host.
- The reason a connection failed stays known after its dialog closes (`App::failed` keeps it), and
  shows in the menu.
- `KnownHost` carries the file and the line of its `Host` line, and carried the `ProxyJump` of its
  cached `ssh -G` until [ADR 0079](0079-ssh-g-results-are-not-cached.md); `HostState` the reason
  of a failure, the tabs on the host, and where it opens.
