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
# Closes the tab under the cursor in the tab menu.
fkey-close-tab = Close

## The mode keys are typed in, as vim shows it on the line above the F-key bar (src/tui/app),
## in Insert and Visual mode. Prefix: mode-.

mode-insert = -- INSERT --
mode-visual = -- VISUAL --
mode-visual-line = -- VISUAL LINE --
# After Ctrl+o in Insert mode, for the one command of Normal mode, as vim says it.
mode-insert-once = -- (insert) --
mode-insert-visual = -- (insert) VISUAL --

## Messages on the line of the mode, in its place until the next key, as vim's on its command
## line (src/tui/app): n or N with no search before, a search that goes on round the end of the
## rows or their start, and g x on a host. Prefix: message-.

message-search-none = No previous search
message-search-bottom = search hit BOTTOM, continuing at TOP
message-search-top = search hit TOP, continuing at BOTTOM
# What quick search, n, or N looked for matches no name, as vim's E486.
message-search-not-found = Pattern not found: { $text }
# g x on a host: the programs of the desktop open local files only.
message-open-remote = Only local files open externally

## The title of the terminal's window or tab (src/tui/app), with ui.terminal_title. Prefix:
## terminal-.

# Where the active panel is: a directory with ~ for home, or host:path. noc is the program's
# name, not to be translated.
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
# Status line during quick search up the rows, as vim's ?: what was typed so far.
panel-search-back = Search up: { $text }
# Status line while the entry under the cursor is renamed in its row: its name until now.
panel-rename = Rename: { $name }
# Status line while a directory is read.
panel-loading = Loading…
# Status line, after a spinner, while the panel waits for a connection to a host; Esc stops it.
panel-connecting = Connecting to { $host }…
# Status line, after a spinner, while a dialog asks whether to connect to the host again.
panel-host-waiting = { $host }: Retry or Cancel?
# Status line of a panel on a host that is not connected, which shows none of its entries;
# reading the directory again connects. The second one names the key that does it.
panel-disconnected = Not connected to { $host }
panel-disconnected-key = Not connected to { $host }; { $key } connects
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

## The dialog after a failed connection (src/tui/app): Retry tries again, Cancel leaves the
## panels where they were. Prefix: reconnect-.

reconnect-title = Connection
reconnect-failed = Cannot connect to { $host }: { $reason }
reconnect-lost = Lost the connection to { $host }: { $reason }

## Connection errors (src/tui/describe). Prefix: error-. Most of the time ssh's own words are
## shown instead.

error-connection-closed = the connection is closed
error-ssh-exited = ssh ended with { $status }
error-ssh-spawn = cannot run { $program }: { $reason }
error-ssh-too-old = OpenSSH { $found } is too old; Noon Commander needs { $required } or newer
error-not-openssh = { $program } is not OpenSSH

## What the windows that list rows to choose from share (src/tui/picker): the location menu,
## the tab menu, the workspaces, and the command history. Prefix: picker-.

# The first line: what was typed to filter the window.
picker-filter = Filter: { $text }
# When the filter leaves no row.
picker-nothing = Nothing matches.

## The location menu of Alt+F1 and Alt+F2 (src/tui/menu), as Far Manager's menu to change
## drives. Prefix: menu-.

# Titles: the panel the menu changes.
menu-left = Left
menu-right = Right
# Headings of the groups of places.
menu-favorites = Favorites
menu-mounts = Mounts
menu-sftp = SFTP
# The row that opens the home directory.
menu-home = Home
# The last row of a group that shows only its first places: how many more it holds, which
# Enter shows and the filter finds.
menu-more = { $count } more…

## The zoxide window of g z (src/tui/jump): the directories zoxide ranks highest for the
## keywords typed, as `z` picks them in a shell. Prefix: jump-.

jump-title = zoxide
# The first line: the keywords typed, separated by spaces.
jump-keywords = Jump to: { $text }
# When zoxide lists no directory.
jump-nothing = zoxide knows no directory that matches. It learns the directories you work in.
# The zoxide program is not there.
jump-not-installed = zoxide is not installed: cannot run { $program }. Get it from https://github.com/ajeetdsouza/zoxide, or set its path in Options → Configuration.

