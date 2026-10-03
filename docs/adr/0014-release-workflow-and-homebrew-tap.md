# 0014. Releases built by GitHub Actions, a Homebrew tap

- Status: Accepted
- Date: 2026-10-03

## Context

Noon Commander could only be built from source. Users of macOS expect `brew install`, and a
release needs binaries for both Apple Silicon and Intel Macs. Linux packages (deb, AUR) are
planned, but not yet.

A binary is only as trustworthy as the way it was made. A release built on a maintainer's
machine depends on that machine; one built in CI depends on the workflow and its tokens. The
maintainer signs commits and tags with an SSH key, which must never leave their machine.

[ADR 0013](0013-just-task-runner.md) has CI run the recipes of the `justfile`, so that CI and
a contributor run the same checks. A release is not a check: nobody builds one locally.

## Decision

- A signed tag `vX.Y.Z` starts `.github/workflows/release.yml`. Releases are built there
  only; its steps are written out in the workflow and are not recipes of the `justfile`, the
  exception to ADR 0013.
- Before anything is published, the workflow runs every check of `ci.yml` on the tag (as a
  reusable workflow), checks that the tag matches `version` in `Cargo.toml`, and that it is an
  annotated tag whose signature GitHub verifies. The release notes are the section of the
  version in `CHANGELOG.md`; without one, the release stops.
- The release starts as a draft. Each target is built on a GitHub-hosted macOS runner,
  `aarch64-apple-darwin` and `x86_64-apple-darwin` (cross-compiled), without rust-cache, so
  that a release binary never comes from a cache another run wrote. The draft is published
  only when the checks and every build passed.
- Packages are named after the project, `noon-commander`. A tarball,
  `noon-commander-X.Y.Z-<target>.tar.gz`, holds `noc`, `LICENSE`, `CHANGELOG.md`, and a
  `README.md` of its own for users (`packaging/tarball/README.md`), since the repository's one
  is written for contributors. Its SHA-256 lies next to it.
- Each tarball gets a build provenance attestation (`actions/attest`, SLSA provenance signed
  through Sigstore with the workflow's OIDC identity, no keys to keep). Before the draft is
  published, the workflow downloads the tarballs and verifies their attestations against this
  workflow and the tag. Users check them with `gh attestation verify`.
- The Homebrew tap is a repository of its own, `noon-commander/homebrew-tap`, so users run
  `brew install noon-commander/tap/noon-commander`. Its formula is rendered from
  `packaging/homebrew/noon-commander.rb.in`, which holds `@VERSION@`, `@REPOSITORY@`, and one
  `@SHA256_…@` per architecture.
- A GitHub App, installed on the tap alone with Contents read and write, commits the formula.
  Its client ID and private key live in the `release` environment, which only `v*` tags may
  use; the token it gets lasts an hour and is revoked when the job ends. The commit goes
  through the contents API without an author or committer, so GitHub signs it for the app and
  shows it as verified; no signing key reaches CI. A personal access token was rejected: it is
  tied to a person and expires.

## Consequences

- A release is: set `version` in `Cargo.toml`, move `Unreleased` in `CHANGELOG.md` under the
  version, commit, `git tag -s vX.Y.Z`, push the tag.
- The tap and its `main` branch can require signed commits: the maintainer's are signed with
  SSH, the app's by GitHub.
- The binaries are not signed or notarized by Apple. A formula leaves no quarantine attribute,
  so Gatekeeper lets `noc` run; a tarball downloaded with a browser gets one.
- Without a cache, every release compiles everything, a few minutes per target.
- Attestations need a public repository on GitHub's free plans.
- The App, the `release` environment, and the tap are set up by hand, as
  `packaging/README.md` describes.
- Linux binaries and packages are a later change to the same workflow.
