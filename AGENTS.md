# AGENTS.md

Guidelines for AI coding agents working in this repository. Human contributors should follow the same rules.

## Project description
<!-- >>> custom project_description -->
Two-panel, Midnight Commander-style terminal file manager for macOS and Linux. Async Rust (tokio, ratatui). Local and SFTP files are equally first-class. Drives the system OpenSSH client; the system `ssh` is the single source of truth for connections and auth.

- Pre-alpha. Read [docs/roadmap.md](docs/roadmap.md) before starting work.
- Background and philosophy: [README.md](README.md). Design: [docs/architecture.md](docs/architecture.md). Decisions: [docs/adr/](docs/adr/).
- Crates, their roles, and the allowed dependencies between them: [docs/architecture.md](docs/architecture.md#crates). Add no other `noc-*` dependency.
- Names: "Noon Commander" in prose only. Everything technical is `noc` (binary, `noc-*` crates, XDG directories, `NOC_*` variables); only packages are `noon-commander`.
- Significant decisions get an ADR in `docs/adr/`.
<!-- <<< custom project_description -->

## Basic

- **English only.** All code, comments, docstrings, commit messages, TODO notes, log messages, and test descriptions must be written in English. No exceptions.
- **Generated files.** `.clippy.toml`, `.deny.toml`, `.editorconfig`, `.gitignore`, `.rumdl.toml`, `.rustfmt.toml`, `.shellcheckrc`, `.taplo.toml`, `AGENTS.md`, `Cargo.toml`, `justfile`, `crates/*/Cargo.toml` are generated. Edit them only inside the `>>> custom` / `<<< custom` blocks; any change outside them is overwritten on the next sync.
  <!-- >>> custom basic -->
  <!-- <<< custom basic -->

## Code conventions

- **Read `.editorconfig` before editing a file.** `.editorconfig` is the single source of truth for line width. The repo-root `.editorconfig` declares `max_line_length`. Code, comments, docstrings, Markdown prose, and any other text you write must respect this limit.
- Do not hand-wrap narrower than `max_line_length`. Don't impose a personal 72/80-column habit on comments or docstrings. Let lines run up to the configured limit, then break.
- Comments explain why, not what. Keep them sparse.
- Edition and MSRV are `edition` and `rust-version` in the root `Cargo.toml`; don't use std APIs newer than the MSRV.
- `clippy::pedantic` and `clippy::cargo` are on, and `just lint` fails on any warning. Fix a lint rather than silence it; when silencing is right, use `#[expect(lint, reason = "...")]` on the narrowest item.
- No `unwrap`/`expect`/`dbg!`/`todo!`/`unimplemented!` outside tests; propagate errors with `?`.
- Prefer `pub(crate)`; `unreachable_pub` is on.
- Add a dependency's version to root `[workspace.dependencies]` first; `cargo add -p <crate> <name>` then writes `name.workspace = true`.
- A new crate goes to `crates/<name>/`; start its `Cargo.toml` from an existing crate's, so its dependencies sit inside the custom block.
  <!-- >>> custom code_conventions -->
- Errors: `thiserror` enums in libraries, `color-eyre` in the binary.
- Logging: `tracing` only. Library crates never print; the terminal belongs to the TUI.
- Secrets: `secrecy::SecretString` only. Never log, persist, or pass them via argv.
- `unsafe` is denied. Sole exception: `setsid` in the `pre_exec` hook of `noc-ssh`. Add no other.
- No blocking I/O in async code; use `spawn_blocking`.
- Cancellation: remote operations are cancel-safe; dropping the future abandons them cleanly. Long work (connect, transfer, recursive walk) also takes a `CancellationToken`.
- No `core` or `utils` crate. Shared code stays where it is used until a second crate needs it, then moves to a crate named for its topic.
- Library crates: no UI code, no user-facing text. Exceptions: `noc-viewer`, which knows nothing of the keymap, theme, or app (ADR 0015); `noc-scroll`, which draws the scroll bar but has no text (ADR 0023).
  <!-- <<< custom code_conventions -->

## Workflow

- The project is managed through [`just`](https://github.com/casey/just). Run `just --list` to see all available recipes; use them instead of calling the underlying tools directly.
- **Never** commit, push, tag, branch, or rewrite history unless explicitly asked. Commits use Conventional Commits, one logical change each.
- Run `just ci` before every commit.
  <!-- >>> custom workflow -->
- CI runs only `just` recipes (ADR 0013): a new check is a recipe called from `ci.yml`. Exception: `release.yml` builds from a signed `v*` tag (ADR 0014); package templates live in `packaging/`.
- After editing `assets/icons/logo.svg`, run `just render-logo`. A new PNG gets its own `_render-logo-png` line in the `render-logo` recipe.
- Platforms: macOS and Linux, both in CI; Windows is out of scope. Unix APIs via `rustix`; `nix` only for the signal mask (ADR 0054) and the names of users and groups (ADR 0098). Code for one platform goes under `cfg(target_os = "macos")` or `cfg(target_os = "linux")`.
- Keep `docs/architecture.md`, `docs/roadmap.md`, `docs/keymap-compare.md`, `docs/keymap-non-vim.md` in sync with code.
- Keep the Development section of `README.md` in sync with the `justfile`, `ci.yml`, and `rust-version`: the build commands, and an install command for every tool a recipe runs.
- User-visible changes go in `CHANGELOG.md` under `Unreleased`.
  <!-- <<< custom workflow -->

## Testing

<!-- >>> custom testing -->
- UI: `insta` snapshots on ratatui's `TestBackend`. Review with `just test-snapshots-review`; after renaming or removing a snapshot test, run `just test-snapshots-orphans`.
- SFTP backends: local `sftp-server` over pipes, no network: `$SFTP_SERVER`, else the usual paths of macOS and Linux. CI sets it, so tests fail rather than skip without one.
- ssh orchestration: set `ssh.program` to `crates/noc-ssh/tests/support/fake-ssh`. It emulates `-V`, `-G`, `-M`, `-O`, `-s … sftp`, and remote commands, and logs its command lines. Keep it in sync with the flags we pass.
- zoxide: `crates/noc-tools/tests/support/fake-zoxide`.
- Never touch the real `~/.ssh`, XDG directories, or zoxide database; use temporary directories.
<!-- <<< custom testing -->

<!-- >>> custom -->
## State files

- `config.toml` and `hosts.toml` (XDG config), `workspaces.toml` (XDG data), `history.toml` and `recent.toml` (XDG state): paths come from `crates/noc-config/src/paths.rs`. Write them only through `noc-config`, which replaces them atomically and keeps the user's comments and other keys in `config.toml` and `hosts.toml`. Per-host settings go only in `hosts.toml` (ADR 0007).
- Only the TUI logs: a file per run in `logs/` in XDG state, written by `crates/noc/src/logging.rs`. Settings in `[log]`, no environment variable (ADR 0067).

## UI conventions

### Keymap

- One keymap: vim's, in its modes, with mc's F-keys (ADR 0022).
- `Mode` and `Context` live in `crates/noc/src/tui/keymap/action.rs`, the bindings in `preset.rs`; an action's first key is what the pull-down menu shows. `App::place` says where keys go: a new window or field reports its mode.
- Counts, registers, the character after `f`, `t`, `r`, `"`, Ctrl+r, Ctrl+v, and Ctrl+x come with the action (`keymap::Typed`, `Extra`), never as bindings (ADR 0051).
- Keys act on the active panel. Ctrl+x gives the other panel the keys for one command and takes them back once it ends (`App::come_back`); commands need no code of their own for it. Every window opens centered over both panels; the panel drawn active is the one it acts on, and its title names no side.
- Code addresses panels by `PanelId` (side + tab number, never reused), never by `Side` (ADR 0011).
- The F-key bar and help are generated from the keymap; never write them by hand.
- Changing any binding: update the `noc` column of [docs/keymap-compare.md](docs/keymap-compare.md) in the same change. A new action gets a row in its mode and context's section, in preset order, with the keys of mc, Far, yazi, ranger. A key that is not vim's also gets a row in [docs/keymap-non-vim.md](docs/keymap-non-vim.md): why, the vim-style keys that do the same, and what it does without the kitty keyboard protocol.
- Keys in text (help, menus, Fluent, docs, comments) follow `keymap::describe`: `Ctrl+l`, `G`, `Alt+W`, `Shift+F6`, `Ctrl+w h`, `PgUp` (full rules in docs/keymap-compare.md). The preset uses crokey syntax (`ctrl-w h`, `shift-z`).
- Mouse: a widget translates a press into the key it stands for (Enter, Esc) and sends it down the keyboard path. Release the mouse on editor hand-off, exit, and panic (ADR 0018).
- Widgets never match raw keys. They receive `Action`s from the keymap.

### Text

- User-facing text lives only in Fluent files, `en-US`, read with `fl!`: `crates/noc/i18n/`, the viewer's in `crates/noc-viewer/i18n/`. A crate with its own text has its own loader, which `noc::i18n::select` selects too. clap `--help` and logs stay outside Fluent.
- Numbers, dates, and times on screen go through `noc_locale::Formats` (ADR 0035), never `format!`; pass numbers to `fl!` as numbers. Group every number, file sizes too, but not a uid or gid (ADR 0096). A time shows its date with the year and its time of day, in digits (`Formats::date_time`).
- No code for one language or region: no layout tables, no special cases, no tags outside settings. Formats come from the system or CLDR; tests use `iso` and patterns of their own.
- A command of the F9 menu ends in `…` when it asks for input before acting (Copy…, Quick search…, Tab list…), not when it only confirms (Delete, Exit) or its window is the result (View, Help).

### Look

- Special characters (icons, markers, frame lines, scroll bars, marks of cut text, checks, spinner) live only in `crates/noc/src/tui/glyphs.rs`: Nerd and plain sets by `ui.icons`, lines by `ui.borders`. Library crates take them as parameters. `scripts/glyphs.sh`, part of `just lint`, checks; Fluent text does not count.
- Icons: Nerd Fonts v3 as literal glyphs in Rust and TOML, never escape sequences. Without icons: `/` directory, `*` executable, `@` symlink, `~` symlink to a directory
- Themes: one function over named palette roles per family. Without truecolor, map to indexes 16–255 only, never the low 16 (ADR 0010).
- Fuzzy matching: `nucleo-matcher` only, only in `crates/noc/src/tui/fuzzy.rs` (ADR 0016).
- A list whose rows come late turns the dots of its title (`window::draw_title_dots`) and shows the skeleton (`window::draw_skeleton`) for as many rows as it had last time; never an empty box, a growing box, or a text that says it waits (ADR 0037).

### Line of the mode

- It shows the mode, then `·` and the kind of window by keymap context (`NORMAL · List`); never a window's title or name, never empty (ADR 0032). The panels show the mode alone; quick search and the command line take the line while they have the keys (ADR 0093).
- A new `Context` gets its kind in `window_kind` (`crates/noc/src/tui/app/render.rs`, no wildcard). A new kind comes only with keys of its own and its `window-` Fluent text.

### Settings

- Every option is editable in Tools → Configuration…: a new `config.toml` option ships with its dialog row (name, hint, Fluent text) and is saved through `noc_config::save_config` (ADR 0009). Apply a change at once where possible; otherwise the dialog says it needs a restart.

## Processes and SSH

- Spawn `ssh` only from `noc-ssh`, other programs only from `noc-tools` (ADR 0012): never through a shell, `--` before destinations and paths, killed on drop, a fake program in tests. Sole exception: at build time, `crates/noc/build.rs` runs `git` for `noc debug version`.
- The sole shell is the command line's `shell.run` (`$SHELL -i -c`), whose command is the user's (ADR 0019, ADR 0054).
- Not killed on drop: the opener of `g x` (`open`, `xdg-open`), detached, output to `/dev/null`, given an absolute path without `--`, which `xdg-open` refuses (ADR 0026); a command of the command line or the editor still running when a SIGHUP or SIGTERM ends Noon Commander (ADR 0054).
- Clipboard: write through OSC 52 only, never read it, never call `pbcopy`/`xclip` (ADR 0008).
- Never forward anything (ADR 0062): sessions always get `policy::SESSION_OPTIONS`, SFTP channels `policy::SFTP_CHANNEL_OPTIONS`. Forced options live in `policy.rs`; change them only with an ADR.
- User-supplied ssh arguments must pass the validator (ADR 0004); unknown flags and forwarding arguments are errors.
- Never pass `StrictHostKeyChecking=no`. Never write to `~/.ssh/`.
- Command-line order: program → forced options → `-F` and `ssh.args` → role options → `--` → destination → remote command (if any).
- `ssh -G` runs `Match exec`: call it lazily (on selection or connect), never for every host at startup.
- Minimum OpenSSH 8.7; `StdinNull` and `ForkAfterAuthentication` are forced off.
- Every ssh child runs in its own session (`setsid`) without a controlling terminal; prompts go through the askpass bridge. Exception: the command line's ssh keeps the terminal (ADR 0019).
- Programs noc runs on a host (`rg`, `fd`, `fdfind`, `id`) go through `Session::spawn_in`: no terminal, every word quoted for a POSIX shell and fish (ADR 0028, ADR 0054, ADR 0097).
- `ssh -O` commands use `-F /dev/null`.

<!-- <<< custom -->
