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
- A marker in front of each host shows its state: `○` not connected, `◌` connecting, `●`
  connected, `✗` the last attempt failed or the connection was lost. F8 (`Esc 8`) in the root
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
  `update(state, msg)` → effects (spawned tasks with a `CancellationToken`) → render, at most 60
  frames per second.
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
show_hidden = true               # names that start with a dot; Alt-. switches while running
type_to_search = true            # typing in a panel starts quick search; false: only Ctrl-S
```

Planned keys and sections, not accepted yet:

```toml
[transfer]                       # M3
parallel_jobs = 2
preserve_mtime = true
atomic_upload = true             # write to a temporary name, then rename

[ui]                             # M2
icons = true
theme = "mc-classic"
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
- **Quick search.** Ctrl-S / Alt-S as in mc, or, since there is no command line, typing in a
  panel (unless `ui.type_to_search` is off) starts quick search: the cursor jumps to the first name from where it is that starts with
  the text, ignoring case, and a character that matches nothing is dropped, as in mc. Ctrl-S
  again finds the next match, round to the top; Backspace takes a character back; Esc ends the
  search, and any other key ends it and then does what it does. The root searches the names it
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
  `keymap.toml` are planned for M4. The F-key bar is generated from the active keymap; the help screen will be too.
- **Dialogs.** Modal and centered over the panels, with mc-style buttons: `[< OK >]` marks the
  default one. Keys go to the first dialog in the queue (contexts `dialog` and `dialog_input`);
  Tab and the arrows move between the field and the buttons, Enter activates, Esc or F10
  cancels.
- **Text.** Fluent files under `crates/sftp-tui/i18n/`, embedded in the binary and read with
  `fl!` from `i18n-embed-fl`, which checks message IDs against `en-US` at compile time; only
  `en-US` for now. `ui.language = "auto"` follows the system locale (through `sys-locale`).
  Arguments are inserted without Unicode isolation marks, which terminals would show.
- **Icons.** Nerd Fonts v3 glyphs from `devicons`, plus our own table for directories, links, and
  virtual-root entries; on by default (`ui.icons`). Icons sit in a fixed-width column. Without
  icons, the mc markers are used: `/` directory, `*` executable, `@` symlink, `~` symlink to a
  directory.
