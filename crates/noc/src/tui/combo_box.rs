//! A combo box: one of a few choices, shown as `[choice ▼]` in the colors of text fields, whose
//! list opens under it to take another, as Far's combo boxes and Qt's `QComboBox` without text
//! of their own (ADR 0057). The dialogs and the Configuration dialog hold them, and draw the list
//! over themselves, in the colors of fields too, so that it stands apart from what it covers;
//! while it is open the list has the keys, as a menu does, in a context of its own,
//! `combo_box`: moving its cursor does not change the choice until Enter takes the row, and Esc
//! keeps the choice it had.

use std::cell::{Cell, RefCell};

use noc_scroll::scrollbar::{self, Bar};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear};

use super::cells::{self, Align};
use super::dialog::pop_up;
use super::keymap::{Action, Extra};
use super::label::Label;
use super::mouse::{Drawn, Pointer, Press};
use super::scroll::Window;
use super::theme::Theme;

/// Rows the list shows at most, as the list of completions.
const MAX_ROWS: usize = 10;
/// Cells of the box around the choice: `[`, and a blank, the arrow, and `]` after it.
const BOX_CELLS: usize = 4;
/// Cells of a row of the list around the choice: the mark of the choice and a blank before it,
/// and a blank after it.
const ROW_CELLS: usize = 3;

/// What a key or a press did to the list of a combo box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComboEvent {
    /// The list stays as it was, or moved its cursor.
    Pending,
    /// The list closed, the choice as it was.
    Closed,
    /// The list closed, with the choice at this index taken.
    Chosen(usize),
}

/// One of its choices, and the list to take another, while it is open.
#[derive(Debug)]
pub(crate) struct ComboBox {
    /// Each with the letter that Alt and it take in the open list.
    items: Vec<Label>,
    chosen: usize,
    list: Option<List>,
    /// Where the box was drawn last, which the list opens from.
    drawn: Cell<Option<Rect>>,
    /// Where a click closed the list last: the second click of a double click there ends with
    /// the list, rather than acting on what is under it.
    closed_at: Option<Position>,
}

/// The open list: the row under the cursor, the rows on screen as vim's window shows lines, and
/// where the last render drew them.
#[derive(Debug, Default)]
struct List {
    cursor: Cell<usize>,
    window: Cell<Window>,
    page: Cell<usize>,
    drawn: RefCell<Drawn>,
    bar: Cell<Option<Bar>>,
}

impl ComboBox {
    /// A combo box of `items`, each with the letter an `&` marks, of which `chosen` is chosen.
    pub(crate) fn new(items: &[String], chosen: usize) -> Self {
        Self {
            items: items
                .iter()
                .map(|item| Label::parse(&cells::sanitize(item.as_bytes())))
                .collect(),
            chosen: chosen.min(items.len().saturating_sub(1)),
            list: None,
            drawn: Cell::new(None),
            closed_at: None,
        }
    }

    /// The index of the choice.
    pub(crate) fn chosen(&self) -> usize {
        self.chosen
    }

    /// The text of the choice, without its letter.
    pub(crate) fn text(&self) -> &str {
        self.items
            .get(self.chosen)
            .map_or("", |item| item.text.as_str())
    }

    /// The letters of the choices, which Alt and they take in the open list.
    #[cfg(test)]
    pub(crate) fn letters(&self) -> Vec<char> {
        self.items.iter().filter_map(Label::letter).collect()
    }

    /// Whether the list is open, and has the keys.
    pub(crate) fn is_open(&self) -> bool {
        self.list.is_some()
    }

    /// Opens the list, with its cursor on the choice.
    pub(crate) fn open(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let list = List::default();
        list.cursor.set(self.chosen);
        list.page.set(1);
        self.list = Some(list);
    }

    /// Takes the next choice, or the one before, round, without the list.
    pub(crate) fn cycle(&mut self, forward: bool) {
        let count = self.items.len().max(1);
        self.chosen = if forward {
            (self.chosen + 1) % count
        } else {
            (self.chosen + count - 1) % count
        };
    }

