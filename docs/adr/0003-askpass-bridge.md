# 0003. Authentication prompts through an askpass bridge

- Status: Accepted
- Date: 2026-10-01

## Context

ssh asks for passwords, key passphrases, OTP codes, and host-key confirmations on `/dev/tty`.
Inside a full-screen TUI this corrupts the screen and steals input. The `openssh` crate avoids the
problem by supporting only password-less authentication, which is not acceptable for Noon Commander.

## Decision

- Every non-interactive ssh process is spawned in a new session (`setsid`), so it has no
  controlling terminal, with this environment: `SSH_ASKPASS` set to the path of the `noc`
  binary, `SSH_ASKPASS_REQUIRE=force`, the bridge socket path, and a per-process token.
- When ssh needs input, it runs `noc <prompt>`. Seeing the bridge variables, the binary acts
  as a small client: it sends the prompt and the `SSH_ASKPASS_PROMPT` hint (`confirm`, `none`, or
  unset) to the TUI over the Unix socket and prints the answer to stdout.
- The TUI shows a modal dialog: masked input for passwords and codes, yes/no for host keys, and a
  notice for FIDO touch requests (hint `none`).
- Answers are held in `secrecy::SecretString` and are never logged or written to disk.
- Protocol: one JSON line from the client (`token`, `context`, `prompt`, `hint`) and one JSON
  line back (`answer` with the text, or `cancel`). The server accepts connections only from
  the same uid and only with the right token, compared in constant time. If the client
  disconnects first, for example because ssh gave up, the UI gets a `Closed` event and drops
  the dialog.
- The command-line subcommands answer prompts on `/dev/tty` instead of in a dialog.

## Consequences

- All prompt types work, including keyboard-interactive authentication and `ProxyJump` hops,
  because child ssh processes inherit the environment.
- OpenSSH 8.4 or newer is required (8.7 for other reasons, see
  [ADR 0001](0001-wrap-system-openssh.md)).
- The askpass client path must start fast and must not initialize the TUI.
- `setsid` needs a small `unsafe` `pre_exec` block, the only planned use of `unsafe`.
