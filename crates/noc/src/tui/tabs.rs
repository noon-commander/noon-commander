//! Tabs: each side has panels of its own, of which one shows. A side with more than one tab
//! shows them on a line above its panel, or in the top of the panel's frame (`ui.tab_bar`).

use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use noc_vfs::Location;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

use super::app::Side;
use super::cells::{self, Align};
use super::panel::{Panel, location_text};
use super::theme::Theme;
use crate::i18n::fl;

/// A panel, by its side and its tab there. Tabs never move between sides and their numbers
/// are never used again, so a reply for a closed tab finds no panel and is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PanelId {
    pub(crate) side: Side,
    pub(crate) tab: u64,
}

#[derive(Debug)]
pub(crate) struct Tab {
    pub(crate) id: PanelId,
    pub(crate) panel: Panel,
    /// What the tab shows changed while it was hidden; it is read again when it shows, so
    /// that hidden tabs cost no listings.
    pub(crate) stale: bool,
}

impl Tab {
    pub(crate) fn new(id: PanelId, panel: Panel) -> Self {
        Self {
            id,
            panel,
            stale: false,
        }
    }
}

/// The tabs of one side, never empty, and the one that shows.
#[derive(Debug)]
pub(crate) struct Tabs {
    tabs: Vec<Tab>,
    active: usize,
}

impl Tabs {
    pub(crate) fn new(tab: Tab) -> Self {
        Self {
            tabs: vec![tab],
            active: 0,
        }
    }

    pub(crate) fn active(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub(crate) fn active_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// The position of the tab that shows.
    pub(crate) fn index(&self) -> usize {
        self.active
    }

    pub(crate) fn len(&self) -> usize {
        self.tabs.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        self.tabs.iter_mut()
    }

    pub(crate) fn get(&self, tab: u64) -> Option<&Tab> {
        self.tabs.iter().find(|shown| shown.id.tab == tab)
    }

    pub(crate) fn get_mut(&mut self, tab: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|shown| shown.id.tab == tab)
    }

    /// Adds `tab` after the one that shows, and shows it.
    pub(crate) fn push(&mut self, tab: Tab) {
        self.active += 1;
        self.tabs.insert(self.active, tab);
    }

    /// Closes the tab that shows, unless it is the last; the one after it shows, or the one
    /// before if it was the last.
    pub(crate) fn close(&mut self) -> Option<Tab> {
        if self.tabs.len() < 2 {
            return None;
        }
        let tab = self.tabs.remove(self.active);
        self.active = self.active.min(self.tabs.len() - 1);
        Some(tab)
    }

    /// Shows the tab at `index`; whether that changed which one shows.
    pub(crate) fn select(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    /// Shows the next tab, or the previous one, round.
    pub(crate) fn step(&mut self, forward: bool) -> bool {
        let count = self.tabs.len();
        let index = if forward {
            (self.active + 1) % count
        } else {
            (self.active + count - 1) % count
        };
        self.select(index)
    }

    /// Where each tab is, as the tab bar names it: the tab that shows, if `full`, as the
    /// panel's title does; the others briefly.
    pub(crate) fn names(&self, root_title: &str, home: &Path, full: bool) -> Vec<String> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let location = tab.panel.location();
                if full && index == self.active {
                    title(location, root_title)
                } else {
                    short_name(location, root_title, home)
                }
            })
            .collect()
    }
}

/// Where a panel is, as its title shows it.
pub(crate) fn title(location: &Location, root_title: &str) -> String {
    match location {
        Location::Root => cells::sanitize(root_title.as_bytes()),
        _ => location_text(location),
    }
}

/// Where a panel is, briefly: the last component of a directory, `~` for the home directory,
/// `host:name` on a host, or the host alone in its start directory.
fn short_name(location: &Location, root_title: &str, home: &Path) -> String {
    match location {
        Location::Root => cells::sanitize(root_title.as_bytes()),
        Location::Sftp => fl!("root-sftp"),
        Location::Local(path) if path == home => "~".to_owned(),
        Location::Local(path) => path.file_name().map_or_else(
            || cells::sanitize(path.as_os_str().as_bytes()),
            |name| cells::sanitize(name.as_bytes()),
        ),
        Location::Remote { host, path } => {
            let mut text = host.as_bytes().to_vec();
            if !path.as_bytes().is_empty() {
                text.push(b':');
                text.extend_from_slice(path.file_name().unwrap_or(path.as_bytes()));
            }
            cells::sanitize(&text)
        }
    }
}

