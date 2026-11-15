# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A setting of the viewer, `viewer.host_part_kib`, 256 by default, in Options → Configuration…
  under View and edit: how much of a file on a host F3 reads at once. A smaller part shows sooner
  on a slow connection.
- Go to directory (`g d`) leaves a host for a local directory with `:`: `:` alone goes back to
  the last local directory of the panel, `:path` starts there, and `:/…` and `:~/…` are local,
  with Tab completing them. A name that starts with a colon is `./:name`
  ([ADR 0065](docs/adr/0065-a-colon-without-a-host-is-local.md)).
- Alt+1 … Alt+9 show the tab of that number in the active panel, as `2g t` does, and Alt+0 the
  last tab; Ctrl+x before them, the other panel's. They work in Visual mode too.
- Settings of the log, `[log]`, in a new category of Options → Configuration…, Log: `level`,
  `filter` (directives such as `info,noc_ssh=trace` in place of the level), and `keep_days`, how
  many days a log file stays after it was last written to, 7 by default, or always with 0. They
  take effect at once.
- A log that cannot be written is said in a dialog at the start, and on the About page of the
  help.
- Ctrl+\\ views the log of this run from its end, in every mode, and Command → View the log; it
  opens in front of the menus, windows, and dialogs, which are there again once it closes. Without
  the kitty keyboard protocol Ctrl+\\ arrives as Ctrl+4, which works too.
- `o o` opens the Configuration dialog, as Options → Configuration… does, which shows the key.
- `e` edits as F4 does in two more places, as it edits a file in the panels: on the rows of the
  location menu, the settings of the host under the cursor, and in the search of a whole tree
  (`f /`, `f f`) outside its fields, the file of the row at the line, which `g f` views there,
  as F3 does.
- F3, and `g f` on the rows, in the location menu view the `ssh_config` file that defines the
  host under the cursor, from its `Host` line, in front of the menu, which is there again once
  the viewer closes. F3 works while the filter takes text too.
- `W` and F2 in the Workspaces window save the tabs of both panels in place of the workspace
  under the cursor, after a question, No by default, or Yes over the workspace restored or saved
  last; F7 adds a new workspace, as `a` does. F2 and F7 work while the filter takes text too
  ([ADR 0070](docs/adr/0070-workspaces-by-name-and-saved-in-place.md)).
- The viewer edits its file with `e` and F4, at the line on top, and reads it again once the
  editor changed it, a file on a host once the copy went back; `g x` opens it externally, and `R`
  reads it again, as in the panels. `] f` and `[ f` view the next file and the one before: of
  the panel, whose cursor goes along, of the files the search of a tree found, or of the
  directory of the log. `F` follows the end of the file as it grows, as `tail -f` and less's `F`
  ([ADR 0080](docs/adr/0080-the-viewer-edits-opens-and-follows-its-file.md)).
- F4 asks first, Cancel by default, before it edits a file that looks binary (a NUL in its first
  8,000 bytes, as git tells one), or one larger than `edit.large_mib`, 64 MiB by default; 0 never
  asks for the size. The setting is in Options → Configuration… under View and edit, which was
  Viewer.

### Changed

- F4 on a host edits a copy that keeps the file's own name, in a directory of its own that names
  the host, as `edit-4242-17-web1/nginx.conf`, where it was `edit-4242-17-nginx.conf`: the editor
  shows the name, and knows the kind of a file by its whole name too, as `Makefile` or `.bashrc`.
- `G` and End in the viewer go to the end of the file, not of the part read last, and at once:
  the viewer reads the last part of the file alone, on a host too, and the part under a scroll
  bar dragged far ahead. The lines there have no numbers until scrolling back reaches what was
  read before; until then the title says where the screen is in bytes, as `1.9G/2.0G 97%`. The
  log (Ctrl+\\) opens the same way
  ([ADR 0077](docs/adr/0077-the-viewer-jumps-to-the-end-of-a-file.md)).
- The viewer shows a large file sooner, and scrolls through it without stopping to read as
  long: it reads parts of 1 MiB where it read 16 (`viewer.part_mib`; a `config.toml` that sets it
  keeps its value). A line longer than a part stays whole, up to 16 MiB, and a local file of at
  most 16 MiB is read whole behind the screen once its start shows, so that its title still has
  the number of its lines ([ADR 0078](docs/adr/0078-the-viewer-reads-small-parts.md)).
