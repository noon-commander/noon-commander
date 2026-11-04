//! Drawing that windows share: frames, buttons, pop-ups, skeletons, and the shine.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use crate::tui::cells;
use crate::tui::glyphs;
use crate::tui::label::Label;
use crate::tui::spinner::Shine;
use crate::tui::theme::{self, Theme};

use super::{Colors, MARGIN, MIN_WIDTH, SIDES};

/// Text without blanks, such as a checksum, in lines of at most `width` characters, of equal
/// length where it breaks.
pub(crate) fn even_lines(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let parts = chars.len().div_ceil(width.max(1)).max(1);
    let per = chars.len().div_ceil(parts).max(1);
    chars
        .chunks(per)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// A label in `room` cells of a dialog's body, its letter set apart if `lettered`; one too long
/// loses its middle, and its letter with it.
pub(super) fn label_spans(
    label: &Label,
    room: usize,
    (colors, lettered): (Colors, bool),
    theme: &Theme,
) -> Vec<Span<'static>> {
    if cells::width(&label.text) <= room {
        let hotkey = lettered.then(|| colors.hotkey_style(false));
        return label.spans(colors.body, hotkey, colors.mark);
    }
    let styles = colors.body_styles();
    cells::fit(
        &label.text,
        room,
        cells::Align::Left,
        styles,
        theme.glyphs.marks,
    )
}