## The search of a whole tree (src/tui/find): g / in a panel, or F9 → Command → Find file….
## ripgrep finds the lines that match the text in the files under the panel's directory. Prefix:
## find-.

# The title: where it searches, as the panel's title shows it.
find-title = Find in { $dir }
# The labels of the fields: the text to find, and the globs of the files to search.
find-text = Text:
find-files = Files:
# The check boxes: the text as a regular expression rather than as it is (Alt+r), and matching
# case rather than only when the text has a capital letter (Alt+c).
find-regex = Regular expression
find-match-case = Match case
# Under the rows: how many lines matched, in how many files.
find-count = { $lines ->
        [one] 1 line
       *[other] { $lines } lines
    } in { $files ->
        [one] 1 file
       *[other] { $files } files
    }
# Right of a file: how many of its lines matched, once they are [search] max_per_file, after
# which the search takes no more of them.
find-file-count-more = { $count }+
# The same with no text, which lists the files.
find-count-files = { $files ->
        [one] 1 file
       *[other] { $files } files
    }
# While nothing is found yet.
find-searching = Searching…
# Nothing matches.
find-none = Nothing found.
# It cannot search: no ripgrep, or a host that is not connected.
find-failed = Cannot search here: { $reason }
# ripgrep is not installed where it searches.
find-not-installed = ripgrep is not installed: cannot run { $program }. Get it from https://github.com/BurntSushi/ripgrep, or set its path in Options → Configuration.
# Under the rows: it stopped at [search] max_results.
find-limit = Stopped after { $count } results: Configuration → Search.
# Under the rows: what ripgrep could not do, such as read a file, and the last of it.
find-errors = { $count ->
        [one] 1 error
       *[other] { $count } errors
    }: { $last }

## Tabs (src/tui/tabs, src/tui/app). Prefix: tabs-. Each side has tabs of its own.

# Titles of the tab menu of t h and t l, drawn over the panel whose tabs it lists;
# each row is where a tab is.
tabs-title-left = Left tabs
tabs-title-right = Right tabs

## Workspaces (src/tui/workspaces, src/tui/app): the tabs of both panels, saved under a name in
## workspaces.toml. Prefix: workspaces-.

# The window of t w, F9 → Workspace → Workspace list….
workspaces-title = Workspaces
# When no workspace is saved yet.
workspaces-none = No workspace is saved yet. a here, or t W in a panel, saves the tabs of both panels under a name.
# Right of a workspace: how many tabs it has, in both panels together.
workspaces-tabs = { $count ->
        [one] 1 tab
       *[other] { $count } tabs
    }
