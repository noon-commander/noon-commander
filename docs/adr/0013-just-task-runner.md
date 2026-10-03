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
  `deny`, `check` (all of them, with `cargo fmt --check`, so it changes no files), `msrv`, `md`
  (markdownlint-cli2 with `.markdownlint.yaml`), `sh` (ShellCheck on every tracked script with a
  `sh`, `bash`, or `dash` shebang), `snap` and `snap-stale` (cargo-insta: review the changed
  UI snapshots, find the ones no test uses), `typos` (spelling, with exceptions in
  `typos.toml`), `unused` (cargo-shear: dependencies no crate uses, also in
  `[workspace.dependencies]`), `toml` and `toml-fmt` (taplo), and `logo`. `msrv` is not part of
  `check`: it needs rustup and the oldest supported toolchain, which a Homebrew Rust lacks; CI
  checks the MSRV either way. `make` brings tabs, `.PHONY`, and an old GNU make on macOS;
  `cargo-make` is heavy for a handful of commands; an `xtask` crate would wrap each cargo command
  in Rust code.
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
- CI keeps its own jobs and does not use `just`; nothing checks that the PNGs match the SVG, so
  `just logo` is run by hand after the SVG changes.
- The resvg version is not pinned, so a newer resvg may render slightly different pixels.
