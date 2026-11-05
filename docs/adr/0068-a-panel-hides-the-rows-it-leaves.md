# 0068. A panel hides the rows it leaves

- Status: Accepted
- Date: 2026-11-05
- Amends: [ADR 0037](0037-spinners.md)

## Context

A panel that read another directory, or connected to a host to open one, kept drawing the rows
of the directory it left until the new ones came, as ADR 0037 had it: the title showed the path it
read, and the status line kept the name under the cursor. Over a slow host that lasted seconds,
and the rows read as those of the path in the title. They still took keys: the cursor moved over
them, Space marked them, and F8 deleted the entries marked there, out of the place the title named.

## Decision

- **A panel is veiled while it reads another location, or its first** (`Panel::veiled`): where
  the request goes elsewhere than the location shown, or before any listing came. Reading the same
  directory again, after work in it, Retry, or a host that is back after a lost connection, keeps
  the rows in sight: they are still those of the path in the title, and the toned-down rows of a
  host not connected stay as [ADR 0039](0039-errors-of-panels-in-dialogs.md) has them.
- **A skeleton stands for its rows once the wait shows**, 150 ms after it starts, with the dots in
  the title: three rows of faint bars in the panel's colors under the column titles, a bar in place
  of the status line, and no totals on the frame or the separator. The shine crosses them as it
  crosses the skeleton of a list, and rests, as the dots do, while a host's dialog asks whether to
  try again. Three rows, not the height of the panel: more looked busier than what they stand for,
  and the directory to come may hold none. Before 150 ms the old rows stay, so a quick listing
  shows no wait and nothing flickers.
- **Its rows take nothing meanwhile**, from the start of the wait, not from when the skeleton
  shows: the keys that need entries do nothing (`App::holds_back`), nor do clicks, the wheel, or
  the scroll bar on the rows, and the panel offers no entry to the commands that act on them.
  The keys that need no entries work as ever (`works_unconnected`), and so do sorting, which holds
  for the rows to come, and the command line, whose prompt says where it runs.
- **A listing that fails shows the rows again**, with the cursor and the marks where they were,
  as the panel keeps what it showed (ADR 0039): it kept them all along, only out of sight.

## Consequences

- No key acts on rows that are not on screen, nor on those of another place than the title's.
- A key typed in the first 150 ms of a wait does nothing, though the old rows still show.
- The status line under the rows no longer keeps the name under the cursor while the panel reads
  another location, as ADR 0037 had it.
