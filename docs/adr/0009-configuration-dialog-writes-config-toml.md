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
  which scroll, with a scroll bar, when they do not fit: Interface (`[ui]`), Transfers
  (`[transfer]`), SSH (`[ssh]`, `[discovery]`), and Volumes (`[volumes]`).
- The dialog has no OK or Cancel, as modern settings windows: each change takes effect as
  it is made, a text field's when the cursor leaves it, after its value is checked. The
  running program uses it at once where it can: the interface, transfers, and, for new
  connections and listings, ssh and the hidden hosts and volumes. The language takes effect
  at the next start, which the dialog says. `noc_config::save_config` writes each change in
  the background, one after another, so that none overtakes the one before it.
- Only keys that a change touched are written: those that differ between the settings before
  and after it. The file is read again first, so keys changed in it
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
- Every option of `config.toml` has its row in the dialog, and a new option comes with one
  (see `AGENTS.md`).
- The settings in `Context` sit behind a lock, so that the dialog can change them for new
  connections, `ssh -G`, and listings; connections that are open keep what they started with.
