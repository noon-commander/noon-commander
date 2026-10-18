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

- **One base for every spinner.** `tui/spinner.rs` draws every spinner in the app, from one clock.
  Each stands next to what tells what it waits for: a title, a path, or the place of the rows to
  come. No text says that something waits: a first version wrote `Reading the file…` after a
  scanner, which said less than the title over it.
- **Dots and skeletons, no scanner.** A first version drew a scanner, eight cells with a lit block
  running to and fro, in front of every text that waited. As each place moved its wait into its
  title or its rows, the scanner was left in the status line of the search of a tree alone, a second
  look of waiting beside the skeleton over it; that line takes a skeleton too. Waiting has two
  looks: dots in one cell, and faint bars where something is still to come.
- **Braille dots where one cell is all there is.** They turn through `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, 80 ms each;
  `|/-\` without icons. The icon of a host connecting in the location menu turns them, and so do the
  titles of panels, of windows, and of the viewer.
- **A panel waits in its title.** The title has one cell in front of the path: the icon of a folder,
  or of a host in the color of its status; without icons mc's `/` or the host's marker. While a
  panel reads a directory, the dots turn in that cell, so the path never moves when they come and
  go, and the title shows the path it reads, `⠹ web:/var/www`, at once: the path says what it waits
  for, which the old one would not, and the status line under the rows keeps the name under the
  cursor. The dots take the color of the host's status while it connects or does not answer, so
  connecting and reading look apart without a word that would take the room of the path. A host that
  does not answer still asks `web: Retry or Cancel?` on the status line, in the colors of errors: it
  is a question, not a wait. A reading that fails puts the old path back.
- **The viewer waits in its title.** Its title has one cell in front of the path too, the icon of
  the file (devicons' for its name; mc's marker of files, a space, without icons), where the dots
  turn while the file is read, so the path never moves. A skeleton of one line, of 80 cells at most,
  stands for the text, the shine crossing it as it crosses rows; more lines looked busier than what
  they stand for. The file is read at once, up to its limit, so there is no percentage to show; a
  gauge of a read in parts is left for later.
- **Shown after 150 ms.** A `Wait`, kept by what waits, remembers the first frame that drew it; the
  spinner and its text show only from 150 ms later, so quick work shows nothing.
- **Frames come from time, not ticks.** A frame of the dots or a step of the shine is the time since
  the wait showed, divided by its step, so every spinner on screen moves together, and tests draw a
  given instant. `App::spin_due` tells the event loop when the next frame is due: when a `Wait`
  starts to show, the next step of a spinner, or the next second of a job's clock. Nothing waiting,
  the loop sleeps until an event.
- **Settings of their own.** A new section, `[animations]`, holds `windows` (the frames of windows,
  which was `ui.animations`) and `spinners`, both on by default, each a check box under Animations
  in the Configuration dialog. `ui.animations` is gone with no fallback, before the first release; a
  config that still has it is an error that names the key. With `spinners` off, a still mark stands
  where the dots were, `⋯` (`…` without icons), and the shine rests.
- **`--slow-spinners SECONDS`, in debug builds only.** It holds back every result of background
  work by so many seconds, in the order they came, so spinners stay long enough to watch. A task
  between the tasks and the event loop stamps each result as it arrives and releases it at its
  time; the delays do not add up. `--slow-connections SECONDS` holds back only a connection made
  or ended, in a queue of its own, so other results come as they are and a host's dots can be
  watched in the location menu. With both, a connection waits for their sum. Release builds have
  neither the flags nor the code.
- **A window waits in its title.** The location menu, the zoxide window, and the search of a whole
  tree turn the dots in front of their title, over the line of the frame, so the title stays where
  it is: the title says what the window lists, and the rows, or the place where they come, stay
  clear. Until the first rows come, a skeleton stands in their place: one row of faint bars, a name
  on the left and what is known of it on the right, whose two bars a brighter shine crosses at once,
  from when the dots show, with the same delay, one cell every 40 ms over every bar, so a short bar
  is crossed sooner, and resting 400 ms after the longest; an empty box looked broken, and three
  still rows read as rows that had come. With `animations.spinners` off the shine rests. Every list
  whose rows do not come at once does the same. The status line of the search shows a skeleton of
  one bar while it searches, as long as the right bar of a row, which the shine crosses with them: a
  count that runs said less than the one at the end. R in the location menu keeps the last rows
  while it lists the volumes and the hosts again, the dots in the title, or shows the reason it
  failed right of the filter, in the colors of errors.
- **A job waits in its gauge.** While a job waits its turn or counts, its window says which on its
  first line, a stage of the job rather than a wait, and its gauge, which has no percentage yet, is
  a faint bar of `▒` over which the shine of the skeleton of rows runs to and fro, turning at its
  ends with no rest, with the same width, colors, and speed in cells, from the same delay
  (`dialog::shine_bar` draws both): an empty gauge at 0% read as a job that stood. It runs to and
  fro rather than sweeping, so that a stage of a job does not read as rows to come. No dots turn in
  its title: the gauge already shows that it works. A stop leaves the bar still: no more is to come.
  The list of jobs says `waiting`, `counting`, or `aborting` in place of the percentage, in a column
  too narrow for a bar.

## Consequences

- A new place that waits keeps a `Wait`, gives it to `App::waits`, and draws `Spin::wait` with
  `Spin::wait_dots` in its title, with a skeleton where rows or text are to come. It needs no tick
  of its own.
- The event loop wakes at most every 40 ms while a spinner shows, and not at all before 150 ms.
- Spinners and windows are turned off apart. Configs with `ui.animations` must move it to
  `animations.windows`.
- The glyphs of spinners live in `glyphs.rs`, in `Spinners`, by `ui.icons`.
- Tests draw twice, at a start and 150 ms later, to see a spinner, and turn spinners off for a
  still mark that does not change between frames.
