# 0095. A name scrolls no further than its end

- Status: Accepted
- Date: 2026-11-22
- Amends: [ADR 0025](0025-marks-of-text-cut-short.md)

## Context

The z keys scrolled the names of all rows of a panel together, as vim scrolls lines that do not wrap ([ADR 0025](0025-marks-of-text-cut-short.md)). A name that fit in its column scrolled too: `z l` on a long name turned `..`, `dir`, and `short` into `«`, `«r`, and `«ort`, which hid names that had nothing to show past the edge. The status line scrolls with the names but no further than its end comes to the right edge, so that a name that fits does not move
([ADR 0094](0094-k-shows-the-whole-name-on-the-status-line.md)).

## Decision

- **Each name scrolls no further than its end comes to the right edge of its column**, as `z e` puts it: a name that fits does not move, a longer one stops where its end shows, and the longest goes as far as the panel's scroll. The status line follows the same rule.
- The panel keeps one scroll (`Panel.shift`) and its limit, the longest name on screen at the right edge; only how far each row follows it changes.

## Consequences

- The names no longer move as one, as vim's lines do: names of different lengths stop at different places, each with its end at the right edge.
- `z h` brings a name back only once the scroll falls below where it stopped.
