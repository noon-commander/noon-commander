# Noon Commander interface text, en-US.
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
# Renames a workspace.
fkey-rename = Rename
fkey-mkdir = Mkdir
fkey-delete = Delete
fkey-quit = Quit
# Closes the connection to the host under the cursor.
fkey-disconnect = Disconn
# Edits the settings of the host under the cursor.
fkey-edit-host = Edit
# Closes a dialog.
fkey-cancel = Cancel
# Opens the pull-down menu.
fkey-pulldown = PullDn

## The title of the terminal's window or tab (src/tui/app), with ui.terminal_title. Prefix:
## terminal-.

# Where the active panel is: a directory with ~ for home, host:path, or the name of the machine
# in the root. noc is the program's name, not to be translated.
terminal-title = { $place } — noc

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
# Status line while the entry under the cursor is renamed in its row: its name until now.
panel-rename = Rename: { $name }
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
# On the bottom of a panel's frame, as in mc: the free space and size of the file system that
# holds the directory, such as 212G / 460G, and the share that is free.
panel-space = { $free } / { $total } ({ $percent }%)
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
error-ssh-too-old = OpenSSH { $found } is too old; Noon Commander needs { $required } or newer
error-not-openssh = { $program } is not OpenSSH

## Virtual root (src/tui/panel). Prefix: root-.

# Title of the panel that lists the mounted volumes and the hosts, where the name of the machine
# is not known.
root-title = Locations
# The first row of the virtual root, which opens the home directory.
root-home = Home
# The row of the virtual root that opens the list of hosts from ssh_config, and that list's title.
root-sftp = SFTP
# Next to that row: how many hosts it holds.
root-sftp-hosts = { $count ->
        [one] { $count } host
       *[other] { $count } hosts
    }
# Status line on that row.
root-sftp-status = Hosts from the ssh config
# Column header: the space left on a volume; its size is under panel-size.
root-free = Free
# Column header: user@hostname:port from ssh -G.
root-address = Address

## The location menu of Alt+F1 and Alt+F2 (src/tui/menu), as Far Manager's menu to change
## drives. Prefix: menu-.

# Titles: the panel the menu changes.
menu-left = Left
menu-right = Right
# The first line: what was typed to filter the menu.
menu-filter = Filter: { $text }
# When the filter leaves no row.
menu-nothing = Nothing matches.

## The zoxide window of Alt+z (src/tui/jump): the directories zoxide ranks highest for the
## keywords typed, as `z` picks them in a shell. Prefix: jump-.

jump-title = zoxide
# The first line: the keywords typed, separated by spaces.
jump-keywords = Jump to: { $text }
# When zoxide lists no directory.
jump-nothing = zoxide knows no directory that matches. It learns the directories you work in.
# The zoxide program is not there.
jump-not-installed = zoxide is not installed: cannot run { $program }. Get it from https://github.com/ajeetdsouza/zoxide, or set its path in Options → Configuration.

## Tabs (src/tui/tabs, src/tui/app). Prefix: tabs-. Each side has tabs of its own.

# Title of the list of a panel's tabs, Ctrl+x Tab; each row is the tab's number and where it is.
tabs-title = Tabs

## Workspaces (src/tui/workspaces, src/tui/app): the tabs of both panels, saved under a name in
## workspaces.toml. Prefix: workspaces-.

# The window of Alt+w, F9 → Workspace → Workspace list….
workspaces-title = Workspaces
# The first line: what was typed to filter the window.
workspaces-filter = Filter: { $text }
# When no workspace is saved yet.
workspaces-none = No workspace is saved yet. Insert here, or Alt+W in a panel, saves the tabs of both panels under a name.
# When the filter leaves no row.
workspaces-nothing = Nothing matches.
# Right of a workspace: how many tabs it has, in both panels together.
workspaces-tabs = { $count ->
        [one] 1 tab
       *[other] { $count } tabs
    }
