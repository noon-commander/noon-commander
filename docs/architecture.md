# Architecture

sftp-tui is a two-panel file manager for SFTP. It never implements SSH: every connection is a
process of the system OpenSSH client, and sftp-tui speaks the SFTP protocol over that process's
stdin and stdout.

This document describes the planned design; see the [roadmap](roadmap.md) for what exists. Key
decisions are recorded as [ADRs](adr/).

## Crates

```text
crates/
├── sftp-tui/           bin + UI: CLI, bootstrap, askpass entry point, ratatui app,
│                       keymap, themes, icons, i18n
├── sftp-tui-config/    XDG paths, TOML schema, defaults
├── sftp-tui-ssh/       host discovery, ssh -G, argument validation, forwarding policy,
│                       ControlMaster, SFTP channels, askpass bridge
├── sftp-tui-vfs/       Vfs trait: virtual root, local, and SFTP backends
└── sftp-tui-ops/       job engine: copy, move, delete, mkdir; progress, cancellation, conflicts
```

Dependencies point one way: `config ← ssh ← vfs ← ops ← sftp-tui`. Library crates contain no UI
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
ssh -F /dev/null -S <sock> -O exit -- sftp-tui                        # disconnect
```

The SFTP protocol client is `openssh-sftp-client`, whose `Sftp::new` works over the pipes of any
child process. Every ssh child runs in its own session (`setsid`), without a controlling
terminal, so it can neither read from nor draw on the TUI's terminal.

`sftp-tui-ssh` API in short: `version::check_version` runs `ssh -V`; `resolve::resolve` runs
`ssh -G`; `Session::connect` starts the master (or nothing, without multiplexing);
`Session::open_sftp` returns the pipes of a new SFTP channel, which `SftpFs::from_pipes` in
`sftp-tui-vfs` turns into a file system; `Session::close` shuts down; `cleanup_stale` removes
leftovers of crashed instances.

The `Vfs` trait of `sftp-tui-vfs`, implemented by `LocalFs` and `SftpFs`, lists directories,
reads metadata with and without following symlinks, canonicalizes paths, creates and removes
directories, removes files, renames, sets permissions and modification times, and reads and
writes files as chunks (`FileReader`, `FileWriter`, whose `finish` reports errors that only
show when a file is closed). Over SFTP, reads and writes of a file keep 64 requests of 32 KiB
in flight, as `sftp(1)` does, so a long round trip does not idle the link; a read that comes
back short is asked again for the rest. Its futures are cancel-safe: a dropped one leaks
nothing, though a change already sent may still happen. Errors are typed, and the UI words
them: not found, permission denied, already exists, or the error of the OS or the server.
SFTP v3 has no code for a name that is taken, so `SftpFs` reports a plain failure to create
or rename as `AlreadyExists` when something has that name. `rename` replaces an existing file
where that takes one step: locally, and over SFTP with `posix-rename@openssh.com`. SFTP keeps
times as whole seconds from 1970 to 2106 and sets the access time with the modification
time, so the access time becomes the current time on both backends. Local times are set by
path (`utimensat`), since opening a FIFO would block.

Every ssh command line is assembled in `sftp-tui-ssh`, in this order: program → forced options →
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
   when the TUI connects to a host (or for `sftp-tui hosts --resolve`, with bounded
   parallelism), never for every host at startup. Results are cached in
   `~/.cache/sftp-tui/resolve.json`, valid while the ssh settings, the config files read
   (inode, mtime, size), and the names in their directories stay the same, so earlier
   addresses show at once. A cached entry only stands in until `ssh -G` runs again.
4. `discovery.hide` hides patterns such as `github.com`.

## Authentication

Prompts go through the askpass bridge ([ADR 0003](adr/0003-askpass-bridge.md)): ssh runs
`sftp-tui` as its `SSH_ASKPASS` program, which forwards the prompt to the TUI over a Unix socket
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
sftp-tui                    the TUI (needs a terminal)
sftp-tui hosts [--resolve]  hosts from ssh_config with cached addresses; --resolve runs ssh -G
sftp-tui ls [LOCATION]      virtual root, a local path, or host:path
sftp-tui config init        write the commented default config.toml
sftp-tui config paths       show the files and directories in use
```

`--config FILE` replaces `~/.config/sftp-tui/config.toml`. Logs go to
`~/.local/state/sftp-tui/sftp-tui.log`; `SFTP_TUI_LOG` sets the filter, for example
`SFTP_TUI_LOG=debug`.

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
  Quitting restores the terminal first, then gives the connections a few seconds to close.
- The terminal is restored on every exit: a guard leaves raw mode and the alternate screen when
  the TUI returns or fails, ratatui's panic hook does it before a panic message, and SIGTERM,
  SIGHUP, and SIGINT end the event loop like a quit.

## File operations

