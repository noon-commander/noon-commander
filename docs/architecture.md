# Architecture

Noon Commander (`noc`) is a Rust-based, two-panel terminal file manager for macOS and Linux,
focused on seamless local and SFTP file operations. SFTP is its foundation and the reason it
exists. It never implements SSH: every connection is a process of the system OpenSSH client, and
Noon Commander speaks the SFTP protocol over that process's stdin and stdout.

This document describes the planned design; see the [roadmap](roadmap.md) for what exists. Key
decisions are recorded as [ADRs](adr/).

## Crates

```text
crates/
├── noc/           bin + UI: CLI, bootstrap, askpass entry point, ratatui app,
│                  keymap, themes, icons, i18n
├── noc-config/    XDG paths, TOML schema, defaults
├── noc-locale/    formats of the region: numbers, dates, times, the order of names
├── noc-ssh/       host discovery, ssh -G, argument validation, forwarding policy,
│                  ControlMaster, SFTP channels, askpass bridge
├── noc-vfs/       Vfs trait: local and SFTP backends, mounted volumes
├── noc-ops/       job engine: copy, move, rename, delete, mkdir, checksums; progress, cancellation, conflicts
├── noc-tools/     external programs other than ssh: zoxide, ripgrep, fd, the editor, the opener, the shell
├── noc-text/      terminal-safe text: sanitizing, widths in cells, fitting, wrapping
├── noc-pattern/   patterns typed to match names: regular expressions or text, smart case,
│                  one Unicode form, case folding
├── noc-scroll/    paging text that ends: how far the page keys go, the scroll bar
├── noc-vars/      variables in text: $NAME, ${NAME}, $$, read once, expanded where it runs
└── noc-viewer/    the F3 viewer: reading through the Vfs, scrolling, wrapping; its own text
```

Dependencies point one way, toward the binary:

```text
noc-text  noc-vfs  noc-scroll  noc-pattern  noc-locale  noc-vars
            no other noc-* crate
noc-config  → noc-vars
noc-ssh     → noc-vars
noc-tools   → noc-pattern, noc-vars
noc-ops     → noc-vfs
noc-viewer  → noc-vfs, noc-text, noc-scroll, noc-locale
noc         → all of them
```

Library crates contain no UI code and no user-facing text; they return typed errors and events,
and the UI turns them into messages. `noc-viewer` is the exception
([ADR 0015](adr/0015-viewer-and-text-crates.md)): a piece of the UI, with Fluent text of its own,
that knows nothing of the keymap, the theme, or the app. The app maps its keys to the viewer's
commands, gives it the styles of its theme, reads the parts it asks for (`Viewer::want`) with
`noc_viewer::read_part` in a task of its own, which also makes their lines away from the thread
that draws, gives them back (`Viewer::take`), and closes the viewer itself. `noc-scroll` draws
too, the scroll bar, but has no text ([ADR 0023](adr/0023-paging-stops-at-the-end.md)). Code
that two crates need goes into a crate of its own topic, such as `noc-text`, or `noc-pattern`,
which builds the regular expressions of fd's search in `noc-tools` and of the regex mode in the
app, or `noc-vars`, which reads the variables of `shell.run` for `noc-config`, expands them for
`noc-tools`, and gives their pieces to `noc-ssh` to quote, rather than into a common `core`
crate.

ssh runs only from `noc-ssh`, and every other program only from `noc-tools`
([ADR 0012](adr/0012-external-tools-and-zoxide.md)): without a shell, with `--` before paths,
without input for background ones, and killed when their future is dropped or they time out.
A program that is not installed is told apart from one that fails. The shell of the command
line, `shell.run`, is the one program started through a shell, since the command is the user's
([ADR 0019](adr/0019-shell-command-line.md), [ADR 0054](adr/0054-shell-run.md)). The desktop's
opener of `g x` is the one program left running: it starts programs that outlive it and Noon
Commander, so its output goes nowhere, it is never killed, and it takes absolute paths without
`--`, which `xdg-open` refuses ([ADR 0026](adr/0026-open-externally.md)).

## Processes

Each connected host has one master connection; everything else is multiplexed over its control
socket ([ADR 0002](adr/0002-controlmaster-per-host.md)):

```text
ssh <master options> -M -N -S <sock> -o ControlPersist=no -- <alias>  # authenticates once
ssh <channel options> -S <sock> -T -s -- <alias> sftp                 # panel channel
ssh <channel options> -S <sock> -T -s -- <alias> sftp                 # transfer channels
ssh <command options> -S <sock> -- <alias> 'cd -- <dir> || exit⏎exec "$SHELL" -i -c <cmd>'
                                                                      # command line
ssh -S <sock> -t -- <alias> 'cd <dir> && exec $SHELL -l'              # console (backlog)
ssh -F /dev/null -S <sock> -O exit -- noc                             # disconnect
```

The SFTP protocol client is `openssh-sftp-client`, whose `Sftp::new` works over the pipes of any
child process. Every ssh child runs in its own session (`setsid`), without a controlling
terminal, so it can neither read from nor draw on the TUI's terminal; the one exception is a
command of the [command line](#command-line-of--and-), which gets the terminal with the panels
hidden. ssh runs in the home
directory (`SshSettings::work_dir`), not in the working directory of the process, which
follows the active panel: a long-lived master or channel would hold that directory, and its
volume could not be unmounted.

`noc-ssh` API in short: `version::check_version` runs `ssh -V`; `resolve::resolve` runs
`ssh -G`; `Session::connect` starts the master (or nothing, without multiplexing);
`Session::open_sftp` returns the pipes of a new SFTP channel, which `SftpFs::from_pipes` in
`noc-vfs` turns into a file system; `Session::close` shuts down; `cleanup_stale` removes
leftovers of crashed instances.

The `Vfs` trait of `noc-vfs`, implemented by `LocalFs` and `SftpFs`, lists directories, reads
metadata with and without following symlinks (locally with the device and inode numbers, which tell
that two paths lead to one object; SFTP v3 has none), tells the size and free space of the file
system that holds a path (`None` from an SFTP server without `statvfs@openssh.com`), canonicalizes
paths, creates and removes directories, removes files, renames, reads and makes symlinks (their
targets stored as given, never resolved), sets permissions and modification times, and reads and
writes files as chunks (`FileReader`, `FileWriter`, whose `finish` reports errors that only show
when a file is closed). Over SFTP, reads and writes of a file keep 64 requests of 32 KiB in flight,
as `sftp(1)` does, so a long round trip does not idle the link; a read that comes back short is
asked again for the rest. A write goes out as it is made, and counts once the server has taken it
(`FileWriter::written`); `settle` waits for more of those on their way
([ADR 0052](adr/0052-bytes-written-count-once-they-arrive.md)). A listing tells of each symlink
the kind of its target and the target as stored; over SFTP each link asks both at once, 64 links
at a time. Symlinks go to the server in OpenSSH's argument order (target first), which the client
uses and which differs from the protocol draft; a server that follows the draft would store them
the other way round. Its futures are cancel-safe: a dropped one leaks nothing, though a change
already sent may still happen. Errors are
typed, and the UI words them: not found, permission denied, already exists, the source itself (from
a job), or the error of the OS or the server. SFTP v3 has no code for a name that is taken, so
`SftpFs` reports a plain failure to create or rename as `AlreadyExists` when something has that
name. `rename` replaces an existing file where that takes one step: locally, and over SFTP with
`posix-rename@openssh.com`. SFTP keeps times as whole seconds from 1970 to 2106 and sets the access
time with the modification time, so the access time becomes the current time on both backends. Local
times are set by path (`utimensat`), since opening a FIFO would block.

Every ssh command line is assembled in `noc-ssh`, in this order: program → forced options →
`ssh.args` → role options → `--` → destination. Per-host options belong in ssh_config
([ADR 0007](adr/0007-typed-host-settings-in-hosts-toml.md)). ssh keeps the first value it sees
for an option, so `-o` values in user arguments cannot override forced options; flags are covered
by validation ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)).

## Locations

