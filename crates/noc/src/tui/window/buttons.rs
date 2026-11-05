//! The buttons of a window, on a line of their own under the rest.

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::tui::cells;
use crate::tui::label::Label;

use super::Colors;

/// The brackets around a button: `{ }` for the default one, `[ ]` for the others.
fn brackets(default: bool) -> (&'static str, &'static str) {
    if default { ("{ ", " }") } else { ("[ ", " ]") }
}

/// Buttons, centered: the default one bold in `{ }`, the one with the focus in its color, each
/// with its letter set apart.
pub(crate) fn button_line(
    labels: &[Label],
    default: usize,
    focus: Option<usize>,
    colors: Colors,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        let focused = focus == Some(index);
        let mut style = if focused {
            colors.focused
        } else {
            colors.button
        };
        if index == default {
            style = style.add_modifier(Modifier::BOLD);
        }
        let (open, close) = brackets(index == default);
        spans.push(Span::styled(open, style));
        let hotkey = colors.hotkey_style(focused);
        spans.extend(label.spans(style, Some(hotkey), colors.mark));
        spans.push(Span::styled(close, style));
    }
    Line::from(spans).centered()
}

/// Where the buttons of `labels`, as [`button_line`] draws them with the one at `default` the
/// default, are when it is drawn on `area`, which centers it.
pub(crate) fn button_spots(labels: &[Label], default: usize, area: Rect) -> Vec<Rect> {
    let widths: Vec<i32> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let (open, close) = brackets(index == default);
            let cells = cells::width(open) + cells::width(&label.text) + cells::width(close);
            i32::try_from(cells).unwrap_or(i32::MAX)
        })
        .collect();
    let gaps = i32::try_from(labels.len().saturating_sub(1)).unwrap_or(i32::MAX);
    let total = widths.iter().sum::<i32>().saturating_add(gaps);
    // Ratatui centers a line that does not fit by cutting as much off either side.
    let mut x = i32::from(area.x) + (i32::from(area.width) - total).div_euclid(2);
    if total > i32::from(area.width) {
        x = i32::from(area.x) - (total - i32::from(area.width)) / 2;
    }
    let mut spots = Vec::new();
    for width in widths {
        let start = x;
        // A blank between buttons.
        x += width + 1;
        let left = start.max(i32::from(area.x));
        let right = (start + width).min(i32::from(area.right()));
        let (Ok(left), Ok(right)) = (u16::try_from(left), u16::try_from(right)) else {
            continue;
        };
        spots.push(Rect::new(left, area.y, right.saturating_sub(left), 1));
    }
    spots
}
