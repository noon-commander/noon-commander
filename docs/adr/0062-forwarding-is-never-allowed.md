# 0062. Forwarding is never allowed

- Status: Accepted
- Date: 2026-11-01
- Supersedes in part: [ADR 0004](0004-forwarding-compile-time-feature.md)

## Context

ADR 0004 compiled forwarding out by default and kept a Cargo feature, `forwarding`, for a build
that would forward. No release has shipped one, and forwarding stays a non-goal. The feature
still cost: CI built, linted, and tested the workspace twice; tests and code branched on
`FORWARDING_ENABLED`; `noc debug version` and the About page carried a `features` line that
only ever named it.

## Decision

- The `forwarding` feature is gone from `noc-ssh` and `noc`. There is one build, and it never
  forwards.
- The master, direct SFTP connections, the console, commands of the command line, and programs
  run without a terminal always get `policy::SESSION_OPTIONS`, the constant that replaces
  `session_options()` of ADR 0019 and ADR 0028: `ClearAllForwardings=yes`,
  `ForwardAgent=no`, `ForwardX11=no`, `Tunnel=no`, and `GSSAPIDelegateCredentials=no`. SFTP
  channels keep `SFTP_CHANNEL_OPTIONS`.
- The validator of ADR 0004 stays as it is, except that its forwarding flags and `-o` keys are
  always rejected: the error says Noon Commander does not allow them, not that the build lacks
  a feature.
- `noc debug version` and the About page lose their `features` line; the keys after it keep
  their order.
- `just clippy` and `just test` run once; `clippy-with` and `test-with` are gone, and CI has
  one clippy and one test job.

## Consequences

- Nothing in the configuration or the build can make Noon Commander forward.
- A script that read the `features` key of `noc debug version` finds it missing.
- CI runs two jobs fewer.
- Allowing forwarding again would take a new ADR, not a flag.
