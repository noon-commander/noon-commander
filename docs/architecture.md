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
├── noc-vfs/       Vfs trait: virtual root, local, and SFTP backends
└── noc-ops/       job engine: copy, move, delete, mkdir; progress, cancellation, conflicts
```

Dependencies point one way: `config ← ssh ← vfs ← ops ← noc`. Library crates contain no UI
code and no user-facing text; they return typed errors and events, and the UI turns them into
messages.

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
terminal, so it can neither read from nor draw on the TUI's terminal.

`noc-ssh` API in short: `version::check_version` runs `ssh -V`; `resolve::resolve` runs
`ssh -G`; `Session::connect` starts the master (or nothing, without multiplexing);
`Session::open_sftp` returns the pipes of a new SFTP channel, which `SftpFs::from_pipes` in
`noc-vfs` turns into a file system; `Session::close` shuts down; `cleanup_stale` removes
leftovers of crashed instances.

The `Vfs` trait of `noc-vfs`, implemented by `LocalFs` and `SftpFs`, lists directories,
reads metadata with and without following symlinks, canonicalizes paths, creates and removes
directories, removes files, renames, reads and makes symlinks (their targets stored as given,
never resolved), sets permissions and modification times, and reads and writes files as chunks (`FileReader`, `FileWriter`, whose `finish` reports errors that only
show when a file is closed). Over SFTP, reads and writes of a file keep 64 requests of 32 KiB
in flight, as `sftp(1)` does, so a long round trip does not idle the link; a read that comes
back short is asked again for the rest. Symlinks go to the server in OpenSSH's argument order
(target first), which the client uses and which differs from the protocol draft; a server
that follows the draft would store them the other way round. Its futures are cancel-safe: a dropped one leaks
nothing, though a change already sent may still happen. Errors are typed, and the UI words
them: not found, permission denied, already exists, or the error of the OS or the server.
SFTP v3 has no code for a name that is taken, so `SftpFs` reports a plain failure to create
or rename as `AlreadyExists` when something has that name. `rename` replaces an existing file
where that takes one step: locally, and over SFTP with `posix-rename@openssh.com`. SFTP keeps
times as whole seconds from 1970 to 2106 and sets the access time with the modification
time, so the access time becomes the current time on both backends. Local times are set by
path (`utimensat`), since opening a FIFO would block.

Every ssh command line is assembled in `noc-ssh`, in this order: program → forced options →
`ssh.args` → host `args` → role options → `--` → destination. ssh keeps the first value it sees
for an option, so `-o` values in user arguments cannot override forced options; flags are covered
by validation ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)).

## Virtual root

The root of the virtual file system lists the local file system and every host from the ssh
config:

```text
┌─ Hosts ─────────────────────────┐┌─ prod-web:/var/www ─────────────┐
│ Name             Address        ││ Name                Size Modify │
│ [Local]          ~              ││ /..               UP-DIR        │
│ ● prod-web       deploy@10.0.0.5││ /html                DIR Sep 30 │
│ ○ staging        ubuntu@stg:2222││  index.php          4.2K Sep 29 │
│ ✗ nas            admin@nas      ││                                 │
└─────────────────────────────────┘└─────────────────────────────────┘
 1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit
