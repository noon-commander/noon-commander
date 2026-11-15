# 0076. ssh -G for the host under the cursor of the location menu

- Status: Accepted; amended by [ADR 0079](0079-ssh-g-results-are-not-cached.md),
  [ADR 0081](0081-words-for-sizes-not-known-and-cards-with-two-ends.md)
- Date: 2026-11-15
- Amends: [ADR 0075](0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md)

## Context

The card of a host in the location menu
([ADR 0075](0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md)) says how to
reach it: its address and the host ssh jumps through. Only `ssh -G` knows them, and it ran only
when a host was connected to, so a host never connected to said `address not known yet`, and the
cache of an earlier run could be out of date. A failure of `ssh -G`, such as a typo in the ssh
config, went to the log alone.

`ssh -G` evaluates the whole config, `Match exec` too, which runs commands. It must not run for
every host at once, as at startup, but may for the host the user is at.

## Decision

- **It runs once the cursor of the location menu rests on a host for 150 ms**, whichever way it
  got there, keys, the mouse, or the menu opening on it (`App::take_resolve`, asked between events;
  `Tasks::resolve`, which waits). Moving over hosts runs it for none of them: a newer request, or
  the cursor leaving the hosts, or the menu closing, stops the one that waits to start.
- **Once a run for each host**. A host that this run resolved, by the menu or by connecting, is not
  asked again. An address cached by an earlier run stood in until then; there is no cache since
  [ADR 0079](0079-ssh-g-results-are-not-cached.md).
- **One that started runs to its end**, as the user chose, though the cursor leaves the host or the
  menu closes; the app keeps its result for the run, as a connection's, within `ssh -G`'s own
  timeout of 15 s.
- **Until it tells the address**, the card shows a skeleton in place of the address, after the
  alias, which a shine crosses, from the moment the cursor is on the host. A first version wrote
  `address not known yet` there until `ssh -G` started, 150 ms later, and it flickered as the
  cursor moved: `ssh -G` runs for every host the cursor rests on, so there is no host whose
  address it will not tell, or fail to. The row shows no address
  ([ADR 0079](0079-ssh-g-results-are-not-cached.md)).
- **A failure shows in the card**, in place of the address, in the colors of errors:
  `ssh -G failed: reason`, on one line. It does not run again for that host in this run, until R in
  the menu lists the places again, which lets it run again for every host it failed for.
- The address and the user that `ssh -G` gives come with the `ProxyJump` too (`Done::Resolved`),
  which the card shows; a connection's `ssh -G` reports its failure the same way
  (`Done::NotResolved`).

## Consequences

- The card of a host says how to reach it after a moment, without connecting, and says when the
  ssh config is wrong for it.
- `ssh -G`, and with it a `Match exec`, runs for hosts the user only looks at in the menu, once a
  run each, at most one starting at a time, as AGENTS.md allows (on selection).
- A host connected to while the menu's `ssh -G` runs for it can be resolved twice; both answers are
  the same.
