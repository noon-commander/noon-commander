# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `noc debug version` prints one `key value` line per fact, for scripts and bug reports: the
  version, the compile-time features, the commit and its time in UTC, the target, the
  operating system, the terminal with its colors and keyboard protocol, and the editor of F4.
  The terminal is asked for its name, so kitty, which sets no `TERM_PROGRAM`, is named too.

### Changed

- `noc -V` and `noc --version` print only the version, as in `noc 0.2.0`; the compile-time
  features (`-forwarding`) moved to `noc debug version`.
- `noc hosts` and `noc ls` moved to `noc debug hosts` and `noc debug ls`.
- Backspace and Delete with nothing typed no longer close quick search, as on the command line,
  so that pressing Backspace once too often no longer goes to the parent directory.
- Ctrl+f in the panels and the lists stops with the last row at the bottom, then goes to it,
  rather than scroll on until only the last row shows at the top, as vim's does.
- The help and the viewer stop scrolling with the end at the bottom, for `j`, Ctrl+e, and
  Ctrl+f too. A dimmed `— end —` follows the last line, a scroll bar shows on the right when the
  text does not fit, and the position says `End` once the end shows: the viewer's on top, the
  help's in its bottom frame.
- The zoxide window (`g z`) opens in Insert mode, so keywords can be typed at once, as in `zi`,
  without `/` first. Esc goes to the rows, and Esc there closes the window.

## [0.2.0] - 2026-10-14

### Added

- Vim's keymap, in its modes, with mc's F-keys ([ADR 0022](docs/adr/0022-vim-modes.md)).
  The help (`g ?`) lists the keys of each mode and context.
  - Normal, Insert, and Visual modes. A line above the F-key bar shows the mode, the keys typed
    so far, and messages. Esc acts at once, and Ctrl+c closes or cancels everywhere.
  - Panels: motions with counts (`5j`, `3G`), scrolling as vim's 'scrolloff', Visual mode (`v`,
    `V`) to act on rows, a jump list per tab (Ctrl+o, Tab), window commands (Ctrl+w w,
    Ctrl+w x, Ctrl+w f), `/` and `?` as incremental search with `n` and `N`, and commands such
    as `y y` (copy), `x` (move), `d D` (delete), `r` (rename), and `i` (view).
  - Text fields: Normal and Visual modes with motions, operators, and counts (`d 2 w`, `c c`),
    registers (`"a` … `"z`, shared by every field), undo and redo, Ctrl+o, Ctrl+r, and Ctrl+v.
  - Lists, dialogs, and the pull-down menu: `j`, `k`, `g g`, `G` move, `q` closes; `/` filters
    a list and `d d` deletes its row. The viewer and the help scroll as vim and less do.
  - The F-keys stay mc's: F1 … F10, Shift+F6, Shift+F7, Shift+F9, Alt+F1, and Alt+F2.
- Key hints, as which-key shows them: after a key such as `g` or Ctrl+w, the keys that can
  follow it appear at the bottom; Ctrl+k shows every key of what is in front.
- A command line: `!` runs a shell command in the panel's directory, locally or on the panel's
  host; `:` runs commands of Noon Commander (`:!command` so far). Ctrl+j or a trailing `\`
  starts a new line, and Tab completes as vim's `:!` does.
- Command history per host in `history.toml`: Up and Down (Ctrl+p, Ctrl+n) on the command line,
  and `q :` for a window of it. A command that starts with a space is not kept.
- `g <` shows the output of commands, as vim does, with `[exit N]` after a command that failed.
- Workspaces: `t W` saves the tabs of both panels under a name, `t w` lists them to restore,
  rename, or delete. They are kept in `workspaces.toml`.
- Mouse: a click moves the cursor, a double click opens, a right click marks, and the wheel
  scrolls; menus, dialogs, tabs, and the F-key bar take clicks too.
- Fuzzy search, as fzf does it, in quick search, the location menu, the tab menu, and the
  zoxide window.
- Rename in place: Shift+F6 or `r`.
- Shift+F9 or `g M` opens the pull-down menu on the command that ran from it last.
- `n`, `N`, and Shift+F7 repeat the last quick search.
- A dialog with Retry and Cancel when a connection fails or is lost; the panels on a lost host
  keep their directory and read it again once it connects.
- The terminal's title shows the active panel's directory and is restored on exit.
- The kitty keyboard protocol, where the terminal has it (Shift+Enter starts a new line on the
  command line; Ctrl+i, Ctrl+m, and Ctrl+[ act as Tab, Enter, and Esc), and bracketed paste, so a
  pasted line break never runs a command.
- Settings, all in Options → Configuration: `ui.which_key`, `ui.which_key_delay_ms`,
  `ui.mouse`, `ui.wheel`, `ui.fuzzy_search`, `ui.terminal_title`, `shell.pause`, and
  `shell.history_size`.

### Changed

- The location menu (Ctrl+w 1, Ctrl+w 2, Alt+F1, Alt+F2) groups places under Favorites, Mounts,
  and SFTP, shows the places used last first (kept in `recent.toml`), and hides the rest behind
  a `N more…` row ([ADR 0021](docs/adr/0021-location-menu-replaces-the-virtual-root.md)). F4
  and F8 there edit a host's settings and disconnect it.
- Disconnecting a host leaves its panels in place, without entries.
- The list of tabs is a tab menu (`t h`, `t l`), drawn over its panel like the location menu;
  `d d` or F8 closes a tab.
- Text fields open in Insert mode with the cursor after the text, rather than with the text
  selected.
- Tab completes paths as vim's 'wildmenu' does, going through the matches, and ignores case.
- F9 always opens on the active panel's menu, at its first command.
- Ctrl+l redraws the screen everywhere, not only in the panels and the viewer.
- Panel titles and the tab menu show the home directory as `~`.
- Keys are written one way everywhere: `Ctrl+r`, `Alt+W`, `G`, `Shift+F6`, `PgUp`.
- The panels give a line to the mode line.
- Release builds use full link-time optimization: the binary is about 6% smaller.

### Removed

- The mc keys other than the F-keys: Esc and a digit no longer stand for an F-key, and the
  location menu has no digit hotkeys.
- The virtual root and the list of hosts: volumes and hosts are chosen in the location menu.
- `ui.type_to_search`: `/` starts quick search. The key is now an error in `config.toml`;
  remove it.

### Fixed

- The Homebrew formula, which Homebrew rejected for its `url` in `on_arm` and `on_intel` blocks.

## [0.1.0] - 2026-10-03

First release.
