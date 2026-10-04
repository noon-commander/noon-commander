# 0015. The viewer and terminal-safe text in crates of their own

- Status: Accepted
- Date: 2026-10-04

## Context

The viewer of F3 lived in the binary, in `crates/noc/src/tui/viewer.rs`: text read up to
16 MiB, scrolled, with long lines wrapped or cut. It is to grow into a viewer of its own, as
mc's and Far's are: a hex mode, search, encodings, files larger than what fits in memory. In the
binary it would grow next to the app's state, and every change to it would build and test the
whole UI.

The viewer used four things of the binary: the keymap's `Action`, the `Theme`, the helpers of
`cells.rs` for terminal-safe text, and the Fluent messages behind `fl!`. The reading of the
file, `read_start` over any `Vfs`, sat in the app's tasks.

Library crates had contained no UI code and no user-facing text. A crate for the viewer cannot
keep that rule: it draws, and it has text.

The helpers of `cells.rs` that made names terminal-safe and measured, fitted, and wrapped text
were needed by the viewer too. Copying them would leave two definitions of what is safe to show
on a terminal, which could drift apart.

A common crate, `noc-core`, was considered for all code shared between crates. Besides the text
helpers, two small pieces are copied today: writing a file atomically (`noc-config`'s
`write_atomic` and `noc-ssh`'s resolve cache, with different rules for permissions and
symlinks) and bytes as hex (`noc-ssh` and the binary).

## Decision

- `noc-viewer` holds the viewer: its state, scrolling, wrapping, drawing, and the reading of the
  start of a file through any `Vfs` (`read_start`, `LIMIT`). It depends on `noc-vfs`,
  `noc-text`, and ratatui, and on nothing of the binary:
  - it takes `Command`s, which the app maps from the keymap's actions, so that widgets still
    never see raw keys; closing the viewer (Quit, Cancel) and its help stay in the app;
  - it draws with `Styles`, which the app takes from its theme;
  - its text is in Fluent files of its own, `crates/noc-viewer/i18n/`, read by a loader of its
    own and checked at compile time as the app's are; `noc::i18n::select` selects its language
    with the app's;
  - the app keeps running the read in a task, finding the host's session, and cancelling it.
- It is a piece of the UI, and the only library crate with UI code and user-facing text.
- `noc-text` holds terminal-safe text: `sanitize`, `width`, `fit`, `wrap`, and `split`. It
  depends only on `unicode-width`. `cells.rs` re-exports them and keeps sizes and times.
- There is no `noc-core`. Code that two crates need goes into a crate of its own topic once the
  second one needs it. Atomic writes and hex stay copied for now: their copies differ, or are a
  few lines long.

## Consequences

- The viewer builds and tests on its own, with its snapshots in its crate; the app's tests of
  F3 check only the wiring.
- A new action of the viewer needs a `Command`, its mapping in the app, a binding in the
  keymap, and a line of help: four places instead of two.
- Every crate with text of its own adds a loader that `noc::i18n::select` must reach; a crate
  that is forgotten there stays in English.
- What is safe to show on a terminal is decided in `noc-text` alone, for the app and the
  viewer.
- A helper shared by crates gets a crate named for what it does, not a place in a crate that
  everything depends on.
