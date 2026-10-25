# 0047. Fields that search remember their way and case

- Status: Accepted
- Date: 2026-10-25
- Amends: [ADR 0030](0030-search-modes.md), [ADR 0044](0044-one-base-for-search-fields.md),
  [ADR 0046](0046-case-and-the-keys-of-search-fields.md)

## Context

Every field that searches started in `ui.search_mode` and smart case each time it opened, and lost
what Ctrl+s switched in it as it closed. Only the pattern of `g +` and `g -` kept its way and case,
and it started in a shell pattern whatever the setting said. Someone who searches the tab menu with
regular expressions had to switch to them each time; the setting named the way every field had,
though only as it opened.

The Files field of the search of a tree took globs that told case apart always, and showed no
glyphs: no field said how it took them, and no key changed it. ripgrep can ignore case in a glob
(`--iglob`); fd cannot in the globs it leaves out (`--exclude`).

## Decision

- **Each field keeps its way and its case until Noon Commander quits** (`search::Memory`, by
  `search::Slot`): quick search, one for `/` and `?` in both panels; the filter of each list apart
  (the location menu, the tab menu, the workspaces, the command history, the key hints, the zoxide
  window); the text of the search of a tree, one for both its modes; its globs; and the pattern of
  `g +` and `g -`, one for both. The next time a field opens, it opens as it was. Nothing is
  written to disk.
- **The App keeps them after each key and each press of the mouse** (`App::remember_searches`),
  so a field keeps what was switched in it however it closes, Esc too.
- **`ui.search_mode` is `ui.default_search_mode`**: the way a field opens in the first time, with
  smart case. A field without that way takes its own: the search of a tree a regular expression,
  `g +` a shell pattern; with `plain` or `regex`, `g +` starts in them. Changing the setting
  leaves the fields that have opened as they were. The old key is an error, as every unknown one.
- **The Files field searches, in one way, shell patterns**, with the glyphs of every field that
  searches (`search::Kind::FILES`). Its case is that of each glob: with smart case a glob with a
  capital letter tells case apart and others do not. ripgrep takes them as `--glob` or `--iglob`.
- **A kind of field says the cases it can take** (`Kind::cases`). By name, fd takes the globs, and
  the field has only case (`Kind::FILES_FD`): its glyph of case takes no click, and no key after
  Ctrl+s switches it; the case for ripgrep comes back in the text mode.
- **A glob that globset cannot read crosses out the glyph** of the Files field, as a text that is
  no regular expression does.

## Consequences

- A field that is switched stays so; the setting changes what a fresh session starts with.
- Quitting forgets every switch.
- `*.rs` now finds `MAIN.RS` in the text mode, as smart case does elsewhere; `*.RS` does not find
  `main.rs`.
- A `config.toml` with `search_mode` fails to load until the key is renamed.
