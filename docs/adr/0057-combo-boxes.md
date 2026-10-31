# 0057. Combo boxes

- Status: Accepted
- Date: 2026-10-31
- Amends: [ADR 0018](0018-mouse-support.md), [ADR 0029](0029-files-and-directories-with-fd.md),
  [ADR 0033](0033-letters-in-dialogs.md)

## Context

Three places asked for one of a few choices, each its own way. The checksum dialog had five radio
buttons, one under another, five rows of a small dialog for one setting. The search of a whole
tree had two on a line, Text and Files and dirs, drawn apart from the rest of `Dialog`
(`in_a_row`). The Configuration dialog had `< choice >`, which Space, Enter, `h`, and `l` went
round without showing the other choices, so a theme was found by going through them.

Radio buttons take a row each, and as many letters as choices among those of the dialog; a list
of choices that grows, as the algorithms or the themes may, makes the dialog grow. Far has combo
boxes for this, and so have Qt (`QComboBox`) and GTK (`GtkComboBox`): a box with the choice and an
arrow, whose list opens under it. vim's pop-up menu, which the list of completions already copies,
shows how a list opens under a line and takes the keys.

"Drop-down" was the other name. The pull-down menu of F9 (`Context::PullDown`) is too close to it;
Far, Qt, and GTK say combo box, and a combo box may take typed text too, which some settings will
want.

## Decision

- **A combo box** (`ComboBox`, `tui/combo_box.rs`) shows its choice as `[SHA-256 ▼]`, after its
  label on its line, as wide as its widest choice, so that it does not move as the choice changes,
  in the colors of the focus while it has it, as a check box. The arrow is `▼`, Far's, as the
  scroll bars have it, or a Nerd Font chevron with `ui.icons`, from `tui/glyphs.rs`.
- **Its list opens under it**, or above it where there is more room, over the dialog and past its
  frame, as the list of completions (`dialog::pop_up`): a frame, at most ten rows, a scroll bar on
  the frame when they do not all fit, the choice marked as the pull-down menu marks the choice
  taken, and the cursor on it.
- **The list has the keys until it closes**, as a menu: a context of its own, `combo_box`, which
  falls back to `global` only, and the line of the mode says `NORMAL · Combo box`. `j`, `k`, the
  arrows, Ctrl+n, Ctrl+p, the page keys, `g g`, and `G` move its cursor, with a count too; the
  wheel moves it as Up and Down do. Moving does not change the choice. Enter, Space, a click on a
  row, or Alt and the letter of a row takes it and closes the list; Esc, `q`, Ctrl+c, F10, or a
  click outside closes it with the choice it had, and the click goes no further. The second click
  of the double click that opened the list, or of one whose first click closed it, does nothing.
- **On a closed box**, Space, a click, and Alt and the letter of its label open the list. Enter
  presses the default button, as it does on a check box, and `j`, `k`, and Tab move the focus.
  The Configuration dialog has no default button: Enter opens the list there too, and `h` and `l`
  still take the choice before and the next without it, applied at once, as before.
- **Radio buttons go.** The checksum dialog has `Algorithm: [SHA-256 ▼]`, its message no longer
  ending in "with:"; the search of a tree has `Mode: [Text ▼]`; the Configuration dialog has a
  combo box for each choice. `Dialog::with_choices`, `in_a_row`, and the mark `(*)` are gone.
- **Letters** ([ADR 0033](0033-letters-in-dialogs.md)): the label of a combo box has one, among
  those of the dialog, and Alt and it give the box the focus and open its list. Its choices may
  have letters of their own, as the algorithms do, unique among them but not among those of the
  dialog: Alt and one takes that choice while the list is open, and does nothing else. The
  choices of the Configuration dialog, names of themes and the like, have none.

## Consequences

- A setting with many choices takes one row and shows them all on demand; the dialogs that had
  radio buttons are shorter.
- Changing the mode of the search of a tree takes two keys, Alt+M and then Alt+D or Alt+X, where
  Alt+D and Alt+X did it alone; `f f` still opens it in Files and dirs.
- In the Configuration dialog Space and Enter open a list rather than go to the next choice, so
  going round takes `l`, as before, or the list.
- The list may cover the elements under the box, and the buttons, as a menu does; it closes
  before anything under it takes a key or a click.
- A combo box that takes typed text, as Far's and Qt's can, filtering its list or giving a value
  of its own, is the next step: the box would hold a text field, and the list would follow it as
  the list of completions follows its field.
