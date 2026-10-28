# 0053. A directory is finished once all in it is done

- Status: Accepted
- Date: 2026-10-28
- Amends: [ADR 0050](0050-time-left-of-a-job.md)

## Context

Writing into a directory changes its time, so a copy that preserves attributes gave the
directories it made the times and permission bits of their sources last, once every entry was
copied. A move between file systems removed the source directories then too, the deepest first.
That last pass sent no reports, and the time left of ADR 0050 knows only the work that jobs
report: it left out two round trips to a host for each directory, three for a move.

A check that ran the copy of `noc-ops` over file systems that take their time, a link with
round trips of 50 ms, replayed its reports into a job's window. An upload of 3,000 files in
1,110 directories took 1,247 seconds, 112 of them in the last pass: the time left was a quarter
short all the way, came to `0:00` two minutes before the end, and showed `-:--` for the rest.
Measured to the last report, it was 2% off. A copy aborted before the end left every directory
it had made with the time of the copy.

## Decision

- A directory is finished once the copy is past the last entry within it, at any depth: entries
  come after the directory they are in, so then nothing more goes into it. Each entry is
  followed by the directories it finishes, the deepest first.
- Finishing a directory that the copy made gives it the attributes of its source; finishing one
  that a move copied removes its source, unless something in it stays. Directories that were
  there keep their own attributes, as before.
- Failures ask as before, now among the entries rather than after them.

## Consequences

- The time of finishing directories falls among the entries, where the time left counts it as
  part of what an entry costs. In the same check the upload above was 2% off, with no `-:--` at
  its end; an upload of 1,500 files in a real tree went from 8 to 4% off, a local copy of 20,000
  from 6 to 3%.
- What is left after the last report is the directories above the last entry, a few round trips.
- A copy that is aborted, or cancelled, keeps the attributes of the directories it finished, and
  a move has removed their sources.
- The time left takes an entry and the directories it finishes as one: the cost of an entry is
  that of files and directories together, as it was before for their opening and creating.
