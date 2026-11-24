#!/usr/bin/env bash
# Fails if code that draws writes a special character itself, not from
# crates/noc/src/tui/glyphs.rs. Comments, tests, and other crates do not count. Needs perl.
#
#   scripts/glyphs.sh
set -euo pipefail

# The perl program's `$` are perl's.
# shellcheck disable=SC2016
git ls-files -z -- crates/noc crates/noc-viewer crates/noc-scroll crates/noc-text \
    ':!crates/noc/src/tui/glyphs.rs' ':!crates/*/tests/*' |
    grep -z '\.rs$' |
    xargs -0 perl -CSD -ne '
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
