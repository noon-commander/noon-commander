# 0008. Clipboard through OSC 52

- Status: Accepted
- Date: 2026-10-02

## Context

Checksums are the first thing Noon Commander offers to copy, and paths, names, and listings
will follow. A terminal program has no clipboard of its own. It can run a helper that the
desktop provides (`pbcopy` on macOS, `wl-copy` on Wayland, `xclip` or `xsel` on X11), or ask
the terminal with the OSC 52 escape sequence (`ESC ] 52 ; c ; <base64> ESC \`) to put the text
on the clipboard of the machine the terminal runs on.

Noon Commander often runs on a server, over ssh, where the user wants the text on the laptop
in front of them. A helper on the server copies to a clipboard nobody sees, or there is none.
OSC 52 goes through ssh with the rest of the output. Many terminals support it: kitty, WezTerm,
Alacritty, foot, Ghostty, iTerm2 (after "Applications in terminal may access clipboard" is
turned on), Windows Terminal, and xterm (with `allowWindowOps` or its `disallowedWindowOps`
setting). tmux passes it on with `set-clipboard on`. macOS Terminal ignores it.

## Decision

- The app copies only through OSC 52. It asks the event loop to copy (`App::take_clipboard`),
  and the loop writes `crossterm::clipboard::CopyToClipboard` (crossterm's `osc52` feature)
  to the terminal between frames, as it writes everything else.
- Only the system clipboard (`c`) is set, not the X11 primary selection.
- The app never reads the clipboard. Reading through OSC 52 would let any program that can
  write to the terminal take what the user copied, and most terminals refuse it anyway.
- Nothing tells whether the terminal took the text, so the UI says it was sent to the
  terminal's clipboard, not that it was copied.

## Consequences

- No new dependency: `base64` was already in the tree, and crossterm encodes the text.
- Copying works the same way locally and over ssh, in every terminal that supports OSC 52 and
  allows it.
- In macOS Terminal, and in terminals or multiplexers that block OSC 52, copying does nothing.
  The roadmap lists a fallback for later: a desktop helper (`pbcopy`, `wl-copy`, `xclip`)
  when Noon Commander runs on the desktop, behind a setting.
