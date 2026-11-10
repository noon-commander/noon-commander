//! How far something is: a bar filled for it with its percentage, or, while that is not known
//! yet, a faint bar that a shine crosses.

use ratatui::text::{Line, Span};

use crate::i18n::fl;
use crate::tui::cells::{self, Align, Cut};
use crate::tui::glyphs;
use crate::tui::spinner::Shine;
use crate::tui::theme::Theme;

use super::shine_bar;

/// `done` of `total`, from 0 to 1.
pub(crate) fn ratio(done: u64, total: u64) -> f64 {
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
pub(crate) fn gauge(ratio: f64, width: usize, theme: &Theme) -> Line<'static> {
    // `ratio` is within 0 … 1, and bars are a few dozen cells.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let (percent, filled) = {
        let percent = (ratio * 100.0).round() as u64;
        // As wide as the widest, so that the bar keeps its length.
        let widest = cells::width(&fl!("job-percent", percent = 100));
        let percent = cells::fit_cut(
            &fl!("job-percent", percent = percent),
            widest,
            Align::Right,
            Cut::End,
        );
        let percent = format!(" {}", percent.head);
        let bar = width.saturating_sub(cells::width(&percent));
        (percent, ((bar as f64 * ratio).round() as usize).min(bar))
    };
    let bar = width.saturating_sub(cells::width(&percent));
    let done = glyphs::GAUGE_DONE.repeat(filled);
    let text = format!("{done}{}", glyphs::GAUGE_LEFT.repeat(bar - filled));
    Line::from(vec![Span::styled(text, theme.gauge), Span::raw(percent)])
}

/// The gauge of what does not know yet how far it is: a faint bar of `width` cells, with no
/// percentage, over which a `shine`, if any, the one of the skeleton of rows, runs to and fro.
pub(crate) fn unknown_gauge(width: u16, shine: Option<Shine<'_>>, theme: &Theme) -> Line<'static> {
    let middle = shine.map(|shine| i32::from(shine.bounce(width)));
    Line::from(shine_bar(
        width,
        middle,
        glyphs::GAUGE_UNKNOWN,
        theme.dialog,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gauge_fills_by_ratio() {
        let theme = Theme::terminal();
        let text = |ratio| gauge(ratio, 15, &theme).to_string();
        assert_eq!(text(0.0), "░░░░░░░░░░   0%");
        assert_eq!(text(0.5), "█████░░░░░  50%");
        assert_eq!(text(1.0), "██████████ 100%");
    }

    #[test]
    fn a_ratio_stays_within_0_and_1() {
        assert!(ratio(1, 0) < f64::EPSILON);
        assert!((ratio(3, 2) - 1.0).abs() < f64::EPSILON);
        assert!((ratio(1, 4) - 0.25).abs() < f64::EPSILON);
    }
}
