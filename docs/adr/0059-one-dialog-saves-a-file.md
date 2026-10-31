# 0059. One dialog saves a file

- Status: Accepted
- Date: 2026-10-31

## Context

Save… in the window of checksums asked for a file name in a `Dialog::form` of its own, and took
what was typed as a name in the directory of the panel: no path, no `~`, no completion. Its
question before it wrote over a file, its error, and its texts were the checksums' own. Other
things will save or create a file: a new file before F4 edits it, the output of a command, a
list of names. Each would have made the same dialog again, a little different.

F7 already reads a path typed in a dialog, from the panel's directory, on its host
(`Panel::resolve`), and Go to directory and F5 complete one with Tab.

## Decision

- **One dialog saves a file, `tui/save.rs`.** What saves gives a `Save`: the title of the dialog,
  the directory a name is read from, the name offered, and the bytes. The rest is the dialog's:
  the field, `File name:`, how it reads it, the question, the error, and the writing.
- **The field takes a path as F7's does**: a name, a relative path, an absolute one, or `~/…`,
  on the host of the directory (`panel::resolve`, which `Panel::resolve` now calls). Tab
  completes files and directories from that directory, not from the panel, which may have gone
  elsewhere since, and no other host: a file goes where what it came from is.
- **It never writes over a file unasked.** A name that is taken asks, `File exists`, with No the
  default; Yes writes over it. A file that cannot be written is an error, in a window of its own.
  Nothing says that it was saved: the panels on its directory read it again and show it.
- **Its texts are its own**, `save-`, but the title, which says what is saved.

## Consequences

- Save… of the checksums takes a path, `~`, and Tab, as the other path fields do.
- A new file to save needs a `Save` and nothing more, and gets the same field, question, and
  error.
- A path into a directory that is not there is an error: the dialog makes no directories.
- What saves cannot yet do something once the file is written, such as open it in the editor;
  that comes with the first that needs it.
