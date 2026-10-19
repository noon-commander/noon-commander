# 0041. Lists always show the whole row

- Status: Accepted
- Date: 2026-10-19
- Amends: [ADR 0025](0025-marks-of-text-cut-short.md),
  [ADR 0036](0036-lists-open-in-insert-mode.md)

## Context

`K` showed the whole text of what is under the cursor, where it was cut short: the name in a panel,
in a frame below the panels, and the row of a list, in two lines under its rows
([ADR 0025](0025-marks-of-text-cut-short.md)). Each list asked for it on its own and opened
without it, so seeing a long path in the zoxide window or the location menu took Esc, to leave the
filter ([ADR 0036](0036-lists-open-in-insert-mode.md)), then `K`, every time a list opened.

A list is a window over the panels, opened to choose one row and closed at once; it has room for
two more lines. The panels are on screen all the time, and the rows below them are rows of files
taken from them.

## Decision

- **Every list shows the whole row under the cursor**, in two lines under its rows, while it has
  rows: the location menu, the tab menu, the zoxide window, the Workspaces window, and the list of
  the key hints. The command history keeps its footer, the whole command under the cursor. A list
  without rows, whose filter matches nothing or that says why it has none, shows no such lines.
- **`K` is for the panels alone.** The lists' Normal mode binds it no more; Options → Show whole
  names still shows the frame below the panels.
- **A skeleton takes the lines of the whole row too**, past the rows of the last list, so that the
  box does not grow when the rows come ([ADR 0037](0037-spinners.md)). The skeleton of a list that
  opens the first time in a run keeps its one row.

## Consequences

- A list is three lines taller: the line over the whole row and its two lines.
- What a list cuts short is always there to read, from the moment it opens, in Insert mode too.
- `K` in a list does nothing, as other keys that nothing binds in Normal mode.
- `picker::Whole` is gone; the app keeps its own for the panels.
