# 0052. Bytes written count once they arrive

- Status: Accepted
- Date: 2026-10-28
- Amends: [ADR 0050](0050-time-left-of-a-job.md)

## Context

A copy counted each chunk as done and copied once the writer took it. A local file takes it at
once. An SFTP writer keeps up to 64 writes of 32 KiB in flight, and only the server's answers tell
it that they have arrived. So an upload ran up to 2 MiB ahead of the link: the first 2 MiB of each
file seemed to go at the speed of the disk it was read from, and the last 2 MiB went out after the
last report of the file, while `finish` waited for them. The speed of an upload jumped with every
file, and the forecast of ADR 0050 took uploads of files of a few MiB for 3 to 4 times as long as
they were, until it took whole entries first.

The writer also pushed its writes into a queue that sent nothing until it was polled, which it was
only once the queue was full or the file finished: a file under 2 MiB went out only in `finish`.

## Decision

- `FileWriter` tells how many bytes have arrived (`written`): handed to the system for a local
  file, taken by the server for a remote one. `settle` waits until more of those on their way
  arrive, and says `false` once none are left.
- The SFTP writer sends each write as it makes it, and at every write takes the answers that have
  come, without waiting for more.
- A copy counts `bytes_done` and `bytes_copied` as `written` grows, and reports only then. After
  the last chunk of a file it settles the writes, reporting what arrives, before it finishes the
  file; it stops between them when cancelled.
- A write the server refuses never counts, and what it took before still does; the job asks
  about the file as before.

## Consequences

- The speed, the gauge, and the time left of an upload follow what the host has taken: the end of
  a file shows as it arrives, and a link that stops shows as no reports, since writes wait for
  room.
- An upload reports up to 64 times as the end of each file arrives, about as often as a download
  of it reports its chunks; a small file reports once for each batch of answers, once or a few
  times.
- The writes of a small file go out at once rather than in `finish`.
- Local writes count as they are handed to the system, as before: a disk whose cache takes data
  faster than it writes it can still run ahead, and pause later.
- The forecast still takes whole entries first, which keeps it right wherever reports run ahead of
  the data.
- Tests check that SFTP writes go out before the queue fills and count once taken, that writes
  refused do not count, that both backends count all that was written once settled, and that a
  copy counts what has arrived rather than what it wrote, what a host took before it refused the
  rest, and stops the wait for the rest when cancelled.
