//! Drawing a dialog and its parts.

use noc_scroll::scrollbar;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::tui::cells;
use crate::tui::glyphs;
use crate::tui::label::{self, Label};
use crate::tui::picker::{Body, ShownRow};
use crate::tui::theme::Theme;

use super::{
    Colors, Dialog, Focus, Part, PickerDraw, Text, button_line, button_spots, draw_box, draw_field,
    draw_separator, draw_title_dots, even_lines, label_spans, placeholder_line, right_frame,
};

impl Dialog {
    /// Draws the lines of the message at the top of `inner`, as many as the `rest` of the
    /// dialog leaves room for, scrolled, with a scroll bar on the right of the frame in `style`
    /// when they do not all fit. Returns how many rows they took.
    fn render_message(
        &self,
        frame: &mut Frame<'_>,
        inner: Rect,
        (lines, letter): (&[String], Option<(usize, usize)>),
        rest: u16,
        colors: Colors,
        theme: &Theme,
    ) -> u16 {
        let style = colors.body;
        let room = usize::from(inner.height.saturating_sub(rest)).max(1);
        let page = lines.len().min(room);
        let offset = self.offset.get().min(lines.len() - page);
        self.offset.set(offset);
        self.page.set(page);
        let rows = u16::try_from(page).unwrap_or(u16::MAX).min(inner.height);
        let bar = right_frame(inner, inner.y, rows);
        let position = (lines.len(), page, offset);
        let arrows = theme.glyphs.scroll_bar;
        self.bar
            .set(scrollbar::render(frame, bar, position, style, arrows));
        for (index, line) in (0..rows).zip(lines.iter().enumerate().skip(offset)) {
            let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
            let (number, text) = line;
            let over = letter
                .filter(|&(at, _)| at == number)
                .map(|(_, column)| (column, colors.hotkey_style(false)));
            let spans = label::spans_over(text, style, over.into_iter().collect());
            frame.render_widget(Line::from(spans), row);
        }
        rows
    }

    /// Draws the dialog centered in `area`, with the terminal cursor in the focused field.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        self.render_with(frame, area, theme, None);
    }

    /// Draws the dialog as [`render`](Self::render) does, but under another, which has the
    /// keys: without the terminal cursor and the open list of a combo box.
    pub(crate) fn render_under(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        self.draw_parts(frame, area, theme, None);
    }

    /// Draws the dialog as [`render`](Self::render) does, with the lines of its picker and
    /// what goes under them as `body` says, but its title, its width, and its buttons, which
    /// are the dialog's; `row` draws a row.
    pub(crate) fn render_picker(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        body: Body<'_>,
        row: &mut dyn FnMut(ShownRow) -> Line<'static>,
    ) {
        let Some(mut picker) = self.picker.take() else {
            self.render(frame, area, theme);
            return;
        };
        let draw = PickerDraw {
            picker: &mut picker,
            body: Some(body),
            row,
        };
        self.render_with(frame, area, theme, Some(draw));
        self.picker = Some(picker);
    }

    /// Draws the dialog, then over it the open list of a combo box, which may go past its
    /// frame, within `area`.
    fn render_with(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        picker: Option<PickerDraw<'_, '_>>,
    ) {
        self.draw_parts(frame, area, theme, picker);
        if let Some(cursor) = self.cursor.get() {
            frame.set_cursor_position(cursor);
        }
        if let Some(combo) = self.open_combo_box() {
            combo.render_list(frame, area, Colors::of(theme, self.tone), theme);
        }
    }

    fn draw_parts(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
        mut picker: Option<PickerDraw<'_, '_>>,
    ) {
        let width = self.box_width(area);
        let text_width = usize::from(width.saturating_sub(4));
        let (lines, letter) = if self.fields.is_empty() {
            (cells::wrap(&self.message.text, text_width), None)
        } else {
            self.message.wrap(text_width)
        };
        let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
        let body = picker
            .as_ref()
            .and_then(|draw| Some((&*draw.picker, draw.body.as_ref()?)));
        let picker_rows = body.map_or(0, |(picker, body)| picker.height(body));
        let fill = body.is_some_and(|(_, body)| body.fill);
        let dots = body.and_then(|(_, body)| body.dots());
        // Borders, the message, the parts under it, a line, the buttons.
        let height = self.parts.iter().fold(rows(lines.len()), |height, &part| {
            height.saturating_add(self.part_rows(part, text_width, picker_rows))
        });
        let colors = Colors::of(theme, self.tone);
        // Rows that fill the room make the dialog as tall as the screen.
        let size = if fill {
            (width, u16::MAX)
        } else {
            (width, height + 4)
        };
        let inner = draw_box(frame, area, size, &self.title, colors, theme);
        if let Some(dots) = dots {
            draw_title_dots(frame, inner, &self.title, (dots, theme.busy_dialog), colors);
        }
        let row = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let mut spots = self.spots.borrow_mut();
        spots.clear();
        self.glyphs.borrow_mut().clear();
        self.cursor.set(None);
        self.picker_area.set(None);
        // The line and the buttons below the rest.
        let rest = height.saturating_sub(rows(lines.len())).saturating_add(2);
        let message = (lines.as_slice(), letter);
        let mut index = self.render_message(frame, inner, message, rest, colors, theme);
        let style = (colors, theme);
        let Some(after) = self.draw_body(frame, (inner, index), style, &mut spots, &mut picker)
        else {
            return;
        };
        index = after;
        if index + 1 < inner.height {
            draw_separator(frame, inner, inner.y + index, colors, theme);
            let labels: Vec<Label> = self.buttons.iter().map(|button| button.label()).collect();
            let focus = match self.focus {
                Focus::Button(index) => Some(index),
                Focus::ComboBox(_)
                | Focus::Field(_)
                | Focus::Check(_)
                | Focus::Item(_)
                | Focus::Picker => None,
            };
            let line = button_line(&labels, self.default, focus, colors);
            let buttons = button_spots(&labels, self.default, row(index + 1));
            spots.extend((0..).map(Focus::Button).zip(buttons));
            frame.render_widget(line, row(index + 1));
        }
    }
}

