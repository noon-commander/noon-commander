# AGENTS.md

Instructions for AI coding agents (and humans) working on sftp-tui.

## Project

sftp-tui is a Midnight Commander-style, two-panel file manager for SFTP, written in async Rust
(tokio, ratatui). It drives the system OpenSSH client instead of implementing SSH.

- Architecture: [docs/architecture.md](docs/architecture.md)
- Roadmap and current milestone: [docs/roadmap.md](docs/roadmap.md)
- Decisions: [docs/adr/](docs/adr/)

Status: pre-alpha. Read the roadmap before starting work; most crates are still empty.

## Philosophy

- SFTP only. sftp-tui moves files; it does not tunnel anything.
- The system `ssh` is the single source of truth for connections, configuration, and
  authentication.
- The sftp-tui config decorates hosts from `ssh_config` (labels, start directories, hiding); it
  never duplicates them.
- The UI never waits on the network. Remote operations are asynchronous, cancellable, and report
  progress.

Non-goals: port, agent, X11, or tunnel forwarding; protocols other than SFTP; an in-process SSH
implementation; storing credentials; editing `~/.ssh/*`.

## Layout

| Crate | Responsibility |
| --- | --- |
| `crates/sftp-tui` | Binary and UI: CLI, bootstrap, askpass entry point, ratatui app, keymap, themes, icons, i18n |
| `crates/sftp-tui-config` | XDG paths, TOML schema, defaults |
| `crates/sftp-tui-ssh` | Host discovery, `ssh -G`, argument validation, forwarding policy, ControlMaster, SFTP channels, askpass bridge |
| `crates/sftp-tui-vfs` | `Vfs` trait and backends: virtual root, local, SFTP |
| `crates/sftp-tui-ops` | Job engine: copy, move, delete, mkdir; progress, cancellation, conflicts |

Dependencies point one way: `config ← ssh ← vfs ← ops ← sftp-tui`. Library crates contain no UI
code and no user-facing text.

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

- SFTP only. Forwarding code may exist only behind the `forwarding` feature, which is checked
  solely in `crates/sftp-tui-ssh/src/policy.rs`. SFTP channels always use
  `policy::SFTP_CHANNEL_OPTIONS`.
- Never add an SSH implementation (`russh`, `ssh2`, `libssh2-sys`); `deny.toml` bans them.
- Spawn `ssh` only from `sftp-tui-ssh`: never through `sh -c`, always with `--` before the
  destination.
- User-supplied ssh arguments must pass the validator (ADR 0004). Unknown flags are errors.
- Never pass `StrictHostKeyChecking=no` and never write to `~/.ssh/`.
- No blocking I/O in async code. Every remote operation takes a `CancellationToken`.
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
  `sftp-tui-ssh`, with a local `#[allow(unsafe_code)]` and a `// SAFETY:` comment.
- Prefer `pub(crate)`; `unreachable_pub` is on.
- Comments explain why, not what, and stay sparse.

## UI conventions

- Keymap: actions per context (`panel`, `dialog`, `viewer`, `quick_search`, `menu`); bindings are
  key sequences so a vim preset can be added. The default preset is mc. The F-key bar and help
  are generated from the active keymap.
- Text: Fluent files in `crates/sftp-tui/i18n/`, `en-US` only for now. Library errors are typed;
  the UI turns them into messages. clap `--help` output and logs stay English.
- Icons: Nerd Fonts v3, written as literal glyphs in Rust and TOML, never as escape sequences.
  Icons are optional (`ui.icons`, on by default); without them use mc markers: `/` directory,
  `*` executable, `@` symlink, `~` symlink to a directory.

## SSH integration

- Build ssh command lines in this order: program → forced options → `ssh.args` → host `args` →
  role options → `--` → destination.
- Forced options live in `policy.rs`; change them only together with an ADR.
- `ssh -G` runs `Match exec` predicates: call it lazily (on selection or connect), never for
  every host at startup.
- Minimum OpenSSH is 8.4 (`SSH_ASKPASS_REQUIRE`).

## Testing

- Unit tests live next to the code in `#[cfg(test)] mod tests`.
- UI: `insta` snapshots on ratatui's `TestBackend`; review with `cargo insta review`.
- SFTP backends run against the local `sftp-server` (`/usr/libexec/sftp-server` on macOS) over
  pipes, with no network.
- ssh orchestration tests set `ssh.program` to a fake ssh that emulates `-V`, `-G`, `-M`, `-O`,
  and `-s … sftp`.
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
- Keep `docs/architecture.md` and `docs/roadmap.md` in sync with the code.
- Add user-visible changes to `CHANGELOG.md` under `Unreleased`.
