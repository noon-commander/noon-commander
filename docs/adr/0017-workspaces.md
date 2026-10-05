# 0017. Workspaces: saved tabs of both panels in `workspaces.toml`

- Status: Accepted
- Date: 2026-10-04

## Context

Tabs ([ADR 0011](0011-tabs-per-panel.md)) keep several directories and hosts open on each side,
but only while Noon Commander runs, and setting up the same layout again for each project takes
many keys. Users asked for a way to keep a layout they like, the tabs of both panels with
their paths, and to come back to it at once.

Such a layout is more than a bookmark, which names one directory, and different from restoring
the panels on start, which brings back the last state without a name. It names a set of tabs
that the user chose.

Where to choose one was open: the location menus of Alt+F1 and Alt+F2 already list where a
panel can go, but a workspace changes both panels, so a row in the menu of one panel would
act on the other too.

## Decision

- A workspace has a name and, for each side, its tabs in order, with the one that shows; and
  the side that has the keys. A tab keeps its location, its sort order, and the name under its
  cursor, an entry or a host alias; not its marks, its scroll position, or its listing.
- Workspaces live in `workspaces.toml` in the XDG data directory (`~/.local/share/noc/`), not
  next to `config.toml`: they are data the user makes in the UI, not settings. `noc-config`
  owns the schema and the file (`Workspaces`, `save_workspace`, `remove_workspace`,
  `rename_workspace`); each change reads the file again, applies itself, checks the result, and
  replaces the file atomically, so changes from another Noon Commander are kept. The file is
  written whole, so comments in it are not kept, which its header says.
- A location is written as one string: `root`, `sftp`, a local path that starts with `/`, `~`
  or `~/…` (directories under the home directory are written from `~`, so a workspace moves
  with the home directory), or `host:path`. A location that this cannot hold, such as a name
  that is not UTF-8, is saved as the nearest directory above it that it can.
- Alt+w opens the window of the saved workspaces, and Alt+W saves the tabs of both panels
  as one. mc's panels bind neither; Far's take Alt and a letter for quick search, which Noon
  Commander starts by typing or with Ctrl+s. Ctrl+x s was the first choice, but mc makes a
  symbolic link with it, which Noon Commander will do too. Where Alt never arrives,
  `Esc w` and `Esc W` do the same, as for Alt+c and Alt+z.
- Saving asks for a name, offering that of the workspace restored or saved last, and saves the
  tabs of both panels under it. Another workspace's name asks before replacing it.
- The window has a filter, as the zoxide window has (keymap context `workspaces`): Enter
  restores, Insert saves the tabs as a new workspace with an empty name, as Insert adds to
  Far's menus, F6 renames, F8 deletes after a question, and the digits restore while the
  filter is empty. The location menus do not list workspaces.
- F9 has a menu of its own, Workspace, between Options and Right: Save workspace…, Workspace
  list… (the window), then the saved workspaces in order, the first ten with `1` … `9` and `0`
  as their letters, and a mark on the one restored or saved last. Choosing one restores it. A
  menu taller than the screen scrolls with the cursor.
- Restoring replaces every tab of both panels with new ones; the old tabs close, with their
  marks. The tab that shows on each side lists its location at once, connecting to its host if
  needed; hidden tabs list theirs when they show, so a workspace with many remote tabs connects
  only to the hosts that show.

## Consequences

- `Panel` can start at a location with the cursor going to a name, and its sort order can be
  read and set; `Tab` keeps a deferred first listing for tabs restored hidden.
- Restoring a workspace loses unsaved marks without asking; a workspace is meant to be
  restored often, and a question each time would be in the way.
- A workspace that names a host no longer in the ssh config, or a directory that is gone, shows
  the panel's error as any other location would.
- Restoring the panels on start, with their tabs, can reuse the same file format later.
