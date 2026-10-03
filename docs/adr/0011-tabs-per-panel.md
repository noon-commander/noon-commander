# 0011. Tabs per panel, addressed by panel id

- Status: Accepted
- Date: 2026-10-03

## Context

Users keep several directories open at once, often on different hosts, and switch between
them. Two panels are not enough for that, and the location menu and Alt-O only replace what a
panel shows. Total Commander, Double Commander, and Far Manager's plugins answer this with
tabs; mc has none.

Until now the app named a panel by its side (`Side::Left`, `Side::Right`). Listings, new
directories, and the dialogs of `+`, `-`, F5, F6, and F7 carried the side, and the reply went
to whatever panel was on that side. With tabs, a reply sent to a side could reach a different
tab from the one that asked.

There were two ways to model a tab: a whole `Panel` per tab, with its listing, cursor, marks,
sort order, and pending request, or a light record (location, the name under the cursor, sort
order) that one panel per side reads again on each switch. A light record re-lists on every
switch, which means a round trip on a remote host, and it loses marks.

## Decision

- Each side has its own tabs, `Tabs`: never empty, one of them shows. A tab is a whole
  `Panel`, so switching shows it at once, with its cursor, marks, sort order, and quick search
  as they were.
- A panel is named by a `PanelId`: its side and the number of its tab. Tab numbers are never
  used again. `Effect::List`, `Effect::CreateDir`, their replies, and the dialogs that act on a
  panel carry a `PanelId`; a reply whose tab was closed finds no panel and is dropped. `Side`
  is left for layout, focus, and what acts on the tab that shows on a side (Alt-O, Alt-I, F5's
  target, the location menus, Left and Right in F9).
- Hidden tabs cost no listings. When a job, F7, or a change of settings would read a
  directory again, the tabs that show it do so at once and hidden tabs are marked stale; a
  stale tab reads its directory when it shows. Disconnecting or losing a host sends every tab
  on it, hidden or not, back to the list of hosts, and a host that connects lists for every
  tab that waits for it.
- A new tab (Ctrl-X T) copies the one that shows, its listing included, so it needs no
  listing; it waits for the same request if that one does. Ctrl-X W closes a tab (the last
  one stays), Alt-Right and Alt-Left or Ctrl-X N and Ctrl-X P go round them, and Ctrl-X Tab
  lists them to choose one. The keys sit under Ctrl-X because Ctrl-T marks, as in mc, most
  terminals do not pass Ctrl-Tab, and Alt and a digit already stand for the F-keys.
- `ui.tab_bar` decides where a side with more than one tab shows them: `line` (the default) on
  a line of its own above both panels, as Total Commander does, with short names; `frame` in
  the top line of the panel's frame in place of its title, which costs no row, with the whole
  location of the tab that shows. A side with one tab shows no bar.

## Consequences

- Every place that went over both panels now goes over every tab, through `PanelId`s.
- A hidden tab can show a directory as it was until it shows again. That is the price of not
  listing hidden remote directories; the tab corrects itself as it appears.
- Hidden tabs keep their listings in memory.
- Tabs are not saved between runs yet; that comes with restoring the panels' state on start.
