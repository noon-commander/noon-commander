//! Text fields: their text, the editor behind them, and drawing them.

use std::cell::Cell;
use std::fmt;
use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar as _;

use crate::tui::cells;
use crate::tui::edit::{Editor, Registers};
use crate::tui::glyphs;
use crate::tui::keymap::{Action, Extra, Mode};
use crate::tui::label::Label;
use crate::tui::search::{GlyphSpots, Input};
use crate::tui::theme::Theme;

use super::Colors;

/// Bytes a secret may have. Reserved up front, so typing never moves it in memory and leaves
/// no copy behind.
pub(super) const SECRET_CAPACITY: usize = 1024;

/// A text field, in vim's modes: a plain one opens in Insert mode, with the cursor at the end
/// of its text. A secret one stays in Insert mode, is masked, and lives in memory reserved up
/// front and wiped when the field goes.
pub(crate) struct Field {
    pub(super) editor: Editor,
    /// The first character the last render showed: the text stays where it is while the
    /// cursor shows, as a vim window's `leftcol`.
    first: Cell<usize>,
}

impl Field {
    pub(super) fn secret() -> Self {
        Self {
            editor: Editor::secret(SECRET_CAPACITY),
            first: Cell::new(0),
        }
    }

    /// A plain field that opens in Insert mode with `text`, the cursor at its end.
    pub(crate) fn plain(text: &str) -> Self {
        Self {
            editor: Editor::new(text, Mode::Insert),
            first: Cell::new(0),
        }
    }

    /// A plain field that opens in Insert mode with `text`, the cursor before character
    /// `cursor`.
    pub(crate) fn at(text: &str, cursor: usize) -> Self {
        let mut field = Self::plain(text);
        field.editor.set_cursor(cursor);
        field
    }

    /// A plain field that opens in Normal mode with `text`.
    pub(crate) fn normal(text: &str) -> Self {
        Self {
            editor: Editor::new(text, Mode::Normal),
            first: Cell::new(0),
        }
    }

    /// The text, as typed.
    pub(crate) fn text(&self) -> &str {
        self.editor.text()
    }

    /// The mode the field is in.
    pub(crate) fn mode(&self) -> Mode {
        self.editor.mode()
    }

    /// Insert mode, or Normal mode, as the focus comes to the field; a secret stays in Insert
    /// mode.
    pub(crate) fn set_mode(&mut self, mode: Mode) {
        self.editor.set_mode(mode);
    }

    /// The operator that waits for its motion, as vim's `showcmd` shows it.
    pub(crate) fn pending(&self) -> Option<String> {
        self.editor.pending()
    }

    /// Whether Normal mode runs one command, after Ctrl+o, then goes back to Insert mode.
    pub(crate) fn once(&self) -> bool {
        self.editor.once()
    }

    /// The editor of the text, for what the line of the mode shows.
    pub(crate) fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Types `c` at the cursor.
    pub(crate) fn insert(&mut self, c: char) {
        self.editor.insert(c);
    }

    /// Puts pasted `text` as one change, in any mode, without its control characters; in
    /// Visual mode, what it replaces goes to the unnamed register of `registers`.
    pub(crate) fn paste(&mut self, text: &str, registers: &mut Registers) {
        let typed = text.chars().filter(|c| !c.is_control());
        // A password goes in character by character, so that no copy of it is left behind.
        if self.editor.is_secret() {
            typed.for_each(|c| self.editor.insert(c));
            return;
        }
        self.editor.paste(&typed.collect::<String>(), registers);
    }

    /// Gives up the command of Normal mode that waits, as the keymap gives up its keys.
    pub(crate) fn give_up(&mut self) {
        self.editor.give_up();
    }

    /// Whether the field is masked, as for a password.
    pub(crate) fn is_secret(&self) -> bool {
        self.editor.is_secret()
    }

    /// Edits the text as Insert mode does, without leaving it; `false` if `action` is not an
    /// edit. For filters, which stay in Insert mode.
    pub(crate) fn edit(&mut self, action: Action) -> bool {
        self.editor.edit(action)
    }

    /// Takes `action`, typed with `extra`, in the field's mode; `false` if it is not the
    /// field's, such as Enter, or Esc in Normal mode.
    pub(crate) fn handle(
        &mut self,
        action: Action,
        extra: Extra,
        registers: &mut Registers,
    ) -> bool {
        self.editor.handle(action, extra, registers)
    }

    /// Puts the cursor at `column` of the `room` cells the field was drawn in, as a click does,
    /// in Insert mode.
    pub(crate) fn click(&mut self, column: usize, room: usize) {
        let (first, shown, _) = self.window(room);
        let mut used = 0;
        let before = shown
            .chars()
            .take_while(|&c| {
                used += c.width().unwrap_or(0);
                used <= column
            })
            .count();
        self.editor.set_mode(Mode::Insert);
        self.editor.set_cursor(first + before);
    }

    /// What fits in `room` cells, as shown (stars for a secret), from where the cursor stays
    /// on screen, and the cursor's column in it.
    pub(crate) fn visible(&self, room: usize) -> (String, usize) {
        let (_, shown, column) = self.window(room);
        (shown, column)
    }

