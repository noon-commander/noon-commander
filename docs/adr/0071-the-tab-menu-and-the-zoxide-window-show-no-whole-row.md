# 0071. The tab menu and the zoxide window show no whole row

- Status: Accepted; amended by
  [ADR 0072](0072-the-command-history-shows-where-a-command-ran.md) and
  [ADR 0087](0087-large-windows.md)
- Date: 2026-11-10
- Amends: [ADR 0041](0041-lists-always-show-the-whole-row.md)

## Context

Every list showed the whole row under the cursor in two lines under its rows, apart from them by a
line ([ADR 0041](0041-lists-always-show-the-whole-row.md)). In the location menu and the key hints
a row holds more than it shows: the address or the mount point of a place, every key of a command.
In the tab menu and the zoxide window a row is a path, and the window is 76 cells wide: the path
fits in nearly every row, and the lines under the rows said again what the row under the cursor
says. They took three lines of a window opened to choose one row and closed at once.

## Decision

- **No whole row in the tab menu and the zoxide window.** The bottom of the frame comes right
  after their last row; the line under the rows and the two lines under it go. A path too long for
  its row still loses its start, with `«` in front of what is left
  ([ADR 0025](0025-marks-of-text-cut-short.md)).
- **The location menu and the key hints keep it**, and the command history and the Workspaces
  window their footers.
- **The skeleton takes the lines of the whole row only for a window that has it** (`Body.has_full`):
  the zoxide window waits with as many lines as its rows will take, so that its box does not shrink
  when they come ([ADR 0037](0037-spinners.md)).

## Consequences

- The tab menu and the zoxide window are three lines shorter.
- The start of a path too long for its row cannot be read in them; the panel shows it once the
  tab shows or the directory opens.
