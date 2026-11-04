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
  the reason, which wraps. Every dialog of an error that has a path, a host, or a file and a
  reason shows it so. An error with no reason apart, such as a name that cannot be a name, stays
  one sentence. Reasons start with a capital, as they start a line.
- **A job says what it could not do.** `noc_ops::Event::Failed` carries the `Step` that failed:
  inspect, list, read, create, write, set attributes, rename, or remove. The dialog says it:
  `Cannot read`, `Cannot create`, `Cannot set the attributes of`.
- **The buttons of a failed job are Retry, Skip, Skip all, and Abort**, Retry first and the
  default. Skip is the word of the question about a taken name too: one button, one word.
- **A button that would do nothing new does not show.** No Retry where it would fail again, as
  when the target is the source itself; Skip is then the default. No Skip all at the last entry,
  where it is Skip. No Skip for the only entry, where it is Abort. The question about a taken name
  drops The rest at the last entry and Skip for the only one, where Abort is the default. Where
  the job still counts, it does not know what is left, and every button shows.
- **The main way on comes first, the way out last.** Cancel or Abort is the last button. The
  dialog after a directory could not be read has Retry, Up, Go to, and Cancel, which takes the
  place of OK. Its default is Up where the panel has nothing to show, and Retry otherwise.
- **Renaming or saving onto a file asks the question of a copy** about a taken name, for one
  file: the path, New and Existing with their sizes and times, each viewing its file, and
  Overwrite, Rename…, and Cancel, the default. For a save, New views the bytes it would write and
  has no time. Rename… offers a free name; a rename takes a name in its directory alone.
- **Replacing a workspace by Save as has No as the default**, as renaming one onto another has.

## Consequences

- The reason is always where the eye looks for it, on its own line, apart from the path.
- What failed in a copy names the right file and the right step.
- The buttons of one failure may differ from the next, by where the job is. Enter always
  does the first one shown.
- A failure with no Retry has Skip as the default, so Enter goes on with the job.
- An error dialog is two lines taller than before.
- On a terminal too short for the dialog, its buttons are cut off first, as before.
