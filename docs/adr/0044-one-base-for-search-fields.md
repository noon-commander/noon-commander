# 0044. One base for every field that searches

- Status: Accepted; amended by [ADR 0045](0045-ctrl-s-switches-the-field-typed-in.md), the
  glyph at the end of the field, by [ADR 0046](0046-case-and-the-keys-of-search-fields.md),
  case in every field and more ways for `g +`, and by
  [ADR 0047](0047-search-fields-remember-their-way-and-case.md), each field keeping its way and
  case, and the Files field of the search of a tree
- Date: 2026-10-24
- Amends: [ADR 0030](0030-search-modes.md)

## Context

Five kinds of field match what is typed: quick search, the filters of the lists, the zoxide
window's filter, the text of the search of a tree (`f /`, `f f`), and the pattern of `g +` and
`g -`. Each kept its way of matching in its own form: quick search and the lists a
`SearchMode` that went round three modes, the search of a tree a `bool` of its own with its own
switch and its own reading of `fzf` as a regular expression, and `g +` a shell pattern built in
the app, with no glyph and no mode at all. Which ways a field has was nowhere written down; it
followed from how each file was written, so a new field, or a new way for one, could differ from
the others without anyone choosing it.

## Decision

- **One module, `tui/search.rs`**, which `tui/search_mode.rs` becomes. Every field that searches
  goes through it.
- **`Way`**: plain, fzf, regex, and glob, a shell pattern as mc's `+` takes. `ui.search_mode`
  keeps its three values; glob is not one of them.
- **`Kind`**: one constant per kind of field, which says its ways in the order Ctrl+s goes round
  them, the way it starts in when the one asked for is not among them, and whether it matches
  names or paths. `QUICK_SEARCH`, `FILTER`, `FILTER_PATHS`: plain, fzf, regex. `TREE`: plain and
  regex, `fzf` starting it in regex. `MARK`: glob alone. These are the behaviors there were;
  nothing changes but where they are written.
- **`Input`**: a field while it is open, its kind and its way. It starts in the way asked for,
  `ui.search_mode` mostly, if the kind has it; switches to the next on Ctrl+s, and says whether it
  changed, so that a kind of one way does nothing; builds the `Matcher`; and gives the glyph.
- **`Matcher` matches in every way**, glob too, and takes `Case`: the way's own (plain and glob
  ignore case, fzf and regex count it once a capital is typed), or always, as the check box of
  `g +` asks. Which fields offer which case is for later; each keeps its own for now.
- **Every field shows the glyph of its way**, even with a single way: `g +` and `g -` gain a
  prompt, `Pattern *:`, with nf-md-asterisk or `*`.
- **`g +` and `g -` start in the way they matched last**, as they start with the last pattern and
  check boxes, rather than in `ui.search_mode`, which has no glob.

## Consequences

- The ways of every field are in one table, in `search.rs`, and a test goes through it: the ways
  each kind has, Ctrl+s going round them, and the way each value of `ui.search_mode` starts it
  in. Giving a field another way, such as regex to `g +`, is a change to its `Kind`, its key in
  the keymap, and its text.
- The dialog of `g +` and `g -` has a prompt over its field.
- `n` and `N` keep the `Way` of the last quick search rather than its `SearchMode`.