/// How a tab bar looks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bar<'a> {
    /// Where each tab is.
    pub(crate) names: &'a [String],
    pub(crate) active: usize,
    /// The side has the keys.
    pub(crate) focused: bool,
    pub(crate) theme: &'a Theme,
}

impl Bar<'_> {
    /// The style of the tab that shows; the others take the panel's or the frame's.
    fn active_style(&self) -> Style {
        if self.focused {
            self.theme.panel_title_active
        } else {
            self.theme.header
        }
    }

    /// The tabs on a line of their own, `area`, between bars.
    pub(crate) fn render_line(&self, frame: &mut Frame<'_>, area: Rect) {
        let theme = self.theme;
        frame.render_widget(
            Line::styled(" ".repeat(usize::from(area.width)), theme.panel),
            area,
        );
        let room = usize::from(area.width);
        let mut x = area.x;
        for (position, (index, label)) in fit(self.names, self.active, room).into_iter().enumerate()
        {
            if position > 0 {
                frame.render_widget(
                    Line::styled("│", theme.panel_border),
                    Rect::new(x, area.y, 1, 1),
                );
                x += 1;
            }
            let style = if index == self.active {
                self.active_style()
            } else {
                theme.panel
            };
            x = draw_segment(frame, x, area, &label, style);
        }
    }

    /// The tabs in the top line of a panel's frame, `area`, over its title; the frame's line
    /// runs between them.
    pub(crate) fn render_frame(&self, frame: &mut Frame<'_>, area: Rect) {
        // Where the title goes: after the left corner, before a line and the right corner.
        let room = usize::from(area.width.saturating_sub(3));
        if room == 0 || area.height == 0 {
            return;
        }
        let line = Rect::new(area.x + 1, area.y, area.width - 3, 1);
        // The title of the panel's frame goes, and the line comes back in its place.
        let horizontal = match self.theme.border_type() {
            ratatui::widgets::BorderType::Double => "═",
            _ => "─",
        };
        frame.render_widget(
            Line::styled(horizontal.repeat(room), self.theme.panel_border),
            line,
        );
        let mut x = line.x;
        for (position, (index, label)) in fit(self.names, self.active, room).into_iter().enumerate()
        {
            if position > 0 {
                x += 1;
            }
            let style = if index == self.active {
                self.active_style()
            } else {
                self.theme.panel_border
            };
            x = draw_segment(frame, x, line, &label, style);
        }
    }
}

/// Draws ` label ` at `x` in `area`'s line; returns where it ends.
fn draw_segment(frame: &mut Frame<'_>, x: u16, area: Rect, label: &str, style: Style) -> u16 {
    let text = format!(" {label} ");
    let width = u16::try_from(cells::width(&text)).unwrap_or(u16::MAX);
    let width = width.min(area.right().saturating_sub(x));
    frame.render_widget(Line::styled(text, style), Rect::new(x, area.y, width, 1));
    x + width
}

