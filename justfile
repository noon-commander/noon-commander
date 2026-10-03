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
