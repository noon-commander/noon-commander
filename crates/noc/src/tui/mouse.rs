//! The mouse, as the app sees it: presses at a cell of the screen. The event loop turns
//! crossterm's events into [`Pointer`]s, so that widgets never see raw mouse events, as they
//! never see raw keys. Moving and dragging do nothing, and terminals send no double clicks, so
//! [`Clicks`] tells them from two clicks by their time and cell.

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

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
