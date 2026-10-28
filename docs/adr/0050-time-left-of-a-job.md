# 0050. The time left of a job

- Status: Accepted; amended by [ADR 0052](0052-bytes-written-count-once-they-arrive.md)
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
  setting attributes come after the last chunk with no report; and an upload queued up to 2 MiB
  at once and counted it, so the time of sending a file's end came after its last report too,
  until [ADR 0052](0052-bytes-written-count-once-they-arrive.md) counted bytes as they arrive.
  Data streaming alone said a byte cost half as much again as it did there, or a third; whole
  entries first still keep the forecast right where reports run ahead of the data.
- **Else streaming tells the cost of a byte.** Where the entries do not tell both, as for files
  of one size, a byte costs what it does in the stretches in which no entry was done, but for
  those that start as an entry starts, which hold its opening; an entry costs what the
  stretches between finished entries took beyond their bytes.
- **Before any data streams, small files tell what they can.** A job whose files have all been
  small takes the cost of an entry from the least squares while it stands clear of the noise;
  the cost of their bytes is mostly noise then, and no less than nothing, so a big file after
  tiny ones can look as quick as an entry until a quarter of a second of its data has streamed.
  Where the entries tell nothing, bytes alone count.
- **Standing still is unknown.** A job may pause between reports as it works: a link that sends
  a chunk every few seconds, a disk that takes its time now and then to write what it holds
  back. A pause is standing still once it is longer than twice the longest pause of over two and
  a half seconds within the last minute of work (`PAUSES`), and five seconds (`RECENT`) at
  least: the time left is unknown, `-:--`, as the speed falls to nothing; and of such a pause
  only the usual one counts for the pace, none before any long pause, so that when work goes on
  its pace is that of the work, not of the stall. Pauses like those before count in full, so a
  slow link or disk shows a time left throughout after its first pause. The time is that of the
  job's stopwatch, so a question left open is no pause (as for the speed).
- **Skipping is a pause.** A stretch in which more bytes were done than copied skipped some:
  neither its time nor its entries count, and the work behind it does not fade, so that Skip all
  over thousands of files, each found out by a round trip to the host, is no stall. Entries
  without data that are skipped cannot be told apart and count.
- **The time left shown follows the end the forecast says, smoothed** (`count_down`). The time at
  which the forecast says the job ends is smoothed, each second taking 9% of the time left as long
  to follow, from a second to half a minute (`ETA_SMOOTH`, `ETA_SMOOTH_MAX`). A forecast that is
  right says the same end every second, so smoothing the end evens out its ups and downs without
  lagging behind; one still settling, early in a short job, is soon followed. Following a
  forecast that stands still, the smoothed one lags by about as much, which with the band below
  must keep within the tenth that moves it halfway: at 15%, a review found it went beyond, from
  half a minute to five minutes left, moved halfway up, lagged again, and went up by up to 15
  seconds every half minute. So the share is that tenth less the band.
- **It counts down by the clock.** Each second it is the one shown before less a second, while
  that keeps within 1% of the forecast smoothed (`ETA_NEAR`); else the nearer edge of that band.
  So it follows the smoothed forecast without going up and down, and holds still where the
  forecast does: 10:00 shown where 9:15 are left counts down by the clock for four seconds,
  then two seconds at a time or so, fewer as it comes near, and is within 3% a minute later.
  Beyond a tenth from the forecast (`ETA_SLACK`), it moves halfway there each second, up or
  down, so that a lasting change shows within a few seconds without a jump, and the smoothing
  starts over from it. The first one, and one counted down to nothing, is the forecast. It keeps
  its fraction of a second and shows rounded, so that a count shows a second less each second.
  With the clock it no longer jumps to and fro by a second or two, which rounding a time left
  over a minute to five seconds hid before.
- It all comes from the counts that jobs report (`Counts`), with no change to `noc-ops` but for
  when a byte written counts (ADR 0052).

## Consequences

- Over an uneven link the time left keeps within a few percent of the average pace while the
  speed swings by a fifth either way, and the one shown goes down by a second each second; tests
  check all three.
- The time left shown keeps within 1% of the forecast smoothed, never going up where the
  forecast stands still; tests check that and the catching up. Where the forecast stands still
  for minutes, it comes to rest below it by as much as the end it says moves in the time the
  smoothing lags, a second less than it takes, and 1% more: 9:25 for 10:00, 1:31 for 1:40. In the
  simulator it is off by 1.3 to 3.7% of the time that is left for real trees on jobs of minutes,
  and by 2 to 3.5% on jobs of under a minute, where it moved to the edge of a band of 3% around
  the forecast itself before, and was off by 1 to 4% and by 1 to 8%. Over all the simulator's
  cases a second goes down by other than one a tenth less often, and the time left goes up a
  third less often. Counting down alone kept the error of its start, 5 to 10%, for the whole
  job; moving all the way to the forecast went up and down where the forecast went down slower
  than the clock; smoothing the end alone, with no band, was a little nearer but went down by
  other than one two thirds as often again.
- A lasting change of under a tenth shows over the time the smoothing takes: 8% off with five
  minutes left is 6% off ten seconds later, 3% after half a minute, and within 2% within a
  minute, where moving to the edge of the band around the forecast stayed 3% off. A forecast more
  than a tenth off for a single second moves the time left halfway, and the smoothing starts
  over from there, so it takes up to a minute to come back within 2%.
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
- With five seconds for every pause, a link that sent a chunk every eight seconds, and a disk
  that blocked for twelve every 64 MiB, showed `-:--` half the time and a time left short by 37
  and 59%; now only their first pause is unknown, and the time left keeps within 4 and 8%, and
  within 7 to 8% where each file takes 6 to 9 seconds to open, rather than 13 to 39%.
  After a stall of a minute and a half the time left is within 2% at once, where the five
  seconds of it that counted made it a fifth too long. Tests check both, and the stall.
- The first pause of such work is unknown and does not count, and a second stall within a
  minute of the first counts as work, its time left growing rather than unknown.
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
