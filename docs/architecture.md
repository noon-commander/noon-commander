# Architecture

Noon Commander (`noc`) is a two-panel terminal file manager for local and SFTP file operations.
SFTP is its foundation and the reason it exists. It never implements SSH: every connection is a
process of the system OpenSSH client, and Noon Commander speaks the SFTP protocol over that
process's stdin and stdout.

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
├── noc-ops/       job engine: copy, move, delete, mkdir, checksums; progress, cancellation, conflicts
└── noc-tools/     external programs other than ssh: zoxide, the editor
```

Dependencies point one way: `config ← ssh ← vfs ← ops ← noc`, and `tools ← noc`. Library
crates contain no UI code and no user-facing text; they return typed errors and events, and the
UI turns them into messages.

ssh runs only from `noc-ssh`, and every other program only from `noc-tools`
([ADR 0012](adr/0012-external-tools-and-zoxide.md)): without a shell, with `--` before paths,
without input for background ones, and killed when their future is dropped or they time out.
A program that is not installed is told apart from one that fails.

## Processes

Each connected host has one master connection; everything else is multiplexed over its control
socket ([ADR 0002](adr/0002-controlmaster-per-host.md)):

```text
ssh <master options> -M -N -S <sock> -o ControlPersist=no -- <alias>  # authenticates once
ssh <channel options> -S <sock> -T -s -- <alias> sftp                 # panel channel
ssh <channel options> -S <sock> -T -s -- <alias> sftp                 # transfer channels
ssh -S <sock> -t -- <alias> 'cd <dir> && exec $SHELL -l'              # console (backlog)
ssh -F /dev/null -S <sock> -O exit -- noc                             # disconnect
```

The SFTP protocol client is `openssh-sftp-client`, whose `Sftp::new` works over the pipes of any
child process. Every ssh child runs in its own session (`setsid`), without a controlling
terminal, so it can neither read from nor draw on the TUI's terminal. ssh runs in the home
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

## Virtual root

The root of the virtual file system lists the home directory, the mounted volumes, and, as one
row, the hosts from the ssh config ([ADR 0006](adr/0006-virtual-root-with-volumes-and-hosts.md)):

```text
╔ alex-mbp ═══════════════════════════╗╔ SFTP ═══════════════════════════════╗
║          Name        │ Free  │ Size ║║        Name       │     Address     ║
║~ Home                │   212G│  994G║║/..                │UP--DIR          ║
║+ Macintosh HD        │   212G│  994G║║● prod-web         │deploy@10.0.0.5  ║
║+ SANDISK             │    12G│   64G║║○ staging          │ubuntu@stg:2222  ║
║+ share               │      ?│     ?║║✗ nas              │admin@nas        ║
║/ SFTP                │        3 hosts║║                                     ║
║● prod-web            │deploy@10.0.0.5║║                                     ║
╟─────────────────────────────────────╢╟─────────────────────────────────────╢
║/Volumes/SANDISK  exfat              ║║prod-web                             ║
╚═════════════════════════════════════╝╚═════════════════════════════════════╝
 1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit
