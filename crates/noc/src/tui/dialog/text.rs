//! Text in a dialog's body: labels in the room they have, and text without blanks in lines of
//! equal length.

use ratatui::text::Span;

use crate::tui::cells;
use crate::tui::label::Label;
use crate::tui::theme::Theme;
use crate::tui::window::Colors;

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
