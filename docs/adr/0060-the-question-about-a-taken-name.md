# 0060. The question about a taken name

- Status: Accepted
- Date: 2026-10-31

## Context

When a copy or a move found its target's name taken by a file, it asked in red, as mc does: the
path in a sentence, the sizes and times of both files in two more, and Yes, No (the default),
All, None, Older, and Abort. Six buttons said two things at once, what to do with this file and
with the ones after it, and only some of the pairs had a button: there was no way to skip this one
and replace the older ones after it. Telling whether the new file was the one to keep meant
reading both, which the question did not let one do: the job waits for its answer, and the viewer
opens only from a panel. And nothing kept both files: the new one went over the old one, or
stayed behind.

## Decision

- **The question is a warning, in parts.** The path of the file there, then, between lines, the
  line of the new file and that of the file there, `New` and `Existing`, each with its size and
  time on the right, the times lined up; then a combo box, The rest; then Overwrite, Skip (the
  default), Rename…, and Abort. Its letters: n, x, t, o, s, r, a.
- **A line that opens what it names is a part of a dialog**, an item (`Dialog::with_item`). It
  takes the focus as the others do; Enter, Space, a click, or Alt and its letter open it, and the
  dialog stays (`DialogEvent::Opened`). A line across the frame is a part too
  (`Dialog::with_line`).
- **New and Existing view their files.** The question goes aside, with the viewer
  (`Viewing::question`), which is in front of every window but the dialogs until it closes: the
  window of the job that asks, the list of jobs, the menus. The job waits. When the viewer closes,
  the question comes back in front, its focus and its combo box as they were. A file that cannot be
  read says so over the question. F1 does nothing in that viewer, as the help would open under it.
  F3 does not view the line with the focus, since Enter and Space do.
- **The buttons are for this file, the combo box for the ones after it.** The answer to the job is
  a pair (`noc_ops::Conflict`): a `Choice` (Overwrite, Skip, Rename to a path, Abort) and a `Rest`
  (ask each time, the same as this one, or overwrite each one there that is older, smaller, or
  larger than its source, or not of its size, and keep the others). Unknown times and sizes keep
  the file there. Older was a button for both at once; it is now a choice of the rest, and the
  button pressed says what this one gets.
- **Rename… copies under another name.** It asks for a name, or a path from the directory of the
  file there, on its host, offering a name that is free there, the cursor before its extension.
  A name taken too asks again. With Same as this one, each later taken name gets a free name in
  the directory where the renamed file went, without asking. The directory it names is read again
  when the job ends. Cancel goes back to the question.
- **A free name needs no more quoting than the name.** The job finds it: the name with 2, 3, and
  so on before its extension, after the last space, `_`, or `-` in what comes before it, or after
  `_` if there is none, which keeps the name one word for a double click in a terminal and for
  vim's `w`: `My Report 2.pdf`, `build-log-2.txt`, `desktop_2.ini`, `.bashrc_2`. The extension is
  what follows the last dot, or `.tar` and what follows it; a dot that starts the name starts
  none. A number already at the end stays, as it may be a year or a version: `photo_2024_2.jpg`.
  The name itself comes first where it is free, as in another directory.
- **A move asks the same, once.** It renames under the new name where it can. Where it cannot,
  across file systems, it copies where the answer said, over the file there if it said so,
  without asking again.

## Consequences

- Both files can be read before one goes over the other, and kept both.
- Any choice for this file goes with any rule for the rest, in one press after the combo box.
- The question is two lines taller than before.
- A free name is found with a request for each name tried, on a host too; numbers far up take as
  many.