```

- The root is titled with the name of the machine. `Home` comes first and opens the home directory,
  with the space of the volume that holds it; the status line shows its path. The volumes follow,
  the system volume (`/`) on top and the others by mount point, named by their label or else their
  mount point, with their free space and size; the status line shows a volume's mount point and file
  system. Every volume opens at its mount point, the system volume at `/`, so `..` from `/` and
  Enter lead back where the panel was.
- Below the volumes, `SFTP` opens the list of hosts, and the hosts that are connected or
  connecting follow it, so that they stay one keystroke away.
- `..` follows paths, as in mc: from `/Volumes/USB` to `/Volumes`; from a local `/` to the root,
  with the cursor on the system volume; from a remote `/` (or the remote home directory) to the
  list of hosts, with the cursor on the host; from the list of hosts to the root.
- Volumes come from `noc_vfs::volumes`: on macOS `/` and the entries of `/Volumes`, without
  symlinks and without mounts marked `nobrowse` (Time Machine snapshots, for instance); on
  Linux the mounts of `/proc/self/mountinfo` that are `/`, on a `/dev/` device, or a network
  file system, without pseudo file systems or those under `/proc`, `/sys`, `/dev`, `/run`
  (but `/run/media`), `/snap`, and `/boot`; elsewhere only `/`. Their sizes come from
  `statfs`/`statvfs` on blocking threads, each awaited for at most 500 ms: a volume that does
  not answer (a dead network mount can block forever) is listed with `?` and is not asked
  again until the earlier call returns. `[volumes] hide` leaves out mount points by pattern;
  the system volume always stays.
- Hosts come in config order, named by their `label` from `hosts.toml` if set (the status line shows
  the alias), with the address cached from an earlier `ssh -G`. The root and the list of hosts
  are listed like directories, in background tasks that read the volumes, scan the ssh config,
  and load the cache, so Ctrl-R reads them again.
- Entering a host connects in the background (the status line says so; Esc stops it) and opens,
  shown as an absolute path, the last directory shown on it in this session if `remember_dir`
  is set and the directory is still there, else the configured `start_dir`, else the remote home
  directory. If the host has an `other_dir`, the other panel opens it at the same time.
- F4 on a host edits its settings in a dialog (label, remote directory, other panel directory,
  remember the last directory); Use Current fills in the directory a panel shows on the host.
  They are saved to `hosts.toml` in the background, and the lists of hosts are read again.
- When a connection is lost, the panels on that host go back to the list of hosts and say why.
- The icon in front of each host shows its state, in a color of its own: a server, gray when
  not connected and green when connected; a spinner while connecting; a server with a cross,
  red, when the last attempt failed or the connection was lost. In the `terminal` theme, which
  has no colors, connected hosts are bold and the others dim. Without icons, the markers are
  `○`, `●`, the spinner, and `✗`. Every row has one cell in front of its name, so names line
  up across volumes, hosts, and `..`. F8 (`Esc 8`) on a host closes the connection to it, or
  stops connecting to it.
- Locations are `Root`, `Sftp` (the list of hosts), `Local(PathBuf)`, or `Remote { host, path }`.
  Remote paths are bytes, because SFTP v3 does not guarantee UTF-8, and are displayed lossily.
  The SFTP client library still requires UTF-8 names; see the known issues in the
  [roadmap](roadmap.md).
- Names from the server that are empty or contain `/` or NUL are dropped from listings: joined
  to a local path, they could point outside the target directory.

### Location menu

Alt-F1 and Alt-F2, as Far Manager's menus to change drives, open a menu over the left or the right
panel with the same places: the home directory, the volumes, then every host (Ctrl-X 1 and Ctrl-X 2
too, for terminals whose Alt-F1 never arrives, such as macOS Terminal without Option as Meta).

```text
 ╔════════════════════ Left ════════════════════╗
 ║ Filter:                                      ║
 ║ ──────────────────────────────────────────── ║
 ║ 1 ~ Home                                212G ║
 ║ 2 + Macintosh HD                        212G ║
 ║ 3 + USB                                  12G ║
 ║ 4 + share                                  ? ║
 ║ ─ SFTP ───────────────────────────────────── ║
 ║ 5 ● Prod                     deploy@10.0.0.5 ║
 ║ 6 ○ db                                       ║
 ╚══════════════════════════════════════════════╝
