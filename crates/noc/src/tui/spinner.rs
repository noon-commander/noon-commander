//! The spinner in front of what Noon Commander waits for: a scanner of eight cells whose head
//! sweeps to and fro with a trail that fades behind it, resting at each end; where there is
//! room for one cell only, dots that turn. A wait shows nothing for its first moment, so that
//! one that ends at once, such as a small local directory, does not flicker. Frames go by the
//! time, not by how often the screen is drawn.

use std::cell::Cell;
use std::time::{Duration, Instant};

use ratatui::style::Style;
use ratatui::text::Span;

use super::glyphs::Spinners;
use super::theme;

/// How long a wait goes before its spinner shows.
pub(crate) const DELAY: Duration = Duration::from_millis(150);
/// How long each frame of the scanner shows.
pub(crate) const SCANNER_FRAME: Duration = Duration::from_millis(40);
/// How long each frame of the dots shows.
pub(crate) const DOTS_FRAME: Duration = Duration::from_millis(80);
/// How long the shine takes to move one cell, so that it goes as fast over every bar, and how
/// long it rests between sweeps.
const SHINE_STEP: Duration = Duration::from_millis(40);
const SHINE_REST: Duration = Duration::from_millis(400);

/// Cells of the scanner.
const WIDTH: usize = 8;
/// Frames the head rests at the right end, and at the left one.
const HOLD_END: usize = 9;
const HOLD_START: usize = 30;
/// Frames of a sweep there and back with its rests.
const CYCLE: usize = WIDTH + HOLD_END + (WIDTH - 1) + HOLD_START;
/// How much of the color shows in the head and each cell of the trail behind it, per mille.
const TRAIL: [u16; 6] = [1000, 900, 650, 423, 275, 179];
/// The cell behind the head glows: its color brightened by so much, per mille.
const GLOW: u16 = 1150;
/// How much of the color shows in the cells that are not lit, per mille, and the least of that
/// which shows as they fade during the rests.
const UNLIT: u16 = 600;
const LEAST: u16 = 300;

/// When a wait began: as the first frame that drew it saw it. Its spinner shows [`DELAY`]
/// after that, from its first frame.
#[derive(Debug, Default)]
pub(crate) struct Wait(Cell<Option<Instant>>);

impl Wait {
    /// When it began, `at` if this is its first frame.
    pub(crate) fn since(&self, at: Instant) -> Instant {
        if let Some(since) = self.0.get() {
            return since;
        }
        self.0.set(Some(at));
        at
    }

    /// When its spinner shows, if a frame drew it.
    pub(crate) fn shows(&self) -> Option<Instant> {
        self.0.get().map(|since| since + DELAY)
    }
}

/// A shine that sweeps over bars, one cell every [`SHINE_STEP`], resting [`SHINE_REST`]
/// between sweeps: how long it has gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shine(Duration);

impl Shine {
    /// How many cells it has come in its sweep over `cells`; none while it rests.
    pub(crate) fn over(self, cells: u16) -> Option<u16> {
        let step = SHINE_STEP.as_millis();
        let sweep = u128::from(cells) * step;
        let into = self.0.as_millis() % (sweep + SHINE_REST.as_millis());
        (into < sweep).then(|| u16::try_from(into / step).unwrap_or(cells))
    }

    /// Where it is as it runs to and fro over `cells`, turning at either end without a rest:
    /// the cell from the start.
    pub(crate) fn bounce(self, cells: u16) -> u16 {
        let last = u128::from(cells.saturating_sub(1));
        if last == 0 {
            return 0;
        }
        let step = self.0.as_millis() / SHINE_STEP.as_millis() % (2 * last);
        u16::try_from(if step <= last { step } else { 2 * last - step }).unwrap_or(0)
    }

    /// The shine once it has come `cells` cells.
    #[cfg(test)]
    pub(crate) fn after(cells: u32) -> Self {
        Self(SHINE_STEP * cells)
    }
}

/// What spinners look like in a frame drawn at a moment.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spin {
    at: Instant,
    /// When the first frame was drawn, which the dots turn from.
    epoch: Instant,
    /// With `animations.spinners`; otherwise a still mark stands in their place.
    turns: bool,
    glyphs: Spinners,
}

impl Spin {
    pub(crate) fn new(at: Instant, epoch: Instant, turns: bool, glyphs: Spinners) -> Self {
        Self {
            at,
            epoch,
            turns,
            glyphs,
        }
    }

