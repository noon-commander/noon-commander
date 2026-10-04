# 0016. Fuzzy search as fzf does it

- Status: Accepted
- Date: 2026-10-04

## Context

Three places take text that picks from a list: quick search in a panel, the filter of the
location menu (Alt-F1, Alt-F2), and the keywords of the zoxide window (Alt-Z). Each matched in
its own literal way: quick search took names that start with the text, as mc does; the
location menu took rows whose texts contain it; the zoxide window passed it to zoxide, whose
keywords must appear in order with the last one in the last component of the path.

Users of fzf, and of the editors and file managers that work like it (Helix, yazi's `fzf`,
zoxide's own `zi`), type a few letters from anywhere in a name, such as `cfg` for `config.rs`,
and expect the best match first. Literal matching finds nothing for that, and in quick search
drops the letters as mistakes.

fzf's matching is more than a subsequence test: it scores matches, so that letters at the
start of words, after `/` or punctuation, and in runs count for more, and its extended search
adds `'exact`, `^prefix`, `suffix$`, and `!not`. A copy of our own would rank differently from
fzf and grow by its bugs. Running fzf itself for each key would start a process per key, and
it does not rank items for a program that draws them itself.

## Decision

- `ui.fuzzy_search`, on by default, makes the three places match as fzf does, through
  `nucleo-matcher`, the matcher of Helix: a port of fzf's algorithm with its scores and its
  extended search, MPL-2.0, with no dependencies beyond `memchr` and `unicode-segmentation`.
  Case counts once the text has a capital letter (smart case), and letters with accents match
  those without. Off, each place matches as it did.
- The code lives in `crates/noc/src/tui/fuzzy.rs`; no other crate needs it.
- Quick search keeps the order of the listing and puts the cursor on the best match; of
  matches as good, the first from where the cursor is, as mc goes from there. Ctrl-S goes to
  the next best, round to the best. A character that nothing matches is still dropped.
- The location menu keeps its order too, the home directory, the volumes, then the hosts,
  and puts the cursor on the best row. A row's texts (name and mount point; alias, label,
  and address) are matched as one line, as fzf matches the columns of one.
- The zoxide window asks zoxide once for every directory, `zoxide query --list --score
  --exclude <dir> --`, and filters them as the keys come, best first and those as good in
  zoxide's order, matching the paths as it shows them, from `~`. This is `zi` with fzf's
  default ranking, where `zi` itself passes `--exact --no-sort`: typing ranks by the match,
  while an empty filter shows zoxide's order.

## Consequences

- A new dependency, `nucleo-matcher`; its license, MPL-2.0, was already allowed.
- `cfg` finds `config.rs`, and `^`, `'`, `$`, and `!` work as in fzf; but a name that starts
  with the text no longer wins over a better fuzzy match elsewhere in a panel, and a space in
  quick search separates words instead of matching a space.
- The zoxide window no longer starts a process per key; the frecency of zoxide only orders
  matches that are as good. `ui.fuzzy_search = false` brings back zoxide's own matching
  ([ADR 0012](0012-external-tools-and-zoxide.md)).
- Tab completion still completes the start of a name: it inserts what candidates agree on,
  which only works with a common prefix.
- Matched letters are not highlighted yet.
