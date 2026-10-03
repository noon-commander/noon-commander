# Noon Commander

A Rust-based terminal file manager for macOS and Linux, focused on seamless local and SFTP file
operations. The command is `noc`.

- Project: <https://github.com/noon-commander/noon-commander>
- Changes in this version: [CHANGELOG.md](CHANGELOG.md)
- License: GPL-3.0-or-later, see [LICENSE](LICENSE)

## Requirements

- macOS on Apple Silicon or Intel, matching the name of this archive.
- OpenSSH 8.7 or newer, which macOS has: check with `ssh -V`.
- A [Nerd Font](https://www.nerdfonts.com) in the terminal for the icons. Without one, turn
  them off in Options → Configuration (F9), or set `icons = false` under `[ui]`.

## Installing

Homebrew installs and updates `noc` for you:

```sh
brew install noon-commander/tap/noon-commander
```

To install from this archive instead, put `noc` in a directory on your `PATH`, for example:

```sh
mkdir -p ~/.local/bin
install -m 755 noc ~/.local/bin/noc
```

The binary is not notarized. If the archive was downloaded with a browser, macOS refuses to
run it; remove the quarantine mark first:

```sh
xattr -d com.apple.quarantine noc
```

## Checking the archive

Each archive is built by GitHub Actions from a signed tag and has a build provenance
attestation. With the [GitHub CLI](https://cli.github.com):

```sh
gh attestation verify noon-commander-<version>-<target>.tar.gz \
  --repo noon-commander/noon-commander
```
