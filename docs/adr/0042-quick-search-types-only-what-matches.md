# 0042. Quick search types only what matches

- Status: Accepted
- Date: 2026-10-19
- Amends: [ADR 0022](0022-vim-modes.md)

## Context

Quick search ran as vim's incremental search ([ADR 0022](0022-vim-modes.md)): a character after
which no name matched stayed typed, and the cursor went back to where the search started. A key
typed by mistake in the middle of a name threw the cursor off the match, and every key after it
missed too, until Backspace took the mistake out. In a file manager the text is typed to reach a
name on screen, not kept as a pattern; mc and Far drop such a character, and the cursor stays on
the match.

## Decision

- **A character after which no name matches is not typed**, wherever the text cursor is: the text,
  the cursor of the panel, and the rows on screen stay as they were. Nothing else tells of it.
- **A character that leaves a regular expression unfinished is typed**, as `(` or `[`, so that the
  rest of it can follow; the prompt marks the text as no regular expression until it is one. A
  blank text searches for nothing, so a space is typed too.
- **Only typing is checked.** Ctrl+r and a pasted text go in character by character, each as if it
  were typed. Edits, Ctrl+s, which switches how the text matches, and Ctrl+g with nothing typed,
  which brings back the last search, keep their text even if nothing matches it; the cursor then
  stays where the search goes from, and Enter says "Pattern not found", as before.

## Consequences

- The cursor never leaves the last match for a mistake; Backspace is needed only to search for
  something else.
- A text that no name holds can no longer be typed to keep for `n`, as vim's search keeps it.
- In the regex mode, a character that makes the expression whole but matching nothing is dropped,
  and the text stays unfinished.
- The search for the text with the character is the one that decides whether it is typed, so a
  key costs no more than before.
