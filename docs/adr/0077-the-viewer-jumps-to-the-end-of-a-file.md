# 0077. The viewer jumps to the end of a file

- Status: Accepted; amended by [ADR 0078](0078-the-viewer-reads-small-parts.md)
- Date: 2026-11-15
- Amends: [ADR 0038](0038-viewer-reads-a-window-of-the-file.md)

## Context

`G` went to the end of what the viewer held and read the next part
([ADR 0038](0038-viewer-reads-a-window-of-the-file.md)): in a file of gigabytes the end took a
press for each part. A thumb dragged to the end read every part on the way, made their lines, and
threw them away. Both kept the numbers of the lines, which take reading the file from its start:
as long as the file is large, and on a host as long as it takes to send.

mc shows the end of any file at once. Its viewer knows where it is in bytes alone: it takes the
size from `fstat`, puts the start of the screen at the size, and goes up a screen of lines
backward. Its status line has the byte the screen ends at and the size, no line. It counts lines
only to go to one (F5), by reading the file up to it.

## Decision

- **`G` and End go to the end of the file.** When more than a part is left past what was read,
  the viewer reads the last part of the file alone and shows its end at the bottom of the screen,
  on a host as on a local disk: one part, whatever the size of the file. When no more than a part
  is left, or the file reports no size, as a file of `/proc`, it reads on in order to the end, and
  holds the parts on the way.
- **A byte far away is jumped to as well**: a thumb dragged past what was read by more than a
  part reads the part at that byte, no nearer the end of the file than a part. A byte that is
  near is read up to in order.
- **A line is still reached in order**, through every part before it, which count the lines: a
  search result has a number, and only reading tells where it is.
- **Parts apart.** The index of parts has the parts read in order from the start and, after a
  jump, the parts read from there on, apart from the others (`Parts`). What lies between was
  never read, so the lines of the parts apart have no numbers. A part read after a jump starts
  after the first line feed from the byte before the place asked for (`Want::after_feed`), at
  the start of a line; `Part` knows where that is. A part read in order ends no further than the
  parts apart start, so that it meets them exactly.
- **Lines are not counted behind the screen.** After a jump the title says where the screen is in
  bytes, as mc does: `at/size percent%`, and `at/size End` at the end, with the byte the first
  line on screen is at, about, and the size, short, as sizes are written on frames. The scroll
  bar goes by bytes, as for every file not held whole.
- **The numbers come back when the parts meet.** Scrolling up from the parts apart reads the part
  before them, which ends where they start. Once what lies between is no more than a part, it is
  read whole, as a part in order: the parts apart are in order then, and their lines have
  numbers. Another jump takes the place of the parts apart; `g g` goes back to the first part.
- **The log** (Ctrl+\\) opens by the same jump.
- **A last part shorter than the screen** brings the part before it, so that the end shows at the
  bottom with text above it.
- **A jump no longer wanted is dropped**: its part would take the place of the parts apart, which
  may be the ones on screen.
- **A file that changed** while the screen was among the parts apart is read again from its
  start, and jumped into at the byte the screen was at.

## Consequences

- The end of a file of any size shows after one part is read.
- After a jump the lines have no numbers until the parts meet; nothing counts them meanwhile. A
  search result in the same file is reached from the start of the file, in order.
- A line longer than a part is cut where a jump lands in it, and where a part read before the
  parts apart starts in it: such a part starts at the byte asked for. Once the parts meet, the
  line counts once more for each cut.
- `Viewer::render` takes how a size is written; `noc-viewer` has no units of its own.
- A part read back into a full window was let go at once, as it lay above the screen, and asked
  for again, for ever. The window lets go of parts at the other end from the one a scroll just
  read.