# The dialog of t W, and of a in the window.
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
## letter that, typed with Alt, opens a menu from the bar, or runs a command while its menu is
## open; letters must differ on the bar and within a menu, and && stands for &.

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
# g x: with the program the desktop has for the file, as a double click (open, xdg-open).
pulldown-system-open = &Open externally
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
# Searches the whole tree under the panel's directory, as g / does.
pulldown-find = &Find file…
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
pulldown-full = Show &whole names
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
config-borders = Borders
config-borders-hint = The frames of panels and dialogs.
config-borders-double = Double ═ ║ ╔
config-borders-single = Single ─ │ ┌
config-icons = Icons
config-icons-hint = Nerd Font icons in front of names; without them, mc's markers.
config-show-hidden = Show hidden files
config-show-hidden-hint = Names that begin with a dot; o h switches them.
config-fuzzy-search = Fuzzy search
config-fuzzy-search-hint = Quick search, the location menu, and the zoxide and Workspaces windows match as fzf does: the letters typed in order, best matches first.
config-mouse = Mouse
config-mouse-hint = Clicks move the cursor, open, and mark, and the wheel scrolls. The terminal then selects text with Shift held, or Option in iTerm2.
config-wheel = Mouse wheel
config-wheel-hint = How far a step of the wheel scrolls: a number of lines, or page.
config-wheel-invalid = "{ $text }" is not a step of the wheel: it takes a number of lines from 1 to { $most }, or page.
config-terminal-title = Terminal title
config-terminal-title-hint = The title of the terminal's window or tab shows the active panel's directory.
config-which-key = Key hints
config-which-key-hint = After a key that starts longer sequences, such as g or Ctrl+w, show the keys that can follow it, as which-key does.
config-which-key-delay = Key hints delay
config-which-key-delay-hint = How long after such a key the hints show, in milliseconds.
config-which-key-delay-invalid = "{ $text }" is not a delay: it takes a whole number of milliseconds, from 0 to 65535.
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
# Settings of ssh, [ssh], and of the hosts of the location menu, [discovery]. Lists are words separated by
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
# Settings of the volumes of the location menu, [volumes].
config-volumes = Volumes
config-hide-volumes = Hidden volumes
config-hide-volumes-hint = Mount points to leave out of the location menu, as patterns with * and ?, separated by spaces. The system volume always shows.
# Settings of zoxide, [zoxide].
config-zoxide = zoxide
config-zoxide-record = Record directories
config-zoxide-record-hint = Add a local directory to zoxide once you copy, delete, view, or edit something there; passing through does not count.
config-zoxide-program = Program
config-zoxide-program-hint = The zoxide program: a name in PATH, or a path. g z jumps to the directories it ranks.
config-zoxide-program-empty = The zoxide program cannot be empty.
config-shell = Command line
config-pause = Wait after a command
config-pause-hint = When the output of a command stays on screen until a key; g < shows it again either way.
config-pause-always = Always
config-pause-on-error = When it fails
config-pause-never = Never
config-history-size = History size
config-history-size-hint = How many commands of ! and : the history keeps, of all hosts together; 0 keeps none.
config-history-size-invalid = "{ $text }" is not a number of commands: it takes a whole number, 0 or more.
config-search = Search
config-search-program = Program
config-search-program-hint = The ripgrep program for local directories: a name in PATH, or a path. On a host, rg runs as the host's PATH finds it.
config-search-program-empty = The ripgrep program cannot be empty.
config-max-results = Most results
config-max-results-hint = The search stops after finding so many lines, or files when it lists them; 1 or more.
config-max-results-invalid = "{ $text }" is not a number of results: it takes a whole number, 1 or more.
config-max-per-file = Most lines in a file
config-max-per-file-hint = The search finds no more lines in one file than so many, so that a large file leaves room for others; 1 or more.
config-max-per-file-invalid = "{ $text }" is not a number of lines: it takes a whole number, 1 or more.
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
# Buttons of a list of files found, as in mc's results of Find File: go to the file in the
# panel, view it, edit it.
dialog-go-to = Go to
dialog-view = View
dialog-edit = Edit

## Help screen (src/tui/help). Prefix: help-. One line per key: say what it does, briefly.

