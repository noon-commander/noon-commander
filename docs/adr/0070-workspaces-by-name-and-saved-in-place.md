# 0070. Workspaces by name, and columns that name no side

- Status: Accepted
- Date: 2026-11-06
- Amends: [ADR 0017](0017-workspaces.md)

## Context

The window of the workspaces (`t w`) and F9 → Workspace showed them in the order they were
saved: a new one went last, and one saved again kept its place. With more than a few, a name was
hard to find by eye, and the digits of F9 → Workspace said nothing of the name they restore.

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
- **Columns that name no side.** The headings go. A mark, `Glyphs.active` (a triangle to the right,
  in the color of titles), goes before the tab that shows on the side with the keys, or before how
  many more if that tab is among those that do not fit; each line of a column keeps its room. In
  each row the count of tabs of the side with the keys is bold, in the color of titles, so every
  workspace says which side has the keys, not only the one under the cursor.

## Consequences

- A workspace moves in the window when it is renamed; the cursor goes with it.
- The window is a line shorter for the same tabs.
- `workspaces.toml` and the window list the workspaces in different orders; the file is data the
  window writes, not a list to read.
