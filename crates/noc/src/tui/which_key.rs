//! The key hints, as which-key shows them: the keys that can follow those typed so far, or
//! every key of the context, each with what it does, in columns at the bottom of the panels.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::cells::{self, Align};
use super::dialog::{Colors, draw_box};
use super::help;
use super::keymap::{Action, Context, Hint, Hints};
use super::theme::Theme;
use crate::i18n::fl;

/// Widest the key column gets; longer lists of keys are cut.
const MAX_KEYS_WIDTH: usize = 16;
/// Narrowest a column gets before the hints that do not fit are left out.
const MIN_COLUMN_WIDTH: usize = 30;
/// Cells between columns.
const GAP: usize = 2;

/// A row of the hints: the keys, what they do, and whether they start longer sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    keys: String,
    text: String,
    group: bool,
}

/// What `action` does in `context`, or in the first context of its chain that says.
fn describe(context: Context, action: Action) -> Option<String> {
    context
        .chain()
        .iter()
        .find_map(|context| help::describe(*context, action))
}

/// The rows of `hints`, without the actions the app cannot do yet.
fn rows(hints: &Hints) -> Vec<Row> {
    hints
        .rows
        .iter()
        .filter_map(|Hint { keys, action, more }| {
            let text = action.and_then(|action| describe(hints.context, action));
            let text = match (text, *more) {
                (Some(text), 0) => text,
                (None, 0) => return None,
                (Some(text), more) => format!("{text} {}", fl!("which-key-more", count = more)),
                (None, more) => fl!("which-key-more", count = more),
            };
            Some(Row {
                keys: keys.clone(),
                text,
                group: *more > 0,
            })
        })
        .collect()
}

/// How the rows go into columns: how many columns, how many rows each, and how wide each is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Grid {
    columns: usize,
    rows: usize,
    width: usize,
}

/// As few columns of `natural` width as `count` rows need within `height` lines of `width`
/// cells; more and narrower ones, down to [`MIN_COLUMN_WIDTH`], where they do not fit. The
/// columns share the width.
fn grid(count: usize, natural: usize, (width, height): (usize, usize)) -> Grid {
    let height = height.max(1);
    let fit = |column: usize| ((width + GAP) / (column + GAP)).max(1);
    let wide = fit(natural);
    let needed = count.div_ceil(height).max(1);
    let columns = if needed <= wide {
        wide.min(count.max(1))
    } else {
        needed.min(fit(MIN_COLUMN_WIDTH.min(natural))).max(wide)
    };
    Grid {
        columns,
        rows: count.div_ceil(columns).clamp(1, height),
        width: ((width + GAP) / columns).saturating_sub(GAP),
    }
}

/// Draws `hints` across the bottom of `area`.
pub(crate) fn render(frame: &mut Frame<'_>, area: Rect, hints: &Hints, theme: &Theme) {
    let rows = rows(hints);
    if rows.is_empty() || area.width < 8 || area.height < 3 {
        return;
    }
    let inner_width = usize::from(area.width.saturating_sub(4));
    let keys_width = rows
        .iter()
        .map(|row| cells::width(&row.keys))
        .max()
        .unwrap_or(0)
        .min(MAX_KEYS_WIDTH);
    let text_width = rows
        .iter()
        .map(|row| cells::width(&row.text))
        .max()
        .unwrap_or(0);
    let natural = keys_width + 1 + text_width;
    let room = usize::from(area.height.saturating_sub(2));
    let grid = grid(rows.len(), natural, (inner_width, room));
    let height = u16::try_from(grid.rows + 2).unwrap_or(area.height);
    let strip = Rect {
        y: area.bottom().saturating_sub(height),
        height,
        ..area
    };
    let title = hints
        .typed
        .clone()
        .unwrap_or_else(|| fl!("which-key-title"));
    let colors = Colors::of(theme, false);
    let inner = draw_box(frame, strip, (area.width, height), &title, colors, theme);
    render_cells(frame, inner, grid, &rows, theme);
    let close = format!(" {} ", fl!("which-key-close"));
    let width = cells::width(&close);
    if width + 4 <= usize::from(inner.width) {
        let width = u16::try_from(width).unwrap_or(0);
        let x = inner.right().saturating_sub(width + 1);
        let at = Rect::new(x, inner.bottom(), width, 1).intersection(frame.area());
        frame.render_widget(Line::styled(close, theme.dialog_title), at);
    }
}