# The dialog of Alt+W, and of Insert in the window.
workspaces-save-title = Save workspace
workspaces-save-prompt = Save the tabs of both panels as:
# The name of an existing workspace was typed.
workspaces-replace-title = Replace workspace
workspaces-replace = Workspace "{ $name }" exists. Replace it with the tabs of both panels?
# F6 in the window.
workspaces-rename-title = Rename workspace
workspaces-rename-prompt = New name for "{ $name }":
# The new name is that of another workspace.
workspaces-rename-replace = Workspace "{ $name }" exists. Replace it?
# F8 in the window.
workspaces-delete-title = Delete workspace
workspaces-delete = Delete workspace "{ $name }"? Its tabs stay as they are.
# A workspace chosen in the menu was removed meanwhile, by another Noon Commander.
workspaces-gone = Workspace "{ $name }" is not saved any more.
# The reason names the file.
workspaces-save-error = Cannot save the workspaces: { $reason }
workspaces-load-error = Cannot read the workspaces: { $reason }

## The pull-down menu of F9 (src/tui/pulldown), as mc's. Prefix: pulldown-. An & marks the
## letter that opens a menu from the bar, or runs a command while its menu is open; letters
## must differ on the bar and within a menu, and && stands for &.

# The menu bar. Left and Right act on the panel drawn on that side.
pulldown-left = &Left
pulldown-file = &File
pulldown-command = &Command
pulldown-options = &Options
pulldown-workspace = &Workspace
pulldown-right = &Right
# Left and Right.
pulldown-location = Change &location…
pulldown-sort-name = Sort by &name
pulldown-sort-extension = Sort by &extension
pulldown-sort-time = Sort by &time
pulldown-sort-size = Sort by si&ze
pulldown-rescan = &Rescan
# Closes the connection of the host the panel shows.
pulldown-disconnect-panel = &Disconnect
# Tabs of the panel.
pulldown-new-tab = Ne&w tab
pulldown-close-tab = &Close tab
pulldown-tab-list = Tab l&ist…
# File.
pulldown-view = &View
pulldown-edit = &Edit
pulldown-copy = &Copy
pulldown-move = &Rename/Move
# Shift+F6: the new name is typed in the entry's row.
pulldown-rename = Re&name in place
pulldown-mkdir = &Mkdir
pulldown-delete = &Delete
pulldown-select = &Select group…
pulldown-unselect = &Unselect group…
pulldown-invert = &Invert selection
pulldown-checksum = Chec&ksums…
pulldown-exit = E&xit
# Command.
pulldown-quick-search = &Quick search
pulldown-swap = S&wap panels
pulldown-other-open = &Open in the other panel
pulldown-other-sync = &This directory in the other panel
pulldown-quick-cd = Quick &cd…
# Opens the zoxide window.
pulldown-jump = &Jump to a directory (zoxide)…
pulldown-jobs = &Background jobs…
pulldown-edit-host = Edit &host settings…
pulldown-disconnect-host = &Disconnect host
pulldown-help = He&lp
pulldown-redraw = Red&raw screen
# Options.
pulldown-configuration = &Configuration…
pulldown-hidden = Show &hidden files
# Workspace. Below these come the saved workspaces, which restore; the first ten have the
# digits 1 … 9 and 0 as their letters, so these two must not use a digit.
pulldown-save-workspace = &Save workspace…
# Opens the window of the saved workspaces, to restore, rename, or delete them.
pulldown-workspace-list = Workspace &list…

## The Configuration dialog, Options → Configuration… (src/tui/configuration): the settings of
## config.toml by category. Prefix: config-. Each setting has a name and a hint, a line shown
## below the settings while it has the cursor.

