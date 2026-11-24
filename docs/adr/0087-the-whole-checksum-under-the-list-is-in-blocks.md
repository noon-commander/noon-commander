# 0087. The whole checksum under the list is in blocks

- Status: Accepted
- Date: 2026-11-16
- Amends: [ADR 0058](0058-the-window-of-checksums-filters-and-views-them.md)

## Context

With several files, the window of checksums shows under its rows the name and the whole checksum of the file under the cursor (ADR 0058). The checksum was one run of hex digits, broken into even lines only where it did not fit: SHA-512 into two lines of 64 digits. A run of 64 digits is hard to read, and harder to compare with a checksum on a download page, which people do a group of digits at a time. Nothing in front of the two lines told the name from the checksum, where the footer of the command history has an icon in front of each line
([ADR 0072](0072-the-command-history-shows-where-a-command-ran.md)).

## Decision

- **The checksum is a block**: groups of 8 digits, a blank between two, at most 4 groups on a line, 35 cells, as many on each line as can be. SHA-256 and BLAKE3 take two lines, SHA-512 four, SHA-1 two, MD5 one. Where 4 groups do not fit, a line takes as many as fit; where not even one does, the digits break into even lines without groups, as before.
- **The footer keeps its height**: the longest checksum of the window sets it, as before.
- **With `ui.icons`, an icon in front of each line**: the file's in front of its name, as the viewer shows it in front of its path, and `nf-md-pound` (`glyphs::CHECKSUM`), as `f #`, in front of the checksum, or of `skipped`. The lines of the checksum after its first are indented as far, so that the block stays one. Without icons nothing goes in front of them.
- **Only the footer.** The dialog of one file or two compared, the rows, To clipboard, All to clipboard, View, and Save… keep the checksum as one run of digits.

## Consequences

- A checksum is read and compared a group at a time.
- The window of several files is a line taller for SHA-256 and BLAKE3, and two for SHA-512.
- A checksum selected with the mouse in the footer has blanks in it; To clipboard copies it without them.
