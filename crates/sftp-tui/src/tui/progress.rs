//! The window of a running job: what it does, where it is, and a gauge.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, draw_box};
use super::keymap::{Action, Resolved};
use super::theme::Theme;
use crate::i18n::fl;

/// Width of the window, borders included.
const WIDTH: u16 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// Counting what to do.
    Scanning { items: u64 },
    /// At `current`, `done` of `total` entries and `bytes` of their bytes.
    Working {
        current: String,
        done: u64,
        total: u64,
        bytes: (u64, u64),
    },
}

/// How far a job is, for its window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) done: u64,
    pub(crate) total: u64,
    /// Bytes done and in all; both zero for jobs that move no data.
    pub(crate) bytes_done: u64,
    pub(crate) bytes_total: u64,
}

/// The window of a job, with Abort.
#[derive(Debug)]
pub(crate) struct JobView {
    title: String,
    /// What the job does, such as `Deleting`.
    doing: String,
    stage: Stage,
    aborting: bool,
}

impl JobView {
    pub(crate) fn new(title: String, doing: String) -> Self {
        Self {
            title,
            doing,
            stage: Stage::Scanning { items: 0 },
            aborting: false,
        }
    }

    pub(crate) fn scanning(&mut self, items: u64) {
        self.stage = Stage::Scanning { items };
    }

    /// The job is at `current`, shown as given, with `counts` behind it.
    pub(crate) fn working(&mut self, current: String, counts: Counts) {
        self.stage = Stage::Working {
            current,
            done: counts.done,
            total: counts.total,
            bytes: (counts.bytes_done, counts.bytes_total),
        };
    }

    /// Whether a key asks to abort: Esc, Enter, or Space, as Abort is the only button.
    pub(crate) fn wants_abort(input: Resolved) -> bool {
        matches!(
            input,
            Resolved::Action(Action::Cancel | Action::Confirm | Action::Toggle)
        )
    }

    /// Shows that the job was asked to stop.
    pub(crate) fn abort(&mut self) {
        self.aborting = true;
    }

    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let colors = Colors::of(theme, false);
        // Borders, what it does, where, the gauge, the count, a blank line, Abort.
        let inner = draw_box(frame, area, (WIDTH, 8), &self.title, colors, theme.shadow);
        let width = usize::from(inner.width);
        // On a small screen, what fits.
        let mut put = |index: u16, line: Line<'static>| {
            if index < inner.height {
                let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
                frame.render_widget(line, row);
            }
        };
        let (doing, current, ratio, count) = match &self.stage {
            Stage::Scanning { items } => (
                fl!("job-scanning"),
                String::new(),
                0.0,
                fl!("job-found", items = items.to_string()),
            ),
            Stage::Working {
                current,
                done,
                total,
                bytes: (bytes_done, bytes_total),
            } => {
                let done_total = (done.to_string(), total.to_string());
                // Data takes the time, so the gauge follows the bytes, if there are any.
                let (ratio, count) = if *bytes_total > 0 {
                    let count = fl!(
                        "job-count-bytes",
                        done = done_total.0,
                        total = done_total.1,
                        bytes_done = cells::size(*bytes_done, 7),
                        bytes_total = cells::size(*bytes_total, 7)
                    );
                    (ratio(*bytes_done, *bytes_total), count)
                } else {
                    let count = fl!("job-count", done = done_total.0, total = done_total.1);
                    (ratio(*done, *total), count)
                };
                (self.doing.clone(), current.clone(), ratio, count)
            }
        };
        let doing = if self.aborting {
            fl!("job-aborting")
        } else {
            doing
        };
        put(0, Line::raw(cells::fit(&doing, width, Align::Left)));
        put(1, Line::raw(cells::fit(&current, width, Align::Left)));
        put(2, gauge(ratio, width, theme));
        put(3, Line::raw(cells::fit(&count, width, Align::Left)));
        let abort = Span::styled(
            format!("[< {} >]", fl!("dialog-abort")),
            colors.focused_style(),
        );
        put(5, Line::from(abort).centered());
    }
}

/// `done` of `total`, from 0 to 1.
fn ratio(done: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    // Counts and sizes stay far below 2^52, where `f64` would round them.
    #[allow(clippy::cast_precision_loss)]
    let ratio = done as f64 / total as f64;
    ratio.clamp(0.0, 1.0)
}

/// A bar of `width` cells, filled for `ratio`, and the percentage after it. Block characters
/// show in any theme, where colors alone would not.
fn gauge(ratio: f64, width: usize, theme: &Theme) -> Line<'static> {
    let percent = format!(" {:>3.0}%", ratio * 100.0);
    let bar = width.saturating_sub(percent.len());
    // `ratio` is within 0 … 1, and bars are a few dozen cells.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let filled = ((bar as f64 * ratio).round() as usize).min(bar);
    let text = format!("{}{}", "█".repeat(filled), "░".repeat(bar - filled));
    Line::from(vec![Span::styled(text, theme.gauge), Span::raw(percent)])
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn draw(view: &JobView) -> String {
        let mut terminal = Terminal::new(TestBackend::new(64, 10)).unwrap();
        terminal
            .draw(|frame| view.render(frame, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn shows_counting_then_progress() {
        let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
        view.scanning(42);
        let text = draw(&view);
        assert!(text.contains("Counting"), "{text}");
        assert!(text.contains("42 found"), "{text}");

        let counts = Counts {
            done: 3,
            total: 12,
            ..Counts::default()
        };
        view.working("/srv/www/index.html".to_owned(), counts);
        insta::assert_snapshot!(draw(&view));

        // With bytes, the gauge follows them.
        let counts = Counts {
            done: 1,
            total: 2,
            bytes_done: 1536,
            bytes_total: 2048,
        };
        view.working("big".to_owned(), counts);
        let text = draw(&view);
        assert!(text.contains("1 of 2, 1536 of 2048 bytes"), "{text}");
        assert!(text.contains(" 75%"), "{text}");

        view.abort();
        assert!(draw(&view).contains("Aborting"));
    }

    #[test]
    fn the_gauge_fills_by_ratio() {
        let theme = Theme::terminal();
        let text = |ratio| gauge(ratio, 15, &theme).to_string();
        assert_eq!(text(0.0), "░░░░░░░░░░   0%");
        assert_eq!(text(0.5), "█████░░░░░  50%");
        assert_eq!(text(1.0), "██████████ 100%");
    }

    #[test]
    fn abort_is_the_only_button() {
        for action in [Action::Cancel, Action::Confirm, Action::Toggle] {
            assert!(JobView::wants_abort(Resolved::Action(action)));
        }
        assert!(!JobView::wants_abort(Resolved::Action(Action::Down)));
        assert!(!JobView::wants_abort(Resolved::Insert('a')));
    }
}
