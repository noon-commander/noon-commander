# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- When a connection to a host fails, or is lost, a dialog says why and offers Retry, the
  default, and Cancel. The panels on a lost host stay on their directory, without its entries,
  and read it again once Retry connects; after Cancel they stay there, and Ctrl+r, or Enter on
  `..`, connects again. While a panel waits for a host, its status line starts with a spinner.
- `recent.toml` in the state directory (`~/.local/state/noc/`) keeps the hosts and volumes used
  last, up to 50 of each, for the location menu.
- Key hints, as which-key shows them: half a second after a key that starts longer sequences,
  such as Ctrl+x, or g and Ctrl+w in the vim keymap, the keys that can follow it show at the
  bottom of the panels with what they do, and the key waits for the next one instead of timing
  out. Alt+/ shows every key of whatever is in front the same way, in both keymaps: a panel, a
  dialog, a menu, a window, the viewer, or the command line, which stays open under them. The
  next key runs as it would have, or shows the keys that can follow it; Esc or Ctrl+[ closes the
  hints, and Backspace or Ctrl+h takes back the last key.
  `ui.which_key` and `ui.which_key_delay_ms` (Key hints and Key hints delay in Options →
  Configuration) turn them off and set the delay.
- `ui.keymap` (Keymap in Options → Configuration) chooses the keymap by name, as `ui.theme` chooses
  the colors, and takes effect at once: `default`, modelled on mc, or `vim`. In the vim keymap, k,
  j, g g, and G move in dialogs too, and g m opens the pull-down menu, as F9 does, where Ctrl+n and
  Ctrl+p move, Ctrl+f and Ctrl+b, or Shift+Down and Shift+Up, go to the last and first command, and
  Ctrl+c closes it. In its panels, Tab, Ctrl+w w, and Ctrl+w Ctrl+w switch to the other panel,
  Ctrl+w h and Ctrl+w l go to the left and the right one, Ctrl+w x swaps them, Ctrl+w f opens the
  directory under the cursor in the other panel, and Ctrl+w v shows this directory there, as vim's
  window commands do; Ctrl+w 1 and Ctrl+w 2 change the left and the right panel's location, as
  Ctrl+x 1 and Ctrl+x 2 do in the default keymap. In the location menu, the zoxide window, the
  workspaces, and the command history, Ctrl+n, Ctrl+p, Ctrl+f, and Ctrl+b move as in the panels,
  and Ctrl+c closes them: the four share their keys in both keymaps. Ctrl+y does what Enter does in
  the menus, the lists, and the list of completions, as in vim's pop-up menu, but not in dialogs,
  renaming, or on the command line, where the shell, mc, and ranger paste with it. In
  both keymaps, Ctrl+h does what Backspace does wherever text is typed, as in mc, and in the default
  one Alt+< and Alt+> go to the first and the last row of a panel or a list, Ctrl+p, Ctrl+n, Alt+v,
  and Ctrl+v move in the lists as in the panels, and Ctrl+p and Ctrl+n in the pull-down menu, and
  Ctrl+i switches panels, as in mc, where the terminal tells Ctrl+i from Tab; Ctrl+g closes the
  lists, Ctrl+g and Ctrl+c dialogs, and in the viewer Insert and Delete scroll a line and Ctrl+h a
  page up, and Ctrl+PgUp and Alt+< go to the start, Ctrl+PgDn and Alt+> to the end, as in mc. In the
  vim keymap, Ctrl+c closes the windows and the lists and ends quick search and renaming, as Esc
  does. In both, F10 closes the command line and the completion list, and Ctrl+c closes the command
  line, as in a shell. `noc keymap diff` compares two keymaps action by action, the default and the
  vim one unless named: `|` where the keys differ, `<` and `>` for an action only one of them binds,
  in color in a terminal; `--all` shows what they bind alike too.
- The vim keymap does in its panels everything the default one does, with vim's keys, and ranger's
  and yazi's where vim has none: l and Right open what is under the cursor, as Enter does, h, Left,
  and Backspace go to the parent directory, Ctrl+u and Ctrl+d go half a page up and down, J, Space,
  Shift+Down, and Insert mark and go down, K and Shift+Up mark and go up, + marks and - unmarks by
  a pattern, * inverts the marks, / starts quick search, where Ctrl+g and Ctrl+t go to the next
  and the previous match and Enter ends it on the match rather than opening it, i views, e edits,
  Y and y y copy, x moves, r renames in the row, A makes a directory, d D deletes, f # makes
  checksums, R reads the directory again, o h shows hidden files, o n, o e, o m, and o s sort,
  q : opens the command history, f j the jobs, g d Quick cd, g z jumps with zoxide, g n, g c, g t,
  and g T open, close, and switch tabs, t h and t l list the left and the right panel's tabs, t w
  and t W list and save workspaces, Z Z quits, and Esc and Ctrl+c stop loading; F3 … F8, Alt+Left,
  and Alt+Right do what they do in the default keymap. q closes the help, and Ctrl+b and Ctrl+f
  turn pages in dialogs. On the command line, Ctrl+b goes to the start, as on vim's command line,
  Ctrl+p and Ctrl+n go through the history in place of Alt+p and Alt+n, Ctrl+r opens it, and
  Ctrl+f opens the command in the editor; Ctrl+p and Ctrl+n move in the completion list.
- The terminal's window or tab is titled with the active panel's directory, as the prompt of the
  command line shows it (`~/src — noc`, or `host:path`), and gets its own title back when Noon
  Commander quits or hands the terminal to a command, where the terminal keeps a stack of titles.
  `ui.terminal_title` (Terminal title in Options → Configuration) turns it off.
- Ctrl+o shows the output of commands in place of the panels, as in mc and Far, wherever it is
  pressed: in the panels, on the command line, over dialogs, menus, and the viewer; the same key
  or Esc brings back what was there. The vim keymap takes g < too, vim's key for the output of the
  last command. Each command now shows after its prompt,
  its output ends on a line of its own, and the line that asks for a key stands out and gives way to
  `[exit N]`, bold and red, after a command that failed. `shell.pause` (Wait after a command in
  Options → Configuration) says when the output waits for a key: `always`, `on-error`, or `never`.
- Command history: each command of the command line is kept in `history.toml` in the state
  directory (`~/.local/state/noc/`), with the host it ran on. Up on the first line of the
  command line, or Alt+p, brings back the commands of the panel's host, and Down, or Alt+n,
  goes forward to what was typed. Alt+h or Ctrl+r on the line, or Alt+h in a panel, opens a
  window of the history that typing filters: Tab shows every host, Enter puts the command on
  the line without running it, its prompt standing out if it ran on another host, and Delete,
  Shift+Delete, or F8 removes it. `shell.history_size` (500, as bash's `HISTSIZE`, or Command
  line → History size in Options → Configuration) sets how many commands are kept; a command
  that starts with a space is never kept.
- Input for the command line: Shift+Enter starts a new line where the terminal speaks the
  kitty keyboard protocol, which Noon Commander now asks for where the terminal has it, with
  the keys typed with Shift reported too, so that Alt with a symbol such as Alt++ or Alt+* still
  does what it does elsewhere; Ctrl+x Ctrl+e opens the command in `$VISUAL` or `$EDITOR` and
  brings back what the editor left, without running it; and pasted text arrives whole through
  bracketed paste, so a pasted line break never runs a command. Text fields and quick search
  take pasted text without its line breaks, and panels ignore it rather than take its
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
  F9 → Workspace → Workspace list…, opens their window, which filters them; Enter restores,
  Insert saves the tabs as a new one, and F6 and F8 rename and delete them. F9 → Workspace
  lists them too. They are kept in `workspaces.toml` in the data directory
  (`~/.local/share/noc/`).
- Fuzzy search, as fzf does it: quick search, the filter of the location menu, and the zoxide
  window find `config.rs` from `cfg`, best match first, with fzf's `'`, `^`, `$`, and `!`.
  On by default; `ui.fuzzy_search = false`, or Fuzzy search in Options → Configuration,
  brings back literal matching.
- Shift+F6 (or F9 → File → Rename in place) renames the entry under the cursor in
  its row: the name is selected but for the last extension, Enter renames, and Esc keeps the
  name. A taken file name asks before it is replaced; a directory's is never replaced.
- Shift+F9 (or g M in the vim keymap) opens the pull-down menu on the command that ran
  from it last, as Far's Shift+F10 does, so Shift+F9 Enter repeats it.
- Quick search is kept once it ends, as vim keeps its last pattern: in both keymaps, n and N go to
  the next and the previous name that matches it, round, and so does Shift+F7 to the
  next, as mc and Far search again with it; Ctrl+s with nothing typed yet (/ and Ctrl+g in the
  vim keymap) searches for it again, as mc's Ctrl+s Ctrl+s does.
- Shift+PgUp and Shift+PgDn go half a page up and down in the panels, as in yazi; Ctrl+u and
  Ctrl+d do in the vim keymap.
- In both keymaps, Ctrl+w h and Ctrl+w l, or Ctrl+x Left and Ctrl+x Right, make the panel drawn on
  the left or on the right active, after Ctrl+u too. In the default keymap, Ctrl+w f and Ctrl+w v
  do what Alt+o and Alt+i do, where the Option key of a macOS terminal sends no Alt.
- In the default keymap, Backspace goes to the parent directory too, as Ctrl+PgUp does, and Ctrl+c
  stops loading, as Esc does. In both keymaps, Ctrl+h goes to the parent directory as Backspace
  does, for terminals that send it for Backspace.
- In the location menu, Delete disconnects the host under the cursor, as F8 does and as in Far's
  menu of drives.
- Alt+1 … Alt+0 stand for F1 … F10 in the windows that list items and in the viewer too, as in
  the panels: Alt+4 and Alt+8 edit and disconnect a host in the location menu, for one, and Alt+2
  wraps the lines of the viewer.

### Changed

- The location menu of Alt+F1 and Alt+F2 groups its places under headings: Favorites (Home, for
  now), Mounts (the volumes), and SFTP (the hosts of the ssh config). Mounts and SFTP show the
  places used last first, and each its first ten, with the place the panel is on and the hosts in
  use; a last row, such as `24 more…`, shows the rest, and the filter finds them all. The digits
  skip that row ([ADR 0021](docs/adr/0021-location-menu-replaces-the-virtual-root.md)).
- After Ctrl+u, Alt+F1 and Alt+F2 open the location menu of the panel drawn on the left and on
  the right, as in Far, and its title names that side; before, they kept to the panels as they
  were before the swap.
- F4 and F8 edit the settings of a host and disconnect it in the location menu, on the host under
  the cursor, rather than in a panel on the list of hosts. F9 → Command → Edit host settings… and
  Disconnect host act on the host the active panel shows. Disconnecting leaves the panels on the
  host there, without entries, rather than sending them to the list of hosts.
- Workspaces saved with the location `root` or `sftp` open the home directory there. A location
  that `workspaces.toml` cannot hold is saved as the nearest directory above it, or else as `~`.
- Esc acts at once in the panels too, instead of waiting a second for a key, as in mc: a key
  typed right after it does what it does alone, where before it could be lost or run an Alt key.
  Alt+1 … Alt+0 stand for F1 … F10 in place of Esc 1 … Esc 0, and Esc followed by a key no longer
  stands for Alt and the key ([ADR 0020](docs/adr/0020-esc-acts-at-once.md)).
- Typing in a panel no longer starts quick search; Ctrl+s or Alt+s do, as in mc. The setting
  `ui.type_to_search` and its row in Options → Configuration are gone; remove the key from
  `config.toml`. `!` and `:` will open a command line for shell commands.
- Quick search edits its text as the other text fields do, with the same keys in both keymaps:
  Left, Right, Home, End, Backspace, Delete, Ctrl+u, Ctrl+k, Ctrl+a, and Ctrl+e. While it runs,
  Home and End move in the text rather than to the first and the last row, and Ctrl+u clears it.
  The help lists Ctrl+s and Alt+s with quick search, as the keys that go on to the next match.
- The filters of the location menu, the tab menu, the workspaces, and the command history, and the
  keywords of the zoxide window, are text fields too: Left and Right move in them, and Ctrl+a and
  Ctrl+e go to their start and end (in the vim keymap too, whose Ctrl+b pages up there); Delete,
  Ctrl+u, and Ctrl+k delete, as Backspace does. Home and End still go to the first and the last
  row; Delete still deletes the workspace or the command under the cursor, and disconnects the
  host in the location menu.
- Ctrl+[ does what Esc does, and Ctrl+m what Enter does, in both keymaps, where the terminal
  speaks the kitty keyboard protocol and so tells them apart.
- Ctrl+Space no longer shows the output of commands: macOS and input methods switch the input
  source with it. In the vim keymap, g < does, as vim shows the output of its last command, and
  brings the panels back.
- In quick search, Ctrl+g (Esc and Ctrl+c in the vim keymap) gives the search up: the cursor goes
  back to where it started, and n and N keep the search before. Ctrl+r goes to the previous match
  in the default keymap, as in emacs's search, and Ctrl+c ends the search there as Esc does.
- Ctrl+w, and Ctrl+Backspace, delete the word before the cursor, and the spaces after it, in every
  text field, quick search, the filters of the lists, and on the command line, as in the shell;
  Ctrl+Left and Ctrl+Right go by words there (and Alt+b and Alt+f in the default keymap, which
  the Option key and the arrows of macOS's Terminal send; Shift+Left and Shift+Right in the vim
  keymap, as on vim's command line).
- In the default keymap, Ctrl+c closes and cancels wherever Esc does, but on the terminal's own
  screen: renaming, the windows that list items, the list of completions, the pull-down menu, and
  the viewer too, as in the vim keymap, whose viewer it closes now too. Ctrl+g, mc's and emacs's
  key, cancels renaming, the pull-down menu, the list of completions, and the command line too.
  Ctrl+p and Ctrl+n move in the list of completions, and Ctrl+p, Ctrl+n, Alt+v, Ctrl+v, Alt+<,
  and Alt+> in the help, as in the panels.
- More keys of mc's input lines and the shell in the default keymap: Ctrl+b, Ctrl+f, and Ctrl+d
  move and delete in the text fields, Alt+d deletes the word after the cursor, Ctrl+p and Ctrl+n
  go through the history on the command line, and the list of completions pages and goes to its
  ends with Alt+v, Ctrl+v, Alt+<, and Alt+>. Ctrl+p and Ctrl+n move in the dialogs' lists, such as
  the jobs and the checksums. In both keymaps, Alt+Backspace deletes the word before the cursor,
  as macOS's Option+Backspace does, and Ctrl+Delete the word after it.
- In the vim keymap, Ctrl+e and Ctrl+y scroll the viewer by a line, and Ctrl+d and Ctrl+u by half
  a page, in the viewer and the help, as in vim and less; Ctrl+b and Ctrl+f page the list of
  completions, as they page the other lists.
- In the vim keymap, Space comes first of the keys that mark, and Ctrl+w p and Ctrl+w r switch and
  swap the panels too, as vim's Ctrl+w p and Ctrl+w r do with windows.
- More of mc's keys in the default keymap: Ctrl+PgDn opens the directory under the cursor, as
  Ctrl+PgUp goes to the parent; u and d go half a page in the viewer and the help, where Space and
  f turn the page down, b and Backspace up, and g and G go to the top and the end, as in mc's help;
  Ctrl+b and Ctrl+f go to the menu on the left and on the right in the pull-down menu, and Alt+<
  and Alt+> to its first and last command.
- In the vim keymap, Ctrl+a goes to the start of text fields, as in the shell, and of the filters
  of the lists, whose Ctrl+b pages up; Ctrl+b and Ctrl+f page from the text fields as everywhere
  but on the command line, so they end quick search and turn the panel's page, or a dialog's; h
  and l go from button to button in dialogs, and Ctrl+p and Ctrl+n move in them, as in the lists;
  Enter and q leave the output of commands, as the prompt under what vim's g < shows does, and the
  help names g < first for it.
- Ctrl+l redraws the screen everywhere in both keymaps, not only in the panels and the viewer: in
  dialogs, as in mc's, and in menus, windows, and on the command line too. What is in front stays
  as it was, and quick search goes on; the help lists the key under Everywhere.
- Ctrl+i does what Tab does wherever Tab does something, in both keymaps, where the terminal
  tells Ctrl+i from Tab: it goes to the next field of a dialog, completes a path, goes through the
  completions, switches the hosts of the command history, goes to the menu on the right in the
  pull-down menu, and in the vim keymap switches panels too, as it already did in the default one.
- More of mc's keys in the default keymap: Alt+< and Alt+> go to the start and the end of a text
  field, in quick search and on the command line too, while the lists keep them for their first
  and last row; Ctrl+a and Ctrl+e go to the first and the last command of the pull-down menu. The
  help names Alt+Backspace first of the keys that delete the word before the cursor, as mc's.
- In the vim keymap, i closes the viewer it opens, as in ranger, and as F3 does.
- In the tab menu, F8 and Delete close the tab under the cursor, as they delete a workspace in its
  window; the menu stays open with the other tabs, and the last tab stays.
- In the vim keymap, Space and f turn the page down, b up, and d and u go half a page in the
  viewer and the help, as in less and ranger's pager; Ctrl+e and Ctrl+y scroll the help by a line,
  as they scroll the viewer.
- The help lists its own keys under This help, such as mc's Space, b, u, and d in the default
  keymap, which it left out.
- Release builds use full link-time optimization, which makes the `noc` binary about 6% smaller.
- The title of a panel, and the tab menu, show the home directory as `~`: `~/src` rather
  than `/Users/me/src`.
- The list of tabs is now a tab menu, like the location menu: it opens for either panel,
  whichever is active, with t h for the left one and t l for the right one in place
  of Ctrl+x Tab, and is drawn over that panel rather than in the middle of the screen. Typing
  filters it, as fzf does with `ui.fuzzy_search`; while the filter is empty, 1 … 9 and 0 show
  the first ten tabs, and the mouse picks one as in the other menus. Showing a tab makes its
  panel active. The actions are `tab_list_left` and `tab_list_right`, in place of `tab_list`,
  and the menu has keys of its own in the `tab_menu` context.
- The default keymap takes the tab and sort keys of the vim keymap: t n opens a tab, t c closes
  it, ] t and [ t switch tabs (Alt+Right and Alt+Left too), and o n, o e, o m, and o s sort, in
  place of Ctrl+x t, Ctrl+x w, Ctrl+x n, Ctrl+x p, and Ctrl+F3 … Ctrl+F6; o h shows hidden files,
  as Alt+. does. Both keymaps make
  checksums with f #, in place of Ctrl+x # and Ctrl+w #, and list the jobs with f j: the
  default one after Ctrl+x j, the vim one in place of w.
- Keys are written one way in the help, the menus, and the docs: letters in lowercase, after a
  modifier too, and in uppercase for Shift and the letter (`Ctrl+r`, `Alt+W`, `G`), `Shift+` only
  before other keys (`Shift+F6`), and `PgUp`, `PgDn` rather than `PageUp`, `PageDown`.
- Home on the first command of the pull-down menu of F9 goes back to the menu bar; on the bar,
  Up and Home no longer open the menu.
- F9 always opens the menu bar alone at the menu of the active panel, and each menu opens at its
  first command; the menu no longer opens again where it closed.

### Removed

- The virtual root and the list of hosts: volumes and hosts are chosen in the location menu.
  A local or remote `/` has no `..` any more, the `root` keymap context is gone, with D D of the
  vim keymap, and the terminal and tab titles no longer show the name of the machine.

## [0.1.0] - 2026-10-03

First release.
