# 0038. The viewer reads a window of the file

- Status: Accepted
- Date: 2026-10-18
- Amends: [ADR 0015](0015-viewer-and-text-crates.md), [ADR 0023](0023-paging-stops-at-the-end.md),
  [ADR 0037](0037-spinners.md)

## Context

F3 read the first 16 MiB of a file at once (`noc_viewer::read_start`, `LIMIT`) and then showed
it. Over SFTP a large file took long, and the viewer showed dots and a skeleton with no sign of
how far it was, nor a way to stop it and look at what had come. What lay past the limit could
not be seen at all, and the limit could not be changed.

## Decision

- **Parts.** The viewer reads the file in parts of `viewer.part_mib` (16 MiB), each through
  `noc_viewer::read_part` over any `Vfs`, from an offset (`Vfs::open_file_at`). A new part ends
  after its last line feed, so that no line is split between parts; a line longer than a part is
  cut. The viewer keeps an index of every part it has read, its offset, length, and number of
  lines, so that line numbers stay those of the file.
- **A window.** It holds at most `viewer.window_mib` (128 MiB), never less than two parts. When a
  move down leaves the end of what it holds on screen and the file goes on, it reads the next part;
  when a move up reaches the top of what it holds and parts before it were dropped, it reads the
  one before again. Parts at the other end are dropped as the window fills, never one that shows on
  screen. Nothing is read ahead: a part is read when the screen reaches it. `g g` goes to the start
  of the file, reading its first part again if it was dropped; `G` goes to the end of what is held
  and reads the next part. A line given from outside (a search result) is reached part by part.
- **A dialog while it reads.** A read that takes longer than a spinner waits (150 ms) shows a
  dialog over the viewer, `Reading`: where the part starts in the file, a gauge of the part with its
  percentage, how much of it has come, and the buttons Stop and Close. Esc stops: the viewer keeps
  what it read and reads no more until the next move. Close closes the viewer. Its keys are those
  of every dialog; the line of the mode says `NORMAL · Dialog`. The viewer's own keys do not change.
  A part that comes whole leaves the window full, at 100%, for as long as a spinner waits, over the
  text, taking no keys: reports come at most every 50 ms, so it would go at the percentage of the
  last one. The last report always goes.
- **No skeleton.** The skeleton line that stood for the text while the file was read (ADR 0037)
  is gone: the window says more. The dots still turn in place of the file's icon.
- **What it shows.** Until the end is known the position says `line/? percent%`, the percentage
  by bytes, and the scroll bar stands for the whole file, by bytes; a drag on it goes to that byte.
  Once the whole file is held the position is as before, and so is the bar if the file is no more
  than a part: counting the rows of every line wrapped takes too long for more. Under the last line
  held, the dimmed line says `more below` while the file goes on, and `EOF` at its end.
- **Lines off the screen's thread.** `read_part` makes the lines of a part on a thread for blocking
  work: for a part of 16 MiB of a binary that took about 0.2 s in a release build and a second in
  a debug one, which on the thread that draws froze the window at its last percentage. Reading
  those bytes from a local disk takes a few milliseconds; making the lines is the work. It goes
  in one pass over the bytes, split at line feeds between the processors, one for each MiB at
  most: about 20 ms in a release build, so that a local file opens before the dialog would show.
  Counting the rows of a whole file wrapped, on the thread that draws, is shared the same way.
- **A file that changes.** Each part notes the file's size and time of change; when they differ
  from the first part's, or a known part comes back with another length, the viewer says the file
  changed, drops what it holds, and reads it again from the start, to the line it showed.
- **Errors.** A first part that fails closes the viewer, as before; a later one says why and keeps
  what is held.
- **Cancel-safe.** Each read takes two tokens: one that stops it between chunks with what it read,
  and one that drops it, when the viewer closes.

## Consequences

- `noc-viewer` no longer has `LIMIT` and `read_start`; the app asks the viewer what to read
  (`Viewer::want`) after every move and gives it the part (`Viewer::take`).
- `Vfs` has `open_file_at`; `open_file` reads from the start through it.
- A new section, `[viewer]`, holds `part_mib` and `window_mib`, under Viewer in the Configuration
  dialog; they take effect at the next F3.
- Moving back past the window reads the file again, which over a slow host takes time; the window
  can be made larger.
