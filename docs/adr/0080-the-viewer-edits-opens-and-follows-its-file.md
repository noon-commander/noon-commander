# 0080. The viewer edits, opens, and follows its file

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0026](0026-open-externally.md), [ADR 0078](0078-the-viewer-reads-small-parts.md)

## Context

The viewer of F3 could only scroll. To change what it showed, the user closed it, found the file in the panel, and pressed F4; to see the next file, closed it, moved the cursor, and pressed F3 again. less edits the file it shows with `v`, at the line on top, and shows it again after; Far's viewer switches to its editor with F6. mc's viewer and Far's go to the next file of the panel (Ctrl+f, Numpad+), and less follows a file that grows with `F`, as `tail -f`. The viewer showed the log of this run (Ctrl+\\), which grows while it is read, but only as far as it was when read.

F4 handed any file to the editor. An editor may damage a binary file as it saves it, and may take long to open a large one; on a host, F4 first copies the whole file here. mc's editor asks before it opens a file larger than its `filesize_threshold`, 64 MiB.

## Decision

- **In every viewer of a file** these keys work; not in one of text that the app holds (the checksums of a window, a file a save would write), which has no file.
  - **`e` and F4 edit the file**, as in the panels, at the line on top where the lines have numbers (not after a jump), once its start is read; F4 shows on the F-key bar only then. The viewer stays: once the editor changed the file it reads it again, up to the line on top; a file on a host once the copy went back.
  - **`g x` opens it externally**, as in the panels: local files only.
  - **`R` reads it again**, up to the line on top, as less's `R`.
  - **`F` follows its end** as it grows, as less's `F`: the viewer goes to the end, and once a second asks the size and the time of the file, as `tail -f` sleeps a second between reads. A file that only grew is read on from what was its end; where its last line had no line feed, the file is read again, as what was added goes on with that line. One that shrank, or whose time changed with the same size, is read again, to its end. `Follow` takes the place of `End` in the position. A move up, `F` again, or a file that can no longer be looked at stops it; the line of the mode says why for the last.
  - **`] f` and `[ f` view the next file and the one before**, as vim-unimpaired's, with a count that many, no further than the last or the first; lines wrap or are cut as they were. The files are those of the list the viewer was opened from, as it shows them: the panel's, whose cursor goes along, as in mc and Far; the files the search of a tree found, each from its first line that matched, the cursor of the search going along. The log has no list: its files are those of its directory, the logs of other runs, sorted as the active panel sorts. The `ssh_config` of a host and the file of a question about a taken name have none: they are one file, not one of a list.
- **The `ssh_config` of a host is edited too.** Noon Commander writes nothing to `~/.ssh`; the user's editor does, on the user's key, as F4 on `~/.ssh/config` in a panel always could.
- **F4 asks first** for a file that looks binary, or that is larger than `edit.large_mib` (`[edit]`, 64 MiB by default, as mc's editor; 0 never asks for the size), wherever it edits: in the panels, in the search of a tree, and in the viewer. A file looks binary with a NUL among its first 8000 bytes, as git tells one (`noc_text::is_binary`). The question is a warning, Cancel the default, with Edit; for a file on a host it says that the file is copied first. Outside the viewer F4 reads the size and the first 8000 bytes before anything else, through the host's session for a remote file, before any copy; the viewer knows both from its first part, which starts the file. A file whose bytes cannot be read goes to the editor all the same, which says why.
- **`edit.large_mib` is in the View and edit category** of the Configuration dialog, which was Viewer: the dialog has a category for each digit, 1 to 9 and 0, and no eleventh.

## Consequences

- Editing from the viewer needs no trip to the panel, and the viewer shows the change at once.
- F4 waits for a round trip, two on a host, before the editor or the copy starts.
- Text in UTF-16 looks binary, as in git, and asks first.
- The log of this run may be edited too. An editor that saves by writing a new file and renaming it over the log leaves the rest of the run's log in the old file, which the logging thread still writes to.
- Following asks a host for the metadata of the file once a second while it follows.
- A last line without its line feed has the file read again each time it grows, from its start for a local file of at most 16 MiB, else its first part and its last.
- `] f` in the log lists its directory each time.