/// Draws `rows` in the cells of `grid`, down each column, each column's keys as wide as its
/// widest. Where not every row fits, the last cell says how many do not.
fn render_cells(frame: &mut Frame<'_>, inner: Rect, grid: Grid, rows: &[Row], theme: &Theme) {
    let cells = grid.rows * grid.columns;
    let shown = if rows.len() > cells {
        cells - 1
    } else {
        rows.len()
    };
    for (column, chunk) in rows[..shown].chunks(grid.rows).enumerate() {
        let keys_width = chunk
            .iter()
            .map(|row| cells::width(&row.keys))
            .max()
            .unwrap_or(0)
            .min(MAX_KEYS_WIDTH)
            .min(grid.width / 2);
        let text_width = grid.width.saturating_sub(keys_width + 1);
        for (line, row) in chunk.iter().enumerate() {
            let style = if row.group {
                theme.dialog_title
            } else {
                theme.dialog
            };
            let cell = Line::from(vec![
                Span::styled(fit_keys(&row.keys, keys_width), theme.dialog_title),
                Span::raw(" "),
                Span::styled(cut(&row.text, text_width), style),
            ]);
            frame.render_widget(cell, cell_area(inner, grid, column, line));
        }
    }
    if shown < rows.len() {
        let hidden = rows.len() - shown;
        let more = fl!("which-key-hidden", count = hidden);
        let at = cell_area(inner, grid, shown / grid.rows, shown % grid.rows);
        frame.render_widget(Line::styled(cut(&more, grid.width), theme.dialog_title), at);
    }
}

/// `keys`, a list such as `Insert, Ctrl+t`, in `width` cells: as many of them as fit whole,
/// and the first cut where not even that one does.
fn fit_keys(keys: &str, width: usize) -> String {
    let mut fitted = String::new();
    for key in keys.split(", ") {
        let next = if fitted.is_empty() {
            key.to_owned()
        } else {
            format!("{fitted}, {key}")
        };
        if cells::width(&next) > width {
            break;
        }
        fitted = next;
    }
    if fitted.is_empty() {
        return cut(keys, width);
    }
    cells::fit(&fitted, width, Align::Left)
}

/// `text` in `width` cells, cut at the end with `…` where it is longer.
fn cut(text: &str, width: usize) -> String {
    if cells::width(text) <= width {
        return cells::fit(text, width, Align::Left);
    }
    if width == 0 {
        return String::new();
    }
    let (head, _) = cells::split(text, width - 1);
    format!("{head}…")
}

/// Where the cell of `column` and `line` is in `inner`.
fn cell_area(inner: Rect, grid: Grid, column: usize, line: usize) -> Rect {
    let x = column * (grid.width + GAP);
    let x = inner.x + u16::try_from(x).unwrap_or(u16::MAX);
    let y = inner.y + u16::try_from(line).unwrap_or(u16::MAX);
    let width = u16::try_from(grid.width).unwrap_or(0);
    Rect::new(x, y, width, 1).intersection(inner)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::keymap::{KeyState, Keymap};

    fn draw(hints: &Hints, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), hints, &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    fn root(keymap: &Keymap, context: Context) -> Hints {
        let mut state = KeyState::default();
        keymap.show_hints(&mut state, context);
        keymap.hints_of(&state, context).unwrap()
    }

    #[test]
    fn columns_share_the_width_and_narrow_where_rows_do_not_fit() {
        assert_eq!(
            grid(4, 30, (100, 10)),
            Grid {
                columns: 3,
                rows: 2,
                width: 32
            }
        );
        assert_eq!(grid(2, 30, (100, 10)).columns, 2, "no empty columns");
        assert_eq!(
            grid(40, 60, (100, 10)),
            Grid {
                columns: 3,
                rows: 10,
                width: 32
            },
            "narrower columns, down to the narrowest that fit"
        );
        assert_eq!(
            grid(1, 200, (40, 10)),
            Grid {
                columns: 1,
                rows: 1,
                width: 40
            }
        );
    }

    #[test]
    fn keys_fit_whole_and_texts_lose_their_end() {
        assert_eq!(
            fit_keys("Insert, Ctrl+t, Shift+Down", 16),
            "Insert, Ctrl+t  "
        );
        assert_eq!(fit_keys("F10", 5), "F10  ");
        assert_eq!(fit_keys("Shift+Down", 6), "Shift…");
        assert_eq!(cut("Open the directory", 10), "Open the …");
        assert_eq!(cut("Quit", 6), "Quit  ");
        assert_eq!(cut("Quit", 0), "");
    }

    #[test]
    fn rows_say_what_keys_do_and_how_many_follow() {
        let keymap = Keymap::mc();
        let rows = rows(&root(&keymap, Context::Panel));
        let row = |keys: &str| rows.iter().find(|row| row.keys == keys).cloned();
        assert_eq!(
            row("Ctrl+x"),
            Some(Row {
                keys: "Ctrl+x".to_owned(),
                text: "+10 keys".to_owned(),
                group: true
            })
        );
        assert_eq!(row("F10").map(|row| row.text), Some("Quit".to_owned()));
        assert!(row("Esc").is_none(), "Esc closes the hints");
    }

    #[test]
    fn draws_the_keys_after_ctrl_x() {
        let keymap = Keymap::mc();
        let ctrl_x = crokey::parse("ctrl-x").unwrap().normalized();
        let hints = Hints {
            context: Context::Panel,
            typed: Some("Ctrl+x".to_owned()),
            rows: keymap.hints(Context::Panel, &[ctrl_x]),
        };
        insta::assert_snapshot!(draw(&hints, 100, 12));
    }

    #[test]
    fn draws_every_key_of_the_root_and_says_how_many_do_not_fit() {
        let keymap = Keymap::mc();
        let hints = root(&keymap, Context::Root);
        insta::assert_snapshot!(draw(&hints, 100, 14));
    }
}
