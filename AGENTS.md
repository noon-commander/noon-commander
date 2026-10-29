# AGENTS.md

Instructions for AI coding agents and humans working on Noon Commander.

**English only**: code, comments, docs, commits, logs, test names. `just typos` checks spelling;
add false positives to `typos.toml`, with a comment if the reason is not obvious.

## Hard rules

- Forwarding exists only behind the `forwarding` feature, checked only in
  `crates/noc-ssh/src/policy.rs`. SFTP channels always use `policy::SFTP_CHANNEL_OPTIONS`.
- Never add an SSH implementation (`russh`, `ssh2`, `libssh2-sys`); `deny.toml` bans them.
- Spawn `ssh` only from `noc-ssh`. Never through `sh -c`. Always `--` before the destination.
- Spawn other programs only from `noc-tools` (ADR 0012). Never through a shell. `--` before
  paths. Kill on drop. Tests use a fake program. At build time, `crates/noc/build.rs` runs
  `git` for `noc debug version`.
- Sole shell exception: the command line's `shell.run` (`$SHELL -i -c`), whose command is the
  user's (ADR 0019, ADR 0054).
- Sole exception to kill on drop: the opener of `g x` (`open`, `xdg-open`), detached, output to
  `/dev/null`; it takes an absolute path without `--`, which `xdg-open` refuses (ADR 0026).
- User-supplied ssh arguments must pass the validator (ADR 0004). Unknown flags are errors.
- Never pass `StrictHostKeyChecking=no`. Never write to `~/.ssh/`.
- Clipboard: write through OSC 52 only. Never read it. Never call `pbcopy`/`xclip` (ADR 0008).
- No blocking I/O in async code; use `spawn_blocking`.
- Remote operations are cancel-safe: dropping the future abandons them cleanly. Long work
  (connect, transfer, recursive walk) also takes a `CancellationToken`.
- Secrets: `secrecy::SecretString` only. Never log, persist, or pass them via argv.
- No user-facing TUI text in code. Put it in Fluent files; read it with `fl!`.
- Widgets never match raw keys. They receive `Action`s from the keymap.
- Address panels by `PanelId` (side + tab number, never reused), not `Side` (ADR 0011).

## Commands

```sh
just check   # fmt-check, clippy and tests (with and without forwarding), cargo-deny
just lint    # Markdown, shell scripts, spelling, TOML, workflows, unused dependencies
just all     # check + lint + stale snapshots; run before committing
just fmt     # format the code
just run     # run noc from source; arguments go to noc
```

- Done means `just check` passes. It changes no files.
- Iterate on one crate with `cargo test -p <crate> [name]` and
  `cargo clippy -p <crate> --all-targets`. `just test`/`just clippy` always build the whole
  workspace twice.
- Other recipes: `just --list`. `just msrv` needs rustup; `just logo` needs `resvg`.
- CI runs `justfile` recipes (ADR 0013). Add a new check as a recipe and call it from `ci.yml`;
  never put raw commands in `ci.yml`.
- Releases are the exception: `release.yml` builds from a signed `v*` tag (ADR 0014). Package
  templates live in `packaging/`. Packages are named `noon-commander`; `noc` is the binary.
- After editing `assets/icons/logo.svg`, run `just logo`. A new PNG gets its own `_logo-png`
  line in the `logo` recipe.

## Project

Two-panel, Midnight Commander-style terminal file manager for macOS and Linux. Async Rust
(tokio, ratatui). Local and SFTP files are equally first-class. Drives the system OpenSSH
client; the system `ssh` is the single source of truth for connections and auth.

- Pre-alpha. M0–M3 done; M4 (polish) is current. Read [docs/roadmap.md](docs/roadmap.md)
  before starting work.
- Background and philosophy: [README.md](README.md). Design:
  [docs/architecture.md](docs/architecture.md). Decisions: [docs/adr/](docs/adr/).
- Names: "Noon Commander" in prose only. Everything technical is `noc` (binary, `noc-*`
  crates, XDG directories, `NOC_*` variables).
