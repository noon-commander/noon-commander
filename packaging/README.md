# Packaging

Templates of the packages that [`release.yml`](../.github/workflows/release.yml) builds from a
signed tag ([ADR 0014](../docs/adr/0014-release-workflow-and-homebrew-tap.md)). Packages are
named `noon-commander`; `noc` is the binary inside them.

| Path | What it is |
| --- | --- |
| `homebrew/noon-commander.rb.in` | The Homebrew formula, rendered into `Formula/noon-commander.rb` of the tap |
| `tarball/README.md` | The README of the release tarball, for users rather than contributors |

## Making a release

1. Set `version` in the root `Cargo.toml`.
2. In `CHANGELOG.md`, rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD` and start a new,
   empty `## [Unreleased]` above it.
3. Commit, then tag and push:

   ```sh
   git tag -s vX.Y.Z -m "Noon Commander X.Y.Z"
   git push origin vX.Y.Z
   ```

The tag must be annotated and signed with a key added to GitHub as a signing key, or the
workflow stops before it builds anything.

## One-time setup

1. **The tap.** A repository `noon-commander/homebrew-tap` with a `Formula/` directory. A
   ruleset on `main`: require signed commits, block force pushes, restrict deletions.
2. **The GitHub App** (organization settings → Developer settings → GitHub Apps):
   - Homepage URL: the project's repository. No callback URL, no OAuth during installation,
     no device flow, webhook off.
   - Repository permissions: Contents, read and write; nothing else.
   - Installable only on this account; install it on `homebrew-tap` alone.
   - Generate a private key.
3. **The `release` environment** in this repository (Settings → Environments), limited to tags
   matching `v*`:
   - variable `TAP_APP_CLIENT_ID`: the app's client ID;
   - secret `TAP_APP_PRIVATE_KEY`: the whole `.pem` file, which can then be deleted.
4. **Tags** (optional): a ruleset on `v*` tags that lets only maintainers create, update, or
   delete them.

## Checking a download

Each tarball has a build provenance attestation:

```sh
gh attestation verify noon-commander-X.Y.Z-aarch64-apple-darwin.tar.gz \
  --repo noon-commander/noon-commander --source-ref refs/tags/vX.Y.Z
```
