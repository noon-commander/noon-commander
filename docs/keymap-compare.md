# Keyboard shortcuts: Noon Commander vs Midnight Commander vs Far Manager vs yazi vs ranger

Sources:

- noc: the keymap in [keymap/preset.rs](../crates/noc/src/tui/keymap/preset.rs): vim's, in its
  modes, with mc's F-keys ([ADR 0022](adr/0022-vim-modes.md)). It is the only one: no
  `ui.keymap` chooses another, and no `noc keymap diff` compares two.
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

The sections follow noc's keymap: one per context and mode, in the order of `preset.rs` and under
the context's name in snake case, then the mouse, then what only mc, Far, or yazi has. A section
lists the actions that its context binds itself in its mode; keys it does not bind go to the
contexts it falls back to in that mode, as its section says. In each table:

- Description: what the keys do; in noc, where noc does it.
- Action: noc's action, as [keymap/action.rs](../crates/noc/src/tui/keymap/action.rs) names it,
  in snake case; "—" where the section's context binds no action for it in its mode. noc's
  actions come first, in the order of the keymap, then what mc, Far, yazi, or ranger does there
  and noc does not.
- noc, mc, far, yazi, ranger: the keys. "—" — not bound, or not documented in the source.

Keys are typed in one of vim's modes. Normal mode takes them as commands: the panels, the lists,
the pull-down menu, the viewer, and the help start in it, and keys that nothing binds there do
nothing. Insert mode takes characters as text: quick search, the command line of `:` and `!`, the
field of a rename, and a dialog whose first focus is a text field start in it. Visual mode
selects rows of a panel, or text of a field, for the commands that act on them. Alt and a key
stand for Esc and the key in Insert and Visual mode, and in the panels in Normal mode, unless
something binds that Alt key, as a quick Esc and the key arrive from a terminal; on a password
field they do nothing. Ctrl+c closes or cancels in every mode. Counts, registers, and the
character typed after f, t, r, ", Ctrl+r, or Ctrl+v come with the action rather than as
bindings; the sections say where they apply. mc, Far, and ranger have no modes; yazi has vi's in
its input boxes, and a visual mode in its panels.

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
  the kitty keyboard protocol. The noc column lists the keys of such a terminal. In the panels,
  Tab and Ctrl+i do different things there: Tab goes to the other panel, as in mc, and Ctrl+i
  forward through the jump list, as in vim. Without the protocol Tab is Ctrl+i, so Tab does what
  Ctrl+i does, and nothing where Ctrl+i does nothing.

Status, of noc as a whole:

- ✅ settled: the keymap binds the action, with the keys it will keep.
- ⏳ not settled yet: noc does not have it yet, or its keys may still change. Since the move to
  vim's modes (ADR 0022), ⏳ also marks the rows whose noc keys changed, and the actions that
  came with the modes, until they are reviewed.
- 🚫 not needed: noc will not have it.

## Everywhere (`global`, every mode)

Keys that do the same wherever they are pressed, in every mode: in the panels, the dialogs, the
menus, the windows, the viewer, and on the command line. Every other context falls back to this one
last, after its own fallbacks, so a context that binds the same key itself takes it first. The
output of commands is the exception: noc draws nothing there. In Insert mode characters are text, so
g < and " work only where keys are commands; Ctrl+k and Ctrl+l work in every mode, in every
terminal. " names a register for the next command, as in vim, wherever keys are commands, so that
the name never runs as a command; only text fields keep text in registers. Ctrl+k after a count or a
register shows the keys they can go with, and keeps them for the key after it; Backspace with
nothing typed closes the hints. Ctrl+k is free in all of noc's modes and is no prefix of tmux or
screen; vim's digraphs of Ctrl+k in Insert mode noc does not have. yazi and ranger show the keys
that can follow on their own, once the first key of a sequence is pressed; yazi binds its help in
every window, and F1 in mc and Far opens the help of the window in front. g <, vim's key for the
output of the last command, shows the output of commands over whatever is in front, which is there
again afterwards; mc and Far hide or show the panels with Ctrl+o in the panels and on their command
line, and mc in its viewer and editor too. noc's Ctrl+o is vim's, which goes back through the jump
list. Ctrl+l redraws the screen under whatever is in front, which stays as it was, quick search too,
as mc's Ctrl+l does in its panels, dialogs, viewer, and editor.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| The keys that can be typed now, over whatever is in front, and what they do, as which-key shows them | `key_hints` | ⏳ | Ctrl+k | — | — | — | — |
| The register for the next command: a … z, which text fields share, and A … Z to append to it | `register` | ⏳ | " | — | — | — | — |
| The output of commands, in place of the panels and what is in front of them (mc, Far: hide or show the panels) | `user_screen` | ⏳ | g < | Ctrl+o | Ctrl+o | — | — |
| Redraw the screen | `redraw` | ✅ | Ctrl+l | Ctrl+l | — | — | Ctrl+l |
| Help: the keys of what is in front, and what they do | — | ⏳ | — | F1 | F1 | \~<br>F1 | — |

## Panels (`panel`, Normal mode)

A panel that lists a directory, in Normal mode, where it starts. It falls back to `global`; keys
that neither binds do nothing. Alt and a key, any key but Esc, stand for Esc and the key here,
unless something binds that Alt key, as a quick Esc and the key arrive from a terminal. mc and Far
have a command line under the panels that takes what is typed; noc opens one with `!` and `:`, yazi
asks for a shell command in an input box, and ranger opens its console with `:`. mc's +, -, \\, and
\*, on the numpad too, mark and unmark only while its command line is empty; with Alt, they always
do. noc keeps them under g, as g +, g -, and g \*, and binds neither +, -, and \*, which are
vim's motions and search. mc's Shift+Up and Shift+Down mark, as in Far, the one exception to vim's
keys, so that `K` can show what is under the cursor, as vim's does: the whole name, below the
panels. As vim scrolls lines that do not wrap, the z keys scroll the names: `«` marks a name that
goes on to the left, and `»` one cut short.

noc groups keys under a first key: vim's g, q, and Ctrl+w, and its own t for the tabs and the
workspaces, o for the sort order, as ranger's sort keys, and the hidden names, and f for the file
operations. f stands for mc's Ctrl+x, whose letters it keeps for mc's commands: c chmod, o chown, e
chattr, l, s, and v links, j the jobs. mc has no checksums, so they take #. Ctrl+w switches the
panels, as it switches vim's windows: Ctrl+w h and Ctrl+w l go to the left and the right one, and
Ctrl+w 1 and Ctrl+w 2 open their location menus, as Alt+F1 and Alt+F2 do. / searches down and ? up,
as in vim and yazi; n and N repeat the last quick search, the way it went and the other way, as in
vim, yazi, and ranger; Shift+F7, mc's and Far's key to search again, repeats it forward too. Of mc's
keys, only the F-keys stay; they work in Visual mode and in quick search too, as their sections say.

