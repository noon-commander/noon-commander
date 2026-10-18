# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Dates, times, and numbers follow the formats of the region (`ui.formats`, a text field under
  Interface in the Configuration dialog, applied at once). `auto` takes them from `LC_ALL`,
  `LC_TIME`, `LC_NUMERIC`, and `LC_COLLATE`, then on macOS from System Settings → Language &
  Region, with the user's own date format, separators of numbers, and 24-hour time, then from
  `LANG`; `iso` writes `2025-10-17 14:05` and `1234.5`; a language tag names a region's formats
  from CLDR, through ICU4X. Every time shows one way, its date with the year and its time of
  day, in digits (`12.10.26 05:05`), in the panels' time column, as wide as the formats need,
  and in dialogs. Every number is grouped as the region groups it: sizes, counts, line numbers,
  speeds, and percentages. Help → About and `noc debug version` tell the language of the
  messages and where the formats come from.
- Sizes in the panels have a unit and a decimal below 100, as on the frame (`512B`, `78.5K`,
  `503G`), so their units line up in the last cell of the column; the unit grows bolder and
  brighter with the size: B and K as the text, M bold, G bold in the theme's accent, T and above
  bold in red. The status line shows every byte of the file under the cursor at its right,
  grouped, as `disk.img  3 221 225 472`.
- Panels sort names as the language of the formats orders them, with the numbers in them by
  value, so `file2` comes before `file10` and letters with accents next to the same letters
  without them. `ui.natural_sort` (on), a check box under Interface, sorts them by their
  characters, ignoring case, as before.
- With a symlink under the cursor, the status line of the panel shows its target as stored in
  the link, as `config -> /etc/app/config.toml`, locally and on a host, with a Nerd Font arrow
  in place of `->` under `ui.icons`. The arrow is toned down, and the target has the color of
  its kind, a broken one red; an
  icon of its kind goes in front of it, or without icons mc's marker after it, as `/opt/`. A
  long target loses its start.
- The bottom of a panel's frame shows at the left the size of the files shown and how many files
  and directories are shown, as `12M in 45 files, 3 dirs`; hidden files count while they show,
  and a link to a directory is a directory. Where it and the free space do not both fit, the
  free space is left out.
- Windows grow from their middle as they open and shrink back as they close, their empty frame
  in a few sizes over about 120 ms; the menus of F9 drop from their bar. Every window with a
  frame does it: dialogs, menus, the help, the Configuration dialog, the search of a tree, jobs,
  and the lists. Keys work on a window at once. `animations.windows` (on) turns it off, a check
  box under Animations in the Configuration dialog.
- Everything that waits for work in the background shows the same spinner. It shows only after 150
  ms, so quick work does not flicker. A panel's title has an icon in front of the path, a folder or
  a host in the color of its status (mc's `/` or the host's marker without icons); while the panel
  reads a directory or connects, or makes a directory of F7 or renames, braille dots turn in its
  place and the title shows the path it reads (`⠹ web:/var/www`), so the path never moves; its
  status line stays as it is. The viewer's title has the file's icon in front of the path, where the
  dots turn while it reads the file, and a skeleton line of faint bars, crossed by the same shine,
  stands for the text. A host connecting in the location menu turns braille dots too, and so do the
  location menu, the zoxide window, and the search of a whole tree in front of their title while
  they wait, which stays in place; until their first rows come, a skeleton row of faint bars stands
  in their place, a shine crossing both bars at once, one cell every 40 ms, while the dots turn; the
  status line of the search shows a skeleton bar until it ends, then how much it found. Until a job
  knows how far it is, while it waits its turn or counts, its gauge is a faint bar of `▒` with no
  percentage, the same shine running to and fro over it as fast; a stop leaves the bar still.
  `animations.spinners` (on), a check box under Animations, leaves a still mark in their place.
- R in the location menu keeps the rows while it lists the volumes and the hosts again, with the
  dots in the title; if that fails, the reason shows right of the filter in the colors of errors.
- Debug builds take `--slow-spinners SECONDS`, which holds back every result of background work
  by that long, and `--slow-connections SECONDS`, which holds back only a connection made or
  ended, to watch the spinners.
