# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Workspaces: Alt-Shift-W saves the tabs of both panels under a name, with their sort orders
  and the rows under their cursors, and restoring one replaces every tab. Alt-W, or
  F9 → Workspace → Workspace list…, opens their window, which filters them; Enter restores,
  Insert saves the tabs as a new one, and F6 and F8 rename and delete them. F9 → Workspace
  lists them too. They are kept in `workspaces.toml` in the data directory
  (`~/.local/share/noc/`).
- Fuzzy search, as fzf does it: quick search, the filter of the location menu, and the zoxide
  window find `config.rs` from `cfg`, best match first, with fzf's `'`, `^`, `$`, and `!`.
  On by default; `ui.fuzzy_search = false`, or Fuzzy search in Options → Configuration,
  brings back literal matching.
- Shift-F6 (or F16, and F9 → File → Rename in place) renames the entry under the cursor in
  its row: the name is selected but for the last extension, Enter renames, and Esc keeps the
  name. A taken file name asks before it is replaced; a directory's is never replaced.

### Changed

- Release builds use full link-time optimization, which makes the `noc` binary about 6% smaller.

## [0.1.0] - 2026-10-03

First release.
