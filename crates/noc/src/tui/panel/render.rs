//! Drawing the panel: its frame, rows, columns, and status line.

use noc_locale::Formats;
use noc_scroll::scrollbar;
use noc_vfs::{DirEntry, FileKind, Location, Metadata};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use unicode_width::UnicodeWidthChar as _;

use crate::format;
use crate::i18n::fl;
use crate::tui::cells::{self, Align, Cut, Marks};
use crate::tui::glyphs;
use crate::tui::picker::FULL_ROWS;
use crate::tui::search::{GlyphSpots, Prompt};
use crate::tui::spinner::Shine;
use crate::tui::theme::Theme;
use crate::tui::window;

use super::layout::{self, Around, Detail};
use super::{
    HostStatus, MIN_LINK_TARGET, MIN_NAME_WIDTH, Panel, Row, Sort, SortKey, View, home_text,
    location_text, short_size, shown_name,
};
use crate::tui::machine::{Identity, Names};

/// Rows of the skeleton that stands for the rows a veiled panel waits for: a few, as more
/// looked busier than what they stand for, and a directory may hold none.
const WAIT_ROWS: u16 = 3;

/// How the status line fits its name: cut at its end.
const AT_END: (Align, Cut) = (Align::Left, Cut::End);

/// What a veiled panel shows once its wait shows: a skeleton for its rows and status line.
struct Veil<'a> {
    /// The shine that crosses the skeleton; none while it rests, as for a host whose dialog
    /// asks whether to try again.
    shine: Option<Shine<'a>>,
}

