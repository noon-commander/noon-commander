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

## Decision

- **The speed and the time left look back over different stretches.** The speed keeps to the
  last five seconds of work (`RECENT`); the time left to the pace of about the last half minute
  (`ETA_HORIZON`, 30 seconds). Work weighs less as it gets older: half a minute older, a third as
  much; a minute, an eighth; a minute and a half, a twentieth. Weights that fade, rather than a
  window with an edge, do not jump when a slow stretch leaves it. Shorter follows every swing of
  the speed; longer, a minute or the whole job, holds on to a slow start and to an old pace for
  minutes.
- **Work is bytes and entries.** An entry costs time as some bytes do; the time left is the
  bytes left and the entries left, each at its cost, at the pace of the work behind. The costs
  come from least squares of the time of the stretches between finished entries on their bytes
  and their entries, fading with the same horizon, so that big and small files tell them apart.
  Where they cannot, as while one file is copied, or files of one size, or where a cost comes out
  below zero, bytes alone count, or entries alone; the job starts with bytes alone.
- **The pace counts every second of work.** Its sums of time, bytes, and entries go over
  stretches of at least a quarter of a second, and the time left counts the stretch since the
  last of them: while no data moves, it grows. The time is that of the job's stopwatch, so a
  question left open is no gap (as for the speed).
- **Skipped entries do not count.** A stretch in which more bytes were done than copied skipped
  some: its entries count neither for the pace nor for their cost, as those skipped took no time
  and which they were is not known. Entries without data that are skipped cannot be told apart
  and count.
- It all comes from the counts that jobs report (`Counts`), with no change to `noc-ops`.

## Consequences

- Over an uneven link the time left keeps within a few percent of the average pace while the
  speed swings by a fifth either way; tests check both, and a change of pace that lasts shows in
  the time left within a minute.
- The 1,000 small files after big ones take about the time that the per-entry cost says, where
  bytes alone gave a fraction of a second; tests check that, and that 5,000 entries skipped at
  once leave it so.
- The time left no longer shows `-:--` while no data moves: it grows.
- The model is two costs and a pace: a link that is fast for big files and slow to open each
  one is right; one whose pace changes with the size of the files in other ways is not.
