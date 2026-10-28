# 0051. Ctrl+x runs a command in the other panel

- Status: Accepted
- Date: 2026-10-28
- Amends: [ADR 0011](0011-tabs-per-panel.md),
  [ADR 0021](0021-location-menu-replaces-the-virtual-root.md), [ADR 0022](0022-vim-modes.md),
  [ADR 0049](0049-menus-of-a-panel-keep-the-active-panel.md)

## Context

Three ways to name a panel lived together. Most keys acted on the active panel. Some acted on the
other one: Ctrl+w f and Ctrl+w v, and F5 and F6, whose target it is. Others named a side: Ctrl+w 1
and Ctrl+w 2 the location menu of the left or the right panel, `t h` and `t l` its tab menu, and
F9 → Left and Right the commands of either
([ADR 0049](0049-menus-of-a-panel-keep-the-active-panel.md)).

Only the two menus could reach the other panel. `g d` and `g z`, which choose where a panel goes as
well, could not: sending the other panel to a path typed, or to a directory of zoxide, took Tab,
the command, and Tab back. Go to directory and the zoxide window were drawn over both panels, for
the active one; for the other panel, something on screen has to say which panel they send.

The sides were not the same side, either. `t h` named the left panel, wherever it was drawn, and
Ctrl+w 1 the panel drawn on the left, so after Ctrl+w x, `t h` opened the tabs of the panel drawn
on the right.

One key before a command, for the other panel, says it once for every command. A key that sends
only some commands, those that need no cursor, makes a second keymap to learn: which keys work
after it, and which it gives up. Tab, the command, and Tab back work for every command; the key
can do that, as vim's Ctrl+o in Insert mode runs one command of Normal mode and goes back.

Ctrl+w could not be that key: it is vim's prefix for windows, whose keys noc keeps for the panels,
and Ctrl+w f and Ctrl+w v already end their sequences. Letters and symbols (`O`, `,`, `w`) were left
to vim's commands; a Ctrl key reads as the prefix it is, apart from the letters of the command after
it, as Ctrl+w does. Of those, Ctrl+a is the prefix of screen and of many a tmux; Ctrl+z suspends;
macOS takes Ctrl+Space to switch the input source; Ctrl+s leads the switches of fields that search
([ADR 0046](0046-case-and-the-keys-of-search-fields.md)); Ctrl+e and Ctrl+y are vim's to scroll a
line, Ctrl+r its redo, Ctrl+o and Ctrl+i its jumps, which the panels keep, and Ctrl+v its Visual
block mode, which some terminals take to paste; Ctrl+\\, Ctrl+], and Ctrl+^ are hard to type on
many layouts.

## Decision

- **Keys act on the active panel; Ctrl+x runs the next command in the other panel, then the keys
  come back.** For the command, the other panel has the keys: it is drawn active, and every key
  does there what it does in the active panel, those of its cursor, its marks, and its files too.
  Ctrl+x `j` moves its cursor, Ctrl+x F5 copies from it to this panel, Ctrl+x Ctrl+w v shows its
  directory in this one, and Ctrl+x `!` runs a command in its directory, on its host. x as in
  crossing over to the other side; vim's Ctrl+x subtracts from the number under the cursor, which
  a listing has not. It arrives the same in every terminal, with or without the kitty keyboard
  protocol.
- **The command ends when the other panel has the keys in Normal mode again, with nothing in front
  of it**: at once after `R`, `j`, or `o s`; once its window closes after `g l`, `g d`, `t t`,
  `g +`, F5, `!`, or F3; once Visual mode or quick search ends; once a program it hands the
  terminal to returns. The keys then go back to the panel where Ctrl+x was typed, whatever the
  command did: Ctrl+x Tab ends where it began.
- **It comes with the command, as a register does** (`keymap::Extra`), not as bindings of its own.
  The count and the register of the command come after it: Ctrl+x `2g t` shows the second tab of
  the other panel. A count or a register typed before it goes. Esc gives it up, and so does Ctrl+x
  again, as the keys would come back where they are.
- **`g l` opens the location menu and `t t` the tab menu** of the active panel; Ctrl+x `g l` and
  Ctrl+x `t t` those of the other. l for location, beside `g d` and `g z`, which also choose where
  the panel goes; vim binds no `g l`. `t t` in the group of the tabs, as vim's `:tabs` lists them.
  Ctrl+w 1, Ctrl+w 2, `t h`, and `t l` are bound nowhere. Ctrl+w keeps the panels as vim's
  windows: Ctrl+w w, Ctrl+w Ctrl+w, Ctrl+w p, Ctrl+w h, Ctrl+w l, Ctrl+w x, Ctrl+w r, Ctrl+w f,
  and Ctrl+w v.
- **Every window opens centered over both panels, those of one panel too**: the location menu, the
  tab menu, Go to directory (after `g d`, and after the Go to button of a listing that failed), the
  zoxide window, and the dialogs of `g +` and `g -`. The panel drawn active says which one they act
  on: after Ctrl+x, the other one, which has the keys. A window is where every other one is,
  whichever panel it is for.
- **Titles name no side**: the location menu says Location and the tab menu Tabs, after Ctrl+w x
  too.
- **F9 → Left and Right act on the panel drawn on their side**, as before, and show the keys that
  do the same from the active panel: in its menu the keys as they are, `g l` for Change location…
  and `t t` for Tab list…, and in the other's Ctrl+x and the key, `Ctrl+x g l` and `Ctrl+x t t`;
  the sort orders, Rescan, New tab, and Close tab too, which showed no key on the other side.
- **The key hints and the line of the mode know it.** After Ctrl+x the hints show after
  `ui.which_key_delay_ms`, with every key of the panel, and the list of Ctrl+k runs the command
  chosen in the other panel; the keys typed show it as `^X`. While the command has the keys, the
  line of the mode starts with `(^X)`, as `(^X) INSERT · Dialog`, as vim's shows `(insert)` after
  Ctrl+o.

## Consequences

- Nothing to learn of which keys Ctrl+x takes: every key does in the other panel what it does in
  the active one, and commands need nothing of their own for it.
- The location menu, the tab menu, Go to directory, and the zoxide window are named as the active
  or the other panel, not by a side, so Ctrl+w x changes nothing about which panel a key reaches.
- Far's users lose the keys of a side; F9 → Left and Right keep them. Far draws its menu of drives
  over its panel; noc's location menu is where every window is, and the frame of the active panel
  says which one it changes.
- Reaching the other panel's location menu takes three keys, Ctrl+x `g l`, where Ctrl+w 2 took two.
- mc's users know Ctrl+x as the prefix of mc's commands of files (chmod, links), which noc keeps
  under `f` ([ADR 0022](0022-vim-modes.md)); after Ctrl+x a letter does what it does in the other
  panel, so Ctrl+x `l` opens the directory under its cursor rather than make a link.
- A count typed before Ctrl+x, as before a register in vim, does nothing.
- The event loop gives the keys back when a command ends without a key, by the mouse or a program
  it ran, as well as after a key.
- The demos open the right panel's location menu with Ctrl+x `g l`.
