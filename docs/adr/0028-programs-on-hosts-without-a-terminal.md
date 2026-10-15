# 0028. Programs on hosts without a terminal

- Status: Accepted
- Date: 2026-10-15
- Amends: [ADR 0019](0019-shell-command-line.md)

## Context

Until now Noon Commander ran two things on a host: `sftp-server`, through the SFTP subsystem, and
the commands of the command line, which the user types and which get the terminal
([ADR 0019](0019-shell-command-line.md)). The search of a whole tree
([ADR 0027](0027-search-of-a-whole-tree.md)) runs ripgrep on the host and reads what it prints,
while the TUI keeps the terminal. ssh passes a remote command to the user's shell on the host as
one string, so its words need quoting there.

## Decision

- **`Session::spawn_in` in `noc-ssh`** starts a program on the host in a directory, over the
  master connection if there is one, and returns the child with its stdout and stderr piped and
  its stdin null. Like every ssh child but the command line's, it runs in a session of its own
  without a controlling terminal, and is killed when dropped. Without multiplexing it
  authenticates on its own, through the askpass bridge.
- **The remote command** is `cd -- '<directory>' || exit`, then `exec` and the program and its
  arguments, each quoted for a POSIX shell (`'` as `'\''`), so that nothing in them, such as the
  text of a search, is read as shell syntax; the home directory needs no `cd`.
- **Forced options** (`policy::EXEC_OPTIONS`): `PermitLocalCommand=no`, `RemoteCommand=none`,
  `RequestTTY=no`, `ControlMaster=no`; then `PROCESS_OPTIONS`; over a master `BatchMode=yes`, so
  that a master gone never makes ssh ask; and `session_options()`. The order of the command line
  is as ever: program → forced options → `-F` and `ssh.args` → role options → `--` → destination
  → remote command.
- **The program is Noon Commander's choice**, never text the user typed as a command: ripgrep
  (`rg`), and fd (`fd`, then `fdfind` as Debian and Ubuntu name it,
  [ADR 0029](0029-files-and-directories-with-fd.md)), with their arguments as `noc-tools` builds
  them.

## Consequences

- Features may run a known program on a host and read its output, as long as each says what it
  runs; the program must be installed there, in the `PATH` of a non-interactive shell.
- The host's shell still reads the command, so a login shell there that is not like a POSIX
  shell, such as csh, may read the quoting otherwise; ssh offers no way around the login shell.
- fake-ssh, the test double, refuses a terminal when given `RequestTTY=no`.
