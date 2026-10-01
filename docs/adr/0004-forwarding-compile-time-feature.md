# 0004. Forwarding is a compile-time feature, off by default

- Status: Accepted
- Date: 2026-10-01

## Context

sftp-tui is SFTP only. Forwarding (ports, agent, X11, tunnels, GSSAPI credential delegation) is a
non-goal, but parts of it may be wanted later. With multiplexing, forwards from the user's
`ssh_config` would be set up by our master connection, so a ban has to cover the master, not only
the SFTP channels.

## Decision

- Cargo feature `forwarding` in `sftp-tui-ssh`, re-exported by `sftp-tui`, off by default. The
  feature adds a capability, as Cargo features should be additive, so there is no `no-forwarding`
  feature.
- `crates/sftp-tui-ssh/src/policy.rs` is the only place that checks the feature.
- Default build: the master, direct SFTP connections (`ssh.multiplex = false`), and the console
  get `ClearAllForwardings=yes`, `ForwardAgent=no`, `ForwardX11=no`, `Tunnel=no`, and
  `GSSAPIDelegateCredentials=no`. Forwarding-related user arguments are rejected with an error
  that names the missing feature.
- Build with `--features forwarding`: the master and the console follow the user's ssh config and
  arguments.
- SFTP channels always use `SFTP_CHANNEL_OPTIONS`, which mirror `sftp(1)`, in every build.
- `sftp-tui --version` shows `+forwarding` or `-forwarding`. CI builds and tests both variants.

### Validation of user-supplied arguments

Flags use an allowlist; `-o` keys use a denylist, because vendor keys such as Apple's
`UseKeychain` must pass. The validator understands bundled flags (`-4Cv`) and attached values
(`-p2222`).

| Group | Flags | `-o` keys |
| --- | --- | --- |
| Allowed | `-4 -6 -a -C -k -q -v -x`, `-B -b -c -e -I -i -J -l -m -P -p` | everything not listed below |
| Reserved by sftp-tui (always rejected) | `-f -G -M -N -n -s -T -t -V -y`, `-E -F -O -Q -S -W` | `ControlMaster`, `ControlPath`, `ControlPersist`, `SessionType`, `StdinNull`, `ForkAfterAuthentication`, `RemoteCommand`, `RequestTTY`, `PermitLocalCommand`, `LocalCommand` |
| Forwarding (allowed only with the feature) | `-A -g -K -X -Y`, `-D -L -R -w` | `LocalForward`, `RemoteForward`, `DynamicForward`, `ForwardAgent`, `ForwardX11`, `ForwardX11Trusted`, `Tunnel`, `TunnelDevice`, `GatewayPorts`, `ExitOnForwardFailure`, `PermitRemoteOpen`, `GSSAPIDelegateCredentials`, `ClearAllForwardings`, and any key containing `Forward` or `Tunnel` |

Unknown flags are errors. `-F` is rejected in favor of `ssh.config_file`, which also drives host
discovery.

## Consequences

- Default binaries cannot forward anything, whatever the configuration says.
- In a `forwarding` build, forwards live as long as the host stays connected in sftp-tui.
- On ssh builds without GSSAPI support, `GSSAPIDelegateCredentials` produces a harmless warning on
  stderr.
- A runtime switch, if ever needed, goes on top of this feature.