config-title = Configuration
# Categories.
config-interface = Interface
# Settings of the interface, [ui].
config-language = Language
config-language-hint = A language tag, such as en-US, or auto for the system locale.
config-theme = Theme
config-theme-hint = Colors: mc-classic, as mc; terminal, the terminal's own; noon, from the logo, and catppuccin, each dark or light.
config-keymap = Keymap
config-keymap-hint = Keys: default, modelled on mc, or vim.
config-borders = Borders
config-borders-hint = The frames of panels and dialogs.
config-borders-double = Double ═ ║ ╔
config-borders-single = Single ─ │ ┌
config-icons = Icons
config-icons-hint = Nerd Font icons in front of names; without them, mc's markers.
config-show-hidden = Show hidden files
config-show-hidden-hint = Names that begin with a dot; Alt+. switches them.
config-fuzzy-search = Fuzzy search
config-fuzzy-search-hint = Quick search, the location menu, and the zoxide and Workspaces windows match as fzf does: the letters typed in order, best matches first.
config-mouse = Mouse
config-mouse-hint = Clicks move the cursor, open, and mark, and the wheel scrolls. The terminal then selects text with Shift held, or Option in iTerm2.
config-wheel = Mouse wheel
config-wheel-hint = How far a step of the wheel scrolls: a number of lines, or page.
config-wheel-invalid = "{ $text }" is not a step of the wheel: it takes a number of lines from 1 to { $most }, or page.
config-terminal-title = Terminal title
config-terminal-title-hint = The title of the terminal's window or tab shows the active panel's directory.
config-menu-bar = Menu bar
config-menu-bar-hint = When the menu bar of F9 shows.
config-menu-bar-on-demand = While a menu is open
config-menu-bar-always = Always
config-tab-bar = Tab bar
config-tab-bar-hint = Where a panel with more than one tab shows them.
config-tab-bar-line = A line above the panel
config-tab-bar-frame = In the panel's frame
# Settings of copying and moving, [transfer].
config-transfers = Transfers
config-atomic-upload = Atomic copies
config-atomic-upload-hint = Write each copy under a hidden name and rename it when complete, so the target never holds part of a file.
config-parallel-jobs = Parallel jobs
config-parallel-jobs-hint = How many jobs run at once; later ones wait. Editing with F4 never waits.
config-parallel-jobs-invalid = "{ $text }" is not a number of jobs: it takes a whole number, at least 1.
# Settings of ssh, [ssh], and of the list of hosts, [discovery]. Lists are words separated by
# spaces, with "…" around a word that has spaces.
config-ssh = SSH
config-ssh-program = Program
config-ssh-program-hint = The OpenSSH client, 8.7 or later: a name in PATH, or a path. Applies to new connections.
config-ssh-program-empty = The ssh program cannot be empty.
config-ssh-config-file = Config file
config-ssh-config-file-hint = Read this file instead of ~/.ssh/config, like ssh -F; empty: the default. Applies to new connections.
config-ssh-args = Extra arguments
config-ssh-args-hint = For every ssh run, such as -o ServerAliveInterval=15. Forwarding is refused. Applies to new connections.
config-ssh-args-invalid = Invalid extra ssh arguments: { $reason }
config-multiplex = Share connections
config-multiplex-hint = One authenticated connection per host for panels and transfers; off for servers with MaxSessions 1.
config-hide-hosts = Hidden hosts
config-hide-hosts-hint = Hosts from ssh_config to leave out, as patterns with * and ?, separated by spaces.
# Settings of the virtual root, [volumes].
config-volumes = Volumes
config-hide-volumes = Hidden volumes
config-hide-volumes-hint = Mount points to leave out of the root, as patterns with * and ?, separated by spaces. The system volume always shows.
# Settings of zoxide, [zoxide].
config-zoxide = zoxide
config-zoxide-record = Record directories
config-zoxide-record-hint = Add a local directory to zoxide once you copy, delete, view, or edit something there; passing through does not count.
config-zoxide-program = Program
config-zoxide-program-hint = The zoxide program: a name in PATH, or a path. Alt+z jumps to the directories it ranks.
config-zoxide-program-empty = The zoxide program cannot be empty.
config-shell = Command line
config-pause = Wait after a command
config-pause-hint = When the output of a command stays on screen until a key; Ctrl+o shows it again either way.
config-pause-always = Always
config-pause-on-error = When it fails
config-pause-never = Never
config-history-size = History size
config-history-size-hint = How many commands of ! and : the history keeps, of all hosts together; 0 keeps none.
config-history-size-invalid = "{ $text }" is not a number of commands: it takes a whole number, 0 or more.
# After the hint of a setting that the running Noon Commander cannot change.
config-restart = Takes effect after a restart.
# The reason names the file.
config-save-error = Cannot save the configuration: { $reason }
# The language field holds something else.
config-language-invalid = "{ $text }" is not auto or a language tag such as en-US.

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
# Fills a field of a form with what a panel shows, such as the directory open on a host.
dialog-use-current = Use current

