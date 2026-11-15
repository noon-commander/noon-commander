# 0086. Groups of keys live in the preset

- Status: Accepted
- Date: 2026-11-16
- Amends: [ADR 0022](0022-vim-modes.md)

## Context

The list of the key hints (Ctrl+k twice) showed every key of what was in front as one long run of
rows: about ninety in the panels, the global keys among them. The help of F1 showed the keys of
the panels the same way. The preset had groups, but only in comments (`// Navigation`, `// Marks`,
`// Search`), and some keys stood apart from their kind: the pull-down menu among the motions,
`g +`, `g -`, and `g *` after the keys of long names.

The groups could come from a table beside the preset, `group(context, action)`, which the list
would sort by. That table and the preset would have to be kept in step by hand, and the rows of a
group could stand in another order in the list than in the help.

## Decision

- **The preset holds the groups.** A context binds its keys alone (`Bound::Keys`), or in groups
  (`Bound::Groups`), each a `keymap::Group` with its keys, in the order the groups show.
  `Keymap::group` says the group of an action in a context.
- **Groups where a context has many keys:** the panels in Normal mode (Navigation, Two panels,
  Tabs, Workspaces, Options and sorting, Marks and selection, Long names, Search, Command line, Go
  to, Files, Other), the panels in Visual mode, by the same names, the text fields in each mode
  (Motions, Changes, Modes), and the viewer (Navigation, Long lines, Files, Other). The other
  contexts have few keys, and no groups.
- **The list of the key hints shows each row under its heading:** the name of its group, or, for a
  context without groups, the name of the context, as Everywhere for `global`
  (`keymap::Heading`). The headings follow the chain, the context in front first. They go while
  the filter ranks the rows best first, as fzf does, which takes them out of their groups, as in
  the location menu, and while every row shown is under one heading, as after Ctrl+w.
- **The help shows the names of the groups over their keys** in the section of their context, in
  the color of titles, without the lines of the headings of the sections.
- The filter does not match the names of the groups: typing a word of one would find every row
  under it, and fzf would find more noise than help.
- The strip of which-key, after the first Ctrl+k, keeps its columns without headings: it has no
  room for them.

## Consequences

- A new key goes in a group of its context, where the help and the list show it; the tests check
  that a context binds each action once, and names each group once.
- Moving the pull-down menu to Other and the marks by pattern to Marks and selection changed the
  order of those keys in the help, in the strip of which-key, and in
  [keymap-compare.md](../keymap-compare.md).
