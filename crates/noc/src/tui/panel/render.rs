//! Drawing the panel: its frame, rows, columns, and status line.

use noc_locale::Formats;
use noc_scroll::scrollbar;
use noc_vfs::{FileKind, Location};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use unicode_width::UnicodeWidthChar as _;

use crate::i18n::fl;
use crate::tui::cells::{self, Align, Cut, Marks};
use crate::tui::glyphs;
use crate::tui::keymap::Action;
use crate::tui::search::Prompt;
use crate::tui::spinner::Shine;
use crate::tui::theme::Theme;
use crate::tui::window;

use super::{
    HostStatus, MIN_LINK_TARGET, MIN_NAME_WIDTH, Panel, Row, Search, Sort, SortKey, View,
    home_text, location_text, short_size, shown_name,
};

/// Rows of the skeleton that stands for the rows a veiled panel waits for: a few, as more
/// looked busier than what they stand for, and a directory may hold none.
const WAIT_ROWS: u16 = 3;

/// What a veiled panel shows once its wait shows: a skeleton for its rows and status line.
struct Veil {
    /// The shine that crosses the skeleton; none while it rests, as for a host whose dialog
    /// asks whether to try again.
    shine: Option<Shine>,
}

impl Panel {
    /// Draws the panel: the location in the frame, column headers, the rows, a bar in the frame
    /// beside the marked ones, the size and number of marked entries on the line below them, a status line with the name under the cursor or
    /// quick search, and the totals of the directory and its file system on the bottom of the
    /// frame. What the panel kept from a host that is not connected is toned down. While it is
    /// veiled, once its wait shows, a skeleton stands for its rows and its status line, and the
    /// bottom of the frame is bare.
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
        if inner.height < 3 || inner.width < 2 {
            return;
        }
        let veil = self.veil(view);
        if veil.is_none() {
            self.render_bottom(frame, area, theme);
        }
        let width = usize::from(inner.width);
        let list_height = usize::from(inner.height - 3);
        self.page = list_height;
        self.list = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 3);
        self.scroll(list_height);
        let line = |y: u16| Rect::new(inner.x, y, inner.width, 1);
        let columns = DirColumns::for_width(width, column_widths(view.formats));
        if let Some(Veil { shine }) = veil {
            frame.render_widget(columns.header(self.sort, theme), line(inner.y));
            let rows = self.list.height.min(WAIT_ROWS);
            let skeleton = Rect {
                height: rows,
                ..self.list
            };
            window::draw_skeleton(frame, skeleton, (theme, theme.panel), shine);
            self.render_separator(frame, area, inner.bottom() - 2, (0, 0), theme);
            self.search_glyphs.set(None);
            let status = window::status_skeleton(inner.width, theme.panel, theme, shine);
            frame.render_widget(status, line(inner.bottom() - 1));
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
                columns.row(row, (self.shift, &found), view),
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
        self.render_separator(frame, area, inner.bottom() - 2, marked, theme);
        self.column_lines = columns
            .lines()
            .into_iter()
            .filter_map(|offset| u16::try_from(offset).ok())
            .filter(|offset| *offset < inner.width)
            .map(|offset| inner.x + offset)
            .collect();
        self.render_column_lines(frame, area, inner, theme);
        self.render_rename(frame, inner, list_height, active, view);
        self.render_status(frame, line(inner.bottom() - 1), active, view);
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
    fn veil(&self, view: &View<'_>) -> Option<Veil> {
        let wait = self.wait().filter(|_| self.veiled())?;
        view.spin.wait_dots(wait)?;
        let stalled = match self.pending.as_ref().map(|pending| &pending.location) {
            Some(Location::Remote { host, .. }) => (view.hosts)(host).status == HostStatus::Stalled,
            _ => false,
        };
        let shine = (!stalled).then(|| view.spin.shine(wait)).flatten();
        Some(Veil { shine })
    }

    /// The status line, at `area`: quick search with its cursor in the active panel, or the name
    /// under the cursor, with the target of a symlink, and the size of a file; while a name is
    /// renamed in its row, the name it had.
    fn render_status(&self, frame: &mut Frame<'_>, area: Rect, active: bool, view: &View<'_>) {
        let theme = view.theme;
        let width = usize::from(area.width);
        self.search_glyphs.set(None);
        if let Some(search) = &self.search {
            self.render_search(frame, area, active, (search, theme));
            return;
        }
        let status_style = (Style::new(), theme.truncated);
        let (status, linked) = self.cursor_status(view);
        // The size of the file under the cursor, every byte of it.
        let size = self.cursor_size(view.formats);
        let mut line = Line::default().style(status_style.0);
        let mut room = width;
        // At the right, as in Far, while the name keeps some room.
        let size = size
            .map(|size| format!(" {size}"))
            .filter(|size| cells::width(size) + MIN_NAME_WIDTH <= room);
        room -= size.as_deref().map_or(0, cells::width);
        let marks = theme.glyphs.marks;
        let fitted = match linked {
            Some(linked) => linked.fit(&status, room, status_style, marks),
            None => cells::fit(&status, room, Align::Left, status_style, marks),
        };
        line.spans.extend(fitted);
        line.spans.extend(size.map(Span::raw));
        frame.render_widget(self.toned(line), area);
    }

    /// The status line of quick search, at `area`: what is typed after its prompt, with the
    /// cursor in the active panel, and the glyph of how it matches at the right end, crossed
    /// out while what is typed is no regular expression (ADR 0045). What is typed is not of
    /// the host, so it is never toned down.
    fn render_search(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        active: bool,
        (search, theme): (&Search, &Theme),
    ) {
        let width = usize::from(area.width);
        let style = (theme.quick_search, theme.truncated_cursor);
        let invalid = self.finder().is_some_and(|finder| finder.error().is_some());
        let mark = search.input.mark(invalid, theme.quick_search, theme);
        let prompt = Prompt::new(|text| {
            if search.back {
                fl!("panel-search-back", text = text)
            } else {
                fl!("panel-search", text = text)
            }
        });
        // The glyphs, when what is typed keeps some room.
        let glyphs = Some(mark.width()).filter(|glyphs| prompt.width() + glyphs < width);
        let room = width - glyphs.unwrap_or(0);
        let (shown, column) = search.field.visible(room.saturating_sub(prompt.width()));
        if active {
            let column = u16::try_from(cells::width(&prompt.before) + column).unwrap_or(u16::MAX);
            let x = area.x.saturating_add(column).min(area.right() - 1);
            frame.set_cursor_position((x, area.y));
        }
        let text = prompt.with(&shown);
        let spans = vec![Span::styled(text, theme.quick_search)];
        let mut line = Line::from(cells::fit_spans(spans, room, style, theme.glyphs.marks))
            .style(theme.quick_search);
        if glyphs.is_some() {
            let at = area.x + u16::try_from(room).unwrap_or(u16::MAX);
            line.spans.extend(mark.spans());
            self.search_glyphs.set(Some(mark.drawn_at(at, area.y)));
        }
        frame.render_widget(line, area);
    }

    /// What a click at `at` stands for on the glyphs of quick search, where the last render
    /// drew them, as the keys after Ctrl+s.
    pub(crate) fn search_action_at(&self, at: Position) -> Option<Action> {
        self.search.as_ref()?;
        self.search_glyphs.get()?.action_at(at)
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

    /// The size of the file under the cursor, every byte of it, which the size column rounds.
    /// Not of a symlink: its status line is about its target, and its own size is only the
    /// length of the path it holds.
    fn cursor_size(&self, formats: &Formats) -> Option<String> {
        match self.row(self.cursor)? {
            Row::Entry(entry)
                if !matches!(entry.metadata.kind, FileKind::Dir | FileKind::Symlink) =>
            {
                entry.metadata.size.map(|size| formats.integer(size))
            }
            Row::Entry(_) | Row::Parent => None,
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

    /// The field of the entry being renamed, over its row from the name on, in the colors of a
    /// dialog's text field, with the selection in reverse.
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

    /// The lines between columns, down from the header to the line above the status line, in
    /// the color of the frame on the background of the rows they cross, so that the cursor and
    /// marks run across them; never underlined, which would cut them. Joined to the frame where
    /// its line runs, not where the title or the total of the marked entries cuts into it.
    fn render_column_lines(&self, frame: &mut Frame<'_>, area: Rect, inner: Rect, theme: &Theme) {
        let color = theme.panel_border.fg.unwrap_or(Color::Reset);
        let (_, bottom) = theme.column_tees();
        let buffer = frame.buffer_mut();
        for &x in &self.column_lines {
            for y in inner.y..inner.bottom() - 2 {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_char(glyphs::COLUMN).set_fg(color);
                    cell.modifier.remove(Modifier::UNDERLINED | Modifier::BOLD);
                }
            }
            if let Some(cell) = buffer.cell_mut((x, inner.bottom() - 2))
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
    /// `text` in exactly `width` cells over `style`: the target cut at its start where it does
    /// not fit, as a path whose end matters most, its icon and marker kept; the name alone where
    /// it leaves the target too little room. The mark of text cut short in `mark`.
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
        let fixed = cells::width(name)
            + cells::width(arrow)
            + cells::width(&self.icon)
            + cells::width(&self.marker);
        if fixed + MIN_LINK_TARGET.min(cells::width(target)) > width {
            return cells::fit(name, width, Align::Left, (style, mark), marks);
        }
        let target_style = style.patch(self.target);
        let mut spans = vec![
            Span::styled(name.to_owned(), style),
            Span::styled(arrow.to_owned(), Theme::icon(style)),
            Span::styled(self.icon.clone(), Theme::icon(target_style)),
        ];
        let room = width - fixed;
        if cells::width(target) <= room {
            spans.push(Span::styled(target.to_owned(), target_style));
        } else {
            let fitted = cells::fit_cut(target, room, Align::Left, Cut::Start);
            spans.extend(cells::spans(fitted, (target_style, mark), marks));
        }
        spans.push(Span::styled(self.marker.clone(), target_style));
        cells::fit_spans(spans, width, (style, mark), marks)
    }
}

impl DirColumns {
    /// Where the lines between columns run, in cells from the left of the rows; the rows leave
    /// a blank there for them.
    fn lines(self) -> Vec<usize> {
        let mut lines = Vec::new();
        if self.size {
            lines.push(self.name);
        }
        if self.time {
            lines.push(self.name + 1 + self.widths.0);
        }
        lines
    }

    /// The column titles; an arrow marks the one the directory is sorted by, and its direction.
    fn header(self, sort: Sort, theme: &Theme) -> Line<'static> {
        let mut titles = [fl!("panel-name"), fl!("panel-size"), fl!("panel-time")];
        let (column, title) = match sort.key {
            SortKey::Name => (0, fl!("panel-name")),
            SortKey::Extension => (0, fl!("panel-name-by-extension")),
            SortKey::Size => (1, fl!("panel-size")),
            SortKey::Time => (2, fl!("panel-time")),
        };
        titles[column] = format!("{}{title}", sort.arrow());
        let [name, size, time] = titles;
        let styles = (theme.header, theme.truncated);
        let titles = [name.as_str(), size.as_str(), time.as_str()];
        let marks = (theme.glyphs.marks, None);
        let (mut name, rest, _) = self.join(titles, [Align::Center; 3], styles, marks);
        name.extend(rest);
        Line::from(name).style(theme.header)
    }

    /// A row, with the name in the color of its kind, as mc highlights files, scrolled `shift`
    /// cells left, the characters of the name that quick search `found` in spans of their own.
    fn row(self, row: Row<'_>, (shift, found): (usize, &[usize]), view: &View<'_>) -> Shown {
        const ALIGN: [Align; 3] = [Align::Left, Align::Right, Align::Left];
        let theme = view.theme;
        let [prefix, name] = shown_name(row, view.decor);
        let mut unit = None;
        let (size, time, style) = match row {
            Row::Parent => (fl!("panel-up-dir"), String::new(), theme.directory),
            Row::Entry(entry) => {
                let size = match entry.metadata.size {
                    _ if entry.is_dir_like() => fl!("panel-dir"),
                    None => String::new(),
                    Some(size) => {
                        // Sizes stay far below 2^52, where `f64` would round them.
                        #[allow(clippy::cast_precision_loss)]
                        let (number, power) = cells::size_parts(size as f64, view.formats);
                        unit = Some((cells::unit(power), theme.size_unit(power)));
                        format!("{number}{}", cells::unit(power))
                    }
                };
                let time = cells::mtime(entry.metadata.modified, view.tz, view.formats);
                (size, time, theme.entry(entry))
            }
        };
        // Without styles: `row_style` gives the row its colors, and the marks their accent.
        let styles = (Style::new(), Style::new());
        let cells = ["", &size, &time];
        let (_, rest, cut) = self.join(cells, ALIGN, styles, (theme.glyphs.marks, unit));
        let mut shown = name_spans(&prefix, &name, (self.name, shift), (style, found), view);
        let after = shown.spans.len();
        shown
            .marks
            .extend(cut.into_iter().map(|index| after + index));
        shown.spans.extend(rest);
        Shown {
            line: Line::from(shown.spans),
            marks: shown.marks,
            found: shown.found,
        }
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
/// loses its middle, as in mc; scrolled `shift` cells left, the name alone, its prefix staying
/// in front of it, `‹` over its first cell and `…` where it goes on past the right one. A cell
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

/// The cells of the size and the time columns: as many as the longest sizes and times of
/// `formats`, the words that stand for a size, and the titles with the arrow of the order need.
fn column_widths(formats: &Formats) -> (usize, usize) {
    let widest = |texts: &[String]| texts.iter().map(|text| cells::width(text)).max();
    let titled = |title: String| cells::width(&title) + 1;
    let size = widest(&[fl!("panel-up-dir"), fl!("panel-dir")])
        .unwrap_or_default()
        .max(titled(fl!("panel-size")))
        .max(cells::short_size_width(formats));
    let time = formats.time_width().max(titled(fl!("panel-time")));
    (size, time)
}

/// Column widths of a directory: the name takes what the size and time columns leave, and
/// narrow panels drop the time, then the size.
#[derive(Debug, Clone, Copy)]
struct DirColumns {
    name: usize,
    size: bool,
    time: bool,
    /// The cells of the size and the time columns.
    widths: (usize, usize),
}

impl DirColumns {
    fn for_width(width: usize, widths: (usize, usize)) -> Self {
        let full = width.saturating_sub(widths.0 + widths.1 + 2);
        if full >= MIN_NAME_WIDTH {
            return Self {
                name: full,
                size: true,
                time: true,
                widths,
            };
        }
        let sized = width.saturating_sub(widths.0 + 1);
        if sized >= MIN_NAME_WIDTH {
            return Self {
                name: sized,
                size: true,
                time: false,
                widths,
            };
        }
        Self {
            name: width,
            size: false,
            time: false,
            widths,
        }
    }

    /// The name cell, and the other cells with the blanks the lines between them go on, in
    /// `style`, with the marks of text cut short of `marks` in `mark` over it, and the `unit`
    /// that ends the size in a style of its own; and the indexes of those marks among the other
    /// cells.
    fn join(
        self,
        [name, size, time]: [&str; 3],
        align: [Align; 3],
        (style, mark): (Style, Style),
        (marks, unit): (Marks, Option<(&str, Style)>),
    ) -> (Vec<Span<'static>>, Vec<Span<'static>>, Vec<usize>) {
        let mut rest = Vec::new();
        let mut cut = Vec::new();
        let cells = [
            (self.size, size, self.widths.0, unit),
            (self.time, time, self.widths.1, None),
        ];
        for ((shown, text, width, unit), align) in cells.into_iter().zip([align[1], align[2]]) {
            if !shown {
                continue;
            }
            rest.push(Span::styled(" ", style));
            let fitted = cells::fit_cut(text, width, align, Cut::Middle);
            if fitted.cut.is_some() {
                // After the text before the mark.
                cut.push(rest.len() + 1);
            }
            let mut spans = cells::spans(fitted, (style, mark), marks);
            if let ([head, tail], Some((unit, unit_style))) = (spans.as_mut_slice(), unit)
                && tail.content.is_empty()
                && let Some(number) = head.content.strip_suffix(unit)
            {
                *head = Span::styled(number.to_owned(), style);
                *tail = Span::styled(unit.to_owned(), style.patch(unit_style));
            }
            rest.extend(spans);
        }
        let name = cells::fit(name, self.name, align[0], (style, mark), marks);
        (name, rest, cut)
    }
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