help-title = Help
# In the bottom frame, on the right: how far the last row on screen is.
help-position = { $percent }%
# The same, once the end of the help shows.
help-position-end = End
# The sections down the left of the help, as the categories of the Configuration dialog. Keys
# folds, and has a group for each part of the app; keep their names short, for a narrow column.
help-section-about = About
help-section-keys = Keys
help-group-everywhere = Everywhere
help-group-panels = Panels
help-group-text = Text fields
help-group-search = Search and renaming
help-group-command-line = Command line
help-group-lists = Lists and menus
help-group-dialogs = Menus and dialogs
help-group-viewer = Viewer
help-group-tips = Tips
# The About page: what the app is, what it was built from, what it runs on, and its files. Each
# fact has a short name on the left.
help-about-name = Noon Commander { $version }
help-about-description = A two-panel terminal file manager in the style of Midnight Commander, for local files and SFTP alike, through the system's OpenSSH client.
help-about-license = License
help-about-source = Source
help-about-build = Build
help-about-commit = Commit
# Whether the sources it was built from had changes not in the commit: yes or no.
help-about-dirty = Modified
help-about-commit-time = Commit time
help-about-target = Target
help-about-profile = Profile
help-about-features = Features
help-about-system = System
help-about-os = OS
help-about-kernel = Kernel
help-about-arch = Architecture
help-about-terminal = Terminal
# The terminal's name and version, as it answers or as its variables tell.
help-about-terminal-program = Program
help-about-term = TERM
help-about-multiplexer = Multiplexer
help-about-colors = Colors
# Whether the terminal speaks the kitty keyboard protocol: kitty or legacy.
help-about-keyboard = Keyboard
# The programs other than ssh that Noon Commander runs, and what each says it is.
help-about-tools = External tools
help-about-editor = Editor
help-about-zoxide = zoxide
help-about-ripgrep = ripgrep
help-about-files = Files
help-about-config = Settings
help-about-hosts = Host settings
help-about-workspaces = Workspaces
help-about-history = History
help-about-recent = Recent places
help-about-log = Log
# A fact that could not be found.
help-about-unknown = unknown
# A program that is not there.
help-about-missing = not found
help-everywhere = Everywhere
help-panels = Panels
help-panels-visual = Panels in Visual mode
help-text-insert = Text fields in Insert mode
help-text-normal = Text fields in Normal mode
help-text-visual = Text fields in Visual mode
help-lists = Lists
help-lists-filter = The filter of a list
help-list-filter = Type in the filter, in Insert mode
help-list-rows = Back to the rows, with the filter kept
help-list-take = Take the row under the cursor
help-list-close = Close the window
help-menu = Location menu
help-tab-menu = Tab menu
help-workspaces = Workspaces
help-find = Search of a whole tree
help-find-fields = Search of a whole tree, in its fields
help-history = Command history
help-history-hosts = The commands of the panel's host, or of all hosts
help-history-take = Put the command on the command line, without running it
help-history-delete = Remove the command from the history
help-pulldown = Pull-down menu
help-quick-search = Quick search
help-renaming = Renaming in place, in Normal mode
help-renaming-insert = Renaming in place
help-command-line = Command line
help-command-line-normal = Command line in Normal mode
help-dialogs = Dialogs and help
help-this-help = This help
help-this-help-top = The top
help-this-help-end = The end
help-this-help-close = Close this help
help-text-fields = Text fields of dialogs
help-path-fields = Path fields
help-completion = Completion list
help-complete = Complete the path: one match goes in, or the first of several, which a list shows
help-complete-next = The next match, put in at once; after the last, what was typed
help-complete-prev = The match before; before the first, what was typed
help-complete-page-down = A page of matches down; after the last, what was typed
help-complete-page-up = A page of matches up; before the first, what was typed
help-complete-parent = Complete in the directory above the one listed, as vim's pop-up menu
help-complete-into = Complete in the directory chosen, as vim's pop-up menu
help-complete-take = Keep the match, and close the list
help-complete-close = Put back what was typed, and close the list; other keys close it and go to the field, Enter and Ctrl+c too
help-viewer = Viewer
help-viewer-top = The start of the file
help-viewer-end = The end of the file
help-viewer-left = One column left, when lines are cut
help-viewer-right = One column right, when lines are cut
help-viewer-scroll-left = A column left, when lines are cut; with a count, that many
help-viewer-scroll-right = A column right, when lines are cut; with a count, that many
help-viewer-scroll-half-left = Half the screen left, when lines are cut
help-viewer-scroll-half-right = Half the screen right, when lines are cut
help-viewer-scroll-start = The start of the lines, when they are cut
help-viewer-scroll-end = The end of the longest line on screen, when lines are cut
help-viewer-wrap = Wrap long lines, or cut them
help-viewer-quit = Close the viewer
help-row-up = One row up
help-row-down = One row down
help-page-up = One page up
help-page-down = One page down
help-window-up = A whole page up, as less's b
help-window-down = A whole page down, as less's Space and f
help-half-page-up = Half a page up
help-half-page-down = Half a page down
help-first-row = First row
help-last-row = Last row
help-enter = Open the directory under the cursor
help-mark = Mark or unmark, then the next row
help-mark-up = Mark or unmark, then the row above
help-names-left = The names a column left; with a count, that many
help-names-right = The names a column right; with a count, that many
help-names-half-left = The names half their column left
help-names-half-right = The names half their column right
help-names-start = The names from their start
help-names-end = The end of the name under the cursor at the right edge
help-show-full = Show the whole name under the cursor below the panels, or stop
help-list-show-full = Show the whole row under the cursor below the rows, or stop
help-invert-marks = Invert the marks on files
help-select = Mark the names that match a pattern
help-unselect = Unmark the names that match a pattern
help-parent = Parent directory
help-jump-back = Back to where the panel was before its last jump, as vim's jump list
help-jump-forward = Forward again, where Ctrl+o came from
help-visual = Visual mode: select rows from here to the cursor; in Visual mode by lines, switch to it, else end it
help-visual-line = Visual mode by lines, as vim's V: select rows from here to the cursor; in Visual mode, switch to it, else end it
help-reselect = Select again the rows selected last
help-other-end = The other end of the selection
help-visual-end = Normal mode again
help-visual-mark = Mark the rows selected, and Normal mode again
help-visual-copy = Copy the rows selected
help-visual-move = Move or rename the rows selected
help-visual-delete = Delete the rows selected
help-visual-checksum = Checksums of the files selected
help-visual-system-open = Open the rows selected with the programs of the desktop
help-switch-panel = The other panel
help-focus-left = The left panel
help-focus-right = The right panel
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
help-quick-search-start = Quick search, typed as vim's search
help-quick-search-back = Quick search up the rows, as vim's ?; n then goes up too, and N down
help-find-open = Search the files below for text, in a dialog
help-match-next = The next name that matches the last quick search, the way it went
help-match-prev = The previous name that matches the last quick search, the other way
help-shell = Open the command line for a shell command, run in this directory
help-command = Open the command line for a command of Noon Commander; :!command runs a shell command
help-view = View the file under the cursor; on a directory, open it
help-edit = Edit the file under the cursor in $VISUAL or $EDITOR
# open on macOS, xdg-open on Linux: the program a double click would start.
help-system-open = Open the marked entries, or the one under the cursor, with the programs of the desktop
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
help-find-view = View the file, from the line
help-find-edit = Edit the file, at the line
help-find-regex = The text as a regular expression, or as it is
help-find-case = Match case, or ignore it unless the text has a capital
help-new-tab = A new tab in this panel, on the same directory
help-close-tab = Close this tab; the last one stays
help-next-tab = The next tab in this panel
help-prev-tab = The previous tab in this panel
help-tab-list-left = The tabs of the left panel, to choose one
help-tab-list-right = The tabs of the right panel, to choose one
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
help-menu-reload = Read the volumes and hosts again
help-menu-close = Close the menu
help-tab-menu-show = Show the tab in the panel
help-tab-menu-close = Close the tab under the cursor; the last one stays
help-pulldown-open = The pull-down menu: Left, File, Command, Options, Workspace, Right
help-pulldown-last = The pull-down menu, on the command of it that ran last
help-pulldown-up = The command above
help-pulldown-down = The command below
help-pulldown-left = The menu to the left
help-pulldown-right = The menu to the right
help-pulldown-home = The first command
help-pulldown-end = The last command
help-pulldown-run = Open the menu, or run the command
help-pulldown-close = Close the menu, then the menu bar
help-search-end = End the search; the cursor stays on the match
help-search-abort = Give the search up and go back to where it started
help-search-next = The next match; with nothing typed, the last search
help-search-next-match = The next match
help-search-prev = The previous match
help-dialog-up = Previous button; in this help, the section above, or one line up
help-dialog-down = Next button; in this help, the section below, or one line down
help-dialog-left = Previous button; in this help, back to the sections, then up to Keys, and fold it
help-dialog-right = Next button; in this help, open Keys, or go to the text
help-dialog-page-up = The first field or button; in a list or this help, one page up
help-dialog-page-down = The last field or button; in a list or this help, one page down
help-dialog-home = The first field or button; in a list or this help, the top
help-dialog-end = The last field or button; in a list or this help, the end
help-next-field = Next field or button; in this help, the sections or the text
help-prev-field = Previous field or button; in this help, the sections or the text
help-confirm = Press the button with the focus
help-toggle = Switch the check box, or press the button
help-dialog-cancel = Cancel, or close this help
help-field-home = Start of the line
help-field-first-non-blank = The first character of the line that is not blank
help-field-end = End of the line
help-field-prev-line = The first character of the line above that is not blank; an operator takes the lines
help-field-next-line = The first character of the line below that is not blank; an operator takes the lines
help-field-this-line = The first character of the line that is not blank; with a count, of the line a count less one below; d _ deletes the line as d d
help-field-backspace = Delete the character before the cursor
help-field-delete = Delete the character at the cursor
help-field-delete-to-start = Delete to the start of the line
help-field-delete-to-end = Delete to the end of the line
help-field-delete-word = Delete the word before the cursor
help-field-left = One character left
help-field-right = One character right
help-field-char-forward = One character right; from the end of a line, to the start of the next
help-field-char-backward = One character left; from the start of a line, to the end of the one before
help-field-word-forward = The start of the next word
help-field-word-backward = The start of the word before the cursor
help-field-word-end = The end of the word
help-field-big-word-forward = The start of the next word, where only blanks part words
help-field-big-word-backward = The start of the word before, where only blanks part words
help-field-big-word-end = The end of the word, where only blanks part words
help-field-find = The next character typed after it, in the line
help-field-find-back = Back to the character typed after it
help-field-till = Right before the next character typed after it
help-field-till-back = Back to right after the character typed after it
help-field-repeat-find = Find again, as the last f, t, F, or T
help-field-repeat-find-back = Find again, the other way
help-field-insert = Insert mode, before the cursor
help-field-append = Insert mode, after the cursor
help-field-insert-at-start = Insert mode, at the start of the line
help-field-append-at-end = Insert mode, at the end of the line
help-field-visual = Visual mode: select from here to the cursor
help-field-normal = Normal mode
help-field-normal-once = One command of Normal mode, then Insert mode again
help-field-substitute = Delete the character at the cursor, then Insert mode
help-field-substitute-line = Delete the text of the line, then Insert mode
help-field-change-to-end = Delete to the end of the line, then Insert mode
help-field-replace = Replace the character at the cursor with the one typed after it
help-field-toggle-case = Switch the case, and move on
help-field-delete-operator = Delete what the motion after it moves over, or the line for d d; in Visual mode, the selection
help-field-change-operator = Delete as d does, then Insert mode
help-field-yank-operator = Copy what the motion after it moves over to a register, or the line for y y
help-field-put-after = Put the text of the register after the cursor
help-field-put-before = Put the text of the register before the cursor
help-field-undo = Undo the last change
help-field-redo = Redo what undo took back
help-field-paste-register = Put the text of the register typed after it
help-field-literal = Type the next key as it is
help-rename-confirm = Rename to the name typed
help-rename-cancel = Keep the name as it was
help-command-up = The line above; from the first line, the command before in the history of the panel's host
help-command-down = The line below; from the last line, the command after in the history
help-command-visual-up = The line above
help-command-visual-down = The line below
help-command-older = The command before in the history of the panel's host
help-command-newer = The command after in the history; after the last, what was typed
help-user-screen = The output of commands, in place of the panels; g < again, Enter, q, or Esc brings them back
help-command-history = The command history: filter it, and take a command of this host or another
help-command-home = Start of the line
help-command-end = End of the line
help-command-new-line = A new line in the command; Shift+Enter where the terminal tells it from Enter
help-command-normal = Normal mode, to edit the command with vim's keys
help-command-run = Run the command; after a \ at the end of the line, a new line
help-command-complete = Complete the word before the cursor, as vim's :! does: the first word as a command of $PATH, the others as files, escaped for the shell; on a host, files there
help-command-close = Close the command line
help-command-cancel = Close the command line; after Ctrl+f, leave Insert mode for Normal mode, as in vim's window of the command line
# The tips after the keys: a title for each, then its points, a line or two each.
help-tip-modes = Modes
help-tip-modes-keys = Keys are vim's, in its modes; the F-keys of the panels are mc's, in Normal and Visual mode and in quick search.
help-tip-modes-fields = Text fields open in Insert mode, where characters are text; Esc goes to Normal mode, and Esc there closes.
help-tip-modes-close = Quick search and the command line close on Esc, as vim's search and command line do.
help-tip-modes-line = The line above the F-key bar says -- INSERT -- and -- VISUAL --, and -- (insert) -- for the one command of Normal mode after Ctrl+o; the cursor is a bar in Insert mode.
help-tip-modes-alt = Alt and a key stand for Esc and the key in Insert and Visual mode and in the panels, but on a password.
help-tip-modes-cancel = Ctrl+c closes or cancels in every mode.
help-tip-counts = Counts and registers
help-tip-counts-count = In Normal mode, a count goes before a command: 5j goes five rows down, 3G to the third row, 2g t to the second tab, and d3w deletes three words.
help-tip-counts-register = In text fields, "a names a register, which every field shares, and A appends to it.
help-tip-lists = Lists
help-tip-lists-open = The lists open in Normal mode: / types in the filter, and Esc goes back to the rows.
help-tip-lists-jump = The zoxide window opens in Insert mode, typing keywords, as z takes them in a shell; Esc goes to the rows, and / types again.
help-tip-lists-jump-fuzzy = The zoxide window opens in Insert mode, typing what filters its directories; Esc goes to the rows, and / types again.
help-tip-fuzzy = Fuzzy matching
help-tip-fuzzy-where = Quick search, the location menu, the tab menu, the zoxide window, and the Workspaces window match as fzf does: the letters typed in order, not necessarily together, best match first.
help-tip-fuzzy-words = Words separated by spaces must all match; 'word matches as it is, ^word at the start, word$ at the end, and !word where it is not.
help-tip-workspaces = Workspaces
help-tip-workspaces-what = A workspace holds the tabs of both panels: where each is, its sort order, and the row under its cursor.
help-tip-workspaces-list = t w and F9 → Workspace list them; restoring one replaces every tab.
help-tip-command = Command line
help-tip-command-open = ! opens the command line, and : too, for commands of Noon Commander, of which :!command is the only one so far.
help-tip-command-run = A command runs with the terminal, in the panel's directory, then a key brings the panels back; cd and export last only as long as the command.
help-tip-pulldown = Pull-down menu
help-tip-pulldown-letters = Alt and the highlighted letter of a menu open it, and Alt and that of a command run it.
help-tip-key-hints = Key hints
help-tip-key-hints-show = After a key that starts longer sequences, such as g or Ctrl+w, the keys that can follow show after a moment, and wait for one.
help-tip-key-hints-keys = Esc closes them, and Backspace takes back the last key, or closes them when none is typed.
help-tip-key-hints-list = Ctrl+k while they show gives them the keys, as a list of whole sequences: / filters it, and Enter runs the command under the cursor.
help-key-hints = The keys that can be typed now and what they do; again, a list to filter
help-key-hints-run = Close the list and run the command, with the count typed before Ctrl+k
help-key-hints-close = Close the list
help-register = The register for the next command: a … z, which text fields share, and A … Z to append to it

