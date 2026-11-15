# 0072. The command history shows where a command ran

- Status: Accepted; amended by [ADR 0082](0082-hosts-are-named-by-their-alias.md)
- Date: 2026-11-10
- Amends: [ADR 0019](0019-shell-command-line.md),
  [ADR 0041](0041-lists-always-show-the-whole-row.md),
  [ADR 0071](0071-the-tab-menu-and-the-zoxide-window-show-no-whole-row.md)

## Context

Each row of the command history (`q :`) showed the host, the directory, and the first line of the
command, and its footer the whole command under the cursor, in one to four lines
([ADR 0019](0019-shell-command-line.md), [ADR 0041](0041-lists-always-show-the-whole-row.md)). In
a row of 72 cells the host, up to 12, and the directory, cut after 18, left the command, which the
window is opened to find, as few as 38. The footer said again the first line, which the row already
showed, and the time a command ran, which `history.toml` keeps, showed nowhere.

## Decision

- **A row is the command alone**, its first line across the row, with `»` and `+N` for the lines
  after it, as before. Rows of another host are still dimmed.
- **The footer says where the command under the cursor ran**, in two lines: the host, by its label,
  else its alias, or `local` for this machine; then the directory, a local one with `~` for the
  home directory, `~` for a host's, which loses its start where it is too long. Both lines show
  with one host too, so that the window keeps its height when Tab switches.
- **When it ran is on the line over the footer**, at its right end, as a title on the frame, in the
  color of the window's title but not bold (`Body.footer_label`), as every time shows
  ([ADR 0035](0035-formats-of-the-region.md)). Beside the host, a long directory under it ran into
  it, and it read as part of the path. A command kept without a time leaves the line bare.
- **An icon in front of each line** (`Glyphs.ran`): with `ui.icons` a server (`nf-md-server`), a
  monitor for this machine (`nf-md-monitor`), and the folder of the panels; without, `≡`, `⌂`, and
  `›`, which no marker of the panels or status of a host uses.

## Consequences

- The command has 70 cells of the row, where it had as few as 38.
- The lines after the first of a command of several cannot be read in the window; Enter puts it on
  the command line whole.
- The footer is two lines for every command, where it was one to four, so the window keeps its
  height as the cursor moves.
