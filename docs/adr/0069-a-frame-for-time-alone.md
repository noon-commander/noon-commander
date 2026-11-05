# 0069. A frame for time alone

- Status: Accepted
- Date: 2026-11-05
- Amends: [ADR 0034](0034-animated-windows.md), [ADR 0035](0035-formats-of-the-region.md),
  [ADR 0037](0037-spinners.md)

## Context

While something waits, a spinner asks for a frame every 40 ms (a shine) or 80 ms (dots alone),
and each frame drew the whole screen again (ADR 0037). Measured in a release build on a screen of
200 × 50 cells, with 934 files in the panels, the location menu waiting for its places took
11.5% of a core at 24 frames a second, and 0% once its rows came; 4.5% over an empty directory.
`sample` split the time of a frame:

- About three quarters in `Panel::render`, the two panels under the window, which had not
  changed. A quarter of the frame was Foundation writing the time and the size of every row
  (`NSDateFormatter`, `NSNumberFormatter`, ADR 0035), a few microseconds a call.
- 13% in `Stage::show` (ADR 0034): a copy of the whole screen for the open window, a mark in each
  cell, and a comparison of each cell after it drew, to find where it was, which had not moved.
- The skeleton itself under 1%; the output to the terminal about 110 bytes a frame.

And a shine at rest, 400 ms between sweeps, still drew every 40 ms, as `Spin::shine` counted a
frame that drew a resting shine as one that drew it moving: only the dots changed.

## Decision

- **The event loop tells a frame for time alone.** It keeps whether anything but time woke it
  since the last frame: a key, a paste, the mouse, a resize, a timer of keys, a result of work,
  the frame that results waited for, the terminal back from another program. A frame that only
  `App::spin_due` or `App::frame_due` asked for is for time alone, which it tells the app
  (`App::frame_for_time`) before it draws.
- **On such a frame a side puts back what it drew.** `render::Frames` keeps the cells each side
  drew on the last frame, its panel and its line of tabs, with where its tabs are, and puts them
  back rather than draw the side again, unless its panel moves with time (`Panel::moves`): while
  it waits, dots turn in its title; while its host connects, in its marker; while a name is typed
  in it, it puts the terminal's cursor there, which a frame does not keep. The bars, the command
  line, the windows, and which-key draw as before.
- **On such a frame a window stays where it was found.** A window that shows whole, and does not
  grow, draws through `Stage::show` without the copy, the marks, and the search for its frame: it
  keeps the place the last frame found. One that opens, grows, or shrinks goes as before.
- **A shine at rest draws at the pace of the dots.** As a shine crosses a bar it tells its `Wait`
  when it moves next: a step later while it sweeps, or when its next sweep starts while it rests,
  the soonest of the bars of the frame. The next frame is due then, or a frame of the dots later
  if that is sooner (`Wait::moves_after`). A shine that runs to and fro over a gauge never rests.
- **The formats keep what they wrote.** `noc_locale::Formats` keeps each text it wrote by what it
  wrote it of: a number with its digits and grouping, a date, a time, a date with its time. The
  memo holds 4,096 texts at most and starts over once full, a few screens of rows; new formats,
  after a change of `ui.formats`, come with a memo of their own. The rows of a panel drawn again,
  as it scrolls, ask Foundation nothing they asked before.

## Consequences

- The location menu waiting over the same panels takes 2.2% of a core rather than 11.5%, at 22
  frames a second rather than 24, as much as over an empty directory; the search of a tree 3.6%
  rather than 12.7%. A panel that waits draws itself on each frame, as before.
- Whatever changes what the panels show must reach the event loop as an event, a timer of keys,
  or a result, never through the timers of time; every change does today. A test that changes the
  app and draws a frame for time alone sees the panels as they were, as
  `a_frame_for_time_alone_puts_back_what_the_panels_drew` checks.
- A window whose size changed on a frame for time alone would shrink from where it was last found
  on another frame; no window changes its size with time alone.
- The app keeps a copy of the cells of both sides between frames, a screen at most.
- The next frame after a sweep comes up to 80 ms after it ends, at the pace of the dots, and the
  next sweep starts on time.
