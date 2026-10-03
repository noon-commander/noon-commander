//! Tabs: each side has panels of its own, of which one shows. A side with more than one tab
//! shows them on a line above its panel, or in the top of the panel's frame (`ui.tab_bar`).

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use noc_vfs::Location;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::BorderType;

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
    /// The directory the tab shows went to zoxide since the tab came there, so that it counts
    /// once a visit.
    pub(crate) noted: bool,
    /// A directory the tab jumps to through zoxide, which already counted it.
    pub(crate) arriving: Option<PathBuf>,
    /// What the tab showed before it went where it is, for `cd -`.
    pub(crate) previous: Option<Location>,
}

impl Tab {
    pub(crate) fn new(id: PanelId, panel: Panel) -> Self {
        Self {
            id,
            panel,
            stale: false,
            noted: false,
            arriving: None,
            previous: None,
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
    /// Draws the tab `index` as ` label ` at `x` in `area`'s line: the one that shows in the
    /// panel's colors, its number in an accent on the side with the keys, the others as tabs
    /// that do not show. Returns where it ends.
    fn draw_tab(
        &self,
        frame: &mut Frame<'_>,
        x: u16,
        area: Rect,
        (index, label): (usize, &str),
    ) -> u16 {
        let theme = self.theme;
        let (style, number) = match (index == self.active, self.focused) {
            (true, true) => (theme.tab_active, theme.tab_active.patch(theme.tab_number)),
            (true, false) => (theme.tab_active_idle, theme.tab_active_idle),
            (false, _) => (theme.tab, theme.tab),
        };
        let (digits, name) = label.split_once(' ').unwrap_or((label, ""));
        let line = Line::from(vec![
            Span::styled(" ", style),
            Span::styled(digits.to_owned(), number),
            Span::styled(format!(" {name} "), style),
        ]);
        let width = u16::try_from(line.width()).unwrap_or(u16::MAX);
        let width = width.min(area.right().saturating_sub(x));
        frame.render_widget(line, Rect::new(x, area.y, width, 1));
        x + width
    }

    /// The tabs on a line of their own, `area`, with a bar between them, between the frame's
    /// verticals above its corners; `‹` and `›` say that tabs are left out before or after the
    /// ones shown.
    pub(crate) fn render_line(&self, frame: &mut Frame<'_>, area: Rect) {
        let theme = self.theme;
        frame.render_widget(
            Line::styled(" ".repeat(usize::from(area.width)), theme.tab),
            area,
        );
        if area.width < 2 {
            return;
        }
        let edge = area.right() - 1;
        let vertical = match theme.border_type() {
            BorderType::Double => "║",
            _ => "│",
        };
        for x in [area.x, edge] {
            frame.render_widget(
                Line::styled(vertical, theme.panel_border),
                Rect::new(x, area.y, 1, 1),
            );
        }
        let inside = Rect::new(area.x + 1, area.y, area.width - 2, 1);
        let room = usize::from(inside.width);
        let mut shown = fit(self.names, self.active, room);
        let more = |shown: &[(usize, String)]| {
            let before = shown.first().is_some_and(|(index, _)| *index > 0);
            let after = shown
                .last()
                .is_some_and(|(index, _)| index + 1 < self.names.len());
            (before, after)
        };
        if more(&shown) != (false, false) {
            shown = fit(self.names, self.active, room.saturating_sub(2));
        }
        let (before, after) = more(&shown);
        let mark = |frame: &mut Frame<'_>, x: u16, text: &str| {
            frame.render_widget(
                Line::styled(text.to_owned(), theme.tab),
                Rect::new(x, area.y, 1, 1),
            );
        };
        let mut x = inside.x;
        if before {
            mark(frame, x, "‹");
            x += 1;
        }
        for (position, (index, label)) in shown.into_iter().enumerate() {
            if position > 0 {
                mark(frame, x, "│");
                x += 1;
            }
            x = self.draw_tab(frame, x, inside, (index, &label));
        }
        if after && inside.width > 0 {
            mark(frame, inside.right() - 1, "›");
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
            BorderType::Double => "═",
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
            x = self.draw_tab(frame, x, line, (index, &label));
        }
    }
}

/// Joins the top corners of a panel's frame, `area`, to the verticals of the line of tabs
/// above it: `╠` and `╣`, or `├` and `┤`.
pub(crate) fn join_frame(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    if area.width < 2 || area.height == 0 {
        return;
    }
    let (left, right) = match theme.border_type() {
        BorderType::Double => ("╠", "╣"),
        _ => ("├", "┤"),
    };
    for (x, tee) in [(area.x, left), (area.right() - 1, right)] {
        frame.render_widget(
            Line::styled(tee, theme.panel_border),
            Rect::new(x, area.y, 1, 1),
        );
    }
}

/// Names of tabs shrink to no fewer cells, so that they still say where the tabs are.
const LEAST: usize = 8;

/// The tabs that fit in `room` cells, by index, each as ` number name ` with a cell between
/// them: the names of hidden tabs shrink first, the widest first, then the name of the tab
/// that shows, but none below [`LEAST`] cells; then tabs far from it are left out.
fn fit(names: &[String], active: usize, room: usize) -> Vec<(usize, String)> {
    if names.is_empty() {
        return Vec::new();
    }
    let active = active.min(names.len() - 1);
    let whole: Vec<usize> = names.iter().map(|name| cells::width(name)).collect();
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
    // Shrinks the names of `shown` from their whole widths until they fit, or cannot shrink.
    let shrink = |shown: &[usize]| {
        let mut widths = whole.clone();
        while width(&widths, shown) > room {
            let widest = shown
                .iter()
                .copied()
                .filter(|&index| widths[index] > LEAST)
                .max_by_key(|&index| (index != active, widths[index]));
            let Some(widest) = widest else { break };
            widths[widest] -= 1;
        }
        widths
    };
    let all: Vec<usize> = (0..names.len()).collect();
    let mut widths = shrink(&all);
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
    // Names that shrank so that every tab might fit get back what the tabs left out leave.
    if shown.len() < names.len() {
        widths = shrink(&shown);
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
        let names = labels(&["projects", "documents-archive", "/Users/me/projects/noon"]);
        let texts = |fitted: Vec<(usize, String)>| -> Vec<String> {
            fitted.into_iter().map(|(_, text)| text).collect()
        };
        assert_eq!(
            texts(fit(&names, 2, 62)),
            [
                "1 projects",
                "2 documents-archive",
                "3 /Users/me/projects/noon"
            ]
        );
        assert_eq!(
            texts(fit(&names, 2, 55)),
            ["1 projects", "2 docum~hive", "3 /Users/me/projects/noon"],
            "a hidden tab first"
        );
        assert_eq!(
            texts(fit(&names, 2, 45)),
            ["1 projects", "2 docu~ive", "3 /Users/~ts/noon"],
            "then the one that shows, but no name below eight cells"
        );
        assert_eq!(
            texts(fit(&names, 2, 30)),
            ["2 docu~ive", "3 /Users~s/noon"],
            "then tabs far from the one that shows go"
        );
        assert_eq!(texts(fit(&labels(&["~", "srv"]), 0, 4)), ["1 ~"]);
    }

    #[test]
    fn the_tab_that_shows_takes_the_panel_colors_on_a_dark_line() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;

        let theme = Theme::mc_classic();
        let names = labels(&["src", "noon"]);
        let draw = |focused: bool, in_frame: bool| {
            let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
            let bar = Bar {
                names: &names,
                active: 1,
                focused,
                theme: &theme,
            };
            terminal
                .draw(|frame| {
                    if in_frame {
                        bar.render_frame(frame, frame.area());
                    } else {
                        bar.render_line(frame, frame.area());
                    }
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let line = draw(true, false);
        let cell = |x: u16| line.cell((x, 0)).unwrap().clone();
        // `║ 1 src │ 2 noon             ║`
        assert_eq!(cell(0).symbol(), "║");
        assert_eq!(cell(0).bg, Color::Blue, "the frame's vertical");
        assert_eq!((cell(2).symbol(), cell(2).bg), ("1", Color::Black));
        assert_eq!((cell(8).symbol(), cell(8).fg), ("│", Color::Gray));
        assert_eq!((cell(10).symbol(), cell(10).fg), ("2", Color::LightYellow));
        assert_eq!((cell(12).symbol(), cell(12).bg), ("n", Color::Blue));
        assert_eq!(cell(20).bg, Color::Black, "the rest of the line");
        let idle = draw(false, false);
        assert_eq!(idle.cell((10, 0)).unwrap().fg, Color::Gray, "no accent");
        // In the frame, the line of the frame runs between them.
        let framed = draw(true, true);
        assert_eq!(framed.cell((2, 0)).unwrap().bg, Color::Black);
        assert_eq!(framed.cell((10, 0)).unwrap().bg, Color::Blue);
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