- Who is where: the prompt of the command line is a shell's, `alice@mac:~/src $` locally and
  `deploy@web:/var/www $` on a host, with the user from `ssh -G`, and the line of the mode shows
  `alice@mac` of the active panel left of the keys typed. Where the user is root, `user@host` is
  red, the prompt ends in `#`, and the panel's cursor is on red; while Noon Commander itself runs
  as root, a shield glyph (`#` without icons) stays on the line of the mode whichever panel is
  active, and the terminal title starts with `[root]`.
- `f /` searches the files below the panel's directory for text with
  [ripgrep](https://github.com/BurntSushi/ripgrep), locally or on a host, where rg runs over
  the connection, in a dialog over both panels; so does F9 → Command → Find file…. One field
  takes the text, another the globs of the files (`*.rs !target`); Ctrl+s takes the text as a
  regular expression or as it is, starting from `ui.search_mode` (fzf there is a regular
  expression), the glyph after Text saying which, and a check box, Match case, switches case
  (Alt+c, its letter); it searches again as any of them changes, and
  with no text lists the files. Each file found shows with its lines that match, the matches
  highlighted. Go to, the default button, goes to the file in the panel (Ctrl+o back), View (F3)
  views it from the line, and Edit (F4) edits it at the line. Its other mode, Files and dirs,
  finds files and directories by name with [fd](https://github.com/sharkdp/fd), locally or on a
  host, where fd or fdfind runs; Go to goes into a directory. `f f` and F9 → Command → Find files
  and dirs… open the dialog in it. `[search] fd` names the program.
  It finds at most `[search] max_results` (10000) lines, and `max_per_file` (100) in one file,
  whose count then shows as `100+`; they and `program` (`rg`) are in the new Search category of
  the Configuration dialog. Hidden files are searched while the panels show them, and
  `.gitignore` is respected.
- Quick search highlights where each name on screen matches what is typed: each place it holds
  the text, or the letters that fuzzy search matched, on the cursor's row too.
- Search modes: quick search and the filters of the lists (the location menu, the tab menu, the
  zoxide window, the workspaces, the command history, the list of the key hints) match what is
  typed as fzf does, as plain text anywhere, or as a regular expression with smart case. Each
  starts in `ui.search_mode` (`fzf`), a choice in the Configuration dialog, and Ctrl+s switches
  its own until it closes; a glyph after the word of the prompt says which (`Filter ~:`), crossed
  out while the text is no regular expression. `n` and `N` match as the last search did.
- Letters in every dialog, as in Far and mc: each element but Cancel has a letter, highlighted in
  its label, and Alt and the letter put the focus in a text field, switch a check box, choose a
  radio button, or press a button, from anywhere in the dialog, while typing in a field too. A
  dialog of one field takes its letter from the prompt over it, as Alt+c for `Copy "x" to:`. The
  window of a job, the jobs, the checksums, and the Configuration dialog have letters too; there
  Alt+1 … Alt+8 open the categories, and the letter of a setting switches it, picks its next
  choice, or puts the focus in its field. The sections of the help (F1) have letters, and Alt and
  one shows its section.
- The help (F1) takes 70% of the screen and has sections down its left, as the Configuration dialog:
  About, with the license and every fact of `noc debug version` (the build, the system, the
  terminal, and the external tools: the editor, zoxide, ripgrep, and fd), and where the settings,
  workspaces, history, and log are; and Keys, which folds, with the keys grouped by part of the app
  (panels, text fields, search and renaming, the command line, lists and menus, menus and dialogs,
  the viewer) and tips, each under its title in points. The parts of a section have their titles in
  bold on a line across, `── Panels ──`. It opens where it was left: the section, Keys open or
  folded, and how far down the text was. Tab moves between the sections and the text, and a click
  chooses a section.
- Keys have a color of their own wherever they are listed with what they do, apart from titles:
  in the help, the key hints, their list of Ctrl+k, and the menus of F9.
- `g x` opens the marked entries, or the one under the cursor, externally, as vim's `gx` does:
  with the program the desktop has for each, through `open` on macOS and `xdg-open` on Linux;
  a directory opens in the desktop's file manager. In Visual mode it opens the rows selected,
  and F9 → File has it as Open externally. Only local files open; on a host the line of the
  mode says so.
- Ctrl+k while the key hints show gives them the keys, as `K K` does to nvim's hover window: a
  list of every key sequence whole, `Ctrl+w h` rather than a `Ctrl+w` group, with what each does,
  over whatever is in front. `/` filters it by keys and by what they do, Enter runs the command
  under the cursor with the count typed before Ctrl+k, and Esc closes it.
- `noc debug version` prints one `key value` line per fact, for scripts and bug reports: the
  version, the compile-time features, the commit and its time in UTC, the target, the operating
  system, the terminal with its colors and keyboard protocol, the editor of F4, zoxide, as
  `zoxide --version` says, ripgrep, as `rg --version` says, and fd, as `fd --version` says, or
  `not found`. The terminal is asked for its name, so kitty, which sets no `TERM_PROGRAM`, is named
  too.
- Scroll bars on the right of the frames wherever the rows do not fit, as in Far: the panels,
  the help, the Configuration dialog, the location and tab menus, the Workspaces, history, and
  zoxide windows, the menus of F9, the list of completions, the jobs, and the checksums; the
  viewer has one in its last column.
- Scroll bars take the mouse: they end in arrows (Nerd Font carets, or `▲` and `▼` without
  icons), a click on one does what a step of the wheel does, a click on the track pages towards
  it, and the thumb can be dragged.
- A dialog's message taller than the screen scrolls, with the mouse and the wheel, and keeps the
  buttons on screen; so do the rows of a command line that do not fit.
- `K` shows the whole name under the cursor in a frame below the panels, in the colors of dialogs,
  following the cursor until `K` or Esc, and the whole row under the cursor under the rows of the
  location menu, the tab menu, the zoxide window, and the Workspaces window, where the paths of a
  workspace's tabs show. Each of them shows it on its own; Options → Show whole names does it for
  the panels.
- The z keys scroll sideways as vim's do on lines that do not wrap: `z h` and `z l` a column,
  with a count that many, `z H` and `z L` half the width, `z s` to the start, and `z e` the end
  into view; the names in the panels, and the lines in the viewer while they are cut.
- The viewer marks lines cut at the edges with `«` and `»`, and starts each wrapped row after the
  first with `»`, as vim's 'listchars' and 'showbreak'.

### Changed

- The lists (the location menu, the tab menu, the Workspaces window, the command history, and the
  list of the key hints) open in Insert mode with their filter taking what is typed, as the
  zoxide window does: Esc goes to the rows, in Normal mode, where `j`, `k`, `d d`, and `q` work,
  `/` or `i` types in the filter again, and Esc closes the window
  ([ADR 0036](docs/adr/0036-lists-open-in-insert-mode.md)). The filter, and the keywords of the
  zoxide window, look like the text fields of dialogs: the colors of a field from the prompt to
  the right edge.
- Fewer lines in the lists: the line under the filter and the one over what `K` shows join the
  frame (`╟───╢`), as the line over the buttons of a dialog does, and the headings of the location
  menu are their names alone, in the color of titles, without a line.
- Messages with a count have plural forms for each: deleting, copying, moving, and checksums of
  several entries, the limit of the search of a tree, and the sizes in the question about a taken
  name; a translation gets the number itself, never `1` written out. The units of sizes, the marks
  of a time and a speed not known yet, and the percentage of a job's gauge are in the messages.
- Quick search, the filters of the lists, `g +` and `g -`, and the completion of paths match
  names in either Unicode form, so a name that macOS keeps in NFD matches the accented letter
  typed, and ignore case by full case folding, so `strasse` finds `Straße`; the highlight covers
  the whole letter with its accent. Regular expressions match the name in NFC.
- A job that moves data counts its sizes with their units, as `1.5M of 4.2G`, rather than
  `bytes` after sizes that had a unit already.
- The viewer's header and end take the size of the start of a long file it reads from the code
  rather than repeating it.
- The Noon themes put error dialogs on a deep red (`#A63A32`) in `noon-dark` and a pale one
  (`#F2A39A`) in `noon-light`, with the cream or navy text of the cursor, rather than on the
  bright red of text, which glared.
- The wheel moves the cursor of a panel where the rows cannot scroll, as when they all fit or the
  last row shows, as in mc and Far; before, it did nothing there.
- Tab in the panels goes to the other panel, as in mc, where the terminal speaks the kitty
  keyboard protocol and so tells Tab from Ctrl+i, which still goes forward through the jump list.
  Elsewhere Tab is Ctrl+i, as before.
- `i` in a panel opens the command line for a shell command, in Insert mode, as `!` still does.
  The viewer moved from `i` to `g f`, as vim's goes to the file under the cursor; F3 stays, and
  `i` no longer closes the viewer.
- The scroll bars of the help and the Configuration dialog moved from a column inside onto the
  frame, which gives their text two more cells.
- A menu of F9 taller than the screen, the list of jobs, and the list of checksums scroll as
  little as shows the cursor, rather than keeping it on their last row.
- `noc -V` and `noc --version` print only the version, as in `noc 0.2.0`; the compile-time
  features (`-forwarding`) moved to `noc debug version`.
- `noc hosts` and `noc ls` moved to `noc debug hosts` and `noc debug ls`.
- Backspace and Delete with nothing typed no longer close quick search, as on the command line,
  so that pressing Backspace once too often no longer goes to the parent directory.
- Ctrl+f in the panels and the lists stops with the last row at the bottom, then goes to it,
  rather than scroll on until only the last row shows at the top, as vim's does.
- The help and the viewer stop scrolling with the end at the bottom, for `j`, Ctrl+e, and
  Ctrl+f too. In the viewer a dimmed `──── EOF ────` across the text follows the last line; a
  scroll bar shows on the right when the text does not fit, and the position says `End` once the
  end shows: the viewer's on top, the help's in its bottom frame.
- Text cut short ends or breaks with `»` instead of `~`, in a bold accent of the theme, so that it
  no longer looks like part of a name nor like mc's marker of a symlink to a directory; with
  icons, a Nerd Font double chevron. The accent shows everywhere text is cut: the panels, the
  key hints of Ctrl+k, the help, the menus of F9 and the menu bar, the F-key bar, the line of tabs,
  the command line, dialogs, and the viewer's title, in a style of its own for each background.
  Paths that do not fit lose their start rather than their middle, as in `«/deploy/site`.
- Shift+Down and Shift+Up mark a row and move down or up, as in mc and Far; `J` and `K` no longer
  mark, and `K` shows the whole name under the cursor.
- `l`, `z l` and `z L` in the viewer stop once the end of the longest line on screen reaches the
  right edge, where `z e` puts it, so that the text never scrolls out of sight; before, they went
  on until its last character reached the left edge.
- `z l` and `z L` in the panels stop once the end of the longest name on screen reaches the right
  edge, as in the viewer; before, they went by the name under the cursor, until its last
  character reached the left edge, so they went further on a longer name, jumped back on a
  shorter one, and could leave the column empty.
- The names in the panels and the lines in the viewer scroll back by themselves as far as the
  ones on screen need, as after paging to shorter ones or widening the terminal.
- The zoxide window (`g z`) opens in Insert mode, so keywords can be typed at once, as in `zi`,
  without `/` first. Esc goes to the rows, and Esc there closes the window.
- `ui.search_mode` (`"plain"`, `"fzf"`, `"regex"`) replaces `ui.fuzzy_search`, which is now an
  unknown key: a `config.toml` that still has it does not load until it is removed or changed
  to `search_mode`.
- Quick search shows the text as typed, capitals too, rather than in lower case; in the plain
  mode it still ignores case.
- The line of the mode says the mode everywhere, as lualine does, rather than vim's `-- INSERT --`
  in some places and nothing in others: `NORMAL`, `INSERT`, `VISUAL`, then the kind of window that
  has the keys, which says which keys work there: `Quick search`, `Rename`, `List` for the menus of
  places and tabs and the other lists, `Menu` for F9, `Dialog` for every dialog, `Help`, and
  `Viewer`, as `INSERT · Quick search` or `NORMAL · List`; the panels show the mode alone. The
  command line takes the line only while it has the keys: the command history of `q :` over it
  shows `NORMAL · List`.
- In dialogs, Alt and a key no longer stand for Esc and the key: Alt and a letter are the
  dialog's letters, and other Alt keys do nothing. Without the kitty keyboard protocol, an Esc and
  a quick letter typed in a field arrive as Alt and the letter, and so act on its element rather
  than leave Insert mode.

### Fixed

- A prompt whose translation put what is typed before the glyph of the search mode could
  panic, and the cursor of every prompt went after the whole message, wherever the typed text
  was in it.
- The viewer no longer lags in files with very long lines, such as binary files, while it
  wraps them: each line wraps once for each width, rather than again for every row it scrolls.

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
