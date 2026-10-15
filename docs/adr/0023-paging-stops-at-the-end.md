# 0023. Paging stops at the end, which is marked

- Status: Accepted
- Date: 2026-10-15

## Context

The panels, the lists, the help, and the viewer scrolled as vim does
([ADR 0022](0022-vim-modes.md)). At the end of the rows, vim's Ctrl+f goes on until only the last
line is left at the top of the window, and so did ours; in the help and the viewer, `j` and
Ctrl+e did too. A panel then showed one file at its top and an empty frame below it, which looks
like a listing that lost its files, and nothing said that the end was reached.

The help and the viewer each had their own copy of the page keys: how far vim's Ctrl+f goes
(`page_less_overlap`), the table of keys to rows, and which of them may go past the end. The
scroll bar of the Configuration dialog lived in the binary, out of reach of `noc-viewer`.

## Decision

- Every key down stops with the end at the bottom of the screen, in the panels, the lists, the
  help, and the viewer. In the panels and the lists, once the last row shows, Ctrl+f puts the
  cursor on it, as mc's PgDn does; the rest of the scrolling stays vim's.
- The help and the viewer mark the end three ways: a dimmed `— end —` after the last line, which
  counts as a row of the text so that the end at the bottom shows it; `End` in place of the
  percentage, on the viewer's title line and in the help's bottom frame; and a scroll bar on the
  right while the text does not fit.
- A crate, `noc-scroll`, holds what the help and the viewer share: `Motion` and how far each goes
  on a page (`Motion::step`), the stop at the end for text of one row per line (`scrolled`), and
  the scroll bar (`scrollbar`), which the Configuration dialog uses too. It depends only on
  ratatui, and draws but has no text. The app maps actions to motions once, in
  `Action::motion`; the viewer's `Command::Scroll` takes one.

## Consequences

- No screen ends in an empty page; `G` and Ctrl+f at the end show the same rows.
- The viewer counts its scroll bar in lines of the file, not rows on screen: wrapping the whole
  file to measure it would read up to 16 MiB of text on every frame. With long wrapped lines the
  thumb is approximate.
- The scroll bars and the percentages leave out the end line: they reach the bottom and 100 with
  the last line of text, one row before the end line shows, and then hold still: the viewer's
  thumb stays as it is with the last line at the bottom, though the end line takes a row of the
  text. A text that fits but for the end line gets no scroll bar.
- The scroll bar takes a column of the viewer only while the text does not fit; the help always
  keeps that column, and a gap before it.
- `noc-scroll` is a second library crate with UI code, after `noc-viewer` (ADR 0015).
