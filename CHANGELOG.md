# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- F3 reads a file in parts (`viewer.part_mib`, 16 MiB) and holds a window of it
  (`viewer.window_mib`, 128 MiB), both under Viewer in the Configuration dialog, so any part of
  a large file can be seen: the next part is read when the screen reaches the end of what is held,
  and a dropped one again when it scrolls back. A read that takes long shows a dialog with a gauge
  of the part; Stop (Esc) keeps what was read, Close closes the viewer. Until the end is known the
  position shows `line/? percent%` and the text ends with `more below`. A file that changes
  between parts is read again from the start; a later part that fails keeps the text.
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
  dots turn while it reads the file. A host connecting in the location menu turns braille dots
  too, and so do the location menu, the zoxide window, and the search of a whole tree in front of
  their title while
  they wait, which stays in place; until their first rows come, a skeleton of faint bars stands in
  their place, as many rows as the window had the last time it opened (one, the first time), so the
  box does not grow when they come, a shine crossing every bar at once, one cell every 40 ms, while
  the dots turn; the status line of the search shows a skeleton bar until it ends, then how much it
  found. Until a job knows how far it is, while it waits its turn or counts, its gauge is a faint
  bar of `▒` with no percentage, the same shine running to and fro over it as fast; a stop leaves
  the bar still. `animations.spinners` (on), a check box under Animations, leaves a still mark in
  their place.
- R in the location menu keeps the rows while it lists the volumes and the hosts again, with the
  dots in the title; if that fails, the reason shows right of the filter in the colors of errors.
- Debug builds take `--slow-spinners SECONDS`, which holds back every result of background work by
  that long, and `--slow-connections SECONDS`, which holds back only a connection made or ended, to
  watch the spinners; `--slow-windows SECONDS` has a window take that long to grow as it opens and
  to shrink as it closes; `--show-sizes` has each window write its size, frame included, as
  `WIDTHxHEIGHT` at the right of the bottom line of its frame.
- Who is where: the prompt of the command line is a shell's, `alice@mac:~/src $` locally and
  `deploy@web:/var/www $` on a host, with the user from `ssh -G`, and the line of the mode shows
  `alice@mac` of the active panel left of the keys typed. Where the user is root, `user@host` is
  red, the prompt ends in `#`, and the panel's cursor is on red; while Noon Commander itself runs
  as root, a shield glyph (`#` without icons) stays on the line of the mode whichever panel is
  active, and the terminal title starts with `[root]`.
