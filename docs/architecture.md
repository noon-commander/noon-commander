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
├── noc-ssh/       host discovery, ssh -G, argument validation, forwarding policy,
│                  ControlMaster, SFTP channels, askpass bridge
├── noc-vfs/       Vfs trait: local and SFTP backends, mounted volumes
├── noc-ops/       job engine: copy, move, rename, delete, mkdir, checksums; progress, cancellation, conflicts
├── noc-tools/     external programs other than ssh: zoxide, ripgrep, fd, the editor, the opener, the shell
├── noc-text/      terminal-safe text: sanitizing, widths in cells, fitting, wrapping
├── noc-pattern/   patterns typed to match names: regular expressions or text, smart case
├── noc-scroll/    paging text that ends: how far the page keys go, the scroll bar
└── noc-viewer/    the F3 viewer: reading through the Vfs, scrolling, wrapping; its own text
```

Dependencies point one way, toward the binary:

```text
noc-config  noc-ssh  noc-text  noc-vfs  noc-scroll  noc-pattern  no other noc-* crate
noc-tools   → noc-pattern
noc-ops     → noc-vfs
noc-viewer  → noc-vfs, noc-text, noc-scroll
noc         → all of them
```

Library crates contain no UI code and no user-facing text; they return typed errors and events,
and the UI turns them into messages. `noc-viewer` is the exception
([ADR 0015](adr/0015-viewer-and-text-crates.md)): a piece of the UI, with Fluent text of its own,
that knows nothing of the keymap, the theme, or the app. The app maps its keys to the viewer's
commands, gives it the styles of its theme, reads the file with `noc_viewer::read_start` in a
task of its own, and closes the viewer itself. `noc-scroll` draws too, the scroll bar, but has
no text ([ADR 0023](adr/0023-paging-stops-at-the-end.md)). Code that two crates need goes into a
crate of its own topic, such as `noc-text`, or `noc-pattern`, which builds the regular
expressions of fd's search in `noc-tools` and of the regex mode in the app, rather than into a
common `core` crate.

ssh runs only from `noc-ssh`, and every other program only from `noc-tools`
([ADR 0012](adr/0012-external-tools-and-zoxide.md)): without a shell, with `--` before paths,
without input for background ones, and killed when their future is dropped or they time out.
A program that is not installed is told apart from one that fails. The shell of the command
line is the one program started through a shell, since the command is the user's
([ADR 0019](adr/0019-shell-command-line.md)). The desktop's opener of `g x` is the one program
left running: it starts programs that outlive it and Noon Commander, so its output goes nowhere,
it is never killed, and it takes absolute paths without `--`, which `xdg-open` refuses
([ADR 0026](adr/0026-open-externally.md)).

## Processes

Each connected host has one master connection; everything else is multiplexed over its control
socket ([ADR 0002](adr/0002-controlmaster-per-host.md)):

```text
ssh <master options> -M -N -S <sock> -o ControlPersist=no -- <alias>  # authenticates once
ssh <channel options> -S <sock> -T -s -- <alias> sftp                 # panel channel
ssh <channel options> -S <sock> -T -s -- <alias> sftp                 # transfer channels
ssh <command options> -S <sock> -- <alias> 'cd -- <dir> || exit⏎<cmd>' # command line
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
metadata with and without following symlinks, tells the size and free space of the file system that
holds a path (`None` from an SFTP server without `statvfs@openssh.com`), canonicalizes paths,
creates and removes directories, removes files, renames, reads and makes symlinks (their targets
stored as given, never resolved), sets permissions and modification times, and reads and writes
files as chunks (`FileReader`, `FileWriter`, whose `finish` reports errors that only show when a
file is closed). Over SFTP, reads and writes of a file keep 64 requests of 32 KiB in flight, as
`sftp(1)` does, so a long round trip does not idle the link; a read that comes back short is asked
again for the rest. Symlinks go to the server in OpenSSH's argument order (target first), which the
client uses and which differs from the protocol draft; a server that follows the draft would store
them the other way round. Its futures are cancel-safe: a dropped one leaks nothing, though a change
already sent may still happen. Errors are typed, and the UI words them: not found, permission
denied, already exists, or the error of the OS or the server. SFTP v3 has no code for a name that is
taken, so `SftpFs` reports a plain failure to create or rename as `AlreadyExists` when something has
that name. `rename` replaces an existing file where that takes one step: locally, and over SFTP with
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
  connection, its status line starts with a spinner, `| Connecting to web…`, and Esc stops it.
- A failed connection, the first attempt or a lost one, opens a dialog, Connection, that says
  `Cannot connect to web: reason` or `Lost the connection to web: reason`, with Retry (the
  default) and Cancel. While it is open, the panels that wait for the host keep waiting, and
  their status line says `web: Retry or Cancel?` after a spinner in the color of a failed host.
  On a lost connection, the panels on the host, in every tab, drop their entries at once and
  wait to read the same directory, with the cursor where it was. Retry connects again, and the
  waiting panels read their directories once it is connected. Cancel after a failed first
  attempt leaves the panel where it was, with `Cannot open web: reason` in its status line;
  after a lost connection, it leaves the panels on the host without entries, and their status
  line says `Not connected to web; R connects`, with the Reload key of the keymap. `R`, or
  Enter on `..`, connects again. Jobs on a lost host end with it.
- F8 or `d d` in the location menu, or Disconnect in the menus of F9, closes the connection to a
  host, or stops connecting to it. Panels on a connected host stay there without entries, with the
  same
  status line, and no dialog asks; a panel that only waited for the host stays where it was.
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

