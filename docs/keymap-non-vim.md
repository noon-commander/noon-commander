# Keys that are not vim's

The keymap is vim's, in its modes, with mc's F-keys ([ADR 0022](adr/0022-vim-modes.md)). This
file lists every key of [keymap/preset.rs](../crates/noc/src/tui/keymap/preset.rs) that does not
come from vim, and why it is there: the program it comes from, or noc's own reason, and what vim
does with the key in that place; which keys of vim's kind do the same; and what the key does in a
terminal without the kitty keyboard protocol. [keymap-compare.md](keymap-compare.md) has every
key, beside those of mc, Far, yazi, and ranger.

Change this file with the preset: a key that is not vim's gets its row here in the same change,
and the row goes with the key; a change of the keys of an action changes its vim-style keys too.
The sections follow the preset, one per context and mode, named as in keymap-compare.md, and merged
where their keys and reasons are the same; a row has only the keys of its action that are not
vim's. Keys are written as `keymap::describe` writes them.

## What counts as vim's

A key is vim's where vim, or nvim, binds it in that mode to the same command, or to what that
command is in a list of files:

- Motions move over rows as over lines: `j`, `k`, `G`, `g g`, the arrows, the page keys, Ctrl+d,
  Ctrl+u, Ctrl+o, and the `z` keys; `/`, `?`, `n`, and `N` search the names, `/` the filter of a
  list too; `v` and `V` select rows.
- Commands of lines act on rows: `y y` and `Y` copy, as vim's yank lines; `d d` removes a row of
  a list, as vim's deletes a line; `K` shows the whole name under the cursor, as vim's looks up
  what is under it; `g x` opens it; `g f` views it, as vim's goes to the file whose name is under
  the cursor; `!` opens the command line for a shell command, as vim's starts `:.!`; `Z Z` quits.
- Ctrl+w acts on the panels as vim's on windows: Ctrl+w f opens the directory under the cursor in
  the other panel, as vim's opens the file under it in a new window, and Ctrl+w v shows this
  directory there, as vim's splits the window on the same file.
- Esc and Ctrl+c leave a mode and give a command up, as in vim, and in noc close what is in front
  too. Enter takes what was typed, as on vim's command line, or what is under the cursor, as in
  netrw and the quickfix list; Ctrl+y too in the lists and menus, as in vim's pop-up menu. Ctrl+[
  and Ctrl+m are bound wherever Esc and Enter are, and Ctrl+i wherever Tab is but in the panels;
  they go with those keys here.
- Text fields, quick search, and the command line take the keys of vim's Insert, Normal, and
  Visual mode, of its command line and the window of it, and of its pop-up menu. Alt and a key
  stand for Esc and the key, as in nvim, but in dialogs, where Alt and a letter are the dialog's
  letters ([ADR 0033](adr/0033-letters-in-dialogs.md)).

Every other key is here: keys that vim does not bind, and keys that it binds to something that a
listing, a dialog, or a pager has not. These contexts have only vim's keys, and no section:
`input` and `rename` in every mode, `path_input`, `completion`, `jump`, `key_hints`, and
`user_screen`; the letters of dialogs, which `path_input` and `completion` take too, are under
`dialog`. The mouse is not in the keymap ([ADR 0018](adr/0018-mouse-support.md));
keymap-compare.md lists what it does.

## Vim-style keys

