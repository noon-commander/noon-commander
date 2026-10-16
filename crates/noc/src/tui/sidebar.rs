//! The list of categories down the left of a dialog, with a line beside it, as the Configuration
//! dialog and the help have it.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::cells;
use super::dialog::Colors;
use super::glyphs;
use super::label::Label;
use super::theme::Theme;

/// The rows of a sidebar and which one is chosen.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sidebar<'a> {
    /// The rows shown, each as it is drawn, with its letter, if it has one.
    pub(crate) rows: &'a [Label],
    /// Cells it takes, if no more than a third of the dialog: wide enough for its longest row,
    /// also those not shown now, so that it keeps its width.
    pub(crate) width: usize,
    pub(crate) chosen: usize,
    /// It has the keys: the chosen row is in the cursor's colors, rather than in bold.
    pub(crate) focused: bool,
}

/// Where a sidebar was drawn.
#[derive(Debug, Default)]
pub(crate) struct Drawn {
    /// The column of the line beside it.
    pub(crate) line: u16,
    /// What is right of the line, a cell away from it.
    pub(crate) content: Rect,
    /// The rows shown: each by its index, and where it is.
    pub(crate) rows: Vec<(usize, Rect)>,
}

/// Cells `rows` need: their longest.
pub(crate) fn width(rows: &[Label]) -> usize {
    rows.iter()
        .map(|row| cells::width(&row.text))
        .max()
        .unwrap_or(0)
}

/// Draws `sidebar` down the left of `body`, scrolled to its chosen row, and the line beside it.
pub(crate) fn render(
    frame: &mut Frame<'_>,
    body: Rect,
    sidebar: Sidebar<'_>,
    theme: &Theme,
    colors: Colors,
) -> Drawn {
    let width = u16::try_from(sidebar.width)
        .unwrap_or(u16::MAX)
        .min(body.width / 3);
    let height = usize::from(body.height);
    let offset = (sidebar.chosen + 1).saturating_sub(height);
    let mut drawn = Drawn::default();
    for (index, name) in sidebar.rows.iter().enumerate().skip(offset).take(height) {
        let y = body.y + u16::try_from(index - offset).unwrap_or(0);
        let on_cursor = index == sidebar.chosen && sidebar.focused;
        let styles = match (index == sidebar.chosen, sidebar.focused) {
            (true, true) => (colors.focused_style(), theme.truncated_cursor),
            (true, false) => (theme.dialog_title.bold(), theme.truncated_dialog),
            (false, _) => (theme.dialog, theme.truncated_dialog),
        };
        let hotkey = colors.hotkey_style(on_cursor);
        let text = name.fit(usize::from(width), styles, hotkey, theme.glyphs.marks);
        let row = Rect::new(body.x, y, width, 1);
        frame.render_widget(Line::from(text).style(styles.0), row);
        drawn.rows.push((index, row));
    }
    let x = body.x + width;
    for y in body.y..body.bottom() {
        let line = Line::styled(glyphs::COLUMN.to_string(), theme.dialog);
        frame.render_widget(line, Rect::new(x, y, 1, 1));
    }
    drawn.line = x;
    drawn.content = Rect::new(
        x + 2,
        body.y,
        body.right().saturating_sub(x + 2),
        body.height,
    );
    drawn
}