## Help screen (src/tui/help). Prefix: help-. One line per key: say what it does, briefly.

help-title = Help
help-panels = Panels
help-root = Volumes and hosts
help-menu = Location menu
help-jump = zoxide
help-workspaces = Workspaces
help-history = Command history
help-history-hosts = The commands of the panel's host, or of all hosts
help-history-take = Put the command on the command line, without running it
help-history-delete = Remove the command from the history
help-pulldown = Pull-down menu
help-quick-search = Quick search
help-renaming = Renaming in place
help-command-line = Command line
help-dialogs = Dialogs and help
help-text-fields = Text fields
help-path-fields = Path fields
help-completion = Completion list
help-complete = Complete the path; again: list the choices
help-complete-next = The next choice, round
help-complete-take = Put the choice in the field
help-complete-close = Close the list; other keys close it and edit the field
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
help-parent = Parent directory; above /, the volumes and hosts
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
help-shell = Open the command line for a shell command, run in this directory
help-command = Open the command line for a command of Noon Commander; :!command runs a shell command
help-view = View the file under the cursor; on a directory, open it
help-edit = Edit the file under the cursor in $VISUAL or $EDITOR
help-copy = Copy the marked entries, or the one under the cursor
help-move = Move or rename the marked entries, or the one under the cursor
help-rename = Rename the entry under the cursor in its row
help-mkdir = Make a directory
help-delete = Delete the marked entries, or the one under the cursor
help-jobs = The running jobs: bring one to the front, or abort it
help-checksum = Checksums of the marked files, or the one under the cursor
help-menu-left = Change the left panel's location: a volume or a host
help-menu-right = Change the right panel's location: a volume or a host
help-quick-cd = Quick cd: type a path as for cd, with ~, .., -, or host:path
help-jump-open = Jump to a directory that zoxide ranks, in this panel
help-jump-go = Open the directory in the active panel
help-new-tab = A new tab in this panel, on the same directory
help-close-tab = Close this tab; the last one stays
help-next-tab = The next tab in this panel
help-prev-tab = The previous tab in this panel
help-tab-list = The tabs of this panel, to choose one
help-save-workspace = Save the tabs of both panels as a workspace
help-workspaces-open = The saved workspaces: restore, rename, or delete one
help-workspaces-save = Save the tabs of both panels as a new workspace
help-workspaces-restore = Replace the tabs of both panels with the workspace's
help-workspaces-rename = Rename the workspace
help-workspaces-delete = Delete the workspace
help-workspaces-close = Close the window
help-help = This help
help-quit = Quit
help-redraw = Redraw the screen
help-disconnect = Disconnect the host under the cursor
help-edit-host = Edit the settings of the host under the cursor
help-menu-open = Open the volume or host in the panel
help-menu-back = Take back the last character of the filter
help-menu-reload = Read the volumes and hosts again
help-menu-close = Close the menu
help-pulldown-open = The pull-down menu: Left, File, Command, Options, Workspace, Right
help-pulldown-up = The command above
help-pulldown-down = The command below
help-pulldown-left = The menu to the left
help-pulldown-right = The menu to the right
help-pulldown-home = The first command
help-pulldown-end = The last command
help-pulldown-run = Open the menu, or run the command
help-pulldown-close = Close the menu, then the menu bar
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
help-field-left = One character left
help-field-right = One character right
help-rename-confirm = Rename to the name typed
help-rename-cancel = Keep the name as it was
help-command-up = The line above; from the first line, the command before in the history of the panel's host
help-command-down = The line below; from the last line, the command after in the history
help-command-older = The command before in the history of the panel's host
help-command-newer = The command after in the history; after the last, what was typed
help-user-screen = The output of commands, in place of the panels; Ctrl+o or Esc brings them back
help-command-history = The command history: filter it, and take a command of this host or another
help-command-home = Start of the line
help-command-end = End of the line
help-command-delete-to-start = Delete to the start of the line
help-command-delete-to-end = Delete to the end of the line
help-command-backspace = Delete the character before the cursor
help-command-new-line = A new line in the command; Shift+Enter where the terminal tells it from Enter
help-command-edit = Edit the command in $VISUAL or $EDITOR; it comes back without running
help-command-run = Run the command; after a \ at the end of the line, a new line
help-command-close = Close the command line
help-note-esc = Esc 1 … Esc 0 stand for F1 … F10, and Esc followed by a key for Alt and the key, for terminals without them. A lone Esc acts after a second; Esc Esc at once.
help-note-menu = Typing in the location menu filters it; 1 … 9 and 0 open the first ten rows while the filter is empty.
help-note-jump = Typing in the zoxide window gives it keywords, as z does in a shell; 1 … 9 and 0 open the first ten rows while there are none.
help-note-jump-fuzzy = Typing in the zoxide window filters its directories; 1 … 9 and 0 open the first ten rows while nothing is typed.
help-note-fuzzy = Quick search, the location menu, the zoxide window, and the Workspaces window match as fzf does: the letters typed in order, not necessarily together, best match first. Words separated by spaces must all match; 'word matches as it is, ^word at the start, word$ at the end, and !word where it is not.
help-note-workspaces = A workspace holds the tabs of both panels: where each is, its sort order, and the row under its cursor. Alt+w and F9 → Workspace list them; restoring one replaces every tab.
help-note-command = ! opens the command line, and : too, for commands of Noon Commander, of which :!command is the only one so far. A command runs with the terminal, in the panel's directory, then a key brings the panels back; cd and export last only as long as the command.
help-note-pulldown = In the pull-down menu, the highlighted letter of a menu opens it, and that of a command runs it. The menu opens again where it closed.