- Non-goals: any forwarding or tunneling, an in-process SSH, storing credentials, editing
  `~/.ssh/*`.

## Layout

| Crate | Responsibility |
| --- | --- |
| `crates/noc` | Binary and UI: CLI, bootstrap, askpass entry point, ratatui app, keymap, themes, icons, i18n |
| `crates/noc-config` | XDG paths, TOML schemas, defaults, writing config files |
| `crates/noc-locale` | Formats of the region: numbers, dates, times, the order of names (ADR 0035) |
| `crates/noc-ssh` | Host discovery, `ssh -G`, argument validation, forwarding policy, ControlMaster, SFTP channels, askpass bridge |
| `crates/noc-vfs` | `Vfs` trait and backends (local, SFTP), mounted volumes |
| `crates/noc-ops` | Job engine: copy, move, delete, mkdir, checksums; progress, cancellation, conflicts |
| `crates/noc-tools` | External programs other than ssh: zoxide, ripgrep, fd, the editor, the opener, the shell of `!` |
| `crates/noc-text` | Terminal-safe text: sanitizing, widths in cells, fitting, wrapping |
| `crates/noc-pattern` | Patterns typed to match names: regular expressions or text as it is, smart case |
| `crates/noc-scroll` | Paging the help and the viewer: how far the page keys go, the scroll bar |
| `crates/noc-vars` | Variables in text: `$NAME`, `${NAME}`, `$$`, read once, expanded where it runs (ADR 0054) |
| `crates/noc-viewer` | The F3 viewer: reading through the `Vfs`, scrolling, wrapping; its own Fluent text |

- `noc-vfs`, `noc-text`, `noc-scroll`, `noc-pattern`, `noc-locale`, `noc-vars` depend on no
  other `noc-*` crate.
- `noc-config` → `noc-vars`. `noc-ssh` → `noc-vars`. `noc-tools` → `noc-pattern`, `noc-vars`.
  `noc-ops` → `noc-vfs`. `noc-viewer` → `noc-vfs`, `noc-text`, `noc-scroll`, `noc-locale`. The
  binary → all.
- Library crates: no UI code, no user-facing text. Exceptions: `noc-viewer`, which knows nothing
  of the keymap, theme, or app (ADR 0015); `noc-scroll`, which draws the scroll bar but has no
  text (ADR 0023).
- No `core` or `utils` crate. Shared code stays where it is used until a second crate needs it,
  then moves to a crate named for its topic.

## State files

Paths come from `crates/noc-config/src/paths.rs`.

| File | Location | Holds |
| --- | --- | --- |
| `config.toml` | XDG config | All settings except per-host ones (ADR 0009) |
| `hosts.toml` | Next to `config.toml` | Typed per-host settings; `[hosts]` in `config.toml` is an error (ADR 0007) |
| `workspaces.toml` | XDG data | Saved tabs of both panels (ADR 0017) |
| `history.toml` | XDG state | Command-line history |
| `recent.toml` | XDG state | The hosts and volumes used last, for the location menu |

- Write these files only through `noc-config`, which replaces them atomically (`write.rs`).
  Edits to `config.toml` and `hosts.toml` keep the user's comments and other keys.
- Logs go to a file in XDG state.

## Code conventions

- Edition 2024, MSRV 1.88. No newer std APIs; CI checks the MSRV.
- `clippy::pedantic` is on workspace-wide; clippy runs with `-D warnings`.
- No `unwrap`/`expect`/`dbg!`/`todo!` outside tests.
- Errors: `thiserror` enums in libraries, `color-eyre` in the binary.
- Logging: `tracing` only. Library crates never print; the terminal belongs to the TUI.
- `unsafe` is denied. Sole planned exception: `setsid` in a `pre_exec` hook in `noc-ssh`, with
  a local `#[allow(unsafe_code)]` and a `// SAFETY:` comment.
- Prefer `pub(crate)`; `unreachable_pub` is on.
- Dependencies: declare versions once in root `[workspace.dependencies]`; crates use
  `name.workspace = true`. Every crate has `[lints] workspace = true`. Remove unused ones from
  both places (`just unused`).
