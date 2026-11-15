# 0078. The viewer reads small parts

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0038](0038-viewer-reads-a-window-of-the-file.md),
  [ADR 0077](0077-the-viewer-jumps-to-the-end-of-a-file.md)

## Context

The viewer read a file in parts of 16 MiB
([ADR 0038](0038-viewer-reads-a-window-of-the-file.md)). Before the first screen of a large file
showed, it read 16 MiB and made a line for the screen of every line in them, where a screen
takes a few thousand bytes; and so again each time scrolling reached the end of what it held.
Reading is not what takes the time: making the lines is. mc shows a file at once because it
reads the 4,096 bytes around the screen and nothing else; it maps no file into memory. On a host
reading does take the time: all 16 MiB had to arrive before anything showed.

A part could not simply be made smaller. Its size also said how long a line may be before it is
cut, how much of a file is read at once on the way to a line, and whether the scroll bar may go
by lines. And a file of a few MiB, read whole as one part, had all its lines counted when it
showed.

## Decision

- **`viewer.part_mib` is 1 by default**, where it was 16: the first screen waits for one part,
  and so does each move past what is held.
- **A line that goes on past its part stays whole.** A new part with no line feed in its bytes
  reads on to the first one, for 16 MiB at most (`Want::most`), where the line is cut, as it was.
  A part read before parts apart ([ADR 0077](0077-the-viewer-jumps-to-the-end-of-a-file.md))
  goes no further than they start.
- **A local file of at most 16 MiB is read whole behind the screen**, part by part, once its
  first part shows (`Sizes::ahead`, `Want::ahead`): all its lines are counted and held within
  moments, so its title says `line/lines` and its scroll bar goes by lines, as when such a file
  was one part. These reads show no window and take no keys; one that fails says so, and a read
  stopped stops them until the next move. Not a file larger than the window holds, nor one
  that tells no size, which may never end.
- **The end of a local file no further than 16 MiB from what was read is read up to in order**,
  and so is a byte: that keeps the numbers of the lines, and takes no longer than one sees.
  Further away, they are jumped to. On a host they are jumped to when further than a part, as
  they were: there every part read is sent.
- **On the way to a line, parts are 16 MiB**: their lines are only counted, and fewer reads get
  there.
- **The scroll bar goes by lines** when the whole file is held and is at most 16 MiB, whatever
  the size of a part: counting the rows of more takes too long.
- **A file that grew did not change.** A part that finds the file larger than the parts before
  it did is taken, and the file goes on past what was its end. A smaller size, or another time
  of change with the same size, still has the file read again from its start
  ([ADR 0038](0038-viewer-reads-a-window-of-the-file.md)). With parts of 1 MiB a log that is
  being written would otherwise be read again at every part, and never to its end.
- **A file on a host has parts of a size of its own**, `viewer.host_part_kib`, 256 by default,
  in the Configuration dialog under Viewer: there every part read is sent over the connection,
  and a slow one shows a small part sooner. Nothing is read ahead on a host.
- **A part asks the file system for its bytes and no more** (`Vfs::open_file_range`). The reader
  of a file on a host keeps 64 reads of 32 KiB in flight; with no end to them it asked for up to
  2 MiB past a part, which the server sent for nothing, eight times a part of 256 KiB. A line
  that goes on past its part is read on by another reader.
- **The size of a file and the bytes of a part are asked for at once**, where the part waited
  for the size: one round trip less for each part on a host.

## Consequences

- A file of any size shows after 1 MiB is read, and scrolling reads 1 MiB at a time.
- A file of 1 to 16 MiB shows `line/?` for the moment it takes to read the rest.
- The index has a part for each MiB read in order, where it had one for each 16.
- A line longer than a part stays whole only when read down. Reached from a jump, scrolling
  up, it is still cut where a part starts in it
  ([ADR 0077](0077-the-viewer-jumps-to-the-end-of-a-file.md)), now each MiB.
- A file written over with more than it had, between two parts, shows old parts beside new
  ones until it is viewed again.
- A `config.toml` that sets `part_mib` keeps its value.
- A file on a host shows after 256 KiB arrive, with two round trips: one for its size and for
  opening it, at once, and one for its bytes.
- A file system implements `Vfs::open_file_range`; `open_file_at` and `open_file` read through
  it, to the end of the file.
