# 0050. The time left of a job

- Status: Accepted
- Date: 2026-10-28

## Context

The window of a job that moves data shows its speed and the time left (`ETA`). Both followed the
average since the start of the job, so a slow start held them down for the rest of a copy. Then
both followed the last five seconds of work: the speed showed what happens now, but the time
left jumped with it over a link or a disk whose pace comes and goes, by as much as the speed,
10 minutes turning into 6 or 15. It also counted bytes alone, so many small files left, which
take their time opening, creating, and setting attributes rather than moving data, looked done
in no time.

A first version of what follows was reviewed against a simulator of the reports that jobs send
(`noc-ops` reports before each entry it tries and after each chunk). It made the time left 35
times too long while Skip all went over thousands of files on a host, as their time looked like
a stall; let it grow beyond any use while no data moved, `62:35:27` after two minutes, as the
work behind faded meanwhile; and took a big file after many tiny ones for no time at all, where
the noise of opening them made the cost of a byte come out below nothing. The decisions below
are those after the review.

## Decision

- **The speed and the time left look back over different stretches.** The speed keeps to the
  last five seconds of work (`RECENT`); the time left to the pace of about the last half minute
  (`ETA_HORIZON`, 30 seconds). Work weighs less as it gets older: half a minute older, a third as
  much; a minute, an eighth; a minute and a half, a twentieth. Weights that fade, rather than a
  window with an edge, do not jump when a slow stretch leaves it. Shorter follows every swing of
  the speed; longer, a minute or the whole job, holds on to a slow start and to an old pace for
  minutes.
- **Work is bytes and entries.** An entry costs time as some bytes do; the time left is the
  bytes left and the entries left, each at its cost, at the pace of the work behind (sums of
  time, bytes, and entries over stretches of at least a quarter of a second). Both costs fade
  with the same horizon, and a group of sums that has faded below `1e-100`, work some three
  hours old that nothing has renewed, is forgotten: left to fade further, the sums stuck among
  the smallest floats, alike, and a cost of an entry went to a second after six hours.
- **Whole entries tell both costs first.** Least squares of the time of the stretches between
  finished entries on their bytes and their entries give both, wherever the time of an entry
  falls among its reports, when each stands at least twice its standard error above nothing.
  A job reports as it starts an entry, before it opens it, and after each chunk; closing and
  setting attributes come after the last chunk with no report; and an upload queues up to 2 MiB
  at once, so the time of sending a file's end comes after its last report too. Data streaming
  alone said a byte cost half as much again as it did there, or a third.
- **Else streaming tells the cost of a byte.** Where the entries do not tell both, as for files
  of one size, a byte costs what it does in the stretches in which no entry was done, but for
  those that start as an entry starts, which hold its opening; an entry costs what the
  stretches between finished entries took beyond their bytes.
- **Before any data streams, small files tell what they can.** A job whose files have all been
  small takes the cost of an entry from the least squares while it stands clear of the noise;
  the cost of their bytes is mostly noise then, and no less than nothing, so a big file after
  tiny ones can look as quick as an entry until a quarter of a second of its data has streamed.
  Where the entries tell nothing, bytes alone count.
- **Standing still is unknown.** Once no report has come for five seconds (`RECENT`), the time
  left is unknown, `-:--`, as the speed falls to nothing in the same time; and of such a gap,
  only those five seconds count for the pace, so that when work goes on its pace is that of the
  work, not of the stall. Gaps up to five seconds count in full, as a slow link that sends a
  chunk every few seconds works all that time. The time is that of the job's stopwatch, so a
  question left open is no gap (as for the speed).
- **Skipping is a pause.** A stretch in which more bytes were done than copied skipped some:
  neither its time nor its entries count, and the work behind it does not fade, so that Skip all
  over thousands of files, each found out by a round trip to the host, is no stall. Entries
  without data that are skipped cannot be told apart and count.
- **The time left shown counts down by the clock** (`count_down`). Each second it is the one
  shown before less a second, while that keeps within 3% of the forecast (`ETA_NEAR`), which the
  forecast's own ups and downs over an uneven link mostly do. Up to a tenth from it
  (`ETA_SLACK`), it also moves a tenth of the way to the forecast each second, so that it does
  not stay off for the rest of the job: 10:00 shown where 9:15 are left goes 9:55, 9:49, … and
  keeps within 3% of the forecast half a minute later. Beyond a tenth, it moves halfway each
  second, up or down, so that a lasting change shows within a few seconds without a jump. The
  first one, and one counted down to nothing, is the forecast. It keeps its fraction of a second
  and shows rounded, so that a count shows a second less each second and a tenth of a small gap
  is not lost to the rounding. With the clock it no longer jumps to and fro by a second or two,
  which rounding a time left over a minute to five seconds hid before.
- It all comes from the counts that jobs report (`Counts`), with no change to `noc-ops`.

## Consequences

- Over an uneven link the time left keeps within a few percent of the average pace while the
  speed swings by a fifth either way, and the one shown goes down by a second each second; tests
  check all three.
- The time left shown keeps within 3% of the forecast once it has caught up; in the simulator it
  is off by 1 to 3% of the time that is left for real trees, where counting down alone kept the
  error of its start, 5 to 10%, for the whole job. Where the forecast itself drifts by more, as
  over a link whose speed jumps every second, it follows: about one second in four goes down by
  other than one. A band of 5% halved that but doubled the error elsewhere.
- In the simulator, the forecast keeps within 1 to 3% of the time that is left for files of
  sizes spread as in real trees, with directories and empty files among them, locally and on a
  host; through Skip all over 5,000 files on a host it falls as they leave the work, where it grew
  to 35 times too long, and is within 2% after it (before it, the files to skip count as work,
  which nothing can tell); a link that sends a chunk every four seconds, within 2% rather than
  10%; a big file after 2,000 tiny ones, exact within seconds of its start. A second review,
  whose simulator reported as jobs do, before each entry opens and with an upload's queue, found
  the forecast 3 to 4 times too long for uploads of files of a few MiB followed by small ones,
  and a fifth to a third too short for downloads; with whole entries first and the opening
  kept out of streaming, it is within 1 to 4%. Tests check the big file after tiny ones over
  ten random runs, the small files left after big ones, files of one size and of many sizes up
  and down, Skip all one entry at a time, a stall and the work after it, hours of one kind of
  work forgetting the other's cost, and each way to the costs.
- Limits of a pace learned as the job goes: small files of one size, then a big file, show the
  big file at their own pace, hours for minutes, until its data streams; a big file, then small
  ones, shows them at the cost of bytes until the first of them is done. A lasting change of
  pace shows in full within about 70 seconds if it halves, 100 if it quarters; swings as long as
  the horizon, half a minute fast and half a minute slow, move the time left by a third either
  way.
- The model is two costs and a pace: a link that is fast for big files and slow to open each
  one is right; one whose pace changes with the size of the files in other ways is not.
- What a job does without reports, such as setting the times of directories at its end, is not
  in the work left: the time left shows `0:00`, then `-:--` once it looks like a stall.
- Hours of one kind of work forget what the other costs: after a file of many hours, small
  files left look as quick as their bytes until the first of them are done.