`sftp-tui-ops` holds jobs that work on any `Vfs` backend: generic code, which the UI runs
with `LocalFs` in a task of its own or with a host's `SftpFs` in that host's task. A job talks
to the UI through a `Reporter`: it sends `Event`s (`Scanning`, `Progress` with the entry at
hand and done/total counts, `Failed`), and when an operation fails it waits for a `Decision`:
Retry, Skip, Skip all (no more questions), or Abort. A `CancellationToken` stops it between
operations and while it waits; so does a UI that stops listening. It returns an `Outcome`:
entries done, entries skipped, and whether it was aborted.

Deleting counts the entries first, so that progress has a total, then removes the deepest
first. Listings report symlinks without following them, so a link goes and its target stays.
A directory in which something stays (skipped, or unreadable) is left alone without asking
again, and an entry that is already gone counts as deleted.

## Configuration and paths

sftp-tui uses the XDG layout on every platform, including macOS, and respects the `XDG_*`
variables:

| Purpose | Path |
| --- | --- |
| Settings, keymap, themes | `~/.config/sftp-tui/` (`config.toml`, `keymap.toml`, `themes/`) |
| Data (bookmarks) | `~/.local/share/sftp-tui/` |
| State (history, last directories, logs) | `~/.local/state/sftp-tui/` |
| Cache (`ssh -G` results) | `~/.cache/sftp-tui/` |
| Runtime (control sockets, askpass socket, F4 temp files) | `$XDG_RUNTIME_DIR/sftp-tui/` or `$TMPDIR/sftp-tui-$UID/`, mode 0700 |

sftp-tui never rewrites `config.toml` wholesale; edits go through `toml_edit` and keep comments.
Unknown keys are errors, so a typo does not silently fall back to a default.
`sftp-tui config init` writes the commented defaults.

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
icons = true                     # Nerd Font icons; false: mc's markers (/ * @ ~ …)
show_hidden = true               # names that start with a dot; Alt-. switches while running
type_to_search = true            # typing in a panel starts quick search; false: only Ctrl-S
```

Planned keys and sections, not accepted yet:

```toml
[transfer]                       # M3
parallel_jobs = 2
preserve_mtime = true
atomic_upload = true             # write to a temporary name, then rename
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
- **Marks.** As in mc: Insert or Ctrl-T marks the entry under the cursor, or unmarks it, and
  moves down (Shift-Down too, Shift-Up moves up); `*` (or Alt-*) inverts the marks on files,
  leaving directories as they are; `..` and the rows of the virtual root cannot be marked.
  Marked rows are yellow, and the line below the listing shows the size of the marked files
  and how many entries are marked, such as `12,345 B in 3 files`. Marks are names, so they
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
- **F8 (or Delete) deletes** the marked entries, or the one under the cursor, after a red
  question with Yes as the default, as in mc: `Delete file "x"?`, `Delete directory "x" and
  everything in it?`, or `Delete 3 files and directories?`. mc asks a second time before it
  goes into a directory that is not empty; sftp-tui says so in the first question instead.
  The job runs in the background, locally or in the host's task, with a window that shows
  what it counts, the entry at hand, a gauge, and done/total; Esc or Abort stops it. A
  failure asks in a red dialog, with mc's buttons: Ignore, Ignore all, Retry, and Abort.
  When the job ends, panels on the directory read it again, and a cursor whose entry is gone
  stays on its row. If the host's connection is lost, the job ends with it.
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
  `dialog_input`; `viewer` and `menu` will follow). Each context falls back along a chain, for
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
- **Text.** Fluent files under `crates/sftp-tui/i18n/`, embedded in the binary and read with
  `fl!` from `i18n-embed-fl`, which checks message IDs against `en-US` at compile time; only
  `en-US` for now. `ui.language = "auto"` follows the system locale (through `sys-locale`).
  Arguments are inserted without Unicode isolation marks, which terminals would show.
- **Themes.** Built in: `mc-classic`, the colors of mc's default skin (blue panels, a cyan
  cursor that replaces the row's colors, yellow headers; directories white, executables green,
  broken links red, devices magenta; gray dialogs with mc's shadow; a black-and-cyan F-key
  bar; red error dialogs), and `terminal`, the terminal's own colors with reverse video.
  `ui.theme` picks one; an unknown name is an error. Only the 16 ANSI colors are used, so the
  terminal's palette decides the exact shades. User themes in `themes/` are planned for M4.
- **Icons.** Nerd Fonts v3 glyphs, on by default (`ui.icons`), in front of each name: our own for
  directories, `..`, links, broken links, FIFOs, sockets, devices, executables, `[Local]`, and
  hosts; `devicons` for files by name or extension. devicons asks the disk whether a name it
  does not know is a directory, so names go to it inside a path with a NUL byte, which names
  nothing: drawing stays free of I/O, and remote names are never looked up locally. Without
  icons, mc's markers: `/` directory, `~` link to a directory, `@` link, `!` broken link, `*`
  executable, `|` FIFO, `=` socket, `-` character device, `+` block device.
