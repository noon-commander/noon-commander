# 0012. External tools in noc-tools, and zoxide

- Status: Accepted
- Date: 2026-10-03

## Context

Users of zoxide jump to the directories they use most with `z foo` in a shell, and expect a
file manager to know those directories too: yazi, ranger, lf, nnn, and joshuto work with
zoxide. Noon Commander had no history of directories and no quick way to reach a deep local
directory.

Running zoxide means running another program besides ssh. Until now only ssh and the editor of
F4 ran as child processes: ssh from `noc-ssh`, under rules of its own (forced options, `--`
before the destination, `setsid`, askpass), and the editor from the TUI, with `std::env` and
`tokio::process` inline. More programs are planned: clipboard helpers (`pbcopy`, `wl-copy`),
a shell for the console. Without a home, each would solve the same problems again: no shell,
`--` before paths, no input, a program that hangs, a program that is not installed.

zoxide's shell hook raises a directory's rank on every `cd`. A file manager passes through far
more directories than a shell does: every level on the way down, every directory looked into
and left. Recording each of them would bury the directories the user works in.

## Decision

- A crate, `noc-tools`, owns the command lines of external programs other than ssh. It
  depends on no other `noc-*` crate; the binary turns `config.toml` into its settings, as it
  does for `noc-ssh`. Programs run without a shell, with `--` before paths and the user's
  input, with stdin from `/dev/null` for background ones, and are killed when their future is
  dropped or a timeout passes. Errors are typed; a program that is not installed is told apart.
  ssh stays in `noc-ssh`. Outside tests, nothing else spawns programs.
- zoxide is the first tool: `zoxide add -- <dir>` and
  `zoxide query --list --score --exclude <dir> -- <keywords>`. Its database and its
  environment (`_ZO_DATA_DIR`, `_ZO_EXCLUDE_DIRS`, `_ZO_RESOLVE_SYMLINKS`) are the user's own;
  Noon Commander does not write it any other way.
- A local directory goes to zoxide only once the user does something in it, at most once a
  visit: a copy, move, or delete from it; a copy or move into it when a panel shows it; F7 in
  it; F3 or F4 on a file in it; checksums of its files; or a jump to it. Entering, leaving,
  marking, searching, sorting, and reading it again do not count. A visit ends when the panel
  goes to another directory. Remote directories never count: zoxide ranks local paths.
- Alt-Z (Ctrl-X Z, Esc Z) opens a window of zoxide's best directories for the keywords typed,
  asking zoxide again on each change, and opens the chosen one in the active panel. The
  matching is zoxide's own, so it ranks as `z` does in the shell.
- `[zoxide]` in `config.toml` has `program` and `record`, which defaults to on: without zoxide
  nothing happens, and a missing program is logged once.

## Consequences

- A new tool goes into `noc-tools` with a fake program in its tests, as `fake-zoxide` is; the
  real database is never touched by tests.
- The editor of F4 moved there too; handing the terminal over stays in the TUI.
- Typing in the zoxide window starts a short process per key; a query still running when the
  next one starts is dropped, which kills it.
- zoxide only learns from what the user does in Noon Commander; directories passed through are
  forgotten, and a directory typed as the target of a copy counts only if a panel shows it.
- zoxide on servers, over the host's master connection, would need Noon Commander to run
  remote commands, which it does not do yet.
