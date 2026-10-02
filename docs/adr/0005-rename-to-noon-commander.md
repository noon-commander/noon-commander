# 0005. Rename to Noon Commander, a full file manager

- Status: Accepted
- Date: 2026-10-02

## Context

The project started as sftp-tui, a two-panel file manager for SFTP only. Working with local
files turned out to be as important as working with remote ones, and the project is to grow into
a full terminal file manager. The name sftp-tui and the "SFTP only" rule no longer describe it.
SFTP stays the foundation and the reason the project exists.

## Decision

- The project is called Noon Commander. The full name is used only in prose: README, docs,
  comments, and user-facing messages.
- Everything technical uses the short name `noc`: the binary, the crates (`noc`, `noc-config`,
  `noc-ssh`, `noc-vfs`, `noc-ops`), the XDG directories (`~/.config/noc/` and so on), the log
  file `noc.log`, environment variables (`NOC_LOG`, `NOC_ASKPASS_*`), and the suffix of
  temporary transfer files (`.name.noc-PID-N`).
- The description is: "A Rust-based terminal file manager for macOS and Linux, focused on
  seamless local and SFTP file operations."
- The "SFTP only" restriction is lifted. Local and remote files are equally first-class, and
  other backends may be added behind the `Vfs` trait.
- Unchanged: Noon Commander never implements SSH (ADR 0001) and drives the system OpenSSH client;
  forwarding stays a non-goal behind the compile-time `forwarding` feature (ADR 0004); it never
  stores credentials and never writes to `~/.ssh/`.

## Consequences

- Settings, state, and caches under the old `sftp-tui` directories are not migrated; the project
  is pre-alpha.
- The old environment variables `SFTP_TUI_*` are no longer read.
- New backends need their own ADR, in particular when they bring new network or authentication
  code.
