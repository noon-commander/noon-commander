# 0046. Case, and the keys of fields that search

- Status: Accepted
- Date: 2026-10-24
- Amends: [ADR 0030](0030-search-modes.md), [ADR 0044](0044-one-base-for-search-fields.md),
  [ADR 0045](0045-ctrl-s-switches-the-field-typed-in.md)

## Context

Every field that searches has its way of matching, and Ctrl+s in the field switches it
([ADR 0045](0045-ctrl-s-switches-the-field-typed-in.md)). Whether case counts was the way's own:
plain text and shell patterns ignored it, fzf and regular expressions had smart case. Two fields
had a check box for it of their own: Match case in the search of a tree, which asked ripgrep and fd
for case or smart case, and Case sensitive in `g +` and `g -`, on by default as in mc. Quick search
and the filters had no way to say it at all.

One key went round the ways, and the only way to reach one was to go round to it. A key for case
would have been another key of its own, and each new switch another; the free keys of a text field
in vim's Insert mode are few.

The glyph of the way stood at the end of the field right against the frame. kitty draws a Nerd Font
icon over two cells when the cell after it is blank, and squeezes or spills it otherwise, so the
glyph went over the frame. nf-md-asterisk, of shell patterns, is drawn larger than the rest.

## Decision

- **Case has three states, smart case first in every field and every way**: smart case (case counts
  only once what is typed has a capital letter, as in ripgrep and fd), case ignored, and case. A
  field starts in smart case; `g +` and `g -` start in the case they had last, as they do their
  way; `n` and `N` match with the case of the last quick search. `noc_pattern::Case` holds it, for
  `search.rs`, the regex engine, nucleo, and ripgrep's and fd's arguments (`--smart-case`,
  `--ignore-case`, `--case-sensitive`; fd's smart case is its default).
- **The check boxes go.** The search of a tree and `g +` take their case from the field, as every
  field does. Alt+c is no letter of the search of a tree any more.
- **The glyph of the case goes before that of the way, at the end of the field**, with a blank
  before them and one after each, in the field's colors: `Filter: text   Aa .*`, and a blank. The
  blank after a glyph is where kitty draws the icon on. The glyphs are
  nf-md-format_letter_case_upper, nf-md-format_letter_case_lower, and nf-cod-case_sensitive (VS
  Code's Match Case), or `Aa`, `aa`, and `AA` without icons. Shell patterns take nf-seti-shell.
- **Ctrl+s is a leader, in the field with the focus alone** (`search_input`): `Ctrl+s Ctrl+s` or
  `Ctrl+s s` the next way, round; `Ctrl+s p`, `f`, `r`, `g` plain text, fzf, a regular expression,
  or a shell pattern; `Ctrl+s c` the next case, round; `Ctrl+s a`, `i`, `m` smart case ("auto"),
  case ignored, or case. Ctrl+s alone waits for the next key, as `g` does, and shows what can
  follow after the delay of the key hints. A click on the glyph of the case does what `Ctrl+s c`
  does, and on that of the way what `Ctrl+s Ctrl+s` does.
- **The keys of the ways a field has not do nothing, and no hints show them.** The keymap knows the
  kind of the field with the focus (`keymap::Place::search`); the key hints, and their counts of
  what can follow a key, leave out the actions that the kind does not allow. Such a key still does
  its nothing rather than type its letter in the field.
- **`g +` and `g -` match as shell patterns, text, or regular expressions**, shell patterns first.
- **In the zoxide window, the plain way's keywords keep their case where it counts.** zoxide ignores
  case; where case counts, a directory it gives shows only if it has each keyword in its case.

## Consequences

- One leader holds every switch of a field that searches; a new switch takes a letter after it,
  not a key of its own.
- Switching the way takes two keys where it took one; going to a way takes two where it took up
  to three.
- A capital typed in quick search makes case count, and a character that then matches nothing is
  dropped, as any is (ADR 0042); `Ctrl+s i` lets it match either case.
- `g +` no longer tells case apart unless the pattern has a capital, or the case says so.
- Fields that search are three cells wider at their end: the glyph of the case and two blanks.
