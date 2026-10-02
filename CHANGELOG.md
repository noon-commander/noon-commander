# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Host settings moved from `[hosts."alias"]` in `config.toml` to `hosts.toml` next to it, one
  `["alias"]` table per host with a required `type = "sftp"`. A `[hosts]` table left in
  `config.toml` is an error that says so
  ([ADR 0007](docs/adr/0007-typed-host-settings-in-hosts-toml.md)).
- Per-host ssh `args` are removed; put per-host options in ssh_config. `ssh.args` stays. The
  `ssh -G` cache starts empty once, as its format changed.

- The virtual root lists `Home`, then the mounted volumes, the system volume first, named by
  their label, with their free space and size, and opens the hosts from ssh_config as one row,
  `SFTP`; the hosts that are connected or connecting show again below it. `Home` opens the home
  directory, as `[Local]` did, and every volume its mount point, the system volume `/`. `..`
  from a remote `/` or a lost connection now leads to the list of hosts. The root is titled
  with the machine's name ([ADR 0006](docs/adr/0006-virtual-root-with-volumes-and-hosts.md)).
- With icons, a host's icon shows its state by its color (and a cross after a failure) instead
  of a marker in front of it, so names line up with the other rows.
- `noc ls` without a location prints the mount points of the volumes, then the hosts as
  `host:`.

- The project is renamed from sftp-tui to Noon Commander and grows into a full terminal file
  manager for local and SFTP file operations ([ADR 0005](docs/adr/0005-rename-to-noon-commander.md)).
  The binary is now `noc`; settings, state, and caches live in `noc` directories
  (`~/.config/noc/` and so on), the log is `noc.log`, and the environment variables are
  `NOC_LOG` and `NOC_ASKPASS_*`. Files under the old `sftp-tui` directories are not migrated.

### Added

- Catppuccin themes in 24-bit color: `[ui] theme = "catppuccin-mocha"`, dark, and
  `"catppuccin-latte"`, light, with the same colors for the same things in both. Where
  `COLORTERM` is not `truecolor` or `24bit`, they use the nearest of the 256 colors
  ([ADR 0010](docs/adr/0010-truecolor-themes.md)).
- Options → Configuration… opens a dialog of every setting of `config.toml`, by category
  with icons, that scrolls with a scroll bar: Interface, Transfers, SSH (with the hidden
  hosts), and Volumes. It has no OK or Cancel: each change takes effect at once (a text
  field's when the cursor leaves it, the language at the next start, ssh settings for new
  connections) and is written to `config.toml`, keeping its comments. Values are checked
  first (the extra ssh arguments as at start), and lists are typed as words, as in a shell
  ([ADR 0009](docs/adr/0009-configuration-dialog-writes-config-toml.md)).
- F9 opens a pull-down menu, as in mc: Left and Right (location menu, sort order, rescan,
  disconnect for the panel on that side), File, Command, and Options (hidden files). Each
  command shows its key from the keymap and has a letter that runs it. Commands that cannot run
  now are dimmed and skipped. `ui.menu_bar = "always"` keeps the menu bar above the panels, as
  in mc; the default `"on-demand"` shows it only while a menu is open, as in Far Manager.
- Ctrl-X # computes checksums (SHA-256, SHA-512, SHA-1, MD5, or BLAKE3) of the marked files
  and directories, or the file under the cursor, as a job with progress. For one file, an
  expected checksum can be pasted, and the file under the other panel's cursor compared. The
  results can be copied, one or all in the format of `sha256sum`, or saved to a file such as
  `SHA256SUMS`.
- Copying to the clipboard through OSC 52, which works over ssh in terminals that allow it
  ([ADR 0008](docs/adr/0008-clipboard-through-osc-52.md)).

- F4 on a host edits its settings: label, remote start directory, a directory for the other
  panel, and whether to remember the last directory; Use Current fills in the directory a panel
  shows on the host. They are saved to `hosts.toml`, keeping its comments and other tables.