A count goes before a motion, a mark, n, or N, and repeats it: 3j goes three rows down. Before G or
g g, it goes to the row of that number, 5G to the fifth; before g t, to the tab of that number, 2g t
to the second. Each tab keeps a jump list, as vim's windows do: changing the directory, G, g g, n,
N, and a quick search that finds a match put where it was in the list, even when they stay on the
row, as in vim, but a directory that cannot be read does not. Ctrl+o goes back through it, and
Ctrl+i forward again; without the kitty keyboard protocol, Tab too, which such terminals send as
Ctrl+i, and which goes to the other panel where they tell them apart. A count goes that many jumps,
and one past the end of the list goes nowhere, as in vim. The rows scroll as vim's window does, with
five rows kept above and below the cursor, as vim's defaults.vim sets 'scrolloff': Ctrl+d and Ctrl+u
scroll half a page and move the cursor as far, and Ctrl+f and Ctrl+b scroll a page less two rows,
with a count that many pages at once, and put the cursor at its top or bottom, where mc's and Far's
page keys move the cursor a page. Unlike vim's, Ctrl+f stops with the last row at the bottom and
then goes to it, as mc's PgDn. n and N say when they go round past the last row or the first, or
find nothing, and n with no search before says so, on the line of the mode until the next key, as
vim's messages. v and V start Visual mode, which selects rows from there to the cursor, as the next
section says: the commands of files act on the rows it selects, which become the marks.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| One row up | `up` | ✅ | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p | — | k<br>Up | Up<br>k |
| One row down | `down` | ⏳ | j<br>Down<br>Ctrl+n<br>Ctrl+j | Down<br>Ctrl+n | — | j<br>Down | Down<br>j |
| One page up | `page_up` | ✅ | Ctrl+b<br>PgUp | PgUp<br>Alt+v | — | Ctrl+b<br>PgUp | PgUp<br>Ctrl+b |
| One page down | `page_down` | ✅ | Ctrl+f<br>PgDn | PgDn<br>Ctrl+v | — | Ctrl+f<br>PgDn | PgDn<br>Ctrl+f |
| Half a page up | `half_page_up` | ✅ | Ctrl+u | — | — | Ctrl+u<br>Shift+PgUp | K<br>Ctrl+u |
| Half a page down | `half_page_down` | ✅ | Ctrl+d | — | — | Ctrl+d<br>Shift+PgDn | J<br>Ctrl+d |
| First row; after a count, the row of that number | `home` | ✅ | g g<br>Home | Home<br>A1<br>Alt+< | — | g g<br>Home | Home<br>g g |
| Last row; after a count, the row of that number | `end` | ✅ | G<br>End | End<br>C1<br>Alt+> | — | G<br>End | End<br>G |
| Open the directory under the cursor; nothing on a file, which the others open | `enter` | ✅ | l<br>Right<br>Enter<br>Ctrl+m | Enter<br>Ctrl+PgDn | Enter<br>Ctrl+PgDn | l<br>Right<br>o<br>Enter | Right<br>Enter<br>l |
| Parent directory; nothing in / | `parent` | ✅ | h<br>Left<br>Backspace<br>Ctrl+h | Ctrl+PgUp | Ctrl+PgUp | h<br>Left | Left<br>h |
| Back through the jump list, as vim's Ctrl+o: to the directory before, or to the row before G, g g, n, N, or a search (mc, yazi, ranger: the directory before in the history) | `jump_back` | ⏳ | Ctrl+o | Alt+y | — | H | H |
| Forward again through the jump list, as vim's Ctrl+i (mc, yazi, ranger: the next directory in the history) | `jump_forward` | ⏳ | Ctrl+i | Alt+u | — | L | L |
| Read the directory again | `reload` | ✅ | R | Ctrl+r | Ctrl+r | — | R |
| Stop loading or connecting | `cancel` | ✅ | Esc<br>Ctrl+[<br>Ctrl+c | — | — | — | Ctrl+c |
| The pull-down menu | `pull_down` | ✅ | g m<br>F9 | F9 | F9 | — | — |
| The pull-down menu, on the command that ran from it last | `pull_down_last` | ✅ | g M<br>Shift+F9 | Shift+F9 | Shift+F10 | — | — |
| The other panel | `switch_panel` | ⏳ | Ctrl+w w<br>Ctrl+w Ctrl+w<br>Ctrl+w p<br>Tab | Tab<br>Ctrl+i | Tab | — | — |
| The left panel, wherever the panels were swapped | `focus_left` | ⏳ | Ctrl+w h | — | — | — | — |
| The right panel, wherever the panels were swapped | `focus_right` | ⏳ | Ctrl+w l | — | — | — | — |
| Swap the panels | `swap_panels` | ✅ | Ctrl+w x<br>Ctrl+w r | Ctrl+u | Ctrl+u | — | — |
| Open the directory under the cursor in the other panel | `other_panel_open` | ✅ | Ctrl+w f | Alt+o | — | — | — |
| Show this directory in the other panel | `other_panel_sync` | ✅ | Ctrl+w v | Alt+i | — | — | — |
| Change the location of the panel drawn on the left, after a swap too | `location_menu_left` | ⏳ | Ctrl+w 1<br>Alt+F1 | — | Alt+F1 | — | — |
| Change the location of the panel drawn on the right | `location_menu_right` | ⏳ | Ctrl+w 2<br>Alt+F2 | — | Alt+F2 | — | — |
| A new tab in this panel, on the same directory | `new_tab` | ✅ | t n<br>g n | — | — | t t | Ctrl+n<br>g n |
| Close this tab; the last one stays | `close_tab` | ✅ | t c<br>g c | — | — | — | Ctrl+w<br>g c |
| The next tab in this panel; after a count, the tab of that number | `next_tab` | ⏳ | g t<br>] t | — | — | ] | Tab<br>Alt+Right<br>g t |
| The previous tab in this panel | `prev_tab` | ⏳ | g T<br>[ t | — | — | [ | Shift+Tab<br>Alt+Left<br>g T |
| The tabs of the left panel, to choose one | `tab_list_left` | ✅ | t h | — | — | — | — |
| The tabs of the right panel, to choose one | `tab_list_right` | ✅ | t l | — | — | — | — |
| The saved workspaces: restore, rename, or delete one | `workspaces` | ✅ | t w | — | — | — | — |
| Save the tabs of both panels as a workspace | `save_workspace` | ✅ | t W | — | — | — | — |
| Show or hide names that start with a dot | `toggle_hidden` | ✅ | o h | Alt+. | Ctrl+h | . | z h<br>Ctrl+h<br>Backspace |
| Sort by name; again: reverse | `sort_by_name` | ✅ | o n | — | Ctrl+F3 | , a<br>, A (reverse) | o b<br>o B (reverse) |
| Sort by extension; again: reverse | `sort_by_extension` | ✅ | o e | — | Ctrl+F4 | , e<br>, E (reverse) | o e<br>o E (reverse) |
| Sort by modification time; again: reverse | `sort_by_time` | ✅ | o m | — | Ctrl+F5 | , m<br>, M (reverse) | o m<br>o M (reverse) |
| Sort by size; again: reverse | `sort_by_size` | ✅ | o s | — | Ctrl+F6 | , s<br>, S (reverse) | o s<br>o S (reverse) |
| Visual mode: select rows from here to the cursor, for the commands of files (yazi, ranger: mark the rows the cursor moves over) | `visual_mode` | ⏳ | v | — | — | v | — |
| Visual mode by lines, as vim's V: the same rows, the mode line says VISUAL LINE | `visual_line_mode` | ⏳ | V | — | — | — | V |
| Select again the rows selected last | `reselect` | ⏳ | g v | — | — | — | — |
| Mark or unmark, then the next row | `mark` | ⏳ | Space<br>Shift+Down | Insert<br>Ctrl+t<br>Shift+Down | Insert<br>Shift+Down | Space | Space |
| Mark or unmark, then the row above | `mark_up` | ⏳ | Shift+Up | Shift+Up | Shift+Up | — | — |
| The names a column left, as vim's zh; with a count, that many | `scroll_left` | ⏳ | z h<br>z Left | — | Alt+Left | — | — |
| The names a column right, as vim's zl, until the end of the longest name on screen is at the right edge; with a count, that many | `scroll_right` | ⏳ | z l<br>z Right | — | Alt+Right | — | — |
| The names half their column left, as vim's zH | `scroll_half_left` | ⏳ | z H | — | — | — | — |
| The names half their column right, as vim's zL | `scroll_half_right` | ⏳ | z L | — | — | — | — |
| The names from their start, as vim's zs | `scroll_start` | ⏳ | z s | — | Alt+Home | — | — |
| The end of the name under the cursor at the right edge, as vim's ze | `scroll_end` | ⏳ | z e | — | Alt+End | — | — |
| Show the whole name under the cursor in a frame below the panels, until asked again or Esc (mc and Far: the mini status line, cut too) | `show_full` | ⏳ | K | — | — | — | — |
| Mark the names that match a pattern | `select` | ⏳ | g + | +<br>Numpad+<br>Alt++ | Numpad+ | — | — |
| Unmark the names that match a pattern | `unselect` | ⏳ | g - | -<br>\\<br>Numpad-<br>Alt+- | Numpad- | — | — |
| Invert the marks on files; directories stay as they are | `invert_marks` | ⏳ | g \* | \*<br>Numpad\*<br>Alt+\* | Numpad* | — | — |
| Start quick search | `quick_search` | ✅ | / | Ctrl+s<br>Alt+s | Alt+characters<br>Alt+Shift+characters | / | f |
| Start quick search up the rows, as vim's ?; n then goes up too, and N down | `quick_search_back` | ⏳ | ? | — | — | ? | — |
| The next name that matches the last quick search | `next_match` | ✅ | n<br>Shift+F7 | — | — | n | n |
| The previous name that matches the last quick search | `prev_match` | ✅ | N | — | — | N | N |
| Open the command line for a shell command | `shell` | ✅ | ! | Typing | Typing | ; (in the background)<br>: | !<br>s |
| Open the command line for a command of noc | `command` | ✅ | : | — | — | — | :<br>; |
| The command history | `command_history` | ✅ | q : | Alt+h | Alt+F8 | — | — |
| Jump to a directory that zoxide ranks | `jump` | ✅ | g z | — | — | Z | — |
| Quick cd: type a path as for cd | `quick_cd` | ✅ | g d | Alt+c | — | g Space | c d |
| View the file under the cursor | `view` | ✅ | i<br>F3 | F3 | F3<br>Numpad5<br>Ctrl+Shift+F3 (always internal) | — | i<br>F3 |
| Edit the file under the cursor in $VISUAL or $EDITOR | `edit` | ✅ | e<br>F4 | F4 | F4<br>Ctrl+Shift+F4 (always internal) | — | F4<br>E |
| Copy the marked entries, or the one under the cursor | `copy` | ✅ | Y<br>y y<br>F5 | F5 | F5 | y, then p | y y, then p p<br>F5, then p p |
| Move or rename the marked entries, or the one under the cursor | `move` | ✅ | x<br>F6 | F6 | F6 | x, then p | d d, then p p<br>F6, then p p |
| Rename the entry under the cursor in its row | `rename` | ✅ | r<br>Shift+F6 | Shift+F6 | Shift+F6 | r | F2 (before the extension)<br>c w (a new name)<br>a (before the extension)<br>A (at the end)<br>I (at the start) |
| Make a directory | `mkdir` | ✅ | A<br>F7 | F7 | F7 | a (a name that ends in /) | F7 |
| Delete the marked entries, or the one under the cursor | `delete` | ✅ | d D<br>F8 | F8 | F8 | D | F8<br>d D |
| Checksums of the marked files, or the one under the cursor | `checksum` | ✅ | f # | — | — | — | — |
| The running jobs: bring one to the front, or abort it | `jobs` | ✅ | f j | Ctrl+x j | — | w | w |
| Help | `help` | ✅ | g ?<br>F1 | F1 | F1 | \~<br>F1 | ?<br>F1 |
| Quit | `quit` | ✅ | Z Z<br>F10 | F10 | F10 | q | Q<br>F10 |
| F1…F10 on terminals without function keys | — | 🚫 | — | Esc, then 1…9, 0 | — | — | — |
| Alt+key on terminals without Alt | — | 🚫 | — | Esc, then the key | — | — | — |
| Enter the archive under the cursor as a directory | — | ⏳ | — | Ctrl+PgDn | Ctrl+PgDn<br>Ctrl+Shift+PgDn (always as archive) | — | — |
| Execute in separate window | — | ⏳ | — | — | Shift+Enter | — | — |
| Run as administrator | — | ⏳ | — | — | Ctrl+Alt+Enter | — | — |
| Open the marked files, or the one under the cursor, with a program chosen from a list | — | ⏳ | — | — | — | O<br>Shift+Enter | r |
| User menu | — | ⏳ | — | F2 | F2 | — | — |
| View without preprocessing | — | ⏳ | — | Shift+F3 | — | — | — |
| Alternative (external/internal) viewer | — | ⏳ | — | — | Alt+F3 | — | — |
| Filtered view (command output) | — | ⏳ | — | Alt+! | — | — | # |
| Alternative (external/internal) editor | — | ⏳ | — | — | Alt+F4 | — | — |
| Edit new file | — | ⏳ | — | Shift+F4 | Shift+F4 | — | — |
| Make an empty file | — | ⏳ | — | — | — | a | Insert |
| Make several files at once | — | ⏳ | — | — | — | A | — |
| Copy current file (ignoring selection) | — | ⏳ | — | Shift+F5 | Shift+F5 | — | — |
| Paste, replacing the files of the same names | — | ⏳ | — | — | — | P | p o |
| Paste after the jobs already queued, not before them | — | ⏳ | — | — | — | — | p P<br>p O (replacing) |
| Paste to a directory typed on the console | — | ⏳ | — | — | — | — | p d |
| Paste to a bookmarked directory | — | ⏳ | — | — | — | — | p \`, then the key<br>p ', then the key |
| Forget the entries yanked for copying or moving | — | ⏳ | — | — | — | Y<br>X | u y<br>u d |
| Add the marked entries to those yanked for copying / take them out / switch them | — | ⏳ | — | — | — | — | y a / y r / y t |
| Add the marked entries to those yanked for moving / take them out / switch them | — | ⏳ | — | — | — | — | d a / d r / d t |
| Yank for copying the rows from the cursor to the first or the last, or to the row below or above | — | ⏳ | — | — | — | — | y g g<br>y G<br>y j<br>y k |
| Yank for moving the rows from the cursor to the first or the last, or to the row below or above | — | ⏳ | — | — | — | — | d g g<br>d G<br>d j<br>d k |
| Delete only the file under cursor | — | ⏳ | — | Shift+F8 | Shift+F8 | — | — |
| Delete bypassing Recycle Bin | — | ⏳ | — | — | Shift+Delete | — | — |
| Wipe | — | ⏳ | — | — | Alt+Delete | — | — |
| Move the marked entries, or the one under the cursor, to the trash | — | ⏳ | — | — | F8 (with Delete to Recycle Bin on) | d | d T |
| Quit without changing to last directory (shell wrapper) | — | ⏳ | — | Shift+F10 | — | Q | — |
| Close this tab; the last one: quit | — | ⏳ | — | — | — | Ctrl+c | q<br>Z Z<br>Z Q |
| Plugin commands | — | ⏳ | — | — | F11 | — | — |
| Plugin configuration | — | ⏳ | — | — | Alt+Shift+F9 | — | — |
| Save setup (noc: the Configuration dialog saves each change) | — | 🚫 | — | — | Shift+F9 | — | — |
| Print files | — | ⏳ | — | — | Alt+F5 | — | — |
| Create link | — | ⏳ | — | Ctrl+x l (hard)<br>Ctrl+x s (absolute symlink)<br>Ctrl+x v (relative symlink) | Alt+F6 | - (absolute symlink)<br>_ (relative symlink)<br>Ctrl+- (hard) | p l (absolute symlink)<br>p L (relative symlink)<br>p h l (hard)<br>p h t (hard, in a copy of the tree) |
| Find file | — | ⏳ | — | Alt+? | Alt+F7 | s | — |
| Find files by their content | — | ⏳ | — | — | — | S | — |
| Stop finding files | — | ⏳ | — | — | — | Ctrl+s | — |
| Jump to a file or directory found with fzf | — | ⏳ | — | — | — | z | — |
| Find folder | — | ⏳ | — | — | Alt+F10 | — | — |
| File permissions/attributes | — | ⏳ | — | Ctrl+x c (chmod) | Ctrl+a<br>F4 (on a directory) | — | = |
| Add or remove a permission (chmod) | — | ⏳ | — | — | — | — | + or -, then u, g, o, a, or none, then r, w, x, X, s, or t |
| Change owner (chown) | — | ⏳ | — | Ctrl+x o | — | — | — |
| File system attributes (chattr) | — | ⏳ | — | Ctrl+x e | — | — | — |
| Apply command to selected files | — | ⏳ | — | — | Ctrl+g | — | @ |
| Describe selected files | — | ⏳ | — | — | Ctrl+z | — | — |
| Add files to archive | — | ⏳ | — | — | Shift+F1 | — | — |
| Extract files from archive | — | ⏳ | — | — | Shift+F2 | — | — |
| Archive commands | — | ⏳ | — | — | Shift+F3 | — | — |
| Temporarily show user screen (while held) | — | ⏳ | — | — | Ctrl+Alt+Shift | — | — |
| A shell in the terminal; leaving it comes back | — | ⏳ | — | — | — | — | S |
| Suspend, back to the shell that started it | — | ⏳ | — | Ctrl+z | — | Ctrl+z | — |
| External panelize | — | ⏳ | — | Ctrl+x ! | — | — | — |
| Add current directory to hotlist / folder shortcut | — | ⏳ | — | Ctrl+x h | Ctrl+Shift+0…9 | — | m, then a key |
| Go to directory from hotlist / folder shortcut | — | ⏳ | — | Ctrl+\ (list) | RightCtrl+0…9 | — | \`, then the key<br>', then the key |
| Remove a bookmark | — | ⏳ | — | — | — | — | u m, then the key |
| Change panel charset | — | ⏳ | — | Alt+e | — | — | — |
| Toggle panel split (vertical/horizontal) | — | ⏳ | — | Alt+, | — | — | — |
| Miller columns, or the tabs side by side | — | ⏳ | — | — | — | — | \~ |
| Change window size | — | ⏳ | — | — | Alt+F9 | — | — |
| Task list | — | ⏳ | — | — | Ctrl+w | — | — |
| Screen grabber | — | ⏳ | — | — | Alt+Insert | — | — |
| Record keyboard macro | — | ⏳ | — | — | Ctrl+.<br>Ctrl+Shift+. (special mode) | — | — |
| Next screen | — | ⏳ | — | Alt+} | Ctrl+Tab | — | — |
| Previous screen | — | ⏳ | — | Alt+{ | Ctrl+Shift+Tab | — | — |
| Screen list | — | ⏳ | — | Alt+\` | F12 | — | — |
| Bring back the tab closed last | — | ⏳ | — | — | — | — | u q |
| Rename this tab | — | ⏳ | — | — | — | t r | — |
| Show tab 1…9 (noc: a count before g t, as 2g t) | — | ⏳ | — | — | — | 1…9 | Alt+1…9 (a new one if there is none) |
| Move this tab left / right | — | ⏳ | — | — | — | { / } | Alt+l / Alt+r |
| Directory history (list) | — | ⏳ | — | Alt+H | Alt+F12 | — | — |
| View and edit history | — | ⏳ | — | Alt+E | Alt+F11 | — | — |
| Info panel | — | ⏳ | — | Ctrl+x i (on other panel) | Ctrl+l | Tab | — |
| Quick view panel | — | ⏳ | — | Ctrl+x q (on other panel) | Ctrl+q | — | z p |
| Scroll the preview up / down | — | ⏳ | — | — | — | K / J | Alt+k / Alt+j |
| Preview directories or not | — | ⏳ | — | — | — | — | z P |
| Shrink the preview when there is nothing to preview, or not | — | ⏳ | — | — | — | — | z c |
| Preview images or not | — | ⏳ | — | — | — | — | z i |
| Preview with the preview script or not | — | ⏳ | — | — | — | — | z v |
| Folder tree | — | ⏳ | — | — | Ctrl+t | — | — |
| Hide/show inactive panel | — | ⏳ | — | — | Ctrl+p | — | — |
| Hide/show left panel | — | ⏳ | — | — | Ctrl+F1 | — | — |
| Hide/show right panel | — | ⏳ | — | — | Ctrl+F2 | — | — |
| Change panels height | — | ⏳ | — | — | Ctrl+Up<br>Ctrl+Down | — | — |
| Change current panel height | — | ⏳ | — | — | Ctrl+Shift+Up<br>Ctrl+Shift+Down | — | — |
| Change panels width (with empty command line) | — | ⏳ | — | Alt+Shift+Left<br>Alt+Shift+Right | Ctrl+Left<br>Ctrl+Right | — | — |
| Restore default panels width | — | ⏳ | — | Alt+= | Ctrl+Numpad5 | — | — |
| Restore default panels height | — | ⏳ | — | — | Ctrl+Alt+Numpad5 | — | — |
| Hide/show functional key bar | — | ⏳ | — | — | Ctrl+b | — | — |
| The mouse on or off | — | ⏳ | — | — | — | — | z m |
| Flush the input after each key or not | — | ⏳ | — | — | — | — | z I |
| Sizes in bytes / with K/M/G/T suffixes | — | ⏳ | — | — | Ctrl+S | — | — |
| Sizes of the marked directories, or the one under the cursor | — | ⏳ | — | Ctrl+Space | F3 (on a directory) | — | d c |
| Keep the sizes of directories up to date or not | — | ⏳ | — | — | — | — | z u |
| Disk usage of the directories here, in the pager | — | ⏳ | — | — | — | — | d u<br>d U (largest first) |
| Next listing format | — | ⏳ | — | Alt+t | — | — | — |
| Line mode: what the rows show beside the names | — | ⏳ | — | — | — | m s (size)<br>m p (permissions)<br>m b (btime)<br>m m (mtime)<br>m o (owner)<br>m n (none) | M f (filename)<br>M i (fileinfo)<br>M m (mtime)<br>M h (humanreadablemtime)<br>M p (permissions)<br>M s (sizemtime)<br>M H (sizehumanreadablemtime)<br>M t (metatitle) |
| Brief view mode | — | ⏳ | — | — | LeftCtrl+1 | — | — |
| Medium view mode | — | ⏳ | — | — | LeftCtrl+2 | — | — |
| Full view mode | — | ⏳ | — | — | LeftCtrl+3 | — | — |
| Wide view mode | — | ⏳ | — | — | LeftCtrl+4 | — | — |
| Detailed view mode | — | ⏳ | — | — | LeftCtrl+5 | — | — |
| Long descriptions view mode | — | ⏳ | — | — | LeftCtrl+7 | — | — |
| File owners view mode | — | ⏳ | — | — | LeftCtrl+8 | — | — |
| File links view mode | — | ⏳ | — | — | LeftCtrl+9 | — | — |
| Alternative full view mode | — | ⏳ | — | — | LeftCtrl+0 | — | — |
| Long/short names | — | ⏳ | — | — | Ctrl+n | — | — |
| Scroll long names | — | ⏳ | — | Alt+(<br>Alt+) | Alt+Left<br>Alt+Right<br>Alt+Home<br>Alt+End | — | — |
| Go to root directory | — | ⏳ | — | — | Ctrl+\ | — | g r<br>g / |
| Go to the home directory | — | ⏳ | — | — | — | g h | g h |
| Go to a directory that the keymap names | — | ⏳ | — | — | — | g c (\~/.config)<br>g d (\~/Downloads)<br>g t (the trash) | g e (/etc)<br>g u (/usr)<br>g d (/dev)<br>g o (/opt)<br>g v (/var)<br>g m (/media)<br>g i (/run/media/$USER)<br>g M (/mnt)<br>g s (/srv)<br>g p (/tmp)<br>g R (ranger's own)<br>g ? (/usr/share/doc/ranger) |
| Follow the symlink under the cursor | — | ⏳ | — | — | — | g f | g L |
| This directory, with its symlinks resolved | — | ⏳ | — | — | — | — | g l |
| Top / middle / bottom file on screen | — | ⏳ | — | Alt+g / Alt+r / Alt+j | — | — | — |
| The next / previous directory beside this one | — | ⏳ | — | — | — | — | ] / [ |
| The next / previous entry of the tree, depth first, entering directories | — | ⏳ | — | — | — | — | } / { |
| The next file after a directory, or the next directory after a file | — | ⏳ | — | — | — | — | ) |
| Unsorted | — | ⏳ | — | — | Ctrl+F7 | — | — |
| Sort by creation time | — | ⏳ | — | — | Ctrl+F8 | , b<br>, B (reverse) | — |
| Sort by access time | — | ⏳ | — | — | Ctrl+F9 | — | o a<br>o A (reverse) |
| Sort by description | — | ⏳ | — | — | Ctrl+F10 | — | — |
| Sort by owner | — | ⏳ | — | — | Ctrl+F11 | — | — |
| Sort by name, numbers by their value | — | ⏳ | — | — | — | , n<br>, N (reverse) | o n<br>o N (reverse) |
| Sort by change time (ctime) | — | ⏳ | — | — | — | — | o c<br>o C (reverse) |
| Sort by type | — | ⏳ | — | — | — | — | o t<br>o T (reverse) |
| Sort randomly | — | ⏳ | — | — | — | , r | o z |
| Sort modes menu | — | ⏳ | — | — | Ctrl+F12 | — | — |
| Use sort groups | — | ⏳ | — | — | Shift+F11 | — | — |
| Show selected files first | — | ⏳ | — | — | Shift+F12 | — | — |
| Reverse the sort order | — | ⏳ | — | — | — | — | o r |
| Directories first or not | — | ⏳ | — | — | — | — | z d |
| Sort ignoring case or not | — | ⏳ | — | — | — | — | z s |
| Select files with same extension | — | ⏳ | — | — | Ctrl+Numpad+ | — | — |
| Deselect files with same extension | — | ⏳ | — | — | Ctrl+Numpad- | — | — |
| Invert selection including folders | — | ⏳ | — | — | Ctrl+Numpad* | Ctrl+r | v |
| Select files with same name | — | ⏳ | — | — | Alt+Numpad+ | — | — |
| Deselect files with same name | — | ⏳ | — | — | Alt+Numpad- | — | — |
| Invert selection of files, deselect folders | — | ⏳ | — | — | Alt+Numpad* | — | — |
| Select all files | — | ⏳ | — | — | Shift+Numpad+ | Ctrl+a | — |
| Deselect all files | — | ⏳ | — | — | Shift+Numpad- | Esc<br>Ctrl+[ | u v |
| Restore previous selection | — | ⏳ | — | — | Ctrl+m | — | — |
| Visual mode: unmark the rows the cursor moves over | — | ⏳ | — | — | — | V | u V |
| Tag or untag the marked entries, or the one under the cursor | — | ⏳ | — | — | — | — | t |
| Tag or untag with a character of one's own | — | ⏳ | — | — | — | — | ", then the character |
| Untag the marked entries, or the one under the cursor | — | ⏳ | — | — | — | — | u t |
| Selected names to clipboard | — | ⏳ | — | — | Ctrl+Insert (with empty command line)<br>Ctrl+Shift+Insert | c f | y n |
| Full names to clipboard | — | ⏳ | — | — | Alt+Shift+Insert | c c | y p |
| Paths of their directories to clipboard | — | ⏳ | — | — | — | c d | y d |
| Names without the extension to clipboard | — | ⏳ | — | — | — | c n | y . |
| URLs of the files / of their directories to clipboard | — | ⏳ | — | — | — | c C / c D | — |
| Real (UNC) names to clipboard | — | ⏳ | — | — | Ctrl+Alt+Insert | — | — |
| Copy files to clipboard | — | ⏳ | — | — | Ctrl+C | — | — |
| Cut files to clipboard | — | ⏳ | — | — | Ctrl+X | — | — |
| Start quick search backwards | — | ⏳ | — | — | — | ? | — |
| Search the names for a pattern typed on the console | — | ⏳ | — | — | — | — | / |
| The next tagged file, or the next file by size, type, or time | — | ⏳ | — | — | — | — | c t (tagged)<br>c s (size)<br>c i (mime type)<br>c c (ctime)<br>c m (mtime)<br>c a (atime) |
| Show only the names that match, as the pattern is typed | — | ⏳ | — | — | — | f | z f<br>z z |
| Filter: directories / files / links | — | ⏳ | — | — | — | — | . d / . f / . l |
| Filter: by mime type / name / hash | — | ⏳ | — | — | — | — | . m / . n / . # |
| Filter: duplicates / unique files | — | ⏳ | — | — | — | — | . " / . ' |
| Filters: or / and / not | — | ⏳ | — | — | — | — | . \| / . & / . ! |
| Filters: rotate / clear / decompose / pop / show | — | ⏳ | — | — | — | — | . r / . c / . \* / . p / . . |
| Read the directory again or not (freeze) | — | ⏳ | — | — | — | — | F |
| Forget the directories read and read this one again | — | ⏳ | — | — | — | — | Ctrl+r |
| The log of messages | — | ⏳ | — | — | — | — | W |
| The console, with the command before | — | ⏳ | — | — | — | — | Ctrl+p |

## Panels (`panel`, Visual mode)

Rows selected in a panel, from the row where v or V started Visual mode to the cursor, as vim's
linewise Visual mode selects lines; with a count, 3V selects three rows. After V the line of the
mode says VISUAL LINE, as vim's. It falls back to `global`. The motions extend the selection, after
a count too, and G and g g after a count go to the row of that number. The commands of files, y, x,
d, D, X, F5, F6, F8, and f #, act on the rows selected: they become the marks, the only ones, Visual
mode ends, and the cursor goes to the first row, as after vim's operators; D and X delete them as d
does, as vim's delete the lines selected. Space marks them beside the marks already there, and Esc
and Ctrl+c leave Visual mode, as does the key of v and V that started it, while the other switches
its kind, as in vim; g v, in Normal mode, selects the same rows again. The other F-keys, Alt+F1,
Alt+F2, and the keys of Ctrl+w that go to another panel end Visual mode and do what they do in
Normal mode, and so do Alt and a key, as Esc and the key; other keys that neither binds do nothing.
yazi's and ranger's visual modes mark the rows that the cursor moves over with the keys of their
panels, whose commands then act on the marks, so their columns here have only the keys that leave
it.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| One row up | `up` | ⏳ | k<br>Up<br>Ctrl+p | — | — | — | — |
| One row down | `down` | ⏳ | j<br>Down<br>Ctrl+n<br>Ctrl+j | — | — | — | — |
| One page up | `page_up` | ⏳ | Ctrl+b<br>PgUp | — | — | — | — |
| One page down | `page_down` | ⏳ | Ctrl+f<br>PgDn | — | — | — | — |
| Half a page up | `half_page_up` | ⏳ | Ctrl+u | — | — | — | — |
| Half a page down | `half_page_down` | ⏳ | Ctrl+d | — | — | — | — |
| First row; after a count, the row of that number | `home` | ⏳ | g g<br>Home | — | — | — | — |
| Last row; after a count, the row of that number | `end` | ⏳ | G<br>End | — | — | — | — |
| The other end of the selection | `other_end` | ⏳ | o | — | — | — | — |
| Mark the rows selected, beside the marks already there, and leave Visual mode | `mark` | ⏳ | Space | — | — | — | — |
| Copy the rows selected | `copy` | ⏳ | y<br>F5 | — | — | — | — |
| Move or rename the rows selected | `move` | ⏳ | x<br>F6 | — | — | — | — |
| Delete the rows selected | `delete` | ⏳ | d<br>D<br>X<br>F8 | — | — | — | — |
| Checksums of the files selected | `checksum` | ⏳ | f # | — | — | — | — |
| Leave Visual mode (yazi: or else unmark everything, or leave the search or the filter) | `normal_mode` | ⏳ | Esc<br>Ctrl+[<br>Ctrl+c | — | — | Esc<br>Ctrl+[ | Esc |
| Leave Visual mode started with v; in Visual mode by lines, switch to it, as vim's | `visual_mode` | ⏳ | v | — | — | — | — |
| Leave Visual mode by lines; in Visual mode started with v, switch to it, as vim's | `visual_line_mode` | ⏳ | V | — | — | — | V |
| Leave Visual mode, and show the help | `help` | ⏳ | F1 | — | — | — | — |
| Leave Visual mode, and view the file under the cursor | `view` | ⏳ | F3 | — | — | — | — |
| Leave Visual mode, and edit the file under the cursor | `edit` | ⏳ | F4 | — | — | — | — |
| Leave Visual mode, and rename the entry under the cursor in its row | `rename` | ⏳ | Shift+F6 | — | — | — | — |
| Leave Visual mode, and make a directory | `mkdir` | ⏳ | F7 | — | — | — | — |
| Leave Visual mode, and go to the next name that matches the last quick search | `next_match` | ⏳ | Shift+F7 | — | — | — | — |
| Leave Visual mode, and open the pull-down menu | `pull_down` | ⏳ | F9 | — | — | — | — |
| Leave Visual mode, and open the pull-down menu on the command that ran from it last | `pull_down_last` | ⏳ | Shift+F9 | — | — | — | — |
| Leave Visual mode, and quit | `quit` | ⏳ | F10 | — | — | — | — |
| Leave Visual mode, and change the location of the panel drawn on the left | `location_menu_left` | ⏳ | Alt+F1 | — | — | — | — |
| Leave Visual mode, and change the location of the panel drawn on the right | `location_menu_right` | ⏳ | Alt+F2 | — | — | — | — |
| Leave Visual mode, and go to the other panel | `switch_panel` | ⏳ | Ctrl+w w<br>Ctrl+w Ctrl+w<br>Ctrl+w p<br>Tab | — | — | — | — |
| Leave Visual mode, and go to the left panel, wherever the panels were swapped | `focus_left` | ⏳ | Ctrl+w h | — | — | — | — |
| Leave Visual mode, and go to the right panel, wherever the panels were swapped | `focus_right` | ⏳ | Ctrl+w l | — | — | — | — |

## Editing text (`input`, Insert mode)

What the text fields share while they take text: quick search, the field of a rename, the text
fields of dialogs, the command line, and the filters of the lists fall back to it, and their
sections list only the keys they add. Keys never go to it alone; the help lists its keys with
each of those fields. It falls back to `global`. Characters are text, and Alt and a key stand
for Esc and the key, as a quick Esc and the key arrive from a terminal, except on a password
field, where they do nothing. Ctrl+r takes the register typed after it, a … z or " for the
unnamed one, and types its text, in quick search and the filters too; Ctrl+v takes the key to
type as it is. The registers live in memory, every text field and the command line share them,
and none reads or writes the system clipboard. Ctrl+o, one command of Normal mode, is bound by
the fields that have Normal mode: the field of a rename, the text fields of dialogs, and the
command line.

The mc column has the keys of mc's input lines, its command line among them; the far column,
those of Far's command line; the ranger column, those of its console. None of them has modes:
their keys are here, but where noc does the same only in Normal or Visual mode, whose rows have
them. The yazi column has the keys of its input boxes, which open in insert mode, that work
there: those with a modifier and the named keys, which work in its normal mode too. Its letters,
digits, and symbols, which work only in normal mode, are under Normal mode.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| One character left | `left` | ✅ | Left | Ctrl+b<br>Left<br>Alt+Left | Left<br>Ctrl+s | Left<br>Ctrl+b | Left<br>Ctrl+b |
| One character right | `right` | ✅ | Right | Ctrl+f<br>Right<br>Alt+Right | Right<br>Ctrl+d | Right<br>Ctrl+f | Right<br>Ctrl+f |
| Start of the line | `text_start` | ⏳ | Home | Ctrl+a<br>Home<br>A1<br>Alt+< | Ctrl+Home | Ctrl+a<br>Home | Home<br>Ctrl+a |
| End of the line | `text_end` | ⏳ | End | Ctrl+e<br>End<br>C1<br>Alt+> | Ctrl+End | Ctrl+e<br>End | End<br>Ctrl+e |
| One word left: the start of the word before the cursor | `word_backward` | ⏳ | Shift+Left<br>Ctrl+Left | Alt+b<br>Ctrl+Left | Ctrl+Left | Alt+b<br>Ctrl+Left | Alt+b<br>Alt+Left |
| One word right: the start of the next word, as vim's w | `word_forward` | ⏳ | Shift+Right<br>Ctrl+Right | — | — | — | — |
| Delete the character before the cursor | `backspace` | ✅ | Backspace<br>Ctrl+h | Ctrl+h<br>Backspace | Backspace | Backspace<br>Ctrl+h | Backspace<br>Ctrl+h |
| Delete the character at the cursor | `delete_char` | ✅ | Delete | Ctrl+d<br>Delete | Delete | Delete<br>Ctrl+d | Delete<br>Ctrl+d |
| Delete the word before the cursor, and the spaces after it, as the shell's Ctrl+w | `delete_word` | ⏳ | Ctrl+w | Ctrl+Alt+h<br>Alt+Backspace | Ctrl+Backspace | Ctrl+w<br>Ctrl+Backspace | Ctrl+w |
| Delete to the start of the line | `delete_to_start` | ✅ | Ctrl+u | Ctrl+w (nothing selected) | — | Ctrl+u | Ctrl+u |
| Put the text of the register typed after it | `paste_register` | ⏳ | Ctrl+r | — | — | — | — |
| Type the next key as it is | `insert_literal` | ⏳ | Ctrl+v | — | — | — | — |
| Normal mode, as in vi, where Esc closes, as the field's section says (yazi: in normal mode, cancel) | `normal_mode` | ⏳ | Esc<br>Ctrl+[ | — | — | Esc<br>Ctrl+[ | — |
| One word right: the end of the word after the cursor (noc: e, in Normal mode) | — | ⏳ | — | Alt+f<br>Ctrl+Right | Ctrl+Right | Alt+f<br>Ctrl+Right | Alt+f<br>Alt+Right |
| Delete the word after the cursor, and the spaces before it, as the shell's Alt+d (noc: d e, in Normal mode) | — | ⏳ | — | Alt+d | Ctrl+Delete | Alt+d<br>Ctrl+Delete | Alt+d |
| Select text (noc: v, in Normal mode) | — | ⏳ | — | Shift+Left<br>Shift+Right<br>Ctrl+Shift+Left<br>Ctrl+Shift+Right<br>Shift+Home<br>Shift+End | — | — | — |
| Delete the selection (noc: d, in Visual mode) | — | ⏳ | — | Ctrl+w | — | — | — |
| Copy (noc: y, in Visual mode) | — | ⏳ | — | Alt+w | Ctrl+Insert | — | — |
| Paste (noc: p and P in Normal mode, Ctrl+r and a register in Insert mode) | — | ⏳ | — | Ctrl+y | Shift+Insert | — | Ctrl+y |
| Swap the characters / the words at the cursor | — | ⏳ | — | — | — | — | Ctrl+t / Alt+t |

## Editing text (`input`, Normal mode)

What the text fields share after Esc, in Normal mode: vim's motions, and its commands that change
the text. The same fields fall back to it, and it falls back to `global`; keys that nothing binds do
nothing. Space and Backspace go on to the next line and back to the one before, as vim's 'whichwrap'
has them; h and l stop at the ends of a line, and -, +, and _ go to the line above, the one below,
and this one, at its first character that is not blank. A count goes before a motion or a command:
3w goes three words on, and d3w deletes three words. d, c, and y take the motion typed after them,
and act on the line when doubled, as d d. f, F, t, T, and r take the character typed after them, and
" of `global` the register for the next command: a … z, whose uppercase A … Z append to it. As
vim's, Shift+Right and Shift+Left are w and b, and Ctrl+Right and Ctrl+Left W and B.

The yazi column has the keys of its input boxes that work only in normal mode: its letters,
digits, and symbols, which are text in insert mode. mc, Far, and ranger have no modes; their keys
are under Insert mode, but where noc does the same only here, as D deletes to the end.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| One character left | `left` | ⏳ | h<br>Left | — | — | h | — |
| One character right | `right` | ⏳ | l<br>Right | — | — | l | — |
| One character left; from the start of a line, to the end of the one before | `char_backward` | ⏳ | Backspace<br>Ctrl+h | — | — | — | — |
| One character right; from the end of a line, to the start of the next | `char_forward` | ⏳ | Space | — | — | — | — |
| Start of the line | `text_start` | ⏳ | 0<br>Home | — | — | 0 | — |
| The first character of the line that is not blank | `first_non_blank` | ⏳ | ^ | — | — | _<br>^ | — |
| End of the line | `text_end` | ⏳ | $<br>End | — | — | $ | — |
| The first character of the line above that is not blank; an operator takes the lines, as vim's - | `prev_line` | ⏳ | - | — | — | — | — |
| The first character of the line below that is not blank; an operator takes the lines, as vim's + | `next_line` | ⏳ | + | — | — | — | — |
| The first character of the line that is not blank, or of the line a count less one below; d _ deletes the line as d d | `this_line` | ⏳ | _ | — | — | — | — |
| The start of the next word | `word_forward` | ⏳ | w<br>Shift+Right | — | — | w | — |
| The start of the word before the cursor | `word_backward` | ⏳ | b<br>Shift+Left | — | — | b | — |
| The end of the word | `word_end` | ⏳ | e | — | — | e | — |
| The start of the next word, where only blanks part words | `big_word_forward` | ⏳ | W<br>Ctrl+Right | — | — | W | — |
| The start of the word before, where only blanks part words | `big_word_backward` | ⏳ | B<br>Ctrl+Left | — | — | B | — |
| The end of the word, where only blanks part words | `big_word_end` | ⏳ | E | — | — | E | — |
| The next character typed after it, in the line | `find_char` | ⏳ | f | — | — | — | — |
| Back to the character typed after it | `find_char_back` | ⏳ | F | — | — | — | — |
| Right before the next character typed after it | `till_char` | ⏳ | t | — | — | — | — |
| Back to right after the character typed after it | `till_char_back` | ⏳ | T | — | — | — | — |
| Find again, as the last f, t, F, or T | `repeat_find` | ⏳ | ; | — | — | — | — |
| Find again, the other way | `repeat_find_back` | ⏳ | , | — | — | — | — |
| Insert mode, before the cursor | `insert_mode` | ⏳ | i<br>Insert | — | — | i | — |
| Insert mode, after the cursor | `append` | ⏳ | a | — | — | a | — |
| Insert mode, at the first character that is not blank | `insert_at_start` | ⏳ | I | — | — | I | — |
| Insert mode, at the end of the line | `append_at_end` | ⏳ | A | — | — | A | — |
| Visual mode: select from here to the cursor | `visual_mode` | ⏳ | v | — | — | v | — |
| Delete the character at the cursor | `delete_char` | ⏳ | x<br>Delete | — | — | x | — |
| Delete the character before the cursor | `backspace` | ⏳ | X | — | — | — | — |
| Delete the character at the cursor, then Insert mode | `substitute` | ⏳ | s | — | — | s | — |
| Delete the text of the line, then Insert mode | `substitute_line` | ⏳ | S | — | — | S | — |
| Delete to the end of the line, then Insert mode | `change_to_end` | ⏳ | C | — | — | C | — |
| Delete to the end of the line | `delete_to_end` | ⏳ | D | Ctrl+k | Ctrl+k | Ctrl+k<br>D | Ctrl+k |
| Replace the character at the cursor with the one typed after it | `replace_char` | ⏳ | r | — | — | r | — |
| Switch the case of the character at the cursor, and move on | `toggle_case` | ⏳ | \~ | — | — | — | — |
| Delete what the motion after it moves over, or the line for d d | `delete_operator` | ⏳ | d | — | — | — | — |
| Delete as d does, then Insert mode | `change_operator` | ⏳ | c | — | — | — | — |
| Copy what the motion after it moves over to a register, or the line for y y | `yank_operator` | ⏳ | y | — | — | — | — |
| Put the text of the register after the cursor | `put_after` | ⏳ | p | — | — | p | — |
| Put the text of the register before the cursor | `put_before` | ⏳ | P | — | — | P | — |
| Undo the last change | `undo` | ⏳ | u | — | — | u | — |
| Redo what undo took back | `redo` | ⏳ | Ctrl+r | — | — | Ctrl+r | — |
| Select the whole text | — | ⏳ | — | — | — | V<br>Ctrl+A<br>Ctrl+E | — |

## Editing text (`input`, Visual mode)

Text selected in a field, from where v started Visual mode to the cursor. The same fields fall back
to it, and it falls back to `global`. The motions extend the selection, after a count too, and, as
in vim, the cursor can go past the last character of a line, with l, Space, $, w, or a count before
v, to take the line break too; d, c, y, and \~ act on what it selects, and " before them names the
register. Esc and v end it, but only after v do j and k keep their column, as vim's curswant. yazi's
visual mode moves with the keys of its normal mode; the yazi column here has the keys that act on
what it selects. mc selects text with Shift and the arrows, under Insert mode.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| One character left | `left` | ⏳ | h<br>Left | — | — | — | — |
| One character right | `right` | ⏳ | l<br>Right | — | — | — | — |
| One character left; from the start of a line, to the end of the one before | `char_backward` | ⏳ | Backspace<br>Ctrl+h | — | — | — | — |
| One character right; from the end of a line, to the start of the next | `char_forward` | ⏳ | Space | — | — | — | — |
| Start of the line | `text_start` | ⏳ | 0<br>Home | — | — | — | — |
| The first character of the line that is not blank | `first_non_blank` | ⏳ | ^ | — | — | — | — |
| End of the line | `text_end` | ⏳ | $<br>End | — | — | — | — |
| The first character of the line above that is not blank; an operator takes the lines, as vim's - | `prev_line` | ⏳ | - | — | — | — | — |
| The first character of the line below that is not blank; an operator takes the lines, as vim's + | `next_line` | ⏳ | + | — | — | — | — |
| The first character of the line that is not blank, or of the line a count less one below; d _ deletes the line as d d | `this_line` | ⏳ | _ | — | — | — | — |
| The start of the next word | `word_forward` | ⏳ | w | — | — | — | — |
| The start of the word before the cursor | `word_backward` | ⏳ | b | — | — | — | — |
| The end of the word | `word_end` | ⏳ | e | — | — | — | — |
| The start of the next word, where only blanks part words | `big_word_forward` | ⏳ | W | — | — | — | — |
| The start of the word before, where only blanks part words | `big_word_backward` | ⏳ | B | — | — | — | — |
| The end of the word, where only blanks part words | `big_word_end` | ⏳ | E | — | — | — | — |
| The next character typed after it, in the line | `find_char` | ⏳ | f | — | — | — | — |
| Back to the character typed after it | `find_char_back` | ⏳ | F | — | — | — | — |
| Right before the next character typed after it | `till_char` | ⏳ | t | — | — | — | — |
| Back to right after the character typed after it | `till_char_back` | ⏳ | T | — | — | — | — |
| Find again, as the last f, t, F, or T | `repeat_find` | ⏳ | ; | — | — | — | — |
| Find again, the other way | `repeat_find_back` | ⏳ | , | — | — | — | — |
| The other end of the selection | `other_end` | ⏳ | o | — | — | — | — |
| Delete the selection | `delete_operator` | ⏳ | d<br>x<br>Delete | — | — | d | — |
| Delete the selection, then Insert mode | `change_operator` | ⏳ | c<br>s | — | — | c | — |
| Copy the selection to a register | `yank_operator` | ⏳ | y | — | — | y | — |
| Switch the case of the selection | `toggle_case` | ⏳ | \~ | — | — | — | — |
| Normal mode again | `normal_mode` | ⏳ | Esc<br>Ctrl+[ | — | — | — | — |
| Normal mode again, keeping the column of j and k, as vim's v | `visual_mode` | ⏳ | v | — | — | — | — |
| Uppercase / lowercase the selection | — | ⏳ | — | — | — | U / u | — |

## Quick search (`quick_search`, Insert mode)

In the active panel, in Insert mode, as vim's search is typed: characters are part of the name
searched for, and the keys of `input` edit it, Ctrl+r among them, which types the text of a
register; it falls back to `input`, then to `global`. As vim's incremental search, it finds the
first match after the row the cursor was on, round, or before it, up, after ?, and with a count
before / or ? the match of that number (2/); each edit searches again from there, and while nothing
matches, the cursor stays there and the text stays as typed. Enter and Ctrl+j end the search, on the
match, or say that nothing matches, and Esc and Ctrl+c give it up, putting back the cursor and the
rows on screen, as in vim, but Backspace and Delete with nothing typed do not, as on the command
line, so that one too many does not go to the parent directory; Delete at the end of the text
deletes the character before it, and Shift and Ctrl with Left and Right go by what spaces part, as
on vim's command line; Ctrl+g and Ctrl+t go to the next match down and the next up, as many
as the count before / or ?, and the search goes on from there, and Ctrl+b and Ctrl+e go to the start
and the end of the text, as on vim's command line. Up, Down, PgUp, PgDn, and the F-keys, Alt+F1 and
Alt+F2 among them, end the search and do what they do in the panel, as in mc, but Shift+F7, which
goes to the next match; Left, Right, Home, and End move in the text, and other keys that none of
them binds do nothing. The last search stays for n, N, and Shift+F7 in the panel. yazi searches as
the name is typed in an input box, and ranger as it is typed on its console; both go to the next and
previous match with keys of the panel, once the search is over.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| End the search; the cursor stays on the match | `cancel` | ⏳ | Enter<br>Ctrl+m<br>Ctrl+j | Esc | — | Ctrl+c<br>Esc Esc | Esc<br>Ctrl+c<br>Ctrl+g |
| Give the search up: the cursor goes back to where it started, and the search is not kept | `abort` | ✅ | Esc<br>Ctrl+[<br>Ctrl+c | — | — | — | — |
| Next match; with nothing typed, the last search again, as mc's Ctrl+s Ctrl+s | `quick_search` | ✅ | Ctrl+g | Ctrl+s<br>Alt+s | Ctrl+Enter | n | n |
| Previous match | `prev_match` | ✅ | Ctrl+t | — | Ctrl+Shift+Enter | N | N |
| Start of the line | `text_start` | ⏳ | Ctrl+b | — | — | — | — |
| End of the line | `text_end` | ⏳ | Ctrl+e | — | — | — | — |
| End the search, and one row up | `up` | ⏳ | Up | — | — | — | — |
| End the search, and one row down | `down` | ⏳ | Down | — | — | — | — |
| End the search, and one page up | `page_up` | ⏳ | PgUp | — | — | — | — |
| End the search, and one page down | `page_down` | ⏳ | PgDn | — | — | — | — |
| End the search, and show the help | `help` | ⏳ | F1 | — | — | — | — |
| End the search, and view the file under the cursor | `view` | ⏳ | F3 | — | — | — | — |
| End the search, and edit the file under the cursor | `edit` | ⏳ | F4 | — | — | — | — |
| End the search, and copy the marked entries, or the one under the cursor | `copy` | ⏳ | F5 | — | — | — | — |
| End the search, and move or rename the marked entries, or the one under the cursor | `move` | ⏳ | F6 | — | — | — | — |
| End the search, and rename the entry under the cursor in its row | `rename` | ⏳ | Shift+F6 | — | — | — | — |
| Next match, with mc's and Far's key to search again | `next_match` | ⏳ | Shift+F7 | — | — | — | — |
| End the search, and make a directory | `mkdir` | ⏳ | F7 | — | — | — | — |
| End the search, and delete the marked entries, or the one under the cursor | `delete` | ⏳ | F8 | — | — | — | — |
| End the search, and open the pull-down menu | `pull_down` | ⏳ | F9 | — | — | — | — |
| End the search, and open the pull-down menu on the command that ran from it last | `pull_down_last` | ⏳ | Shift+F9 | — | — | — | — |
| End the search, and quit | `quit` | ⏳ | F10 | — | — | — | — |
| End the search, and change the location of the panel drawn on the left | `location_menu_left` | ⏳ | Alt+F1 | — | — | — | — |
| End the search, and change the location of the panel drawn on the right | `location_menu_right` | ⏳ | Alt+F2 | — | — | — | — |
| Paste from clipboard | — | 🚫 | — | — | Ctrl+v<br>Shift+Insert | — | — |

## Renaming in place (`rename`, Insert mode)

r or Shift+F6 edits the name in the entry's row, in Insert mode, with the keys of `input`; it
falls back to `input`, then to `global`, and other keys do nothing. Enter renames in every mode,
and Ctrl+c leaves the name as it was in every mode; Esc goes to Normal mode, through `input`, and
there leaves the name as it was. Ctrl+o runs one command of Normal mode, then Insert mode comes
back, as in vim. None of mc, Far, yazi, and ranger renames in the row: mc and Far ask in a
dialog, yazi in an input box, and ranger on its console, whose keys are those of `input`.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Rename to the name typed | `confirm` | ✅ | Enter<br>Ctrl+m | — | — | — | — |
| One command of Normal mode, then Insert mode again | `normal_once` | ⏳ | Ctrl+o | — | — | — | — |
| Keep the name as it was | `cancel` | ⏳ | Ctrl+c | — | — | — | — |

## Renaming in place (`rename`, Normal mode)

After Esc: the keys of `input` in Normal mode edit the name, and it falls back to them, then to
`global`. Esc leaves the name as it was.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Rename to the name typed | `confirm` | ⏳ | Enter<br>Ctrl+m | — | — | — | — |
| Keep the name as it was | `cancel` | ⏳ | Esc<br>Ctrl+[<br>Ctrl+c | — | — | — | — |

## Renaming in place (`rename`, Visual mode)

Text selected in the name. It falls back to `input` in Visual mode, whose Esc goes back to Normal
mode, then to `global`.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Rename to the name typed | `confirm` | ⏳ | Enter<br>Ctrl+m | — | — | — | — |
| Keep the name as it was | `cancel` | ⏳ | Ctrl+c | — | — | — | — |

## Dialogs and help (`dialog`, Normal mode)

A dialog whose focus is on a button, a check box, or a list, and the windows of the jobs, a job, and
checksums, in Normal mode, where they start; it falls back to `global`. The help falls back to it,
so its keys scroll and close the help too, and so do the text fields of dialogs in Normal mode,
after the keys of `input`: j and k move from a field, as Up and Down do, and a count moves as many
stops, past the fields too, g g and G to the first and the last field or button, as vim's to the
first and the last line, and q and Esc close the dialog. Dialogs are modal: the panel's keys do
nothing there. The yazi column has the keys of its confirmations (confirm), its list of tasks
(tasks), and its help; the ranger column, those of its list of tasks, since ranger asks its
questions on the console.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Previous button; in the help and lists, one row up | `up` | ✅ | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p (help, lists) | Up (help) | k<br>Up<br>Ctrl+p (help) | Up<br>k<br>Ctrl+p |
| Next button; in the help and lists, one row down | `down` | ✅ | j<br>Down<br>Ctrl+n | Down<br>Ctrl+n (help, lists) | Down (help) | j<br>Down<br>Ctrl+n (help) | Down<br>j<br>Ctrl+n<br>Enter |
| Previous button | `left` | ✅ | h<br>Left | Left | — | — | — |
| Next button | `right` | ✅ | l<br>Right | Right | — | — | — |
| The first field or button; in the help and lists, one page up | `page_up` | ⏳ | Ctrl+b<br>PgUp | PgUp<br>Alt+v<br>b (help)<br>Backspace (help) | — | — | p<br>b<br>Ctrl+b |
| The last field or button; in the help and lists, one page down | `page_down` | ⏳ | Ctrl+f<br>PgDn | PgDn<br>Ctrl+v<br>Space (help)<br>f (help) | — | — | n<br>f<br>Ctrl+f<br>Space |
| The first field or button; in the help and lists, the first row | `home` | ⏳ | g g<br>Home | Home<br>A1<br>g (help)<br>Ctrl+Home (help)<br>Ctrl+PgUp (help)<br>Alt+< | — | — | Home<br>g |
| The last field or button; in the help and lists, the last row | `end` | ⏳ | G<br>End | End<br>C1<br>G (help)<br>Ctrl+End (help)<br>Ctrl+PgDn (help)<br>Alt+> | — | — | End<br>G |
| Next field or button | `next_field` | ✅ | Tab<br>Ctrl+i | Tab | — | — | — |
| Previous field or button | `prev_field` | ✅ | Shift+Tab | Shift+Tab | — | — | — |
| Press the focused button; from a text field or a check box, the default one; close the help | `confirm` | ✅ | Enter<br>Ctrl+m | Enter | Ctrl+Enter (default button) | Enter (confirm, help)<br>y (confirm) | — |
| Switch the check box, choose the radio button, or press the focused button | `toggle` | ✅ | Space | Space | — | — | — |
| Cancel; abort the job in front; close the help or the window | `cancel` | ⏳ | Esc<br>Ctrl+[<br>Ctrl+c<br>q<br>F10 | Esc<br>Esc Esc<br>F10<br>Ctrl+g<br>Ctrl+c (abort copy/delete) | Esc | Esc<br>Ctrl+[<br>Ctrl+c<br>n (confirm)<br>w (tasks) | Esc<br>q<br>Q<br>w<br>Ctrl+c |
| Copy/move in background (in dialog) | — | ⏳ | — | Alt+b | — | — | — |
| Focus first dialog item | — | ⏳ | — | — | Home | — | — |
| Focus default dialog item | — | ⏳ | — | — | PgDn<br>End | — | — |
| Move dialog | — | ⏳ | — | — | Ctrl+F5 | — | — |
| Checkbox: on / off / undefined | — | ⏳ | — | — | Numpad+ / Numpad- / Numpad* | — | — |
| Help: follow link | — | ⏳ | — | Enter<br>Right | Enter | — | — |
| Help: next / previous link | — | ⏳ | — | Tab / Alt+Tab | Tab / Shift+Tab | — | — |
| Help: previous topic | — | ⏳ | — | F3<br>Left<br>l | Alt+F1<br>Backspace | — | — |
| Help: contents | — | ⏳ | — | F2<br>c | Shift+F1 | — | — |
| Help: plugins help | — | ⏳ | — | — | Shift+F2 | — | — |
| Help: search | — | ⏳ | — | — | F7 | — | — |
| Help: maximize/restore window | — | ⏳ | — | — | F5 | — | — |
| Help: full list of help keys | — | ⏳ | — | F1 (again) | — | — | — |
| The output of the job under the cursor | — | ⏳ | — | — | — | Enter (tasks) | — |
| Abort the job under the cursor | — | ⏳ | — | — | — | x (tasks) | d d<br>Delete |
| Run the job under the cursor first | — | ⏳ | — | — | — | — | K<br>PgUp |
| Run the job under the cursor last | — | ⏳ | — | — | — | — | J<br>PgDn |

## Help (`help`, Normal mode)

g ? or F1: the keys of every mode and context, and notes on them, in Normal mode. It falls back to
`dialog`, whose j, k, Ctrl+b, Ctrl+f, g g, G, Esc, and q scroll the help and close it, then to
`global`; the keys of the other programs' help that do the same are there too. It adds the keys of
the viewer: Ctrl+y, Ctrl+e, Ctrl+u, and Ctrl+d, as in vim, and Space, f, b, d, and u, as in less and
ranger's pager, which scroll as in the viewer; Space presses no button here.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| One line up | `up` | ✅ | Ctrl+y | — | — | — | — |
| One line down | `down` | ✅ | Ctrl+e | — | — | — | — |
| A whole page up, as less's b | `window_up` | ✅ | b | b<br>Backspace | — | — | — |
| A whole page down, as less's Space and f | `window_down` | ✅ | Space<br>f | Space<br>f | — | — | — |
| Half a page up | `half_page_up` | ✅ | Ctrl+u<br>u | u | — | — | — |
| Half a page down | `half_page_down` | ✅ | Ctrl+d<br>d | d | — | — | — |

## Text fields (`dialog_input`, Insert mode)

A text field in a dialog, in Insert mode, where a dialog whose first focus is a text field
starts; it falls back to `input`, whose keys edit the text, then to `dialog`, then to `global`.
Up, Down, Tab, and Shift+Tab still move between the fields and the buttons, and Enter presses the
default button. Esc goes to Normal mode, through `input`, where the field binds nothing of its
own: the keys of `input` edit the text, then those of `dialog` move, accept, and close, Esc among
them; Ctrl+o runs one command of Normal mode, then Insert mode comes back. A secret field, such
as a password, has no Normal mode: Esc closes its dialog, and Ctrl+o and Alt and a key do
nothing. The mc column has the keys of mc's input lines; the far column, those of Far's dialogs.
mc and Far move between the fields and press the buttons with the keys of their dialogs, under
`dialog`.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Previous field or button | `up` | ⏳ | Up | — | — | — | — |
| Next field or button | `down` | ⏳ | Down | — | — | — | — |
| Next field or button | `next_field` | ⏳ | Tab<br>Ctrl+i | — | — | — | — |
| Previous field or button | `prev_field` | ⏳ | Shift+Tab | — | — | — | — |
| Press the default button | `confirm` | ⏳ | Enter<br>Ctrl+m | — | — | — | — |
| One command of Normal mode, then Insert mode again | `normal_once` | ⏳ | Ctrl+o | — | — | — | — |
| Cancel the dialog | `cancel` | ⏳ | Ctrl+c<br>F10 | — | — | — | — |
| Input line history | — | ⏳ | — | Alt+h | Ctrl+Up (in dialogs)<br>Ctrl+Down (in dialogs) | — | — |
| Previous history entry | — | ⏳ | — | Alt+p<br>Ctrl+Down | Ctrl+e | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p |
| Next history entry | — | ⏳ | — | Alt+n<br>Ctrl+Up | Ctrl+x | j<br>Down<br>Ctrl+n | Down<br>Ctrl+n |
| Dialog history: clear | — | ⏳ | — | Shift+Delete<br>D | Delete | — | — |
| Dialog history: delete item | — | ⏳ | — | Delete<br>d | Shift+Delete | — | — |
| Dialog history: mark item | — | ⏳ | — | — | Insert | — | — |
| Insert file name under cursor into dialog | — | ⏳ | — | — | Shift+Enter | — | — |
| Insert passive panel file name into dialog | — | ⏳ | — | — | Ctrl+Shift+Enter | — | — |

## Text fields (`dialog_input`, Visual mode)

Text selected in a dialog's text field. It falls back to `input` in Visual mode, whose Esc goes
back to Normal mode, then to `dialog` and `global`.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Press the default button | `confirm` | ⏳ | Enter<br>Ctrl+m | — | — | — | — |
| Cancel the dialog | `cancel` | ⏳ | Ctrl+c<br>F10 | — | — | — | — |

## Path fields (`path_input`, Insert mode)

A text field for a path, in Quick cd, F5, F6, and F7, in Insert mode; it falls back to
`dialog_input`, then as text fields do. Tab completes as vim's command line: one match goes in, or
the first of several, which the completion list shows. Shift+Tab and Down still leave the field.
yazi lists the choices for a path as it is typed, without a key; ranger completes on its console
without a list.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Complete the path: one match goes in, or the first of several, which a list shows | `complete` | ⏳ | Tab<br>Ctrl+i | Alt+Tab | — | — | Tab |

## Completion list (`completion`, Insert mode)

Under a path field, or over the command line, after a Tab that found several matches, in Insert
mode, as vim's command-line completion with defaults.vim's 'wildmenu': the first match goes in with
the Tab, and each move puts the next one in the field at once, round through what was typed; the
list shows as a column, as vim's pop-up menu ('wildoptions' pum) shows them, with its keys: Up and
Down go through the matches, and Left and Right complete in the directory above and in the one
chosen. Case never counts, as with vim's 'fileignorecase' on macOS. Ctrl+y keeps the match and
Ctrl+e puts back what was typed. Keys it does not bind close it and go to the field with the match
in it: Enter confirms, Esc goes to Normal mode, or closes the command line, and Ctrl+c closes the
dialog or the command line, as in vim. Over the command line the list falls back to `command_line`.
ranger has no list: Tab and Shift+Tab put the next and the previous choice in the console's line.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| The next match in the field; after the last, what was typed | `down` | ⏳ | Tab<br>Ctrl+i<br>Ctrl+n<br>Down | — | — | Alt+j<br>Down<br>Ctrl+n | Tab |
| The match before in the field; before the first, what was typed | `up` | ⏳ | Shift+Tab<br>Ctrl+p<br>Up | — | — | Alt+k<br>Up<br>Ctrl+p | Shift+Tab |
| A page of matches down | `page_down` | ⏳ | PgDn | — | — | — | — |
| A page of matches up | `page_up` | ⏳ | PgUp | — | — | — | — |
| Complete in the directory above the one listed, as vim's pop-up menu | `left` | ⏳ | Left | — | — | — | — |
| Complete in the directory chosen, as vim's pop-up menu | `right` | ⏳ | Right | — | — | — | — |
| Keep the match, and close the list | `accept_completion` | ⏳ | Ctrl+y | — | — | Tab | — |
| Put back what was typed, and close the list | `cancel_completion` | ⏳ | Ctrl+e | — | — | Ctrl+c | — |
| Put the choice in the field and run it | — | ⏳ | — | — | — | Enter | — |

## Viewer (`viewer`, Normal mode)

i or F3, in Normal mode; it falls back to `global`. yazi has no viewer: it previews the file under
the cursor beside the list. The ranger column has the keys of its pager, which shows files, the
output of commands, and the log. The key that opens the viewer closes it too: F3, and i, as in
ranger. Ctrl+f and Ctrl+b turn the page but for two rows, as in vim; Space, f, and b turn the whole
page, and u and d half of it, as in less. Unlike vim's, every key down stops with the end of the
text at the bottom, where a dimmed line marks it; the help scrolls the same way. Lines cut show
`«` and `»` where they go on past the screen, as vim's 'listchars' `precedes` and `extends`, and the
z keys scroll them as vim's do; wrapped lines start each row after the first with `»`, as vim's
'showbreak'.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| One line up | `up` | ✅ | k<br>Ctrl+y<br>Up<br>Ctrl+p | Up<br>k<br>y<br>Insert<br>Ctrl+p | Up | — | Up<br>k<br>Ctrl+p |
| One line down | `down` | ✅ | j<br>Ctrl+e<br>Down<br>Ctrl+n<br>Enter<br>Ctrl+m | Down<br>j<br>e<br>Enter<br>Delete<br>Ctrl+n | Down | — | Down<br>j<br>Ctrl+n<br>Enter |
| One page up, less the two rows that vim's Ctrl+b keeps | `page_up` | ⏳ | Ctrl+b<br>PgUp | PgUp<br>Alt+v | PgUp | — | PgUp<br>p<br>Ctrl+b |
| One page down, less the two rows that vim's Ctrl+f keeps | `page_down` | ⏳ | Ctrl+f<br>PgDn | PgDn<br>Ctrl+v | PgDn | — | PgDn<br>n<br>Ctrl+f |
| A whole page up, as less's b | `window_up` | ⏳ | b | b<br>Backspace<br>Ctrl+h | — | — | b |
| A whole page down, as less's Space and f | `window_down` | ⏳ | Space<br>f | Space<br>f | — | — | f<br>Space |
| Half a page up | `half_page_up` | ✅ | Ctrl+u<br>u | u | — | — | Ctrl+u<br>u |
| Half a page down | `half_page_down` | ✅ | Ctrl+d<br>d | d | — | — | Ctrl+d<br>d |
| The start of the file | `home` | ✅ | g g<br>Home | Home<br>A1<br>g<br>Ctrl+Home<br>Ctrl+PgUp<br>Alt+< | Home<br>Ctrl+Home | — | Home<br>g |
| The end of the file | `end` | ✅ | G<br>End | End<br>C1<br>G<br>Ctrl+End<br>Ctrl+PgDn<br>Alt+> | End<br>Ctrl+End | — | End<br>G |
| One column left, when lines are cut (ranger: four) | `left` | ✅ | h<br>Left | Left<br>h | Left | — | Left<br>h |
| One column right, when lines are cut, until the end of the longest line on screen is at the right edge (ranger: four) | `right` | ⏳ | l<br>Right | Right<br>l | Right | — | Right<br>l |
| A column left, when lines are cut, as vim's zh; with a count, that many | `scroll_left` | ⏳ | z h<br>z Left | — | — | — | — |
| A column right, when lines are cut, as vim's zl and as far as l; with a count, that many | `scroll_right` | ⏳ | z l<br>z Right | — | — | — | — |
| Half the screen left, when lines are cut, as vim's zH | `scroll_half_left` | ⏳ | z H | — | — | — | — |
| Half the screen right, when lines are cut, as vim's zL | `scroll_half_right` | ⏳ | z L | — | — | — | — |
| The start of the lines, when they are cut, as vim's zs | `scroll_start` | ⏳ | z s | — | — | — | — |
| The end of the longest line on screen at the right edge, when lines are cut, as vim's ze | `scroll_end` | ⏳ | z e | — | — | — | — |
| Wrap long lines, or cut them | `toggle_wrap` | ✅ | F2 | F2 | F2 | — | — |
| Help | `help` | ✅ | g ?<br>F1 | F1 | F1 | — | — |
| Close the viewer | `quit` | ✅ | q<br>i<br>Esc<br>Ctrl+[<br>Ctrl+c<br>F3<br>F10 | F3<br>F10<br>q<br>Esc | F10<br>F3<br>Numpad5<br>Esc | — | Esc<br>q<br>Q<br>i<br>F3 |
| F1…F10 on terminals without function keys | — | 🚫 | — | Esc, then 1…9, 0 | — | — | — |
| Wrap type (by chars/words) | — | ⏳ | — | — | Shift+F2 | — | — |
| Hex/code mode | — | ⏳ | — | F4 | F4 | — | — |
| Select mode (text/code/dump) | — | ⏳ | — | — | Shift+F4 | — | — |
| Go to position | — | ⏳ | — | F5 | Alt+F8 | — | — |
| Switch to editor | — | ⏳ | — | — | F6 | — | E |
| Search | — | ⏳ | — | F7<br>/<br>? (backward) | F7 | — | — |
| Continue search forward | — | ⏳ | — | Ctrl+s | Shift+F7<br>Space | — | — |
| Continue search backward | — | ⏳ | — | Ctrl+r | Alt+F7 | — | — |
| Continue search in chosen direction | — | ⏳ | — | Shift+F7<br>n | — | — | — |
| Temporarily reverse search direction | — | ⏳ | — | N | — | — | — |
| Raw/Parsed | — | ⏳ | — | F8 | — | — | — |
| Format/Unformat | — | ⏳ | — | F9 | — | — | — |
| Code page | — | ⏳ | — | Alt+e | F8 (OEM/ANSI)<br>Shift+F8 (menu) | — | — |
| 20 columns left / right | — | ⏳ | — | Ctrl+Left / Ctrl+Right (10 columns) | Ctrl+Left / Ctrl+Right | — | — |
| Leftmost / rightmost column | — | ⏳ | — | Ctrl+a / Ctrl+e | Ctrl+Shift+Left / Ctrl+Shift+Right | — | — |
| Shift characters/bytes (dump, code) | — | ⏳ | — | — | Ctrl+Left / Ctrl+Right | — | — |
| Bytes per line −1 / +1 (code) | — | ⏳ | — | — | Alt+Left / Alt+Right | — | — |
| Bytes per line to nearest multiple of 16 (code) | — | ⏳ | — | — | Ctrl+Alt+Left / Ctrl+Alt+Right | — | — |
| Set bookmark | — | ⏳ | — | m<br>0…9, then m | RightCtrl+0…9<br>Ctrl+Shift+0…9 | — | — |
| Go to bookmark | — | ⏳ | — | r<br>0…9, then r | LeftCtrl+0…9 | — | — |
| Return to previous position | — | ⏳ | — | — | Alt+Backspace<br>Ctrl+z | — | — |
| Next file | — | ⏳ | — | Ctrl+f | Numpad+ | — | — |
| Previous file | — | ⏳ | — | Ctrl+b | Numpad- | — | — |
| Ruler | — | ⏳ | — | Alt+r | — | — | — |
| User screen | — | ⏳ | — | Ctrl+o | Ctrl+o<br>Ctrl+Alt+Shift (temporarily) | — | — |
| Go to file in panel | — | ⏳ | — | — | Ctrl+F10 | — | — |
| Plugin commands | — | ⏳ | — | — | F11 | — | — |
| View and edit history | — | ⏳ | — | Alt+E | Alt+F11 | — | — |
| Far window size | — | ⏳ | — | — | Alt+F9 | — | — |
| Viewer settings | — | ⏳ | — | — | Alt+Shift+F9 | — | — |
| Functional key bar | — | ⏳ | — | — | Ctrl+b | — | — |
| Status line | — | ⏳ | — | — | Ctrl+B | — | — |
| Scrollbar | — | ⏳ | — | — | Ctrl+s | — | — |
| Copy selection | — | ⏳ | — | — | Ctrl+Insert<br>Ctrl+c | — | — |
| Clear selection | — | ⏳ | — | — | Ctrl+u | — | — |
| Select text manually | — | ⏳ | — | — | Shift+Click | — | — |

## Lists (`list`, Normal mode)

The keys that the windows listing items to choose from share, in Normal mode, where all but the
zoxide window start: the location menu, the tab menu, the zoxide window, the workspaces, and the
command history fall back to it, then to `global`, and their sections list only the keys they add.
Keys that nothing binds do nothing: no digit or letter picks an item. The rows scroll as the
panels' do, as vim's window, headings among them. `/` types in the filter, or the keywords of the
zoxide window, in Insert mode, and Esc there comes back with the filter kept. Keys never go to it
alone; the help lists its keys with each of those windows, with what Enter takes there. The mc
column has the keys of mc's lists, such as its histories and its hotlist; the far column, those of
Far's menus; the yazi column, those of its pick window, which asks for the program to open files
with. None of them has modes: their keys are here, not under Insert mode.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Previous item | `up` | ⏳ | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p | Up | k<br>Up | — |
| Next item | `down` | ⏳ | j<br>Down<br>Ctrl+n<br>Ctrl+j | Down<br>Ctrl+n | Down | j<br>Down | — |
| One page up | `page_up` | ⏳ | Ctrl+b<br>PgUp | PgUp<br>Alt+v | — | — | — |
| One page down | `page_down` | ⏳ | Ctrl+f<br>PgDn | PgDn<br>Ctrl+v | — | — | — |
| Half a page up | `half_page_up` | ⏳ | Ctrl+u | — | — | — | — |
| Half a page down | `half_page_down` | ⏳ | Ctrl+d | — | — | — | — |
| First item | `home` | ⏳ | g g<br>Home | Home<br>A1<br>Alt+< | — | — | — |
| Last item | `end` | ⏳ | G<br>End | End<br>C1<br>Alt+> | — | — | — |
| Type in the filter, in Insert mode | `quick_search` | ⏳ | / | — | Ctrl+Alt+f<br>RightAlt | — | — |
| Show the whole row under the cursor under the rows, until asked again | `show_full` | ⏳ | K | — | — | — | — |
| Take the item, as the window's section says | `confirm` | ⏳ | Enter<br>Ctrl+m | Enter | Enter | Enter | — |
| Close the window | `cancel` | ⏳ | Esc<br>Ctrl+[<br>Ctrl+c<br>q<br>F10 | Esc<br>F10<br>Ctrl+g | Esc | Esc<br>Ctrl+[<br>Ctrl+c | — |
| F1…F10 on terminals without function keys, in the windows that bind them | — | 🚫 | — | Esc, then 1…9, 0 | — | — | — |

## Lists (`list`, Insert mode)

While `/` has the filter take text, in Insert mode: characters are text, and the windows fall back
to `list`, then to `input`, whose keys edit the filter, then to `global`. Home and End go to the
start and the end of the filter here, and to the first and the last item in Normal mode. Ctrl+r
types the text of a register, as in a text field. Esc goes back to the rows with the filter
kept, and Ctrl+c closes the window. mc, Far, and yazi have no modes in their lists: their keys
are under Normal mode.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Previous item | `up` | ⏳ | Up<br>Ctrl+p | — | — | — | — |
| Next item | `down` | ⏳ | Down<br>Ctrl+n | — | — | — | — |
| One page up | `page_up` | ⏳ | PgUp | — | — | — | — |
| One page down | `page_down` | ⏳ | PgDn | — | — | — | — |
| Take the item, as the window's section says | `confirm` | ⏳ | Enter<br>Ctrl+m<br>Ctrl+y | — | — | — | — |
| Back to the rows, with the filter kept | `normal_mode` | ⏳ | Esc<br>Ctrl+[ | — | — | — | — |
| Close the window | `cancel` | ⏳ | Ctrl+c<br>F10 | — | — | — | — |

## Location menu (`location_menu`, Normal mode)

Ctrl+w 1 or Alt+F1, and Ctrl+w 2 or Alt+F2: the home directory, the volumes, and the hosts, under
their headings. It falls back to `list`, whose Enter opens the place in the panel, or, on the last
row of a group that shows only its first places, shows the rest. The far column has the keys of
Far's menus, its drive menu among them.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Edit the settings of the host under the cursor | `edit_host` | ✅ | F4 | — | — | — | — |
| Disconnect the host under the cursor, or stop connecting to it | `disconnect` | ⏳ | d d<br>F8 | — | Delete | — | — |
| Read the volumes and hosts again | `reload` | ⏳ | R | — | Ctrl+r | — | — |
| Lock filter | — | ⏳ | — | — | Ctrl+Alt+l | — | — |
| Shift all items by 1 position | — | ⏳ | — | — | Alt+Left<br>Alt+Right | — | — |
| Shift selected item by 1 position | — | ⏳ | — | — | Alt+Shift+Left<br>Alt+Shift+Right | — | — |
| Shift all items by 20 positions | — | ⏳ | — | — | Ctrl+Alt+Left<br>Ctrl+Alt+Right | — | — |
| Shift selected item by 20 positions | — | ⏳ | — | — | Ctrl+Shift+Left<br>Ctrl+Shift+Right | — | — |
| Align all items left / right | — | ⏳ | — | — | Alt+Home / Alt+End | — | — |
| Align selected item left / right | — | ⏳ | — | — | Alt+Shift+Home / Alt+Shift+End | — | — |
| Fixed menu columns | — | ⏳ | — | — | Shift+F5 | — | — |

## Location menu (`location_menu`, Insert mode)

While `/` has its filter take text. It falls back to `list`, then to `input`, then to `global`;
F4 and F8 still edit and disconnect the host under the cursor.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Edit the settings of the host under the cursor | `edit_host` | ⏳ | F4 | — | — | — | — |
| Disconnect the host under the cursor, or stop connecting to it | `disconnect` | ⏳ | F8 | — | — | — | — |

## Tab menu (`tab_menu`, Normal mode)

t h and t l: the tabs of the left or the right panel, drawn over it as the location menu is. It
falls back to `list`, whose Enter shows the tab in the panel; d d and F8 close the tab under the
cursor, as they delete a workspace, and the menu stays open; with a count, 3d d closes three, as
vim's deletes three lines, and so do d d in the location menu, the workspaces, and the history. mc,
Far, yazi, and ranger have no menu of tabs.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Close the tab under the cursor; the last one stays | `close_tab` | ⏳ | d d<br>F8 | — | — | — | — |

## Tab menu (`tab_menu`, Insert mode)

While `/` has its filter take text. It falls back to `list`, then to `input`, then to `global`.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Close the tab under the cursor; the last one stays | `close_tab` | ⏳ | F8 | — | — | — | — |

## zoxide window (`jump`, Normal mode)

g z: the directories that zoxide ranks. It opens in Insert mode, its keywords taking text, as z
takes them in a shell; Esc goes to the rows, in Normal mode, where `/` types again and Esc closes
it. It binds no keys of its own, in either mode: it falls back to `list`, whose Enter opens the
directory in the active panel, and, while the keywords take text, to `input` too.

## Workspaces window (`workspaces`, Normal mode)

t w, or F9 → Workspace → Workspace list…; F9 → Workspace lists the first ten too, which Alt and
their digits restore there. It falls back to `list`, whose Enter restores the workspace: it
replaces the tabs of both panels. a adds one; r and F6 rename, and d d and F8 delete, as they
rename and delete files.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Save the tabs of both panels as a new workspace | `save_workspace` | ⏳ | a | — | — | — | — |
| Rename the workspace | `move` | ⏳ | r<br>F6 | — | — | — | — |
| Delete the workspace | `delete` | ⏳ | d d<br>F8 | — | — | — | — |

## Workspaces window (`workspaces`, Insert mode)

While `/` has its filter take text. It falls back to `list`, then to `input`, then to `global`;
F6 and F8 still rename and delete the workspace under the cursor.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Rename the workspace | `move` | ⏳ | F6 | — | — | — | — |
| Delete the workspace | `delete` | ⏳ | F8 | — | — | — | — |

## Pull-down menu (`pull_down`, Normal mode)

F9 or g m; Shift+F9 or g M, on the command that ran from it last. It falls back to `global`. h, j,
k, and l move, as the arrows do, g g and G go to the first and the last command and stay there, as
vim's do, and Alt and the highlighted letter of a menu open it, and of a command run it; mc's menus
take the letter alone.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| The command above | `up` | ⏳ | k<br>Up<br>Ctrl+p | Up<br>Ctrl+p | — | — | — |
| The command below | `down` | ⏳ | j<br>Down<br>Ctrl+n | Down<br>Ctrl+n | — | — | — |
| The menu to the left | `left` | ⏳ | h<br>Left<br>Shift+Tab | Left<br>Ctrl+b | — | — | — |
| The menu to the right | `right` | ⏳ | l<br>Right<br>Tab<br>Ctrl+i | Right<br>Ctrl+f | — | — | — |
| The first command | `home` | ⏳ | g g<br>Home<br>Ctrl+b<br>PgUp | Home<br>Alt+<<br>Ctrl+a | — | — | — |
| The last command | `end` | ⏳ | G<br>End<br>Ctrl+f<br>PgDn | End<br>Alt+><br>Ctrl+e | — | — | — |
| Open the menu, or run the command | `confirm` | ⏳ | Enter<br>Ctrl+m<br>Ctrl+y | Enter | — | — | — |
| Close the menu, then the menu bar | `cancel` | ⏳ | Esc<br>Ctrl+[<br>Ctrl+c<br>q<br>F9<br>F10 | Esc<br>F10<br>Ctrl+g | — | — | — |
| Open a menu or run a command by its letter (noc: Alt and the letter) | — | ⏳ | — | Letter | — | — | — |

## Command line (`command_line`, Insert mode)

`!` and `:` open it, in Insert mode, as vim's command line: Esc closes it, and Ctrl+f goes to Normal
mode, in place of vim's window of the command line, after which Esc leaves Insert mode, as in that
window; Ctrl+c and F10 close it in any mode. Ctrl+o runs one command of Normal mode, then Insert
mode comes back. Tab completes the word before the cursor as vim's :! does, with the completion
list. It falls back to `input`, whose Home and End go to the start and the end of the line here and
whose Ctrl+u deletes to the start, then to `global`; other keys, but characters, do nothing. Ctrl+j
arrives as LF where Enter arrives as CR, so it starts a new line in every terminal; Shift+Enter does
where the terminal speaks the kitty keyboard protocol. In mc and Far the command line is always
under the panels, so their keys here are pressed in the panels. yazi asks for a shell command in an
input box (; and : in the panels), and ranger's console (: and !) is its command line. The keys of
their input lines that noc lacks, such as the clipboard, are under `input`, as are yazi's modes.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| The line above; from the first line, the command before in the history of the panel's host | `up` | ✅ | Up | — | — | Up | Up |
| The line below; from the last line, the command after in the history | `down` | ✅ | Down | — | — | Down | Down |
| The start of the line, as on vim's command line | `text_start` | ✅ | Ctrl+b | — | — | — | — |
| The end of the line, as on vim's command line | `text_end` | ⏳ | Ctrl+e | — | — | — | — |
| A new line in the command; Shift+Enter where the terminal speaks the kitty keyboard protocol | `new_line` | ✅ | Ctrl+j<br>Shift+Enter | — | — | — | — |
| Normal mode, to edit the command with vim's keys | `normal_mode` | ⏳ | Ctrl+f | — | — | — | — |
| The command before in the history of the panel's host | `older_command` | ✅ | Ctrl+p | Alt+p<br>Ctrl+Down | Ctrl+e | Ctrl+p | Ctrl+p |
| The command after in the history of the panel's host | `newer_command` | ✅ | Ctrl+n | Alt+n<br>Ctrl+Up | Ctrl+x | Ctrl+n | Ctrl+n |
| One command of Normal mode, then Insert mode again | `normal_once` | ⏳ | Ctrl+o | — | — | — | — |
| Complete the word before the cursor, as vim's :! does: the first as a command of $PATH, the others as files, escaped for the shell; on a host, files there | `complete` | ⏳ | Tab<br>Ctrl+i | Alt+Tab | — | — | Tab |
| Run the command; after a \\ at the end of the line, a new line | `confirm` | ✅ | Enter<br>Ctrl+m | Enter | Enter | Enter | Enter<br>Ctrl+j |
| Close the command line; after Ctrl+f, leave Insert mode for Normal mode, as in vim's window of the command line | `cancel` | ⏳ | Esc<br>Ctrl+[ | — | Esc | Esc Esc | Esc<br>Ctrl+g |
| Close the command line, in any mode | `abort` | ⏳ | Ctrl+c<br>F10 | — | — | Ctrl+c | Ctrl+c |
| Edit the command in $VISUAL or $EDITOR, until a key of vim's is chosen for it | — | ⏳ | — | — | — | — | — |
| Insert current file name | — | ⏳ | — | Alt+Enter<br>Ctrl+Enter | Ctrl+j<br>Ctrl+Enter | — | — |
| Insert file name from passive panel | — | ⏳ | — | — | Ctrl+Shift+Enter | — | — |
| Insert full name of current file | — | ⏳ | — | Ctrl+Shift+Enter | Ctrl+f | — | — |
| Insert full file name from passive panel | — | ⏳ | — | — | Ctrl+; | — | — |
| Insert UNC file name (active / passive) | — | ⏳ | — | — | Ctrl+Alt+f / Ctrl+Alt+; | — | — |
| Insert tagged files of current panel | — | ⏳ | — | Ctrl+x t | — | — | — |
| Insert tagged files of other panel | — | ⏳ | — | Ctrl+x Ctrl+t | — | — | — |
| Insert current panel path | — | ⏳ | — | Ctrl+x p<br>Alt+a | Ctrl+Shift+[ | — | — |
| Insert other panel path | — | ⏳ | — | Ctrl+x Ctrl+p<br>Alt+A | Ctrl+Shift+] | — | — |
| Insert left / right panel path | — | ⏳ | — | — | Ctrl+[ / Ctrl+] | — | — |
| Insert UNC path of left / right panel | — | ⏳ | — | — | Ctrl+Alt+[ / Ctrl+Alt+] | — | — |
| Insert UNC path of active / passive panel | — | ⏳ | — | — | Alt+Shift+[ / Alt+Shift+] | — | — |
| Completion | — | ⏳ | — | Alt+Tab | Ctrl+Space | — | Tab |
| Insert character literally (quote); noc: Ctrl+v, under `input` | — | ⏳ | — | Ctrl+q | — | — | — |
| Clear command line | — | ⏳ | — | — | Ctrl+y | — | — |
| Select block in command line | — | ⏳ | — | Shift+Left<br>Shift+Right<br>Shift+Home<br>Shift+End<br>Ctrl+Shift+Left<br>Ctrl+Shift+Right | Alt+Shift+Left<br>Alt+Shift+Right<br>Alt+Shift+Home<br>Alt+Shift+End | — | — |

## Command line (`command_line`, Normal mode)

After Ctrl+f: the keys of `input` in Normal mode edit the command, and it falls back to them, then
to `global`. k and j go through the lines of the command, then the history, and q : opens the
window of the command history, as in vim; Esc closes the command line. The yazi column has the
keys of its input boxes in normal mode.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| The line above; from the first line, the command before in the history of the panel's host | `up` | ⏳ | k<br>Up | — | — | k | — |
| The line below; from the last line, the command after in the history | `down` | ⏳ | j<br>Down | — | — | j | — |
| The command history | `command_history` | ⏳ | q : | Alt+h | Alt+F8 | — | — |
| Run the command; after a \\ at the end of the line, a new line | `confirm` | ⏳ | Enter<br>Ctrl+m | — | — | — | — |
| Close the command line | `cancel` | ⏳ | Esc<br>Ctrl+[<br>Ctrl+c<br>F10 | — | — | — | — |

## Command line (`command_line`, Visual mode)

Text selected in the command. j and k go through its lines, but not the history; it falls back to
`input` in Visual mode, whose Esc goes back to Normal mode, then to `global`.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| The line above, in the same column, as vim's k; not into the history | `up` | ⏳ | k<br>Up | — | — | — | — |
| The line below, in the same column, as vim's j; not into the history | `down` | ⏳ | j<br>Down | — | — | — | — |
| Run the command | `confirm` | ⏳ | Enter<br>Ctrl+m | — | — | — | — |
| Close the command line | `cancel` | ⏳ | Ctrl+c<br>F10 | — | — | — | — |

## Command history (`command_history`, Normal mode)

q : in the panels, and on the command line in Normal mode. It falls back to `list`, whose Enter
puts the command on the command line without running it, as Ctrl+Enter does in Far. Far's rows
are those of its history menus.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| The commands of the panel's host, or of all hosts | `next_field` | ✅ | Tab<br>Ctrl+i | — | — | — | — |
| Remove the command from the history | `delete` | ⏳ | d d<br>F8 | Delete<br>d | Shift+Delete | — | — |
| Re-run command / open item | — | ⏳ | — | — | Enter | — | — |
| Run in separate window | — | ⏳ | — | — | Shift+Enter | — | — |
| Run as administrator | — | ⏳ | — | — | Ctrl+Alt+Enter | — | — |
| Folder history menu: go to on passive panel | — | ⏳ | — | — | Ctrl+Shift+Enter | — | — |
| Clear history | — | ⏳ | — | Shift+Delete<br>D | Delete | — | — |
| Lock/unlock item | — | ⏳ | — | — | Insert | — | — |
| Refresh (remove unavailable) | — | ⏳ | — | — | Ctrl+r (view and folder history) | — | — |
| Copy item to clipboard | — | ⏳ | — | — | Ctrl+c<br>Ctrl+Insert | — | — |
| View history menu: open in editor | — | ⏳ | — | F4 | F4 | — | — |
| View history menu: open in viewer | — | ⏳ | — | F3 | F3<br>Numpad5 | — | — |
| Command history menu: additional info | — | ⏳ | — | — | F3 | — | — |

## Command history (`command_history`, Insert mode)

While `/` has its filter take text. It falls back to `list`, then to `input`, then to `global`;
Tab and F8 still switch the hosts and remove the command.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| The commands of the panel's host, or of all hosts | `next_field` | ⏳ | Tab<br>Ctrl+i | — | — | — | — |
| Remove the command from the history | `delete` | ⏳ | F8 | — | — | — | — |

## Output of commands (`user_screen`, Normal mode)

The terminal's own screen, with the output of commands, in place of the panels. It falls back to
nothing, `global` neither: only the keys that bring the panels back do anything, g < again, Enter
and q, as the prompt under the output that vim's g < shows takes them, and Esc and Ctrl+c. mc
and Far show it and hide it with Ctrl+o.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Back to the panels | `cancel` | ⏳ | g <<br>Enter<br>Ctrl+m<br>q<br>Esc<br>Ctrl+[<br>Ctrl+c | Ctrl+o | Ctrl+o | — | — |

## Mouse

Not in the keymap: a press acts on what is under it. yazi and ranger take the mouse only in the
panel, its columns, and its tabs, and ranger in its pager too; neither has a double click.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Move the cursor to a row, activate the panel | — | ⏳ | Click | Click | Click | Click | Click |
| Open the row (as Enter) | — | ⏳ | Double click | Double click | Double click<br>Middle click | Right click | Right click |
| Mark the row, cursor stays on it | — | ⏳ | Right click | Right click | Right click | — | — |
| Scroll a panel or the viewer | — | ⏳ | Wheel | Wheel | Wheel | Wheel | Wheel |
| Press an F key | — | ⏳ | Click on the F-key bar | Click on the button bar | Click on the key bar | — | — |
| Show a tab | — | ⏳ | Click on the tab | — | — | Click on the tab | Click on the tab |
| Open a menu of the menu bar / run a command | — | ⏳ | Click | Click | Click | — | — |
| Close the menu bar | — | ⏳ | Click outside it | Click outside it | Click outside it | — | — |
| Dialog: press a button, switch a check box, choose a radio button | — | ⏳ | Click | Click | Click | — | — |
| Dialog: put the cursor in a text field | — | ⏳ | Click | Click | Click | — | — |
| Dialog: choose a radio button and press the default button | — | ⏳ | Double click | — | — | — | — |
| Menu or list in a window: move the cursor / open the row | — | ⏳ | Click / Double click | Click / Double click | Click / Click in a menu, Double click in a dialog's list | — | — |
| Close a menu or the list of completions | — | ⏳ | Click outside it | Click outside a menu | Click outside it | — | — |
| Scroll a menu, a list, or the help | — | ⏳ | Wheel | Wheel (not the help) | Wheel | — | — |
| Scroll a dialog's message that does not fit the screen | — | ⏳ | Wheel | — | — | — | — |
| Scroll bar of a panel, a list, a menu, a window, the viewer: a step of the wheel / a page | — | ⏳ | Click on an arrow / Click on the track | — | Click on an arrow / Click on the track | — | — |
| Scroll bar: scroll to where the thumb is dragged | — | ⏳ | Drag the thumb | — | Drag the thumb | — | — |
| Back / forward in the directory history | — | ⏳ | — | Click on < / > in the panel's frame | — | Back button / Forward button | — |
| Go up to a directory on the left, on the row clicked | — | ⏳ | — | — | — | Click in the parent column | Click in the parent column |
| Enter the directory previewed, on the row clicked | — | ⏳ | — | — | — | Click in the preview | Click in the preview |
| Scroll the preview | — | ⏳ | — | — | — | Wheel on the preview | Wheel on the preview of a file |
| The next / previous directory beside this one | — | ⏳ | — | — | — | — | Wheel on the parent column |
| The next / previous tab | — | ⏳ | — | — | — | — | Wheel on the tab bar |
| Go to a directory of the path in the title bar | — | ⏳ | — | — | — | — | Click on it |
| Scroll five times as far | — | ⏳ | — | — | — | — | Ctrl+Wheel |
| Resize the columns | — | ⏳ | — | — | — | Drag a border of the list | — |

## Editor (mc, Far)

noc edits files in `$VISUAL` or `$EDITOR`.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Character left / right | — | ⏳ | — | Left / Right | Left / Right | — | — |
| Character left without wrapping to previous line | — | ⏳ | — | — | Ctrl+s | — | — |
| Line up / down | — | ⏳ | — | Up / Down | Up / Down | — | — |
| Word left / right | — | ⏳ | — | Ctrl+Left / Ctrl+Right | Ctrl+Left / Ctrl+Right | — | — |
| Scroll screen up / down | — | ⏳ | — | Ctrl+Up / Ctrl+Down | Ctrl+Up / Ctrl+Down | — | — |
| Page up / down | — | ⏳ | — | PgUp / PgDn | PgUp / PgDn | — | — |
| Beginning / end of line | — | ⏳ | — | Home / End | Home / End | — | — |
| Beginning of file | — | ⏳ | — | Ctrl+Home<br>Alt+< | Ctrl+Home<br>Ctrl+PgUp | — | — |
| End of file | — | ⏳ | — | Ctrl+End<br>Alt+> | Ctrl+End<br>Ctrl+PgDn | — | — |
| Beginning / end of screen | — | ⏳ | — | Ctrl+PgUp / Ctrl+PgDn | Ctrl+n / Ctrl+e | — | — |
| Delete character | — | ⏳ | — | Delete<br>Ctrl+d | Delete | — | — |
| Delete character left | — | ⏳ | — | Backspace<br>Ctrl+h | Backspace | — | — |
| Delete line | — | ⏳ | — | Ctrl+y | Ctrl+y | — | — |
| Delete to end of line | — | ⏳ | — | Ctrl+k | Ctrl+k | — | — |
| Delete word left | — | ⏳ | — | Alt+Backspace | Ctrl+Backspace | — | — |
| Delete word right | — | ⏳ | — | Alt+d | Ctrl+t<br>Ctrl+Delete | — | — |
| Block selection | — | ⏳ | — | Shift+cursor keys<br>Ctrl+Shift+cursor keys<br>F3 (start, then end) | Shift+cursor keys<br>Ctrl+Shift+cursor keys | — | — |
| Vertical block | — | ⏳ | — | Alt+cursor keys<br>Shift+F3 (start, then end) | Alt+cursor keys (not on the numpad)<br>Alt+Shift+cursor keys<br>Ctrl+Alt+cursor keys (not on the numpad) | — | — |
| Select all text | — | ⏳ | — | — | Ctrl+a | — | — |
| Clear selection | — | ⏳ | — | — | Ctrl+u | — | — |
| Copy block to clipboard | — | ⏳ | — | Ctrl+Insert (to mcedit.clip) | Ctrl+Insert<br>Ctrl+c | — | — |
| Paste from clipboard | — | ⏳ | — | Shift+Insert | Shift+Insert<br>Ctrl+v | — | — |
| Cut to clipboard | — | ⏳ | — | Shift+Delete | Shift+Delete<br>Ctrl+x | — | — |
| Append block to clipboard | — | ⏳ | — | — | Ctrl+Numpad+ | — | — |
| Delete block | — | ⏳ | — | F8 | Ctrl+d | — | — |
| Copy block to cursor position | — | ⏳ | — | F5 | Ctrl+p (persistent blocks) | — | — |
| Move block to cursor position | — | ⏳ | — | F6 | Ctrl+m (persistent blocks) | — | — |
| Indent block left / right | — | ⏳ | — | — | Alt+u / Alt+i | — | — |
| Format the paragraph | — | ⏳ | — | Alt+p | — | — | — |
| Help | — | ⏳ | — | F1 | F1 | — | — |
| Save | — | ⏳ | — | F2 | F2 | — | — |
| Save as | — | ⏳ | — | Shift+F2<br>Ctrl+F2 | Shift+F2 | — | — |
| New file | — | ⏳ | — | Ctrl+n | Shift+F4 | — | — |
| Switch to viewer | — | ⏳ | — | — | F6 | — | — |
| Search | — | ⏳ | — | F7 | F7 | — | — |
| Replace | — | ⏳ | — | F4 | Ctrl+F7 | — | — |
| Continue search/replace forward / backward | — | ⏳ | — | Shift+F7 (search, forward)<br>Shift+F4 (replace, forward) | Shift+F7 / Alt+F7 | — | — |
| Code page | — | ⏳ | — | Alt+e | F8 (OEM/ANSI)<br>Shift+F8 (select) | — | — |
| Go to line and position | — | ⏳ | — | Alt+l | Alt+F8 | — | — |
| Far window size | — | ⏳ | — | — | Alt+F9 | — | — |
| Editor settings | — | ⏳ | — | — | Alt+Shift+F9 | — | — |
| Quit | — | ⏳ | — | F10<br>Esc | F10<br>F4<br>Esc | — | — |
| Save and quit | — | ⏳ | — | — | Shift+F10 | — | — |
| Locate file in panel | — | ⏳ | — | — | Ctrl+F10 | — | — |
| Plugin commands | — | ⏳ | — | — | F11 | — | — |
| View and edit history | — | ⏳ | — | Alt+E | Alt+F11 | — | — |
| Undo | — | ⏳ | — | Ctrl+u | Alt+Backspace<br>Ctrl+z | — | — |
| Redo | — | ⏳ | — | Alt+r | Ctrl+Z | — | — |
| Lock editing | — | ⏳ | — | — | Ctrl+l | — | — |
| User screen | — | ⏳ | — | Ctrl+o | Ctrl+o<br>Ctrl+Alt+Shift (temporarily) | — | — |
| Treat next key as character code | — | ⏳ | — | Ctrl+q | Ctrl+q | — | — |
| Set bookmark | — | ⏳ | — | Alt+k (on the line, or off) | RightCtrl+0…9<br>Ctrl+Shift+0…9 | — | — |
| Go to bookmark | — | ⏳ | — | Alt+j (next)<br>Alt+i (previous) | LeftCtrl+0…9 | — | — |
| Insert current panel file name | — | ⏳ | — | — | Shift+Enter | — | — |
| Insert passive panel file name | — | ⏳ | — | — | Ctrl+Shift+Enter | — | — |
| Insert full name of edited file | — | ⏳ | — | — | Ctrl+f | — | — |
| Functional key bar | — | ⏳ | — | — | Ctrl+b | — | — |
| Status line | — | ⏳ | — | — | Ctrl+B | — | — |
| Record macro | — | ⏳ | — | Ctrl+r (start/stop) | Ctrl+.<br>Ctrl+Shift+. (special mode) | — | — |
| Run macro | — | ⏳ | — | The key assigned to it | — | — | — |

## Directory tree (mc, Far)

mc's directory tree, from the Command menu, and Far's tree panel, Ctrl+t in the panels.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Rescan directory (Far: the whole tree) | — | ⏳ | — | Ctrl+r<br>F2 | Ctrl+r | — | — |
| Forget directory from tree | — | ⏳ | — | F3 | — | — | — |
| Static/dynamic navigation | — | ⏳ | — | F4 | — | — | — |
| Copy / move directory | — | ⏳ | — | F5 / F6 | — | — | — |
| Delete directory | — | ⏳ | — | F8<br>Delete | — | — | — |
| Search next match | — | ⏳ | — | Ctrl+s<br>Alt+s | Ctrl+Enter | — | — |
| The next / previous branch on the same level | — | ⏳ | — | — | Numpad+ / Numpad- | — | — |
| Delete last search character | — | ⏳ | — | Ctrl+h<br>Backspace | — | — | — |
| Help | — | ⏳ | — | F1 | — | — | — |
| Exit without changing directory | — | ⏳ | — | Esc<br>F10 | — | — | — |

## Diff viewer (mc)

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Help | — | ⏳ | — | F1 | — | — | — |
| Save changes | — | ⏳ | — | F2 | — | — | — |
| Edit left file | — | ⏳ | — | F4 | — | — | — |
| Edit right file | — | ⏳ | — | Shift+F4 | — | — | — |
| Merge current hunk | — | ⏳ | — | F5 (left to right)<br>Shift+F5 (right to left) | — | — | — |
| Search | — | ⏳ | — | F7 | — | — | — |
| Continue search | — | ⏳ | — | Shift+F7 | — | — | — |
| Quit | — | ⏳ | — | F10<br>Esc<br>q<br>Q | — | — | — |
| Hunk status | — | ⏳ | — | Alt+s<br>s | — | — | — |
| Line numbers | — | ⏳ | — | Alt+n<br>l | — | — | — |
| Maximize left panel | — | ⏳ | — | f | — | — | — |
| Equalize panel widths | — | ⏳ | — | = | — | — | — |
| Shrink right / left panel | — | ⏳ | — | > / < | — | — | — |
| Tab size | — | ⏳ | — | 2, 3, 4, 8 | — | — | — |
| Swap panels | — | ⏳ | — | Ctrl+u | — | — | — |
| Refresh screen | — | ⏳ | — | Ctrl+r | — | — | — |
| Show command screen | — | ⏳ | — | Ctrl+o | — | — | — |
| Next hunk | — | ⏳ | — | Enter<br>Space<br>n | — | — | — |
| Previous hunk | — | ⏳ | — | Backspace<br>p | — | — | — |
| Go to line | — | ⏳ | — | g<br>G | — | — | — |
| Line down / up | — | ⏳ | — | Down / Up | — | — | — |
| Page up / down | — | ⏳ | — | PgUp / PgDn | — | — | — |
| Beginning of line | — | ⏳ | — | Home | — | — | — |
| Beginning of file | — | ⏳ | — | Ctrl+Home | — | — | — |
| End of file | — | ⏳ | — | Ctrl+End | — | — | — |

## Sort modes menu (Far)

Ctrl+F12 in the panels.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Ascending / descending / invert | — | ⏳ | — | — | + / - / * | — | — |
| Criteria: ascending / descending / invert | — | ⏳ | — | — | + / - / * | — | — |
| Additional criteria | — | ⏳ | — | — | F4 | — | — |
| Criteria: add / remove / replace | — | ⏳ | — | — | Insert / Delete / F4 | — | — |
| Criteria: inherit order from sort mode | — | ⏳ | — | — | = | — | — |
| Criteria: move up / down | — | ⏳ | — | — | Ctrl+Up / Ctrl+Down | — | — |
| Criteria: reset | — | ⏳ | — | — | Ctrl+r | — | — |

## Screen grabber (Far)

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Start | — | ⏳ | — | — | Alt+Insert | — | — |
| Stream/block mode | — | ⏳ | — | — | Space | — | — |
| Move the cursor | — | ⏳ | — | — | Cursor keys<br>Click | — | — |
| Selection | — | ⏳ | — | — | Shift+cursor keys<br>Drag | — | — |
| Resize selected area | — | ⏳ | — | — | Alt+Shift+cursor keys | — | — |
| Move selected area | — | ⏳ | — | — | Alt+cursor keys | — | — |
| Copy to clipboard | — | ⏳ | — | — | Enter<br>Ctrl+Insert<br>Right click<br>Double click | — | — |
| Append to clipboard | — | ⏳ | — | — | Ctrl+Numpad+ | — | — |
| Cancel | — | ⏳ | — | — | Esc | — | — |
| Select whole screen | — | ⏳ | — | — | Ctrl+a | — | — |
| Clear selection | — | ⏳ | — | — | Ctrl+u | — | — |
| Selection horizontally by 10 | — | ⏳ | — | — | Ctrl+Shift+Left / Ctrl+Shift+Right | — | — |
| Selection vertically by 5 | — | ⏳ | — | — | Ctrl+Shift+Up / Ctrl+Shift+Down | — | — |

## Task list (Far)

Ctrl+w in the panels.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Kill task | — | ⏳ | — | — | Delete | — | — |
| Refresh list | — | ⏳ | — | — | Ctrl+r | — | — |
| Window title / module path | — | ⏳ | — | — | F2 | — | — |

## Spot (yazi)

Tab in the panels: what yazi knows of the file under the cursor, in a table.

| Description | Action | Status | noc | mc | far | yazi | ranger |
| --- | --- | :-: | --- | --- | --- | --- | --- |
| Close the spot | — | ⏳ | — | — | — | Esc<br>Ctrl+[<br>Ctrl+c<br>Tab | — |
| One line up | — | ⏳ | — | — | — | k<br>Up | — |
| One line down | — | ⏳ | — | — | — | j<br>Down | — |
| The previous file | — | ⏳ | — | — | — | h<br>Left | — |
| The next file | — | ⏳ | — | — | — | l<br>Right | — |
| Copy the cell under the cursor | — | ⏳ | — | — | — | c c | — |