```

- `..` from `/` of any file system leads back to the virtual root, with the cursor on the file
  system just left. `[Local]` opens the home directory.
- Hosts come in config order, named by their `hosts.<alias>.label` if set (the status line shows
  the alias), with the address cached from an earlier `ssh -G`. The root is listed like a
  directory, in a background task that scans the ssh config and loads the cache, so Ctrl-R
  rereads the ssh config.
- Entering a host connects in the background (the status line says so; Esc stops it) and opens
  the configured `start_dir` or the remote home directory, shown as an absolute path.
- When a connection is lost, the panels on that host go back to the root and say why.
- A marker in front of each host shows its state: `○` not connected, a spinner while
  connecting, `●` connected, `✗` the last attempt failed or the connection was lost. F8 (`Esc 8`) in the root
  closes the connection to the host under the cursor, or stops connecting to it.
- Locations are `Root`, `Local(PathBuf)`, or `Remote { host, path }`. Remote paths are bytes,
  because SFTP v3 does not guarantee UTF-8, and are displayed lossily. The SFTP client library
  still requires UTF-8 names; see the known issues in the [roadmap](roadmap.md).
- Names from the server that are empty or contain `/` or NUL are dropped from listings: joined
  to a local path, they could point outside the target directory.

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
noc ls [LOCATION]      virtual root, a local path, or host:path
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

When the name of a file or symlink is taken, the job sends `Exists` with the metadata of both,
for sizes and times, and waits for a `Conflict`: Overwrite, Skip, Overwrite all, Skip all,
Overwrite older (every later one that is older than its source; unknown times keep the
target), or Abort. With `overwrite`, the job replaces without asking, as when an edited
file goes back where it came from. A directory where a file would go is a failure, not a question. A symlink
in the way is removed first, so that a file written directly never goes through it; renaming
replaces it anyway. Without `posix-rename`, the target is removed before the rename.

Moving places sources as copying does. Between file systems (local and a host, or two
hosts), it is a copy with `remove_sources`: each file or symlink goes from the source once
it is copied, and each directory once everything in it has gone, so whatever is skipped or
fails stays, with the directories around it. Within one file system (`move_within`), each
source is renamed, after the same question if its name is taken by something a rename
replaces; a directory whose name is taken by a directory merges into it by copying and
removing, and so does anything the rename refuses as crossing file systems (`EXDEV`
locally, or a plain failure over SFTP v3, which has no code for it). Moves keep times and
permissions.

## Configuration and paths

Noon Commander uses the XDG layout on every platform, including macOS, and respects the `XDG_*`
variables:

| Purpose | Path |
| --- | --- |
| Settings, keymap, themes | `~/.config/noc/` (`config.toml`, `keymap.toml`, `themes/`) |
| Data (bookmarks) | `~/.local/share/noc/` |
| State (history, last directories, logs) | `~/.local/state/noc/` |
| Cache (`ssh -G` results) | `~/.cache/noc/` |
| Runtime (control sockets, askpass socket, F4 temp files) | `$XDG_RUNTIME_DIR/noc/` or `$TMPDIR/noc-$UID/`, mode 0700 |

Noon Commander never rewrites `config.toml` wholesale; edits go through `toml_edit` and keep
comments. Unknown keys are errors, so a typo does not silently fall back to a default.
`noc config init` writes the commented defaults.

```toml
[ssh]
program = "ssh"                  # name in PATH or absolute path, OpenSSH 8.7+
config_file = "~/.ssh/config"    # optional: passed as -F, also drives host discovery
args = ["-o", "ServerAliveInterval=15"]
multiplex = true                 # false: one connection per channel

[discovery]
hide = ["github.com", "gitlab.com", "bitbucket.org"]

[hosts."prod-web"]               # decorates the ssh_config host, never duplicates it
label = "Prod"
start_dir = "/var/www"
args = ["-o", "Compression=yes"]

[ui]
language = "auto"                # or a language tag such as "en-US"; others fall back to it
theme = "mc-classic"             # or "terminal": the terminal's own colors, reverse video
borders = "double"               # frames of panels and dialogs: ═ ║ ╔; "single": ─ │ ┌
icons = true                     # Nerd Font icons; false: mc's markers (/ * @ ~ …)
show_hidden = true               # names that start with a dot; Alt-. switches while running
type_to_search = true            # typing in a panel starts quick search; false: only Ctrl-S

