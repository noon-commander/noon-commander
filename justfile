set shell := ["bash", "-ceuo", "pipefail"]
set ignore-comments

# >>> custom vars
logo_svg := "assets/icons/logo.svg"
msrv := `sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml`
# <<< custom vars

# List available recipes.
default:
    @just --list --unsorted

# Format the code.
format:
    RUST_LOG=warn taplo fmt
    {{ shell(_shell_scripts, 'shfmt --write') }}
    cargo fmt --all

# Check formatting without changing files.
format-check:
    RUST_LOG=warn taplo fmt --check
    {{ shell(_shell_scripts, 'shfmt --diff') }}
    cargo fmt --all --check

_shell_scripts := '''
    shebang='^#!.*[/[:space:]](sh|bash|dash|ksh)([[:space:]]|$)'
    scripts=()
    while IFS= read -r -d '' file; do
        [[ -f $file ]] || continue
        IFS= read -r line < "$file" || true
        [[ $line =~ $shebang ]] && scripts+=("$file")
    done < <(git ls-files -z)
    if [[ -n ${scripts[*]-} ]]; then
        printf '%s' "$1"
        printf ' %q' "${scripts[@]}"
    fi
'''

# Lint the code.
lint: gha glyphs
    markdownlint-cli2
    RUST_LOG=warn taplo lint
    {{ shell(_shell_scripts, 'shellcheck') }}
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo shear --locked

# Run the tests.
test:
    cargo test --workspace --locked

# Everything that must pass before a commit.
ci: format-check lint test deny

# Build distribution.
dist:
    rm -rf target/dist
    # >>> custom dist
    # <<< custom dist

# Remove caches, build artifacts and generated files.
clean:
    rm -rf target
    find . -type f -name .DS_Store -delete
    # >>> custom clean
    # <<< custom clean

# >>> custom
# Build the workspace for development; extra arguments go to cargo
build *args:
    cargo build --workspace {{ args }}

# Build the optimized noc binary into target/release/noc; extra arguments go to cargo
release *args:
    cargo build --release --locked --package noc {{ args }}

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
    notes=$(packaging/release-notes.sh "$version")
    git commit -m "chore(release): bump version to $version" -- Cargo.toml Cargo.lock CHANGELOG.md
    # Until the draft is published, releases.atom shows the tag with its message, and feed readers
    # keep that entry; whitespace keeps the notes' `#` headings, which strip would drop.
    git tag -s --cleanup=whitespace "v$version" -m "Noon Commander $version" -m "$notes"
    git push --atomic origin HEAD "v$version"

# Print the release notes of a version, as release.yml takes them from CHANGELOG.md
release-notes version:
    @packaging/release-notes.sh {{ quote(version) }}

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

# Show newer dependency versions for Cargo.toml and Cargo.lock; changes no files (needs cargo-edit)
outdated:
    cargo upgrade --dry-run --incompatible allow
    cargo update --dry-run --verbose

# Lint the GitHub Actions workflows and audit their security (needs actionlint and zizmor)
gha:
    actionlint
    zizmor --offline --persona=pedantic --quiet .github/

# Fail if code that draws writes a special character itself, not from crates/noc/src/tui/glyphs.rs
# (needs perl)
glyphs:
    #!/usr/bin/env bash
    set -euo pipefail
    git ls-files -z -- crates/noc crates/noc-viewer crates/noc-scroll crates/noc-text \
        ':!crates/noc/src/tui/glyphs.rs' ':!crates/*/tests/*' \
        | grep -z '\.rs$' \
        | xargs -0 perl -CSD -ne '
            $tests = 1 if /^\s*mod tests\b/ || $ARGV =~ m{/tests\.rs$};
            if (!$tests && !m{^\s*//}
                && /[\x{2190}-\x{2BFF}\x{E000}-\x{F8FF}\x{F0000}-\x{FFFFD}\x{AB}\x{BB}\x{2022}\x{2026}]/) {
                print "$ARGV:$.: $_";
                $found = 1;
            }
            if (eof) { close ARGV; $tests = 0 }
            END {
                if ($found) {
                    print STDERR "Special characters go in crates/noc/src/tui/glyphs.rs\n";
                    $? = 1;
                }
            }'

# Build with the oldest supported Rust, as CI does (needs rustup)
msrv:
    @command -v rustup >/dev/null || { echo "just msrv needs rustup: https://rustup.rs" >&2; exit 1; }
    rustup run {{msrv}} cargo check --workspace --all-targets --all-features --locked

# check, lint, and stale snapshots; changes no files
all: ci lint snap-stale

# Take the screenshots of docs/demo/*.tape with made-up files and hosts (needs vhs, a Nerd Font)
demo:
    cargo build -p noc
    docs/demo/setup.sh
    mkdir -p assets/screenshots
    for tape in docs/demo/*.tape; do vhs "$tape"; done

# Render every predefined PNG from the logo (needs resvg)
logo: (_logo-png "github" "512")

_logo-png name size:
    resvg --width {{size}} --height {{size}} {{logo_svg}} assets/icons/logo-{{name}}.png
# <<< custom
