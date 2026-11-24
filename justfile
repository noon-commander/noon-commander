set shell := ["bash", "-ceuo", "pipefail"]
set ignore-comments

# >>> custom vars
logo_svg := "assets/icons/logo.svg"
# <<< custom vars

# List available recipes.
default:
    @just --list --unsorted

_justfiles := '''
    while IFS= read -r -d '' file; do
        [[ -f $file ]] || continue
        printf '%s --justfile %q\n' "$1" "$file"
    done < <(git ls-files -z --cached --others --exclude-standard -- \
        ':(glob,icase)**/justfile' ':(glob,icase)**/.justfile' ':(glob,icase)**/*.just')
'''

_shell_scripts := '''
    shebang='^#!.*[/[:space:]](sh|bash|dash|ksh)([[:space:]]|$)'
    scripts=()
    while IFS= read -r -d '' file; do
        [[ -f $file ]] || continue
        IFS= read -r line < "$file" || true
        [[ $line =~ $shebang ]] && scripts+=("$file")
    done < <(git ls-files -z --cached --others --exclude-standard)
    if [[ -n ${scripts[*]-} ]]; then
        printf '%s' "$1"
        printf ' %q' "${scripts[@]}"
    fi
'''

# Lint the code and check its formatting without changing files.
lint:
    {{ shell(_justfiles, 'just --fmt --check') }}
    rumdl check
    RUST_LOG=warn taplo lint
    RUST_LOG=warn taplo fmt --check
    {{ shell(_shell_scripts, 'shellcheck') }}
    {{ shell(_shell_scripts, 'shfmt --diff') }}
    actionlint
    zizmor --offline --pedantic --quiet --strict-collection .
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo shear --locked
    cargo deny --workspace --locked check -D warnings
    cargo fmt --all --check
    # >>> custom lint
    scripts/glyphs.sh
    # <<< custom lint

# Fix what lint finds, where a safe automatic fix exists.
fix:
    {{ shell(_justfiles, 'just --fmt') }}
    rumdl fmt
    RUST_LOG=warn taplo fmt
    {{ shell(_shell_scripts, 'shfmt --write') }}
    zizmor --offline --pedantic --quiet --no-exit-codes --fix .
    cargo fmt --all
    # >>> custom fix
    # <<< custom fix

# Run the tests.
test:
    cargo test --workspace --locked

# Everything that must pass before a commit.
ci: lint test
    # >>> custom ci
    # <<< custom ci

# Build the debug binaries.
[positional-arguments]
build *args:
    cargo build --workspace --locked "${@}"

# Build the release binaries.
[positional-arguments]
build-release *args:
    cargo build --workspace --locked --release "${@}"

# Check the build with the oldest supported Rust, as CI does.
build-msrv:
    cargo +"$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)" \
        check --workspace --all-targets --all-features --locked

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
    rm -rf .rumdl_cache
    rm -rf target
    find . -type f -name .DS_Store -delete
    # >>> custom clean
    # <<< custom clean

# >>> custom
# Commit the version bump, sign a tag for it, and push both, which starts release.yml.
[confirm("Commit, tag, and push this release?")]
push-version version:
    packaging/push-version.sh {{ quote(version) }}

# Print the release notes of a version, as release.yml takes them from CHANGELOG.md.
release-notes version:
    packaging/release-notes.sh {{ quote(version) }}

# Run the tests and review the changed UI snapshots.
test-snapshots-review:
    cargo insta test --workspace --review

# Fail if a snapshot file has no test left.
test-snapshots-orphans:
    cargo insta test --workspace --check --unreferenced=reject

# Take the screenshots of docs/demo/*.tape with made-up files and hosts.
render-screenshots: build
    docs/demo/setup.sh
    mkdir -p assets/screenshots
    for tape in docs/demo/*.tape; do vhs "$tape"; done

# Render every predefined PNG from the logo.
render-logo: (_render-logo-png "github" "512")

_render-logo-png name size:
    resvg --width {{ size }} --height {{ size }} {{ logo_svg }} assets/icons/logo-{{ name }}.png
# <<< custom
