# Noon Commander

A Rust-based terminal file manager for macOS and Linux, focused on seamless local and SFTP file
operations. The command is `noc`.

> **Status: pre-alpha.** You can browse local directories and SFTP hosts, and copy, move,
> delete, view, and edit files between them; polish comes next. See the
> [roadmap](docs/roadmap.md).

## Why

- **Two panels and mc keys.** Copy, move, rename, view, and delete files on your machine, between
  your machine and your servers, or between two servers. The keys are the ones you know from
  Midnight Commander.
- **SFTP at the core.** Remote hosts are not an add-on: Noon Commander was started to make SFTP
  feel as natural as the local disk, and remote files work just like local ones.
- **Your `~/.ssh/config` just works.** Noon Commander never implements SSH. It runs the `ssh` you
  already use, so `ProxyJump`, `Match`, `Include`, agents, FIDO keys, certificates, and
  `known_hosts` behave exactly as in your terminal.
- **Every disk and server in one place.** The virtual root lists the mounted volumes and all
  hosts from your ssh config; Alt-F1 and Alt-F2 switch a panel to any of them, as in Far
  Manager.

## Philosophy

- A full file manager: local and remote files are equally first-class. SFTP is the foundation,
  and more backends may follow.
- The system OpenSSH client is the single source of truth for connections and authentication.
  Noon Commander never links an SSH library and never stores passwords.
- SFTP channels get the same safety options as the stock `sftp(1)` client.
- Port, agent, and X11 forwarding are compiled out by default
  ([ADR 0004](docs/adr/0004-forwarding-compile-time-feature.md)).
- The UI never waits on the network: every remote operation is asynchronous, cancellable, and
  shows progress.

Non-goals: forwarding of any kind, a built-in SSH implementation, a password manager, editing
`~/.ssh/config` or `known_hosts`.

## Planned features

- Virtual root with the mounted volumes and all hosts from `ssh_config`.
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
./target/release/noc --version
```

Configuration lives in `~/.config/noc/`, following the XDG layout on macOS too;
`noc config init` writes the commented defaults.

## Contributing

See [AGENTS.md](AGENTS.md) for the project rules, layout, and commands. They apply to humans and
AI agents alike.

## License

Copyright (C) 2026 0ldkettle

Noon Commander is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

Noon Commander is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
[GNU General Public License](LICENSE) for more details.