```

- The cursor starts on the home directory or the volume that holds the panel's directory,
  whichever is nearer, or on its host. Enter
  opens the row in that panel, which becomes active; while the filter is empty, `1` … `9` and
  `0` open the first ten rows.
- Typing filters the rows by name, mount point, alias, and address, ignoring case; Backspace
  takes a character back. F8 disconnects the host under the cursor, Ctrl-R reads the volumes
  and hosts again, and Esc or F10 closes the menu.
- The menu is modal (keymap context `menu`) and lists the root in the background; its listing
  carries a generation of its own, so a stale one is dropped.

### Pull-down menu

F9 (`Esc 9`) opens mc's menu bar, as Far does: the first time, the bar alone with the menu of
the active panel selected; later, where it was when it closed. An open menu:

```text
  Left     File     Command     Options     Right                       2 jobs 37%
 ╔══════════════════════════════╗
 ║   Change location…    Alt-F1 ║
 ╟──────────────────────────────╢
 ║ • Sort by name       Ctrl-F3 ║
 ║   Sort by extension  Ctrl-F4 ║
 ║   Sort by time       Ctrl-F5 ║
 ║   Sort by size       Ctrl-F6 ║
 ╟──────────────────────────────╢
 ║   Rescan              Ctrl-r ║
 ║   Disconnect                 ║
 ╚══════════════════════════════╝