The column Vim-style keys says which keys of vim's kind do what the row's keys do, in the same
context and mode. Keys of vim's kind are those that vim's Normal mode takes: letters, digits,
symbols, and Space; Ctrl and a letter; Esc, Enter, and Backspace; and sequences of them. The F-keys,
Tab, the arrows, the page keys, Home, End, Delete, Insert, and Alt with any key, or Shift with one
that is not a letter, are not. Ctrl+[, Ctrl+m, and Ctrl+i go with Esc, Enter, and Tab.

- After each key, in brackets, where it comes from: vim, for vim's own keys, which this file lists
  nowhere else; otherwise the program, or noc, as the key's own row says.
- A row whose keys are all of vim's kind lists them again: they are their own vim-style keys.
- — means none: only the row's keys do it there. Keys that end a mode and act, such as the F-keys
  in Visual mode and quick search, have none, though Esc or Enter and then a key of the panel do it
  in two.
- A row with several actions has a line for each key, as the Action column does.

## Without the kitty keyboard protocol

noc asks for the kitty keyboard protocol where the terminal speaks it, and the preset lists the
keys of such a terminal ([ADR 0019](adr/0019-shell-command-line.md),
[ADR 0022](adr/0022-vim-modes.md)). Elsewhere some keys arrive as others: Ctrl+i as Tab, Ctrl+[ as
Esc, Ctrl+m and Shift+Enter as Enter; and an Esc with a key typed quickly after it arrives as Alt
and the key. The About section of the help, and `noc debug version`, say which keyboard the
terminal has.

The column Without kitty says what a key does there, where that is something else; Same marks the
others. A row whose keys differ has a line for each key.

- Ctrl+[ and Ctrl+m are bound wherever Esc and Enter are, so they lose nothing. Ctrl+i is bound
  wherever Tab is but in the panels; there the keymap drops the bindings of Tab and gives it those
  of Ctrl+i, so Tab differs only there.
- An Alt key that nothing binds stands for Esc and the key either way, but in dialogs and the
  pull-down menu, where Alt and a letter are their letters, and an Esc and a quick letter act on
  what the letter marks. Where an Alt key is bound itself, an Esc and a quick key do what the Alt
  key does, rather than what Esc and then the key would.

## noc's own groups

The panels group keys of noc's own under a first key, as vim groups its own under `g`, `z`, and
Ctrl+w. Each takes a letter whose command in vim works in the text of a line, which a listing has
not:

- `t`: the tabs and the workspaces. vim's `t` goes to before a character in the line.
- `o`: the order of the rows, as ranger's sort keys, and the hidden names. vim's `o` opens a line
  below.
- `f`: the file operations. It stands for mc's Ctrl+x and keeps its letters for mc's commands:
  `c` chmod, `o` chown, `e` chattr, `l`, `s`, and `v` links, `j` the jobs. mc has no checksums, so
  they take `#`, for a hash; the search of the files below takes `/` and `f`, which mc's Ctrl+x
  does not use either. vim's `f` goes to a character in the line.

## Everywhere (`global`, every mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Ctrl+k | `key_hints` | noc | Ctrl+k (noc) | Same | The keys that can be typed now, in every mode and every terminal. vim's Normal mode binds no Ctrl+k, and noc has not the digraphs that vim's Insert mode types with it. It arrives as a byte of its own without the kitty keyboard protocol, and is no prefix of tmux or screen. It replaced Alt+/, which an Esc and a quick `/` arrived as, so that Esc then `/` showed the hints rather than start quick search ([ADR 0022](adr/0022-vim-modes.md)). Again while the hints show, it gives them the keys, as `K K` gives nvim's hover window the keys. |

## Panels (`panel`, Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| l<br>Right | `enter` | ranger, yazi | Enter (vim); l (ranger, yazi) | Same | Open the directory under the cursor, as in ranger and yazi: a row has no column for vim's l and Right to go to, so they go into the tree. |
| h<br>Left<br>Backspace<br>Ctrl+h | `parent` | ranger, yazi, noc | h (ranger, yazi); Backspace, Ctrl+h (noc) | Same | The parent directory, where h and Left go in ranger and yazi. vim's Backspace and Ctrl+h go left as its h does, so they go up with it; some terminals send Ctrl+h for Backspace. |
| R | `reload` | ranger | R (ranger) | Same | Read the directory again, as ranger's R does. vim's R starts Replace mode, which a listing has not. |
| g m<br>F9 | `pull_down` | noc, mc | g m (noc) | Same | F9 is mc's key for the pull-down menu. g m is its key among vim's g keys, m for menu; vim's g m goes to the middle of the screen line. |
| g M<br>Shift+F9 | `pull_down_last` | noc, mc | g M (noc) | Same | The menu on the command that ran from it last, as Far's Shift+F10, so that Shift+F9 Enter runs it again; Shift+F9 is mc's key for its menu where it was left. g M goes with g m; vim's g M goes to the middle of the text of the line. |
| Tab | `switch_panel` | mc, Far | Ctrl+w w, Ctrl+w Ctrl+w, Ctrl+w p (vim) | What Ctrl+i does: forward through the jump list | The other panel, as mc's and Far's Tab. Only the kitty keyboard protocol tells it from Ctrl+i, vim's key forward through the jump list, which noc keeps ([ADR 0022](adr/0022-vim-modes.md)). |
| Ctrl+x | `other_panel` | noc | Ctrl+x (noc) | Same | Before a command, as a register goes before it: the command runs in the other panel, which has the keys until it ends, then they come back, as vim's Ctrl+o in Insert mode runs one command of Normal mode ([ADR 0051](adr/0051-ctrl-x-sends-a-command-to-the-other-panel.md)). It takes any command, as if the other panel were active, with its count after it (Ctrl+x 2g t); the line of the mode starts with (^X) while the command has the keys. Esc gives it up, and so does Ctrl+x again. Without it, Tab or Ctrl+w w, the command, and Tab or Ctrl+w w again do the same. x as in crossing over to the other side; vim's Ctrl+x subtracts from the number under the cursor, which a listing has not. Ctrl+w, vim's for windows, cannot lead it, as Ctrl+w f and Ctrl+w v end their sequences. It arrives as a byte of its own without the kitty keyboard protocol too, and is no prefix of tmux or screen. mc's Ctrl+x leads its commands of files, which noc keeps under f; after Ctrl+x their letters do what they do in the other panel. |
| t n<br>t c | `new_tab`<br>`close_tab` | noc | t n (noc); g n (ranger)<br>t c (noc); g c (ranger) | Same | Group `t`: n for new, c for close. |
| g n<br>g c | `new_tab`<br>`close_tab` | ranger | t n (noc); g n (ranger)<br>t c (noc); g c (ranger) | Same | ranger's keys for a new tab and to close one. vim's g n selects the next match of the search; vim binds no g c, which nvim comments with. |
| ] t<br>[ t | `next_tab`<br>`prev_tab` | yazi | g t (vim); ] t (yazi)<br>g T (vim); [ t (yazi) | Same | yazi goes through the tabs with ] and [; t says what to go to, as in vim's pairs of ] and [ (] s, ] c). vim binds no ] t; nvim 0.11 goes through tags with it. |
| t t | `tab_list` | noc | t t (noc) | Same | Group `t`: the tabs of the panel, listed in a menu, as vim's `:tabs` lists them; Ctrl+x t t, the other panel's. yazi makes a new tab with t t, which t n does here. |
| t w<br>t W | `workspaces`<br>`save_workspace` | noc | t w (noc)<br>t W (noc) | Same | Group `t`: w for workspaces, and W saves one. |
| o h | `toggle_hidden` | noc | o h (noc) | Same | Group `o`, with what the rows show. ranger switches the hidden names with z h, which scrolls the names left here, as vim's does. |
| o n<br>o e<br>o m<br>o s | `sort_by_name`<br>`sort_by_extension`<br>`sort_by_time`<br>`sort_by_size` | ranger | o n (noc)<br>o e (ranger)<br>o m (ranger)<br>o s (ranger) | Same | Group `o`: ranger's sort keys o e, o m, and o s; n for name, where ranger has o b, for basename. |
| Space | `mark` | yazi, ranger | Space (yazi, ranger) | Same | Mark and go down, as in yazi and ranger, leaving v to Visual mode. vim's Space goes right as l does. |
| Shift+Down<br>Shift+Up | `mark`<br>`mark_up` | mc, Far | Space (yazi, ranger)<br>— | Same | Mark and go down or up, as in mc and Far, so that K keeps vim's meaning: the whole name under the cursor ([ADR 0025](adr/0025-marks-of-text-cut-short.md)). vim's Shift+Down and Shift+Up turn the page, as Ctrl+f and Ctrl+b do here. |
| g +<br>g -<br>g \* | `select`<br>`unselect`<br>`invert_marks` | mc | g + (mc)<br>g - (mc)<br>g \* (mc) | Same | mc's +, -, and \*, under g: alone they are vim's line motions and search, which stay unbound rather than mark where a vim user moves. vim's g + and g - go through its undo tree and g \* searches for the word under the cursor, none of which a listing has ([ADR 0022](adr/0022-vim-modes.md)). |
| Shift+F7 | `next_match` | mc, Far | n (vim) | Same | Their key to search again. |
| f /<br>f f | `find_file`<br>`find_names` | noc | f / (noc)<br>f f (noc) | Same | Group `f`: search the files below for text, / as quick search's `/` ([ADR 0027](adr/0027-search-of-a-whole-tree.md)), or find them by name, in the Files and dirs mode of the same dialog, f f as telescope's `<leader>ff` finds files ([ADR 0029](adr/0029-files-and-directories-with-fd.md)). |
| i | `shell` | noc | i (noc); ! (vim) | Same | Open the command line for a shell command, in Insert mode, as typing in mc's panels goes into its command line. vim's i starts Insert mode, which a listing has not; this i starts it where there is text to type. |
| g z | `jump` | noc | g z (noc) | Same | z for zoxide, whose command is z. vim binds no g z. |
| g d | `go_to_dir` | noc | g d (noc) | Same | d for directory: go to one typed as for cd. vim's g d goes to a declaration, which a listing has not. |
| g l | `location_menu` | noc | g l (noc) | Same | The location menu of the panel, its home, volumes, and hosts, as Far's Alt+F1 and Alt+F2 change the drive of a panel ([ADR 0021](adr/0021-location-menu-replaces-the-virtual-root.md)); Ctrl+x g l, the other panel's, and the keys come back ([ADR 0051](adr/0051-ctrl-x-sends-a-command-to-the-other-panel.md)). l for location, beside g d and g z, which also choose where the panel goes. vim binds no g l; ranger's resolves the symlinks of this directory. Alt+F1 and Alt+F2, which desktops take, are not bound ([ADR 0049](adr/0049-menus-of-a-panel-keep-the-active-panel.md)). |
| F3 | `view` | mc | g f (vim) | Same | mc's key to view. |
| e<br>F4 | `edit` | noc, mc | e (noc) | Same | e for edit, as ranger's E; F4 is mc's. vim's e goes to the end of a word. |
| F5 | `copy` | mc | Y, y y (vim) | Same | mc's key to copy. |
| y p<br>Shift+F5 | `duplicate` | noc, mc | y p (noc) | Same | Copy the entry under the cursor beside it under a new name, as vim's y y p puts a copy of a line under it; Shift+F5 is mc's and Far's key for it. vim's y waits for a motion, which p is not, so y p does nothing there. ranger copies the path with y p; paths to the clipboard, when noc has them, take yazi's c keys. |
| x<br>F6 | `move` | yazi, mc | x (yazi) | Same | yazi cuts with x to move; F6 is mc's. vim's x deletes the character under the cursor into a register. |
| r<br>Shift+F6 | `rename` | yazi, mc | r (yazi) | Same | yazi renames with r, and mc with Shift+F6. vim's r replaces the character under the cursor; this r replaces the name, in its row. |
| A<br>F7 | `mkdir` | noc, mc | A (noc) | Same | A adds a directory, as yazi's a adds an entry; F7 is mc's. vim's A appends at the end of the line. |
| d D<br>F8 | `delete` | ranger, mc | d D (ranger) | Same | ranger's d D; F8 is mc's. A lone d waits for what follows, as vim's operator does, and d d is kept for the trash ([roadmap](roadmap.md)). |
| f #<br>f j | `checksum`<br>`jobs` | noc, mc | f # (noc)<br>f j (mc) | Same | Group `f`: # for a hash, which mc has not, and j, mc's Ctrl+x j. |
| g ?<br>F1 | `help` | noc, mc | g ? (noc) | Same | F1 is mc's. ? is ranger's help, and alone vim's search up, so help goes under g, where vim's g ? encodes text in rot13. |
| F10 | `quit` | mc | Z Z (vim) | Same | mc's key to quit. |

## Panels (`panel`, Visual mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Space | `mark` | yazi, ranger | Space (yazi, ranger) | Same | Mark the rows selected, beside the marks already there, and leave Visual mode, as Space marks in Normal mode. |
| F5 | `copy` | mc | y (vim) | Same | As in Normal mode, on the rows selected. |
| x<br>F6 | `move` | yazi, mc | x (yazi) | Same | As in Normal mode, on the rows selected. vim's x deletes what Visual mode selects. |
| F8 | `delete` | mc | d, D, X (vim) | Same | As in Normal mode, on the rows selected. |
| f # | `checksum` | noc | f # (noc) | Same | As in Normal mode, on the rows selected. |
| F1<br>F3<br>F4<br>Shift+F5<br>Shift+F6<br>F7<br>Shift+F7<br>F9<br>Shift+F9<br>F10 | `help`<br>`view`<br>`edit`<br>`duplicate`<br>`rename`<br>`mkdir`<br>`next_match`<br>`pull_down`<br>`pull_down_last`<br>`quit` | mc | — | Same | mc's F-keys work in Visual mode too ([ADR 0022](adr/0022-vim-modes.md)). These do not act on the rows selected, so they end Visual mode, then do what they do in Normal mode. |
| Tab | `switch_panel` | mc, Far | Ctrl+w w, Ctrl+w Ctrl+w, Ctrl+w p (vim) | Nothing, as Ctrl+i in Visual mode, as in vim | As in Normal mode; it ends Visual mode first. |

## Quick search (`quick_search`, Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Up<br>Down<br>PgUp<br>PgDn | `up`<br>`down`<br>`page_up`<br>`page_down` | mc | — | Same | End the search and move in the panel, as in mc. On vim's command line they go through the history. |
| F1<br>F3<br>F4<br>F5<br>Shift+F5<br>F6<br>Shift+F6<br>F7<br>F8<br>F9<br>Shift+F9<br>F10 | `help`<br>`view`<br>`edit`<br>`copy`<br>`duplicate`<br>`move`<br>`rename`<br>`mkdir`<br>`delete`<br>`pull_down`<br>`pull_down_last`<br>`quit` | mc | — | Same | End the search and do what they do in the panel, as in mc ([ADR 0022](adr/0022-vim-modes.md)). |
| Shift+F7 | `next_match` | mc, Far | Ctrl+g (vim) | Same | The next match, with their key to search again. |

## Dialogs and help (`dialog`, Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Tab<br>Ctrl+i | `next_field` | mc | j, Ctrl+n (vim), but on a list and in the help | Same | The next field or button, as in mc's dialogs. vim's Tab is Ctrl+i, which goes forward through the jump list, which a dialog has not. |
| Shift+Tab | `prev_field` | mc | k, Ctrl+p (vim), but on a list and in the help | Same | The field or button before, as in mc's dialogs. |
| Space | `toggle` | mc | Space (mc) | Same | Switch the check box, choose the radio button, or press the button, as in mc's dialogs. vim's Space goes right. |
| q | `cancel` | less, ranger | Esc, Ctrl+c (vim); q (less, ranger) | Same | Close, as q quits less and closes ranger's windows. vim's q records a macro. |
| F10 | `cancel` | mc | Esc, Ctrl+c (vim); q (less, ranger) | Same | Cancel, as in mc's dialogs ([ADR 0022](adr/0022-vim-modes.md)). |
| Alt and a letter | — | Far, mc | — | Esc and a quick letter arrive as Alt and the letter too, and act on its element rather than leave Insert mode or close the dialog | Put the focus in a text field, in Insert mode, switch a check box, choose a radio button, or press a button, by the letter highlighted in its label, from anywhere in the dialog, as in Far's and mc's dialogs; with Alt, so that the plain letters stay vim's keys and text ([ADR 0033](adr/0033-letters-in-dialogs.md)). In every mode, in the text fields too (`dialog_input`, `path_input`, `completion`, `find`, `find_input`), where Alt and a key no longer stand for Esc and the key; the Configuration dialog takes Alt+1 … Alt+8 for its categories. Not in the preset: `Context::alt_letters_are_hotkeys` asks for it. |

## Help (`help`, Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Alt and a letter | — | Far, mc | — | Esc and a quick letter arrive as Alt and the letter too, and show its section rather than close the help | Show the section whose letter it is, as the letters of dialogs, `dialog` above, act on their elements, opening Keys for one of its groups; the text gets the keys, to scroll it ([ADR 0033](adr/0033-letters-in-dialogs.md)). With Alt, so that the plain letters stay the help's keys. Not in the preset: `Context::alt_letters_are_hotkeys` asks for it. |
| b<br>Space<br>f<br>u<br>d | `window_up`<br>`window_down`<br>`window_down`<br>`half_page_up`<br>`half_page_down` | less | b (less)<br>Space, f (less)<br>Space, f (less)<br>Ctrl+u (vim); u (less)<br>Ctrl+d (vim); d (less) | Same | A whole page up or down, and half of one, as in less and ranger's pager. vim's b, Space, f, u, and d go back a word, go right, find a character, undo, and delete, none of which the help has; Space presses no button here. |

## Text fields (`dialog_input`, Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Up<br>Down | `up`<br>`down` | mc, Far | — | Same | The field or button before and after, as in the dialogs of mc and Far. vim's Up and Down in Insert mode go to the line above and below. |
| Tab<br>Ctrl+i<br>Shift+Tab | `next_field`<br>`next_field`<br>`prev_field` | mc | — | Same | As in `dialog`. vim's Tab in Insert mode types a tab. |
| F10 | `cancel` | mc | Ctrl+c (vim) | Same | As in `dialog`. |

## Text fields (`dialog_input`, Visual mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| F10 | `cancel` | mc | Ctrl+c (vim) | Same | As in `dialog`. |

## Fields that search (`search_input`, Insert and Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Ctrl+s Ctrl+s<br>Ctrl+s s | `search_mode` | noc | Ctrl+s Ctrl+s (noc) | Same | Switch how the field with the focus matches, round through the ways it has: as it is, as fzf does, as a regular expression, or as a shell pattern ([ADR 0030](adr/0030-search-modes.md), [ADR 0045](adr/0045-ctrl-s-switches-the-field-typed-in.md), [ADR 0046](adr/0046-case-and-the-keys-of-search-fields.md)): quick search, the filter of a list while it takes text, the text of the search of a tree, and the pattern of g +; nowhere else, so that two such fields in one dialog never ask which. Ctrl+s leads every switch of the field, as g leads keys of the panels, so that each takes a letter rather than a key of its own: a text field in Insert mode has few keys left that vim does not bind. vim binds no Ctrl+s in Normal mode, in Insert mode, or on its command line; terminals stop their output with it (XOFF), which raw mode turns off, so it arrives, as a byte of its own without the kitty keyboard protocol too. Ctrl+x is vim's to subtract in Normal mode and to start completion in Insert mode, as Ctrl+x Ctrl+f completes file names; Ctrl+Shift+s arrives as Ctrl+s without the protocol, and kitty takes it to paste from the selection. mc's Ctrl+s searches again, which Ctrl+g does here, as on vim's command line. Ctrl+s replaced Alt+r, which an Esc and a quick r arrived as, so that Esc then r switched the mode rather than give the search up and rename ([ADR 0033](adr/0033-letters-in-dialogs.md)). |
| Ctrl+s p<br>Ctrl+s f<br>Ctrl+s r<br>Ctrl+s g | `search_plain`<br>`search_fzf`<br>`search_regex`<br>`search_glob` | noc | Ctrl+s p, f, r, g (noc) | Same | Match as it is, as fzf does, as a regular expression, or as a shell pattern, by its initial, if the field has that way; the key hints show only those it has. |
| Ctrl+s c | `case_mode` | noc | Ctrl+s c (noc) | Same | Switch whether case counts, round: smart case, case ignored, case. c for case. |
| Ctrl+s a<br>Ctrl+s i<br>Ctrl+s m | `case_smart`<br>`case_ignore`<br>`case_match` | noc | Ctrl+s a, i, m (noc) | Same | Smart case (a for auto: case counts once a capital is typed), ignore case, or match case, as ripgrep's `--smart-case`, `--ignore-case`, and `--case-sensitive` name them. vim's `\c` and `\C` say it in the pattern, which a field that takes names as text cannot. |

## Viewer (`viewer`, Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| b<br>Space<br>f<br>u<br>d | `window_up`<br>`window_down`<br>`window_down`<br>`half_page_up`<br>`half_page_down` | less | b (less)<br>Space, f (less)<br>Space, f (less)<br>Ctrl+u (vim); u (less)<br>Ctrl+d (vim); d (less) | Same | As in the help. |
| F2 | `toggle_wrap` | mc, Far | — | Same | Their key to wrap long lines, or cut them. |
| g ?<br>F1 | `help` | noc, mc | g ? (noc) | Same | As in the panels. |
| q<br>F3<br>F10 | `quit` | less, mc | Esc, Ctrl+c (vim); q (less) | Same | q quits less, mc's viewer, and ranger's pager; vim's q records a macro. F3 and F10 close mc's viewer. |

## Lists (`list`, Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| q<br>F10 | `cancel` | less, ranger, mc | Esc, Ctrl+c (vim); q (less, ranger) | Same | As in `dialog`; F10 closes mc's lists too. |

## Lists (`list`, Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| F10 | `cancel` | mc | Ctrl+c (vim) | Same | As in Normal mode. |

## Location menu (`location_menu`, Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| F4 | `edit_host` | mc | — | Same | Edit the settings of the host under the cursor, as F4 edits a file ([ADR 0007](adr/0007-typed-host-settings-in-hosts-toml.md), [ADR 0021](adr/0021-location-menu-replaces-the-virtual-root.md)). |
| F8 | `disconnect` | mc | d d (vim) | Same | Disconnect the host under the cursor, as F8 deletes. |
| R | `reload` | ranger | R (ranger) | Same | Read the volumes and hosts again, as R reads a directory again in the panels. |

## Location menu (`location_menu`, Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| F4<br>F8 | `edit_host`<br>`disconnect` | mc | — | Same | As in Normal mode, while the filter takes text. |

## Tab menu (`tab_menu`, Normal and Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| F8 | `close_tab` | mc | d d (vim), in Normal mode | Same | Close the tab under the cursor, as F8 deletes. |

## Workspaces window (`workspaces`, Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| a | `save_workspace` | noc | a (noc) | Same | Save the tabs of both panels as a new workspace, a for add. vim's a appends after the cursor. |
| r<br>F6 | `move` | yazi, mc | r (yazi) | Same | Rename the workspace, as r and F6 rename in the panels. |
| F8 | `delete` | mc | d d (vim) | Same | Delete the workspace, as F8 deletes files. |

## Workspaces window (`workspaces`, Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| F6<br>F8 | `move`<br>`delete` | mc | — | Same | As in Normal mode, while the filter takes text. |

## Pull-down menu (`pull_down`, Normal mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Shift+Tab<br>Tab<br>Ctrl+i | `left`<br>`right`<br>`right` | noc | h (vim)<br>l (vim)<br>l (vim) | Same | The menu to the left and the right, as Tab goes through the fields of a dialog. vim's Tab is Ctrl+i, which goes forward through the jump list. |
| q | `cancel` | less, ranger | Esc, Ctrl+c (vim); q (less, ranger) | Same | As in `dialog`. |
| F9<br>F10 | `cancel` | noc, mc | Esc, Ctrl+c (vim); q (less, ranger) | Same | F9 closes the menu, then the menu bar, as Esc does; F10 closes mc's menus. |
| Alt and a letter | — | mc | — | Esc and a quick letter arrive as Alt and the letter too, and run its command rather than close the menu | Open the menu, or run the command, whose letter it is, as mc's menus take the letter alone; with Alt, so that h, j, k, and l stay the motions ([ADR 0022](adr/0022-vim-modes.md)). Not in the preset: `Context::alt_letters_are_hotkeys` asks for it. |

## Command line (`command_line`, Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Ctrl+j<br>Shift+Enter | `new_line` | noc | Ctrl+j (noc) | Same<br>Shift+Enter arrives as Enter, and runs the command | A new line in the command, which may span lines ([ADR 0019](adr/0019-shell-command-line.md)). vim's command line has one line, and its Ctrl+j runs it, as Enter does. Ctrl+j arrives as LF where Enter arrives as CR, so it works in every terminal; Shift+Enter, only with the kitty keyboard protocol. |
| F10 | `abort` | mc | Ctrl+c (vim) | Same | Close the command line in any mode, as F10 cancels a dialog ([ADR 0022](adr/0022-vim-modes.md)). |

## Command line (`command_line`, Normal and Visual mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| F10 | `cancel` | mc | Ctrl+c (vim); Esc (vim), in Normal mode | Same | As in Insert mode. |

## Command history (`command_history`, Normal and Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| Tab<br>Ctrl+i | `next_field` | noc | — | Same | The commands of the panel's host, or of all hosts: the window's one switch, as Tab goes to the next field of a dialog. vim's Tab is Ctrl+i, which goes forward through the jump list. |
| F8 | `delete` | mc | d d (vim), in Normal mode | Same | Remove the command, as F8 deletes. |

## Search of a whole tree (`find`, Normal mode; `find_input`, Normal and Insert mode)

| Keys | Action | From | Vim-style keys | Without kitty | Why |
| --- | --- | --- | --- | --- | --- |
| F3<br>F4 | `view`<br>`edit` | mc, Far | — | Same | View the file of the row from its line, or edit it at the line, as in the results of mc's and Far's Find file. |
