//! The tab menu of `t h` and `t l`: the tabs of one panel, drawn over it as the location menu
//! is, to show one, or to close one. Typing filters it; while the filter is empty, `1` … `9` and
//! `0` show the first ten tabs.

use noc_config::SearchMode;
use noc_scroll::scrollbar::Bar;
use ratatui::Frame;
use ratatui::layout::Rect;

use super::app::Side;
use super::cells::{self, Align, Cut};
use super::dialog::Colors;
use super::keymap::{Action, Mode, Resolved};
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
    /// Close the tab at this index; the menu stays open.
    Close(usize),
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
    /// at `current`, which shows; its filter matches in `mode` (`ui.search_mode`).
    pub(crate) fn new(side: Side, titles: Vec<String>, current: usize, mode: SearchMode) -> Self {
        let mut picker = Picker::new(mode);
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

    /// Takes the tabs again after one closed; the cursor stays on its row, or goes to the last.
    pub(crate) fn set_titles(&mut self, titles: Vec<String>) {
        self.picker
            .set_rows(titles.iter().map(|title| vec![title.clone()]).collect());
        self.titles = titles;
        let last = self.picker.len().saturating_sub(1);
        self.picker.set_cursor(self.picker.cursor().min(last));
    }

    /// The mode keys are typed in: Insert while the filter takes them.
    /// The rows, for the count before a page key.
    pub(crate) fn picker_mut(&mut self) -> &mut Picker {
        &mut self.picker
    }

    pub(crate) fn mode(&self) -> Mode {
        self.picker.mode()
    }

    /// Whether its filter takes the keys and switches how it matches.
    pub(crate) fn switches(&self) -> bool {
        self.picker.switches()
    }

    /// Takes a key, as the picker does, and d d and F8, which close the tab under the cursor.
    pub(crate) fn handle(&mut self, input: Resolved) -> TabMenuEvent {
        let tab =
            |menu: &Self, row: usize| menu.picker.shown(0..menu.titles.len()).get(row).copied();
        match self.picker.handle(input) {
            Pick::Chosen(row) => tab(self, row).map_or(TabMenuEvent::Pending, TabMenuEvent::Show),
            Pick::Closed => TabMenuEvent::Closed,
            Pick::Other(Action::CloseTab) => {
                tab(self, self.picker.cursor()).map_or(TabMenuEvent::Pending, TabMenuEvent::Close)
            }
            Pick::Pending | Pick::Filtered | Pick::Other(_) => TabMenuEvent::Pending,
        }
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.picker.bar()
    }

    /// Shows the rows from `top`, where the thumb of the scroll bar was dragged.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        self.picker.scroll_to(top);
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
        // The frame, and a cell of room on either side.
        let width = u16::try_from(widest.unwrap_or(0) + 4)
            .unwrap_or(u16::MAX)
            .max(MIN_WIDTH);
        let body = Body {
            title: &title,
            width,
            lines: &lines,
            full: shown
                .get(self.picker.cursor())
                .map(|title| (*title).clone()),
            ..Body::default()
        };
        let focused = Colors::of(theme, false).focused_style();
        self.picker.render(frame, area, theme, body, &mut |row| {
            let styles = if row.selected {
                (focused, theme.truncated_cursor)
            } else {
                (theme.dialog, theme.truncated_dialog)
            };
            let fit = (Align::Left, Cut::Start);
            cells::fitted(shown[row.index], row.width, fit, styles, theme.glyphs.marks)
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
        TabMenu::new(Side::Left, titles.to_vec(), 1, SearchMode::Plain)
    }

    fn typed(menu: &mut TabMenu, text: &str) {
        menu.handle(Resolved::Action(Action::QuickSearch));
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
    fn starts_on_the_tab_that_shows_and_enter_shows_tabs() {
        let mut menu = menu();
        assert_eq!(press(&mut menu, Action::Confirm), TabMenuEvent::Show(1));
        press(&mut menu, Action::End);
        assert_eq!(press(&mut menu, Action::Confirm), TabMenuEvent::Show(3));
        assert_eq!(menu.handle(Resolved::Insert('1')), TabMenuEvent::Pending);
        assert_eq!(press(&mut menu, Action::Disconnect), TabMenuEvent::Pending);
        assert_eq!(press(&mut menu, Action::Cancel), TabMenuEvent::Closed);
    }

    #[test]
    fn close_tab_closes_the_tab_under_the_cursor_and_the_cursor_stays_on_its_row() {
        let titles = |titles: &[&str]| titles.iter().map(|&title| title.to_owned()).collect();
        let mut middle = menu();
        assert_eq!(press(&mut middle, Action::CloseTab), TabMenuEvent::Close(1));
        middle.set_titles(titles(&["~/src", "web:/var/www", "~/src/noc"]));
        assert_eq!(press(&mut middle, Action::Confirm), TabMenuEvent::Show(1));
        // Filtered, by its index among all the tabs.
        typed(&mut middle, "noc");
        assert_eq!(press(&mut middle, Action::CloseTab), TabMenuEvent::Close(2));
        middle.set_titles(titles(&["~/src", "web:/var/www"]));
        assert_eq!(
            press(&mut middle, Action::CloseTab),
            TabMenuEvent::Pending,
            "none matches"
        );
        // The last row closed, the cursor goes to the one before.
        let mut last = menu();
        assert_eq!(press(&mut last, Action::End), TabMenuEvent::Pending);
        assert_eq!(press(&mut last, Action::CloseTab), TabMenuEvent::Close(3));
        last.set_titles(titles(&["~/src", "/var/log", "web:/var/www"]));
        assert_eq!(press(&mut last, Action::Confirm), TabMenuEvent::Show(2));
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
    fn draws_the_tabs_over_its_panel() {
        let mut menu = menu();
        insta::assert_snapshot!(draw(&mut menu, 40, 13));
        typed(&mut menu, "www");
        let text = draw(&mut menu, 40, 10);
        assert!(
            text.contains("Filter: www")
                && text.contains(" = ║")
                && text.contains("web:/var/www")
                && !text.contains("~/src"),
            "{text}"
        );
    }
}
