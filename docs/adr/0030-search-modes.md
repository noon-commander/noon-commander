# 0030. Search modes

- Status: Accepted
- Date: 2026-10-16
- Amends: [ADR 0016](0016-fuzzy-search.md), [ADR 0027](0027-search-of-a-whole-tree.md),
  [ADR 0029](0029-files-and-directories-with-fd.md)

## Context

Quick search and the filters of the lists (the location menu, the tab menu, the zoxide window,
the workspaces, the command history, the list of the key hints) match what is typed as fzf does
while `ui.fuzzy_search` is on ([ADR 0016](0016-fuzzy-search.md)), and as plain text anywhere
while it is off. The switch is global and lives in the Configuration dialog: to look for a name
the other way, the user goes there, changes it for every list, and comes back. Nothing on screen
says which way a search matches, so a search that finds nothing does not tell a missing name from
the wrong mode.

Neither way takes a regular expression, which the search of a tree takes with Alt+r
([ADR 0027](0027-search-of-a-whole-tree.md), [ADR 0029](0029-files-and-directories-with-fd.md)):
`^\d` for the names that start with a digit, `\.(jpe?g|png)$` for images.

## Decision

- **Three modes**: plain, the text anywhere, ignoring case; fzf, as ADR 0016 has it; and regex,
  a regular expression with smart case: case counts only once it has a capital letter that is not
  escaped, as `\S` is not.
- **`ui.search_mode`** (`"plain"`, `"fzf"`, `"regex"`; `"fzf"` by default) replaces
  `ui.fuzzy_search`. The old key is not read: it is an unknown key, as any other.
- **Alt+r switches the mode** of the search or the window it is pressed in, round: plain, fzf,
  regex. It is the search of a tree's key for regular expressions. The mode lasts until the search
  ends or the window closes; the next one starts in `ui.search_mode` again. Nothing is saved.
  `n` and `N` match the last search the way it matched.
- **A glyph after the word of the prompt** says which mode, as in `Filter ~:`:
  nf-cod-whole_word, nf-md-approximately_equal, and nf-cod-regex with icons; `=`, `~`, and `.*`
  without. While the text is no regular expression, the glyph is crossed out in the color of a
  broken link, nothing matches, and the lists say what is wrong with it, as the regex engine
  says, in place of the rows.
- **The search of a tree (`g /`) starts in the search mode too**, and its check box of regular
  expressions goes. It has no fzf mode, ripgrep and fd matching a pattern rather than ranking
  names, so there `fzf` is regex, which the check box was by default. Alt+r switches its text
  between plain and regex, for this dialog only, and the label of the field says which:
  `Text .*:`. The check box of case stays.
- **The zoxide window** asks zoxide for every directory and filters them itself in the fzf and
  regex modes, and gives zoxide the keywords in the plain mode, as before; Alt+r going to or
  from the plain mode asks zoxide again.
- **One engine for regular expressions**: the new `noc-pattern` crate builds them, with the
  `regex` crate, smart case, and the last line of its error, for the regex mode and for fd's
  search of names in `noc-tools`, which used to build them itself. A pattern matches the same
  names in both.
- **Quick search keeps the text as typed**: it no longer lowers it, so that a capital counts
  once the mode becomes fzf or regex. The plain mode ignores case as it matches.

## Consequences

- A search that finds nothing can be tried the other ways without leaving it.
- A `config.toml` with `ui.fuzzy_search` does not load until the key is removed or changed to
  `search_mode`; Noon Commander is pre-alpha, and the error names the key.
- `noc-tools` depends on `noc-pattern`, which depends on no other `noc-*` crate.
- Alt+r is one action everywhere, `search_mode`; in the search of a tree it goes round two modes
  rather than three. `toggle_regex` goes.
- The dialog of `g /` is a line shorter.
