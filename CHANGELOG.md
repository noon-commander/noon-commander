# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The project is renamed from sftp-tui to Noon Commander and grows into a full terminal file
  manager for local and SFTP file operations ([ADR 0005](docs/adr/0005-rename-to-noon-commander.md)).
  The binary is now `noc`; settings, state, and caches live in `noc` directories
  (`~/.config/noc/` and so on), the log is `noc.log`, and the environment variables are
  `NOC_LOG` and `NOC_ASKPASS_*`. Files under the old `sftp-tui` directories are not migrated.

### Added

- Project skeleton: Cargo workspace, crate layout, CI, and documentation.
- Configuration in `~/.config/noc/config.toml` (XDG layout on macOS too):
  `noc config init` writes the commented defaults, `noc config paths` shows the
  files and directories in use, and `--config` selects another file.
- `noc hosts` lists the hosts from ssh_config, following `Include`; `--resolve` adds
  their addresses from `ssh -G`. Addresses are cached in `~/.cache/noc/resolve.json`
  and shown without `--resolve` until the ssh configuration changes.
- `noc ls` lists the virtual root, a local directory, or a remote one (`host:path`) over
  SFTP through the system OpenSSH client, with password and host-key prompts on the terminal.
- Validation of `ssh.args` and per-host `args`: options that Noon Commander manages itself and,
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