```

- Left and Right act on the panel drawn on that side, after Ctrl-U too: its location menu,
  sort order (`•` marks the current one), Rescan, Disconnect while it shows a host, and its
  [tabs](#tabs): New tab, Close tab, and Tab list…. File
  has F3 … F8, `+`, `-`, `*`, Checksums, and Exit; Command has quick search,
  [Quick cd](#quick-cd), the [zoxide](#zoxide) window, the other-panel
  commands, the jobs, host settings and disconnect for the host under the cursor, help, and
  redraw; Options has Configuration… and Show hidden files (`✓` while on). Without icons, the
  marks are `*` and `x`.
- Commands do what their keys do, through the same `Action`s. Each shows the key that does
  it in the active panel's context (`Keymap::key`); Left and Right show sort and rescan keys
  only for the active panel, as keys act there. Commands that cannot run now are dimmed, and
  the cursor skips them.
- On the bar, Left and Right select the next menu, round the bar; Enter, Up, Down, or the
  menu's highlighted letter (`&` in the Fluent messages) opens it. In an open menu, Left and
  Right open the next one; Up and Down move round it; Home and End go to its first and last
  command; Enter or the command's highlighted letter runs it. Esc, F9, or F10 goes back to the
  bar, and closes the bar from there. Running a command closes it first.
- The app keeps the menu's `Place` when it closes: the selected menu, whether it was open, and
  the cursor of each menu. F9 opens it there again, so F9 Enter repeats the last command; a
  command that cannot run now passes the cursor on to the next one.
- `ui.menu_bar` decides where the bar is: `on-demand` (the default) draws it over the top line
  of the panels only while a menu is open, as Far does; `always` keeps it above the panels, as
  mc does, which takes a row from them. The jobs indicator sits at the right end of that row.
- The menu is modal (keymap context `pull_down`); letters are text there, so they are hotkeys.

### Quick cd

Alt-C (`Esc C`), as mc's Quick cd, asks for a path and opens it in the active panel, as `cd`
in a shell reads it: relative to the panel's directory, `/…` absolute, `~` and `~/…` the home
directory, `..` and `.` resolved by name, `-` the directory the tab showed before, and
`host:path` on a host, scp-style, which connects to it if needed. On a host, paths stay on it
and `~` is the remote home. From the volumes and hosts, relative paths start at the home
directory. `cd ..` puts the cursor on the directory left, as Ctrl-PgUp does; a path that is not
there leaves the panel where it is, with the reason below the listing. It is in F9 → Command.

### zoxide

[zoxide](https://github.com/ajeetdsouza/zoxide) ranks the directories the user works in
([ADR 0012](adr/0012-external-tools-and-zoxide.md)). Alt-Z (Ctrl-X Z) opens a window of its
best directories, which Enter opens in the active panel:

```text
╔═════════════════════════════ zoxide ═════════════════════════════╗
║ Jump to: src                                                     ║
║ ──────────────────────────────────────────────────────────────── ║
║ 1 ~/src/noc                                                 56.0 ║
║ 2 /srv/www/src                                              12.5 ║
╚══════════════════════════════════════════════════════════════════╝
```

- Typing gives zoxide keywords, as `z foo bar` in a shell; each change asks
  `zoxide query --list --score --exclude <dir> -- <keywords>` again, leaving out the panel's
  directory, and a query still running is dropped. While nothing is typed, `1` … `9` and `0`
  open the first ten rows. Directories under the home directory show from `~`.
- A local directory goes to zoxide (`zoxide add -- <dir>`) once the user does something in it,
  at most once a visit: a copy, move, or delete from it, a copy or move into it when a panel
  shows it, F7, F3 or F4 on a file, checksums, or a jump there. Passing through, marking,
  searching, sorting, and Ctrl-R do not count. Each tab remembers whether its visit counted;
  going to another directory starts a new one. One task adds them in turn, so no two zoxide
  processes write its database at once.
- `zoxide.record` turns recording off; without zoxide the window says it is not installed,
  and recording is logged once and stops until `zoxide.program` changes.

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
- Ctrl-X T opens a new tab after the one that shows, on the same directory with the same
  listing and cursor, without marks; Ctrl-X W closes the tab that shows (the last one stays),
  and the next one shows. Alt-Right and Alt-Left, or Ctrl-X N and Ctrl-X P, go round the tabs;
  Ctrl-X Tab lists them, with their whole locations, to choose one. Left and Right in F9 do
  the same for the panel on that side.
- Names on the line of tabs are brief: the last component of a directory, `~` for the home
  directory, `host:name` on a host, and the host alone in its start directory. The frame's
  verticals close each side's line, and its top corners become tees (`╠`, `╣`) that join it,
  so the two sides stay apart. When the tabs do not fit, the names of hidden
  tabs shrink first, then the one that shows, none below eight cells; then tabs far from it
  are left out, and `‹` and `›` say so.
- Panels are named by a `PanelId`, their side and tab number, which is never used again.
  Listings, new directories, and the dialogs of `+`, `-`, F5, F6, and F7 carry it, so a reply
  reaches the tab that asked even after a switch, and one for a closed tab is dropped. What
  acts on the other side (Alt-O, Alt-I, F5's target, `other_dir`) acts on the tab that shows
  there. Disconnecting or losing a host sends every tab on it back to the list of hosts.
- Tabs look like tabs: the line is dark (`tab`: gray on black in mc-classic, as the F-key
  bar; `crust` in Catppuccin; dim text in `terminal`), and the tab that shows takes the
  panel's colors as if it grew out of it (`tab_active`), with its number in an accent on the
  side that has the keys (`tab_number`) and plainer on the other (`tab_active_idle`). Tabs in
  the frame take the same styles, with the frame's line between them.
- Ctrl-U swaps the sides with their tabs. Alt-. and the settings apply to every tab.

### Configuration dialog

Options → Configuration… shows the settings of `config.toml` by category, as a modern
settings window: there is no OK or Cancel, and every change takes effect as it is made
([ADR 0009](adr/0009-configuration-dialog-writes-config-toml.md)):

```text
  ╔═════════════════════════ Configuration ══════════════════════════╗
  ║  󰍹 Interface │ Language           auto                         █ ║
  ║  󰓡 Transfers │ Theme              < mc-classic >               █ ║
  ║  󰣀 SSH       │ Borders            < Double ═ ║ ╔ >             ░ ║
  ║  󰋊 Volumes   │ Icons              [x]                          ░ ║
  ║  󰥨 zoxide    │ Show hidden files  [x]                          ░ ║
  ╟──────────────┴───────────────────────────────────────────────────╢
  ║ A language tag, such as en-US, or auto for the system locale.    ║
  ║ Takes effect after a restart.                                    ║
  ╚══════════════════════════════════════════════════════════════════╝
