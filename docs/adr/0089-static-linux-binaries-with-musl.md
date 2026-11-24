# 0089. Static Linux binaries, linked with musl

- Status: Accepted
- Date: 2026-11-16
- Amends: [ADR 0014](0014-release-workflow-and-homebrew-tap.md)

## Context

Releases had binaries for macOS alone. Linux is a target of Noon Commander, CI builds and tests it there, and the packages planned for it, AUR and deb, want a binary of each release to install. A Linux binary linked with glibc runs only where the glibc is as new as the one it was built with: one from `ubuntu-latest` would refuse older distributions, and one built on an old system would depend on that system staying on the runners.

A binary linked statically with musl needs no library at run time. What it loses is what musl does not do as glibc does: NSS, through which glibc asks LDAP or systemd for users and hosts, and a fast allocator. Noon Commander shows owners as numbers and resolves no host itself, since `ssh` does; it finds the home directory in `$HOME`, and musl reads `/etc/passwd` without it. Its work waits on disks and the network more than on memory.

The only C in the dependencies is BLAKE3's code for SIMD, in C and assembly, which `cc` compiles for the target. GitHub has hosted runners for both x86-64 and 64-bit ARM Linux.

## Decision

- Each release has two Linux tarballs, `noon-commander-X.Y.Z-x86_64-unknown-linux-musl.tar.gz` and `noon-commander-X.Y.Z-aarch64-unknown-linux-musl.tar.gz`, with the same files, attestation, and checksum as those of macOS. The tarball's `README.md` covers both systems.
- `noc` for Linux is linked statically with musl. `release.yml` builds each on a GitHub-hosted runner of its architecture, `ubuntu-latest` and `ubuntu-24.04-arm`, with no cross compiler; `musl-tools` gives BLAKE3 the C compiler for musl. A step fails the build if the binary asks for a dynamic loader (an `INTERP` header).
- The tarballs are packed by GNU tar with files owned by 0:0.
- `publish` verifies the attestations of all four tarballs before it publishes the release.
- The release build of CI on Linux builds `x86_64-unknown-linux-musl`, as `release.yml` does, so that a change which breaks the musl build fails before a tag.

## Consequences

- One Linux binary runs on any distribution of its architecture, old or new, and the packages for Linux can install it as it is.
- A user who is not in `/etc/passwd`, as one from LDAP, needs `$HOME` set, as a login sets it, for `noc` to find the home directory; nothing else of `noc` asks for users or hosts.
- If musl's allocator shows in a profile, an allocator crate can replace it; that is a new dependency, so it needs `deny.toml` and the MSRV checked.
- Each release builds two more targets, a few minutes each, on runners that run in parallel.
