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
- [x] Clipboard through OSC 52 ([ADR 0008](adr/0008-clipboard-through-osc-52.md))
- [x] Checksums (Ctrl-X #): SHA-256, SHA-512, SHA-1, MD5, BLAKE3 of files and trees, an
      expected checksum, comparison with the file in the other panel, Copy and Save
- [ ] Verify files of checksums (`*.sha256`, `SHA256SUMS`, `*.md5`, `*.sfv`): `OK`, `FAILED`,
      and missing, with a total
- [x] F9 pull-down menu, as in mc, with Left/Right → Change location; `ui.menu_bar`
- [x] Configuration dialog (Options → Configuration…): categories with icons, scrolling
      settings with a scroll bar, Interface; writes changed keys to `config.toml`
      ([ADR 0009](adr/0009-configuration-dialog-writes-config-toml.md))
- [x] Configuration: categories for `[transfer]`, `[ssh]`, `[discovery]`, `[volumes]`, applied
      at once
- [x] Catppuccin themes in 24-bit color, Mocha and Latte, with the nearest of 256 colors
      where `COLORTERM` does not announce 24-bit color
      ([ADR 0010](adr/0010-truecolor-themes.md))
- [x] Noon themes from the logo's colors, dark and light (`noon-dark`, `noon-light`)
- [ ] More built-in dark and light pairs from the same palette roles: Gruvbox (dark/light),
      Rosé Pine (main/dawn), Tokyo Night (night/day), Solarized (dark/light)
- [ ] User themes and keymap overrides
- [x] Tabs: each panel has its own (Ctrl-X T, Ctrl-X W, Alt-Left/Right, Ctrl-X Tab), on a line
      above the panels or in their frames, `ui.tab_bar`
      ([ADR 0011](adr/0011-tabs-per-panel.md))
- [ ] Bookmarks and history
- [ ] chmod and symlinks
- [ ] Mouse support
- [ ] Restore panel state on start, with the tabs

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
- Clipboard fallback where OSC 52 does not work (macOS Terminal, blocked multiplexers): a
  desktop helper (`pbcopy`, `wl-copy`, `xclip`) when Noon Commander runs on the desktop,
  behind a setting ([ADR 0008](adr/0008-clipboard-through-osc-52.md)).
- Checksums on the server (`sha256sum` over the host's master connection) for large remote
  files, instead of reading them over SFTP. It runs remote commands, which Noon Commander does
  not do yet, and the tools differ (`sha256sum`, `shasum`, `b3sum`), so it needs an ADR.
  OpenSSH's server has no SFTP `check-file` extension.
- CRC32, for `.sfv` files.
- Resumable transfers.
- Release binaries and a Homebrew tap.
- Watch for volumes that are mounted or unmounted (DiskArbitration on macOS, `poll` on
  `/proc/self/mountinfo` on Linux) instead of reading them on each listing; eject from the root.
- Plugins for other backends as rows of the virtual root.
- Anything behind the `forwarding` feature ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)).