## Host settings, F4 on a host (src/tui/app), saved to hosts.toml. Prefix: host-.

# Title: the host alias.
host-edit-title = Host { $host }
# Field labels.
host-label = Label (shown instead of the alias):
host-start-dir = Remote directory (opened on connect; empty: home):
host-other-dir = Other panel directory (local, / or ~/…; empty: unchanged):
host-remember-dir = Remember the last directory in this session
# The reason names the file.
host-save-error = Cannot save the host settings: { $reason }
# The other panel directory must be a local path.
host-other-dir-invalid = the other panel directory must start with / or ~/

## Marking by pattern, + and - (src/tui/app). Prefix: pattern-. As in mc.

# Dialog titles.
pattern-select = Select
pattern-unselect = Unselect
# Check boxes: leave directories alone; tell upper and lower case apart.
pattern-files-only = Files only
pattern-case-sensitive = Case sensitive

## Quick cd, Alt+c (src/tui/app, src/tui/cd). Prefix: cd-. As in mc.

cd-title = Quick cd
# Above the field: a path as cd takes it in a shell.
cd-prompt = cd

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
# The time a job has worked, without the time it waited for answers: M:SS or H:MM:SS.
job-elapsed = Time { $elapsed }
# The same, for jobs that move data, with the time left and the average speed; each is shown
# as -:-- or - until it is known.
job-timing = Time { $elapsed }   ETA { $left }   { $speed }
# An average speed: the size is shown like those in panels, such as 1.5M.
job-speed = { $size }/s
# While a job waits for others to finish, as transfer.parallel_jobs allows.
job-waiting = Waiting for other jobs to finish…

## The list of jobs, Ctrl+x j (src/tui/jobs), as mc's Background jobs. Prefix: jobs-.

jobs-title = Jobs
jobs-none = No jobs are running.
# The button that brings the selected job to the front, in its window.
jobs-show = Show
# How far a job is, in the list, in a column of 9.
jobs-state-waiting = waiting
jobs-state-counting = counting
jobs-state-aborting = aborting
jobs-state-percent = { $percent }%
# The time left of a job that moves data, in a column of 12, once it is known.
jobs-left = ETA { $left }
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

## Renaming in place, Shift+F6 (src/tui/app, src/tui/tasks). Prefix: rename-.

rename-error = Cannot rename { $path }: { $reason }
# The new name is a file's: the question whether to rename over it, which removes that file.
rename-exists =
    "{ $name }" is there already.
    Overwrite it?