- A file that grows while the viewer shows it, as a log that is being written, goes on past
  what was its end, where a new part had it read again from its start.
- The viewer reads a file on a host in parts of 256 KiB, so that it shows once that much has
  arrived, where it waited for 16 MiB. A part takes one round trip less, and the host is asked
  for no byte past it, where it sent up to 2 MiB more for nothing
  ([ADR 0078](docs/adr/0078-the-viewer-reads-small-parts.md)).
- The window of a job takes a count before an arrow, and Home and End, `g g`, and `G` go to
  its first button and its last, as in every dialog
  ([ADR 0073](docs/adr/0073-a-window-with-buttons-is-a-dialog.md)).
- The list of jobs (`f j`) takes the keys as every dialog with rows does: its rows are a stop of
  the focus, where the motions move the cursor, with a count, PageUp and PageDown a page, and
  Enter, Space, and a double click press Show; Tab, Left, and Right go to the buttons, where Up
  and Down move the focus. The wheel scrolls the rows. Right, Right, Enter aborts the job under
  the cursor, as Alt+a does.
- The tab menu (`t t`) and the zoxide window (`g z`) no longer repeat the row under the cursor in
  two lines under the rows, and are three lines shorter
  ([ADR 0071](docs/adr/0071-the-tab-menu-and-the-zoxide-window-show-no-whole-row.md)).
- The rows of the command history (`q :`) show the command alone, across the whole row. Under them
  two lines say where the command under the cursor ran: its host, or `local`, then its directory,
  with `~` for the home directory. A server, a monitor, and a folder go in front of them, or `≡`,
  `⌂`, and `›` without icons. The date and time it ran are on the line over them, at its right
  end, in the color of the title. The whole command no longer shows under the rows
  ([ADR 0072](docs/adr/0072-the-command-history-shows-where-a-command-ran.md)).
- Under the rows of the window of checksums of several files, the whole checksum of the file under
  the cursor is a block of groups of 8 digits, at most 4 to a line, as
  `ba7816bf 8f01cfea 414140de 5dae2223`. With icons, the file's icon goes in front of its name and
  a pound sign in front of its checksum, whose next lines are indented as far
  ([ADR 0087](docs/adr/0087-the-whole-checksum-under-the-list-is-in-blocks.md)).
- The location menu (`g l`) shows the size of the home directory's volume and of each volume
  after its free space, as `212G / 994G`, with the slashes of the rows in one column.
- The location menu shows its rows after at most 150 ms, where a network mount that hung kept it
  waiting 500 ms. The sizes of a volume that has not answered by then are a faint bar with a
  shine, which they replace once they come, in the menu that is open; after 5 s, or once the
  query fails after it hung, the bar gives way to `not responding`, in the colors of warnings,
  which an answer that comes later still replaces. A volume that tells no size says
  `size unknown`, faint, in place of `?`. A volume that hangs is never asked again until it
  answers, however often the menu opens or `R` lists the volumes again
  ([ADR 0074](docs/adr/0074-the-location-menu-shows-its-rows-before-the-volumes-answer.md),
  [ADR 0081](docs/adr/0081-words-for-sizes-not-known-and-cards-with-two-ends.md)).
- Under its rows the location menu shows a card of the place under the cursor in place of the
  whole row. Its first line says what the place is, and at the right end where it comes from. For
  a volume: the type of its file system and its kind, its mount point and what it mounts, as
  `/dev/disk4s1`, and its free space, size, and the share free; for the home directory the same of
  its volume, its path and the name of its volume, and the space of its volume; for a host: `sftp`,
  its address and `ProxyJump`, and the file and line of its `Host` line; then how it is,
  `Not connected`, `Connected`, `Connecting…`, or why its last attempt failed, in the colors of
  errors; for the row of the rest, the names it stands for, over two lines. Its first line starts
  with the icon of the row, a host's in the color of its status. The menu keeps its height
  ([ADR 0075](docs/adr/0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md),
  [ADR 0081](docs/adr/0081-words-for-sizes-not-known-and-cards-with-two-ends.md),
  [ADR 0082](docs/adr/0082-hosts-are-named-by-their-alias.md),
  [ADR 0083](docs/adr/0083-a-host-not-connected-says-so.md),
  [ADR 0084](docs/adr/0084-cards-show-a-place-and-where-it-comes-from.md)).
