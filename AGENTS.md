# AGENTS.md

Instructions for AI coding agents (and humans) working on Noon Commander.

**English only.** All code, comments, docstrings, commit messages, TODO notes, log messages, and
test descriptions must be written in English. No exceptions.

## Project

Noon Commander (binary `noc`) is a Midnight Commander-style, two-panel terminal file manager for
macOS and Linux, focused on seamless local and SFTP file operations, written in async Rust
(tokio, ratatui). SFTP is its foundation and the reason it exists. It drives the system OpenSSH
client instead of implementing SSH.

Naming: the full name "Noon Commander" is used only in prose; everything technical is `noc`
(binary, crates `noc-*`, XDG directories, `NOC_*` environment variables).

- Architecture: [docs/architecture.md](docs/architecture.md)
- Roadmap and current milestone: [docs/roadmap.md](docs/roadmap.md)
- Decisions: [docs/adr/](docs/adr/)

Status: pre-alpha. Read the roadmap before starting work. Browsing (M2) and file operations
(M3) work; polish (M4) is next.

## Philosophy

- A full file manager: local and remote files are equally first-class. SFTP is the foundation;
  other backends may be added behind the `Vfs` trait.
- Noon Commander moves files; it does not tunnel anything.
- The system `ssh` is the single source of truth for connections, configuration, and
  authentication.
- The Noon Commander config decorates hosts from `ssh_config` (labels, start directories,
  hiding); it never duplicates them.
- The UI never waits on the network. Remote operations are asynchronous, cancellable, and report
  progress.

Non-goals: port, agent, X11, or tunnel forwarding; an in-process SSH implementation; storing
credentials; editing `~/.ssh/*`.

## Layout

| Crate | Responsibility |
| --- | --- |
| `crates/noc` | Binary and UI: CLI, bootstrap, askpass entry point, ratatui app, keymap, themes, icons, i18n |
| `crates/noc-config` | XDG paths, TOML schema, defaults |
| `crates/noc-ssh` | Host discovery, `ssh -G`, argument validation, forwarding policy, ControlMaster, SFTP channels, askpass bridge |
| `crates/noc-vfs` | `Vfs` trait and backends (local, SFTP), mounted volumes |
| `crates/noc-ops` | Job engine: copy, move, delete, mkdir, checksums; progress, cancellation, conflicts |
| `crates/noc-tools` | External programs other than ssh: zoxide, the editor |

Dependencies point one way: `config ← ssh ← vfs ← ops ← noc`, and `tools ← noc`. Library
crates contain no UI code and no user-facing text.

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features forwarding -- -D warnings
cargo test --workspace
cargo test --workspace --features forwarding
cargo deny check    # brew install cargo-deny
```

Work is done when all of them pass.

## Hard rules

- No forwarding by default. Forwarding code may exist only behind the `forwarding` feature,
  which is checked solely in `crates/noc-ssh/src/policy.rs`. SFTP channels always use
  `policy::SFTP_CHANNEL_OPTIONS`.
- Never add an SSH implementation (`russh`, `ssh2`, `libssh2-sys`); `deny.toml` bans them.
- Spawn `ssh` only from `noc-ssh`: never through `sh -c`, always with `--` before the
  destination. Spawn every other program only from `noc-tools` (ADR 0012): never through a
  shell, with `--` before paths, killed on drop; tests use a fake program.
- User-supplied ssh arguments must pass the validator (ADR 0004). Unknown flags are errors.
- Never pass `StrictHostKeyChecking=no` and never write to `~/.ssh/`.
- No blocking I/O in async code; use `spawn_blocking`. Remote operations must be cancel-safe
  (dropping the future abandons them cleanly); long-running work such as connecting, transfers,
  and recursive walks also takes a `CancellationToken`.
- Secrets live in `secrecy::SecretString`. Never log them, persist them, or pass them via argv.
- No user-facing TUI text in code: it goes to Fluent files and is read with `fl!`.
- Widgets never match raw keys; they receive `Action`s from the keymap.

## Code conventions

- Edition 2024, MSRV 1.88 (`rust-version`). CI checks the MSRV, so don't use newer std APIs.
- Dependency versions are declared once in the root `[workspace.dependencies]`; crates use
  `name.workspace = true`. Every crate has `[lints] workspace = true`.
- Errors: `thiserror` enums in libraries, `color-eyre` in the binary. No `unwrap`/`expect`
  outside tests.
- Logging: `tracing` only, written to a file in the XDG state directory (the terminal belongs to
  the TUI). Library crates never print.
- `unsafe` is denied. The only planned exception is `setsid` in a `pre_exec` hook in
  `noc-ssh`, with a local `#[allow(unsafe_code)]` and a `// SAFETY:` comment.