```

- Categories are on the left, with a Nerd Font icon each (without `ui.icons`, names only);
  the settings of the chosen one are on the right: check boxes `[x]`, choices `< … >`, and
  text fields. Every option of `config.toml` is there, so that nobody has to edit the file:
  Interface (`[ui]`), Transfers (`[transfer]`), SSH (`[ssh]` and the hidden hosts of
  `[discovery]`), Volumes (`[volumes]`), and zoxide (`[zoxide]`).
- Paths under the home directory show from `~`, and are written as typed. Lists, such as
  `ssh.args` and the hidden hosts and volumes, are words separated by spaces, as a shell
  reads them: `"…"` around a word with spaces, `\` before a character to take it as it is.
- Settings that do not fit scroll with the cursor, and a scroll bar (`█` on `░`) shows in the
  last column. The two lines below them, set apart by a line across the dialog, say what the
  setting under the cursor does and whether it takes effect only after a restart, or, in
  the colors of errors, why the text typed cannot be used.
- Tab moves between the categories and the settings; Up and Down move within them, and Home
  and End go to the first and last category; Left on anything but a text field goes back to
  the categories; Esc or F10 closes the dialog.
- Space or Enter switches a check box or picks the next choice, and Left and Right pick
  choices: each takes effect at once. A text field takes effect when the cursor leaves it
  (Up, Down, Tab) or on Enter, and as the dialog closes. Its value is checked first: the
  language tag, a whole number of parallel jobs, a program for ssh, and the extra ssh
  arguments, through the same validator as at start
  ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)). An invalid one keeps the cursor
  in the field; Esc puts back what it had and closes the dialog.
- A change is used at once, but the language, which the next start reads: the interface
  redraws, copies and the job queue follow `[transfer]`, and the settings in `Context`, which
  sit behind a lock, change for new connections, `ssh -G`, and listings, so panels on the
  root or the list of hosts read them again. Connections that are open stay as they are.
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
   when the TUI connects to a host (or for `noc hosts --resolve`, with bounded
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
noc                    the TUI (needs a terminal)
noc hosts [--resolve]  hosts from ssh_config with cached addresses; --resolve runs ssh -G
noc ls [LOCATION]      a local path or host:path; without one, the mount points and hosts
noc config init        write the commented default config.toml
noc config paths       show the files and directories in use
```

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
- Another program (the editor of F4; the Ctrl-O console later) gets the terminal as the shell
  would give it: the event loop shows the cursor, leaves the alternate screen and raw mode,
  and drops its `EventStream` first. The stream's reader thread holds crossterm's input lock
  while it waits, and a new stream takes that lock, so the old one must go before the
  program starts, or its reader eats the first key, and the new one is made after it. The
  loop waits for the program, then takes the terminal back and draws everything. Ctrl-C in
  the program reaches Noon Commander too, so the SIGINT stream is made anew; SIGTERM and SIGHUP
  wait until the program ends. ssh children are in sessions of their own and see none of
  it.
- The working directory of the process follows the active panel: after each turn of the
  event loop, a local directory the active panel now shows (once its listing arrived)
  goes to a task that `chdir`s to it in `spawn_blocking`, the latest of the waiting ones only.
  A remote panel or the virtual root leaves it where it was. Programs that the app starts,
  such as the editor, inherit it.

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
| Data (bookmarks) | `~/.local/share/noc/` |
| State (history, last directories, logs) | `~/.local/state/noc/` |
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
hide = ["/Volumes/Backup*"]      # mount points to leave out of the root; never the system volume

[ui]
language = "auto"                # or a language tag such as "en-US"; others fall back to it
theme = "mc-classic"             # "terminal", "noon-dark", "noon-light", "catppuccin-mocha", …
borders = "double"               # frames of panels and dialogs: ═ ║ ╔; "single": ─ │ ┌
icons = true                     # Nerd Font icons; false: mc's markers (/ * @ ~ …)
show_hidden = true               # names that start with a dot; Alt-. switches while running
type_to_search = true            # typing in a panel starts quick search; false: only Ctrl-S
menu_bar = "on-demand"           # the F9 menu bar while a menu is open; "always": above the panels
tab_bar = "line"                 # tabs on a line above the panels; "frame": in the panel's frame

[transfer]
atomic_upload = true             # copies go to a hidden temporary name, then are renamed
parallel_jobs = 2                # jobs that run at once; later ones wait; F4 never waits

