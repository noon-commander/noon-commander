# Keyboard shortcuts: Noon Commander vs Midnight Commander vs Far Manager

Sources:

- noc: the presets in [keymap/default.rs](../crates/noc/src/tui/keymap/default.rs) and
  [keymap/vim.rs](../crates/noc/src/tui/keymap/vim.rs); the default preset is modelled on mc,
  `ui.keymap` chooses the preset, and `noc keymap diff` compares them.
- [mc.1.in](https://github.com/MidnightCommander/mc/blob/master/doc/man/mc.1.in)
- [FarEng.hlf.m4](https://github.com/FarGroup/FarManager/blob/master/far/FarEng.hlf.m4)

The sections follow noc's keymap: one per context, in the order of the presets and under the
name `noc keymap diff` gives it, then the mouse, then what only mc or Far has. A context lists
the actions it binds itself; keys it does not bind go to the context it falls back to, as its
section says. In each table:

- Description: what the keys do; in noc, where noc does it.
- Action: noc's action, as the presets and `noc keymap diff` name it; "—" where no action of
  noc's keymap does it. noc's actions come first, in the order of the default preset, then what
  mc or Far does there and noc does not.
- noc (default), noc (vim), mc, far: the keys. "—" — not bound in noc, or not documented in the
  source.

Keys are written the same way in every column, whatever notation the source uses:

- Letters are lowercase, after a modifier too (`Ctrl+l`); an uppercase letter is Shift and the
  letter (`G`, `Alt+H`, `Ctrl+S`).
- Shift is written out only before keys that are not letters (`Shift+F6`, `Shift+Enter`,
  `Ctrl+Shift+[`); symbols are written as typed (`+`, `*`, `?`).
- Modifiers come in the order `Ctrl+Alt+Shift+`; the keys of a sequence are apart (`Ctrl+x l`,
  `Esc 1`, `Z Z`).
- Named keys: Enter, Esc, Tab, Space, Backspace, Delete, Insert, Home, End, PgUp, PgDn, Up,
  Down, Left, Right, F1…F24, Numpad+, Numpad-, Numpad\*, Numpad5. The mouse: Click, Double
  click, Right click, Wheel.

Status, of noc as a whole:

- ✅ settled: both presets bind the action, with the keys they will keep.
- ⏳ not settled yet: noc does not bind it yet, or its keys may still change. Its noc columns
  may show planned keys, which no preset binds yet.
- 🚫 not needed: noc will not have it.

## Panels (`panel`)

A panel that lists a directory. The volumes and hosts and quick search fall back to it. mc and
Far have a command line under the panels that takes what is typed; noc opens one with `!`.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| One row up | `up` | ✅ | Up<br>Ctrl+p | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p | — |
| One row down | `down` | ✅ | Down<br>Ctrl+n | j<br>Down<br>Ctrl+n | Down<br>Ctrl+n | — |
| One page up | `page_up` | ✅ | PgUp<br>Alt+v | Ctrl+b<br>Shift+Up<br>PgUp | PgUp<br>Alt+v | — |
| One page down | `page_down` | ✅ | PgDn<br>Ctrl+v | Ctrl+f<br>Shift+Down<br>Shift+Enter<br>PgDn | PgDn<br>Ctrl+v | — |
| First row | `home` | ✅ | Home | g g<br>Home | Home<br>A1<br>Alt+< | — |
| Last row | `end` | ✅ | End | G<br>End | End<br>C1<br>Alt+> | — |
| Open the directory or file under the cursor | `enter` | ✅ | Enter | l<br>Enter | Enter | Enter |
| Mark or unmark, then the next row | `mark` | ⏳ | Insert<br>Ctrl+t<br>Shift+Down | Space<br>Insert | Insert<br>Ctrl+t | Insert<br>Shift+cursor keys |
| Mark or unmark, then the row above | `mark_up` | ⏳ | Shift+Up | — | — | Shift+Up |
| Mark the names that match a pattern | `select` | ⏳ | +<br>Alt++ | + | +<br>Alt++ (alternate_plus_minus) | Numpad+ |
| Unmark the names that match a pattern | `unselect` | ⏳ | -<br>\\<br>Alt+- | \\ | \\<br>Alt+- (alternate_plus_minus) | Numpad- |
| Invert the marks on files; directories stay as they are | `invert_marks` | ⏳ | \*<br>Alt+\* | v | \*<br>Alt+\* (alternate_plus_minus) | Numpad* |
| Parent directory; above /, the volumes and hosts | `parent` | ✅ | Ctrl+PgUp | h<br>-<br>Backspace | Ctrl+PgUp | Ctrl+PgUp |
| The other panel | `switch_panel` | ⏳ | Tab | Tab<br>Ctrl+w w | Tab<br>Ctrl+i<br>Left<br>Right | Tab |
| Swap the panels | `swap_panels` | ⏳ | Ctrl+u | Ctrl+w x | — | Ctrl+u |
| Open the directory under the cursor in the other panel | `other_panel_open` | ⏳ | Alt+o | Alt+o | Alt+o | — |
| Show this directory in the other panel | `other_panel_sync` | ⏳ | Alt+i | Alt+i | Alt+i | — |
| Read the directory again | `reload` | ⏳ | Ctrl+r | R | — | Ctrl+r |
| Stop loading or connecting | `cancel` | ⏳ | Esc<br>Esc Esc | Esc<br>Ctrl+c | — | — |
| Show or hide names that start with a dot (Far: hidden and system files) | `toggle_hidden` | ⏳ | Alt+. | z h | — | Ctrl+h |
| Sort by name; again: reverse | `sort_by_name` | ⏳ | Ctrl+F3 | o n | — | Ctrl+F3 |
| Sort by extension; again: reverse | `sort_by_extension` | ⏳ | Ctrl+F4 | o e | — | Ctrl+F4 |
| Sort by modification time; again: reverse | `sort_by_time` | ⏳ | Ctrl+F5 | o m | — | Ctrl+F5 |
| Sort by size; again: reverse | `sort_by_size` | ⏳ | Ctrl+F6 | o s | — | Ctrl+F6 |
| Start quick search | `quick_search` | ⏳ | Ctrl+s<br>Alt+s | / | Ctrl+s<br>Alt+s | Alt+letters<br>Alt+Shift+letters |
| Open the command line for a shell command | `shell` | ✅ | ! | ! | Typing | Typing |
| Open the command line for a command of noc | `command` | ✅ | : | : | — | — |
| The command history | `command_history` | ⏳ | Alt+h | q : | Alt+h | Alt+F8 |
| The output of commands, in place of the panels (mc, Far: hide or show the panels) | `user_screen` | ⏳ | Ctrl+o | Ctrl+o | Ctrl+o | Ctrl+o |
| Help | `help` | ✅ | F1 | g ?<br>F1 | F1 | F1 |
| View the file under the cursor | `view` | ⏳ | F3 | i<br>F3 | F3 | F3<br>Numpad5<br>Ctrl+Shift+F3 (always internal) |
| Edit the file under the cursor in $VISUAL or $EDITOR | `edit` | ⏳ | F4 | e<br>F4 | F4 | F4<br>Ctrl+Shift+F4 (always internal) |
| Copy the marked entries, or the one under the cursor | `copy` | ⏳ | F5 | y y<br>F5 | F5 | F5 |
| Move or rename the marked entries, or the one under the cursor | `move` | ⏳ | F6 | d d<br>F6 | F6 | F6 |
| Rename the entry under the cursor in its row (mc, Far: rename or move it in a dialog, ignoring the marks) | `rename` | ⏳ | Shift+F6<br>F16 | c w<br>Shift+F6<br>F16 | F16 | Shift+F6 |
| Make a directory | `mkdir` | ⏳ | F7 | F7 | F7 | F7 |
| Delete the marked entries, or the one under the cursor | `delete` | ⏳ | F8<br>Delete | D D<br>F8<br>Delete | F8 | F8 |
| The running jobs: bring one to the front, or abort it | `jobs` | ⏳ | Ctrl+x j | w | — | — |
| Checksums of the marked files, or the one under the cursor | `checksum` | ⏳ | Ctrl+x # | Ctrl+x # | — | — |
| Change the left panel's location: a volume or a host (Far: its drive) | `location_menu_left` | ⏳ | Alt+F1<br>Ctrl+x 1 | Alt+F1 | — | Alt+F1 |
| Change the right panel's location: a volume or a host (Far: its drive) | `location_menu_right` | ⏳ | Alt+F2<br>Ctrl+x 2 | Alt+F2 | — | Alt+F2 |
| Jump to a directory that zoxide ranks | `jump` | ⏳ | Alt+z<br>Ctrl+x z | Alt+z | — | — |
| Quick cd: type a path as for cd | `quick_cd` | ⏳ | Alt+c | c d | Alt+c | — |
| A new tab in this panel, on the same directory | `new_tab` | ⏳ | Ctrl+x t | g n | — | — |
| Close this tab; the last one stays | `close_tab` | ⏳ | Ctrl+x w | g c | — | — |
| The next tab in this panel | `next_tab` | ⏳ | Alt+Right<br>Ctrl+x n | g t<br>Alt+Right | — | — |
| The previous tab in this panel | `prev_tab` | ⏳ | Alt+Left<br>Ctrl+x p | g T<br>Alt+Left | — | — |
| The tabs of this panel, to choose one | `tab_list` | ⏳ | Ctrl+x Tab | Ctrl+x Tab | — | — |
| The saved workspaces: restore, rename, or delete one | `workspaces` | ⏳ | Alt+w | Alt+w | — | — |
| Save the tabs of both panels as a workspace | `save_workspace` | ⏳ | Alt+W | Alt+W | — | — |
| The pull-down menu | `pull_down` | ✅ | F9 | F9 | F9 | F9 |
| Quit | `quit` | ✅ | F10 | Z Z<br>F10 | F10 | F10 |
| Redraw the screen | `redraw` | ✅ | Ctrl+l | Ctrl+l | Ctrl+l | — |
| F1…F10 on terminals without function keys | — | ⏳ | Esc, then 1…9, 0 | Esc, then 1…9, 0 | Esc, then 1…9, 0 | — |
| Alt+key on terminals without Alt | — | ⏳ | Esc, then the key | Esc, then the key | Esc, then the key | — |
| Enter the directory or archive under the cursor | — | ⏳ | — | — | Ctrl+PgDn | Ctrl+PgDn<br>Ctrl+Shift+PgDn (always as archive) |
| Execute in separate window | — | ⏳ | — | — | — | Shift+Enter |
| Run as administrator | — | ⏳ | — | — | — | Ctrl+Alt+Enter |
| User menu | — | ⏳ | — | — | F2 | F2 |
| View without preprocessing | — | ⏳ | — | — | F13 | — |
| Alternative (external/internal) viewer | — | ⏳ | — | — | — | Alt+F3 |
| Filtered view (command output) | — | ⏳ | — | — | Alt+! | — |
| Alternative (external/internal) editor | — | ⏳ | — | — | — | Alt+F4 |
| Edit new file | — | ⏳ | — | — | F14 | Shift+F4 |
| Copy current file (ignoring selection) | — | ⏳ | — | — | F15 | Shift+F5 |
| Delete only the file under cursor | — | ⏳ | — | — | — | Shift+F8 |
| Delete bypassing Recycle Bin | — | ⏳ | — | — | — | Shift+Delete |
| Wipe | — | ⏳ | — | — | — | Alt+Delete |
| Quit without changing to last directory (shell wrapper) | — | ⏳ | — | Z Q | Shift+F10 | — |
| Plugin commands | — | ⏳ | — | — | — | F11 |
| Plugin configuration | — | ⏳ | — | — | — | Alt+Shift+F9 |
| Save setup | — | ⏳ | — | — | — | Shift+F9 |
| Repeat last menu item | — | ⏳ | — | — | — | Shift+F10 |
| Print files | — | ⏳ | — | — | — | Alt+F5 |
| Create link | — | ⏳ | — | — | Ctrl+x l (hard)<br>Ctrl+x s (absolute symlink)<br>Ctrl+x v (relative symlink) | Alt+F6 |
| Find file | — | ⏳ | — | — | Alt+? | Alt+F7 |
| Find folder | — | ⏳ | — | — | — | Alt+F10 |
| File permissions/attributes | — | ⏳ | — | — | Ctrl+x c (chmod) | Ctrl+a |
| Change owner (chown) | — | ⏳ | — | — | Ctrl+x o | — |
| File system attributes (chattr) | — | ⏳ | — | — | Ctrl+x e | — |
| Apply command to selected files | — | ⏳ | — | — | — | Ctrl+g |
| Describe selected files | — | ⏳ | — | — | — | Ctrl+z |
| Add files to archive | — | ⏳ | — | — | — | Shift+F1 |
| Extract files from archive | — | ⏳ | — | — | — | Shift+F2 |
| Archive commands | — | ⏳ | — | — | — | Shift+F3 |
| Temporarily show user screen (while held) | — | ⏳ | — | — | — | Ctrl+Alt+Shift |
| External panelize | — | ⏳ | — | — | Ctrl+x ! | — |
| Add current directory to hotlist / folder shortcut | — | ⏳ | — | — | Ctrl+x h | Ctrl+Shift+0…9 |
| Go to directory from hotlist / folder shortcut | — | ⏳ | — | — | Ctrl+\ (list) | RightCtrl+0…9 |
| Change panel charset | — | ⏳ | — | — | Alt+e | — |
| Toggle panel split (vertical/horizontal) | — | ⏳ | — | — | Alt+, | — |
| Change window size | — | ⏳ | — | — | — | Alt+F9 |
| Task list | — | ⏳ | — | — | — | Ctrl+w |
| Screen grabber | — | ⏳ | — | — | — | Alt+Insert |
| Record keyboard macro | — | ⏳ | — | — | — | Ctrl+. |
| Next screen | — | ⏳ | — | — | Alt+} | Ctrl+Tab |
| Previous screen | — | ⏳ | — | — | Alt+{ | Ctrl+Shift+Tab |
| Screen list | — | ⏳ | — | — | Alt+` | F12 |
| Directory history (list) | — | ⏳ | — | — | Alt+H | Alt+F12 |
| Previous directory in history | — | ⏳ | — | — | Alt+y | — |
| Next directory in history | — | ⏳ | — | — | Alt+u | — |
| View and edit history | — | ⏳ | — | — | — | Alt+F11 |
| Info panel | — | ⏳ | — | — | Ctrl+x i (on other panel) | Ctrl+l |
| Quick view panel | — | ⏳ | — | — | Ctrl+x q (on other panel) | Ctrl+q |
| Folder tree | — | ⏳ | — | — | — | Ctrl+t |
| Hide/show inactive panel | — | ⏳ | — | — | — | Ctrl+p |
| Hide/show left panel | — | ⏳ | — | — | — | Ctrl+F1 |
| Hide/show right panel | — | ⏳ | — | — | — | Ctrl+F2 |
| Change panels height | — | ⏳ | — | — | — | Ctrl+Up<br>Ctrl+Down |
| Change current panel height | — | ⏳ | — | — | — | Ctrl+Shift+Up<br>Ctrl+Shift+Down |
| Change panels width (with empty command line) | — | ⏳ | — | — | — | Ctrl+Left<br>Ctrl+Right |
| Restore default panels width | — | ⏳ | — | — | — | Ctrl+Numpad5 |
| Restore default panels height | — | ⏳ | — | — | — | Ctrl+Alt+Numpad5 |
| Hide/show functional key bar | — | ⏳ | — | — | — | Ctrl+b |
| Sizes in bytes / with K/M/G/T suffixes | — | ⏳ | — | — | — | Ctrl+S |
| Next listing format | — | ⏳ | — | — | Alt+t | — |
| Brief view mode | — | ⏳ | — | — | — | LeftCtrl+1 |
| Medium view mode | — | ⏳ | — | — | — | LeftCtrl+2 |
| Full view mode | — | ⏳ | — | — | — | LeftCtrl+3 |
| Wide view mode | — | ⏳ | — | — | — | LeftCtrl+4 |
| Detailed view mode | — | ⏳ | — | — | — | LeftCtrl+5 |
| Descriptions view mode | — | ⏳ | — | — | — | LeftCtrl+6 |
| Long descriptions view mode | — | ⏳ | — | — | — | LeftCtrl+7 |
| File owners view mode | — | ⏳ | — | — | — | LeftCtrl+8 |
| File links view mode | — | ⏳ | — | — | — | LeftCtrl+9 |
| Alternative full view mode | — | ⏳ | — | — | — | LeftCtrl+0 |
| Long/short names | — | ⏳ | — | — | — | Ctrl+n |
| Scroll long names | — | ⏳ | — | — | Alt+(<br>Alt+) | Alt+Left<br>Alt+Right<br>Alt+Home<br>Alt+End |
| Go to root directory | — | ⏳ | — | — | — | Ctrl+\ |
| Top / middle / bottom file on screen | — | ⏳ | — | — | Alt+g / Alt+r / Alt+j | — |
| Unsorted | — | ⏳ | — | — | — | Ctrl+F7 |
| Sort by creation time | — | ⏳ | — | — | — | Ctrl+F8 |
| Sort by access time | — | ⏳ | — | — | — | Ctrl+F9 |
| Sort by description | — | ⏳ | — | — | — | Ctrl+F10 |
| Sort by owner | — | ⏳ | — | — | — | Ctrl+F11 |
| Sort modes menu | — | ⏳ | — | — | — | Ctrl+F12 |
| Use sort groups | — | ⏳ | — | — | — | Shift+F11 |
| Show selected files first | — | ⏳ | — | — | — | Shift+F12 |
| Select files with same extension | — | ⏳ | — | — | — | Ctrl+Numpad+ |
| Deselect files with same extension | — | ⏳ | — | — | — | Ctrl+Numpad- |
| Invert selection including folders | — | ⏳ | — | — | — | Ctrl+Numpad* |
| Select files with same name | — | ⏳ | — | — | — | Alt+Numpad+ |
| Deselect files with same name | — | ⏳ | — | — | — | Alt+Numpad- |
| Invert selection of files, deselect folders | — | ⏳ | — | — | — | Alt+Numpad* |
| Select all files | — | ⏳ | — | — | — | Shift+Numpad+ |
| Deselect all files | — | ⏳ | — | — | — | Shift+Numpad- |
| Restore previous selection | — | ⏳ | — | — | — | Ctrl+m |
| Selected names to clipboard | — | ⏳ | — | — | — | Ctrl+Insert (with empty command line)<br>Ctrl+Shift+Insert |
| Full names to clipboard | — | ⏳ | — | — | — | Alt+Shift+Insert |
| Real (UNC) names to clipboard | — | ⏳ | — | — | — | Ctrl+Alt+Insert |
| Copy files to clipboard | — | ⏳ | — | — | — | Ctrl+C |
| Cut files to clipboard | — | ⏳ | — | — | — | Ctrl+X |

## Volumes and hosts (`root`)

A panel on the virtual root or the list of hosts; keys it does not bind go to the panel.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Edit the settings of the host under the cursor | `edit_host` | ⏳ | F4 | F4 | — | — |
| Disconnect the host under the cursor | `disconnect` | ⏳ | F8 | D D<br>F8 | — | — |

## Quick search (`quick_search`)

In the active panel; characters are part of the name searched for, and keys it does not bind go
to the panel.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Take back the last character | `backspace` | ⏳ | Backspace | Backspace<br>Ctrl+h | Backspace<br>Delete | — |
| End the search | `cancel` | ⏳ | Esc | Esc | — | — |
| Next match; the default preset leaves it to the panel | `quick_search` | ⏳ | Ctrl+s<br>Alt+s | Ctrl+g | Ctrl+s | Ctrl+Enter |
| Previous match | — | ⏳ | — | — | — | Ctrl+Shift+Enter |
| Search with previous pattern | — | ⏳ | — | — | Ctrl+s Ctrl+s | — |
| Paste from clipboard | — | ⏳ | — | — | — | Ctrl+v<br>Shift+Insert |

## Renaming in place (`rename`)

Shift+F6 edits the name in the entry's row; keys it does not bind do nothing. Neither mc nor Far
renames in the row: their Shift+F6 asks in a dialog, whose keys are those of text fields.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| One character left | `left` | ⏳ | Left | Left | — | — |
| One character right | `right` | ⏳ | Right | Right | — | — |
| Start of the text | `home` | ⏳ | Home<br>Ctrl+a | Home<br>Ctrl+b | — | — |
| End of the text | `end` | ⏳ | End<br>Ctrl+e | End<br>Ctrl+e | — | — |
| Delete the character before the cursor | `backspace` | ⏳ | Backspace | Backspace<br>Ctrl+h | — | — |
| Delete the character at the cursor | `delete` | ⏳ | Delete | Delete | — | — |
| Delete to the start | `delete_to_start` | ⏳ | Ctrl+u | Ctrl+u | — | — |
| Delete to the end | `delete_to_end` | ⏳ | Ctrl+k | Ctrl+k | — | — |
| Rename to the name typed | `confirm` | ⏳ | Enter | Enter | — | — |
| Keep the name as it was | `cancel` | ⏳ | Esc | Esc<br>Ctrl+c | — | — |

## Dialogs and help (`dialog`)

A dialog whose focus is on a button, a check box, or a list, and the windows of the help, the
jobs, a job, and checksums. Dialogs are modal: the panel's keys do nothing there.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Previous button; in the help and lists, one row up | `up` | ⏳ | Up | k<br>Up | — | — |
| Next button; in the help and lists, one row down | `down` | ⏳ | Down | j<br>Down | — | — |
| Previous button | `left` | ⏳ | Left | Left | — | — |
| Next button | `right` | ⏳ | Right | Right | — | — |
| In the help and lists, one page up | `page_up` | ⏳ | PgUp | Ctrl+b<br>PgUp | Backspace (help) | — |
| In the help and lists, one page down | `page_down` | ⏳ | PgDn | Ctrl+f<br>PgDn | Space (help) | — |
| In the help and lists, the first row | `home` | ⏳ | Home | g g<br>Home | — | — |
| In the help and lists, the last row | `end` | ⏳ | End | G<br>End | — | — |
| Next field or button | `next_field` | ⏳ | Tab | Tab | — | — |
| Previous field or button | `prev_field` | ⏳ | Shift+Tab | Shift+Tab | — | — |
| Press the focused button; from a text field or a check box, the default one; close the help | `confirm` | ⏳ | Enter | Enter | — | Ctrl+Enter (default button) |
| Switch the check box, choose the radio button, or press the focused button | `toggle` | ⏳ | Space | Space | — | — |
| Cancel; abort the job in front; close the help or the window | `cancel` | ⏳ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10<br>q (help) | Esc Esc<br>Esc (if Esc key mode is enabled)<br>Ctrl+c (abort copy/delete) | — |
| Copy/move in background (in dialog) | — | ⏳ | — | — | Alt+b | — |
| Focus first dialog item | — | ⏳ | — | — | — | Home |
| Focus default dialog item | — | ⏳ | — | — | — | PgDn<br>End |
| Move dialog | — | ⏳ | — | — | — | Ctrl+F5 |
| Checkbox: on / off / undefined | — | ⏳ | — | — | — | Numpad+ / Numpad- / Numpad* |
| Help: follow link | — | ⏳ | — | — | — | Enter |
| Help: next / previous link | — | ⏳ | — | — | — | Tab / Shift+Tab |
| Help: previous topic | — | ⏳ | — | — | — | Alt+F1<br>Backspace |
| Help: contents | — | ⏳ | — | — | — | Shift+F1 |
| Help: plugins help | — | ⏳ | — | — | — | Shift+F2 |
| Help: search | — | ⏳ | — | — | — | F7 |
| Help: maximize/restore window | — | ⏳ | — | — | — | F5 |
| Help: full list of help keys | — | ⏳ | — | — | F1 (again) | — |

## Viewer (`viewer`)

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| One line up | `up` | ✅ | Up<br>k<br>y<br>Ctrl+p | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p | Up |
| One line down | `down` | ✅ | Down<br>j<br>e<br>Enter<br>Ctrl+n | j<br>Down<br>Ctrl+n<br>Enter | Down<br>Ctrl+n | Down |
| One page up | `page_up` | ✅ | PgUp<br>b<br>Alt+v<br>Backspace | Ctrl+b<br>Shift+Up<br>PgUp | PgUp<br>Alt+v<br>Ctrl+b<br>b<br>Ctrl+h<br>Backspace<br>Delete | PgUp |
| One page down | `page_down` | ✅ | PgDn<br>Space<br>f<br>Ctrl+v | Ctrl+f<br>Shift+Down<br>Shift+Enter<br>PgDn | PgDn<br>Space<br>Ctrl+v | PgDn |
| The start of the file | `home` | ✅ | Home<br>g<br>Ctrl+Home | g g<br>Home | Home<br>A1<br>g | Home<br>Ctrl+Home |
| The end of the file | `end` | ✅ | End<br>G<br>Ctrl+End | G<br>End | End<br>C1<br>G | End<br>Ctrl+End |
| One column left, when lines are cut | `left` | ⏳ | Left<br>h | h<br>Left | — | Left |
| One column right, when lines are cut | `right` | ⏳ | Right<br>l | l<br>Right | — | Right |
| Wrap long lines, or cut them | `toggle_wrap` | ⏳ | F2 | F2 | F2 | F2 |
| Help | `help` | ✅ | F1 | g ?<br>F1 | F1 | F1 |
| Close the viewer | `quit` | ⏳ | F3<br>F10<br>q<br>Esc | q<br>Esc<br>F3<br>F10 | F10<br>Esc | F10<br>F3<br>Numpad5<br>Esc |
| Redraw the screen | `redraw` | ✅ | Ctrl+l | Ctrl+l | Ctrl+l | — |
| Wrap type (by chars/words) | — | ⏳ | — | — | — | Shift+F2 |
| Hex/code mode | — | ⏳ | — | — | F4 | F4 |
| Select mode (text/code/dump) | — | ⏳ | — | — | — | Shift+F4 |
| Go to position | — | ⏳ | — | — | F5 | Alt+F8 |
| Switch to editor | — | ⏳ | — | — | — | F6 |
| Search | — | ⏳ | — | — | F7<br>/<br>? (backward) | F7 |
| Continue search forward | — | ⏳ | — | — | Ctrl+s | Shift+F7<br>Space |
| Continue search backward | — | ⏳ | — | — | Ctrl+r | Alt+F7 |
| Continue search in chosen direction | — | ⏳ | — | — | F17<br>n | — |
| Temporarily reverse search direction | — | ⏳ | — | — | N | — |
| Raw/Parsed | — | ⏳ | — | — | F8 | — |
| Format/Unformat | — | ⏳ | — | — | F9 | — |
| Code page | — | ⏳ | — | — | Alt+e | F8 (OEM/ANSI)<br>Shift+F8 (menu) |
| Half page up / down | — | ⏳ | — | — | u / d | — |
| 20 columns left / right | — | ⏳ | — | — | — | Ctrl+Left / Ctrl+Right |
| Leftmost / rightmost column | — | ⏳ | — | — | — | Ctrl+Shift+Left / Ctrl+Shift+Right |
| Shift characters/bytes (dump, code) | — | ⏳ | — | — | — | Ctrl+Left / Ctrl+Right |
| Bytes per line −1 / +1 (code) | — | ⏳ | — | — | — | Alt+Left / Alt+Right |
| Bytes per line to nearest multiple of 16 (code) | — | ⏳ | — | — | — | Ctrl+Alt+Left / Ctrl+Alt+Right |
| Set bookmark | — | ⏳ | — | — | [n] m | RightCtrl+0…9<br>Ctrl+Shift+0…9 |
| Go to bookmark | — | ⏳ | — | — | [n] r | LeftCtrl+0…9 |
| Return to previous position | — | ⏳ | — | — | — | Alt+Backspace<br>Ctrl+z |
| Next file | — | ⏳ | — | — | Ctrl+f | Numpad+ |
| Previous file | — | ⏳ | — | — | Ctrl+b | Numpad- |
| Ruler | — | ⏳ | — | — | Alt+r | — |
| User screen | — | ⏳ | — | — | Ctrl+o | Ctrl+o<br>Ctrl+Alt+Shift (temporarily) |
| Go to file in panel | — | ⏳ | — | — | — | Ctrl+F10 |
| Plugin commands | — | ⏳ | — | — | — | F11 |
| View and edit history | — | ⏳ | — | — | — | Alt+F11 |
| Far window size | — | ⏳ | — | — | — | Alt+F9 |
| Viewer settings | — | ⏳ | — | — | — | Alt+Shift+F9 |
| Functional key bar | — | ⏳ | — | — | — | Ctrl+b |
| Status line | — | ⏳ | — | — | — | Ctrl+B |
| Scrollbar | — | ⏳ | — | — | — | Ctrl+s |
| Copy selection | — | ⏳ | — | — | — | Ctrl+Insert<br>Ctrl+c |
| Clear selection | — | ⏳ | — | — | — | Ctrl+u |
| Select text manually | — | ⏳ | — | — | — | Shift+Click |

## Location menu (`menu`)

Alt+F1 and Alt+F2: the volumes and hosts; characters filter it. The far column has the keys of
Far's menus, its drive menu among them.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Previous item | `up` | ⏳ | Up | Ctrl+p<br>Up | — | — |
| Next item | `down` | ⏳ | Down | Ctrl+n<br>Down | — | — |
| One page up | `page_up` | ⏳ | PgUp | PgUp | — | — |
| One page down | `page_down` | ⏳ | PgDn | PgDn | — | — |
| First item | `home` | ⏳ | Home | Home | — | — |
| Last item | `end` | ⏳ | End | End | — | — |
| Open the volume or host in the panel | `confirm` | ⏳ | Enter | Enter | — | — |
| Take back the last character of the filter | `backspace` | ⏳ | Backspace | Backspace<br>Ctrl+h | — | — |
| Disconnect the host | `disconnect` | ⏳ | F8 | F8 | — | — |
| Read the volumes and hosts again | `reload` | ⏳ | Ctrl+r | Ctrl+r | — | — |
| Close the menu | `cancel` | ⏳ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10 | — | — |
| Filter the items | — | ⏳ | Typing | Typing | — | Ctrl+Alt+f<br>RightAlt |
| Open an item by its number (empty filter) | — | ⏳ | 1…9, 0 | 1…9, 0 | — | — |
| Lock filter | — | ⏳ | — | — | — | Ctrl+Alt+l |
| Shift all items by 1 position | — | ⏳ | — | — | — | Alt+Left<br>Alt+Right |
| Shift selected item by 1 position | — | ⏳ | — | — | — | Alt+Shift+Left<br>Alt+Shift+Right |
| Shift all items by 20 positions | — | ⏳ | — | — | — | Ctrl+Alt+Left<br>Ctrl+Alt+Right |
| Shift selected item by 20 positions | — | ⏳ | — | — | — | Ctrl+Shift+Left<br>Ctrl+Shift+Right |
| Align all items left / right | — | ⏳ | — | — | — | Alt+Home / Alt+End |
| Align selected item left / right | — | ⏳ | — | — | — | Alt+Shift+Home / Alt+Shift+End |
| Fixed menu columns | — | ⏳ | — | — | — | Shift+F5 |

## zoxide window (`jump`)

Alt+z: the directories that zoxide ranks; characters are keywords, as z takes them in a shell.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Previous directory | `up` | ⏳ | Up | Ctrl+p<br>Up | — | — |
| Next directory | `down` | ⏳ | Down | Ctrl+n<br>Down | — | — |
| One page up | `page_up` | ⏳ | PgUp | PgUp | — | — |
| One page down | `page_down` | ⏳ | PgDn | PgDn | — | — |
| First directory | `home` | ⏳ | Home | Home | — | — |
| Last directory | `end` | ⏳ | End | End | — | — |
| Open the directory in the active panel | `confirm` | ⏳ | Enter | Enter | — | — |
| Take back the last character of the keywords | `backspace` | ⏳ | Backspace | Backspace<br>Ctrl+h | — | — |
| Close the window | `cancel` | ⏳ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10 | — | — |
| Open a directory by its number (no keywords) | — | ⏳ | 1…9, 0 | 1…9, 0 | — | — |

## Workspaces window (`workspaces`)

Alt+w, or F9 → Workspace → Workspace list…; F9 → Workspace lists the first ten too, by their
digits. Characters filter it.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Save the tabs of both panels as a new workspace | `save_workspace` | ⏳ | Insert | Insert | — | — |
| Previous workspace | `up` | ⏳ | Up | Ctrl+p<br>Up | — | — |
| Next workspace | `down` | ⏳ | Down | Ctrl+n<br>Down | — | — |
| One page up | `page_up` | ⏳ | PgUp | PgUp | — | — |
| One page down | `page_down` | ⏳ | PgDn | PgDn | — | — |
| First workspace | `home` | ⏳ | Home | Home | — | — |
| Last workspace | `end` | ⏳ | End | End | — | — |
| Restore: replace the tabs of both panels | `confirm` | ⏳ | Enter | Enter | — | — |
| Take back the last character of the filter | `backspace` | ⏳ | Backspace | Backspace<br>Ctrl+h | — | — |
| Rename the workspace | `move` | ⏳ | F6 | F6 | — | — |
| Delete the workspace | `delete` | ⏳ | F8<br>Delete | F8<br>Delete | — | — |
| Close the window | `cancel` | ⏳ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10 | — | — |
| Restore a workspace by its number (empty filter) | — | ⏳ | 1…9, 0 | 1…9, 0 | — | — |

## Pull-down menu (`pull_down`)

F9; the highlighted letter of a menu opens it, and that of a command runs it.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| The command above | `up` | ⏳ | Up | Ctrl+p<br>Up | — | — |
| The command below | `down` | ⏳ | Down | Ctrl+n<br>Down | — | — |
| The menu to the left | `left` | ⏳ | Left | Left | — | — |
| The menu to the right | `right` | ⏳ | Right | Right | — | — |
| The first command | `home` | ⏳ | Home<br>PgUp | Home<br>PgUp | — | — |
| The last command | `end` | ⏳ | End<br>PgDn | End<br>PgDn | — | — |
| Open the menu, or run the command | `confirm` | ⏳ | Enter | Enter | — | — |
| Close the menu, then the menu bar | `cancel` | ⏳ | Esc<br>F9<br>F10 | Esc<br>F9<br>F10 | — | — |
| Open a menu or run a command by its letter | — | ⏳ | Letter | Letter | — | — |

## Text fields (`dialog_input`)

A text field in a dialog; keys it does not bind go to the dialog, whose Left and Right move the
text cursor here. The mc column has the keys of mc's input lines, its command line among them;
the far column, those of Far's command line.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| One character left, with the dialog's key | `left` | ⏳ | Left | Left | Ctrl+b<br>Left | Left<br>Ctrl+s |
| One character right, with the dialog's key | `right` | ⏳ | Right | Right | Ctrl+f<br>Right | Right<br>Ctrl+d |
| Start of the text | `home` | ⏳ | Home<br>Ctrl+a | Home<br>Ctrl+b | Ctrl+a | Ctrl+Home |
| End of the text | `end` | ⏳ | End<br>Ctrl+e | End<br>Ctrl+e | Ctrl+e | Ctrl+End |
| Delete the character before the cursor | `backspace` | ⏳ | Backspace | Backspace<br>Ctrl+h | Ctrl+h<br>Backspace | Backspace |
| Delete the character at the cursor | `delete` | ⏳ | Delete | Delete | Ctrl+d<br>Delete | Delete |
| Delete to the start | `delete_to_start` | ⏳ | Ctrl+u | Ctrl+u | — | — |
| Delete to the end | `delete_to_end` | ⏳ | Ctrl+k | Ctrl+k | Ctrl+k | Ctrl+k |
| Word left | — | ⏳ | — | — | Alt+b | Ctrl+Left |
| Word right | — | ⏳ | — | — | Alt+f | Ctrl+Right |
| Delete word left | — | ⏳ | — | Ctrl+w | Ctrl+Alt+h<br>Alt+Backspace | Ctrl+Backspace |
| Delete word right | — | ⏳ | — | — | — | Ctrl+Delete |
| Set mark | — | ⏳ | — | — | Ctrl+@ | — |
| Cut (mark to cursor) | — | ⏳ | — | — | Ctrl+w | — |
| Copy | — | ⏳ | — | — | Alt+w | Ctrl+Insert |
| Paste | — | ⏳ | — | — | Ctrl+y | Shift+Insert |
| Input line history | — | ⏳ | — | — | Alt+h | Ctrl+Up<br>Ctrl+Down (in dialogs) |
| Previous / next history entry | — | ⏳ | — | — | Alt+p / Alt+n | — |
| Dialog history: clear | — | ⏳ | — | — | — | Delete |
| Dialog history: delete item | — | ⏳ | — | — | — | Shift+Delete |
| Dialog history: mark item | — | ⏳ | — | — | — | Insert |
| Insert file name under cursor into dialog | — | ⏳ | — | — | — | Shift+Enter |
| Insert passive panel file name into dialog | — | ⏳ | — | — | — | Ctrl+Shift+Enter |

## Path fields (`path_input`)

A text field for a path, in Quick cd, F5, F6, and F7; keys it does not bind go to text fields.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Complete the path; again: list the choices | `complete` | ⏳ | Tab | Tab | Alt+Tab | — |

## Completion list (`completion`)

Under a path field after a Tab that gets no further; keys it does not bind close it and go to
the field.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Previous choice | `up` | ⏳ | Up | Ctrl+p<br>Up | — | — |
| Next choice | `down` | ⏳ | Down | Ctrl+n<br>Down | — | — |
| One page up | `page_up` | ⏳ | PgUp | PgUp | — | — |
| One page down | `page_down` | ⏳ | PgDn | PgDn | — | — |
| First choice | `home` | ⏳ | Home | Home | — | — |
| Last choice | `end` | ⏳ | End | End | — | — |
| Next choice, round | `complete` | ⏳ | Tab | Tab | — | — |
| Put the choice in the field | `confirm` | ⏳ | Enter | Ctrl+y<br>Enter | — | — |
| Close the list | `cancel` | ⏳ | Esc | Ctrl+e<br>Esc | — | — |
| Close the list and edit the field | — | ⏳ | Other keys | Other keys | — | — |

## Command line (`command_line`)

`!` and `:` open it; keys it does not bind do nothing. In mc and Far the command line is always
under the panels, so their keys here are pressed in the panels. The keys of their input lines
that noc lacks, such as words and the clipboard, are under text fields.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| One character left | `left` | ⏳ | Left | Left | Ctrl+b<br>Left | Left<br>Ctrl+s |
| One character right | `right` | ⏳ | Right | Right | Ctrl+f<br>Right | Right<br>Ctrl+d |
| The line above; from the first line, the command before in the history of the panel's host | `up` | ⏳ | Up | Up | — | — |
| The line below; from the last line, the command after in the history | `down` | ⏳ | Down | Down | — | — |
| Start of the line | `home` | ⏳ | Home<br>Ctrl+a | Home<br>Ctrl+b | Ctrl+a | Ctrl+Home |
| End of the line | `end` | ⏳ | End<br>Ctrl+e | End<br>Ctrl+e | Ctrl+e | Ctrl+End |
| Delete the character before the cursor | `backspace` | ⏳ | Backspace | Backspace<br>Ctrl+h | Ctrl+h<br>Backspace | Backspace |
| Delete the character at the cursor | `delete` | ⏳ | Delete | Delete | Ctrl+d<br>Delete | Delete |
| Delete to the start of the line | `delete_to_start` | ⏳ | Ctrl+u | Ctrl+u | — | — |
| Delete to the end of the line | `delete_to_end` | ⏳ | Ctrl+k | Ctrl+k | Ctrl+k | Ctrl+k |
| A new line in the command | `new_line` | ⏳ | Ctrl+j<br>Shift+Enter (kitty keyboard protocol) | Ctrl+j<br>Shift+Enter (kitty keyboard protocol) | — | — |
| Edit the command in $VISUAL or $EDITOR | `edit_command` | ⏳ | Ctrl+x Ctrl+e | Ctrl+f<br>Ctrl+x Ctrl+e | — | — |
| The command before in the history of the panel's host | `older_command` | ⏳ | Alt+p | Ctrl+p | Alt+p | Ctrl+e |
| The command after in the history of the panel's host | `newer_command` | ⏳ | Alt+n | Ctrl+n | Alt+n | Ctrl+x |
| The command history | `command_history` | ⏳ | Alt+h<br>Ctrl+r | Ctrl+r | Alt+h | Alt+F8 |
| The output of commands, in place of the panels | `user_screen` | ⏳ | Ctrl+o | Ctrl+o | Ctrl+o | Ctrl+o |
| Run the command; after a \\ at the end of the line, a new line | `confirm` | ⏳ | Enter | Enter | Enter | Enter |
| Close the command line | `cancel` | ⏳ | Esc | Esc<br>Ctrl+c | — | Esc |
| Insert current file name | — | ⏳ | — | — | Alt+Enter<br>Ctrl+Enter | Ctrl+j<br>Ctrl+Enter |
| Insert file name from passive panel | — | ⏳ | — | — | — | Ctrl+Shift+Enter |
| Insert full name of current file | — | ⏳ | — | — | Ctrl+Shift+Enter | Ctrl+f |
| Insert full file name from passive panel | — | ⏳ | — | — | — | Ctrl+; |
| Insert UNC file name (active / passive) | — | ⏳ | — | — | — | Ctrl+Alt+f / Ctrl+Alt+; |
| Insert tagged files of current panel | — | ⏳ | — | — | Ctrl+x t | — |
| Insert tagged files of other panel | — | ⏳ | — | — | Ctrl+x Ctrl+t | — |
| Insert current panel path | — | ⏳ | — | — | Ctrl+x p | Ctrl+Shift+[ |
| Insert other panel path | — | ⏳ | — | — | Ctrl+x Ctrl+p | Ctrl+Shift+] |
| Insert left / right panel path | — | ⏳ | — | — | — | Ctrl+[ / Ctrl+] |
| Insert UNC path of left / right panel | — | ⏳ | — | — | — | Ctrl+Alt+[ / Ctrl+Alt+] |
| Insert UNC path of active / passive panel | — | ⏳ | — | — | — | Alt+Shift+[ / Alt+Shift+] |
| Completion | — | ⏳ | — | — | Alt+Tab | — |
| Insert character literally (quote) | — | ⏳ | — | — | Ctrl+q | — |
| Clear command line | — | ⏳ | — | — | — | Ctrl+y |
| Select block in command line | — | ⏳ | — | — | — | Alt+Shift+Left/Right/Home/End |

## Command history (`history`)

Alt+h in the panels and on the command line; characters filter it. Far's rows are those of its
history menus.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| One row up | `up` | ⏳ | Up | Up | — | — |
| One row down | `down` | ⏳ | Down | Down | — | — |
| One page up | `page_up` | ⏳ | PgUp | PgUp | — | — |
| One page down | `page_down` | ⏳ | PgDn | PgDn | — | — |
| First row | `home` | ⏳ | Home | Home | — | — |
| Last row | `end` | ⏳ | End | End | — | — |
| The commands of the panel's host, or of all hosts | `next_field` | ⏳ | Tab | Tab | — | — |
| Put the command on the command line, without running it | `confirm` | ⏳ | Enter | Enter | — | Ctrl+Enter |
| Take back the last character of the filter | `backspace` | ⏳ | Backspace | Backspace | — | — |
| Remove the command from the history | `delete` | ⏳ | Delete | Delete | — | Shift+Delete |
| Close the window | `cancel` | ⏳ | Esc<br>F10 | Esc<br>F10 | — | — |
| Re-run command / open item | — | ⏳ | — | — | — | Enter |
| Run in separate window | — | ⏳ | — | — | — | Shift+Enter |
| Run as administrator | — | ⏳ | — | — | — | Ctrl+Alt+Enter |
| Folder history menu: go to on passive panel | — | ⏳ | — | — | — | Ctrl+Shift+Enter |
| Clear history | — | ⏳ | — | — | — | Delete |
| Lock/unlock item | — | ⏳ | — | — | — | Insert |
| Refresh (remove unavailable) | — | ⏳ | — | — | — | Ctrl+r |
| Copy item to clipboard | — | ⏳ | — | — | — | Ctrl+c<br>Ctrl+Insert |
| View history menu: open in editor | — | ⏳ | — | — | — | F4 |
| View history menu: open in viewer | — | ⏳ | — | — | — | F3<br>Numpad5 |
| Command history menu: additional info | — | ⏳ | — | — | — | F3 |

## Output of commands (`user_screen`)

The terminal's own screen, with the output of commands, in place of the panels; only the keys
that bring the panels back do anything.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Back to the panels | `cancel` | ⏳ | Ctrl+o<br>Esc | Ctrl+o<br>Esc | Ctrl+o | Ctrl+o |

## Mouse

Not in the keymap: a press acts on what is under it.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Move the cursor to a row, activate the panel | — | ⏳ | Click | Click | Click | Click |
| Open the row (as Enter) | — | ⏳ | Double click | Double click | Double click | Double click |
| Mark the row, cursor stays on it | — | ⏳ | Right click | Right click | Right click | Right click |
| Scroll a panel or the viewer | — | ⏳ | Wheel | Wheel | Wheel | Wheel |
| Press an F key | — | ⏳ | Click on the F-key bar | Click on the F-key bar | Click on the button bar | Click on the key bar |
| Show a tab | — | ⏳ | Click on the tab | Click on the tab | — | — |
| Open a menu of the menu bar / run a command | — | ⏳ | Click | Click | Click | Click |
| Close the menu bar | — | ⏳ | Click outside it | Click outside it | Click outside it | Click outside it |
| Dialog: press a button, switch a check box, choose a radio button | — | ⏳ | Click | Click | Click | Click |
| Dialog: put the cursor in a text field | — | ⏳ | Click | Click | Click | Click |
| Dialog: choose a radio button and press the default button | — | ⏳ | Double click | Double click | — | — |
| Menu or list in a window: move the cursor / open the row | — | ⏳ | Click / Double click | Click / Double click | Click / Double click | Click / Double click |
| Close a menu or the list of completions | — | ⏳ | Click outside it | Click outside it | Click outside it | Click outside it |
| Scroll a menu, a list, or the help | — | ⏳ | Wheel | Wheel | Wheel | Wheel |

## Editor (mc, Far)

noc edits files in `$VISUAL` or `$EDITOR`.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Character left / right | — | ⏳ | — | — | — | Left / Right |
| Character left without wrapping to previous line | — | ⏳ | — | — | — | Ctrl+s |
| Line up / down | — | ⏳ | — | — | — | Up / Down |
| Word left / right | — | ⏳ | — | — | — | Ctrl+Left / Ctrl+Right |
| Scroll screen up / down | — | ⏳ | — | — | — | Ctrl+Up / Ctrl+Down |
| Page up / down | — | ⏳ | — | — | — | PgUp / PgDn |
| Beginning / end of line | — | ⏳ | — | — | — | Home / End |
| Beginning of file | — | ⏳ | — | — | — | Ctrl+Home<br>Ctrl+PgUp |
| End of file | — | ⏳ | — | — | — | Ctrl+End<br>Ctrl+PgDn |
| Beginning / end of screen | — | ⏳ | — | — | — | Ctrl+n / Ctrl+e |
| Delete character | — | ⏳ | — | — | — | Delete |
| Delete character left | — | ⏳ | — | — | — | Backspace |
| Delete line | — | ⏳ | — | — | — | Ctrl+y |
| Delete to end of line | — | ⏳ | — | — | — | Ctrl+k<br>Alt+d |
| Delete word left | — | ⏳ | — | — | — | Ctrl+Backspace |
| Delete word right | — | ⏳ | — | — | — | Ctrl+t<br>Ctrl+Delete |
| Block selection | — | ⏳ | — | — | Shift+cursor keys | Shift+cursor keys<br>Ctrl+Shift+cursor keys |
| Vertical block | — | ⏳ | — | — | — | Alt+cursor keys (not on the numpad)<br>Alt+Shift+cursor keys<br>Ctrl+Alt+cursor keys (not on the numpad) |
| Select all text | — | ⏳ | — | — | — | Ctrl+a |
| Clear selection | — | ⏳ | — | — | — | Ctrl+u |
| Copy block to clipboard | — | ⏳ | — | — | Ctrl+Insert (to mcedit.clip) | Ctrl+Insert<br>Ctrl+c |
| Paste from clipboard | — | ⏳ | — | — | Shift+Insert | Shift+Insert<br>Ctrl+v |
| Cut to clipboard | — | ⏳ | — | — | Shift+Delete | Shift+Delete<br>Ctrl+x |
| Append block to clipboard | — | ⏳ | — | — | — | Ctrl+Numpad+ |
| Delete block | — | ⏳ | — | — | Ctrl+Delete | Ctrl+d |
| Copy block to cursor position | — | ⏳ | — | — | — | Ctrl+p |
| Move block to cursor position | — | ⏳ | — | — | — | Ctrl+m |
| Indent block left / right | — | ⏳ | — | — | — | Alt+u / Alt+i |
| Format block | — | ⏳ | — | — | F19 | — |
| Help | — | ⏳ | — | — | — | F1 |
| Save | — | ⏳ | — | — | — | F2 |
| Save as | — | ⏳ | — | — | — | Shift+F2 |
| New file | — | ⏳ | — | — | — | Shift+F4 |
| Switch to viewer | — | ⏳ | — | — | — | F6 |
| Search | — | ⏳ | — | — | — | F7 |
| Replace | — | ⏳ | — | — | — | Ctrl+F7 |
| Continue search/replace forward / backward | — | ⏳ | — | — | — | Shift+F7 / Alt+F7 |
| Code page | — | ⏳ | — | — | Alt+e | F8 (OEM/ANSI)<br>Shift+F8 (select) |
| Go to line and position | — | ⏳ | — | — | — | Alt+F8 |
| Far window size | — | ⏳ | — | — | — | Alt+F9 |
| Editor settings | — | ⏳ | — | — | — | Alt+Shift+F9 |
| Quit | — | ⏳ | — | — | — | F10<br>F4<br>Esc |
| Save and quit | — | ⏳ | — | — | — | Shift+F10 |
| Locate file in panel | — | ⏳ | — | — | — | Ctrl+F10 |
| Plugin commands | — | ⏳ | — | — | — | F11 |
| View and edit history | — | ⏳ | — | — | — | Alt+F11 |
| Undo | — | ⏳ | — | — | — | Alt+Backspace<br>Ctrl+z |
| Redo | — | ⏳ | — | — | — | Ctrl+Z |
| Lock editing | — | ⏳ | — | — | — | Ctrl+l |
| User screen | — | ⏳ | — | — | — | Ctrl+o<br>Ctrl+Alt+Shift (temporarily) |
| Treat next key as character code | — | ⏳ | — | — | — | Ctrl+q |
| Set bookmark | — | ⏳ | — | — | — | RightCtrl+0…9<br>Ctrl+Shift+0…9 |
| Go to bookmark | — | ⏳ | — | — | — | LeftCtrl+0…9 |
| Insert current panel file name | — | ⏳ | — | — | — | Shift+Enter |
| Insert passive panel file name | — | ⏳ | — | — | — | Ctrl+Shift+Enter |
| Insert full name of edited file | — | ⏳ | — | — | — | Ctrl+f |
| Functional key bar | — | ⏳ | — | — | — | Ctrl+b |
| Status line | — | ⏳ | — | — | — | Ctrl+B |
| Record macro | — | ⏳ | — | — | Ctrl+r (start/stop) | Ctrl+. |
| Run macro | — | ⏳ | — | — | Ctrl+a, then key | — |

## Directory tree (mc)

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Rescan directory | — | ⏳ | — | — | Ctrl+r<br>F2 | — |
| Forget directory from tree | — | ⏳ | — | — | F3 | — |
| Static/dynamic navigation | — | ⏳ | — | — | F4 | — |
| Copy / move directory | — | ⏳ | — | — | F5 / F6 | — |
| Make subdirectory | — | ⏳ | — | — | F7 | — |
| Delete directory | — | ⏳ | — | — | F8 | — |
| Search next match | — | ⏳ | — | — | Ctrl+s<br>Alt+s | — |
| Delete last search character | — | ⏳ | — | — | Ctrl+h<br>Backspace | — |
| Help | — | ⏳ | — | — | F1 | — |
| Exit without changing directory | — | ⏳ | — | — | Esc<br>F10 | — |

## Diff viewer (mc)

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Help | — | ⏳ | — | — | F1 | — |
| Save changes | — | ⏳ | — | — | F2 | — |
| Edit left file | — | ⏳ | — | — | F4 | — |
| Edit right file | — | ⏳ | — | — | F14 | — |
| Merge current hunk | — | ⏳ | — | — | F5 | — |
| Search | — | ⏳ | — | — | F7 | — |
| Continue search | — | ⏳ | — | — | F17 | — |
| Quit | — | ⏳ | — | — | F10<br>Esc<br>q | — |
| Hunk status | — | ⏳ | — | — | Alt+s<br>s | — |
| Line numbers | — | ⏳ | — | — | Alt+n<br>l | — |
| Maximize left panel | — | ⏳ | — | — | f | — |
| Equalize panel widths | — | ⏳ | — | — | = | — |
| Shrink right / left panel | — | ⏳ | — | — | > / < | — |
| Show CR as ^M | — | ⏳ | — | — | c | — |
| Tab size | — | ⏳ | — | — | 2, 3, 4, 8 | — |
| Swap panels | — | ⏳ | — | — | Ctrl+u | — |
| Refresh screen | — | ⏳ | — | — | Ctrl+r | — |
| Show command screen | — | ⏳ | — | — | Ctrl+o | — |
| Next hunk | — | ⏳ | — | — | Enter<br>Space<br>n | — |
| Previous hunk | — | ⏳ | — | — | Backspace<br>p | — |
| Go to line | — | ⏳ | — | — | g | — |
| Line down / up | — | ⏳ | — | — | Down / Up | — |
| Page up / down | — | ⏳ | — | — | PgUp / PgDn | — |
| Beginning of line | — | ⏳ | — | — | Home<br>A1 | — |
| End of line | — | ⏳ | — | — | End | — |
| Beginning of file | — | ⏳ | — | — | Ctrl+Home | — |
| End of file | — | ⏳ | — | — | Ctrl+End<br>C1 | — |

## Sort modes menu (Far)

Ctrl+F12 in the panels.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Ascending / descending / invert | — | ⏳ | — | — | — | + / - / * |
| Additional criteria | — | ⏳ | — | — | — | F4 |
| Criteria: add / remove / replace | — | ⏳ | — | — | — | Insert / Delete / F4 |
| Criteria: inherit order from sort mode | — | ⏳ | — | — | — | = |
| Criteria: move up / down | — | ⏳ | — | — | — | Ctrl+Up / Ctrl+Down |
| Criteria: reset | — | ⏳ | — | — | — | Ctrl+r |

## Screen grabber (Far)

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Start | — | ⏳ | — | — | — | Alt+Insert |
| Stream/block mode | — | ⏳ | — | — | — | Space |
| Selection | — | ⏳ | — | — | — | Shift+cursor keys |
| Resize selected area | — | ⏳ | — | — | — | Alt+Shift+cursor keys |
| Move selected area | — | ⏳ | — | — | — | Alt+cursor keys |
| Copy to clipboard | — | ⏳ | — | — | — | Enter<br>Ctrl+Insert |
| Append to clipboard | — | ⏳ | — | — | — | Ctrl+Numpad+ |
| Cancel | — | ⏳ | — | — | — | Esc |
| Select whole screen | — | ⏳ | — | — | — | Ctrl+a |
| Clear selection | — | ⏳ | — | — | — | Ctrl+u |
| Selection horizontally by 10 | — | ⏳ | — | — | — | Ctrl+Shift+Left / Ctrl+Shift+Right |
| Selection vertically by 5 | — | ⏳ | — | — | — | Ctrl+Shift+Up / Ctrl+Shift+Down |

## Task list (Far)

Ctrl+w in the panels.

| Description | Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | :-: | --- | --- | --- | --- |
| Kill task | — | ⏳ | — | — | — | Delete |
| Refresh list | — | ⏳ | — | — | — | Ctrl+r |
| Window title / module path | — | ⏳ | — | — | — | F2 |
