# 0050. The time left of a job

- Status: Accepted; amended by [ADR 0052](0052-bytes-written-count-once-they-arrive.md),
  [ADR 0053](0053-a-directory-is-finished-once-all-in-it-is-done.md), and
  [ADR 0055](0055-a-job-s-reports-go-in-batches.md)
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
  stretches between finished entries took beyond their bytes. The job's first report starts an
  entry too.
- **Before any data streams, small files tell what they can.** A job whose files have all been
  small takes the cost of an entry from the least squares while it stands clear of the noise;
  the cost of their bytes is mostly noise then, and no less than nothing, so a big file after
  tiny ones can look as quick as an entry until a quarter of a second of its data has streamed.
  Where the entries tell nothing, bytes alone count.
- **Files far larger than those behind are unknown until a byte's cost is.** Until data has
  streamed within an entry, or finished entries with data have had sizes at least twice apart,
  the work tells nothing of what a byte costs beside an entry. While then the entries left are
  on average more than ten times as large as those done (`AHEAD`), the time left is unknown,
  `-:--`: small files of one size, then a big one, showed the big one at their own pace, nine
  hours for three quarters of a job of under five minutes. Once the work has told it, this
  no longer holds for the rest of the job, so that the time left does not come and go.
- **Standing still is unknown.** A job may pause between reports as it works: a link that sends
  a chunk every few seconds, a disk that takes its time now and then to write what it holds
  back. A pause is standing still once it is longer than twice the longest pause of over two and
  a half seconds within the last minute of work (`PAUSES`), and five seconds (`RECENT`) at
  least: the time left is unknown, `-:--`, as the speed falls to nothing; and of such a pause
  only the usual one counts for the pace, none before any long pause, so that when work goes on
  its pace is that of the work, not of the stall. Pauses like those before count in full, so a
  slow link or disk shows a time left throughout after its first pause. Until a pause is known
  to be standing still, the forecast is that of the last report. The time is that of the job's
  stopwatch, so a question left open is no pause (as for the speed).
- **Skipping is a pause.** A stretch in which more bytes were done than copied skipped some:
  neither its time nor its entries count, and the work behind it does not fade, so that Skip all
  over thousands of files, each found out by a round trip to the host, is no stall. Entries
  without data that are skipped cannot be told apart and count.
- **It shows once its forecasts agree.** Early in a job the forecast knows little of its pace:
  shown after the first second of work, it was three times too long for five seconds of an
  upload of a tree. So the time left is unknown until the last three forecasts, a second of work
  apart at least, agree: each less the time since it was made, the longest is at most a quarter
  longer than the shortest (`ETA_SETTLE`, `ETA_AGREE`). A forecast that is right agrees within
  three seconds. After half a minute of work it shows whether or not they do (`ETA_SETTLE_MAX`).
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
  starts over from it. More than twice as long as the forecast, or less than half as long
  (`ETA_JUMP`), it is the forecast at once: halving the way, a time left of hours took nine
  seconds to come down to the minute a big file after small ones had left, showing hours. The
  first one, and one counted down to nothing, is the forecast too. It keeps its fraction of a
  second and shows rounded, so that a count shows a second less each second. With the clock it
  no longer jumps to and fro by a second or two, which rounding a time left over a minute to five
  seconds hid before.
- It all comes from the counts that jobs report (`Counts`), with no change to `noc-ops` but for
  when a byte written counts (ADR 0052) and when a directory is finished (ADR 0053).
