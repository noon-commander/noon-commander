# 0040. One picker for every list

- Status: Accepted
- Date: 2026-10-19
- Amends: [ADR 0016](0016-fuzzy-search.md)

## Context

The location menu, the tab menu, the Workspaces window, the command history, and the list of the
key hints share `tui/picker.rs`: the filter, the cursor, scrolling, the mouse, the box, and what
`K` shows. The zoxide window had its own copy of all of it, since zoxide, not a filter, picks its
rows in the plain mode, and since it sorted the rows by the match where the others kept their
order ([ADR 0016](0016-fuzzy-search.md)). The two copies drifted: the zoxide window said nothing
while it asked zoxide again, and each change to the lists, such as Insert mode as they open
([ADR 0036](0036-lists-open-in-insert-mode.md)) or the skeleton and the dots of
[ADR 0037](0037-spinners.md), was made twice.

The location menu and the zoxide window also both wait for their rows from a task, each with its
own generation, wait, skeleton, and error.

Keeping their order, the lists put the cursor on the best match but showed the others around it
in the order they had, so the second best could be anywhere. fzf, `zi`, telescope, and the
pickers of editors show the best match first.

## Decision

- **Every list uses the picker**, the zoxide window too. A window may ask for paths to be matched
  as paths (`Picker::paths`, fzf's score after `/`) and for its own program to pick the rows in
  the plain mode (`Picker::plain_keywords`), as zoxide does with keywords; the picker then says
  that the filter changed (`Pick::Filtered`), and the window asks again. A window may give its own
  prompt (`Jump to`) and its own text for a filter that matches nothing.
- **In the fzf mode, every list ranks its rows**: the best match first, and those as good in the
  order the list has them. In the plain and regex modes the rows keep their order. With an empty
  filter every list shows its own order. The headings of the location menu go while the rows are
  ranked, as they no longer group them.
- **`tui/loading.rs` keeps the wait for rows from a task** (`Loading`): the answer the list waits
  for, the dots of the first wait and of the next ones, the skeleton as tall as the last list, and
  why no rows came. The location menu and the zoxide window hold one, and the picker draws it.

## Consequences

- The zoxide window draws, scrolls, and takes the keys and the mouse as the other lists do, and
  turns its dots while zoxide is asked again.
- A change to how the lists work is made once.
- In the fzf mode the location menu loses its groups while the filter has text: a host may come
  before the home directory. Its places are short, and the best match first is what the filter is
  typed for.
- The command history, recent first, ranks by the match too, recent first among matches as good.
- `Pick::Filtered` is one more answer that the windows which do not ask again treat as nothing to
  do.