- Once the cursor of the location menu rests on a host for 150 ms, `ssh -G` runs for it, once a
  run, and its card shows the address and the `ProxyJump` it gives, a faint bar with a shine
  until then, or `ssh -G failed`, in the colors of errors, why going to the log. R in the menu
  lets it run again for hosts it failed for. One that has started runs to its end
  ([ADR 0076](docs/adr/0076-ssh-g-for-the-host-under-the-cursor-of-the-location-menu.md)).

- The Workspaces window and F9 → Workspace list the workspaces in the order of their names, as
  the panels order names, and the digits of F9 → Workspace follow that order.
- The columns of tabs in the Workspaces window no longer say Left and Right: a triangle marks the
  tab that shows on the side with the keys, and each row's count of that side's tabs is bold.
- The Workspaces window marks the workspace restored or saved last with a dot, as F9 → Workspace
  does, and opens with the cursor on it.
- `t W` asks before saving over the workspace restored or saved last too, as `W` in the window
  does, with Yes as the default; over another one, No stays the default.

- The key hints show a key that only starts one sequence as that sequence whole, with what it
  does, as `] t` and `Z Z, F10`, rather than `+1 key`.
- The list of the key hints (Ctrl+k twice) shows its rows under headings: the keys of the panels,
  the text fields, and the viewer in groups, such as Navigation, Two panels, Tabs, Marks and
  selection, Search, Files, Motions, and Changes, and the keys of the other parts under their
  names, such as Everywhere. The headings go while the filter ranks the rows best first. The help
  of F1 shows the same groups over the keys of the panels, the text fields, and the viewer. The
  pull-down menu (`g m`, F9) is in Other, and `g +`, `g -`, and `g *` in Marks and selection, so
  they come later in the help and in the key hints of which-key
  ([ADR 0086](docs/adr/0086-groups-of-keys-live-in-the-preset.md)).
- A window that waits for its rows, such as the location menu or the search of a tree, takes a
  fifth of the processor it took: a frame that only a spinner asks for puts back what the panels
  drew rather than draw them again, a shine at rest draws at the pace of the dots, and the times
  and sizes of rows are written once ([ADR 0069](docs/adr/0069-a-frame-for-time-alone.md)).
- Each run of the TUI writes a log of its own, `~/.local/state/noc/logs/noc-<time>-<pid>.log`,
  readable by the user alone, and kept for `log.keep_days` days after its last write; the
  subcommands no longer write one. A thread of its own writes it, and its lines say which host,
  job, search, or prompt they come from, and how long each took
  ([ADR 0067](docs/adr/0067-a-log-file-for-each-run.md)). `~/.local/state/noc/noc.log` is no
  longer written, and can be removed.

- Visual mode stays with its tab: going to the other panel (Ctrl+w, Tab, a click) or to another
  tab keeps the rows selected, which show there and are selected again once the panel or the tab
  has the keys. Ctrl+x runs its command on them
  ([ADR 0066](docs/adr/0066-visual-mode-stays-with-its-tab.md)).
- A panel that reads another directory, or connects to a host to open one, no longer shows the
  rows it leaves: once the dots turn in its title, a skeleton of three rows stands in their place
  and in place of the status line, and the frame shows no totals. Meanwhile the rows take no keys
  or clicks, sorting and the command line aside; a listing that fails shows them again, with the
  cursor and the marks ([ADR 0068](docs/adr/0068-a-panel-hides-the-rows-it-leaves.md)).

- F10 and `Z Z` always ask before quitting: Yes is the default, or No while jobs run.
- A notice of ssh, such as a request to touch a security key or to use Touch ID, turns the dots
  in front of its title while ssh waits, and has Cancel, which stops connecting, in place of OK,
  which hid the dialog alone.
- Error dialogs say what failed, then the path on a line of its own, then, under a line, why,
  and always the code of the system, such as `Code: 13`, or `Code: —` where there is none
  ([ADR 0063](docs/adr/0063-errors-in-parts-and-the-order-of-buttons.md)).
