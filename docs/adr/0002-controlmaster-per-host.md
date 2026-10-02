# 0002. One ControlMaster connection per host

- Status: Accepted
- Date: 2026-10-01

## Context

Panels, background transfers, and later the console all need channels to the same host. Separate
ssh connections would repeat authentication (passwords, OTP codes, hardware-key touches) for each
of them.

## Decision

For each connected host, Noon Commander starts one master connection and multiplexes everything else
over its control socket:

```text
ssh <master options> -M -N -S <sock> -o ControlPersist=no -- <alias>  # authenticates once
ssh <channel options> -S <sock> -T -s -- <alias> sftp                 # panel channel
ssh <channel options> -S <sock> -T -s -- <alias> sftp                 # transfer channels
ssh -F /dev/null -S <sock> -O exit -- noc                             # disconnect
```

- The master is our child process (`ControlPersist=no`), so we see its exit status and stderr.
  It is ready when its control socket appears: ssh creates it only after authentication.
- Master options (`policy.rs`): `PermitLocalCommand=no`, `RemoteCommand=none`, `RequestTTY=no`,
  `StdinNull=no`, `ForkAfterAuthentication=no`, plus the forwarding policy
  ([ADR 0004](0004-forwarding-compile-time-feature.md)). Channel options:
  `SFTP_CHANNEL_OPTIONS`, `StdinNull=no`, `ForkAfterAuthentication=no`, and `BatchMode=yes`:
  if the master is gone, ssh falls back to a direct connection, which must not prompt.
- Our `-S` overrides any `ControlPath` from the user's config; channels use `ControlMaster=no`.
- `ssh -O` commands only talk to the socket, so they run with `-F /dev/null`: no config is
  evaluated, and no `Match exec` runs.
- Sockets live in `$XDG_RUNTIME_DIR/noc/` or `$TMPDIR/noc-$UID/` (mode 0700) and are
  named `cm-<pid>-<8 hex digits>`. macOS limits socket paths to 104 bytes, and ssh appends a
  17-character temporary suffix while creating the socket, so the path length is checked
  before connecting.
- Shutdown: `ssh -O exit`, then SIGTERM, then SIGKILL to the master's process group, which also
  stops a `ProxyCommand`.
- `ssh.multiplex = false` falls back to one connection per channel, for example for servers with
  `MaxSessions 1`.

## Consequences

- One authentication per host; new channels open instantly.
- If Noon Commander crashes, a master may outlive it. On startup, Noon Commander looks for sockets
  whose owner pid is no longer running, closes their masters with `-O exit`, and removes the
  files; sockets of running instances are left alone.
- Unix only.