- Prefer `pub(crate)`; `unreachable_pub` is on.
- Comments explain why, not what, and stay sparse.

## UI conventions

- Keymap: actions per context (`panel`, `dialog`, `viewer`, `quick_search`, `menu`); bindings are
  key sequences so a vim preset can be added. The default preset is mc. The F-key bar and help
  are generated from the active keymap. When you add, change, or remove a binding in any preset,
  update the `noc (default)` or `noc (vim)` column in
  [docs/keymap-compare.md](docs/keymap-compare.md) in the same change; an action that mc and Far
  lack gets its own row, or a `(noc)` section.
- Text: Fluent files in `crates/noc/i18n/`, `en-US` only for now. Library errors are typed;
  the UI turns them into messages. clap `--help` output and logs stay English.
- Icons: Nerd Fonts v3, written as literal glyphs in Rust and TOML, never as escape sequences.
  Icons are optional (`ui.icons`, on by default); without them use mc markers: `/` directory,
  `*` executable, `@` symlink, `~` symlink to a directory.
- Settings: every option in `config.toml` must be editable in the UI, in the Configuration
  dialog (Options → Configuration…); users should never have to edit the file by hand. A new
  option ships together with its row in the dialog (name, hint, Fluent text) and is written
  back through `noc_config::save_config` ([ADR 0009](docs/adr/0009-configuration-dialog-writes-config-toml.md)).
  Apply it at once where the running app can; otherwise the dialog says it takes effect after
  a restart.

## SSH integration

- Build ssh command lines in this order: program → forced options → `ssh.args` →
  role options → `--` → destination.
- Forced options live in `policy.rs`; change them only together with an ADR.
- `ssh -G` runs `Match exec` predicates: call it lazily (on selection or connect), never for
  every host at startup.
- Minimum OpenSSH is 8.7: `SSH_ASKPASS_REQUIRE` (8.4) and the `StdinNull` and
  `ForkAfterAuthentication` keywords (8.7), which Noon Commander forces off.
- Every ssh child runs in its own session (`setsid`), without a controlling terminal; prompts go
  through the askpass bridge. `ssh -O` commands use `-F /dev/null`.

## Testing

- Unit tests live next to the code in `#[cfg(test)] mod tests`.
- UI: `insta` snapshots on ratatui's `TestBackend`; review with `cargo insta review`.
- SFTP backends run against the local `sftp-server` (`/usr/libexec/sftp-server` on macOS) over
  pipes, with no network.
- ssh orchestration tests set `ssh.program` to `crates/noc-ssh/tests/support/fake-ssh`, a
  POSIX shell script that emulates `-V`, `-G`, `-M`, `-O`, and `-s … sftp` (served by the local
  `sftp-server`) and logs its command lines. Keep it in sync with the flags we pass.
- zoxide tests use `crates/noc-tools/tests/support/fake-zoxide` the same way; tests never touch
  the real zoxide database.
- Tests must not touch the real `~/.ssh` or XDG directories; use temporary directories.

## Platforms

macOS first: all development and CI run there. Linux is a target, so keep code portable (Unix
APIs via `rustix`, nothing macOS-specific outside `cfg(target_os = "macos")`). Windows is out of
scope.

## Git

- Never commit, push, tag, or create branches unless the maintainer explicitly asks.
- Never rewrite history (amend, rebase, force-push) unless explicitly asked.
- When asked to commit: Conventional Commits, one logical change per commit.

## Docs

- Record significant decisions as ADRs in `docs/adr/` (`NNNN-title.md`: Status, Context,
  Decision, Consequences).
- Keep `docs/architecture.md`, `docs/roadmap.md`, and `docs/keymap-compare.md` in sync with the
  code.
- Add user-visible changes to `CHANGELOG.md` under `Unreleased`.
