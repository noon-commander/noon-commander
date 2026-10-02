# 0007. Typed host settings in hosts.toml, written by the TUI

- Status: Accepted
- Date: 2026-10-02

## Context

Host settings lived in `config.toml` as `[hosts."alias"]` tables: a label, a remote start
directory, and extra ssh arguments. They could only be changed by editing the file. Users want
to change them from the host list, and want more of them: a directory for the other panel when a
host opens, and a way to return to the last directory shown on a host.

Hosts other than SFTP may come later, from other backends or plugins, with settings of their
own. An "other panel" directory may then name a place on another host, not only a local path.

Per-host ssh arguments duplicated what a `Host` block in ssh_config does, and every one of them
had to pass the argument validator.

## Decision

- Host settings move to `hosts.toml`, next to `config.toml` (so `--config` moves both). Each
  host is a top-level table keyed by its name, with a required `type` that decides which other
  keys it may have; unknown types and keys are errors.
- `type = "sftp"` names a Host alias from ssh_config. Its table only decorates the host:
  `label`, `start_dir`, `other_dir`, `remember_dir`. How to reach the host stays in ssh_config;
  a table for an alias that ssh_config does not have is ignored. Later types, such as hosts from
  plugins, may define the host itself.
- `other_dir` is a local path for now and must start with `/` or `~/` (or be `~`). Other forms
  are rejected, which leaves them free for later, such as `host:/path`.
- `remember_dir` returns to the last directory shown on the host in this session, if it is still
  there, before `start_dir`. Nothing is written to disk for it.
- Per-host ssh `args` are removed; `ssh.args` stays for every invocation, and per-host options go
  in ssh_config. The ssh command line is: program → forced options → `-F` and `ssh.args` → role
  options → `--` → destination. The `ssh -G` cache is keyed by destination alone (format 2).
- F4 on a host edits its table. Noon Commander reads the file again before writing, changes only
  that table through `toml_edit`, so comments, formatting, and other tables stay, removes values
  that are defaults and a table left with none, refuses to touch a file that is not valid TOML,
  and replaces the file atomically, following a symbolic link (dotfile managers link it).
- A `[hosts]` table left in `config.toml` is an error that says where it went; the file is not
  migrated automatically.

## Consequences

- Noon Commander writes one of its own config files at runtime. `config.toml` stays the user's;
  only `hosts.toml` is written, and only one table at a time.
- `toml_edit` is a new dependency, from the same project as `toml`, sharing its parser crates.
- Host settings can change while Noon Commander runs: they sit behind a lock in the shared
  context, so host tasks that are already connected see a new `start_dir` on the next open.
- Configs with `[hosts]` stop loading until the tables are moved, and `args` must go to
  ssh_config. This is acceptable before the first release.