- `other_dir`: opening a host sends the other panel to this local directory.
- `remember_dir`: opening a host again returns to the last directory shown on it in this
  session, if it is still there, before `start_dir`.
- `noc config init` also writes a commented `hosts.toml`, unless one exists; `noc config paths`
  shows it.
- Panels show the free space and size of the file system that holds their directory on the
  bottom of their frame, as mc does, such as `123G / 500G (24%)`, for local and SFTP
  directories alike.
- Alt-F1 and Alt-F2 open a location menu over the left or the right panel, as in Far Manager
  (Ctrl-X 1 and Ctrl-X 2 too): `Home`, the volumes, and the hosts, with `1` … `0` as hotkeys, typing to
  filter, F8 to disconnect a host, and Ctrl-R to read them again.
- `[volumes] hide` leaves mount points out of the virtual root, by pattern.
- Project skeleton: Cargo workspace, crate layout, CI, and documentation.
- Configuration in `~/.config/noc/config.toml` (XDG layout on macOS too):
  `noc config init` writes the commented defaults, `noc config paths` shows the
  files and directories in use, and `--config` selects another file.
- `noc hosts` lists the hosts from ssh_config, following `Include`; `--resolve` adds
  their addresses from `ssh -G`. Addresses are cached in `~/.cache/noc/resolve.json`
  and shown without `--resolve` until the ssh configuration changes.
- `noc ls` lists the virtual root, a local directory, or a remote one (`host:path`) over
  SFTP through the system OpenSSH client, with password and host-key prompts on the terminal.
- Validation of `ssh.args`: options that Noon Commander manages itself and,
  in default builds, forwarding options are rejected.
- Logs in `~/.local/state/noc/noc.log`; `NOC_LOG` sets the level.
- `noc` without a subcommand starts the TUI: two panels on the current directory, with
  name, size, and modification time. Keys follow Midnight Commander: arrows, PgUp/PgDn,
  Home/End, Enter, Ctrl-PgUp for the parent, Tab for the other panel, Ctrl-R to reread, F10 or
  `Esc 0` to quit, Ctrl-L to redraw; `Esc` followed by a key works like Alt with that key.
- Going up from `/` leads to the virtual root: `[Local]`, which opens the home directory, and
  the hosts from ssh_config in config order, shown by their `label` if set, with the addresses
  cached by `noc hosts --resolve`. Ctrl-R there rereads ssh_config.
- Enter on a host connects in the background and opens its `start_dir` or the remote home
  directory; Esc stops the attempt. If the connection is lost, its panels go back to the host
  list and say why.
- ssh's questions appear as dialogs: passwords, passphrases, and one-time codes in a masked
  field; unknown host keys and confirmations as Yes/No with No as the default; notices such as
  touching a security key until ssh is done. A prompt from another host waits its turn.
- The host list marks each host as not connected, connecting, connected, or failed, and F8
  (`Esc 8`) there disconnects the host under the cursor. Connecting runs `ssh -G`, whose
  address is shown and cached for the next run.
- `Esc` followed quickly by a digit now works as the F-key too; terminals deliver it as Alt and
  the digit.
- `[ui] language` selects the interface language (`auto` by default); only English exists so
  far. Interface text lives in Fluent files.
- Panels sort by name, extension, modification time, or size with Ctrl-F3 … Ctrl-F6, as in Far
  Manager; the same key again reverses the order. `[ui] show_hidden` (on by default) shows
  names that start with a dot; Alt-. switches it while Noon Commander runs.
- Quick search: typing in a panel, or Ctrl-S, moves the cursor to the first name that starts
  with what was typed; Ctrl-S again finds the next one, and Esc or any other key ends it.
  `[ui] type_to_search = false` leaves it to Ctrl-S and Alt-S, as in mc.
- Nerd Font icons in front of names, by file type and extension (`[ui] icons`, on by default),
  in a dimmed color of the name so the names stand out; without them, mc's markers such as `/`
  for directories and `*` for executables. A host that is connecting shows a spinner.
