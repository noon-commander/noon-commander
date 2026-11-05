//! What a window shows while its rows are still to come (ADR 0037): the dots in front of its
//! title, the skeletons of the rows, and the shine that crosses them.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::tui::cells;
use crate::tui::spinner::Shine;
use crate::tui::theme::{self, Theme};

use super::Colors;

/// Draws `dots` in `color` in front of `title` on the frame around `inner`, the room inside a box that
/// [`draw_box`] drew with that title: over the line of the frame, so that the title stays where
/// it is when they come and go. Nothing where the line has no room for them.
pub(crate) fn draw_title_dots(
    frame: &mut Frame<'_>,
    inner: Rect,
    title: &str,
    (dots, color): (char, Style),
    colors: Colors,
) {
    // Where the frame puts its title: between its corners.
    let left = inner.x.saturating_sub(1);
    let room = inner.width.saturating_add(2);
    // The title with a blank cell on either side.
    let width = u16::try_from(cells::width(title))
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let Some(free) = room.checked_sub(width) else {
        return;
    };
    // A blank cell and the dots.
    if free / 2 < 2 || inner.y == 0 {
        return;
    }
    let x = left + free / 2 - 2;
    let title = colors.title.bold();
    let line = Line::from(vec![
        Span::styled(" ", title),
        Span::styled(dots.to_string(), title.patch(color)),
    ]);
    let area = Rect::new(x, inner.y - 1, 2, 1).intersection(frame.area());
    frame.render_widget(line, area);
}

/// Rows a window shows while its first ones are still to come, the first time it opens; then as
/// many as came the last time.
pub(crate) const SKELETON_ROWS: u16 = 1;

/// Twelfths of the width: of the bar on the left of each skeleton row, of the one on the right,
/// over and over, so that rows under each other do not look alike.
const SKELETON_BARS: [(u16, u16); 4] = [(6, 3), (4, 3), (7, 2), (5, 3)];

/// Draws a skeleton on every line of `area`, where rows are still to come, in the colors of
/// `style`, the dialog's or the panel's: a faint bar on the left for a name and one on the right
/// for what is known of it. A `shine`, if any, crosses every bar at once, lighting them where it
/// passes.
pub(crate) fn draw_skeleton(
    frame: &mut Frame<'_>,
    area: Rect,
    (theme, style): (&Theme, Style),
    shine: Option<Shine<'_>>,
) {
    if area.width < 8 {
        return;
    }
    let glyph = theme.glyphs.skeleton.to_string();
    let bars = SKELETON_BARS
        .iter()
        .cycle()
        .take(usize::from(area.height))
        .map(|(left, right)| (area.width * left / 12, area.width * right / 12));
    let longest = bars
        .clone()
        .map(|(left, right)| left.max(right))
        .max()
        .unwrap_or(0);
    let middle = shine_middle(shine, longest);
    for (y, (left, right)) in (area.y..area.bottom()).zip(bars) {
        let line: Line<'_> = shine_bar(left, middle, &glyph, style)
            .into_iter()
            .chain([Span::styled(
                " ".repeat(usize::from(area.width - left - right)),
                style,
            )])
            .chain(shine_bar(right, middle, &glyph, style))
            .collect();
        frame.render_widget(line, Rect::new(area.x, y, area.width, 1));
    }
}

/// A skeleton of a line of status over `width` cells, such as how many a search found, in the
/// colors of `style`: a faint bar from the left edge, as long as the right bar of a skeleton row.
/// A `shine`, if any, crosses it with the bars of a skeleton row over as many cells.
pub(crate) fn status_skeleton(
    width: u16,
    style: Style,
    theme: &Theme,
    shine: Option<Shine<'_>>,
) -> Line<'static> {
    let length = if width < 8 {
        0
    } else {
        width * SKELETON_BARS[0].1 / 12
    };
    let middle = shine_middle(shine, width * SKELETON_BARS[0].0 / 12);
    let glyph = theme.glyphs.skeleton.to_string();
    Line::from(shine_bar(length, middle, &glyph, style))
}

/// Cells from the middle of a shine to where it fades out.
const SHINE_HALF: u16 = 5;

