# 0036. Lists open in Insert mode

- Status: Accepted
- Date: 2026-10-18
- Amends: [ADR 0022](0022-vim-modes.md)

## Context

The windows that list items to choose from all have a filter: the location menu, the tab menu,
the zoxide window, the Workspaces window, the command history, and the list of the key hints. As
ADR 0022 decided, all but the zoxide window opened in Normal mode, where `j`, `k`, `d d`, and `q`
act on the rows, and `/` typed in the filter: they were to be mostly browsed. The zoxide window
opened in Insert mode, as `zi` does.

The same kind of window thus took the keys two ways, and the line of the mode said `NORMAL · List`
in one and `INSERT · List` in the other. In each of them, what is wanted most of the time is a row
found by a few letters; with Normal mode first, that took `/` every time. fzf, telescope, and the
pickers of nvim open with the prompt taking the keys.

## Decision

- **Every list opens in Insert mode, with its filter taking the keys**, as the zoxide window does:
  characters are text, the keys of `input` edit the filter, and Up, Down, Ctrl+p, Ctrl+n, the page
  keys, Enter, Ctrl+s, Ctrl+c, and F10 keep working, as do the F-keys a window binds in Insert
  mode.
- **Esc goes back to the rows**, in Normal mode, with the filter kept; Esc there closes the
  window. `/` types in the filter again, and so does `i`, as it starts Insert mode in a text
  field. The keys of Normal mode (`j`, `k`, `g g`, `G`, counts,
  `K`, `q`, `d d`, and the letters of a window, such as `a` and `r` in the Workspaces window or
  `R` in the location menu) wait for Esc.
- `Picker::new` starts filtering; the keymap adds `i` to `/` in the lists' Normal mode.

## Consequences

- All lists take the keys one way, and the line of the mode says `INSERT · List` as they open.
- Closing a list with Esc takes two presses; Ctrl+c and F10 close it at once.
- `q` no longer closes a list as it opens, nor `d d` deletes a row: both type in the filter. `q :`
  from the command line in Normal mode opens the history in Insert mode.
