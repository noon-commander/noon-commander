# 0043. One dialog for copy and move

- Status: Accepted
- Date: 2026-10-24

## Context

F5 and F6 opened two dialogs that differed in their words and in Preserve attributes, which the
dialog of F6 left out because moves always kept the attributes. The two looked different for no
reason of the user's, a move between file systems could not leave the attributes behind, and the
code that built them branched on the kind in several places.

## Decision

- **F5, F6, and Shift+F5 open one dialog** (`tui/transfer.rs`): the prompt, the target field, and
  Preserve attributes, for a copy and a move alike. Only the words differ: the title, the prompt,
  and later the job's window and errors.
- **No switch between a copy and a move in the dialog.** The key that opened it decides, as in mc
  and Far; F5 and F6 stay one keystroke apart.
- **Preserve attributes is one setting for both**, remembered from the last dialog. Where a move
  copies, between the local file system and a host or two hosts, it applies as to a copy; a
  rename keeps the attributes anyway.
- **One set of messages**: `transfer-*` in Fluent, with `$kind` selecting the words of a copy or
  a move.

## Consequences

- The two dialogs have the same size and layout.
- A move between file systems may now drop the attributes, if the check box is off.
- The jobs of copies and moves are one kind with two flavors (`JobKind::Transfer`), so their
  windows and errors take their words from the same place.
