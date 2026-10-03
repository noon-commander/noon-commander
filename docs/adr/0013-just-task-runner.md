# 0013. just as the task runner, resvg for logo PNGs

- Status: Accepted
- Date: 2026-10-03

## Context

The checks that must pass before work is done were a list of six cargo commands in
`AGENTS.md`, two of them only differing in `--features forwarding`. A new task joined them:
the logo exists only as `assets/icons/logo.svg`, and places such as a GitHub avatar want a PNG
of a given size.

A task runner gives these commands one home and short names. The candidates were `make`,
`cargo-make`, an `xtask` crate in the workspace, and `just`. Rendering the logo needs an SVG
renderer: a library in an `xtask` crate, or a command-line program.

## Decision

- `just` runs the project's tasks from a `justfile` at the root: `fmt`, `clippy`, `test`,
  `deny`, `check` (all of them, with `cargo fmt --check`, so it changes no files), `lint` (the
  checks below of everything but the Rust code), `all` (`check`, `lint`, and `snap-stale`),
  `msrv`, `md`
  (markdownlint-cli2 with `.markdownlint.yaml`), `sh` (ShellCheck on every tracked script with a
  `sh`, `bash`, or `dash` shebang), `snap` and `snap-stale` (cargo-insta: review the changed
  UI snapshots, find the ones no test uses), `typos` (spelling, with exceptions in
  `typos.toml`), `unused` (cargo-shear: dependencies no crate uses, also in
  `[workspace.dependencies]`), `toml` and `toml-fmt` (taplo), `gha` (actionlint and
  zizmor on the workflows), `outdated` (`cargo upgrade --dry-run` from cargo-edit for `Cargo.toml`,
  `cargo update --dry-run` for `Cargo.lock`), and `logo`. `msrv` is not part of `check`: it
  needs rustup and the oldest supported toolchain, which a Homebrew Rust lacks; CI runs it in a
  job of its own. `make` brings tabs, `.PHONY`, and an old GNU make on macOS; `cargo-make` is
  heavy for a handful of commands; an `xtask` crate would wrap each cargo command in Rust code.
- The workflows pin every action to a commit hash, with its version in a comment, and check out
  without keeping the token (`persist-credentials: false`): a moved tag cannot run new code in
  CI. Dependabot (`.github/dependabot.yml`) proposes new hashes weekly, a week after a release.
  Rust is installed with the runner's own rustup rather than an action, and matrix values reach
  commands through `env:`, never as `${{ }}` inside `run:`. `just gha` runs zizmor as
  `pedantic`, so these code smells fail it too.
- CI runs the recipes of the `justfile`, so it checks what `just check` and `just lint` check
  locally. Its jobs stay apart to run in parallel: rustfmt, clippy and tests per feature set
  (`clippy-with` and `test-with`, with `--locked`), MSRV, cargo-deny, and lint. The tools are
  built with `cargo install` at versions pinned in `ci.yml`, and rust-cache keeps them;
  actionlint comes from `go install`, and markdownlint-cli2 from `npm ci` with its own
  `package-lock.json` in `.github/markdownlint/`, which Dependabot updates. ShellCheck comes
  with the runner. cargo-deny-action is gone.
- The logo is rendered by the `resvg` program. The workspace gets no new crate and no new
  dependencies, so `deny.toml` and the MSRV are untouched.
- The PNGs are defined in the `justfile`, each with a name and a size, and `just logo` renders
  all of them to `assets/icons/logo-<name>.png`. There is no size argument: every PNG in the
  repository comes from the list. The first is `github`, 512 × 512, for the GitHub avatar.
- resvg writes only the `IHDR`, `IDAT`, and `IEND` chunks: no author, copyright, time, or
  other metadata reaches the PNG.

## Consequences

- Contributors install `just` and `resvg` (and `cargo-deny`) to use the tasks; the plain cargo
  commands still work without them.
- A check added to the `justfile` and called from `ci.yml` runs the same in both places; a
  command written into `ci.yml` by hand would drift from the local one.
- The first run of each CI job builds its tools, a few minutes; later runs take them from the
  cache. The tool versions in `ci.yml` are raised by hand; `just outdated` does not see them.
- Nothing checks that the PNGs match the SVG, so `just logo` is run by hand after the SVG
  changes.
- A new action is added pinned to a hash, or `just gha` fails.
- The resvg version is not pinned, so a newer resvg may render slightly different pixels.
