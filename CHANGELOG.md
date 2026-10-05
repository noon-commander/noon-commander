# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Ctrl-O shows the output of commands in place of the panels, as in mc and Far, in the panels and on
  the command line; Ctrl-O or Esc brings the panels back. Each command now shows after its prompt,
  its output ends on a line of its own, and the line that asks for a key stands out and gives way to
  `[exit N]`, bold and red, after a command that failed. `shell.pause` (Wait after a command in
  Options → Configuration) says when the output waits for a key: `always`, `on-error`, or `never`.
- Command history: each command of the command line is kept in `history.toml` in the state
  directory (`~/.local/state/noc/`), with the host it ran on. Up on the first line of the
  command line, or Alt-P, brings back the commands of the panel's host, and Down, or Alt-N,
  goes forward to what was typed. Alt-H or Ctrl-R on the line, or Alt-H in a panel, opens a
  window of the history that typing filters: Tab shows every host, Enter puts the command on
  the line without running it, its prompt standing out if it ran on another host, and Delete
  removes it. `shell.history_size` (500, as bash's `HISTSIZE`, or Command line → History size
  in Options → Configuration) sets how many commands are kept; a command that starts with a
  space is never kept.
- Input for the command line: Shift-Enter starts a new line where the terminal speaks the
  kitty keyboard protocol, which Noon Commander now asks for where the terminal has it; Ctrl-X
  Ctrl-E opens the command in `$VISUAL` or `$EDITOR` and brings back what the editor left,
  without running it; and pasted text arrives whole through bracketed paste, so a pasted line
  break never runs a command. Text fields and quick search take pasted text without its line
  breaks, and panels ignore it rather than take its characters as keys.
- A command line for shell commands: `!` opens it in a panel, and so does `:`, for commands of Noon
  Commander, of which `:!command` is the only one so far. The prompt shows the panel's directory,
  with the host's label or alias on a host, where the command runs over the host's connection with a
  terminal of its own. Ctrl-J, or a `\` at the end of the line, starts a new line, and the line
  grows up to a third of the screen; Enter runs the command in the panel's directory, locally in
  `$SHELL` (or `/bin/sh`), with the panels hidden, says how it failed, and a key brings the panels
  back, which read their directories again. Esc closes it.
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