impl Panel {
    /// Draws the panel: the location in the frame, column headers, the rows, a bar in the frame
    /// beside the marked ones, the size and number of marked entries on the line below them, a
    /// status line with the name under the cursor, in two rows while `K` shows it whole, and the
    /// totals of the directory and its file system on the bottom of the frame. What the panel
    /// kept from a host that is not connected is toned down. While it is veiled, once its wait
    /// shows, a skeleton stands for its rows and its status line, and the bottom of the frame is
    /// bare.
    pub(crate) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        active: bool,
        view: &View<'_>,
    ) {
        let theme = view.theme;
        let title = self.title(area.width, active, view);
        let block = Block::bordered()
            .border_type(theme.border_type())
            .title(title)
            .style(theme.panel)
            .border_style(theme.panel_border);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.column_lines.clear();
        self.list = Rect::default();
        self.bar = None;
        let status_rows = if view.whole { FULL_ROWS } else { 1 };
        // The header, the separator, and the status line.
        let around = 2 + status_rows;
        if inner.height < around || inner.width < 2 {
            return;
        }
        let veiled = self.veil(view).is_some();
        if !veiled {
            self.render_bottom(frame, area, theme);
        }
        let width = usize::from(inner.width);
        let list_height = usize::from(inner.height - around);
        self.page = list_height;
        self.list = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - around);
        self.scroll(list_height);
        let separator = inner.bottom() - 1 - status_rows;
        let status = Rect::new(inner.x, separator + 1, inner.width, status_rows);
        let line = |y: u16| Rect::new(inner.x, y, inner.width, 1);
        let columns = DirColumns::for_width(width, &self.layout.columns, |detail| {
            detail_cells(detail, view.formats)
        });
        if let Some(Veil { shine }) = self.veil(view).filter(|_| veiled) {
            frame.render_widget(columns.header(self.sort, theme), line(inner.y));
            let rows = self.list.height.min(WAIT_ROWS);
            let skeleton = Rect {
                height: rows,
                ..self.list
            };
            window::draw_skeleton(frame, skeleton, (theme, theme.panel), shine);
            self.render_separator(frame, area, separator, (0, 0), theme);
            let skeleton = window::status_skeleton(inner.width, theme.panel, theme, shine);
            frame.render_widget(skeleton, line(status.y));
            return;
        }
        let bar = Rect::new(area.right() - 1, self.list.y, 1, self.list.height);
        let position = (self.rows(), list_height, self.window.top());
        let (style, arrows) = (theme.panel_border, theme.glyphs.scroll_bar);
        self.bar = scrollbar::render(frame, bar, position, style, arrows);

        self.names = (view.decor, columns.name);
        // Other rows on screen, or a wider column, may need less of the scroll.
        self.shift = self.shift.min(self.shift_limit());
        frame.render_widget(columns.header(self.sort, theme), line(inner.y));
        let visual = self.visual_rows();
        let mut finder = self.finder();
        let mine = self.identity(view);
        let owners = self.owners(mine.as_ref());
        for (screen_row, index) in (self.window.top()..self.rows())
            .take(list_height)
            .enumerate()
        {
            let Some(row) = self.row(index) else { break };
            let marked = matches!(row, Row::Entry(entry) if self.marked.contains(&entry.name));
            let cursor = (index == self.cursor).then_some(active);
            let selected = visual.as_ref().is_some_and(|rows| rows.contains(&index));
            let found = match (&mut finder, self.search_name(index)) {
                (Some(finder), Some(name)) => finder.positions(&name),
                _ => Vec::new(),
            };
            let text = row_style(
                columns.row(row, (self.shift, &found), (view, owners)),
                cursor,
                (marked, selected),
                theme,
            );
            let y = inner.y + 1 + u16::try_from(screen_row).unwrap_or(u16::MAX);
            frame.render_widget(self.toned(text), line(y));
            if marked {
                let style = theme.panel_border.patch(theme.mark).patch(theme.marked);
                let bar = Line::styled(glyphs::MARKED_ROW.to_string(), style);
                frame.render_widget(self.toned(bar), Rect::new(area.x, y, 1, 1));
            }
        }

        let marked = self.marked_total();
        self.render_separator(frame, area, separator, marked, theme);
        self.column_lines = columns
            .lines()
            .into_iter()
            .filter_map(|offset| u16::try_from(offset).ok())
            .filter(|offset| *offset < inner.width)
            .map(|offset| inner.x + offset)
            .collect();
        self.render_column_lines(frame, area, (inner.y, separator), theme);
        let at = u16::try_from(columns.name_at()).map_or(inner.width, |at| at.min(inner.width));
        let from_name = Rect {
            x: inner.x + at,
            width: inner.width - at,
            ..inner
        };
        self.render_rename(frame, from_name, list_height, active, view);
        self.render_status(frame, status, view);
    }

    /// Whether the panel would draw otherwise on a frame for time alone, or must draw on every
    /// frame: while it waits, or its host connects, dots turn in its title; while a name is
    /// renamed in it, it puts the cursor of the terminal there.
    pub(crate) fn moves(&self, view: &View<'_>) -> bool {
        if self.wait().is_some() || self.renaming.is_some() {
            return true;
        }
        let location = self
            .pending
            .as_ref()
            .map_or(&self.location, |pending| &pending.location);
        matches!(location, Location::Remote { host, .. }
            if (view.hosts)(host).status == HostStatus::Connecting)
    }

    /// The title on the frame: the location, or the one being listed, after one cell that keeps
    /// the path in place: the icon of a directory, or of a host in the color of its status; and
    /// while it is listed, dots that turn, in the color of a host it connects to.
    fn title(&self, width: u16, active: bool, view: &View<'_>) -> Line<'static> {
        let theme = view.theme;
        let (style, mark) = if active {
            (theme.panel_title_active, theme.truncated_cursor)
        } else {
            (theme.panel_border, theme.truncated)
        };
        let location = self
            .pending
            .as_ref()
            .map_or(&self.location, |pending| &pending.location);
        let dots = self.wait().and_then(|wait| view.spin.wait_dots(wait));
        let (icon, color) = match location {
            Location::Local(_) => match dots {
                Some(dots) => (dots, Some(theme.busy)),
                None => (view.decor.directory(), None),
            },
            Location::Remote { host, .. } => {
                let status = (view.hosts)(host).status;
                // A host that does not answer waits for the answer to its dialog, not for work:
                // its marker stands still.
                let dots = dots.filter(|_| status != HostStatus::Stalled);
                let color = match status {
                    HostStatus::Idle | HostStatus::Connected | HostStatus::Failed
                        if dots.is_some() =>
                    {
                        theme.busy
                    }
                    _ => theme.host_status(status),
                };
                let icon = dots.unwrap_or_else(|| view.decor.host_marker(status, view.spin.dots()));
                (icon, Some(color))
            }
        };
        let text = match location {
            Location::Local(path) => home_text(path, &self.home),
            Location::Remote { .. } => location_text(location),
        };
        let icon_style = color.map_or(style, |color| style.patch(color));
        let lead = [
            Span::styled(" ", style),
            Span::styled(icon.to_string(), icon_style),
            Span::styled(" ", style),
        ];
        // A path that does not fit keeps its last directories.
        let lead_width: usize = lead.iter().map(Span::width).sum();
        let room = usize::from(width)
            .saturating_sub(lead_width + 3)
            .min(cells::width(&text));
        let fit = (Align::Left, Cut::Start);
        let path = cells::fitted(&text, room, fit, (style, mark), theme.glyphs.marks);
        let mut title = Line::from(lead.to_vec());
        title.spans.extend(path.spans);
        title.spans.push(Span::styled(" ", style));
        title
    }

    /// The skeleton that stands for the rows of a veiled panel, once its wait shows; `None`
    /// while the panel shows its rows.
    fn veil(&self, view: &View<'_>) -> Option<Veil<'_>> {
        let wait = self.wait().filter(|_| self.veiled())?;
        view.spin.wait_dots(wait)?;
        let stalled = match self.pending.as_ref().map(|pending| &pending.location) {
            Some(Location::Remote { host, .. }) => (view.hosts)(host).status == HostStatus::Stalled,
            _ => false,
        };
        let shine = (!stalled).then(|| view.spin.shine(wait)).flatten();
        Some(Veil { shine })
    }

    /// The status line, at `area`: the name under the cursor, with the target of a symlink, and
    /// the details of the layout beside it, a line between each, but while `K` shows the name
    /// whole; while a name is renamed in its row, the name it had.
    /// A text that does not fit in the first row goes on in the next ones, plain, where `area`
    /// has more, as [`wrapped`] breaks it; otherwise it is cut at its end, as the viewer's lines,
    /// so that the mark reads as text that goes on, where the row above keeps the end of the name.
    /// It scrolls with the names, plain, `«` over its first cell, but no further than its end
    /// comes to the right edge of its last row.
    fn render_status(&self, frame: &mut Frame<'_>, area: Rect, view: &View<'_>) {
        let theme = view.theme;
        let width = usize::from(area.width);
        let status_style = (Style::new(), theme.truncated);
        let (status, linked) = self.cursor_status(view);
        // As in Far, while the name keeps some room, each with a blank, a line, and a blank.
        let details = if view.whole {
            (Vec::new(), Vec::new())
        } else {
            self.cursor_details(view)
        };
        let (before, after, room) = layout::fit(details, width, MIN_NAME_WIDTH, |parts| {
            parts
                .iter()
                .map(|(text, _)| cells::width(text))
                .sum::<usize>()
                + 2
        });
        let marks = theme.glyphs.marks;
        let wraps = area.height > 1 && cells::width(&status) > room;
        let rows = if wraps { usize::from(area.height) } else { 1 };
        // No further than the end of the text comes to the right edge of its last row; that far
        // once the name under the cursor shows its end in its row, as the line may be narrower.
        let room_of_rows = room + (rows - 1) * width.saturating_sub(1);
        let end = cells::width(&status).saturating_sub(room_of_rows);
        let to_end = self
            .row(self.cursor)
            .map_or(0, |row| self.shift_to_end(row));
        let shift = if to_end > 0 && self.shift >= to_end {
            end
        } else {
            self.shift.min(end)
        };
        let mut lines = if wraps || shift > 0 {
            wrapped(&status, (room, width, rows), shift, status_style, marks)
        } else {
            let fitted = match linked {
                Some(linked) => linked.fit(&status, room, status_style, marks),
                None => cells::fitted(&status, room, AT_END, status_style, marks).spans,
            };
            vec![Line::from(fitted)]
        };
        if let Some(first) = lines.first_mut() {
            let color = theme.panel_border.fg.unwrap_or(Color::Reset);
            let line = || Span::styled(format!(" {} ", glyphs::COLUMN), Style::new().fg(color));
            let spans = |parts: Parts| {
                parts
                    .into_iter()
                    .map(|(text, style)| Span::styled(text, style))
            };
            let name = std::mem::take(&mut first.spans);
            for parts in before {
                first.spans.extend(spans(parts));
                first.spans.push(line());
            }
            first.spans.extend(name);
            for parts in after {
                first.spans.push(line());
                first.spans.extend(spans(parts));
            }
        }
        for (y, line) in (area.y..area.bottom()).zip(lines) {
            let line = line.style(status_style.0);
            frame.render_widget(self.toned(line), Rect::new(area.x, y, area.width, 1));
        }
    }

    /// Quick search on the line `area` below the panels, as vim's search on its command line:
    /// what is typed after `/`, or `?` up the rows, with the cursor, and the glyphs of how it
    /// matches at the right end, crossed out while what is typed is no regular expression
    /// (ADR 0045, ADR 0093). Returns where the glyphs are, for the mouse; nothing without a
    /// search.
    pub(crate) fn render_search(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &Theme,
    ) -> Option<GlyphSpots> {
        let search = self.search.as_ref().filter(|_| !area.is_empty())?;
        let style = Style::new();
        let invalid = self.finder().is_some_and(|finder| finder.error().is_some());
        let mark = search.input.mark(invalid, style, theme);
        let prompt = Prompt::new(|text| {
            if search.back {
                fl!("panel-search-back", text = text)
            } else {
                fl!("panel-search", text = text)
            }
        });
        let field = (&prompt, &search.field);
        let (column, glyphs) = mark.render_field(frame, area, field, style, theme);
        let x = area.x.saturating_add(column).min(area.right() - 1);
        frame.set_cursor_position((x, area.y));
        glyphs
    }

    /// `line` toned down if it shows what the panel kept from a host that is not connected.
    fn toned<'a>(&self, mut line: Line<'a>) -> Line<'a> {
        if self.disconnected {
            line.style = Theme::offline(line.style);
            for span in &mut line.spans {
                span.style = Theme::offline(span.style);
            }
        }
        line
    }

    /// The details of the row under the cursor on the status line, before its name and after
    /// it, as the layout lists them: those it has, a symlink its own, as its row. None of `..`.
    fn cursor_details(&self, view: &View<'_>) -> (Vec<Parts>, Vec<Parts>) {
        let Some(Row::Entry(entry)) = self.row(self.cursor) else {
            return (Vec::new(), Vec::new());
        };
        let mine = self.identity(view);
        let owners = self.owners(mine.as_ref());
        let texts = |details: &[Detail]| {
            (details.iter())
                .filter_map(|&detail| status_parts(detail, entry, view, owners))
                .collect()
        };
        let status = &self.layout.status;
        (texts(&status.before), texts(&status.after))
    }

    /// Who the directory is read as, where known: Noon Commander for a local one, the user of the
    /// connection for a host's.
    fn identity(&self, view: &View<'_>) -> Option<Identity> {
        match &self.location {
            Location::Local(_) => view.identity.cloned(),
            Location::Remote { host, .. } => (view.hosts)(host).identity,
        }
    }

    /// What the panel knows of the owners of its entries, read as `mine`.
    fn owners<'a>(&'a self, mine: Option<&'a Identity>) -> Owners<'a> {
        Owners {
            mine,
            listed: &self.owners,
        }
    }

    /// The status line of the row under the cursor: its name, and the target of a symlink.
    fn cursor_status(&self, view: &View<'_>) -> (String, Option<Linked>) {
        let entry = match self.row(self.cursor) {
            Some(Row::Parent) => return ("..".to_owned(), None),
            Some(Row::Entry(entry)) => entry,
            None => return (String::new(), None),
        };
        let name = cells::sanitize(&entry.name);
        let Some(stored) = entry
            .link_target
            .as_ref()
            .filter(|_| entry.metadata.kind == FileKind::Symlink)
        else {
            return (name, None);
        };
        let theme = view.theme;
        let target = cells::sanitize(stored);
        let status = fl!(
            "panel-link",
            name = name.clone(),
            arrow = theme.glyphs.link_arrow,
            target = target.clone()
        );
        // The parts are styled where the name starts the text and the target ends it.
        let Some(target_start) = status.len().checked_sub(target.len()).filter(|&start| {
            status.starts_with(&name) && status.ends_with(&target) && name.len() <= start
        }) else {
            return (status, None);
        };
        let (icon, marker) = view.decor.target(entry.target_kind, stored);
        let linked = Linked {
            name_end: name.len(),
            target_start,
            target: theme.target(entry.target_kind),
            icon,
            marker,
        };
        (status, Some(linked))
    }

    /// The field of the entry being renamed, over its row from the name on, `inner` the inside of
    /// the panel from there, in the colors of a dialog's text field, with the selection in
    /// reverse.
    fn render_rename(
        &self,
        frame: &mut Frame<'_>,
        inner: Rect,
        list_height: usize,
        active: bool,
        view: &View<'_>,
    ) {
        let Some(renaming) = &self.renaming else {
            return;
        };
        let Some(Row::Entry(entry)) = self.row(self.cursor) else {
            return;
        };
        let Some(screen_row) = self
            .cursor
            .checked_sub(self.window.top())
            .filter(|&row| row < list_height)
        else {
            return;
        };
        let prefix = cells::width(&view.decor.entry(entry));
        let prefix = u16::try_from(prefix).unwrap_or(u16::MAX);
        if prefix.saturating_add(1) >= inner.width {
            return;
        }
        let y = inner.y + 1 + u16::try_from(screen_row).unwrap_or(u16::MAX);
        let area = Rect::new(inner.x + prefix, y, inner.width - prefix, 1);
        let theme = view.theme;
        let ([before, selected, after], column) =
            renaming.field.visible_parts(usize::from(area.width));
        let line = Line::from(vec![
            Span::raw(before),
            Span::styled(selected, selection(theme.dialog_input)),
            Span::raw(after),
        ])
        .style(theme.dialog_input);
        frame.render_widget(Clear, area);
        frame.render_widget(line, area);
        if active {
            let column = u16::try_from(column).unwrap_or(u16::MAX);
            frame.set_cursor_position((area.x.saturating_add(column).min(area.right() - 1), y));
        }
    }

    /// The line between the listing and the status line, across the panel's frame, with the
    /// total of the `marked` entries in the middle: how many, and the bytes in their files.
    fn render_separator(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        y: u16,
        (count, bytes): (usize, u64),
        theme: &Theme,
    ) {
        let inside = area.width.saturating_sub(2);
        let (left, right) = theme.tees();
        let separator = format!("{left}{}{right}", glyphs::LINE.repeat(usize::from(inside)));
        frame.render_widget(
            Line::styled(separator, theme.panel_border),
            Rect::new(area.x, y, area.width, 1),
        );
        if count == 0 {
            return;
        }
        let total = fl!("panel-marked", size = bytes, count = count);
        let total = format!(" {total} ");
        let width = cells::width(&total).min(usize::from(inside));
        let styles = (theme.mark, theme.truncated);
        let total = cells::fit(&total, width, Align::Left, styles, theme.glyphs.marks);
        let total_width = u16::try_from(width).unwrap_or(u16::MAX);
        let x = area.x + area.width.saturating_sub(total_width) / 2;
        frame.render_widget(
            self.toned(Line::from(total)),
            Rect::new(x, y, total_width.min(inside), 1),
        );
    }

    /// The lines between columns, down from the header at `top` to the `separator` above the
    /// status line, in the color of the frame on the background of the rows they cross, so that
    /// the cursor and marks run across them; never underlined, which would cut them. Joined to
    /// the frame where its line runs, not where the title or the total of the marked entries cuts
    /// into it.
    fn render_column_lines(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        (top, separator): (u16, u16),
        theme: &Theme,
    ) {
        let color = theme.panel_border.fg.unwrap_or(Color::Reset);
        let (_, bottom) = theme.column_tees();
        let buffer = frame.buffer_mut();
        for &x in &self.column_lines {
            for y in top..separator {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_char(glyphs::COLUMN).set_fg(color);
                    cell.modifier.remove(Modifier::UNDERLINED | Modifier::BOLD);
                }
            }
            if let Some(cell) = buffer.cell_mut((x, separator))
                && cell.symbol() == glyphs::LINE
            {
                cell.set_char(bottom);
            }
        }
        self.join_columns(frame, area, theme);
    }

    /// Joins the lines between columns to the top of the frame, `area`, drawn again over the
    /// panel's, as the tabs do; where it shows a title or tabs, the line stops short of it.
    pub(crate) fn join_columns(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let (top, _) = theme.column_tees();
        let horizontal = theme.lines().horizontal.to_string();
        let buffer = frame.buffer_mut();
        for &x in &self.column_lines {
            if let Some(cell) = buffer.cell_mut((x, area.y))
                && [horizontal.as_str(), glyphs::LINE].contains(&cell.symbol())
            {
                cell.set_char(top);
            }
        }
    }

    /// The bottom of the frame: at the left the size of the shown files and how many files and
    /// directories are shown, at the right the free space and size of the directory's file
    /// system, as in mc. Each is left out when unknown or when it does not fit; the free space
    /// goes first.
    fn render_bottom(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let y = area.bottom() - 1;
        // A corner and a line on either side.
        let mut room = area.width.saturating_sub(4);
        if let Some(text) = self.total_text() {
            let width = u16::try_from(cells::width(&text)).unwrap_or(u16::MAX);
            if width > room {
                return;
            }
            frame.render_widget(
                self.toned(Line::styled(text, theme.panel)),
                Rect::new(area.x + 2, y, width, 1),
            );
            // A line between the two.
            room = room.saturating_sub(width.saturating_add(1));
        }
        let Some(text) = self.space_text() else {
            return;
        };
        let width = u16::try_from(cells::width(&text)).unwrap_or(u16::MAX);
        if width > room {
            return;
        }
        frame.render_widget(
            self.toned(Line::styled(text, theme.panel)),
            Rect::new(area.right() - 2 - width, y, width, 1),
        );
    }

    /// The size of the shown files and how many files and directories are shown, padded for
    /// the frame; none while the panel holds no entries of a host that is not connected.
    fn total_text(&self) -> Option<String> {
        if self.disconnected && !self.has_entries() {
            return None;
        }
        let (files, dirs, bytes) = self.shown_total();
        let text = fl!(
            "panel-total",
            size = short_size(bytes),
            files = files,
            dirs = dirs
        );
        Some(format!(" {text} "))
    }

    /// The free space and size of the directory's file system, padded for the frame; none when
    /// unknown.
    fn space_text(&self) -> Option<String> {
        // Pseudo file systems such as devfs report nothing.
        let space = self.space.filter(|space| space.total > 0)?;
        let percent = u128::from(space.available.min(space.total)) * 100 / u128::from(space.total);
        let text = fl!(
            "panel-space",
            free = short_size(space.available),
            total = short_size(space.total),
            percent = u64::try_from(percent).unwrap_or(100)
        );
        Some(format!(" {text} "))
    }
}