- A failed copy, move, delete, or checksum names the step that failed, such as `Cannot read` or
  `Cannot create`, and asks as the question about a taken name does: Retry (the default), Skip,
  and Abort for this failure, and The rest for the later ones, Ask each time, Skip every failure,
  or Skip failures with the same code. Ignore is now Skip. What would change nothing does not
  show: Retry where the target is the source itself, The rest at the last entry, Skip for the only
  one.
- The question about a taken name has no The rest at the last entry, and no Skip for the only
  one.
- Renaming or saving onto a file asks the question of a copy about a taken name: both files with
  their sizes and times, Overwrite, Rename… to a free name, and Cancel, the default.
- The dialog after a directory could not be read has Retry, Up, Go to, and Cancel in place of OK;
  Retry is the default, or Up where the panel has nothing to show.
- Replacing a workspace by Save as has No as the default.
- A shell of `!` or `:` that cannot run, and an edited remote file whose changes did not go back,
  say so in parts too, with the code of the system and where the edited copy stays.
- Most errors offer a way on rather than OK: Retry does it again (F7, save, F3, F4, an edited
  copy that did not go back, `!` and `:`, the settings, workspaces, and history files); Connect
  connects a host that is not connected and then does what could not start; Change… opens
  Shift+F6, F5, F6, or F7 again with what was typed, after a rename that failed, a copy onto its
  source, or a directory name that is taken.

- A copy, move, delete, or checksum on a host whose connection is lost waits for it, rather than
  ending: Retry in Connection connects again, and the job goes on where it was, copying again
  only the file it was cut off in; Cancel ends it
  ([ADR 0064](docs/adr/0064-jobs-wait-for-a-lost-host.md)).
- The question of ssh about a host key shows in the colors of warnings. A yes/no question from
  a server shows as a question of its own, in the colors of dialogs, whose No answers `no`; it can
  no longer pass for one about a host key ([ADR 0003](docs/adr/0003-askpass-bridge.md)).
- Every dialog and list is 76 cells wide where the screen has room: the dialogs of `g +` and
  `g -`, Find file… (`f /`), the location menu (`g l`), the zoxide window (`g z`), the
  workspaces, the tab menu (`t t`), which no longer grows with its locations, and the window
  over the viewer while it reads.
- What a list says of the rows it does not show, `24 more…`, is faint, apart from the rows: the
  last row of a group of the location menu, but under the cursor, the last line of a column of
  tabs in the workspaces, and the last cell of the key hints, which no longer says `… 24 more`.
  A chevron down goes in front of it, in the column of the icons of the location menu.
- The location menu lists the hosts not used lately by name, after those used last, where it
  listed them in the order of the ssh config. Names compare as in the panels, with the numbers in
  them by value while `ui.natural_sort` is on
  ([ADR 0085](docs/adr/0085-the-location-menu-lists-hosts-by-name.md)).
- The fields of the host settings (F4 on a host) have short labels: Remote directory, Other panel
  directory, and Shell, whose explanations show faint in the empty fields. Run with in Options →
  Configuration… under Command line is Shell too.

### Removed

- The labels of hosts: `label` in `hosts.toml` and its field in the dialog of F4. A host is named
  by its alias everywhere: in the location menu, the prompt of the command line, the title of the
  terminal, the command history, and `noc debug hosts`. A `label` left in `hosts.toml` is an
  error; delete the line ([ADR 0082](docs/adr/0082-hosts-are-named-by-their-alias.md)).
- The cache of `ssh -G` results, `~/.cache/noc/resolve.json`, which nothing reads any more and
  which may be deleted: an address shows as `ssh -G` says it in this run. The rows of hosts in the
  location menu no longer show addresses, which their cards do, and the filter finds a host by its
  address once `ssh -G` has told it. `noc debug hosts` runs `ssh -G` for every host each time, and
  `--resolve` is gone ([ADR 0079](docs/adr/0079-ssh-g-results-are-not-cached.md)).
- `NOC_LOG`: `log.level` and `log.filter` take its place.
- Stop and Close in the window over the viewer while it reads a part that takes a while: Cancel
  alone takes their place, the default, which stops the reading and keeps what was read, as Esc
  did; Esc in the viewer then closes it
  ([ADR 0073](docs/adr/0073-a-window-with-buttons-is-a-dialog.md)).

