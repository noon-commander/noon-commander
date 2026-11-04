# 0063. Errors in parts, and the order of buttons

- Status: Accepted
- Date: 2026-11-02
- Amends: [ADR 0039](0039-errors-of-panels-in-dialogs.md),
  [ADR 0059](0059-one-dialog-saves-a-file.md), [ADR 0060](0060-the-question-about-a-taken-name.md)

## Context

An error dialog said what failed in one sentence: `Cannot delete /a/long/path: permission
denied`. A long path wrapped into the reason, and the reason ended up in the middle of a line or
at its end, where it was easy to miss.

A job said what failed by the kind of job alone. A copy that could not read its source said
`Cannot copy to /the/source`, which names the wrong file and the wrong thing.

The buttons of a failed job were mc's, Ignore, Ignore all, Retry, and Abort, with Ignore as the
default. The question about a taken name called the same thing Skip. Every button showed every
time: Ignore all at the last entry, Ignore for the only one, where it does what Abort does, and
Retry where the target is the source itself, which no retry mends.

The dialog after a directory could not be read had OK first, which did what Esc does. Renaming
or saving onto a file asked Yes or No, with neither file's size or time and no other name to
take. Replacing a workspace by Save as had Yes as the default, while renaming one onto another
had No.

## Decision

- **An error shows a problem in parts** (`Problem`, `Dialog::failure`). First what could not be
  done, such as `Cannot delete`. Then the path on a line of its own, cut in the middle where too
  long. Then, under a line across the frame, as over The rest of the question about a taken name,
  the reason, which wraps, and under it always the code the system gave the failure, its number
  in bold: `Code: 13` for an `errno`, `SFTP code: 3` for a status of SFTP, which does not tell the
  `errno` on the host. Where neither gave one, as for what Noon Commander found itself or what ssh
  said, the code is a dash, `Code: —`, so that its place is always there. The number alone shows,
  not its name, such as `EACCES`, which would take a table for each system or `nix`; the words of
  the reason leave out the `(os error 13)` that the system puts after them. `VfsError` keeps the
  code (`VfsError::code`), and a `Reason` carries it with its words to the dialog. Every dialog
  of an error that has a path, a host, or a file and a reason shows it so. An error with no reason
  apart, such as a name that cannot be a name, stays one sentence. Reasons start with a capital,
  as they start a line.
- **A job says what it could not do.** `noc_ops::Event::Failed` carries the `Step` that failed:
  inspect, list, read, create, write, set attributes, rename, or remove. The dialog says it:
  `Cannot read`, `Cannot create`, `Cannot set the attributes of`.
- **A failed job asks as the question about a taken name does**: the buttons are for this
  failure, Retry first and the default, Skip, and Abort; under a line, a combo box, The rest, is
  for the failures after it: Ask each time, Skip every failure, and Skip failures with the same
  code, which leaves each failure with that code without asking and asks about the others. A
  failure without a code has no such choice. The answer is a `noc_ops::Resolution`: a `Decision`
  and a `Later`. Skip is the word of the question about a taken name too: one button, one word.
- **What would do nothing new does not show.** No Retry where it would fail again, as when the
  target is the source itself; Skip is then the default. No The rest at the last entry. No Skip
  for the only entry, where it is Abort. The question about a taken name drops The rest at the
  last entry and Skip for the only one, where Abort is the default. Where the job still counts,
  it does not know what is left, and everything shows.
- **The main way on comes first, the way out last.** The way out is the last button, and Esc
  and F10 press it. Which word it has follows from what stands behind the dialog:

  | Button | What it does | Where |
  | --- | --- | --- |
  | Abort | Stops a job that runs: some of its work is done, and what is left is not | The failure of a job, the question of a job about a taken name, the window of a job, the list of jobs |
  | Cancel | Does nothing: nothing has begun, or what it gives up is one thing | Forms, Connection, a host not connected, a directory that could not be read, a rename or a save onto a file, the settings of a host, the results of a search, a password of ssh |
  | No | Answers a question asked as one | Delete, Quit while jobs run, replacing a workspace, the questions of ssh |
  | OK | Takes a form, or closes what has nothing to decide | Forms, and every error with nothing to do but read it |

  Abort has a letter; Cancel has none, as Esc and F10 cancel. So one question, about a taken name,
  ends in Abort in a copy, which stops the copy, and in Cancel in a rename or a save, which gives
  up only that. The dialog after a directory could not be read has Retry, Up, Go to, and Cancel,
  which takes the place of OK, as OK did what Esc does. Its default is Up where the panel has
  nothing to show, and Retry otherwise.
- **Renaming or saving onto a file asks the question of a copy** about a taken name, for one
  file: the path, New and Existing with their sizes and times, each viewing its file, and
  Overwrite, Rename…, and Cancel, the default. For a save, New views the bytes it would write and
  has no time. Rename… offers a free name; a rename takes a name in its directory alone.
- **An error that has a way on offers it first, the default, and Cancel** (`Way`):

  | Way | Errors | What it does |
  | --- | --- | --- |
  | Retry | F7, a save, F3, a part F3 could not read, the editor of F4, an edited copy that did not go back, the shell of `!` and `:`, the configuration, the settings of a host, the workspaces, the history | Does the same again (`Again`) |
  | Connect | Any of these, and F5, F6, F8, `f #`, F4, a rename, whose host is not connected | Connects to the hosts, and does it once they are connected (`App::pending`) |
  | Change… | Shift+F6 that failed, F5 or F6 onto or into the source, F7 onto a name that is taken | Opens the field or the dialog again with what was typed |

  Connect takes the place of Retry, which would fail again. What waits for a host goes with it
  when the user gives the host up in Connection, or disconnects it. A rename that failed opens
  its field again, as the name typed is what most often needs a change, and Enter there tries
  again. Cancel after the editor could not run ends the edit, as if the file did not change.
  What Retry would fail at again keeps OK: `g x`, whose program ran and said no, a name that
  cannot be a name, and what else was typed wrong in a form, over which most of these show. F3
  over the question about a taken name keeps OK too, as its line views the file again.
- **Replacing a workspace by Save as has No as the default**, as renaming one onto another has.
- **Errors that had their reason in a sentence show it in parts too**: a shell of `!` or `:` that
  could not run (`Cannot run /bin/zsh`, then why and its code), and an edited copy of a remote
  file that did not go back (`The changes did not go back to`, the remote file, then why and where
  the copy stays).

## Consequences

- The reason is always where the eye looks for it, on its own line, apart from the path.
- What failed in a copy names the right file and the right step.
- The buttons of one failure may differ from the next, by where the job is. Enter always
  does the first one shown.
- A failure with no Retry has Skip as the default, so Enter goes on with the job.
- An error dialog is three lines taller than before, and the failure of a job two more with The
  rest.
- A run of failures of one kind, such as files that may not be read among many, can be passed
  by while the others still ask.
- On a terminal too short for the dialog, its buttons are cut off first, as before.
- Most errors are one key from trying again, or from the dialog to mend what was typed.
- A failed request carries what it asked for back to the app, which keeps it while its error
  waits: the bytes of a save, the old and the new configuration, a change of the workspaces or
  of the history.
