# 0024. Scroll bars on the frames, wherever rows scroll

- Status: Accepted
- Date: 2026-10-15

## Context

Only the help, the viewer, and the Configuration dialog had a scroll bar
([ADR 0023](0023-paging-stops-at-the-end.md)), and the mouse could press it
([ADR 0018](0018-mouse-support.md)). The panels, the location menu, the tab menu, the
Workspaces and history windows, the zoxide window, a tall menu of F9, the list of completions,
the list of jobs, and the list of checksums scroll too, without a sign of how far, and nothing
for the mouse to grab. A dialog's message taller than the screen was cut off, and with it the
buttons. A command line of more rows than fit showed no sign of the others either.

The bars of the help and the Configuration dialog took a column inside, and a gap before it,
which narrowed the text by two cells. Far draws its scroll bars on the right of the frame. mc
draws one only in its list boxes, without the mouse; its panels have none.

A setting for when bars show (always, with an empty track where everything fits, or only when
needed) was tried and dropped: it applied to some places and not others, such as dialogs and
the command line, where a bar on every short question would only be noise, and that was hard
to tell.

## Decision

- A scroll bar shows wherever there is more than fits, and only then, everywhere alike. There
  is no setting for it.
- The bar goes on the right of the frame, along the rows that scroll, over the frame's line:
  the panels (between the header and the line above the status line), the help, the settings
  of the Configuration dialog, the location menu, the tab menu, the Workspaces and history
  windows, the zoxide window, a menu of F9, the list of completions, the list of jobs, and the
  list of checksums. The text keeps its width. The viewer, without a frame, keeps it in its last
  column.
- A dialog's message that does not fit the screen scrolls, between the top and the rest of the
  dialog, which keeps its buttons; so do the rows of a command line that do not fit, in the last
  column of the line. The wheel scrolls such a message.
- The mouse presses every bar alike (ADR 0018): an arrow does what a step of the wheel does
  there, the track pages, and the thumb drags. Where the wheel moves a cursor, in the lists, so
  do the arrows and the track, and a dragged thumb puts the cursor on the screen it scrolled
  to; where the wheel scrolls without one, in the panels, the help, and the viewer, they scroll
  and the cursor moves only to stay on screen. The list of completions and the command line
  scroll under the thumb without changing what was typed, until the next key.
- `noc-scroll` draws the bars and tells their parts (`scrollbar::Bar`); the theme carries their
  arrows, which `ui.icons` picks, to every widget, as it carries `ui.borders`. `App` keeps, for
  what is in front, the bars and what each scrolls (`Scrolled`).

## Consequences

- Every list says how long it is and where it is, and the mouse can page or drag through it.
- The bar takes no room: the help and the settings of the Configuration dialog are two cells
  wider than before. While it shows, it covers the right side of the frame along the rows, so
  a frame's line there comes and goes with the length of what it holds.
- The lists that followed the cursor alone keep where a drag put them: the menus of F9, the
  list of jobs, and the list of checksums now scroll as little as shows the cursor, rather than
  always putting it at the bottom.
- The command line's terminal cursor does not show while the bar has scrolled it off the
  screen.
- Keys still cannot scroll a dialog's message; only the mouse can.