/// The tabs that fit in `room` cells, by index, each as ` number name ` with a cell between
/// them: the names of hidden tabs shrink first, the widest first, down to two cells, then the
/// name of the tab that shows; then tabs far from it are left out.
fn fit(names: &[String], active: usize, room: usize) -> Vec<(usize, String)> {
    /// A character and the `~` that stands for the rest.
    const LEAST: usize = 2;
    if names.is_empty() {
        return Vec::new();
    }
    let active = active.min(names.len() - 1);
    let mut widths: Vec<usize> = names.iter().map(|name| cells::width(name)).collect();
    // ` number name `.
    let segment =
        |widths: &[usize], index: usize| (index + 1).to_string().len() + widths[index] + 3;
    let width = |widths: &[usize], shown: &[usize]| -> usize {
        shown
            .iter()
            .map(|&index| segment(widths, index))
            .sum::<usize>()
            + shown.len().saturating_sub(1)
    };
    let all: Vec<usize> = (0..names.len()).collect();
    while width(&widths, &all) > room {
        let widest = (0..names.len())
            .filter(|&index| widths[index] > LEAST)
            .max_by_key(|&index| (index != active, widths[index]));
        let Some(widest) = widest else { break };
        widths[widest] -= 1;
    }
    let mut shown = vec![active];
    let (mut before, mut after) = (active, active + 1);
    loop {
        let mut grew = false;
        if after < names.len() {
            let mut wider = shown.clone();
            wider.push(after);
            if width(&widths, &wider) <= room {
                shown = wider;
                after += 1;
                grew = true;
            }
        }
        if before > 0 {
            let mut wider = shown.clone();
            wider.insert(0, before - 1);
            if width(&widths, &wider) <= room {
                shown = wider;
                before -= 1;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    shown
        .into_iter()
        .map(|index| {
            let name = cells::fit(&names[index], widths[index], Align::Left);
            (index, format!("{} {name}", index + 1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use noc_vfs::RemotePath;

    use super::*;

    fn labels(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn names_tabs_briefly() {
        let home = Path::new("/home/me");
        let local = |path: &str| Location::Local(PathBuf::from(path));
        let remote = |path: &str| Location::Remote {
            host: "web".to_owned(),
            path: RemotePath::from(path),
        };
        let name = |location: &Location| short_name(location, "mbp", home);
        assert_eq!(name(&local("/home/me/src")), "src");
        assert_eq!(name(&local("/home/me")), "~");
        assert_eq!(name(&local("/")), "/");
        assert_eq!(name(&Location::Root), "mbp");
        assert_eq!(name(&Location::Sftp), "SFTP");
        assert_eq!(name(&remote("")), "web");
        assert_eq!(name(&remote("/var/log")), "web:log");
        assert_eq!(name(&remote("/")), "web:/");
    }

    #[test]
    fn fits_hidden_tabs_first_then_drops_far_tabs() {
        let names = labels(&["src", "/Users/me/projects/noon", "~"]);
        let texts = |fitted: Vec<(usize, String)>| -> Vec<String> {
            fitted.into_iter().map(|(_, text)| text).collect()
        };
        assert_eq!(
            texts(fit(&names, 1, 50)),
            ["1 src", "2 /Users/me/projects/noon", "3 ~"]
        );
        // ` 1 s~ │ 2 /Users/me/projects/noon │ 3 ~ `: 40 cells.
        assert_eq!(
            texts(fit(&names, 1, 40)),
            ["1 s~", "2 /Users/me/projects/noon", "3 ~"]
        );
        let narrow = texts(fit(&names, 1, 30));
        assert_eq!(narrow[0], "1 s~");
        assert_eq!(narrow[2], "3 ~");
        assert!(
            narrow[1].starts_with("2 /Use") && narrow[1].ends_with("noon"),
            "{narrow:?}"
        );
        assert_eq!(cells::width(&narrow[1]), 30 - 6 - 5 - 2 - 2, "{narrow:?}");
        // Too narrow for all: the active one stays, with neighbours that fit.
        let many: Vec<String> = (1..=9).map(|n| format!("dir{n}")).collect();
        let fitted = fit(&many, 8, 20);
        let shown: Vec<usize> = fitted.iter().map(|(index, _)| *index).collect();
        assert_eq!(shown.last(), Some(&8), "{fitted:?}");
        let width: usize = fitted
            .iter()
            .map(|(_, label)| cells::width(label) + 2)
            .sum::<usize>()
            + fitted.len()
            - 1;
        assert!(width <= 20, "{fitted:?}");
    }

    #[test]
    fn closing_keeps_one_tab_and_shows_a_neighbour() {
        let id = |tab| PanelId {
            side: Side::Left,
            tab,
        };
        let panel = || Panel::new(Location::Root, PathBuf::from("/home/me"), true).0;
        let mut tabs = Tabs::new(Tab::new(id(1), panel()));
        assert!(tabs.close().is_none(), "the last tab stays");
        tabs.push(Tab::new(id(2), panel()));
        tabs.push(Tab::new(id(3), panel()));
        assert_eq!((tabs.index(), tabs.active().id.tab), (2, 3));
        assert!(tabs.step(true));
        assert_eq!(tabs.active().id.tab, 1, "round");
        assert!(tabs.step(false));
        assert_eq!(tabs.active().id.tab, 3);
        assert_eq!(tabs.close().map(|tab| tab.id.tab), Some(3));
        assert_eq!(tabs.active().id.tab, 2, "the one before the last");
        assert!(tabs.select(0));
        assert!(!tabs.select(0));
        assert_eq!(tabs.close().map(|tab| tab.id.tab), Some(1));
        assert_eq!(tabs.active().id.tab, 2, "the one after");
        assert_eq!(tabs.len(), 1);
    }
}
