# 0022. One keymap, in vim's modes

- Status: Accepted
- Date: 2026-10-13
- Supersedes: [ADR 0020](0020-esc-acts-at-once.md)

## Context

Noon Commander had two keymap presets, `ui.keymap = "default"`, modelled on Midnight
Commander, and `"vim"`, with vim's letters in the panels. Neither had modes: a context, such as
a panel, a dialog's text field, or the command line, decided what keys did, and every text field
behaved as a desktop one, with readline's keys. Keeping two presets in step doubled every
change of a binding, its tests, and its row in `docs/keymap-compare.md`, and the vim preset
stopped where vim's model starts: its fields had no Normal mode, no operators, no counts, and
no registers.

## Decision

- **One keymap, vim's, in its modes.** `ui.keymap` is gone; a `config.toml` that still has it
  is an error, as any unknown key is. `noc keymap diff` and `just keymap-diff` are gone with the
  second preset. Bindings are looked up by mode and context: Normal, Insert, and Visual mode,
  each with the context chains of before. The keys of `global` work in every mode.
- **The F-keys of the panels stay mc's**, in Normal and Visual mode and in quick search: F1 …
  F10, Shift+F6, Shift+F7, Shift+F9, Alt+F1, and Alt+F2; in Visual mode, those that do not act on
  the rows selected end it first. Dialogs keep F10, which cancels, as mc's do. No other key of mc
  stays: not `Ctrl+x …`, `Ctrl+u`, `Alt+o`, Insert, or readline's keys in text fields. Alt+1 …
  Alt+0 no longer stand for F1 … F10. mc's `+`, `-`, and `*`, which mark by a pattern and invert
  the marks, go under `g` as `g +`, `g -`, and `g *`, which vim gives to its undo tree and to a
  search for the word under the cursor, neither of which a listing has; `+`, `-`, and `*` alone,
  vim's line motions and search, stay unbound rather than mark files where a vim user moves.
  Shift+Up and Shift+Down no longer mark either; since
  [ADR 0025](0025-marks-of-text-cut-short.md) they do again, so that `K` can show the whole name
  under the cursor.
- **Where keys start.** The panels, the lists but the zoxide window, the pull-down menu, the
  viewer, and the help start in Normal mode. Quick search, the command line of `:` and `!`, the
  field of a rename, the keywords of the zoxide window, and a dialog whose first focus is a text
  field start in Insert mode; a dialog whose first focus is a button, in Normal mode. The zoxide
  window is there to type keywords, as `zi` is, where the other lists are mostly browsed.
- **Esc goes to Normal mode**, then closes in Normal mode, in the field of a rename and the fields
  of dialogs. Quick search, `:`, and `!` are typed as vim's command line: Esc closes them, and
  Ctrl+f puts `:` and `!` in Normal mode in place of vim's window of the command line, after which
  Esc leaves Insert mode, as in that window, and Ctrl+c and F10 close it. A secret field has no
  Normal mode: Esc closes its dialog. Ctrl+c closes or cancels in every mode, the terminal's own
  screen of `g <` included.
- **Esc still acts at once**, as ADR 0020 decided: it never waits for a key after it. Typed
  while a command is pending, a sequence such as `g`, a count, a register, or an operator, it
  gives the command up, as in vim, instead of doing what it does alone; so does a key that
  cannot go on with the command, such as one that is no motion after `d`, or `x` after `g`, and
  in Normal and Visual mode that key does nothing itself, as in vim.
- **Alt and a key stand for Esc and the key** in Insert and Visual mode, as an Esc and a quick next
  key arrive in a terminal, and in the panels in Normal mode, unless something binds that Alt key or
  the key goes on with a command typed so far. Any key but Esc does, Enter, Backspace, and the
  arrows too. Without the kitty keyboard protocol, an Esc and a quick key arrive as Alt and the key,
  and vim gets them the same way. With it, which Noon Commander asks for, Alt arrives as Alt: vim
  would take an Alt key that nothing maps as a character with the eighth bit set (Alt+j as `ê`),
  typed in Insert mode and doing nothing in Normal mode. Noon Commander keeps Esc and the key, as
  nvim does in Insert and Visual mode and on its command line (`:help i_ALT`), and in the panels
  too, as vim does without the protocol. On a password they do nothing: the Esc would close its
  dialog and send the key to the panels. The pull-down menu runs the command of a letter with Alt
  and the letter; its plain letters move as `h`, `j`, `k`, and `l`.
