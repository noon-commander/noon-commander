//! The list of jobs, as mc's Background jobs (`f j`): how far each is, with buttons that
//! bring the selected one to the front or abort it. A dialog with a picker without a filter,
//! which this wraps (ADR 0073).

use noc_scroll::Motion;
use noc_scroll::scrollbar::Bar;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Button, Dialog, DialogEvent};
use super::edit::Registers;
use super::keymap::{Action, Resolved, Typed};
use super::mouse::{Pointer, Scroll};
use super::picker::{Body, MenuLine, Picker};
use super::theme::Theme;
use super::window::{Colors, Tone};
use crate::i18n::fl;

/// Columns for what a job does, how far it is, and the time it has left, unless the words in
/// them need more.
const TITLE: usize = 8;
const STATE: usize = 9;
const LEFT: usize = 12;

/// A job in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) id: u64,
    /// What it does, such as `Copy`.
    pub(crate) title: String,
    /// How far it is, such as `37%` or `waiting`.
    pub(crate) state: String,
    /// The time it has left, such as `ETA 1:30`, or empty.
    pub(crate) left: String,
    /// The entry at hand.
    pub(crate) current: String,
}

/// What a key did in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobsEvent {
    Pending,
    /// Bring the job to the front, in its window.
    Show(u64),
    Abort(u64),
    Closed,
}

/// The state of the list. Its rows come from the app with every key and every draw, as jobs
/// come and go.
#[derive(Debug)]
pub(crate) struct JobsList {
    /// The selected job, while it is there.
    selected: Option<u64>,
    /// Its row, for when it has gone: the row after it is selected then.
    row: usize,
    /// The rows, and Show, the default, Abort, and OK.
    dialog: Dialog,
}

impl JobsList {
    /// The list, with its rows in front.
    pub(crate) fn new() -> Self {
        let buttons = vec![Button::Show, Button::Abort, Button::Ok];
        let dialog = Dialog::question(&fl!("jobs-title"), "", buttons, 0, Tone::Plain)
            .with_picker(Picker::without_filter().bare());
        Self {
            selected: None,
            row: 0,
            dialog,
        }
    }

    /// The row of the selected job, or the nearest one.
    fn index(&self, rows: &[Row]) -> usize {
        self.selected
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .unwrap_or_else(|| self.row.min(rows.len().saturating_sub(1)))
    }

    fn select(&mut self, rows: &[Row], index: usize) {
        self.row = index;
        self.selected = rows.get(index).map(|row| row.id);
    }

    /// Gives the picker a row for each of `rows`, with the cursor on the selected job; the row
    /// it is on.
    fn put_cursor(&mut self, rows: &[Row]) -> usize {
        let index = self.index(rows);
        if let Some(picker) = self.dialog.picker_mut() {
            picker.set_rows(vec![Vec::new(); rows.len()]);
            picker.set_cursor(index);
        }
        index
    }

    /// Selects the job under the cursor of the picker, if the cursor moved from `before`.
    fn take_cursor(&mut self, rows: &[Row], before: usize) {
        let index = self.dialog.picker().map_or(before, Picker::cursor);
        if index != before {
            self.select(rows, index);
        }
    }

    /// Takes a key as its dialog does: on the rows the motions choose a job, as many as a count
    /// says, and Tab, Left, and Right go to the buttons; Enter or Space presses the button with
    /// the focus, or Show on the rows, Alt and a letter its button; Esc closes the list.
    pub(crate) fn handle(&mut self, typed: Typed, rows: &[Row]) -> JobsEvent {
        let before = self.put_cursor(rows);
        let event = self.dialog.handle_typed(typed, &mut Registers::default());
        self.take_cursor(rows, before);
        match event {
            DialogEvent::Pressed(button) => self.press(button, rows),
            DialogEvent::Cancelled => JobsEvent::Closed,
            DialogEvent::Pending | DialogEvent::Opened(_) => JobsEvent::Pending,
        }
    }

    /// What `button` does to the selected job.
    fn press(&self, button: Button, rows: &[Row]) -> JobsEvent {
        let id = rows.get(self.index(rows)).map(|row| row.id);
        match (button, id) {
            (Button::Show, Some(id)) => JobsEvent::Show(id),
            (Button::Abort, Some(id)) => JobsEvent::Abort(id),
            (Button::Show | Button::Abort, None) => JobsEvent::Pending,
            _ => JobsEvent::Closed,
        }
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.dialog.picker().and_then(Picker::bar)
    }