/// The brackets around a button: `{ }` for the default one, `[ ]` for the others.
pub(super) fn brackets(default: bool) -> (&'static str, &'static str) {
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

/// Draws a line across a dialog whose room inside is `inner`, on its row `y`, joined to the
/// frame as mc joins them: `╟───╢`, or `├───┤` with single lines. It sets the buttons apart
/// from what is above them.
pub(crate) fn draw_separator(
    frame: &mut Frame<'_>,
    inner: Rect,
    y: u16,
    colors: Colors,
    theme: &Theme,
) {
    let (left, right) = theme.tees();
    // The frame is a cell of padding and a cell of border away from the room inside.
    let x = inner.x.saturating_sub(2);
    let width = inner.width.saturating_add(4);
    let line = format!(
        "{left}{}{right}",
        glyphs::LINE.repeat(usize::from(width.saturating_sub(2)))
    );
    let row = Rect::new(x, y, width, 1).intersection(frame.area());
    frame.render_widget(Line::styled(line, colors.body), row);
}

/// The cells of the right side of the frame around `inner`, the room inside a box that
/// [`draw_box`] drew, from the row `y` down `height` rows: where its scroll bar goes.
pub(crate) fn right_frame(inner: Rect, y: u16, height: u16) -> Rect {
    Rect::new(inner.right().saturating_add(1), y, 1, height)
}

/// The frame around `inner`, the room inside a box that [`draw_box`] drew.
pub(crate) fn frame_around(inner: Rect) -> Rect {
    Rect::new(
        inner.x.saturating_sub(2),
        inner.y.saturating_sub(1),
        inner.width.saturating_add(4),
        inner.height.saturating_add(2),
    )
}

/// Where a list that pops up from `anchor` goes in `area`, `size` with its frame, from the
/// column `x`, as vim's pop-up menu: under `anchor`, or above it where there is more room there,
/// as tall as the room lets it be.
pub(crate) fn pop_up(anchor: Rect, area: Rect, x: u16, (width, height): (u16, u16)) -> Rect {
    let below = area.bottom().saturating_sub(anchor.bottom());
    let above = anchor.y.saturating_sub(area.y);
    let (y, height) = if below >= height || below >= above {
        (anchor.bottom(), height.min(below))
    } else {
        let height = height.min(above);
        (anchor.y - height, height)
    };
    let width = width.min(area.right().saturating_sub(x));
    Rect::new(x, y, width, height)
}

/// The width of the frame of a window that would be `width` cells wide, borders included, in
/// `area`: as wide as that, but leaving [`SIDES`] cells free on either side; where that leaves
/// too few, [`MIN_WIDTH`], or the whole of `area`. Every window that [`draw_box`] draws has it.
pub(crate) fn box_width(width: u16, area: Rect) -> u16 {
    let room = area.width.saturating_sub(2 * SIDES);
    width.min(room.max(area.width.min(MIN_WIDTH)))
}

/// The cells of a line inside the frame of a window that would be `width` cells wide in `area`.
pub(crate) fn inner_width(width: u16, area: Rect) -> usize {
    usize::from(box_width(width, area).saturating_sub(4))
}

/// Draws an empty dialog box centered in `area`: a frame of `size` in the theme's lines, as wide
/// as [`box_width`] lets it be, with its title bold in the middle, a blank cell around it where
/// there is room, and mc's shadow. Returns the room inside, one column in from the frame on
/// either side.
pub(crate) fn draw_box(
    frame: &mut Frame<'_>,
    area: Rect,
    (width, height): (u16, u16),
    title: &str,
    colors: Colors,
    theme: &Theme,
) -> Rect {
    let size = (box_width(width, area), height);
    draw_frame(frame, area, size, title, colors, theme)
}

/// Draws an empty box as [`draw_box`] does, but of `size` as it is, where the screen has room:
/// for a strip that spans the screen, such as the key hints at its bottom.
pub(crate) fn draw_frame(
    frame: &mut Frame<'_>,
    area: Rect,
    (width, height): (u16, u16),
    title: &str,
    colors: Colors,
    theme: &Theme,
) -> Rect {
    // On a small screen the frame keeps its room and the margin gives way.
    let fit = |size: u16, room: u16| {
        let size = size.min(room);
        let margin = ((room - size) / 2).min(MARGIN);
        (size + 2 * margin, margin)
    };
    let (width, margin_x) = fit(width, area.width);
    let (height, margin_y) = fit(height, area.height);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    let outer = Rect::new(x, y, width, height);
    frame.render_widget(Clear, outer);
    frame.render_widget(Block::new().style(colors.body), outer);
    if let Some(shadow) = theme.shadow {
        // Two columns to the right and a row below, as mc draws it.
        let right = Rect::new(outer.right(), outer.y + 1, 2, outer.height);
        let below = Rect::new(outer.x + 2, outer.bottom(), outer.width, 1);
        for rect in [right, below] {
            frame
                .buffer_mut()
                .set_style(rect.intersection(area), shadow);
        }
    }
    let title = Line::styled(format!(" {title} "), colors.title.bold()).centered();
    let block = Block::bordered()
        .border_type(theme.border_type())
        .title(title)
        .style(colors.body);
    let framed = outer.inner(Margin::new(margin_x, margin_y));
    let inner = block.inner(framed).inner(Margin::new(1, 0));
    frame.render_widget(block, framed);
    inner
}

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
pub(super) const SKELETON_BARS: [(u16, u16); 4] = [(6, 3), (4, 3), (7, 2), (5, 3)];

/// Draws a skeleton on every line of `area`, where rows are still to come: a faint bar on the
/// left for a name and one on the right for what is known of it. A `shine`, if any, crosses
/// every bar at once, lighting them where it passes.
pub(crate) fn draw_skeleton(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    shine: Option<Shine>,
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
        let line: Line<'_> = shine_bar(left, middle, &glyph, theme.dialog)
            .into_iter()
            .chain([Span::styled(
                " ".repeat(usize::from(area.width - left - right)),
                theme.dialog,
            )])
            .chain(shine_bar(right, middle, &glyph, theme.dialog))
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
    shine: Option<Shine>,
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
pub(super) const SHINE_HALF: u16 = 5;

/// Where the middle of `shine` is, in cells from the start of bars of `longest` cells at most:
/// it comes in at one end and goes out at the other, as fast over every bar. None while it
/// rests.
pub(crate) fn shine_middle(shine: Option<Shine>, longest: u16) -> Option<i32> {
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