impl Dialog {
    /// The rows `part` takes, in lines `width` cells across: a field's label takes one of its
    /// own, and the picker `picker` rows.
    fn part_rows(&self, part: Part, width: usize, picker: u16) -> u16 {
        match part {
            Part::Field(index) if !self.fields[index].label.text.is_empty() => 2,
            Part::Text(index) => match &self.texts[index] {
                Text::Even(text) => {
                    u16::try_from(even_lines(text, width).len()).unwrap_or(u16::MAX)
                }
                Text::Wrap(text) => {
                    u16::try_from(cells::wrap(text, width).len()).unwrap_or(u16::MAX)
                }
                Text::Line(_) | Text::Pair { .. } | Text::Verdict { .. } => 1,
            },
            Part::Picker => picker,
            Part::ComboBox(_)
            | Part::Field(_)
            | Part::Check(_)
            | Part::Item(_)
            | Part::Separator
            | Part::Line => 1,
        }
    }

    /// The lines of text `index`, `width` cells across.
    fn text_lines(
        &self,
        index: usize,
        width: usize,
        colors: Colors,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        match &self.texts[index] {
            Text::Line(text) => {
                let marks = theme.glyphs.marks;
                let cut = cells::fit(text, width, cells::Align::Left, colors.body_styles(), marks);
                vec![Line::from(cut)]
            }
            Text::Pair { label, value } => {
                let value = Span::styled(format!(" {value}"), colors.body.bold());
                let line = Line::from(vec![Span::styled(label.clone(), colors.body), value]);
                vec![line]
            }
            Text::Wrap(text) => cells::wrap(text, width)
                .into_iter()
                .map(|line| Line::styled(line, colors.body))
                .collect(),
            Text::Even(text) => even_lines(text, width).into_iter().map(Line::raw).collect(),
            Text::Verdict { text, good } => {
                let (style, sign) = if *good {
                    (theme.dialog_title, glyphs::MATCH)
                } else {
                    (theme.error_title, glyphs::MISMATCH)
                };
                vec![Line::from(Span::styled(
                    format!("{sign} {text}"),
                    style.bold(),
                ))]
            }
        }
    }

