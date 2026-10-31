# 0058. The window of checksums filters them and views them

- Status: Accepted
- Date: 2026-10-31
- Amends: [ADR 0015](0015-viewer-and-text-crates.md),
  [ADR 0036](0036-lists-open-in-insert-mode.md)

## Context

The window that `f #` shows when its job ends listed the files with their checksums cut short in
the middle, and under them the name and the whole checksum of the selected file, then the
verdict, with nothing between the list and that file. With a tree of files, finding one meant
scrolling down to it: the list had no filter, though every other list has one. And the lines of
`sha256sum` that Copy all and Save… write could not be read on screen before they were copied or
saved.

The location menu (`g l`) shows the whole row under the cursor apart from the rows, under a line,
in as many lines whatever the row. The lists share their filter, `picker::Picker`, which opens
typing in it (ADR 0036). The viewer of F3 reads files only, in parts, through a task.

## Decision

- **A filter, as the lists have.** With several files the window is a `Picker`: the filter on top,
  the rows under it, ten at most, then the footer and the buttons. The filter finds a file by its
  name or its checksum, or by `skipped`; it matches as the other filters do, Ctrl+s and the key
  after it switch how, and it opens as it was last (`Slot::Checksums`, ADR 0047). Copy takes the
  file under the cursor among those shown, and does nothing while none is; Copy all, View, and
  Save… always take every file.
- **It opens on the rows, in Normal mode**, unlike the lists: the window is first a result to
  read, and the buttons have the keys of a dialog. `/` types in the filter, Esc goes back to the
  rows, and Esc again closes the window. `Picker::on_rows` opens a picker so.
- **A context of its own, `checksums`.** In Normal mode it falls back to `dialog`, whose keys move
  the cursor of the rows and the focus of the buttons, Enter and Space press the button with the
  focus, and Esc closes; in Insert mode, while the filter takes text, to `list`, then `input`, and
  Tab and Shift+Tab still go through the buttons. Alt and a letter press a button in both, as in
  every dialog (ADR 0033). The line of the mode says `Dialog`, as for the search of a tree. One
  file has no list, and no filter: its window stays a dialog, in `dialog`.
- **The file under the cursor goes apart**, in the footer of the picker, under a line that joins
  the frame, as in the location menu: its name, its whole checksum in as many lines as the
  longest of them takes, so that the window does not move as the cursor goes, then the verdict,
  and what the last button did. Without a file under the cursor the footer is blank. `Picker`
  takes buttons for it (`Body::buttons`), under the footer, under a line.
- **View, and F3, show the lines in the viewer**, those that Copy all copies and Save… writes,
  under the name Save… offers (`SHA256SUMS`). The viewer takes text that the app holds as well as
  a file (`Source::Text`): `noc_viewer::part_of` cuts the parts it asks for from the text, as
  `read_part` cuts those of a file, at once, without a task, as the lines of checksums are small
  next to a file of megabytes. The window of checksums goes into the viewer and comes back in
  front, as it was, when the viewer closes. A viewer that was open under the window closes. One
  file has no View: its one line is on screen.

## Consequences

- A file is found in a long list by typing part of its name or of its checksum, and the lines of
  a file of checksums can be read, and searched once the viewer can, before they are copied or
  saved.
- The window of several files is three lines taller: the filter, the line under it, and the line
  over the footer.
- The viewer is no longer only for files: what else the app holds as text, such as a log or the
  output of a command, can be shown the same way.
- Text in the viewer is cut into parts on the thread that draws; text of many megabytes there
  would want a task, as files have.