/// The status line of a symlink under the cursor: its name and its target, which ends the text.
/// The name starts the text, plain, as the row above shows its kind; the arrow comes between,
/// toned down; and the target, which tells its kind by its color and by an icon in front of it
/// or a marker after it, ends it.
#[derive(Debug)]
struct Linked {
    /// Where the name ends in the text, and where the target starts.
    name_end: usize,
    target_start: usize,
    /// The target's style, by its kind.
    target: Style,
    /// The icon in front of the target, or the marker after it.
    icon: String,
    marker: String,
}

impl Linked {
    /// `text` in exactly `width` cells over `style`: where the name and the target do not both
    /// fit, they share the room evenly, what one leaves going to the other; the name cut at its
    /// end, the target at its start, as a path whose end matters most, its icon and marker kept.
    /// The name alone where either would get too little room. The mark of text cut short in
    /// `mark`.
    fn fit(
        &self,
        text: &str,
        width: usize,
        (style, mark): (Style, Style),
        marks: Marks,
    ) -> Vec<Span<'static>> {
        let name = &text[..self.name_end];
        let arrow = &text[self.name_end..self.target_start];
        let target = &text[self.target_start..];
        let (name_width, target_width) = (cells::width(name), cells::width(target));
        let fixed = cells::width(arrow) + cells::width(&self.icon) + cells::width(&self.marker);
        let room = width.saturating_sub(fixed);
        if MIN_LINK_TARGET.min(name_width) + MIN_LINK_TARGET.min(target_width) > room {
            return cells::fitted(name, width, AT_END, (style, mark), marks).spans;
        }
        let name_room = name_width.min(room.saturating_sub(target_width).max(room / 2));
        let target_room = target_width.min(room - name_room);
        let target_style = style.patch(self.target);
        let mut spans = cells::fitted(name, name_room, AT_END, (style, mark), marks).spans;
        spans.push(Span::styled(arrow.to_owned(), Theme::icon(style)));
        spans.push(Span::styled(self.icon.clone(), Theme::icon(target_style)));
        let fitted = cells::fit_cut(target, target_room, Align::Left, Cut::Start);
        spans.extend(cells::spans(fitted, (target_style, mark), marks));
        spans.push(Span::styled(self.marker.clone(), target_style));
        cells::fit_spans(spans, width, (style, mark), marks)
    }
}

