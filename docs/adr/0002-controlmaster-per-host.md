# 0002. One ControlMaster connection per host

- Status: Accepted
- Date: 2026-10-01

## Context

Panels, background transfers, and later the console all need channels to the same host. Separate
ssh connections would repeat authentication (passwords, OTP codes, hardware-key touches) for each
of them.

## Decision

For each connected host, sftp-tui starts one master connection and multiplexes everything else
over its control socket:

```text
ssh -M -N -S <sock> -o ControlPersist=no <master options> -- <alias>  # authenticates once
ssh -S <sock> -T -s <channel options> -- <alias> sftp                 # panel channel
ssh -S <sock> -T -s <channel options> -- <alias> sftp                 # transfer channels
ssh -S <sock> -O exit -- <alias>                                      # disconnect
```

- The master is our child process (`ControlPersist=no`), so we see its exit status and stderr.
- Master options: `PermitLocalCommand=no`, `RemoteCommand=none`, `RequestTTY=no`, plus the
  forwarding policy ([ADR 0004](0004-forwarding-compile-time-feature.md)). Channel options:
  `policy::SFTP_CHANNEL_OPTIONS`.
- Our `-S` overrides any `ControlPath` from the user's config; channels use `ControlMaster=no`.
- Sockets live in `$XDG_RUNTIME_DIR/sftp-tui/` or `$TMPDIR/sftp-tui-$UID/` (mode 0700) and have
  12-hex-digit names. macOS limits socket paths to 104 bytes, and ssh appends a 17-character
  temporary suffix while creating the socket, so the path length is checked at startup.
- `ssh.multiplex = false` falls back to one connection per channel, for example for servers with
  `MaxSessions 1`.

## Consequences

- One authentication per host; new channels open instantly.
- If sftp-tui crashes, a master may outlive it. On startup we close stale sockets with `-O exit`;
  on SIGTERM and SIGHUP we shut the masters down.
- Unix only.
