# 0092. Ctrl+k lists the keys at once

- Status: Accepted
- Date: 2026-11-19
- Amends: [ADR 0022](0022-vim-modes.md), [ADR 0091](0091-pull-down-menu-without-left-and-right.md)

## Context

Ctrl+k showed the key hints of whatever was in front in the strip of which-key, at the bottom of the panels, and Ctrl+k again, while they showed, gave them the keys: the list of every sequence whole, to filter and run from ([ADR 0022](0022-vim-modes.md)). The two steps followed nvim's `K K`, whose first `K` shows the hover window while the keys still go to the text, and whose second puts the cursor in it.

The first step did little. The strip has a cell for each key; in the panels, with about ninety, most did not fit, and its last cell said how many it left out. Where they did fit, in a dialog or a menu, the list shows the same keys, and what they do in full. The strip's own work, the keys that can follow `g`, Ctrl+w, or Ctrl+x, needs no Ctrl+k: they show `ui.which_key_delay_ms` after such a key. So the list took two keys, and the first opened something that the second replaced.

The likeness to `K K` was loose too: nvim's second `K` gives the keys to the window the first opened, where noc's put another window in place of the strip.

## Decision

- **Ctrl+k opens the list of the key hints**, in every mode and context, over everything, the dialogs too: every sequence that can follow the keys typed before it, whole, with the count, the register, and Ctrl+x typed before it for the command run from it. With nothing typed, it lists every key of what is in front, as Ctrl+k twice did.
- **The strip of which-key shows only on its own**, after a key that starts longer sequences or after Ctrl+x, as `ui.which_key` and `ui.which_key_delay_ms` say. Ctrl+k while it shows opens the list, as before; Esc and Backspace do there what they did.
- **Ctrl+k in the list does nothing**: the list stays as it is, rather than list its own keys in its place. Ctrl+k twice, as a habit, opens the list as before.
- **F9 → Help → Key hints… ends in `…`**, as a command that opens a list to choose from ([ADR 0091](0091-pull-down-menu-without-left-and-right.md)); it lists every key of the active panel.
- The keymap takes the hints of what was typed when Ctrl+k comes (`KeyState::take_listed`), and the app opens the list from them. The app no longer asks the event loop to show the strip.

## Consequences

- The list takes one key, and every key of a context shows there, however many it has.
- A look at the keys and a key typed after it, which the strip allowed, takes the filter and Enter, or Esc twice and the key: the list opens in Insert mode, where letters filter it ([ADR 0036](0036-lists-open-in-insert-mode.md)). An Esc more closes a Ctrl+k typed by mistake.
- Backspace no longer takes back a key after Ctrl+k, where it edits the filter of the list. It still does while the strip shows on its own.
- With `ui.which_key` off the strip never shows; Ctrl+k after `g` lists what can follow it.
- The strip of every key still shows after Ctrl+x, once due, and after Backspace takes back the only key of a sequence while the strip shows.
- nvim's `K K` no longer explains Ctrl+k. A list of the keys behind one key, as Telescope's `keymaps` picker in nvim and Helix's command palette (`Space ?`), comes closer.
