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
- [x] Master connection, SFTP channels, askpass bridge
      ([ADR 0002](adr/0002-controlmaster-per-host.md), [ADR 0003](adr/0003-askpass-bridge.md))
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
- [x] Sorting (`o n`, `o e`, `o m`, `o s`) and hidden files (`ui.show_hidden`, `o h`)
- [x] Quick search
- [x] Keymap engine with the mc preset
- [x] Nerd Font icons and mc markers
- [x] Fluent i18n (`en-US`) and `ui.language`
- [x] Askpass and host-key dialogs
- [x] mc-classic and terminal themes (`ui.theme`)
- [x] F-key bar and help screen (F1), both from the keymap

## M3: File operations (done)

- [x] Panel actions that the keymap binds already: Ctrl+w x swaps the panels, Ctrl+w f opens
      the directory under the cursor in the other panel, Ctrl+w v shows this directory there
- [x] Marks: Space, `J`, `K`, `g *`; their total below the listing
- [x] `g +` and `g -` mark and unmark by pattern, as mc's `+` and `-`
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
- [x] A list of jobs (`f j`)

## M4: Polish (current)

- [x] Virtual root of mounted volumes and a list of SFTP hosts, with the connected hosts in the
      root; `[volumes] hide` ([ADR 0006](adr/0006-virtual-root-with-volumes-and-hosts.md))
- [x] Location menu: Alt+F1, Alt+F2 (Ctrl+w 1, Ctrl+w 2), with a filter
- [x] Host settings in `hosts.toml`, typed and edited with F4 on a host: `other_dir`,
      `remember_dir` ([ADR 0007](adr/0007-typed-host-settings-in-hosts-toml.md))
- [x] Clipboard through OSC 52 ([ADR 0008](adr/0008-clipboard-through-osc-52.md))
- [x] Checksums (f #): SHA-256, SHA-512, SHA-1, MD5, BLAKE3 of files and trees, an
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
- [ ] User themes
- [ ] Keymap overrides: the user's own bindings per mode and context, on top of vim's
      ([ADR 0022](adr/0022-vim-modes.md))
- [x] Tabs: each panel has its own (`t n`, `t c`, `g t`, `g T`), on a line above the panels or
      in their frames, `ui.tab_bar`
      ([ADR 0011](adr/0011-tabs-per-panel.md))
- [x] zoxide: `g z` jumps to the directories it ranks; directories where the user did
      something go to it, `[zoxide]`; external programs live in `noc-tools`
      ([ADR 0012](adr/0012-external-tools-and-zoxide.md))
- [x] Release workflow from signed tags: macOS binaries for arm64 and x86_64 with build
      provenance attestations, and a Homebrew tap
      ([ADR 0014](adr/0014-release-workflow-and-homebrew-tap.md))
- [x] Quick cd (`g d`), as in mc, with `~`, `..`, `-`, and `host:path`
- [x] Tab completion of paths in Quick cd, F5, F6, and F7, with a list of choices
- [x] Shift+F6 renames the entry under the cursor in its row, in Insert mode with the cursor
      before the last extension
- [x] Fuzzy search as fzf does it in quick search, the location menu, and the zoxide window,
      `ui.fuzzy_search` ([ADR 0016](adr/0016-fuzzy-search.md))
- [x] Workspaces: the tabs of both panels saved under a name (t W), restored from
      F9 → Workspace or their window (t w), which also saves, renames, and deletes them;
      `workspaces.toml`
      ([ADR 0017](adr/0017-workspaces.md))
- [ ] Favorites in the location menu: directories the user chooses, local or remote, as
      bookmarks in its Favorites group
- [ ] History of the directories visited
- [ ] chmod and symlinks
- [ ] Trash: the marked entries, or the one under the cursor, moved to the trash (`d d`)
      rather than deleted: `~/.Trash` on macOS, the FreeDesktop trash on Linux. SFTP
      has no trash, so what hosts do needs an ADR
- [x] Mouse support ([ADR 0018](adr/0018-mouse-support.md)): panels, tabs, the F-key bar, the
      viewer, the pull-down menu, dialogs, and every window; `ui.mouse` and `ui.wheel`
- [ ] Restore panel state on start, with the tabs
- [x] Typing in a panel no longer starts quick search; `ui.type_to_search` is gone
- [x] Shell command line ([ADR 0019](adr/0019-shell-command-line.md)): `!` (and `:!`) runs a
      command in a local panel's directory; several lines (Ctrl+j, `\` Enter)
- [x] Shell command line on hosts, over the master connection, with a terminal
- [x] Shell command line: Shift+Enter (kitty keyboard protocol), bracketed paste
- [x] `g <` shows the output of commands in place of the panels; each command shows after its
      prompt, its output ends on a line of its own, and a failure leaves `[exit N]`;
      `shell.pause`. `! $SHELL` gives an interactive shell, so there is no console of its own
- [x] Command history per host: Up/Down (Ctrl+p/Ctrl+n) for the panel's host, a window
      (`q :`) for one host or all; `shell.history_size`, `history.toml` in the state directory
- [x] Terminal title: the active panel's directory, the terminal's own title back on quitting;
      `ui.terminal_title`
- [x] Key hints, as which-key: the keys that can follow `g`, Ctrl+w, …, and every key of
      whatever is in front (Ctrl+k); `ui.which_key`, `ui.which_key_delay_ms`
- [x] One keymap, vim's, in its modes, with mc's F-keys, in place of two presets chosen by
      `ui.keymap`, one modelled on mc ([ADR 0022](adr/0022-vim-modes.md)): Normal, Insert, and
      Visual mode in the panels, the lists, the dialogs, and the text fields; vim's motions,
      operators, registers, counts, and undo in text fields; Visual mode and a jump list
      (Ctrl+o, Tab) in the panels; the mode and the command typed on a line above the F-key
      bar, and the cursor's shape
- [ ] `.` repeats the last change, in text fields and of files
- [ ] Yanks copied to the system clipboard through OSC 52, behind a setting
- [ ] A key of vim's for editing the command line in `$EDITOR`
- [ ] Words as vim's 'iskeyword' parts them: also where the script changes (Latin, Han, kana,
      emoji), and with `×` and `÷` as letters
