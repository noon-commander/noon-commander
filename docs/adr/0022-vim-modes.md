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
  the marks, go under `g` as `g +`, `g -`, and `g *`, leaving vim's motions and search unbound
  rather than giving them other meanings; Shift+Up and Shift+Down no longer mark.
- **Where keys start.** The panels, the lists, the pull-down menu, the viewer, and the help
  start in Normal mode. Quick search, the command line of `:` and `!`, the field of a rename,
  and a dialog whose first focus is a text field start in Insert mode; a dialog whose first
  focus is a button, in Normal mode.
- **Esc goes to Normal mode**, then closes in Normal mode, in the field of a rename and the
  fields of dialogs. Quick search, `:`, and `!` are typed as vim's command line: Esc closes
  them, and Ctrl+f puts `:` and `!` in Normal mode in place of vim's window of the command
  line. A secret field has no Normal mode: Esc closes its dialog. Ctrl+c closes or cancels in
  every mode, the terminal's own screen of `g <` included.
- **Esc still acts at once**, as ADR 0020 decided: it never waits for a key after it. Typed
  while a command is pending, a sequence such as `g`, a count, a register, or an operator, it
  gives the command up, as in vim, instead of doing what it does alone; so does a key that
  cannot go on with the command, such as one that is no motion after `d`, or `x` after `g`, and
  in Normal and Visual mode that key does nothing itself, as in vim.
- **Alt and a key stand for Esc and the key** in Insert and Visual mode, as an Esc and a quick next
  key arrive in a terminal, and in the panels in Normal mode, unless something binds that Alt key or
  the key goes on with a command typed so far. Any key but Esc does, Enter, Backspace, and the
  arrows too, as nvim takes Alt and a key that nothing maps for Esc and the key. On a password they
  do nothing: the Esc would close its dialog and send the key to the panels. The pull-down menu runs
  the command of a letter with Alt and the letter; its plain letters move as `h`, `j`, `k`, and `l`.
- **Normal mode in text fields is vim's**: its motions, the operators `d`, `c`, and `y` on a
  motion or doubled for the line, `x`, `s`, `r`, `~`, `p`, `u`, Ctrl+r, and Visual mode. A text
  field opens in Insert mode with the cursor after its text, not selected: Ctrl+u clears it.
- **Counts and registers.** In Normal and Visual mode, a count goes before a command: motions
  repeat, `G` and `g g` go to the row of that number, `g t` to the tab. Registers `a` … `z` and
  the unnamed one live in memory and every text field and the command line share them; `A` …
  `Z` append. None reads or writes the system clipboard.
- **The panels in vim's way**: `v` and `V` select rows in Visual mode, whose commands of files
  act on the rows selected; Ctrl+o and Ctrl+i (Tab) go back and forth through a jump list per
  tab, of locations and of `G`, `g g`, and searches; Ctrl+w switches the panels, as vim's
  windows.
- **The lists** (the location menu, the tab menu, the zoxide window, the workspaces, the command
  history) take `j`, `k`, and `q` in Normal mode, and `d d` where a row can go; `/` types in
  their filter in Insert mode, and Esc goes back to the rows. Their digit hotkeys are gone.
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
