# 0009. The Configuration dialog writes config.toml

- Status: Accepted
- Date: 2026-10-02

## Context

Until now Noon Commander never wrote `config.toml`: `noc config init` created it once, and the
user edited it by hand. Settings such as the theme, the frames, the icons, or the menu bar are
easier to try in the program, as mc and Far allow (Options → Configuration), and seeing the
effect at once is the point of trying them.

The file belongs to the user. It has comments, often the commented defaults that
`noc config init` writes, and the user may edit it while Noon Commander runs. `hosts.toml`
already faced this ([ADR 0007](0007-typed-host-settings-in-hosts-toml.md)): F4 on a host
rewrites one table through `toml_edit` and keeps everything else.

## Decision

- Options → Configuration… opens a dialog of the settings, by category: a list of categories
  on the left, each with a Nerd Font icon, and the settings of the chosen one on the right,
  which scroll, with a scroll bar, when they do not fit. The first category is Interface
  (`[ui]`); others follow.
- OK uses the settings at once where the running program can (theme, frames, icons, hidden
  files, quick search, menu bar); the language takes effect at the next start, which the
  dialog says. Then `noc_config::save_config` writes them in the background.
- Only keys that the dialog changed are written: those that differ between what the dialog
  showed and what it closed with. The file is read again first, so keys changed in it
  meanwhile stay. A changed key keeps its comments, a new key goes at the end of its table
  (made if missing), and a value set back to its default is written only where the file sets
  the key.
- The result is checked against the schema before it replaces the file, atomically, as
  `hosts.toml` is replaced. A file that is not valid TOML, or that the change would make
  invalid, is left alone, and an error dialog says why; the settings stay in use for the
  session.
- The file written is the one in use: `config.toml`, or the file of `--config`.

## Consequences

- `config.toml` is no longer read-only to Noon Commander. A key that both the user and the
  dialog change while it runs gets the dialog's value.
- Comments and formatting survive, but new keys go at the end of their table rather than
  next to the commented defaults.
- Settings that the dialog cannot apply at once must say so, or move to the next start.
- The settings that only the start reads, in `Context`, stay as they were loaded; categories
  for them (ssh, discovery, volumes) need a way to pass changes on, or a restart note.