- [ ] More of vim's commands in text fields: `r`, `u`, `U`, `D`, `X`, `C`, `S`, and `J` in
      Visual mode, and `U` in Normal mode
- [ ] Text objects in text fields: `iw`, `aw`, `iW`, `aW`, quotes, and brackets (`c i w`,
      `d i "`, `c i (`); `c i w` only moves the cursor until then
- [ ] Registers as vim's: `"0` the last yank, `"1` … `"9` and `"-` the deletes; any other
      name than these, `a` … `z`, `A` … `Z`, `"`, and `_` gives the command up
- [ ] `p` in Visual mode puts in place of the selection; `Y` yanks to the end of the line, as
      nvim's
- [ ] A count repeats what `i`, `a`, `I`, and `A` type (`3ix` Esc)
- [ ] The lines of the command line in Normal mode: `o`, `O`, `J`, `g g`, `G`, `d j`, `d k`,
      `V`, and `r` Enter; `v $` takes the line break; a put of lines within a line leaves the
      cursor at their start, and Ctrl+r of whole lines ends them with a line break
- [ ] Ctrl+w with Ctrl held (`Ctrl+w Ctrl+h`, `Ctrl+l`, `Ctrl+x`, `Ctrl+r`, `Ctrl+p`,
      `Ctrl+v`, `Ctrl+f`) and with the arrows; a count before `Ctrl+w w` names the panel
- [ ] After a count or a register, Ctrl+c only gives the command up, and Esc in Visual mode
      ends it too, as in vim
- [ ] `g`, Ctrl+w, `Z`, and the other sequences never time out, with the key hints off too,
      as vim's own commands; Delete takes back the last digit of a count (vim's `N<Del>`)
- [ ] A count before `G` and `g g` goes to that line or row in the viewer, the help, and the
      lists, as in the panels; before Ctrl+d and Ctrl+u, it scrolls that many rows, which they
      keep, as vim's `'scroll'`
- [ ] `/` and Enter with nothing typed searches for the last pattern again
- [ ] Visual mode in the panels: `/`, `n`, and `N` move its end; `g ?`, `g m`, `g M`, `Z Z`,
      Ctrl+w 1 and 2, and `Y` do what they do in Normal mode, as their F-keys already do
- [ ] The jump list as vim's by default: a jump after Ctrl+o keeps the places after it, and a
      place already in the list moves to its end, rather than as `jumpoptions=stack`
- [ ] Up and Down on the command line recall only the commands that start with what is typed,
      as on vim's
- [ ] `H`, `M`, `L`, `z z`, `z t`, `z b`, Ctrl+e, and Ctrl+y in the panels and the lists
- [ ] Enter and `l` on a file open it
- [ ] Smaller keys of vim's: in the viewer `0`, `$`, `z h`, `z l`, and search (`/`, `?`, `n`,
      `N`); F1 and `g ?` close the help; Space goes back from the output of commands, as from
      vim's hit-enter prompt; Ctrl+u and Ctrl+d in the lists of dialogs; Ctrl+e closes the
      pull-down menu and the filters of lists, where Ctrl+y confirms; Backspace on an empty
      command line closes it; Replace mode (`R`, Insert in Insert mode), `Z Q`, `' '`, and
      marks (`m a`); a click or a paste gives up a command that waits, such as `d` or `"a`
- [x] The location menu replaces the virtual root and the list of hosts: groups Favorites,
      Mounts, and SFTP, the places used last first (`recent.toml`), the first ten of each with
      a row for the rest; F4 and F8 on hosts there; a Retry dialog for failed connections, and
      panels that stay on a lost host; no `..` in `/`
      ([ADR 0021](adr/0021-location-menu-replaces-the-virtual-root.md))

## Known issues

- **Non-UTF-8 remote file names end the SFTP session.** `openssh-sftp-client` 0.15 decodes names
  as UTF-8 (`openssh-sftp-protocol` deserializes `Box<Path>` through `ssh_format`, which calls
  `str::from_utf8`), so listing a directory with, say, a cp1251 or KOI8-R name fails and the
  session closes. Options: an upstream patch that keeps names as bytes, a fork, or our own SFTP
  v3 codec behind `SftpFs`.

## Backlog

- Commands of Noon Commander after `:` (`:!` runs a shell command already).
- Linux: CI job, release binaries, and packages (AUR, deb).
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
- Watch for volumes that are mounted or unmounted (DiskArbitration on macOS, `poll` on
  `/proc/self/mountinfo` on Linux) instead of reading them each time the location menu opens;
  eject from the menu.
- Plugins for other backends as a group of the location menu.
- zoxide on a host, over its master connection, for remote directories; it runs remote
  commands, which needs an ADR, as checksums on the server do.
- Anything behind the `forwarding` feature ([ADR 0004](adr/0004-forwarding-compile-time-feature.md)).
