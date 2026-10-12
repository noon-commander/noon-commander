# 0020. Esc acts at once

- Status: Superseded by [ADR 0022](0022-vim-modes.md)
- Date: 2026-10-11

## Context

mc waits a second after Esc in a panel for a key that may follow: `Esc 1` … `Esc 0` stand for
F1 … F10, and Esc and a character for Alt and that character, for terminals without F-keys or
without an Alt that sends anything. Esc alone acts only once the second has passed. Noon
Commander did the same in the panels and the root.

With two keymap presets and keys that are letters, the wait did more harm than good:

- A key typed within the second after Esc became Alt and that key. Where nothing bound that
  Alt combination, both the Esc and the key were lost: `Esc j` in the vim preset moved nowhere.
- Where something did, it ran instead: in the default preset `Esc o n`, meant to cancel and then
  sort by name, opened the directory under the cursor in the other panel (Alt+o).
- `Esc /`, a habit of vim users, showed the key hints (Alt+/) instead of starting quick search.
- Esc itself, which stops loading and connecting, came a second late.

Ctrl+1 … Ctrl+0 could stand for the F-keys instead, but only where the terminal speaks the kitty
keyboard protocol. Elsewhere terminals send Ctrl+2 as NUL (Ctrl+Space, the output of commands),
Ctrl+3 as Esc, and Ctrl+8 as Backspace, and Ctrl+1, Ctrl+9, and Ctrl+0 as plain digits.

## Decision

- **Esc never waits.** It acts at once in every context, the panels and the root too.
- **Alt+1 … Alt+0 stand for F1 … F10** in the panels and the root, where Esc digits stood for
  them, unless the context binds that Alt key itself. Like the Esc digits before them, the help
  leaves them out of its lists and explains them once, the key hints leave them out, and the
  pull-down menu shows one only where nothing else does the same.
- **No stand-in for Alt.** Esc followed by a key no longer means Alt and that key. Terminals
  whose Alt sends nothing, such as macOS Terminal unless Option is set to act as Meta, reach the
  actions that only Alt keys do through the pull-down menu.
- **An Alt and a character that nothing binds count as Esc and the character** in the panels and
  the root, because an Esc and a quick next key reach a terminal program in one read, which
  crossterm reports as Alt and the key.
- No Ctrl+1 … Ctrl+0, for the reasons above.

## Consequences

- No key typed after Esc is lost or turned into another command, and Esc stops loading at once.
- `Esc 1` … `Esc 0` and `Esc` and a letter, which mc users may type, cancel and then do what
  the key does alone.
- In macOS Terminal with its defaults, which sends neither Alt nor F-keys without Fn, the F-keys
  need Fn or the system setting that makes them standard function keys, and Alt+o, Alt+i, Alt+h,
  Alt+c, and Alt+/ have no key; the other Alt keys of the default preset have one that is not
  Alt.
- The demo tapes, whose browser sends neither Alt nor F-keys, reach a host through the location
  menu (`Ctrl+x 2`) and quit through SIGTERM, which noc handles as quitting.
