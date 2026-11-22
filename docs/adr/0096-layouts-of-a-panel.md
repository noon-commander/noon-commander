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
  modification time, the mode (`-rw-r--r--`, as `ls -l` writes it), the owner (`501:20`). Each
  has its title, its width, and its text in a column and on the status line, where the size has
  every byte rather than its rounded value. A detail not known, or a directory's size, is left out
  of the status line and blank in a column.
- **The name stands anywhere** among the details, in the columns and on the status line alike, as
  in Far: a layout lists the details before the name and those after it, so that it holds one
  name, always, without a check. The name takes the cells the details leave.
- **A narrow panel drops the details at the end of the list first**, those after the name before
  those before it, while the name keeps 8 cells. The status line drops its own the same way.
- **A line between the name and each detail on the status line**, `│` in the color of the frame,
  as the lines between the columns above it, so that the details do not run into the name or into
  each other; each takes two cells more for it.
- **While `K` shows the name whole, the status line has no details**: both rows are the name's.
  Details that kept their place in the first row left the name too little of it, and dropping
  them one by one as the name needed the room moved them about.
- **The status line of a symlink has the link's own details**, as its row has them. Without them
  the target took the whole line. Where the name and the target do not both fit before them, they
  share the room evenly, what one does not need going to the other: the name loses its end, the
  target its start; with less than 4 cells for either, the name shows alone.
- **The status line has the size of a directory too**, in bytes, which its row shows as `DIR`.
- **The owner is in digits alone, not grouped**, where its name is not known
  ([ADR 0098](0098-owners-by-name-where-known.md)): a uid and a gid name a user and a group rather
  than count, as `ls -n` and `chown` write them; the rule that every number is grouped does not
  hold for them. Its column is always 11 cells, as `65535:65535`, so that it does not change from
  one directory to the next; a longer owner loses its middle.
- **The status line scrolls to its end once the name under the cursor shows its end in its row**:
  with its details the line is narrower than the column of names, so their scroll alone stopped
  short of the end of the name.
- **One built-in layout for now**: the size and the time after the name in the columns, as
  before; the size, the mode, and the owner after it on the status line. Each panel tab has its
  layout; a new tab takes its panel's.

## Consequences

- The columns look as they did. The status line shows more, and its size is no longer at its
  right edge; on a narrow panel the owner goes first, then the mode. On a panel 40 cells wide the
  name keeps only its 8 cells beside the three details.
- A layout of a user's needs only its lists. Settings for layouts, a key that switches them, a
  layout per tab saved in the workspaces, and a way to edit the lists in Tools → Configuration…
  come later.
- A rename's field starts where the name starts, after the details before it, and runs to the end
  of the row, as it did.
