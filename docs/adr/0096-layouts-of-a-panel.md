# 0096. Layouts of a panel

- Status: Accepted
- Date: 2026-11-22

## Context

The columns of a panel were fixed: the name, the size, and the modification time, with the time
and then the size dropped on a narrow panel. The status line showed the name under the cursor and
the size of a file. Panels are to show more of a file, and users are to choose what, in as many
modes of their own as they like, as Far's view modes and mc's listing formats let them; the
settings come later. The code had to stop fixing the columns first.

## Decision

- **A layout says what a panel shows beside the names**: the details of its columns, and those
  of its status line (`panel::layout`). A detail is one thing known of an entry: the size, the
  modification time. Each detail has its title, its width, and its text in a column and on the
  status line, where the size has every byte rather than its rounded value.
- **The name stands anywhere** among the details, in the columns and on the status line alike, as
  in Far: a layout lists the details before the name and those after it, so that it holds one
  name, always, without a check. The name takes the cells the details leave.
- **A narrow panel drops the details at the end of the list first**, those after the name before
  those before it, while the name keeps 8 cells. The status line drops its own the same way.
- **One built-in layout for now**: the size and the time after the name in the columns, the size
  after it on the status line, as before. Each panel tab has its layout; a new tab takes its
  panel's.

## Consequences

- The columns and the status line look as they did; a layout of a user's needs only its list.
- Settings for layouts, a key that switches them, and a layout per tab saved in the workspaces
  come later, with a way to edit the lists in Tools → Configuration….
- A rename's field starts where the name starts, after the details before it, and runs to the end
  of the row, as it did.