    /// Takes a key, typed with `extra`, while the list is open: Up and Down move its cursor as
    /// many rows as the count says, the page keys a page, Home and End to the first row and the
    /// last, or to the row a count gives; Enter takes the row and closes the list, and Esc
    /// closes it as it was. It is modal: other keys do nothing.
    pub(crate) fn handle(&mut self, action: Action, extra: Extra) -> ComboEvent {
        let len = self.items.len();
        let Some(list) = &mut self.list else {
            return ComboEvent::Pending;
        };
        let bottom = len.saturating_sub(1);
        let times = extra.times();
        let page = list.page.get().max(1);
        let cursor = list.cursor.get_mut();
        // The row a count gives, from 1, or `or` without one.
        let counted = |or: usize| {
            extra
                .count
                .map_or(or, |row| row.saturating_sub(1))
                .min(bottom)
        };
        match action {
            Action::Up => *cursor = cursor.saturating_sub(times),
            Action::Down => *cursor = cursor.saturating_add(times).min(bottom),
            Action::PageUp | Action::PageDown => {
                let down = action == Action::PageDown;
                list.window.get_mut().page(down, times, cursor, len, page);
            }
            Action::Home => *cursor = counted(0),
            Action::End => *cursor = counted(bottom),
            Action::Confirm => {
                let row = *cursor;
                return self.take(row);
            }
            Action::Cancel => {
                self.list = None;
                return ComboEvent::Closed;
            }
            _ => {}
        }
        ComboEvent::Pending
    }

    /// Alt and `c` while the list is open: takes the row whose letter it is, and closes the
    /// list; a letter no row has does nothing.
    pub(crate) fn hotkey(&mut self, c: char) -> ComboEvent {
        if self.list.is_none() {
            return ComboEvent::Pending;
        }
        match self.items.iter().position(|item| item.has(c)) {
            Some(row) => self.take(row),
            None => ComboEvent::Pending,
        }
    }

    /// Takes the choice at `row` and closes the list.
    fn take(&mut self, row: usize) -> ComboEvent {
        self.chosen = row.min(self.items.len().saturating_sub(1));
        self.list = None;
        ComboEvent::Chosen(self.chosen)
    }

    /// Takes a press of the mouse while the list is open: a click on a row takes it, one on the
    /// frame does nothing, and one elsewhere closes the list as it was, as Esc. The second
    /// click of the double click that opened it, and the wheel, which the window gives as keys,
    /// do nothing here.
    pub(crate) fn pointer(&mut self, Pointer { press, at }: Pointer) -> ComboEvent {
        let Some(list) = &self.list else {
            return ComboEvent::Pending;
        };
        if press != Press::Click {
            return ComboEvent::Pending;
        }
        let (row, on_frame) = {
            let drawn = list.drawn.borrow();
            (drawn.row_at(at), drawn.frame.contains(at))
        };
        let event = match row {
            Some(row) => self.take(row),
            None if on_frame => return ComboEvent::Pending,
            None => {
                self.list = None;
                ComboEvent::Closed
            }
        };
        self.closed_at = Some(at);
        event
    }

    /// Whether `pointer`, the press after the one that closed the list, is the second click of
    /// a double click there, which ends with the list rather than act on what is under it.
    /// Every press after the list closed asks, so that only the next one can be.
    pub(crate) fn spent(&mut self, pointer: Pointer) -> bool {
        let at = self.closed_at.take();
        pointer.press == Press::DoubleClick && at == Some(pointer.at)
    }

