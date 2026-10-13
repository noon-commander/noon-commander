---
description: Check the ✅ rows of docs/keymap-compare.md against the keymap preset
agent: plan
---

# Check the ✅ rows of keymap-compare

Check that every row marked ✅ in `docs/keymap-compare.md` is written correctly and matches the
code. Change no files; report what you find. $ARGUMENTS

Sources of truth:

- `crates/noc/src/tui/keymap/preset.rs`: the keymap, the only preset. `PRESET` lists blocks of
  `(Mode, Context, &[(Action, &[keys])])`, keys in crokey's syntax; the keys of the
  `Context::Global` block, in Normal mode, work in every mode.
- `crates/noc/src/tui/keymap/action.rs`: the variants of `Mode`, `Context`, and `Action`, and
  `Context::chain(mode)`, what each context falls back to in each mode. The doc names contexts
  and actions in snake case, from the CamelCase variants: `DialogInput` → `dialog_input`,
  `PullDownLast` → `pull_down_last`.
- `describe` and `describe_key` in `crates/noc/src/tui/keymap/mod.rs`: how keys are written in
  the docs (`shift-g` → `G`, `shift-z shift-z` → `Z Z`, `ctrl-w h` → `Ctrl+w h`, `pageup` →
  `PgUp`, `backtab` → `Shift+Tab`, `alt-shift-w` → `Alt+W`). The doc escapes `*`, `~`, `|`,
  `\`, and `` ` `` for Markdown, as `\*` and `\~`; that is no discrepancy.
- The Status bullets at the top of `docs/keymap-compare.md`: ✅ means that the keymap binds the
  action, with the keys it will keep.

The doc has one section per block of `PRESET`, in its order, headed with the context and the
mode, as `` ## Panels (`panel`, Visual mode) ``; the heading of `global` says `every mode`. A
section's rows list the actions that its context binds itself in its mode, in the preset's
order, then rows whose action is "—", for what the other programs do there.

For each such section and each ✅ row in it:

1. The Action column names an action that the block of the section's mode and context binds,
   in snake case.
2. The `noc` column lists exactly the keys of that action in that block, one per `<br>`, in the
   preset's order, written as `describe` writes them.
3. The block binds the action; otherwise the row cannot be ✅.

Then look around the ✅ rows:

- Actions that the preset binds with the keys the table shows, but that are still ⏳: list them
  as candidates for ✅, without deciding.
- Actions that a block binds but whose section has no row, and rows whose action the block no
  longer binds.
- Keys of a ✅ row that another row of the same section lists in the `noc` column: one key cannot
  do two things in one mode and context.
- The prose of each section with a ✅ row: what it says the context falls back to should match
  `Context::chain(mode)`, and the keys it names should do there what the preset says.
- `git status` and `git diff` of `preset.rs`, `action.rs`, and the doc, so uncommitted changes
  are checked too.

Report in the user's language: a table of the ✅ rows checked (section, action, result), then each
discrepancy with the file, the line, and the fix you suggest. Do not edit anything; the maintainer
decides.
