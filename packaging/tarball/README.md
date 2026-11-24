# Noon Commander

A Rust-based terminal file manager for macOS and Linux, focused on seamless local and SFTP file operations. The command is `noc`.

- Project: <https://github.com/noon-commander/noon-commander>
- Changes in this version: [CHANGELOG.md](CHANGELOG.md)
- License: GPL-3.0-or-later, see [LICENSE](LICENSE)

## Requirements

- The system and processor in the name of this archive: macOS on Apple Silicon (`aarch64-apple-darwin`) or Intel (`x86_64-apple-darwin`), or Linux on x86-64 (`x86_64-unknown-linux-musl`) or 64-bit ARM (`aarch64-unknown-linux-musl`). The Linux `noc` is static: it needs no libraries and runs on any distribution.
- OpenSSH 8.7 or newer: check with `ssh -V`. macOS has it; on Linux it comes with the distribution's `openssh` or `openssh-client` package.
- A [Nerd Font](https://www.nerdfonts.com) in the terminal for the icons. Without one, turn them off in Tools → Configuration (F9), or set `icons = false` under `[ui]`.

## Installing

On macOS, Homebrew installs and updates `noc` for you:

```sh
brew install noon-commander/tap/noon-commander
```

To install from this archive instead, put `noc` in a directory on your `PATH`, for example:

```sh
mkdir -p ~/.local/bin
install -m 755 noc ~/.local/bin/noc
```

The macOS binary is not notarized. If the archive was downloaded with a browser, macOS refuses to run it; remove the quarantine mark first:

```sh
xattr -d com.apple.quarantine noc
```

## Checking the archive

Each archive is built by GitHub Actions from a signed tag and has a build provenance attestation. With the [GitHub CLI](https://cli.github.com):

```sh
gh attestation verify noon-commander-<version>-<target>.tar.gz \
  --repo noon-commander/noon-commander
```
