# 0049. The menus of a panel keep the active panel

- Status: Accepted
- Date: 2026-10-26
- Amends: [ADR 0021](0021-location-menu-replaces-the-virtual-root.md),
  [ADR 0022](0022-vim-modes.md)

## Context

Ctrl+w 1 and Ctrl+w 2 open the location menu of the panel drawn on the left or the right, and so
did Alt+F1 and Alt+F2, Far Manager's keys to change the drive of a panel
([ADR 0021](0021-location-menu-replaces-the-virtual-root.md), [ADR 0022](0022-vim-modes.md)).
Enter in the menu opened the place in its panel and gave that panel the keys, and so did Enter in
the tab menu of `t h` and `t l` with the tab it showed. To send the other panel somewhere, as the
target of F5, took Ctrl+w 2, the place, and Ctrl+w h back, where Ctrl+w f and Ctrl+w v, which
change the other panel too, leave the keys where they are.

Alt+F1 and Alt+F2 seldom reach a terminal program: KDE Plasma opens its launcher and KRunner
with them, GNOME its overview and its dialog to run a command, Xfce its menu of applications and
its finder of them. On macOS the Option key types characters unless the terminal sends it as Alt,
and Option with the brightness keys, which F1 and F2 are by default, opens the settings of the
displays. The browser behind vhs sends no Alt either, so the demos used Ctrl+w 2. The two were
the only Alt keys of the preset, besides the letters of the pull-down menu and the dialogs, and
Far's, not mc's, though ADR 0022 kept them with mc's F-keys.

## Decision

- **The keys stay with the active panel.** The location menu opens the place chosen in its
  panel, the one drawn on its side, and the tab menu (`t h`, `t l`) shows the tab chosen in its
  panel; the keys stay where they were, as after Ctrl+w f and Ctrl+w v, and Ctrl+w h, Ctrl+w l,
  and Tab go to the other panel. F9 → Left or Right → Change location and Tab list… do the same.
  A click on a panel, or on one of its tabs, still gives it the keys.
- **Ctrl+w 1 and Ctrl+w 2 alone open the location menus.** Alt+F1 and Alt+F2 are bound nowhere:
  not in the panels, in Visual mode, or in quick search. As any Alt key that nothing binds, they
  stand for Esc and the key ([ADR 0022](0022-vim-modes.md)): in the panels, Alt+F1 shows the
  help, as F1 does, and Alt+F2 does what Esc does.
- Visual mode and quick search have no key of their own for the menus, as Ctrl+w 1 and Ctrl+w 2
  are not bound there; F9 ends them and opens the pull-down menu, whose Left and Right have
  Change location.

## Consequences

- A place or a tab chosen for the other panel, as a target for F5 or F6, no longer takes the keys
  there; working in that panel takes Ctrl+w l, Ctrl+w h, or Tab more.
- Far's users lose Alt+F1 and Alt+F2; the help, the key hints, and the pull-down menu show
  Ctrl+w 1 and Ctrl+w 2.
- The demos go to the right panel with Ctrl+w l after its menu.
