# 0056. The time left of a job stays as it is

- Status: Accepted
- Date: 2026-10-31
- Amends: [ADR 0050](0050-time-left-of-a-job.md)

## Context

[ADR 0050](0050-time-left-of-a-job.md), with ADRs 0052, 0053, and 0055, made the time left of a
job from two costs, of a byte and of an entry, at the pace of the last half minute of work, and
added cases on top: a file far larger than those done, stalls, skipping, failures, forecasts
that must agree, counting down by the clock, reports taken late. From 26 to 31 October that was
32 commits to `progress.rs`: some 550 lines of code with 19 constants, and 1,800 lines of tests.
Each review, with a simulator, found a rarer case, and one fix led to the next: a big file's
bytes took their cost from its own data, and the next day the small files behind it had to be
weighed at what they took.

A check over 28 scenarios set it against times left from bytes alone, without the smoothing of
the time shown: the bytes left at the average speed since the start, at the speed of the last
five seconds, and at a pace of bytes that fades over half a minute. The mean error of the time
left shown:

- Small files and a big one on a host: 1.3%, against 113 to 137%.
- 1,000 files of 4 KiB, then 200 of 1 MiB: 0.3%, against 19 to 20 times as long.
- 5,000 directories, then a big file: 0.3%, against 13 to 54 times as long.
- Skip all over 3,000 files, then a copy: 1%, against 3.3 to 22 times as long.
- A local disk at one pace, 10,000 small files and a big one: 0%, against a third.
- Trees of 128 bytes to 1 GiB in any order, at one pace: on a disk, all within 0.1%; on a host,
  1%, the error it starts with and keeps, against 0.1 to 1.7%, but 1.4% at worst, against 14 to
  58%.
- Such trees whose pace goes with the size of the files: 0.4 to 1%, against 0.4 to 4.3%; 7.5% at
  worst, against 6 to 119%.
- In those trees on a host, or with such a pace, jobs of half an hour to three hours, the time
  shown went up 0 to 22 times; theirs, 43 to 913 times.
- Small files after a big one whose bytes went slower: 91%, against 64 to 71%. No better.

What it still gets wrong comes from what the reports hold: the bytes and the entries left, not
which of those bytes are the entry's under way. Where the pace of a byte goes with the size of
its file, another case does not help.

## Decision

- **The time left of a job stays as it is**: `Forecast`, `Sums`, `Skips`, and `count_down` in
  `crates/noc/src/tui/progress.rs`, their constants, and what `noc-ops` reports for them.
- **It changes for defects only.** A defect is: it does other than ADR 0050 says; it shows what
  makes no sense, such as a time left of years or a panic; or a change elsewhere, in what
  `noc-ops` reports or how reports reach the window, breaks what it takes for granted, such as a
  report as each entry starts and after each chunk. A fix comes with a test that fails without
  it.
- **A scenario it gets wrong is a limit.** It goes into the consequences below, with its
  numbers, rather than into the code as a case, a constant, or a tuning.
- **Taking a part out is welcome** where a check shows it does not pull its weight: the
  scenarios run with it and without it.
- Any other change to how the time left works, a new model or the size of the entry under way
  in the reports, needs an ADR that replaces this one.

## Consequences

- The time left takes no more work than its defects need. A review of it looks for defects;
  one that finds only limits ends once they are written down here.
- Limits that stay, as measured for this ADR, as shares of the time that was left:
  - Small files after a big file whose bytes went slower than theirs cost what its bytes do.
    With 10,000 left, 1.09 to 1.30 while it goes; with 100,000, 1.6 to 2.1 while it goes, and
    2.2 down to 1.6 in the small files after it. It is right where the disk stays slow.
  - Entries with few bytes left after such a big file cost nothing while it goes: 0.64 down to
    0.50, and `0:01` shown at its end with 10 seconds left. After it, 8% short two seconds on,
    4% after five.
  - Directories and files are entries alike: 5,000 directories left after files that took
    22 ms each to open showed up to 4.4 times as long.
  - What is to come is unknown: the pace of a big file before its data streams, 3.5 to 4 times
    too long where it goes ten times as fast; a pace that halves, 34% short at worst as the
    time left catches up, which takes about 70 seconds (ADR 0050); a failure.
  - The first pause like a stall does not count: a disk that blocks for 12 seconds every 64 MiB
    showed `0:06` for 12:46 until its second block.
  - The time shown keeps the error it starts with, up to 1% (`ETA_NEAR`), for the whole job.
