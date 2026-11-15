# 0084. Cards show a place and where it comes from

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0075](0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md)

## Context

The first line of the card of a host in the location menu had two ends: what the host is, its
address, at the start, and where it comes from, the file and the line of its `Host` line, against
the right end ([ADR 0082](0082-hosts-are-named-by-their-alias.md)). The other cards had one text:
a volume said what it mounts and where as `mount` says it, `/dev/disk4s1 on /Volumes/USB`, which
lost its middle where it was too long; the home directory its path alone. The same line told the
same things three ways, and where a volume is, the part that matters most, went with its source
into one cut.

## Decision

- **Every first line has two ends**, as the host's: what the place is at the start, after the icon
  of its row, and where it comes from against the right end. The end takes what the start leaves,
  but at least half the line (`menu::ends`). A path at the start loses its start where it is too
  long, an address its middle; what stands at the right end loses its start.
- **A volume**: its mount point, and what it mounts, `/dev/disk4s1` or `//me@nas/share`. While the
  volume is still to answer, a skeleton with the shine of its sizes, which come with the same
  answer; once it has not answered, `source unknown` in the colors of warnings; where it answered
  without, `source unknown`, faint, as `Size unknown`.
- **The home directory**: its path, and the name of its volume, its label or else its mount
  point; nothing where no volume holds it.
- **A host** stays as it is: its address, and the file and the line of its `Host` line.
- **The row of the rest** keeps its names over both lines: it is no place, and comes from none.
- The second lines stay as they are.

## Consequences

- One function draws the first line of every place (`Look::first_line`), whose ends are text, a
  skeleton, or nothing.
- `menu-card-mounted` is gone; `menu-card-source-unknown` says what a volume did not tell.
- On Linux what a volume mounts comes from `/proc/self/mountinfo` and is there before the volume
  answers; on macOS it comes from `statfs` with the sizes, so the skeleton waits for both.
