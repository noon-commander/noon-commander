# Roadmap

## M0: Skeleton (done)

- [x] Cargo workspace and crate layout
- [x] License, README, AGENTS.md, CHANGELOG, editor and lint configuration
- [x] Architecture document and ADRs 0001–0004
- [x] CI: rustfmt, clippy, and tests (default and `forwarding`), MSRV, cargo-deny

## M1: Core without UI (done)

- [x] `noc-config`: XDG paths, TOML schema, defaults, `noc config init`
- [x] Host discovery: scanner for `Host`, `Match`, and `Include`; lazy `ssh -G`
- [x] Cache for `ssh -G` results, keyed by the state of the config files
- [x] ssh argument validator and forwarding policy ([ADR 0004](adr/0004-forwarding-compile-time-feature.md))
- [x] Master connection, SFTP channels, askpass bridge ([ADR 0002](adr/0002-controlmaster-per-host.md), [ADR 0003](adr/0003-askpass-bridge.md))
- [x] `noc-vfs`: local and SFTP backends
- [x] Debug commands: `noc hosts`, `noc ls <host>:<path>`
- [x] Tests: local `sftp-server` over pipes, fake `ssh` program

## M2: TUI for browsing (done)

- [x] TUI skeleton: alternate screen, event loop, two panels, F10 quits, terminal restore
- [x] Local panels: listing, navigation, background loading, errors
- [x] Virtual root: `[Local]` and the hosts from ssh_config, with labels and cached addresses
- [x] Opening hosts: background connect (Esc stops it), remote listings, `start_dir`, lost
      connections
- [x] Disconnect, `ssh -G` on connect, connection state in the root
- [x] Sorting (Ctrl-F3 … Ctrl-F6) and hidden files (`ui.show_hidden`, Alt-.)
- [x] Quick search
- [x] Keymap engine with the mc preset
- [x] Nerd Font icons and mc markers
- [x] Fluent i18n (`en-US`) and `ui.language`
- [x] Askpass and host-key dialogs
- [x] mc-classic and terminal themes (`ui.theme`)
- [x] F-key bar and help screen (F1), both from the keymap

## M3: File operations (done)

- [x] Panel actions that the keymap binds already: Ctrl-U swaps the panels, Alt-O opens the
      directory under the cursor in the other panel, Alt-I shows this directory there
- [x] Marks: Insert or Ctrl-T, Shift-Up/Down, `*`; their total below the listing
- [x] `+` and `-` mark and unmark by pattern
- [x] VFS: create and remove directories, remove files, rename, set permissions and times
- [x] F7 mkdir
- [x] Job engine (`noc-ops`): progress, a decision on each failure, cancellation; deleting,
      copying, and moving, with a question when a name is taken
- [x] F8 delete
- [x] VFS: read and write files as streams
- [x] F5 copy in any direction, with progress, cancellation, and a question for taken names
- [x] `[transfer]` settings: `atomic_upload`
- [x] F6 move/rename
- [x] F3 viewer
- [x] F4 edit via `$EDITOR` (suspends and resumes the TUI)
- [x] Background jobs: the Background button, an indicator, a question on quit
- [x] Job queue: `[transfer] parallel_jobs`
- [x] A list of jobs (Ctrl-X J)

## M4: Polish (current)

- [x] Virtual root of mounted volumes and a list of SFTP hosts, with the connected hosts in the
      root; `[volumes] hide` ([ADR 0006](adr/0006-virtual-root-with-volumes-and-hosts.md))
- [x] Location menu: Alt-F1, Alt-F2 (Ctrl-X 1, Ctrl-X 2), with a filter and hotkeys
- [x] Host settings in `hosts.toml`, typed and edited with F4 on a host: `other_dir`,
      `remember_dir` ([ADR 0007](adr/0007-typed-host-settings-in-hosts-toml.md))
- [ ] F9 pull-down menu, as in mc, with Left/Right → Change location
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
- Watch for volumes that are mounted or unmounted (DiskArbitration on macOS, `poll` on
  `/proc/self/mountinfo` on Linux) instead of reading them on each listing; eject from the root.
- Plugins for other backends as rows of the virtual root.
- Anything behind the `forwarding` feature ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)).