    /// The scroll bar of the open list at the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.list.as_ref()?.bar.get()
    }

    /// Shows the rows of the open list from `top`, where the thumb of its scroll bar was
    /// dragged, keeping the cursor on screen.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        let len = self.items.len();
        if let Some(list) = &mut self.list {
            let page = list.page.get().max(1);
            let cursor = list.cursor.get_mut();
            list.window.get_mut().scroll_to(top, cursor, len, page);
        }
    }

    /// Cells of the widest choice.
    fn widest(&self) -> usize {
        self.items
            .iter()
            .map(|item| cells::width(&item.text))
            .max()
            .unwrap_or(0)
    }

    /// Cells the box takes where it has room: as wide for every choice, the widest's.
    #[cfg(test)]
    pub(crate) fn width(&self) -> usize {
        self.widest() + BOX_CELLS
    }

    /// The box in `width` cells at most, as `[choice ▼]`: the choice padded to the widest, or
    /// cut in its middle where the room is short, in the colors of text fields, or of the
    /// choice under the cursor of the list if `focused` (see [`selected`]).
    pub(crate) fn box_spans(
        &self,
        width: usize,
        focused: bool,
        theme: &Theme,
    ) -> Vec<Span<'static>> {
        let room = self.widest().min(width.saturating_sub(BOX_CELLS));
        let style = if focused {
            selected(theme)
        } else {
            theme.dialog_input
        };
        let styles = (style, theme.truncated_cursor);
        let marks = theme.glyphs.marks;
        let mut spans = vec![Span::styled("[", style)];
        spans.extend(cells::fit(self.text(), room, Align::Left, styles, marks));
        spans.push(Span::styled(format!(" {}]", theme.glyphs.combo_box), style));
        spans
    }

    /// Says where the box was drawn, which the list opens from; `None` where it was not.
    pub(crate) fn place(&self, area: Option<Rect>) {
        self.drawn.set(area);
    }

    /// Draws the list, if it is open, from where the box was drawn, within `area`: under the
    /// box, or above it where there is more room there, in a frame in the colors of text fields,
    /// which a dialog has not, with a scroll bar on its right when the rows do not all fit. The
    /// row under the cursor is as a box with the focus (see [`selected`]), the choice has the
    /// mark of a choice taken, and the letters stand apart.
    pub(crate) fn render_list(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let (Some(list), Some(anchor)) = (&self.list, self.drawn.get()) else {
            return;
        };
        let mut drawn = list.drawn.borrow_mut();
        *drawn = Drawn::default();
        list.bar.set(None);
        let len = self.items.len();
        let height = u16::try_from(len.min(MAX_ROWS)).unwrap_or(0) + 2;
        let width = u16::try_from(self.widest() + ROW_CELLS + 2)
            .unwrap_or(u16::MAX)
            .max(anchor.width);
        let x = anchor.x.min(area.right().saturating_sub(width)).max(area.x);
        let outer = pop_up(anchor, area, x, (width, height));
        if outer.height < 3 || outer.width < 4 {
            return;
        }
        drawn.frame = outer;
        frame.render_widget(Clear, outer);
        let (body, mark) = (theme.dialog_input, theme.truncated_cursor);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .style(body);
        let inner = block.inner(outer);
        frame.render_widget(block, outer);
        let page = usize::from(inner.height).max(1);
        list.page.set(page);
        let (mut cursor, mut window) = (list.cursor.get(), list.window.get());
        window.follow(&mut cursor, len, page);
        list.cursor.set(cursor);
        list.window.set(window);
        let top = window.top();
        let bar = Rect::new(outer.right() - 1, inner.y, 1, inner.height);
        let position = (len, page, top);
        list.bar.set(scrollbar::render(
            frame,
            bar,
            position,
            body,
            theme.glyphs.scroll_bar,
        ));
        let room = usize::from(inner.width).saturating_sub(2);
        let rows = self.items.iter().enumerate().skip(top).take(page);
        for (offset, (row, item)) in rows.enumerate() {
            // The letter as on the command under the cursor of a menu, where the color of the
            // letters of dialogs may not show.
            let (style, hotkey) = if row == cursor {
                (selected(theme), theme.menu_hotkey_selected)
            } else {
                (body, theme.dialog_hotkey)
            };
            let chosen = if row == self.chosen {
                theme.glyphs.menu_marks.0
            } else {
                ' '
            };
            let mut spans = vec![Span::styled(format!("{chosen} "), style)];
            spans.extend(item.fit(room, (style, mark), hotkey, theme.glyphs.marks));
            let y = inner.y + u16::try_from(offset).unwrap_or(0);
            let line = Rect::new(inner.x, y, inner.width, 1);
            frame.render_widget(Line::from(spans).style(style), line);
            drawn.rows.push((row, line));
        }
    }
}