- `f /` searches the files below the panel's directory for text with
  [ripgrep](https://github.com/BurntSushi/ripgrep), locally or on a host, where rg runs over the
  connection, in a dialog over both panels; so does F9 → Command → Find file…. One field takes the
  text, another the globs of the files (`*.rs !target`); Ctrl+s and a letter in the text take it as
  a regular expression or as it is, starting from `ui.default_search_mode` (fzf there is a regular
  expression), and switch its case, smart at first, and in the globs, shell patterns, switch their
  case, glob by glob (`--iglob` where it does not count; by name, fd tells case apart in them), the
  glyphs at the end of each field saying which, dimmed where a click does not switch them; it
  searches again as any of them changes, and with no text lists the files. Each file found shows
  with its lines that match, the matches highlighted. Go to, the default button, goes to the file in
  the panel (Ctrl+o back), View (F3) views it from the line, and Edit (F4) edits it at the line. Its
  other mode, Files and dirs, finds files and directories by name with
  [fd](https://github.com/sharkdp/fd), locally or on a host, where fd or fdfind runs; Go to goes
  into a directory. `f f` and F9 → Command → Find files and dirs… open the dialog in it. `[search]
  fd` names the program. It finds at most `[search] max_results` (10000) lines, and `max_per_file`
  (100) in one file, whose count then shows as `100+`; they and `program` (`rg`) are in the new
  Search category of the Configuration dialog. Hidden files are searched while the panels show them,
  and `.gitignore` is respected.
- Quick search highlights where each name on screen matches what is typed: each place it holds
  the text, or the letters that fuzzy search matched, on the cursor's row too.
- Search modes: quick search and the filters of the lists (the location menu, the tab menu, the
  zoxide window, the workspaces, the command history, the list of the key hints) match what is typed
  as fzf does, as plain text anywhere, or as a regular expression. Each opens as it was last until
  Noon Commander quits, the first time in `ui.default_search_mode` (`fzf`), a choice in the
  Configuration dialog, and in smart case: case counts only once what is typed has a capital letter
  ([ADR 0047](docs/adr/0047-search-fields-remember-their-way-and-case.md)). Two glyphs at the end of
  the field say the case and the way (`Filter: text   Aa ~`), the way's crossed out while the text
  is no regular expression. In the field, and nowhere else, not on the rows of a list, Ctrl+s leads
  the keys that switch them: `Ctrl+s Ctrl+s` or `Ctrl+s s` the next way, `Ctrl+s p`, `f`, `r`, `g` a
  way by its letter, `Ctrl+s c` the next case, and `Ctrl+s a`, `i`, `m` smart case, case ignored, or
  case; a click on a glyph switches it too ([ADR
  0045](docs/adr/0045-ctrl-s-switches-the-field-typed-in.md), [ADR
  0046](docs/adr/0046-case-and-the-keys-of-search-fields.md)). `n` and `N` match as the last search
  did, with its case.
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
  following the cursor until `K` or Esc; Options → Show whole names does it too. The lists always
  show the whole row under the cursor under their rows, while they have rows: the location menu,
  the tab menu, the zoxide window, and the key hints, with all the keys of a hint
  ([ADR 0041](docs/adr/0041-lists-always-show-the-whole-row.md)).
- The Workspaces window shows, under its rows, where the tabs of the workspace under the cursor
  are: a column for each panel, headed Left and Right, one tab to a line, its whole location, in
  as many lines as the workspace with the most tabs on a side needs, four at most, the last saying
  how many more; the heading of the side with the keys, and the tab that shows on each side, stand
  out. A line between the columns joins the frame (`┬`, `╧`).
- The z keys scroll sideways as vim's do on lines that do not wrap: `z h` and `z l` a column,
  with a count that many, `z H` and `z L` half the width, `z s` to the start, and `z e` the end
  into view; the names in the panels, and the lines in the viewer while they are cut.
- The viewer marks lines cut at the edges with `«` and `»`, and starts each wrapped row after the
  first with `»`, as vim's 'listchars' and 'showbreak'.
- Shift+F5, or `y p`, as vim's `y y p` puts a copy of a line under it, copies the entry under the
  cursor, not the marked ones, as mc's and Far's Shift+F5: the dialog of F5 opens with its name,
  the cursor before the extension, so that the copy goes beside it under the name typed; a path
  there copies elsewhere. File → Duplicate does it too.
- Ctrl+x runs the next command in the other panel, then gives the keys back, as vim's Ctrl+o in
  Insert mode runs one command of Normal mode: any command, as if the other panel were active, so
  Ctrl+x `g l` opens its location menu, Ctrl+x `j` moves its cursor, and Ctrl+x F5 copies from it
  to this panel. The keys come back once the command ends: at once, or when its window closes, its
  Visual mode or quick search ends, or the program it ran returns. Its count goes after it
  (Ctrl+x `2g t`); Esc and Ctrl+x again give it up. While the command has the keys, the line of the
  mode starts with `(^X)`
  ([ADR 0051](docs/adr/0051-ctrl-x-sends-a-command-to-the-other-panel.md)).

### Changed

- Questions before something that cannot be undone are no longer red as errors: deleting, a file
  that is there already, replacing or deleting a workspace, and quitting with jobs that would stop
  are on a background of their own, yellow in mc-classic, peach in Catppuccin, orange in the Noon
  themes. Errors stay red, and a panel on a host that is not connected now asks in one.
- Quick cd (`g d`) is now Go to directory, in its title, in F9 → Command, and in the help; its
  field is labelled Path.
- F5, F6, and Shift+F5 open one dialog, the same for a copy and a move but for its words: the
  dialog of F6 gets Preserve attributes too, shared with copies. Off, a move between the local
  file system and a host, or two hosts, leaves the attributes behind
  ([ADR 0043](docs/adr/0043-one-dialog-for-copy-and-move.md)).
- The dialog of `g +` and `g -` has a prompt, `Pattern:`, and its field ends with the glyphs of
  its case and its way, as every field that searches
  ([ADR 0044](docs/adr/0044-one-base-for-search-fields.md)). Besides shell patterns it takes
  plain text and regular expressions, and the case of the field replaces the Case sensitive check
  box: at first in `ui.default_search_mode`, or a shell pattern for `fzf`, and smart case, then as
  it was last ([ADR 0046](docs/adr/0046-case-and-the-keys-of-search-fields.md)).
- F7 opens with an empty name instead of the name under the cursor, which what was typed went
  after.
- In the fzf search mode, every list shows the best match first, as fzf does: the location menu,
  the tab menu, the Workspaces window, the command history, and the key hints, as the zoxide
  window did; matches as good keep their order. The location menu drops the headings of its
  groups while it ranks. The zoxide window turns its dots while it asks zoxide again
  ([ADR 0040](docs/adr/0040-one-picker-for-every-list.md)).
- A directory that cannot be read opens a dialog, `Cannot open dir: reason`, instead of a line
  under the panel, with ways out: OK stays, Retry reads it again, Up goes to the nearest parent
  that is there, and Go to opens Go to directory with the path. A directory gone under the panel
  has Up as its default ([ADR 0039](docs/adr/0039-errors-of-panels-in-dialogs.md)).
- A panel on a host that is no longer connected, after a lost connection or a disconnection,
  keeps its entries, cursor, and marks, toned down, instead of emptying: the cursor moves, quick
  search, sorting, and marks work on them. A key that needs the host asks, `Not connected to
  web`, with Connect, Local (the panel's last local directory), and Cancel; `R` still connects
  at once. The status line no longer says `web: Retry or Cancel?` while the Connection dialog
  asks, nor `Not connected to web; R connects`, and Cancel in the Connection dialog no longer
  leaves `Cannot open web: reason` behind.
- The status line no longer says `Rename: name` during a rename in the row; it shows the name
  and size of the entry as ever.
- The Workspaces window counts the tabs of each panel apart, the left panel's, then the right's,
  as `2 │ 1`, in place of `3 tabs`.
- The lists (the location menu, the tab menu, the Workspaces window, the command history, and the
  list of the key hints) open in Insert mode with their filter taking what is typed, as the
  zoxide window does: Esc goes to the rows, in Normal mode, where `j`, `k`, `d d`, and `q` work,
  `/` or `i` types in the filter again, and Esc closes the window
  ([ADR 0036](docs/adr/0036-lists-open-in-insert-mode.md)). The filter, and the keywords of the
  zoxide window, look like the text fields of dialogs: the colors of a field from the prompt to
  the right edge.
- Fewer lines in the lists: the line under the filter and the one over the whole row join the
  frame (`╟───╢`), as the line over the buttons of a dialog does, and the headings of the location
  menu are their names alone, in the color of titles, without a line.
- Messages with a count have plural forms for each: deleting, copying, moving, and checksums of
  several entries, the limit of the search of a tree, and the sizes in the question about a taken
  name; a translation gets the number itself, never `1` written out. The units of sizes, the marks
  of a time and a speed not known yet, and the percentage of a job's gauge are in the messages.
- F5, F6, F8, and `f #` on several entries say how many are files and how many directories, as
  `Copy 2 files and 1 directory to:`, `Move 3 files to:`, or `Delete 2 directories?`, rather than
  `2 files and directories`. Symlinks to directories count as directories, as on the panel's
  frame.
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
- Quick search no longer types a character after which no name matches, as in mc: the text and
  the cursor stay as they were. A character that leaves a regular expression unfinished, such as
  `(`, is still typed, so that the rest can follow
  ([ADR 0042](docs/adr/0042-quick-search-types-only-what-matches.md)).
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
- The z keys in the panels scroll the names alone: the icon, or mc's marker without icons, stays
  in front, with `«` after it, as on the frame; before, it scrolled out of sight with the name,
  and a rename in a scrolled row showed part of the name in its place.
- The zoxide window (`g z`) opens in Insert mode, so keywords can be typed at once, as in `zi`,
  without `/` first. Esc goes to the rows, and Esc there closes the window.
- `ui.default_search_mode` (`"plain"`, `"fzf"`, `"regex"`) replaces `ui.fuzzy_search`, which is
  now an unknown key: a `config.toml` that still has it does not load until it is removed or
  changed to `default_search_mode`.
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
- Dialogs are one width, 76 cells where the screen has room: those of F5, F6, and Shift+F5, F7,
  F4 on a host, checksums and saving them, changing directory, saving and renaming a workspace,
  and the window of a running job, were 60 or 70. Selecting by pattern (`g +`, `g -`) keeps mc's
  50, and find its 120.
- A place chosen in the location menu, or a tab in the tab menu, goes to its panel and leaves the
  keys with the active one, as Ctrl+w f and Ctrl+w v do, rather than move them to the panel it
  changed
  ([ADR 0049](docs/adr/0049-menus-of-a-panel-keep-the-active-panel.md)).
- `g l` opens the location menu of the active panel, in place of Ctrl+w 1 and Ctrl+w 2, which
  named a side, and `t t` its tab menu, as vim's `:tabs` lists the tabs, in place of `t h` and
  `t l`; Ctrl+x before them opens those of the other panel. After Ctrl+w x, no key opens the tabs
  of the panel drawn on the other side any more, as `t h` did
  ([ADR 0051](docs/adr/0051-ctrl-x-sends-a-command-to-the-other-panel.md)).
- The location menu (`g l`) and the tab menu (`t t`) open centered over both panels, as every
  other window does, rather than over their panel; the panel drawn active, after Ctrl+x the other
  one, is the one they change. Their titles, `Location` and `Tabs`, name no side, where they said
  `Left` or `Right`, and `Left tabs` or `Right tabs`. The tab menu grows with its widest location
  up to 76 cells, as the other lists, rather than to the width of its panel.
- F9 → Left and Right show the keys of their commands on both sides: as they are in the menu of
  the active panel, `g l` for Change location… and `t t` for Tab list…, and after Ctrl+x in the
  other's, as `Ctrl+x g l`; so do the sort orders, Rescan, New tab, and Close tab, which showed no
  key on the other side.
- The speed of a job that moves data follows the last five seconds of work rather than the
  average since the start, so a slow start no longer holds it down for the rest of the copy;
  while no data moves it falls to `0B/s`. The time left follows the pace of about the last half
  minute, so it no longer swings with the speed over an uneven link, and counts the entries left
  as well as the bytes, at what each has cost so far, so that many small files left no longer
  look done in no time, sent to a host as well as copied from one. It shows `-:--` once no report
  has come for longer than the job pauses as it works, five seconds at least, so a slow link or
  disk keeps a time left, and a stall does not slow the pace once work goes on; Skip all over
  many files is no stall
  ([ADR 0050](docs/adr/0050-time-left-of-a-job.md)). Both change once a second, with the time the
  job has worked, rather than with every frame, and the time left counts down a second each
  second while it keeps within 1% of what the pace says, evened out over up to half a minute;
  beyond that it follows it, halfway each second beyond a tenth, so that it does not stay off,
  and never goes up and down where the pace slows.
- A job in its window, or in the list of jobs, draws the screen only when what it shows changes,
  not for every chunk it reports, and its time draws once a second rather than every 150 ms. The
  bar of the gauge moves with the whole percent beside it.

### Removed

- Alt+F1 and Alt+F2, which desktops take: `g l` and Ctrl+x `g l` open the location menus. In the
  panels they now stand for Esc and F1 or F2, as other Alt keys that nothing binds do
  ([ADR 0049](docs/adr/0049-menus-of-a-panel-keep-the-active-panel.md)).
- Ctrl+w 1, Ctrl+w 2, `t h`, and `t l`: `g l` and `t t` open the menus of the active panel, and
  Ctrl+x before them those of the other
  ([ADR 0051](docs/adr/0051-ctrl-x-sends-a-command-to-the-other-panel.md)). Ctrl+w keeps the
  panels as vim's windows: Ctrl+w w, Ctrl+w p, Ctrl+w h, Ctrl+w l, Ctrl+w x, Ctrl+w r, Ctrl+w f,
  and Ctrl+w v.

### Fixed

- The error after OK in a host's settings with an other panel directory that is not local
  starts with a capital letter, as every other error does.
- A prompt whose translation put what is typed before the glyph of the search mode could
  panic, and the cursor of every prompt went after the whole message, wherever the typed text
  was in it.
- The viewer no longer lags in files with very long lines, such as binary files, while it
  wraps them: each line wraps once for each width, rather than again for every row it scrolls.
- The cursor of a field no longer flickers over cells that change while it waits, such as the
  skeleton that a shine crosses under `f f`: every frame is drawn as synchronized output, which
  the terminal shows whole. Terminals without it still may show the cursor on its way.
- A row of the viewer or a name in a panel no longer comes out a cell short, shifting the scroll
  bar or the frame, when it holds a mark that joins the next character, such as U+0602 in a
  binary file: such marks show as `?` in names and as `·` in the viewer.
- The viewer shows invalid UTF-8 and characters the terminal would act on as a faint `·`, as mc
  shows `.`, rather than as `?` and `�`, which stood out in a binary file and passed for text.
- A host whose start directory cannot be opened says which directory it is, rather than only
  the host's name; Up takes the panel to the nearest directory above it that is there, and Go to
  starts from its path, rather than from `host:`.
- A copy no longer goes over its own source, which without `transfer.atomic_upload` emptied it:
  through a hard link, or, where the file system ignores case or Unicode normalization, as
  macOS's do by default, under its name in another case (`Notes.txt` for `notes.txt`). That is an
  error now, locally and on a host. There a move to its own name in another case renames it, a
  directory too, rather than ask about a taken name or go into itself; and renaming in place to
  another entry's name in another case asks before replacing it, rather than replace it unasked.
- A copy no longer keeps a CPU core busy drawing the screen: its progress came hundreds of times
  a second, as each chunk was written, and every report drew a frame. Results of background work
  now draw at most 25 frames a second; keys still draw at once.
- A copy to a host no longer keeps a CPU core busy: every chunk it wrote was packed into the
  request one byte at a time. The chunks now go to the SFTP channel as they are.
- The speed, gauge, and time left of a copy to a host follow what the host has taken, rather than
  run up to 2 MiB ahead of the link at the start of each file and stand still while its end goes
  out; a file under 2 MiB goes out as it is written rather than all at once as it closes
  ([ADR 0052](docs/adr/0052-bytes-written-count-once-they-arrive.md)).
- A job cancelled while its host does not answer ends within two seconds, rather than wait as
  long as the host stays silent; a copy over a slow link stops waiting for its data at once, then
  takes away the file it did not finish.
- With Atomic copies on, a copy that fails names the file, rather than the hidden name it was
  written under.
- The Connection dialog of a lost or failed host no longer keeps the screen drawing 25 times a
  second for as long as it waits, as after a laptop slept: the panels that wait for the host show
  its marker of a failure in their titles, still, rather than red dots that turned.
- Waits that only turn dots, as a panel that reads a directory or connects, or the title of the
  viewer, draw half as many frames: one for each frame of the dots, 12 a second, rather than 25.
- A job in the background draws the screen only when the percentage in the corner changes, about
  once a second for a large copy, rather than up to 25 times a second for every chunk it moves.

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
