# sftp-tui interface text, en-US.
#
# Each section below belongs to one module. Message IDs are kebab-case and start with the
# section's prefix; arguments are written { $name }.

## F-key bar (src/tui). Prefix: fkey-. Keep labels short, as mc does: a slot has about
## seven cells in an 80-column terminal.

fkey-quit = Quit
# Closes the connection to the host under the cursor.
fkey-disconnect = Disconn
# Closes a dialog.
fkey-cancel = Cancel

## Panels (src/tui/panel). Prefixes: panel-, error-.

# Column headers.
panel-name = Name
panel-size = Size
panel-time = Modify time
# The name column's title while the panel is sorted by extension.
panel-name-by-extension = Name, by extension
# The size column of the `..` row and of directories.
panel-up-dir = UP--DIR
panel-dir = DIR
# Status line during quick search: what was typed so far.
panel-search = Search: { $text }
# Status line while a directory is read.
panel-loading = Loading…
# Status line while the panel waits for a connection to a host; Esc stops it.
panel-connecting = Connecting to { $host }…
# Status line when a directory cannot be read; the panel keeps showing the previous one.
panel-error = Cannot open { $path }: { $reason }
error-not-found = no such file or directory
error-permission-denied = permission denied
# Status line when the connection of a host the panel showed ends; the panel goes back to the
# list of hosts.
panel-host-lost = Lost the connection to { $host }: { $reason }

## Connection errors (src/tui/describe). Prefix: error-. Most of the time ssh's own words are
## shown instead.

error-connection-closed = the connection is closed
error-ssh-exited = ssh ended with { $status }
error-ssh-spawn = cannot run { $program }: { $reason }
error-ssh-too-old = OpenSSH { $found } is too old; sftp-tui needs { $required } or newer
error-not-openssh = { $program } is not OpenSSH

## Virtual root (src/tui/panel). Prefix: root-.

# Title of the panel that lists the local file system and the hosts from ssh_config.
root-title = Hosts
# The row that opens the local file system.
root-local = [Local]
# Column header: user@hostname:port from ssh -G.
root-address = Address

## Dialogs (src/tui/dialog). Prefix: dialog-. Their text comes from ssh; these are the buttons.

dialog-ok = OK
dialog-cancel = Cancel
dialog-yes = Yes
dialog-no = No
