# 0091. The pull-down menu acts on the active panel, without Left and Right

- Status: Accepted; amended by [ADR 0092](0092-ctrl-k-lists-the-keys-at-once.md): Key hints… ends in `…`, as it opens the list of the keys
- Date: 2026-11-19
- Amends: [ADR 0009](0009-configuration-dialog-writes-config-toml.md), [ADR 0017](0017-workspaces.md), [ADR 0051](0051-ctrl-x-sends-a-command-to-the-other-panel.md), [ADR 0070](0070-workspaces-by-name-and-saved-in-place.md)

## Context

The F9 pull-down menu followed Midnight Commander's bar: `Left`, `File`, `Command`, `Options`, `Workspace`, `Right`. `Left` and `Right` held the same ten commands for each panel drawn on that side, `Command` gathered everything that did not fit in `File` (search, navigation, two-panel commands, background jobs, hosts, help, redraw, and the log), `Disconnect` lived in both `Left` / `Right` and `Command`, and `Options` held only three items.

Since [ADR 0051](0051-ctrl-x-sends-a-command-to-the-other-panel.md), every key acts on the active panel and Ctrl+x runs the next command in the other panel, then gives the keys back. `Left` and `Right` were the last place in the interface that addressed panels by side, carrying four side-addressed `Command` variants (`On`, `Location`, `Tabs`, `DisconnectPanel`) and `Keymap::other_key` solely to mirror what Ctrl+x already does for every command, including F9 itself (`Ctrl+x F9`).

Three windows were also absent from the menu: the command history (`q :`), the output of commands (`g <`), and the key hints (`Ctrl+k`); so were the command line itself (`!`), and the jump list (`Ctrl+o`, `Ctrl+i`) and the matches of the last quick search (`n`, `N`), whose keys are vim's and hard to find without it.

`Workspace` listed the saved workspaces under its commands, the first ten with `1` … `9` and `0` ([ADR 0017](0017-workspaces.md)): a copy of their window (`t w`), one entry below, without its filter, its tabs, or its commands, and with a `Command::Workspace` of its own. Its first three commands were those of the tabs, which its title did not name; `Tabs` could not have `T`, which `Tools` has, and a menu of its own would have left `Workspace` two commands and the bar 76 cells.

`Find file…`, mc's name, searched the files for text, while `Find files and dirs…` beside it found them by name.

`Tools` mixed four things: the jobs, the command line, the host of the panel, and the settings.

The ellipsis followed no rule: Select group… and Go to directory… had one, Copy, Rename/Move, and Mkdir, which ask just as much, had none, and of the windows Tab list… and Background jobs… had one, Help and Command output none.

## Decision

- **Seven menus by task, without `Left` and `Right`:** `File`, `View`, `Go`, `Search`, `Tools`, `Window`, `Help`. Every command is an `Action` and acts on the active panel, as its key does, and `Ctrl+x F9` (or `Ctrl+x g m`) runs any menu command in the other panel and returns the keys once it ends.
- **Contents:**
  - **File:** View, Edit, Open externally, Checksums…; Copy…, Duplicate…, Rename/Move…, Rename in place…, Mkdir…, Delete; Select group…, Unselect group…, Invert selection; Exit.
  - **View:** Sort by name, Sort by extension, Sort by time, Sort by size; Show hidden files, Show whole names; Rescan, Redraw screen: how the listing shows.
  - **Go:** Change location…, Edit host settings…, Disconnect host; Go to directory…, Jump to a directory (zoxide)…; Jump back, Jump forward; Open in the other panel, This directory in the other panel, which say where the other panel goes, not what shows. The host commands act on the active panel's host, which Change location… lists; the panels have no keys for them.
  - **Search:** Quick search…, Next match, Previous match; Find text…, Find files and dirs…, named after the modes of the dialog they open.
  - **Tools:** Shell command…, Command history…, Command output; Background jobs; Configuration…, last, where the settings usually are.
  - **Window:** Swap panels; New tab, Close tab, Next tab, Previous tab, Tab list…; Save workspace…, Workspace list…, as the Window menu of macOS and of editors arranges the windows and holds the tabs; the key of Swap panels, `Ctrl+w x`, is a command of vim's windows too. The saved workspaces are not listed: Workspace list… opens their window.
  - **Help:** Help, Key hints; View the log.
- **`…` ends a command that asks for something before it acts:** text, a choice from a list, or settings, in a dialog, a field, or a window. A question that only confirms does not count (Delete, Exit), nor does a window that is the result itself (View, Help, Background jobs, View the log).
- **F9 opens the bar on `File`**, with no menu open yet, regardless of which panel is active. Shift+F9 still opens on the command that ran last.
- `Command` is removed with its side-addressed variants (`On`, `Location`, `Tabs`, `DisconnectPanel`) and `Workspace`, and so is `Keymap::other_key`; `Disconnect host` is the single disconnect entry.

## Consequences

- Every menu command follows the same active-panel and Ctrl+x model as the rest of Noon Commander, with no duplicate panel menus; the seven-menu bar takes 64 cells, as many as before, leaving room for the jobs indicator on an 80-column terminal. On a narrower one, the indicator covers the end of the bar, `Help` first, while jobs run; Alt+H still opens it.
- Acting on the other panel from the menu now takes `Ctrl+x F9` (or switching panels first) instead of opening `Left` or `Right`.
- `Configuration…` moves from `Options` to `Tools`, and `Show hidden files` / `Show whole names` move from `Options` to `View`; `o o`, `o h`, and `K` are unchanged.
- In File, Alt+r runs Rename in place…, as `r` does in the panels, and Alt+n runs Rename/Move…. Alt+s runs Sort by size, as `o s` sorts, and Alt+n New tab, as `t n` opens one, where Alt+z and Alt+w were left from the menus of each panel.
- `Shell command…`, `Command history…`, `Command output`, `Key hints`, `Jump back`, `Jump forward`, `Next match`, `Previous match`, `Next tab`, and `Previous tab` are reachable from F9.
- `Workspace` is named `Window`; Alt+W still opens it.
- Restoring a workspace from F9 takes Workspace list… and Enter, or the filter, in place of Alt and a digit.
