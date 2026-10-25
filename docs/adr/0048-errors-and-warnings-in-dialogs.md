# 0048. Errors and warnings in dialogs

- Status: Accepted
- Date: 2026-10-25

## Context

mc draws errors red, and the questions it asks before deleting too. Noon Commander did the same:
a dialog was red or not (`error: bool`), whether something had gone wrong or the user was asked
before something that cannot be undone. The two looked alike, though the first says what happened
and the second asks what to do. Each red question was put together where it was asked, with its
title and its flag; only an error with OK had a function of its own.

## Decision

- **A dialog has a tone** (`dialog::Tone`): plain, an error, or a warning. Its colors follow from
  it (`Colors::of`).
- **An error is red**, as in mc: its body, frame, title, and buttons (`error_dialog`,
  `error_title`, …). Errors: a job that failed, a directory that could not be read, a host that
  could not be reached or was lost, a host that is not connected, and every error with OK.
- **A warning has a background of its own**, orange where the theme has it, apart from dialogs,
  errors, the cursor, and the menu bar (`warning_dialog`, `warning_title`, …): black on yellow in
  mc-classic, the nearest of the 16 colors to orange; `base` on `peach` in Catppuccin, a peach
  darkened in Latte, where `base` does not read on Catppuccin's own; cream on a dark orange, nearly
  brown, in noon-dark and navy on a light orange in noon-light, the Noon role `warning`; in
  256 colors noon-dark takes `#D75F00`, as the nearest is the color of its menu bar, and the next
  its cursor's. The terminal's
  colors cannot tell a warning from an error. Warnings: deleting files or a workspace, a file
  that is there already (copy, move, checksums, rename), replacing a workspace, quitting with
  jobs that would stop.
- **An error says Error**: `Dialog::error` with OK, `Dialog::failure` with the buttons for what to
  do now. A dialog on a connection keeps its own title.
- **Paths and reasons are made safe where they go into the message** (`cells::sanitize`), not in
  the message: that would turn its own line breaks into `?`.

## Consequences

- A warning no longer looks as if something had failed, and an error stands out from the
  questions asked every day.
- A new question chooses its tone; a new theme gives warnings their colors, which tests check
  against those of dialogs, errors, the cursor, and the menu bar, in 256 colors too.
- A frame in the colors of a warning on the body of dialogs was tried first: it did not stand
  out enough, and the body is what says which kind of dialog it is.
