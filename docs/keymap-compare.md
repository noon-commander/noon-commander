# Keyboard shortcuts: Noon Commander vs Midnight Commander vs Far Manager

Sources:

- noc: the presets in [keymap/default.rs](../crates/noc/src/tui/keymap/default.rs) and
  [keymap/vim.rs](../crates/noc/src/tui/keymap/vim.rs); the default preset is modelled on mc,
  `ui.keymap` chooses the preset, and `noc keymap diff` compares them.
- [mc.1.in](https://github.com/MidnightCommander/mc/blob/master/doc/man/mc.1.in)
- [FarEng.hlf.m4](https://github.com/FarGroup/FarManager/blob/master/far/FarEng.hlf.m4)

"—" — not bound in noc, or not documented in the source. Sections marked (noc) exist only in noc.
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

- ✅ implemented: noc binds the action.
- ⏳ not used yet: noc does not bind it, but may.
- 🚫 not needed: noc will not have it.

## Function keys and file operations

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | :-: | --- | --- | --- | --- |
| Help | ✅ | F1 | g ?<br>F1 | F1 | F1 |
| User menu | ⏳ | — | — | F2 | F2 |
| View file | ⏳ | F3 | i<br>F3 | F3 | F3<br>Numpad5<br>Ctrl+Shift+F3 (always internal) |
| View without preprocessing | ⏳ | — | — | F13 | — |
| Alternative (external/internal) viewer | ⏳ | — | — | — | Alt+F3 |
| Filtered view (command output) | ⏳ | — | — | Alt+! | — |
| Edit file | ⏳ | F4 | e<br>F4 | F4 | F4<br>Ctrl+Shift+F4 (always internal) |
| Alternative (external/internal) editor | ⏳ | — | — | — | Alt+F4 |
| Edit new file | ⏳ | — | — | F14 | Shift+F4 |
| Copy | ⏳ | F5 | y y<br>F5 | F5 | F5 |
| Copy current file (ignoring selection) | ⏳ | — | — | F15 | Shift+F5 |
| Rename/move | ⏳ | F6 | d d<br>F6 | F6 | F6 |
| Rename/move current file | ⏳ | Shift+F6<br>F16 (rename in its row) | c w<br>Shift+F6<br>F16 (rename in its row) | F16 | Shift+F6 |
| Make directory | ⏳ | F7 | F7 | F7 | F7 |
| Delete | ⏳ | F8<br>Delete | D D<br>F8<br>Delete | F8 | F8 |
| Disconnect host (virtual root, host list) | ⏳ | F8 | D D<br>F8 | — | — |
| Delete only the file under cursor | ⏳ | — | — | — | Shift+F8 |
| Delete bypassing Recycle Bin | ⏳ | — | — | — | Shift+Delete |
| Wipe | ⏳ | — | — | — | Alt+Delete |
| Abort copy/delete | ⏳ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10 | Ctrl+c<br>Esc | — |
| Copy/move in background (in dialog) | ⏳ | — | — | Alt+b | — |
| Menu bar | ✅ | F9 | F9 | F9 | F9 |
| Quit | ✅ | F10 | Z Z<br>F10 | F10 | F10 |
| Quit without changing to last directory (shell wrapper) | ⏳ | — | Z Q | Shift+F10 | — |
| F1…F10 on terminals without function keys | ⏳ | Esc, then 1…9, 0 (in panels) | Esc, then 1…9, 0 (in panels) | Esc, then 1…9, 0 | — |
| Alt+key on terminals without Alt | ⏳ | Esc, then the key (in panels) | Esc, then the key (in panels) | Esc, then the key | — |
| Plugin commands | ⏳ | — | — | — | F11 |
| Plugin configuration | ⏳ | — | — | — | Alt+Shift+F9 |
| Save setup | ⏳ | — | — | — | Shift+F9 |
| Repeat last menu item | ⏳ | — | — | — | Shift+F10 |
| Change drive in left panel | ⏳ | Alt+F1<br>Ctrl+x 1 | Alt+F1 | — | Alt+F1 |
| Change drive in right panel | ⏳ | Alt+F2<br>Ctrl+x 2 | Alt+F2 | — | Alt+F2 |
| Print files | ⏳ | — | — | — | Alt+F5 |
| Create link | ⏳ | — | — | Ctrl+x l (hard)<br>Ctrl+x s (absolute symlink)<br>Ctrl+x v (relative symlink) | Alt+F6 |
| Find file | ⏳ | — | — | Alt+? | Alt+F7 |
| Find folder | ⏳ | — | — | — | Alt+F10 |
| File permissions/attributes | ⏳ | — | — | Ctrl+x c (chmod) | Ctrl+a |
| Change owner (chown) | ⏳ | — | — | Ctrl+x o | — |
| File system attributes (chattr) | ⏳ | — | — | Ctrl+x e | — |
| Checksums | ⏳ | Ctrl+x # | Ctrl+x # | — | — |
| Apply command to selected files | ⏳ | — | — | — | Ctrl+g |
| Describe selected files | ⏳ | — | — | — | Ctrl+z |
| Add files to archive | ⏳ | — | — | — | Shift+F1 |
| Extract files from archive | ⏳ | — | — | — | Shift+F2 |
| Archive commands | ⏳ | — | — | — | Shift+F3 |
| Execute / change directory / enter archive | ✅ | Enter | l<br>Enter | Enter | Enter |
| Execute in separate window | ⏳ | — | — | — | Shift+Enter |
| Run as administrator | ⏳ | — | — | — | Ctrl+Alt+Enter |

## General commands

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Repaint screen | ✅ | Ctrl+l | Ctrl+l | Ctrl+l | — |
| Show command output / user screen | ⏳ | Ctrl+o (back: Ctrl+o, Esc) | Ctrl+o (back: Ctrl+o, Esc) | Ctrl+o | Ctrl+o |
| Temporarily show user screen (while held) | ⏳ | — | — | — | Ctrl+Alt+Shift |
| Quick cd | ⏳ | Alt+c | c d | Alt+c | — |
| Jump to a directory zoxide ranks | ⏳ | Alt+z<br>Ctrl+x z | Alt+z | — | — |
| External panelize | ⏳ | — | — | Ctrl+x ! | — |
| Add current directory to hotlist / folder shortcut | ⏳ | — | — | Ctrl+x h | Ctrl+Shift+0…9 |
| Go to directory from hotlist / folder shortcut | ⏳ | — | — | Ctrl+\ (list) | RightCtrl+0…9 |
| Change panel charset | ⏳ | — | — | Alt+e | — |
| Toggle panel split (vertical/horizontal) | ⏳ | — | — | Alt+, | — |
| Change window size | ⏳ | — | — | — | Alt+F9 |
| Task list | ⏳ | — | — | — | Ctrl+w |
| Background jobs | ⏳ | Ctrl+x j | w | — | — |
| Screen grabber | ⏳ | — | — | — | Alt+Insert |
| Record keyboard macro | ⏳ | — | — | Ctrl+r (in editor) | Ctrl+. |
| Run macro | ⏳ | — | — | Ctrl+a, then assigned key (in editor) | — |

## History

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Command history (list) | ⏳ | Alt+h<br>Ctrl+r (command line) | q : (panel)<br>Ctrl+r (command line) | Alt+h | Alt+F8 |
| Previous command (the panel's host) | ⏳ | Up (first line)<br>Alt+p | Up (first line)<br>Ctrl+p | Alt+p | Ctrl+e |
| Next command (the panel's host) | ⏳ | Down (last line)<br>Alt+n | Down (last line)<br>Ctrl+n | Alt+n | Ctrl+x |
| Directory history (list) | ⏳ | — | — | Alt+H | Alt+F12 |
| Previous directory in history | ⏳ | — | — | Alt+y | — |
| Next directory in history | ⏳ | — | — | Alt+u | — |
| View and edit history | ⏳ | — | — | — | Alt+F11 |
| History menu: re-run command / open item | ⏳ | — | — | — | Enter |
| History menu: run in separate window | ⏳ | — | — | — | Shift+Enter |
| History menu: run as administrator | ⏳ | — | — | — | Ctrl+Alt+Enter |
| History menu: put into command line | ⏳ | Enter | Enter | — | Ctrl+Enter |
| Folder history menu: go to on passive panel | ⏳ | — | — | — | Ctrl+Shift+Enter |
| History menu: clear history | ⏳ | — | — | — | Delete |
| History menu: delete current item | ⏳ | Delete | Delete | — | Shift+Delete |
| Command history menu: this host / all hosts | ⏳ | Tab | Tab | — | — |
| History menu: lock/unlock item | ⏳ | — | — | — | Insert |
| History menu: refresh (remove unavailable) | ⏳ | — | — | — | Ctrl+r |
| History menu: copy item to clipboard | ⏳ | — | — | — | Ctrl+c<br>Ctrl+Insert |
| View history menu: open in editor | ⏳ | — | — | — | F4 |
| View history menu: open in viewer | ⏳ | — | — | — | F3<br>Numpad5 |
| Command history menu: additional info | ⏳ | — | — | — | F3 |

## Panel control

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Change active panel | ⏳ | Tab | Tab<br>Ctrl+w w | Tab<br>Ctrl+i<br>Left<br>Right | Tab |
| Swap panels | ⏳ | Ctrl+u | Ctrl+w x | — | Ctrl+u |
| Reread panel | ⏳ | Ctrl+r | R | — | Ctrl+r |
| Stop loading or connecting | ⏳ | Esc<br>Esc Esc | Esc<br>Ctrl+c | — | — |
| Info panel | ⏳ | — | — | Ctrl+x i (on other panel) | Ctrl+l |
| Quick view panel | ⏳ | — | — | Ctrl+x q (on other panel) | Ctrl+q |
| Folder tree | ⏳ | — | — | — | Ctrl+t |
| Hide/show both panels | ⏳ | Ctrl+o | Ctrl+o | Ctrl+o | Ctrl+o |
| Hide/show inactive panel | ⏳ | — | — | — | Ctrl+p |
| Hide/show left panel | ⏳ | — | — | — | Ctrl+F1 |
| Hide/show right panel | ⏳ | — | — | — | Ctrl+F2 |
| Change panels height | ⏳ | — | — | — | Ctrl+Up<br>Ctrl+Down |
| Change current panel height | ⏳ | — | — | — | Ctrl+Shift+Up<br>Ctrl+Shift+Down |
| Change panels width (with empty command line) | ⏳ | — | — | — | Ctrl+Left<br>Ctrl+Right |
| Restore default panels width | ⏳ | — | — | — | Ctrl+Numpad5 |
| Restore default panels height | ⏳ | — | — | — | Ctrl+Alt+Numpad5 |
| Hide/show functional key bar | ⏳ | — | — | — | Ctrl+b |
| Sizes in bytes / with K/M/G/T suffixes | ⏳ | — | — | — | Ctrl+S |
| Next listing format | ⏳ | — | — | Alt+t | — |
| Brief view mode | ⏳ | — | — | — | LeftCtrl+1 |
| Medium view mode | ⏳ | — | — | — | LeftCtrl+2 |
| Full view mode | ⏳ | — | — | — | LeftCtrl+3 |
| Wide view mode | ⏳ | — | — | — | LeftCtrl+4 |
| Detailed view mode | ⏳ | — | — | — | LeftCtrl+5 |
| Descriptions view mode | ⏳ | — | — | — | LeftCtrl+6 |
| Long descriptions view mode | ⏳ | — | — | — | LeftCtrl+7 |
| File owners view mode | ⏳ | — | — | — | LeftCtrl+8 |
| File links view mode | ⏳ | — | — | — | LeftCtrl+9 |
| Alternative full view mode | ⏳ | — | — | — | LeftCtrl+0 |
| Hidden and system files | ⏳ | Alt+. | z h | — | Ctrl+h |
| Long/short names | ⏳ | — | — | — | Ctrl+n |
| Scroll long names | ⏳ | — | — | Alt+(<br>Alt+) | Alt+Left<br>Alt+Right<br>Alt+Home<br>Alt+End |
| Open directory under cursor on other panel | ⏳ | Alt+o | Alt+o | Alt+o | — |
| Current directory to other panel | ⏳ | Alt+i | Alt+i | Alt+i | — |
| Go to parent directory | ✅ | Ctrl+PgUp | h<br>- | Ctrl+PgUp | Ctrl+PgUp |
| Enter directory / archive | ⏳ | — | — | Ctrl+PgDn | Ctrl+PgDn<br>Ctrl+Shift+PgDn (always as archive) |
| Go to root directory | ⏳ | — | — | — | Ctrl+\ |
| Cursor up | ✅ | Up<br>Ctrl+p | k<br>Up | Up<br>Ctrl+p | — |
| Cursor down | ✅ | Down<br>Ctrl+n | j<br>Down | Down<br>Ctrl+n | — |
| First entry | ✅ | Home | g g<br>Home | Home<br>A1<br>Alt+< | — |
| Last entry | ✅ | End | G<br>End | End<br>C1<br>Alt+> | — |
| Page down | ⏳ | PgDn<br>Ctrl+v | Ctrl+f<br>PgDn | PgDn<br>Ctrl+v | — |
| Page up | ⏳ | PgUp<br>Alt+v | Ctrl+b<br>PgUp | PgUp<br>Alt+v | — |
| Top / middle / bottom file on screen | ⏳ | — | — | Alt+g / Alt+r / Alt+j | — |

## Sorting

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| By name | ⏳ | Ctrl+F3 | o n | — | Ctrl+F3 |
| By extension | ⏳ | Ctrl+F4 | o e | — | Ctrl+F4 |
| By write time | ⏳ | Ctrl+F5 | o m | — | Ctrl+F5 |
| By size | ⏳ | Ctrl+F6 | o s | — | Ctrl+F6 |
| Unsorted | ⏳ | — | — | — | Ctrl+F7 |
| By creation time | ⏳ | — | — | — | Ctrl+F8 |
| By access time | ⏳ | — | — | — | Ctrl+F9 |
| By description | ⏳ | — | — | — | Ctrl+F10 |
| By owner | ⏳ | — | — | — | Ctrl+F11 |
| Sort modes menu | ⏳ | — | — | — | Ctrl+F12 |
| Use sort groups | ⏳ | — | — | — | Shift+F11 |
| Show selected files first | ⏳ | — | — | — | Shift+F12 |
| Sort menu: ascending / descending / invert | ⏳ | — | — | — | + / - / * |
| Sort menu: additional criteria | ⏳ | — | — | — | F4 |
| Criteria: add / remove / replace | ⏳ | — | — | — | Insert / Delete / F4 |
| Criteria: inherit order from sort mode | ⏳ | — | — | — | = |
| Criteria: move up / down | ⏳ | — | — | — | Ctrl+Up / Ctrl+Down |
| Criteria: reset | ⏳ | — | — | — | Ctrl+r |

## File selection

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Select/deselect file | ⏳ | Insert<br>Ctrl+t<br>Shift+Down<br>Shift+Up (moves up)<br>Right click | Space<br>Insert<br>Shift+Down<br>Shift+Up (moves up)<br>Right click | Insert<br>Ctrl+t<br>Right click | Insert<br>Shift+cursor keys<br>Right click |
| Select group | ⏳ | +<br>Alt++ | + | +<br>Alt++ (alternate_plus_minus) | Numpad+ |
| Deselect group | ⏳ | -<br>\\<br>Alt+- | \\ | \\<br>Alt+- (alternate_plus_minus) | Numpad- |
| Invert selection | ⏳ | \*<br>Alt+\* (files only) | v | \*<br>Alt+\* (alternate_plus_minus) | Numpad* |
| Select files with same extension | ⏳ | — | — | — | Ctrl+Numpad+ |
| Deselect files with same extension | ⏳ | — | — | — | Ctrl+Numpad- |
| Invert selection including folders | ⏳ | — | — | — | Ctrl+Numpad* |
| Select files with same name | ⏳ | — | — | — | Alt+Numpad+ |
| Deselect files with same name | ⏳ | — | — | — | Alt+Numpad- |
| Invert selection of files, deselect folders | ⏳ | — | — | — | Alt+Numpad* |
| Select all files | ⏳ | — | — | — | Shift+Numpad+ |
| Deselect all files | ⏳ | — | — | — | Shift+Numpad- |
| Restore previous selection | ⏳ | — | — | — | Ctrl+m |

## Clipboard in panels

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Selected names to clipboard | ⏳ | — | — | — | Ctrl+Insert (with empty command line)<br>Ctrl+Shift+Insert |
| Full names to clipboard | ⏳ | — | — | — | Alt+Shift+Insert |
| Real (UNC) names to clipboard | ⏳ | — | — | — | Ctrl+Alt+Insert |
| Copy files to clipboard | ⏳ | — | — | — | Ctrl+C |
| Cut files to clipboard | ⏳ | — | — | — | Ctrl+X |

## Quick search in panel

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Start quick search | ⏳ | Ctrl+s<br>Alt+s | / | Ctrl+s<br>Alt+s | Alt+letters<br>Alt+Shift+letters |
| Next match | ⏳ | Ctrl+s<br>Alt+s | Ctrl+g | Ctrl+s | Ctrl+Enter |
| Previous match | ⏳ | — | — | — | Ctrl+Shift+Enter |
| Search with previous pattern | ⏳ | — | — | Ctrl+s Ctrl+s | — |
| Correct typing | ⏳ | Backspace | Backspace<br>Ctrl+h | Backspace<br>Delete | — |
| End quick search | ⏳ | Esc | Esc | — | — |
| Paste from clipboard | ⏳ | — | — | — | Ctrl+v<br>Shift+Insert |

## Command line

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Open the command line for a shell command | ✅ | ! | ! | Typing | Typing |
| Open the command line for noc commands | ✅ | : | : | — | — |
| Run the command | ⏳ | Enter | Enter | Enter | Enter |
| New line in the command | ⏳ | Ctrl+j<br>\\ Enter | Ctrl+j<br>\\ Enter | — | — |
| New line in the command (kitty keyboard protocol) | ⏳ | Shift+Enter | Shift+Enter | — | — |
| Edit the command in $EDITOR | ⏳ | Ctrl+x Ctrl+e | Ctrl+f<br>Ctrl+x Ctrl+e | — | — |
| Close the command line | ⏳ | Esc | Esc<br>Ctrl+c | — | Esc |
| Line above / below in the command | ⏳ | Up / Down | Up / Down | — | — |
| Insert current file name | ⏳ | — | — | Alt+Enter<br>Ctrl+Enter | Ctrl+j<br>Ctrl+Enter |
| Insert file name from passive panel | ⏳ | — | — | — | Ctrl+Shift+Enter |
| Insert full name of current file | ⏳ | — | — | Ctrl+Shift+Enter | Ctrl+f |
| Insert full file name from passive panel | ⏳ | — | — | — | Ctrl+; |
| Insert UNC file name (active / passive) | ⏳ | — | — | — | Ctrl+Alt+f / Ctrl+Alt+; |
| Insert tagged files of current panel | ⏳ | — | — | Ctrl+x t | — |
| Insert tagged files of other panel | ⏳ | — | — | Ctrl+x Ctrl+t | — |
| Insert current panel path | ⏳ | — | — | Ctrl+x p | Ctrl+Shift+[ |
| Insert other panel path | ⏳ | — | — | Ctrl+x Ctrl+p | Ctrl+Shift+] |
| Insert left / right panel path | ⏳ | — | — | — | Ctrl+[ / Ctrl+] |
| Insert UNC path of left / right panel | ⏳ | — | — | — | Ctrl+Alt+[ / Ctrl+Alt+] |
| Insert UNC path of active / passive panel | ⏳ | — | — | — | Alt+Shift+[ / Alt+Shift+] |
| Completion | ⏳ | — | — | Alt+Tab | — |
| Insert character literally (quote) | ⏳ | — | — | Ctrl+q | — |
| Clear command line | ⏳ | — | — | — | Ctrl+y |
| Select block in command line | ⏳ | — | — | — | Alt+Shift+Left/Right/Home/End |

## Input lines

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Character left | ⏳ | Left | Left | Ctrl+b<br>Left | Left<br>Ctrl+s |
| Character right | ⏳ | Right | Right | Ctrl+f<br>Right | Right<br>Ctrl+d |
| Word left | ⏳ | — | — | Alt+b | Ctrl+Left |
| Word right | ⏳ | — | — | Alt+f | Ctrl+Right |
| Beginning of line | ⏳ | Home<br>Ctrl+a | Home<br>Ctrl+b | Ctrl+a | Ctrl+Home |
| End of line | ⏳ | End<br>Ctrl+e | End<br>Ctrl+e | Ctrl+e | Ctrl+End |
| Delete character left | ⏳ | Backspace | Backspace<br>Ctrl+h | Ctrl+h<br>Backspace | Backspace |
| Delete character under cursor | ⏳ | Delete | Delete | Ctrl+d<br>Delete | Delete |
| Delete word left | ⏳ | — | Ctrl+w | Ctrl+Alt+h<br>Alt+Backspace | Ctrl+Backspace |
| Delete word right | ⏳ | — | — | — | Ctrl+Delete |
| Delete to end of line | ⏳ | Ctrl+k | Ctrl+k | Ctrl+k | Ctrl+k |
| Delete to beginning of line | ⏳ | Ctrl+u | Ctrl+u | — | — |
| Set mark | ⏳ | — | — | Ctrl+@ | — |
| Cut (mark to cursor) | ⏳ | — | — | Ctrl+w | — |
| Copy | ⏳ | — | — | Alt+w | Ctrl+Insert |
| Paste | ⏳ | — | — | Ctrl+y | Shift+Insert |
| Input line history | ⏳ | — | — | Alt+h | Ctrl+Up<br>Ctrl+Down (in dialogs) |
| Previous / next history entry | ⏳ | — | — | Alt+p / Alt+n | — |
| Complete path (path fields) | ⏳ | Tab | Tab | Alt+Tab | — |

## Menus and dialogs

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Filter menu items | ⏳ | Typing (location menu) | Typing (location menu) | — | Ctrl+Alt+f<br>RightAlt |
| Lock filter | ⏳ | — | — | — | Ctrl+Alt+l |
| Shift all items by 1 position | ⏳ | — | — | — | Alt+Left<br>Alt+Right |
| Shift selected item by 1 position | ⏳ | — | — | — | Alt+Shift+Left<br>Alt+Shift+Right |
| Shift all items by 20 positions | ⏳ | — | — | — | Ctrl+Alt+Left<br>Ctrl+Alt+Right |
| Shift selected item by 20 positions | ⏳ | — | — | — | Ctrl+Shift+Left<br>Ctrl+Shift+Right |
| Align all items left / right | ⏳ | — | — | — | Alt+Home / Alt+End |
| Align selected item left / right | ⏳ | — | — | — | Alt+Shift+Home / Alt+Shift+End |
| Fixed menu columns | ⏳ | — | — | — | Shift+F5 |
| Close dialog | ✅ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10 | Esc Esc<br>Esc (if Esc key mode is enabled) | — |
| Next dialog item | ⏳ | Tab<br>Down<br>Right | Tab<br>j (not in a text field)<br>Down<br>Right | — | — |
| Previous dialog item | ⏳ | Shift+Tab<br>Up<br>Left | Shift+Tab<br>k (not in a text field)<br>Up<br>Left | — | — |
| Press focused button | ⏳ | Enter<br>Space | Enter<br>Space | — | — |
| Switch check box / choose radio button | ⏳ | Space | Space | — | — |
| List in a window (jobs, checksums): row up / down | ⏳ | Up / Down | k / j<br>Up / Down | — | — |
| List in a window: page up / down | ⏳ | PgUp / PgDn | Ctrl+b / Ctrl+f<br>PgUp / PgDn | — | — |
| List in a window: first / last row | ⏳ | Home / End | g g / G<br>Home / End | — | — |
| Focus first dialog item | ⏳ | — | — | — | Home |
| Focus default dialog item | ⏳ | — | — | — | PgDn<br>End |
| Default action | ⏳ | Enter (from a text field or check box) | Enter (from a text field or check box) | — | Ctrl+Enter |
| Move dialog | ⏳ | — | — | — | Ctrl+F5 |
| Checkbox: on / off / undefined | ⏳ | — | — | — | Numpad+ / Numpad- / Numpad* |
| Dialog history: clear | ⏳ | — | — | — | Delete |
| Dialog history: delete item | ⏳ | — | — | — | Shift+Delete |
| Dialog history: mark item | ⏳ | — | — | — | Insert |
| Insert file name under cursor into dialog | ⏳ | — | — | — | Shift+Enter |
| Insert passive panel file name into dialog | ⏳ | — | — | — | Ctrl+Shift+Enter |

## Screen switching

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Next screen | ⏳ | — | — | Alt+} | Ctrl+Tab |
| Previous screen | ⏳ | — | — | Alt+{ | Ctrl+Shift+Tab |
| Screen list | ⏳ | — | — | Alt+` | F12 |

## Viewer

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Help | ✅ | F1 | g ?<br>F1 | F1 | F1 |
| Toggle line wrap | ⏳ | F2 | F2 | F2 | F2 |
| Wrap type (by chars/words) | ⏳ | — | — | — | Shift+F2 |
| Hex/code mode | ⏳ | — | — | F4 | F4 |
| Select mode (text/code/dump) | ⏳ | — | — | — | Shift+F4 |
| Go to position | ⏳ | — | — | F5 | Alt+F8 |
| Switch to editor | ⏳ | — | — | — | F6 |
| Search | ⏳ | — | — | F7<br>/<br>? (backward) | F7 |
| Continue search forward | ⏳ | — | — | Ctrl+s | Shift+F7<br>Space |
| Continue search backward | ⏳ | — | — | Ctrl+r | Alt+F7 |
| Continue search in chosen direction | ⏳ | — | — | F17<br>n | — |
| Temporarily reverse search direction | ⏳ | — | — | N | — |
| Raw/Parsed | ⏳ | — | — | F8 | — |
| Format/Unformat | ⏳ | — | — | F9 | — |
| Code page | ⏳ | — | — | Alt+e | F8 (OEM/ANSI)<br>Shift+F8 (menu) |
| Quit | ⏳ | F3<br>F10<br>q<br>Esc | q<br>Esc<br>F3<br>F10 | F10<br>Esc | F10<br>F3<br>Numpad5<br>Esc |
| Line down | ✅ | Down<br>j<br>e<br>Enter<br>Ctrl+n | j<br>Ctrl+n<br>Down<br>Enter | Down<br>Ctrl+n | Down |
| Line up | ✅ | Up<br>k<br>y<br>Ctrl+p | k<br>Ctrl+p<br>Up | Up<br>Ctrl+p | Up |
| Page down | ✅ | PgDn<br>Space<br>f<br>Ctrl+v | Ctrl+f<br>PgDn | PgDn<br>Space<br>Ctrl+v | PgDn |
| Page up | ✅ | PgUp<br>b<br>Alt+v<br>Backspace | Ctrl+b<br>PgUp | PgUp<br>Alt+v<br>Ctrl+b<br>b<br>Ctrl+h<br>Backspace<br>Delete | PgUp |
| Half page up / down | ⏳ | — | — | u / d | — |
| Beginning of file | ✅ | Home<br>g<br>Ctrl+Home | g g<br>Home | Home<br>A1<br>g | Home<br>Ctrl+Home |
| End of file | ✅ | End<br>G<br>Ctrl+End | G<br>End | End<br>C1<br>G | End<br>Ctrl+End |
| Column left / right | ⏳ | Left / Right<br>h / l | h / l<br>Left / Right | — | Left / Right |
| 20 columns left / right | ⏳ | — | — | — | Ctrl+Left / Ctrl+Right |
| Leftmost / rightmost column | ⏳ | — | — | — | Ctrl+Shift+Left / Ctrl+Shift+Right |
| Shift characters/bytes (dump, code) | ⏳ | — | — | — | Ctrl+Left / Ctrl+Right |
| Bytes per line −1 / +1 (code) | ⏳ | — | — | — | Alt+Left / Alt+Right |
| Bytes per line to nearest multiple of 16 (code) | ⏳ | — | — | — | Ctrl+Alt+Left / Ctrl+Alt+Right |
| Set bookmark | ⏳ | — | — | [n] m | RightCtrl+0…9<br>Ctrl+Shift+0…9 |
| Go to bookmark | ⏳ | — | — | [n] r | LeftCtrl+0…9 |
| Return to previous position | ⏳ | — | — | — | Alt+Backspace<br>Ctrl+z |
| Next file | ⏳ | — | — | Ctrl+f | Numpad+ |
| Previous file | ⏳ | — | — | Ctrl+b | Numpad- |
| Ruler | ⏳ | — | — | Alt+r | — |
| Repaint screen | ✅ | Ctrl+l | Ctrl+l | Ctrl+l | — |
| User screen | ⏳ | — | — | Ctrl+o | Ctrl+o<br>Ctrl+Alt+Shift (temporarily) |
| Go to file in panel | ⏳ | — | — | — | Ctrl+F10 |
| Plugin commands | ⏳ | — | — | — | F11 |
| View and edit history | ⏳ | — | — | — | Alt+F11 |
| Far window size | ⏳ | — | — | — | Alt+F9 |
| Viewer settings | ⏳ | — | — | — | Alt+Shift+F9 |
| Functional key bar | ⏳ | — | — | — | Ctrl+b |
| Status line | ⏳ | — | — | — | Ctrl+B |
| Scrollbar | ⏳ | — | — | — | Ctrl+s |
| Copy selection | ⏳ | — | — | — | Ctrl+Insert<br>Ctrl+c |
| Clear selection | ⏳ | — | — | — | Ctrl+u |
| Select text manually | ⏳ | — | — | — | Shift+Click |

## Editor

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Character left / right | ⏳ | — | — | — | Left / Right |
| Character left without wrapping to previous line | ⏳ | — | — | — | Ctrl+s |
| Line up / down | ⏳ | — | — | — | Up / Down |
| Word left / right | ⏳ | — | — | — | Ctrl+Left / Ctrl+Right |
| Scroll screen up / down | ⏳ | — | — | — | Ctrl+Up / Ctrl+Down |
| Page up / down | ⏳ | — | — | — | PgUp / PgDn |
| Beginning / end of line | ⏳ | — | — | — | Home / End |
| Beginning of file | ⏳ | — | — | — | Ctrl+Home<br>Ctrl+PgUp |
| End of file | ⏳ | — | — | — | Ctrl+End<br>Ctrl+PgDn |
| Beginning / end of screen | ⏳ | — | — | — | Ctrl+n / Ctrl+e |
| Delete character | ⏳ | — | — | — | Delete |
| Delete character left | ⏳ | — | — | — | Backspace |
| Delete line | ⏳ | — | — | — | Ctrl+y |
| Delete to end of line | ⏳ | — | — | — | Ctrl+k<br>Alt+d |
| Delete word left | ⏳ | — | — | — | Ctrl+Backspace |
| Delete word right | ⏳ | — | — | — | Ctrl+t<br>Ctrl+Delete |
| Block selection | ⏳ | — | — | Shift+cursor keys | Shift+cursor keys<br>Ctrl+Shift+cursor keys |
| Vertical block | ⏳ | — | — | — | Alt+cursor keys (not on the numpad)<br>Alt+Shift+cursor keys<br>Ctrl+Alt+cursor keys (not on the numpad) |
| Select all text | ⏳ | — | — | — | Ctrl+a |
| Clear selection | ⏳ | — | — | — | Ctrl+u |
| Copy block to clipboard | ⏳ | — | — | Ctrl+Insert (to mcedit.clip) | Ctrl+Insert<br>Ctrl+c |
| Paste from clipboard | ⏳ | — | — | Shift+Insert | Shift+Insert<br>Ctrl+v |
| Cut to clipboard | ⏳ | — | — | Shift+Delete | Shift+Delete<br>Ctrl+x |
| Append block to clipboard | ⏳ | — | — | — | Ctrl+Numpad+ |
| Delete block | ⏳ | — | — | Ctrl+Delete | Ctrl+d |
| Copy block to cursor position | ⏳ | — | — | — | Ctrl+p |
| Move block to cursor position | ⏳ | — | — | — | Ctrl+m |
| Indent block left / right | ⏳ | — | — | — | Alt+u / Alt+i |
| Format block | ⏳ | — | — | F19 | — |
| Help | ⏳ | — | — | — | F1 |
| Save | ⏳ | — | — | — | F2 |
| Save as | ⏳ | — | — | — | Shift+F2 |
| New file | ⏳ | — | — | — | Shift+F4 |
| Switch to viewer | ⏳ | — | — | — | F6 |
| Search | ⏳ | — | — | — | F7 |
| Replace | ⏳ | — | — | — | Ctrl+F7 |
| Continue search/replace forward / backward | ⏳ | — | — | — | Shift+F7 / Alt+F7 |
| Code page | ⏳ | — | — | Alt+e | F8 (OEM/ANSI)<br>Shift+F8 (select) |
| Go to line and position | ⏳ | — | — | — | Alt+F8 |
| Far window size | ⏳ | — | — | — | Alt+F9 |
| Editor settings | ⏳ | — | — | — | Alt+Shift+F9 |
| Quit | ⏳ | — | — | — | F10<br>F4<br>Esc |
| Save and quit | ⏳ | — | — | — | Shift+F10 |
| Locate file in panel | ⏳ | — | — | — | Ctrl+F10 |
| Plugin commands | ⏳ | — | — | — | F11 |
| View and edit history | ⏳ | — | — | — | Alt+F11 |
| Undo | ⏳ | — | — | — | Alt+Backspace<br>Ctrl+z |
| Redo | ⏳ | — | — | — | Ctrl+Z |
| Lock editing | ⏳ | — | — | — | Ctrl+l |
| User screen | ⏳ | — | — | — | Ctrl+o<br>Ctrl+Alt+Shift (temporarily) |
| Treat next key as character code | ⏳ | — | — | — | Ctrl+q |
| Set bookmark | ⏳ | — | — | — | RightCtrl+0…9<br>Ctrl+Shift+0…9 |
| Go to bookmark | ⏳ | — | — | — | LeftCtrl+0…9 |
| Insert current panel file name | ⏳ | — | — | — | Shift+Enter |
| Insert passive panel file name | ⏳ | — | — | — | Ctrl+Shift+Enter |
| Insert full name of edited file | ⏳ | — | — | — | Ctrl+f |
| Functional key bar | ⏳ | — | — | — | Ctrl+b |
| Status line | ⏳ | — | — | — | Ctrl+B |
| Record macro | ⏳ | — | — | Ctrl+r (start/stop) | Ctrl+. |
| Run macro | ⏳ | — | — | Ctrl+a, then key | — |

## Directory tree (mc)

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Rescan directory | ⏳ | — | — | Ctrl+r<br>F2 | — |
| Forget directory from tree | ⏳ | — | — | F3 | — |
| Static/dynamic navigation | ⏳ | — | — | F4 | — |
| Copy / move directory | ⏳ | — | — | F5 / F6 | — |
| Make subdirectory | ⏳ | — | — | F7 | — |
| Delete directory | ⏳ | — | — | F8 | — |
| Search next match | ⏳ | — | — | Ctrl+s<br>Alt+s | — |
| Delete last search character | ⏳ | — | — | Ctrl+h<br>Backspace | — |
| Help | ⏳ | — | — | F1 | — |
| Exit without changing directory | ⏳ | — | — | Esc<br>F10 | — |

## Diff viewer (mc)

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Help | ⏳ | — | — | F1 | — |
| Save changes | ⏳ | — | — | F2 | — |
| Edit left file | ⏳ | — | — | F4 | — |
| Edit right file | ⏳ | — | — | F14 | — |
| Merge current hunk | ⏳ | — | — | F5 | — |
| Search | ⏳ | — | — | F7 | — |
| Continue search | ⏳ | — | — | F17 | — |
| Quit | ⏳ | — | — | F10<br>Esc<br>q | — |
| Hunk status | ⏳ | — | — | Alt+s<br>s | — |
| Line numbers | ⏳ | — | — | Alt+n<br>l | — |
| Maximize left panel | ⏳ | — | — | f | — |
| Equalize panel widths | ⏳ | — | — | = | — |
| Shrink right / left panel | ⏳ | — | — | > / < | — |
| Show CR as ^M | ⏳ | — | — | c | — |
| Tab size | ⏳ | — | — | 2, 3, 4, 8 | — |
| Swap panels | ⏳ | — | — | Ctrl+u | — |
| Refresh screen | ⏳ | — | — | Ctrl+r | — |
| Show command screen | ⏳ | — | — | Ctrl+o | — |
| Next hunk | ⏳ | — | — | Enter<br>Space<br>n | — |
| Previous hunk | ⏳ | — | — | Backspace<br>p | — |
| Go to line | ⏳ | — | — | g | — |
| Line down / up | ⏳ | — | — | Down / Up | — |
| Page up / down | ⏳ | — | — | PgUp / PgDn | — |
| Beginning of line | ⏳ | — | — | Home<br>A1 | — |
| End of line | ⏳ | — | — | End | — |
| Beginning of file | ⏳ | — | — | Ctrl+Home | — |
| End of file | ⏳ | — | — | Ctrl+End<br>C1 | — |

## Help viewer

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Follow link | ⏳ | — | — | — | Enter |
| Next / previous link | ⏳ | — | — | — | Tab / Shift+Tab |
| Page forward / backward | ⏳ | PgDn / PgUp | Ctrl+f / Ctrl+b<br>PgDn / PgUp | Space / Backspace | — |
| Previous topic | ⏳ | — | — | — | Alt+F1<br>Backspace |
| Contents | ⏳ | — | — | — | Shift+F1 |
| Plugins help | ⏳ | — | — | — | Shift+F2 |
| Search in help | ⏳ | — | — | — | F7 |
| Maximize/restore window | ⏳ | — | — | — | F5 |
| Full list of help keys | ⏳ | — | — | F1 (again) | — |
| Line down / up | ⏳ | Down / Up | j / k<br>Down / Up | — | — |
| Beginning / end | ⏳ | Home / End | g g / G<br>Home / End | — | — |
| Close help | ⏳ | Esc<br>F10<br>Enter | q<br>Esc<br>F10<br>Enter | — | — |

## Screen grabber (Far)

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Start | ⏳ | — | — | — | Alt+Insert |
| Stream/block mode | ⏳ | — | — | — | Space |
| Selection | ⏳ | — | — | — | Shift+cursor keys |
| Resize selected area | ⏳ | — | — | — | Alt+Shift+cursor keys |
| Move selected area | ⏳ | — | — | — | Alt+cursor keys |
| Copy to clipboard | ⏳ | — | — | — | Enter<br>Ctrl+Insert |
| Append to clipboard | ⏳ | — | — | — | Ctrl+Numpad+ |
| Cancel | ⏳ | — | — | — | Esc |
| Select whole screen | ⏳ | — | — | — | Ctrl+a |
| Clear selection | ⏳ | — | — | — | Ctrl+u |
| Selection horizontally by 10 | ⏳ | — | — | — | Ctrl+Shift+Left / Ctrl+Shift+Right |
| Selection vertically by 5 | ⏳ | — | — | — | Ctrl+Shift+Up / Ctrl+Shift+Down |

## Task list (Far)

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Kill task | ⏳ | — | — | — | Delete |
| Refresh list | ⏳ | — | — | — | Ctrl+r |
| Window title / module path | ⏳ | — | — | — | F2 |

## Tabs (noc)

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| New tab on the same location | ⏳ | Ctrl+x t | g n | — | — |
| Close tab | ⏳ | Ctrl+x w | g c | — | — |
| Next tab | ⏳ | Alt+Right<br>Ctrl+x n | g t<br>Alt+Right | — | — |
| Previous tab | ⏳ | Alt+Left<br>Ctrl+x p | g T<br>Alt+Left | — | — |
| Tab list | ⏳ | Ctrl+x Tab | Ctrl+x Tab | — | — |
| Workspaces window | ⏳ | Alt+w | Alt+w | — | — |
| Save the tabs of both panels as a workspace | ⏳ | Alt+W | Alt+W | — | — |

## Mouse

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Move the cursor to a row, activate the panel | ⏳ | Click | Click | Click | Click |
| Open the row (as Enter) | ⏳ | Double click | Double click | Double click | Double click |
| Mark the row, cursor stays on it | ⏳ | Right click | Right click | Right click | Right click |
| Scroll a panel or the viewer | ⏳ | Wheel | Wheel | Wheel | Wheel |
| Press an F key | ⏳ | Click on the F-key bar | Click on the F-key bar | Click on the button bar | Click on the key bar |
| Show a tab | ⏳ | Click on the tab | Click on the tab | — | — |
| Open a menu of the menu bar / run a command | ⏳ | Click | Click | Click | Click |
| Close the menu bar | ⏳ | Click outside it | Click outside it | Click outside it | Click outside it |
| Dialog: press a button, switch a check box, choose a radio button | ⏳ | Click | Click | Click | Click |
| Dialog: put the cursor in a text field | ⏳ | Click | Click | Click | Click |
| Dialog: choose a radio button and press the default button | ⏳ | Double click | Double click | — | — |
| Menu or list in a window: move the cursor / open the row | ⏳ | Click / Double click | Click / Double click | Click / Double click | Click / Double click |
| Close a menu or the list of completions | ⏳ | Click outside it | Click outside it | Click outside it | Click outside it |
| Scroll a menu, a list, or the help | ⏳ | Wheel | Wheel | Wheel | Wheel |

## Workspaces window (noc)

Alt+w, or F9 → Workspace → Workspace list…; F9 → Workspace lists the first ten too, by their
digits.

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Save the tabs of both panels as a new workspace | ⏳ | Insert | Insert | — | — |
| Previous / next workspace | ⏳ | Up / Down | Ctrl+p / Ctrl+n<br>Up / Down | — | — |
| Page up / down | ⏳ | PgUp / PgDn | PgUp / PgDn | — | — |
| First / last workspace | ⏳ | Home / End | Home / End | — | — |
| Restore (replace the tabs of both panels) | ⏳ | Enter | Enter | — | — |
| Restore by number (empty filter) | ⏳ | 1…9, 0 | 1…9, 0 | — | — |
| Remove last filter character | ⏳ | Backspace | Backspace<br>Ctrl+h | — | — |
| Rename | ⏳ | F6 | F6 | — | — |
| Delete | ⏳ | F8<br>Delete | F8<br>Delete | — | — |
| Close | ⏳ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10 | — | — |

## Location menu (noc)

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Previous / next item | ⏳ | Up / Down | Ctrl+p / Ctrl+n<br>Up / Down | — | — |
| Page up / down | ⏳ | PgUp / PgDn | PgUp / PgDn | — | — |
| First / last item | ⏳ | Home / End | Home / End | — | — |
| Open volume or host | ⏳ | Enter | Enter | — | — |
| Open item by number (empty filter) | ⏳ | 1…9, 0 | 1…9, 0 | — | — |
| Remove last filter character | ⏳ | Backspace | Backspace<br>Ctrl+h | — | — |
| Disconnect host | ⏳ | F8 | F8 | — | — |
| Reread volumes and hosts | ⏳ | Ctrl+r | Ctrl+r | — | — |
| Close | ⏳ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10 | — | — |

## Completion list (noc)

Under a path field (Quick cd, F5, F6, F7) after a Tab that gets no further.

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Previous / next choice | ⏳ | Up / Down | Ctrl+p / Ctrl+n<br>Up / Down | — | — |
| Next choice, round | ⏳ | Tab | Tab | — | — |
| Page up / down | ⏳ | PgUp / PgDn | PgUp / PgDn | — | — |
| First / last choice | ⏳ | Home / End | Home / End | — | — |
| Put the choice in the field | ⏳ | Enter | Ctrl+y<br>Enter | — | — |
| Close | ⏳ | Esc | Ctrl+e<br>Esc | — | — |
| Close and edit the field | ⏳ | Other keys | Other keys | — | — |

## zoxide window (noc)

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Previous / next directory | ⏳ | Up / Down | Ctrl+p / Ctrl+n<br>Up / Down | — | — |
| Page up / down | ⏳ | PgUp / PgDn | PgUp / PgDn | — | — |
| First / last directory | ⏳ | Home / End | Home / End | — | — |
| Open in the active panel | ⏳ | Enter | Enter | — | — |
| Open by number (no keywords) | ⏳ | 1…9, 0 | 1…9, 0 | — | — |
| Remove last keyword character | ⏳ | Backspace | Backspace<br>Ctrl+h | — | — |
| Close | ⏳ | Esc<br>F10 | Esc<br>Ctrl+c<br>F10 | — | — |

## Renaming in place (noc)

Shift+F6 edits the name in the entry's row; the keys of input lines edit it, and others do
nothing.

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Rename to the name typed | ⏳ | Enter | Enter | — | — |
| Keep the name | ⏳ | Esc | Esc<br>Ctrl+c | — | — |

## Pull-down menu (noc)

| Action | Status | noc (default) | noc (vim) | mc | far |
| --- | --- | --- | --- | --- | --- |
| Previous / next menu | ⏳ | Left / Right | Left / Right | — | — |
| Previous / next command | ⏳ | Up / Down | Ctrl+p / Ctrl+n<br>Up / Down | — | — |
| First command | ⏳ | Home<br>PgUp | Home<br>PgUp | — | — |
| Last command | ⏳ | End<br>PgDn | End<br>PgDn | — | — |
| Open menu / run command | ⏳ | Enter | Enter | — | — |
| Open menu / run command by its letter | ⏳ | Letter | Letter | — | — |
| Close menu, then menu bar | ⏳ | Esc<br>F9<br>F10 | Esc<br>F9<br>F10 | — | — |