/// Where the middle of `shine` is, in cells from the start of bars of `longest` cells at most:
/// it comes in at one end and goes out at the other, as fast over every bar. None while it
/// rests.
fn shine_middle(shine: Option<Shine<'_>>, longest: u16) -> Option<i32> {
    let cells = shine?.over(longest.saturating_add(2 * SHINE_HALF))?;
    Some(i32::from(cells) - i32::from(SHINE_HALF))
}

/// A faint bar of `length` cells of `glyph` in the color of `style` blended into its
/// background, which a shine whose middle is at `middle` (from [`shine_middle`]), if any,
/// lights where it passes. Where the colors are not known, the bar is dim and the shine plain.
pub(crate) fn shine_bar(
    length: u16,
    middle: Option<i32>,
    glyph: &str,
    style: Style,
) -> Vec<Span<'static>> {
    // How much of the color shows: in the bar, in the middle of the shine; per mille.
    const FAINT: u16 = 250;
    const BRIGHT: u16 = 650;
    let shade = |alpha: u16| match (style.fg, style.bg) {
        (Some(fg), Some(bg)) => theme::blend(fg, bg, (1000, alpha)).map(|fg| style.fg(fg)),
        _ => None,
    };
    let faint = shade(FAINT).unwrap_or_else(|| style.add_modifier(Modifier::DIM));
    (0..length)
        .map(|x| {
            let near = middle
                .map(|middle| (i32::from(x) - middle).unsigned_abs())
                .filter(|distance| *distance < u32::from(SHINE_HALF));
            let style = near.map_or(faint, |distance| {
                let distance = u16::try_from(distance).unwrap_or(SHINE_HALF);
                shade(BRIGHT - (BRIGHT - FAINT) * distance / SHINE_HALF).unwrap_or(style)
            });
            Span::styled(glyph.to_owned(), style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    use super::*;

    fn skeleton_colors(shine: Option<Shine<'_>>) -> Vec<Option<Color>> {
        let mut theme = Theme::terminal();
        theme.dialog = Style::new()
            .fg(Color::Rgb(255, 255, 255))
            .bg(Color::Rgb(0, 0, 0));
        let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
        terminal
            .draw(|frame| draw_skeleton(frame, frame.area(), (&theme, theme.dialog), shine))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..30).map(|x| buffer[(x, 0)].style().fg).collect()
    }

    #[test]
    fn a_skeleton_lights_its_bars_where_the_shine_passes() {
        let faint = Some(Color::Rgb(64, 64, 64));
        let still = skeleton_colors(None);
        // Bars of 15 and 7 cells, the gap between them in the color of the dialog.
        assert!(
            still[..15]
                .iter()
                .chain(&still[23..])
                .all(|fg| *fg == faint)
        );
        assert!(
            still[15..23]
                .iter()
                .all(|fg| *fg == Some(Color::Rgb(255, 255, 255)))
        );
        assert_eq!(
            skeleton_colors(Some(Shine::after(0))),
            still,
            "the shine comes in from the left"
        );
        // Twelve cells on, coming in five cells before the bars, its middle is on cell 7 of both:
        // they shine at once and as fast, the shorter bar on the right at its end already.
        let shining = skeleton_colors(Some(Shine::after(12)));
        let next = Some(Color::Rgb(145, 145, 145));
        assert_eq!(shining[7], Some(Color::Rgb(166, 166, 166)));
        assert_eq!(shining[8], next);
        assert_eq!(shining[..3], still[..3]);
        assert_eq!(shining[23 + 6], next);
        assert_eq!(shining[23 + 2], faint);
    }

    #[test]
    fn a_skeleton_fills_its_area_with_rows_of_bars_of_their_own_lengths() {
        let theme = Theme::terminal();
        let mut terminal = Terminal::new(TestBackend::new(24, 5)).unwrap();
        terminal
            .draw(|frame| draw_skeleton(frame, frame.area(), (&theme, theme.dialog), None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = (0..5)
            .map(|y| (0..24).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        assert_eq!(
            lines,
            [
                "------------      ------",
                "--------          ------",
                "--------------      ----",
                "----------        ------",
                "------------      ------",
            ]
        );
    }
}
