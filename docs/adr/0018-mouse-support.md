# 0018. Mouse support

- Status: Accepted
- Date: 2026-10-05

## Context

mc and Far take the mouse: a click moves the cursor, a double click opens, the wheel scrolls,
and a right click marks. Users coming from them reach for it, and the F-key bar,
the tabs, and the pull-down menu look like things to click.

The keymap is the only way into the app so far: widgets never see raw keys, only `Action`s.
Terminals report the mouse only while asked to, and while they do, plain dragging no longer
selects text; a modifier does (Shift in most terminals, Option in iTerm2). Some users would
rather keep the terminal's selection, so the mouse has to be optional. crossterm reports
presses, releases, drags, moves, and the wheel, but no double clicks.

## Decision

- `ui.mouse` turns the mouse on, by default, and can be switched in the Configuration dialog,
  which takes effect at once: the event loop captures the mouse while `ui.mouse` is on and
  releases it when it goes off, when the editor gets the terminal, when Noon Commander ends,
  and on a panic (the hook of `ratatui::try_init` does not release it, so one around it does).
- `ui.wheel` says how far a step of the wheel scrolls: a number of lines from 1 to 20 (3 by
  default), or `"page"`. One key with two kinds of value reads better than two keys of which
  one only counts with a value of the other.
- The event loop turns crossterm's mouse events into `Pointer`s (`tui/mouse.rs`): a click, a
  double click, a right click, or a step of the wheel, at a cell. Two clicks on the same cell
  within 400 ms are a double click; a third starts again. Moves, drags, releases, and the
  middle button are dropped without a redraw. A press ends a pending key sequence.
- Rendering records where it drew what can be pressed (the F-key slots, the panels, their
  rows, and the tabs), and `App::pointer` looks there. What is in front takes the press, as it
  takes keys; a window or a menu over the panels keeps the mouse from them.
- A click on an F-key slot presses that key, through the same path as the keyboard; only
  slots with a label take it, and the second click of a double click does nothing, so that a
  double click does not press the key twice.
- In a panel, a click puts the cursor on the row and gives the panel the keys; a double click
  then does what Enter does, so it opens directories and hosts and does nothing on a file, as
  Enter does. A right click marks or unmarks the row, as in mc and Far, with the cursor on it; unlike
  Insert, the cursor does not move on. The wheel scrolls the panel under it, which need not be
  the active one, and keeps its cursor on screen. A click on a tab shows it. Clicks end quick
  search, as a key of the panel's own would, and do nothing while a name is edited in its row.
- In the viewer the wheel scrolls as Up and Down, or PageUp and PageDown.
- In the pull-down menu, a click on a title opens its menu, or closes it if it is the one
  open; a click on a command runs it if it runs now; the wheel moves the cursor; and a click
  outside the bar and the menu closes the menu bar, as in mc. With `ui.menu_bar = "always"`, a
  click on a title of the idle bar opens that menu.
- In a dialog, a click chooses a radio button, puts the cursor in a text field where it was
  clicked (the text stays, as after a move of the cursor), switches a check box, or presses a
  button; a double click on a radio button also presses the default button, as Enter does
  there. A click outside the dialog does nothing: it is modal.
- A double click counts only if its first click left the same thing in front: when the first
  click ran a command of the menu or pressed a button that closed a dialog, the second click
  does nothing, rather than open what is under it. Dialogs get an id for this, so that one
  that takes the place of another is not the same.
- In the other windows and menus (the location menu, the zoxide and Workspaces windows, the
  checksums, a job's window, the list of jobs, the help, and the list of completions) the
  wheel stands for Up and Down, or PageUp and PageDown, and goes to them as those keys do;
  it scrolls the help, and does nothing in a job's window, where those keys move between the
  buttons. A click on a row puts the cursor there, a double click on a row of a menu opens it
  as Enter does, and a click on a button presses it. A click outside a menu or the list of
  completions closes it, as in mc; outside a window with buttons it does nothing.
- Widgets turn a press into what they draw: they move their own cursor or focus, and return
  the key the press stands for, such as Enter or Esc, which `App` hands to them as if typed.
  So the mouse reuses what the keys do, and no window has a second way to open or close.
- In the Configuration dialog a click on a category shows its settings; on a setting it puts
  the cursor there, and on its value switches a check box, picks the next choice (the one
  before at `<`), or puts the cursor in the text. Leaving a text field applies it, as a key
  that leaves it does.

## Consequences

- The F-key bar, the panels, and the tab bars report where they drew; any change to their
  layout keeps the mouse right without more work.
- With the mouse on, selecting text needs a modifier; `ui.mouse = false`, or Mouse in
  Options → Configuration, brings plain selection back.
- Moves of the mouse arrive all the time while it is captured, so they must stay cheap: they
  skip the redraw.
- Mouse bindings are not in `keymap.toml`; what a click does follows mc and Far, and the keys
  stay the way to do anything.
