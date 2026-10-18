//! The spinner in front of what Noon Commander waits for: dots that turn in one cell, and the
//! shine that crosses the skeletons standing where what it waits for is still to come. A wait
//! shows nothing for its first moment, so that one that ends at once, such as a small local
//! directory, does not flicker. Frames go by the time, not by how often the screen is drawn.

use std::cell::Cell;
use std::time::{Duration, Instant};

use super::glyphs::Spinners;

/// How long a wait goes before its spinner shows.
pub(crate) const DELAY: Duration = Duration::from_millis(150);
/// How long each frame of the dots shows.
pub(crate) const DOTS_FRAME: Duration = Duration::from_millis(80);
/// How long the shine takes to move one cell, so that it goes as fast over every bar, and how
/// long it rests between sweeps.
pub(crate) const SHINE_STEP: Duration = Duration::from_millis(40);
const SHINE_REST: Duration = Duration::from_millis(400);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::glyphs::Glyphs;

    fn spin(at: Instant, turns: bool) -> Spin {
        Spin::new(at, at, turns, Glyphs::NERD.spinners)
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
