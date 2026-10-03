# Keyboard shortcuts: Noon Commander vs Midnight Commander vs Far Manager

Sources:

- noc: the presets in [keymap/mod.rs](../crates/noc/src/tui/keymap/mod.rs); the default preset is
  modelled on mc. There is no vim preset yet, so its column is empty.
- [mc.1.in](https://github.com/MidnightCommander/mc/blob/master/doc/man/mc.1.in)
- [FarEng.hlf.m4](https://github.com/FarGroup/FarManager/blob/master/far/FarEng.hlf.m4)

"—" — not bound in noc, or not documented in the source. mc notation normalized to
`Ctrl+`/`Alt+`/`Shift+`; mc and noc letters are lowercase unless noted. `Ctrl+X l` is a key
sequence. Sections marked (noc) exist only in noc.

## Function keys and file operations

| Action                                                  | noc (default)                 | noc (vim) | mc                                                                            | far                                              |
| ------------------------------------------------------- | ----------------------------- | --------- | ----------------------------------------------------------------------------- | ------------------------------------------------ |
| Help                                                    | F1                            | —         | F1                                                                            | F1                                               |
| User menu                                               | —                             | —         | F2                                                                            | F2                                               |
| View file                                               | F3                            | —         | F3                                                                            | F3<br>Numpad5<br>Ctrl+Shift+F3 (always internal) |
| View without preprocessing                              | —                             | —         | F13                                                                           | —                                                |
| Alternative (external/internal) viewer                  | —                             | —         | —                                                                             | Alt+F3                                           |
| Filtered view (command output)                          | —                             | —         | Alt+!                                                                         | —                                                |
| Edit file                                               | F4                            | —         | F4                                                                            | F4<br>Ctrl+Shift+F4 (always internal)            |
| Edit host settings (virtual root, host list)            | F4                            | —         | —                                                                             | —                                                |
| Alternative (external/internal) editor                  | —                             | —         | —                                                                             | Alt+F4                                           |
| Edit new file                                           | —                             | —         | F14                                                                           | Shift+F4                                         |
| Copy                                                    | F5                            | —         | F5                                                                            | F5                                               |
| Copy current file (ignoring selection)                  | —                             | —         | F15                                                                           | Shift+F5                                         |
| Rename/move                                             | F6                            | —         | F6                                                                            | F6                                               |
| Rename/move current file                                | —                             | —         | F16                                                                           | Shift+F6                                         |
| Make directory                                          | F7                            | —         | F7                                                                            | F7                                               |
| Delete                                                  | F8<br>Delete                  | —         | F8                                                                            | F8                                               |
| Disconnect host (virtual root, host list)               | F8                            | —         | —                                                                             | —                                                |
| Delete only the file under cursor                       | —                             | —         | —                                                                             | Shift+F8                                         |
| Delete bypassing Recycle Bin                            | —                             | —         | —                                                                             | Shift+Del                                        |
| Wipe                                                    | —                             | —         | —                                                                             | Alt+Del                                          |
| Abort copy/delete                                       | Esc<br>F10                    | —         | Ctrl+C<br>Esc                                                                 | —                                                |
| Copy/move in background (in dialog)                     | —                             | —         | Alt+B                                                                         | —                                                |
| Menu bar                                                | F9                            | —         | F9                                                                            | F9                                               |
| Quit                                                    | F10                           | —         | F10                                                                           | F10                                              |
| Quit without changing to last directory (shell wrapper) | —                             | —         | Shift+F10                                                                     | —                                                |
| F1…F10 on terminals without function keys               | Esc, then 1…9, 0 (in panels)  | —         | Esc, then 1…9, 0                                                              | —                                                |
| Alt+key on terminals without Alt                        | Esc, then the key (in panels) | —         | Esc, then the key                                                             | —                                                |
| Plugin commands                                         | —                             | —         | —                                                                             | F11                                              |
| Plugin configuration                                    | —                             | —         | —                                                                             | Alt+Shift+F9                                     |
| Save setup                                              | —                             | —         | —                                                                             | Shift+F9                                         |
| Repeat last menu item                                   | —                             | —         | —                                                                             | Shift+F10                                        |
| Change drive in left panel                              | Alt+F1<br>Ctrl+X 1            | —         | —                                                                             | Alt+F1                                           |
| Change drive in right panel                             | Alt+F2<br>Ctrl+X 2            | —         | —                                                                             | Alt+F2                                           |
| Print files                                             | —                             | —         | —                                                                             | Alt+F5                                           |
| Create link                                             | —                             | —         | Ctrl+X l (hard)<br>Ctrl+X s (absolute symlink)<br>Ctrl+X v (relative symlink) | Alt+F6                                           |
| Find file                                               | —                             | —         | Alt+?                                                                         | Alt+F7                                           |
| Find folder                                             | —                             | —         | —                                                                             | Alt+F10                                          |
| File permissions/attributes                             | —                             | —         | Ctrl+X c (chmod)                                                              | Ctrl+A                                           |
| Change owner (chown)                                    | —                             | —         | Ctrl+X o                                                                      | —                                                |
| File system attributes (chattr)                         | —                             | —         | Ctrl+X e                                                                      | —                                                |
| Checksums                                               | Ctrl+X #                      | —         | —                                                                             | —                                                |
| Apply command to selected files                         | —                             | —         | —                                                                             | Ctrl+G                                           |
| Describe selected files                                 | —                             | —         | —                                                                             | Ctrl+Z                                           |
| Add files to archive                                    | —                             | —         | —                                                                             | Shift+F1                                         |
| Extract files from archive                              | —                             | —         | —                                                                             | Shift+F2                                         |
| Archive commands                                        | —                             | —         | —                                                                             | Shift+F3                                         |
| Execute / change directory / enter archive              | Enter (directories and hosts) | —         | Enter                                                                         | Enter                                            |
| Execute in separate window                              | —                             | —         | —                                                                             | Shift+Enter                                      |
| Run as administrator                                    | —                             | —         | —                                                                             | Ctrl+Alt+Enter                                   |

## General commands

| Action                                             | noc (default) | noc (vim) | mc                                    | far            |
| -------------------------------------------------- | ------------- | --------- | ------------------------------------- | -------------- |
| Repaint screen                                     | Ctrl+L        | —         | Ctrl+L                                | —              |
| Show command output / user screen                  | —             | —         | Ctrl+O                                | Ctrl+O         |
| Temporarily show user screen (while held)          | —             | —         | —                                     | Ctrl+Alt+Shift |
| Quick cd                                           | —             | —         | Alt+C                                 | —              |
| External panelize                                  | —             | —         | Ctrl+X !                              | —              |
| Add current directory to hotlist / folder shortcut | —             | —         | Ctrl+X h                              | Ctrl+Shift+0…9 |
| Go to directory from hotlist / folder shortcut     | —             | —         | Ctrl+\ (list)                         | RightCtrl+0…9  |
| Change panel charset                               | —             | —         | Alt+E                                 | —              |
| Toggle panel split (vertical/horizontal)           | —             | —         | Alt+,                                 | —              |
| Change window size                                 | —             | —         | —                                     | Alt+F9         |
| Task list                                          | —             | —         | —                                     | Ctrl+W         |
| Background jobs                                    | Ctrl+X j      | —         | —                                     | —              |
| Screen grabber                                     | —             | —         | —                                     | Alt+Ins        |
| Record keyboard macro                              | —             | —         | Ctrl+R (in editor)                    | Ctrl+.         |
| Run macro                                          | —             | —         | Ctrl+A, then assigned key (in editor) | —              |

## History

| Action                                      | noc (default) | noc (vim) | mc                               | far                |
| ------------------------------------------- | ------------- | --------- | -------------------------------- | ------------------ |
| Command history (list)                      | —             | —         | Alt+H                            | Alt+F8             |
| Previous command                            | —             | —         | Alt+P                            | Ctrl+E             |
| Next command                                | —             | —         | Alt+N                            | Ctrl+X             |
| Directory history (list)                    | —             | —         | Alt+Shift+h<br>Alt+H (uppercase) | Alt+F12            |
| Previous directory in history               | —             | —         | Alt+Y                            | —                  |
| Next directory in history                   | —             | —         | Alt+U                            | —                  |
| View and edit history                       | —             | —         | —                                | Alt+F11            |
| History menu: re-run command / open item    | —             | —         | —                                | Enter              |
| History menu: run in separate window        | —             | —         | —                                | Shift+Enter        |
| History menu: run as administrator          | —             | —         | —                                | Ctrl+Alt+Enter     |
| History menu: put into command line         | —             | —         | —                                | Ctrl+Enter         |
| Folder history menu: go to on passive panel | —             | —         | —                                | Ctrl+Shift+Enter   |
| History menu: clear history                 | —             | —         | —                                | Del                |
| History menu: delete current item           | —             | —         | —                                | Shift+Del          |
| History menu: lock/unlock item              | —             | —         | —                                | Ins                |
| History menu: refresh (remove unavailable)  | —             | —         | —                                | Ctrl+R             |
| History menu: copy item to clipboard        | —             | —         | —                                | Ctrl+C<br>Ctrl+Ins |
| View history menu: open in editor           | —             | —         | —                                | F4                 |
| View history menu: open in viewer           | —             | —         | —                                | F3<br>Numpad5      |
| Command history menu: additional info       | —             | —         | —                                | F3                 |

## Panel control

| Action                                        | noc (default)  | noc (vim) | mc                             | far                                              |
| --------------------------------------------- | -------------- | --------- | ------------------------------ | ------------------------------------------------ |
| Change active panel                           | Tab            | —         | Tab<br>Ctrl+I<br>Left<br>Right | Tab                                              |
| Swap panels                                   | Ctrl+U         | —         | —                              | Ctrl+U                                           |
| Reread panel                                  | Ctrl+R         | —         | —                              | Ctrl+R                                           |
| Stop loading or connecting                    | Esc<br>Esc Esc | —         | —                              | —                                                |
| Info panel                                    | —              | —         | Ctrl+X i (on other panel)      | Ctrl+L                                           |
| Quick view panel                              | —              | —         | Ctrl+X q (on other panel)      | Ctrl+Q                                           |
| Folder tree                                   | —              | —         | —                              | Ctrl+T                                           |
| Hide/show both panels                         | —              | —         | Ctrl+O                         | Ctrl+O                                           |
| Hide/show inactive panel                      | —              | —         | —                              | Ctrl+P                                           |
| Hide/show left panel                          | —              | —         | —                              | Ctrl+F1                                          |
| Hide/show right panel                         | —              | —         | —                              | Ctrl+F2                                          |
| Change panels height                          | —              | —         | —                              | Ctrl+Up<br>Ctrl+Down                             |
| Change current panel height                   | —              | —         | —                              | Ctrl+Shift+Up<br>Ctrl+Shift+Down                 |
| Change panels width (with empty command line) | —              | —         | —                              | Ctrl+Left<br>Ctrl+Right                          |
| Restore default panels width                  | —              | —         | —                              | Ctrl+Numpad5                                     |
| Restore default panels height                 | —              | —         | —                              | Ctrl+Alt+Numpad5                                 |
| Hide/show functional key bar                  | —              | —         | —                              | Ctrl+B                                           |
| Sizes in bytes / with K/M/G/T suffixes        | —              | —         | —                              | Ctrl+Shift+S                                     |
| Next listing format                           | —              | —         | Alt+T                          | —                                                |
| Brief view mode                               | —              | —         | —                              | LeftCtrl+1                                       |
| Medium view mode                              | —              | —         | —                              | LeftCtrl+2                                       |
| Full view mode                                | —              | —         | —                              | LeftCtrl+3                                       |
| Wide view mode                                | —              | —         | —                              | LeftCtrl+4                                       |
| Detailed view mode                            | —              | —         | —                              | LeftCtrl+5                                       |
| Descriptions view mode                        | —              | —         | —                              | LeftCtrl+6                                       |
| Long descriptions view mode                   | —              | —         | —                              | LeftCtrl+7                                       |
| File owners view mode                         | —              | —         | —                              | LeftCtrl+8                                       |
| File links view mode                          | —              | —         | —                              | LeftCtrl+9                                       |
| Alternative full view mode                    | —              | —         | —                              | LeftCtrl+0                                       |
| Hidden and system files                       | Alt+.          | —         | —                              | Ctrl+H                                           |
| Long/short names                              | —              | —         | —                              | Ctrl+N                                           |
| Scroll long names                             | —              | —         | Alt+(<br>Alt+)                 | Alt+Left<br>Alt+Right<br>Alt+Home<br>Alt+End     |
| Open directory under cursor on other panel    | Alt+O          | —         | Alt+O                          | —                                                |
| Current directory to other panel              | Alt+I          | —         | Alt+I                          | —                                                |
| Go to parent directory                        | Ctrl+PgUp      | —         | Ctrl+PgUp                      | Ctrl+PgUp                                        |
| Enter directory / archive                     | —              | —         | Ctrl+PgDn                      | Ctrl+PgDn<br>Ctrl+Shift+PgDn (always as archive) |
| Go to root directory                          | —              | —         | —                              | Ctrl+\                                           |
| Cursor up                                     | Up<br>Ctrl+P   | —         | Up<br>Ctrl+P                   | —                                                |
| Cursor down                                   | Down<br>Ctrl+N | —         | Down<br>Ctrl+N                 | —                                                |
| First entry                                   | Home           | —         | Home<br>A1<br>Alt+<            | —                                                |
| Last entry                                    | End            | —         | End<br>C1<br>Alt+>             | —                                                |
| Page down                                     | PgDn<br>Ctrl+V | —         | PgDn<br>Ctrl+V                 | —                                                |
| Page up                                       | PgUp<br>Alt+V  | —         | PgUp<br>Alt+V                  | —                                                |
| Top / middle / bottom file on screen          | —              | —         | Alt+G / Alt+R / Alt+J          | —                                                |

## Sorting

| Action                                     | noc (default) | noc (vim) | mc  | far                 |
| ------------------------------------------ | ------------- | --------- | --- | ------------------- |
| By name                                    | Ctrl+F3       | —         | —   | Ctrl+F3             |
| By extension                               | Ctrl+F4       | —         | —   | Ctrl+F4             |
| By write time                              | Ctrl+F5       | —         | —   | Ctrl+F5             |
| By size                                    | Ctrl+F6       | —         | —   | Ctrl+F6             |
| Unsorted                                   | —             | —         | —   | Ctrl+F7             |
| By creation time                           | —             | —         | —   | Ctrl+F8             |
| By access time                             | —             | —         | —   | Ctrl+F9             |
| By description                             | —             | —         | —   | Ctrl+F10            |
| By owner                                   | —             | —         | —   | Ctrl+F11            |
| Sort modes menu                            | —             | —         | —   | Ctrl+F12            |
| Use sort groups                            | —             | —         | —   | Shift+F11           |
| Show selected files first                  | —             | —         | —   | Shift+F12           |
| Sort menu: ascending / descending / invert | —             | —         | —   | + / - / *           |
| Sort menu: additional criteria             | —             | —         | —   | F4                  |
| Criteria: add / remove / replace           | —             | —         | —   | Ins / Del / F4      |
| Criteria: inherit order from sort mode     | —             | —         | —   | =                   |
| Criteria: move up / down                   | —             | —         | —   | Ctrl+Up / Ctrl+Down |
| Criteria: reset                            | —             | —         | —   | Ctrl+R              |

## File selection

| Action                                      | noc (default)                                         | noc (vim) | mc                                  | far                                           |
| ------------------------------------------- | ----------------------------------------------------- | --------- | ----------------------------------- | --------------------------------------------- |
| Select/deselect file                        | Insert<br>Ctrl+T<br>Shift+Down<br>Shift+Up (moves up) | —         | Insert<br>Ctrl+T                    | Ins<br>Shift+cursor keys<br>Right mouse click |
| Select group                                | +<br>Alt++                                            | —         | +<br>Alt++ (alternate_plus_minus)   | Gray +                                        |
| Deselect group                              | -<br>\\<br>Alt+-                                      | —         | \\<br>Alt+- (alternate_plus_minus)  | Gray -                                        |
| Invert selection                            | \*<br>Alt+\* (files only)                             | —         | \*<br>Alt+\* (alternate_plus_minus) | Gray *                                        |
| Select files with same extension            | —                                                     | —         | —                                   | Ctrl+Gray +                                   |
| Deselect files with same extension          | —                                                     | —         | —                                   | Ctrl+Gray -                                   |
| Invert selection including folders          | —                                                     | —         | —                                   | Ctrl+Gray *                                   |
| Select files with same name                 | —                                                     | —         | —                                   | Alt+Gray +                                    |
| Deselect files with same name               | —                                                     | —         | —                                   | Alt+Gray -                                    |
| Invert selection of files, deselect folders | —                                                     | —         | —                                   | Alt+Gray *                                    |
| Select all files                            | —                                                     | —         | —                                   | Shift+Gray +                                  |
| Deselect all files                          | —                                                     | —         | —                                   | Shift+Gray -                                  |
| Restore previous selection                  | —                                                     | —         | —                                   | Ctrl+M                                        |

## Clipboard in panels

| Action                        | noc (default) | noc (vim) | mc  | far                                                  |
| ----------------------------- | ------------- | --------- | --- | ---------------------------------------------------- |
| Selected names to clipboard   | —             | —         | —   | Ctrl+Ins (with empty command line)<br>Ctrl+Shift+Ins |
| Full names to clipboard       | —             | —         | —   | Alt+Shift+Ins                                        |
| Real (UNC) names to clipboard | —             | —         | —   | Ctrl+Alt+Ins                                         |
| Copy files to clipboard       | —             | —         | —   | Ctrl+Shift+C                                         |
| Cut files to clipboard        | —             | —         | —   | Ctrl+Shift+X                                         |

## Quick search in panel

| Action                       | noc (default)                                 | noc (vim) | mc               | far                              |
| ---------------------------- | --------------------------------------------- | --------- | ---------------- | -------------------------------- |
| Start quick search           | Ctrl+S<br>Alt+S<br>Typing (ui.type_to_search) | —         | Ctrl+S<br>Alt+S  | Alt+letters<br>Alt+Shift+letters |
| Next match                   | Ctrl+S<br>Alt+S                               | —         | Ctrl+S           | Ctrl+Enter                       |
| Previous match               | —                                             | —         | —                | Ctrl+Shift+Enter                 |
| Search with previous pattern | —                                             | —         | Ctrl+S Ctrl+S    | —                                |
| Correct typing               | Backspace                                     | —         | Backspace<br>Del | —                                |
| End quick search             | Esc                                           | —         | —                | —                                |
| Paste from clipboard         | —                                             | —         | —                | Ctrl+V<br>Shift+Ins              |

## Command line

| Action                                    | noc (default) | noc (vim) | mc                      | far                           |
| ----------------------------------------- | ------------- | --------- | ----------------------- | ----------------------------- |
| Insert current file name                  | —             | —         | Alt+Enter<br>Ctrl+Enter | Ctrl+J<br>Ctrl+Enter          |
| Insert file name from passive panel       | —             | —         | —                       | Ctrl+Shift+Enter              |
| Insert full name of current file          | —             | —         | Ctrl+Shift+Enter        | Ctrl+F                        |
| Insert full file name from passive panel  | —             | —         | —                       | Ctrl+;                        |
| Insert UNC file name (active / passive)   | —             | —         | —                       | Ctrl+Alt+F / Ctrl+Alt+;       |
| Insert tagged files of current panel      | —             | —         | Ctrl+X t                | —                             |
| Insert tagged files of other panel        | —             | —         | Ctrl+X Ctrl+T           | —                             |
| Insert current panel path                 | —             | —         | Ctrl+X p                | Ctrl+Shift+[                  |
| Insert other panel path                   | —             | —         | Ctrl+X Ctrl+P           | Ctrl+Shift+]                  |
| Insert left / right panel path            | —             | —         | —                       | Ctrl+[ / Ctrl+]               |
| Insert UNC path of left / right panel     | —             | —         | —                       | Ctrl+Alt+[ / Ctrl+Alt+]       |
| Insert UNC path of active / passive panel | —             | —         | —                       | Alt+Shift+[ / Alt+Shift+]     |
| Completion                                | —             | —         | Alt+Tab                 | —                             |
| Insert character literally (quote)        | —             | —         | Ctrl+Q                  | —                             |
| Clear command line                        | —             | —         | —                       | Ctrl+Y                        |
| Select block in command line              | —             | —         | —                       | Alt+Shift+Left/Right/Home/End |

## Input lines

| Action                        | noc (default)  | noc (vim) | mc                          | far                               |
| ----------------------------- | -------------- | --------- | --------------------------- | --------------------------------- |
| Character left                | Left           | —         | Ctrl+B<br>Left              | Left<br>Ctrl+S                    |
| Character right               | Right          | —         | Ctrl+F<br>Right             | Right<br>Ctrl+D                   |
| Word left                     | —              | —         | Alt+B                       | Ctrl+Left                         |
| Word right                    | —              | —         | Alt+F                       | Ctrl+Right                        |
| Beginning of line             | Home<br>Ctrl+A | —         | Ctrl+A                      | Ctrl+Home                         |
| End of line                   | End<br>Ctrl+E  | —         | Ctrl+E                      | Ctrl+End                          |
| Delete character left         | Backspace      | —         | Ctrl+H<br>Backspace         | BS                                |
| Delete character under cursor | Delete         | —         | Ctrl+D<br>Delete            | Del                               |
| Delete word left              | —              | —         | Alt+Ctrl+H<br>Alt+Backspace | Ctrl+BS                           |
| Delete word right             | —              | —         | —                           | Ctrl+Del                          |
| Delete to end of line         | Ctrl+K         | —         | Ctrl+K                      | Ctrl+K                            |
| Delete to beginning of line   | Ctrl+U         | —         | —                           | —                                 |
| Set mark                      | —              | —         | Ctrl+@                      | —                                 |
| Cut (mark to cursor)          | —              | —         | Ctrl+W                      | —                                 |
| Copy                          | —              | —         | Alt+W                       | Ctrl+Ins                          |
| Paste                         | —              | —         | Ctrl+Y                      | Shift+Ins                         |
| Input line history            | —              | —         | Alt+H                       | Ctrl+Up<br>Ctrl+Down (in dialogs) |
| Previous / next history entry | —              | —         | Alt+P / Alt+N               | —                                 |

## Menus and dialogs

| Action                                            | noc (default)                          | noc (vim) | mc                                          | far                                 |
| ------------------------------------------------- | -------------------------------------- | --------- | ------------------------------------------- | ----------------------------------- |
| Filter menu items                                 | Typing (location menu)                 | —         | —                                           | Ctrl+Alt+F<br>RAlt                  |
| Lock filter                                       | —                                      | —         | —                                           | Ctrl+Alt+L                          |
| Shift all items by 1 position                     | —                                      | —         | —                                           | Alt+Left<br>Alt+Right               |
| Shift selected item by 1 position                 | —                                      | —         | —                                           | Alt+Shift+Left<br>Alt+Shift+Right   |
| Shift all items by 20 positions                   | —                                      | —         | —                                           | Ctrl+Alt+Left<br>Ctrl+Alt+Right     |
| Shift selected item by 20 positions               | —                                      | —         | —                                           | Ctrl+Shift+Left<br>Ctrl+Shift+Right |
| Align all items left / right                      | —                                      | —         | —                                           | Alt+Home / Alt+End                  |
| Align selected item left / right                  | —                                      | —         | —                                           | Alt+Shift+Home / Alt+Shift+End      |
| Fixed menu columns                                | —                                      | —         | —                                           | Shift+F5                            |
| Close dialog                                      | Esc<br>F10                             | —         | Esc Esc<br>Esc (if Esc key mode is enabled) | —                                   |
| Next dialog item                                  | Tab<br>Down<br>Right                   | —         | —                                           | —                                   |
| Previous dialog item                              | Shift+Tab<br>Up<br>Left                | —         | —                                           | —                                   |
| Press focused button                              | Enter<br>Space                         | —         | —                                           | —                                   |
| Switch check box / choose radio button            | Space                                  | —         | —                                           | —                                   |
| List in a window (jobs, checksums): row up / down | Up / Down                              | —         | —                                           | —                                   |
| List in a window: page up / down                  | PgUp / PgDn                            | —         | —                                           | —                                   |
| List in a window: first / last row                | Home / End                             | —         | —                                           | —                                   |
| Focus first dialog item                           | —                                      | —         | —                                           | Home                                |
| Focus default dialog item                         | —                                      | —         | —                                           | PgDn<br>End                         |
| Default action                                    | Enter (from a text field or check box) | —         | —                                           | Ctrl+Enter                          |
| Move dialog                                       | —                                      | —         | —                                           | Ctrl+F5                             |
| Checkbox: on / off / undefined                    | —                                      | —         | —                                           | Gray + / Gray - / Gray *            |
| Dialog history: clear                             | —                                      | —         | —                                           | Del                                 |
| Dialog history: delete item                       | —                                      | —         | —                                           | Shift+Del                           |
| Dialog history: mark item                         | —                                      | —         | —                                           | Ins                                 |
| Insert file name under cursor into dialog         | —                                      | —         | —                                           | Shift+Enter                         |
| Insert passive panel file name into dialog        | —                                      | —         | —                                           | Ctrl+Shift+Enter                    |

## Screen switching

| Action          | noc (default) | noc (vim) | mc    | far            |
| --------------- | ------------- | --------- | ----- | -------------- |
| Next screen     | —             | —         | Alt+} | Ctrl+Tab       |
| Previous screen | —             | —         | Alt+{ | Ctrl+Shift+Tab |
| Screen list     | —             | —         | Alt+` | F12            |

## Viewer

| Action                                          | noc (default)                     | noc (vim) | mc                                                            | far                                    |
| ----------------------------------------------- | --------------------------------- | --------- | ------------------------------------------------------------- | -------------------------------------- |
| Help                                            | F1                                | —         | F1                                                            | F1                                     |
| Toggle line wrap                                | F2                                | —         | F2                                                            | F2                                     |
| Wrap type (by chars/words)                      | —                                 | —         | —                                                             | Shift+F2                               |
| Hex/code mode                                   | —                                 | —         | F4                                                            | F4                                     |
| Select mode (text/code/dump)                    | —                                 | —         | —                                                             | Shift+F4                               |
| Go to position                                  | —                                 | —         | F5                                                            | Alt+F8                                 |
| Switch to editor                                | —                                 | —         | —                                                             | F6                                     |
| Search                                          | —                                 | —         | F7<br>/<br>? (backward)                                       | F7                                     |
| Continue search forward                         | —                                 | —         | Ctrl+S                                                        | Shift+F7<br>Space                      |
| Continue search backward                        | —                                 | —         | Ctrl+R                                                        | Alt+F7                                 |
| Continue search in chosen direction             | —                                 | —         | F17<br>n                                                      | —                                      |
| Temporarily reverse search direction            | —                                 | —         | N (uppercase)                                                 | —                                      |
| Raw/Parsed                                      | —                                 | —         | F8                                                            | —                                      |
| Format/Unformat                                 | —                                 | —         | F9                                                            | —                                      |
| Code page                                       | —                                 | —         | Alt+E                                                         | F8 (OEM/ANSI)<br>Shift+F8 (menu)       |
| Quit                                            | F3<br>F10<br>q<br>Esc             | —         | F10<br>Esc                                                    | F10<br>F3<br>Numpad5<br>Esc            |
| Line down                                       | Down<br>j<br>e<br>Enter<br>Ctrl+N | —         | Down<br>Ctrl+N                                                | Down                                   |
| Line up                                         | Up<br>k<br>y<br>Ctrl+P            | —         | Up<br>Ctrl+P                                                  | Up                                     |
| Page down                                       | PgDn<br>Space<br>f<br>Ctrl+V      | —         | PgDn<br>Space<br>Ctrl+V                                       | PgDn                                   |
| Page up                                         | PgUp<br>b<br>Alt+V<br>Backspace   | —         | PgUp<br>Alt+V<br>Ctrl+B<br>b<br>Ctrl+H<br>Backspace<br>Delete | PgUp                                   |
| Half page up / down                             | —                                 | —         | u / d                                                         | —                                      |
| Beginning of file                               | Home<br>g<br>Ctrl+Home            | —         | Home<br>A1<br>g                                               | Home<br>Ctrl+Home                      |
| End of file                                     | End<br>G (uppercase)<br>Ctrl+End  | —         | End<br>C1<br>G (uppercase)                                    | End<br>Ctrl+End                        |
| Column left / right                             | Left / Right<br>h / l             | —         | —                                                             | Left / Right                           |
| 20 columns left / right                         | —                                 | —         | —                                                             | Ctrl+Left / Ctrl+Right                 |
| Leftmost / rightmost column                     | —                                 | —         | —                                                             | Ctrl+Shift+Left / Ctrl+Shift+Right     |
| Shift characters/bytes (dump, code)             | —                                 | —         | —                                                             | Ctrl+Left / Ctrl+Right                 |
| Bytes per line −1 / +1 (code)                   | —                                 | —         | —                                                             | Alt+Left / Alt+Right                   |
| Bytes per line to nearest multiple of 16 (code) | —                                 | —         | —                                                             | Ctrl+Alt+Left / Ctrl+Alt+Right         |
| Set bookmark                                    | —                                 | —         | [n] m                                                         | RightCtrl+0…9<br>Ctrl+Shift+0…9        |
| Go to bookmark                                  | —                                 | —         | [n] r                                                         | LeftCtrl+0…9                           |
| Return to previous position                     | —                                 | —         | —                                                             | Alt+BS<br>Ctrl+Z                       |
| Next file                                       | —                                 | —         | Ctrl+F                                                        | Gray +                                 |
| Previous file                                   | —                                 | —         | Ctrl+B                                                        | Gray -                                 |
| Ruler                                           | —                                 | —         | Alt+R                                                         | —                                      |
| Repaint screen                                  | Ctrl+L                            | —         | Ctrl+L                                                        | —                                      |
| User screen                                     | —                                 | —         | Ctrl+O                                                        | Ctrl+O<br>Ctrl+Alt+Shift (temporarily) |
| Go to file in panel                             | —                                 | —         | —                                                             | Ctrl+F10                               |
| Plugin commands                                 | —                                 | —         | —                                                             | F11                                    |
| View and edit history                           | —                                 | —         | —                                                             | Alt+F11                                |
| Far window size                                 | —                                 | —         | —                                                             | Alt+F9                                 |
| Viewer settings                                 | —                                 | —         | —                                                             | Alt+Shift+F9                           |
| Functional key bar                              | —                                 | —         | —                                                             | Ctrl+B                                 |
| Status line                                     | —                                 | —         | —                                                             | Ctrl+Shift+B                           |
| Scrollbar                                       | —                                 | —         | —                                                             | Ctrl+S                                 |
| Copy selection                                  | —                                 | —         | —                                                             | Ctrl+Ins<br>Ctrl+C                     |
| Clear selection                                 | —                                 | —         | —                                                             | Ctrl+U                                 |
| Select text manually                            | —                                 | —         | —                                                             | Shift+mouse click                      |

## Editor

| Action                                           | noc (default) | noc (vim) | mc                        | far                                                                 |
| ------------------------------------------------ | ------------- | --------- | ------------------------- | ------------------------------------------------------------------- |
| Character left / right                           | —             | —         | —                         | Left / Right                                                        |
| Character left without wrapping to previous line | —             | —         | —                         | Ctrl+S                                                              |
| Line up / down                                   | —             | —         | —                         | Up / Down                                                           |
| Word left / right                                | —             | —         | —                         | Ctrl+Left / Ctrl+Right                                              |
| Scroll screen up / down                          | —             | —         | —                         | Ctrl+Up / Ctrl+Down                                                 |
| Page up / down                                   | —             | —         | —                         | PgUp / PgDn                                                         |
| Beginning / end of line                          | —             | —         | —                         | Home / End                                                          |
| Beginning of file                                | —             | —         | —                         | Ctrl+Home<br>Ctrl+PgUp                                              |
| End of file                                      | —             | —         | —                         | Ctrl+End<br>Ctrl+PgDn                                               |
| Beginning / end of screen                        | —             | —         | —                         | Ctrl+N / Ctrl+E                                                     |
| Delete character                                 | —             | —         | —                         | Del                                                                 |
| Delete character left                            | —             | —         | —                         | BS                                                                  |
| Delete line                                      | —             | —         | —                         | Ctrl+Y                                                              |
| Delete to end of line                            | —             | —         | —                         | Ctrl+K<br>Alt+D                                                     |
| Delete word left                                 | —             | —         | —                         | Ctrl+BS                                                             |
| Delete word right                                | —             | —         | —                         | Ctrl+T<br>Ctrl+Del                                                  |
| Block selection                                  | —             | —         | Shift+cursor keys         | Shift+cursor keys<br>Ctrl+Shift+cursor keys                         |
| Vertical block                                   | —             | —         | —                         | Alt+gray cursor keys<br>Alt+Shift+cursor keys<br>Ctrl+Alt+gray keys |
| Select all text                                  | —             | —         | —                         | Ctrl+A                                                              |
| Clear selection                                  | —             | —         | —                         | Ctrl+U                                                              |
| Copy block to clipboard                          | —             | —         | Ctrl+Ins (to mcedit.clip) | Ctrl+Ins<br>Ctrl+C                                                  |
| Paste from clipboard                             | —             | —         | Shift+Ins                 | Shift+Ins<br>Ctrl+V                                                 |
| Cut to clipboard                                 | —             | —         | Shift+Del                 | Shift+Del<br>Ctrl+X                                                 |
| Append block to clipboard                        | —             | —         | —                         | Ctrl+Gray +                                                         |
| Delete block                                     | —             | —         | Ctrl+Del                  | Ctrl+D                                                              |
| Copy block to cursor position                    | —             | —         | —                         | Ctrl+P                                                              |
| Move block to cursor position                    | —             | —         | —                         | Ctrl+M                                                              |
| Indent block left / right                        | —             | —         | —                         | Alt+U / Alt+I                                                       |
| Format block                                     | —             | —         | F19                       | —                                                                   |
| Help                                             | —             | —         | —                         | F1                                                                  |
| Save                                             | —             | —         | —                         | F2                                                                  |
| Save as                                          | —             | —         | —                         | Shift+F2                                                            |
| New file                                         | —             | —         | —                         | Shift+F4                                                            |
| Switch to viewer                                 | —             | —         | —                         | F6                                                                  |
| Search                                           | —             | —         | —                         | F7                                                                  |
| Replace                                          | —             | —         | —                         | Ctrl+F7                                                             |
| Continue search/replace forward / backward       | —             | —         | —                         | Shift+F7 / Alt+F7                                                   |
| Code page                                        | —             | —         | Alt+E                     | F8 (OEM/ANSI)<br>Shift+F8 (select)                                  |
| Go to line and position                          | —             | —         | —                         | Alt+F8                                                              |
| Far window size                                  | —             | —         | —                         | Alt+F9                                                              |
| Editor settings                                  | —             | —         | —                         | Alt+Shift+F9                                                        |
| Quit                                             | —             | —         | —                         | F10<br>F4<br>Esc                                                    |
| Save and quit                                    | —             | —         | —                         | Shift+F10                                                           |
| Locate file in panel                             | —             | —         | —                         | Ctrl+F10                                                            |
| Plugin commands                                  | —             | —         | —                         | F11                                                                 |
| View and edit history                            | —             | —         | —                         | Alt+F11                                                             |
| Undo                                             | —             | —         | —                         | Alt+BS<br>Ctrl+Z                                                    |
| Redo                                             | —             | —         | —                         | Ctrl+Shift+Z                                                        |
| Lock editing                                     | —             | —         | —                         | Ctrl+L                                                              |
| User screen                                      | —             | —         | —                         | Ctrl+O<br>Ctrl+Alt+Shift (temporarily)                              |
| Treat next key as character code                 | —             | —         | —                         | Ctrl+Q                                                              |
| Set bookmark                                     | —             | —         | —                         | RightCtrl+0…9<br>Ctrl+Shift+0…9                                     |
| Go to bookmark                                   | —             | —         | —                         | LeftCtrl+0…9                                                        |
| Insert current panel file name                   | —             | —         | —                         | Shift+Enter                                                         |
| Insert passive panel file name                   | —             | —         | —                         | Ctrl+Shift+Enter                                                    |
| Insert full name of edited file                  | —             | —         | —                         | Ctrl+F                                                              |
| Functional key bar                               | —             | —         | —                         | Ctrl+B                                                              |
| Status line                                      | —             | —         | —                         | Ctrl+Shift+B                                                        |
| Record macro                                     | —             | —         | Ctrl+R (start/stop)       | Ctrl+.                                                              |
| Run macro                                        | —             | —         | Ctrl+A, then key          | —                                                                   |

## Directory tree (mc)

| Action                          | noc (default) | noc (vim) | mc                  | far |
| ------------------------------- | ------------- | --------- | ------------------- | --- |
| Rescan directory                | —             | —         | Ctrl+R<br>F2        | —   |
| Forget directory from tree      | —             | —         | F3                  | —   |
| Static/dynamic navigation       | —             | —         | F4                  | —   |
| Copy / move directory           | —             | —         | F5 / F6             | —   |
| Make subdirectory               | —             | —         | F7                  | —   |
| Delete directory                | —             | —         | F8                  | —   |
| Search next match               | —             | —         | Ctrl+S<br>Alt+S     | —   |
| Delete last search character    | —             | —         | Ctrl+H<br>Backspace | —   |
| Help                            | —             | —         | F1                  | —   |
| Exit without changing directory | —             | —         | Esc<br>F10          | —   |

## Diff viewer (mc)

| Action                    | noc (default) | noc (vim) | mc                  | far |
| ------------------------- | ------------- | --------- | ------------------- | --- |
| Help                      | —             | —         | F1                  | —   |
| Save changes              | —             | —         | F2                  | —   |
| Edit left file            | —             | —         | F4                  | —   |
| Edit right file           | —             | —         | F14                 | —   |
| Merge current hunk        | —             | —         | F5                  | —   |
| Search                    | —             | —         | F7                  | —   |
| Continue search           | —             | —         | F17                 | —   |
| Quit                      | —             | —         | F10<br>Esc<br>q     | —   |
| Hunk status               | —             | —         | Alt+S<br>s          | —   |
| Line numbers              | —             | —         | Alt+N<br>l          | —   |
| Maximize left panel       | —             | —         | f                   | —   |
| Equalize panel widths     | —             | —         | =                   | —   |
| Shrink right / left panel | —             | —         | > / <               | —   |
| Show CR as ^M             | —             | —         | c                   | —   |
| Tab size                  | —             | —         | 2, 3, 4, 8          | —   |
| Swap panels               | —             | —         | Ctrl+U              | —   |
| Refresh screen            | —             | —         | Ctrl+R              | —   |
| Show command screen       | —             | —         | Ctrl+O              | —   |
| Next hunk                 | —             | —         | Enter<br>Space<br>n | —   |
| Previous hunk             | —             | —         | Backspace<br>p      | —   |
| Go to line                | —             | —         | g                   | —   |
| Line down / up            | —             | —         | Down / Up           | —   |
| Page up / down            | —             | —         | PgUp / PgDn         | —   |
| Beginning of line         | —             | —         | Home<br>A1          | —   |
| End of line               | —             | —         | End                 | —   |
| Beginning of file         | —             | —         | Ctrl+Home           | —   |
| End of file               | —             | —         | Ctrl+End<br>C1      | —   |

## Help viewer

| Action                  | noc (default)       | noc (vim) | mc                | far             |
| ----------------------- | ------------------- | --------- | ----------------- | --------------- |
| Follow link             | —                   | —         | —                 | Enter           |
| Next / previous link    | —                   | —         | —                 | Tab / Shift+Tab |
| Page forward / backward | PgDn / PgUp         | —         | Space / Backspace | —               |
| Previous topic          | —                   | —         | —                 | Alt+F1<br>BS    |
| Contents                | —                   | —         | —                 | Shift+F1        |
| Plugins help            | —                   | —         | —                 | Shift+F2        |
| Search in help          | —                   | —         | —                 | F7              |
| Maximize/restore window | —                   | —         | —                 | F5              |
| Full list of help keys  | —                   | —         | F1 (again)        | —               |
| Line down / up          | Down / Up           | —         | —                 | —               |
| Beginning / end         | Home / End          | —         | —                 | —               |
| Close help              | Esc<br>F10<br>Enter | —         | —                 | —               |

## Screen grabber (Far)

| Action                       | noc (default) | noc (vim) | mc  | far                                |
| ---------------------------- | ------------- | --------- | --- | ---------------------------------- |
| Start                        | —             | —         | —   | Alt+Ins                            |
| Stream/block mode            | —             | —         | —   | Space                              |
| Selection                    | —             | —         | —   | Shift+cursor keys                  |
| Resize selected area         | —             | —         | —   | Alt+Shift+cursor keys              |
| Move selected area           | —             | —         | —   | Alt+cursor keys                    |
| Copy to clipboard            | —             | —         | —   | Enter<br>Ctrl+Ins                  |
| Append to clipboard          | —             | —         | —   | Ctrl+Gray +                        |
| Cancel                       | —             | —         | —   | Esc                                |
| Select whole screen          | —             | —         | —   | Ctrl+A                             |
| Clear selection              | —             | —         | —   | Ctrl+U                             |
| Selection horizontally by 10 | —             | —         | —   | Ctrl+Shift+Left / Ctrl+Shift+Right |
| Selection vertically by 5    | —             | —         | —   | Ctrl+Shift+Up / Ctrl+Shift+Down    |

## Task list (Far)

| Action                     | noc (default) | noc (vim) | mc  | far    |
| -------------------------- | ------------- | --------- | --- | ------ |
| Kill task                  | —             | —         | —   | Del    |
| Refresh list               | —             | —         | —   | Ctrl+R |
| Window title / module path | —             | —         | —   | F2     |

## Tabs (noc)

| Action                       | noc (default)         | noc (vim) | mc  | far |
| ---------------------------- | --------------------- | --------- | --- | --- |
| New tab on the same location | Ctrl+X t              | —         | —   | —   |
| Close tab                    | Ctrl+X w              | —         | —   | —   |
| Next tab                     | Alt+Right<br>Ctrl+X n | —         | —   | —   |
| Previous tab                 | Alt+Left<br>Ctrl+X p  | —         | —   | —   |
| Tab list                     | Ctrl+X Tab            | —         | —   | —   |

## Location menu (noc)

| Action                             | noc (default) | noc (vim) | mc  | far |
| ---------------------------------- | ------------- | --------- | --- | --- |
| Previous / next item               | Up / Down     | —         | —   | —   |
| Page up / down                     | PgUp / PgDn   | —         | —   | —   |
| First / last item                  | Home / End    | —         | —   | —   |
| Open volume or host                | Enter         | —         | —   | —   |
| Open item by number (empty filter) | 1…9, 0        | —         | —   | —   |
| Remove last filter character       | Backspace     | —         | —   | —   |
| Disconnect host                    | F8            | —         | —   | —   |
| Reread volumes and hosts           | Ctrl+R        | —         | —   | —   |
| Close                              | Esc<br>F10    | —         | —   | —   |

## Pull-down menu (noc)

| Action                                | noc (default)    | noc (vim) | mc  | far |
| ------------------------------------- | ---------------- | --------- | --- | --- |
| Previous / next menu                  | Left / Right     | —         | —   | —   |
| Previous / next command               | Up / Down        | —         | —   | —   |
| First command                         | Home<br>PgUp     | —         | —   | —   |
| Last command                          | End<br>PgDn      | —         | —   | —   |
| Open menu / run command               | Enter            | —         | —   | —   |
| Open menu / run command by its letter | Letter           | —         | —   | —   |
| Close menu, then menu bar             | Esc<br>F9<br>F10 | —         | —   | —   |
