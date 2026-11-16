#!/bin/sh
# Updates a clone of the AUR repository of a package (ADR 0090): renders its PKGBUILD from the
# template in packaging/aur, builds the package with makepkg to check it, and commits PKGBUILD,
# .SRCINFO, and LICENSE. It pushes nothing; release.yml pushes the commit.
#
#   packaging/aur/update.sh PACKAGE DIR
#
# PACKAGE is a directory of packaging/aur; DIR, which must not exist yet, gets the clone. The
# placeholders of the template come from the environment: VERSION, REPOSITORY, SHA256_SOURCE,
# SHA256_X86_64, SHA256_AARCH64. A PKGBUILD that differs from the one in the AUR in nothing but
# pkgver, as that of a VCS package does, is neither built nor committed. Needs makepkg and git,
# and to run as a user other than root.
set -eu

# The author of the commits, as the Maintainer line of each template says.
author_name=0ldkettle
author_email=packages@noon-commander.app

if [ $# -ne 2 ]; then
    echo "usage: $0 PACKAGE DIR" >&2
    exit 2
fi
package=$1
dir=$2
here=$(cd "$(dirname "$0")" && pwd)
template=$here/$package/PKGBUILD.in
case $package in
    '' | .* | */*) template= ;;
esac
if [ -z "$template" ] || [ ! -f "$template" ]; then
    echo "No template for $package in $here" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for name in VERSION REPOSITORY SHA256_SOURCE SHA256_X86_64 SHA256_AARCH64; do
    if grep -q "@$name@" "$template" && [ -z "$(printenv "$name" || true)" ]; then
        echo "$name is not set, and the template of $package needs it" >&2
        exit 1
    fi
done
sed -e "s|@VERSION@|${VERSION-}|g" -e "s|@REPOSITORY@|${REPOSITORY-}|g" \
    -e "s|@SHA256_SOURCE@|${SHA256_SOURCE-}|g" -e "s|@SHA256_X86_64@|${SHA256_X86_64-}|g" \
    -e "s|@SHA256_AARCH64@|${SHA256_AARCH64-}|g" "$template" >"$work/PKGBUILD"
if grep -n '@[A-Z0-9_]*@' "$work/PKGBUILD" >&2; then
    echo "The PKGBUILD of $package has placeholders left" >&2
    exit 1
fi

git clone --quiet "https://aur.archlinux.org/$package.git" "$dir"

if [ -f "$dir/PKGBUILD" ]; then
    sed '/^pkgver=/d' "$dir/PKGBUILD" >"$work/old"
    sed '/^pkgver=/d' "$work/PKGBUILD" >"$work/new"
    if cmp -s "$work/old" "$work/new" && cmp -s "$dir/LICENSE" "$here/LICENSE"; then
        echo "$package is up to date in the AUR"
        exit 0
    fi
fi

mkdir "$work/build"
cp "$work/PKGBUILD" "$work/build/"
(cd "$work/build" && makepkg --cleanbuild --noconfirm --noprogressbar)

# makepkg wrote the pkgver of a VCS package into its PKGBUILD.
cp "$work/build/PKGBUILD" "$here/LICENSE" "$dir/"
(cd "$dir" && makepkg --printsrcinfo >.SRCINFO)
git -C "$dir" add PKGBUILD .SRCINFO LICENSE
if git -C "$dir" diff --cached --quiet; then
    echo "$package is up to date in the AUR"
    exit 0
fi
pkgver=$(sed -n 's/^[[:space:]]*pkgver = //p' "$dir/.SRCINFO")
pkgrel=$(sed -n 's/^[[:space:]]*pkgrel = //p' "$dir/.SRCINFO")
version=$pkgver-$pkgrel
git -C "$dir" -c user.name="$author_name" -c user.email="$author_email" commit --quiet \
    -m "$package $version"
echo "Committed $package $version"
