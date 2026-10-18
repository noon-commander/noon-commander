# 0037. Spinners

- Status: Accepted
- Date: 2026-10-18
- Amends: [ADR 0034](0034-animated-windows.md)

## Context

Several places wait for work in the background: a panel reading a directory or waiting for a
connection, the location menu looking for volumes and hosts, the zoxide window asking zoxide, the
search of a tree, a job that is starting or stopping, and the viewer reading a file. Each did it
its own way. The panels drew a one-cell spinner from `glyphs.rs` on a tick of the event loop,
other places wrote `Loading…` with no spinner, and some drew nothing at all. The spinner did not
say what it waited for, and the text did not show that something was still going on.

Quick work flickered: a directory read in 5 ms drew a spinner for one frame and took it away.
And there was no way to watch a spinner for long enough to check how it looks, short of a slow
host.

`ui.animations` (ADR 0034) turned the frames of windows on and off. A spinner is an animation
too, which some users want and others do not, apart from windows.

## Decision

- **One spinner, always with its text.** `tui/spinner.rs` draws every spinner in the app, and
  each says what it waits for after it: `Reading the directory on web…`, `Connecting to web…`,
  `Looking for volumes and hosts…`, `Asking zoxide…`, `Reading the file…`. The texts are in the
  Fluent files, as any other.
- **A scanner.** Eight cells, a lit block that runs from one end to the other and
  back, with a trail that fades behind it, and a pause at each end, 40 ms a step. The trail is
  one color, `busy` of the theme (`busy_dialog` in dialogs), blended into the background:
  brighter at the head, fading to 30% in the pause. Where the colors are not known (the low 16,
  the terminal's own), the head is bold, the next two cells plain, and the rest dim. The blocks
  are `■` and `⬝` with icons, `■` and `·` without.
- **Braille dots where one cell is all there is.** The icon of a host connecting in the location
  menu turns through `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, 80 ms each; `|/-\` without icons.
- **Shown after 150 ms.** A `Wait`, kept by what waits, remembers the first frame that drew it;
  the spinner and its text show only from 150 ms later, so quick work shows nothing. A job's
  window, which shows whole, draws its spinner at once.
- **Frames come from time, not ticks.** A frame of the scanner is the time since the app's first
  frame, divided by its step, so every spinner on screen moves together, and tests draw a given
  instant. `App::spin_due` tells the event loop when the next frame is due: when a `Wait` starts
  to show, the next step of a spinner, or the next second of a job's clock. Nothing waiting, the
  loop sleeps until an event.
- **Settings of their own.** A new section, `[animations]`, holds `windows` (the frames of
  windows, which was `ui.animations`) and `spinners`, both on by default, each a check box under
  Animations in the Configuration dialog. `ui.animations` is gone with no fallback, before the
  first release; a config that still has it is an error that names the key. With `spinners` off,
  a still mark stands where the spinner was, `⋯` (`…` without icons), and the text stays.
- **`--slow-spinners SECONDS`, in debug builds only.** It holds back every result of background
  work by so many seconds, in the order they came, so spinners stay long enough to watch. A task
  between the tasks and the event loop stamps each result as it arrives and releases it at its
  time; the delays do not add up. `--slow-connections SECONDS` holds back only a connection made
  or ended, in a queue of its own, so other results come as they are and a host's dots can be
  watched in the location menu. With both, a connection waits for their sum. Release builds have
  neither the flags nor the code.
- **R keeps the rows.** Listing the volumes and the hosts again in the location menu leaves the
  last rows in place; the spinner and `Looking again…` show right of the filter, or the reason
  it failed in the colors of errors.

## Consequences

- A new place that waits keeps a `Wait`, gives it to `App::waits`, and draws `Spin::wait` with
  its text. It needs no tick of its own.
- The event loop wakes at most every 40 ms while a spinner shows, and not at all before 150 ms.
- Spinners and windows are turned off apart. Configs with `ui.animations` must move it to
  `animations.windows`.
- The glyphs of spinners live in `glyphs.rs`, in `Spinners`, by `ui.icons`.
- Tests draw twice, at a start and 150 ms later, to see a spinner, and turn spinners off for a
  still mark that does not change between frames.