    /// Chooses a job as a press on the scroll bar asks: the next one or the one before, a page
    /// on or back, or the nearest on the screen scrolled to where the thumb was dragged.
    pub(crate) fn scroll(&mut self, scroll: Scroll, rows: &[Row]) {
        let before = self.put_cursor(rows);
        let Some(picker) = self.dialog.picker_mut() else {
            return;
        };
        let action = match scroll {
            Scroll::By(Motion::Up) => Action::Up,
            Scroll::By(Motion::Down) => Action::Down,
            Scroll::By(Motion::PageUp) => Action::PageUp,
            Scroll::By(Motion::PageDown) => Action::PageDown,
            Scroll::By(_) => return,
            Scroll::To(top) => {
                picker.scroll_to(top);
                self.take_cursor(rows, before);
                return;
            }
        };
        picker.handle(Resolved::Action(action));
        self.take_cursor(rows, before);
    }

    /// A step of the wheel, as `action` `count` times: it scrolls the rows, wherever the focus
    /// is, keeping the selected job among them.
    pub(crate) fn wheel(&mut self, (action, count): (Action, usize), rows: &[Row]) {
        let before = self.put_cursor(rows);
        if let Some(picker) = self.dialog.picker_mut() {
            picker.wheel(action, count);
        }
        self.take_cursor(rows, before);
    }

    /// Takes a press of the mouse, where the list was drawn last, as its dialog does: a click
    /// chooses a job, a double click shows it, and one on a button returns Enter, which presses
    /// it.
    pub(crate) fn pointer(&mut self, pointer: Pointer, rows: &[Row]) -> Option<Action> {
        let before = self.put_cursor(rows);
        let event = self.dialog.pointer(pointer);
        self.take_cursor(rows, before);
        match event {
            DialogEvent::Pressed(_) => Some(Action::Confirm),
            DialogEvent::Pending | DialogEvent::Opened(_) | DialogEvent::Cancelled => None,
        }
    }

