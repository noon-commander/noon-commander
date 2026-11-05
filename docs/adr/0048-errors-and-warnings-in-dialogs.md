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

- **A dialog has a tone** (`window::Tone`): plain, an error, or a warning. Its colors follow from
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
- **Fields, combo boxes, and check boxes take the tone too**: errors and warnings have their own
  fields (`error_input`, `warning_input`), for text fields, combo boxes, and the lists they open,
  and their own brackets and checks (`error_bracket`, `error_symbol`, `warning_bracket`,
  `warning_symbol`). A field stands apart from the body and from the button with the focus,
  whose colors a combo box takes with the focus, as the buttons beside it. In the themes in RGB
  both are shades of the tone, never the colors of plain dialogs, the cursor, or the menu bar:
  a field away from the text of the body, darker in noon-dark and Mocha, lighter in noon-light
  and Latte, and the focus the other way, each the text on the other (the roles
  `warning_field`, `warning_focus`, and `danger_` or `red_` the same). In mc-classic, of 16
  colors, a field is white on black in errors and black on gray in warnings, where cyan is the
  focus. No accent reads both on the shades and on red or orange, so Catppuccin and Noon set the
  brackets apart by dim and the checks by bold.
- **The letters of labels have a color in errors and warnings**, as in the other dialogs, and
  are underlined on the focus, as there: a palette role `tone_hotkey` where the theme's accent
  does not read on the tone (the gold of the letters in noon-dark, a dark brown in noon-light, a
  dark bordeaux in Mocha, a light gold in Latte), and over the fields, in the list of a combo box,
  `error_input_hotkey` and `warning_input_hotkey`: in Catppuccin the tone's own color, as its
  fields are the other way from it.
- **An error says Error**: `Dialog::error` with OK, `Dialog::failure` with the buttons for what to
  do now. A dialog on a connection keeps its own title.
- **Paths and reasons are made safe where they go into the message** (`cells::sanitize`), not in
  the message: that would turn its own line breaks into `?`.

## Consequences

- A warning no longer looks as if something had failed, and an error stands out from the
  questions asked every day.
- A new question chooses its tone; a new theme gives warnings their colors, which tests check
  against those of dialogs, errors, the cursor, and the menu bar, in 256 colors too; and fields,
  which tests check against the body and the focus of their tone.
- The fields of dialogs once took `dialog_input` in every tone: in a warning a combo box without
  the focus looked like the button with it, as in mc-classic both are black on cyan. A field
  darker than the dialogs' surface was tried next: in Noon it was a navy block on orange, and the
  focus, on that surface, navy too.
- A frame in the colors of a warning on the body of dialogs was tried first: it did not stand
  out enough, and the body is what says which kind of dialog it is.