## Key hints, as which-key shows them (src/tui/which_key): the keys that can follow those
## typed so far, at the bottom of the panels. Prefix: which-key-.

# The title while no key is typed; otherwise it is the keys typed, such as Ctrl+w.
which-key-title = Keys
# After a key that starts longer sequences: how many.
which-key-more = { $count ->
        [one] +{ $count } key
       *[other] +{ $count } keys
    }
# In the last cell, for the keys that do not fit.
which-key-hidden = … { $count } more
# On the bottom of the frame.
which-key-close = Ctrl+k lists, Esc closes, Backspace goes back

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

## Marking by pattern, g + and g - (src/tui/app). Prefix: pattern-. As mc's + and -.

# Dialog titles.
pattern-select = Select
pattern-unselect = Unselect
# Check boxes: leave directories alone; tell upper and lower case apart.
pattern-files-only = Files only
pattern-case-sensitive = Case sensitive

## Quick cd, g d (src/tui/app, src/tui/cd). Prefix: cd-. As in mc.

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

## The list of jobs, f j (src/tui/jobs), as mc's Background jobs. Prefix: jobs-.

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

## Checksums, f # (src/tui/app, src/tui/sums). Prefix: checksum-.

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

## Opening externally, g x (src/tui). Prefix: open-. The program the desktop has for the file:
## open on macOS, xdg-open on Linux.

open-error = Cannot open { $path }: { $reason }
# Why the opener did not run.
open-cannot-run = cannot run { $program }: { $reason }

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
# The window of the command history, q : in a panel or on the command line in Normal mode.
history-title = Command history
# Which commands the window shows; Tab switches.
history-this-host = { $host } ⇄ all
history-all-hosts = all hosts ⇄ this one
# The host of commands that ran on this machine.
history-local = local
history-none = No commands yet. Commands run with ! and : come here.
# When the filter leaves no command.
history-nothing = No command matches.
# history.toml could not be read or written; the reason names the file.
history-error = Cannot keep the command history: { $reason }
