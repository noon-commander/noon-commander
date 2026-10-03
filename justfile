set shell := ["bash", "-euo", "pipefail", "-c"]

logo_svg := "assets/icons/logo.svg"

# Show available recipes
default:
    @just --list

# Format the code
fmt:
    cargo fmt --all

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

# Everything that must pass before work is done
check: fmt clippy test deny

# Render every predefined PNG from the logo (needs resvg)
logo: (_logo-png "github" "512")

_logo-png name size:
    resvg --width {{size}} --height {{size}} {{logo_svg}} assets/icons/logo-{{name}}.png
