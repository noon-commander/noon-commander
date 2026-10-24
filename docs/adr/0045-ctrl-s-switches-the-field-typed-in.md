# 0045. Ctrl+s switches the field it is typed in

- Status: Accepted
- Date: 2026-10-24
- Amends: [ADR 0030](0030-search-modes.md), [ADR 0033](0033-letters-in-dialogs.md),
  [ADR 0044](0044-one-base-for-search-fields.md)

## Context

Every field that searches has its way of matching what is typed, and Ctrl+s switches it
([ADR 0044](0044-one-base-for-search-fields.md)). The key was bound in the contexts of the windows
that hold such fields: quick search, the lists in Normal and Insert mode, and the search of a tree,
where it switched the text from anywhere in the dialog, from the field of the globs and from the
rows too ([ADR 0033](0033-letters-in-dialogs.md)). That worked while each window had one such field.
A dialog with two, two patterns of names, say, could not tell which of them Ctrl+s meant, and the
keys of a context cannot say either, since they are the same whatever has the focus.

The glyph of the way stood after the word of the prompt (`Filter ~:`), away from the field it
speaks of, and nothing but the key switched it.

## Decision

- **Ctrl+s switches the field that has the focus, and no other.** Quick search; the filter of a
  list while it takes the keys, in Insert mode; and a field of a dialog that searches, in Insert
  and Normal mode. On the rows of a list, on a check box, a button, or the rows of a dialog, and
  in a field with one way, such as the pattern of `g +`, Ctrl+s does nothing.
- **A context of its own, `search_input`, as a layer.** It binds Ctrl+s alone, and goes in front
  of the chain of the context where the keys go while such a field has the focus. The app tells
  the keymap where keys go as a `keymap::Place`: the mode, the context, and whether the field with
  the focus switches how it searches. The chains stay as they are, and no context is split in two
  for a field of its own. The F-key bar, the help, and the key hints show Ctrl+s where it works,
  and nowhere else.
- **The glyph goes to the end of the field**, after a blank: at the right end of the status line
  of quick search, at the end of the field of a filter, and at the end of a field of a dialog. The
  prompts lose it (`Filter:`, `Text:`). It stays crossed out while the text is no regular
  expression, and shows in a field of one way too.
- **A click on the glyph does what Ctrl+s does**, after giving the field the focus: a list goes to
  its filter, in Insert mode; a dialog to the field. A double click does no more than the click.
- **A dialog holds the way of its fields.** A field searches when the dialog gives it a
  `search::Input`; the dialog draws its glyph, takes Ctrl+s and the click on the glyph, and says
  the way of each field. The windows that own such dialogs read it from there.

## Consequences

- A dialog may have as many fields that search as it needs: each switches alone, with its own key
  under the focus and its own glyph under the mouse.
- In the search of a tree, Ctrl+s works in the field of the text alone, not from the rows, the
  globs, or the check box, where it used to.
- In a list, Ctrl+s works in the filter alone, so from the rows it takes `/` first, or a click on
  the glyph.
- The prompts and the label of the text of `f /` are shorter by the glyph; the field of a filter
  is shorter by it and its blank.
- Contexts that are only fallbacks, `search_input` among them, have no kind of window on the line
  of the mode.
