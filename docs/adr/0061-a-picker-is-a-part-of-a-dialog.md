# 0061. A picker is a part of a dialog

- Status: Accepted
- Date: 2026-10-31
- Amends: [ADR 0040](0040-one-picker-for-every-list.md),
  [ADR 0058](0058-the-window-of-checksums-filters-and-views-them.md)

## Context

The window of checksums was neither a dialog nor a list. With several files it was a `Picker`,
which drew its box and, for this window alone, buttons under its footer (`Body.buttons`); with one
file, or two compared, it drew a box of its own, with the frame, the lines, and the buttons of a
dialog, but not a `Dialog`. So it had its own focus of the buttons, its own key handling, and its
own mouse, beside those of every dialog, and none of what a dialog gains: a dialog's keys, the
scrolling of a message taller than the screen, what the parts of a dialog learn next.

A `Dialog` held combo boxes, fields, check boxes, items, lines, and one list (`Dialog::with_list`),
whose rows its owner draws, for the search of a tree. A `Picker` held the filter, the rows it
shows, their cursor and scrolling, headings, and a footer. Neither could be the other's part.

## Decision

- **A picker is a part of a dialog** (`Dialog::with_picker`), under the parts before it. The dialog
  draws the frame, the title, and the buttons; the picker draws its filter, its rows, and what goes
  under them in the room between (`Picker::render_in`), as many lines as its `Body` asks
  (`Body::height`), fewer where the screen has no room, for the line and the buttons under it. Its
  owner gives it the lines and draws the rows, as for a list (`Dialog::render_picker`).
- **Its rows are a stop of the focus**, as a field or a check box is, in the order of the parts;
  without fields or combo boxes before them they start with it. While they have it, the motions
  and the page keys move their cursor, `/` types in the filter, and what is typed goes there
  while it takes text, with the keys after Ctrl+s. The motions never leave the rows: they stop at
  the first row and the last, as in the list of the search of a tree. Tab and Shift+Tab leave
  them for the stop after or before, and so do Left and Right on the rows, where they edit
  nothing. On the buttons the motions go through the stops, round to the rows, which keep their
  cursor, and `/` brings the focus back to the filter, typing in it. Leaving the rows, the filter
  stops taking text. Enter and Space on a row press the default button; Esc on the rows closes
  the dialog; Alt and a letter press a button, as in every dialog. The dialog's mode is the
  picker's while its rows have the focus: Insert while its filter takes text. The cursor's row
  dims while the focus is elsewhere, as the list's does.
- **The mouse**: a click on a row gives the rows the focus and puts the cursor there, and a double
  click does no more; a click on a glyph of how the filter matches switches it.
- **Lines of text are a part too** (`Dialog::with_text`), which do not wrap as the message does:
  a line that loses its middle where too long, as a name; text without blanks, as a checksum, in
  as few lines of equal length as it takes (`Text::Even`); and a verdict, after ✓ or ✗, bold, in
  the color of titles or of errors (`Text::Verdict`).
- **The window of checksums is a `Dialog`**, which `SumsWindow` wraps as `FindWindow` wraps the
  search of a tree: the picker for several files, and lines of text for one file or two compared.
  Its buttons are the dialog's (`To clipboard`, `All to clipboard`, `View`, `Save…`, `OK`), and a
  picker has no buttons of its own any more (`Body.buttons` is gone).
- The other lists stay windows of their own, a `Picker` in its box (`Picker::render`); the list of
  a dialog stays beside the picker.

## Consequences

- The window of checksums looks as it did, but its keys are a dialog's: with several files, the
  rows have the focus, which Tab takes to the buttons and the motions there bring back, where the
  focus stayed on the buttons; with one file or two compared, the motions move the focus of the
  buttons, where they did nothing.
- What a dialog learns, the window of checksums has; a window that wants a filter and buttons
  takes a picker in a dialog rather than drawing its own.
- Two kinds of rows remain in dialogs, the list and the picker. Each list window could become a
  dialog with a picker once a dialog may have no buttons, and the list of the search of a tree a
  picker without a filter; each needs a decision of its own.
