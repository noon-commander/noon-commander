# Keyboard shortcuts: Noon Commander vs Midnight Commander vs Far Manager vs yazi vs ranger

Sources:

- noc: the presets in [keymap/default.rs](../crates/noc/src/tui/keymap/default.rs) and
  [keymap/vim.rs](../crates/noc/src/tui/keymap/vim.rs); the default preset is modelled on mc,
  `ui.keymap` chooses the preset, and `noc keymap diff` compares them.
- mc: [mc.default.keymap](https://github.com/MidnightCommander/mc/blob/master/misc/mc.default.keymap),
  the keys mc binds, and [mc.1.in](https://github.com/MidnightCommander/mc/blob/master/doc/man/mc.1.in),
  which explains them; where the two disagree, or neither says what a key does, the
  [source](https://github.com/MidnightCommander/mc/tree/master/src).
- Far: [FarEng.hlf.m4](https://github.com/FarGroup/FarManager/blob/master/far/FarEng.hlf.m4);
  the mouse, which it barely documents, also
  [filelist.cpp](https://github.com/FarGroup/FarManager/blob/master/far/filelist.cpp),
  [keybar.cpp](https://github.com/FarGroup/FarManager/blob/master/far/keybar.cpp),
  [vmenu.cpp](https://github.com/FarGroup/FarManager/blob/master/far/vmenu.cpp), and
  [dialog.cpp](https://github.com/FarGroup/FarManager/blob/master/far/dialog.cpp).
- yazi: [keymap-default.toml](https://github.com/sxyazi/yazi/blob/main/yazi-config/preset/keymap-default.toml);
  the mouse, the components in
  [yazi-plugin/preset/components](https://github.com/sxyazi/yazi/tree/main/yazi-plugin/preset/components).
- ranger: [rc.conf](https://github.com/ranger/ranger/blob/master/ranger/config/rc.conf); the
  mouse, MOUSE BUTTONS in [ranger.pod](https://github.com/ranger/ranger/blob/master/doc/ranger.pod)
  and [browsercolumn.py](https://github.com/ranger/ranger/blob/master/ranger/gui/widgets/browsercolumn.py).

The sections follow noc's keymap: one per context, in the order of the presets and under the
name `noc keymap diff` gives it, then the mouse, then what only mc, Far, or yazi has. A context
lists the actions it binds itself; keys it does not bind go to the context it falls back to, as
its section says. In each table:

- Description: what the keys do; in noc, where noc does it.
- Action: noc's action, as the presets and `noc keymap diff` name it; "—" where no action of
  noc's keymap does it. noc's actions come first, in the order of the default preset, then what
  mc, Far, yazi, or ranger does there and noc does not.
- noc (default), noc (vim), mc, far, yazi, ranger: the keys. "—" — not bound in noc, or not
  documented in the source.

yazi and ranger show one directory between its parent and a preview, with tabs and no second
panel, so the keys of the other panel are "—" for them. Both copy and move in two steps: they
yank the entries in one directory and paste them in another. yazi's input boxes and ranger's
console take the keys of the text fields and the command line.

Keys are written the same way in every column, whatever notation the source uses:

- Letters are lowercase, after a modifier too (`Ctrl+l`); an uppercase letter is Shift and the
  letter (`G`, `Alt+H`, `Ctrl+S`).
- Shift is written out only before keys that are not letters (`Shift+F6`, `Shift+Enter`,
  `Ctrl+Shift+[`); symbols are written as typed (`+`, `*`, `?`).
- Modifiers come in the order `Ctrl+Alt+Shift+`; the keys of a sequence are apart (`Ctrl+x l`,
  `Esc Esc`, `Z Z`).
- Named keys: Enter, Esc, Tab, Space, Backspace, Delete, Insert, Home, End, PgUp, PgDn, Up,
  Down, Left, Right, F1…F24, Numpad+, Numpad-, Numpad\*, Numpad5; A1 and C1 are mc's names for
  the keys at the upper and the lower left of the numpad. The mouse: Click, Double click, Right
  click, Middle click, Wheel, Drag.
- mc reads Shift+F1…Shift+F10 as F11…F20, so its column writes them as Shift+F1…Shift+F10.
  It waits a second after Esc for a key that may follow; Esc Esc does not wait. noc binds no
  F11…F20 in their place: crossterm 0.29, which reads the keys, takes the codes of F15…F20 for
  other F-keys, so that the Linux console's Shift+F9 would arrive as F16.
- noc binds Ctrl+[ wherever it binds Esc, Ctrl+m wherever it binds Enter, and Ctrl+i wherever
  it binds Tab: terminals send them as Esc, Enter, and Tab, but tell them apart where they speak
  the kitty keyboard protocol.

Status, of noc as a whole:

- ✅ settled: the presets bind the action, with the keys they will keep; "—" in a preset's column
  leaves it to the context the section falls back to, or unbound there.
- ⏳ not settled yet: noc does not bind it yet, or its keys may still change. Its noc columns
  may show planned keys, which no preset binds yet.
- 🚫 not needed: noc will not have it.

## Everywhere (`global`)

Keys that do the same wherever they are pressed: in the panels, the dialogs, the menus, the
windows, the viewer, and on the command line. Every other context falls back to this one last,
after its own fallbacks, so a context that binds the same key itself takes it first. The output of
commands is the exception: noc draws nothing there. None of mc, Far, yazi, and ranger binds
Alt+/; mc's Alt+?, which finds files, stays free for noc's Find file. yazi and ranger show the
keys that can follow on their own, once the first key of a sequence is pressed; yazi binds its
help in every window, and F1 in mc and Far opens the help of the window in front. Ctrl+o shows
the output of commands over whatever is in front, which is there again afterwards; mc and Far
hide or show the panels with it in the panels and on their command line, and mc in its viewer
and editor too. Ctrl+\` does the same in both presets, where the terminal speaks the kitty
keyboard protocol; without it, terminals send it as Ctrl+Space, which noc leaves to macOS and to
input methods, which switch the input source with it. The vim preset takes g <, vim's key for
the output of the last command. Ctrl+l redraws the screen under whatever is in front, which stays
as it was, quick search too, as mc's Ctrl+l does in its panels, dialogs, viewer, and editor.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| The keys that can be typed now, over whatever is in front, and what they do, as which-key shows them | `key_hints` | ✅ | Alt+/ | Alt+/ | — | — | — | — |
| The output of commands, in place of the panels and what is in front of them (mc, Far: hide or show the panels) | `user_screen` | ✅ | Ctrl+o<br>Ctrl+\` | g <<br>Ctrl+\` | Ctrl+o | Ctrl+o | — | — |
| Redraw the screen | `redraw` | ✅ | Ctrl+l | Ctrl+l | Ctrl+l | — | — | Ctrl+l |
| Help: the keys of what is in front, and what they do | — | ⏳ | — | — | F1 | F1 | \~<br>F1 | — |

## Panels (`panel`)

A panel that lists a directory. Quick search falls back to it. mc and Far have a command line
under the panels that takes what is typed; noc opens one with `!`, yazi asks for a shell command
in an input box, and ranger opens its console with `:`. mc's +, -, \\, and \*, on the numpad
too, mark and unmark only while its command line is empty; with Alt, they always do.

Both presets group keys of their own under a first key: t for the tabs and the workspaces, o for
the sort order, as ranger's sort keys, and the hidden names, and f for the file operations. f
stands for mc's Ctrl+x, whose letters it keeps for mc's commands: c chmod, o chown, e chattr, l,
s, and v links, j the jobs. mc has no checksums, so they take #. The vim preset adds vim's g, q,
and Ctrl+w; it opens the location menus with Ctrl+w 1 and Ctrl+w 2, as the default does with
Ctrl+x 1 and Ctrl+x 2. Both presets bind vim's Ctrl+w h and Ctrl+w l, and Ctrl+x Left and
Ctrl+x Right, for the left and the right panel. The default takes vim's Ctrl+w f and Ctrl+w v
too, as keys that need no Alt, which the Option key of macOS terminals often does not send. In
both, n and N repeat the last quick search, forward and backward, as in vim, yazi, and ranger;
Shift+F7, mc's and Far's key to search again, repeats it forward too.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| One row up | `up` | ✅ | Up<br>Ctrl+p | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p | — | k<br>Up | Up<br>k |
| One row down | `down` | ✅ | Down<br>Ctrl+n | j<br>Down<br>Ctrl+n | Down<br>Ctrl+n | — | j<br>Down | Down<br>j |
| One page up | `page_up` | ✅ | PgUp<br>Alt+v | Ctrl+b<br>PgUp | PgUp<br>Alt+v | — | Ctrl+b<br>PgUp | PgUp<br>Ctrl+b |
| One page down | `page_down` | ✅ | PgDn<br>Ctrl+v | Ctrl+f<br>PgDn | PgDn<br>Ctrl+v | — | Ctrl+f<br>PgDn | PgDn<br>Ctrl+f |
| Half a page up | `half_page_up` | ✅ | Shift+PgUp | Ctrl+u | — | — | Ctrl+u<br>Shift+PgUp | K<br>Ctrl+u |
| Half a page down | `half_page_down` | ✅ | Shift+PgDn | Ctrl+d | — | — | Ctrl+d<br>Shift+PgDn | J<br>Ctrl+d |
| First row | `home` | ✅ | Home<br>Alt+< | g g<br>Home | Home<br>A1<br>Alt+< | — | g g<br>Home | Home<br>g g |
| Last row | `end` | ✅ | End<br>Alt+> | G<br>End | End<br>C1<br>Alt+> | — | G<br>End | End<br>G |
| Open the directory or file under the cursor | `enter` | ✅ | Enter<br>Ctrl+m<br>Ctrl+PgDn | l<br>Right<br>Enter<br>Ctrl+m | Enter<br>Ctrl+PgDn | Enter<br>Ctrl+PgDn | l<br>Right<br>o<br>Enter | Right<br>Enter<br>l |
| Parent directory; nothing in / | `parent` | ✅ | Ctrl+PgUp<br>Backspace<br>Ctrl+h | h<br>Left<br>Backspace<br>Ctrl+h | Ctrl+PgUp | Ctrl+PgUp | h<br>Left | Left<br>h |
| Read the directory again | `reload` | ✅ | Ctrl+r | R | Ctrl+r | Ctrl+r | — | R |
| Stop loading or connecting | `cancel` | ✅ | Esc<br>Ctrl+[<br>Ctrl+c | Esc<br>Ctrl+[<br>Ctrl+c | — | — | — | Ctrl+c |
| The pull-down menu | `pull_down` | ✅ | F9 | g m<br>F9 | F9 | F9 | — | — |
| The pull-down menu, on the command that ran from it last | `pull_down_last` | ✅ | Shift+F9 | g M<br>Shift+F9 | Shift+F9 | Shift+F10 | — | — |
| The other panel | `switch_panel` | ✅ | Tab<br>Ctrl+i | Tab<br>Ctrl+i<br>Ctrl+w w<br>Ctrl+w Ctrl+w<br>Ctrl+w p | Tab<br>Ctrl+i | Tab | — | — |
| The left panel, wherever the panels were swapped | `focus_left` | ✅ | Ctrl+x Left<br>Ctrl+w h | Ctrl+w h<br>Ctrl+x Left | — | — | — | — |
| The right panel, wherever the panels were swapped | `focus_right` | ✅ | Ctrl+x Right<br>Ctrl+w l | Ctrl+w l<br>Ctrl+x Right | — | — | — | — |
| Swap the panels | `swap_panels` | ✅ | Ctrl+u | Ctrl+w x<br>Ctrl+w r | Ctrl+u | Ctrl+u | — | — |
| Open the directory under the cursor in the other panel | `other_panel_open` | ✅ | Alt+o<br>Ctrl+w f | Ctrl+w f | Alt+o | — | — | — |
| Show this directory in the other panel | `other_panel_sync` | ✅ | Alt+i<br>Ctrl+w v | Ctrl+w v | Alt+i | — | — | — |
| Change the location of the panel drawn on the left, after Ctrl+u too | `location_menu_left` | ✅ | Alt+F1<br>Ctrl+x 1 | Ctrl+w 1 | — | Alt+F1 | — | — |
| Change the location of the panel drawn on the right | `location_menu_right` | ✅ | Alt+F2<br>Ctrl+x 2 | Ctrl+w 2 | — | Alt+F2 | — | — |
| A new tab in this panel, on the same directory | `new_tab` | ✅ | t n | t n<br>g n | — | — | t t | Ctrl+n<br>g n |
| Close this tab; the last one stays | `close_tab` | ✅ | t c | t c<br>g c | — | — | — | Ctrl+w<br>g c |
| The next tab in this panel | `next_tab` | ✅ | ] t<br>Alt+Right | ] t<br>g t<br>Alt+Right | — | — | ] | Tab<br>Alt+Right<br>g t |
| The previous tab in this panel | `prev_tab` | ✅ | [ t<br>Alt+Left | [ t<br>g T<br>Alt+Left | — | — | [ | Shift+Tab<br>Alt+Left<br>g T |
| The tabs of the left panel, to choose one | `tab_list_left` | ✅ | t h | t h | — | — | — | — |
| The tabs of the right panel, to choose one | `tab_list_right` | ✅ | t l | t l | — | — | — | — |
| The saved workspaces: restore, rename, or delete one | `workspaces` | ✅ | t w | t w | — | — | — | — |
| Save the tabs of both panels as a workspace | `save_workspace` | ✅ | t W | t W | — | — | — | — |
| Show or hide names that start with a dot | `toggle_hidden` | ✅ | Alt+.<br>o h | o h | Alt+. | Ctrl+h | . | z h<br>Ctrl+h<br>Backspace |
| Sort by name; again: reverse | `sort_by_name` | ✅ | o n | o n | — | Ctrl+F3 | , a<br>, A (reverse) | o b<br>o B (reverse) |
| Sort by extension; again: reverse | `sort_by_extension` | ✅ | o e | o e | — | Ctrl+F4 | , e<br>, E (reverse) | o e<br>o E (reverse) |
| Sort by modification time; again: reverse | `sort_by_time` | ✅ | o m | o m | — | Ctrl+F5 | , m<br>, M (reverse) | o m<br>o M (reverse) |
| Sort by size; again: reverse | `sort_by_size` | ✅ | o s | o s | — | Ctrl+F6 | , s<br>, S (reverse) | o s<br>o S (reverse) |
| Mark or unmark, then the next row | `mark` | ✅ | Insert<br>Ctrl+t<br>Shift+Down | Space<br>J<br>Shift+Down<br>Insert | Insert<br>Ctrl+t<br>Shift+Down | Insert<br>Shift+Down | Space | Space |
| Mark or unmark, then the row above | `mark_up` | ✅ | Shift+Up | K<br>Shift+Up | Shift+Up | Shift+Up | — | — |
| Mark the names that match a pattern | `select` | ✅ | +<br>Alt++ | + | +<br>Numpad+<br>Alt++ | Numpad+ | — | — |
| Unmark the names that match a pattern | `unselect` | ✅ | -<br>\\<br>Alt+- | - | -<br>\\<br>Numpad-<br>Alt+- | Numpad- | — | — |
| Invert the marks on files; directories stay as they are | `invert_marks` | ✅ | \*<br>Alt+\* | \* | \*<br>Numpad\*<br>Alt+\* | Numpad* | — | — |
| Start quick search | `quick_search` | ✅ | Ctrl+s<br>Alt+s | / | Ctrl+s<br>Alt+s | Alt+characters<br>Alt+Shift+characters | / | f |
| The next name that matches the last quick search | `next_match` | ✅ | Shift+F7<br>n | n<br>Shift+F7 | — | — | n | n |
| The previous name that matches the last quick search | `prev_match` | ✅ | N | N | — | — | N | N |
| Open the command line for a shell command | `shell` | ✅ | ! | ! | Typing | Typing | ; (in the background)<br>: | !<br>s |
| Open the command line for a command of noc | `command` | ✅ | : | : | — | — | — | :<br>; |
| The command history | `command_history` | ✅ | Alt+h | q : | Alt+h | Alt+F8 | — | — |
| Jump to a directory that zoxide ranks | `jump` | ✅ | Alt+z<br>Ctrl+x z | g z | — | — | Z | — |
| Quick cd: type a path as for cd | `quick_cd` | ✅ | Alt+c | g d | Alt+c | — | g Space | c d |
| View the file under the cursor | `view` | ✅ | F3 | i<br>F3 | F3 | F3<br>Numpad5<br>Ctrl+Shift+F3 (always internal) | — | i<br>F3 |
| Edit the file under the cursor in $VISUAL or $EDITOR | `edit` | ✅ | F4 | e<br>F4 | F4 | F4<br>Ctrl+Shift+F4 (always internal) | — | F4<br>E |
| Copy the marked entries, or the one under the cursor | `copy` | ✅ | F5 | Y<br>y y<br>F5 | F5 | F5 | y, then p | y y, then p p<br>F5, then p p |
| Move or rename the marked entries, or the one under the cursor | `move` | ✅ | F6 | x<br>F6 | F6 | F6 | x, then p | d d, then p p<br>F6, then p p |
| Rename the entry under the cursor in its row | `rename` | ✅ | Shift+F6 | r<br>Shift+F6 | Shift+F6 | Shift+F6 | r | F2 (before the extension)<br>c w (a new name)<br>a (before the extension)<br>A (at the end)<br>I (at the start) |
| Make a directory | `mkdir` | ✅ | F7 | A<br>F7 | F7 | F7 | a (a name that ends in /) | F7 |
| Delete the marked entries, or the one under the cursor | `delete` | ✅ | F8 | d D<br>F8 | F8 | F8 | D | F8<br>d D |
| Checksums of the marked files, or the one under the cursor | `checksum` | ✅ | f # | f # | — | — | — | — |
| The running jobs: bring one to the front, or abort it | `jobs` | ✅ | Ctrl+x j<br>f j | f j | Ctrl+x j | — | w | w |
| Help | `help` | ✅ | F1 | g ?<br>F1 | F1 | F1 | \~<br>F1 | ?<br>F1 |
| Quit | `quit` | ✅ | F10 | Z Z<br>F10 | F10 | F10 | q | Q<br>F10 |
| F1…F10 on terminals without function keys | — | ✅ | Alt+1…9, 0 | Alt+1…9, 0 | Esc, then 1…9, 0 | — | — | — |
| Alt+key on terminals without Alt | — | 🚫 | — | — | Esc, then the key | — | — | — |
| Enter the archive under the cursor as a directory | — | ⏳ | — | — | Ctrl+PgDn | Ctrl+PgDn<br>Ctrl+Shift+PgDn (always as archive) | — | — |
| Execute in separate window | — | ⏳ | — | — | — | Shift+Enter | — | — |
| Run as administrator | — | ⏳ | — | — | — | Ctrl+Alt+Enter | — | — |
| Open the marked files, or the one under the cursor, with a program chosen from a list | — | ⏳ | — | — | — | — | O<br>Shift+Enter | r |
| User menu | — | ⏳ | — | — | F2 | F2 | — | — |
| View without preprocessing | — | ⏳ | — | — | Shift+F3 | — | — | — |
| Alternative (external/internal) viewer | — | ⏳ | — | — | — | Alt+F3 | — | — |
| Filtered view (command output) | — | ⏳ | — | — | Alt+! | — | — | # |
| Alternative (external/internal) editor | — | ⏳ | — | — | — | Alt+F4 | — | — |
| Edit new file | — | ⏳ | — | — | Shift+F4 | Shift+F4 | — | — |
| Make an empty file | — | ⏳ | — | — | — | — | a | Insert |
| Make several files at once | — | ⏳ | — | — | — | — | A | — |
| Copy current file (ignoring selection) | — | ⏳ | — | — | Shift+F5 | Shift+F5 | — | — |
| Paste, replacing the files of the same names | — | ⏳ | — | — | — | — | P | p o |
| Paste after the jobs already queued, not before them | — | ⏳ | — | — | — | — | — | p P<br>p O (replacing) |
| Paste to a directory typed on the console | — | ⏳ | — | — | — | — | — | p d |
| Paste to a bookmarked directory | — | ⏳ | — | — | — | — | — | p \`, then the key<br>p ', then the key |
| Forget the entries yanked for copying or moving | — | ⏳ | — | — | — | — | Y<br>X | u y<br>u d |
| Add the marked entries to those yanked for copying / take them out / switch them | — | ⏳ | — | — | — | — | — | y a / y r / y t |
| Add the marked entries to those yanked for moving / take them out / switch them | — | ⏳ | — | — | — | — | — | d a / d r / d t |
| Yank for copying the rows from the cursor to the first or the last, or to the row below or above | — | ⏳ | — | — | — | — | — | y g g<br>y G<br>y j<br>y k |
| Yank for moving the rows from the cursor to the first or the last, or to the row below or above | — | ⏳ | — | — | — | — | — | d g g<br>d G<br>d j<br>d k |
| Delete only the file under cursor | — | ⏳ | — | — | Shift+F8 | Shift+F8 | — | — |
| Delete bypassing Recycle Bin | — | ⏳ | — | — | — | Shift+Delete | — | — |
| Wipe | — | ⏳ | — | — | — | Alt+Delete | — | — |
| Move the marked entries, or the one under the cursor, to the trash | — | ⏳ | — | d d | — | F8 (with Delete to Recycle Bin on) | d | d T |
| Quit without changing to last directory (shell wrapper) | — | ⏳ | — | Z Q | Shift+F10 | — | Q | — |
| Close this tab; the last one: quit | — | ⏳ | — | — | — | — | Ctrl+c | q<br>Z Z<br>Z Q |
| Plugin commands | — | ⏳ | — | — | — | F11 | — | — |
| Plugin configuration | — | ⏳ | — | — | — | Alt+Shift+F9 | — | — |
| Save setup (noc: the Configuration dialog saves each change) | — | 🚫 | — | — | — | Shift+F9 | — | — |
| Print files | — | ⏳ | — | — | — | Alt+F5 | — | — |
| Create link | — | ⏳ | — | — | Ctrl+x l (hard)<br>Ctrl+x s (absolute symlink)<br>Ctrl+x v (relative symlink) | Alt+F6 | - (absolute symlink)<br>_ (relative symlink)<br>Ctrl+- (hard) | p l (absolute symlink)<br>p L (relative symlink)<br>p h l (hard)<br>p h t (hard, in a copy of the tree) |
| Find file | — | ⏳ | — | — | Alt+? | Alt+F7 | s | — |
| Find files by their content | — | ⏳ | — | — | — | — | S | — |
| Stop finding files | — | ⏳ | — | — | — | — | Ctrl+s | — |
| Jump to a file or directory found with fzf | — | ⏳ | — | — | — | — | z | — |
| Find folder | — | ⏳ | — | — | — | Alt+F10 | — | — |
| File permissions/attributes | — | ⏳ | — | — | Ctrl+x c (chmod) | Ctrl+a<br>F4 (on a directory) | — | = |
| Add or remove a permission (chmod) | — | ⏳ | — | — | — | — | — | + or -, then u, g, o, a, or none, then r, w, x, X, s, or t |
| Change owner (chown) | — | ⏳ | — | — | Ctrl+x o | — | — | — |
| File system attributes (chattr) | — | ⏳ | — | — | Ctrl+x e | — | — | — |
| Apply command to selected files | — | ⏳ | — | — | — | Ctrl+g | — | @ |
| Describe selected files | — | ⏳ | — | — | — | Ctrl+z | — | — |
| Add files to archive | — | ⏳ | — | — | — | Shift+F1 | — | — |
| Extract files from archive | — | ⏳ | — | — | — | Shift+F2 | — | — |
| Archive commands | — | ⏳ | — | — | — | Shift+F3 | — | — |
| Temporarily show user screen (while held) | — | ⏳ | — | — | — | Ctrl+Alt+Shift | — | — |
| A shell in the terminal; leaving it comes back | — | ⏳ | — | — | — | — | — | S |
| Suspend, back to the shell that started it | — | ⏳ | — | — | Ctrl+z | — | Ctrl+z | — |
| External panelize | — | ⏳ | — | — | Ctrl+x ! | — | — | — |
| Add current directory to hotlist / folder shortcut | — | ⏳ | — | — | Ctrl+x h | Ctrl+Shift+0…9 | — | m, then a key |
| Go to directory from hotlist / folder shortcut | — | ⏳ | — | — | Ctrl+\ (list) | RightCtrl+0…9 | — | \`, then the key<br>', then the key |
| Remove a bookmark | — | ⏳ | — | — | — | — | — | u m, then the key |
| Change panel charset | — | ⏳ | — | — | Alt+e | — | — | — |
| Toggle panel split (vertical/horizontal) | — | ⏳ | — | — | Alt+, | — | — | — |
| Miller columns, or the tabs side by side | — | ⏳ | — | — | — | — | — | \~ |
| Change window size | — | ⏳ | — | — | — | Alt+F9 | — | — |
| Task list | — | ⏳ | — | — | — | Ctrl+w | — | — |
| Screen grabber | — | ⏳ | — | — | — | Alt+Insert | — | — |
| Record keyboard macro | — | ⏳ | — | — | — | Ctrl+.<br>Ctrl+Shift+. (special mode) | — | — |
| Next screen | — | ⏳ | — | — | Alt+} | Ctrl+Tab | — | — |
| Previous screen | — | ⏳ | — | — | Alt+{ | Ctrl+Shift+Tab | — | — |
| Screen list | — | ⏳ | — | — | Alt+\` | F12 | — | — |
| Bring back the tab closed last | — | ⏳ | — | — | — | — | — | u q |
| Rename this tab | — | ⏳ | — | — | — | — | t r | — |
| Show tab 1…9 | — | ⏳ | — | — | — | — | 1…9 | Alt+1…9 (a new one if there is none) |
| Move this tab left / right | — | ⏳ | — | — | — | — | { / } | Alt+l / Alt+r |
| Directory history (list) | — | ⏳ | — | — | Alt+H | Alt+F12 | — | — |
| Previous directory in history | — | ⏳ | — | [ h | Alt+y | — | H | H |
| Next directory in history | — | ⏳ | — | ] h | Alt+u | — | L | L |
| View and edit history | — | ⏳ | — | — | Alt+E | Alt+F11 | — | — |
| Info panel | — | ⏳ | — | — | Ctrl+x i (on other panel) | Ctrl+l | Tab | — |
| Quick view panel | — | ⏳ | — | — | Ctrl+x q (on other panel) | Ctrl+q | — | z p |
| Scroll the preview up / down | — | ⏳ | — | — | — | — | K / J | Alt+k / Alt+j |
| Preview directories or not | — | ⏳ | — | — | — | — | — | z P |
| Shrink the preview when there is nothing to preview, or not | — | ⏳ | — | — | — | — | — | z c |
| Preview images or not | — | ⏳ | — | — | — | — | — | z i |
| Preview with the preview script or not | — | ⏳ | — | — | — | — | — | z v |
| Folder tree | — | ⏳ | — | — | — | Ctrl+t | — | — |
| Hide/show inactive panel | — | ⏳ | — | — | — | Ctrl+p | — | — |
| Hide/show left panel | — | ⏳ | — | — | — | Ctrl+F1 | — | — |
| Hide/show right panel | — | ⏳ | — | — | — | Ctrl+F2 | — | — |
| Change panels height | — | ⏳ | — | — | — | Ctrl+Up<br>Ctrl+Down | — | — |
| Change current panel height | — | ⏳ | — | — | — | Ctrl+Shift+Up<br>Ctrl+Shift+Down | — | — |
| Change panels width (with empty command line) | — | ⏳ | — | — | Alt+Shift+Left<br>Alt+Shift+Right | Ctrl+Left<br>Ctrl+Right | — | — |
| Restore default panels width | — | ⏳ | — | — | Alt+= | Ctrl+Numpad5 | — | — |
| Restore default panels height | — | ⏳ | — | — | — | Ctrl+Alt+Numpad5 | — | — |
| Hide/show functional key bar | — | ⏳ | — | — | — | Ctrl+b | — | — |
| The mouse on or off | — | ⏳ | — | — | — | — | — | z m |
| Flush the input after each key or not | — | ⏳ | — | — | — | — | — | z I |
| Sizes in bytes / with K/M/G/T suffixes | — | ⏳ | — | — | — | Ctrl+S | — | — |
| Sizes of the marked directories, or the one under the cursor | — | ⏳ | — | — | Ctrl+Space | F3 (on a directory) | — | d c |
| Keep the sizes of directories up to date or not | — | ⏳ | — | — | — | — | — | z u |
| Disk usage of the directories here, in the pager | — | ⏳ | — | — | — | — | — | d u<br>d U (largest first) |
| Next listing format | — | ⏳ | — | — | Alt+t | — | — | — |
| Line mode: what the rows show beside the names | — | ⏳ | — | — | — | — | m s (size)<br>m p (permissions)<br>m b (btime)<br>m m (mtime)<br>m o (owner)<br>m n (none) | M f (filename)<br>M i (fileinfo)<br>M m (mtime)<br>M h (humanreadablemtime)<br>M p (permissions)<br>M s (sizemtime)<br>M H (sizehumanreadablemtime)<br>M t (metatitle) |
| Brief view mode | — | ⏳ | — | — | — | LeftCtrl+1 | — | — |
| Medium view mode | — | ⏳ | — | — | — | LeftCtrl+2 | — | — |
| Full view mode | — | ⏳ | — | — | — | LeftCtrl+3 | — | — |
| Wide view mode | — | ⏳ | — | — | — | LeftCtrl+4 | — | — |
| Detailed view mode | — | ⏳ | — | — | — | LeftCtrl+5 | — | — |
| Descriptions view mode | — | ⏳ | — | — | — | LeftCtrl+6 | — | — |
| Long descriptions view mode | — | ⏳ | — | — | — | LeftCtrl+7 | — | — |
| File owners view mode | — | ⏳ | — | — | — | LeftCtrl+8 | — | — |
| File links view mode | — | ⏳ | — | — | — | LeftCtrl+9 | — | — |
| Alternative full view mode | — | ⏳ | — | — | — | LeftCtrl+0 | — | — |
| Long/short names | — | ⏳ | — | — | — | Ctrl+n | — | — |
| Scroll long names | — | ⏳ | — | — | Alt+(<br>Alt+) | Alt+Left<br>Alt+Right<br>Alt+Home<br>Alt+End | — | — |
| Go to root directory | — | ⏳ | — | — | — | Ctrl+\ | — | g r<br>g / |
| Go to the home directory | — | ⏳ | — | — | — | — | g h | g h |
| Go to a directory that the keymap names | — | ⏳ | — | — | — | — | g c (\~/.config)<br>g d (\~/Downloads)<br>g t (the trash) | g e (/etc)<br>g u (/usr)<br>g d (/dev)<br>g o (/opt)<br>g v (/var)<br>g m (/media)<br>g i (/run/media/$USER)<br>g M (/mnt)<br>g s (/srv)<br>g p (/tmp)<br>g R (ranger's own)<br>g ? (/usr/share/doc/ranger) |
| Follow the symlink under the cursor | — | ⏳ | — | — | — | — | g f | g L |
| This directory, with its symlinks resolved | — | ⏳ | — | — | — | — | — | g l |
| Top / middle / bottom file on screen | — | ⏳ | — | — | Alt+g / Alt+r / Alt+j | — | — | — |
| The next / previous directory beside this one | — | ⏳ | — | — | — | — | — | ] / [ |
| The next / previous entry of the tree, depth first, entering directories | — | ⏳ | — | — | — | — | — | } / { |
| The next file after a directory, or the next directory after a file | — | ⏳ | — | — | — | — | — | ) |
| Unsorted | — | ⏳ | — | — | — | Ctrl+F7 | — | — |
| Sort by creation time | — | ⏳ | — | — | — | Ctrl+F8 | , b<br>, B (reverse) | — |
| Sort by access time | — | ⏳ | — | — | — | Ctrl+F9 | — | o a<br>o A (reverse) |
| Sort by description | — | ⏳ | — | — | — | Ctrl+F10 | — | — |
| Sort by owner | — | ⏳ | — | — | — | Ctrl+F11 | — | — |
| Sort by name, numbers by their value | — | ⏳ | — | — | — | — | , n<br>, N (reverse) | o n<br>o N (reverse) |
| Sort by change time (ctime) | — | ⏳ | — | — | — | — | — | o c<br>o C (reverse) |
| Sort by type | — | ⏳ | — | — | — | — | — | o t<br>o T (reverse) |
| Sort randomly | — | ⏳ | — | — | — | — | , r | o z |
| Sort modes menu | — | ⏳ | — | — | — | Ctrl+F12 | — | — |
| Use sort groups | — | ⏳ | — | — | — | Shift+F11 | — | — |
| Show selected files first | — | ⏳ | — | — | — | Shift+F12 | — | — |
| Reverse the sort order | — | ⏳ | — | — | — | — | — | o r |
| Directories first or not | — | ⏳ | — | — | — | — | — | z d |
| Sort ignoring case or not | — | ⏳ | — | — | — | — | — | z s |
| Select files with same extension | — | ⏳ | — | — | — | Ctrl+Numpad+ | — | — |
| Deselect files with same extension | — | ⏳ | — | — | — | Ctrl+Numpad- | — | — |
| Invert selection including folders | — | ⏳ | — | — | — | Ctrl+Numpad* | Ctrl+r | v |
| Select files with same name | — | ⏳ | — | — | — | Alt+Numpad+ | — | — |
| Deselect files with same name | — | ⏳ | — | — | — | Alt+Numpad- | — | — |
| Invert selection of files, deselect folders | — | ⏳ | — | — | — | Alt+Numpad* | — | — |
| Select all files | — | ⏳ | — | — | — | Shift+Numpad+ | Ctrl+a | — |
| Deselect all files | — | ⏳ | — | — | — | Shift+Numpad- | Esc<br>Ctrl+[ | u v |
| Restore previous selection | — | ⏳ | — | — | — | Ctrl+m | — | — |
| Leave visual mode (yazi: or else unmark everything, or leave the search or the filter) | — | ⏳ | — | — | — | — | Esc<br>Ctrl+[ | Esc |
| Visual mode: mark the rows the cursor moves over | — | ⏳ | — | v | — | — | v | V |
| Visual mode: unmark the rows the cursor moves over | — | ⏳ | — | — | — | — | V | u V |
| Tag or untag the marked entries, or the one under the cursor | — | ⏳ | — | — | — | — | — | t |
| Tag or untag with a character of one's own | — | ⏳ | — | — | — | — | — | ", then the character |
| Untag the marked entries, or the one under the cursor | — | ⏳ | — | — | — | — | — | u t |
| Selected names to clipboard | — | ⏳ | — | — | — | Ctrl+Insert (with empty command line)<br>Ctrl+Shift+Insert | c f | y n |
| Full names to clipboard | — | ⏳ | — | — | — | Alt+Shift+Insert | c c | y p |
| Paths of their directories to clipboard | — | ⏳ | — | — | — | — | c d | y d |
| Names without the extension to clipboard | — | ⏳ | — | — | — | — | c n | y . |
| URLs of the files / of their directories to clipboard | — | ⏳ | — | — | — | — | c C / c D | — |
| Real (UNC) names to clipboard | — | ⏳ | — | — | — | Ctrl+Alt+Insert | — | — |
| Copy files to clipboard | — | ⏳ | — | — | — | Ctrl+C | — | — |
| Cut files to clipboard | — | ⏳ | — | — | — | Ctrl+X | — | — |
| Start quick search backwards | — | ⏳ | — | — | — | — | ? | — |
| Search the names for a pattern typed on the console | — | ⏳ | — | — | — | — | — | / |
| The next tagged file, or the next file by size, type, or time | — | ⏳ | — | — | — | — | — | c t (tagged)<br>c s (size)<br>c i (mime type)<br>c c (ctime)<br>c m (mtime)<br>c a (atime) |
| Show only the names that match, as the pattern is typed | — | ⏳ | — | — | — | — | f | z f<br>z z |
| Filter: directories / files / links | — | ⏳ | — | — | — | — | — | . d / . f / . l |
| Filter: by mime type / name / hash | — | ⏳ | — | — | — | — | — | . m / . n / . # |
| Filter: duplicates / unique files | — | ⏳ | — | — | — | — | — | . " / . ' |
| Filters: or / and / not | — | ⏳ | — | — | — | — | — | . \| / . & / . ! |
| Filters: rotate / clear / decompose / pop / show | — | ⏳ | — | — | — | — | — | . r / . c / . \* / . p / . . |
| Read the directory again or not (freeze) | — | ⏳ | — | — | — | — | — | F |
| Forget the directories read and read this one again | — | ⏳ | — | — | — | — | — | Ctrl+r |
| The log of messages | — | ⏳ | — | — | — | — | — | W |
| The console, with the command before | — | ⏳ | — | — | — | — | — | Ctrl+p |

## Editing text (`input`)

The keys that the text fields share, which edit the text: quick search, renaming in place, the text
fields of dialogs, the command line, and the filters of the lists fall back to it, and their
sections list only the keys they add. Keys never go to it alone; the help lists its keys with each
of those fields. The mc column has the keys of mc's input lines, its command line among them; the
far column, those of Far's command line. The yazi column has the keys of its input boxes, which open
in insert mode; its letters, digits, and symbols without a modifier work in normal mode, after Esc,
as in vi. The ranger column has the keys of its console.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| One character left | `left` | ✅ | Left<br>Ctrl+b | Left | Ctrl+b<br>Left<br>Alt+Left | Left<br>Ctrl+s | h<br>Left<br>Ctrl+b | Left<br>Ctrl+b |
| One character right | `right` | ✅ | Right<br>Ctrl+f | Right | Ctrl+f<br>Right<br>Alt+Right | Right<br>Ctrl+d | l<br>Right<br>Ctrl+f | Right<br>Ctrl+f |
| Start of the text | `text_start` | ✅ | Home<br>Ctrl+a | Home<br>Ctrl+b<br>Ctrl+a | Ctrl+a<br>Home<br>A1<br>Alt+< | Ctrl+Home | 0<br>Ctrl+a<br>Home | Home<br>Ctrl+a |
| End of the text | `text_end` | ✅ | End<br>Ctrl+e | End<br>Ctrl+e | Ctrl+e<br>End<br>C1<br>Alt+> | Ctrl+End | $<br>Ctrl+e<br>End | End<br>Ctrl+e |
| One word left: the start of the word before the cursor | `word_left` | ✅ | Ctrl+Left<br>Alt+b | Shift+Left<br>Ctrl+Left | Alt+b<br>Ctrl+Left | Ctrl+Left | b<br>B (WORD)<br>Alt+b<br>Ctrl+Left | Alt+b<br>Alt+Left |
| One word right: the end of the word after the cursor | `word_right` | ✅ | Ctrl+Right<br>Alt+f | Shift+Right<br>Ctrl+Right | Alt+f<br>Ctrl+Right | Ctrl+Right | w<br>W (WORD)<br>e (its end)<br>E (the WORD's end)<br>Alt+f (its end)<br>Ctrl+Right (its end) | Alt+f<br>Alt+Right |
| Delete the character before the cursor | `backspace` | ✅ | Backspace<br>Ctrl+h | Backspace<br>Ctrl+h | Ctrl+h<br>Backspace | Backspace | Backspace<br>Ctrl+h | Backspace<br>Ctrl+h |
| Delete the character at the cursor | `delete_char` | ✅ | Delete<br>Ctrl+d | Delete | Ctrl+d<br>Delete | Delete | Delete<br>Ctrl+d | Delete<br>Ctrl+d |
| Delete to the start | `delete_to_start` | ✅ | Ctrl+u | Ctrl+u | Ctrl+w (nothing selected) | — | Ctrl+u | Ctrl+u |
| Delete to the end | `delete_to_end` | ✅ | Ctrl+k | Ctrl+k | Ctrl+k | Ctrl+k | Ctrl+k | Ctrl+k |
| Delete the word before the cursor, and the spaces after it, as the shell's Ctrl+w | `delete_word` | ✅ | Ctrl+w<br>Alt+Backspace<br>Ctrl+Backspace | Ctrl+w<br>Alt+Backspace<br>Ctrl+Backspace | Ctrl+Alt+h<br>Alt+Backspace | Ctrl+Backspace | Ctrl+w<br>Ctrl+Backspace | Ctrl+w |
| Delete the word after the cursor, and the spaces before it, as the shell's Alt+d | `delete_next_word` | ✅ | Alt+d<br>Ctrl+Delete | Ctrl+Delete | Alt+d | Ctrl+Delete | Alt+d<br>Ctrl+Delete | Alt+d |
| Select text | — | ⏳ | — | — | Shift+Left<br>Shift+Right<br>Ctrl+Shift+Left<br>Ctrl+Shift+Right<br>Shift+Home<br>Shift+End | — | — | — |
| Cut the selection | — | ⏳ | — | — | — | — | d<br>D (to the end)<br>x (one character) | — |
| Delete the selection | — | ⏳ | — | — | Ctrl+w | — | — | — |
| Copy | — | ⏳ | — | — | Alt+w | Ctrl+Insert | y | — |
| Paste | — | ⏳ | — | — | Ctrl+y | Shift+Insert | p<br>P (before the cursor) | Ctrl+y |
| Normal mode, as in vi; in normal mode, cancel | — | ⏳ | — | — | — | — | Esc<br>Ctrl+[ | — |
| Insert mode: at the cursor / at the first character that is not a space / after the cursor / at the end | — | ⏳ | — | — | — | — | i / I / a / A | — |
| Visual mode: select characters | — | ⏳ | — | — | — | — | v | — |
| Select the whole text | — | ⏳ | — | — | — | — | V<br>Ctrl+A<br>Ctrl+E | — |
| The first character that is not a space | — | ⏳ | — | — | — | — | _<br>^ | — |
| Replace the character at the cursor | — | ⏳ | — | — | — | — | r | — |
| Cut, then insert mode | — | ⏳ | — | — | — | — | c<br>C (to the end)<br>s (one character)<br>S (the whole text) | — |
| Undo | — | ⏳ | — | — | — | — | u | — |
| Redo | — | ⏳ | — | — | — | — | Ctrl+r | — |
| Uppercase / lowercase the selection | — | ⏳ | — | — | — | — | U / u | — |
| Swap the characters / the words at the cursor | — | ⏳ | — | — | — | — | — | Ctrl+t / Alt+t |

## Quick search (`quick_search`)

In the active panel; characters are part of the name searched for, the keys of `input` edit it,
and other keys end the search and go to the panel: Home and End move the text cursor here, not
the panel's. An edit searches again from the name under the cursor, which stays if it still
matches; as in mc, a character that nothing matches is dropped. In the default preset, Enter
ends the search and opens the entry, through the panel, as in mc, and Ctrl+g gives the search up
and Ctrl+r goes to the previous match, as in emacs's search; in the vim preset, Enter only ends
it, on the match, and Esc gives it up, as in vim. The last search stays for n, N, and Shift+F7
in the panel. yazi
searches as the name is typed in an input box, and ranger as it is typed on its console; both go
to the next and previous match with keys of the panel, once the search is over.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| End the search; the cursor stays on the match | `cancel` | ✅ | Esc<br>Ctrl+[<br>Ctrl+c | Enter<br>Ctrl+m | Esc | — | Ctrl+c<br>Esc Esc | Esc<br>Ctrl+c<br>Ctrl+g |
| Give the search up: the cursor goes back to where it started, and the search is not kept | `abort` | ✅ | Ctrl+g | Esc<br>Ctrl+[<br>Ctrl+c | — | — | — | — |
| Next match; with nothing typed, the last search again, as mc's Ctrl+s Ctrl+s | `quick_search` | ✅ | Ctrl+s<br>Alt+s | Ctrl+g | Ctrl+s<br>Alt+s | Ctrl+Enter | n | n |
| Previous match | `prev_match` | ✅ | Ctrl+r | Ctrl+t | — | Ctrl+Shift+Enter | N | N |
| Paste from clipboard | — | 🚫 | — | — | — | Ctrl+v<br>Shift+Insert | — | — |

## Renaming in place (`rename`)

Shift+F6 edits the name in the entry's row with the keys of `input`; other keys, but the global
ones, do nothing. None of mc, Far, yazi, and ranger renames in the row: mc and Far ask in a
dialog, yazi in an input box, and ranger on its console, whose keys are those of `input`.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Rename to the name typed | `confirm` | ✅ | Enter<br>Ctrl+m | Enter<br>Ctrl+m | — | — | — | — |
| Keep the name as it was | `cancel` | ✅ | Esc<br>Ctrl+[<br>Ctrl+g<br>Ctrl+c | Esc<br>Ctrl+[<br>Ctrl+c | — | — | — | — |

## Dialogs and help (`dialog`)

A dialog whose focus is on a button, a check box, or a list, and the windows of the jobs, a job,
and checksums; the help falls back to it, so its keys scroll and close the help too. Dialogs are
modal: the panel's keys do nothing there. The yazi column has the keys of its confirmations
(confirm), its list of tasks (tasks), and its help; the ranger column, those of its list of tasks,
since ranger asks its questions on the console.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Previous button; in the help and lists, one row up | `up` | ✅ | Up<br>Ctrl+p | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p (help, lists) | Up (help) | k<br>Up<br>Ctrl+p (help) | Up<br>k<br>Ctrl+p |
| Next button; in the help and lists, one row down | `down` | ✅ | Down<br>Ctrl+n | j<br>Down<br>Ctrl+n | Down<br>Ctrl+n (help, lists) | Down (help) | j<br>Down<br>Ctrl+n (help) | Down<br>j<br>Ctrl+n<br>Enter |
| Previous button | `left` | ✅ | Left | h<br>Left | Left | — | — | — |
| Next button | `right` | ✅ | Right | l<br>Right | Right | — | — | — |
| In the help and lists, one page up | `page_up` | ✅ | PgUp | Ctrl+b<br>PgUp | PgUp<br>Alt+v<br>b (help)<br>Backspace (help) | — | — | p<br>b<br>Ctrl+b |
| In the help and lists, one page down | `page_down` | ✅ | PgDn | Ctrl+f<br>PgDn | PgDn<br>Ctrl+v<br>Space (help)<br>f (help) | — | — | n<br>f<br>Ctrl+f<br>Space |
| In the help and lists, the first row | `home` | ✅ | Home | g g<br>Home | Home<br>A1<br>g (help)<br>Ctrl+Home (help)<br>Ctrl+PgUp (help)<br>Alt+< | — | — | Home<br>g |
| In the help and lists, the last row | `end` | ✅ | End | G<br>End | End<br>C1<br>G (help)<br>Ctrl+End (help)<br>Ctrl+PgDn (help)<br>Alt+> | — | — | End<br>G |
| Next field or button | `next_field` | ✅ | Tab<br>Ctrl+i | Tab<br>Ctrl+i | Tab | — | — | — |
| Previous field or button | `prev_field` | ✅ | Shift+Tab | Shift+Tab | Shift+Tab | — | — | — |
| Press the focused button; from a text field or a check box, the default one; close the help | `confirm` | ✅ | Enter<br>Ctrl+m | Enter<br>Ctrl+m | Enter | Ctrl+Enter (default button) | Enter (confirm, help)<br>y (confirm) | — |
| Switch the check box, choose the radio button, or press the focused button | `toggle` | ✅ | Space | Space | Space | — | — | — |
| Cancel; abort the job in front; close the help or the window | `cancel` | ✅ | Esc<br>Ctrl+[<br>F10<br>Ctrl+g<br>Ctrl+c | Esc<br>Ctrl+[<br>Ctrl+c<br>F10 | Esc<br>Esc Esc<br>F10<br>Ctrl+g<br>Ctrl+c (abort copy/delete) | Esc | Esc<br>Ctrl+[<br>Ctrl+c<br>n (confirm)<br>w (tasks) | Esc<br>q<br>Q<br>w<br>Ctrl+c |
| Copy/move in background (in dialog) | — | ⏳ | — | — | Alt+b | — | — | — |
| Focus first dialog item | — | ⏳ | — | — | — | Home | — | — |
| Focus default dialog item | — | ⏳ | — | — | — | PgDn<br>End | — | — |
| Move dialog | — | ⏳ | — | — | — | Ctrl+F5 | — | — |
| Checkbox: on / off / undefined | — | ⏳ | — | — | — | Numpad+ / Numpad- / Numpad* | — | — |
| Help: follow link | — | ⏳ | — | — | Enter<br>Right | Enter | — | — |
| Help: next / previous link | — | ⏳ | — | — | Tab / Alt+Tab | Tab / Shift+Tab | — | — |
| Help: previous topic | — | ⏳ | — | — | F3<br>Left<br>l | Alt+F1<br>Backspace | — | — |
| Help: contents | — | ⏳ | — | — | F2<br>c | Shift+F1 | — | — |
| Help: plugins help | — | ⏳ | — | — | — | Shift+F2 | — | — |
| Help: search | — | ⏳ | — | — | — | F7 | — | — |
| Help: maximize/restore window | — | ⏳ | — | — | — | F5 | — | — |
| Help: full list of help keys | — | ⏳ | — | — | F1 (again) | — | — | — |
| The output of the job under the cursor | — | ⏳ | — | — | — | — | Enter (tasks) | — |
| Abort the job under the cursor | — | ⏳ | — | — | — | — | x (tasks) | d d<br>Delete |
| Run the job under the cursor first | — | ⏳ | — | — | — | — | — | K<br>PgUp |
| Run the job under the cursor last | — | ⏳ | — | — | — | — | — | J<br>PgDn |

## Help (`help`)

The help of F1. Keys it does not bind go to the dialog, whose keys scroll the help and close it;
the keys of the other programs' help are there too. The default preset adds the page keys of its
lists and the keys of mc's help, whose Space turns the page here rather than press a button; the
vim preset, Ctrl+u and Ctrl+d, as in vim and a pager.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| One page up | `page_up` | ✅ | Alt+v<br>b<br>Backspace<br>Ctrl+h | — | b<br>Backspace | — | — | — |
| One page down | `page_down` | ✅ | Ctrl+v<br>Space<br>f | — | Space<br>f | — | — | — |
| Half a page up | `half_page_up` | ✅ | u | Ctrl+u | u | — | — | — |
| Half a page down | `half_page_down` | ✅ | d | Ctrl+d | d | — | — | — |
| The top | `home` | ✅ | Alt+<<br>g<br>Ctrl+Home<br>Ctrl+PgUp | — | g<br>Ctrl+Home<br>Ctrl+PgUp | — | — | — |
| The end | `end` | ✅ | Alt+><br>G<br>Ctrl+End<br>Ctrl+PgDn | — | G<br>Ctrl+End<br>Ctrl+PgDn | — | — | — |
| Close the help | `cancel` | ✅ | — | q | F10<br>Esc | Esc | Esc<br>Ctrl+[<br>Ctrl+c | — |

## Viewer (`viewer`)

yazi has no viewer: it previews the file under the cursor beside the list. The ranger column has
the keys of its pager, which shows files, the output of commands, and the log.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| One line up | `up` | ✅ | Up<br>k<br>y<br>Insert<br>Ctrl+p | k<br>Ctrl+y<br>Up<br>Ctrl+p | Up<br>k<br>y<br>Insert<br>Ctrl+p | Up | — | Up<br>k<br>Ctrl+p |
| One line down | `down` | ✅ | Down<br>j<br>e<br>Enter<br>Ctrl+m<br>Delete<br>Ctrl+n | j<br>Ctrl+e<br>Down<br>Ctrl+n<br>Enter<br>Ctrl+m | Down<br>j<br>e<br>Enter<br>Delete<br>Ctrl+n | Down | — | Down<br>j<br>Ctrl+n<br>Enter |
| One page up | `page_up` | ✅ | PgUp<br>Alt+v<br>b<br>Backspace<br>Ctrl+h | Ctrl+b<br>Shift+Up<br>PgUp | PgUp<br>Alt+v<br>b<br>Backspace<br>Ctrl+h | PgUp | — | PgUp<br>p<br>b<br>Ctrl+b |
| One page down | `page_down` | ✅ | PgDn<br>Space<br>f<br>Ctrl+v | Ctrl+f<br>Shift+Down<br>PgDn | PgDn<br>Space<br>f<br>Ctrl+v | PgDn | — | PgDn<br>n<br>f<br>Ctrl+f<br>Space |
| Half a page up | `half_page_up` | ✅ | u | Ctrl+u | u | — | — | Ctrl+u<br>u |
| Half a page down | `half_page_down` | ✅ | d | Ctrl+d | d | — | — | Ctrl+d<br>d |
| The start of the file | `home` | ✅ | Home<br>g<br>Ctrl+Home<br>Ctrl+PgUp<br>Alt+< | g g<br>Home | Home<br>A1<br>g<br>Ctrl+Home<br>Ctrl+PgUp<br>Alt+< | Home<br>Ctrl+Home | — | Home<br>g |
| The end of the file | `end` | ✅ | End<br>G<br>Ctrl+End<br>Ctrl+PgDn<br>Alt+> | G<br>End | End<br>C1<br>G<br>Ctrl+End<br>Ctrl+PgDn<br>Alt+> | End<br>Ctrl+End | — | End<br>G |
| One column left, when lines are cut (ranger: four) | `left` | ✅ | Left<br>h | h<br>Left | Left<br>h | Left | — | Left<br>h |
| One column right, when lines are cut (ranger: four) | `right` | ✅ | Right<br>l | l<br>Right | Right<br>l | Right | — | Right<br>l |
| Wrap long lines, or cut them | `toggle_wrap` | ✅ | F2 | F2 | F2 | F2 | — | — |
| Help | `help` | ✅ | F1 | g ?<br>F1 | F1 | F1 | — | — |
| Close the viewer | `quit` | ✅ | F3<br>F10<br>q<br>Esc<br>Ctrl+[<br>Ctrl+c | q<br>Esc<br>Ctrl+[<br>Ctrl+c<br>F3<br>F10 | F3<br>F10<br>q<br>Esc | F10<br>F3<br>Numpad5<br>Esc | — | Esc<br>q<br>Q<br>i<br>F3 |
| F1…F10 on terminals without function keys | — | ✅ | Alt+1…9, 0 | Alt+1…9, 0 | Esc, then 1…9, 0 | — | — | — |
| Wrap type (by chars/words) | — | ⏳ | — | — | — | Shift+F2 | — | — |
| Hex/code mode | — | ⏳ | — | — | F4 | F4 | — | — |
| Select mode (text/code/dump) | — | ⏳ | — | — | — | Shift+F4 | — | — |
| Go to position | — | ⏳ | — | — | F5 | Alt+F8 | — | — |
| Switch to editor | — | ⏳ | — | — | — | F6 | — | E |
| Search | — | ⏳ | — | — | F7<br>/<br>? (backward) | F7 | — | — |
| Continue search forward | — | ⏳ | — | — | Ctrl+s | Shift+F7<br>Space | — | — |
| Continue search backward | — | ⏳ | — | — | Ctrl+r | Alt+F7 | — | — |
| Continue search in chosen direction | — | ⏳ | — | — | Shift+F7<br>n | — | — | — |
| Temporarily reverse search direction | — | ⏳ | — | — | N | — | — | — |
| Raw/Parsed | — | ⏳ | — | — | F8 | — | — | — |
| Format/Unformat | — | ⏳ | — | — | F9 | — | — | — |
| Code page | — | ⏳ | — | — | Alt+e | F8 (OEM/ANSI)<br>Shift+F8 (menu) | — | — |
| 20 columns left / right | — | ⏳ | — | — | Ctrl+Left / Ctrl+Right (10 columns) | Ctrl+Left / Ctrl+Right | — | — |
| Leftmost / rightmost column | — | ⏳ | — | — | Ctrl+a / Ctrl+e | Ctrl+Shift+Left / Ctrl+Shift+Right | — | — |
| Shift characters/bytes (dump, code) | — | ⏳ | — | — | — | Ctrl+Left / Ctrl+Right | — | — |
| Bytes per line −1 / +1 (code) | — | ⏳ | — | — | — | Alt+Left / Alt+Right | — | — |
| Bytes per line to nearest multiple of 16 (code) | — | ⏳ | — | — | — | Ctrl+Alt+Left / Ctrl+Alt+Right | — | — |
| Set bookmark | — | ⏳ | — | — | m<br>0…9, then m | RightCtrl+0…9<br>Ctrl+Shift+0…9 | — | — |
| Go to bookmark | — | ⏳ | — | — | r<br>0…9, then r | LeftCtrl+0…9 | — | — |
| Return to previous position | — | ⏳ | — | — | — | Alt+Backspace<br>Ctrl+z | — | — |
| Next file | — | ⏳ | — | — | Ctrl+f | Numpad+ | — | — |
| Previous file | — | ⏳ | — | — | Ctrl+b | Numpad- | — | — |
| Ruler | — | ⏳ | — | — | Alt+r | — | — | — |
| User screen | — | ⏳ | — | — | Ctrl+o | Ctrl+o<br>Ctrl+Alt+Shift (temporarily) | — | — |
| Go to file in panel | — | ⏳ | — | — | — | Ctrl+F10 | — | — |
| Plugin commands | — | ⏳ | — | — | — | F11 | — | — |
| View and edit history | — | ⏳ | — | — | Alt+E | Alt+F11 | — | — |
| Far window size | — | ⏳ | — | — | — | Alt+F9 | — | — |
| Viewer settings | — | ⏳ | — | — | — | Alt+Shift+F9 | — | — |
| Functional key bar | — | ⏳ | — | — | — | Ctrl+b | — | — |
| Status line | — | ⏳ | — | — | — | Ctrl+B | — | — |
| Scrollbar | — | ⏳ | — | — | — | Ctrl+s | — | — |
| Copy selection | — | ⏳ | — | — | — | Ctrl+Insert<br>Ctrl+c | — | — |
| Clear selection | — | ⏳ | — | — | — | Ctrl+u | — | — |
| Select text manually | — | ⏳ | — | — | — | Shift+Click | — | — |

## Lists (`list`)

The keys that the windows listing items to choose from share: the location menu, the tab menu, the
zoxide window, the workspaces, and the command history fall back to it, then to `input`, whose keys
edit their filter or keywords, and their sections list only the keys they add. Home and End go to
the first and the last item here, and Ctrl+a and Ctrl+e of `input` to the start and the end of the
filter, in both presets (vim's Ctrl+b stays the page up); Delete takes a character of the filter
where the window does not take it. Keys never go to it alone; the help lists its keys with each
of those windows, with what Enter takes there. The mc column has the keys of mc's lists, such as
its histories and its hotlist; the far column, those of Far's menus; the yazi column, those of its
pick window, which asks for the program to open files with.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Previous item | `up` | ✅ | Up<br>Ctrl+p | Up<br>Ctrl+p | Up<br>Ctrl+p | Up | k<br>Up | — |
| Next item | `down` | ✅ | Down<br>Ctrl+n | Down<br>Ctrl+n | Down<br>Ctrl+n | Down | j<br>Down | — |
| One page up | `page_up` | ✅ | PgUp<br>Alt+v | Ctrl+b<br>Shift+Up<br>PgUp | PgUp<br>Alt+v | — | — | — |
| One page down | `page_down` | ✅ | PgDn<br>Ctrl+v | Ctrl+f<br>Shift+Down<br>PgDn | PgDn<br>Ctrl+v | — | — | — |
| First item | `home` | ✅ | Home<br>Alt+< | Home | Home<br>A1<br>Alt+< | — | — | — |
| Last item | `end` | ✅ | End<br>Alt+> | End | End<br>C1<br>Alt+> | — | — | — |
| Take the item, as the window's section says | `confirm` | ✅ | Enter<br>Ctrl+m | Ctrl+y<br>Enter<br>Ctrl+m | Enter | Enter | Enter | — |
| Close the window | `cancel` | ✅ | Esc<br>Ctrl+[<br>F10<br>Ctrl+g<br>Ctrl+c | Esc<br>Ctrl+[<br>Ctrl+c<br>F10 | Esc<br>F10<br>Ctrl+g | Esc | Esc<br>Ctrl+[<br>Ctrl+c | — |
| F1…F10 on terminals without function keys, in the windows that bind them | — | ⏳ | Alt+1…9, 0 | Alt+1…9, 0 | Esc, then 1…9, 0 | — | — | — |

## Location menu (`location_menu`)

Alt+F1 and Alt+F2: the home directory, the volumes, and the hosts, under their headings;
characters filter it. It falls back to `list`, whose Enter opens the place in the panel, or, on
the last row of a group that shows only its first places, shows the rest. The far column has the
keys of Far's menus, its drive menu among them.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Edit the settings of the host under the cursor | `edit_host` | ✅ | F4 | F4 | — | — | — | — |
| Disconnect the host under the cursor, or stop connecting to it | `disconnect` | ✅ | F8<br>Delete | F8<br>Delete | — | Delete | — | — |
| Read the volumes and hosts again | `reload` | ✅ | Ctrl+r | Ctrl+r | — | Ctrl+r | — | — |
| Filter the items | — | ⏳ | Typing | Typing | — | Ctrl+Alt+f<br>RightAlt | — | — |
| Open one of the first ten places by its number (empty filter) | — | ⏳ | 1…9, 0 | 1…9, 0 | — | — | — | — |
| Lock filter | — | ⏳ | — | — | — | Ctrl+Alt+l | — | — |
| Shift all items by 1 position | — | ⏳ | — | — | — | Alt+Left<br>Alt+Right | — | — |
| Shift selected item by 1 position | — | ⏳ | — | — | — | Alt+Shift+Left<br>Alt+Shift+Right | — | — |
| Shift all items by 20 positions | — | ⏳ | — | — | — | Ctrl+Alt+Left<br>Ctrl+Alt+Right | — | — |
| Shift selected item by 20 positions | — | ⏳ | — | — | — | Ctrl+Shift+Left<br>Ctrl+Shift+Right | — | — |
| Align all items left / right | — | ⏳ | — | — | — | Alt+Home / Alt+End | — | — |
| Align selected item left / right | — | ⏳ | — | — | — | Alt+Shift+Home / Alt+Shift+End | — | — |
| Fixed menu columns | — | ⏳ | — | — | — | Shift+F5 | — | — |

## Tab menu (`tab_menu`)

t h and t l: the tabs of the left or the right panel, drawn over it as the location menu is;
characters filter it. It binds no keys of its own: it falls back to `list`, whose Enter shows the
tab in the panel. mc, Far, yazi, and ranger have no menu of tabs.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Filter the tabs | — | ⏳ | Typing | Typing | — | — | — | — |
| Show a tab by its number (empty filter) | — | ⏳ | 1…9, 0 | 1…9, 0 | — | — | — | — |

## zoxide window (`jump`)

Alt+z: the directories that zoxide ranks; characters are keywords, as z takes them in a shell.
It binds no keys of its own: it falls back to `list`, whose Enter opens the directory in the
active panel.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Open a directory by its number (no keywords) | — | ⏳ | 1…9, 0 | 1…9, 0 | — | — | — | — |

## Workspaces window (`workspaces`)

t w, or F9 → Workspace → Workspace list…; F9 → Workspace lists the first ten too, by their
digits. Characters filter it. It falls back to `list`, whose Enter restores the workspace: it
replaces the tabs of both panels.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Save the tabs of both panels as a new workspace | `save_workspace` | ✅ | Insert | Insert | — | — | — | — |
| Rename the workspace | `move` | ✅ | F6 | F6 | — | — | — | — |
| Delete the workspace | `delete` | ✅ | F8<br>Delete | F8<br>Delete | — | — | — | — |
| Restore a workspace by its number (empty filter) | — | ⏳ | 1…9, 0 | 1…9, 0 | — | — | — | — |

## Pull-down menu (`pull_down`)

F9, or g m in the vim keymap; Shift+F9, or g M, on the command that ran from it last. The
highlighted letter of a menu opens it, and that of a command runs it, so h, j, k, and l do not
move in it.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| The command above | `up` | ✅ | Up<br>Ctrl+p | Up<br>Ctrl+p | Up<br>Ctrl+p | — | — | — |
| The command below | `down` | ✅ | Down<br>Ctrl+n | Down<br>Ctrl+n | Down<br>Ctrl+n | — | — | — |
| The menu to the left | `left` | ✅ | Left<br>Ctrl+b<br>Shift+Tab | Left<br>Shift+Tab | Left<br>Ctrl+b | — | — | — |
| The menu to the right | `right` | ✅ | Right<br>Ctrl+f<br>Tab<br>Ctrl+i | Right<br>Tab<br>Ctrl+i | Right<br>Ctrl+f | — | — | — |
| The first command | `home` | ✅ | Home<br>Alt+<<br>PgUp | Ctrl+b<br>Shift+Up<br>Home<br>PgUp | Home<br>Alt+< | — | — | — |
| The last command | `end` | ✅ | End<br>Alt+><br>PgDn | Ctrl+f<br>Shift+Down<br>End<br>PgDn | End<br>Alt+> | — | — | — |
| Open the menu, or run the command | `confirm` | ✅ | Enter<br>Ctrl+m | Ctrl+y<br>Enter<br>Ctrl+m | Enter | — | — | — |
| Close the menu, then the menu bar | `cancel` | ✅ | Esc<br>Ctrl+[<br>F9<br>F10<br>Ctrl+g<br>Ctrl+c | Esc<br>Ctrl+[<br>Ctrl+c<br>F9<br>F10 | Esc<br>F10<br>Ctrl+g | — | — | — |
| Open a menu or run a command by its letter | — | ⏳ | Letter | Letter | Letter | — | — | — |

## Text fields (`dialog_input`)

A text field in a dialog; it falls back to `input`, whose keys edit the text, then to the
dialog. The mc column has the keys of mc's input lines; the far column, those of Far's dialogs.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Input line history | — | ⏳ | — | — | Alt+h | Ctrl+Up (in dialogs)<br>Ctrl+Down (in dialogs) | — | — |
| Previous history entry | — | ⏳ | — | — | Alt+p<br>Ctrl+Down | Ctrl+e | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p |
| Next history entry | — | ⏳ | — | — | Alt+n<br>Ctrl+Up | Ctrl+x | j<br>Down<br>Ctrl+n | Down<br>Ctrl+n |
| Dialog history: clear | — | ⏳ | — | — | Shift+Delete<br>D | Delete | — | — |
| Dialog history: delete item | — | ⏳ | — | — | Delete<br>d | Shift+Delete | — | — |
| Dialog history: mark item | — | ⏳ | — | — | — | Insert | — | — |
| Insert file name under cursor into dialog | — | ⏳ | — | — | — | Shift+Enter | — | — |
| Insert passive panel file name into dialog | — | ⏳ | — | — | — | Ctrl+Shift+Enter | — | — |

## Path fields (`path_input`)

A text field for a path, in Quick cd, F5, F6, and F7; keys it does not bind go to text fields.
yazi lists the choices for a path as it is typed, without a key; ranger completes on its console
without a list.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Complete the path; again: list the choices | `complete` | ✅ | Tab<br>Ctrl+i | Tab<br>Ctrl+i | Alt+Tab | — | — | Tab |

## Completion list (`completion`)

Under a path field after a Tab that gets no further; keys it does not bind close it and go to
the field. ranger has no list: Tab and Shift+Tab put the next and the previous choice in the
console's line.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Previous choice | `up` | ✅ | Up<br>Ctrl+p | Up<br>Ctrl+p | — | — | Alt+k<br>Up<br>Ctrl+p | Shift+Tab |
| Next choice | `down` | ✅ | Down<br>Ctrl+n | Down<br>Ctrl+n | — | — | Alt+j<br>Down<br>Ctrl+n | — |
| One page up | `page_up` | ✅ | PgUp<br>Alt+v | Ctrl+b<br>PgUp | — | — | — | — |
| One page down | `page_down` | ✅ | PgDn<br>Ctrl+v | Ctrl+f<br>PgDn | — | — | — | — |
| First choice | `home` | ✅ | Home<br>Alt+< | Home | — | — | — | — |
| Last choice | `end` | ✅ | End<br>Alt+> | End | — | — | — | — |
| Next choice, round | `complete` | ✅ | Tab<br>Ctrl+i | Tab<br>Ctrl+i | — | — | — | Tab |
| Put the choice in the field | `confirm` | ✅ | Enter<br>Ctrl+m | Ctrl+y<br>Enter<br>Ctrl+m | — | — | Tab | — |
| Close the list | `cancel` | ✅ | Esc<br>Ctrl+[<br>Ctrl+g<br>Ctrl+c | Esc<br>Ctrl+[<br>Ctrl+c | — | — | Ctrl+c | — |
| Close the list and edit the field | — | ⏳ | Other keys | Other keys | — | — | — | — |
| Put the choice in the field and run it | — | ⏳ | — | — | — | — | Enter | — |

## Command line (`command_line`)

`!` and `:` open it; it falls back to `input`, whose Home and End go to the start and the end of
the line here, and whose Ctrl+u and Ctrl+k delete to them. Other keys, but the global ones, do
nothing. In mc and Far the command line is always under the panels, so their keys here are
pressed in the panels. yazi asks for a shell command in an input box (; and : in the panels),
and ranger's console (: and !) is its command line. The keys of their input lines that noc
lacks, such as words and the clipboard, are under `input`, as are yazi's modes.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| The line above; from the first line, the command before in the history of the panel's host | `up` | ✅ | Up | Up | — | — | k<br>Up | Up |
| The line below; from the last line, the command after in the history | `down` | ✅ | Down | Down | — | — | j<br>Down | Down |
| A new line in the command | `new_line` | ✅ | Ctrl+j<br>Shift+Enter (kitty keyboard protocol) | Ctrl+j<br>Shift+Enter (kitty keyboard protocol) | — | — | — | — |
| Edit the command in $VISUAL or $EDITOR | `edit_command` | ✅ | Ctrl+x Ctrl+e | Ctrl+f<br>Ctrl+x Ctrl+e | — | — | — | — |
| The command before in the history of the panel's host | `older_command` | ✅ | Alt+p<br>Ctrl+p | Ctrl+p | Alt+p<br>Ctrl+Down | Ctrl+e | Ctrl+p | Ctrl+p |
| The command after in the history of the panel's host | `newer_command` | ✅ | Alt+n<br>Ctrl+n | Ctrl+n | Alt+n<br>Ctrl+Up | Ctrl+x | Ctrl+n | Ctrl+n |
| The command history | `command_history` | ✅ | Alt+h<br>Ctrl+r | Ctrl+r | Alt+h | Alt+F8 | — | — |
| Run the command; after a \\ at the end of the line, a new line | `confirm` | ✅ | Enter<br>Ctrl+m | Enter<br>Ctrl+m | Enter | Enter | Enter | Enter<br>Ctrl+j |
| Close the command line | `cancel` | ✅ | Esc<br>Ctrl+[<br>Ctrl+g<br>Ctrl+c<br>F10 | Esc<br>Ctrl+[<br>Ctrl+c<br>F10 | — | Esc | Ctrl+c<br>Esc Esc | Esc<br>Ctrl+c<br>Ctrl+g |
| Insert current file name | — | ⏳ | — | — | Alt+Enter<br>Ctrl+Enter | Ctrl+j<br>Ctrl+Enter | — | — |
| Insert file name from passive panel | — | ⏳ | — | — | — | Ctrl+Shift+Enter | — | — |
| Insert full name of current file | — | ⏳ | — | — | Ctrl+Shift+Enter | Ctrl+f | — | — |
| Insert full file name from passive panel | — | ⏳ | — | — | — | Ctrl+; | — | — |
| Insert UNC file name (active / passive) | — | ⏳ | — | — | — | Ctrl+Alt+f / Ctrl+Alt+; | — | — |
| Insert tagged files of current panel | — | ⏳ | — | — | Ctrl+x t | — | — | — |
| Insert tagged files of other panel | — | ⏳ | — | — | Ctrl+x Ctrl+t | — | — | — |
| Insert current panel path | — | ⏳ | — | — | Ctrl+x p<br>Alt+a | Ctrl+Shift+[ | — | — |
| Insert other panel path | — | ⏳ | — | — | Ctrl+x Ctrl+p<br>Alt+A | Ctrl+Shift+] | — | — |
| Insert left / right panel path | — | ⏳ | — | — | — | Ctrl+[ / Ctrl+] | — | — |
| Insert UNC path of left / right panel | — | ⏳ | — | — | — | Ctrl+Alt+[ / Ctrl+Alt+] | — | — |
| Insert UNC path of active / passive panel | — | ⏳ | — | — | — | Alt+Shift+[ / Alt+Shift+] | — | — |
| Completion | — | ⏳ | — | — | Alt+Tab | Ctrl+Space | — | Tab |
| Insert character literally (quote) | — | ⏳ | — | — | Ctrl+q | — | — | — |
| Clear command line | — | ⏳ | — | — | — | Ctrl+y | — | — |
| Select block in command line | — | ⏳ | — | — | Shift+Left<br>Shift+Right<br>Shift+Home<br>Shift+End<br>Ctrl+Shift+Left<br>Ctrl+Shift+Right | Alt+Shift+Left<br>Alt+Shift+Right<br>Alt+Shift+Home<br>Alt+Shift+End | — | — |

## Command history (`command_history`)

Alt+h in the panels and on the command line; characters filter it. It falls back to `list`,
whose Enter puts the command on the command line without running it, as Ctrl+Enter does in
Far. Far's rows are those of its history menus.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| The commands of the panel's host, or of all hosts | `next_field` | ✅ | Tab<br>Ctrl+i | Tab<br>Ctrl+i | — | — | — | — |
| Remove the command from the history | `delete` | ✅ | Delete<br>Ctrl+Backspace<br>F8 | Delete<br>Ctrl+Backspace<br>F8 | Delete<br>d | Shift+Delete | — | — |
| Re-run command / open item | — | ⏳ | — | — | — | Enter | — | — |
| Run in separate window | — | ⏳ | — | — | — | Shift+Enter | — | — |
| Run as administrator | — | ⏳ | — | — | — | Ctrl+Alt+Enter | — | — |
| Folder history menu: go to on passive panel | — | ⏳ | — | — | — | Ctrl+Shift+Enter | — | — |
| Clear history | — | ⏳ | — | — | Shift+Delete<br>D | Delete | — | — |
| Lock/unlock item | — | ⏳ | — | — | — | Insert | — | — |
| Refresh (remove unavailable) | — | ⏳ | — | — | — | Ctrl+r (view and folder history) | — | — |
| Copy item to clipboard | — | ⏳ | — | — | — | Ctrl+c<br>Ctrl+Insert | — | — |
| View history menu: open in editor | — | ⏳ | — | — | F4 | F4 | — | — |
| View history menu: open in viewer | — | ⏳ | — | — | F3 | F3<br>Numpad5 | — | — |
| Command history menu: additional info | — | ⏳ | — | — | — | F3 | — | — |

## Output of commands (`user_screen`)

The terminal's own screen, with the output of commands, in place of the panels; only the keys
that bring the panels back do anything. The vim preset takes Enter and q too, as the prompt under
the output that vim's g < shows does.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Back to the panels | `cancel` | ✅ | Ctrl+o<br>Esc<br>Ctrl+[<br>Ctrl+\` | g <<br>Enter<br>Ctrl+m<br>q<br>Esc<br>Ctrl+[<br>Ctrl+\` | Ctrl+o | Ctrl+o | — | — |

## Mouse

Not in the keymap: a press acts on what is under it. yazi and ranger take the mouse only in the
panel, its columns, and its tabs, and ranger in its pager too; neither has a double click.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Move the cursor to a row, activate the panel | — | ⏳ | Click | Click | Click | Click | Click | Click |
| Open the row (as Enter) | — | ⏳ | Double click | Double click | Double click | Double click<br>Middle click | Right click | Right click |
| Mark the row, cursor stays on it | — | ⏳ | Right click | Right click | Right click | Right click | — | — |
| Scroll a panel or the viewer | — | ⏳ | Wheel | Wheel | Wheel | Wheel | Wheel | Wheel |
| Press an F key | — | ⏳ | Click on the F-key bar | Click on the F-key bar | Click on the button bar | Click on the key bar | — | — |
| Show a tab | — | ⏳ | Click on the tab | Click on the tab | — | — | Click on the tab | Click on the tab |
| Open a menu of the menu bar / run a command | — | ⏳ | Click | Click | Click | Click | — | — |
| Close the menu bar | — | ⏳ | Click outside it | Click outside it | Click outside it | Click outside it | — | — |
| Dialog: press a button, switch a check box, choose a radio button | — | ⏳ | Click | Click | Click | Click | — | — |
| Dialog: put the cursor in a text field | — | ⏳ | Click | Click | Click | Click | — | — |
| Dialog: choose a radio button and press the default button | — | ⏳ | Double click | Double click | — | — | — | — |
| Menu or list in a window: move the cursor / open the row | — | ⏳ | Click / Double click | Click / Double click | Click / Double click | Click / Click in a menu, Double click in a dialog's list | — | — |
| Close a menu or the list of completions | — | ⏳ | Click outside it | Click outside it | Click outside a menu | Click outside it | — | — |
| Scroll a menu, a list, or the help | — | ⏳ | Wheel | Wheel | Wheel (not the help) | Wheel | — | — |
| Back / forward in the directory history | — | ⏳ | — | — | Click on < / > in the panel's frame | — | Back button / Forward button | — |
| Go up to a directory on the left, on the row clicked | — | ⏳ | — | — | — | — | Click in the parent column | Click in the parent column |
| Enter the directory previewed, on the row clicked | — | ⏳ | — | — | — | — | Click in the preview | Click in the preview |
| Scroll the preview | — | ⏳ | — | — | — | — | Wheel on the preview | Wheel on the preview of a file |
| The next / previous directory beside this one | — | ⏳ | — | — | — | — | — | Wheel on the parent column |
| The next / previous tab | — | ⏳ | — | — | — | — | — | Wheel on the tab bar |
| Go to a directory of the path in the title bar | — | ⏳ | — | — | — | — | — | Click on it |
| Scroll five times as far | — | ⏳ | — | — | — | — | — | Ctrl+Wheel |
| Resize the columns | — | ⏳ | — | — | — | — | Drag a border of the list | — |

## Editor (mc, Far)

noc edits files in `$VISUAL` or `$EDITOR`.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Character left / right | — | ⏳ | — | — | Left / Right | Left / Right | — | — |
| Character left without wrapping to previous line | — | ⏳ | — | — | — | Ctrl+s | — | — |
| Line up / down | — | ⏳ | — | — | Up / Down | Up / Down | — | — |
| Word left / right | — | ⏳ | — | — | Ctrl+Left / Ctrl+Right | Ctrl+Left / Ctrl+Right | — | — |
| Scroll screen up / down | — | ⏳ | — | — | Ctrl+Up / Ctrl+Down | Ctrl+Up / Ctrl+Down | — | — |
| Page up / down | — | ⏳ | — | — | PgUp / PgDn | PgUp / PgDn | — | — |
| Beginning / end of line | — | ⏳ | — | — | Home / End | Home / End | — | — |
| Beginning of file | — | ⏳ | — | — | Ctrl+Home<br>Alt+< | Ctrl+Home<br>Ctrl+PgUp | — | — |
| End of file | — | ⏳ | — | — | Ctrl+End<br>Alt+> | Ctrl+End<br>Ctrl+PgDn | — | — |
| Beginning / end of screen | — | ⏳ | — | — | Ctrl+PgUp / Ctrl+PgDn | Ctrl+n / Ctrl+e | — | — |
| Delete character | — | ⏳ | — | — | Delete<br>Ctrl+d | Delete | — | — |
| Delete character left | — | ⏳ | — | — | Backspace<br>Ctrl+h | Backspace | — | — |
| Delete line | — | ⏳ | — | — | Ctrl+y | Ctrl+y | — | — |
| Delete to end of line | — | ⏳ | — | — | Ctrl+k | Ctrl+k | — | — |
| Delete word left | — | ⏳ | — | — | Alt+Backspace | Ctrl+Backspace | — | — |
| Delete word right | — | ⏳ | — | — | Alt+d | Ctrl+t<br>Ctrl+Delete | — | — |
| Block selection | — | ⏳ | — | — | Shift+cursor keys<br>Ctrl+Shift+cursor keys<br>F3 (start, then end) | Shift+cursor keys<br>Ctrl+Shift+cursor keys | — | — |
| Vertical block | — | ⏳ | — | — | Alt+cursor keys<br>Shift+F3 (start, then end) | Alt+cursor keys (not on the numpad)<br>Alt+Shift+cursor keys<br>Ctrl+Alt+cursor keys (not on the numpad) | — | — |
| Select all text | — | ⏳ | — | — | — | Ctrl+a | — | — |
| Clear selection | — | ⏳ | — | — | — | Ctrl+u | — | — |
| Copy block to clipboard | — | ⏳ | — | — | Ctrl+Insert (to mcedit.clip) | Ctrl+Insert<br>Ctrl+c | — | — |
| Paste from clipboard | — | ⏳ | — | — | Shift+Insert | Shift+Insert<br>Ctrl+v | — | — |
| Cut to clipboard | — | ⏳ | — | — | Shift+Delete | Shift+Delete<br>Ctrl+x | — | — |
| Append block to clipboard | — | ⏳ | — | — | — | Ctrl+Numpad+ | — | — |
| Delete block | — | ⏳ | — | — | F8 | Ctrl+d | — | — |
| Copy block to cursor position | — | ⏳ | — | — | F5 | Ctrl+p (persistent blocks) | — | — |
| Move block to cursor position | — | ⏳ | — | — | F6 | Ctrl+m (persistent blocks) | — | — |
| Indent block left / right | — | ⏳ | — | — | — | Alt+u / Alt+i | — | — |
| Format the paragraph | — | ⏳ | — | — | Alt+p | — | — | — |
| Help | — | ⏳ | — | — | F1 | F1 | — | — |
| Save | — | ⏳ | — | — | F2 | F2 | — | — |
| Save as | — | ⏳ | — | — | Shift+F2<br>Ctrl+F2 | Shift+F2 | — | — |
| New file | — | ⏳ | — | — | Ctrl+n | Shift+F4 | — | — |
| Switch to viewer | — | ⏳ | — | — | — | F6 | — | — |
| Search | — | ⏳ | — | — | F7 | F7 | — | — |
| Replace | — | ⏳ | — | — | F4 | Ctrl+F7 | — | — |
| Continue search/replace forward / backward | — | ⏳ | — | — | Shift+F7 (search, forward)<br>Shift+F4 (replace, forward) | Shift+F7 / Alt+F7 | — | — |
| Code page | — | ⏳ | — | — | Alt+e | F8 (OEM/ANSI)<br>Shift+F8 (select) | — | — |
| Go to line and position | — | ⏳ | — | — | Alt+l | Alt+F8 | — | — |
| Far window size | — | ⏳ | — | — | — | Alt+F9 | — | — |
| Editor settings | — | ⏳ | — | — | — | Alt+Shift+F9 | — | — |
| Quit | — | ⏳ | — | — | F10<br>Esc | F10<br>F4<br>Esc | — | — |
| Save and quit | — | ⏳ | — | — | — | Shift+F10 | — | — |
| Locate file in panel | — | ⏳ | — | — | — | Ctrl+F10 | — | — |
| Plugin commands | — | ⏳ | — | — | — | F11 | — | — |
| View and edit history | — | ⏳ | — | — | Alt+E | Alt+F11 | — | — |
| Undo | — | ⏳ | — | — | Ctrl+u | Alt+Backspace<br>Ctrl+z | — | — |
| Redo | — | ⏳ | — | — | Alt+r | Ctrl+Z | — | — |
| Lock editing | — | ⏳ | — | — | — | Ctrl+l | — | — |
| User screen | — | ⏳ | — | — | Ctrl+o | Ctrl+o<br>Ctrl+Alt+Shift (temporarily) | — | — |
| Treat next key as character code | — | ⏳ | — | — | Ctrl+q | Ctrl+q | — | — |
| Set bookmark | — | ⏳ | — | — | Alt+k (on the line, or off) | RightCtrl+0…9<br>Ctrl+Shift+0…9 | — | — |
| Go to bookmark | — | ⏳ | — | — | Alt+j (next)<br>Alt+i (previous) | LeftCtrl+0…9 | — | — |
| Insert current panel file name | — | ⏳ | — | — | — | Shift+Enter | — | — |
| Insert passive panel file name | — | ⏳ | — | — | — | Ctrl+Shift+Enter | — | — |
| Insert full name of edited file | — | ⏳ | — | — | — | Ctrl+f | — | — |
| Functional key bar | — | ⏳ | — | — | — | Ctrl+b | — | — |
| Status line | — | ⏳ | — | — | — | Ctrl+B | — | — |
| Record macro | — | ⏳ | — | — | Ctrl+r (start/stop) | Ctrl+.<br>Ctrl+Shift+. (special mode) | — | — |
| Run macro | — | ⏳ | — | — | The key assigned to it | — | — | — |

## Directory tree (mc, Far)

mc's directory tree, from the Command menu, and Far's tree panel, Ctrl+t in the panels.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Rescan directory (Far: the whole tree) | — | ⏳ | — | — | Ctrl+r<br>F2 | Ctrl+r | — | — |
| Forget directory from tree | — | ⏳ | — | — | F3 | — | — | — |
| Static/dynamic navigation | — | ⏳ | — | — | F4 | — | — | — |
| Copy / move directory | — | ⏳ | — | — | F5 / F6 | — | — | — |
| Delete directory | — | ⏳ | — | — | F8<br>Delete | — | — | — |
| Search next match | — | ⏳ | — | — | Ctrl+s<br>Alt+s | Ctrl+Enter | — | — |
| The next / previous branch on the same level | — | ⏳ | — | — | — | Numpad+ / Numpad- | — | — |
| Delete last search character | — | ⏳ | — | — | Ctrl+h<br>Backspace | — | — | — |
| Help | — | ⏳ | — | — | F1 | — | — | — |
| Exit without changing directory | — | ⏳ | — | — | Esc<br>F10 | — | — | — |

## Diff viewer (mc)

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Help | — | ⏳ | — | — | F1 | — | — | — |
| Save changes | — | ⏳ | — | — | F2 | — | — | — |
| Edit left file | — | ⏳ | — | — | F4 | — | — | — |
| Edit right file | — | ⏳ | — | — | Shift+F4 | — | — | — |
| Merge current hunk | — | ⏳ | — | — | F5 (left to right)<br>Shift+F5 (right to left) | — | — | — |
| Search | — | ⏳ | — | — | F7 | — | — | — |
| Continue search | — | ⏳ | — | — | Shift+F7 | — | — | — |
| Quit | — | ⏳ | — | — | F10<br>Esc<br>q<br>Q | — | — | — |
| Hunk status | — | ⏳ | — | — | Alt+s<br>s | — | — | — |
| Line numbers | — | ⏳ | — | — | Alt+n<br>l | — | — | — |
| Maximize left panel | — | ⏳ | — | — | f | — | — | — |
| Equalize panel widths | — | ⏳ | — | — | = | — | — | — |
| Shrink right / left panel | — | ⏳ | — | — | > / < | — | — | — |
| Tab size | — | ⏳ | — | — | 2, 3, 4, 8 | — | — | — |
| Swap panels | — | ⏳ | — | — | Ctrl+u | — | — | — |
| Refresh screen | — | ⏳ | — | — | Ctrl+r | — | — | — |
| Show command screen | — | ⏳ | — | — | Ctrl+o | — | — | — |
| Next hunk | — | ⏳ | — | — | Enter<br>Space<br>n | — | — | — |
| Previous hunk | — | ⏳ | — | — | Backspace<br>p | — | — | — |
| Go to line | — | ⏳ | — | — | g<br>G | — | — | — |
| Line down / up | — | ⏳ | — | — | Down / Up | — | — | — |
| Page up / down | — | ⏳ | — | — | PgUp / PgDn | — | — | — |
| Beginning of line | — | ⏳ | — | — | Home | — | — | — |
| Beginning of file | — | ⏳ | — | — | Ctrl+Home | — | — | — |
| End of file | — | ⏳ | — | — | Ctrl+End | — | — | — |

## Sort modes menu (Far)

Ctrl+F12 in the panels.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Ascending / descending / invert | — | ⏳ | — | — | — | + / - / * | — | — |
| Criteria: ascending / descending / invert | — | ⏳ | — | — | — | + / - / * | — | — |
| Additional criteria | — | ⏳ | — | — | — | F4 | — | — |
| Criteria: add / remove / replace | — | ⏳ | — | — | — | Insert / Delete / F4 | — | — |
| Criteria: inherit order from sort mode | — | ⏳ | — | — | — | = | — | — |
| Criteria: move up / down | — | ⏳ | — | — | — | Ctrl+Up / Ctrl+Down | — | — |
| Criteria: reset | — | ⏳ | — | — | — | Ctrl+r | — | — |

## Screen grabber (Far)

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Start | — | ⏳ | — | — | — | Alt+Insert | — | — |
| Stream/block mode | — | ⏳ | — | — | — | Space | — | — |
| Move the cursor | — | ⏳ | — | — | — | Cursor keys<br>Click | — | — |
| Selection | — | ⏳ | — | — | — | Shift+cursor keys<br>Drag | — | — |
| Resize selected area | — | ⏳ | — | — | — | Alt+Shift+cursor keys | — | — |
| Move selected area | — | ⏳ | — | — | — | Alt+cursor keys | — | — |
| Copy to clipboard | — | ⏳ | — | — | — | Enter<br>Ctrl+Insert<br>Right click<br>Double click | — | — |
| Append to clipboard | — | ⏳ | — | — | — | Ctrl+Numpad+ | — | — |
| Cancel | — | ⏳ | — | — | — | Esc | — | — |
| Select whole screen | — | ⏳ | — | — | — | Ctrl+a | — | — |
| Clear selection | — | ⏳ | — | — | — | Ctrl+u | — | — |
| Selection horizontally by 10 | — | ⏳ | — | — | — | Ctrl+Shift+Left / Ctrl+Shift+Right | — | — |
| Selection vertically by 5 | — | ⏳ | — | — | — | Ctrl+Shift+Up / Ctrl+Shift+Down | — | — |

## Task list (Far)

Ctrl+w in the panels.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Kill task | — | ⏳ | — | — | — | Delete | — | — |
| Refresh list | — | ⏳ | — | — | — | Ctrl+r | — | — |
| Window title / module path | — | ⏳ | — | — | — | F2 | — | — |

## Spot (yazi)

Tab in the panels: what yazi knows of the file under the cursor, in a table.

| Description | Action | Status | noc (default) | noc (vim) | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- | --- |
| Close the spot | — | ⏳ | — | — | — | — | Esc<br>Ctrl+[<br>Ctrl+c<br>Tab | — |
| One line up | — | ⏳ | — | — | — | — | k<br>Up | — |
| One line down | — | ⏳ | — | — | — | — | j<br>Down | — |
| The previous file | — | ⏳ | — | — | — | — | h<br>Left | — |
| The next file | — | ⏳ | — | — | — | — | l<br>Right | — |
| Copy the cell under the cursor | — | ⏳ | — | — | — | — | c c | — |
