# 0079. ssh -G results are not cached

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0076](0076-ssh-g-for-the-host-under-the-cursor-of-the-location-menu.md), [ADR 0075](0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md), [ADR 0007](0007-typed-host-settings-in-hosts-toml.md)

## Context

`ssh -G` gives the address of a host, and it runs `Match exec` commands, so it runs only for a host the user is at. Its results were cached in `~/.cache/noc/resolve.json`, so that the location menu showed the addresses of earlier runs at once, in the rows of the hosts and in their cards.

Since [ADR 0076](0076-ssh-g-for-the-host-under-the-cursor-of-the-location-menu.md), `ssh -G` runs once a run for the host that the cursor of the menu rests on, cached or not; a cached address only stood in for a moment. It could be wrong all the same: a `Match exec` that answers otherwise now, or a file ssh reads that discovery does not see, changes it without changing the stamp the cache was valid for. The cache took a stamp of every config file and of the names in their directories, a file written after each `ssh -G`, and a lock to write it in turn.

## Decision

- **Nothing of `ssh -G` is kept across runs.** What it tells lives in the app for the run (`App::ssh_g`). `noc-ssh` loses `ResolveCache`, `CachedHost`, `ConfigStamp`, and `DiscoveryOptions::root_files`, which only the stamp used; `KnownHost` loses its address and `ProxyJump`.
- **The rows of hosts in the location menu show no address.** Without the cache it would fill in as the cursor visits hosts, and the column of names would move with it. The card shows it, a skeleton until `ssh -G` tells it, or why it failed.
- **The filter finds a host by the address `ssh -G` told in this run**, which the app passes to the menu as it comes (`LocationMenu::set_addresses`).
- **`noc debug hosts` runs `ssh -G` for every host each time**, four at a time, and prints what each says, or its error; `--resolve` is gone.
- **An old `resolve.json` is left alone**: nothing reads it, and it may be deleted. The cache directory stays in `Paths` and in `noc config paths`, for what may go there later.

## Consequences

- An address always shows as `ssh -G` says it now, after a moment in each run.
- Noon Commander writes nothing to `~/.cache/noc/`.
- An address typed in the filter finds a host only once `ssh -G` has told it in this run.
- `noc debug hosts` runs the `Match exec` commands of every host each time it runs.