impl DirColumns {
    /// The columns of a directory `width` cells wide, with the details of `around` in their
    /// `cells`: the name takes what they leave, and narrow panels drop those at the end of the
    /// list first.
    fn for_width(width: usize, around: &Around, cells: impl Fn(Detail) -> usize) -> Self {
        let sized = |details: &[Detail]| {
            (details.iter())
                .map(|&detail| (detail, cells(detail)))
                .collect()
        };
        let parts = (sized(&around.before), sized(&around.after));
        let (before, after, name) = layout::fit(parts, width, MIN_NAME_WIDTH, |(_, cells)| *cells);
        Self {
            before,
            name,
            after,
        }
    }

    /// Where the name starts, in cells from the left of the rows.
    fn name_at(&self) -> usize {
        self.before.iter().map(|(_, cells)| cells + 1).sum()
    }

    /// Where the lines between columns run, in cells from the left of the rows; the rows leave
    /// a blank there for them.
    fn lines(&self) -> Vec<usize> {
        let mut lines = Vec::new();
        let mut x = 0;
        for (_, cells) in &self.before {
            x += cells;
            lines.push(x);
            x += 1;
        }
        x += self.name;
        for (_, cells) in &self.after {
            lines.push(x);
            x += 1 + cells;
        }
        lines
    }

