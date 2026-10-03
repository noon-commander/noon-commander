set shell := ["bash", "-euo", "pipefail", "-c"]

logo_svg := "assets/icons/logo.svg"
msrv := `sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml`

# Show available recipes
default:
    @just --list

# Format the code
fmt:
    cargo fmt --all

# Fail if the code is not formatted, without changing it
fmt-check:
    cargo fmt --all --check

# Clippy with and without the forwarding feature
clippy:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --features forwarding -- -D warnings

# Tests with and without the forwarding feature
test:
    cargo test --workspace
    cargo test --workspace --features forwarding

# License, advisory, and ban checks (needs cargo-deny)
deny:
    cargo deny check

# Run the tests and review the changed UI snapshots (needs cargo-insta)
snap:
    cargo insta test --workspace --review

# Fail if a snapshot file has no test left (needs cargo-insta)
snap-stale:
    cargo insta test --workspace --unreferenced=reject

# Lint the Markdown files with .markdownlint.yaml (needs markdownlint-cli2)
md:
    markdownlint-cli2 "**/*.md" "#target"

# Find misspelled words in code and docs, with typos.toml (needs typos)
typos:
    typos

# Find dependencies no crate uses, in the crates and the workspace (needs cargo-shear)
unused:
    cargo shear

# Check the TOML files: valid, and formatted as taplo.toml says (needs taplo)
toml:
    RUST_LOG=warn taplo lint
    RUST_LOG=warn taplo fmt --check

# Format the TOML files, keeping their comments (needs taplo)
toml-fmt:
    RUST_LOG=warn taplo fmt

# Lint the shell scripts, found by their shebang (needs shellcheck)
sh:
    #!/usr/bin/env bash
    set -euo pipefail
    scripts=()
    while IFS= read -r -d '' file; do
        IFS= read -r line < "$file" || true
        [[ $line =~ ^\#!.*[/[:space:]](sh|bash|dash)$ ]] && scripts+=("$file")
    done < <(git ls-files -z)
    shellcheck "${scripts[@]}"

# Build with the oldest supported Rust, as CI does (needs rustup)
msrv:
    @command -v rustup >/dev/null || { echo "just msrv needs rustup: https://rustup.rs" >&2; exit 1; }
    rustup run {{msrv}} cargo check --workspace --all-targets --all-features --locked

# Everything that must pass before work is done; changes no files
check: fmt-check clippy test deny

# Render every predefined PNG from the logo (needs resvg)
logo: (_logo-png "github" "512")

_logo-png name size:
    resvg --width {{size}} --height {{size}} {{logo_svg}} assets/icons/logo-{{name}}.png
