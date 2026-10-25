//! The window over the viewer while it reads a part of a file that takes a while: where the
//! part starts, a gauge, and buttons that stop the reading or close the viewer.

use std::cell::{Cell, RefCell};
use std::time::Instant;

use noc_viewer::{Progress, Want};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use tokio_util::sync::CancellationToken;

use super::cells::{self, Align};
use super::dialog::{
    Colors, Tone, button_line, button_spots, draw_box, draw_separator, frame_around,
};
use super::keymap::{Action, Resolved};
use super::label::Label;
use super::mouse::{Drawn, Pointer};
use super::panel::short_size;
use super::progress::{gauge, ratio, unknown_gauge};
use super::spinner::{DELAY, Spin, Wait};
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the window, borders included.
const WIDTH: u16 = 50;

/// A button of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadButton {
    /// Ends the reading and keeps what it read.
    Stop,
    /// Closes the viewer.
    Close,
}

const BUTTONS: [ReadButton; 2] = [ReadButton::Stop, ReadButton::Close];

impl ReadButton {
    /// Its name, with the letter that Alt and it press.
    fn label(self) -> Label {
        Label::parse(&match self {
            Self::Stop => fl!("viewer-read-stop"),
            Self::Close => fl!("viewer-read-close"),
        })
    }
}

/// A part of the file the viewer reads, and its window, which shows once the reading has
/// taken as long as a spinner waits.
#[derive(Debug)]
pub(crate) struct Reading {
    pub(crate) want: Want,
    /// Ends the reading with what it read.
    stop: CancellationToken,
    progress: Option<Progress>,
    /// Since when it reads, for when the window shows and the shine of its gauge.
    wait: Wait,
    /// Whether the last frame drew the window, which then takes the keys.
    shown: Cell<bool>,
    /// Stop was pressed: the window is gone while the reading ends.
    stopped: bool,
    /// The part came whole while the window showed: it shows full for as long as a spinner
    /// waits, from its first frame, and takes no keys.
    finished: Option<Wait>,
    focus: usize,
    /// Where the last render drew it, for the mouse.
    drawn: RefCell<Drawn>,
}

impl Reading {
    pub(crate) fn new(want: Want, stop: CancellationToken) -> Self {
        Self {
            want,
            stop,
            progress: None,
            wait: Wait::default(),
            shown: Cell::new(false),
            stopped: false,
            finished: None,
            focus: 0,
            drawn: RefCell::default(),
        }
    }

    pub(crate) fn progress(&mut self, progress: Progress) {
        self.progress = Some(progress);
    }

    /// The wait whose spinner the window shows with, while it may still show.
    pub(crate) fn wait(&self) -> Option<&Wait> {
        (!self.stopped && self.finished.is_none()).then_some(&self.wait)
    }

    /// Whether the window is on screen and takes the keys.
    pub(crate) fn shown(&self) -> bool {
        self.shown.get() && !self.stopped && self.finished.is_none()
    }

    /// The part came whole: the window, if it showed, stays a while with its gauge full, so
    /// that it does not go at the percentage of its last report.
    pub(crate) fn finish(mut self) -> Option<Self> {
        if !self.shown() {
            return None;
        }
        if let Some(progress) = &mut self.progress {
            progress.read = progress.total;
        }
        self.finished = Some(Wait::default());
        Some(self)
    }

    /// When the window of a finished part goes, once a frame drew it full.
    pub(crate) fn goes(&self) -> Option<Instant> {
        self.finished.as_ref().and_then(Wait::shows)
    }

    /// Whether the window of a finished part has shown long enough, in a frame at `at`.
    pub(crate) fn gone(&self, at: Instant) -> bool {
        self.finished
            .as_ref()
            .is_some_and(|finished| at >= finished.since(at) + DELAY)
    }

    /// Ends the reading with what it read, and takes the window away.
    pub(crate) fn stop(&mut self) {
        self.stop.cancel();
        self.stopped = true;
    }

    /// Takes a key: Enter or Space presses the focused button, Esc stops, arrows and Tab move
    /// the focus, and Alt and a button's letter press it.
    pub(crate) fn handle(&mut self, input: Resolved) -> Option<ReadButton> {
        let action = match input {
            Resolved::Action(action) => action,
            Resolved::Hotkey(c) => {
                return BUTTONS.into_iter().find(|button| button.label().has(c));
            }
            Resolved::Insert(_) => return None,
        };
        let count = BUTTONS.len();
        match action {
            Action::Cancel => Some(ReadButton::Stop),
            Action::Confirm | Action::Toggle => Some(BUTTONS[self.focus]),
            Action::NextField | Action::Right | Action::Down => {
                self.focus = (self.focus + 1) % count;
                None
            }
            Action::PrevField | Action::Left | Action::Up => {
                self.focus = (self.focus + count - 1) % count;
                None
            }
            _ => None,
        }
    }

