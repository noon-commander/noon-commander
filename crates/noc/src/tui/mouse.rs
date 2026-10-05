//! The mouse, as the app sees it: presses at a cell of the screen. The event loop turns
//! crossterm's events into [`Pointer`]s, so that widgets never see raw mouse events, as they
//! never see raw keys. Moving and dragging do nothing, and terminals send no double clicks, so
//! [`Clicks`] tells them from two clicks by their time and cell.

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::keymap::Action;

/// The longest time between the two clicks of a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    /// The left button.
    Click,
    /// The left button again, soon, on the same cell.
    DoubleClick,
    /// The right button.
    RightClick,
    WheelUp,
    WheelDown,
}

/// A press of the mouse at a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pointer {
    pub(crate) press: Press,
    pub(crate) at: Position,
}

/// The last click, to tell a double click.
#[derive(Debug, Default)]
pub(crate) struct Clicks {
    last: Option<(Position, Instant)>,
}

impl Clicks {
    /// The press that `event` is at `now`, if it is one the app takes.
    pub(crate) fn pointer(&mut self, event: MouseEvent, now: Instant) -> Option<Pointer> {
        let at = Position::new(event.column, event.row);
        let press = match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let double = self.last.is_some_and(|(cell, time)| {
                    cell == at && now.saturating_duration_since(time) <= DOUBLE_CLICK
                });
                // A third click starts again, as a click.
                self.last = (!double).then_some((at, now));
                if double {
                    Press::DoubleClick
                } else {
                    Press::Click
                }
            }
            MouseEventKind::Down(MouseButton::Right) => Press::RightClick,
            MouseEventKind::ScrollUp => Press::WheelUp,
            MouseEventKind::ScrollDown => Press::WheelDown,
            _ => return None,
        };
        if press != Press::Click && press != Press::DoubleClick {
            self.last = None;
        }
        Some(Pointer { press, at })
    }
}

/// Where a window drew what the mouse can press: its frame, the rows of its list that show,
/// and its buttons.
#[derive(Debug, Clone, Default)]
pub(crate) struct Drawn {
    pub(crate) frame: Rect,
    /// By their index in the list.
    pub(crate) rows: Vec<(usize, Rect)>,
    pub(crate) buttons: Vec<Rect>,
}

impl Drawn {
    /// The row of the list at `at`.
    pub(crate) fn row_at(&self, at: Position) -> Option<usize> {
        let row = self.rows.iter().find(|(_, area)| area.contains(at));
        row.map(|(index, _)| *index)
    }

    /// The button at `at`.
    pub(crate) fn button_at(&self, at: Position) -> Option<usize> {
        self.buttons.iter().position(|button| button.contains(at))
    }

    /// What `pointer` does in a menu drawn here: a click on a row puts `cursor` on it, a double
    /// click opens it too, as Enter, and a click outside the menu closes it, as Esc.
    pub(crate) fn menu_press(&self, pointer: Pointer, cursor: &mut usize) -> Option<Action> {
        let Pointer { press, at } = pointer;
        if !self.frame.contains(at) {
            return (press == Press::Click).then_some(Action::Cancel);
        }
        let row = self.row_at(at)?;
        match press {
            Press::Click => *cursor = row,
            Press::DoubleClick => {
                *cursor = row;
                return Some(Action::Confirm);
            }
            Press::RightClick | Press::WheelUp | Press::WheelDown => {}
        }
        None
    }

    /// What `pointer` does to the buttons drawn here: a click gives a button the `focus` and
    /// presses it, as Enter.
    pub(crate) fn button_press(&self, pointer: Pointer, focus: &mut usize) -> Option<Action> {
        if pointer.press != Press::Click {
            return None;
        }
        *focus = self.button_at(pointer.at)?;
        Some(Action::Confirm)
    }
}

/// Where `text` starts on the screen in `buffer`, for tests that click on it.
#[cfg(test)]
pub(crate) fn find(buffer: &ratatui::buffer::Buffer, text: &str) -> Position {
    let chars: Vec<char> = text.chars().collect();
    for y in 0..buffer.area.height {
        let line: Vec<&str> = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        let found = line.windows(chars.len()).position(|cells| {
            cells
                .iter()
                .zip(&chars)
                .all(|(cell, c)| cell.starts_with(*c))
        });
        if let Some(x) = found {
            return Position::new(u16::try_from(x).unwrap_or(0), y);
        }
    }
    panic!("no {text:?} on the screen");
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;

    fn event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left(column: u16, row: u16) -> MouseEvent {
        event(MouseEventKind::Down(MouseButton::Left), column, row)
    }

    #[test]
    fn tells_double_clicks_by_time_and_cell() {
        let mut clicks = Clicks::default();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let press = |clicks: &mut Clicks, event, ms| clicks.pointer(event, at(ms)).unwrap().press;
        assert_eq!(press(&mut clicks, left(3, 4), 0), Press::Click);
        assert_eq!(press(&mut clicks, left(3, 4), 300), Press::DoubleClick);
        assert_eq!(press(&mut clicks, left(3, 4), 350), Press::Click, "a third");
        assert_eq!(
            press(&mut clicks, left(3, 4), 1000),
            Press::Click,
            "too late"
        );
        assert_eq!(
            press(&mut clicks, left(4, 4), 1100),
            Press::Click,
            "elsewhere"
        );
        let right = event(MouseEventKind::Down(MouseButton::Right), 4, 4);
        assert_eq!(press(&mut clicks, right, 1200), Press::RightClick);
        assert_eq!(
            press(&mut clicks, left(4, 4), 1300),
            Press::Click,
            "not after another"
        );
    }

    #[test]
    fn menus_take_clicks_on_rows_and_close_at_clicks_outside() {
        let drawn = Drawn {
            frame: Rect::new(10, 5, 20, 6),
            rows: vec![(4, Rect::new(12, 7, 16, 1)), (5, Rect::new(12, 8, 16, 1))],
            buttons: Vec::new(),
        };
        let mut cursor = 4;
        let press = |press, x, y| Pointer {
            press,
            at: Position::new(x, y),
        };
        assert_eq!(
            drawn.menu_press(press(Press::Click, 15, 8), &mut cursor),
            None
        );
        assert_eq!(cursor, 5);
        let double = press(Press::DoubleClick, 15, 7);
        assert_eq!(drawn.menu_press(double, &mut cursor), Some(Action::Confirm));
        assert_eq!(cursor, 4);
        assert_eq!(
            drawn.menu_press(press(Press::Click, 15, 6), &mut cursor),
            None
        );
        let outside = press(Press::Click, 3, 3);
        assert_eq!(drawn.menu_press(outside, &mut cursor), Some(Action::Cancel));
        let wheel = press(Press::WheelDown, 3, 3);
        assert_eq!(drawn.menu_press(wheel, &mut cursor), None);
    }

    #[test]
    fn takes_presses_and_the_wheel_only() {
        let mut clicks = Clicks::default();
        let now = Instant::now();
        let wheel = clicks.pointer(event(MouseEventKind::ScrollDown, 1, 2), now);
        assert_eq!(
            wheel,
            Some(Pointer {
                press: Press::WheelDown,
                at: Position::new(1, 2)
            })
        );
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Middle),
            MouseEventKind::ScrollLeft,
        ] {
            assert_eq!(clicks.pointer(event(kind, 0, 0), now), None, "{kind:?}");
        }
    }
}