    /// The column titles; an arrow marks the one the directory is sorted by, and its direction.
    fn header(&self, sort: Sort, theme: &Theme) -> Line<'static> {
        let arrowed = |title: String, by: bool| {
            if by {
                format!("{}{title}", sort.arrow())
            } else {
                title
            }
        };
        let name = match sort.key {
            SortKey::Name => arrowed(fl!("panel-name"), true),
            SortKey::Extension => arrowed(fl!("panel-name-by-extension"), true),
            SortKey::Size | SortKey::Time => fl!("panel-name"),
        };
        let title = |detail| {
            let by = sorted_by(detail) == Some(sort.key);
            Cell::plain(arrowed(detail_title(detail), by), Align::Center)
        };
        let styles = (theme.header, theme.truncated);
        let marks = theme.glyphs.marks;
        let [before, after] = self.details(title, styles, marks);
        let mut spans = before.spans;
        spans.extend(cells::fit(&name, self.name, Align::Center, styles, marks));
        spans.extend(after.spans);
        Line::from(spans).style(theme.header)
    }

    /// A row, with the name in the color of its kind, as mc highlights files, scrolled `shift`
    /// cells left, the characters of the name that quick search `found` in spans of their own.
    fn row(
        &self,
        row: Row<'_>,
        (shift, found): (usize, &[usize]),
        (view, owners): (&View<'_>, Owners<'_>),
    ) -> Shown {
        let theme = view.theme;
        let [prefix, name] = shown_name(row, view.decor);
        let style = match row {
            Row::Parent => theme.directory,
            Row::Entry(entry) => theme.entry(entry),
        };
        // Without styles: `row_style` gives the row its colors, and the marks their accent.
        let styles = (Style::new(), Style::new());
        let cell = |detail| column_cell(detail, row, (view, owners));
        let [before, after] = self.details(cell, styles, theme.glyphs.marks);
        let name = name_spans(&prefix, &name, (self.name, shift), (style, found), view);
        let mut shown = Shown {
            line: Line::from(before.spans),
            marks: before.cut,
            found: Vec::new(),
        };
        let at = shown.line.spans.len();
        shown
            .marks
            .extend(name.marks.iter().map(|index| at + index));
        shown
            .found
            .extend(name.found.iter().map(|index| at + index));
        shown.line.spans.extend(name.spans);
        let at = shown.line.spans.len();
        shown.marks.extend(after.cut.iter().map(|index| at + index));
        shown.line.spans.extend(after.spans);
        shown
    }

    /// The cells of the details before the name and after it, each with the blank beside it that
    /// a line between columns runs on, as `cell` gives them, in `style`, with the marks of text
    /// cut short of `marks` in `mark` over it, and the unit that ends a size in a style of its
    /// own.
    fn details(
        &self,
        cell: impl Fn(Detail) -> Cell,
        (style, mark): (Style, Style),
        marks: Marks,
    ) -> [Part; 2] {
        let part = |details: &[(Detail, usize)], blank_first: bool| {
            let mut part = Part::default();
            for &(detail, width) in details {
                if blank_first {
                    part.spans.push(Span::styled(" ", style));
                }
                let Cell { parts, align } = cell(detail);
                let text: String = parts.iter().map(|(text, _)| text.as_str()).collect();
                let fitted = cells::fit_cut(&text, width, align, Cut::Middle);
                if fitted.cut.is_some() {
                    // After the text before the mark; cut short, it is plain.
                    part.cut.push(part.spans.len() + 1);
                    part.spans
                        .extend(cells::spans(fitted, (style, mark), marks));
                } else {
                    // Padded as `fit_cut` pads it, its parts in their styles.
                    let gap = width.saturating_sub(cells::width(&text));
                    let (left, right) = match align {
                        Align::Left => (0, gap),
                        Align::Center => (gap / 2, gap - gap / 2),
                        Align::Right => (gap, 0),
                    };
                    part.spans.push(Span::styled(" ".repeat(left), style));
                    let parts = parts.into_iter();
                    part.spans
                        .extend(parts.map(|(text, own)| Span::styled(text, style.patch(own))));
                    part.spans.push(Span::styled(" ".repeat(right), style));
                }
                if !blank_first {
                    part.spans.push(Span::styled(" ", style));
                }
            }
            part
        };
        [part(&self.before, false), part(&self.after, true)]
    }
}