    /// Takes a press of the mouse, where the window was drawn last: a click on a button
    /// returns Enter, which presses it.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        let drawn = self.drawn.borrow().clone();
        drawn.button_press(pointer, &mut self.focus)
    }

    /// Draws the window over `area`, once the reading has taken as long as a spinner waits.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme, spin: &Spin) {
        if self.stopped || spin.wait_dots(&self.wait).is_none() {
            return;
        }
        self.shown.set(true);
        let colors = Colors::of(theme, Tone::Plain);
        // Borders, where the part starts, the gauge, how much it read, a line, the buttons.
        let inner = draw_box(
            frame,
            area,
            (WIDTH, 7),
            &fl!("viewer-reading"),
            colors,
            theme,
        );
        let width = usize::from(inner.width);
        let mut put = |index: u16, line: Line<'static>| {
            if index < inner.height {
                let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
                frame.render_widget(line, row);
            }
        };
        let text = |text: &str| {
            Line::from(cells::fit(
                text,
                width,
                Align::Left,
                colors.body_styles(),
                theme.glyphs.marks,
            ))
        };
        let offset = short_size(self.want.offset);
        let place = match self.progress.and_then(|progress| progress.size) {
            Some(size) => fl!("viewer-read-at", offset = offset, size = short_size(size)),
            None => fl!("viewer-read-at-unknown", offset = offset),
        };
        put(0, text(&place));
        match self.progress {
            Some(progress) => {
                put(1, gauge(ratio(progress.read, progress.total), width, theme));
                let count = fl!(
                    "viewer-read-count",
                    read = short_size(progress.read),
                    total = short_size(progress.total)
                );
                put(2, text(&count));
            }
            None => put(1, unknown_gauge(inner.width, spin.shine(&self.wait), theme)),
        }
        let labels: Vec<Label> = BUTTONS.iter().map(|button| button.label()).collect();
        let buttons = button_line(&labels, 0, Some(self.focus), colors);
        let row = Rect::new(inner.x, inner.y + 4, inner.width, 1);
        *self.drawn.borrow_mut() = Drawn {
            frame: frame_around(inner),
            rows: Vec::new(),
            buttons: if inner.height > 4 {
                button_spots(&labels, 0, row)
            } else {
                Vec::new()
            },
        };
        put(4, buttons);
        if inner.height > 4 {
            draw_separator(frame, inner, inner.y + 3, colors, theme);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::glyphs::Glyphs;
    use crate::tui::spinner::DELAY;

    const WANT: Want = Want {
        offset: 32 << 20,
        length: 16 << 20,
        new: true,
    };

    /// Draws `reading` first at `start`, then at `now`.
    fn draw(reading: &Reading, start: Instant, now: Instant) -> String {
        let mut terminal = Terminal::new(TestBackend::new(54, 9)).unwrap();
        for at in [start, now] {
            terminal
                .draw(|frame| {
                    let spin = Spin::new(at, start, false, Glyphs::PLAIN.spinners);
                    reading.render(frame, frame.area(), &Theme::terminal(), &spin);
                })
                .unwrap();
        }
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn shows_once_the_reading_has_taken_as_long_as_a_spinner_waits() {
        let reading = Reading::new(WANT, CancellationToken::new());
        let start = Instant::now();
        assert_eq!(draw(&reading, start, start).trim(), "");
        assert!(!reading.shown());
        let text = draw(&reading, start, start + DELAY + Duration::from_millis(1));
        assert!(reading.shown());
        assert!(text.contains("Reading"), "{text}");
    }

    #[test]
    fn shows_where_the_part_starts_and_how_much_of_it_was_read() {
        let mut reading = Reading::new(WANT, CancellationToken::new());
        reading.progress(Progress {
            read: 4 << 20,
            total: 16 << 20,
            size: Some(100 << 20),
        });
        let start = Instant::now();
        let text = draw(&reading, start, start + DELAY);
        assert!(text.contains("At 32.0M of 100M"), "{text}");
        assert!(text.contains("4.0M of 16.0M"), "{text}");
        assert!(text.contains("25%"), "{text}");
        assert!(text.contains("Stop") && text.contains("Close"), "{text}");
    }

    #[test]
    fn esc_stops_and_the_buttons_take_their_keys() {
        let stop = CancellationToken::new();
        let mut reading = Reading::new(WANT, stop.clone());
        let press = |reading: &mut Reading, action| reading.handle(Resolved::Action(action));
        assert_eq!(press(&mut reading, Action::Cancel), Some(ReadButton::Stop));
        assert_eq!(press(&mut reading, Action::Right), None);
        assert_eq!(
            press(&mut reading, Action::Confirm),
            Some(ReadButton::Close)
        );
        assert_eq!(
            reading.handle(Resolved::Hotkey('s')),
            Some(ReadButton::Stop)
        );
        reading.stop();
        assert!(stop.is_cancelled());
        assert!(reading.wait().is_none());
        assert!(!reading.shown());
    }
}
