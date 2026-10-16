# 0031. Who and where on screen

- Status: Accepted
- Date: 2026-10-16
- Amends: [ADR 0021](0021-location-menu-replaces-the-virtual-root.md)

## Context

A local panel shows a directory, `~/src`, and nothing says on which machine. Noon Commander run
on a server over ssh looks the same as on the desktop, and the name of the machine left the
terminal title with the virtual root ([ADR 0021](0021-location-menu-replaces-the-virtual-root.md)).
Nothing says either that the user is root: Noon Commander run with `sudo`, or a panel on a host
whose ssh user is `root`, copies and deletes as any other, and a mistake there costs more.

Shells answer both on every prompt: `alice@mac:~/src $`, and `#` for root. Starship, ranger, and
lf show the user and the host too, root in red; Far puts `(Administrator)` in its title.

## Decision

- **The machine** is the system's host name up to its first dot (`uname`), read once at start.
  **The user** is `root` while the effective uid is 0, whatever `$USER` says after `sudo` or `su`,
  else `$USER`, else `$LOGNAME`. Both are made terminal-safe; a part that is empty is left out.
- **On a host** the user is the one `ssh -G` gave when connecting, and the host is its label or
  alias, as the prompt already showed it. Until `ssh -G` answers, only the host shows. The real
  uid on the host is not asked for: that would run a command there on every connection, and a
  user called `root` is uid 0 in practice.
- **The prompt of the command line** is a shell's: `alice@mac:~/src $`, `deploy@web:/var/www $`,
  and `#` where the user is root. It takes a third of the width at most: the middle of the
  directory goes first, down to eight cells, then the user, then the directory, so that the host
  stays.
- **The line of the mode** shows `user@host` of the active panel left of the cells of what is
  typed, which stay reserved so that it does not move as keys are typed. It shows only while it
  fits after the mode or a message, which come first.
- **Root** shows in a theme role of its own, `root`, red and bold (bold alone in the `terminal`
  theme): `user@host` in the prompt and on the line of the mode where the user is root in the
  active panel. While Noon Commander itself runs as root, the line of the mode also shows a glyph
  before `user@host`, whichever panel is active: nf-md-shield_account with icons, `#` without.
- **The cursor of a panel where the user is root is on red**: a local panel while Noon Commander
  runs as root, a panel on a host whose ssh user is root. Roles `root_cursor` and
  `root_marked_cursor` take the place of `cursor` and `marked_cursor` there, the mark of a
  marked entry still underlined; the cursor of the inactive panel, the title, and quick search
  keep their colors. The `terminal` theme has no colors and makes that cursor bold.
  The terminal title starts with `[root]` while Noon Commander runs as root or the active panel is
  on a host as root.
- **Always on**, without a setting: the line has room for it, and the prompt was already the
  panel's place.

## Consequences

- Every screen says on which machine and as whom the active panel works. The terminal title
  marks root, but still does not name the machine; the line of the mode does.
- A local root is never out of sight, though the active panel is on a host as another user,
  where the panels can still copy into local directories with root's rights.
- `Done::Resolved` carries the user from `ssh -G`, and the app keeps it for the session.
- A host whose ssh user is not called `root` but has uid 0 does not show as root.
