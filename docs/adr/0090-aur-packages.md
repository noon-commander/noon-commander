# 0090. AUR packages, updated by the release workflow

- Status: Accepted
- Date: 2026-11-16
- Amends: [ADR 0014](0014-release-workflow-and-homebrew-tap.md)

## Context

Users of Arch Linux install what the official repositories lack from the AUR, which holds a
`PKGBUILD` and its `.SRCINFO` per package in a git repository. Its rules name three kinds: a
package built from the sources of a version has no suffix, one that installs a prebuilt binary
ends in `-bin`, and one built from the latest commit ends in `-git`. A `-git` package must not be
pushed only to raise its version. Packages that break the rules are deleted without warning, and
an account whose automation misbehaves may lose its packages.

The AUR takes pushes over SSH. A key belongs to an account and can push to every package of it,
and the account needs a verified email. Names cannot be reserved: a package exists once its first
commit is pushed. Commits stay public with their author. `makepkg`, which builds a package and
writes `.SRCINFO`, runs on Arch Linux only, and never as root.

Releases have static Linux binaries since [ADR 0089](0089-static-linux-binaries-with-musl.md).

## Decision

- Three packages: `noon-commander` builds the source archive GitHub makes of the tag and runs the
  tests in `check()`; `noon-commander-bin` installs the static `noc` of the release tarball for
  `x86_64` or `aarch64`; `noon-commander-git` builds `main`, versioned by `git describe`. The
  last two provide `noon-commander` and conflict with it. All depend on `openssh`, the two that
  compile also on `glibc` and `libgcc`; `fd`, `ripgrep`, `xdg-utils`, and `zoxide` are optional.
  The two that compile turn off `debug` (the release profile strips `noc`) and makepkg's `lto`,
  which can break the link of BLAKE3's C code; Rust's LTO comes from `Cargo.toml`.
- Their templates are `packaging/aur/<package>/PKGBUILD.in`, with `@VERSION@`, `@REPOSITORY@`,
  and `@SHA256_…@`. The `publish` job gives the checksums: those of the Linux tarballs once their
  attestations are verified, and that of the source archive at
  `https://github.com/<repository>/archive/refs/tags/vX.Y.Z.tar.gz`.
- `packaging/aur/update.sh` renders a template, clones the package's AUR repository over HTTPS,
  builds the package with `makepkg`, tests included, and commits `PKGBUILD`, `.SRCINFO`, and
  `LICENSE` as `0ldkettle <packages@noon-commander.app>`, the Maintainer line of the templates.
  `LICENSE` is the 0BSD license that Arch gives the sources of its packages and asks of the
  AUR's (RFC 0040), copied byte for byte from devtools, `Copyright Arch Linux Contributors`
  included, so that a check of its text, which the RFC plans, finds it. A `PKGBUILD` that differs
  from the AUR's in nothing but `pkgver` is neither built nor committed: `-git` is pushed only
  when its build changes, and a release run again changes nothing. The script pushes nothing, so
  it also checks a template wherever `makepkg` runs.
- The job `aur` of `release.yml` runs after `publish`, one per package, in parallel, in an
  `archlinux:base-devel` container pinned by digest and raised by hand, which `pacman -Syu`
  brings up to date first, without the sandbox of its downloads, which a container does not
  always allow; the signatures of the packages are still checked. `update.sh` runs as a user of
  its own. The key reaches only the step
  that pushes: `AUR_SSH_PRIVATE_KEY` in the `release` environment, which only `v*` tags may use.
  ssh checks the AUR's host keys against `packaging/aur/known_hosts`, taken from the AUR and
  compared with the fingerprints it publishes; checking is never turned off.
- The key is one of its own, added to the maintainer's AUR account, which owns the packages, so
  that it can be revoked alone.

## Consequences

- The packages appear in the AUR with the first release after this change.
- A package that fails to build or test on Arch is not updated, and its job fails; the release is
  already published by then. The job can be run again.
- The key can push to every package of the account, not only these three.
- `makepkg` verifies git tags only with PGP, and the tags are signed with SSH: `noon-commander`
  relies on the checksum of the archive. Built from an archive, its `noc debug version` has no
  commit; that of `-git` has one.
- CI builds the packages for `x86_64` only; `aarch64` is not tried.
- Each release compiles `noc` twice more, with the tests, in jobs of their own.
- A fix to a `PKGBUILD` between releases is pushed by hand with a higher `pkgrel`; the templates
  keep `pkgrel=1` for the next version.
