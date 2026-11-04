# 0066. Visual mode stays with its tab

- Status: Accepted
- Date: 2026-11-04
- Amends: [ADR 0022](0022-vim-modes.md),
  [ADR 0051](0051-ctrl-x-sends-a-command-to-the-other-panel.md)

## Context

A tab is a whole panel: its listing, cursor, marks, sort order, and jump list stay with it while
another tab or the other panel has the keys ([ADR 0011](0011-tabs-per-panel.md)). Visual mode did
not: it ended in every panel without the keys, in the other panel after Ctrl+w w, Tab, or a click,
and in a tab that another one hid. vim ends Visual mode when it leaves a window, too.

Rows are selected to act on them, and acting often needs a look elsewhere first: at the other
panel, where F5 copies to, or at another tab of the same panel. Ending Visual mode there threw
the selection away; `g v` brought it back, but only after `v` had been given up, and only by a key
to learn.

Alt+1 … Alt+9 now show the tab of that number, and Alt+0 the last one, in Normal and in Visual
mode. In Visual mode they had to either end it or keep it.

## Decision

- **Visual mode is the tab's, as its cursor is.** Ctrl+w w, Ctrl+w Ctrl+w, Ctrl+w p, Tab, Ctrl+w h,
  Ctrl+w l, a click in the other panel, and Alt and a digit leave it as it is in the panel or the
  tab they leave, which has it again when it has the keys. Nothing else in the app ends it but the
  keys of the panel that has it: Esc, Ctrl+c, `v` and `V`, Space, the commands of files, and the
  F-keys that do not act on the rows selected, as before.
- **A panel without the keys draws the rows it selects**, reversed, as the panel with the keys does,
  so the screen says what it keeps.
- **Ctrl+x into a panel that kept Visual mode** runs the command there in Visual mode: Ctrl+x `j`
  extends its selection, Ctrl+x F5 copies the rows it selects. The command ends, and the keys come
  back, when that panel is in Normal mode again or still in the Visual mode it kept; Ctrl+x `v`
  ends it there.

## Consequences

- Rows selected in one panel or tab survive a look at the other panel or at another tab, and the
  command of files can then run on them from where they are.
- The line of the mode says the mode of the panel with the keys only; a panel without them shows
  its kept selection, but not that it is in Visual mode, until it has the keys.
- Unlike vim, leaving a window no longer ends Visual mode; `g v` is still there for a selection
  that a command ended.
- The keys after Ctrl+x are read in the mode of the panel where Ctrl+x was typed, Normal mode; in a
  panel that kept Visual mode the action they resolve to runs as Visual mode runs it, as `y y`
  copies the rows selected.
