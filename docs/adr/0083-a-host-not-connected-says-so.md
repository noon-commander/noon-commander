# 0083. A host not connected says so

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0075](0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md)

## Context

The second line of the card of a host in the location menu says how it is: `Connected`,
`Connecting…`, or why its last attempt failed. A host not connected said nothing of how it was;
in place of a status, after folders, the line named where opening it goes: the remote directory on
the side of the panel of the menu and the local `other_dir` on the other side. The line of one
state read as a status, the line of the other as a pair of paths, and which side each path stood
on changed with the panel. The directories are the host's settings, which the dialog of F4 shows.

## Decision

- **`Not connected`** on the second line of a host that is not connected, in the colors of the
  card, as `Connected` and `Connecting…`: the line says how the host is in every state.
- **The card no longer says where opening a host goes**: no remote directory, no `other_dir`, no
  folder in front of them. They show in the dialog of F4.

## Consequences

- `HostState` no longer carries the directories, and the card no longer depends on the side of the
  panel of the menu.
- Where a host opens is no longer seen before it opens, but in the dialog of F4.
