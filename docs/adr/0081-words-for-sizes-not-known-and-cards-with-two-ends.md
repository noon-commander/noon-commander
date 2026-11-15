# 0081. Words for sizes not known, and cards with two ends

- Status: Accepted
- Date: 2026-11-15
- Amends: [ADR 0074](0074-the-location-menu-shows-its-rows-before-the-volumes-answer.md),
  [ADR 0075](0075-the-location-menu-shows-a-card-of-the-place-under-the-cursor.md),
  [ADR 0076](0076-ssh-g-for-the-host-under-the-cursor-of-the-location-menu.md)

## Context

The location menu showed `- / -` in place of the sizes of a volume that did not answer in 5 s and
of one that tells none: the user could not tell what it meant, nor the one from the other. The card
said `Did not answer in 5 s`, a number of no use to the user, in the colors of the text.

The card of a host put its address after its alias, `web · deploy@10.0.0.5 via bastion`, as one
text. Where `ssh -G` failed, it wrote why, `ssh -G failed: reason`, and the reason, ssh's own
words, can be long. A connected host said how many tabs were on it, which told little; one not
connected said `Opens /srv/app · other panel ~/logs`, many words for two directories. Why a host
failed to connect was crossed out: the card took the style of what is typed wrong
(`Theme::invalid`), as the note `Not listed again: …` right of the filter did. The row of the rest
of a group named the places it stands for on one line, and left the second empty.

A query of a file system that failed after it hung, as a soft network mount gives up, left the
volume as it was listed, without sizes; once its answer came, it read as one that tells none.

## Decision

- **Words in place of the sizes.** A volume given up for after 5 s says `not responding`, in the
  colors of warnings; one that tells none, or tells a size of 0, says `size unknown`, faint, as
  what only helps. They stand at the right end of the row, where the sizes would be; under the
  cursor they take its colors. The card says `Not responding` and `Size unknown` the same way. The
  column of the sizes is as wide as the widest of the rows shown, words too; the slashes of the
  sizes still line up.
- **A late failure does not respond.** `noc-vfs` tells what a late answer is (`LateAnswer`): the
  volume, no volume to show, or a failure, which leaves the volume as it was and makes it
  `not responding` at once (`Places::failed`), until R or the next menu asks again.
- **Two roles of the theme** for text over dialogs: `dialog_warning_text`, peach in Catppuccin,
  the light orange of warnings in Noon, yellow in mc classic; and `dialog_error_text`, the red of
  the host that failed. The terminal theme makes both bold, as its messages on the line of the
  mode. Neither is crossed out: `Theme::invalid` stays for what is typed wrong. The note right of
  the filter takes `dialog_error_text` too.
- **The first line of a host has two ends**: its alias at the left; at the right end its address
  with the host ssh jumps through, a skeleton of 16 cells while `ssh -G` runs, or `ssh -G failed`
  in the colors of errors. The end takes what the alias leaves after a blank, but at least half the
  line (`ends`), and loses its middle where it is too long, so that the port stays. Why `ssh -G`
  failed goes to the log alone, at `warn`; it went there at `debug`, under the default `info`, so
  that it showed nowhere but in the card.
- **The second line of a host says how it is in one word**: `Connected`, or `Connecting…`, whose
  icon turns in front of the first line; why its last attempt failed, in the colors of errors.
- **Where a host not connected opens** shows as directories after folders (`glyphs.ran.dir`), each
  on the side of its panel: the remote one, `~` for the home, on the side of the panel of the menu;
  the local `other_dir` of the host, if any, on the other side, at its end. The one on the left
  stands in the column of the icons. A directory that is too long loses its start.
- **The row of the rest names its places over both lines**, whole names first, the rest on the
  second line under the first's text, which loses its end to a mark.
- `HostState` loses the count of tabs; `unresolved` is a flag, and `Done::NotResolved` carries no
  reason.

## Consequences

- `- / -` no longer shows; a volume that hangs and one without sizes tell apart in the row.
- A column of sizes with words in it is wider than one of sizes alone: names have a few cells less.
- Why `ssh -G` failed is in the log only, under `logs/` in the XDG state directory.
- Which directory is whose shows by its side, with no words. The menu of the other panel shows the
  same host's directories the other way round: each stands on the side of the panel it opens in.