/// A text in parts, each with a style of its own over the style around it.
type Parts = Vec<(String, Style)>;

/// The cell of a detail in a column: its text in parts, as the unit that ends a size in a style of
/// its own, and how it aligns.
struct Cell {
    parts: Parts,
    align: Align,
}

impl Cell {
    fn plain(text: String, align: Align) -> Self {
        Self {
            parts: vec![(text, Style::new())],
            align,
        }
    }
}

/// Spans of cells, with the indexes of those that are marks of text cut short.
#[derive(Default)]
struct Part {
    spans: Vec<Span<'static>>,
    cut: Vec<usize>,
}

/// The cell of `detail` in the column of `row`, what is ours set apart.
fn column_cell(detail: Detail, row: Row<'_>, (view, owners): (&View<'_>, Owners<'_>)) -> Cell {
    let Row::Entry(entry) = row else {
        return match detail {
            Detail::Size => Cell::plain(fl!("panel-up-dir"), Align::Right),
            Detail::Time | Detail::Mode | Detail::Owner => Cell::plain(String::new(), Align::Left),
        };
    };
    let metadata = &entry.metadata;
    match detail {
        Detail::Size => match metadata.size {
            _ if entry.is_dir_like() => Cell::plain(fl!("panel-dir"), Align::Right),
            None => Cell::plain(String::new(), Align::Right),
            Some(size) => {
                // Sizes stay far below 2^52, where `f64` would round them.
                #[allow(clippy::cast_precision_loss)]
                let (number, power) = cells::size_parts(size as f64, view.formats);
                let unit = cells::unit(power).to_owned();
                Cell {
                    parts: vec![(number, Style::new()), (unit, view.theme.size_unit(power))],
                    align: Align::Right,
                }
            }
        },
        Detail::Time => {
            let time = cells::mtime(metadata.modified, view.tz, view.formats);
            Cell::plain(time, Align::Left)
        }
        Detail::Mode => Cell {
            parts: mode_parts(entry, owners.mine).unwrap_or_default(),
            align: Align::Left,
        },
        Detail::Owner => Cell {
            parts: owner_parts(entry, owners).unwrap_or_default(),
            align: Align::Right,
        },
    }
}

/// The text of `detail` of `entry` on the status line, in parts, what is ours set apart; none
/// where it is not known.
fn status_parts(
    detail: Detail,
    entry: &DirEntry,
    view: &View<'_>,
    owners: Owners<'_>,
) -> Option<Parts> {
    let metadata = &entry.metadata;
    let plain = |text: String| vec![(text, Style::new())];
    match detail {
        // Every byte of it, which the column rounds, or shows as `DIR`.
        Detail::Size => metadata.size.map(|size| plain(view.formats.integer(size))),
        Detail::Time => Some(cells::mtime(metadata.modified, view.tz, view.formats))
            .filter(|time| !time.is_empty())
            .map(plain),
        Detail::Mode => mode_parts(entry, owners.mine),
        Detail::Owner => owner_parts(entry, owners),
    }
}

/// Which set of permissions of `metadata` applies to `mine`, as the system checks them: 0 the
/// owner's, 1 the group's, 2 the others'. None for uid 0, whom they do not limit, nor where the
/// owner is not known.
fn applies(metadata: &Metadata, mine: &Identity) -> Option<usize> {
    if mine.uid == 0 {
        return None;
    }
    let (uid, gid) = (metadata.uid?, metadata.gid?);
    Some(if uid == mine.uid {
        0
    } else if mine.groups.contains(&gid) {
        1
    } else {
        2
    })
}

/// What is `mine` as it is, and the rest toned down, as an icon is (ADR 0097).
fn own(mine: bool) -> Style {
    if mine {
        Style::new()
    } else {
        Theme::icon(Style::new())
    }
}

/// The type and the permissions of `entry`, as `ls -l` writes them, in parts: the set that
/// applies to `mine` as it is, the type and the other sets toned down; a symlink's toned down
/// whole, as the system checks its target's; plain where it is not known who reads it, or no set
/// applies. None when they are not known.
fn mode_parts(entry: &DirEntry, mine: Option<&Identity>) -> Option<Parts> {
    let metadata = &entry.metadata;
    let text = format::mode(metadata.kind, Some(metadata.permissions?));
    if mine.is_some() && metadata.kind == FileKind::Symlink {
        return Some(vec![(text, own(false))]);
    }
    let Some(set) = mine.and_then(|mine| applies(metadata, mine)) else {
        return Some(vec![(text, Style::new())]);
    };
    // The type, then three sets of three letters, all ASCII.
    let mut parts = vec![(text[..1].to_owned(), own(false))];
    for index in 0..3 {
        let start = 1 + 3 * index;
        parts.push((text[start..start + 3].to_owned(), own(index == set)));
    }
    Some(parts)
}

/// What a panel knows of the owners of its entries: who reads it, and the names of owners and
/// groups, those its listing came with, then those of who reads it (ADR 0098).
#[derive(Clone, Copy)]
struct Owners<'a> {
    mine: Option<&'a Identity>,
    listed: &'a Names,
}

impl Owners<'_> {
    /// The name of the user `uid`, else its number.
    fn user(self, uid: u32) -> String {
        let named = self.listed.users.get(&uid);
        let named = named.or_else(|| self.mine?.names.users.get(&uid));
        named.cloned().unwrap_or_else(|| uid.to_string())
    }

    /// The name of the group `gid`, else its number.
    fn group(self, gid: u32) -> String {
        let named = self.listed.groups.get(&gid);
        let named = named.or_else(|| self.mine?.names.groups.get(&gid));
        named.cloned().unwrap_or_else(|| gid.to_string())
    }
}

