# 0098. Owners by name where known

- Status: Accepted
- Date: 2026-11-22

## Context

The status line shows the owner of the entry under the cursor in digits, `501:20`
([ADR 0096](0096-layouts-of-a-panel.md)), since names would need the host's own users and groups:
the local ones are wrong for a host. Names are what one reads, `alice:staff`. SFTP v3, which the
panels speak to hosts, gives only the numbers; its `longname` holds names as `ls -l` writes them,
but the protocol says not to parse it, and `openssh-sftp-client` drops it. rustix, which the code
uses for Unix APIs, has no lookup of users and groups, as it is the C library's, not a system
call's; `nix` has it, safe, and is already a dependency for the signal mask
([ADR 0054](0054-shell-run.md)).

## Decision

- **A name where it is known, else the number**, each of the two on its own, as `alice:20`. The
  numbers stay in digits alone, not grouped (ADR 0096).
- **Locally, every owner and group of a listing by name**: the task that lists a directory looks up
  the names of its uids and gids (`getpwuid_r` and `getgrgid_r` through `nix`'s `User` and
  `Group`) in `spawn_blocking`, and the listing comes with them. It waits for them no more than for
  the space of a volume, 500 ms, so that a directory service that does not answer does not hold it
  up; the listing then shows numbers.
- **On a host, the names of who the connection runs as**: `id -un` and `id -Gn`, at once with
  `id -u` and `id -G` ([ADR 0097](0097-the-mode-and-the-owner-set-apart-what-is-ours.md)), name its
  uid and its groups, the groups in the order of `id -G`; where they are not as many, as a name
  with a space in it splits in two, the groups keep their numbers. Other owners keep theirs.
- **Names are terminal-safe**, as every name from outside.
- **On the status line, an owner too long loses the ends of its names before it goes**: where the
  name of the entry would get less than its 8 cells, the names of the owner and of its group share
  what is left evenly, what one does not need going to the other, each cut at its end, down to 4
  cells each with the mark; with less room the owner goes, as any detail. A name is what tells
  who, and its start tells most of it.

## Consequences

- `nix` serves the names of users and groups besides the signal mask.
- A host shows its other owners in digits, `root` among them. `getent` would name them on Linux
  and BSD, not on macOS, at the cost of a command for each new number.
- A local listing looks up its names each time it is read, as names change rarely but may.
- A column of owners stays 11 cells: an owner by name longer than that loses its middle there.
