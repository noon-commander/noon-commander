# 0082. Hosts are named by their alias

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0007](0007-typed-host-settings-in-hosts-toml.md), [ADR 0019](0019-shell-command-line.md), [ADR 0021](0021-location-menu-replaces-the-virtual-root.md), [ADR 0031](0031-who-and-where-on-screen.md), [ADR 0072](0072-the-command-history-shows-where-a-command-ran.md), [ADR 0075](0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md), [ADR 0081](0081-words-for-sizes-not-known-and-cards-with-two-ends.md)

## Context

A host could have a `label` in `hosts.toml`, set in the dialog of F4, shown in its place wherever the host was named: its row in the location menu, the prompt of the command line, the title of the terminal, the command history, and `noc debug hosts`. The ssh config names the host already, by the alias the user types to ssh; a second name hid the first, so that the card of the location menu had to show the alias besides it, and the filter had to find both.

The line over the card named, at its right end, the volume of the home directory and the type of its file system, `Macintosh HD · apfs`, where the card of a volume named the type and its kind, `apfs · system`: the same place told two ways. Over the card of a host it named the file and the line of its `Host` line, and the first line of the card started with the alias, which the row shows already.

## Decision

- **No labels.** `label` leaves the schema of `hosts.toml` and the dialog of F4. A host is named by its alias everywhere: its row in the location menu, the prompt, the title of the terminal, the command history, and `noc debug hosts`. The filter of the menu finds a host by its alias and its address.
- **A `label` in `hosts.toml` is an error**, as every key the schema does not know: Noon Commander says which file and which key, and starts once the line is gone. It does not take it out of the file itself.
- **The line over the card of a host** says its kind, `sftp`, as the line over the card of a volume says the kind of the volume.
- **The line over the card of the home directory** names its volume as the card of that volume does: the type of its file system and its kind, `apfs · system`, or its kind alone.
- **The first line of a host** loses the alias: its address and the host ssh jumps through, the skeleton, or `ssh -G failed` start it, after the icon; the file and the line of its `Host` line stand against its right end. The end takes what the start leaves, but at least half the line (`menu::ends`); the address loses its middle where it is too long, the file its start.

## Consequences

- A `hosts.toml` with a `label` stops Noon Commander until the user deletes the line.
- A host shows as the user knows it from ssh: what the prompt and the title say is what `ssh` takes.
- The dialog of F4 has three fields and a check box.
