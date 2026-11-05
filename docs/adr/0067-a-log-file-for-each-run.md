# 0067. A log file for each run

- Status: Accepted
- Date: 2026-11-05

## Context

Noon Commander logged with `tracing` to one file, `~/.local/state/noc/noc.log`, appended to by
every process that loaded the config: the TUI, `noc debug hosts`, and `noc debug ls`. Nothing
removed old lines, so the file only grew; two instances wrote their lines into each other; the
filter came from `NOC_LOG` alone, which no setting showed; a log that could not be opened was
silently dropped; and each event was written to the file in the task that made it, blocking I/O
in async code.

## Decision

- **Only the TUI logs.** The subcommands have the terminal to say what they do; they write no log
  file and no log to stderr.
- **A file for each run** in `$XDG_STATE_HOME/noc/logs/`:
  `noc-<UTC time it started, as 20261105T142301Z>-<process ID>.log`, created new, with mode
  0600. The time first makes the names sort as the files were made; the process ID tells apart
  two that started in the same second.
- **Rotation by runs.** At the start, and when the setting changes, the oldest files named
  `noc-*.log` go until `log.keep` are left (default 10), the file of this run always among them.
  Other files in the directory are left alone, the old `noc.log` too. There is no limit on the
  size of a file.
- **Settings, not a variable.** `[log]` in `config.toml`: `level` (`off`, `error`, `warn`,
  `info`, `debug`, `trace`; default `info`), `filter` (`EnvFilter` directives that take the
  place of `level` unless empty, such as `info,noc_ssh=trace`, checked at load and in the
  dialog), and `keep`. Options → Configuration… has them in the category Log; they take effect at
  once, the filter through `tracing_subscriber::reload`. `NOC_LOG` is gone.
- **A thread of its own writes the file.** Each event is formatted into a buffer of its own,
  which goes whole over an unbounded channel to the thread `noc-log`; sending never waits. The
  thread flushes whenever no more lines wait. As the TUI ends, the log ends: an empty line, which
  no event makes, tells the thread to write what came before and stop, and the TUI waits for it
  in a blocking task.
- **Spans for the work that lasts.** `host{host, connection}` for the tasks of a connection,
  `ssh_master{destination, pid}` for a master from its start to its end, `job{id, kind}` for
  copy, move, delete, and checksums, `search{generation, kind}` for a search once its delay is
  over, and `askpass{id}` for a prompt. All are at level info, so that warnings in them carry
  their context at the default level, and each closes with a line that says how long it took.
- **A log that cannot be written is said.** The TUI starts all the same, with an error dialog
  that names the directory or the file and why; the About page of the help says `none:` and why
  in place of the file.

## Consequences

- A run's log is whole and its own, so it can be attached to a report as it is.
- The logs directory holds at most `log.keep` files of noc, however long each run was.
- A run with a small `log.keep` can remove the file of another instance still running, which
  then writes on to a file without a name until it ends.
- Lines of events after the log ended, as the runtime shuts down, are lost.
- The channel is unbounded: a flood of events at `trace` takes memory until the thread catches
  up, rather than slowing the TUI down.