    /// The scanner of `wait` and a space after it, in `colors`, the color of its head and the
    /// style it is drawn over; `None` while the wait is too young to show.
    pub(crate) fn wait(&self, wait: &Wait, colors: (Style, Style)) -> Option<Vec<Span<'static>>> {
        let since = wait.since(self.at);
        let shown = self.at.checked_duration_since(since)?.checked_sub(DELAY)?;
        Some(self.scanner(shown, colors))
    }

    /// The scanner `elapsed` after it began, and a space.
    fn scanner(&self, elapsed: Duration, (color, surface): (Style, Style)) -> Vec<Span<'static>> {
        if !self.turns {
            return vec![Span::styled(format!("{} ", self.glyphs.still), color)];
        }
        let frame = frame_of(elapsed, SCANNER_FRAME, CYCLE);
        let scan = Scan::at(frame);
        let mut spans: Vec<Span<'static>> = (0..WIDTH)
            .map(|cell| {
                let (glyph, style) = match scan.trail(cell) {
                    Some(step) => (self.glyphs.lit, lit_style(step, color, surface)),
                    None => (self.glyphs.unlit, unlit_style(scan.fade(), color, surface)),
                };
                Span::styled(glyph.to_string(), style)
            })
            .collect();
        spans.push(Span::raw(" "));
        spans
    }

    /// The dots of `wait` in one cell, from when they show; `None` while the wait is too young
    /// to show.
    pub(crate) fn wait_dots(&self, wait: &Wait) -> Option<char> {
        let since = wait.since(self.at);
        let shown = self.at.checked_duration_since(since)?.checked_sub(DELAY)?;
        Some(self.dots_after(shown))
    }

    /// The dots in one cell, at their frame now, or the still mark.
    pub(crate) fn dots(&self) -> char {
        self.dots_after(self.at.saturating_duration_since(self.epoch))
    }

    /// The shine of `wait`, which sweeps from when its dots show on; none before, or without
    /// `animations.spinners`.
    pub(crate) fn shine(&self, wait: &Wait) -> Option<Shine> {
        let since = wait.since(self.at);
        let elapsed = self.at.checked_duration_since(since)?.checked_sub(DELAY)?;
        self.turns.then_some(Shine(elapsed))
    }

    fn dots_after(&self, elapsed: Duration) -> char {
        if !self.turns {
            return self.glyphs.still;
        }
        let dots = self.glyphs.dots;
        dots.get(frame_of(elapsed, DOTS_FRAME, dots.len()))
            .copied()
            .unwrap_or(self.glyphs.still)
    }
}

/// The frame of `count` that shows `elapsed` after the first, each lasting `length`.
fn frame_of(elapsed: Duration, length: Duration, count: usize) -> usize {
    let frames = elapsed.as_millis() / length.as_millis().max(1);
    usize::try_from(frames % u128::try_from(count.max(1)).unwrap_or(1)).unwrap_or(0)
}

/// Where the scanner is in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Scan {
    /// The cell of the head.
    head: usize,
    /// Whether it goes right, its trail on its left.
    forward: bool,
    /// While it rests at an end: how many frames of how many it has rested.
    rest: Option<(usize, usize)>,
    /// While it moves: how many frames of how many it has moved.
    moved: (usize, usize),
}

impl Scan {
    fn at(frame: usize) -> Self {
        let back = WIDTH + HOLD_END;
        let start = back + WIDTH - 1;
        let (head, forward, rest, moved) = if frame < WIDTH {
            (frame, true, None, (frame, WIDTH))
        } else if frame < back {
            (WIDTH - 1, true, Some((frame - WIDTH, HOLD_END)), (0, 0))
        } else if frame < start {
            let moved = frame - back;
            (WIDTH - 2 - moved, false, None, (moved, WIDTH - 1))
        } else {
            (0, false, Some((frame - start, HOLD_START)), (0, 0))
        };
        Self {
            head,
            forward,
            rest,
            moved,
        }
    }

    /// The step of the trail that `cell` shows, 0 for the head; `None` if it is not lit. While
    /// the head rests, the trail goes out from its end.
    fn trail(self, cell: usize) -> Option<usize> {
        let behind = if self.forward {
            self.head.checked_sub(cell)?
        } else {
            cell.checked_sub(self.head)?
        };
        let step = behind + self.rest.map_or(0, |(rested, _)| rested);
        (step < TRAIL.len()).then_some(step)
    }

    /// How much of their color the cells that are not lit keep, per mille: less and less as
    /// the head rests, more and more again as it moves.
    fn fade(self) -> u16 {
        let span = 1000 - LEAST;
        let part = |done: usize, of: usize| {
            let done = u16::try_from(done.min(of)).unwrap_or(u16::MAX);
            let of = u16::try_from(of.max(1)).unwrap_or(u16::MAX);
            u16::try_from(u32::from(span) * u32::from(done) / u32::from(of)).unwrap_or(span)
        };
        match self.rest {
            Some((rested, of)) => 1000 - part(rested, of),
            None => LEAST + part(self.moved.0, self.moved.1.saturating_sub(1)),
        }
    }
}

