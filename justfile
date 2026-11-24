set shell := ["bash", "-ceuo", "pipefail"]
set ignore-comments

# >>> custom vars
logo_svg := "assets/icons/logo.svg"
msrv := `sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml`
# <<< custom vars

# List available recipes.
default:
    @just --list --unsorted

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

# Lint the code and check its formatting without changing files.
lint:
    markdownlint-cli2
    RUST_LOG=warn taplo lint
    RUST_LOG=warn taplo fmt --check
    {{ shell(_shell_scripts, 'shellcheck') }}
    {{ shell(_shell_scripts, 'shfmt --diff') }}
    actionlint
    zizmor --offline --pedantic --quiet --strict-collection .
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo shear --locked
    cargo fmt --all --check
    # >>> custom lint
    scripts/glyphs.sh
    # <<< custom lint

# Fix what lint finds, where a safe automatic fix exists.
fix:
    RUST_LOG=warn taplo fmt
    {{ shell(_shell_scripts, 'shfmt --write') }}
    zizmor --offline --pedantic --quiet --no-exit-codes --fix .
    cargo fmt --all

# Run the tests.
test:
    cargo test --workspace --locked

# Everything that must pass before a commit.
ci: lint test deny

# Build the debug binaries.
[positional-arguments]
build *args:
    cargo build --workspace --locked "${@}"

# Build the release binaries.
[positional-arguments]
build-release *args:
    cargo build --workspace --locked --release "${@}"

# Run the binary, passing the arguments to it.
[positional-arguments]
run *args:
    cargo run --locked -- "${@}"

# Show outdated dependencies without changing files.
outdated:
    cargo upgrade --dry-run --incompatible allow
    cargo update --dry-run --verbose

# Upgrade the dependencies to the latest versions.
upgrade:
    cargo upgrade --incompatible allow
    cargo update

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
# Commit the version bump, sign a tag for it, and push both, which starts release.yml (ADR 0014)
[confirm("Commit, tag, and push this release?")]
push-version version:
    packaging/push-version.sh {{ quote(version) }}

# Print the release notes of a version, as release.yml takes them from CHANGELOG.md
release-notes version:
    @packaging/release-notes.sh {{ quote(version) }}

# License, advisory, and ban checks (needs cargo-deny)
deny:
    cargo deny check

# Run the tests and review the changed UI snapshots (needs cargo-insta)
snap:
    cargo insta test --workspace --review

# Fail if a snapshot file has no test left (needs cargo-insta)
snap-stale:
    cargo insta test --workspace --unreferenced=reject

# Build with the oldest supported Rust, as CI does (needs rustup)
msrv:
    @command -v rustup >/dev/null || { echo "just msrv needs rustup: https://rustup.rs" >&2; exit 1; }
    rustup run {{msrv}} cargo check --workspace --all-targets --all-features --locked

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
