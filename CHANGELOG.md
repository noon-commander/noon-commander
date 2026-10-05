# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A command line for shell commands: `!` opens it in a local panel, and so does `:`, for
  commands of Noon Commander, of which `:!command` is the only one so far. The prompt shows the
  panel's directory. Ctrl-J, or a `\` at the end of the line, starts a new line, and the line
  grows up to a third of the screen; Enter runs the command in `$SHELL` (or `/bin/sh`) in the
  panel's directory with the panels hidden, says how it failed, and a key brings the panels
  back, which read their directories again. Esc, or Backspace on an empty line, closes it.
- The mouse: a click in a panel moves the cursor there, a double click opens, a right click
  marks the row as in mc and Far, and the wheel scrolls the panel or the viewer under it. The
  F-key bar, the tabs, the pull-down menu of F9, dialogs, and every window and menu take
  clicks and the wheel too. It is on by default; `ui.mouse = false`, or Mouse in
  Options → Configuration, turns it off, and `ui.wheel` sets how far the wheel scrolls: a
  number of lines (3) or `"page"`. While it is on, the terminal selects text with Shift held,
  or Option in iTerm2.
- Workspaces: Alt-Shift-W saves the tabs of both panels under a name, with their sort orders
  and the rows under their cursors, and restoring one replaces every tab. Alt-W, or
  F9 → Workspace → Workspace list…, opens their window, which filters them; Enter restores,
  Insert saves the tabs as a new one, and F6 and F8 rename and delete them. F9 → Workspace
  lists them too. They are kept in `workspaces.toml` in the data directory
  (`~/.local/share/noc/`).
- Fuzzy search, as fzf does it: quick search, the filter of the location menu, and the zoxide
  window find `config.rs` from `cfg`, best match first, with fzf's `'`, `^`, `$`, and `!`.
  On by default; `ui.fuzzy_search = false`, or Fuzzy search in Options → Configuration,
  brings back literal matching.
- Shift-F6 (or F16, and F9 → File → Rename in place) renames the entry under the cursor in
  its row: the name is selected but for the last extension, Enter renames, and Esc keeps the
  name. A taken file name asks before it is replaced; a directory's is never replaced.

### Changed

- Typing in a panel no longer starts quick search; Ctrl-S or Alt-S do, as in mc. The setting
  `ui.type_to_search` and its row in Options → Configuration are gone; remove the key from
  `config.toml`. `!` and `:` will open a command line for shell commands.
- Release builds use full link-time optimization, which makes the `noc` binary about 6% smaller.

## [0.1.0] - 2026-10-03

First release.
