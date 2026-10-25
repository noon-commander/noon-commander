# 0034. Animated windows

- Status: Accepted; its setting is `animations.windows` since [ADR 0037](0037-spinners.md)
- Date: 2026-10-16

## Context

Windows appeared and went in a single frame. Classic DOS file managers drew a window's frame
growing from its middle before its contents showed, and shrinking back as it closed: the eye
follows where the window comes from and where the keys go next.

Noon Commander has many windows, each with its own `Option` or queue in `App` and its own
renderer: dialogs, the location and tab menus, the zoxide window, the workspaces, the command
history, the search of a tree, the Configuration dialog, the help, the list of jobs, a job in
front, the results of checksums, the list of key hints, and the menus of F9. Most of them draw
their frame through `dialog::draw_box`, the menus of F9 their own. Closing a window drops its
state at once, so nothing of it is left to draw while it shrinks. The event loop redraws only
after an event, or on the spinner's tick while something spins.

## Decision

- **`ui.animations`, on by default**, a check box in Options → Configuration → Interface,
  applied at once. Off, windows come and go in one frame, as before.
- **Only the frame moves.** A window opens as its empty frame, in its colors and lines, with its
  shadow, at 1/7 of its size, then 2/7 … 6/7, 20 ms each, and then whole with its contents: about
  120 ms. It closes the other way, 6/7 down to 1/7, over what is under it. Dialogs and windows
  grow from their middle; a menu of F9 drops from its bar, which shows at once.
- **The animation sits on top of rendering, not in each window.** `tui/zoom.rs` keeps a `Stage`.
  `App::render` draws each window through `Stage::show` under a `zoom::Window` naming it: a dialog
  by its id, a job's window and checksum results by the job's id, a panel's menu by its side.
  `show` keeps the screen as it was before the window drew, and finds the frame by the cells the
  window drew: the first top left corner of the theme's lines that it drew, the corners to its
  right and below, and the blank cells drawn on both sides of it. Every cell holds a mark while
  the window draws, so that a cell it drew as it was counts too, as the frame of a dialog over a
  window of its size. A window that the last frame did not have grows:
  its cells and its shadow's are put back from before, and the smaller frame drawn there. Those
  of the last frame that this one did not draw shrink, drawn over everything at the end of the
  frame from where they were last.
- **One window in place of another only opens.** When a window opens on a frame, those that
  closed on it do not shrink: the next dialog in the queue, the dialog a menu's command opened,
  the menu of the other side.
- **The same window that moves or changes size does not animate**: the search of a tree as its
  results come, the menus of F9 as Left and Right go from one to the next.
- **Some frames show what is there at once**: the first, one of a new size of the screen, one
  where the viewer came or went (the search of a tree hides under it), and the first after
  another program had the terminal.
- **Keys never wait.** A window takes keys from the frame it opens on; a closed one is gone for
  the keys while it shrinks. The mouse finds a window where it is whole.
- **The event loop wakes for each size** at `App::frame_due`, the next one due, in a
  `sleep_until` beside the key timer, and sleeps without one as before.
- **Completion lists and the key hints of which-key are not animated**: they come and change as
  the user types, and would only lag behind the keys.

## Consequences

- Every window that draws a frame animates without code of its own; a new window needs only its
  `zoom::Window` and its call through `Stage::show` in `App::render`.
- While windows animate, each render keeps a copy of the screen for each window it draws, a few
  thousand cells; turned off, it keeps none.
- A window whose top left corner lands on the same corner in the same colors as the window under
  it is not found on that frame, and shows at once.
- Tests of the app turn `ui.animations` off, so screens show windows whole; `tui/zoom.rs` and a
  test of the app animate at given instants, which `App::render` takes beside the time of dates.
- Debug builds take `--slow-windows SECONDS`: a frame takes that long to grow or shrink, its six
  sizes shown for equal parts of it, never faster than usual, so each size can be watched. Release
  builds have neither the flag nor the code.
