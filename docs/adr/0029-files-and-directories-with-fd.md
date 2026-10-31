# 0029. Files and directories with fd

- Status: Accepted; amended by [ADR 0057](0057-combo-boxes.md): the modes are in a combo box
- Date: 2026-10-16
- Amends: [ADR 0027](0027-search-of-a-whole-tree.md),
  [ADR 0028](0028-programs-on-hosts-without-a-terminal.md)

## Context

The search of a whole tree ([ADR 0027](0027-search-of-a-whole-tree.md)) finds text in files with
ripgrep, and with no text lists the files. Finding a file or a directory by its name takes
another tool: ripgrep lists files only, never directories, and cannot match a pattern against
names. mc's Find File and Far's Find file ask for a mask of names; fzf, telescope, and yazi find
names with fd.

fd walks a tree as ripgrep does, with the same ignore files, matches its pattern against the name
of each entry with the same regex engine, and prints directories with a `/` at the end (since fd
8.3, 2021).

## Decision

- **The dialog of `g /` has two modes**, radio buttons on a line above the fields: Text, as
  before, and Files and dirs, which finds files and directories by name with fd. Either change
  searches again, as the fields and the check boxes do.
- **fd runs again on every change**, as ripgrep does, 150 ms after the last key; the text goes to
  fd as its pattern and fd matches it: a regular expression, or as it is (`--fixed-strings`)
  without the first check box; case only once the text has a capital letter, fd's smart case, or
  always with the second (`--case-sensitive`). It matches the name only, not the path, as the
  quick search of the panels does.
- **The Files field**: a glob after `!` goes to fd as `--exclude`, which leaves out what it
  matches, a directory with all below it. fd cannot take the other globs with a pattern, so
  `noc-tools` keeps the entries that match one of them, with `globset`, ripgrep's glob engine: a
  glob without `/` against the name, one with `/` against the path, as in a `.gitignore`.
- **What matched shows** in the colors of what a search found (`find_match`): `noc-tools` matches
  the name again with the `regex` crate, fd's engine, as fd did.
- **fd runs from `noc-tools`** (`fd.rs`), with `--print0 --color never --show-errors`, and
  `--hidden` while the panels show hidden files. `[search] fd` (`fd`) names it, and
  `[search] max_results` bounds what it finds, as for ripgrep. The About page of the help and
  `noc debug version` show what `fd --version` says.
- **On a host fd runs there**, over the master without a terminal
  ([ADR 0028](0028-programs-on-hosts-without-a-terminal.md)): `fd`, and when the host's shell
  does not find it (127), `fdfind`, as Debian and Ubuntu name it. Without either, the dialog says
  that fd is not installed.
- **The rows** are the files and directories found, in the order of their paths, each with its
  icon or marker and its path, which loses its start where it is too long, since the name is at
  its end. Under them, how many it found. The buttons are Go to and Cancel: Go to goes into a
  directory, or to a file in its directory with the cursor on it, and where the panel was goes to
  the jump list. F3 and F4 view and edit a file found, from its start; on a directory they do
  nothing.

## Consequences

- Finding names needs fd, locally and on each host searched; the text of files still needs
  ripgrep.
- A link to a directory shows as a file, as fd marks only directories; Go to puts the cursor on
  it. fd before 8.3 marks no directory, so Go to puts the cursor on every entry found.
- A glob with a `/` in the Files field matches paths here, as `.gitignore` does, which differs a
  little from what ripgrep does with it in the Text mode.
- Hosts run a second program that the user did not type; ADR 0028 lists them.
