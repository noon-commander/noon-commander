---
description: Prepare a release X.Y.Z: review and condense the changelog, set the version
---

# Prepare release $1

Prepare the release of version `$1` of Noon Commander, up to the point where the maintainer runs
`just release-tag $1` (see `packaging/README.md` and ADR 0014). Do not commit, tag, or push.

Today: !`date +%Y-%m-%d`

Last tag: !`git describe --tags --abbrev=0`

Version in `Cargo.toml`: !`sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1`

`git status --short`:

!`git status --short`

## 1. Check the input

- `$1` must be a version `X.Y.Z`, greater than the version in `Cargo.toml` and the last tag.
  If it is empty or not, stop and ask.
- `CHANGELOG.md` must have a non-empty `## [Unreleased]` section and no `## [$1]` section yet.
- If the working tree has uncommitted changes, list them and leave them alone; never stage or
  revert them.

## 2. Check the `Unreleased` section against the last release

The changelog lists what changed since the last tag, not the history of the work in between.
Compare with the last release through `git show <tag>:<path>` (the preset of the keymap, the
config schema in `crates/noc-config/src/config.rs`, `Cargo.toml`, the files of `crates/noc/src/tui/`)
and `git log <tag>..HEAD`:

- Drop entries under Changed or Removed about features that the last release did not have: a
  change to something added in this release is part of its Added entry, or nothing.
- Move an entry to the right heading: new to the user is Added, different from the last release
  is Changed, gone is Removed.
- Check the keys an entry names against `crates/noc/src/tui/keymap/preset.rs`, and the settings
  against `crates/noc-config/src/config.rs`. Fix what is wrong; report what you cannot verify.
- Add what `git log <tag>..HEAD` shows as user-visible but the changelog leaves out, briefly.

## 3. Condense it

Rewrite the section so that a user reads it in a minute:

- One entry per feature, one to three lines; at most one level of sub-bullets, for a large
  feature such as a keymap. No details that the help, the docs, or an ADR already give: link
  the ADR instead.
- Name keys and settings, not every key: a few examples and "the help lists them all".
- Settings new in the release go in one Added entry: "Settings, all in Options →
  Configuration: …".
- Breaking changes stay in Removed or Changed with what the user has to do (for example, a
  config key that is now an error).
- Keep the project's style: English, keys as `keymap::describe` writes them (`Ctrl+w h`, `G`,
  `Alt+W`, `Shift+F6`, `PgUp`), bare-character keys in backticks (`g g`, `d d`), lines of at
  most 100 characters, the headings Added, Changed, Removed (and Fixed, Deprecated, Security
  when needed) in Keep a Changelog's order.

## 4. Set the version

- In `CHANGELOG.md`, rename `## [Unreleased]` to `## [$1] - <today>`, with today's date from
  above, and put a new, empty `## [Unreleased]` above it.
- Set `version` in `[workspace.package]` of the root `Cargo.toml` to `$1`; the crates take it
  with `version.workspace = true`.
- Update `Cargo.lock` with `cargo update --workspace`; it must change only the versions of the
  `noc-*` crates and `noc`. Check with `git diff Cargo.lock`.
- Look for other places that name the old version (`rg -F '<old version>'` outside `target/`
  and `CHANGELOG.md`) and report them; change them only if they must follow the release.

## 5. Verify

Run `just md`, `just typos`, `just toml`, and `cargo check --workspace --locked`. Fix what they
report in the files you changed.

## 6. Report

In the user's language:

- What you dropped, moved, or fixed in the changelog and why (one line each), and the number of
  lines before and after.
- The files changed (`git status --short`) and the results of the checks.
- The next step: review `git diff`, then `just release-tag $1`, which commits `Cargo.toml`,
  `Cargo.lock`, and `CHANGELOG.md`, signs the tag `v$1`, and pushes.
