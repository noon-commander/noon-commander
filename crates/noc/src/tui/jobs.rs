//! The list of jobs, as mc's Background jobs (Ctrl-X J): how far each is, with buttons that
//! bring the selected one to the front or abort it.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::cells::{self, Align};
use super::dialog::{Colors, button_line, draw_box};
use super::keymap::{Action, Resolved};
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the window, borders included, where the screen has room.
const WIDTH: u16 = 76;
/// Columns for what a job does, how far it is, and the time it has left.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Show,
    Abort,
    Close,
}

const BUTTONS: [Button; 3] = [Button::Show, Button::Abort, Button::Close];

/// The state of the list. Its rows come from the app with every key and every draw, as jobs
/// come and go.
#[derive(Debug, Default)]
pub(crate) struct JobsList {
    /// The selected job, while it is there.
    selected: Option<u64>,
    /// Its row, for when it has gone: the row after it is selected then.
    row: usize,
    /// The button with the focus.
    focus: usize,
}

impl JobsList {
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

    /// Takes a key: arrows choose a job, Left, Right, and Tab a button, Enter or Space press
    /// it, and Esc closes the list.
    pub(crate) fn handle(&mut self, input: Resolved, rows: &[Row]) -> JobsEvent {
        let Resolved::Action(action) = input else {
            return JobsEvent::Pending;
        };
        let index = self.index(rows);
        let last = rows.len().saturating_sub(1);
        let count = BUTTONS.len();
        match action {
            Action::Up => self.select(rows, index.saturating_sub(1)),
            Action::Down => self.select(rows, (index + 1).min(last)),
            Action::Home | Action::PageUp => self.select(rows, 0),
            Action::End | Action::PageDown => self.select(rows, last),
            Action::Right | Action::NextField => self.focus = (self.focus + 1) % count,
            Action::Left | Action::PrevField => self.focus = (self.focus + count - 1) % count,
            Action::Confirm | Action::Toggle => {
                let id = rows.get(index).map(|row| row.id);
                return match (BUTTONS[self.focus], id) {
                    (Button::Close, _) => JobsEvent::Closed,
                    (Button::Show, Some(id)) => JobsEvent::Show(id),
                    (Button::Abort, Some(id)) => JobsEvent::Abort(id),
                    (Button::Show | Button::Abort, None) => JobsEvent::Pending,
                };
            }
            Action::Cancel => return JobsEvent::Closed,
            _ => {}
        }
        JobsEvent::Pending
    }

    /// Draws the list centered in `area`: a row for each job, scrolled to the selected one.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme, rows: &[Row]) {
        let colors = Colors::of(theme, false);
        let listed = u16::try_from(rows.len().max(1)).unwrap_or(u16::MAX);
        // Borders, the rows, a blank line, the buttons.
        let size = (WIDTH, listed.saturating_add(4));
        let title = fl!("jobs-title");
        let inner = draw_box(frame, area, size, &title, colors, theme);
        let width = usize::from(inner.width);
        let visible = usize::from(inner.height.saturating_sub(2));
        let line = |shown: usize| {
            let y = inner.y + u16::try_from(shown).unwrap_or(0);
            Rect::new(inner.x, y, inner.width, 1)
        };
        if rows.is_empty() && visible > 0 {
            let none = cells::fit(&fl!("jobs-none"), width, Align::Left);
            frame.render_widget(Line::raw(none), line(0));
        }
        let index = self.index(rows);
        let top = (index + 1).saturating_sub(visible);
        for (shown, (number, row)) in rows.iter().enumerate().skip(top).take(visible).enumerate() {
            let text = format!(
                "{} {} {}  {}",
                cells::fit(&row.title, TITLE, Align::Left),
                cells::fit(&row.state, STATE, Align::Right),
                cells::fit(&row.left, LEFT, Align::Right),
                row.current
            );
            let text = cells::fit(&text, width, Align::Left);
            let text = if number == index {
                Line::styled(text, colors.focused_style())
            } else {
                Line::raw(text)
            };
            frame.render_widget(text, line(shown));
        }
        if inner.height >= 2 {
            let labels = [fl!("jobs-show"), fl!("dialog-abort"), fl!("dialog-ok")];
            let buttons = button_line(&labels, 0, Some(self.focus), colors);
            let y = inner.bottom() - 1;
            frame.render_widget(buttons, Rect::new(inner.x, y, inner.width, 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

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
        list.handle(Resolved::Action(action), rows)
    }

    fn draw(list: &JobsList, rows: &[Row], height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|frame| list.render(frame, frame.area(), &Theme::terminal(), rows))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn shows_each_job_and_the_buttons() {
        let list = JobsList::default();
        insta::assert_snapshot!(draw(&list, &rows(&[1, 2]), 8));
        let text = draw(&list, &[], 8);
        assert!(text.contains("No jobs are running"), "{text}");
    }

    #[test]
    fn arrows_choose_a_job_and_the_buttons_act_on_it() {
        let rows = rows(&[1, 2, 3]);
        let mut list = JobsList::default();
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(1));
        press(&mut list, Action::Down, &rows);
        press(&mut list, Action::Down, &rows);
        press(&mut list, Action::Down, &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Show(3));
        press(&mut list, Action::Up, &rows);
        press(&mut list, Action::Right, &rows);
        assert_eq!(press(&mut list, Action::Toggle, &rows), JobsEvent::Abort(2));
        press(&mut list, Action::NextField, &rows);
        assert_eq!(press(&mut list, Action::Confirm, &rows), JobsEvent::Closed);
        press(&mut list, Action::Left, &rows);
        press(&mut list, Action::Home, &rows);
        assert_eq!(
            press(&mut list, Action::Confirm, &rows),
            JobsEvent::Abort(1)
        );
        assert_eq!(press(&mut list, Action::Cancel, &rows), JobsEvent::Closed);
        assert_eq!(press(&mut list, Action::Confirm, &[]), JobsEvent::Pending);
    }

    #[test]
    fn the_selection_follows_its_job_and_then_its_row() {
        let mut list = JobsList::default();
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
        let mut list = JobsList::default();
        press(&mut list, Action::End, &rows);
        let text = draw(&list, &rows, 7);
        assert!(text.contains("/srv/file6"), "{text}");
        assert!(!text.contains("/srv/file1"), "{text}");
    }
}
