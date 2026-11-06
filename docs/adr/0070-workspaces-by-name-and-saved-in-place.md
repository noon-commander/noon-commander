# 0070. Workspaces by name

- Status: Accepted
- Date: 2026-11-06
- Amends: [ADR 0017](0017-workspaces.md)

## Context

The window of the workspaces (`t w`) and F9 → Workspace showed them in the order they were
saved: a new one went last, and one saved again kept its place. With more than a few, a name was
hard to find by eye, and the digits of F9 → Workspace said nothing of the name they restore.

## Decision

- **By name.** The window and F9 → Workspace show the workspaces in the order of their names, as
  the panels order names: as `ui.natural_sort` and the formats in use say, ties by the bytes of the
  name. The digits of F9 → Workspace go to the first ten in that order. The app sorts the
  workspaces once they come from `workspaces.toml`, and again when `ui.natural_sort` or the formats
  change; the file keeps them in the order they were saved, as `noc-config` knows no language.
  There is no other order to choose, and no key for one.

## Consequences

- A workspace moves in the window when it is renamed; the cursor goes with it.
- `workspaces.toml` and the window list the workspaces in different orders; the file is data the
  window writes, not a list to read.
