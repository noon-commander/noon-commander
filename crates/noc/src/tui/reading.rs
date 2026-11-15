//! The window over the viewer while it reads a part of a file that takes a while: where the
//! part starts, a gauge, and Cancel, which stops the reading. A dialog, which this wraps.

use std::cell::Cell;
use std::time::Instant;

use noc_viewer::{Progress, Want};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio_util::sync::CancellationToken;

use super::dialog::{Button, Dialog, DialogEvent, Text};
use super::edit::Registers;
use super::keymap::{Action, Resolved, Typed};
use super::mouse::Pointer;
use super::panel::short_size;
use super::spinner::{DELAY, Spin, Wait};
use super::theme::Theme;
use super::window::Tone;
use crate::i18n::fl;

/// Where its lines of text are among those of its dialog.
const PLACE: usize = 0;
const GAUGE: usize = 1;
const COUNT: usize = 2;

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
    /// Cancel was pressed: the window is gone while the reading ends.
    stopped: bool,
    /// The part came whole while the window showed: it shows full for as long as a spinner
    /// waits, from its first frame, and takes no keys.
    finished: Option<Wait>,
    /// Where the part starts, the gauge, and how much it read, each on a line, and Cancel: as
    /// tall whether how much it read is known or not.
    dialog: Dialog,
}

impl Reading {
    pub(crate) fn new(want: Want, stop: CancellationToken) -> Self {
        let title = fl!("viewer-reading");
        let dialog = Dialog::question(&title, "", vec![Button::Cancel], 0, Tone::Plain)
            .with_text(Text::Line(String::new()))
            .with_text(Text::Gauge(None))
            .with_text(Text::Line(String::new()));
        let mut reading = Self {
            want,
            stop,
            progress: None,
            wait: Wait::default(),
            shown: Cell::new(false),
            stopped: false,
            finished: None,
            dialog,
        };
        reading.put_texts();
        reading
    }

    pub(crate) fn progress(&mut self, progress: Progress) {
        self.progress = Some(progress);
        self.put_texts();
    }

    /// Puts in words where the part starts and how much of it was read, and fills the gauge.
    fn put_texts(&mut self) {
        let offset = short_size(self.want.offset);
        let place = match self.progress.and_then(|progress| progress.size) {
            Some(size) => fl!("viewer-read-at", offset = offset, size = short_size(size)),
            None => fl!("viewer-read-at-unknown", offset = offset),
        };
        let (gauge, count) = match self.progress {
            Some(progress) => (
                Some((progress.read, progress.total)),
                fl!(
                    "viewer-read-count",
                    read = short_size(progress.read),
                    total = short_size(progress.total)
                ),
            ),
            // The line stays, so that the window does not grow when it is known.
            None => (None, String::new()),
        };
        self.dialog.replace_text(PLACE, Text::Line(place));
        self.dialog.replace_text(GAUGE, Text::Gauge(gauge));
        self.dialog.replace_text(COUNT, Text::Line(count));
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
        self.put_texts();
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

    /// Takes a key as its dialog does; whether it stops the reading: Esc, F10, Enter, Space,
    /// and a click on Cancel do.
    pub(crate) fn handle(&mut self, input: Resolved) -> bool {
        let typed = Typed::from(input);
        match self.dialog.handle_typed(typed, &mut Registers::default()) {
            DialogEvent::Pressed(_) | DialogEvent::Cancelled => true,
            DialogEvent::Pending | DialogEvent::Opened(_) => false,
        }
    }

    /// Takes a press of the mouse, where the window was drawn last: a click on Cancel returns
    /// Enter, which presses it.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<Action> {
        match self.dialog.pointer(pointer) {
            DialogEvent::Pressed(_) => Some(Action::Confirm),
            DialogEvent::Pending | DialogEvent::Opened(_) | DialogEvent::Cancelled => None,
        }
    }

    /// Draws the window over `area`, once the reading has taken as long as a spinner waits.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme, spin: &Spin) {
        if self.stopped || spin.wait_dots(&self.wait).is_none() {
            return;
        }
        self.shown.set(true);
        let shine = spin.shine(&self.wait);
        self.dialog.render_shining(frame, area, theme, shine);
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
        after_feed: false,
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
        assert!(text.contains("{ Cancel }"), "{text}");
    }

    #[test]
    fn as_tall_before_how_much_it_read_is_known() {
        let mut reading = Reading::new(WANT, CancellationToken::new());
        let start = Instant::now();
        let after = start + DELAY + Duration::from_millis(1);
        insta::assert_snapshot!(
            "until_how_much_it_read_is_known",
            screen(&reading, start, after)
        );
        reading.progress(Progress {
            read: 4 << 20,
            total: 16 << 20,
            size: Some(100 << 20),
        });
        insta::assert_snapshot!(
            "once_how_much_it_read_is_known",
            screen(&reading, start, after)
        );
    }

    /// `reading` drawn on a screen of 80 by 9, first at `start`, then at `now`.
    fn screen(reading: &Reading, start: Instant, now: Instant) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 9)).unwrap();
        for at in [start, now] {
            terminal
                .draw(|frame| {
                    let spin = Spin::new(at, start, false, Glyphs::PLAIN.spinners);
                    reading.render(frame, frame.area(), &Theme::terminal(), &spin);
                })
                .unwrap();
        }
        terminal.backend().to_string()
    }

    #[test]
    fn esc_enter_and_space_press_cancel_which_stops() {
        let stop = CancellationToken::new();
        let mut reading = Reading::new(WANT, stop.clone());
        let mut press = |action| reading.handle(Resolved::Action(action));
        for action in [Action::Cancel, Action::Confirm, Action::Toggle] {
            assert!(press(action), "{action:?}");
        }
        for action in [Action::Right, Action::NextField, Action::Down] {
            assert!(!press(action), "{action:?}");
        }
        assert!(press(Action::Confirm), "Cancel keeps the focus");
        assert!(
            !reading.handle(Resolved::Hotkey('s')),
            "no letters but those of buttons"
        );
        reading.stop();
        assert!(stop.is_cancelled());
        assert!(reading.wait().is_none());
        assert!(!reading.shown());
    }
}
