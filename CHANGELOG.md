# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- One keymap, vim's, in its modes, with mc's F-keys ([ADR 0022](docs/adr/0022-vim-modes.md)). The
  panels, the lists, the pull-down menu, the viewer, and the help take keys in Normal mode; quick
  search, the command line of `!` and `:`, renaming in place, and a dialog that opens on a text
  field take them in Insert mode, where characters are text. In a text field Esc goes to Normal
  mode, and in Normal mode closes; quick search and the command line close on Esc, as vim's search
  and command line do. Ctrl+c closes or cancels in every mode. Esc acts at once, where 0.1.0's
  panels waited a second after it: it never waits for a key after it, and gives up a command typed
  so far. So does a key that cannot go on with it, which does nothing itself, as in vim: g x and
  Ctrl+w e do nothing. A line above the F-key bar shows `-- INSERT --` or `-- VISUAL --`, and the
  count, register, operator, or keys of a command typed so far, and messages, such as that n went
  round past the last row, until the next key; the cursor is a bar in Insert mode and a block in the
  others.
  - Panels: k, j (Ctrl+j), Ctrl+u, Ctrl+d, Ctrl+b, Ctrl+f, g g, and G move, l and Enter open, h and
    Backspace go up; a count repeats a motion (5j), and G, g g, and g t take it as a row or a tab
    (3G). The rows scroll as vim's window does, five kept above and below the cursor, as with
    defaults.vim's 'scrolloff': Ctrl+d and Ctrl+u scroll half a page and move the cursor as far, and
    Ctrl+f and Ctrl+b scroll a page and put the cursor at its top or bottom. v and V select rows in
    Visual mode, as many as a count says (3V), or the last selection that many times, whose y, x, d,
    D, X, F5, F6, F8, and f # act on them as on marked entries, and Space marks them; o goes to the
    other end, g v selects them again, and the other F-keys and Ctrl+w end it and do what they do.
    Ctrl+o and Tab (Ctrl+i) go back and forth through a jump list per tab, of the directories left
    and of G, g g, and the n, N, and searches that move; a count past its end goes nowhere. Ctrl+w
    w, Ctrl+w p, Ctrl+w h, and Ctrl+w l switch panels, as vim's windows; Ctrl+w x swaps them, Ctrl+w
    f and Ctrl+w v open the directory under the cursor or this one in the other panel, and Ctrl+w 1
    and Ctrl+w 2 change their locations. / starts quick search, typed as vim's search, from the row
    after the cursor and with a count to the match of that number (2/), and ? up the rows, after
    which n goes up and N down: Enter and Ctrl+j end it on the match, and say when it went round
    past the last row, Esc gives it up, and so do Backspace and Delete with nothing typed, Ctrl+g
    and Ctrl+t go to the next and the previous match, and the arrows, the page keys, and the F-keys
    end it and do what they do, as in mc. Space, J, and K mark, with a count as far as the listing
    goes, g + and g - mark and unmark by a pattern, and g \* inverts, as mc's +, -, and \*, which
    noc leaves unbound, as Shift+Up and Shift+Down; i views, e edits, Y and y y copy, x moves, r
    renames in the row, A makes a directory, d D deletes, R reads the directory again; t n, t c, g
    t, g T, t h, t l, t w, and t W for tabs and workspaces; o n, o e, o m, o s sort and o h shows
    hidden files; g z jumps with zoxide, g d is Quick cd, f j lists the jobs, q : opens the command
    history, g m and g M the pull-down menu, g ? the help, g < the output of commands, and Z Z
    quits.
  - Text fields have vim's Normal mode after Esc: h, l, w, b, e, W, B, E, 0, ^, $, f, t, F, T, ;,
    and , move; d, c, and y act on a motion, or on the line when doubled, with counts (2d3w); x, X,
    s, S, C, D, r, and ~ change; p and P put; u and Ctrl+r undo and redo; i, a, I, and A go to
    Insert mode, and v to Visual mode, whose d, c, y, and ~ act on what it selects; a count before v
    selects that many characters, or the last selection that many times. Text goes to registers: the
    unnamed one, and "a … "z, which "A … "Z append to, and what a delete appends the unnamed one
    keeps alone, as vim's register 1 does; every field and the command line share them, and Ctrl+r
    and a register puts one in Insert mode. " names a register wherever keys are commands, so that
    "x in a panel moves nothing; after an operator, as in vim, " gives it up, and the key after it
    is a command. Ctrl+o runs one command of Normal mode from Insert mode, from after the end of a
    line on its last character, and back after the text that p and P put or the end of the line that
    it deleted to, but before the character that 0, ^, and Home go to, as vim's; Ctrl+v types the
    next key as it is, and Alt and a key stand for Esc and the key, as in vim in a terminal; on a
    password they do nothing. As vim's, Ctrl+w and Ctrl+u stop once where Insert mode started, after
    a move of the cursor too, which starts a new change for undo, as Home and End do where the
    cursor is already, and so does Ctrl+u, which vim's defaults.vim maps after Ctrl+g u, but not
    Ctrl+w; undo and redo leave the cursor where the change starts, on its line and in its column,
    and after d d, c c, and S on the first character that is not blank, as 'startofline' has it. On
    the command line, Space and Backspace go on to the next line and back, and operators over lines
    follow vim's rules: d b at the start of a line keeps its line break, and d 2 $ from the indent
    deletes the lines whole.
  - The command line is vim's: Esc closes it, Ctrl+b and Ctrl+e go to the ends of the line, Ctrl+p
    and Ctrl+n through the history, Delete at the end deletes the character before, Shift and Ctrl
    with Left and Right go by what spaces part, as in quick search and the filters, and Ctrl+f goes
    to Normal mode, after which Esc leaves Insert mode, as in vim's window of the command line, and
    Ctrl+c and F10 close the line, where j and k go through its lines, keeping their column, and the
    history, and q : opens the window of the history, which q closes back to where q : was typed, a
    panel too.
  - The lists (the location menu, the tab menu, the zoxide window, the workspaces, the command
    history) open in Normal mode: j, k, Ctrl+j, g g, G, Ctrl+d, Ctrl+u, Ctrl+f, and Ctrl+b move, and
    the rows scroll as the panels' do, Enter takes the row, d d deletes it where F8 does, and with a
    count as many rows (3d d), and q closes; / types in the filter, where Ctrl+r types a register
    too, and Esc goes back to the rows with the filter kept.
  - Dialogs: j, k, h, l, and Tab move between fields and buttons, a count as many, past the fields
    too, g g and G to the first and the last, Space switches, and q closes, in Normal mode; the
    focus comes to a field in Insert mode with Tab or a click. A long text stays where it is while
    the cursor shows, and else scrolls to put the cursor in the middle, as vim's 'sidescroll' 0.
  - The pull-down menu: h, j, k, and l move, g g and G go to the first and the last command, and
    Alt and a highlighted letter run its command.
  - The F-keys of the panels stay mc's, in Normal and Visual mode and in quick search: F1 …
    F10, Shift+F6, Shift+F7, Shift+F9, Alt+F1, and Alt+F2.
