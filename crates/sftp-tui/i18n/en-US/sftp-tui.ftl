# sftp-tui interface text, en-US.
#
# Each section below belongs to one module. Message IDs are kebab-case and start with the
# section's prefix; arguments are written { $name }.

## F-key bar (src/tui). Prefix: fkey-. Keep labels short, as mc does: a slot has about
## seven cells in an 80-column terminal.

fkey-help = Help
fkey-view = View
fkey-edit = Edit
# Wraps long lines in the viewer, or cuts them.
fkey-wrap = Wrap
fkey-copy = Copy
fkey-move = RenMov
fkey-mkdir = Mkdir
fkey-delete = Delete
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
# Below the listing: the size of the marked files in bytes, with a comma between each group of
# three digits, and how many entries are marked.
panel-marked = { $size } B in { $count ->
        [one] { $count } file
       *[other] { $count } files
    }
# Status line when a directory cannot be read; the panel keeps showing the previous one.
panel-error = Cannot open { $path }: { $reason }
error-not-found = no such file or directory
error-permission-denied = permission denied
error-already-exists = already exists
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
# Buttons of a failed file operation, as in mc: leave this entry, leave every failing one
# without asking again, try again, stop.
dialog-skip = Ignore
dialog-skip-all = Ignore all
dialog-retry = Retry
dialog-abort = Abort
# Buttons of a taken name, as in mc: overwrite this and every later one, keep them all,
# overwrite those that are older.
dialog-all = All
dialog-none = None
dialog-older = Older
# Title of error dialogs.
dialog-error = Error

## Help screen (src/tui/help). Prefix: help-. One line per key: say what it does, briefly.

help-title = Help
help-panels = Panels
help-root = Host list
help-quick-search = Quick search
help-dialogs = Dialogs and help
help-text-fields = Text fields
help-viewer = Viewer
help-viewer-top = The start of the file
help-viewer-end = The end of the file
help-viewer-left = One column left, when lines are cut
help-viewer-right = One column right, when lines are cut
help-viewer-wrap = Wrap long lines, or cut them
help-viewer-quit = Close the viewer
help-row-up = One row up
help-row-down = One row down
help-page-up = One page up
help-page-down = One page down
help-first-row = First row
help-last-row = Last row
help-enter = Open the directory or host under the cursor
help-mark = Mark or unmark, then the next row
help-mark-up = Mark or unmark, then the row above
help-invert-marks = Invert the marks on files
help-select = Mark the names that match a pattern
help-unselect = Unmark the names that match a pattern
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
help-view = View the file under the cursor; on a directory, open it
help-edit = Edit the file under the cursor in $VISUAL or $EDITOR
help-copy = Copy the marked entries, or the one under the cursor
help-move = Move or rename the marked entries, or the one under the cursor
help-mkdir = Make a directory
help-delete = Delete the marked entries, or the one under the cursor
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
help-toggle = Switch the check box, or press the button
help-dialog-cancel = Cancel, or close this help
help-field-home = Start of the text
help-field-end = End of the text
help-field-backspace = Delete the character before the cursor
help-field-delete = Delete the character at the cursor
help-field-delete-to-start = Delete to the start
help-field-delete-to-end = Delete to the end
help-note-esc = Esc 1 … Esc 0 stand for F1 … F10, and Esc followed by a key for Alt and the key, for terminals without them. A lone Esc acts after a second; Esc Esc at once.
help-note-typing = Typing in a panel starts quick search.

## Marking by pattern, + and - (src/tui/app). Prefix: pattern-. As in mc.

# Dialog titles.
pattern-select = Select
pattern-unselect = Unselect
# Check boxes: leave directories alone; tell upper and lower case apart.
pattern-files-only = Files only
pattern-case-sensitive = Case sensitive

## Making directories, F7 (src/tui/app). Prefix: mkdir-. As in mc.

mkdir-title = Create a new directory
# Above the name field; the name under the cursor is filled in.
mkdir-prompt = Enter directory name:
mkdir-error = Cannot create directory { $path }: { $reason }

## The window of a running job (src/tui/progress). Prefix: job-.

# While a job counts what it has to do.
job-scanning = Counting…
job-found = { $items } found
# Entries done or skipped, of all.
job-count = { $done } of { $total }
# The same, for jobs that move data: the sizes are shown like those in panels.
job-count-bytes = { $done } of { $total }, { $bytes_done } of { $bytes_total } bytes
job-aborting = Aborting…
# The button that sends the job behind the panels, where it goes on.
job-background = Background
# At the top right while jobs run in the background: how many, and how far they are together.
jobs-running = { $count ->
        [one] { $count } job
       *[other] { $count } jobs
    } { $percent }%
# F10 while jobs run.
quit-title = Quit
quit-jobs = { $count ->
        [one] A job is still running. Quit and stop it?
       *[other] { $count } jobs are still running. Quit and stop them?
    }

## Deleting, F8 (src/tui/app). Prefix: delete-. As in mc.

delete-title = Delete
delete-file = Delete file "{ $name }"?
delete-directory = Delete directory "{ $name }" and everything in it?
delete-many = Delete { $count } files and directories?
# Above the entry the job works on.
delete-deleting = Deleting
delete-error = Cannot delete { $path }: { $reason }

## Copying, F5 (src/tui/app). Prefix: copy-. As in mc.

copy-title = Copy
# Above the target field, which opens with the other panel's directory.
copy-one = Copy "{ $name }" to:
copy-many = Copy { $count } files and directories to:
copy-preserve = Preserve attributes
# Above the entry the job works on.
copy-copying = Copying
copy-error = Cannot copy to { $path }: { $reason }
# A taken name: what is to be copied, what is there, and the question.
copy-exists-title = File exists
copy-exists =
    { $path } is there already.
    New:      { $new_size } bytes, { $new_time }
    Existing: { $old_size } bytes, { $old_time }
    Overwrite it?

## Moving, F6 (src/tui/app). Prefix: move-. As in mc; a new name in the field renames.

move-title = Move
move-one = Move "{ $name }" to:
move-many = Move { $count } files and directories to:
move-moving = Moving
move-error = Cannot move to { $path }: { $reason }

## Why a copy or a move cannot start (src/tui/app). Prefix: transfer-.

transfer-same = the source and the target are the same
# The target is in a source directory.
transfer-into-itself = it is in { $path }

## The viewer, F3 (src/tui/viewer). Prefix: viewer-. As in mc.

# Right of the title: the first line on screen, the lines, and how far the last one on screen
# is.
viewer-position = { $line }/{ $lines } { $percent }%
# The same, when only the start of a long file was read.
viewer-truncated = { $position } of the first 16 MiB
viewer-error = Cannot view { $path }: { $reason }

## Editing, F4 (src/tui). Prefix: edit-. The editor of $VISUAL or $EDITOR, or vi.

edit-error = Cannot edit { $path }: { $reason }
# The edited copy of a remote file could not go back; it stays where the user can find it.
edit-kept = The changes to { $path } did not go back; they are in { $copy }
# Why the editor did not run.
edit-cannot-run = cannot run { $program }: { $reason }
