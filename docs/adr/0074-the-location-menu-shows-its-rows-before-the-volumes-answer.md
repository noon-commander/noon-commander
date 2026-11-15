# 0074. The location menu shows its rows before the volumes answer

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0021](0021-location-menu-replaces-the-virtual-root.md),
  [ADR 0037](0037-spinners.md)

## Context

The location menu showed its rows once every volume had said how big it is, or 500 ms had gone
by ([ADR 0021](0021-location-menu-replaces-the-virtual-root.md)). A network mount that hung kept
the menu waiting that long each time it opened, then showed `?` for good: the call went on in a
blocking thread and answered later, but nothing waited for that answer. A later listing found the
call still running and gave the volume `?` at once, without asking again. The same `?` stood for a
file system that tells no size, so a mount that did not answer read like one that has nothing to
tell.

A slow mount that answers after a second or two, such as a NAS that wakes up, never showed its
sizes while the menu was open.

## Decision

- **Rows after at most 150 ms.** The menu waits for the volumes as long as a wait goes before its
  spinner shows (`places::FIRST_ANSWER`, `spinner::DELAY`), so volumes that answer at once show no
  skeleton. Those that have not answered by then are listed as they are without an answer: on
  Linux with what `/proc/self/mountinfo` tells; on macOS as a network file system of unknown type,
  as `statfs` alone tells a mount point from a directory there.
- **A skeleton in place of the sizes still to come**: a faint bar as wide as the sizes known in the
  other rows, the slash between them too, or as wide as sizes take at most while none is known. One
  shine crosses all of them at once, as it crosses the skeleton of rows (`window::cell_skeleton`).
  No dots turn in the title: the rows came, and a mount that hangs would turn them for as long as
  the menu stays open.
- **Answers come as they come.** Each one replaces its volume in the menu, and in the volumes the
  app knows panels to be on (`Done::VolumeAnswered`, by mount point). On macOS, a `statfs` that
  answers that the path is no mount point, or a mount not to browse, takes its row away; the cursor
  stays on its row, or else on the row now in its place. A query that fails after it hung, as a
  soft network mount (SMB by default, NFS with `soft`) gives up after its own timeout, leaves the
  row as it was, with `- / -`: the volume is still mounted, and if it was unmounted meanwhile, the
  next listing leaves it out. A failure in time still leaves the volume out, as before.
- **One query of a file system at a time.** A query still running is shared: a listing that finds
  it waits for its answer rather than ask again, so R, opening the menu again, and the listing at
  the start never start a second blocking call on a mount that hangs. A new listing stops waiting
  for the answers of the one before it, and waits for those still to come itself.
- **Given up for after 5 s**, from when the volume was asked, by this listing or an earlier one
  (`menu::NO_ANSWER`): the skeleton turns to `- / -`, the shine stops, and nothing wakes the event
  loop for it any more but the moment it gives up. An answer that comes later still shows. `- / -`
  in the columns of the sizes also stands for a volume that tells no size, in place of `?`.
- **`--slow-volumes SECONDS`, in debug builds only**, lists every volume, the system volume too, as
  if none answered in time, and sends each answer that many seconds later, so that the skeletons and
  `- / -` can be watched without a mount that hangs.

`noc ls` and `noc debug ls` still wait up to 500 ms for every volume, and the frame of a panel still
waits as long for the space of its directory.

## Consequences

- The menu opens in at most 150 ms, where a mount that hung held it for 500 ms.
- A mount that answers late shows its sizes in the menu that is open.
- `- / -` and the skeleton tell apart sizes not known from sizes still to come; `?` no longer shows
  in the menu.
- On macOS a row of `/Volumes` that turns out to be no mount point can show for a moment, then go.
  `statfs` of a directory does not hang, so this happens only when a mount that hung is unmounted
  and its directory stays, or with a mount not to browse that hangs.
- A mount that hung and was unmounted meanwhile stays in the open menu, with `- / -`, until R or
  the next menu.
- `noc-vfs` keeps the queries still running by path, with their answers, instead of a set of paths
  (`volumes_within`, `Late`).
