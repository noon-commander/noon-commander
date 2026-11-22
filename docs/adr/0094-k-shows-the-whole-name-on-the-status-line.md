# 0094. `K` shows the whole name on the status line

- Status: Accepted
- Date: 2026-11-22
- Amends: [ADR 0025](0025-marks-of-text-cut-short.md),
  [ADR 0041](0041-lists-always-show-the-whole-row.md)

## Context

`K` showed the whole name under the cursor of the active panel in a frame of its own below the
panels: two rows inside single lines, in the colors of dialogs. It looked like a dialog stuck to
the bottom of the screen, and its frame took two of its four rows. The status line of the panel,
which shows the same name cut short, stayed as it was right above it.

The lists show the whole row under the cursor under their rows, below a line, inside their own
frame ([ADR 0041](0041-lists-always-show-the-whole-row.md)).

## Decision

- **`K` makes the status lines of both panels two rows high**, as the lists show their row: the
  name under the cursor goes on in the second row where it does not fit in the first, and what does
  not fit in both ends with `»`. A name that fits keeps its one row, the second blank, so that
  nothing moves as the cursor goes from a short name to a long one.
- **The second row starts with `»`**, in the accent of marks, as the viewer's wrapped rows do
  (vim's 'showbreak'), so that it reads as the first going on rather than a name of its own.
- **The size of a file stays at the right of the first row**, where it is without `K`; the first
  row of the name ends left of it, and the second has the whole width.
- **Without `K`, the status line cuts a name at its end**, `»` in its last cell before the size,
  as the viewer cuts a line that goes on past the screen, rather than in its middle as names lose
  it elsewhere ([ADR 0025](0025-marks-of-text-cut-short.md)). A `»` in the middle of the line was
  taken for a part of the name, or for the arrow of a symlink, which shares the line; the row
  above keeps the end of the name, with its extension.
- **A status that goes on is plain**: the arrow and the target of a symlink lose their colors and
  icons where the text takes the second row.
- **Both panels**, so that their separators and the tees of their columns stay at one height; the
  inactive one shows its own name whole too.
- The frame below the panels is gone, and with it the colors of dialogs for the name.

## Consequences

- Each panel loses one row of its rows while `K` shows names, rather than the screen four.
- Esc, `K` again, and F9 → View → Show whole names end it, as before; it is still not saved.
- A veiled panel's skeleton keeps to the first row of its status line.