- TOML is formatted by taplo: `just toml` checks, `just toml-fmt` formats.
- Comments explain why, not what. Keep them sparse.

## UI conventions

### Keymap

- One keymap: vim's, in its modes, with mc's F-keys (ADR 0022). No other mc keys, no second
  preset, no `ui.keymap`.
- Modes and contexts: `Mode` and `Context` in `crates/noc/src/tui/keymap/action.rs`. Bindings
  are key sequences per mode and context; `Context::chain(mode)` gives the fallbacks.
  `global` keys work in every mode.
- The preset: `crates/noc/src/tui/keymap/preset.rs`. An action's first key is what the
  pull-down menu shows. In Insert mode characters are text; in Normal mode unbound ones do
  nothing.
- The app says where keys go: `App::place` returns the mode and the context. A new window or
  field reports its mode; text fields keep theirs in their `edit::Editor`.
- Counts, registers, and the character after `f`, `t`, `r`, `"`, Ctrl+r, Ctrl+v come with the
  action (`keymap::Typed`, `Extra`), never as bindings. So does Ctrl+x, which runs the command
  in the other panel (ADR 0051).
- Keys act on the active panel. Ctrl+x gives the other panel the keys for one command, any
  command, and gives them back once it ends (`App::come_back`); a command needs nothing of its
  own for it. Every window opens centered over both panels, one panel's too: the panel drawn
  active is the one it acts on, and its title names no side.
- The F-key bar and help are generated from the keymap, per mode and context.
- Changing any binding: update the `noc` column of
  [docs/keymap-compare.md](docs/keymap-compare.md) in the same change. A new action gets a row
  in its mode and context's section, in preset order, with the keys of mc, Far, yazi, ranger.
  A key that is not vim's also gets a row in [docs/keymap-non-vim.md](docs/keymap-non-vim.md):
  the reason for it, the vim-style keys that do the same, and what it does without the kitty
  keyboard protocol.
- Keys in text (help, menus, Fluent, docs, comments) follow `keymap::describe`: `Ctrl+l`, `G`,
  `Alt+W`, `Shift+F6`, `Ctrl+w h`, `PgUp`. Full rules: docs/keymap-compare.md. The preset uses
  crokey syntax (`ctrl-w h`, `shift-z`).
- Mouse: a widget translates a press into the key it stands for (Enter, Esc) and sends it down
  the keyboard path. Release the mouse on editor hand-off, exit, and panic (ADR 0018).

### Text

- Fluent files: `crates/noc/i18n/`, viewer's own in `crates/noc-viewer/i18n/`. `en-US` only.
- A crate with its own text has its own loader; `noc::i18n::select` selects its language too.
- Library errors are typed; the UI turns them into messages.
- Numbers, dates, and times on screen go through `noc_locale::Formats` (ADR 0035), never
  `format!`; pass numbers to `fl!` as numbers, so plural forms and the formatter see them. Every
  number is grouped, file sizes too. A time always shows its date, with the year, and its time of
  day, in digits (`Formats::date_time`).
- No code for one language or region: no tables of a layout, no special cases, no tags but in
  settings. Formats come from the system or CLDR; tests use `iso` and patterns of their own.
- clap `--help` and logs stay English, outside Fluent.

### Look

- Special characters (icons, markers, lines and tees of frames, scroll bars, marks of text cut
  short, checks, spinner) live only in `crates/noc/src/tui/glyphs.rs`: Nerd and plain sets by
  `ui.icons`, lines by `ui.borders`. Library crates take them as parameters. `just glyphs`
  checks; Fluent text is text and does not count.
- Icons: Nerd Fonts v3 as literal glyphs in Rust and TOML, never escape sequences.
- Icons are optional (`ui.icons`, on by default). Without them use mc markers: `/` directory,
  `*` executable, `@` symlink, `~` symlink to a directory.
- Themes: one function over named palette roles per family. Without truecolor, map to indexes
  16–255 only, never the low 16 (ADR 0010).