    /// Draws the list centered in `area`: a row for each job, scrolled to the selected one, or
    /// that none runs.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        rows: &[Row],
    ) {
        self.put_cursor(rows);
        let colors = Colors::plain(theme);
        let lines: Vec<MenuLine> = (0..rows.len()).map(MenuLine::Row).collect();
        let body = Body {
            lines: &lines,
            message: rows.is_empty().then(|| fl!("jobs-none")),
            ..Body::default()
        };
        let widths = rows.iter().fold((TITLE, STATE, LEFT), |widths, row| {
            (
                widths.0.max(cells::width(&row.title)),
                widths.1.max(cells::width(&row.state)),
                widths.2.max(cells::width(&row.left)),
            )
        });
        let marks = theme.glyphs.marks;
        self.dialog
            .render_picker(frame, area, theme, body, &mut |shown| {
                let styles = if shown.selected {
                    (colors.focused_style(), theme.truncated_cursor)
                } else {
                    colors.body_styles()
                };
                let (title, state, left) = widths;
                let row = &rows[shown.index];
                let gap = |cells: usize| Span::styled(" ".repeat(cells), styles.0);
                let mut spans = cells::fit(&row.title, title, Align::Left, styles, marks);
                spans.push(gap(1));
                spans.extend(cells::fit(&row.state, state, Align::Right, styles, marks));
                spans.push(gap(1));
                spans.extend(cells::fit(&row.left, left, Align::Right, styles, marks));
                spans.push(gap(2));
                let room = shown.width.saturating_sub(title + state + left + 4);
                spans.extend(cells::fit(&row.current, room, Align::Left, styles, marks));
                Line::from(cells::fit_spans(spans, shown.width, styles, marks)).style(styles.0)
            });
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::keymap::Extra;

    fn rows(ids: &[u64]) -> Vec<Row> {
        ids.iter()
            .map(|&id| Row {
                id,
                title: "Copy".to_owned(),
                state: format!("{id}0%"),
                left: if id == 1 {
                    "ETA 1:30".to_owned()
                } else {
                    String::new()
                },
                current: format!("/srv/file{id}"),
            })
            .collect()
    }

    fn press(list: &mut JobsList, action: Action, rows: &[Row]) -> JobsEvent {
        list.handle(Typed::from(Resolved::Action(action)), rows)
    }

    fn draw(list: &mut JobsList, rows: &[Row], height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|frame| list.render(frame, frame.area(), &Theme::terminal(), rows))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn shows_each_job_and_the_buttons() {
        let mut list = JobsList::new();
        insta::assert_snapshot!(draw(&mut list, &rows(&[1, 2]), 8));
        let text = draw(&mut list, &[], 8);
        assert!(text.contains("No jobs are running"), "{text}");
    }

    #[test]
    fn the_rows_and_the_buttons_are_stops_as_in_every_dialog() {
        let rows = rows(&[1, 2, 3]);
        let mut list = JobsList::new();
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(1));
        for _ in 0..3 {
            press(&mut list, Action::Down, &rows);
        }
        assert_eq!(press(&mut list, Action::Toggle, &rows), JobsEvent::Show(3));
        press(&mut list, Action::Up, &rows);
        // Right leaves the rows for the buttons, where the motions go through the stops.
        press(&mut list, Action::Right, &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(2));
        press(&mut list, Action::Right, &rows);
        assert_eq!(press(&mut list, Action::Toggle, &rows), JobsEvent::Abort(2));
        press(&mut list, Action::Down, &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Closed);
        // Past the last button, back to the rows, which keep their cursor.
        press(&mut list, Action::NextField, &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(2));
        let twice = Typed {
            resolved: Resolved::Action(Action::Up),
            extra: Extra {
                count: Some(2),
                ..Extra::default()
            },
        };
        list.handle(twice, &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(1));
        press(&mut list, Action::End, &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(3));
        assert_eq!(press(&mut list, Action::Cancel, &rows), JobsEvent::Closed);
        assert_eq!(press(&mut list, Action::Confirm, &[]), JobsEvent::Pending);
    }

    #[test]
    fn alt_and_a_letter_press_its_button() {
        let rows = rows(&[1, 2]);
        let mut list = JobsList::new();
        let hotkey = |list: &mut JobsList, c, rows: &[Row]| {
            list.handle(Typed::from(Resolved::Hotkey(c)), rows)
        };
        press(&mut list, Action::Down, &rows);
        assert_eq!(hotkey(&mut list, 's', &rows), JobsEvent::Show(2));
        assert_eq!(hotkey(&mut list, 'a', &rows), JobsEvent::Abort(2));
        assert_eq!(hotkey(&mut list, 'o', &rows), JobsEvent::Closed);
        assert_eq!(hotkey(&mut list, 'x', &rows), JobsEvent::Pending);
        assert_eq!(hotkey(&mut list, 's', &[]), JobsEvent::Pending);
    }

    #[test]
    fn the_selection_follows_its_job_and_then_its_row() {
        let mut list = JobsList::new();
        press(&mut list, Action::Down, &rows(&[1, 2, 3]));
        // A job above ends: the selection stays on its job.
        assert_eq!(
            press(&mut list, Action::Confirm, &rows(&[2, 3])),
            JobsEvent::Show(2)
        );
        // The job itself ends: the row after it takes its place.
        assert_eq!(
            press(&mut list, Action::Confirm, &rows(&[1, 3])),
            JobsEvent::Show(3)
        );
        assert_eq!(
            press(&mut list, Action::Confirm, &rows(&[1])),
            JobsEvent::Show(1)
        );
    }

    #[test]
    fn a_long_list_scrolls_to_the_selection() {
        let rows = rows(&[1, 2, 3, 4, 5, 6]);
        let mut list = JobsList::new();
        press(&mut list, Action::End, &rows);
        let text = draw(&mut list, &rows, 7);
        assert!(text.contains("/srv/file6"), "{text}");
        assert!(!text.contains("/srv/file1"), "{text}");
    }

    #[test]
    fn the_scroll_bar_and_the_wheel_choose_a_job_on_the_screen() {
        let rows = rows(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let mut list = JobsList::new();
        let top = draw(&mut list, &rows, 9);
        assert!(top.contains('▲') && top.contains('█'), "{top}");
        let bar = list.bar().unwrap();
        list.scroll(Scroll::By(Motion::Down), &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(2));
        list.scroll(Scroll::To(bar.dragged(0, u16::MAX)), &rows);
        let end = draw(&mut list, &rows, 9);
        assert!(
            end.contains("/srv/file9") && !end.contains("/srv/file1"),
            "{end}"
        );
        assert_eq!(
            press(&mut list, Action::Confirm, &rows),
            JobsEvent::Show(7),
            "a job on the screen"
        );
        // On a button too, the wheel scrolls the rows.
        press(&mut list, Action::NextField, &rows);
        list.wheel((Action::Up, 3), &rows);
        let up = draw(&mut list, &rows, 9);
        assert!(
            up.contains("/srv/file2") && !up.contains("/srv/file7"),
            "{up}"
        );
        press(&mut list, Action::PrevField, &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(4));
    }
}