A panel always shows a directory: a `Location` is `Local(PathBuf)` or `Remote { host, path }`.
Volumes and hosts are chosen in the [location menu](#location-menu), which replaced the virtual
root of [ADR 0006](adr/0006-virtual-root-with-volumes-and-hosts.md)
([ADR 0021](adr/0021-location-menu-replaces-the-virtual-root.md)).

- `..` follows paths, as in mc: from `/Volumes/USB` to `/Volumes`. A local `/` and a remote `/`
  have no `..`, and going up from them does nothing (`Location::parent` is `None`).
- Volumes come from `noc_vfs::volumes`: on macOS `/` and the entries of `/Volumes`, without
  symlinks and without mounts marked `nobrowse` (Time Machine snapshots, for instance); on
  Linux the mounts of `/proc/self/mountinfo` that are `/`, on a `/dev/` device, or a network
  file system, without pseudo file systems or those under `/proc`, `/sys`, `/dev`, `/run`
  (but `/run/media`), `/snap`, and `/boot`; elsewhere only `/`. Their sizes come from
  `statfs`/`statvfs` on blocking threads, each awaited for at most 500 ms: a volume that does
  not answer (a dead network mount can block forever) is listed with `?` and is not asked
  again until the earlier call returns. `[volumes] hide` leaves out mount points by pattern;
  the system volume always stays. A volume is named by its label or else its mount point, and
  opens at its mount point, the system volume at `/`.
- The app reads the volumes once at start, in the background, to know which volume a panel is
  on; the location menu reads them again each time it opens.
- Hosts come in config order, named by their `label` from `hosts.toml` if set, with the address
  of this session's `ssh -G`, or else the one cached from an earlier one. `[discovery] hide`
  leaves out hosts by pattern.
- Opening a host connects in the background and opens, shown as an absolute path, the last
  directory shown on it in this session if `remember_dir` is set and the directory is still
  there, else the configured `start_dir`, else the remote home directory. If the host has an
  `other_dir`, the other panel opens it at the same time. While a panel waits for the
  connection, braille dots in the color of a host connecting turn in place of the host's icon in its
  title, before the path it opens, `⠹ web:/var/www`, and Esc stops it.
- A failed connection, the first attempt or a lost one, opens a dialog, Connection, that says
  `Cannot connect to web: reason` or `Lost the connection to web: reason`, with Retry (the
  default) and Cancel. While it is open, the panels that wait for the host keep waiting, with
  the host's marker of a failure in their titles, still, so the event loop sleeps until the answer.
  On a lost connection, the panels on the host, in every tab, keep their entries, toned down,
  and wait to read the same directory, with the cursor where it was. Retry connects again, and
  the waiting panels read their directories once it is connected, keeping the marks on names
  still there. Cancel after a failed first attempt leaves the panel where it was; after a lost
  connection, it leaves the panels on the host with their entries toned down. No second dialog
  says why again. Jobs on a lost host end with it.
- A panel on a host that is not connected draws what it kept dim, without bold, over its colors
  (`Theme::offline`): rows, status line, the total of the marked entries, and the totals on the
  frame, not the frame, title, or headers; quick search, being typed, is not dim. The cursor and
  marks stay; a rename in its row ends.
- In it, the keys that look through the entries work: moving the cursor and scrolling, quick
  search, sorting, marking, Visual mode (`works_offline` in `app.rs`). A key that needs the host
  opens the dialog Connection, `Not connected to web`, with Connect (the default), which reads
  the directory again, Local, which goes to the last local directory of the panel's jump list or
  else the home directory, and Cancel; so do the keys that look through entries where the panel
  has none. Keys that do not touch the panel's entries work without it: other panels and tabs,
  menus, help, jobs, Quit, Go to directory, the jump list, workspaces, hidden files
  (`works_unconnected`). `R` connects at once
  ([ADR 0039](adr/0039-errors-of-panels-in-dialogs.md)).
- F8 or `d d` in the location menu, or Disconnect in the menus of F9, closes the connection to a
  host, or stops connecting to it. Panels on a connected host stay there with their entries
  toned down, as after a lost connection, and no dialog asks; a panel that only waited for the
  host stays where it was.
- A directory that cannot be read opens a dialog, Error, `Cannot open dir: reason`, and the
  panel keeps what it showed. OK stays; Retry reads it again; Up, where it leads elsewhere, goes
  to the nearest parent, on the directory it came from, past parents that are not there either;
  Go to opens Go to directory with the path. The default is Up for a directory gone under the panel,
  OK for one not there or not allowed, Retry for other errors. A panel has one such dialog at a
  time, the last failure's.
- F4 in the location menu edits the settings of a host in a dialog (label, remote directory,
  other panel directory, remember the last directory); Use Current fills in the directory a
  panel shows on the host. They are saved to `hosts.toml` in the background, and the menu reads
  the hosts again.
- In the location menu, the icon in front of each host shows its state, in a color of its own:
  a server, gray when not connected and green when connected; a spinner while connecting; a
  server with a cross, red, when the last attempt failed, the connection was lost, or a dialog
  asks whether to retry. In the `terminal` theme, which has no colors, connected hosts are bold
  and the others dim. Without icons, the markers are `○`, `●`, the spinner, and `✗`. Every row
  has one cell in front of its name, so names line up across the home directory, volumes, and
  hosts.
- Remote paths are bytes, because SFTP v3 does not guarantee UTF-8, and are displayed lossily.
  The SFTP client library still requires UTF-8 names; see the known issues in the
  [roadmap](roadmap.md).
- Names from the server that are empty or contain `/` or NUL are dropped from listings: joined
  to a local path, they could point outside the target directory.

### Location menu

`g l` opens a menu for the active panel, and Ctrl+x `g l` for the other one, as Far Manager's
Alt+F1 and Alt+F2 open its menus to change the drive of the left or the right panel (F9 → Left or
Right → Change location too). noc binds no Alt+F1 and Alt+F2, which desktops take
([ADR 0049](adr/0049-menus-of-a-panel-keep-the-active-panel.md)), and names the panel as active or
other, not by its side ([ADR 0051](adr/0051-ctrl-x-sends-a-command-to-the-other-panel.md)). The
menu is centered over both panels, as every window is, and its title, Location, names no side:
the panel drawn active is the one it changes, after Ctrl+x the other one. It is the one place to
choose a volume or a host, grouped under headings:

```text
 ╔══════════════════ Location ══════════════════╗
 ║ Filter:                                    ~ ║
 ╟──────────────────────────────────────────────╢
 ║ Favorites                                    ║
 ║ ~ Home                                  212G ║
 ║ Mounts                                       ║
 ║ + Macintosh HD                          212G ║
 ║ + USB                                    12G ║
 ║ SFTP                                         ║
 ║ ● Prod                       deploy@10.0.0.5 ║
 ║ ○ db                                         ║
 ║ ○ staging                    ubuntu@stg:2222 ║
 ║ ✗ nas                              admin@nas ║
 ║ ○ backup                                     ║
 ║ ○ ci                                         ║
 ║ ○ git                                        ║
 ║ ○ mail                                       ║
 ║ ○ vpn                                        ║
 ║ ○ www                                        ║
 ║ 24 more…                                     ║
 ╚══════════════════════════════════════════════╝
```

- Favorites holds `Home`, which opens the home directory, with the free space of the volume
  that holds it; the user's favorites will join it. Mounts holds the volumes, with their free
  space, and SFTP the hosts, with their addresses. Plugins for other backends will be a group
  of their own. A group without rows is not drawn.
- Mounts and SFTP put the places used last first, then the others in their usual order: the
  system volume, then the others by mount point; the hosts in config order. Each group shows
  its first ten places (a constant, not a setting), and always the volume or host the panel is
  on and the hosts that are connected, connecting, or waiting for Retry. A last row,
  `24 more…`, stands for the rest: the cursor and the mouse select it, and Enter on it shows the
  whole group until the menu closes.
- The menu opens in Insert mode, as the other [lists](#lists) do, with the filter taking the keys
  and the cursor on the home directory or the volume that holds the panel's directory, whichever
  is nearer, or on its host. Up and Down move, and Enter opens the row in that panel; the keys stay
  with the active panel, as after Ctrl+w f and Ctrl+w v. After Esc, `j` and `k` move too, and
  `g g` and `G` go to the first and the last row.
- The filter, which `/` or `i` types in again, finds the places by name, path or mount point,
  alias, label, and address, and finds every one, hidden ones too; the rows of more go away while
  the filter has text. It matches them in the [search mode](#search-modes): as fzf does, the
  default, best first, without the headings of the groups; or a row's text must hold the filter,
  or match it as a regular expression, in their order, with smart case. The cursor goes to the
  first row. The filter is a text field, edited with the keys of `input`; Esc goes back to the
  rows with the filter kept. F4 edits the settings of the host under the cursor, F8, or `d d`
  on the rows, disconnects it or stops connecting to it, `R` on the rows reads the volumes and
  hosts again, and Esc or `q` on the rows, Ctrl+c, or F10 closes the menu.
- `recent.toml` in the state directory keeps the places used last: the aliases of hosts and
  the mount points of volumes, most recent first, up to 50 of each. A host counts as used when
  a connection to it succeeds; a volume when a panel arrives in a directory on it in any way:
  the menu, Go to directory, zoxide, Enter, or a workspace. The app reads the file at start, and
  after a turn of the event loop that changed the lists, a task writes it through `noc-config`,
  whole and atomically, one change after another. Mount points that are not UTF-8 are not kept,
  since TOML cannot hold them.
- The menu is modal (keymap context `location_menu`). Each time it opens, and on `R`, a
  background task reads the volumes, scans the ssh config, and loads the cache of `ssh -G`;
  that listing carries a generation of its own, so a stale one is dropped. A host that connects
  or disconnects while the menu is open shows its new state at once, and the cursor stays on
  its row.

### Pull-down menu

F9 (or `g m`) opens mc's menu bar, as Far does: the bar alone with the menu of the active panel
selected. Shift+F9 opens it on the command that ran last, as Far's Shift+F10. An open
menu:

```text
  Left     File     Command     Options     Workspace     Right         2 jobs 37%
 ╔══════════════════════════╗
 ║   Change location…   g l ║
 ╟──────────────────────────╢
 ║ • Sort by name       o n ║
 ║   Sort by extension  o e ║
 ║   Sort by time       o m ║
 ║   Sort by size       o s ║
 ╟──────────────────────────╢
 ║   Rescan               R ║
 ║   Disconnect             ║
 ╚══════════════════════════╝
```

- Left and Right act on the panel drawn on that side, after Ctrl+w x too: its location menu, sort
  order (`•` marks the current one), Rescan, Disconnect while it shows a host, and its
  [tabs](#tabs): New tab, Close tab, and Tab list…. File has F3, F4, Open externally (`g x`), F5 …
  F8, `g +`, `g -`, `g *`, Checksums, and Exit; Command has quick search, Find file… (the [search of
  a whole tree](#search-of-a-whole-tree)), [Go to directory](#go-to-directory), the
  [zoxide](#zoxide) window, the other-panel commands, the jobs, host settings and disconnect for the
  host the active panel shows, help, and redraw; Options has Configuration… and Show hidden files
  (`✓` while on); Workspace has Save workspace…, Workspace list…, and the saved
  [workspaces](#workspaces), with digits as their letters, typed with Alt. Without icons, the marks
  are `*` and `x`. A menu taller than the screen scrolls with the cursor.
- Commands do what their keys do, through the same `Action`s. Each shows the key that does
  it in the active panel's context (`Keymap::key`). Left and Right show the keys of the active
  panel's menu as they are, `g l` for Change location… and `t t` for Tab list…, and those of the
  other panel's after Ctrl+x, `Ctrl+x g l` and `Ctrl+x t t`; the sort orders, Rescan, New tab,
  and Close tab too. Commands that cannot run now are dimmed, in the theme's `disabled`, and the
  cursor skips them.
- On the bar, `h` and `l` (Left and Right, Shift+Tab and Tab) select the next menu, round the bar;
  Enter, `j` (Down), or Alt and the menu's highlighted letter (`&` in the Fluent messages, which
  `Label` in `tui/label.rs` reads, for the menus and the [dialogs](#ui) alike) opens it,
  `G` (End) opens it on its last command, and `k` (Up) and `g g` (Home) do nothing. In an open menu,
  `h` and `l` open the next one, round the bar too; `k` and `j` move round it, from the last command
  to the first and back, and the mouse wheel stops at its ends; `g g` and `G` go to its first and
  last command, and stay there, as vim's do; Enter or Alt and the command's highlighted letter runs
  it. Esc, `q`, F9, or F10 goes back to the bar, and closes the bar from there. Running a command
  closes it first.
- The menu keeps nothing when it closes, and a menu opens at its first command. The app keeps
  only the `Command` that ran last, so Shift+F9 Enter repeats it. A command on a panel stays
  with that panel after Ctrl+w x, under the other title. A command that cannot run now passes the
  cursor on to the next one; one that is gone, such as a deleted workspace, leaves the bar as F9
  opens it.
- `ui.menu_bar` decides where the bar is: `on-demand` (the default) draws it over the top line
  of the panels only while a menu is open, as Far does; `always` keeps it above the panels, as
  mc does, which takes a row from them. The jobs indicator sits at the right end of that row.
- The menu is modal (keymap context `pull_down`), in Normal mode; Alt and a letter or a digit reach
  its hotkeys, so the plain letters stay vim's motions. The keymap hands them to the menu as
  `Resolved::Hotkey`, as it does to the dialogs, and Alt and a letter that nothing has do nothing.

### Go to directory

`g d`, as mc's Quick cd, asks for a path in a dialog for the active panel, or after Ctrl+x for
the other one, as the [location menu](#location-menu) does, and opens the path in that panel, as
`cd` in a shell reads it: relative to the panel's directory, `/…` absolute, `~` and `~/…` the home
directory, `..` and `.` resolved by name, `-` the directory the tab showed before, and
`host:path` on a host, scp-style, which connects to it if needed. On a host, paths stay on it
and `~` is the remote home. [Completion](#completion) reads from the panel's directory too.
`cd ..` puts the cursor on the directory left, as `h` does; a
path that is not there leaves the panel where it is, with the reason below the listing. The Go to
button of a listing that failed opens it for the panel that failed. It is in F9 → Command.

### Completion

Tab completes the path before the cursor in the fields of Go to directory, F5, F6, and F7, in Insert
mode (keymap context `path_input`), and the word before the cursor on the [command
line](#command-line-of--and-), as vim's command line completes with defaults.vim's 'wildmenu'
(`tui/complete.rs`), from a listing of the directory it names, read as the dialog reads its text:

- The last part, after the last `/`, is completed. Nothing matching, nothing happens; one match goes
  in, with `/` after a directory and `:` after a host; of several, the first goes in whole, and a
  list under the field shows them all, as a column, as vim's pop-up menu (context `completion`).
  There, as in vim's 'wildmenu', Tab, Ctrl+n, and Down put the next match in the field, Shift+Tab,
  Ctrl+p, and Up the one before, PgDn and PgUp a page on, round through what was typed, which comes
  back after the last; Left completes again in the directory above the one listed and Right in the
  directory chosen, as in vim's pop-up menu ('wildoptions' pum, `wildmenu_process_key_filenames`);
  Ctrl+y keeps the match and Ctrl+e puts back what was typed, both closing the list; any other key
  closes it, keeps the match, and goes to the field: Enter confirms the dialog with it, as vim's
  Enter runs the command line, Esc goes to Normal mode, and Ctrl+c closes the dialog or the command
  line. A click puts a match in; a double click keeps it.
- On the command line of `!` and `:` (context `command_completion`, which falls back to
  `completion`, then to the command line), as vim's `:!`: the word starts after the last space, tab,
  or line break not escaped with a backslash, and its escapes are taken out to match. In a local
  panel the first word of the command, without a `/`, completes as the executable files of the
  directories of `$PATH`, read once at startup, and with a `/` as the executable files and the
  directories there; the others as files and directories of the panel's directory, where the command
  runs, with `~` and absolute paths as Go to directory reads them. On a host every word completes
  as its files, through its session, and the first as a command never. Names go in escaped for
  `sh` with backslashes, as vim's do; the list shows over the line.
- Names that start with a dot count only after a typed dot. Case never counts, as vim's
  'fileignorecase' has it on macOS. Names that are not UTF-8 or hold control characters cannot be
  typed and are left out.
- Go to directory and F7 offer directories and links to them; F5 and F6 files too. Before a
  `:`, Go to directory offers every host of the ssh config, F5 and F6 the connected ones, F7 none.
- Local directories are read in the background, remote ones through the session of their
  host; a host that is not connected offers nothing, and a Tab never connects. Each listing
  has a generation, and a reply for text that changed since is dropped.

### zoxide

[zoxide](https://github.com/ajeetdsouza/zoxide) ranks the directories the user works in
([ADR 0012](adr/0012-external-tools-and-zoxide.md)). `g z` opens a window of its best
directories for the active panel, or after Ctrl+x for the other one, as the
[location menu](#location-menu) does; Enter opens the directory in that panel:

```text
╔═════════════════════════════ zoxide ═════════════════════════════╗
║ Jump to: src                                                   = ║
║ ──────────────────────────────────────────────────────────────── ║
║ ~/src/noc                                                   56.0 ║
║ /srv/www/src                                                12.5 ║
╚══════════════════════════════════════════════════════════════════╝
```

- In the fzf and regex [search modes](#search-modes), the window asks
  `zoxide query --list --score --exclude <dir> --` once, for every directory but that panel's,
  and what is typed filters them: as fzf does, best match first and those as good in zoxide's
  order, as `zi` does with fzf, or as a regular expression, in zoxide's order. In the plain
  mode what is typed is zoxide keywords, as `z foo bar` in a shell; each change asks
  `zoxide query --list --score --exclude <dir> -- <keywords>` again, and a query still running
  is dropped. Going to or from the plain mode asks zoxide again. zoxide ignores case: where it
  counts, a directory shows only if it has each keyword in its case. As the other
  [lists](#lists), the window opens in Insert mode, typing keywords, as `zi` does: Esc goes to
  the rows, in Normal mode, where `/` or `i` types again and Esc closes the window. Directories under
  the home directory show from `~`, and the window matches them as shown.
- A local directory goes to zoxide (`zoxide add -- <dir>`) once the user does something in it,
  at most once a visit: a copy, move, or delete from it, a copy or move into it when a panel
  shows it, F7, F3 or F4 on a file, `g x`, checksums, or a jump there. Passing through, marking,
  searching, sorting, and `R` do not count. Each tab remembers whether its visit counted;
  going to another directory starts a new one. One task adds them in turn, so no two zoxide
  processes write its database at once.
- `zoxide.record` turns recording off; without zoxide the window says it is not installed,
  and recording is logged once and stops until `zoxide.program` changes.

### Search modes

Every field that searches matches what is typed in one of its ways, with its case
([ADR 0030](adr/0030-search-modes.md), [ADR 0044](adr/0044-one-base-for-search-fields.md),
[ADR 0045](adr/0045-ctrl-s-switches-the-field-typed-in.md),
[ADR 0046](adr/0046-case-and-the-keys-of-search-fields.md),
[ADR 0047](adr/0047-search-fields-remember-their-way-and-case.md)). `tui/search.rs` says which:
each kind of field is a `search::Kind`, with its ways in the order `Ctrl+s Ctrl+s` goes round
them, the way it starts in for each value of `ui.default_search_mode`, `fzf` by default, and the
cases it can take; a field while it is open is a `search::Input`, with its way and its case, which
switches them, builds its `Matcher`, and gives the glyphs at the end of the field. A field never
chooses its ways itself. A field of a dialog searches when the dialog gives it an `Input`
(`Dialog::with_search`); the dialog then draws its glyphs, takes the keys after Ctrl+s and the
clicks on the glyphs, and says its way and case (`Dialog::search`).

| Field | `search::Kind` | Ways, as `Ctrl+s Ctrl+s` goes | Cases | `fzf` starts it in |
| --- | --- | --- | --- | --- |
| Quick search, `/` and `?` | `QUICK_SEARCH` | plain, fzf, regex | all | fzf |
| Filters of the lists | `FILTER` | plain, fzf, regex | all | fzf |
| Filter of the zoxide window | `FILTER_PATHS` | plain, fzf, regex, on paths | all | fzf |
| Text of the search of a tree, `f /` and `f f` | `TREE` | plain, regex | all | regex |
| Files of the search of a tree, Text mode | `FILES` | glob | all | glob |
| Files of the search of a tree, Files and dirs | `FILES_FD` | glob | case | glob |
| Pattern of `g +` and `g -` | `MARK` | glob, plain, regex | all | glob |

The filters of the lists are those of the location menu, the tab menu, the Workspaces window, the
command history, and the list of the key hints. Each field keeps its way and its case until Noon
Commander quits (`search::Memory`, by `search::Slot`, nothing on disk): the next time it opens, it
opens as it was. One slot is quick search in both panels, `/` and `?`; each list has its own; the
text of the search of a tree is one in both its modes, its globs another, and `g +` and `g -`
share one. A field opens in `ui.default_search_mode` the first time, or in its own way if it has
not that one: `fzf` is regex for the text of the search of a tree and glob for `g +`. Changing the
setting leaves the fields that have opened as they were. The App keeps how each open field is
after each key and press of the mouse (`App::remember_searches`), so Esc keeps a switch too. `n`
and `N` match as the last search did.

Case (`search::Case`, which is `noc_pattern::Case`) is smart in every field and every way to
start: it counts only once what is typed has a capital letter, as ripgrep and fd have it; then
ignored, or counted, as the field is switched. A kind says the cases it can take (`Kind::cases`):
one alone takes no click on its glyph and no key after Ctrl+s. The search of a tree gives the case
of the text to ripgrep (`--smart-case`, `--ignore-case`, `--case-sensitive`) and fd (its default,
`--ignore-case`, `--case-sensitive`). The case of the globs is glob by glob: ripgrep takes each as
`--glob` where case counts for it and `--iglob` where not; fd has no `--exclude` that ignores case,
so by name the field has only case, and ripgrep's comes back in the Text mode.

Two glyphs at the end of the field say the case, then the way, in every field: at the right end
of the status line of quick search, and at the end of the field of the filters and of the
dialogs. A blank goes before them and one after each, in the colors of the field
(`Filter: text   Aa ~`, and a blank): kitty draws a Nerd Font icon over two cells when the cell
after it is blank, and spills it over what comes next otherwise. In `glyphs.rs`: for the case,
nf-md-format_letter_case_upper, nf-md-format_letter_case_lower, nf-cod-case_sensitive (`Aa`, `aa`,
`AA` without icons); for the way, nf-cod-whole_word, nf-md-approximately_equal, nf-cod-regex,
nf-seti-shell (`=`, `~`, `.*`, `*`). The glyph of the way is crossed out, in the color of a
broken link, while the text is no regular expression, when the lists say what is wrong with it in
place of the rows, and while a glob of the Files field is none that globset reads
(`noc_tools::globs`). A glyph that a click does not switch, as the field has one way or one case,
shows in the theme's `disabled`, as a command of the pull-down menu that cannot run: the glyph of
shell patterns of the Files field, and by name its case too. What is wrong shows over it.

The keys switch the field that has the focus, and no other: quick search, the filter of a list
while it takes the keys, in Insert mode, and a field of a dialog that searches, in either mode.
Ctrl+s leads them: `Ctrl+s Ctrl+s` or `Ctrl+s s` the next way, `Ctrl+s p`, `f`, `r`, `g` a way by
its letter, `Ctrl+s c` the next case, and `Ctrl+s a`, `i`, `m` smart case, case ignored, or case.
Elsewhere, on the rows of a list or on a check box or a button, they do nothing. A click on the
glyph of the case does what `Ctrl+s c` does, and on that of the way what `Ctrl+s Ctrl+s` does,
giving the field the focus first: a list goes to its filter, in Insert mode. The keys live in the
keymap context `search_input`, which goes in front of the chain of where the keys go while such a
field has the focus: `App::key_place` gives the keymap a `keymap::Place`, the mode, the context,
and the kind of the field, and `Place::chain` puts `search_input` first. So the F-key bar, the
help, and the key hints show them where they work, and nowhere else; the key hints leave out the
ways the kind has not (`Place::allows`), whose keys do nothing rather than type.

The [search of a whole tree](#search-of-a-whole-tree) has no fzf way, ripgrep and fd
matching a pattern rather than ranking names, so `fzf` starts it in regex. The ways:

- **Plain**: the text anywhere. The rows keep their order, and quick search goes down them as
  vim's search does.
- **fzf**: as below.
- **Regex**: a regular expression, built by the `noc-pattern` crate as the search of a tree
  builds its patterns for fd: the `regex` crate, where a capital that is escaped, as in `\S`,
  does not make case count, and the last line of what the engine says is wrong. The rows keep
  their order, as in the plain mode.
- **Glob**: a shell pattern, as mc's `+` and `-` read it (`tui/pattern.rs`): `*`, `?`,
  `[a-z]`, `{a,b}`, and `\`; the whole name matches. What does not parse is taken as it is, so
  nothing is ever wrong with it.

The fzf mode follows [ADR 0016](adr/0016-fuzzy-search.md). `tui/fuzzy.rs` wraps
`nucleo-matcher`, Helix's port of fzf's algorithm:

- The letters typed must come in order, not necessarily together: `cfg` finds `config.rs`.
  Runs of them, and letters that start a word (after a space, `/`, or punctuation, or a
  capital after a small letter), score higher; in the zoxide window, after `/` most.
- Words separated by spaces must all match, in any order. As in fzf's extended search,
  `'word` matches as it is, `^word` at the start, `word$` at the end, and `!word` only where
  it is not; `\` takes a space or one of these as it is.
- Case counts as the field says, smart case to start; letters with accents match those
  without.
- Lists sort their rows by the match, best first, and keep their order among matches as good;
  the cursor goes to the first ([ADR 0040](adr/0040-one-picker-for-every-list.md)). The
  location menu drops the headings of its groups meanwhile. Quick search keeps the order of the
  listing and goes to the best match.
- Tab completion in path fields still completes the start of a name.

### Search of a whole tree

`f /` in a panel, or F9 → Command → Find file…, opens a dialog over both panels that searches the
files below the active panel's directory with ripgrep; `f f`, or F9 → Command → Find files and
dirs…, opens it in its Files and dirs mode, which finds their names with fd
([ADR 0027](adr/0027-search-of-a-whole-tree.md),
[ADR 0029](adr/0029-files-and-directories-with-fd.md)):

```text
╔════════════════════════ Find in ~/src/noc ═════════════════════════╗
║ (*) Text   ( ) Files and dirs                                      ║
║ Text:                                                              ║
║ handle                                                      Aa .*  ║
║ Files:                                                             ║
║ *.rs !target                                                 Aa *  ║
╟────────────────────────────────────────────────────────────────────╢
║   crates/noc/src/tui/app.rs                                     2  ║
║    2599  fn handle(&mut self, input: Resolved) {                   ║
║    3700  fn handle_find(&mut self) {                               ║
║   crates/noc/src/tui/find.rs                                    1  ║
║     172  pub(crate) fn handle(&mut self, input: Resolved) {        ║
╟────────────────────────────────────────────────────────────────────╢
║ ⠹ 3 lines in 2 files                                               ║
║ 2 errors: rg: ./secret: Permission denied (os error 13)            ║
╟────────────────────────────────────────────────────────────────────╢
║              [< Go to >] [ View ] [ Edit ] [ Cancel ]              ║
╚════════════════════════════════════════════════════════════════════╝
```

- A `Dialog` (`tui/dialog.rs`) with a list (`Dialog::with_list`): two fields, the text and the
  globs of the files, split at blanks, `!` before one leaving those out; the rows; and four
  buttons. The text is a regular expression in the regex and fzf [search modes](#search-modes) and
  as it is in the plain one, with smart case; the keys after Ctrl+s in its field switch both, and
  the glyphs at the end of the field say which (`Dialog::with_search`). The globs are shell
  patterns, their field's one way, whose case the keys after Ctrl+s switch too. Both fields open
  as they were last. With no text it lists the files. The dialog keeps
  the cursor of the rows and scrolls them; `FindWindow` (`tui/find.rs`) keeps what was found and
  draws each row (`ListView`).
- Two modes, radio buttons on a line above the fields (`Dialog::in_a_row`): Text, with ripgrep
  (`FindQuery::Lines`), and Files and dirs, with fd (`FindQuery::Names`), whose buttons are Go to
  and Cancel (`Dialog::set_buttons`). By name, fd matches the text against the names, as a
  regular expression or as it is, with the case of the text; `!` globs go to it as `--exclude`,
  which tells case apart, as do the others then (`search::Kind::FILES_FD`), and
  `noc_tools::fd::Names` keeps what the other globs let through and finds where the name
  matches, with fd's regex engine, for `find_match`. On a host `fd` runs,
  or `fdfind` where the shell says 127. Its rows (`Finding::Entry`) are the files and
  directories, each with its icon and its path, which loses its start when too long
  (`cells::fit_spans_end`); Go to goes into a directory (`FindEvent::Open`).
- Each change starts a search 150 ms after the last key (`FIND_DELAY`), the first at once. A task
  (`Effect::Find`) runs ripgrep through `noc_tools::ripgrep`: locally in the directory, as
  `[search] program`; on a host, `rg` through the host's task and `Session::spawn_in`, over the
  master without a terminal ([ADR 0028](adr/0028-programs-on-hosts-without-a-terminal.md)). It
  reads ripgrep's JSON (`Hits`) while a task of its own drains stderr, reports what it found at
  most every 50 ms (`Done::Found`), stops ripgrep after `[search] max_results` lines (or files),
  takes `[search] max_per_file` lines of a file at most (`--max-count`, `100+` by the file),
  then reports how it ended (`Done::FindEnded`): the limit, and the errors ripgrep told of. Each
  search has a generation, so the reports of an earlier one are dropped; the dialog holds the
  `CancellationToken` of the last, so a new search or closing the dialog stops it.
- Its keys are those of dialogs (contexts `find_input` in a field, `find` elsewhere): it opens in
  Insert mode, typing in the text; Tab, Up, and Down go through the fields, the rows, and the
  buttons; Esc leaves Insert mode in a field, and again closes the dialog. While the rows have the
  focus, the motions and the page keys move their cursor. F3 and F4 work wherever the focus is,
  the keys after Ctrl+s in the field of the text, and the letters of the dialog work as in every
  dialog ([ADR 0033](adr/0033-letters-in-dialogs.md)). The wheel scrolls the rows.
- The rows are the files, in the order of their paths, each with how many lines matched, and their
  lines with their numbers, the matches in a color of their own (`find_match`), apart from the
  cursor's; a line too long starts a little before its first match. The rows of a search stay until
  the next one finds something or ends, the cursor on the row it was on. While it searches, the dots
  turn in front of the title, a skeleton stands in place of the rows until the first come, and the
  status line under them a skeleton bar, which the shine crosses with the row. Once it ends, the
  status says how many lines it found in how many files; that it stopped at the limit; and the
  errors. A pattern that is not a regular expression, a missing ripgrep, or a host that is not
  connected says why in place of the rows.
- Go to, the default button, goes to the file in the active panel: its directory, with the cursor
  on it; where the panel was goes to the jump list. View (F3) views it with the line at the top,
  and Edit (F4) edits it at the line (`Editor::edit_at`), a remote one through a local copy; the
  dialog is there again after both. On a file's row they take its first line that matched.
- It is not in the queue of dialogs, which shows only the one in front, over everything: it
  stays under the viewer, the jobs, the help, and the dialogs, as the window it was.

### Lists

The location menu, the tab menu, the zoxide window, the Workspaces window, and the command history
list rows to choose from. Each opens in Insert mode, with its filter taking the keys ([ADR
0036](adr/0036-lists-open-in-insert-mode.md)): characters are text, the keys of `input` edit it
(Ctrl+r types a register, which the app reads, and Ctrl+o, which has no Normal mode to go to, does
nothing), Up, Down, Ctrl+p, Ctrl+n, and the page keys move, Enter takes the row under the cursor,
and Ctrl+c or F10 closes the window. Esc goes to the rows, with the filter kept, in Normal mode
(keymap context `list`, which each one's context falls back to): `j`, `k`, Ctrl+j, Ctrl+d,
Ctrl+u, Ctrl+f, Ctrl+b, `g g`, and `G` move, and the rows scroll as the panels' do, headings among
them, a count repeats a motion, Enter takes the row, `d d` and F8 delete it where the window can,
as many rows as a count says (`3d d`, as vim's deletes three lines), `/` or `i` types in the
filter again, and Esc, `q`, Ctrl+c, or F10 closes the window. Text pasted while the rows have the keys
goes to the filter, in Insert mode. There are no digit hotkeys.

- All share `tui/picker.rs` ([ADR 0040](adr/0040-one-picker-for-every-list.md)): the filter, a
  text field with a cursor (`dialog::Field`), drawn in the colors of the fields of dialogs after
  its prompt (`search::Mark::render_field`), which matches in the
  [search mode](#search-modes); the cursor, scrolling, the mouse, and the box, which each draws
  centered over both panels, the menus of one panel and the zoxide window too. The only lines
  across a list are under the filter and over the whole row under the cursor, and they join the
  frame (`╟───╢`) as the line
  over the buttons of a dialog does; a heading is its text alone, in the color of titles. Each
  holds its own rows, says what the filter finds them by, and draws them. A row may show only
  while the filter is empty, as the rows of more in the location menu do, or only while it has
  text, as the places behind them do.
- The zoxide window gives its own prompt, matches paths as paths, after `/` best, and in the
  plain mode lets zoxide pick its rows from the keywords typed: the picker says the filter changed
  (`Pick::Filtered`), and the window asks zoxide again.
- The location menu and the zoxide window wait for their rows from a task, through
  `tui/loading.rs` (`Loading`): the answer they wait for, an older one dropped; the dots in front
  of the title, at first and while they ask again; the skeleton, as tall as the last list without
  a filter; and why no rows came, in their place.
- Every list shows the whole row under the cursor in two lines under the rows, while it has rows
  (`Body.full`; [ADR 0041](adr/0041-lists-always-show-the-whole-row.md)): the directory of the
  zoxide window, the place of the location menu, the title of a tab, a key hint with all its keys.
  The command history shows the whole command there instead, and the Workspaces window where the
  tabs of the workspace are, in two columns (`Body.footer`, `Body.footer_column`), whose line
  joins the line over them and the bottom of the frame (`┬`, `╧`). A skeleton that stands for the
  rows of the last list takes those lines too, so that the box does not grow when the rows come.
  `K` is the panels' alone.

### Tabs

Each side has tabs of its own ([ADR 0011](adr/0011-tabs-per-panel.md)). A side with more than
one shows them, numbered, on a line above the panels (`ui.tab_bar = "line"`, the default):

```text
║ 1 src │ 2 noon │ 3 web:log          ║║ 1 ~                              ║
╠ /Users/me/projects/noon ══════════╣╠ /Users/me ═══════════════════════╣
```

or in the top line of the panel's frame, in place of its title (`"frame"`), which costs no
row and names the tab that shows by its whole location:

```text
╔ 1 src ═ 2 /Users/me/projects/noon ═ 3 web:log ═╗
```

- A tab is a whole `Panel` (listing, cursor, marks, sort order, quick search), so switching
  shows it at once. Hidden tabs list nothing: when a job or F7 changes a directory that a
  hidden tab shows, the tab is marked stale and reads it again when it shows.
- `t n` opens a new tab after the one that shows, on the same directory with the same listing
  and cursor, without marks; `t c` closes the tab that shows (the last one stays), and the next
  one shows. `g t` and `g T`, or `] t` and `[ t`, go round the tabs, and `g t` with a count goes
  to the tab of that number, as in vim; `t t` opens the tab menu of the active panel, as vim's
  `:tabs` lists them: the tabs with their whole locations, centered over both panels, to show
  one, under the title `Tabs`, which names no side; it grows with the widest location up to the
  width of the other lists, 76 cells. Ctrl+x
  before `t t`, `t n`, `t c`, `g t`, and `g T` does it in the other panel, with a count after
  Ctrl+x (`Ctrl+x 2g t`), as before any command. `d d` and F8 (`close_tab`) close the tab under
  the cursor there as `t c`
  closes the one that shows (`Tabs::close_at`), and the menu stays open with the rest. Left and
  Right in F9 do the same for the panel on that side.
- Names on the line of tabs are brief: the last component of a directory, `~` for the home
  directory, `host:name` on a host, and the host alone in its start directory. The frame's
  verticals close each side's line, and its top corners become tees (`╠`, `╣`) that join it,
  so the two sides stay apart. When the tabs do not fit, the names of hidden
  tabs shrink first, then the one that shows, none below eight cells; then tabs far from it
  are left out, and `«` and `»` say so.
- Panels are named by a `PanelId`, their side and tab number, which is never used again.
  Listings, new directories, and the dialogs of `g +`, `g -`, F5, F6, and F7 carry it, so a reply
  reaches the tab that asked even after a switch, and one for a closed tab is dropped. What
  acts on the other side (Ctrl+w f, Ctrl+w v, Ctrl+x, F5's target, `other_dir`) acts on the tab
  that shows there. Disconnecting or losing a host leaves every tab on it there, hidden or not,
  without its entries; after a lost connection they wait for Retry and read their directories
  when the host is back.
- Tabs look like tabs: the line is dark (`tab`: gray on black in mc-classic, as the F-key
  bar; `crust` in Catppuccin; dim text in `terminal`), and the tab that shows takes the
  panel's colors as if it grew out of it (`tab_active`), with its number in an accent on the
  side that has the keys (`tab_number`) and plainer on the other (`tab_active_idle`). Tabs in
  the frame take the same styles, with the frame's line between them.
- Ctrl+w x swaps the sides with their tabs. `o h` and the settings apply to every tab.
- Each tab keeps a jump list of its own, as vim's windows do, and none goes to a workspace: see
  [Panels](#ui).

### Workspaces

A workspace is the tabs of both panels, saved under a name
([ADR 0017](adr/0017-workspaces.md)): for each side its tabs in order, each with its location,
sort order, and the name under its cursor, and the tab that shows; and the side with the keys.

- `t W`, or F9 → Workspace → Save workspace…, asks for a name, offering
  that of the workspace restored or saved last, and saves the tabs under it; the name of
  another workspace asks before replacing it. A location that `workspaces.toml` cannot hold,
  such as a name that is not UTF-8, is saved as the nearest directory above it that it can
  hold, or else as `~`.
- `t w`, or F9 → Workspace → Workspace list…, opens a window of the saved
  workspaces, one of the [lists](#lists), with a filter, which matches in the
  [search mode](#search-modes) (keymap context `workspaces`): Enter restores, `a` saves the tabs
  as a new workspace, starting with an empty name, `r` or F6 renames, and `d d` or F8 deletes
  after a question. A workspace just saved there gets the cursor. Each row counts the tabs of the
  left panel, then those of the right. Under the rows, a column for each panel lists where the
  tabs of the workspace under the cursor are, one to a line, cut at the start where they do not
  fit, in as many lines as the workspace with the most tabs on a side needs, four at most, the
  last saying how many more; the heading of the side with the keys is in the color of titles, and
  the tab that shows on each side is bold.

  ```text
  ╔═══════════════════════ Workspaces ═══════════════════════╗
  ║ Filter:                                                = ║
  ╟──────────────────────────────────────────────────────────╢
  ║ noon                                               2 │ 1 ║
  ║ deploy                                             1 │ 3 ║
  ╟────────────────────────────┬─────────────────────────────╢
  ║ Left                       │ Right                       ║
  ║ ~/src/noc                  │ web:/var/www                ║
  ║ ~/Downloads                │                             ║
  ║                            │                             ║
  ╚════════════════════════════╧═════════════════════════════╝
  ```

- F9 → Workspace lists the saved workspaces below its two commands, the first ten with
  Alt+1 … Alt+9 and Alt+0, and marks the one restored or saved last; choosing one restores it.
- Restoring replaces every tab of both panels with new ones (new `PanelId`s, so replies for the
  old tabs are dropped). The tab that shows on each side lists its location at once, connecting
  to its host if needed; a hidden one keeps its first request and sends it when it shows, so
  only the hosts that show connect. The cursor goes to the saved name once the listing arrives.
- `noc-config` owns `workspaces.toml`; one task reads and changes it in turn, and each change
  reads the file again first, so another Noon Commander's changes are kept. The app reads it
  at start, and takes the workspaces back after each change.

### Configuration dialog

Options → Configuration… shows the settings of `config.toml` by category, as a modern
settings window: there is no OK or Cancel, and every change takes effect as it is made
([ADR 0009](adr/0009-configuration-dialog-writes-config-toml.md)):

```text
  ╔═════════════════════════ Configuration ══════════════════════════╗
  ║  󰍹 Interface  │ Language           auto                          ▲
  ║  󰤺 Animations │ Theme              < mc-classic >                █
  ║  󰓡 Transfers  │ Borders            < Double ═ ║ ╔ >              █
  ║  󰣀 SSH        │ Icons              [x]                           ░
  ║  󰋊 Volumes    │ Show hidden files  [x]                           ▼
  ╟───────────────┴──────────────────────────────────────────────────╢
  ║ A language tag, such as en-US, or auto for the system locale.    ║
  ║ Takes effect after a restart.                                    ║
  ╚══════════════════════════════════════════════════════════════════╝
```

- Categories are on the left, with a Nerd Font icon each (without `ui.icons`, names only);
  the settings of the chosen one are on the right: check boxes `[x]`, choices `< … >`, and
  text fields. Every option of `config.toml` is there, so that nobody has to edit the file:
  Interface (`[ui]`), Animations (`[animations]`), Transfers (`[transfer]`), SSH (`[ssh]` and
  the hidden hosts of `[discovery]`), Volumes (`[volumes]`), zoxide (`[zoxide]`), Command line
  (`[shell]`), and Search (`[search]`).
- Paths under the home directory show from `~`, and are written as typed. Lists, such as
  `ssh.args` and the hidden hosts and volumes, are words separated by spaces, as a shell
  reads them: `"…"` around a word with spaces, `\` before a character to take it as it is.
- Settings that do not fit scroll with the cursor, and a scroll bar (`█` on `░`, between `▲`
  and `▼`, Nerd Font carets with `ui.icons`) shows on the right of the frame, as everywhere
  ([ADR 0024](adr/0024-scroll-bars-on-the-frames.md)). The two lines below
  them, set apart by a line across the dialog, say what the setting under the cursor does and
  whether it takes effect only after a restart, or, in the colors of errors, why the text
  typed cannot be used.
- Tab moves between the categories and the settings; `j` and `k` (Down and Up) move within
  them, and `g g` and `G` go to the first and last of them; `h` (Left) on anything but a text
  field goes back to the categories; Esc, `q`, or F10 closes the dialog, in Normal mode (a
  text field in Insert mode takes a first Esc to Normal mode).
- Alt+1 … Alt+8 open the categories, from anywhere in the dialog, the digit shown in front of
  each, and each setting has a letter, unique within its page: Alt and it do what Space or Enter
  does on the row, or put the focus in its text field, in Insert mode, as the letters of every
  dialog do ([ADR 0033](adr/0033-letters-in-dialogs.md)).
- Space or Enter switches a check box or picks the next choice, and `h` and `l` pick choices: each
  takes effect at once. The dialog opens on the language, a text field, in Insert mode; a text field
  the cursor comes to with `j` or `k` is in Normal mode, where `i`, `a`, or `A` goes to Insert mode,
  and one it comes to with Tab or Shift+Tab, a click, or from a field in Insert mode, in Insert mode
  (see [Text fields](#ui)); what Visual mode selects shows in reverse. A text field takes effect
  when the cursor leaves it (Up, Down, Tab, `g g`, `G`) or on Enter, and as the dialog closes. A
  count before `j` or `k` moves as many rows, past the text fields too. Its value is checked first:
  the language tag, the formats (`auto`, `iso`, or a tag), a whole number of parallel jobs, a
  program for ssh, and the extra ssh arguments,
  through the same validator as at start ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)).
  An invalid one keeps the cursor in the field; Esc puts back what it had and closes the dialog.
- A change is used at once, but the language, which the next start reads: the interface
  redraws, new formats are made in a task and the panels sort again, copies and the job queue
  follow `[transfer]`, and the settings in `Context`, which
  sit behind a lock, change for new connections, `ssh -G`, and listings, so the location menu
  lists the volumes and hosts with them the next time it opens. Connections that are open stay
  as they are.
- Each change writes its keys to the config file in the background. One task writes them in
  turn, so that a change never overtakes the one before it; a failure shows an error.

## Host discovery

OpenSSH cannot list hosts, and `ssh -G` resolves a single host while executing `Match exec`
predicates. So:

1. A tolerant scanner reads only `Host`, `Match`, and `Include` (recursive, with globs, `~`, and
   `${ENV}`) from `~/.ssh/config` and `/etc/ssh/ssh_config`, or from `ssh.config_file`, which,
   like `ssh -F`, replaces both. It skips every other keyword and never fails on unknown ones.
2. Concrete patterns (no `*`, `?`, or `!`) become hosts. `Include` lines with `%` tokens cannot be
   expanded statically; they are skipped and logged.
3. Effective values (user, hostname, port, proxy jump) come only from `ssh -G`, which runs
   when the TUI connects to a host (or for `noc debug hosts --resolve`, with bounded
   parallelism), never for every host at startup. Results are cached in
   `~/.cache/noc/resolve.json`, valid while the ssh settings, the config files read
   (inode, mtime, size), and the names in their directories stay the same, so earlier
   addresses show at once. A cached entry only stands in until `ssh -G` runs again.
4. `discovery.hide` hides patterns such as `github.com`.

## Authentication

Prompts go through the askpass bridge ([ADR 0003](adr/0003-askpass-bridge.md)): ssh runs
`noc` as its `SSH_ASKPASS` program, which forwards the prompt to the TUI over a Unix socket
and returns the answer. The command-line subcommands answer prompts on `/dev/tty`; the TUI
shows them as dialogs:

- passwords, passphrases, PINs, and codes: a masked field, OK, and Cancel;
- host keys and confirmations: ssh's question, Yes, and No, which is the default;
- notices, such as a request to touch a security key: shown until ssh is done.

Dialogs queue: a new prompt, say from a second host, waits until the one in use is answered,
so it never takes over the keys mid-password. A dialog closes by itself when ssh stops
waiting. The typed secret lives in memory reserved up front, so it is never copied by growing,
and is wiped when the dialog closes; it is passed on as a `SecretString` and never logged.
Messages from ssh, which may quote the server, are shown terminal-safe.

## Command line

```text
noc                          the TUI (needs a terminal)
noc -V, --version            the version: noc 0.2.0
noc config init              write the commented default config.toml
noc config paths             show the files and directories in use
noc debug version            version, build, system, terminal, editor, zoxide: one `key value` line each
noc debug hosts [--resolve]  hosts from ssh_config with cached addresses; --resolve runs ssh -G
noc debug ls [LOCATION]      a local path or host:path; without one, the mount points and hosts
```

`crates/noc/build.rs` records the commit, its time in UTC, and whether the sources differ from
it, with `git`; without git they are `unknown`. `noc debug version` asks the terminal, in a
blocking task: in raw mode on `/dev/tty`, and only when stdin is a terminal and `noc` is in the
foreground, it sends XTVERSION, the kitty keyboard query, and primary device attributes, whose
answer ends the wait (at most a second). It asks zoxide too, `zoxide --version`, as `[zoxide]
program` names it. The facts are `about::Fact::ALL`, in order, which the About page of the help
shows too, each under the title of its `Topic`; the help names each in a `match` that a new fact
does not compile without.

`--config FILE` replaces `~/.config/noc/config.toml`. Logs go to
`~/.local/state/noc/noc.log`; `NOC_LOG` sets the filter, for example
`NOC_LOG=debug`.

## Async model

- tokio runtime; ratatui with the crossterm `EventStream`.
- One-way data flow: events (keys, VFS replies, job progress, connection state) →
  `update(state, msg)` → effects (spawned tasks with a `CancellationToken`) → render. Reports
  that queue up, such as the progress of a job, are all taken before the next frame, so a job
  that reports every entry does not draw every entry. Results of background work draw at most
  once every 40 ms (`Pace` in `tui/mod.rs`): a copy reports every chunk, thousands a second, each
  apart from the next, and drew a frame for each. Its reports of progress reach the loop in
  batches, at most one each 20 ms, each report with the time the job made it, so that the loop
  turns once a batch rather than once a report
  ([ADR 0055](adr/0055-a-job-s-reports-go-in-batches.md)). Keys and the mouse draw at once, and so do
  results after a quiet spell. A report of a job draws only when what the screen shows of it
  changes (`App::job_report`, `App::sight`): its window while it is in front; from the
  background its row while the list of jobs is open, and else the percentage in the corner. The
  window's `Look` holds what it shows before it is put in words: entries, sizes as they read,
  the whole percent that the gauge follows too, the second that the time shows, and the speed
  and the time left held for that second. Making it takes no formatting, which goes through the
  system on macOS, so it is made for every report, and the window draws from it. A row's
  `RowLook`, taken from it, holds less: the state or the percent, the time left, and the entry at
  hand. The time of a working job wakes the loop when the second it shows changes
  (`JobView::next_tick`), not at a fixed rate; for a row, only when the job moves data, whose
  row shows the time left.
- Every request carries a generation number, so stale replies (for example, a listing of a
  directory the user has already left) are dropped.
- The UI task never awaits network I/O.
- One task per host owns its ssh session and SFTP channel. The app sends it listing requests
  over a channel and gets told when the host is connected and when the connection ends: on
  request, when the master exits, or, without multiplexing, when the channel's ssh exits.
  Quitting restores the terminal first, then stops the jobs and gives them a few seconds to
  clean up through sessions that are still there, then gives the connections a few seconds
  to close.
- The terminal is restored on every exit: a guard leaves raw mode and the alternate screen when
  the TUI returns or fails, ratatui's panic hook does it before a panic message, and SIGTERM,
  SIGHUP, SIGINT, and the terminal going away end the event loop like a quit.
- Another program (the editor of F4, a command of the command line, the output of commands of
  `g <`) gets the terminal as the shell would give it: the event loop shows the cursor, gives it
  the terminal's own shape back, leaves the alternate
  screen and raw mode, and drops its `EventStream` first. The stream's reader thread holds
  crossterm's input lock while it waits, and a new stream takes that lock, so the old one must go
  before the program starts, or its reader eats the first key, and the new one is made after it. The
  loop waits for the program, then takes the terminal back and draws everything. Meanwhile it
  takes the jobs' reports of progress as they come, which their windows count without drawing,
  so that a copy's thousands a second do not pile up until the terminal is back; the rest of what
  background work sends, questions among it, waits in the order it came until the app has taken
  the program's end ([ADR 0055](adr/0055-a-job-s-reports-go-in-batches.md)). The editor and a
  local command run as jobs, in process groups of their own with the terminal, so Ctrl+c and
  Ctrl+z reach them alone (`noc_tools::job`, [ADR 0054](adr/0054-shell-run.md)); the SIGINT
  stream is still made anew. SIGTERM, SIGHUP, and the terminal going away, which `tui::told`
  hears for the whole run, end every wait for a key there, and the event loop ends without a new
  stream: a terminal that went away leaves crossterm's reader of the last stream spinning at the
  end of its input with the lock that a new stream would wait for, forever
  ([crossterm#793](https://github.com/crossterm-rs/crossterm/issues/793)). The kernel sends
  SIGHUP to the leader of the terminal's session alone, which may ignore it rather than pass it
  on, as `nohup`, `trap '' HUP`, or `sudo` may; so a thread watches a copy of stdin with mio
  (epoll, kqueue) for the end of its input, which a key does not wake it for twice. poll(2) on
  macOS does not tell of that end, and tokio's `AsyncFd` takes `unsafe`. ssh children are in
  sessions of their own and see none of it.
- The working directory of the process follows the active panel: after each turn of the
  event loop, a local directory the active panel now shows (once its listing arrived)
  goes to a task that `chdir`s to it in `spawn_blocking`, the latest of the waiting ones only.
  A remote panel leaves it where it was. Programs that the app starts, such as the editor,
  inherit it.

## File operations

`noc-ops` holds jobs that work on any `Vfs` backend: generic code, which the UI runs
with `LocalFs` in a task of its own or with a host's `SftpFs` in that host's task. A copy runs
in a task of its own with the sessions of the hosts at its ends, which their tasks share
(`Arc<SftpFs>`), so a copy between two hosts has both; it shares the panels' SFTP channel,
so on a slow link listings wait behind its data. Separate transfer channels may follow. When
a host's task ends while a copy holds its session, the session goes with the copy, which
fails soon with the channel gone; the UI ends the job at once. A job talks
to the UI through a `Reporter`: it sends `Event`s (`Scanning`, `Progress` with the entry at
hand and done/total counts, `Failed`), and when an operation fails it waits for a `Decision`:
Retry, Skip, Skip all (no more questions), or Abort. A `CancellationToken` stops it between
operations and while it waits for a decision, for data, or for a host to open a file or set its
attributes; so does a UI that stops listening. A request to a host that stopped answering never
ends, so the task that runs a job gives it two seconds once cancelled (`noc_ops::in_time`), then
drops it, which abandons what it waited for; work on a host is cancel-safe. A job dropped ends
as not complete, without the sums a checksum found; what it asked of the host before, such as
removing a file it did not finish, still happens there, in order, while the session lasts. It
returns an `Outcome`: entries done, entries skipped, and whether it was aborted.

`Progress` carries two byte counts. `bytes_done` drives the gauge: it takes a skipped file's
size at once and goes back to the start of a file that is retried. `bytes_copied` counts only
bytes read and written and never goes back, so the UI derives the speed from it and the time
left from both. Both count a byte written once it has arrived: a copy reports as the writer's
`written` grows, and after the last chunk of a file reports what arrives as it settles the rest,
so that an upload follows the link rather than running up to 2 MiB ahead of it. The task that
runs a job passes its reports on as it takes them (`forward` in `tui/tasks.rs`), in the job's own
task, with the time it takes each: when the job made it. Reports of progress go in batches, the
first after a quiet spell at once, then at most one each 20 ms (`BATCH`), and those that wait go
before a question or the end. The window counts each report of a batch (`JobView::passed`) and
shows the last (`JobView::working`), so the speed and the time left are those of the reports one
by one ([ADR 0055](adr/0055-a-job-s-reports-go-in-batches.md)). The window counts each report and
question at that time, not when the UI takes it: a report waits for its batch, and a question
asked while the terminal is handed over to another program waits until it is back, while the UI
takes the reports of progress meanwhile. One taken more than a quarter of a second after the
speed and the time left of the second were worked out makes them be worked out again, the time
left counting down from the one of the second before, as it would have. The job's
window times only work: its stopwatch starts with the
first `Progress` and stands from a `Failed` or `Exists` question, from when the job asked it,
until the answer, including while the question waits behind other dialogs. The speed is that of
the last five seconds of work, by
the stopwatch: the window keeps `bytes_copied` at moments of work at least a quarter of a second
apart, and divides what was copied since the last of them five seconds back by the time since.
A slow start is behind it after five seconds; when no data comes it falls to zero. The time
left looks further back, so that it does not swing with the speed
([ADR 0050](adr/0050-time-left-of-a-job.md)): its `Forecast` sums the time, the bytes copied, and
the entries done over stretches of work, each weighing a third as much every half minute older
(`ETA_HORIZON`), and forgets a group of them that has faded to nothing. What a byte and an entry
cost come from least squares of the time of the stretches between finished entries on their
bytes and their entries, when both stand clear of the noise: jobs report as an entry starts,
before it opens, and close it after their last report. Else a byte costs what it does in the
stretches in which no entry was done but for those that start with an entry's opening (as do
the job's first report and a question, after which Retry opens the entry again), and an
entry what its stretches took beyond their bytes; among small files only, the
least squares still give the cost of an entry if it stands clear; else bytes alone count. The
time left is the bytes and entries left at those costs and that pace. It is unknown while the
entries left are on average more than ten times as large as those done since the forecast's
first report (`AHEAD`), not the renames of a move, until the work has told what a byte costs:
data has streamed within an entry, or finished entries with data had sizes at least twice apart.
It is unknown once no
report has come for longer than twice the longest pause between reports in the last minute
(`PAUSES`), five seconds (`RECENT`) at least, so that a slow link or disk that pauses as it
works keeps a time left; of such a stall only the usual pause counts for the pace, and a
stretch that skipped data is a pause. It is unknown, too, until the last three forecasts, a
second of work apart, agree within a quarter, each less the time since it was made (`ETA_SETTLE`,
`ETA_AGREE`), or the job has worked half a minute since its first report (`ETA_SETTLE_MAX`), as
early forecasts know little of the pace. A job that counts after some work, as a move does
before it copies what it could not rename, starts the speed and the time left over
(`JobView::scanning`). Both are
worked out once in each second that the time shows and held until
the next, so they change with it rather than with every frame. The
time left shown follows the end that the forecast says, smoothed over 9% of the time left, from
a second to half a minute (`ETA_SMOOTH`, `ETA_SMOOTH_MAX`), which a forecast that is right does
not move (`count_down`); 9%, so that following a forecast that stands still, it lags no further
than the tenth beyond which it moves halfway. It counts down by the clock, a second each
second, while it keeps within 1% of the forecast smoothed (`ETA_NEAR`), and else shows the
nearer edge of that band, so that it holds still where the forecast does rather than go up and
down; beyond a tenth from the forecast (`ETA_SLACK`), it moves halfway there each second, and
more than twice as long, or less than half (`ETA_JUMP`), it is the forecast at once. It keeps its
fraction of a second and shows rounded.

Renaming one entry in its directory (`rename`) is no job: one call with no progress, which
the UI runs like a new directory, through the host's shared session (`Arc<SftpFs>`) for a
remote one. It returns `RenameError::Exists` with the kind of a taken name, and replaces a
file there only when asked.

A file system that ignores case, or Unicode normalization, as macOS's do by default (and
others: an SMB share, a host with such a disk), takes `Name` for `name`: a name spelled
otherwise leads to an entry that its directory lists under another. A name is the entry's own
spelled otherwise (`spelling.rs`) when its directory does not list it byte for byte and it is
the same object: by device and inode locally; over SFTP, which tells neither, an entry of the
same directory with the same metadata (kind, size, permissions, time, and owner). A file system
that tells case apart lists every name byte for byte, so none of this happens there. Renaming to
the entry's own name spelled otherwise gives it that spelling; one that leads to another entry
finds the name taken.

Deleting counts the entries first, so that progress has a total, then removes the deepest
first. Listings report symlinks without following them, so a link goes and its target stays.
A directory in which something stays (skipped, or unreadable) is left alone without asking
again, and an entry that is already gone counts as deleted.

Copying works between any two backends (`copy` takes an `Endpoint` for each side: a `Vfs`
and how its paths are reported). If the target is a directory, the sources go into it under
their own names; otherwise a single source becomes the target, and several make it a new
directory. Directories merge into a directory of the same name; symlinks are copied as
symlinks, with their targets as stored; FIFOs, sockets, and devices fail. The job counts
entries and bytes first, then copies parents before what they hold, reporting progress as the
data of each chunk arrives. With `preserve`, copies get the modification times and permission
bits of their sources, and directories that the copy made get theirs once the copy is past the
last entry within them, the deepest first, since writing into a directory changes its time
([ADR 0053](adr/0053-a-directory-is-finished-once-all-in-it-is-done.md)): among the entries,
where the time left counts it, and before an abort; directories that were there keep their own.
With `atomic`, each file is written under a hidden temporary name next to its target
(`.name.noc-PID-N`) and renamed when complete, so the target never holds part of a file; its
failures name the target, not the name the user never sees. Without it, the target is written
directly. Either way, a file that does not finish (an error, Skip, or cancellation) is removed;
written directly over an existing file, that file is gone too. Cancelled while a host opens it, a
temporary file goes as well, once made; a target written directly stays, empty, if the host does
not open it within the two seconds a cancelled job has, since it may be one that another hand
made just then.

When the name of a file or symlink is taken, the job sends `Exists` with the metadata of both, for
sizes and times, and waits for a `Conflict`: Overwrite, Skip, Overwrite all, Skip all, Overwrite
older (every later one that is older than its source; unknown times keep the target), or Abort. With
`overwrite`, the job replaces without asking, as when an edited file goes back where it came from. A
directory where a file would go is a failure, not a question. A symlink in the way is removed first,
so that a file written directly never goes through it; renaming replaces it anyway. Without
`posix-rename`, the target is removed before the rename.

A copy never goes over its source, which writing it directly would empty first: a target that is
the source by device and inode, under a name spelled otherwise, through a directory spelled
otherwise on the way, or a hard link, is a failure (`VfsError::SameFile`), not a question, and so
is a directory that is its own source to go into. Within one file system (`copy_within`, which
the UI runs for a copy on one host or on the local file system), one source to its own name spelled
otherwise is that failure too, before anything is counted, over SFTP as well. Over SFTP the
directories on the way are taken as typed: a copy into its own directory spelled otherwise there
asks, as for any taken name, and its temporary file keeps the source whole, unless
`atomic_upload` is off.

Moving places sources as copying does. Between file systems (local and a host, or two
hosts), it is a copy with `remove_sources`: each file or symlink goes from the source once
it is copied, and each directory once everything in it has gone, as soon as the copy is past
it, so whatever is skipped or fails stays, with the directories around it. Within one file
system (`move_within`), each source is renamed, after the same question if its name is taken
by something a rename replaces; a directory whose name is taken by a directory merges into it
by copying and removing, and so does anything the rename refuses as crossing file systems
(`EXDEV` locally, or a plain failure over SFTP v3, which has no code for it). One source to its
own name spelled otherwise is renamed to that spelling, without a question and without a copy, a
directory too, where it would otherwise go into itself; a target that is the source by device and
inode is a failure, since a rename onto itself does nothing. Moves keep times and permissions.

Checksums (`Checksum`) hash files with SHA-256, SHA-512, SHA-1, MD5, or BLAKE3 (RustCrypto's
`sha2`, `sha1`, and `md-5`, and `blake3`). A job may span several endpoints, such as a local
file and one on a host to compare: it scans every group of targets first, so that progress
has one total, then hashes them in turn. Symlinks among the targets are followed; inside
directories, symlinks to files are hashed and other symlinks left out, so that a walk never
loops, and FIFOs, sockets, and devices are left out without being opened, since reading a
FIFO would wait for a writer. Directories are walked in the order of their names, and each
file is named relative to the targets' directory (`dir/sub/file`), as `sha256sum` would name
it from there. Files are read as streams through the `Vfs`, over SFTP too, and hashed off the
async thread (`spawn_blocking`) in batches of 256 KiB while the next batch is read. A failure
asks as other jobs do: Retry hashes the file from its start, and a skipped file has no
checksum. The job returns its sums with its outcome; an aborted one shows none.

## Configuration and paths

Noon Commander uses the XDG layout on every platform, including macOS, and respects the `XDG_*`
variables:

| Purpose | Path |
| --- | --- |
| Settings, keymap, themes | `~/.config/noc/` (`config.toml`, `hosts.toml`, `keymap.toml`, `themes/`) |
| Data (workspaces, bookmarks) | `~/.local/share/noc/` (`workspaces.toml`) |
| State (`history.toml`, `recent.toml`, last directories, logs) | `~/.local/state/noc/` |
| Cache (`ssh -G` results) | `~/.cache/noc/` |
| Runtime (control sockets, askpass socket, F4 temp files) | `$XDG_RUNTIME_DIR/noc/` or `$TMPDIR/noc-$UID/`, mode 0700 |

Unknown keys are errors, so a typo does not silently fall back to a default. `noc config init`
writes the commented defaults, and a commented `hosts.toml` unless one exists. The
Configuration dialog writes only the keys it changed, through `toml_edit`, keeping comments and
the other keys; the result is checked against the schema and replaces the file atomically
([ADR 0009](adr/0009-configuration-dialog-writes-config-toml.md)).

```toml
[ssh]
program = "ssh"                  # name in PATH or absolute path, OpenSSH 8.7+
config_file = "~/.ssh/config"    # optional: passed as -F, also drives host discovery
args = ["-o", "ServerAliveInterval=15"]
multiplex = true                 # false: one connection per channel

[discovery]
hide = ["github.com", "gitlab.com", "bitbucket.org"]

[volumes]
hide = ["/Volumes/Backup*"]      # mount points to leave out of the location menu; never the system volume

[ui]
language = "auto"                # or a language tag such as "en-US"; others fall back to it
formats = "auto"                 # dates, numbers, order of names: the system's; "iso"; a tag
natural_sort = true              # names as the language orders them, file2 before file10
theme = "mc-classic"             # "terminal", "noon-dark", "noon-light", "catppuccin-mocha", …
borders = "double"               # frames of panels and dialogs: ═ ║ ╔; "single": ─ │ ┌
icons = true                     # Nerd Font icons; false: mc's markers (/ * @ ~ …)
show_hidden = true               # names that start with a dot; o h switches while running
default_search_mode = "fzf"      # how fields that search open first; "plain"; "regex"; Ctrl+s s
menu_bar = "on-demand"           # the F9 menu bar while a menu is open; "always": above the panels
tab_bar = "line"                 # tabs on a line above the panels; "frame": in the panel's frame
terminal_title = true            # the window's title shows the active panel's directory
which_key = true                 # after g, Ctrl+w, …: the keys that can follow, as which-key
which_key_delay_ms = 500         # how long after such a key the hints show

[animations]
windows = true                   # frames of windows grow as they open and shrink as they close
spinners = true                  # the spinner in front of what waits turns; false: a still mark

[transfer]
atomic_upload = true             # copies go to a hidden temporary name, then are renamed
parallel_jobs = 2                # jobs that run at once; later ones wait; F4 never waits

[zoxide]
program = "zoxide"               # name in PATH or a path
record = true                    # add directories where the user did something

[shell]
run = ["$SHELL", "-i", "-c"]     # runs ! and :!, the command last; $NAME where it runs
history_size = 500               # commands of ! and : in history.toml, of all hosts together
pause = "always"                 # output waits for a key: "always", "on-error", or "never"

[search]
program = "rg"                   # ripgrep for f /, name in PATH or a path; rg on hosts
fd = "fd"                        # fd for f f, files and dirs, likewise; fd or fdfind on hosts
max_results = 10000              # it stops after finding so many lines, or files and dirs
max_per_file = 100               # and takes no more lines of one file
```

Preserving attributes is a choice in the copy dialog, as in mc, not a setting.

Host settings live in `hosts.toml` next to `config.toml`, one table per host with a required
`type` that decides its other keys ([ADR 0007](adr/0007-typed-host-settings-in-hosts-toml.md)).
F4 on a host in the location menu rewrites only its table through `toml_edit`, keeping comments
and the other tables, and replaces the file atomically. Its field of `run` shows `shell.run`,
faint, while it is empty (`Dialog::with_placeholder`), since that is what counts then; a
placeholder is never the field's text. The settings sit behind a lock in the
shared `Context`, so the TUI and the host tasks see a change at once.

```toml
["prod-web"]                     # an ssh_config alias; decorates it, never duplicates it
type = "sftp"
label = "Prod"
start_dir = "/var/www"           # opened on connect instead of the remote home
other_dir = "~/projects/site"    # the other panel opens it with the host; / or ~/ for now
remember_dir = true              # reopen the last directory of this session
run = ["$SHELL", "-c"]           # runs ! there instead of shell.run; $SHELL is the host's
```

Workspaces live in `workspaces.toml` in the data directory ([ADR 0017](adr/0017-workspaces.md)).
Noon Commander writes the whole file on each change, behind a header that says so; unknown keys,
a blank or repeated name, a side without tabs, or two current tabs on a side are errors, and a
file with one is left alone.

```toml
[[workspace]]
name = "noon"
active = "right"                 # the side with the keys

[[workspace.left]]
location = "~/src/noon"          # /…, ~, ~/…, or host:path; root and sftp read as ~
cursor = "Cargo.toml"            # the entry under the cursor
current = true                   # the tab that shows; else the first

[[workspace.left]]
location = "web:/var/www"
sort = "time"                    # name (the default), extension, time, size
descending = true

[[workspace.right]]
location = "/"
```

The places used last live in `recent.toml` in the state directory, for the
[location menu](#location-menu). Noon Commander writes the whole file on each change, behind a
header that says so; deleting it only forgets their order.

```toml
hosts = ["prod-web", "db"]       # aliases, most recent first, up to 50
volumes = ["/Volumes/USB", "/"]  # mount points, the same way; only those that are UTF-8
```

## UI

- **Keys.** The keymap is vim's, in its modes, with mc's F-keys ([ADR 0022](adr/0022-vim-modes.md));
  see the keymap below. The panels take keys in Normal mode, as vim's buffers do; text fields, quick
  search, the command line, and the filters of the lists in Insert mode. The line above the F-key
  bar shows on the left the mode, in bold capitals as lualine shows it, `NORMAL`, `INSERT`,
  `VISUAL`, or `VISUAL LINE`, and after Ctrl+o `(insert)`; then, after `·`, the kind of window
  that has the keys, named by its keymap context so that it says which keys work
  ([ADR 0032](adr/0032-line-of-the-mode.md)):
  `Quick search`, `Rename`, `List` for the location menu, the tab menu, the zoxide window, the
  workspaces, the command history, and the list of the key hints, `Menu` for F9, `Dialog` for
  every dialog, the Configuration dialog, the search of a tree, the jobs, and the checksums,
  `Help`, and `Viewer`; `window_kind` maps every context, and none to the contexts that are
  only fallbacks, such as `search_input`. The panels show the mode alone. Where
  the line is short the kind goes first, then the mode is cut. On the right it shows the count,
  register, operator, or keys of a command typed so far, as typed, Ctrl+x as `^X`, or how much
  Visual mode selects, as vim's `showcmd` does; while the command of Ctrl+x has the keys, the mode
  starts with `(^X)`, as vim's `(insert)` after Ctrl+o. Unlike vim's `showmode`, it shows every mode
  everywhere, Normal mode and quick search too. A message, such as that a search went round, takes
  the place
  of the mode until the next key, as vim's messages do on its command line. Left of the cells of
  what is typed it shows who is where in the active panel, `alice@mac` or `deploy@web`, as long
  as it fits after the mode or the message ([ADR 0031](adr/0031-who-and-where-on-screen.md)). The
  command line takes the line while it has the keys; a window over it, such as the command
  history of `q :`, takes the line back. The cursor is a bar in Insert mode and a block in the
  others (DECSCUSR), and the terminal's own shape comes back whenever another program gets the
  terminal, and on quitting.
- **Panels.** Each panel lists a directory with `..` first (but in a `/`), then directories,
  then files, by name as the language of the formats orders names, with the numbers in them by
  value (`ui.natural_sort`; off, by their characters ignoring case); columns are name, size, and
  modification time (local time: the date, with its year, and the time, in the formats of the
  region, as `12.10.26 05:05`), and narrow panels drop the time, then the size. Sizes have a
  unit and a decimal below 100 (`512B`, `78.5K`, `503G`), the unit from M on bold, from G on in
  a color too, as the theme's `size_units` say; the status line shows every byte of the file
  under the cursor at its right, while the name keeps 8 cells. The size column is as wide as the
  longest size and the words `UP--DIR` and `DIR` (7 cells in English); the time column as the
  longest time of the formats. Each panel has its own sort order:
  name, extension, modification time, or size (`o n`, `o e`, `o m`, `o s`);
  time and size start newest and largest first, the same key again reverses, ties go by name,
  and an arrow in the header marks the order. Directories stay first. Names that start with a
  dot are shown unless `ui.show_hidden` is off; `o h` switches them in both panels.
  Sorting and hiding keep the cursor on its entry.
- **Names that do not fit** ([ADR 0025](adr/0025-marks-of-text-cut-short.md)) lose their middle to
  `»` (a Nerd Font chevron with icons, `Theme.glyphs.marks`), in the theme's bold accent
  everywhere (`truncated`; `truncated_cursor` under the cursor, `truncated_dialog` in dialogs and
  lists, `truncated_menu` in the menus of F9, `truncated_bar` on the bars, `truncated_error` in
  error dialogs), and paths their start, as titles do (`«/deploy/site`); `noc_text::fit_cut` says
  where the mark goes, and `cells::fit`, `cells::fitted`, and `cells::spans` draw it as spans.
  `K` shows the whole name under the cursor in a frame of two rows below the panels, single lines
  in the colors of dialogs, following the cursor until `K` or Esc (`App.whole`, not saved). The
  z keys scroll the names of all rows as vim scrolls lines that do not wrap (`Panel.shift`, back
  to 0 in another directory): `z h`/`z l` a column, `z H`/`z L` half the column, `z s` the
  start, `z e` the end of the name under the cursor into view; `z l` and `z L` stop with the end
  of the longest name on screen at the right edge (`Panel::shift_limit`), and each render brings
  the scroll back as far as the names on screen need. Only the names scroll: the icon or marker
  stays in front, and a scrolled name shows `«` over its first cell after it and `»` where it is
  cut.
- **Scrolling.** The rows of a panel and of the lists scroll as vim 9.1 scrolls a window
  (`tui/scroll.rs`, a port of vim's `move.c` for lines one row high, checked against vim on random
  keys): five rows stay above and below the cursor, as vim's `defaults.vim` sets 'scrolloff'
  (`scroll::SCROLLOFF`, no setting), fewer at the ends and in a low window; a jump far off the
  screen puts the cursor in the middle. Ctrl+d and Ctrl+u scroll half a window and move the cursor
  as far, Ctrl+f and Ctrl+b scroll a window less two rows, with a count that many windows at once,
  and put the cursor at its top or bottom. Unlike vim's, Ctrl+f stops with the last row at the
  bottom, and once it shows, puts the cursor on it, as mc's PgDn does. The wheel scrolls as vim's
  Ctrl+e and Ctrl+y, but never past the last row either; where it cannot scroll, as when the rows
  all fit, it moves the cursor as far, as in mc and Far. The view is worked out before each frame,
  as vim does it before it redraws. When the rows do not fit, a scroll bar runs down the right of
  the frame along them, as in Far ([ADR 0024](adr/0024-scroll-bars-on-the-frames.md)).
- **The cursor of the inactive panel.** Unlike mc, the inactive panel shows where its cursor
  is (the checksum comparison reads the file there): its row gets a background of its own
  under the row's colors, which stay (a mark too). The theme sets it as `cursor_inactive`:
  the dark gray of the 16 colors in `mc-classic`, dimmed reverse video in `terminal`, the
  dialogs' `surface0` in the Catppuccin themes, and in the Noon themes the text fields' blue
  (`#2D4672`) in `noon-dark`, as the dialogs' one is a single step of gray above the panels in
  256 colors, and the dialogs' blue-gray (`#E8ECF4`) in `noon-light`.
- **The cursor where the user is root.** In a local panel while Noon Commander runs as root, and
  in a panel on a host whose ssh user is root, the cursor is on red: `root_cursor` and
  `root_marked_cursor` take the place of `cursor` and `marked_cursor` in the theme that panel is
  drawn with ([ADR 0031](adr/0031-who-and-where-on-screen.md)).
- **Free space.** As in mc, the bottom of a panel's frame shows the free space and size of the
  file system that holds the directory, and the share that is free: `123G / 500G (24%)`. It is
  read with every listing, so it changes when a job reads the panel again or on `R`; a panel
  on another directory of the same file system keeps what it read last. Locally it waits at
  most as long as a volume in the location menu, so a dead network mount leaves it out; over
  SFTP it needs the `statvfs@openssh.com` extension, which OpenSSH's server has. A file system
  that reports no size, or a panel too narrow for it, shows none.
- **Total of the directory.** At the left of the same line, the size of the files shown and how
  many files and directories are shown: `12M in 45 files, 3 dirs`, the size as the Size column
  writes it. Hidden files count while the panel shows them, `..` does not, and a link to a
  directory is a directory. A panel on a host that is not connected shows none. Where the panel
  is too narrow for both, the free space is left out first.
- **Targets of symlinks.** With a symlink under the cursor, the status line shows its target as
  stored in the link, as mc does: `config -> /etc/app/config.toml`, read with the listing; with
  `ui.icons` a Nerd Font arrow (nf-md-arrow_right) takes the place of `->`. The name stays plain,
  as its row above already shows its kind; the arrow is toned down as an icon; the target has the
  color of its kind (`directory`, `file`, `device`, `special`, `stale_link` where nothing is
  there; never `executable`, as the target's permissions are not read). The target tells its
  kind too: with icons by the icon of a directory, a file (devicons' by its last name), or a
  broken link in front of it, in its color toned down; without them by
  mc's marker after it where `ls -F` puts one, `/opt/`, and `!` after a broken one. A target too
  long loses its start, as a path in the title does, its icon and marker kept; with too little
  room for it the name shows alone.
- **Marks.** Space or Shift+Down marks the entry under the cursor, or unmarks it, and moves down,
  as mc's Insert does (Shift+Up moves up), and a count marks that many, the row at the end of the listing
  once; `g *` inverts the marks on files, leaving directories as they are; `..`
  cannot be marked. Marked rows are underlined, and yellow in mc-classic (bold, which mc uses
  without colors, is for directories); the line below the listing shows the size of the marked files
  and how many entries are marked, such as `12,345 B in 3 files`. Marks are names, so they survive
  sorting and `R` (for names still there); another directory starts unmarked, and entries that get
  hidden lose their marks, so that no operation acts on what is not shown. `g +` marks and `g -` unmarks
  the names that match a shell pattern, in a dialog with mc's option Files only (off). Patterns are
  read as mc reads them: `*`, `?`, `[a-z]` (`[!…]` or `[^…]` outside the set), `{a,b}`, and `\`
  for the next character as it is; the whole name must match, and what does not parse is literal.
  The keys after Ctrl+s take the pattern as text or as a regular expression too, and switch its
  case, smart at first, in place of mc's Case sensitive ([search modes](#search-modes)). The dialog
  opens with the last pattern (`*` at first), its way and case, and Files only.
- **Visual mode.** `v` or `V` selects rows from the one under the cursor to wherever the cursor
  goes, as vim's Visual mode selects lines, and with a count as many rows (`3V`), or, after a
  command of Visual mode, as many as it took times the count, as vim's `resel_VIsual`; `o` goes to
  the other end, `g v` selects the rows selected last again, and Esc ends it, as does the key of `v`
  and `V` that started it, while the other switches its kind, as vim's; after `V` the line of the
  mode says `VISUAL LINE`. Space marks the rows selected; `y`, `x`, `d`, `D`, `X`, F5, F6, F8,
  `f #`, and `g x` make them the marked entries and act on them, as on any marks, and put the
  cursor on the first row, as vim's operators do. The line of the mode shows how many entries it
  selects, without `..`. The other F-keys, Ctrl+w to the other panel, and the commands of the
  pull-down menu end Visual mode and do what they do in Normal mode, and so does the panel losing
  the keys to the other one or to another tab. The selection is held by the entries on its ends,
  so that a listing of the same directory or another sort order keeps it on them.
- **Jumps.** Each tab keeps a jump list, as vim does with `jumpoptions=stack`: going to another
  directory (Enter, `h`, Go to directory, a menu, zoxide, a workspace's tab), `G`, `g g`, `n`, `N`,
  and a quick search that found a match put where the panel was on it, up to 100 places, even when
  they stay on the row, as vim's; a jump after Ctrl+o keeps the place Ctrl+o went to, and drops
  those after it. `n` with a count puts it there once, and one that finds nothing, never; nor does a
  directory that cannot be read. Ctrl+o goes back, and Ctrl+i forward again (Tab too without the
  kitty keyboard protocol), with a count too, without changing the places, as vim's; after a jump
  that stayed on the row, the first Ctrl+o stays there too. A count past the end of the list goes
  nowhere and keeps it as it is, as vim's; a place in this directory moves the cursor at once,
  another one reads its directory.
- **F7 makes a directory**, as in mc: the dialog opens with the name under the cursor, in Insert
  mode with the cursor after it; Ctrl+u clears it. The name may be a relative path, an absolute
  one, or start with `~` for the home directory (the remote one on a host; `\~` for a name that
  starts with `~`); missing parents are not made. The directory is made in the background,
  locally or by the host's task, and panels on the directory it is in read it again, the one
  that asked with the cursor on it. An error shows in a red dialog, as mc shows errors.
- **F5 copies** the marked entries, or the one under the cursor, as mc does: a dialog asks
  where to, opening with the other panel's location (`host:/path` for a host), and whether to
  preserve attributes (times and permission bits; on, and remembered). F5, F6, and Shift+F5
  share the dialog (`tui/transfer.rs`, [ADR 0043](adr/0043-one-dialog-for-copy-and-move.md)):
  the same field and check box, only the words differ. A typed target is `host:path` for a
  host the app knows, or a path from the active panel's directory, as F7 takes it. A target
  that is the source directory, a source, or in one of the sources, is an error; so is, from
  the job, a source under its name spelled otherwise, where the file system ignores case (see
  File operations).
  The job's window shows entries and bytes, with the gauge on the bytes. A taken name asks in
  red, as mc does, with the path, both sizes and times, and Yes, No (the default), All, None,
  Older, and Abort. Copies are written under a temporary name and renamed when complete,
  unless `transfer.atomic_upload` is off.
  When the job ends, panels on the target, its parent, and the source directory read them
  again.
- **Shift+F5 copies the entry under the cursor** (`y p`, as vim's `y y p` puts a copy of a
  line under it), not the marked ones, which stay marked, as mc's and Far's Shift+F5: the dialog
  of F5 opens with its name, in Insert mode with the cursor where Shift+F6 puts it, before a
  file's last extension, so that the copy goes beside it under the name typed (`report
  (1).pdf`). A path or `host:path` there copies elsewhere, as with F5; the name unchanged is an
  error, the source itself; a name that is not UTF-8 cannot be edited, which an error says: F5
  copies it. In Visual mode and in quick search, Shift+F5 ends them first, as the other F-keys
  do.
- **F3 views** the file under the cursor, as mc does (on a directory it opens it): the viewer
  (`noc-viewer`) takes the screen, with the path and the position (first line, lines, and how far
  the last line on screen is, or `End` once the end shows) on top, a scroll bar on the right when
  the text does not fit, and its own F-key bar. It reads the file in parts of `viewer.part_mib`,
  locally or through the host's shared session, in the background (closing the viewer stops
  that), each ending after its last line feed, the next when a move down shows the end of what it
  holds, and keeps a window of `viewer.window_mib`, reading a part it dropped again when a move up
  reaches it ([ADR 0038](adr/0038-viewer-reads-a-window-of-the-file.md)). Until the end is known
  the position is `line/? percent%` by bytes, the scroll bar goes by bytes, and `more below` ends
  the text. A read longer than 150 ms shows the `Reading` dialog (`tui/reading.rs`): where the
  part starts, a gauge, Stop (Esc), which keeps what was read, and Close. A file whose size or
  time changes between parts is read again from the start. Text is UTF-8; invalid bytes and
  what the terminal would act on show as `·` in the theme's faint `hidden`, as mc's `.`
  (`Theme.glyphs.hidden`); tabs go to stops of 8, and
  `\r\n` ends lines.
  Long lines wrap, each row after the first starting with `»`, as vim's 'showbreak' (F2 cuts
  them, `«` and `»` mark where they go on past the screen, as vim's 'listchars', and Left and
  Right, `h`, `l`, and the z keys as vim's scroll then, no further than the end of the longest
  line on screen at the right edge, and back as far as the lines on screen need after paging or
  widening); the position is a line and a row within it, so that only the lines in view are
  wrapped, whatever the size of the file. Keys follow vim and less: `j`/`k`, Ctrl+e/Ctrl+y, and
  the arrows a line, Ctrl+f/Ctrl+b and PgDn/PgUp a page but for the two rows that vim keeps, Space/`f`
  and `b` a whole page, as less, Ctrl+d/Ctrl+u and `d`/`u` half a page, rounded down as vim's
  'scroll'; unlike vim's, every key down stops with the end at the bottom, where a dimmed line
  across the text, `──── EOF ────`, follows the last line (`noc-scroll` has
  the steps of both and the scroll bar, [ADR 0023](adr/0023-paging-stops-at-the-end.md)); `g g`/`G`
  and Home/End the ends, and F3, F10, `q`, or Esc to close; F1 or `g ?` shows the help over
  it.
- **F4 edits** the file under the cursor in `$VISUAL`, else `$EDITOR`, else `vi`, split at
  spaces (`code -w`), without a shell. A local file is edited where it is. A remote one is
  copied, with its times and permission bits, to `edit-PID-N-name` in the runtime directory
  (the name last, so that the editor knows the kind of file); when the editor exits and the
  copy's size or time changed, it goes back over the original by the copy job, with
  `overwrite` and as `transfer.atomic_upload` says, keeping the original's permissions. Both
  copies show the job's window. The local copy is removed once it went back, or at once if
  it did not change; one that did not go back (a failure, Abort, or the host gone) stays, and
  a red dialog says where. Panels on the file's directory read it again.
- **`g x` opens externally** the marked entries, or the one under the cursor, as vim's `gx`
  opens what is under its cursor: each with the program the desktop has for it, as a double
  click would, through `open` on macOS and `xdg-open` on Linux, one after another
  (`noc_tools::opener`). A directory opens in the desktop's file manager. The opener runs
  without a shell, input, or output, in a process group of its own, and is never killed: a
  program it starts runs on after Noon Commander quits. One still running after five seconds
  counts as having opened its file. The first failure stops the rest and shows in a red dialog,
  with the exit code, since the opener's output goes nowhere. Remote entries do not open yet:
  on a host, the line of the mode says that only local files open externally, and the command
  is dimmed in the File menu.
- **F6 moves or renames** the marked entries, or the one under the cursor, with the dialog
  of F5 (`Move "x" to:`). Preserve attributes, shared with copies, applies where a move copies;
  a rename keeps them anyway. A new name in the
  field renames in place, as in mc, its own name in another case too, where the file system
  ignores case. Locally, and within one host, the job renames (`move_within`); between the
  local file system and a host, or two hosts, it copies and removes each source once all of it
  is copied. Its window says Moving; when it ends, the
  panels on both sides read their directories again.
- **Shift+F6 renames in place** the entry under the cursor, not the marked ones: its
  row becomes a text field from the name on, over the size and time columns, in Insert mode
  with the cursor before a file's last extension (after the whole name of a directory, a
  dotfile, or a name that ends with a dot), so that Ctrl+u takes away the rest of the name.
  Enter renames in every mode; Esc goes to Normal mode, where vim's motions and operators edit
  the name, and Esc there keeps the name; Ctrl+c keeps it in every mode; other panel keys do
  nothing (`Context::Rename`). The status line stays as it was, with the old name and its size.
  An empty or unchanged name
  renames nothing; one with `/` or NUL, or `.` or `..`, is an error and the field stays. A name
  that is not UTF-8 cannot be edited, which an error says: F6 renames it. `noc_ops::rename`
  checks the new name first: a directory's is an error, a file's asks in a red question with No
  as the default, and the entry's own name spelled otherwise, differing only in case or
  normalization where the file system ignores them (see File operations), renames it; spelled
  so, another entry's name is taken. Without `posix-rename` the file there goes first. A reload
  keeps the field on its entry; the field goes with the entry or when the panel goes elsewhere.
- **F8 deletes** the marked entries, or the one under the cursor, after a red question with Yes as
  the default, as in mc: `Delete file "x"?`, `Delete directory "x" and everything in it?`, or
  `Delete 2 files and 1 directory?`. mc asks a second time before it goes into a directory that is
  not empty; Noon Commander says so in the first question instead. The job runs in a task of its
  own, or in the host's task, with a window that shows what it counts, the entry at hand, a gauge,
  and done/total; Esc or Abort stops it. A failure asks in a red dialog, with mc's buttons: Ignore,
  Ignore all, Retry, and Abort. When the job ends, panels on the directory read it again, and a
  cursor whose entry is gone stays on its row. If the host's connection is lost, the job ends with
  it.
- **Jobs.** A job's window has Background, the default, and Abort: Enter sends the job
  behind the panels, which take the keys again, and Esc aborts, as in mc. As many jobs run
  at once as `transfer.parallel_jobs` says (2); a later one waits, in front or behind, and
  starts when one ends, the oldest first. Abort takes a waiting job away at once. The jobs
  of F4 never wait, but count while they run. `f j` lists the jobs, as mc's Background
  jobs: a row for each, with what it does, how far it is (a percentage, `counting`,
  `waiting`, or `aborting`), and the entry at hand, and Show (the default), Abort, and OK.
  Show brings the selected job to the front, in its window; the selection follows its job
  while jobs above it end, and the next row once it ends itself. Questions from jobs
  behind the panels open over whatever is on screen, in turn with other dialogs, and the top
  right corner, where Far has its clock, says how many run and how far they are together
  (`2 jobs 37%`, the mean of their gauges). The jobs of F4 stay in front, so that the editor
  does not open in the middle of other work. F10 asks before quitting while jobs run;
  quitting stops them and waits up to five seconds, so that a copy removes its unfinished
  file before the connections close.
- **Checksums.** `f #` (not in mc) asks for an algorithm (radio buttons, the last one
  chosen first, SHA-256 at first) for the marked entries, or the one under the cursor; files
  in directories count. For one file, the dialog has a field for the checksum it should have,
  pasted from a download page or a line of `sha256sum` (the first word counts, in any case);
  hex of another length picks the algorithm that has it, and anything else is an error. If
  the other panel's cursor is on a file, a check box compares the two (checked when they have
  the same name), which hashes both. The job has the usual window, Background, and list.
  When it ends, a window shows the checksums: with several files, a list with each checksum
  shortened in the middle and a mark (✓, ✗, or `-` for skipped), and under it the selected
  file's whole checksum; then the verdict (matches or not, the same or different), in color.
  Copy puts the selected checksum on the clipboard, Copy all every line in the format of
  `sha256sum` (`<hex>  <name>`; names with a backslash or a line break escaped as GNU
  coreutils does), and Save… writes those lines to a file in the panel's directory, locally
  or on the host (`name.sha256` for one file, `SHA256SUMS` for several; `b3`, `md5`, …
  for the others), after asking before it replaces one. Windows of jobs that end while one
  is open wait their turn.
- **Clipboard.** Copying goes through OSC 52: the event loop writes the escape sequence
  between frames, and the terminal puts the text on the clipboard of the machine it runs on,
  over ssh too. Nothing tells whether it did, so the UI says the text was sent to the
  terminal's clipboard. Noon Commander never reads the clipboard
  ([ADR 0008](adr/0008-clipboard-through-osc-52.md)).
- **Terminal title.** With `ui.terminal_title` (the default) the event loop titles the
  terminal's window or tab with OSC 0 between frames: the active panel's directory
  (`~/src — noc`, `host:path` with the host's label), after `[root]` while Noon Commander runs as
  root or the active panel is on a host as root, sanitized like every name. Before the first
  title it saves the terminal's own on xterm's stack of titles (`CSI 22 t`) and takes it back from
  there (`CSI 23 t`) on quitting and before handing the terminal to another program, which gets the
  shell's title and may set its own.
- **The other panel.** As vim's windows: Ctrl+w w, Ctrl+w Ctrl+w, or Ctrl+w p makes the other
  panel active; Ctrl+w x or Ctrl+w r swaps the panels, as mc's Ctrl+u, and the active one stays
  active on the other side; Ctrl+w f opens the directory under the cursor in the other panel, as
  mc's Alt+o (from a file, the parent directory with the cursor on this one) and moves the cursor
  down; Ctrl+w v shows this directory in the other panel with the cursor on the same name, as
  mc's Alt+i. Panels keep their identity when swapped (only where they are drawn changes), so a
  listing still in flight reaches the panel that asked for it. Sort order and errors go with the
  panel; mc keeps the sort order on its side. Ctrl+w h and Ctrl+w l make the panel drawn on that
  side active, whichever it is after Ctrl+w x. Keys act on the active panel; Ctrl+x runs the next
  command in the other panel, then gives the keys back, as vim's Ctrl+o in Insert mode runs one
  command of Normal mode ([ADR 0051](adr/0051-ctrl-x-sends-a-command-to-the-other-panel.md)). It
  comes with the command, as a register does (`Extra`), with the count and the register after it
  (`Ctrl+x 2g t`); those typed before it go, and Esc and Ctrl+x again give it up. `App::typed`
  makes the other panel active for the command and keeps where the keys were (`returns`), so any
  command works there as in the active panel, those of the cursor, the marks, and the files too,
  and needs nothing of its own. `App::come_back` gives the keys back once the panel has them in
  Normal mode with nothing in front and no program to hand the terminal to: after the key, or, as
  the event loop asks before each frame, once the mouse closed its window or the program it ran
  returned. Windows of a panel open centered over both, as every window does; the panel drawn
  active, after Ctrl+x the other one, says which they act on.
- **Quick search.** `/` starts quick search, typed as vim's search, in Insert mode, and `?` starts
  it up the rows; typing in a panel in Normal mode does nothing but its commands, and `i`, `!`, and
  `:` open the [command line](#command-line-of--and-). As vim's search, it looks from the row after
  the one where it started, round, or after `?` up from the row before it, round to the bottom, and
  a count before `/` or `?` goes to the match of that number (`2/`). It matches in the
  [search mode](#search-modes), with smart case, which the keys after Ctrl+s, or a click on a glyph
  at the right end of the status line, switch while it runs. As fzf, the default, the cursor
  jumps to the best match as fzf ranks it, the first after where it started of those as good, and
  Ctrl+g to the next best, round to the best; in the plain and regex modes, to the first name after
  where it started that holds the text anywhere, or that matches it as a regular
  expression, as the filters of the lists match, and Ctrl+g to the next one, round to the top, as
  many as the count. It runs as vim's incremental search (`panel::Search`, with vim's `search_start`,
  `match_start`, and `old_viewstate`): every edit of the text searches again from where it searches
  from, with the rows on screen as they were before it. As in mc, a character after which no name
  matches is not typed, and nothing moves; one that leaves a regular expression unfinished is
  ([ADR 0042](adr/0042-quick-search-types-only-what-matches.md)). While nothing matches, after
  a switch or with the last search brought back, the cursor stays where it searches from and the text
  as it is. Ctrl+g and Ctrl+t move where it searches from as vim's do, so that an
  edit after them searches on from the match before, and Enter stays on the match. The listing keeps
  its order. While it runs, every name on screen shows where it matches in the colors of what is
  found (`find_match`, as in the [search of a whole tree](#search-of-a-whole-tree)), the cursor's
  too: each place it holds the text or matches the regular expression, or the letters fzf matched
  (`Matcher::positions`), in what of the name shows. The text, kept as typed, is a field with a
  cursor, edited with the keys that every text field shares in Insert mode (`input`): Left,
  Right, Shift+Left, Shift+Right, Home, End (and Ctrl+b, Ctrl+e, as on vim's command line),
  Backspace, Delete, Ctrl+u, Ctrl+w, and Ctrl+r and a register; as on vim's
  command line, Delete at the end of the text deletes the character before, Shift and Ctrl with Left
  and Right go by what spaces part. Unlike vim's, Backspace and Delete with nothing typed keep the
  search open, as on the command line, so that one too many does not go to the parent directory. As
  in vim, Enter (and Ctrl+j) ends the search: it searches once more from where it searches from,
  with the count, and goes to the match, or says "Pattern not found", keeping the text for `n`
  either way; Esc or Ctrl+c (`abort`) gives it up and puts back the cursor and the rows on screen as
  they were when it started, keeping nothing for `n`. Ctrl+g goes to the next match down and Ctrl+t
  to the next up, after `?` too, as in vim. The arrows, the page keys, and the F-keys of the panel
  end the search and do what they do, as in mc; Shift+F7 goes to the next match. While it runs,
  every character is text, even one that a panel binds, such as `*`. A search that finds a match
  puts where it started in the jump list, as vim's, and so does one that finds none after Ctrl+g or
  Ctrl+t moved where it searches from. The app keeps what the last search found, whichever panel it
  ran in, as vim keeps its last pattern: `n` and `N` (and Shift+F7, mc's and Far's key to search
  again) go to the next and the previous name that matches it, round, the way it went and the other
  way, so up after `?`, and Ctrl+g with nothing typed yet searches for it again, as mc's Ctrl+s
  Ctrl+s does. As vim's, they say when they go round past the last row or the first ("search hit
  BOTTOM, continuing at TOP"), when nothing matches ("Pattern not found: name"), and that there was
  no search before, on the line of the mode until the next key; so does Enter in quick search, when
  its last search went round (a count can take it round too; after Ctrl+g or Ctrl+t it does not),
  but in the fzf mode, which ranks the rows rather than go down them. Long names lose their middle,
  marked with `»` in an accent. Names are shown terminal-safe: control and bidi characters, and
  marks that join the next character, become `?`. Listings run in background tasks; a reply
  carries the generation of its request, so a stale one is dropped. If a directory cannot be read,
  the panel stays where it was and says why below the listing. Going up puts the cursor on the
  directory just left. A panel shows a [`Location`](#locations), always a directory.
- <a id="command-line-of--and-"></a>**Command line of `!` and `:`** ([ADR
  0019](adr/0019-shell-command-line.md)). In a local panel, `i` or `!` opens a line above the F-key
  bar for a shell command, after a prompt as a shell's: `alice@mac:~/src $`, with `~` for home, and
  `#` where the user is root. It takes a third of the width at most: the middle of the directory goes
  first, then the user, then the directory. `:` opens it for commands of Noon Commander, of which
  `!command` is the only one so far. In a panel on a connected host the prompt has the user that
  `ssh -G` gave and the host's label or alias, `deploy@web:/var/www $`. The
  line opens in Insert mode, as vim's command line, where every character is text. Ctrl+j, or Enter
  after an odd number of `\` at the end of a line, starts a new line, marked `>`; long lines wrap,
  and the line grows up to a third of the screen and ten rows, then scrolls to the cursor. Up and
  Down move between lines, Home and End (Ctrl+b, Ctrl+e) go to the ends of the line. Tab completes
  the word before the cursor, as vim's `:!` does (see [Completion](#completion)). Ctrl+f goes to
  Normal mode where the cursor is, in place of vim's window of the command line: vim's motions,
  operators, registers, and undo edit the command there, Esc then leaves Insert mode, as in that
  window, rather than close the line, and Ctrl+c and F10 close it in any mode; `j` and `k` go
  through its lines, in the column of the first of them, or after `v` ended Visual mode, or after
  the end of the line where Ctrl+o left Insert mode, or at the ends after `$` and End, as vim's
  `curswant`, which the next Ctrl+o keeps, and then the history, which goes on from the command an
  undo brings back (in Visual mode only its lines), and `q :` opens the window of the history. Enter
  hands the command to the event loop, which suspends the TUI as for F4 and runs it with
  `noc-tools`' `Shell`: the words of `shell.run`, `$SHELL -i -c` by default, with the variables of
  Noon Commander's environment in them (`/bin/sh` for a program that comes out empty), then the
  command in one argument as typed, in the panel's directory; `-i` gives the shell the aliases and
  functions of its rc files ([ADR 0054](adr/0054-shell-run.md)). The shell runs as a job, as a shell
  runs one, and so does the editor of F4: `noc_tools::job` puts it in a process group of its own
  with the terminal, and `Running::wait` waits for SIGCHLD and asks `waitid` whether it ended or
  was stopped, then takes the terminal back (`tcsetpgrp` with SIGTTOU blocked, through `nix`'s
  `pthread_sigmask`), which zsh, which `exec`s the last command of `-c`, never gives back. One
  stopped by Ctrl+z gets `[stopped]` under its output, as a mark of a failure, and the keys of a
  dialog, which are read (`output::finish`): Enter continues it (`Running::resume`), Esc ends it
  with SIGHUP (`Running::end`), and after that kills it, `[stopped] Enter continue · Esc kill`,
  if it went on. A `Running` dropped before it ends kills all of its group. What ends Noon
  Commander while it runs goes on to it, as a shell passes SIGHUP on (`Running::pass_on`, from
  `output::finish`): SIGHUP, or SIGTERM, then SIGCONT, to its group, which has a second to end
  and is then let go (`Running::let_go`), not killed. On a host, the
  host's task builds the ssh command from its session (`Session::command_in`) and the event loop
  runs it with the terminal: over the master, with `cd -- '<dir>' || exit` and then `exec` and
  the words of the host's `run` in `hosts.toml`, else of `shell.run`, quoted but for each
  variable, which the host's shell expands as `"$NAME"`, then the command quoted, `'` and `\`
  outside the quotes after a `\`, as POSIX shells and fish both read them. What ends Noon
  Commander goes on to that ssh, which ends the session, so that the host ends the command.
  `tui/output.rs` shows the command after its
  prompt first, bold, and afterwards ends its output on a line of its own as zsh does (a dim `⏎`,
  spaces to the last column, `\r`, a cleared line); then a line in reverse video asks for a key,
  with the exit status of a failure, unless `shell.pause` says not to wait, and after the key `[exit
  N]` takes its place for a command that failed. The event loop then takes the terminal back; both
  panels read their directories again, as in mc. Esc, Ctrl+c, or F10 closes the line, as on vim's
  command line, in Insert mode and in Normal mode; Backspace on an empty line does not. `g <`, a
  `global` key, as vim shows the output of its last command, hands the terminal over without a
  program wherever it is pressed but in Insert mode: its own screen shows, with the output of the
  commands, until a key that the `user_screen` context binds to Cancel (`g <`, Enter, `q`, Esc), and
  then whatever was in front, a dialog, a menu, the viewer, or the line, is there again as it was.
  Shift+Enter starts a new line too where the terminal speaks the kitty keyboard protocol: at start
  the event loop asks for it with `supports_keyboard_enhancement` (in `spawn_blocking`, before the
  stream of events takes crossterm's reader) and then pushes `DISAMBIGUATE_ESCAPE_CODES` and
  `REPORT_ALTERNATE_KEYS`, so that Alt with a symbol typed with Shift, such as Alt++, arrives as
  that symbol (Ctrl+[, Ctrl+m, and Ctrl+i then arrive apart from Esc, Enter, and Tab, so the keymap
  binds them wherever it binds those). The preset lists the keys of such a terminal, where Tab in
  the panels goes to the other panel and Ctrl+i forward through the jump list; the app builds its
  keymap with `Keyboard::Kitty` there, and else with `Keyboard::Legacy`, which drops the bindings of
  Tab and gives Tab those of Ctrl+i, the key it arrives as. The event loop pops the modes, and turns
  bracketed paste off, whenever another
  program gets the terminal, on exit, and on a panic. With bracketed paste a paste arrives as one
  event: the command line takes it whole, line breaks included, and runs nothing; text fields, quick
  search, and the menus' filters take its characters without line breaks; panels, the pull-down
  menu, dialogs' buttons, and the viewer ignore it. Each command that ran, unless it starts with a
  space, goes to `history.toml` in the state directory with its host (the alias; none for a local
  one), directory, and time: `noc-config`'s `add_command` reads the file again, replaces the same
  command on the same host, keeps the newest `shell.history_size`, and writes it whole and
  atomically, mode 0600; one task makes the changes in turn, and the app keeps the commands in
  memory. Up on the first line of a command, or Ctrl+p, goes back through the commands of the
  panel's host, and Down on the last line, or Ctrl+n, forward to what was typed, as on vim's command
  line; the line of `:` shows them after `!`, which runs them. `q :` in a panel, or on the line in
  Normal mode, opens the window of the history (`tui/history.rs`), one of the [lists](#lists): the
  commands of the panel's host, or of all of them after Tab, newest first, filtered as quick search
  matches, each with its host's label or alias, its directory, and its first line, and the whole
  command under the cursor below; Enter puts it on the line, with the prompt reversed until the next
  key if it ran on another host, and F8, or `d d` on the rows, removes it.
- **Keymap.** The keymap is vim's, in its modes, with mc's F-keys ([ADR
  0022](adr/0022-vim-modes.md)): one preset, `tui/keymap/preset.rs`, maps keys to `Action`s per mode
  (`Mode::Normal`, `Insert`, `Visual`) and context (`global`, `panel`, `input`, `quick_search`,
  `rename`, `command_line`, `command_history`, `find`, `user_screen`, `list`, `location_menu`,
  `tab_menu`, `jump`, `workspaces`, `pull_down`, `dialog`, `help`, `dialog_input`, `path_input`,
  `completion`, `viewer`), in the order the help lists them. The app says which mode and context the
  next key goes to (`App::place`): the mode is that of the text field with the keys, Visual mode
  while a panel selects rows, and else Normal mode. Each context falls back along a chain in its
  mode (`Context::chain`), for example the help to the dialog, the text fields (`quick_search`,
  `rename`, `dialog_input`, `command_line`) to `input`, which holds the keys that edit text in each
  mode, and the windows that list items (`location_menu`, `tab_menu`, `jump`, `workspaces`,
  `command_history`) to `list`, which holds the keys they share, then, in Insert mode only, to
  `input` for their filters; the search of a tree falls back as a dialog does, `find` to `dialog`
  and its fields, `find_input`, to `dialog_input`. Every one but `user_screen` falls back to
  `global` last, whose keys are the same in every mode: the key hints, the output of commands, and
  Ctrl+l, which redraws the screen; the app takes them before whatever is in front, which stays as
  it was. The first context that knows a key sequence decides, except that a sequence it only starts
  does what a later context binds it to. Bindings are key sequences matched by prefix with a
  1-second timeout (`g g`, `Z Z`, `Ctrl+w h`). The panels group keys under `g`, Ctrl+w (the
  panels, as vim's windows), `t` (tabs and workspaces), `o` (sort order and hidden names), and `f`
  (`f j` the jobs, `f #` the checksums, `f /` and `f f` the search of the files below by text and
  by name, the rest of mc's Ctrl+x letters left for later). In Insert mode a character is text
  unless the context itself binds it, and in Normal and Visual mode a
  character nothing binds does nothing. The keymap reads what vim lets come with a command and hands
  it to the app with the action (`Typed`, `Extra`): a count before it (digits, `0` only after
  another), a register after `"`, the character after the actions that take one (`f`, `t`, `F`,
  `T`, `r`, `"`, and Ctrl+r and Ctrl+v in Insert mode), and, in the panels, noc's Ctrl+x before
  it, which runs it in the other panel. The app repeats a motion over rows a count's
  times, and gives the count to what takes it as a number (`G`, `g g`, `g t`, Ctrl+o) and to the
  text fields, whose editor (`tui/edit.rs`) reads it itself. `Esc` acts at once: it never waits for
  a key after it, and while a command is pending it gives the command up; so does a key that cannot
  go on with it, which in Normal and Visual mode goes too, as in vim (`g x` and Ctrl+w `e` do
  nothing), unless what came before it does something alone. The keymap says so
  (`KeyState::take_dropped`), and the app then gives up the part a text field holds, such as an
  operator that waits for its motion, or the one command after Ctrl+o. A count typed after a
  register multiplies the one typed before it. An `Esc` and a quick next key arrive as Alt and that
  key, so in Insert and Visual mode, and in the panels, Alt and a key that nothing binds, any key
  but `Esc`, go to the keymap as `Esc`, then as the key in the mode `Esc` left (`App::key`, with
  `Keymap::alt_as_esc`), even while a command is pending, unless the key goes on with it. vim gets
  them so without the kitty keyboard protocol; with it, which Noon Commander asks for, vim would
  type the key with the eighth bit set, and Noon Commander does as nvim does in Insert and Visual
  mode ([ADR 0022](adr/0022-vim-modes.md)). The pull-down menu and the dialogs (contexts
  `pull_down`, `dialog`, `dialog_input`, `path_input`, `completion`, `find`, and `find_input`,
  which `Context::alt_letters_are_hotkeys` names) take Alt and a key otherwise, in every mode: Alt
  and a letter or a digit that nothing binds go to the app as `Resolved::Hotkey`, the letter of a
  menu, a command, or an element of the dialog, and do nothing where nothing has the letter, and
  Alt never stands for Esc there ([ADR 0033](adr/0033-letters-in-dialogs.md)), as it never did on
  a password, where the `Esc` would close its dialog and send the key to the panels. Keys are
  written with `crokey` names. User overrides of the keymap are planned. The F-key
  bar is generated from the keymap in the mode and context of the next key, and so is the help
  screen (F1): the keys of each mode and context, with what they do, for what the app can do
  already; a prompt from ssh shows over it. It takes 70% of the width and of the height of the
  screen, and, as the Configuration dialog, lists its sections on the left (`tui/sidebar.rs`, which
  both draw): About, with the license and the report of `crate::about` that `noc debug version`
  prints too (the build; the system, read once at start; the terminal, from its variables and the
  keyboard protocol the TUI found, without asking for its name; and the external tools, the editor,
  zoxide, ripgrep, and fd, which are asked in the background at start and again when `[zoxide]
  program`, `[search] program`, or `[search] fd` changes, `Effect::Detect`), and the files the app
  keeps; and Keys, which folds, with a group for each part of the app (panels, text fields, search
  and renaming, the command line, lists and menus, menus and dialogs, the viewer) and tips, each
  under its title in points after a bullet (`•`, `-` without icons). It opens at the top of About
  first, and then where it was when it closed last (`help::Spot`, which the app keeps): the section,
  Keys open or folded, what had the keys, and how far down the text was. The titles of the parts of
  a section are bold on a line across (`── Panels ──`), and the keys and the names of the facts take
  the theme's `keys`, a color apart from the titles, as the keys of the key hints and of their list
  (Ctrl+k) do, and those of the menus of F9 in `menu_keys`. Tab moves between the sections and the
  text; in the sections `j` and `k` choose, `l` or Enter opens Keys or goes to the text, and `h`
  goes up to Keys and folds it; in the text `h` goes back. Each section has a letter, highlighted
  in its name: Alt and it show the section from anywhere, opening Keys for a group, and give the
  text the keys, as the letters of dialogs do. A click chooses a section, and one on Keys opens or
  folds it. The text scrolls as the viewer does, with the keys it does not take in the
  sections and with the wheel too, stopping with its last line at the bottom, with no line after it,
  a scroll bar, and how far down it is in its bottom frame.
- **Text fields.** A text field (`dialog::Field`, and the command line) holds a `tui/edit.rs`
  `Editor`: the text, the cursor, and vim's modes over it. In Insert mode characters go in at the
  cursor, and Left, Right, Home, End, Shift+Left, Shift+Right, Backspace, Delete, Ctrl+w, Ctrl+u,
  Ctrl+r and a register, and Ctrl+v edit around it. Ctrl+v types the next character, or Tab, as it
  is; unlike vim's, it gives up on any other control key, so that no control character gets into a
  name or a command. As vim's 'backspace' has them, Ctrl+w and Ctrl+u stop once where Insert mode
  started, but on the command line, whose keys are those of vim's, and at the start of a line join
  it to the line above; a move of the cursor, Ctrl+u, and the focus coming back start a new change
  for undo, and a new insert at the next change, as vim's `Insstart` and `Insstart_orig`, and so do
  Home and End where the cursor is already, as vim's `start_arrow`: right after a move, Ctrl+w and
  Ctrl+u still stop where the insert started, and after a move to the right with something typed, or
  after Ctrl+u, they keep stopping there. Ctrl+u is a change of its own because vim's `defaults.vim`
  maps it after Ctrl+g u; Ctrl+w, which it does not map, deletes within the change, as in vim. On
  the command line, in quick search, and in the filters, as on vim's command line, Delete at the end
  deletes the character before, and Shift and Ctrl with Left and Right go by what spaces part, Right
  on to the space after. Ctrl+o, in the fields that have a Normal mode, runs one command of it,
  after which, or after Esc, Insert mode is back; from after the end of a line the command starts on
  its last character, and the cursor goes back after it if it is still there, as in vim, and so
  after a command that deletes to the end of the line and after the text that `p` and `P` put, but
  not after `0`, `^`, and Home, which vim's `ins_at_eol` leaves before the character they go to,
  even the last; Insert mode then starts on that last character, before the cursor goes past it, so
  the first Ctrl+w or Ctrl+u deletes only it, as vim's `Insstart` is set. `$`, End, and Backspace
  onto the line above leave the cursor after the end of the line, as vim's do under `restart_edit`,
  so Insert mode starts there; and the end of the line is a place for Backspace to stop, as in
  Visual mode. A key that is not the field's, such as `j` to the next field of a dialog, or Ctrl+l,
  is that one command too. Esc goes to Normal mode, with the cursor on the character before it.
  Normal mode has vim's motions (`h`, `l`, `w`, `b`, `e`, `W`, `B`, `E`, `0`, `^`, `$`, `f`, `t`,
  `F`, `T`, `;`, `,`, and `-`, `+`, and `_`, which go to the first character that is not blank of
  the line above, below, or this one, and take lines whole after an operator, so `d _` is `d d`),
  the operators `d`, `c`, and `y` on a motion or doubled for the line, `x`, `X`, `s`, `S`, `C`, `D`,
  `r`, `~`, `p`, `P`, `u`, Ctrl+r, `i`, `a`, `I`, `A`, and `v`, Visual mode, whose motions select
  and whose `d`, `c`, `y`, and `~` act on the selection, and where, as in vim, the cursor can go
  past the last character of a line (`l`, Space, `$`, `w`, `e` at the end, a count before `v`,
  Backspace from the line below), so that the selection takes the line break too where a line
  follows; a count before `v` selects that many characters, or, after a command of Visual mode, as
  much as it took times the count, as vim's `resel_VIsual` (to the ends of lines after `$`); Space
  and Backspace move as `l` and `h`, and on the command line go on to the next line and back to the
  one before, as vim's 'whichwrap' has them. As in vim, `w` and `b` stop on an empty line, as on a
  word, and after an operator `w` from an empty line stops at the start of the next line, so that `d
  w` deletes it whole, `C` and `D` take a count of lines as `$` does, and more lines than one from
  the last line go nowhere (`2 d d`, `3 D`), `cw` changes to the end of the word, which is the
  cursor on its last character, `c b` at the start of the text goes nowhere, `^` on a line of blanks
  is the last of them, and `~` keeps a character whose other case would take more than one, and is a
  change for undo even on one without a case. On the command line, operators take what vim's rules
  give them: an exclusive motion that ends at the start of a line ends at the end of the line
  before, or takes the lines whole from the indent (`:help exclusive`, so `d w` on an empty line
  takes it), and a delete over lines from the indent to the end of a line takes them whole (`:help
  d`, as `d 2 $`); Backspace from the start of a line to the start of a line above deletes the
  characters but keeps lines in the register, as vim's `op_yank`. A count repeats a motion or a
  change, and multiplies with the count of an operator (`2d3w`); it stops where the motion can go no
  further. Undo takes back one change at a time, and all that one stay in Insert mode typed, a
  change of an operator `c` with it, and, as in vim, a change that changes nothing (`x` on an empty
  line, `d 0` at its start, Delete at the end in Insert mode, `C` on an empty line) is one too, so
  that `u` after it takes back nothing to see; but after `d d` deleted the only line, vim's
  `ML_EMPTY`, deletes are no change at all. It keeps 1000, and undo and redo leave the cursor where
  the change starts, as vim's do, on its line and in its column (after `d b` at the start of the
  word deleted; after `d d`, `c c`, and `S` of one line on its first character that is not blank, if
  the cursor was after it, as 'startofline' moves it first; from a line that is gone, on the first
  character of the last one that is not blank). The registers (`edit::Registers`) live in the app,
  which every field and the command line share: the unnamed one, which every yank and delete fills,
  a yank of nothing and `C` on an empty line with nothing, as vim's, but not a delete after `d d`
  deleted the only line, and `a` … `z`, of which `A` … `Z` append; `_` keeps nothing. `"` names one
  in `global`, wherever keys are commands, so that the name never runs as a command; after an
  operator that waits, as in vim, `"` gives it up and names none, and the key after it is a command
  of its own (the app tells the keymap, `KeyState::set_operator`). None reads or writes the system
  clipboard. Pasted text is one change, as vim puts it: at the cursor in Insert mode; in Normal mode
  as if typed in Insert mode, then Esc, so before the cursor at the start of a line and after it
  elsewhere, with the cursor on its last character; and in place of the selection in Visual mode,
  which goes to the unnamed register. After Ctrl+o, Normal mode stays after it, and an operator that
  waits is given up, and the paste with it, as in vim. What a delete appends to a register (`"Ad
  w`), the unnamed register keeps alone, as vim's register 1, which it reads then. A field holds one
  line, and a line put in it becomes text, its breaks spaces; the command line holds lines, and `d
  d`, `y y`, and `p` take and put them whole. A secret field stays in Insert mode, keeps no undo,
  and never copies its text into a register; what reads its text reads it into memory reserved up
  front and wiped when it goes.
- **Key hints.** As which-key does, the keymap lists the keys that can follow a pending sequence, or
  every key of a context, with what the help says they do (`Keymap::hints`, drawn by
  `tui/which_key.rs` in columns across the bottom of the panels). The event loop owns the
  `KeyState`: with `ui.which_key` on, a sequence that does nothing alone and does not start with
  `Esc` (such as `g` or Ctrl+w) does not time out; its hints show `ui.which_key_delay_ms` after its
  last key (`Keymap::hints_due`), and it then waits for the next key. So does Ctrl+x, whose hints
  list every key of the panel. The action `key_hints` shows
  every key of the mode and context at once: Ctrl+k everywhere, in every mode, bound once in the
  `global` context. In Insert mode the hints leave out characters, which are text. The app takes it
  before whatever is in front, which stays open under the hints. While hints show, the next key runs
  as it would have, or shows the keys that can follow it; `Esc` closes them and `Backspace` takes
  back the last key, or with none closes them, and so do `Ctrl+[` and `Ctrl+h`, which the kitty
  keyboard protocol tells apart from them. Ctrl+k after a count or a register shows every key, and
  keeps them for the key after it. Ctrl+k while hints show gives them the keys, as `K K` gives
  nvim's hover window the keys: the keymap resolves it with the count and the register, and the app
  opens `tui/key_hints.rs` in place of the hints it has, over everything, the dialogs too
  (`Context::KeyHints`, which falls back to `list`). It lists every sequence of the hints, whole,
  after the keys they had typed (`Keymap::sequences`), one row for each command; it opens with its
  filter taking the keys, Enter closes it and runs the command where the keys went before, with
  that count and register, and Esc goes to the rows, where Esc again closes it. `Esc` never starts
  a sequence, and never shows hints. The `KeyState` also keeps what the pending command has typed,
  which the line above the F-key bar shows (`KeyState::showcmd`).
- **Mouse.** On by default (`ui.mouse`; [ADR 0018](adr/0018-mouse-support.md)). The event loop
  captures the mouse while it is on and turns crossterm's events into `Pointer`s
  (`tui/mouse.rs`): a click, a double click (the same cell within 400 ms), a right click, or a
  step of the wheel, or a drag with the left button; moves are dropped without a redraw, and
  so are drags while no thumb of a scroll bar is held. Rendering records where the
  F-key slots, the panels, their rows, the tabs, the menu bar, and each window's rows and
  buttons were drawn (`mouse::Drawn`), and `App::pointer` looks there, front first, as keys
  go. A click on an F-key slot presses its key; in a panel a click moves the cursor and gives
  the panel the keys, a double click does what Enter does, a right click marks the row as in
  mc and Far, and the wheel scrolls the panel under it, by `ui.wheel` lines or a page, or moves
  its cursor as far where it cannot scroll; a click
  on a tab shows it, and in the viewer the wheel scrolls. In the pull-down menu a click opens
  a menu or runs a command; in a dialog a click chooses, puts the cursor in a field, switches
  a check box, or presses a button. Other windows and menus take the wheel as Up and Down, a
  click on a row or a button, and in menus a double click that opens and a click outside
  that closes. Scroll bars end in arrows; as in Far, a click on one does what a step of the
  wheel does there, a click on the track pages towards it, and the thumb, pressed, drags
  (`noc_scroll::scrollbar::Bar`); `App::scroll_bars` lists those of what is in front, with
  what each scrolls (`Scrolled`). Widgets
  move their own cursor or focus and return the key a press stands for,
  which they then take as if typed, so the mouse reuses what the keys do. A double click
  counts only if its first click left the same thing in front, so one that closed a menu or
  a dialog does not reach what is under it. While the mouse is captured, the terminal selects
  text with a modifier held: Shift in most terminals, Option in iTerm2.
- **Dialogs.** Modal and centered over the panels, those for one panel too, such as Go to
  directory and those of `g +` and `g -`, with mc-style buttons: `[< OK >]` marks the default
  one, and a line across the dialog (`╟───╢`, or `├───┤` with single lines) sets the buttons apart
  from what is above them, in every dialog and window. A dialog has a message, radio buttons (`(*)`
  on the chosen one), text fields, check boxes, and buttons, each optional but the buttons;
  ssh's prompts and the app's own questions are the same kind of dialog, and each one in the queue
  knows where its answer goes. Keys go to the first dialog in the queue (contexts `dialog` and
  `dialog_input`). A dialog whose first focus is a text field opens in Insert mode, with the cursor
  after the field's text, and any other in Normal mode. In Normal mode `j`, `k`, `h`, `l`, the
  arrows, and Tab move between the radio buttons, the fields, the check boxes, and the buttons, as
  many as a count says, past the fields too, `g g` and `G` (Home, End, Ctrl+b, Ctrl+f) go to the
  first of them and the last, Space chooses a radio button, switches a check box, or presses a
  button, Enter presses the button with the focus (the default one from elsewhere, after choosing
  the radio button it is on), and Esc, `q`, Ctrl+c, or F10 cancels. A field takes the keys of a
  [text field](#ui) in its mode: in Insert mode every character is text, Space too, Tab, Up, and
  Down still move the focus, Enter presses the default button, Esc goes to Normal mode, and Ctrl+c
  or F10 cancels; in Normal mode, Esc gives up an operator that waits, then cancels. The focus comes
  to a field in Insert mode with Tab, a click, or from a field in Insert mode, and else in Normal
  mode. Long text stays where it is while the cursor shows, and else scrolls to put the cursor in
  the middle, as vim's 'sidescroll' 0 does; what Visual mode selects shows in reverse. A message
  taller than the screen scrolls between the top and the rest of the dialog, which keeps its
  buttons, with a scroll bar on the frame that the mouse and the wheel move.
- **Letters of dialogs** ([ADR 0033](adr/0033-letters-in-dialogs.md)). As in Far and mc, every
  element of a dialog but Cancel has a letter, which an `&` marks in its Fluent text (`Label`, in
  `tui/label.rs`, as for the menus of F9) and which shows in the theme's `dialog_hotkey`, or
  `error_hotkey` in red dialogs, as mc's `dhotnormal` and `errdhotnormal`, and on what has the
  focus in `dialog_hotkey_focused` or `error_hotkey_focused`, as mc's `dhotfocus` and
  `errdhotfocus`: the color of titles, not bold, and underlined where that color would not show
  (the cursor, red dialogs, a title's own color). Alt and it act on the
  element from anywhere in the dialog, in every mode, while typing in a field too: a text field
  gets the focus, in Insert mode, as with Tab; a check box switches and a radio button is chosen,
  each getting the focus; a button is pressed. The one field of a form without a label (Copy,
  Move, Create a new directory, Go to directory, Save workspace, Rename workspace, Save
  checksums) takes its letter from the prompt over it, as `&Copy "x" to:`. What ssh asks is ssh's
  text, with no letter, but the buttons of its dialogs have theirs. Names that the user gave, of
  files, paths, workspaces, and hosts, have their `&` doubled before they go into a label, so they
  never make a letter. The window of a job, the jobs, the checksums, and the Configuration dialog
  have letters too. Letters are unique within a dialog, which a test checks.
- **Text.** Fluent files under `crates/noc/i18n/`, embedded in the binary and read with
  `fl!` from `i18n-embed-fl`, which checks message IDs against `en-US` at compile time; only
  `en-US` for now. `ui.language = "auto"` follows the system locale (through `sys-locale`).
  Arguments are inserted without Unicode isolation marks, which terminals would show. Numbers
  are passed to messages as numbers, so that plural forms see them, and written by
  `noc_locale::fluent`, the formatter of every bundle, in the formats of the region. A prompt
  with what is typed in it (`Search: …`) is split where its message puts the typed text
  (`search::Prompt`), so a translation may put it anywhere.
- **Formats of the region** ([ADR 0035](adr/0035-formats-of-the-region.md)). `noc-locale` writes
  numbers, dates, and times, and compares names, as `ui.formats` says: `auto` takes each part
  from `LC_ALL`, its own `LC_TIME`, `LC_NUMERIC`, or `LC_COLLATE`, then, on macOS, System
  Settings through Foundation's formatters, which apply the user's own date format, separators,
  and 24-hour time, then `LANG`; `iso` is neutral (`2025-10-17 14:05`, `1234.5`); a language tag
  names a region's formats in CLDR, through ICU4X. A time shows its date, with the year, and its
  time of day, in digits; every number is grouped. The formats in use are
  global (`noc_locale::current`); a change of the setting makes new ones in a task
  (`Effect::Formats`) and sorts the panels again. Help → About shows where they come from.
- **Themes.** Built in: `mc-classic`, the colors of mc's default skin (blue panels, a cyan
  cursor that replaces the row's colors, a dark gray one under them in the inactive panel,
  yellow headers; directories white, executables green, broken links red, devices magenta; gray
  dialogs with mc's shadow; a black-and-cyan F-key bar; red error dialogs, and warnings black
  on yellow); `terminal`, the
  terminal's own colors with reverse video; `noon-dark` and `noon-light`, the colors of the
  logo (`assets/icons/logo.svg`); and `catppuccin-mocha` and `catppuccin-latte`, the dark and
  light flavors of [Catppuccin](https://catppuccin.com)
  ([ADR 0010](adr/0010-truecolor-themes.md)).
  `ui.theme` picks one; an unknown name is an error. `mc-classic` uses the 16 ANSI colors, so
  the terminal's palette decides its shades. The Catppuccin themes are built by one function
  from a palette of named colors, so both flavors give each color the same role: panels on
  `base`, dialogs, menus, and the F-key labels on `surface0`, text fields on `surface1`, the
  idle menu bar on `mantle`, the F-key numbers and the shadow on `crust`; a `blue` cursor, the
  active panel's title, and focused buttons; `lavender` headers, `mauve` marks and dialog
  titles, warnings on `peach`, darkened in Latte; directories `blue`, executables `green`,
  symlinks `teal`, broken links `red`,
  devices `pink`; hosts `overlay1`, `yellow`, `green`, `red` by state; the units of sizes G in
  `peach`, the color Catppuccin gives numbers, and T `red`. The Noon themes are
  built the same way from a palette of roles taken from the logo: the cursor, the active
  panel's title, and focused buttons are on the logo's gold (`#A46D00`), and the menu bar on
  its amber frame (`#996400`), with cream text; error dialogs and the cursor where the user is
  root are on a deep red (`#A63A32`) with cream text, as the gold is deep, rather than on the
  bright red of text, which glares as a background; warnings are on a dark orange, nearly brown
  (`#8F4F0C`), with cream text, in 256 colors on `#D75F00`, as the nearest is the menu bar's,
  and in `noon-light` on a light orange (`#F9955F`) with navy text
  ([ADR 0048](adr/0048-errors-and-warnings-in-dialogs.md)); the F-key bar is dark, its
  numbers bold in the color of the headers; `noon-dark` has the logo's navy panels and dialogs a
  shade lighter, cream directories, and bright gold headers (`#FFC24A`), which read better on navy;
  `noon-light` has cream panels (`#FFFBEA`) with navy text, pale blue-gray dialogs, the gold
  and the amber twice as light (`#FFC24A`, `#FFB833`) and a pale red (`#F2A39A`) with navy
  text, and a dark amber for headers, which the bright gold is too pale for on cream. These
  themes are 24-bit RGB where `COLORTERM` is `truecolor` or `24bit`; elsewhere each is the
  nearest of the 6×6×6 cube and the gray ramp of the 256-color palette, never the 16 colors
  below them, which the terminal's palette redefines. User themes in `themes/` are planned for M4.
  Panels and dialogs are framed with double lines unless `ui.borders` is `single`; as in mc,
  a dialog leaves a blank cell between its frame and its edge, which gives way on a screen too
  small for it.
- **Icons.** Nerd Fonts v3 glyphs, on by default (`ui.icons`), in front of each name: our own for
  directories, `..`, links, broken links, FIFOs, sockets, devices, and executables, and in the
  location menu for the home directory, volumes, network volumes, and hosts (which tell their
  state); `devicons` for files by name or extension. devicons asks the disk whether a name it
  does not know is a directory, so names go to it inside a path with a NUL byte, which names
  nothing: drawing stays free of I/O, and remote names are never looked up locally. Without
  icons, mc's markers: `/` directory, `~` link to a directory, `@` link, `!` broken link, `*`
  executable, `|` FIFO, `=` socket, `-` character device, `+` block device and volume; the
  home directory is `~`.
- **Animated windows.** With `animations.windows` (the default; [ADR
  0034](adr/0034-animated-windows.md)), a window's frame grows from its middle as it opens and
  shrinks back as it closes: six sizes, 20 ms each, empty and in the window's colors, with its
  shadow; a menu of F9 drops from its bar. `tui/zoom.rs` does it on top of rendering: `App::render`
  draws each window through `Stage::show` under a `zoom::Window` that tells it apart (a dialog by
  its id, a job by its own), which marks every cell before the window draws to tell the cells it
  drew, even as they were, and finds the frame among them (its first top left corner, and the blank
  cells drawn around it); a window new since the last frame
  grows, its cells under it put back from before, and one of the last frame that is gone shrinks
  over everything at the end, unless another opened in its place. Keys reach the window at once;
  only the screen waits. The event loop wakes for the next size at `App::frame_due`. The first
  frame, a new size of the screen, the viewer coming or going, and the terminal coming back from
  another program show their windows at once. Completion lists and the key hints of which-key appear
  at once, as they come while typing. In debug builds, `--slow-windows SECONDS` has a frame take
  that long to grow or shrink, its six sizes shown for equal parts of it (`Stage::slow`), so each
  can be watched; `--show-sizes` has each window write the size of its frame as `WIDTHxHEIGHT` at
  the right of its bottom line, once it shows whole, animations on or off (`Stage::show_sizes`).
  Only windows drawn through `Stage::show` have it; the panels, the bars, and which-key do not.
- **Spinners.** Whatever waits for work in the background shows one spinner, next to what tells what
  it waits for ([ADR 0037](adr/0037-spinners.md)). `tui/spinner.rs` keeps its clock. Braille dots
  turn, in the theme's `busy` color, where one cell is all there is: in place of the icon in front
  of the path in a panel's title (a folder, or a host in the color of its status), which shows the
  directory it reads at once (and while a directory of F7 or a rename it started is done, with the
  reading after it in one wait: `Panel::begin_work`), so the path never moves and the status line
  under the rows stays as it is; the dots take the color of the host's status while it connects; in
  place of the file's icon in front of the path in the viewer's title, while its `Reading` window
  shows a gauge; on the icon of a host connecting in the location menu;
  and in front of the titles of the location menu, the zoxide window, and the search of a whole
  tree, drawn over the line of the frame by `dialog::draw_title_dots`, so the title stays in place;
  until their first rows come, `dialog::draw_skeleton` draws rows of faint bars, as many as the
  window had the last time it opened (`App` keeps them in memory, one the first time), which a shine
  crosses at once from when the dots show, one cell every 40 ms, in their place; the status line of
  the search has a bar of its own, crossed with them. Until a job knows how far it is, its gauge is
  a faint bar of `▒` with no percentage, over which the same shine runs to and fro as fast, turning
  at its ends (`dialog::shine_bar`, which the skeleton of rows draws too; `Shine::bounce`). Each
  waiting thing keeps a `Wait`, which remembers the first frame that drew it; nothing shows until
  150 ms later, so quick work does not flicker. Frames come from the time since the app's first
  frame, so spinners move together and tests draw given instants; `App::spin_due` tells the event
  loop when the next frame is due. With `animations.spinners` off, a still mark stands in place of
  the spinner. R in the location menu keeps its rows while the dots turn, and shows the reason it
  failed right of the filter. In debug builds, `--slow-spinners SECONDS` holds back every result of
  background work by that long, in order, and `--slow-connections SECONDS` holds back only
  connections made or ended, in a queue of their own, so the dots of a host connecting can be
  watched in the location menu. Each frame goes out as synchronized output (`draw_whole` in
  `tui/mod.rs`), so the cursor of a field does not show on its way through the cells a shine
  changes. A spinner's next frame is due 40 ms after a frame that drew the shine of its `Wait`,
  which remembers it, else 80 ms later, the next frame of the dots.
- **Glyphs.** Every special character the UI draws is in `tui/glyphs.rs`: `Glyphs::NERD` and
  `Glyphs::PLAIN`, which `ui.icons` picks and the theme carries (`Theme.glyphs`), with the icons
  and markers of kinds, hosts, and volumes, the glyph of root, the scroll bars, the marks of text
  cut short, and the marks of the pull-down menu; `Lines::DOUBLE` and `Lines::SINGLE` by
  `ui.borders`, with the tees that join lines to frames; and the characters that are the same either
  way: the single line, the line between columns, the `·` after the mode on its line, the spinner,
  the arrows of the sort order, the checks of checksums, the progress bar, the marks of dialogs'
  radio buttons, check boxes, and secrets, and the icons of the Configuration dialog's categories.
  `decor.rs` picks among them by kind, and asks devicons for files. `noc-scroll`, `noc-text`, and
  `noc-viewer` hold none: they take what they draw with as parameters (`BarGlyphs`, `Marks`, the
  viewer's `Styles`). `just glyphs`, part of `just lint`, fails on a special character written in
  Rust code that draws, outside `glyphs.rs` and the tests.

## Releases

A signed tag `vX.Y.Z` starts `.github/workflows/release.yml`, the only place where releases
are built ([ADR 0014](adr/0014-release-workflow-and-homebrew-tap.md)):

```text
ci (ci.yml on the tag) ─────────────────────┐
draft ──► build (aarch64, x86_64 macOS) ────┴──► publish ──► homebrew
```

- `draft` checks the tag against `Cargo.toml` and its signature, takes the notes from
  `CHANGELOG.md` with `packaging/release-notes.sh`, which joins its wrapped lines, and creates
  a draft release.
- `build` compiles `noc` for each target without a cache, packs
  `noon-commander-X.Y.Z-<target>.tar.gz`, attests its build provenance, and uploads it.
- `publish` verifies the tarballs' attestations and publishes the draft once CI and every
  build passed.
- `homebrew` renders `packaging/homebrew/noon-commander.rb.in` and commits it to
  `noon-commander/homebrew-tap` through the API as a GitHub App, so GitHub signs the commit.
