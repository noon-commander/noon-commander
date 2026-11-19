# 0091. The pull-down menu acts on the active panel, without Left and Right

- Status: Accepted
- Date: 2026-11-19
- Amends: [ADR 0009](0009-configuration-dialog-writes-config-toml.md),
  [ADR 0017](0017-workspaces.md), [ADR 0051](0051-ctrl-x-sends-a-command-to-the-other-panel.md),
  [ADR 0070](0070-workspaces-by-name-and-saved-in-place.md)

## Context

The F9 pull-down menu followed Midnight Commander's bar: `Left`, `File`, `Command`, `Options`,
`Workspace`, `Right`. `Left` and `Right` held the same ten commands for each panel drawn on that
side, `Command` gathered everything that did not fit in `File` (search, navigation, two-panel
commands, background jobs, hosts, help, redraw, and the log), `Disconnect` lived in both `Left` /
`Right` and `Command`, and `Options` held only three items.

Since [ADR 0051](0051-ctrl-x-sends-a-command-to-the-other-panel.md), every key acts on the active
panel and Ctrl+x runs the next command in the other panel, then gives the keys back. `Left` and
`Right` were the last place in the interface that addressed panels by side, carrying four
side-addressed `Command` variants (`On`, `Location`, `Tabs`, `DisconnectPanel`) and
`Keymap::other_key` solely to mirror what Ctrl+x already does for every command, including F9
itself (`Ctrl+x F9`).

Three windows were also absent from the menu: the command history (`q :`), the output of commands
(`g <`), and the key hints (`Ctrl+k`).

`Workspace` listed the saved workspaces under its commands, the first ten with `1` … `9` and `0`
([ADR 0017](0017-workspaces.md)): a copy of their window (`t w`), one entry below, without its
filter, its tabs, or its commands, and with a `Command::Workspace` of its own.

## Decision

- **Seven menus by task, without `Left` and `Right`:** `File`, `View`, `Go`, `Search`, `Tools`,
  `Workspace`, `Help`. Every command is an `Action` and acts on the active panel, as its key does,
  and `Ctrl+x F9` (or `Ctrl+x g m`) runs any menu command in the other panel and returns the keys
  once it ends.
- **Contents:**
  - **File:** View, Edit, Open externally, Checksums; Copy, Duplicate, Rename/Move, Rename in
    place, Mkdir, Delete; Select group…, Unselect group…, Invert selection; Exit.
  - **View:** Sort by name, Sort by extension, Sort by time, Sort by size; Show hidden files, Show
    whole names; Swap panels, Open in the other panel, This directory in the other panel; Rescan,
    Redraw screen.
  - **Go:** Change location…, Go to directory…, Jump to a directory (zoxide)….
  - **Search:** Quick search, Find file…, Find files and dirs….
  - **Tools:** Background jobs…; Command history…, Command output; Edit host settings…, Disconnect
    host; Configuration….
  - **Workspace:** New tab, Close tab, Tab list…; Save workspace…, Workspace list…. The saved
    workspaces are not listed: Workspace list… opens their window.
  - **Help:** Help, Key hints; View the log.
- **F9 opens the bar on `File`**, with no menu open yet, regardless of which panel is active.
  Shift+F9 still opens on the command that ran last.
- `Command` is removed with its side-addressed variants (`On`, `Location`, `Tabs`,
  `DisconnectPanel`) and `Workspace`, and so is `Keymap::other_key`; `Disconnect host` is the
  single disconnect entry.

## Consequences

- Every menu command follows the same active-panel and Ctrl+x model as the rest of Noon Commander,
  with no duplicate panel menus; the seven-menu bar takes 67 cells (vs. 64 before), leaving room
  for the jobs indicator on an 80-column terminal. On a narrower one, the indicator covers the end
  of the bar, `Help` first, while jobs run; Alt+H still opens it.
- Acting on the other panel from the menu now takes `Ctrl+x F9` (or switching panels first) instead
  of opening `Left` or `Right`.
- `Configuration…` moves from `Options` to `Tools`, and `Show hidden files` / `Show whole names`
  move from `Options` to `View`; `o o`, `o h`, and `K` are unchanged.
- `Command history…`, `Command output`, and `Key hints` are reachable from F9.
- Restoring a workspace from F9 takes Workspace list… and Enter, or the filter, in place of Alt and
  a digit.