- Fuzzy matching: `nucleo-matcher` only, only in `crates/noc/src/tui/fuzzy.rs` (ADR 0016).
- A list whose rows do not come at once turns the dots in front of its title
  (`dialog::draw_title_dots`) and shows the skeleton with its shine (`dialog::draw_skeleton`) in
  place of its first rows, as many as it had the last time it opened; never an empty box, a box that
  grows when the rows come, or a text that says it waits (ADR 0037).

### Line of the mode

- It says which keys work: the mode, then `·` and the kind of window by its keymap context
  (`List`, `Menu`, `Dialog`, `Help`, `Viewer`, `Quick search`, `Rename`), as `NORMAL · List`.
  The panels show the mode alone. It is never empty (ADR 0032).
- Same keys, same kind. Never put a window's title or own name there.
- A new `Context` gets its kind in `window_kind` (`crates/noc/src/tui/app.rs`), whose match has
  no wildcard. A new kind comes only with keys of its own, with its `window-` Fluent text.

### Settings

- Every option must be editable in Options → Configuration…; users never edit files by hand.
- A new `config.toml` option ships with its dialog row (name, hint, Fluent text) and is saved
  through `noc_config::save_config` (ADR 0009). Per-host options go to `hosts.toml` (ADR 0007).
- Apply a change immediately where possible; otherwise the dialog says it needs a restart.

## SSH integration

- Command-line order: program → forced options → `-F` and `ssh.args` → role options → `--` →
  destination → remote command (if any).
- Forced options live in `policy.rs`. Change them only with an ADR.
- `ssh -G` runs `Match exec`: call it lazily (on selection or connect), never for every host
  at startup.
- Minimum OpenSSH 8.7: `SSH_ASKPASS_REQUIRE` (8.4); `StdinNull` and `ForkAfterAuthentication`
  (8.7), which are forced off.
- Every ssh child runs in its own session (`setsid`) without a controlling terminal; prompts go
  through the askpass bridge. Exception: the command line's ssh keeps the terminal (ADR 0019).
- A program noc runs on a host (`rg`, `fd`, `fdfind`, for `f /` and `f f`) goes through
  `Session::spawn_in`: no terminal, every word quoted for a POSIX shell (ADR 0028).
- `ssh -O` commands use `-F /dev/null`.

## Testing

- Unit tests next to the code in `#[cfg(test)] mod tests`.
- UI: `insta` snapshots on ratatui's `TestBackend`. Review with `just snap`. After renaming or
  removing a snapshot test, `just snap-stale` finds orphaned files.
- SFTP backends: local `sftp-server` (`/usr/libexec/sftp-server` on macOS) over pipes, no
  network.
- ssh orchestration: set `ssh.program` to `crates/noc-ssh/tests/support/fake-ssh`. It emulates
  `-V`, `-G`, `-M`, `-O`, `-s … sftp`, and remote commands, and logs its command lines. Keep it
  in sync with the flags we pass.
- zoxide: `crates/noc-tools/tests/support/fake-zoxide`. Never touch the real database.
- Never touch the real `~/.ssh` or XDG directories; use temporary directories.
- Shell scripts: POSIX `sh`, pass ShellCheck (`just sh`).

## Workflow

- Platforms: macOS first (development and CI Rust jobs). Linux is a target: Unix APIs via
  `rustix` (`nix` only for the signal mask, which `rustix` lacks; ADR 0054), macOS-only code
  under `cfg(target_os = "macos")`. Windows is out of scope.
- Git: never commit, push, tag, branch, or rewrite history unless explicitly asked. Commits use
  Conventional Commits, one logical change each.
- ADRs: significant decisions go in `docs/adr/NNNN-title.md` (Status, Context, Decision,
  Consequences).
- Keep `docs/architecture.md`, `docs/roadmap.md`, `docs/keymap-compare.md`,
  `docs/keymap-non-vim.md` in sync with code.
- User-visible changes go in `CHANGELOG.md` under `Unreleased`.
- Markdown follows `.markdownlint.yaml` (100-character lines); `just md` checks it.
