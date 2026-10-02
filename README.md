# sftp-tui

A Midnight Commander-style, two-panel file manager for SFTP, built on top of your system OpenSSH
client.

> **Status: pre-alpha.** You can browse local directories and SFTP hosts, and copy, move,
> delete, view, and edit files between them; polish comes next. See the
> [roadmap](docs/roadmap.md).

## Why

- **Your `~/.ssh/config` just works.** sftp-tui never implements SSH. It runs the `ssh` you
  already use, so `ProxyJump`, `Match`, `Include`, agents, FIDO keys, certificates, and
  `known_hosts` behave exactly as in your terminal.
- **Every server in one place.** The virtual root lists the local file system and all hosts from
  your ssh config.
- **Two panels and mc keys.** Copy, move, rename, view, and delete files between your machine and
  your servers, or between two servers.

## Philosophy

sftp-tui does files over SFTP and nothing else.

- The system OpenSSH client is the single source of truth for connections and authentication.
  sftp-tui never links an SSH library and never stores passwords.
- SFTP channels get the same safety options as the stock `sftp(1)` client.
- Port, agent, and X11 forwarding are compiled out by default
  ([ADR 0004](docs/adr/0004-forwarding-compile-time-feature.md)).
- The UI never waits on the network: every remote operation is asynchronous, cancellable, and
  shows progress.

Non-goals: forwarding of any kind, protocols other than SFTP, a built-in SSH implementation, a
password manager, editing `~/.ssh/config` or `known_hosts`.

## Planned features

- Virtual root with the local file system and all hosts from `ssh_config`.
- Copy and move between local and remote, or between two remote hosts.
- Password, OTP, and host-key prompts inside the TUI (via `SSH_ASKPASS`).
- One authentication per host, shared by all panels and transfers (OpenSSH `ControlMaster`).
- Configurable `ssh` binary and extra arguments.
- Nerd Font icons, on by default.
- Midnight Commander keybindings; a vim preset may follow.

## Requirements

- macOS (Linux support is planned).
- OpenSSH 8.7 or newer: check with `ssh -V`.
- Rust 1.88 or newer to build from source.

## Building

```sh
cargo build --release
./target/release/sftp-tui --version
```

Configuration will live in `~/.config/sftp-tui/`, following the XDG layout on macOS too.

## Contributing

See [AGENTS.md](AGENTS.md) for the project rules, layout, and commands. They apply to humans and
AI agents alike.

## License

Copyright (C) 2026 0ldkettle

sftp-tui is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

sftp-tui is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without
even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
[GNU General Public License](LICENSE) for more details.
