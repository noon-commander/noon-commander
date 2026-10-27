# 0051. Ctrl+x sends a command to the other panel

- Status: Accepted
- Date: 2026-10-28
- Amends: [ADR 0021](0021-location-menu-replaces-the-virtual-root.md),
  [ADR 0022](0022-vim-modes.md), [ADR 0049](0049-menus-of-a-panel-keep-the-active-panel.md)

## Context

Three ways to name a panel lived together. Most keys acted on the active panel. Some acted on the
other one: Ctrl+w f and Ctrl+w v, and F5 and F6, whose target it is. Others named a side: Ctrl+w 1
and Ctrl+w 2 the location menu of the left or the right panel, `t h` and `t l` its tab menu, and
F9 → Left and Right the commands of either
([ADR 0049](0049-menus-of-a-panel-keep-the-active-panel.md)).

Only the two menus could reach the other panel. `g d` and `g z`, which choose where a panel goes as
well, could not: sending the other panel to a path typed, or to a directory of zoxide, took Tab,
the command, and Tab back. Go to directory and the zoxide window were drawn over both panels, so
nothing on screen said which panel they would send.

The sides were not the same side, either. `t h` named the left panel, wherever it was drawn, and
Ctrl+w 1 the panel drawn on the left, so after Ctrl+w x, `t h` opened the tabs of the panel drawn
on the right.

One key before a command, for the other panel, says it once for every command. Ctrl+w could not
be that key: it is vim's prefix for windows, whose keys noc keeps for the panels, and Ctrl+w f and
Ctrl+w v already end their sequences. Letters and symbols (`O`, `,`, `w`) were left to vim's
commands; a Ctrl key reads as the prefix it is, apart from the letters of the command after it, as
Ctrl+w does. Of those, Ctrl+a is the prefix of screen and of many a tmux; Ctrl+z suspends; macOS
takes Ctrl+Space to switch the input source; Ctrl+s leads the switches of fields that search
([ADR 0046](0046-case-and-the-keys-of-search-fields.md)); Ctrl+e and Ctrl+y are vim's to scroll a
line, Ctrl+r its redo, and Ctrl+v its Visual block mode, which some terminals take to paste;
Ctrl+\\, Ctrl+], and Ctrl+^ are hard to type on many layouts.

## Decision

- **Keys act on the active panel; Ctrl+x before one acts on the other panel, and the keys stay.**
  The active panel keeps them, as after Ctrl+w f and Ctrl+w v. x as in crossing over to the other
  side; vim's Ctrl+x subtracts from the number under the cursor, which a listing has not. It
  arrives the same in every terminal, with or without the kitty keyboard protocol.
- **It comes with the command, as a count or a register does** (`keymap::Extra`), not as bindings
  of its own. The commands that take it are those of a panel that need neither its cursor nor the
  marks it has: where it goes (`g l`, `g d`, `g z`), its tabs (`t t`, `t n`, `t c`, `g t`, `g T`),
  its order (`o n`, `o e`, `o m`, `o s`), its marks by a pattern (`g +`, `g -`), and reading it
  again (`R`). A count goes before it or after it: `2 Ctrl+x g t` and `Ctrl+x 2g t` show the second
  tab of the other panel. Any other command typed after it is given up, as vim gives up a key that
  cannot go on; Esc gives it up too.
- **`g l` opens the location menu and `t t` the tab menu**, of the active panel; Ctrl+x `g l` and
  Ctrl+x `t t` those of the other. l for location, beside `g d` and `g z`, which also choose where
  the panel goes; vim binds no `g l`. `t t` in the group of the tabs, as vim's `:tabs` lists them.
  Ctrl+w 1, Ctrl+w 2, `t h`, and `t l` are bound nowhere. Ctrl+w keeps the panels as vim's
  windows: Ctrl+w w, Ctrl+w Ctrl+w, Ctrl+w p, Ctrl+w h, Ctrl+w l, Ctrl+w x, Ctrl+w r, Ctrl+w f,
  and Ctrl+w v.
- **What Ctrl+x can send opens over its panel**, as the location menu does: the tab menu, Go to
  directory (after `g d`, and after the Go to button of a listing that failed), the zoxide window,
  and the dialogs of `g +` and `g -`, which were over both. For the other panel, Go to directory
  reads relative paths, `-` (the directory before), and completion from that panel's directory;
  the zoxide window leaves out that panel's directory and opens the one chosen there, which zoxide
  counts as before; `g +` and `g -` mark and unmark its names.
- **Titles name the side where the panel is drawn**: the tab menu says Left tabs or Right tabs,
  after Ctrl+w x too, as the location menu says Left or Right.
- **F9 → Left and Right act on the panel drawn on their side**, as before, and show the keys that
  do the same from the active panel: in its menu the keys as they are, `g l` for Change location…
  and `t t` for Tab list…, and in the other's Ctrl+x and the key, `Ctrl+x g l` and `Ctrl+x t t`;
  the sort orders, Rescan, New tab, and Close tab too, which showed no key on the other side.
- **The key hints and the line of the mode know it.** After Ctrl+x the hints show after
  `ui.which_key_delay_ms`, with only the keys it takes; the keys typed show it as `^X`.

## Consequences

- The location menu, the tab menu, Go to directory, and the zoxide window are named as the active
  or the other panel, not by a side, so Ctrl+w x changes nothing about which panel a key reaches.
- Far's users lose the keys of a side; F9 → Left and Right keep them.
- Reaching the other panel's location menu takes three keys, Ctrl+x `g l`, where Ctrl+w 2 took two.
- mc's users know Ctrl+x as the prefix of mc's commands of files (chmod, links), which noc keeps
  under `f` ([ADR 0022](0022-vim-modes.md)); after Ctrl+x those letters do nothing.
- A new command that works without the cursor and the marks of a panel can take Ctrl+x, rather
  than a key for each side.
- The demos open the right panel's location menu with Ctrl+x `g l`.
