# 0033. Letters in dialogs

- Status: Accepted
- Date: 2026-10-16
- Amends: [ADR 0022](0022-vim-modes.md), [ADR 0027](0027-search-of-a-whole-tree.md),
  [ADR 0030](0030-search-modes.md)

## Context

Dialogs took keys only through their focus: Tab, `j`, `k`, and the arrows moved it, and Space or
Enter acted on what had it. Switching the second check box of a dialog, or pressing a button that is
not the default, took a key for each element before it. Far and mc mark a letter in the label of
each element of a dialog, highlighted, and Alt and that letter act on the element from anywhere in
the dialog, from a text field too; mc takes the letter alone too while the focus is not in a text
field. The pull-down menu of Noon Commander already took its letters as mc's menus do, with Alt,
so that its plain letters stay vim's motions.

The search of a tree was the one dialog with Alt keys of its own, taken from VS Code: Alt+r for
regular expressions and Alt+c for case ([ADR 0027](0027-search-of-a-whole-tree.md)). Alt+r became
the key of the search mode in quick search and the filters of the lists too
([ADR 0030](0030-search-modes.md)), where an Alt key fits badly: without the kitty keyboard
protocol, an Esc and a quick `r` arrive as Alt+r, so Esc then `r` switched the mode rather than
give the search up and rename.

In dialogs, as everywhere in Insert and Visual mode, Alt and a key stood for Esc and the key
([ADR 0022](0022-vim-modes.md)), so no Alt key there was free for a letter.

## Decision

- **Every element of a dialog but Cancel has a letter**, marked with `&` in its Fluent text and
  drawn apart, in the theme's `dialog_hotkey`, or `error_hotkey` in the red dialogs, as mc's
  `dhotnormal` and `errdhotnormal`, and on what has the focus in `dialog_hotkey_focused` and
  `error_hotkey_focused`, as mc's `dhotfocus` and `errdhotfocus`. Without being bold, which drew
  the eye from the labels: in the color of titles (the Noon themes' gold, Catppuccin's mauve), and
  underlined on the cursor, where that color would not show, and in the red dialogs, where it
  would read poorly; a letter in the color of its text, as on a title, is underlined. The menus
  of F9 take the same, with `menu_hotkey_selected` on the cursor and the menu bar. Alt and the
  letter act on the element from anywhere in the dialog, while typing in a field too:

  | Element | Alt and its letter |
  | --- | --- |
  | Text field | puts the focus in it, in Insert mode, as Tab does |
  | Check box | switches it, and puts the focus on it |
  | Radio button | chooses it, and puts the focus on it |
  | Button | presses it |

  Cancel needs none: Esc, Ctrl+c, `q`, and F10 close dialogs, as before.
- **The one field of a form, which has no label, takes its letter from the prompt over it**, in
  Copy, Move, Create a new directory, Quick cd, Save workspace, Rename workspace, and Save
  checksums, as `&Copy "x" to:` and `&Enter directory name:`. What ssh asks, a password or a
  host key, is ssh's text and has no letter; the buttons of its dialogs do.
- **Text the user named never makes a letter**: the `&` of the names of files, paths, workspaces,
  and hosts is doubled, `&&`, before they go into a label, and shows as one `&`.
- **The windows that have letters**: every dialog of `tui/dialog.rs` (the questions, the errors,
  the forms, the host settings, the checksum dialog, and the search of a tree), the window of a
  job, the list of the jobs, the results of the checksums, and the Configuration dialog. There,
  Alt+1 … Alt+7 open its categories, Interface, Transfers, SSH, Volumes, zoxide, Command line, and
  Search, the digit shown in front of each; each setting has a letter, and Alt and it do what
  Space or Enter does on its row, switching a check box or picking the next choice, or put the
  focus in its text field, in Insert mode. Letters are unique within a page.
- **The sections of the help have letters too**, as the dialogs' elements: About, Keys, and each
  group under Keys. Alt and a section's letter show it, opening Keys for a group, and give its
  text the keys, to scroll it; the help's plain letters stay its keys.
- **Letters are unique within a dialog**, and among the sections of the help; tests check it.
- **Alt and a letter are the dialog's**, in every mode, in the contexts `dialog`, `dialog_input`,
  `path_input`, `completion`, `find`, `find_input`, and `help`, as they are the pull-down menu's in
  `pull_down`. There Alt and a key no longer stand for Esc and the key; Alt and a letter that no
  element has do nothing, as in the pull-down menu. The keymap hands the letter to the app as
  `Resolved::Hotkey`, which the preset does not list: `Context::alt_letters_are_hotkeys` asks for
  it, for the pull-down menu too. One `Label` (`tui/label.rs`) reads the `&` of the menus and of
  the dialogs.
- **Ctrl+s switches the search mode**, in place of Alt+r, wherever Alt+r did: in quick search, in
  the filters of the lists, in Normal and Insert mode, and in the search of a tree, wherever its
  focus is. vim binds no Ctrl+s in Normal mode, in Insert mode, or on its command line: terminals
  stop their output with it (XOFF), which raw mode turns off, so the key arrives, as a byte of its
  own without the kitty keyboard protocol too. Ctrl+x is vim's to subtract in Normal mode and to
  start completion in Insert mode, as Ctrl+x Ctrl+f completes file names, which the fields of
  paths may want. Ctrl+Shift+s arrives as Ctrl+s without the protocol, and kitty takes it for
  itself, to paste from the selection. mc's Ctrl+s starts quick search and goes to its next match;
  Noon Commander keeps only mc's F-keys, so in its quick search Ctrl+s switches the mode, and
  Ctrl+g goes to the next match, as on vim's command line.
- **The search of a tree loses its Alt keys.** Its check box of case is labelled `Match &case`, so
  Alt+c still switches it, as its letter rather than a binding, and the action `match_case` goes.
  The check box of regular expressions does not come back: Ctrl+s switches them, and the label
  `Text .*:` says which.

## Consequences

- Every element of a dialog is one key away, from anywhere in it, as in Far and mc.
- Without the kitty keyboard protocol, an Esc and a quick letter in a dialog arrive as Alt and the
  letter, and act on its element rather than leave Insert mode and do what the letter does. An Esc
  typed slowly before the letter still leaves Insert mode; with the protocol, Esc and the letter
  always arrive apart.
- Alt and a key no longer leave Insert mode in dialogs, as ADR 0022 had them do; they still do in
  quick search, the field of a rename, the filters of the lists, and on the command line.
- Alt+r in quick search and the lists is an Alt key that nothing binds again, as any other there:
  in quick search it gives the search up, then `r` renames in the panel, as Esc then `r` would.
- The keymap binds no Alt key in dialogs: their Alt keys are their letters, which change with the
  language and with the dialog.
- Translations must mark a letter on every element, unique within each dialog, and within each page
  of the Configuration dialog.