/// The owner of `entry` and its group, by name where `owners` knows it, else by number in digits
/// alone, not grouped, as `ls -n` and `chown` write them, as they name rather than count
/// (ADR 0096, ADR 0098); in parts, each that is ours as it is and the rest toned down, plain
/// where it is not known who reads it. None when they are not known.
fn owner_parts(entry: &DirEntry, owners: Owners<'_>) -> Option<Parts> {
    let (uid, gid) = (entry.metadata.uid?, entry.metadata.gid?);
    let (user, group) = (owners.user(uid), owners.group(gid));
    let Some(mine) = owners.mine else {
        return Some(vec![(format!("{user}:{group}"), Style::new())]);
    };
    Some(vec![
        (user, own(uid == mine.uid)),
        (":".to_owned(), own(false)),
        (group, own(mine.groups.contains(&gid))),
    ])
}

/// The title of the column of `detail`.
fn detail_title(detail: Detail) -> String {
    match detail {
        Detail::Size => fl!("panel-size"),
        Detail::Time => fl!("panel-time"),
        Detail::Mode => fl!("panel-mode"),
        Detail::Owner => fl!("panel-owner"),
    }
}

/// What a directory is sorted by when its column of `detail` has the arrow, if it sorts.
fn sorted_by(detail: Detail) -> Option<SortKey> {
    match detail {
        Detail::Size => Some(SortKey::Size),
        Detail::Time => Some(SortKey::Time),
        Detail::Mode | Detail::Owner => None,
    }
}

/// A row as drawn, before [`row_style`] gives it its colors, with the indexes of its spans that
/// are marks of a name cut short, and of those that show what quick search found.
struct Shown {
    line: Line<'static>,
    marks: Vec<usize>,
    found: Vec<usize>,
}

/// The spans of a name cell, with the indexes of those that are marks, and of those that show
/// what quick search found.
#[derive(Default)]
struct NameSpans {
    spans: Vec<Span<'static>>,
    marks: Vec<usize>,
    found: Vec<usize>,
}

/// Which characters of a text a part of its cell shows.
#[derive(Clone, Copy)]
enum Shows {
    Nothing,
    /// From character `start` of the text on, after `skip` blanks; blanks after what it shows
    /// show nothing, as the characters they stand for differ.
    From {
        start: usize,
        skip: usize,
    },
    /// The last `count` characters of the text, at its end.
    Last(usize),
}

impl NameSpans {
    /// Puts `part` of a cell of `text` in `style`, cut in spans where it goes from characters
    /// that quick search `found` to others, as `shows` says which of them it shows.
    fn push(&mut self, part: &str, style: Style, shows: Shows, (text, found): (&[char], &[bool])) {
        let chars: Vec<char> = part.chars().collect();
        let source = |index: usize| {
            let at = match shows {
                Shows::From { start, skip } if index >= skip => start + index - skip,
                Shows::Last(count) if index + count >= chars.len() => {
                    (text.len() + index).checked_sub(chars.len())?
                }
                Shows::Nothing | Shows::From { .. } | Shows::Last(_) => return None,
            };
            (text.get(at) == Some(&chars[index])).then_some(at)
        };
        let mut run = String::new();
        let mut in_found = false;
        for (index, &c) in chars.iter().enumerate() {
            let hit = source(index).is_some_and(|at| found.get(at) == Some(&true));
            if hit != in_found && !run.is_empty() {
                self.run(std::mem::take(&mut run), style, in_found);
            }
            in_found = hit;
            run.push(c);
        }
        if !run.is_empty() || chars.is_empty() {
            self.run(run, style, in_found);
        }
    }

    fn run(&mut self, text: String, style: Style, found: bool) {
        if found {
            self.found.push(self.spans.len());
        }
        self.spans.push(Span::styled(text, style));
    }

    /// Puts a mark of text cut short.
    fn mark(&mut self, mark: &str, style: Style) {
        self.marks.push(self.spans.len());
        self.spans.push(Span::styled(mark.to_owned(), style));
    }
}

/// How many characters at the end of `text` fit in `cells`, as [`cells::fit_cut`] keeps them.
fn last_fitting(text: &[char], cells: usize) -> usize {
    let mut used = 0;
    text.iter()
        .rev()
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= cells
        })
        .count()
}

/// The index of the first character of `text` that starts at cell `cells` or after.
fn first_from(text: &[char], cells: usize) -> usize {
    let mut at = 0;
    text.iter()
        .take_while(|c| {
            let before = at;
            at += c.width().unwrap_or(0);
            before < cells
        })
        .count()
}

