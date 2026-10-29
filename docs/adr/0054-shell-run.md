# 0054. What runs a command of the command line

- Status: Accepted
- Date: 2026-10-29
- Amends: [ADR 0019](0019-shell-command-line.md)

## Context

ADR 0019 ran a command of the command line as `$SHELL -c <command>` locally, and on a host gave
the command to the host's login shell, which sshd runs as `$SHELL -c`. Neither shell is
interactive, so neither reads the rc files where users keep their aliases and functions: zsh
reads `~/.zshrc` only when interactive, and bash, which may read `~/.bashrc` when sshd starts
it, expands no aliases without `-i`, and most `~/.bashrc` files return at once when not
interactive. fish reads `config.fish` either way. `ll`, `gst`, or a function from the rc files
did not work after `!`.

`$SHELL -i -c` reads them. Bash and zsh with `-i` take no history expansion from the text of
`-c`. An interactive shell starts slower, by as much as the rc files take, and the rc files of a
server may be heavier than those of a laptop, or print.

## Decision

- **`shell.run` says what runs a command**: a list of words, the program and its arguments,
  after which the command comes as one more argument, as typed. Its default is
  `["$SHELL", "-i", "-c"]`. It is a list, as `ssh.args` is, so that no word needs splitting;
  the Configuration dialog shows it as words, under Command line → Run with, and a change counts
  from the next command.
- **`$NAME` and `${NAME}` in the words take the value of a variable where the command runs**,
  and `$$` is a `$`. A name is a letter or `_`, then letters, digits, and `_`; a `$` before
  anything else stays a `$`, and a `${` without its `}`, or around what is no name, is an error,
  in `config.toml` and in the dialog. Nothing else is special: a value is never split into
  words, matched as a pattern, or given a `~`.
- **Locally**, Noon Commander expands the variables from its own environment and starts the
  program from `noc-tools`, without a second shell. A program that comes out empty, as without
  `$SHELL`, is `/bin/sh`, as before.
- **On a host**, the host's shell expands them. The remote command is `cd -- '<directory>' ||
  exit`, as before, then on a line of its own `exec`, each word with its text in single quotes
  and each variable as `"$NAME"`, and the command in single quotes: `exec "$SHELL" '-i' '-c'
  'make test'`. `"$NAME"` and `'…'\''…'` read the same in fish as in a POSIX shell; `${NAME}`
  and `${NAME:-default}` do not, so neither is written. sshd always sets `SHELL`, so there is no
  `/bin/sh` there.
- **A host may have its own**: `run` in its table of `hosts.toml`, the same list, in place of
  `shell.run` there, for a server whose rc files are slow or print. F4 on the host edits it in a
  field that shows `shell.run`, faint, while it is empty; empty, `shell.run` counts.
- **Text fields of dialogs show a placeholder**: faint text while the field is empty, which is
  never its value, in the style of the hints of settings. The field of `run` is the first.
- **The variables are read by a crate of their own, `noc-vars`**, which depends on no other
  `noc-*` crate. `noc-config` keeps the words as its `Template`s, `noc-tools` expands them, and
  `noc-ssh` quotes their pieces; so the three depend on it, and a later setting with variables
  in it reads them the same.

## Consequences

- Aliases and functions of the user's rc files work after `!`, locally and on hosts, and so do
  settings the rc files make, such as `PATH`. Each command starts as slowly as the shell with
  its rc files; `["$SHELL", "-c"]` gives the speed back, for all hosts or for one.
- What the rc files print shows before the output of the command, on the terminal's screen.
- `! $SHELL` starts a shell inside the shell of `shell.run`; it works as before.
- The shell of the command line stays the one exception to ADR 0012's rule against starting
  programs through a shell, now named `shell.run`.
- `noc-config`, `noc-ssh`, and `noc-tools` depend on `noc-vars`.
