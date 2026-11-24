# 0093. Quick search is typed on the line of the mode

- Status: Accepted
- Date: 2026-11-21
- Amends: [ADR 0032](0032-line-of-the-mode.md), [ADR 0045](0045-ctrl-s-switches-the-field-typed-in.md)

## Context

Quick search was typed in the status line of its panel, under the rows, as mc types it there: `Search: text`, with the glyphs of how it matches at the right end, in colors of its own. While it ran, the line of the mode said `INSERT · Quick search`.

The keys are vim's ([ADR 0022](0022-vim-modes.md)), and `/` and `?` start the search as vim's do, but vim types its search on the command line, at the bottom of the screen, where noc's command line of `!` and `:` is typed too. The search took the status line, so that the name under the cursor, which the search moves, did not show whole while it ran, nor did the size of the file.

## Decision

- **Quick search takes the line of the mode while it has the keys**, as the command line does and as vim's search takes its command line: `/text`, or `?text` up the rows, with the cursor in it and the glyphs of the case and the way at the right end of the line, which a click switches. It has the colors of the command line, none of its own; the theme's `quick_search` is gone.
- **The status line of the panel keeps the name under the cursor**, with the target of a symlink and the size of a file, while the search runs.
- **The line of the mode names no kind of window for quick search**: `window_kind` maps `quick_search` to none, as `command_line`, which shows itself. A window or a dialog over the search takes the line back, as it does from the command line; the search shows again once it closes.

## Consequences

- The line no longer says `INSERT` while the search runs: the prompt and the cursor, a bar in Insert mode, stand for it, as on the command line. After Ctrl+x, the line no longer shows `(^X)` while the search runs in the other panel, which is drawn active.
- Nor does the line show, while the search runs, the keys of a command typed so far, as `^S` after Ctrl+s or `^R` after Ctrl+r, or who is where, as on the command line; the key hints show what can follow Ctrl+s, unless `ui.which_key` is off.
- The search is drawn by the app, on the line it lays out above the F-key bar, rather than by the panel, which no longer needs to draw on every frame while it searches (ADR 0069).
- Messages after the search, such as `search hit BOTTOM, continuing at TOP`, take the same line once it ends, as in vim.
