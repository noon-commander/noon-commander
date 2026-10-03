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

# Clippy with and without the forwarding feature; extra arguments go to cargo, as --locked in CI
clippy *args: (clippy-with "" args) (clippy-with "forwarding" args)

# Clippy with the given features ("" for none)
clippy-with features *args:
    cargo clippy --workspace --all-targets {{ args }} {{ if features == "" { "" } else { "--features " + features } }} -- -D warnings

# Tests with and without the forwarding feature; extra arguments go to cargo
test *args: (test-with "" args) (test-with "forwarding" args)

# Tests with the given features ("" for none)
test-with features *args:
    cargo test --workspace {{ args }} {{ if features == "" { "" } else { "--features " + features } }}

# Build the workspace for development; extra arguments go to cargo
build *args:
    cargo build --workspace {{ args }}

# Build the optimized noc binary into target/release/noc; extra arguments go to cargo
release *args:
    cargo build --release --locked -p noc {{ args }}

# Commit the version bump, sign a tag for it, and push both, which starts release.yml (ADR 0014)
[confirm("Commit, tag, and push this release?")]
release-tag version:
    #!/usr/bin/env bash
    set -euo pipefail
    version={{ quote(version) }}
    cargo_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
    if [[ $version != "$cargo_version" ]]; then
        echo "Cargo.toml has version $cargo_version, not $version" >&2
        exit 1
    fi
    if ! grep -qF "## [$version] - " CHANGELOG.md; then
        echo "CHANGELOG.md has no section for $version" >&2
        exit 1
    fi
    if [[ $(git branch --show-current) != main ]]; then
        echo "Releases are tagged on main" >&2
        exit 1
    fi
    git commit -m "chore(release): bump version to $version" -- Cargo.toml Cargo.lock CHANGELOG.md
    git tag -s "v$version" -m "Noon Commander $version"
    git push --atomic origin HEAD "v$version"

# Run noc from the sources; arguments go to noc
run *args:
    cargo run -p noc -- {{ args }}

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

# Show newer dependency versions for Cargo.toml and Cargo.lock; changes no files (needs cargo-edit)
outdated:
    cargo upgrade --dry-run --incompatible allow
    cargo update --dry-run --verbose

# Lint the GitHub Actions workflows and audit their security (needs actionlint and zizmor)
gha:
    actionlint
    zizmor --offline --persona=pedantic --quiet .github/

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

# The fast checks of everything but the Rust code; changes no files
lint: md sh typos toml gha unused

# check, lint, and stale snapshots; changes no files
all: check lint snap-stale

# Render every predefined PNG from the logo (needs resvg)
logo: (_logo-png "github" "512")

_logo-png name size:
    resvg --width {{size}} --height {{size}} {{logo_svg}} assets/icons/logo-{{name}}.png