[zoxide]
program = "zoxide"               # name in PATH or a path
record = true                    # add directories where the user did something
```

Preserving attributes is a choice in the copy dialog, as in mc, not a setting.

Host settings live in `hosts.toml` next to `config.toml`, one table per host with a required
`type` that decides its other keys ([ADR 0007](adr/0007-typed-host-settings-in-hosts-toml.md)).
F4 on a host rewrites only its table through `toml_edit`, keeping comments and the other tables,
and replaces the file atomically. The settings sit behind a lock in the shared `Context`, so the
TUI and the host tasks see a change at once.

```toml
["prod-web"]                     # an ssh_config alias; decorates it, never duplicates it
type = "sftp"
label = "Prod"
start_dir = "/var/www"           # opened on connect instead of the remote home
other_dir = "~/projects/site"    # the other panel opens it with the host; / or ~/ for now
remember_dir = true              # reopen the last directory of this session
```

## UI

- **Panels.** Each panel lists a directory with `..` first, then directories, then files, by
  name ignoring case; columns are name, size, and modification time (local time, `ls -l`
  style), and narrow panels drop the time, then the size. Each panel has its own sort order:
  name, extension, modification time, or size (Far's Ctrl-F3 … Ctrl-F6, as mc binds none;
  macOS keeps them for keyboard navigation unless that is turned off in its settings);
  time and size start newest and largest first, the same key again reverses, ties go by name,
  and an arrow in the header marks the order. Directories stay first. Names that start with a
  dot are shown unless `ui.show_hidden` is off; Alt-. switches them in both panels, as in mc.
  Sorting and hiding keep the cursor on its entry.
- **The cursor of the inactive panel.** Unlike mc, the inactive panel shows where its cursor
  is (the checksum comparison reads the file there): its row gets a background of its own
  under the row's colors, which stay (a mark too). The theme sets it as `cursor_inactive`:
  the dark gray of the 16 colors in `mc-classic`, dimmed reverse video in `terminal`, the
  dialogs' `surface0` in the Catppuccin themes, and in the Noon themes the text fields' blue
  (`#2D4672`) in `noon-dark`, as the dialogs' one is a single step of gray above the panels in
  256 colors, and the dialogs' blue-gray (`#E8ECF4`) in `noon-light`.
- **Free space.** As in mc, the bottom of a panel's frame shows the free space and size of the
  file system that holds the directory, and the share that is free: `123G / 500G (24%)`. It is
  read with every listing, so it changes when a job reads the panel again or on Ctrl-R; a panel
  on another directory of the same file system keeps what it read last. Locally it waits at
  most as long as a volume of the virtual root, so a dead network mount leaves it out; over
  SFTP it needs the `statvfs@openssh.com` extension, which OpenSSH's server has. A file system
  that reports no size, or a panel too narrow for it, shows none.
