# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Key hints, as which-key shows them: half a second after a key that starts longer sequences,
  such as Ctrl+x, or g and Ctrl+w in the vim keymap, the keys that can follow it show at the
  bottom of the panels with what they do, and the key waits for the next one instead of timing
  out. ? shows every key of the panel the same way, in both keymaps. The next key
  runs as it would have, or shows the keys that can follow it; Esc closes the hints, and
  Backspace takes back the last key. `ui.which_key` and `ui.which_key_delay_ms` (Key hints and
  Key hints delay in Options → Configuration) turn them off and set the delay.
- `ui.keymap` (Keymap in Options → Configuration) chooses the keymap by name, as `ui.theme`
  chooses the colors, and takes effect at once: `default`, modelled on mc, or `vim`. In the vim
  keymap, k, j, g g, and G move in dialogs too, and g m opens the pull-down menu, as F9 does,
  where Ctrl+n and Ctrl+p move, Ctrl+f and Ctrl+b, or Shift+Down and Shift+Up, go to the last
  and first command, and Ctrl+c closes it. In its panels, Tab, Ctrl+w w, Ctrl+w Ctrl+w, and
  Ctrl+w p switch to the other panel, and Ctrl+w x swaps them, as vim's window commands do. In
  the location menu, Ctrl+n, Ctrl+p, Ctrl+f, and Ctrl+b move as in the panels, and Ctrl+c
  closes it. Ctrl+y does what Enter does in the menus, the lists, and the dialogs, as in vim's
  pop-up menu.
  `noc keymap diff` compares two keymaps action by
  action, the default and the vim one unless named: `|` where the keys differ, `<` and `>` for an
  action only one of them binds, in color in a terminal; `--all` shows what they bind alike too.
- The terminal's window or tab is titled with the active panel's directory, as the prompt of the
  command line shows it (`~/src — noc`, or `host:path`), and gets its own title back when Noon
  Commander quits or hands the terminal to a command, where the terminal keeps a stack of titles.
  `ui.terminal_title` (Terminal title in Options → Configuration) turns it off.
- Ctrl+o shows the output of commands in place of the panels, as in mc and Far, in the panels and on
  the command line; Ctrl+o or Esc brings the panels back. Each command now shows after its prompt,
  its output ends on a line of its own, and the line that asks for a key stands out and gives way to
  `[exit N]`, bold and red, after a command that failed. `shell.pause` (Wait after a command in
  Options → Configuration) says when the output waits for a key: `always`, `on-error`, or `never`.
- Command history: each command of the command line is kept in `history.toml` in the state
  directory (`~/.local/state/noc/`), with the host it ran on. Up on the first line of the
  command line, or Alt+p, brings back the commands of the panel's host, and Down, or Alt+n,
  goes forward to what was typed. Alt+h or Ctrl+r on the line, or Alt+h in a panel, opens a
  window of the history that typing filters: Tab shows every host, Enter puts the command on
  the line without running it, its prompt standing out if it ran on another host, and Delete
  removes it. `shell.history_size` (500, as bash's `HISTSIZE`, or Command line → History size
  in Options → Configuration) sets how many commands are kept; a command that starts with a
  space is never kept.
- Input for the command line: Shift+Enter starts a new line where the terminal speaks the
  kitty keyboard protocol, which Noon Commander now asks for where the terminal has it; Ctrl+x
  Ctrl+e opens the command in `$VISUAL` or `$EDITOR` and brings back what the editor left,
  without running it; and pasted text arrives whole through bracketed paste, so a pasted line
  break never runs a command. Text fields and quick search take pasted text without its line
  breaks, and panels ignore it rather than take its characters as keys.
- A command line for shell commands: `!` opens it in a panel, and so does `:`, for commands of Noon
  Commander, of which `:!command` is the only one so far. The prompt shows the panel's directory,
  with the host's label or alias on a host, where the command runs over the host's connection with a
  terminal of its own. Ctrl+j, or a `\` at the end of the line, starts a new line, and the line
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
- Workspaces: Alt+W saves the tabs of both panels under a name, with their sort orders
  and the rows under their cursors, and restoring one replaces every tab. Alt+w, or
  F9 → Workspace → Workspace list…, opens their window, which filters them; Enter restores,
  Insert saves the tabs as a new one, and F6 and F8 rename and delete them. F9 → Workspace
  lists them too. They are kept in `workspaces.toml` in the data directory
  (`~/.local/share/noc/`).
- Fuzzy search, as fzf does it: quick search, the filter of the location menu, and the zoxide
  window find `config.rs` from `cfg`, best match first, with fzf's `'`, `^`, `$`, and `!`.
  On by default; `ui.fuzzy_search = false`, or Fuzzy search in Options → Configuration,
  brings back literal matching.
- Shift+F6 (or F16, and F9 → File → Rename in place) renames the entry under the cursor in
  its row: the name is selected but for the last extension, Enter renames, and Esc keeps the
  name. A taken file name asks before it is replaced; a directory's is never replaced.
- Shift+F9 (or F19, and g M in the vim keymap) opens the pull-down menu on the command that ran
  from it last, as Far's Shift+F10 does, so Shift+F9 Enter repeats it.

### Changed

- Typing in a panel no longer starts quick search; Ctrl+s or Alt+s do, as in mc. The setting
  `ui.type_to_search` and its row in Options → Configuration are gone; remove the key from
  `config.toml`. `!` and `:` will open a command line for shell commands.
- Release builds use full link-time optimization, which makes the `noc` binary about 6% smaller.
- The title of a panel, and the list of tabs (Ctrl+x Tab), show the home directory as `~`: `~/src`
  rather than `/Users/me/src`.
- Keys are written one way in the help, the menus, and the docs: letters in lowercase, after a
  modifier too, and in uppercase for Shift and the letter (`Ctrl+r`, `Alt+W`, `G`), `Shift+` only
  before other keys (`Shift+F6`), and `PgUp`, `PgDn` rather than `PageUp`, `PageDown`.
- Left and Right on the menu bar of F9 stop at its first and last menu instead of going round.
- Up and Down in the pull-down menu of F9 no longer go round it: Down stops at the last command,
  and Up on the first goes back to the menu bar, as Home there does; on the bar, Up and Home no
  longer open the menu.
- F9 always opens the menu bar alone at the menu of the active panel, and each menu opens at its
  first command; the menu no longer opens again where it closed.

## [0.1.0] - 2026-10-03

First release.
