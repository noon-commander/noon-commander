# sftp-tui interface text, en-US.
#
# Each section below belongs to one module. Message IDs are kebab-case and start with the
# section's prefix; arguments are written { $name }.

## F-key bar (src/tui). Prefix: fkey-.

fkey-quit = Quit

## Panels (src/tui/panel). Prefixes: panel-, error-.

# Column headers.
panel-name = Name
panel-size = Size
panel-time = Modify time
# The size column of the `..` row and of directories.
panel-up-dir = UP--DIR
panel-dir = DIR
# Status line while a directory is read.
panel-loading = Loading…
# Status line when a directory cannot be read; the panel keeps showing the previous one.
panel-error = Cannot open { $path }: { $reason }
error-not-found = no such file or directory
error-permission-denied = permission denied

## Virtual root (src/tui/panel). Prefix: root-.

# Title of the panel that lists the local file system and the hosts from ssh_config.
root-title = Hosts
# The row that opens the local file system.
root-local = [Local]
# Column header: user@hostname:port from ssh -G.
root-address = Address
