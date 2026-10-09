---
description: Check the ✅ rows of docs/keymap-compare.md against the keymap presets
agent: plan
---

# Check the ✅ rows of keymap-compare

Check that every row marked ✅ in `docs/keymap-compare.md` is written correctly and matches the
code. Change no files; report what you find. $ARGUMENTS

Sources of truth:

- `crates/noc/src/tui/keymap/default.rs` and `crates/noc/src/tui/keymap/vim.rs`: the presets.
- `crates/noc/src/tui/keymap/action.rs`: the snake-case action names `noc keymap diff` uses.
- `describe_key` in `crates/noc/src/tui/keymap/mod.rs`: how keys are written in the docs
  (`shift-g` → `G`, `shift-z shift-z` → `Z Z`, `pageup` → `PgUp`, `alt-shift-w` → `Alt+W`).
- The legend at the top of `docs/keymap-compare.md`: ✅ means that both presets bind the action,
  with the keys they will keep.

For each section (one per context, named as in the presets) and each ✅ row in it:

1. The Action column names an action of that context, as `action.rs` names it.
2. The `noc (default)` column lists exactly the keys of that action in `default.rs`, in the
   preset's order, written as `describe_key` writes them.
3. The `noc (vim)` column does the same for `vim.rs`.
4. Both presets bind the action in that context; otherwise the row cannot be ✅.

Then look around the ✅ rows:

- Actions that both presets bind with the keys the table shows, but that are still ⏳: list them
  as candidates for ✅, without deciding.
- Keys of a ✅ row that a ⏳ row of the same context lists in the same column: one key cannot do
  two things in one context.
- Comments in the presets: where both presets bind an action alike, they should carry the same
  comment (AGENTS.md).
- `git status` and `git diff` of the presets and the doc, so uncommitted changes are checked too.

Report in the user's language: a table of the ✅ rows checked (context, action, result), then each
discrepancy with the file, the line, and the fix you suggest. Do not edit anything; the maintainer
decides.
