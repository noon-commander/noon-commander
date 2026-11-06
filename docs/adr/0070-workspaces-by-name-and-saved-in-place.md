# 0070. Workspaces by name, saved in place of one, the last marked, and columns that name no side

- Status: Accepted
- Date: 2026-11-06
- Amends: [ADR 0017](0017-workspaces.md)

## Context

The window of the workspaces (`t w`) and F9 → Workspace showed them in the order they were
saved: a new one went last, and one saved again kept its place. With more than a few, a name was
hard to find by eye, and the digits of F9 → Workspace said nothing of the name they restore.

Saving the tabs again in place of a workspace took `t W` in a panel, which offers the name of the
workspace restored or saved last and saved over it without asking, or typing the name of another
one, which asks before replacing it, No by default. In the window, where the workspace is under
the cursor, there was no key for it. Nor did the window show which workspace was restored or saved
last, which F9 → Workspace marks and `t W` offers; it opened on its first row.

Under the rows, the column of each panel was headed Left and Right, the heading of the side with
the keys in the color of titles. The columns stand where the panels stand, so the headings said
again what their place says, and a line went to them that tabs could use; only one heading told
something, and only for the workspace under the cursor.

## Decision

- **By name.** The window and F9 → Workspace show the workspaces in the order of their names, as
  the panels order names: as `ui.natural_sort` and the formats in use say, ties by the bytes of the
  name. The digits of F9 → Workspace go to the first ten in that order. The app sorts the
  workspaces once they come from `workspaces.toml`, and again when `ui.natural_sort` or the formats
  change; the file keeps them in the order they were saved, as `noc-config` knows no language.
  There is no other order to choose, and no key for one.
- **Saved in place of the one under the cursor.** In the window, `W` and F2 (`replace_workspace`)
  ask whether to replace the workspace under the cursor with the tabs of both panels. Yes saves
  them under its name, which becomes the one saved last, and the window stays open on it. `W` as
  `t W` saves in the panels; F2 as mc's editor saves. vim's `W` moves to the next WORD, which a
  list has not.
- **Every save over a workspace asks, the same way.** `W` and F2 in the window, and `t W` with the
  name of a workspace, ask before replacing it: Yes by default over the one restored or saved
  last, which saving again is meant for, and No over another one, so that Enter by habit changes
  nothing. `t W` asked nothing over the one restored or saved last before; now Enter twice saves
  it.
- **`a` and F7 add one.** `a` stays, as yazi's `a` adds an entry and `A` adds a directory in the
  panels; vim's `a` appends after the cursor, which a list has not. F7 adds one too, as mc's F7
  makes a directory, so that adding works while the filter takes text, as F2, F6, and F8 do.
- **Columns that name no side.** The headings go. A mark, `Glyphs.active` (a triangle to the right,
  in the color of titles), goes before the tab that shows on the side with the keys, or before how
  many more if that tab is among those that do not fit; each line of a column keeps its room. In
  each row the count of tabs of the side with the keys is bold, in the color of titles, so every
  workspace says which side has the keys, not only the one under the cursor.
- **The one restored or saved last.** The window marks it as F9 → Workspace does, with the dot of
  a radio button (`Glyphs.choice`) in the color of titles, in a room of two cells in front of every
  name, as the columns under the rows keep for their mark; and it opens with the cursor on it, as
  the tab menu opens on the tab that shows, so that Enter brings it back and `W` saves over it. It
  is the one of this run alone: the panels start where they start, not on a workspace, so a mark
  kept from the last run would name tabs that do not show.

## Consequences

- A workspace moves in the window when it is renamed; the cursor goes with it.
- The window is a line shorter for the same tabs.
- `workspaces.toml` and the window list the workspaces in different orders; the file is data the
  window writes, not a list to read.
