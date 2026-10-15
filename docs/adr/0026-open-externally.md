# 0026. Opening files externally with the desktop's opener

- Status: Accepted
- Date: 2026-10-15
- Amends: [ADR 0012](0012-external-tools-and-zoxide.md)

## Context

A file manager in a terminal still runs on a desktop. PDFs, images, and office documents
need the programs the desktop has for them, and a directory sometimes needs the desktop's file
manager. Far opens them with Shift+Enter, and yazi and ranger through openers that fall back to
the desktop's. Noon Commander could only view a file (F3) or edit it in the terminal (F4).

The desktop opens a path as a double click does through `open` on macOS and `xdg-open` on Linux.
Both differ from the other tools of ADR 0012:

- They start a program that should outlive them, and Noon Commander too. `open` asks
  LaunchServices and exits at once. `xdg-open` often execs a program or waits for one, which
  then inherits its stdout and stderr.
- `xdg-open` takes exactly one path and no `--`: it rejects any argument that starts with `-`.

ADR 0012 says programs are killed when their future is dropped or a timeout passes, read from
pipes, and take `--` before paths.

## Decision

- `g x` opens the marked entries, or the one under the cursor, as vim's `gx` opens what is under
  its cursor. In Visual mode it opens the rows selected, as the other commands of files act on
  them. The File menu has it as Open externally. Shift+Enter is not bound: without the kitty
  keyboard protocol, terminals send it as Enter.
- `noc_tools::opener` runs `open` on macOS and `xdg-open` elsewhere. It runs once per path, one
  after another, so both work the same way. The first failure stops the rest.
- The opener runs without a shell, as ADR 0012 requires. Unlike the other tools:
  - Its stdin, stdout, and stderr are `/dev/null`, not pipes. A program it starts would inherit a
    pipe and die on a write once Noon Commander closed it.
  - It runs in a process group of its own, so the Ctrl+c of a command that has the terminal does
    not reach it.
  - It is never killed. Noon Commander waits up to five seconds for it to exit and reports a
    failure. One still running then counts as having opened its file and is left alone.
  - It takes the path alone, absolute, without `--`, as the editor of F4 takes its file. An
    absolute path never starts with `-`.
- Only local paths open. On a host, the line of the mode says so, and the File menu dims the
  command. Opening a remote file through a local copy is left for later.

## Consequences

- A failure shows only the opener's exit code, since its output goes nowhere. `xdg-open`
  documents its codes: 2 for a missing file, 3 for no program to open it, 4 for a failed action.
- No setting names the opener. A user who wants another program can put one named `xdg-open`
  earlier in `PATH`.
- Opening many marked files starts one program per file. On macOS, an application gets them one
  by one, not as one `open` with several paths.
- `g x` in a directory counts for zoxide, as F3 and F4 on a file do.