- **Normal mode in text fields is vim's**: its motions, the operators `d`, `c`, and `y` on a
  motion or doubled for the line, `x`, `s`, `r`, `~`, `p`, `u`, Ctrl+r, and Visual mode. A text
  field opens in Insert mode with the cursor after its text, not selected: Ctrl+u clears it.
- **Counts and registers.** In Normal and Visual mode, a count goes before a command: motions
  repeat, `G` and `g g` go to the row of that number, `g t` to the tab. Registers `a` … `z` and
  the unnamed one live in memory and every text field and the command line share them; `A` …
  `Z` append. `"` names one wherever keys are commands, the panels and the lists too, so that the
  name never runs as a command, as vim never runs it. None reads or writes the system clipboard.
- **The panels in vim's way**: `v` and `V` select rows in Visual mode, whose commands of files act
  on the rows selected; Ctrl+o and Ctrl+i (Tab) go back and forth through a jump list per tab, of
  locations and of `G`, `g g`, and searches; Ctrl+w switches the panels, as vim's windows. The rows
  scroll as vim 9.1 scrolls a window, with the 'scrolloff' of its `defaults.vim`, five rows, fixed:
  Ctrl+d, Ctrl+u, Ctrl+f, and Ctrl+b scroll the rows and move the cursor as vim's do, rather than
  move the cursor by a page as mc's keys do, in the lists too. One exception: Ctrl+f stops with
  the last row at the bottom rather than scroll on until it is alone at the top, which left a
  nearly empty panel; once the last row shows, Ctrl+f goes to it
  ([ADR 0023](0023-paging-stops-at-the-end.md)).
- **Quick search is vim's incremental search**, not mc's: a character that matches nothing stays
  typed while the cursor goes back to where the search started, an edit searches again from there,
  Enter says "Pattern not found" and keeps the text for `n`, and Esc puts back the rows on screen
  too, as vim's `search_start` and `old_viewstate` have it; this replaces ADR 0016's dropped
  characters. Tab completes as vim's command line with 'wildmenu' does, rather than as bash: the
  first match goes in whole, and the next Tab the next, round through what was typed; the list stays
  a column, as vim's pop-up menu, with its keys: Up and Down choose, Left and Right go up and down
  the directories; case never counts, as vim's 'fileignorecase' on macOS. The command line completes
  too, as vim's `:!`.
- **The lists** (the location menu, the tab menu, the zoxide window, the workspaces, the command
  history) take `j`, `k`, and `q` in Normal mode, and `d d` where a row can go; `/` types in
  their filter in Insert mode, and Esc goes back to the rows. The zoxide window opens with its
  keywords in Insert mode, so its first Esc goes to the rows and the second closes it. Their
  digit hotkeys are gone.
- **Ctrl+k shows the key hints** of whatever is in front, in every mode and every terminal, in
  place of Alt+/, which an Esc and a quick `/` arrived as: Esc then `/` started key hints rather
  than quick search. Ctrl+k comes as a byte of its own, without the kitty keyboard protocol, is
  free in every mode, and is no prefix of tmux or screen.
- **The mode shows** as vim's `showmode` and `showcmd` show it, on a line above the F-key bar
  that the command line takes when open: `-- INSERT --` or `-- VISUAL --` on the left, the
  count, register, operator, or keys of the pending command on the right. The cursor is a bar
  in Insert mode and a block in the others, and the terminal's own shape comes back on leaving.

## Consequences

- One preset to keep, `crates/noc/src/tui/keymap/preset.rs`; `docs/keymap-compare.md` compares
  it with mc, Far, yazi, and ranger, by mode.
- Users of mc's keys keep the F-keys, the F-key bar, and the pull-down menu; the rest of mc's
  keys are gone. Dialogs no longer replace the text they open with on the first key typed.
- The panels lose a line to the line of the mode, as vim's screen gives one to its command line.
- `$EDITOR` no longer edits the command line, until a key of vim's is chosen for it.
- User overrides of the keymap, planned for M4, will be bindings per mode and context.