### Fixed

- Why `R` in the location menu could not list the places again, right of the filter, was crossed
  out, as what is typed wrong is. It shows in the colors of errors, as everything that failed in
  the menu, and nothing there is crossed out
  ([ADR 0081](docs/adr/0081-words-for-sizes-not-known-and-cards-with-two-ends.md)).
- The viewer read the same part again and again, for ever, when scrolling up reached a part it
  had let go while it held as much as `viewer.window_mib` allows.
- A copy or a move that could not read its source said `Cannot copy to` the source.
- Disconnecting a host while a job ran on it did not end the job: a delete never ended, and a
  copy failed file by file. The job ends now.
- A copy retried after a failure that also kept it from removing what it wrote asked whether to
  replace that file, or left its temporary file behind.
- When ssh asked again about a host key on the terminal, after an answer that was neither yes nor
  no, what was typed did not show.
- Dialogs dropped the spaces a line of their text starts with, such as the indent of the other
  names of a host key in ssh's question; such a line and the lines it breaks into keep them.
- Cancel or Esc on a password, a code, or a server's question did not stop connecting: ssh sent
  the server an empty answer and asked again. It stops connecting now, and sends nothing
  ([ADR 0003](docs/adr/0003-askpass-bridge.md)).
- A lost connection could give as its reason something ssh wrote while it connected, such as the
  last line of the picture of `VisualHostKey`. The reason is now what ssh wrote after it
  connected, else the signal that ended ssh, such as when it was killed, else that the
  connection is closed.
- The viewer froze on a file of lines of megabytes, such as one of zero bytes: each frame
  counted the cells of every line on screen from its start, wrapped or cut, wrapped its lines
  twice when it held the whole file, and wrapped them all again for each part it read. A line
  is counted and wrapped once now.
- What a list says in place of its rows took one line, and the rest of it did not show, such as
  the end of why the zoxide window lists nothing, and of how to save a workspace when none is
  saved. It takes the lines it wraps into now.

## [0.3.1] - 2026-11-01

### Changed

- The question about a taken name in a copy or a move views both files, chooses for the later
  taken names, and Rename… copies under a free name, such as `report_2.txt`, or one typed
  ([ADR 0060](docs/adr/0060-the-question-about-a-taken-name.md)).
- The default button of a dialog is drawn bold in braces, `{ OK }`, rather than as `[< OK >]`.
- The window of checksums is a dialog: Tab takes the focus from its rows to its buttons, and a
  double click on a row copies its checksum, as Enter does
  ([ADR 0061](docs/adr/0061-a-picker-is-a-part-of-a-dialog.md)).
- Space on a row of the search of a tree goes to it, as Enter does.
- On a narrow screen every window leaves two cells free on either side, as dialogs did.
- Marked rows keep the colors of their names and are no longer underlined: a bar `▌` in the
  panel's frame shows the mark, and the Noon and Catppuccin themes tint the row.

### Removed

- The `forwarding` build feature: no build forwards ports, the agent, X11, or tunnels
  ([ADR 0062](docs/adr/0062-forwarding-is-never-allowed.md)). Drop `--features forwarding`
  from builds; `noc debug version` and the About page have no `features` line.

### Fixed

- A panel, a list, or the viewer made taller, or the viewer wider, no longer shows empty rows
  after the end while rows above it hide.
- In error dialogs and warnings of the Noon and Catppuccin themes the letters for Alt have a
  color, as in other dialogs, rather than only an underline.
- A move between two local disks asked twice about a taken name.

## [0.3.0] - 2026-10-31

### Added

