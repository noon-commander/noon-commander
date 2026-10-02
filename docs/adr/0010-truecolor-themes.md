# 0010. Truecolor themes with a 256-color fallback

- Status: Accepted
- Date: 2026-10-02

## Context

The built-in themes so far use only the 16 ANSI colors (`mc-classic`) or none (`terminal`), so
the terminal's palette decides how they look. A theme of its own, with several shades of
background for panels, dialogs, menus, and shadows, needs exact colors, which only 24-bit RGB
gives. Users also expect a dark and a light variant of the same theme, matching their terminal.

Not every terminal shows 24-bit color. ratatui and crossterm write RGB as it is and do not bring
it down to what the terminal can show, so on a terminal without it the colors come out wrong
or not at all. Terminals that have it announce it with `COLORTERM=truecolor` (or `24bit`);
there is no reliable query beyond that.

## Decision

- The first truecolor themes are Catppuccin's Mocha (dark) and Latte (light):
  `catppuccin-mocha` and `catppuccin-latte`. Catppuccin is popular, MIT-licensed, and gives
  every flavor the same named colors (`base`, `mantle`, `crust`, `surface0..2`,
  `overlay0..2`, `text`, and accents), with five layers of background.
- One function builds a Catppuccin theme from a palette of those names, so both flavors give
  each color the same role. Other dark and light pairs (Gruvbox, Rosé Pine, Tokyo Night,
  Solarized) can follow as palettes for the same roles.
- At start, Noon Commander reads `COLORTERM`. Unless it is `truecolor` or `24bit`, the
  palette is brought down to the 256-color palette: each color becomes the nearest of the
  6×6×6 cube and the gray ramp (indexes 16 … 255), never one of the 16 colors below them,
  which the terminal's palette redefines. The log says so.
- `mc-classic` and `terminal` are unchanged.

## Consequences

- On terminals without 24-bit color, the Catppuccin themes are close to the original, but
  some shades merge: in Latte, `base` and `mantle` become the same gray.
- A terminal that has 24-bit color but does not set `COLORTERM` (or loses it, as through some
  `sudo` or `tmux` setups) gets the 256-color version; there is no setting to force either
  yet.
- Latte's accents are designed for its light background and are paler against it than
  Mocha's; the theme keeps Catppuccin's colors rather than darkening them.
- User themes (M4) will need the same choice: RGB in the file, brought down the same way.