Ctrl+w 1 and Ctrl+w 2 open a menu over the left or the right panel (Alt+F1 and Alt+F2 too, as Far
Manager's menus to change drives, and F9 → Left or Right → Change location). Left and right are
where the panels are drawn, as in Far: after Ctrl+w x, Ctrl+w 1 opens the menu of the right panel,
drawn on the left, and its title says Left. It is the one place to choose a volume or a host,
grouped under headings:

```text
 ╔════════════════════ Left ════════════════════╗
 ║ Filter:                                      ║
 ║ ──────────────────────────────────────────── ║
 ║ ─ Favorites ──────────────────────────────── ║
 ║ ~ Home                                  212G ║
 ║ ─ Mounts ─────────────────────────────────── ║
 ║ + Macintosh HD                          212G ║
 ║ + USB                                    12G ║
 ║ ─ SFTP ───────────────────────────────────── ║
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
- The menu opens in Normal mode, as the other [lists](#lists) do, with the cursor on the home
  directory or the volume that holds the panel's directory, whichever is nearer, or on its host:
  `j` and `k` move, `g g` and `G` go to the first and the last row, and Enter opens the row in
  that panel, which becomes active.
- `/` types in the filter, in Insert mode, which finds the places by name, path or mount point,
  alias, label, and address, and finds every one, hidden ones too; the rows of more go away while
  the filter has text. It matches them in the [search mode](#search-modes): as fzf does, the
  default, with the cursor on the best row; or a row's text must hold the filter, ignoring case,
  or match it as a regular expression, with the cursor on the first. The rows keep their order
  either way. The filter is a text field, edited with the keys of `input`; Esc goes back to the
  rows with the filter kept. F4 edits the settings of the host under the cursor, F8 or `d d`
  disconnects it or stops connecting to it, `R` reads the volumes and hosts again, and Esc, `q`,
  or F10 closes the menu.
- `recent.toml` in the state directory keeps the places used last: the aliases of hosts and
  the mount points of volumes, most recent first, up to 50 of each. A host counts as used when
  a connection to it succeeds; a volume when a panel arrives in a directory on it in any way:
  the menu, Quick cd, zoxide, Enter, or a workspace. The app reads the file at start, and after
  a turn of the event loop that changed the lists, a task writes it through `noc-config`, whole
  and atomically, one change after another. Mount points that are not UTF-8 are not kept, since
  TOML cannot hold them.
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
 ╔══════════════════════════════╗
 ║   Change location…  Ctrl+w 1 ║
 ╟──────────────────────────────╢
 ║ • Sort by name           o n ║
 ║   Sort by extension      o e ║
 ║   Sort by time           o m ║
 ║   Sort by size           o s ║
 ╟──────────────────────────────╢
 ║   Rescan                   R ║
 ║   Disconnect                 ║
 ╚══════════════════════════════╝
```

- Left and Right act on the panel drawn on that side, after Ctrl+w x too: its location menu, sort
  order (`•` marks the current one), Rescan, Disconnect while it shows a host, and its
  [tabs](#tabs): New tab, Close tab, and Tab list…. File has F3, F4, Open externally (`g x`), F5 …
  F8, `g +`, `g -`, `g *`, Checksums, and Exit; Command has quick search, Find file… (the [search of
  a whole tree](#search-of-a-whole-tree)), [Quick cd](#quick-cd), the [zoxide](#zoxide) window, the
  other-panel commands, the jobs, host settings and disconnect for the host the active panel shows,
  help, and redraw; Options has Configuration… and Show hidden files (`✓` while on); Workspace has
  Save workspace…, Workspace list…, and the saved [workspaces](#workspaces), with digits as their
  letters, typed with Alt. Without icons, the marks are `*` and `x`. A menu taller than the screen
  scrolls with the cursor.
- Commands do what their keys do, through the same `Action`s. Each shows the key that does
  it in the active panel's context (`Keymap::key`); Left and Right show sort and rescan keys
  only for the active panel, as keys act there. Commands that cannot run now are dimmed, and
  the cursor skips them.
- On the bar, `h` and `l` (Left and Right, Shift+Tab and Tab) select the next menu, round the bar;
  Enter, `j` (Down), or Alt and the menu's highlighted letter (`&` in the Fluent messages) opens it,
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
  its hotkeys, so the plain letters stay vim's motions.

### Quick cd

`g d`, as mc's Quick cd, asks for a path and opens it in the active panel, as `cd`
in a shell reads it: relative to the panel's directory, `/…` absolute, `~` and `~/…` the home
directory, `..` and `.` resolved by name, `-` the directory the tab showed before, and
`host:path` on a host, scp-style, which connects to it if needed. On a host, paths stay on it
and `~` is the remote home. `cd ..` puts the cursor on the directory left, as `h` does; a
path that is not there leaves the panel where it is, with the reason below the listing. It is in
F9 → Command.

### Completion

Tab completes the path before the cursor in the fields of Quick cd, F5, F6, and F7, in Insert
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
  runs, with `~` and absolute paths as Quick cd reads them. On a host every word completes as its
  files, through its session, and the first as a command never. Names go in escaped for `sh` with
  backslashes, as vim's do; the list shows over the line.
- Names that start with a dot count only after a typed dot. Case never counts, as vim's
  'fileignorecase' has it on macOS. Names that are not UTF-8 or hold control characters cannot be
  typed and are left out.
- Quick cd and F7 offer directories and links to them; F5 and F6 files too. Before a `:`,
  Quick cd offers every host of the ssh config, F5 and F6 the connected ones, F7 none.
- Local directories are read in the background, remote ones through the session of their
  host; a host that is not connected offers nothing, and a Tab never connects. Each listing
  has a generation, and a reply for text that changed since is dropped.

### zoxide

[zoxide](https://github.com/ajeetdsouza/zoxide) ranks the directories the user works in
([ADR 0012](adr/0012-external-tools-and-zoxide.md)). `g z` opens a window of its best
directories, which Enter opens in the active panel:

```text
╔═════════════════════════════ zoxide ═════════════════════════════╗
║ Jump to: src                                                     ║
║ ──────────────────────────────────────────────────────────────── ║
║ ~/src/noc                                                   56.0 ║
║ /srv/www/src                                                12.5 ║
╚══════════════════════════════════════════════════════════════════╝
```

- In the fzf and regex [search modes](#search-modes), the window asks
  `zoxide query --list --score --exclude <dir> --` once, for every directory but the panel's,
  and what is typed filters them: as fzf does, best match first and those as good in zoxide's
  order, as `zi` does with fzf, or as a regular expression, in zoxide's order. In the plain
  mode what is typed is zoxide keywords, as `z foo bar` in a shell; each change asks
  `zoxide query --list --score --exclude <dir> -- <keywords>` again, and a query still running
  is dropped. Alt+r going to or from the plain mode asks zoxide again. Unlike the other
  [lists](#lists), the window opens in Insert mode, typing keywords, as `zi` does: Esc goes to
  the rows, in Normal mode, where `/` types again and Esc closes the window. Directories under
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

Quick search and the filters of the lists (the location menu, the tab menu, the zoxide window,
the Workspaces window, the command history, and the list of the key hints) match what is typed
in one of three modes ([ADR 0030](adr/0030-search-modes.md)). Each search and each window starts
in `ui.search_mode`, `fzf` by default, and Alt+r switches its own, round: plain, fzf, regex. The
mode is not kept: the next search or window starts in `ui.search_mode` again, but `n` and `N`
match as the last search did. A glyph after the word of the prompt says which mode
(`Search ~:`, `Filter ~:`, `Jump to ~:`; in `glyphs.rs`: nf-cod-whole_word,
nf-md-approximately_equal, nf-cod-regex; `=`, `~`, `.*` without icons); crossed out, in the color
of a broken link, while the text is no regular expression, when the lists say what is wrong with
it in place of the rows. The [search of a whole tree](#search-of-a-whole-tree) starts in the
search mode too, but has no fzf mode: there `fzf` is the regex mode, Alt+r switches between it
and the plain one, and its Text field shows which (`Text .*:`). `tui/search_mode.rs` holds the
`Matcher` of each mode:

- **Plain**: the text anywhere, ignoring case. The rows keep their order, and quick search goes
  down them as vim's search does.
- **fzf**: as below.
- **Regex**: a regular expression, built by the `noc-pattern` crate as the search of a tree
  builds its patterns for fd: the `regex` crate, smart case (case counts only once the
  expression has a capital letter that is not escaped, as `\S` is not), and the last line of
  what the engine says is wrong. The rows keep their order, as in the plain mode.

The fzf mode follows [ADR 0016](adr/0016-fuzzy-search.md). `tui/fuzzy.rs` wraps
`nucleo-matcher`, Helix's port of fzf's algorithm:

- The letters typed must come in order, not necessarily together: `cfg` finds `config.rs`.
  Runs of them, and letters that start a word (after a space, `/`, or punctuation, or a
  capital after a small letter), score higher; in the zoxide window, after `/` most.
- Words separated by spaces must all match, in any order. As in fzf's extended search,
  `'word` matches as it is, `^word` at the start, `word$` at the end, and `!word` only where
  it is not; `\` takes a space or one of these as it is.
- Case counts only once the text has a capital letter; letters with accents match those
  without.
- Lists keep their order, and the cursor goes to the best match; the zoxide window, whose
  order is zoxide's rank, sorts by the match instead, and keeps zoxide's order among matches
  as good.
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
║ Text .*:                                                           ║
║ handle                                                             ║
║ Files:                                                             ║
║ *.rs !target                                                       ║
║ [ ] Match case                                                     ║
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
  globs of the files, split at blanks, `!` before one leaving those out; a check box, matching
  case rather than only when the text has a capital letter; the rows; and four buttons. The text
  is a regular expression in the regex and fzf [search modes](#search-modes) and as it is in the
  plain one; Alt+r switches it for this dialog, and the glyph after `Text` says which
  (`Dialog::set_label`). With no text it lists the files. The dialog keeps the cursor of the
  rows and scrolls them; `FindWindow` (`tui/find.rs`) keeps what was found and draws each row
  (`ListView`).
- Two modes, radio buttons on a line above the fields (`Dialog::in_a_row`): Text, with ripgrep
  (`FindQuery::Lines`), and Files and dirs, with fd (`FindQuery::Names`), whose buttons are Go to
  and Cancel (`Dialog::set_buttons`). By name, fd matches the text against the names, as a
  regular expression or as it is, with smart case or case, as Alt+r and the check box say; `!`
  globs go to it as `--exclude`, and `noc_tools::fd::Names` keeps what the other globs let through and
  finds where the name matches, with fd's regex engine, for `find_match`. On a host `fd` runs,
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
  Insert mode, typing in the text; Tab, Up, and Down go through the fields, the check box, the
  rows, and the buttons; Esc leaves Insert mode in a field, and again closes the dialog. While the
  rows have the focus, the motions and the page keys move their cursor. F3, F4, Alt+r, and Alt+c
  work wherever the focus is. The wheel scrolls the rows.
- The rows are the files, in the order of their paths, each with how many lines matched, and their
  lines with their numbers, the matches in a color of their own (`find_match`), apart from the
  cursor's; a line too long starts a little before its first match. The rows of a search stay until
  the next one finds something or ends, the cursor on the row it was on. Under them, how many lines
  it found in how many files, after the spinner while it searches; that it stopped at the limit; and
  the errors. A pattern that is not a regular expression, a missing ripgrep, or a host that is not
  connected says why in place of the rows.
- Go to, the default button, goes to the file in the active panel: its directory, with the cursor
  on it; where the panel was goes to the jump list. View (F3) views it with the line at the top,
  and Edit (F4) edits it at the line (`Editor::edit_at`), a remote one through a local copy; the
  dialog is there again after both. On a file's row they take its first line that matched.
- It is not in the queue of dialogs, which shows only the one in front, over everything: it
  stays under the viewer, the jobs, the help, and the dialogs, as the window it was.

### Lists

The location menu, the tab menu, the zoxide window, the Workspaces window, and the command history
list rows to choose from. Their rows take keys in Normal mode (keymap context `list`, which each
one's context falls back to), where all but the zoxide window open: `j`, `k`, Ctrl+j, Ctrl+d,
Ctrl+u, Ctrl+f, Ctrl+b, `g g`, and `G` move, and the rows scroll as the panels' do, headings among
them, a count repeats a motion, Enter takes the row under the cursor, `d d` and F8 delete it where
the window can, as many rows as a count says (`3d d`, as vim's deletes three lines), and Esc, `q`,
Ctrl+c, or F10 closes the window. `/` types in the filter, in Insert mode, where characters are
text, the keys of `input` edit it (Ctrl+r types a register, which the app reads, and Ctrl+o, which
has no Normal mode to go to, does nothing), Up, Down, Ctrl+p, and Ctrl+n still move, and Esc goes
back to the rows with the filter kept. Text pasted while the
rows have the keys goes to the filter, in Insert mode. There are no digit hotkeys.

- All but the zoxide window share `tui/picker.rs`: the filter, a text field with a cursor
  (`dialog::Field`), which matches in the [search mode](#search-modes); the cursor, scrolling,
  the mouse, and the box, which the menus draw over their panel and the windows over both. Each
  holds its own rows, says what the filter finds them by, and draws them. A row may show only
  while the filter is empty, as the rows of more in the location menu do, or only while it has
  text, as the places behind them do.
- The zoxide window does not use it: zoxide, not a filter, picks its rows from the keywords
  typed. It has the same modes, but opens in Insert mode, with the keywords taking the keys.
- `K` shows the whole row under the cursor in two lines under the rows, until `K` again, in each
  list on its own (`picker::Whole`), and not in the panels, which have theirs: the directory of the
  zoxide window, the place of the location menu, the title of a tab, where the tabs of a workspace
  go. The command history always shows the whole command there.

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
  to the tab of that number, as in vim; `t h` and `t l` open the tab menu of the left and the
  right panel, whichever is active: the tabs with their whole locations, over that panel, to show
  one. The panel whose tab is shown becomes active. `d d` and F8 (`close_tab`) close the tab
  under the cursor there as `t c` closes the one that shows
  (`Tabs::close_at`), and the menu stays open with the rest. Left and Right in F9 do the same for
  the panel on that side.
- Names on the line of tabs are brief: the last component of a directory, `~` for the home
  directory, `host:name` on a host, and the host alone in its start directory. The frame's
  verticals close each side's line, and its top corners become tees (`╠`, `╣`) that join it,
  so the two sides stay apart. When the tabs do not fit, the names of hidden
  tabs shrink first, then the one that shows, none below eight cells; then tabs far from it
  are left out, and `«` and `»` say so.
- Panels are named by a `PanelId`, their side and tab number, which is never used again.
  Listings, new directories, and the dialogs of `g +`, `g -`, F5, F6, and F7 carry it, so a reply
  reaches the tab that asked even after a switch, and one for a closed tab is dropped. What
  acts on the other side (Ctrl+w f, Ctrl+w v, F5's target, `other_dir`) acts on the tab that shows
  there. Disconnecting or losing a host leaves every tab on it there, hidden or not, without
  its entries; after a lost connection they wait for Retry and read their directories when the
  host is back.
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
  after a question. A workspace just saved there gets the cursor.

  ```text
  ╔═══════════════════════ Workspaces ═══════════════════════╗
  ║ Filter:                                                  ║
  ║ ──────────────────────────────────────────────────────── ║
  ║ noon                                              3 tabs ║
  ║ deploy                                            2 tabs ║
  ╚══════════════════════════════════════════════════════════╝
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
  ║  󰍹 Interface │ Language           auto                           ▲
  ║  󰓡 Transfers │ Theme              < mc-classic >                 █
  ║  󰣀 SSH       │ Borders            < Double ═ ║ ╔ >               █
  ║  󰋊 Volumes   │ Icons              [x]                            ░
  ║  󰥨 zoxide    │ Show hidden files  [x]                            ▼
  ╟──────────────┴───────────────────────────────────────────────────╢
  ║ A language tag, such as en-US, or auto for the system locale.    ║
  ║ Takes effect after a restart.                                    ║
  ╚══════════════════════════════════════════════════════════════════╝
```

- Categories are on the left, with a Nerd Font icon each (without `ui.icons`, names only);
  the settings of the chosen one are on the right: check boxes `[x]`, choices `< … >`, and
  text fields. Every option of `config.toml` is there, so that nobody has to edit the file:
  Interface (`[ui]`), Transfers (`[transfer]`), SSH (`[ssh]` and the hidden hosts of
  `[discovery]`), Volumes (`[volumes]`), zoxide (`[zoxide]`), Command line (`[shell]`), and
  Search (`[search]`).
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
- Space or Enter switches a check box or picks the next choice, and `h` and `l` pick choices: each
  takes effect at once. The dialog opens on the language, a text field, in Insert mode; a text field
  the cursor comes to with `j` or `k` is in Normal mode, where `i`, `a`, or `A` goes to Insert mode,
  and one it comes to with Tab or Shift+Tab, a click, or from a field in Insert mode, in Insert mode
  (see [Text fields](#ui)); what Visual mode selects shows in reverse. A text field takes effect
  when the cursor leaves it (Up, Down, Tab, `g g`, `G`) or on Enter, and as the dialog closes. A
  count before `j` or `k` moves as many rows, past the text fields too. Its value is checked first:
  the language tag, a whole number of parallel jobs, a program for ssh, and the extra ssh arguments,
  through the same validator as at start ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)).
  An invalid one keeps the cursor in the field; Esc puts back what it had and closes the dialog.
- A change is used at once, but the language, which the next start reads: the interface
  redraws, copies and the job queue follow `[transfer]`, and the settings in `Context`, which
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
  that reports every entry does not draw every entry.
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
  SIGHUP, and SIGINT end the event loop like a quit.
- Another program (the editor of F4, a command of the command line, the output of commands of
  `g <`) gets the terminal as the shell would give it: the event loop shows the cursor, gives it
  the terminal's own shape back, leaves the alternate
  screen and raw mode, and drops its `EventStream` first. The stream's reader thread holds
  crossterm's input lock while it waits, and a new stream takes that lock, so the old one must go
  before the program starts, or its reader eats the first key, and the new one is made after it. The
  loop waits for the program, then takes the terminal back and draws everything. Ctrl+c in the
  program reaches Noon Commander too, so the SIGINT stream is made anew; SIGTERM and SIGHUP wait
  until the program ends. ssh children are in sessions of their own and see none of it.
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
operations and while it waits; so does a UI that stops listening. It returns an `Outcome`:
entries done, entries skipped, and whether it was aborted.

`Progress` carries two byte counts. `bytes_done` drives the gauge: it takes a skipped file's
size at once and goes back to the start of a file that is retried. `bytes_copied` counts only
bytes read and written and never goes back, so the UI derives the average speed from it and the
time left from both. The job's window times only work: its stopwatch starts with the first
`Progress` and stands from a `Failed` or `Exists` question until the answer, including while
the question waits behind other dialogs.

Renaming one entry in its directory (`rename`) is no job: one call with no progress, which
the UI runs like a new directory, through the host's shared session (`Arc<SftpFs>`) for a
remote one. It returns `RenameError::Exists` with the kind of a taken name, and replaces a
file there only when asked.

Deleting counts the entries first, so that progress has a total, then removes the deepest
first. Listings report symlinks without following them, so a link goes and its target stays.
A directory in which something stays (skipped, or unreadable) is left alone without asking
again, and an entry that is already gone counts as deleted.

Copying works between any two backends (`copy` takes an `Endpoint` for each side: a `Vfs`
and how its paths are reported). If the target is a directory, the sources go into it under
their own names; otherwise a single source becomes the target, and several make it a new
directory. Directories merge into a directory of the same name; symlinks are copied as
symlinks, with their targets as stored; FIFOs, sockets, and devices fail. The job counts
entries and bytes first, then copies parents before what they hold, reporting progress after
every chunk. With `preserve`, copies get the modification times and permission bits of their
sources, and directories that the copy made get theirs last, since writing into a directory
changes its time; directories that were there keep their own. With `atomic`,
each file is written under a hidden temporary name next to its target
(`.name.noc-PID-N`) and renamed when complete, so the target never holds part of a
file; without it, the target is written directly. Either way, a file that does not finish
(an error, Skip, or cancellation) is removed; written directly over an existing file, that
file is gone too.

When the name of a file or symlink is taken, the job sends `Exists` with the metadata of both, for
sizes and times, and waits for a `Conflict`: Overwrite, Skip, Overwrite all, Skip all, Overwrite
older (every later one that is older than its source; unknown times keep the target), or Abort. With
`overwrite`, the job replaces without asking, as when an edited file goes back where it came from. A
directory where a file would go is a failure, not a question. A symlink in the way is removed first,
so that a file written directly never goes through it; renaming replaces it anyway. Without
`posix-rename`, the target is removed before the rename.

Moving places sources as copying does. Between file systems (local and a host, or two
hosts), it is a copy with `remove_sources`: each file or symlink goes from the source once
it is copied, and each directory once everything in it has gone, so whatever is skipped or
fails stays, with the directories around it. Within one file system (`move_within`), each
source is renamed, after the same question if its name is taken by something a rename
replaces; a directory whose name is taken by a directory merges into it by copying and
removing, and so does anything the rename refuses as crossing file systems (`EXDEV`
locally, or a plain failure over SFTP v3, which has no code for it). Moves keep times and
permissions.

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
theme = "mc-classic"             # "terminal", "noon-dark", "noon-light", "catppuccin-mocha", …
borders = "double"               # frames of panels and dialogs: ═ ║ ╔; "single": ─ │ ┌
icons = true                     # Nerd Font icons; false: mc's markers (/ * @ ~ …)
show_hidden = true               # names that start with a dot; o h switches while running
search_mode = "fzf"              # quick search and filters; "plain": the text; "regex"; Alt+r
menu_bar = "on-demand"           # the F9 menu bar while a menu is open; "always": above the panels
tab_bar = "line"                 # tabs on a line above the panels; "frame": in the panel's frame
terminal_title = true            # the window's title shows the active panel's directory
which_key = true                 # after g, Ctrl+w, …: the keys that can follow, as which-key
which_key_delay_ms = 500         # how long after such a key the hints show

[transfer]
atomic_upload = true             # copies go to a hidden temporary name, then are renamed
parallel_jobs = 2                # jobs that run at once; later ones wait; F4 never waits

[zoxide]
program = "zoxide"               # name in PATH or a path
record = true                    # add directories where the user did something

[shell]
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
and the other tables, and replaces the file atomically. The settings sit behind a lock in the
shared `Context`, so the TUI and the host tasks see a change at once.

```toml
["prod-web"]                     # an ssh_config alias; decorates it, never duplicates it
type = "sftp"
label = "Prod"
start_dir = "/var/www"           # opened on connect instead of the remote home
other_dir = "~/projects/site"    # the other panel opens it with the host; / or ~/ for now
remember_dir = true              # reopen the last directory of this session
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
  search, and the command line in Insert mode. The line above the F-key bar shows the mode, `--
  NORMAL --`, `-- INSERT --`, or `-- VISUAL --`, on the left, and the count, register, operator,
  or keys of a command typed so far on the right, as typed, or how much Visual mode selects, as
  vim's `showmode` and `showcmd` do, though vim's shows no mode in Normal mode nor while it
  searches; after Ctrl+o it shows `-- (insert) --`. A message, such as
  that a search went round, takes the place of the mode until the next key, as vim's messages do
  on its command line. The command line takes it while open. The cursor is a bar in Insert mode
  and a block in the others (DECSCUSR), and the terminal's own shape comes back whenever another
  program gets the terminal, and on quitting.
- **Panels.** Each panel lists a directory with `..` first (but in a `/`), then directories,
  then files, by name ignoring case; columns are name, size, and modification time (local time,
  `ls -l` style), and narrow panels drop the time, then the size. Each panel has its own sort order:
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
  the scroll back as far as the names on screen need. A scrolled name shows `«` over its first
  cell and `»` where it is cut.
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
- **Free space.** As in mc, the bottom of a panel's frame shows the free space and size of the
  file system that holds the directory, and the share that is free: `123G / 500G (24%)`. It is
  read with every listing, so it changes when a job reads the panel again or on `R`; a panel
  on another directory of the same file system keeps what it read last. Locally it waits at
  most as long as a volume in the location menu, so a dead network mount leaves it out; over
  SFTP it needs the `statvfs@openssh.com` extension, which OpenSSH's server has. A file system
  that reports no size, or a panel too narrow for it, shows none.
- **Marks.** Space or Shift+Down marks the entry under the cursor, or unmarks it, and moves down,
  as mc's Insert does (Shift+Up moves up), and a count marks that many, the row at the end of the listing
  once; `g *` inverts the marks on files, leaving directories as they are; `..`
  cannot be marked. Marked rows are underlined, and yellow in mc-classic (bold, which mc uses
  without colors, is for directories); the line below the listing shows the size of the marked files
  and how many entries are marked, such as `12,345 B in 3 files`. Marks are names, so they survive
  sorting and `R` (for names still there); another directory starts unmarked, and entries that get
  hidden lose their marks, so that no operation acts on what is not shown. `g +` marks and `g -` unmarks
  the names that match a shell pattern, in a dialog with mc's options: Files only (off) and Case
  sensitive (on). Patterns are read as mc reads them: `*`, `?`, `[a-z]` (`[!…]` or `[^…]` outside
  the set), `{a,b}`, and `\` for the next character as it is; the whole name must match, and what
  does not parse is literal. mc's regular expressions are left out. The dialog opens with the last
  pattern (`*` at first) and options.
- **Visual mode.** `v` or `V` selects rows from the one under the cursor to wherever the cursor
  goes, as vim's Visual mode selects lines, and with a count as many rows (`3V`), or, after a
  command of Visual mode, as many as it took times the count, as vim's `resel_VIsual`; `o` goes to
  the other end, `g v` selects the rows selected last again, and Esc ends it, as does the key of `v`
  and `V` that started it, while the other switches its kind, as vim's; after `V` the line of the
  mode says `-- VISUAL LINE --`. Space marks the rows selected; `y`, `x`, `d`, `D`, `X`, F5, F6, F8,
  `f #`, and `g x` make them the marked entries and act on them, as on any marks, and put the
  cursor on the first row, as vim's operators do. The line of the mode shows how many entries it
  selects, without `..`. The other F-keys, Ctrl+w to the other panel, and the commands of the
  pull-down menu end Visual mode and do what they do in Normal mode, and so does the panel losing
  the keys to the other one or to another tab. The selection is held by the entries on its ends,
  so that a listing of the same directory or another sort order keeps it on them.
- **Jumps.** Each tab keeps a jump list, as vim does with `jumpoptions=stack`: going to another
  directory (Enter, `h`, Quick cd, a menu, zoxide, a workspace's tab), `G`, `g g`, `n`, `N`, and a
  quick search that found a match put where the panel was on it, up to 100 places, even when they
  stay on the row, as vim's; a jump after Ctrl+o keeps the place Ctrl+o went to, and drops those
  after it. `n` with a count puts it there once, and one that finds nothing, never; nor does a
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
  preserve attributes (times and permission bits; on, and remembered). A typed target is
  `host:path` for a host the app knows, or a path from the active panel's directory, as F7
  takes it. A target that is the source directory, or in one of the sources, is an error.
  The job's window shows entries and bytes, with the gauge on the bytes. A taken name asks in
  red, as mc does, with the path, both sizes and times, and Yes, No (the default), All, None,
  Older, and Abort. Copies are written under a temporary name and renamed when complete,
  unless `transfer.atomic_upload` is off.
  When the job ends, panels on the target, its parent, and the source directory read them
  again.
- **F3 views** the file under the cursor, as mc does (on a directory it opens it): the viewer
  (`noc-viewer`) takes the screen, with the path and the position (first line, lines, and how far
  the last line on screen is, or `End` once the end shows) on top, a scroll bar on the right when
  the text does not fit, and its own F-key bar. It reads the first 16 MiB, locally or
  through the host's shared session, in the background (closing the viewer stops that), and says so
  when the file is longer. Text is UTF-8 with invalid bytes replaced, tabs go to stops of 8, `\r\n`
  ends lines, and control characters are shown safely. Long lines wrap, each row after the first
  starting with `»`, as vim's 'showbreak' (F2 cuts them, `«` and `»` mark where they go on past
  the screen, as vim's 'listchars', and Left and Right, `h`, `l`, and the z keys as vim's scroll
  then, no further than the end of the longest line on screen at the right edge, and back as far
  as the lines on screen need after paging or widening); the
  position is a line and a row within it, so that only the lines in view are wrapped, whatever
  the size of the file. Keys follow vim and less: `j`/`k`, Ctrl+e/Ctrl+y, and the
  arrows a line, Ctrl+f/Ctrl+b and PgDn/PgUp a page but for the two rows that vim keeps, Space/`f`
  and `b` a whole page, as less, Ctrl+d/Ctrl+u and `d`/`u` half a page, rounded down as vim's
  'scroll'; unlike vim's, every key down stops with the end at the bottom, where a dimmed line
  across the text, `──── EOF ────`, follows the last line (`noc-scroll` has
  the steps of both and the scroll bar, [ADR 0023](adr/0023-paging-stops-at-the-end.md)); `g g`/`G`
  and Home/End the ends, and F3, F10, `q`, `i`, or Esc to close; F1 or `g ?` shows the help over
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
  of F5 (`Move "x" to:`, without Preserve attributes: moves keep them); a new name in the
  field renames in place, as in mc. Locally, and within one host, the job renames
  (`move_within`); between the local file system and a host, or two hosts, it copies and
  removes each source once all of it is copied. Its window says Moving; when it ends, the
  panels on both sides read their directories again.
- **Shift+F6 renames in place** the entry under the cursor, not the marked ones: its
  row becomes a text field from the name on, over the size and time columns, in Insert mode
  with the cursor before a file's last extension (after the whole name of a directory, a
  dotfile, or a name that ends with a dot), so that Ctrl+u takes away the rest of the name.
  Enter renames in every mode; Esc goes to Normal mode, where vim's motions and operators edit
  the name, and Esc there keeps the name; Ctrl+c keeps it in every mode; other panel keys do
  nothing (`Context::Rename`). The status line shows the old name. An empty or unchanged name
  renames nothing; one with `/` or NUL, or `.` or `..`, is an error and the field stays. A name
  that is not UTF-8 cannot be edited, which an error says: F6 renames it. `noc_ops::rename`
  checks the new name first: a directory's is an error, a file's asks in a red question with No
  as the default, and a name the file system takes for the entry itself, differing only in case
  or normalization and not listed byte for byte, renames it. Without `posix-rename` the file
  there goes first. A reload keeps the field on its entry; the field goes with the entry or when
  the panel goes elsewhere.
- **F8 deletes** the marked entries, or the one under the cursor, after a red question with Yes as
  the default, as in mc: `Delete file "x"?`, `Delete directory "x" and everything in it?`, or
  `Delete 3 files and directories?`. mc asks a second time before it goes into a directory that is
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
  terminal's window or tab with OSC 0 between frames: the active panel's directory as the
  prompt of the command line shows it (`~/src — noc`, `host:path` with the host's label),
  sanitized like every name. Before the first title it saves the terminal's own on xterm's
  stack of titles (`CSI 22 t`) and takes it back from there (`CSI 23 t`) on quitting and before
  handing the terminal to another program, which gets the shell's title and may set its own.
- **The other panel.** As vim's windows: Ctrl+w w, Ctrl+w Ctrl+w, or Ctrl+w p makes the other
  panel active; Ctrl+w x or Ctrl+w r swaps the panels, as mc's Ctrl+u, and the active one stays
  active on the other side; Ctrl+w f opens the directory under the cursor in the other panel, as
  mc's Alt+o (from a file, the parent directory with the cursor on this one) and moves the cursor
  down; Ctrl+w v shows this directory in the other panel with the cursor on the same name, as
  mc's Alt+i. Panels keep their identity when swapped (only where they are drawn changes), so a
  listing still in flight reaches the panel that asked for it. Sort order and errors go with the
  panel; mc keeps the sort order on its side. Ctrl+w h and Ctrl+w l make the panel drawn on that
  side active, whichever it is after Ctrl+w x.
- **Quick search.** `/` starts quick search, typed as vim's search, in Insert mode, and `?` starts
  it up the rows; typing in a panel in Normal mode does nothing but its commands, and `!` and `:`
  open the [command line](#command-line-of--and-). As vim's search, it looks from the row after the
  one where it started, round, or after `?` up from the row before it, round to the bottom, and a
  count before `/` or `?` goes to the match of that number (`2/`). It matches in the
  [search mode](#search-modes), which Alt+r switches while it runs. As fzf, the default, the cursor
  jumps to the best match as fzf ranks it, the first after where it started of those as good, and
  Ctrl+g to the next best, round to the best; in the plain and regex modes, to the first name after
  where it started that holds the text anywhere, ignoring case, or that matches it as a regular
  expression, as the filters of the lists match, and Ctrl+g to the next one, round to the top, as
  many as the count. It runs as vim's incremental search (`panel::Search`, with vim's `search_start`,
  `match_start`, and `old_viewstate`): every edit of the text searches again from where it searches
  from, with the rows on screen as they were before it; while nothing matches, the cursor stays
  there and the text as typed. Ctrl+g and Ctrl+t move where it searches from as vim's do, so that an
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
  marked with `»` in an accent. Names are shown terminal-safe: control and bidi characters become
  `?`. Listings run in background tasks; a reply carries the generation of its request, so a stale
  one is dropped. If a directory cannot be read, the panel stays where it was and says why below the
  listing. Going up puts the cursor on the directory just left. A panel shows a
  [`Location`](#locations), always a directory.
- <a id="command-line-of--and-"></a>**Command line of `!` and `:`** ([ADR
  0019](adr/0019-shell-command-line.md)). In a local panel, `!` opens a line above the F-key bar for
  a shell command, after a prompt with the panel's directory (`~` for home, its middle cut past a
  third of the width); `:` opens it for commands of Noon Commander, of which `!command` is the only
  one so far. In a panel on a connected host the prompt starts with the host's label or alias. The
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
  `noc-tools`' `Shell`: `$SHELL -c <command>` (`/bin/sh` without `$SHELL`) in the panel's directory,
  the command in one argument as typed. On a host, the host's task builds the ssh command from its
  session (`Session::command_in`) and the event loop runs it with the terminal: over the master,
  with `cd -- '<dir>' || exit` before the command. `tui/output.rs` shows the command after its
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
  key if it ran on another host, and `d d` or F8 removes it.
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
  1-second timeout (`g g`, `Z Z`, `Ctrl+w h`). The panels group keys under `g`, Ctrl+w (the other
  panel, as vim's windows), `t` (tabs and workspaces), `o` (sort order and hidden names), and `f`
  (`f j` the jobs, `f #` the checksums, `f /` and `f f` the search of the files below by text and
  by name, the rest of mc's Ctrl+x letters left for later). In Insert mode a character is text
  unless the context itself binds it, and in Normal and Visual mode a
  character nothing binds does nothing. The keymap reads what vim lets come with a command and hands
  it to the app with the action (`Typed`, `Extra`): a count before it (digits, `0` only after
  another), a register after `"`, and the character after the actions that take one (`f`, `t`, `F`,
  `T`, `r`, `"`, and Ctrl+r and Ctrl+v in Insert mode). The app repeats a motion over rows a count's
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
  mode ([ADR 0022](adr/0022-vim-modes.md)). On a password they do nothing, as the `Esc` would close
  its dialog and send the key to the panels. The pull-down menu takes Alt and a letter as its hotkey
  instead. Keys are written with `crokey` names. User overrides of the keymap are planned. The F-key
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
  goes up to Keys and folds it; in the text `h` goes back. A click chooses a section, and one on
  Keys opens or folds it. The text scrolls as the viewer does, with the keys it does not take in the
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
  last key (`Keymap::hints_due`), and it then waits for the next key. The action `key_hints` shows
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
  after the keys they had typed (`Keymap::sequences`), one row for each command; `/` filters it,
  Enter closes it and runs the command where the keys went before, with that count and register,
  and Esc closes it. `Esc` never starts a sequence, and never shows hints. The
  `KeyState` also keeps what the pending command has typed, which the line above the F-key bar shows
  (`KeyState::showcmd`).
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
- **Dialogs.** Modal and centered over the panels, with mc-style buttons: `[< OK >]` marks the
  default one, and a line across the dialog (`╟───╢`, or `├───┤` with single lines) sets the buttons
  apart from what is above them, in every dialog and window. A dialog has a message, radio buttons
  (`(*)` on the chosen one), text fields, check boxes, and buttons, each optional but the buttons;
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
- **Text.** Fluent files under `crates/noc/i18n/`, embedded in the binary and read with
  `fl!` from `i18n-embed-fl`, which checks message IDs against `en-US` at compile time; only
  `en-US` for now. `ui.language = "auto"` follows the system locale (through `sys-locale`).
  Arguments are inserted without Unicode isolation marks, which terminals would show.
- **Themes.** Built in: `mc-classic`, the colors of mc's default skin (blue panels, a cyan
  cursor that replaces the row's colors, a dark gray one under them in the inactive panel,
  yellow headers; directories white, executables green, broken links red, devices magenta; gray
  dialogs with mc's shadow; a black-and-cyan F-key bar; red error dialogs); `terminal`, the
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
  titles; directories `blue`, executables `green`, symlinks `teal`, broken links `red`,
  devices `pink`; hosts `overlay1`, `yellow`, `green`, `red` by state. The Noon themes are
  built the same way from a palette of roles taken from the logo: the cursor, the active
  panel's title, and focused buttons are on the logo's gold (`#A46D00`), and the menu bar on
  its amber frame (`#996400`), with cream text; the F-key bar is dark, its numbers bold in the
  color of the headers; `noon-dark` has the logo's navy panels and dialogs a shade lighter,
  cream directories, and bright gold headers (`#FFC24A`), which read better on navy;
  `noon-light` has cream panels (`#FFFBEA`) with navy text, pale blue-gray dialogs, the gold
  and the amber twice as light (`#FFC24A`, `#FFB833`) with navy text, and a dark amber for
  headers, which the bright gold is too pale for on cream. These themes are 24-bit RGB where
  `COLORTERM` is `truecolor` or `24bit`; elsewhere each is the nearest of the 6×6×6 cube and
  the gray ramp of the 256-color palette, never the 16 colors below them, which the terminal's
  palette redefines. User themes in `themes/` are planned for M4.
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
- **Glyphs.** Every special character the UI draws is in `tui/glyphs.rs`: `Glyphs::NERD` and
  `Glyphs::PLAIN`, which `ui.icons` picks and the theme carries (`Theme.glyphs`), with the icons
  and markers of kinds, hosts, and volumes, the scroll bars, the marks of text cut short, and the
  marks of the pull-down menu; `Lines::DOUBLE` and `Lines::SINGLE` by `ui.borders`, with the
  tees that join lines to frames; and the characters that are the same either way: the single
  line, the line between columns, the spinner, the arrows of the sort order, the checks of
  checksums, the progress bar, the marks of dialogs' radio buttons, check boxes, and secrets,
  and the icons of the Configuration dialog's categories. `decor.rs` picks among them by kind,
  and asks devicons for files. `noc-scroll`, `noc-text`, and `noc-viewer` hold none: they take
  what they draw with as parameters (`BarGlyphs`, `Marks`, the viewer's `Styles`). `just
  glyphs`, part of `just lint`, fails on a special character written in Rust code that draws,
  outside `glyphs.rs` and the tests.

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