[transfer]
atomic_upload = true             # copies go to a hidden temporary name, then are renamed
parallel_jobs = 2                # jobs that run at once; later ones wait; F4 never waits
```

Preserving attributes is a choice in the copy dialog, as in mc, not a setting.

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
- **Marks.** As in mc: Insert or Ctrl-T marks the entry under the cursor, or unmarks it, and
  moves down (Shift-Down too, Shift-Up moves up); `*` (or Alt-*) inverts the marks on files,
  leaving directories as they are; `..` and the rows of the virtual root cannot be marked.
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
  virtual root.
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
- **The other panel.** As in mc: Ctrl-U swaps the panels, and the active one stays active on
  the other side; Alt-O opens the directory or host under the cursor in the other panel (from
  a file, the parent directory with the cursor on this one) and moves the cursor down; Alt-I
  shows this directory in the other panel with the cursor on the same name. Panels keep their
  identity when swapped (only where they are drawn changes), so a listing still in flight
  reaches the panel that asked for it. Sort order and errors go with the panel; mc keeps the
  sort order on its side.
- **Quick search.** Ctrl-S / Alt-S as in mc, or, since there is no command line, typing in a
  panel (unless `ui.type_to_search` is off) starts quick search: the cursor jumps to the first name from where it is that starts with
  the text, ignoring case, and a character that matches nothing is dropped, as in mc. Ctrl-S
  again finds the next match, round to the top; Backspace takes a character back; Esc ends the
  search, and any other key ends it and then does what it does. While it runs, every
  character is text, even one that a panel binds, such as `*`. The root searches the names it
  shows: labels, or aliases. Long names lose their middle, marked
  with `~`. Names are shown terminal-safe: control and bidi characters become `?`. Listings
  run in background tasks; a reply carries the generation of its request, so a stale one is
  dropped. If a directory cannot be read, the panel stays where it was and says why below the
  listing. Going up puts the cursor on the directory just left. A panel shows a `Location`, so
  the [virtual root](#virtual-root) is one more kind of listing.
- **Keymap.** Keys map to `Action`s per context (`panel`, `root`, `quick_search`, `dialog`,
  `dialog_input`, `viewer`; `menu` will follow). Each context falls back along a chain, for
  example the root and quick search to the panel; the first context that knows a key sequence
  decides, except that a sequence it only starts does what a later context binds it to.
  Bindings are key sequences matched by prefix with a 1-second timeout, so a vim preset
  (`g g`, `d d`) can follow the default mc preset. As in mc, `Esc` in a panel waits for the
  next key: `Esc 1` … `Esc 0` stand for F1 … F10, `Esc` followed by a character stands for Alt
  and that character, for terminals whose Alt key sends nothing, and `Esc` alone cancels once
  the timeout passes (`Esc Esc` at once). An `Esc` and a quick next key arrive as Alt and that
  key, so there an unbound Alt and a character count as `Esc` and the character. In dialogs and
  quick search `Esc` acts at once. Keys are written with `crokey` names. User overrides in
  `keymap.toml` are planned for M4. The F-key bar is generated from the active keymap, and so
  is the help screen (F1): the keys of each context, with what they do, for what the app can
  do already; a prompt from ssh shows over it.
- **Dialogs.** Modal and centered over the panels, with mc-style buttons: `[< OK >]` marks the
  default one. A dialog has a message, a text field, check boxes, and buttons, each optional
  but the buttons; ssh's prompts and the app's own questions are the same kind of dialog, and
  each one in the queue knows where its answer goes. Keys go to the first dialog in the queue
  (contexts `dialog` and `dialog_input`); Tab and the arrows move between the field, the check
  boxes, and the buttons, Space switches a check box or presses a button, Enter presses the
  button with the focus (the default one from the field or a check box), and Esc or F10
  cancels. In a text field every character is text, Space too. A field that opens with text
  shows it dimmed, and typing replaces it, as in mc; an edit or a cursor move keeps it. Long
  text scrolls to keep the cursor in view.
- **Text.** Fluent files under `crates/noc/i18n/`, embedded in the binary and read with
  `fl!` from `i18n-embed-fl`, which checks message IDs against `en-US` at compile time; only
  `en-US` for now. `ui.language = "auto"` follows the system locale (through `sys-locale`).
  Arguments are inserted without Unicode isolation marks, which terminals would show.
- **Themes.** Built in: `mc-classic`, the colors of mc's default skin (blue panels, a cyan
  cursor that replaces the row's colors, yellow headers; directories white, executables green,
  broken links red, devices magenta; gray dialogs with mc's shadow; a black-and-cyan F-key
  bar; red error dialogs), and `terminal`, the terminal's own colors with reverse video.
  `ui.theme` picks one; an unknown name is an error. Only the 16 ANSI colors are used, so the
  terminal's palette decides the exact shades. User themes in `themes/` are planned for M4.
  Panels and dialogs are framed with double lines unless `ui.borders` is `single`; as in mc,
  a dialog leaves a blank cell between its frame and its edge, which gives way on a screen too
  small for it.
- **Icons.** Nerd Fonts v3 glyphs, on by default (`ui.icons`), in front of each name: our own for
  directories, `..`, links, broken links, FIFOs, sockets, devices, executables, `[Local]`, and
  hosts; `devicons` for files by name or extension. devicons asks the disk whether a name it
  does not know is a directory, so names go to it inside a path with a NUL byte, which names
  nothing: drawing stays free of I/O, and remote names are never looked up locally. Without
  icons, mc's markers: `/` directory, `~` link to a directory, `@` link, `!` broken link, `*`
  executable, `|` FIFO, `=` socket, `-` character device, `+` block device.
