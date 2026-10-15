# 0027. Searching a whole tree with ripgrep

- Status: Accepted
- Date: 2026-10-15
- Amends: [ADR 0012](0012-external-tools-and-zoxide.md), [ADR 0022](0022-vim-modes.md)

## Context

Quick search finds names in the listing of the panel only. A file somewhere below it, or the file
that holds a line, takes going down the directories one by one. mc's Find File (Alt+?) and Far's
Find file (Alt+F7) ask for a file mask and a text in a dialog, then list each file found, and mc
the lines that matched, in a window of their own, where Enter goes to the file and F3 and F4 view
and edit it. VS Code's search has a field for the text, one for the files, and switches for
regular expressions and case, and searches as they change; telescope's `live_grep` searches with
ripgrep as the text is typed.

ripgrep searches a tree for text faster than anything Noon Commander could do through its `Vfs`,
respects `.gitignore`, reports in JSON, and is installed on most machines of developers. Reading a
remote tree through SFTP to search it would read every file over the network; ripgrep on the host
reads them where they are.

A first version read the tree through the `Vfs` and filtered the paths with the fuzzy filter of
the lists. It found names only, and had to read a whole remote tree over SFTP.

## Decision

- **`g /` in a panel** opens a window that searches the files below the panel's directory, typing
  in its text; F9 → Command → Find file… does the same. `g /` stands next to quick search's `/`,
  as vim's `g` keys stand next to the plain ones.
- **Two fields**: the text, and the files, whose globs, split at blanks, each go to ripgrep as a
  `--glob`, `!` before one leaving those files out. Tab and Shift+Tab go between them. With no text
  the window lists the files that the globs let through (`rg --files`).
- **The text** is a regular expression, or with Alt+r the text as it is (`--fixed-strings`). Its
  case counts only with a capital letter in it (`--smart-case`), or with Alt+c always
  (`--case-sensitive`). `.*` and `Aa` right of the field show the two, bright while on, as VS Code
  shows them.
- **It searches as either field changes**, 150 ms after the last key, so that a word typed starts
  one search; a new one stops the one before. The window opens listing the files at once.
- **ripgrep runs from `noc-tools`** (`ripgrep.rs`), with `--json` for lines, which keeps paths and
  lines that are not UTF-8, and `--files --null` for files; with `--no-config`, so that the
  user's `RIPGREP_CONFIG_PATH` cannot change what it prints; with `--hidden` while the panels show
  hidden files (`ui.show_hidden`); never with `--follow`. It respects `.gitignore` and `.ignore`.
  `[search] program` (`rg`) names it. It goes as deep as the tree does: ripgrep is fast, follows
  no link, and leaves out what `.gitignore` does, so a limit on the depth would only hide files.
  What bounds it is what it finds: it stops after `[search] max_results` (10000) lines, or files
  when it lists them, since the first letter typed in a large tree finds millions, and takes at
  most `[search] max_per_file` (100) lines of one file (`--max-count`), so that one large log
  leaves room for the others; a file that reached it shows its count as `100+`. All three are in a
  Search category of the Configuration dialog, and the About page of the help and
  `noc debug version` show what `rg --version` says.
- **On a host, ripgrep runs there** over the host's master connection, without a terminal
  ([ADR 0028](0028-programs-on-hosts-without-a-terminal.md)): `rg` as the host's `PATH` finds it.
  Without it, the window says that ripgrep is not installed; the panels do not fall back to
  reading the tree through SFTP.
- **It runs in a task of its own**, cancel-safe, which reads ripgrep's stdout and its stderr at
  once, so that neither pipe fills, and reports what it found at most every 50 ms. Closing the
  window or a new search stops it through a `CancellationToken`, and the child is killed on drop;
  each search has a generation, so the reports of an earlier one are dropped. Its rows stay on
  screen until the next one finds something or ends.
- **The window** shows each file found, in the order of their paths, with how many of its lines
  matched, and the lines under it with their numbers, without the blanks they start with, the
  matches highlighted; a line too long for the window starts a little before its first match.
  Esc goes to the rows, in Normal mode, where `i` and `/` go back to the field. Right of the files,
  how many lines it found in how many files, and the spinner while it searches; under the rows,
  that it stopped at the limit, and how many errors ripgrep told of, with the last. A pattern that
  is not a regular expression shows ripgrep's error in place of the rows.
- **Keys act on the row under the cursor**, as in the results of mc's Find File: Enter goes to the
  file in the active panel, its directory with the cursor on it, and where the panel was goes to
  the jump list; F3 views it from the line, at the top of the viewer, and F4 edits it at the line,
  a remote file as a local copy as F4 always does. On a file's row, they take its first line that
  matched. The window stays after F3 and F4.
- **The editor gets the line** as it takes it: `+N file` for vi, vim, Neovim, nano, Emacs, micro,
  and kak; `-g file:N` for VS Code and its kin; `file:N` for Helix, Sublime Text, and Zed.

## Consequences

- Searching needs ripgrep, locally and on each host searched; nothing else in Noon Commander does.
- Files that `.gitignore` leaves out are not searched, as with ripgrep on the command line; a glob
  in the files field does not bring them back.
- A search on a host runs a program there that the user did not type, the first besides
  `sftp-server`.
- What the search found is not kept: the next `g /` searches again. There is no panelize, as mc's
  and Far's, which would put the files found in a panel to act on.
- The viewer reads the start of a file only; a line past it shows the end of what was read.