    /// What [`visible`](Self::visible) shows, cut in three: before what Visual mode selects,
    /// the selection, and after it; and the cursor's column.
    pub(crate) fn visible_parts(&self, room: usize) -> ([String; 3], usize) {
        let (first, shown, column) = self.window(room);
        let chars: Vec<char> = shown.chars().collect();
        let (start, end) = self.editor.selection().map_or((0, 0), |selected| {
            let clamp = |index: usize| index.saturating_sub(first).min(chars.len());
            (clamp(selected.start), clamp(selected.end))
        });
        let part = |range: Range<usize>| chars[range].iter().collect::<String>();
        let parts = [part(0..start), part(start..end), part(end..chars.len())];
        (parts, column)
    }

    /// The first character shown, what fits from there, and the cursor's column. The text
    /// shown stays while the cursor shows; once it would not, the cursor goes to the middle,
    /// as vim's 'sidescroll' 0 has it.
    fn window(&self, room: usize) -> (usize, String, usize) {
        let chars: Vec<char> = if self.editor.is_secret() {
            vec![glyphs::SECRET; self.editor.text().chars().count()]
        } else {
            self.editor.text().chars().collect()
        };
        let cursor = self.editor.cursor().min(chars.len());
        let width = |c: char| c.width().unwrap_or(0);
        let cells = |range: Range<usize>| chars[range].iter().map(|&c| width(c)).sum::<usize>();
        let first = self.first.get().min(chars.len());
        let (left, start) = (cells(0..first), cells(0..cursor));
        let end = start + chars.get(cursor).map_or(1, |&c| width(c).max(1));
        let first = if start < left || end > left + room {
            let target = start.saturating_sub(room / 2);
            (0..=cursor)
                .find(|&index| cells(0..index) >= target)
                .unwrap_or(cursor)
        } else {
            first
        };
        self.first.set(first);
        let column = cells(first..cursor);
        let mut used = 0;
        let shown: String = chars[first..]
            .iter()
            .take_while(|&&c| {
                used += width(c);
                used <= room
            })
            .collect();
        // Character for character: `shown` is UTF-8, and only control characters change.
        (first, cells::sanitize(shown.as_bytes()), column)
    }
}

impl fmt::Debug for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.editor.fmt(f)
    }
}

/// A text field, with its label drawn on a line of its own above it unless empty.
#[derive(Debug)]
pub(super) struct LabelledField {
    pub(super) label: Label,
    pub(super) field: Field,
    /// How it matches, if it searches: the glyphs of its case and its way end it, and the keys
    /// after Ctrl+s or a click on a glyph switch them while it has the focus (ADR 0045).
    pub(super) search: Option<Input>,
    /// What shows, faint, while the field is empty, such as the value that then counts; it is
    /// never the field's text. Terminal-safe. Fields that search show none.
    pub(super) placeholder: Option<String>,
    /// Shown where it cannot be used: its label faint, the field faint and empty but for its
    /// placeholder; the focus, the mouse, and its letter pass it by.
    pub(super) disabled: bool,
}

impl LabelledField {
    pub(super) fn unlabelled(field: Field) -> Self {
        Self {
            label: Label::default(),
            field,
            search: None,
            placeholder: None,
            disabled: false,
        }
    }
}

/// Draws the field of `labelled` on the line `area`, with the glyphs of how it matches at its
/// end if it searches. Returns the cursor's column in `area`, where the field takes a click, and
/// where the glyphs are.
pub(super) fn draw_field(
    frame: &mut Frame<'_>,
    area: Rect,
    labelled: &LabelledField,
    (colors, theme): (Colors, &Theme),
) -> (usize, Rect, Option<GlyphSpots>) {
    let field = &labelled.field;
    let Some(input) = labelled.search else {
        let room = usize::from(area.width);
        let (line, column) = match &labelled.placeholder {
            Some(placeholder) if field.text().is_empty() => {
                (placeholder_line(placeholder, room, colors.input, theme), 0)
            }
            _ => field_line(field, room, colors.input, theme),
        };
        frame.render_widget(line, area);
        return (column, area, None);
    };
    let invalid = input.invalid(field.text());
    let mark = input.mark(invalid, colors.input, theme);
    let (column, drawn) = mark.render_in_field(frame, area, field, theme);
    let right = drawn.map_or(area.right(), GlyphSpots::left);
    let text = Rect {
        width: right.saturating_sub(area.x),
        ..area
    };
    (usize::from(column), text, drawn)
}

/// What of `field` fits in `room` cells, in `style`, the color of fields, what Visual mode
/// selects reversed; and the cursor's column.
pub(crate) fn field_line(
    field: &Field,
    room: usize,
    style: Style,
    theme: &Theme,
) -> (Line<'static>, usize) {
    let ([before, selected, after], column) = field.visible_parts(room);
    let used = cells::width(&before) + cells::width(&selected);
    let after = cells::fit(
        &after,
        room.saturating_sub(used),
        cells::Align::Left,
        (Style::new(), theme.truncated_cursor),
        theme.glyphs.marks,
    );
    let mut line = Line::from(vec![
        Span::raw(before),
        Span::styled(selected, style.add_modifier(Modifier::REVERSED)),
    ]);
    line.spans.extend(after);
    (line.style(style), column)
}

/// `placeholder` in an empty field of `room` cells in `style`, faint, as the hints of settings
/// are.
pub(super) fn placeholder_line(
    placeholder: &str,
    room: usize,
    style: Style,
    theme: &Theme,
) -> Line<'static> {
    let faint = style.patch(theme.disabled);
    let spans = cells::fit(
        placeholder,
        room,
        cells::Align::Left,
        (faint, theme.truncated_cursor),
        theme.glyphs.marks,
    );
    Line::from(spans).style(style)
}
