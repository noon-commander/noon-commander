//! The tab menu of Ctrl+x [ and Ctrl+x ]: the tabs of one panel, drawn over it as the location
//! menu is, to show one. Typing filters it; while the filter is empty, `1` … `9` and `0` show
//! the first ten tabs.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::app::Side;
use super::cells::{self, Align};
use super::dialog::Colors;
use super::keymap::{Action, Resolved};
use super::mouse::Pointer;
use super::picker::{Body, MenuLine, Pick, Picker};
use super::theme::Theme;
use crate::i18n::fl;

/// Narrowest the menu gets, in cells, borders included; it grows with the widest location up
/// to the width of the panel.
const MIN_WIDTH: u16 = 30;

/// What a key did in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabMenuEvent {
    Pending,
    Closed,
    /// Show the tab at this index in the menu's panel.
    Show(usize),
}

/// The menu of the tabs of one panel.
#[derive(Debug)]
pub(crate) struct TabMenu {
    side: Side,
    /// Where each tab is, as the panel's title shows it.
    titles: Vec<String>,
    picker: Picker,
}

impl TabMenu {
    /// A menu of the tabs on `side`, which are where `titles` say, with the cursor on the one
    /// at `current`, which shows; its filter matches as fzf does if `fuzzy`
    /// (`ui.fuzzy_search`).
    pub(crate) fn new(side: Side, titles: Vec<String>, current: usize, fuzzy: bool) -> Self {
        let mut picker = Picker::new(fuzzy);
        picker.set_rows(titles.iter().map(|title| vec![title.clone()]).collect());
        picker.set_cursor(current);
        Self {
            side,
            titles,
            picker,
        }
    }

    /// The panel the menu is for.
    pub(crate) fn side(&self) -> Side {
        self.side
    }

    /// Takes a key, as the picker does.
    pub(crate) fn handle(&mut self, input: Resolved) -> TabMenuEvent {
        match self.picker.handle(input) {
            Pick::Chosen(row) => match self.picker.shown(0..self.titles.len()).get(row) {
                Some(&index) => TabMenuEvent::Show(index),
                None => TabMenuEvent::Pending,
            },
            Pick::Closed => TabMenuEvent::Closed,
            Pick::Pending | Pick::Other(_) => TabMenuEvent::Pending,
        }
    }

    /// Takes a press of the mouse, where the menu was drawn last: a click puts the cursor on
    /// a row, and returns the key that a double click on one, or a click outside the menu,
    /// stands for: Enter or Esc.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        self.picker.pointer(pointer)
    }

    /// Draws the menu over `area`, the panel it is for: the filter, then the tabs, scrolled to
    /// the cursor.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let shown = self.picker.shown(&self.titles);
        let lines: Vec<MenuLine> = (0..shown.len()).map(MenuLine::Row).collect();
        let title = match self.side {
            Side::Left => fl!("tabs-title-left"),
            Side::Right => fl!("tabs-title-right"),
        };
        let widest = self.titles.iter().map(|title| cells::width(title)).max();
        // The frame, a cell of room on either side, a hotkey, and a space.
        let width = u16::try_from(widest.unwrap_or(0) + 6)
            .unwrap_or(u16::MAX)
            .max(MIN_WIDTH);
        let body = Body {
            title: &title,
            width,
            lines: &lines,
            ..Body::default()
        };
        let focused = Colors::of(theme, false).focused_style();
        self.picker.render(frame, area, theme, body, &mut |row| {
            let text = cells::fit(shown[row.index], row.width.saturating_sub(2), Align::Left);
            let style = if row.selected { focused } else { theme.dialog };
            Line::styled(format!("{} {text}", row.hotkey), style)
        });
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn menu() -> TabMenu {
        let titles = ["~/src", "/var/log", "web:/var/www", "~/src/noc"].map(str::to_owned);
        TabMenu::new(Side::Left, titles.to_vec(), 1, false)
    }

    fn typed(menu: &mut TabMenu, text: &str) {
        for c in text.chars() {
            assert_eq!(menu.handle(Resolved::Insert(c)), TabMenuEvent::Pending);
        }
    }

    fn press(menu: &mut TabMenu, action: Action) -> TabMenuEvent {
        menu.handle(Resolved::Action(action))
    }

    fn draw(menu: &mut TabMenu, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| menu.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn starts_on_the_tab_that_shows_and_enter_and_digits_show_tabs() {
        let mut menu = menu();
        assert_eq!(press(&mut menu, Action::Confirm), TabMenuEvent::Show(1));
        assert_eq!(menu.handle(Resolved::Insert('4')), TabMenuEvent::Show(3));
        assert_eq!(menu.handle(Resolved::Insert('5')), TabMenuEvent::Pending);
        assert_eq!(press(&mut menu, Action::Disconnect), TabMenuEvent::Pending);
        assert_eq!(press(&mut menu, Action::Cancel), TabMenuEvent::Closed);
    }

    #[test]
    fn typing_filters_by_location() {
        let mut menu = menu();
        typed(&mut menu, "SRC");
        assert_eq!(press(&mut menu, Action::Down), TabMenuEvent::Pending);
        assert_eq!(
            press(&mut menu, Action::Confirm),
            TabMenuEvent::Show(3),
            "the second of those that match"
        );
        typed(&mut menu, "x");
        assert_eq!(press(&mut menu, Action::Confirm), TabMenuEvent::Pending);
        assert!(draw(&mut menu, 40, 10).contains("Nothing matches"));
    }

    #[test]
    fn draws_the_tabs_with_hotkeys_over_its_panel() {
        let mut menu = menu();
        insta::assert_snapshot!(draw(&mut menu, 40, 10));
        typed(&mut menu, "www");
        let text = draw(&mut menu, 40, 10);
        assert!(
            text.contains("Filter: www")
                && text.contains("web:/var/www")
                && !text.contains("~/src"),
            "{text}"
        );
    }
}
