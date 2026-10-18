# 0039. Errors of panels in dialogs

- Status: Accepted
- Date: 2026-10-18
- Amends: [ADR 0021](0021-location-menu-replaces-the-virtual-root.md),
  [ADR 0037](0037-spinners.md)

## Context

The status line under a panel's rows told the name under the cursor, and, over it, three things
that went wrong:

- `Cannot open dir: reason`, after a directory could not be read. The panel stayed where it was,
  and the line said so until the next listing. It offered no way out, and a directory that was
  gone under the panel left it there, empty, with nothing to do but find a way up by hand.
- `web: Retry or Cancel?`, while the Connection dialog asked the same.
- `Not connected to web; R connects`, in a panel left on a host after Cancel or a disconnection.
  Every other key did nothing there, without a word.

A line at the bottom is easy to miss, and once seen it does not say what to do. mc and Far
report a directory they cannot enter in a dialog, and go up without asking when the directory of
a panel is gone.

## Decision

- **A directory that cannot be read opens a dialog**, Error, `Cannot open dir: reason`, with
  OK, Retry, Up, and Go to. The panel keeps what it showed. OK stays; Retry reads the directory
  again; Up goes to its nearest parent, on the directory it came from, and on up past every
  parent that is not there either, without asking, until one can be read or fails otherwise;
  Go to opens Quick cd with the path typed, `host:/path` on a host, to mend it.
- **Up is there only where it leads elsewhere**: not from `/` or a host's home, nor when the
  parent is where the panel already is.
- **The default follows the error.** A directory that is not there: Up when it was the panel's
  own (gone under it, or the first of a new tab or of the start), else OK. Not allowed: OK.
  Anything else, as a dropped connection: Retry. The listings carry the kind with their reason
  (`ListError`, `ListErrorKind`), from `VfsError`.
- **One such dialog per panel.** A failure replaces the dialog of the one before in the same
  panel, so listings read again after work do not pile them up.
- **Unlike mc, a panel whose directory is gone asks** rather than going up by itself, with Up as
  the default: Enter does what mc does, and the panel does not move without a word.
- **No status line for hosts.** While the Connection dialog asks, the status line of a waiting
  panel shows the name under the cursor, as any panel; the dots in its title show the wait.
  Cancel in it leaves no second dialog: the panels that were to open the host stay where they
  were, and those on it stay there without entries.
- **A key in a panel on a host that is not connected asks**: Connection, `Not connected to web`,
  with Connect, the default, which reads the directory again, Local, which goes to the last
  local directory of the panel's jump list or else the home directory, and Cancel. The keys that
  do not touch the panel's entries work as ever: other panels and tabs, menus, help, the jobs,
  Quit, Quick cd, the jump list, workspaces, hidden files. `R` connects without asking. The
  status line of such a panel is empty. F8 and Disconnect still ask nothing.

This amends ADR 0021, whose panels on a host said the reason and how to connect in their status
line, and ADR 0037, whose status line asked `web: Retry or Cancel?`.

## Consequences

- Errors of listings are seen, and each comes with its ways out; a vanished directory is one
  Enter from a directory that is there.
- A failure costs a key to close, which the status line did not. Listings read again in the
  background can open a dialog; there is one per panel at most.
- The status line only describes the entry under the cursor, the rename, or quick search.
- Typed errors reach the panel, so other kinds can choose their own buttons later.