- `f /` searches the text of the files below the panel's directory with
  [ripgrep](https://github.com/BurntSushi/ripgrep), and `f f` their names with
  [fd](https://github.com/sharkdp/fd), locally or on a host; F3 views a result and F4 edits it at
  its line ([ADR 0027](docs/adr/0027-search-of-a-whole-tree.md)).
- Search modes: quick search and the filters of the lists match as fzf does, as plain text, or as
  a regular expression, and `g +` and `g -` take the last two besides shell patterns, in smart
  case at first. Ctrl+s and a key switch them, as `Ctrl+s s` and `Ctrl+s c`
  ([ADR 0030](docs/adr/0030-search-modes.md)). Quick search highlights what matches.
- Ctrl+x runs the next command in the other panel, then gives the keys back: Ctrl+x `g l` opens
  its location menu, Ctrl+x F5 copies from it
  ([ADR 0051](docs/adr/0051-ctrl-x-sends-a-command-to-the-other-panel.md)).
- `g x` opens the marked entries, or the one under the cursor, with the desktop's program, as
  vim's `gx` ([ADR 0026](docs/adr/0026-open-externally.md)).
- Shift+F5 or `y p` copies the entry under the cursor beside it, as mc's Shift+F5.
- The command line runs commands with `shell.run`, `["$SHELL", "-i", "-c"]` by default, so the
  shell's aliases and functions work; a host may have its own `run` (F4 on it in the location
  menu). Local commands and the editor run as jobs: Ctrl+c and Ctrl+z reach them alone
  ([ADR 0054](docs/adr/0054-shell-run.md)).
- The viewer reads a file in parts, so it shows all of a large file rather than its first 16 MiB
  ([ADR 0038](docs/adr/0038-viewer-reads-a-window-of-the-file.md)).
- Dates, times, and numbers follow the formats of the region (`ui.formats`); a time always shows
  its year, and numbers are grouped ([ADR 0035](docs/adr/0035-formats-of-the-region.md)).
- The status line shows the target of a symlink and the size of a file in bytes; the panel's
  frame totals what it shows, as `12M in 45 files, 3 dirs`.
- The command line's prompt is a shell's, as `deploy@web:/var/www $`, and root shows red
  ([ADR 0031](docs/adr/0031-who-and-where-on-screen.md)).
- Letters in every dialog, as in Far and mc: Alt and the letter of a label acts on it, even from a
  text field ([ADR 0033](docs/adr/0033-letters-in-dialogs.md)).
- The help (F1) has sections: About, with the facts of `noc debug version` and where the files
  are kept, and Keys, grouped by part of the app.
- Ctrl+k while the key hints show lists every key with what it does, to filter and run.
- `K` shows the whole name under the cursor, and the z keys scroll names and the viewer's lines
  sideways, as vim's (`z h`, `z l`, `z e`). The lists always show their row under the cursor
  whole ([ADR 0041](docs/adr/0041-lists-always-show-the-whole-row.md)).
- Scroll bars on the frames wherever rows do not fit, which take clicks and drags
  ([ADR 0024](docs/adr/0024-scroll-bars-on-the-frames.md)).
- Windows grow as they open and shrink as they close
  ([ADR 0034](docs/adr/0034-animated-windows.md)).
- The window of checksums has a filter (`/`), and F3 shows them in the viewer
  ([ADR 0058](docs/adr/0058-the-window-of-checksums-filters-and-views-them.md)).
- `noc debug version` prints the facts for a bug report, one `key value` line each.
- Settings, all in Options → Configuration: `ui.formats`, `ui.natural_sort`,
  `ui.default_search_mode`, `animations.windows`, `animations.spinners`, `shell.run`,
  `search.program`, `search.fd`, `search.max_results`, `search.max_per_file`, `viewer.part_mib`,
  and `viewer.window_mib`.

### Changed

- Keys; the help lists them all:
  - `g l` opens the location menu and `t t` the tab menu of the active panel, which keeps the keys
    ([ADR 0049](docs/adr/0049-menus-of-a-panel-keep-the-active-panel.md)).
  - `i` opens the command line, as `!` does; the viewer moved to `g f`, and F3 stays.
  - Tab goes to the other panel, as in mc, where the terminal speaks the kitty keyboard protocol.
  - Shift+Down and Shift+Up mark, as in mc and Far; `J` and `K` no longer do.
  - In dialogs, Alt and a key no longer stand for Esc and the key.
- The lists, the zoxide window too, open typing in their filter; Esc goes to the rows
  ([ADR 0036](docs/adr/0036-lists-open-in-insert-mode.md)). In fzf mode, the best match is first.
- Quick search types no character that no name matches
  ([ADR 0042](docs/adr/0042-quick-search-types-only-what-matches.md)), and Backspace with nothing
  typed no longer closes it. Names match in either Unicode form and by full case folding, so
  `strasse` finds `Straße`.
- Paging stops with the end at the bottom, in the panels, the lists, the help, and the viewer
  ([ADR 0023](docs/adr/0023-paging-stops-at-the-end.md)).
- Text cut short ends in `»` rather than `~`, and paths lose their start rather than their middle
  ([ADR 0025](docs/adr/0025-marks-of-text-cut-short.md)).
- The line of the mode always says the mode and what has the keys, as `NORMAL · List`
  ([ADR 0032](docs/adr/0032-line-of-the-mode.md)).
- Everything that waits shows one spinner, in the title, and a list a skeleton of its rows
  ([ADR 0037](docs/adr/0037-spinners.md)).
- A directory that cannot be read opens a dialog with Retry, Up, and Go to
  ([ADR 0039](docs/adr/0039-errors-of-panels-in-dialogs.md)). A panel whose host is no longer
  connected keeps its entries, toned down.
- Questions before what cannot be undone are a warning, not red as errors
  ([ADR 0048](docs/adr/0048-errors-and-warnings-in-dialogs.md)).
- Combo boxes replace radio buttons and the `< … >` choices
  ([ADR 0057](docs/adr/0057-combo-boxes.md)); check boxes show `[✓]`.
- F5 and F6 open one dialog, so a move can keep attributes too
  ([ADR 0043](docs/adr/0043-one-dialog-for-copy-and-move.md)); F7 opens with an empty name.
- F5, F6, F8, and `f #` count files and directories apart, as `Copy 2 files and 1 directory to:`.
- Quick cd (`g d`) is now Go to directory.
- The window of checksums has To clipboard and All to clipboard in place of Copy and Copy all,
  and its Save… takes a path, which Tab completes
  ([ADR 0059](docs/adr/0059-one-dialog-saves-a-file.md)).
- Sizes have a unit, `512B`, `78.5K`, `503G`, bolder as the size grows.
- Panels sort names as the language does, numbers by value (`file2` before `file10`); turn
  `ui.natural_sort` off to sort as before.
- The Workspaces window counts the tabs of each panel apart and shows where they are.
- A job's speed and time left follow its recent pace, bytes and entries alike
  ([ADR 0050](docs/adr/0050-time-left-of-a-job.md)).
- The wheel moves the cursor where the rows cannot scroll, as in mc and Far.
- `noc -V` and `noc --version` print only the version, and `noc hosts` and `noc ls` are now
  `noc debug hosts` and `noc debug ls`.

### Removed

- Alt+F1, Alt+F2, Ctrl+w 1, Ctrl+w 2, `t h`, and `t l`: use `g l` and `t t`, with Ctrl+x before
  them for the other panel.
- `ui.fuzzy_search`: a `config.toml` that still has it does not load. Use
  `ui.default_search_mode` (`"fzf"`, `"plain"`, or `"regex"`).

### Fixed

- A copy no longer goes over its own source through a hard link, or under its name in another
  case where the file system ignores case, which emptied it. With Atomic copies on, a copy that
  fails names the file rather than its hidden temporary name.
- Copies to a host no longer keep a CPU core busy, and their progress follows what the host has
  taken ([ADR 0052](docs/adr/0052-bytes-written-count-once-they-arrive.md)).
- A cancelled job ends within two seconds when its host does not answer.
- Jobs take far less CPU to show their progress
  ([ADR 0055](docs/adr/0055-a-job-s-reports-go-in-batches.md)), and no longer fill memory while
  another program has the terminal.
- A copy gives a directory its times and permissions once all in it is copied, not at the end,
  so an aborted copy keeps them
  ([ADR 0053](docs/adr/0053-a-directory-is-finished-once-all-in-it-is-done.md)).
- A job's total follows files that grow or shrink, so its gauge ends at 100%.
- Noon Commander ends when its terminal goes away, rather than stay behind at full CPU, and no
  longer aborts on its way out.
- Commands on a host whose login shell is fish no longer fail on a `\` before a `'` or at
  their end.
- The viewer no longer lags on very long lines, and shows what cannot be shown as a faint `·`.
- The cursor of a field no longer flickers while something on screen moves.
- An expected checksum that is none no longer closes the dialog of `f #`, and an error about a
  dialog's fields shows over it rather than in its place.

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
