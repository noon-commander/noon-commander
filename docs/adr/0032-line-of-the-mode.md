# 0032. The line of the mode says which keys work

- Status: Accepted
- Date: 2026-10-16
- Amends: [ADR 0022](0022-vim-modes.md)

## Context

The line above the F-key bar showed the mode as vim's `showmode` does: `-- INSERT --` and
`-- VISUAL --`, and nothing in Normal mode nor in quick search, typed as vim's search. The line
was empty most of the time, and when it was not, it still did not say where the keys went: the
same `NORMAL` stood for the panels, a list, a dialog, and the help, whose keys differ.

Naming the window by its title did not answer that either: every dialog has a title of its own,
`Create a new directory`, `File exists`, or, for a prompt of ssh, the host, `web`; windows that
take the same keys got different names, and names said nothing of the keys.

lualine, the status line of many nvim setups, shows the mode in capitals and, in the next
section, what the buffer is.

## Decision

- **The line shows the mode everywhere**, in bold capitals as lualine shows it: `NORMAL`,
  `INSERT`, `VISUAL`, `VISUAL LINE`, and after Ctrl+o `(insert)`. It is never empty: a message
  takes the place of the mode until the next key, and the command line takes the line while it
  has the keys.
- **After the mode and `·` comes the kind of window that has the keys**, named by its keymap
  context, so that the line says which keys work there:

  | Kind | Contexts |
  | --- | --- |
  | none | `panel` |
  | `Quick search` | `quick_search` |
  | `Rename` | `rename` |
  | `List` | `location_menu`, `tab_menu`, `jump`, `workspaces`, `command_history`, `key_hints` |
  | `Menu` | `pull_down` |
  | `Dialog` | `dialog`, `dialog_input`, `path_input`, `completion`, `find`, `find_input` |
  | `Help` | `help` |
  | `Viewer` | `viewer` |

  Windows with the same keys share a kind: the Configuration dialog, the search of a tree, the
  jobs, and the checksums are `Dialog`; the location menu and the zoxide window are `List`. No
  title or name of a window goes there. The panels show the mode alone.
- **`window_kind` in `crates/noc/src/tui/app.rs` maps every context**, without a wildcard, so
  that a new context does not build until it gets a kind. A new kind comes only with keys of
  its own.
- **The command line takes the line only while it has the keys.** A window over it, such as the
  command history of `q :`, takes the line back, as any window does.
- Where the line is short, the kind goes first, then the mode is cut. `user@host`
  ([ADR 0031](0031-who-and-where-on-screen.md)) and the keys typed keep their places on the
  right.

## Consequences

- The line always says the mode and which keys work, which vim's `showmode` does not.
- The keymap's contexts become visible: a context that should feel like another must take its
  keys too, or get a kind of its own.
- The line no longer tells one dialog from another; their frames do.
- `CommandLine::render` always puts the cursor on the line, since it draws only while it has the
  keys.
