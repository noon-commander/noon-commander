# 0064. Jobs wait for a lost host

- Status: Accepted
- Date: 2026-11-04

## Context

A job on a host ended with the connection of the host. A copy of many files to a host whose
connection dropped for a moment stopped there: the user had to start it again, which asked
about every file it had copied already, as each name was taken now. A delete on a host ran in the
task of the host, and went with it.

The job had the SFTP session of the connection it started with (`Arc<SftpFs>`), which the task
of the host shared. A new connection has a new session, and the job could not take it.

## Decision

- **A job waits for its host.** When the connection of a host is lost, its jobs stay. The
  dialog Connection says that the connection was lost, with Retry and Cancel, as it did; the jobs
  ask nothing of their own. Their windows say `Waiting for web to connect…`, and the list of jobs
  says `offline`. A failure a job meets while the host is away does not ask: it waits for the
  host. A failure it asked about before the app heard of the loss stops asking, and waits too.
- **Retry goes on where the job was.** Once the host is connected again, each job whose hosts are
  all connected gets Retry for the failure it waits on, and tries the step again through the new
  connection. What it had done stays done: nothing is copied twice, and nothing asks again about
  the names it took.
- **Cancel ends the jobs.** Cancel in Connection gives the host up, and the jobs on it with it,
  as Abort would. So does disconnecting the host, and a connection the user asked to stop. Abort
  in the window of a waiting job stops it alone.
- **A job reaches its host through a link** (`HostLink`): it follows the host from one connection
  to the next (`Relink`, which the app tells of each connection). The SFTP session of a job
  (`Rejoin`, a `Vfs`) takes the session of the connection the host has the first time it needs
  one, and that of the next connection once the host is connected again. A reader or a writer
  opened before stays on the old session and fails, and the job opens its file again. A delete
  on a host runs in a task of its own, as a copy and a checksum do, not in the task of the host.
- **A file whose copy failed starts over.** A copy writes a file from its start, so a retry after
  a lost connection writes the whole file again. What the first attempt left, which it could not
  remove with the connection gone, is replaced without asking if it is the target, or removed
  first if it is a temporary name of an atomic copy.

## Consequences

- A copy of many files survives a connection that drops, with one dialog and one key.
- A job waits as long as the user leaves Connection open; its clock stands meanwhile, as it does
  while any question waits.
- `noc-ops` knows nothing of connections. It tries the step again on Retry, as it always did; the
  app holds the answer until the host is back.
- A file whose copy was cut off is copied again in whole: there is no resuming within a file
  yet. The roadmap has it, under resumable transfers.
- A job that the host lost before its failure reached the app may ask about that failure once the
  host is back, as the error came after the new connection. Retry then works.