- When a connection to a host fails, or is lost, a dialog says why and offers Retry, the
  default, and Cancel. The panels on a lost host stay on their directory, without its entries,
  and read it again once Retry connects; after Cancel they stay there, and R, or Enter on `..`,
  connects again. While a panel waits for a host, its status line starts with a spinner.
- `recent.toml` in the state directory (`~/.local/state/noc/`) keeps the hosts and volumes used
  last, up to 50 of each, for the location menu.
- Key hints, as which-key shows them: half a second after a key that starts longer sequences, such
  as g or Ctrl+w, the keys that can follow it show at the bottom of the panels with what they do,
  and the key waits for the next one instead of timing out. Ctrl+k shows every key of whatever is in
  front the same way, in its mode: a panel, a dialog, a menu, a window, the viewer, or the command
  line, which stays open under them. The next key runs as it would have, or shows the keys that can
  follow it; Esc or Ctrl+[ closes the hints, and Backspace or Ctrl+h takes back the last key, or
  closes them when none is typed. Ctrl+k after a count or a register keeps them for the key after
  it. `ui.which_key` and `ui.which_key_delay_ms` (Key hints and Key hints delay in Options →
  Configuration) turn them off and set the delay.
- The terminal's window or tab is titled with the active panel's directory, as the prompt of the
  command line shows it (`~/src — noc`, or `host:path`), and gets its own title back when Noon
  Commander quits or hands the terminal to a command, where the terminal keeps a stack of titles.
  `ui.terminal_title` (Terminal title in Options → Configuration) turns it off.
- g < shows the output of commands in place of the panels, as vim shows the output of its last
  command, wherever it is pressed in Normal mode: in the panels, over dialogs, menus, and the
  viewer; g <, Enter, q, or Esc brings back what was there. Each command now shows after its
  prompt, its output ends on a line of its own, and the line that asks for a key stands out and
  gives way to `[exit N]`, bold and red, after a command that failed. `shell.pause` (Wait after a
  command in Options → Configuration) says when the output waits for a key: `always`,
  `on-error`, or `never`.
- Command history: each command of the command line is kept in `history.toml` in the state directory
  (`~/.local/state/noc/`), with the host it ran on. Up on the first line of the command line, or
  Ctrl+p, brings back the commands of the panel's host, and Down, or Ctrl+n, goes forward to what
  was typed; on the line of :, they come after !, which runs them. q : in a panel, or on the line in
  Normal mode, opens a window of the history, which / filters: Tab shows every host, Enter puts the
  command on the line without running it, its prompt standing out if it ran on another host, and d d
  or F8 removes it. `shell.history_size` (500, as bash's `HISTSIZE`, or Command line → History size
  in Options → Configuration) sets how many commands are kept; a command that starts with a space is
  never kept.
- Input for the command line: Shift+Enter starts a new line where the terminal speaks the kitty
  keyboard protocol, which Noon Commander now asks for where the terminal has it, with the keys
  typed with Shift reported too, so that Alt and a symbol typed with Shift, such as Alt+*, stand for
  Esc and that symbol; and pasted text arrives whole through bracketed paste, so a pasted line break
  never runs a command. Text fields take pasted text without its line breaks, as one change in any
  mode, as vim puts it: in Normal mode as typed in Insert mode, after the cursor but at the start of
  a line before it, and in place of the selection in Visual mode; quick search and the filters of
  the lists take it too, a list in Normal mode in its filter; panels ignore it rather than take its
  characters as keys.
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
- Workspaces: t W saves the tabs of both panels under a name, with their sort orders and the
  rows under their cursors, and restoring one replaces every tab. t w, or
  F9 → Workspace → Workspace list…, opens their window; Enter restores, a saves the tabs as a
  new one, r or F6 renames, and d d or F8 deletes. F9 → Workspace lists them too. They are kept
  in `workspaces.toml` in the data directory (`~/.local/share/noc/`).
- Fuzzy search, as fzf does it: quick search, the filter of the location menu, and the zoxide
  window find `config.rs` from `cfg`, best match first, with fzf's `'`, `^`, `$`, and `!`.
  On by default; `ui.fuzzy_search = false`, or Fuzzy search in Options → Configuration,
  brings back literal matching.
- Shift+F6 or r (or F9 → File → Rename in place) renames the entry under the cursor in its row,
  in Insert mode with the cursor before the last extension: Enter renames, and Esc goes to
  Normal mode, then keeps the name. A taken file name asks before it is replaced; a directory's
  is never replaced.
- Shift+F9 or g M opens the pull-down menu on the command that ran from it last, as Far's
  Shift+F10 does, so Shift+F9 Enter repeats it.
- Quick search is kept once it ends, as vim keeps its last pattern: n and N go to the next and
  the previous name that matches it, round, and so does Shift+F7 to the next, as mc and Far
  search again with it; Ctrl+g in quick search with nothing typed yet searches for it again, as
  mc's Ctrl+s Ctrl+s does.

### Changed

- Text fields open with the cursor after their text, in Insert mode, rather than with the text
  selected for typing to replace: Ctrl+u clears it, as in vim. Renaming in place puts the cursor
  before the last extension.
- The panels lose a line to the line of the mode, above the F-key bar.
- The location menu of Ctrl+w 1 and Ctrl+w 2 (Alt+F1 and Alt+F2) groups its places under
  headings: Favorites (Home, for now), Mounts (the volumes), and SFTP (the hosts of the ssh
  config). Mounts and SFTP show the places used last first, and each its first ten, with the
  place the panel is on and the hosts in use; a last row, such as `24 more…`, shows the rest,
  and the filter finds them all
  ([ADR 0021](docs/adr/0021-location-menu-replaces-the-virtual-root.md)). After Ctrl+w x, it
  opens over the panel drawn on that side, as in Far, and its title names that side.
- F4 and F8 (or d d) edit the settings of a host and disconnect it in the location menu, on the
  host under the cursor, rather than in a panel on the list of hosts. F9 → Command → Edit host
  settings… and Disconnect host act on the host the active panel shows. Disconnecting leaves the
  panels on the host there, without entries, rather than sending them to the list of hosts.
- Workspaces saved with the location `root` or `sftp` open the home directory there. A location
  that `workspaces.toml` cannot hold is saved as the nearest directory above it, or else as `~`.
- Ctrl+[ does what Esc does, Ctrl+m what Enter does, and Ctrl+i what Tab does, where the
  terminal speaks the kitty keyboard protocol and so tells them apart.
- Ctrl+Space no longer shows the output of commands: macOS and input methods switch the input
  source with it.
- Ctrl+e and Ctrl+y scroll the viewer by a line, and Ctrl+d and Ctrl+u by half a page, in the
  viewer and the help, as in vim and less; Space and f turn the page down, b up, and d and u go
  half a page there, as in less.
- Ctrl+l redraws the screen everywhere, not only in the panels and the viewer: in dialogs, menus,
  windows, and on the command line too. What is in front stays as it was, and quick search goes
  on; the help lists the key under Everywhere.
- Ctrl+i does what Tab does wherever Tab does something, where the terminal tells Ctrl+i from
  Tab: it goes to the next field of a dialog, completes a path, goes through the completions,
  switches the hosts of the command history, goes to the menu on the right in the pull-down
  menu, and goes forward through the jump list in the panels.
- i closes the viewer it opens, as in ranger, and as F3 does.
- In the tab menu, d d and F8 close the tab under the cursor, as they delete a workspace in its
  window; the menu stays open with the other tabs, and the last tab stays.
- The help lists the keys of each mode and context, its own under This help.
- Release builds use full link-time optimization, which makes the `noc` binary about 6% smaller.
- The title of a panel, and the tab menu, show the home directory as `~`: `~/src` rather
  than `/Users/me/src`.
- The list of tabs is now a tab menu, like the location menu: it opens for either panel,
  whichever is active, with t h for the left one and t l for the right one, and is drawn over
  that panel rather than in the middle of the screen. / filters it, as fzf does with
  `ui.fuzzy_search`, and the mouse picks one as in the other menus. Showing a tab makes its
  panel active.
- Keys are written one way in the help, the menus, and the docs: letters in lowercase, after a
  modifier too, and in uppercase for Shift and the letter (`Ctrl+r`, `Alt+W`, `G`), `Shift+` only
  before other keys (`Shift+F6`), and `PgUp`, `PgDn` rather than `PageUp`, `PageDown`.
- Home on the first command of the pull-down menu of F9 goes back to the menu bar; on the bar,
  Up and Home no longer open the menu.
- F9 always opens the menu bar alone at the menu of the active panel, and each menu opens at its
  first command; the menu no longer opens again where it closed.

### Removed

- The virtual root and the list of hosts: volumes and hosts are chosen in the location menu.
  A local or remote `/` has no `..` any more, and the terminal and tab titles no longer show the
  name of the machine.
- The keys modelled on mc: there is one keymap, vim's, in its modes
  ([ADR 0022](docs/adr/0022-vim-modes.md)). Of mc's keys only the F-keys stay. Esc 1 … Esc 0 no
  longer stand for F1 … F10, Esc and a key no longer stand for Alt and the key, and the location
  menu has no digit hotkeys.
- `ui.type_to_search` and its row in Options → Configuration: typing in a panel no longer starts
  quick search; `/` does. Remove the key from `config.toml`, where it is an error now, as any
  unknown key is.

## [0.1.0] - 2026-10-03

First release.