/// A lit cell at `step` of the trail: `color` faded into `surface`; where their values are not
/// known, bold at the head and dim at the end of the trail.
fn lit_style(step: usize, color: Style, surface: Style) -> Style {
    let gain = if step == 1 { GLOW } else { 1000 };
    let alpha = TRAIL.get(step).copied().unwrap_or(0);
    faded(color, surface, (gain, alpha)).unwrap_or(match step {
        0 => color.bold(),
        1 | 2 => color,
        _ => color.dim(),
    })
}

/// A cell that is not lit, which keeps `fade` of its share of `color`.
fn unlit_style(fade: u16, color: Style, surface: Style) -> Style {
    let alpha = u16::try_from(u32::from(UNLIT) * u32::from(fade) / 1000).unwrap_or(UNLIT);
    faded(color, surface, (1000, alpha)).unwrap_or(color.dim())
}

/// The color of `color` at `alpha` over the background of `surface`, if both have one.
fn faded(color: Style, surface: Style, light: (u16, u16)) -> Option<Style> {
    let mixed = theme::blend(color.fg?, surface.bg?, light)?;
    Some(Style::new().fg(mixed))
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::tui::glyphs::Glyphs;

    fn spin(at: Instant, turns: bool) -> Spin {
        Spin::new(at, at, turns, Glyphs::NERD.spinners)
    }

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn the_head_sweeps_right_rests_sweeps_back_and_rests_longer() {
        let frames: Vec<String> = (0..CYCLE)
            .map(|frame| {
                let scan = Scan::at(frame);
                (0..WIDTH)
                    .map(|cell| {
                        if scan.trail(cell).is_some() {
                            '■'
                        } else {
                            '⬝'
                        }
                    })
                    .collect()
            })
            .collect();
        assert_eq!(frames[0], "■⬝⬝⬝⬝⬝⬝⬝");
        assert_eq!(frames[3], "■■■■⬝⬝⬝⬝");
        assert_eq!(frames[7], "⬝⬝■■■■■■");
        // Resting at the right end, the trail goes out from its end, then the head.
        assert_eq!(frames[8], "⬝⬝■■■■■■");
        assert_eq!(frames[9], "⬝⬝⬝■■■■■");
        assert_eq!(frames[13], "⬝⬝⬝⬝⬝⬝⬝■");
        assert_eq!(frames[14], "⬝⬝⬝⬝⬝⬝⬝⬝");
        // Back, its trail on its right.
        assert_eq!(frames[17], "⬝⬝⬝⬝⬝⬝■■");
        assert_eq!(frames[23], "■■■■■■⬝⬝");
        assert_eq!(frames[24], "■■■■■■⬝⬝");
        assert_eq!(frames[30], "⬝⬝⬝⬝⬝⬝⬝⬝");
        assert_eq!(frames[CYCLE - 1], "⬝⬝⬝⬝⬝⬝⬝⬝");
    }

    #[test]
    fn the_cells_that_are_not_lit_fade_as_it_rests_and_come_back_as_it_moves() {
        assert_eq!(Scan::at(0).fade(), LEAST);
        assert_eq!(Scan::at(WIDTH - 1).fade(), 1000);
        assert_eq!(Scan::at(WIDTH).fade(), 1000);
        assert!(Scan::at(WIDTH + HOLD_END - 1).fade() < 400);
        assert_eq!(Scan::at(CYCLE - HOLD_START).fade(), 1000);
    }

    #[test]
    fn a_wait_shows_its_scanner_after_a_moment_from_its_first_frame() {
        let start = Instant::now();
        let wait = Wait::default();
        let colors = (
            Style::new().fg(Color::Rgb(255, 0, 0)),
            Style::new().bg(Color::Rgb(0, 0, 0)),
        );
        assert_eq!(spin(start, true).wait(&wait, colors), None);
        assert_eq!(wait.shows(), Some(start + DELAY));
        let almost = start + DELAY.saturating_sub(Duration::from_millis(1));
        assert_eq!(spin(almost, true).wait(&wait, colors), None);
        let first = spin(start + DELAY, true).wait(&wait, colors).unwrap();
        assert_eq!(text(&first), "■⬝⬝⬝⬝⬝⬝⬝ ");
        assert_eq!(first[0].style.fg, Some(Color::Rgb(255, 0, 0)));
        let later = start + DELAY + SCANNER_FRAME * 2;
        let third = spin(later, true).wait(&wait, colors).unwrap();
        assert_eq!(text(&third), "■■■⬝⬝⬝⬝⬝ ");
        // The glow behind the head, then the trail fading into the background.
        let fg = |index: usize| third[index].style.fg;
        assert_eq!(fg(2), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(fg(1), Some(Color::Rgb(230, 0, 0)));
        assert_eq!(fg(0), Some(Color::Rgb(166, 0, 0)));
        // Without animations, a still mark, which waits as long.
        let still = Wait::default();
        assert_eq!(spin(start, false).wait(&still, colors), None);
        let shown = spin(start + DELAY, false).wait(&still, colors).unwrap();
        assert_eq!(text(&shown), "⋯ ");
    }

    #[test]
    fn without_known_colors_the_head_is_bold_and_the_rest_dim() {
        let start = Instant::now();
        let wait = Wait::default();
        let colors = (
            Style::new().fg(Color::LightYellow),
            Style::new().bg(Color::Blue),
        );
        wait.since(start);
        let spans = spin(start + DELAY, true).wait(&wait, colors).unwrap();
        let at = start + DELAY + SCANNER_FRAME * 4;
        let spans_later = spin(at, true).wait(&wait, colors).unwrap();
        assert_eq!(text(&spans), "■⬝⬝⬝⬝⬝⬝⬝ ");
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert!(spans[1].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(text(&spans_later), "■■■■■⬝⬝⬝ ");
        assert!(spans_later[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(spans_later[3].style, Style::new().fg(Color::LightYellow));
    }

    #[test]
    fn the_dots_turn_from_the_first_frame() {
        let start = Instant::now();
        let at = |ms| {
            Spin::new(
                start + Duration::from_millis(ms),
                start,
                true,
                Glyphs::NERD.spinners,
            )
        };
        assert_eq!(at(0).dots(), '⠋');
        assert_eq!(at(79).dots(), '⠋');
        assert_eq!(at(80).dots(), '⠙');
        assert_eq!(at(800).dots(), '⠋');
        let plain = Spin::new(start + DOTS_FRAME, start, true, Glyphs::PLAIN.spinners);
        assert_eq!(plain.dots(), '/');
        assert_eq!(spin(start, false).dots(), '⋯');
    }

    #[test]
    fn the_dots_of_a_wait_turn_from_when_they_show() {
        let start = Instant::now();
        let wait = Wait::default();
        assert_eq!(spin(start, true).wait_dots(&wait), None);
        let almost = start + DELAY.saturating_sub(Duration::from_millis(1));
        assert_eq!(spin(almost, true).wait_dots(&wait), None);
        assert_eq!(spin(start + DELAY, true).wait_dots(&wait), Some('⠋'));
        let next = start + DELAY + DOTS_FRAME;
        assert_eq!(spin(next, true).wait_dots(&wait), Some('⠙'));
        assert_eq!(spin(next, false).wait_dots(&wait), Some('⋯'));
    }

    #[test]
    fn the_shine_crosses_from_when_the_dots_show_and_rests_between_sweeps() {
        let start = Instant::now();
        let wait = Wait::default();
        assert_eq!(spin(start, true).shine(&wait), None);
        let almost = start + DELAY.saturating_sub(Duration::from_millis(1));
        assert_eq!(spin(almost, true).shine(&wait), None);
        let shown = start + DELAY;
        let over = |at, cells| spin(at, true).shine(&wait).and_then(|s| s.over(cells));
        assert_eq!(over(shown, 20), Some(0));
        assert_eq!(
            spin(shown, true).wait_dots(&wait),
            Some('⠋'),
            "with the dots"
        );
        let later = shown + SHINE_STEP * 5;
        assert_eq!(over(later, 20), Some(5));
        assert_eq!(over(later, 50), Some(5), "as fast over a longer bar");
        assert_eq!(spin(later, false).shine(&wait), None);
        assert_eq!(over(shown + SHINE_STEP * 20, 20), None, "resting");
        let next = shown + SHINE_STEP * 20 + SHINE_REST;
        assert_eq!(over(next, 20), Some(0));
        assert_eq!(over(next, 50), Some(30), "a longer sweep rests later");
    }

    #[test]
    fn a_bouncing_shine_runs_to_and_fro_as_fast() {
        let at = |cells| Shine::after(cells).bounce(10);
        assert_eq!(
            [at(0), at(4), at(9), at(10), at(17), at(18)],
            [0, 4, 9, 8, 1, 0]
        );
        assert_eq!(Shine::after(5).bounce(1), 0);
        assert_eq!(Shine::after(5).bounce(0), 0);
    }
}