/// The colors of the choice under the cursor of the list, and of a box with the focus, over
/// those of text fields: the focus's, as on a button; where fields have the focus's colors, as
/// in mc-classic and the terminal theme, those of fields reversed, as Far shows the text
/// selected in a field.
fn selected(theme: &Theme) -> Style {
    let (input, focus) = (theme.dialog_input, theme.dialog_button_focused);
    if focus != input {
        focus
    } else if input.add_modifier.contains(Modifier::REVERSED) {
        input.not_reversed()
    } else {
        input.reversed()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn algorithms() -> ComboBox {
        let items = ["SHA-&256", "SHA-&512", "SHA-&1", "&MD5", "&BLAKE3"].map(str::to_owned);
        ComboBox::new(&items, 0)
    }

    fn key(combo: &mut ComboBox, action: Action) -> ComboEvent {
        combo.handle(action, Extra::default())
    }

    fn counted(combo: &mut ComboBox, action: Action, count: usize) -> ComboEvent {
        let extra = Extra {
            count: Some(count),
            ..Extra::default()
        };
        combo.handle(action, extra)
    }

    /// Draws the box at `at` on a screen of `width` by `height`, and the list over it, in
    /// `theme`.
    fn draw_themed(
        combo: &ComboBox,
        (width, height): (u16, u16),
        at: Position,
        theme: &Theme,
    ) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let spans = combo.box_spans(combo.width(), false, theme);
                let cells = spans.iter().map(Span::width).sum::<usize>();
                let area = Rect::new(at.x, at.y, u16::try_from(cells).unwrap(), 1);
                frame.render_widget(Line::from(spans), area);
                combo.place(Some(area));
                combo.render_list(frame, frame.area(), theme);
            })
            .unwrap();
        terminal
    }

    fn draw(combo: &ComboBox, size: (u16, u16), at: Position) -> Terminal<TestBackend> {
        draw_themed(combo, size, at, &Theme::terminal())
    }

    fn click(combo: &mut ComboBox, press: Press, at: Position) -> ComboEvent {
        combo.pointer(Pointer { press, at })
    }

    #[test]
    fn the_list_moves_its_cursor_and_enter_takes_the_row() {
        let mut combo = algorithms();
        assert_eq!(key(&mut combo, Action::Down), ComboEvent::Pending);
        assert_eq!(combo.chosen(), 0, "closed, it takes no keys");
        combo.open();
        assert!(combo.is_open());
        key(&mut combo, Action::Down);
        assert_eq!(combo.chosen(), 0, "the cursor moves, not the choice");
        counted(&mut combo, Action::Down, 9);
        key(&mut combo, Action::Up);
        assert_eq!(key(&mut combo, Action::Confirm), ComboEvent::Chosen(3));
        assert_eq!((combo.chosen(), combo.text()), (3, "MD5"));
        assert!(!combo.is_open());
        combo.open();
        key(&mut combo, Action::Home);
        assert_eq!(key(&mut combo, Action::Confirm), ComboEvent::Chosen(0));
        combo.open();
        counted(&mut combo, Action::End, 2);
        assert_eq!(
            key(&mut combo, Action::Confirm),
            ComboEvent::Chosen(1),
            "a count says the row"
        );
        combo.open();
        key(&mut combo, Action::End);
        key(&mut combo, Action::NextField);
        assert!(combo.is_open(), "other keys do nothing");
        assert_eq!(key(&mut combo, Action::Cancel), ComboEvent::Closed);
        assert_eq!(combo.chosen(), 1, "Esc keeps the choice");
    }

    #[test]
    fn alt_and_a_letter_take_its_row_in_the_open_list() {
        let mut combo = algorithms();
        assert_eq!(combo.letters(), ['2', '5', '1', 'm', 'b']);
        assert_eq!(combo.hotkey('b'), ComboEvent::Pending, "closed");
        combo.open();
        assert_eq!(combo.hotkey('z'), ComboEvent::Pending);
        assert!(combo.is_open());
        assert_eq!(combo.hotkey('B'), ComboEvent::Chosen(4));
        assert!(!combo.is_open());
    }

    #[test]
    fn left_and_right_go_round_the_choices() {
        let mut combo = algorithms();
        combo.cycle(false);
        assert_eq!(combo.text(), "BLAKE3");
        combo.cycle(true);
        combo.cycle(true);
        assert_eq!(combo.text(), "SHA-512");
    }

    #[test]
    fn draws_the_box_and_the_list_under_it() {
        let mut combo = algorithms();
        combo.cycle(true);
        combo.open();
        key(&mut combo, Action::Down);
        let terminal = draw(&combo, (24, 9), Position::new(2, 0));
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn the_box_and_its_list_are_in_the_colors_of_fields_the_cursor_apart() {
        use crate::tui::theme::ColorDepth;

        let mut combo = algorithms();
        combo.open();
        key(&mut combo, Action::Down);
        let mocha = Theme::by_name("catppuccin-mocha", ColorDepth::TrueColor).unwrap();
        for theme in [Theme::mc_classic(), mocha] {
            let terminal = draw_themed(&combo, (24, 9), Position::new(2, 0), &theme);
            let buffer = terminal.backend().buffer();
            let cell = |text: &str| {
                let at = super::super::mouse::find(buffer, text);
                buffer[(at.x, at.y)].clone()
            };
            let input = theme.dialog_input.bg.unwrap();
            assert_ne!(theme.dialog.bg, Some(input), "apart from the dialog");
            assert_eq!(cell("SHA-256").bg, input, "the box");
            assert_eq!(cell("BLAKE3").bg, input, "a row");
            assert_eq!(buffer[(2, 1)].bg, input, "the frame");
            let under = cell("SHA-512");
            let want = selected(&theme);
            assert_eq!(
                (Some(under.fg), Some(under.bg)),
                (want.fg, want.bg),
                "the cursor"
            );
            assert!(under.modifier.contains(want.add_modifier));
            assert_ne!(want, theme.dialog_input, "apart from the rows");
        }
    }

    #[test]
    fn the_list_opens_above_where_there_is_more_room() {
        let mut combo = algorithms();
        combo.open();
        let terminal = draw(&combo, (24, 9), Position::new(2, 7));
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn a_long_list_scrolls_with_a_bar() {
        let items: Vec<String> = (1..=30).map(|n| format!("item {n}")).collect();
        let mut combo = ComboBox::new(&items, 20);
        combo.open();
        let terminal = draw(&combo, (20, 14), Position::new(0, 0));
        let screen = terminal.backend().to_string();
        assert!(
            screen.contains("item 21") && !screen.contains("item 1 "),
            "{screen}"
        );
        assert!(combo.bar().is_some());
        combo.scroll_to(0);
        let screen = draw(&combo, (20, 14), Position::new(0, 0))
            .backend()
            .to_string();
        assert!(screen.contains("item 1 "), "{screen}");
        assert_eq!(
            key(&mut combo, Action::Confirm),
            ComboEvent::Chosen(4),
            "the cursor stays on screen, with vim's 'scrolloff'"
        );
    }

    #[test]
    fn a_click_takes_a_row_and_one_outside_closes() {
        let mut combo = algorithms();
        combo.open();
        let terminal = draw(&combo, (24, 9), Position::new(2, 0));
        let at = crate::tui::mouse::find(terminal.backend().buffer(), "MD5");
        let box_at = Position::new(3, 0);
        assert_eq!(
            click(&mut combo, Press::DoubleClick, box_at),
            ComboEvent::Pending,
            "the second click of the one that opened it"
        );
        assert_eq!(click(&mut combo, Press::Click, at), ComboEvent::Chosen(3));
        let double = Pointer {
            press: Press::DoubleClick,
            at,
        };
        assert!(combo.spent(double), "its second click ends with the list");
        assert!(!combo.spent(double), "once");
        combo.open();
        draw(&combo, (24, 9), Position::new(2, 0));
        let frame = Position::new(2, 1);
        assert_eq!(click(&mut combo, Press::Click, frame), ComboEvent::Pending);
        let outside = Position::new(22, 8);
        assert_eq!(click(&mut combo, Press::Click, outside), ComboEvent::Closed);
        assert_eq!(combo.chosen(), 3);
        let other = Pointer {
            press: Press::Click,
            at: outside,
        };
        assert!(!combo.spent(other), "a click is no second click");
    }
}