rename-dir-exists = a directory has that name
# A name with a slash, or . or ..: renaming only gives a new name in the same directory.
rename-invalid = "{ $name }" cannot be a name: renaming does not move
rename-not-utf8 = "{ $name }" cannot be edited: the name is not valid UTF-8; F6 can rename it

## Why a copy or a move cannot start (src/tui/app). Prefix: transfer-.

transfer-same = the source and the target are the same
# The target is in a source directory.
transfer-into-itself = it is in { $path }

## Checksums, Ctrl+x # (src/tui/app, src/tui/sums). Prefix: checksum-.

checksum-title = Checksum
# Above the algorithms. Directories count with the files in them.
checksum-one = Checksum of "{ $name }" with:
checksum-directory = Checksums of the files in "{ $name }" with:
checksum-many = Checksums of { $count } files and directories with:
# Above a field for a checksum to compare with, as a download page gives it.
checksum-expected = Expected checksum (optional; paste it):
# A check box: also hash the file under the cursor of the other panel, and compare the two.
checksum-compare = Compare with { $path }
# The names of the algorithms.
checksum-sha256 = SHA-256
checksum-sha512 = SHA-512
checksum-sha1 = SHA-1
checksum-md5 = MD5
checksum-blake3 = BLAKE3
checksum-expected-invalid = "{ $text }" is not a checksum: it takes the hex digits of an MD5, SHA-1, SHA-256, BLAKE3, or SHA-512 checksum.
# Above the file the job reads.
checksum-hashing = Hashing
checksum-error = Cannot read { $path }: { $reason }
checksum-no-files = There are no files to hash.
# In place of the checksum of a file that was skipped after a failure.
checksum-skipped = skipped
checksum-matches = Matches the expected checksum.
checksum-differs = Does not match the expected checksum.
checksum-same = The files are the same.
checksum-different = The files differ.
# Buttons: the checksum of the selected file; every line, as sha256sum prints them; a file of
# them.
checksum-copy = Copy
checksum-copy-all = Copy all
checksum-save = Save…
# Nothing tells whether the terminal took it: some ignore the request (OSC 52).
checksum-copied = Sent to the terminal's clipboard.
checksum-save-title = Save checksums
checksum-save-prompt = File name:
checksum-saved = Saved to { $path }.
checksum-exists = { $path } is there already. Overwrite it?
checksum-save-error = Cannot save { $path }: { $reason }

## The viewer, F3 (src/tui/app). Prefix: viewer-. Its own text is in crates/noc-viewer.

viewer-error = Cannot view { $path }: { $reason }

## Editing, F4 (src/tui). Prefix: edit-. The editor of $VISUAL or $EDITOR, or vi.

edit-error = Cannot edit { $path }: { $reason }
# The edited copy of a remote file could not go back; it stays where the user can find it.
edit-kept = The changes to { $path } did not go back; they are in { $copy }
# Why the editor did not run.
edit-cannot-run = cannot run { $program }: { $reason }

## The command line of ! and : (src/tui). Prefix: command-. Commands run in $SHELL, or /bin/sh.

# A command after : that Noon Commander does not have.
command-unknown = Unknown command: { $command }. :!command runs a shell command.
# The shell did not start.
command-error = Cannot run { $program }: { $reason }
# Printed in the terminal after a command, with the panels hidden.
command-exit-code = The command exited with { $code }.
command-signal = The command was ended by signal { $signal }.
command-press-key = Press any key to return to Noon Commander.
# Stays on the screen after a command that failed, after the key it waited for.
command-exit-mark = [exit { $code }]
command-signal-mark = [signal { $signal }]
# The window of the command history, Alt+h or Ctrl+r on the command line.
history-title = Command history
# The filter, at the top.
history-filter = Filter: { $text }
# Which commands the window shows; Tab switches.
history-this-host = { $host } ⇄ all
history-all-hosts = all hosts ⇄ this one
# The host of commands that ran on this machine.
history-local = local
history-none = No commands yet. Commands run with ! and : come here.
history-nothing = No command matches.
# history.toml could not be read or written; the reason names the file.
history-error = Cannot keep the command history: { $reason }
# The command could not go to the editor of Ctrl+x Ctrl+e, or come back.
command-edit-error = Cannot edit the command: { $reason }