/// The name cell, `cells` wide, in `style`, with the icon of its `prefix` in a toned-down
/// `style`, and the indexes of the spans that are marks: `…` where a name that does not fit
/// loses its middle, as in mc; scrolled `shift` cells left, but no further than its end comes to
/// the right edge, the name alone, its prefix staying in front of it, `‹` over its first cell and
/// `…` where it goes on past the right one. A cell
/// that leaves no room after the prefix does not scroll. A cell cut so short that the prefix
/// lost its end, and mc's markers, are drawn in `style`. The characters of the name that quick
/// search `found` get spans of their own.
fn name_spans(
    prefix: &str,
    name: &str,
    (cells, shift): (usize, usize),
    (style, found): (Style, &[usize]),
    view: &View<'_>,
) -> NameSpans {
    let text = format!("{prefix}{name}");
    let chars: Vec<char> = text.chars().collect();
    let offset = prefix.chars().count();
    let mut hits = vec![false; chars.len()];
    for &index in found {
        if let Some(hit) = hits.get_mut(offset + index) {
            *hit = true;
        }
    }
    let of = (chars.as_slice(), hits.as_slice());
    let shown_marks = view.theme.glyphs.marks;
    let mut spans = NameSpans::default();
    let prefix_cells = cells::width(prefix);
    // No further than its end comes to the right edge: a name that fits does not move.
    let shift = shift.min(cells::width(name).saturating_sub(cells.saturating_sub(prefix_cells)));
    let fitted = if shift > 0 && cells > prefix_cells {
        let prefix_style = if view.decor.icons() {
            Theme::icon(style)
        } else {
            style
        };
        spans.push(prefix, prefix_style, Shows::Nothing, of);
        let (_, shown) = cells::split(name, shift);
        let (_, rest) = cells::split(&shown, 1);
        spans.mark(shown_marks.before, style);
        let room = cells - prefix_cells - 1;
        let fitted = cells::fit_cut(&rest, room, Align::Left, Cut::End);
        // The characters of the name that start past the cells scrolled and the mark, after the
        // blanks left of a wide character cut.
        let start = offset + first_from(&chars[offset..], shift + 1);
        let skip = rest.chars().count().saturating_sub(chars.len() - start);
        spans.push(&fitted.head, style, Shows::From { start, skip }, of);
        fitted
    } else {
        let fitted = cells::fit_cut(&text, cells, Align::Left, Cut::Middle);
        match fitted.head.strip_prefix(prefix) {
            Some(rest) if view.decor.icons() && !prefix.is_empty() => {
                spans.push(prefix, Theme::icon(style), Shows::Nothing, of);
                let shows = Shows::From {
                    start: offset,
                    skip: 0,
                };
                spans.push(rest, style, shows, of);
            }
            _ => spans.push(&fitted.head, style, Shows::From { start: 0, skip: 0 }, of),
        }
        fitted
    };
    let tail = if fitted.cut == Some(Cut::Middle) {
        Shows::Last(last_fitting(&chars, cells.saturating_sub(1) / 2))
    } else {
        Shows::Nothing
    };
    if fitted.cut.is_some() {
        spans.mark(fitted.mark(shown_marks), style);
    }
    spans.push(&fitted.tail, style, tail, of);
    spans
}

/// `text` scrolled `shift` cells left and in at most `rows` lines, the first of `first` cells, the
/// others of `width`, in `style`, with the marks of `marks` in `mark`: `«` over the first cell of
/// text scrolled; the rows after the first start with the mark of text that goes on, as the
/// viewer's wrapped lines do; and the last is cut at its end with it if the text goes on past it.
fn wrapped(
    text: &str,
    (first, width, rows): (usize, usize, usize),
    shift: usize,
    (style, mark): (Style, Style),
    marks: Marks,
) -> Vec<Line<'static>> {
    let mut spans = Vec::new();
    let (mut text, mut first) = (text.to_owned(), first);
    if shift > 0 {
        let (_, shown) = cells::split(&text, shift);
        text = cells::split(&shown, 1).1;
        spans.push(Span::styled(marks.before, style.patch(mark)));
        first = first.saturating_sub(1);
    }
    let head = if rows > 1 {
        cells::chop(&text, first, None).swap_remove(0)
    } else {
        text.clone()
    };
    let rest = &text[head.len()..];
    spans.extend(cells::fitted(&head, first, AT_END, (style, mark), marks).spans);
    let mut lines = vec![Line::from(spans)];
    let room = width.saturating_sub(1);
    let rows = cells::chop(rest, room, None).len().min(rows - 1);
    for row in cells::whole(rest, (room, rows), None, (style, mark), marks) {
        let mut line = Line::from(Span::styled(marks.after, style.patch(mark)));
        line.spans.extend(row.spans);
        lines.push(line);
    }
    lines
}

/// The cells of a column of `detail`: as many as its longest text with `formats` needs, and its
/// title, with the arrow of the order where it sorts.
fn detail_cells(detail: Detail, formats: &Formats) -> usize {
    let title = cells::width(&detail_title(detail)) + usize::from(sorted_by(detail).is_some());
    let text = match detail {
        Detail::Size => [fl!("panel-up-dir"), fl!("panel-dir")]
            .iter()
            .map(|text| cells::width(text))
            .max()
            .unwrap_or_default()
            .max(cells::short_size_width(formats)),
        Detail::Time => formats.time_width(),
        // `ls -l`'s type and nine permissions.
        Detail::Mode => 10,
        // `65535:65535`, so that the column does not change from one directory to the next;
        // a longer one, as an owner by name may be, loses its middle.
        Detail::Owner => 11,
    };
    text.max(title)
}

/// The columns of a directory: the details beside the name, each with its cells, and the cells of
/// the name.
#[derive(Debug)]
struct DirColumns {
    before: Vec<(Detail, usize)>,
    name: usize,
    after: Vec<(Detail, usize)>,
}

/// `style` for selected text: its colors swapped, or else reversed.
fn selection(style: Style) -> Style {
    match (style.fg, style.bg) {
        (Some(fg), Some(bg)) => style.fg(bg).bg(fg),
        _ if style.add_modifier.contains(Modifier::REVERSED) => {
            style.remove_modifier(Modifier::REVERSED)
        }
        _ => style.add_modifier(Modifier::REVERSED),
    }
}

/// `shown` as a row under the cursor, `Some(true)` in the active panel, `marked`, and `selected`
/// by Visual mode. The cursor replaces the colors of the row, as in mc, and a selection reverses
/// them; a mark and the cursor of the inactive panel only put a background under them, and the
/// bar in the frame shows the mark. The marks of a name cut short keep an accent over them, and
/// what quick search found its own colors.
fn row_style(
    shown: Shown,
    cursor: Option<bool>,
    (marked, selected): (bool, bool),
    theme: &Theme,
) -> Line<'static> {
    let Shown {
        line: mut text,
        marks,
        found,
    } = shown;
    let style = match (cursor == Some(true), selected) {
        (true, _) => Some(theme.cursor),
        (false, true) => Some(selection(theme.panel)),
        (false, false) => None,
    };
    if let Some(style) = style {
        for span in &mut text.spans {
            span.style = Style::new();
        }
        text = text.style(style);
    } else if marked {
        text = text.patch_style(theme.marked);
    }
    if cursor == Some(false) {
        text = text.patch_style(theme.cursor_inactive);
    }
    let mark = if cursor == Some(true) || selected {
        theme.truncated_cursor
    } else {
        theme.truncated
    };
    for index in marks {
        if let Some(span) = text.spans.get_mut(index) {
            span.style = span.style.patch(mark);
        }
    }
    for index in found {
        if let Some(span) = text.spans.get_mut(index) {
            span.style = span.style.patch(theme.find_match);
        }
    }
    text
}