- **Marks.** As in mc: Insert or Ctrl-T marks the entry under the cursor, or unmarks it, and
  moves down (Shift-Down too, Shift-Up moves up); `*` (or Alt-*) inverts the marks on files,
  leaving directories as they are; `..`, volumes, and hosts cannot be marked.
  Marked rows are underlined, and yellow in mc-classic (bold, which mc uses without colors, is
  for directories); the line below the listing shows the size of the marked files and how
  many entries are marked, such as `12,345 B in 3 files`. Marks are names, so they
  survive sorting and Ctrl-R (for names still there); another directory starts unmarked, and
  entries that get hidden lose their marks, so that no operation acts on what is not shown.
  `+` (or Alt-+) marks and `-` (or `\`, Alt--) unmarks the names that match a shell pattern,
  in a dialog with mc's options: Files only (off) and Case sensitive (on). Patterns are read
  as mc reads them: `*`, `?`, `[a-z]` (`[!…]` or `[^…]` outside the set), `{a,b}`, and `\`
  for the next character as it is; the whole name must match, and what does not parse is
  literal. mc's regular expressions are left out. The dialog opens with the last pattern
  (`*` at first) and options. `+`, `-`, `\`, and `*` are commands, as in mc with an empty
  command line, so typing them does not start quick search.
- **F7 makes a directory**, as in mc: the dialog opens with the name under the cursor, which
  typing replaces. The name may be a relative path, an absolute one, or start with `~` for the
  home directory (the remote one on a host; `\~` for a name that starts with `~`); missing
  parents are not made. The directory is made in the background, locally or by the host's
  task, and panels on the directory it is in read it again, the one that asked with the
  cursor on it. An error shows in a red dialog, as mc shows errors. F7 is not offered in the
  virtual root or the list of hosts.
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
- **F3 views** the file under the cursor, as mc does (on a directory it opens it): the
  viewer takes the screen, with the path and the position (first line, lines, and how far
  the last line on screen is) on top, and its own F-key bar. It reads the first 16 MiB,
  locally or through the host's shared session, in the background (closing the viewer stops
  that), and says so when the file is longer. Text is UTF-8 with invalid bytes replaced,
  tabs go to stops of 8, `\r\n` ends lines, and control characters are shown safely. Long
  lines wrap (F2 cuts them, and Left and Right scroll then); the position is a line and a
  row within it, so that only the lines in view are wrapped, whatever the size of the file.
  Keys follow mc's viewer: arrows, `j`/`k`, PgUp/PgDn, Space and `b`, Home/End, `g`/`G`,
  and F3, F10, `q`, or Esc to close; F1 shows the help over it.
- **F4 edits** the file under the cursor in `$VISUAL`, else `$EDITOR`, else `vi`, split at
  spaces (`code -w`), without a shell. A local file is edited where it is. A remote one is
  copied, with its times and permission bits, to `edit-PID-N-name` in the runtime directory
  (the name last, so that the editor knows the kind of file); when the editor exits and the
  copy's size or time changed, it goes back over the original by the copy job, with
  `overwrite` and as `transfer.atomic_upload` says, keeping the original's permissions. Both
  copies show the job's window. The local copy is removed once it went back, or at once if
  it did not change; one that did not go back (a failure, Abort, or the host gone) stays, and
  a red dialog says where. Panels on the file's directory read it again.
- **F6 moves or renames** the marked entries, or the one under the cursor, with the dialog
  of F5 (`Move "x" to:`, without Preserve attributes: moves keep them); a new name in the
  field renames in place, as in mc. Locally, and within one host, the job renames
  (`move_within`); between the local file system and a host, or two hosts, it copies and
  removes each source once all of it is copied. Its window says Moving; when it ends, the
  panels on both sides read their directories again.
- **F8 (or Delete) deletes** the marked entries, or the one under the cursor, after a red
  question with Yes as the default, as in mc: `Delete file "x"?`, `Delete directory "x" and
  everything in it?`, or `Delete 3 files and directories?`. mc asks a second time before it
  goes into a directory that is not empty; Noon Commander says so in the first question instead.
  The job runs in a task of its own, or in the host's task, with a window that shows
  what it counts, the entry at hand, a gauge, and done/total; Esc or Abort stops it. A
  failure asks in a red dialog, with mc's buttons: Ignore, Ignore all, Retry, and Abort.
  When the job ends, panels on the directory read it again, and a cursor whose entry is gone
  stays on its row. If the host's connection is lost, the job ends with it.
- **Jobs.** A job's window has Background, the default, and Abort: Enter sends the job
  behind the panels, which take the keys again, and Esc aborts, as in mc. As many jobs run
  at once as `transfer.parallel_jobs` says (2); a later one waits, in front or behind, and
  starts when one ends, the oldest first. Abort takes a waiting job away at once. The jobs
  of F4 never wait, but count while they run. Ctrl-X J lists the jobs, as mc's Background
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
- **Checksums.** Ctrl-X # (not in mc) asks for an algorithm (radio buttons, the last one
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
- **The other panel.** As in mc: Ctrl-U swaps the panels, and the active one stays active on
  the other side; Alt-O opens the directory or host under the cursor in the other panel (from
  a file, the parent directory with the cursor on this one) and moves the cursor down; Alt-I
  shows this directory in the other panel with the cursor on the same name. Panels keep their
  identity when swapped (only where they are drawn changes), so a listing still in flight
  reaches the panel that asked for it. Sort order and errors go with the panel; mc keeps the
  sort order on its side.
- **Quick search.** Ctrl-S / Alt-S as in mc, or, since there is no command line, typing in a panel
  (unless `ui.type_to_search` is off) starts quick search: the cursor jumps to the first name from
  where it is that starts with the text, ignoring case, and a character that matches nothing is
  dropped, as in mc. Ctrl-S again finds the next match, round to the top; Backspace takes a
  character back; Esc ends the search, and any other key ends it and then does what it does. While
  it runs, every character is text, even one that a panel binds, such as `*`. The root and the list
  of hosts search the names they show: volume labels, labels, or aliases. Long names lose their
  middle, marked with `~`. Names are shown terminal-safe: control and bidi characters become `?`.
  Listings run in background tasks; a reply carries the generation of its request, so a stale one is
  dropped. If a directory cannot be read, the panel stays where it was and says why below the
  listing. Going up puts the cursor on the directory just left. A panel shows a `Location`, so the
  [virtual root](#virtual-root) and the list of hosts are kinds of listing too.
- **Keymap.** Keys map to `Action`s per context (`panel`, `root`, `quick_search`, `menu`,
  `pull_down`, `dialog`, `dialog_input`, `viewer`). Each context falls back along a chain, for
  example the root and quick search to the panel; the first context that knows a key sequence
  decides, except that a sequence it only starts does what a later context binds it to. Bindings are
  key sequences matched by prefix with a 1-second timeout, so a vim preset (`g g`, `d d`) can follow
  the default mc preset. As in mc, `Esc` in a panel waits for the next key: `Esc 1` … `Esc 0` stand
  for F1 … F10, `Esc` followed by a character stands for Alt and that character, for terminals whose
  Alt key sends nothing, and `Esc` alone cancels once the timeout passes (`Esc Esc` at once). An
  `Esc` and a quick next key arrive as Alt and that key, so there an unbound Alt and a character
  count as `Esc` and the character. In dialogs, quick search, and the menus `Esc` acts at once. Keys
  are written with `crokey` names. User overrides in `keymap.toml` are planned for M4. The F-key bar
  is generated from the active keymap, and so is the help screen (F1): the keys of each context,
  with what they do, for what the app can do already; a prompt from ssh shows over it.
- **Dialogs.** Modal and centered over the panels, with mc-style buttons: `[< OK >]` marks the
  default one, and a line across the dialog (`╟───╢`, or `├───┤` with single lines) sets the buttons
  apart from what is above them, in every dialog and window. A dialog has a message, radio buttons
  (`(*)` on the chosen one), text fields, check boxes, and buttons, each optional but the buttons;
  ssh's prompts and the app's own questions are the same kind of dialog, and each one in the queue
  knows where its answer goes. Keys go to the first dialog in the queue (contexts `dialog` and
  `dialog_input`); Tab and the arrows move between the radio buttons, the fields, the check boxes,
  and the buttons, Space chooses a radio button, switches a check box, or presses a button, Enter
  presses the button with the focus (the default one from elsewhere, after choosing the radio button
  it is on), and Esc or F10 cancels. In a text field every character is text, Space too. A field
  that opens with text shows it dimmed, and typing replaces it, as in mc; an edit or a cursor move
  keeps it. Long text scrolls to keep the cursor in view.
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
  directories, `..`, links, broken links, FIFOs, sockets, devices, executables, the home
  directory, volumes, network volumes, the list of hosts, and hosts (which tell their state);
  `devicons` for files by name or extension. devicons asks the disk whether a name it does not
  know is a directory, so names go to it inside a path with a NUL byte, which names
  nothing: drawing stays free of I/O, and remote names are never looked up locally. Without
  icons, mc's markers: `/` directory, `~` link to a directory, `@` link, `!` broken link, `*`
  executable, `|` FIFO, `=` socket, `-` character device, `+` block device and volume; the
  home directory is `~`, and the list of hosts `/`, as it opens like a directory.
