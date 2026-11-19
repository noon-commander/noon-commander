# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Releases have Linux binaries for x86-64 and 64-bit ARM, linked statically with musl, so that
  they run on any distribution
  ([ADR 0089](docs/adr/0089-static-linux-binaries-with-musl.md)).
- Packages for Arch Linux in the AUR: `noon-commander` builds a release from its sources,
  `noon-commander-bin` installs its binary, and `noon-commander-git` builds the latest commit
  ([ADR 0090](docs/adr/0090-aur-packages.md)).

### Changed

- The F9 pull-down menu is reorganized into `File`, `View`, `Go`, `Search`, `Tools`, `Workspace`,
  and `Help`, without `Left` and `Right`: every command acts on the active panel (Ctrl+x F9 acts on
  the other), Go adds Jump back and Jump forward, Tools and Help add Command history…, Command
  output, and Key hints; a command ends in `…` when it asks for something before it acts
  ([ADR 0091](docs/adr/0091-pull-down-menu-without-left-and-right.md)).

### Removed

- F9 → Workspace no longer lists the saved workspaces: Workspace list… (`t w`) opens their window,
  which restores them by name
  ([ADR 0091](docs/adr/0091-pull-down-menu-without-left-and-right.md)).

## [0.3.2] - 2026-11-16

### Added

- The viewer edits its file with `e` and F4, at the line on top, and reads it again once it
  changed; `g x` opens it externally, `R` reads it again, `] f` and `[ f` view the next file and
  the one before, and `F` follows the end of the file as it grows, as `tail -f` does
  ([ADR 0080](docs/adr/0080-the-viewer-edits-opens-and-follows-its-file.md)).
- Ctrl+\\ views the log of this run from its end, in every mode, as F9 → Command → View the log
  does; without the kitty keyboard protocol it arrives as Ctrl+4, which works too.
- In the location menu (`g l`), F3 and `g f` view the `ssh_config` file that defines the host
  under the cursor, at its `Host` line, and `e` edits the host's settings, as F4 does.
- In the search of a tree (`f /`, `f f`), `e` edits the file of the row at its line, as F4
  does, and `g f` views it, as F3 does.
- In the Workspaces window (`t w`), `W` and F2 save the tabs of both panels in place of the
  workspace under the cursor, and F7 adds one, as `a` does
  ([ADR 0070](docs/adr/0070-workspaces-by-name-and-saved-in-place.md)).
- Alt+1 … Alt+9 show the tab of that number in the active panel, and Alt+0 the last one, in
  Visual mode too.
- `o o` opens the Configuration dialog.
- Go to directory (`g d`) leaves a host for a local directory with `:`: `:` alone goes back to
  the panel's last local directory, and `:/…` and `:~/…` are local paths
  ([ADR 0065](docs/adr/0065-a-colon-without-a-host-is-local.md)).
- F4 asks first, Cancel by default, before it edits a file that looks binary or is larger than
  `edit.large_mib`.
- A dialog at the start, and the About page of the help, say when the log cannot be written.
- Settings, all in Options → Configuration…: `viewer.host_part_kib`, how much of a file on a host
  F3 reads at once (256 KiB); `edit.large_mib`, the size above which F4 asks (64 MiB; 0 never
  asks); and, in a new Log category, `log.level`, `log.filter` (such as `info,noc_ssh=trace`),
  and `log.keep_days`, how long a log file stays after its last write (7 days; 0 keeps all).

### Changed

- Each run of the TUI writes a log of its own in `~/.local/state/noc/logs/`, readable by the user
  alone, whose lines name the host, job, or search they come from; the subcommands write none.
  `~/.local/state/noc/noc.log` is no longer written and can be removed
  ([ADR 0067](docs/adr/0067-a-log-file-for-each-run.md)).
- Errors say what failed, then the path, then why, with the code of the system, and offer a way
  on rather than OK: Retry, Connect for a host that is not connected, or Change… to edit what was
  typed. Dialogs show only the buttons that act
  ([ADR 0063](docs/adr/0063-errors-in-parts-and-the-order-of-buttons.md)).
- A failed copy, move, delete, or checksum names the step that failed and asks as the question
  about a taken name does: Retry, Skip, Abort, and what to do with the later failures. Ignore is
  now Skip. Renaming or saving onto a file asks the question about a taken name too.
- A copy, move, delete, or checksum on a host whose connection is lost waits for it: Retry
  connects again, and the job goes on where it was
  ([ADR 0064](docs/adr/0064-jobs-wait-for-a-lost-host.md)).
- F10 and `Z Z` always ask before quitting: Yes by default, or No while jobs run.
- `t W` asks before saving over any workspace: Yes by default over the one restored or saved
  last, which it replaced without asking, and No over another, where Yes was the default.
- The Workspaces window and F9 → Workspace list the workspaces by name. The window marks the one
  restored or saved last and opens on it; its columns of tabs mark the side with the keys rather
  than say Left and Right.
- Visual mode stays with its tab: going to the other panel or to another tab keeps the rows
  selected ([ADR 0066](docs/adr/0066-visual-mode-stays-with-its-tab.md)).
- A panel that reads another directory, or connects to a host, shows a skeleton in place of the
  rows it leaves ([ADR 0068](docs/adr/0068-a-panel-hides-the-rows-it-leaves.md)).
