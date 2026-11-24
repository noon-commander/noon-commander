#!/bin/sh
# Commits the version bump of a release, signs its tag, and pushes both, which starts release.yml
# (ADR 0014).
#
#   packaging/push-version.sh VERSION
#
# Changes nothing unless Cargo.toml has VERSION, CHANGELOG.md has a section for it, and the
# branch is main.
set -eu

if [ $# -ne 1 ]; then
    echo "usage: $0 VERSION" >&2
    exit 2
fi
version=$1

cargo_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ "${version}" != "${cargo_version}" ]; then
    echo "Cargo.toml has version ${cargo_version}, not ${version}" >&2
    exit 1
fi
if ! grep -qF "## [${version}] - " CHANGELOG.md; then
    echo "CHANGELOG.md has no section for ${version}" >&2
    exit 1
fi
branch=$(git branch --show-current)
if [ "${branch}" != main ]; then
    echo "Releases are tagged on main" >&2
    exit 1
fi

notes=$(packaging/release-notes.sh "${version}")
git commit -m "chore(release): bump version to ${version}" -- Cargo.toml Cargo.lock CHANGELOG.md
# Until the draft is published, releases.atom shows the tag with its message, and feed readers
# keep that entry; whitespace keeps the notes' `#` headings, which strip would drop.
git tag -s --cleanup=whitespace "v${version}" -m "Noon Commander ${version}" -m "${notes}"
git push --atomic origin HEAD "v${version}"
