set shell := ["bash", "-euo", "pipefail", "-c"]

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