- The viewer:
  - `G` and End go to the end of the file at once, on a host too, and the log opens there
    ([ADR 0077](docs/adr/0077-the-viewer-jumps-to-the-end-of-a-file.md)).
  - It reads parts of 1 MiB where it read 16 (`viewer.part_mib`; a value set in `config.toml`
    stays), and of 256 KiB on a host, so that a file shows sooner
    ([ADR 0078](docs/adr/0078-the-viewer-reads-small-parts.md)).
  - A file that grows while it shows goes on past what was its end, rather than being read again.
- The location menu (`g l`):
  - It shows its rows within 150 ms, before the volumes answer; a volume that hangs says
    `not responding` and is not asked again until it answers
    ([ADR 0074](docs/adr/0074-the-location-menu-shows-its-rows-before-the-volumes-answer.md)).
  - It shows the size of each volume after its free space, as `212G / 994G`.
  - Under the rows, a card of the place under the cursor takes the place of the whole row; a
    host's gives its address and `ProxyJump`, from `ssh -G` once the cursor rests on it, and
    whether it is connected
    ([ADR 0075](docs/adr/0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md),
    [ADR 0076](docs/adr/0076-ssh-g-for-the-host-under-the-cursor-of-the-location-menu.md)).
  - It lists the hosts not used lately by name, not in the order of the ssh config
    ([ADR 0085](docs/adr/0085-the-location-menu-lists-hosts-by-name.md)).
- The help (F1), the list of the key hints (Ctrl+k twice), the Configuration dialog, and the
  search of a tree take 80% of the screen, at least 76 cells
  ([ADR 0088](docs/adr/0088-large-windows.md)). Every other dialog and list is 76 cells wide
  where the screen has room, such as the dialogs of `g +` and `g -` and the tab menu (`t t`).
- The help and the list of the key hints show the keys in groups, such as Navigation, Tabs, and
  Marks and selection ([ADR 0086](docs/adr/0086-groups-of-keys-live-in-the-preset.md)). The list
  says in full what the command under the cursor does, and the key hints show a key that starts
  one sequence as that sequence, as `] t`, rather than `+1 key`.
- The window of a job and the list of jobs (`f j`) take the keys of every dialog, such as a
  count, `g g`, and `G`; the list moves its cursor with the motions and goes to its buttons with
  Tab ([ADR 0073](docs/adr/0073-a-window-with-buttons-is-a-dialog.md)).
- The command history (`q :`) shows only the command on each row, and under the rows where and
  when the command under the cursor ran
  ([ADR 0072](docs/adr/0072-the-command-history-shows-where-a-command-ran.md)). The tab menu
  and the zoxide window (`g z`) no longer repeat the row under the cursor
  ([ADR 0071](docs/adr/0071-the-tab-menu-and-the-zoxide-window-show-no-whole-row.md)).
- The window of checksums of several files shows the whole checksum of the file under the cursor
  in groups of 8 digits
  ([ADR 0087](docs/adr/0087-the-whole-checksum-under-the-list-is-in-blocks.md)).
- F4 on a host edits a copy that keeps the file's name, so that the editor knows its kind, as
  `Makefile` or `.bashrc`.
- ssh: a question about a host key shows in the colors of warnings, a server's yes/no question as
  one of its own, and a notice such as a request to touch a security key has Cancel, which stops
  connecting ([ADR 0003](docs/adr/0003-askpass-bridge.md)).
- What a list says of the rows it does not show, as `24 more…`, is faint.
- A window that waits for its rows, such as the location menu, takes a fifth of the processor it
  took ([ADR 0069](docs/adr/0069-a-frame-for-time-alone.md)).
- In Options → Configuration…, Viewer is now View and edit, and Command line → Run with is Shell.
  The fields of the host settings (F4 on a host) have shorter labels.

### Removed

- Host labels: a host is named by its alias everywhere. A `label` left in `hosts.toml` is an
  error; delete the line ([ADR 0082](docs/adr/0082-hosts-are-named-by-their-alias.md)).
- The cache of `ssh -G` results, `~/.cache/noc/resolve.json`, which can be deleted. The rows of
  hosts in the location menu no longer show addresses, which their cards do, and
  `noc debug hosts` has no `--resolve` any more
  ([ADR 0079](docs/adr/0079-ssh-g-results-are-not-cached.md)).
- `NOC_LOG`: set `log.level` or `log.filter` instead.
- Stop and Close in the window over the viewer while it reads: Cancel takes their place
  ([ADR 0073](docs/adr/0073-a-window-with-buttons-is-a-dialog.md)).

### Fixed

- The viewer froze on a file of very long lines, such as one of zero bytes, and could read the
  same part again and again when scrolling back up.
- A failed reload (`R`) of the location menu was crossed out, as text typed wrong is; it shows in
  the colors of errors.
- A copy or a move that could not read its source said `Cannot copy to` the source.
- Disconnecting a host while a job ran on it did not end the job.
- A copy retried after a failure asked whether to replace its own partial file, or left its
  temporary file behind.
- Cancel or Esc on a password, a code, or a server's question did not stop connecting: ssh sent
  the server an empty answer and asked again.
- A lost connection could give as its reason something ssh wrote while it connected.
- ssh's repeated question about a host key on the terminal did not show what was typed.
- Dialogs dropped the indent of their lines, such as that of the other names of a host key.
- What a list says in place of its rows, such as why the zoxide window is empty, showed only
  its first line.

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
