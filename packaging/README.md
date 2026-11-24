# Packaging

Templates of the packages that [`release.yml`](../.github/workflows/release.yml) builds from a signed tag ([ADR 0014](../docs/adr/0014-release-workflow-and-homebrew-tap.md)). Packages are named `noon-commander`; `noc` is the binary inside them.

| Path | What it is |
| ---- | ---------- |
| `homebrew/noon-commander.rb.in` | The Homebrew formula, rendered into `Formula/noon-commander.rb` of the tap |
| `aur/<package>/PKGBUILD.in` | The `PKGBUILD` of each AUR package: `noon-commander`, `-bin`, and `-git` ([ADR 0090](../docs/adr/0090-aur-packages.md)) |
| `aur/update.sh` | Renders a `PKGBUILD`, builds it with `makepkg`, and commits it to a clone of its AUR repository |
| `aur/LICENSE` | The 0BSD license of the `PKGBUILD`s, which goes into each AUR repository: Arch's own, from devtools, byte for byte |
| `aur/known_hosts` | The host keys of `aur.archlinux.org`, which the push checks |
| `tarball/README.md` | The README of the release tarball, for users rather than contributors |
| `release-notes.sh` | Prints a version's section of `CHANGELOG.md` as the notes of its release |
| `push-version.sh` | Commits the version bump, signs its tag, and pushes both: `just push-version` |

## Making a release

1. Set `version` in the root `Cargo.toml` and run `just build`, which brings `Cargo.lock` to it: the release builds with `--locked`.
2. In `CHANGELOG.md`, rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD` and start a new, empty `## [Unreleased]` above it. `just release-notes X.Y.Z` prints the notes the release will get: the section with its wrapped lines joined, since GitHub breaks lines where they do.
3. Commit, tag, and push, after a question:

   ```sh
   just push-version X.Y.Z
   ```

   It checks the version in `Cargo.toml`, the section in `CHANGELOG.md`, and that the branch is `main`, then commits `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md` as `chore(release): bump version to X.Y.Z`, signs the tag `vX.Y.Z`, and pushes the branch and the tag together. The tag's message holds the release notes: until the draft is published, the releases feed shows the tag with its message, and feed readers keep that entry.

The tag must be annotated and signed with a key added to GitHub as a signing key, or the workflow stops before it builds anything.

## One-time setup

1. **The tap.** A repository `noon-commander/homebrew-tap` with a `Formula/` directory. A ruleset on `main`: require signed commits, block force pushes, restrict deletions.
2. **The GitHub App** (organization settings → Developer settings → GitHub Apps):
   - Homepage URL: the project's repository. No callback URL, no OAuth during installation, no device flow, webhook off.
   - Repository permissions: Contents, read and write; nothing else.
   - Installable only on this account; install it on `homebrew-tap` alone.
   - Generate a private key.

3. **The `release` environment** in this repository (Settings → Environments), limited to tags matching `v*`:
   - variable `TAP_APP_CLIENT_ID`: the app's client ID;
   - secret `TAP_APP_PRIVATE_KEY`: the whole `.pem` file, which can then be deleted.

4. **The AUR key.** An AUR account with a verified email; the packages are created by the first release that pushes them. A key of its own for the workflow, added to the account (My Account → SSH Public Key), whose private half goes into the `release` environment and nowhere else:

   ```sh
   dir=$(mktemp -d)
   ssh-keygen -t ed25519 -N '' -C 'noon-commander release.yml' -f "$dir/aur"
   cat "$dir/aur.pub"   # into the AUR account
   gh secret set AUR_SSH_PRIVATE_KEY --env release < "$dir/aur"
   rm -rf "$dir"
   ```

   `aur/known_hosts` holds the AUR's host keys; if they change, take them with `ssh-keyscan aur.archlinux.org` and compare `ssh-keygen -lf` with the fingerprints on <https://aur.archlinux.org>.

5. **Tags** (optional): a ruleset on `v*` tags that lets only maintainers create, update, or delete them.

## Checking an AUR package

`aur/update.sh` builds a package as the workflow does and commits it to a new clone, without pushing, on Arch Linux and as a user other than root. For the package built from a version:

```sh
url=https://github.com/noon-commander/noon-commander/archive/refs/tags/vX.Y.Z.tar.gz
VERSION=X.Y.Z REPOSITORY=noon-commander/noon-commander \
  SHA256_SOURCE=$(curl -fsSL "$url" | sha256sum | cut -d ' ' -f 1) \
  packaging/aur/update.sh noon-commander /tmp/aur-noon-commander
```

`noon-commander-bin` takes `SHA256_X86_64` and `SHA256_AARCH64`, the checksums of the two Linux tarballs, in place of `SHA256_SOURCE`; `noon-commander-git` takes `REPOSITORY` alone. The directory of the clone must not exist yet.

## Checking a download

Each tarball has a build provenance attestation, which this checks as `release.yml` does:

```sh
gh attestation verify noon-commander-X.Y.Z-aarch64-apple-darwin.tar.gz \
  --repo noon-commander/noon-commander --source-ref refs/tags/vX.Y.Z \
  --signer-workflow noon-commander/noon-commander/.github/workflows/release.yml
```

Its SHA-256 is next to it in the release, for `shasum -a 256 -c` (`sha256sum -c` on Linux):

```sh
shasum -a 256 -c noon-commander-X.Y.Z-aarch64-apple-darwin.tar.gz.sha256
```
