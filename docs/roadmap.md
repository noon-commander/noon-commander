# Roadmap

## M0: Skeleton (done)

- [x] Cargo workspace and crate layout
- [x] License, README, AGENTS.md, CHANGELOG, editor and lint configuration
- [x] Architecture document and ADRs 0001–0004
- [x] CI: rustfmt, clippy, and tests (default and `forwarding`), MSRV, cargo-deny

## M1: Core without UI (done)

- [x] `sftp-tui-config`: XDG paths, TOML schema, defaults, `sftp-tui config init`
- [x] Host discovery: scanner for `Host`, `Match`, and `Include`; lazy `ssh -G`
- [x] Cache for `ssh -G` results, keyed by the state of the config files
- [x] ssh argument validator and forwarding policy ([ADR 0004](adr/0004-forwarding-compile-time-feature.md))
- [x] Master connection, SFTP channels, askpass bridge ([ADR 0002](adr/0002-controlmaster-per-host.md), [ADR 0003](adr/0003-askpass-bridge.md))
- [x] `sftp-tui-vfs`: virtual root, local, and SFTP backends
- [x] Debug commands: `sftp-tui hosts`, `sftp-tui ls <host>:<path>`
- [x] Tests: local `sftp-server` over pipes, fake `ssh` program

## M2: TUI for browsing (current)

- [x] TUI skeleton: alternate screen, event loop, two panels, F10 quits, terminal restore
- [x] Local panels: listing, navigation, background loading, errors
- [x] Virtual root: `[Local]` and the hosts from ssh_config, with labels and cached addresses
- [ ] Opening hosts: connect, remote listings, disconnect
- [ ] Panels: sorting, hidden files, quick search
- [x] Keymap engine with the mc preset
- [ ] Nerd Font icons and mc markers
- [x] Fluent i18n (`en-US`) and `ui.language`
- [ ] Askpass and host-key dialogs
- [ ] mc-classic theme, F-key bar

## M3: File operations

- [ ] Selection: Insert, `+`, `-`, `*`
- [ ] F5 copy and F6 move/rename in any direction, with progress, cancellation, and a conflict
      dialog
- [ ] F7 mkdir, F8 delete
- [ ] F3 viewer, F4 edit via `$EDITOR` (suspends and resumes the TUI)
- [ ] Background job queue

## M4: Polish

- [ ] User themes and keymap overrides
- [ ] Bookmarks and history
- [ ] chmod and symlinks
- [ ] Mouse support
- [ ] Restore panel state on start

## Known issues

- **Non-UTF-8 remote file names end the SFTP session.** `openssh-sftp-client` 0.15 decodes names
  as UTF-8 (`openssh-sftp-protocol` deserializes `Box<Path>` through `ssh_format`, which calls
  `str::from_utf8`), so listing a directory with, say, a cp1251 or KOI8-R name fails and the
  session closes. Options: an upstream patch that keeps names as bytes, a fork, or our own SFTP
  v3 codec behind `SftpFs`.

## Backlog

- Console (Ctrl-O), variant A: suspend the TUI and run `$SHELL` in the panel's directory, locally
  or with `ssh -t` over the host's master connection. It reuses the suspend/resume mechanism from
  F4. Variant B, a persistent mc-style subshell, may come later.
- vim keymap preset.
- Linux: CI job and packages (AUR, deb).
- Translations.
- Resumable transfers.
- Release binaries and a Homebrew tap.
- Anything behind the `forwarding` feature ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)).
