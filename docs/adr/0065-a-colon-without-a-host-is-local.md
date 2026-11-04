# 0065. A colon without a host is local

- Status: Accepted
- Date: 2026-11-04

## Context

Go to directory (`g d`) reads its text as `cd` in a shell, and `host:path` as scp does. On a host
every path stays on it: `/…` is the root of the host and `~` its home directory, as a shell on the
host has them. So `g d` could open another host, but not come back from one: the way back was the
location menu (`g l`), `-` right after going to the host, or the Local button once the host was
gone.

`host:` alone, and a relative `host:path`, start at the home directory on the host because sshd
starts the session there and the SFTP server reads relative paths from it, as scp does. The local
side has no such place: a local relative path is read from the directory of the process.

## Decision

- **`:` without a host is the local side**, in Go to directory: `:` alone goes to the panel's last
  local directory (the last local one of its jump list, as the Local button takes it, with the
  cursor where it was; the home directory if there is none; its own directory in a local panel).
- A relative `:path` starts at that directory; `:/…` is absolute and `:~` and `:~/…` are the local
  home directory. In a local panel `:` reads the same, so the text means one thing everywhere.
- Tab completes local paths after `:`. F5, F6, F7, and the command line do not take `:`.
- A name that starts with a colon is written `./:name`, as `./a:b` is for a name with a colon.

## Consequences

- A host can be left by typing: `g d`, `:`, Enter.
- `:name` no longer opens a directory named `:name`; that takes `./:name`.
- A reserved alias such as `local:` was not taken: it could be a host of the ssh config.
