# 0025. Marks of text cut short, and where to see it whole

- Status: Accepted; amended by [ADR 0041](0041-lists-always-show-the-whole-row.md)
- Date: 2026-10-15
- Amends: [ADR 0022](0022-vim-modes.md)

## Context

Text that did not fit lost its middle to `~`, as in mc: names in the panels, paths in titles and
lists, labels everywhere. The `~` took the style of the text around it, so it was easy to miss,
it could be part of a real name such as `notes~`, and without icons mc's own `~` marks a symlink
to a directory. Nothing showed the whole text, but the status line of the panel, which cuts
long names too. The viewer cut long lines at the edge of the screen without a sign, and wrapped
rows looked like lines of their own.

## Decision

- **Double chevrons mark text that goes on**: `»` where text is cut short or goes on after,
  `«` where it goes on before; with icons (`ui.icons`), Nerd Font's `nf-md-chevron_double_right`
  and `nf-md-chevron_double_left`. They are drawn everywhere in an accent of the theme, bold, never
  in the style of the text around them, so that a mark always reads as one: `truncated` on the
  panels, the viewer, and the line of the mode; and styles of their own where that accent may not
  read on the background: under the cursor, the selection, a selected command or title, and in
  text fields (`truncated_cursor`); in dialogs and lists, which-key and the help among them
  (`truncated_dialog`); in the menus of F9 (`truncated_menu`); on the F-key bar, the menu bar,
  and the line of tabs (`truncated_bar`); and in error dialogs (`truncated_error`). Names lose
  their middle, so that the extension shows; paths lose their start, so that the last
  directories show (`«/deploy/site`); the rest loses its end. `noc-text` returns what shows
  before and after the mark, and where the text was cut, so that each widget draws the mark of
  the theme in its style; the app's `cells::fit` gives spans, never a string with the mark in it.
- **`K` shows the whole name** under the cursor of the active panel in a frame of two rows below the
  panels, of single lines in the colors of dialogs, which set it apart from the panels, and the
  whole row under the cursor of the lists under their rows: the directory of the zoxide window, the
  place of the location menu with its address or mount point, the title of a tab, where the tabs of
  a workspace go. It stays, following the cursor, until `K` again, Esc in a panel, or F9's Options
  menu. The panels and each list ask for it on their own: `K` in a list leaves the panels as they
  are, and a list opens without it. It is not saved: it starts hidden. The frame keeps two rows, so
  that nothing moves as the cursor goes from a short name to a long one; what does not fit in them
  ends with `»`. `K` is vim's key for what is under the cursor; it marked the row above, which
  Shift+Up does now, as in mc and Far, with Shift+Down beside Space; `J` marks no more. This is the
  one key of mc beside the F-keys that ADR 0022 keeps.
- **The z keys scroll sideways as vim's do on lines that do not wrap**: `z h` and `z l` a column,
  with a count that many; `z H` and `z L` half the width; `z s` to the start; `z e` the end into
  view. In a panel they scroll the names, all rows together, with `«` over the first cell of
  a name that goes on to the left and `»` at the end of one cut short. Vim's `zl` stops with the
  last character of the cursor line at the left edge, under the `«` that would cover it, which
  scrolls the text out of sight; and since all names scroll together, a limit set by the name
  under the cursor went further on a longer one and jumped back on a shorter one. So `z l`,
  `z L`, and the viewer's `l` stop where `z e` puts the end of the longest name or line on
  screen: at the right edge. The scroll comes back by itself as far as what is on screen needs,
  after paging to shorter names or widening the terminal, as vim's view follows its cursor.
  `z s` goes to the start; `z e` goes by the end of the name under the cursor, the viewer's by
  the longest line on screen. Moving the cursor keeps the names where they are, as far as the
  names on screen let it; another directory shows them from their start. In the viewer they do
  nothing while lines wrap, as in vim.
- **The viewer marks lines that go on past the screen** as vim's 'listchars' does: `«` over the
  first cell when text is hidden on the left, `»` over the last when it goes on to the right.
  Wrapped rows after the first start with `»`, as vim's 'showbreak', which takes a cell of them.
  The same two marks everywhere, rather than one for each case, keep one meaning: there is more
  that way.

## Consequences

- Every widget that cuts text draws the mark in its accent, the menus, the F-key bar, and
  which-key too. A row made of several cells cuts each cell on its own, or cuts the styled row
  at its end (`cells::fit_spans`), so that no mark ends up inside text cut again. Text with no
  styles, such as the title of the terminal's window, is the only place a mark is plain.
- Six styles of marks in each theme rather than one: each reads on its own background, which
  the theme tests check for every theme.
- Every place that cuts text takes the marks from the theme, which `ui.icons` sets, as it sets
  the arrows of the scroll bars.
- Without icons, mc's `~` marks only a symlink to a directory again.
- Wrapping counts a cell less in rows after the first, everywhere the viewer measures rows: its
  scroll bar, paging, and the end of the text.
- The panels lose two rows while `K` shows a name.