- Midnight Commander's colors (`[ui] theme = "mc-classic"`, the default): blue panels, a cyan
  cursor, names colored by type, gray dialogs with a shadow. `theme = "terminal"` keeps the
  terminal's own colors. In both, directories are bold.
- Panels and dialogs are framed with double lines (`═`, `║`, `╔`), as in mc;
  `[ui] borders = "single"` draws single ones (`─`, `│`, `┌`).
- Dialogs leave a blank cell between their frame and their edge, and their titles are bold and
  centered, as in mc.
- F1 shows the keys of panels, the host list, quick search, and dialogs, with what they do,
  read from the keymap; the F-key bar shows `1Help`.
- Ctrl-U swaps the panels; Alt-O opens the directory or host under the cursor in the other
  panel; Alt-I shows the current directory there, as in mc.
- Marks, as in mc: Insert or Ctrl-T marks or unmarks the entry under the cursor (Shift-Up and
  Shift-Down too), `*` inverts the marks on files, and the line below the listing shows the
  size and number of marked entries. Marked rows are underlined, and yellow in mc-classic.
- `+` marks and `-` unmarks the names that match a shell pattern such as `*.{jpg,png}`, with
  mc's Files only and Case sensitive options. Dialogs can now have text fields and check
  boxes; Space switches a check box.
- F7 makes a directory, locally or on a host, and puts the cursor on it. The dialog opens with
  the name under the cursor; `~` stands for the home directory. Errors show in a red dialog.
- F8 (or Delete) deletes the marked entries or the one under the cursor, locally or on a host,
  after asking. Directories go with everything in them; symlinks are deleted, never followed.
  A window shows the progress, and Esc stops it; a failure offers Ignore, Ignore all, Retry,
  and Abort, as in mc.
- F5 copies the marked entries or the one under the cursor to the other panel's directory,
  or to a typed path or `host:path`: between local directories, to and from hosts, and
  between two hosts. Directories are copied with everything in them and symlinks as
  symlinks; times and permissions are kept unless Preserve attributes is off. Files are
  written under a temporary name and renamed when complete, so a cancelled copy leaves no
  half file. A taken name asks with both sizes and times: Yes, No, All, None, Older, Abort.
- `[transfer] atomic_upload = false` writes copies to their targets directly instead of under
  a temporary name.
- F6 moves the marked entries or the one under the cursor, or renames it in place when given a
  new name, as in mc. Within one file system it renames; between the local file system and a
  host, or two hosts, it copies and removes each source once all of it is copied, so what is
  skipped stays where it was.
- F3 views the file under the cursor, locally or on a host: text with long lines wrapped (F2
  cuts them), scrolled with mc's viewer keys, up to the first 16 MiB.
- F4 edits the file under the cursor in `$VISUAL` or `$EDITOR` (`vi` if neither is set),
  with the terminal handed over until the editor exits. A remote file is edited as a local
  copy, which goes back, with the original's permissions, if it changed; if it cannot go
  back, it stays, and Noon Commander says where.
- A job's window has a Background button, the default: Enter sends the job behind the panels,
  and other jobs can start while it runs. The top right corner shows how many jobs run and
  how far they are; their questions open as they come. F10 asks before quitting while jobs
  run, and quitting lets them remove their unfinished files first.
- `[transfer] parallel_jobs` (2 by default) is how many jobs run at once; later ones wait
  their turn, and editing with F4 never waits.
- Ctrl-X J lists the jobs, as mc's Background jobs do, with how far each is: Show brings one to
  the front, and Abort stops it.
- A job's window shows how long it has worked, and a copy or move also shows its average speed
  and the time left, which Ctrl-X J lists too. Unlike mc, time spent waiting for an answer to
  an error or a taken name does not count, so an open question lowers neither the speed nor
  the time left; nor do skipped files or retries bend the speed.
