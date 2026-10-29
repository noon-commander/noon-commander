# 0054. What runs a command of the command line

- Status: Accepted
- Date: 2026-10-29
- Amends: [ADR 0019](0019-shell-command-line.md),
  [ADR 0028](0028-programs-on-hosts-without-a-terminal.md)

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

An interactive shell also has job control: it moves to a process group of its own and gives it
the terminal. bash and dash give it back to Noon Commander's group when they end. zsh runs the
last command of `-c`, if it is no builtin, in its own place, with `exec`, so nothing gives the
terminal back once that command ends: Noon Commander is then in the background, and the first
thing it does with the terminal, raw mode, stops it with SIGTTOU; the shell it was started from
says it is suspended. And Ctrl+z stops that command with the terminal in its hands, while Noon
Commander waits for it to end, which it never does: no key reaches anything. Before `-i`, the
shell stayed in Noon Commander's group, and Ctrl+z suspended both.

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
  'make test'`. A `'` or a `\` in the text goes outside the quotes after a `\`: `'it'\''s'`,
  `'a'\\'b'`. A POSIX shell takes only `'` as special inside single quotes; fish, which may be
  the login shell that reads the remote command, also takes `\\` and `\'` there, and quoting
  a `\` as a `'` is quoted reads the same in both. The programs of ADR 0028 are quoted so too.
  `"$NAME"` reads the same in fish as in a POSIX shell; `${NAME}` and `${NAME:-default}` do not,
  so neither is written. sshd always sets `SHELL`, so there is no `/bin/sh` there.
- **A host may have its own**: `run` in its table of `hosts.toml`, the same list, in place of
  `shell.run` there, for a server whose rc files are slow or print. F4 on the host edits it in a
  field that shows `shell.run`, faint, while it is empty; empty, `shell.run` counts.
- **The shell runs as a job, as a shell runs one, and so does the editor of F4.** Where Noon
  Commander's group has the terminal, `noc-tools` (`job`, through `Shell::start` and
  `Editor::start`) starts it in a process group of its own and gives that group the terminal,
  so that Ctrl+c and Ctrl+z reach it and not Noon Commander. One that found itself in the
  background first, as an interactive shell checks at once, and stopped itself with SIGTTIN or
  SIGTTOU, is continued in front.
- **Noon Commander waits as a shell waits** (`Running::wait`): for SIGCHLD, which comes when a
  child ends or is stopped, and then asks `waitid` (`WSTOPPED`, `WNOHANG`) whether its child,
  the shell or the command zsh ran in its place, is stopped. When it ends or is stopped,
  Noon Commander takes the terminal back for its own group. From the background that takes
  SIGTTOU blocked, as shells do it: blocked in the calling thread for that one call, with `nix`'s
  `pthread_sigmask`, since `rustix` has no safe signal mask and `unsafe` is denied; `rustix` does
  the rest (`tcgetpgrp`, `tcsetpgrp`, `waitid`, `kill`).
- **A command that is stopped, as by Ctrl+z, asks what next.** `[stopped]` under its output
  says so, as `[exit N]` says a command failed, then the keys of a dialog's question, read from
  the keymap: `[stopped] Enter continue · Esc end`. Confirm (Enter) gives it the terminal back
  and continues it; Cancel (Esc, Ctrl+c) ends it as a shell ends the stopped jobs it leaves,
  with SIGHUP and then SIGCONT, with the terminal, so that it can put the terminal back as it
  was; one that goes on after that gets SIGKILL the next time. Ended so, it fails with signal
  1, as any command ended by a signal. The editor of F4 asks so too. A command that bash or zsh
  runs as a job of its own is theirs to stop: they say it is suspended and go on, or end.
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
- Ctrl+z no longer suspends Noon Commander with a command, and Ctrl+c no longer reaches Noon
  Commander while a local command runs. A command that is stopped asks, or its shell says so.
- A command on a host is not a job of Noon Commander's: ssh takes its keys in raw mode, and
  Ctrl+z and Ctrl+c go to the host's terminal, as before.
- A login shell of fish reads remote commands with `\` in them as a POSIX shell does, which it
  did not before, for the programs of ADR 0028 either.
- The shell of the command line stays the one exception to ADR 0012's rule against starting
  programs through a shell, now named `shell.run`.
- `noc-config`, `noc-ssh`, and `noc-tools` depend on `noc-vars`; `noc-tools` depends on `rustix`
  and, for the signal mask alone, on `nix`, with its `signal` feature only.