- **Each report counts at the time the job made it**, which the task that runs the job takes as
  it passes the report on, in a batch with others
  ([ADR 0055](0055-a-job-s-reports-go-in-batches.md)), and a question at the time the job asked
  it, which stops the clock there. The UI took no reports while the terminal was handed over to
  the editor, a command, or the user screen, and those that waited came at once when it was back;
  it now takes those of progress meanwhile, but a question still waits (ADR 0055). Counted when
  the UI took them, the work meanwhile looked like a stall, left out, and then all its data had
  moved in a quarter of a second: the time left was a half of what was left after half a minute
  away, a fifth after two minutes, and took one or two minutes to come right. A report taken
  more than a quarter of a second after the speed and the time left of the second were worked
  out (`LATE`) makes them be worked out again, so that the time left is right in the same
  second, rather than show `-:--` until the next.

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
- In a check that runs the copy of `noc-ops` over file systems that take their time, the time
  left first shows after 2.5 to 10 seconds of work rather than one, a little nearer on average,
  and the most it is off over a job falls from 28–35% to 10–13% for a local tree of 20,000
  files, from 37–109% to 4–16% for a download of a tree, and from 31–226% to 25–173% for an
  upload of one, where the forecasts of its first small files can agree and still be three times
  too long. After a stretch of Skip all, which tells no pace, it shows three seconds later.
- In the same check, a copy of 20 GiB whose reports the UI took only after half a minute away
  showed 2:38 left when 5:39 were, and was off by 23% on average over the next two minutes; two
  minutes away, 0:45 for 4:09, and 44%. Counted at the times the job made them, the reports keep
  it within 1.4% throughout, as does a question asked while the UI was away. In Noon Commander
  itself, back from the user screen after 15 seconds of a copy of 100,000 small files, it showed
  `-:--`, then `0:03` with 13 seconds left; now `0:13` with 15, at once. After 8 seconds of a
  download over SFTP, `0:02` with 6 left; now `0:05` with 5.
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
- The first pause of such work is unknown and does not count. Through a shorter pause the
  forecast stands where the last report left it, and the time left shown holds near it as the
  clock goes on, rather than count down or grow. So does a second stall within a minute of the
  first, rather than show `-:--`, until the first is more than a minute old: then it is unknown,
  even partway through. One that ends before that counts as work, and the time left after it is
  longer by its share of the pace; one that does not, does not count either. In the simulator,
  with two stalls of 40 seconds, 30 seconds apart, the time left held between 1:49 and 1:42 for
  the first 28 seconds of the second, while two and a half minutes were left; it showed `-:--`
  for the rest of it, and was right after it.
- Limits of a pace learned as the job goes: small files of one size, then a big file, show
  `-:--` until its data streams, rather than the big file at their pace, hours for minutes; in
  the check that runs the copy of `noc-ops`, it is within 2% once it shows, a second or two into
  the big file, where it was 19 to 187 times too long, locally, to a host, and from one; and so
  is a copy of 200 files of 1 MiB after 1,000 of 4 KiB, where it was 60 times too long. Files
  of sizes apart, or a big one first, show as before. A big file, then small ones, shows them at
  the cost of bytes until the first of them is done. A lasting change of pace shows in full
  within about 70 seconds if it halves, 100 if it quarters; swings as long as the horizon, half a
  minute fast and half a minute slow, move the time left by a third either way.
- The first report was not taken to start an entry, so where the first file took a quarter of a
  second or more to open, as on a host far away, its opening was the cost of a byte, which no
  later stretch faded away, as time and bytes fade alike: 1,000 files of 4 KiB, then one of
  600 MiB, showed 13 hours where `-:--` should be, and 200 files of 1 KiB after 200 of up to
  30 KiB showed 6 seconds for 83. Now they show `-:--`, and within 1%; a test checks both.
- The model is two costs and a pace: a link that is fast for big files and slow to open each
  one is right; one whose pace changes with the size of the files in other ways is not.
- What a job does without reports is not in the work left: the time left shows `0:00`, then
  `-:--` once it looks like a stall. Setting the attributes of directories at the end of a copy
  was such work, two round trips to a host for each, two minutes of an upload of 20 minutes,
  until [ADR 0053](0053-a-directory-is-finished-once-all-in-it-is-done.md) did it among the
  entries.
- Hours of one kind of work forget what the other costs: after a file of many hours, small
  files left look as quick as their bytes until the first of them are done.
