# 0001. Wrap the system OpenSSH client

- Status: Accepted
- Date: 2026-10-01

## Context

Noon Commander needs SSH connections that behave exactly like the user's `ssh`: `~/.ssh/config` with
`Host`, `Match`, `Include`, `ProxyJump`, and `ProxyCommand`; agents, FIDO keys, certificates, and
Kerberos; `known_hosts`. SSH libraries such as russh and libssh2 reimplement subsets of this, and
each needs its own configuration and authentication code.

## Decision

Noon Commander never implements SSH. It spawns the system OpenSSH client and speaks the SFTP
protocol over the stdin and stdout of `ssh -s <host> sftp`.

- The SFTP client library is `openssh-sftp-client`; its `Sftp::new` accepts any pipes. The
  fallback is `russh-sftp`. Both sit behind our `SftpFs` adapter.
- The `ssh` binary and extra arguments are configurable: `ssh.program` and `ssh.args`. (Per-host
  `args` existed until [ADR 0007](0007-typed-host-settings-in-hosts-toml.md).)
- OpenSSH 8.7 or newer is required: 8.4 added `SSH_ASKPASS_REQUIRE`
  ([ADR 0003](0003-askpass-bridge.md)), and 8.7 added the `StdinNull` and
  `ForkAfterAuthentication` keywords, which Noon Commander forces off so that a user's config cannot
  close ssh's stdin or send it to the background. `noc` checks the version with `ssh -V`
  before connecting.
- SSH implementation crates are banned in `deny.toml`.

## Consequences

- Everything the user's ssh setup supports works without code on our side.
- We depend on the `ssh` command-line contract: argument syntax, exit codes, and stderr text.
  Errors are less structured than with a library.
- Process management (spawning, killing, cleanup) is our responsibility.
- Windows is out of scope, because OpenSSH for Windows lacks `ControlMaster`
  ([ADR 0002](0002-controlmaster-per-host.md)).
