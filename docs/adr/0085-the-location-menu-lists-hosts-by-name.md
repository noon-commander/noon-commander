# 0085. The location menu lists hosts by name

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0021](0021-location-menu-replaces-the-virtual-root.md)

## Context

The location menu put the hosts used last first in SFTP, then the others in the order of the ssh config ([ADR 0021](0021-location-menu-replaces-the-virtual-root.md)): the order of their `Host` lines, with the files of `Include` read where they stand. That order belongs to how the config is written, by topic, by history, or by `Match` and defaults that must come before others, and says nothing to someone looking for a host among dozens. Every other list of names in Noon Commander, the panels and the workspaces, is in the order of the names.

## Decision

- **The hosts not used lately come by name**, after those used last, which keep the order of `recent.toml`, the most recent first.
- **Names compare as the panels compare them** (`panel::name_key`), as `ui.natural_sort` says: by their language, with the numbers in them by value, `h2` before `h10`; else by their characters, ignoring case. Names that compare the same go by their bytes.
- The menu sorts the hosts once, when a listing comes (`LocationMenu::listed`), not each time it draws.
- The volumes keep their order: the system volume, then the others by mount point.
- Discovery keeps the order of the config (`noc_ssh::Discovery::hosts`); what else takes the hosts from it, such as `noc debug hosts`, is unchanged.

## Consequences

- The first ten hosts shown, the names on the card of the row of the rest, and the rows the filter finds as good as each other follow the names.
- Where the hosts stand no longer follows how the ssh config is written; a user who ordered it so that the hosts would show so finds them by name, or by the filter.
