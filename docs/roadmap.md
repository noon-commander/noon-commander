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
- [x] Debug commands: `noc debug hosts`, `noc debug ls <host>:<path>`
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
- [x] Marks: Space, Shift+Down, Shift+Up (were `J` and `K`), `g *`; their total below the
      listing
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
- [x] Help (F1) in sections, as the Configuration dialog: About (version, build, system, files)
      and Keys, which folds, with a group per part of the app; it opens where it was left
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
- [x] Fuzzy search as fzf does it in quick search, the location menu, and the zoxide window
      ([ADR 0016](adr/0016-fuzzy-search.md))
- [x] Search modes: plain, fzf, and regex in quick search and the filters of the lists,
      `ui.search_mode`, switched by Ctrl+s ([ADR 0030](adr/0030-search-modes.md))
- [x] `f /` searches the files below for text with ripgrep, local or on the host, in a dialog
      with a field for the text and one for the files; `[search]`
      ([ADR 0027](adr/0027-search-of-a-whole-tree.md),
      [ADR 0028](adr/0028-programs-on-hosts-without-a-terminal.md))
- [x] `f f` finds files and directories by name with fd, in the Files and dirs mode of `f /`
      ([ADR 0029](adr/0029-files-and-directories-with-fd.md))
- [ ] Panelize what the search of a tree found, as mc and Far do, to act on it in a panel
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
- [x] Scroll bars on the frames wherever rows do not fit, which the mouse presses and drags
      ([ADR 0024](adr/0024-scroll-bars-on-the-frames.md))
- [x] Text cut short marked with `»` and `«` (Nerd Font chevrons with icons) in an accent, `K`
      for the whole name or row under the cursor, the z keys to scroll names and cut lines
      sideways, the same marks in the viewer ([ADR 0025](adr/0025-marks-of-text-cut-short.md))
- [x] `g x` opens the marked entries, or the one under the cursor, externally, as vim's `gx`:
      `open` on macOS, `xdg-open` on Linux, local files only
      ([ADR 0026](adr/0026-open-externally.md))
- [ ] `g x` on a host: a copy of the remote file in the runtime directory, which the desktop's
      program then opens, as F4 edits a copy
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
- [x] Ctrl+k while the key hints show gives them the keys, as nvim's `K K`: a list of every
      sequence whole, which what is typed filters and Enter runs from
- [x] One keymap, vim's, in its modes, with mc's F-keys, in place of two presets chosen by
      `ui.keymap`, one modelled on mc ([ADR 0022](adr/0022-vim-modes.md)): Normal, Insert, and
      Visual mode in the panels, the lists, the dialogs, and the text fields; vim's motions,
      operators, registers, counts, and undo in text fields; Visual mode and a jump list
      (Ctrl+o, Ctrl+i) in the panels, where Tab goes to the other panel if the terminal tells it
      from Ctrl+i; the mode and the command typed on a line above the F-key
      bar, and the cursor's shape
- [ ] `.` repeats the last change, in text fields and of files
- [ ] Yanks copied to the system clipboard through OSC 52, behind a setting
- [ ] A key of vim's for editing the command line in `$EDITOR`
- [ ] Words as vim's 'iskeyword' parts them: also where the script changes (Latin, Han, kana,
      emoji), with `×` and `÷` as letters, and with U+200B as a blank
- [ ] A character and its combining marks as one, as vim's: an `e` with U+0301 (names in NFD,
      as macOS has them) is moved over, deleted, replaced, yanked, and typed after as one
      character, and the cursor never stops on the mark
- [ ] `j` and `k` keep the column on the screen, as vim's, rather than the character: a wide
      character takes two cells, and a tab goes to the next multiple of 8, as the command line
      would show it
- [ ] `~` switches the case of `ß` and `İ` as vim's does, to `ẞ` and `i`
- [ ] More of vim's commands in text fields: `r`, `u`, `U`, `D`, `X`, `C`, `S`, and `J` in
      Visual mode, and `U` in Normal mode; Shift and Ctrl with Left and Right move in Visual mode
      as `w`, `W`, `b`, and `B`; Ctrl+r Ctrl+r, Ctrl+r Ctrl+o, and Ctrl+r Ctrl+p put a register
      in Insert mode, as Ctrl+r does
- [ ] More of vim's keys: in text fields `g e`, `g E`, `|`, the operators `g u`, `g U`, and
      `g ~`, Ctrl+a and Ctrl+x, `g v`, `g i`, `g I`, `g p`, and `g P`; in the panels a count before
      `%`, and in Visual mode Ctrl+o, Tab, `O`, and `g v`
