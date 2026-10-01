# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Project skeleton: Cargo workspace, crate layout, CI, and documentation.
- Configuration in `~/.config/sftp-tui/config.toml` (XDG layout on macOS too):
  `sftp-tui config init` writes the commented defaults, `sftp-tui config paths` shows the
  files and directories in use, and `--config` selects another file.
- `sftp-tui hosts` lists the hosts from ssh_config, following `Include`; `--resolve` adds
  their addresses from `ssh -G`. Addresses are cached in `~/.cache/sftp-tui/resolve.json`
  and shown without `--resolve` until the ssh configuration changes.
- `sftp-tui ls` lists the virtual root, a local directory, or a remote one (`host:path`) over
  SFTP through the system OpenSSH client, with password and host-key prompts on the terminal.
- Validation of `ssh.args` and per-host `args`: options that sftp-tui manages itself and, in
  default builds, forwarding options are rejected.
- Logs in `~/.local/state/sftp-tui/sftp-tui.log`; `SFTP_TUI_LOG` sets the level.
- `sftp-tui` without a subcommand starts the TUI: two panels on the current directory, with
  name, size, and modification time. Keys follow Midnight Commander: arrows, PgUp/PgDn,
  Home/End, Enter, Ctrl-PgUp for the parent, Tab for the other panel, Ctrl-R to reread, F10 or
  `Esc 0` to quit, Ctrl-L to redraw; `Esc` followed by a key works like Alt with that key.
- Going up from `/` leads to the virtual root: `[Local]`, which opens the home directory, and
  the hosts from ssh_config in config order, shown by their `label` if set, with the addresses
  cached by `sftp-tui hosts --resolve`. Ctrl-R there rereads ssh_config.
- Enter on a host connects in the background and opens its `start_dir` or the remote home
  directory; Esc stops the attempt. Keys and the agent work; password and host-key prompts are
  declined for now. If the connection is lost, its panels go back to the host list and say why.
- The host list marks each host as not connected, connecting, connected, or failed, and F8
  (`Esc 8`) there disconnects the host under the cursor. Connecting runs `ssh -G`, whose
  address is shown and cached for the next run.
- `Esc` followed quickly by a digit now works as the F-key too; terminals deliver it as Alt and
  the digit.
- `[ui] language` selects the interface language (`auto` by default); only English exists so
  far. Interface text lives in Fluent files.
