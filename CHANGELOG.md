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
- `[ui] language` selects the interface language (`auto` by default); only English exists so
  far. Interface text lives in Fluent files.
