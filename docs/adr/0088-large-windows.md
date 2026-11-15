# 0088. Large windows

- Status: Accepted
- Date: 2026-11-16
- Amends: [ADR 0041](0041-lists-always-show-the-whole-row.md),
  [ADR 0061](0061-a-picker-is-a-part-of-a-dialog.md),
  [ADR 0071](0071-the-tab-menu-and-the-zoxide-window-show-no-whole-row.md)

## Context

Every window was 76 cells wide (`window::WIDTH`) and as tall as what it showed, but four. The help
took 70% of the width and of the height of the screen, by constants of its own. The Configuration
dialog was 20 rows tall at most. The search of a tree was as tall as the screen. The list of the
key hints (Ctrl+k twice) was as tall as its rows, and grew and shrank as the filter changed them.

These four are not windows to choose one row from and close at once. One reads them and scrolls
through them, and they hold more than 76 cells show: the help and the list of the key hints have
about ninety keys in the panels, the Configuration dialog a column of categories beside its
settings, the search of a tree lines of files with their paths. On a wide screen they used little
of it; the list of the key hints cut its keys at 24 cells, and showed the whole row under the
cursor again in two lines under the rows, keys first, for what the row cut.

## Decision

- **Two sizes of windows.** A window to choose from and close, a dialog or a list, keeps its
  76 cells and its height by what it shows. A window to read and scroll through is large: 80% of
  the width and of the height of the screen, the same whatever it shows (`window::large`).
- **Large windows:** the help (F1), the list of the key hints, the Configuration dialog, and the
  search of a tree (`f /`, `f f`). A list is large by `Body.large`, a dialog by `Dialog::large`.
- **A large window is never narrower than any other**: on a screen where 80% is less than 76
  cells, it is 76, where the screen has room, as every window is.
- **The list of the key hints shows what the command under the cursor does, whole**, in the two
  lines under the rows, broken between words, without its keys. The column of keys takes as many
  cells as the longest keys shown, up to a third of a row, rather than up to 24: in a large window
  nearly every list of keys fits. Keys that do not are left out of the row, as before.

## Consequences

- On a wide screen the four windows show more, and the help is wider and taller: 80% of the
  screen, where it was 70%.
- On a small screen the Configuration dialog and the search of a tree are a few rows shorter: on a
  screen of 24 rows the Configuration dialog is 17 rows tall, where it was 20, and the search of a
  tree shows fewer files at once.
- The list of the key hints keeps its size as the filter changes its rows; the bottom of its frame
  no longer moves.
- A key that does not fit in its column of a narrow list of the key hints cannot be read there; the
  help shows the keys of each command whole.
