# 0055. A job's reports go in batches

- Status: Accepted
- Date: 2026-10-29
- Amends: [ADR 0050](0050-time-left-of-a-job.md)

## Context

A copy reports before each entry and after each chunk it moves, 256 KiB on a local disk and
32 KiB to or from a host: thousands of reports a second, some 30,000 for a download of 1 GB/s
through a local `sftp-server`. Each went to the UI on its own, and the loop of the UI turned once
for each: it took the report, made the look of the job's window twice to tell whether the screen
changed, wrote the entry's location as text, and polled the terminal's events again, for which
crossterm takes a lock it keeps busy itself. In such a download of 16 GiB the thread of the UI
took 35 to 44% of a core, three quarters of it in that poll; a local copy at 3.5 GB/s, 12 to 14%;
a copy of 100,000 small files, 6 to 7%. The speed and the time left took 15 ns of each report.

Sending only the last report of each stretch would cut as much, but the time left learns what an
entry costs from each finished entry apart (ADR 0050): many small files in one stretch would look
alike, and the forecast fall back to bytes alone, which is off by 13 to 61% for the trees of the
check of ADR 0050, where it is off by 2 to 4%.

## Decision

- The task that runs a job (`forward` in `tui/tasks.rs`) sends its reports of progress together,
  each with the time the job made it (ADR 0050): the first after a quiet spell at once, then at
  most one batch each 20 ms (`BATCH`). Reports that wait go once the 20 ms are up, whether or not
  another comes.
- None is left out: the window counts each report of a batch at its time (`JobView::passed`) and
  shows the last (`JobView::working`), so the speed and the time left are those of the reports one
  by one. The entry's location goes once a batch.
- What waits goes before anything else the job reports, a question or its end, so that all keep
  their order.
- Reports of counting go in the same batches, but only the last of those that wait: each tells
  how many entries the job has found so far, which the next tells as well. It goes with the time
  of the first of them, so that the window knows when a job began to count after some work, and
  stops its clock there (ADR 0050). One that comes after progress waits behind that progress;
  progress that comes after it goes after it. Jobs report
  counting after each entry they were given as well as after each directory they list, so that
  files alone, a round trip each on a host, do not show `0 found` until the job starts on them;
  locally, that is thousands of reports a second.
- 20 ms is under the 40 ms between frames drawn for results of background work
  (`RESULTS_FRAME`): no batch waits for a frame.
- While the terminal is handed over to another program, the editor, a command, or the output of
  commands (`g <`), the event loop still takes the batches as they come, and the windows count
  them, without drawing (`meanwhile` in `tui/mod.rs`). The rest of what background work sends,
  questions among it, waits in the order it came until the app has taken the program's end, as
  before: the app does what it asks then, not while another program has the terminal.

## Consequences

- The thread of the UI takes under 2% of a core in each of these copies, a twentieth of what it
  took for the download, and the whole process a quarter less there. Copies go as fast as before.
- In the check of ADR 0050, batches of 20 ms change the mean error of the time left by half a
  point at most, and batches of 50 ms by under one.
- What shows of a job is 20 ms behind it at most.
- A job given thousands of files counts them in a few batches, rather than one report each, and
  the count of files on a host goes up as the job finds them, rather than stay at `0 found` for
  50 seconds for 1,000 files a twentieth of a second away.
- Batches alone made the reports that waited while the terminal was handed over grow five times
  slower, 2.3 MiB a second for the download rather than 11; taken meanwhile, they no longer
  grow. A checksum of a big file, at about 1.2 GB/s, left 22 MiB more with the process after a
  minute of `g <`, which it kept after the return; now none. The time left after the return is
  as right as before, within the swings of the job's own pace.
- A task of its own for the terminal's events would make each turn of the loop cheaper, not
  fewer; with batches, the loop waits idle almost all the time, and it is not needed.