- [ ] More of vim's keys in text fields: `g _`, `%`, `v` and `V` after an operator (`d v e`),
      `V`, search (`*`, `#`, `/`, `?`, `n`, `N`), macros (`q`, `@`), and marks (`m`, `` ` ``),
      so that `q a` no longer falls through to `a` and `m b` to `b`; in Insert mode Ctrl+a and
      the register `.` (`" . p`, Ctrl+r `.`), Ctrl+q as Ctrl+v, and Ctrl+g `u`, where Ctrl+g
      now does nothing and `u` is typed
- [ ] The keys of vim's command line for completion: Shift+Tab with no list goes back from the
      last match, Ctrl+d lists the matches, Ctrl+l puts in what they share, and Ctrl+a all of them
- [ ] Text objects in text fields: `iw`, `aw`, `iW`, `aW`, quotes, and brackets (`c i w`,
      `d i "`, `c i (`); `c i w` only moves the cursor until then
- [ ] Registers as vim's: `"0` the last yank, `"1` … `"9` and `"-` the deletes; any other
      name than these, `a` … `z`, `A` … `Z`, `"`, and `_` gives the command up
- [ ] `p` in Visual mode puts in place of the selection; `Y` yanks the line, as vim's `y y`
- [ ] A count repeats what `i`, `a`, `I`, and `A` type (`3ix` Esc)
- [ ] The lines of the command line in Normal mode: `o`, `O`, `J`, `g g`, `G`, `d j`, `d k`, `V`,
      `r` Enter, and Ctrl+d and Ctrl+u, which scroll in vim's window of the command line; a put of
      lines within a line leaves the cursor at their start, and Ctrl+r of whole lines ends them with
      a line break
- [ ] The command line shows its mode and the command typed, as vim's window of the command
      line does: Normal and Visual mode after Ctrl+f, an operator or a register that waits
      (`2d`, `"a`), and `(insert)` after Ctrl+o
- [ ] Ctrl+v and a code types the character of that code, as vim's: `065`, `x41`, `o101`,
      `u20ac`, `U0001F600`
- [ ] `:` and `!` in Visual mode in the panels open the command line for the rows selected, as
      vim's `:'<,'>`
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
- [ ] Visual mode in the panels: `/`, `?`, `n`, and `N` move its end; `g ?`, `g m`, `g M`, `Z Z`,
      Ctrl+w 1 and 2, and `Y` do what they do in Normal mode, as their F-keys already do
- [ ] The jump list as vim's by default: a jump after Ctrl+o keeps the places after it, and a
      place already in the list moves to its end, rather than as `jumpoptions=stack`
- [ ] Up and Down on the command line recall only the commands that start with what is typed,
      as on vim's
- [ ] `H`, `M`, `L`, `z z`, `z t`, `z b`, Ctrl+e, and Ctrl+y in the panels and the lists
- [ ] Enter and `l` on a file open it
- [ ] Smaller keys of vim's: in the viewer `0`, `$`, `z h`, `z l`, and search (`/`, `?`, `n`, `N`);
      Space goes back from the output of commands, as from vim's hit-enter prompt; Ctrl+u and
      Ctrl+d in the lists of dialogs; Ctrl+e closes the pull-down menu and the filters of lists,
      where Ctrl+y confirms; Replace mode (`R`, Insert in Insert mode),
      `Z Q`, `' '`, and marks (`m a`); a click gives up a command that waits, such as `d` or `"a`,
      and a paste uses up a count or a register that waits, as vim's does
- [x] The location menu replaces the virtual root and the list of hosts: groups Favorites,
      Mounts, and SFTP, the places used last first (`recent.toml`), the first ten of each with
      a row for the rest; F4 and F8 on hosts there; a Retry dialog for failed connections, and
      panels that stay on a lost host; no `..` in `/`
      ([ADR 0021](adr/0021-location-menu-replaces-the-virtual-root.md))
- [x] Who is where: `user@host` on the line of the mode and in a shell's prompt; `#`, red, and
      a cursor on red for root; a glyph and `[root]` in the terminal title while Noon Commander
      runs as root
      ([ADR 0031](adr/0031-who-and-where-on-screen.md))
- [x] Letters in every dialog, as in Far and mc: Alt and the letter puts the focus in a field,
      switches a check box, chooses a radio button, or presses a button, while typing too;
      Alt+1 … Alt+8 open the categories of the Configuration dialog, and Alt and a letter the
      sections of the help ([ADR 0033](adr/0033-letters-in-dialogs.md))
- [x] Windows grow from their middle as they open and shrink back as they close, the menus of
      F9 drop from their bar; `animations.windows` ([ADR 0034](adr/0034-animated-windows.md))
- [x] One spinner for everything that waits, shown after 150 ms; `animations.spinners`; a panel
  turns braille dots in place of the icon before the path in its title, and the viewer in place of
  the file's icon, a skeleton line for the text; the location menu, the zoxide window, and the
  search of a whole tree turn them in front of their title, a skeleton row with a shine in place of
  the rows to come; a job's gauge is a faint bar with the shine running to and fro until it knows
  how far it is; R in the location menu keeps the rows; `--slow-spinners` and `--slow-connections`
  in debug builds ([ADR 0037](adr/0037-spinners.md))
- [ ] F3 reads a file in parts and shows how much it has read in a gauge, in percent of its size
      or of the limit, so that a large file over SFTP shows how far it is; cancel-safe, as the
      reading at once is now
- [x] Formats of the region for dates, times, and numbers: the system's (on macOS as System
      Settings has them), `iso`, or a language tag, `ui.formats`; plural forms that see the
      numbers in every message with a count ([ADR 0035](adr/0035-formats-of-the-region.md))
- [x] Names sorted as the language of the formats orders them, numbers by value;
      `ui.natural_sort`
- [x] Search that matches names in either Unicode form, NFC or NFD (as macOS has them), and
      ignores case by full case folding (`ß` as `ss`), in plain text, regular expressions,
      `g +` and `g -`, and completion
- [ ] Letters of other keyboard layouts as their keys in Normal and Visual mode, from the key the
      kitty keyboard protocol reports for the place on a standard keyboard, once crossterm reads
      it ([crossterm#1074](https://github.com/crossterm-rs/crossterm/pull/1074)); letters and
      digits at least, and whether Shift with a sign, as `;` for `:`, goes by the standard layout

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
