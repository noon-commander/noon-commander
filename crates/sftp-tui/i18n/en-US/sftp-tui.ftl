# sftp-tui interface text, en-US.
#
# Each section below belongs to one module. Message IDs are kebab-case and start with the
# section's prefix; arguments are written { $name }.

## F-key bar (src/tui). Prefix: fkey-. Keep labels short, as mc does: a slot has about
## seven cells in an 80-column terminal.

fkey-help = Help
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

## Help screen (src/tui/help). Prefix: help-. One line per key: say what it does, briefly.

help-title = Help
help-panels = Panels
help-root = Host list
help-quick-search = Quick search
help-dialogs = Dialogs and help
help-text-fields = Text fields
help-row-up = One row up
help-row-down = One row down
help-page-up = One page up
help-page-down = One page down
help-first-row = First row
help-last-row = Last row
help-enter = Open the directory or host under the cursor
help-parent = Parent directory; above /, the host list
help-switch-panel = The other panel
help-swap-panels = Swap the panels
help-other-open = Open the directory under the cursor in the other panel
help-other-sync = Show this directory in the other panel
help-reload = Read the directory again
help-stop = Stop loading or connecting
help-toggle-hidden = Show or hide names that start with a dot
help-sort-name = Sort by name; again: reverse
help-sort-extension = Sort by extension; again: reverse
help-sort-time = Sort by modification time, newest first; again: reverse
help-sort-size = Sort by size, largest first; again: reverse
help-quick-search-start = Quick search; again: the next match
help-help = This help
help-quit = Quit
help-redraw = Redraw the screen
help-disconnect = Disconnect the host under the cursor
help-search-back = Take back the last character
help-search-end = End the search
help-dialog-up = Previous button; in this help, one line up
help-dialog-down = Next button; in this help, one line down
help-dialog-left = Previous button
help-dialog-right = Next button
help-dialog-page-up = In this help, one page up
help-dialog-page-down = In this help, one page down
help-dialog-home = In this help, the top
help-dialog-end = In this help, the end
help-next-field = Next field or button
help-prev-field = Previous field or button
help-confirm = Press the button with the focus
help-dialog-cancel = Cancel, or close this help
help-field-home = Start of the text
help-field-end = End of the text
help-field-backspace = Delete the character before the cursor
help-field-delete = Delete the character at the cursor
help-field-delete-to-start = Delete to the start
help-field-delete-to-end = Delete to the end
help-note-esc = Esc 1 … Esc 0 stand for F1 … F10, and Esc followed by a key for Alt and the key, for terminals without them. A lone Esc acts after a second; Esc Esc at once.
help-note-typing = Typing in a panel starts quick search.
