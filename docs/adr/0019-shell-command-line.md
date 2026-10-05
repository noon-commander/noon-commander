# 0019. Shell command line

- Status: Accepted
- Date: 2026-10-05

## Context

mc and Far keep a command line under the panels: typing goes into it, and Enter runs the line
in the panel's directory through a persistent subshell. Noon Commander has no command line,
and so typing in a panel started quick search (`ui.type_to_search`). The backlog had a Ctrl-O
console that suspends the TUI and starts `$SHELL`, and a persistent subshell as a later
variant.

A persistent subshell has to follow the panel's directory, read the shell's own back, and
keep a pty alive for each host; it is the largest and least portable part of mc. Running one
command at a time, as F4 runs the editor, covers what users run from a file manager: a build,
an archive, a `grep`, a service restart on a host.

A vim keymap preset is planned. vim users open a command line with `:` and run a program with
`:!`; mc users type into a line that is always there. One line for both presets keeps the code
and the help in one place.

Running a command on a host is the first time Noon Commander runs anything there but
`sftp-server`. ADR 0012 forbids starting programs through a shell, and the shell is here the
point.

## Decision

- **No persistent subshell.** The variant is dropped from the roadmap. Each command runs on
  its own and ends; nothing of it, such as `cd` or `export`, outlives it.
- **Typing in a panel does nothing**, as in mc with an empty command line and in vim's normal
  mode. `ui.type_to_search` is removed, with no compatibility for it; quick search starts with
  Ctrl-S or Alt-S.
- **The line is hidden until opened**, in both presets, by the same keys: `!` opens it for a
  shell command; `:` opens it for commands of Noon Commander, of which there is only `:!cmd`
  for now, the same as `!`. `:` is kept for those commands, so that adding them later breaks
  nothing. Esc, or Backspace on an empty line, closes it. mc and Far bind neither key in a
  panel. The keymap is all that differs between the presets.
- **The line shows where the command runs**: a prompt with the host's label or alias (none
  for a local panel) and the panel's directory, shortened from the start as paths in panels
  are, then `$`. Lines after the first start with `>`. Text is shown terminal-safe through
  `noc-text`.
- **A command may span lines.** Shift-Enter, Ctrl-J, or a `\` at the end of the line followed
  by Enter start a new line; Enter runs the command. Ctrl-J arrives as LF where Enter arrives
  as CR, so it works in every terminal. Shift-Enter needs the kitty keyboard protocol: the
  event loop asks for it (`PushKeyboardEnhancementFlags`) only where
  `supports_keyboard_enhancement()` finds it, since it changes how every key arrives,
  including Esc and Alt, on which `Esc 1` … `Esc 0` and `Esc` as Alt rest. The keymap tests run
  with and without it.
- **The line grows with its text**, wrapped lines included, up to a third of the screen and
  at most ten rows; the panels shrink to make room, and beyond that the line scrolls. There is
  no setting for its height. In a command of several lines, Up and Down move between lines,
  and from the first or the last one go through the history, as fish and zsh do.
- **Pasted text never runs.** Bracketed paste (crossterm's `Event::Paste`) puts what is pasted
  into the line as it is, line breaks included; only Enter runs it.
- **Ctrl-X Ctrl-E opens the command in `$EDITOR`**, as in bash and zsh, through `noc-tools` as
  F4 does, in a temporary file. What the editor leaves goes back into the line and does not
  run.
- **Running suspends the TUI** as F4 does: the terminal goes back to the shell, the command
  runs with it, and a key returns to the panels, as mc does without a subshell. Both panels
  then read their directories again.
  - Local panel: `$SHELL -c <command>` (`/bin/sh` without `$SHELL`), started from `noc-tools`
    in the panel's directory, not through a second shell. The command is the one argument of
    `-c`; nothing of Noon Commander's is put into its text.
  - Panel on a host: ssh over the host's master connection (`-S`), built in `noc-ssh` in the
    usual order (program → forced options → `ssh.args` → role options → `--` → destination
    → remote command). The remote command is `cd -- '<directory>' || exit`, the directory
    quoted for a POSIX shell, then the command on lines of its own, so that a failed `cd`
    runs none of it; the home directory needs no `cd`. The forced options are
    `policy::COMMAND_OPTIONS` (`RequestTTY=yes`, `RemoteCommand=none`, `ControlMaster=no`,
    `PermitLocalCommand=no`), `PROCESS_OPTIONS`, and `session_options()`. Unlike every other
    ssh child, it keeps the terminal and runs in no session of its own, since the command
    needs the terminal; with the master in place it asks nothing. With
    `ssh.multiplex = false` it authenticates on its own, and ssh asks in the terminal. The
    host's task builds the command from its session, and the event loop starts it.
  - This is the one exception to ADR 0012's rule against starting programs through a shell.
- **History is one list, and each entry belongs to a host.** It is kept in a file in the
  state directory (`~/.local/state/noc/`). An entry holds the whole command, its lines
  included, the host (the alias from `ssh_config`, or local), the directory, and the time.
  The same command on the same host is not kept twice: it moves to the top with the newer
  directory. A command that starts with a space is not kept, as with bash's
  `HISTCONTROL=ignorespace`, since commands may hold secrets. `shell.history_size` sets how
  many entries are kept in all, 500 by default as bash's `HISTSIZE`; it has its row in the
  Configuration dialog.
- **Up and Down in the line (Alt-P and Alt-N as in mc) go through the history of the panel's
  host only**, so a command typed for one machine does not come up on another.
- **The history window (Alt-H as in mc, Ctrl-R as in bash)** looks like the zoxide and
  Workspaces windows. Typing filters it as quick search matches. Each row shows the host, the
  directory, and the first line of the command, with `…` and `+N` for the lines after it; the
  whole command of the row under the cursor shows below the list. The panel's host is
  highlighted, others are dimmed. Tab switches between this host, the default, and all hosts.
  Enter puts the command into the line without running it, as in mc, and does not change the
  directory; a command from another host makes the prompt flash, so that it is clear where it
  will run. Delete removes the entry.

## Consequences

- Noon Commander runs commands on hosts. Features that need that, such as checksums on the
  server or zoxide on a host, still need decisions of their own about what they run, but not
  about how.
- Letters in a panel are free; a vim preset can bind them without a mode for typing.
- `!` and `:` are text wherever a field takes text, so they act as commands only in panels;
  quick search keeps them as text while it runs.
- Commands that change the shell's state (`cd`, `export`, `source`) do nothing lasting; the
  help says so.
- The kitty keyboard protocol touches every key, so its switch is tested separately; where a
  terminal lacks it, Ctrl-J and `\` Enter still give a new line.
- Remote commands run with the user's ssh setup and the host's shell; Noon Commander quotes
  only the directory and passes the command as written.