    /// Draws the parts under the message from row `index` of `inner`, the room inside the
    /// frame, top to bottom, a separator as a blank row, the picker as `picker` says; and keeps
    /// where each that takes the focus is for the mouse in `spots`. Returns the row after them,
    /// or `None` without room.
    fn draw_body(
        &self,
        frame: &mut Frame<'_>,
        (inner, mut index): (Rect, u16),
        style: (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
        picker: &mut Option<PickerDraw<'_, '_>>,
    ) -> Option<u16> {
        // The boxes line up a blank after the widest label.
        let room = usize::from(inner.width);
        let widest = self
            .combo_boxes
            .iter()
            .map(|labelled| cells::width(&labelled.label.text))
            .max()
            .unwrap_or(0);
        let label_width = (widest + 1).min(room / 2);
        for labelled in &self.combo_boxes {
            labelled.combo.place(None);
        }
        for &part in &self.parts {
            if index >= inner.height {
                return None;
            }
            let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
            match part {
                Part::ComboBox(number) => {
                    self.draw_combo_box(frame, (row, number, label_width), style, spots);
                    index += 1;
                }
                Part::Field(number) => {
                    index =
                        self.draw_labelled_field(frame, (inner, index), number, style, spots)?;
                }
                Part::Check(number) => {
                    self.draw_check(frame, (row, number), style, spots);
                    index += 1;
                }
                Part::Item(number) => {
                    self.draw_item(frame, (row, number), style, spots);
                    index += 1;
                }
                Part::Text(number) => {
                    for line in self.text_lines(number, room, style.0, style.1) {
                        if index >= inner.height {
                            return None;
                        }
                        let row = Rect::new(inner.x, inner.y + index, inner.width, 1);
                        frame.render_widget(line, row);
                        index += 1;
                    }
                }
                Part::Picker => {
                    let Some(draw) = picker.as_mut() else {
                        continue;
                    };
                    let Some(body) = draw.body.take() else {
                        continue;
                    };
                    // Its rows give way to the line and the buttons under it, or take all the
                    // room they leave.
                    let room = inner.height.saturating_sub(index + 2);
                    let height = if body.fill {
                        room
                    } else {
                        draw.picker.height(&body).min(room)
                    };
                    let area = Rect::new(inner.x, inner.y + index, inner.width, height);
                    self.picker_area.set(Some(area));
                    // The cursor's row dims while the focus is elsewhere.
                    let focused = self.focus == Focus::Picker;
                    let row = &mut *draw.row;
                    let mut row = |shown: ShownRow| {
                        let line = row(shown);
                        if shown.selected && !focused {
                            line.patch_style(Modifier::DIM)
                        } else {
                            line
                        }
                    };
                    draw.picker.render_in(frame, area, style.1, body, &mut row);
                    index += height;
                }
                Part::Separator => index += 1,
                Part::Line => {
                    draw_separator(frame, inner, row.y, style.0, style.1);
                    index += 1;
                }
            }
        }
        Some(index)
    }

    /// Draws field `number` under its label from row `index` of `inner`, with the terminal
    /// cursor in it if it has the focus, and keeps where for the mouse in `spots`, and where
    /// the glyph that switches how it searches is. A disabled one is faint, label and field
    /// with its placeholder, and takes no clicks. Returns the row after it, or `None` without
    /// room.
    fn draw_labelled_field(
        &self,
        frame: &mut Frame<'_>,
        (inner, mut index): (Rect, u16),
        number: usize,
        (colors, theme): (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
    ) -> Option<u16> {
        let row = |index: u16| Rect::new(inner.x, inner.y + index, inner.width, 1);
        let labelled = &self.fields[number];
        let label = &labelled.label;
        if !label.text.is_empty() {
            // Faint, its letter as the rest, as nothing takes it.
            let lettered = if labelled.disabled {
                (colors.disabled(theme), false)
            } else {
                (colors, true)
            };
            let room = usize::from(inner.width).max(1);
            frame.render_widget(
                Line::from(label_spans(label, room, lettered, theme)),
                row(index),
            );
            index += 1;
        }
        if index >= inner.height {
            return None;
        }
        if labelled.disabled {
            if let Some(placeholder) = &labelled.placeholder {
                let room = usize::from(inner.width);
                let line = placeholder_line(placeholder, room, colors.input, theme);
                frame.render_widget(line, row(index));
            }
            let style = colors.input.patch(theme.disabled);
            frame.buffer_mut().set_style(row(index), style);
            return Some(index + 1);
        }
        let (column, area, glyph) = draw_field(frame, row(index), labelled, (colors, theme));
        self.glyphs
            .borrow_mut()
            .extend(glyph.map(|glyph| (number, glyph)));
        spots.push((Focus::Field(number), area));
        if number == 0 {
            self.field_area.set(Some(row(index)));
        }
        if self.focus == Focus::Field(number) {
            let column = u16::try_from(column).unwrap_or(0);
            self.cursor
                .set(Some(Position::new(inner.x + column, inner.y + index)));
        }
        Some(index + 1)
    }

    /// Draws check box `number` on `row`, and keeps where for the mouse in `spots`.
    fn draw_check(
        &self,
        frame: &mut Frame<'_>,
        (row, number): (Rect, usize),
        (colors, theme): (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
    ) {
        let check = &self.checks[number];
        let focused = self.focus == Focus::Check(number);
        let style = if focused { colors.focused } else { colors.body };
        let mut spans = colors.check_box(check.on, focused, theme);
        spans.push(Span::styled(" ", style));
        let hotkey = colors.hotkey_style(focused);
        spans.extend(check.label.spans(style, Some(hotkey), colors.mark));
        frame.render_widget(Line::from(spans), row);
        spots.push((Focus::Check(number), row));
    }

    /// Draws item `number` on `row`, its detail on the right, cut at its end where it does not
    /// fit; the whole row in the colors of the focus while it has it. Keeps where for the mouse
    /// in `spots`.
    fn draw_item(
        &self,
        frame: &mut Frame<'_>,
        (row, number): (Rect, usize),
        (colors, theme): (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
    ) {
        let item = &self.items[number];
        let focused = self.focus == Focus::Item(number);
        let style = if focused { colors.focused } else { colors.body };
        let hotkey = colors.hotkey_style(focused);
        let mut spans = item.label.spans(style, Some(hotkey), colors.mark);
        let used: usize = spans.iter().map(Span::width).sum();
        let room = usize::from(row.width).saturating_sub(used + 1);
        let cut = (cells::Align::Right, cells::Cut::End);
        let detail = cells::fitted(
            &item.detail,
            room,
            cut,
            (style, colors.mark),
            theme.glyphs.marks,
        );
        spans.push(Span::styled(" ", style));
        spans.extend(detail.spans);
        frame.render_widget(Line::from(spans).style(style), row);
        spots.push((Focus::Item(number), row));
    }

    /// Draws combo box `number` on `row` after its label, the box `label_width` cells in, in the
    /// colors of fields, apart with the focus; and keeps where for the mouse in `spots`, the
    /// whole line.
    fn draw_combo_box(
        &self,
        frame: &mut Frame<'_>,
        (row, number, label_width): (Rect, usize, usize),
        (colors, theme): (Colors, &Theme),
        spots: &mut Vec<(Focus, Rect)>,
    ) {
        let labelled = &self.combo_boxes[number];
        let room = usize::from(row.width);
        let hotkey = colors.hotkey_style(false);
        let mut spans = labelled.label.fit(
            label_width,
            colors.body_styles(),
            hotkey,
            theme.glyphs.marks,
        );
        let focused = self.focus == Focus::ComboBox(number);
        let boxed = labelled
            .combo
            .box_spans(room - label_width, focused, colors, theme);
        let cells = boxed.iter().map(Span::width).sum::<usize>();
        let x = row.x + u16::try_from(label_width).unwrap_or(0);
        let width = u16::try_from(cells).unwrap_or(u16::MAX);
        let area = Rect::new(x, row.y, width, 1).intersection(row);
        labelled.combo.place(Some(area));
        spans.extend(boxed);
        frame.render_widget(Line::from(spans).style(colors.body), row);
        spots.push((Focus::ComboBox(number), row));
    }
}
