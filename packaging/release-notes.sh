#!/bin/sh
# Prints the section of a version in CHANGELOG.md, without its heading, as the notes of its
# GitHub release (ADR 0014).
#
#   packaging/release-notes.sh VERSION [CHANGELOG]
#
# GitHub renders every line break of release notes as a break, so the lines that CHANGELOG.md
# wraps at 100 characters are joined back into their paragraphs and list items. Headings, list
# items, table rows, code blocks, and hard breaks keep their lines. Fails when CHANGELOG
# (default CHANGELOG.md) has no section for VERSION, or an empty one.
set -eu

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 VERSION [CHANGELOG]" >&2
    exit 2
fi
version=$1
changelog=${2:-CHANGELOG.md}

if ! awk -v version="$version" '
    # Blank lines are held back, so that none starts or ends the notes and runs of them become one.
    function emit(text) {
        if (blank && printed) print ""
        print text
        blank = 0
        printed = 1
    }
    function flush() {
        if (line != "") emit(line)
        line = ""
    }
    /^## / { if (found) exit; found = index($0, "## [" version "]") == 1; next }
    !found { next }
    fence { emit($0); if ($0 ~ /^[[:space:]]*(```|~~~)/) fence = 0; next }
    /^[[:space:]]*(```|~~~)/ { flush(); emit($0); fence = 1; next }
    /^[[:space:]]*$/ { flush(); blank = 1; next }
    /^#/ || /^[[:space:]]*\|/ { flush(); emit($0); next }
    /^[[:space:]]*([-*+]|[0-9]+[.)])[[:space:]]/ { flush(); line = $0; next }
    line == "" { line = $0; next }
    line ~ /(\\|  )$/ { emit(line); line = $0; next }
    { sub(/^[[:space:]]+/, ""); line = line " " $0 }
    END { flush(); if (!printed) exit 1 }
' "$changelog"; then
    echo "$changelog has no section for $version" >&2
    exit 1
fi
