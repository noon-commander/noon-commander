//! The viewer on screen: a file's text, scrolled, with long lines wrapped or cut.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use noc_scroll::scrollbar::{self, Bar, BarGlyphs};
use noc_scroll::{Motion, Step};
use noc_text::{Align, Cut, Fitted, Marks};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::i18n::fl;
use crate::parts::{Known, Parts};
use crate::read::{Part, Stamp, Want};
use crate::text::{cut, row_of, row_starts, rows_of_lines, wider_than, width};

/// Long lines whose rows are kept, as wrapped: more than a few screens take.
const WRAPPED_LINES: usize = 1024;

/// Where the rows of lines start, by line.
type Wrapped = HashMap<usize, Rc<[usize]>>;

/// Whether rows after the first of a wrapped line start with a mark in `width` cells: not
/// where it would leave no room for text.
fn breaks(width: usize) -> bool {
    width >= 2
}

/// The cells of text in each row after the first of a line wrapped in `width` cells.
fn continued(width: usize) -> usize {
    if breaks(width) { width - 1 } else { width }
}

/// What the viewer is asked to do. The app maps its keys to these; closing the viewer is the
/// app's own business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Scrolls; never past the end of the file at the bottom of the screen. Home goes back to
    /// the first column too.
    Scroll(Motion),
    /// Puts a line of the file at the top, or as near as the end lets it; the viewer reads the
    /// parts of the file on the way to it.
    Top(usize),
    /// Goes where the thumb of the scroll bar was dragged, in its units: lines when the whole
    /// file is held and is no more than a part, else bytes.
    Bar(usize),
    /// One column left, when lines are cut.
    Left,
    /// One column right, when lines are cut; no further than the end of the longest line on
    /// screen at the right edge.
    Right,
    /// Count columns left, when lines are cut, as vim's `zh`.
    ScrollLeft(usize),
    /// Count columns right, when lines are cut, as vim's `zl`; no further than
    /// [`Right`](Self::Right) goes.
    ScrollRight(usize),
    /// Count half widths of the text left, when lines are cut, as vim's `zH`.
    HalfLeft(usize),
    /// Count half widths of the text right, when lines are cut, as vim's `zL`; no further than
    /// [`Right`](Self::Right) goes.
    HalfRight(usize),
    /// Back to the first column, when lines are cut, as vim's `zs` with the cursor at the start
    /// of the lines.
    ScrollStart,
    /// The end of the longest line on screen to the right edge, when lines are cut, as vim's
    /// `ze`.
    ScrollEnd,
    /// Wraps long lines, or cuts them.
    ToggleWrap,
}

/// How the viewer looks: the colors of the app's theme, and the characters it draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Styles {
    /// The title line: the path and the position.
    pub header: Style,
    /// The style of the mark where the title is cut, over `header`.
    pub header_mark: Style,
    /// The text, the screen behind it, and the scroll bar; dimmed, the line that ends the text.
    pub text: Style,
    /// The scroll bar.
    pub bar: BarGlyphs,
    /// The line across the screen after the end of the text, a character of one cell.
    pub line: &'static str,
    /// The style of the marks of text that goes on past the screen.
    pub mark: Style,
    /// In place of what cannot be shown as it is: invalid UTF-8, and characters a terminal
    /// would act on; one cell.
    pub hidden: &'static str,
    /// The style of [`Styles::hidden`], over `text`.
    pub hidden_style: Style,
    /// The marks: [`Marks::before`] over the first cell of a cut line with text hidden on the
    /// left, as vim's 'listchars' `precedes`; [`Marks::after`] over the last cell of one that
    /// goes on to the right, as `extends`, and in front of each row of a wrapped line after its
    /// first, as vim's 'showbreak'; and where the title is cut.
    pub marks: Marks,
}

/// How much of a file the viewer reads at once, and how much it holds, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    /// A part read at once.
    pub part: u64,
    /// The parts held; at least two.
    pub window: u64,
}

/// What came of a part the viewer took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// The part is the viewer's.
    Read,
    /// The file changed since the parts before were read: the viewer forgot them, and reads it
    /// again from its start to the line it was at.
    Changed,
}

#[derive(Debug)]
enum State {
    Loading,
    Text {
        /// The lines of the held parts: terminal-safe, tabs expanded.
        lines: Vec<String>,
    },
}

/// Where the viewer goes once it has read the part that has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Goal {
    Start,
    Line(usize),
    Byte(u64),
}

/// The way the viewer was scrolled past the held parts: it reads the next part that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pull {
    Forward,
    Back,
}

/// A file on screen. Its position is a line and a row within it, so that wrapping takes
/// only the lines in view, whatever the size of the file.
///
/// It reads the file in parts and holds a few of them: the app reads the part [`want`] asks
/// for and gives it to [`take`]. Scrolled past the held parts, it asks for the next one that
/// way; told to go to a line, for the one with it.
///
/// [`want`]: Self::want
/// [`take`]: Self::take
#[derive(Debug)]
pub struct Viewer {
    title: String,
    sizes: Sizes,
    state: State,
    parts: Parts,
    /// The file as it was when its first part was read.
    stamp: Option<Stamp>,
    goal: Option<Goal>,
    pull: Option<Pull>,
    /// The first held line on screen, and its first row there. The line after the last is the
    /// one that marks the end of the text.
    top: (usize, usize),
    /// Cells cut off the left of each line, when lines are not wrapped.
    column: usize,
    wrap: bool,
    /// Rows and columns of text at the last render.
    page: usize,
    width: usize,
    /// The scroll bar of the last render, if the text did not fit.
    bar: Option<Bar>,
    /// The rows of the whole text wrapped, and the width they were counted in.
    measured: Option<(usize, usize)>,
    /// Where the rows of the long lines wrapped lately start, by line, and the width they were
    /// wrapped in: moving a row on in a line of megabytes must not wrap it all again.
    wrapped: RefCell<(usize, Wrapped)>,
}

impl Viewer {
    /// The viewer of the file `title`, while it loads. The title is shown as it is, so it
    /// must be terminal-safe.
    pub fn new(title: String, sizes: Sizes) -> Self {
        Self {
            title,
            sizes,
            state: State::Loading,
            parts: Parts::default(),
            stamp: None,
            goal: None,
            pull: None,
            top: (0, 0),
            column: 0,
            wrap: true,
            page: 1,
            width: 80,
            bar: None,
            measured: None,
            wrapped: RefCell::default(),
        }
    }

    /// The scroll bar of the last render, for the mouse; `None` when the text fit.
    pub fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// The part of the file to read next, if any: the first one while it loads, then the one
    /// a scroll past the held parts or a line to go to needs.
    pub fn want(&self) -> Option<Want> {
        let length = self.sizes.part.max(1);
        if self.loading() {
            return Some(Want {
                offset: 0,
                length,
                new: true,
            });
        }
        if let Some(goal) = self.goal {
            let index = match goal {
                Goal::Start => Some(0),
                Goal::Line(line) => self.parts.part_of_line(line),
                Goal::Byte(byte) => self.parts.part_of_byte(byte),
            };
            return match index {
                Some(index) if self.parts.held().contains(&index) => None,
                Some(index) => self.parts.again(index),
                None => self.parts.new_part(length).or_else(|| {
                    let last = self.parts.known().len().checked_sub(1)?;
                    if self.parts.held().contains(&last) {
                        None
                    } else {
                        self.parts.again(last)
                    }
                }),
            };
        }
        match self.pull? {
            Pull::Forward => self.parts.next(length),
            Pull::Back => self.parts.previous(),
        }
    }

    /// Takes a part read for [`want`](Self::want). A part no longer wanted is kept if it fits.
    pub fn take(&mut self, part: Part) -> Taken {
        let known = self.parts.at(part.want.offset);
        let same = part.want.new
            || part.stopped
            || known.is_some_and(|index| self.parts.known()[index].len == part.len);
        if !same || self.stamp.is_some_and(|stamp| stamp != part.stamp) {
            self.reset();
            return Taken::Changed;
        }
        self.stamp = Some(part.stamp);
        self.parts.set_size(part.stamp.size);
        if part.want.new {
            self.take_new(part);
        } else if let Some(index) = known
            && !part.stopped
            && !self.parts.held().contains(&index)
        {
            self.place(index, part.lines);
        }
        self.settle();
        Taken::Read
    }

    fn take_new(&mut self, part: Part) {
        let first = self.parts.known().is_empty();
        if part.want.offset != self.parts.frontier() || self.parts.all_known() {
            return;
        }
        if part.is_empty() && !(part.end && first) {
            if part.end {
                self.parts.ends();
            }
            if self.loading() {
                // Stopped before a line came.
                self.state = State::Text { lines: Vec::new() };
            }
            return;
        }
        let known = Known {
            offset: part.want.offset,
            len: part.len,
            lines: part.lines.len(),
        };
        let index = self.parts.push(known, part.end);
        // The parts on the way to a line are only counted.
        if self.loading() || self.serves_goal(index) {
            self.place(index, part.lines);
        }
    }

    /// Stops wanting parts until the next move.
    pub fn stop(&mut self) {
        self.goal = None;
        self.pull = None;
    }

    /// Forgets the parts of the file, which changed, to read it again up to the line on top.
    fn reset(&mut self) {
        if self.goal.is_none() && !self.loading() {
            self.goal = Some(Goal::Line(self.parts.first_line() + self.top.0));
        }
        self.parts = Parts::default();
        self.stamp = None;
        self.state = State::Loading;
        self.pull = None;
        self.top = (0, 0);
        self.forget_rows();
    }

    fn forget_rows(&mut self) {
        self.measured = None;
        self.wrapped.borrow_mut().1.clear();
    }

    /// Whether the file ends with the held parts.
    fn at_file_end(&self) -> bool {
        self.parts.all_known() && self.parts.held().end == self.parts.known().len()
    }

    /// Whether known part `index` has the goal, or is the last when the file ends before it.
    fn serves_goal(&self, index: usize) -> bool {
        let last = self.parts.all_known() && index + 1 == self.parts.known().len();
        match self.goal {
            None => true,
            Some(Goal::Start) => index == 0,
            Some(Goal::Line(line)) => self.parts.part_of_line(line).map_or(last, |at| at == index),
            Some(Goal::Byte(byte)) => self.parts.part_of_byte(byte).map_or(last, |at| at == index),
        }
    }

    /// Holds the lines of known part `index`: after the held parts, before them, or in their
    /// place.
    fn place(&mut self, index: usize, new: Vec<String>) {
        let held = self.parts.held();
        match &mut self.state {
            State::Text { lines } if index == held.end => {
                lines.extend(new);
                self.parts.set_held(held.start..index + 1);
            }
            State::Text { lines } if index + 1 == held.start => {
                self.top.0 += new.len();
                lines.splice(0..0, new);
                self.parts.set_held(index..held.end);
            }
            _ => {
                self.state = State::Text { lines: new };
                self.top = (0, 0);
                self.parts.set_held(index..index + 1);
            }
        }
        self.forget_rows();
    }

    /// After a part was taken: goes to the goal if it is held now, lets go of parts beyond
    /// the window, and stops pulling once the screen is filled.
    fn settle(&mut self) {
        self.reach();
        self.trim();
        self.pull = self.pull.filter(|pull| match pull {
            Pull::Forward => self.bottom() == self.end() && self.parts.more_after(),
            Pull::Back => self.top == (0, 0) && self.parts.more_before(),
        });
    }

    /// Goes to the goal, if the part with it is held.
    fn reach(&mut self) {
        let Some(goal) = self.goal else {
            return;
        };
        let held = self.parts.held();
        let line = match goal {
            Goal::Start => (held.start == 0).then_some(0),
            Goal::Line(line) => match self.parts.part_of_line(line) {
                Some(index) if held.contains(&index) => Some(line),
                None if self.at_file_end() => Some(line),
                _ => None,
            },
            Goal::Byte(byte) => match self.parts.part_of_byte(byte) {
                Some(index) if held.contains(&index) => Some(self.parts.line_at(index, byte)),
                None if self.at_file_end() => Some(usize::MAX),
                _ => None,
            },
        };
        if let Some(line) = line {
            let first = self.parts.first_line();
            self.top = (line.saturating_sub(first), 0).min(self.last_top());
            if goal == Goal::Start {
                self.column = 0;
            }
            self.goal = None;
        }
    }

    /// Lets go of held parts while they take more than the window: those off screen, on the
    /// side away from it.
    fn trim(&mut self) {
        let window = self.sizes.window.max(self.sizes.part.saturating_mul(2));
        loop {
            let held = self.parts.held();
            if held.len() < 2 || self.parts.held_bytes() <= window {
                return;
            }
            let front = self.parts.known()[held.start].lines;
            let back = self.parts.known()[held.end - 1].lines;
            let bottom = self.bottom().0;
            let State::Text { lines } = &mut self.state else {
                return;
            };
            if self.top.0 >= front {
                lines.drain(..front);
                self.top.0 -= front;
                self.parts.set_held(held.start + 1..held.end);
            } else if bottom + back < lines.len() {
                lines.truncate(lines.len() - back);
                self.parts.set_held(held.start..held.end - 1);
            } else {
                return;
            }
            self.forget_rows();
        }
    }

    /// Goes to `goal`, at once if the part with it is held, else once it is read.
    fn go_to(&mut self, goal: Goal) {
        self.pull = None;
        self.goal = Some(goal);
        if !self.loading() {
            self.reach();
        }
    }

    /// Scrolls, or switches wrapping.
    pub fn handle(&mut self, command: Command) {
        match command {
            Command::Scroll(motion) => self.scroll(motion),
            Command::Top(line) => self.go_to(Goal::Line(line)),
            Command::Bar(position) if self.bar_in_lines() => {
                self.top = (position, 0).min(self.last_top());
            }
            Command::Bar(position) => {
                self.go_to(Goal::Byte(u64::try_from(position).unwrap_or(u64::MAX)));
            }
            Command::Left => self.column = self.column.saturating_sub(1),
            Command::ToggleWrap => {
                self.wrap = !self.wrap;
                self.top.1 = 0;
                self.column = 0;
            }
            // Wrapped lines hold still sideways, as in vim.
            _ if self.wrap => {}
            Command::Right => self.go_right(1),
            Command::ScrollLeft(count) => self.column = self.column.saturating_sub(count),
            Command::ScrollRight(count) => self.go_right(count),
            Command::HalfLeft(count) => {
                self.column = self
                    .column
                    .saturating_sub(count.saturating_mul(self.width / 2));
            }
            Command::HalfRight(count) => self.go_right(count.saturating_mul(self.width / 2)),
            Command::ScrollStart => self.column = 0,
            Command::ScrollEnd => self.column = self.last_column(),
        }
    }

    fn lines(&self) -> &[String] {
        match &self.state {
            State::Text { lines, .. } => lines,
            State::Loading => &[],
        }
    }

    /// The cells of the longest line on screen at the last render, lines cut.
    fn longest_on_screen(&self) -> usize {
        let lines = self.lines().iter().skip(self.top.0).take(self.page);
        lines.map(|line| width(line)).max().unwrap_or(0)
    }

    /// How far the lines can scroll left: until the end of the longest line on screen is at the
    /// right edge, as `ze` puts it, so that scrolling never leaves the screen empty.
    fn last_column(&self) -> usize {
        self.longest_on_screen().saturating_sub(self.width)
    }

    /// `count` columns right, as vim's `zl`, but no further than [`Self::last_column`].
    fn go_right(&mut self, count: usize) {
        self.column = self.column.saturating_add(count).min(self.last_column());
    }

    /// The rows line `index` takes on screen.
    fn rows_of(&self, index: usize) -> usize {
        self.row_starts(index).map_or(1, |starts| starts.len())
    }

    /// Where the rows of line `index` start, wrapped; `None` for a line of one row however it
    /// wraps. A line wraps once for each width, while it is among the last ones wrapped.
    fn row_starts(&self, index: usize) -> Option<Rc<[usize]>> {
        let line = self.lines().get(index)?;
        // No cell takes less than a byte.
        if !self.wrap || line.len() <= self.width {
            return None;
        }
        let mut wrapped = self.wrapped.borrow_mut();
        let (width, lines) = &mut *wrapped;
        if *width != self.width || lines.len() >= WRAPPED_LINES {
            *width = self.width;
            lines.clear();
        }
        let starts = lines
            .entry(index)
            .or_insert_with(|| row_starts(line, self.width, continued(self.width)).into());
        Some(Rc::clone(starts))
    }

    /// The position `count` rows after `from`, or the end of the text.
    fn after(&self, (mut line, mut row): (usize, usize), count: usize) -> (usize, usize) {
        let lines = self.lines().len();
        for _ in 0..count {
            if row + 1 < self.rows_of(line) {
                row += 1;
            } else if line < lines {
                (line, row) = (line + 1, 0);
            } else {
                break;
            }
        }
        (line, row)
    }

    /// The position `count` rows before `from`, or the first one.
    fn before(&self, (mut line, mut row): (usize, usize), count: usize) -> (usize, usize) {
        for _ in 0..count {
            if row > 0 {
                row -= 1;
            } else if line > 0 {
                line -= 1;
                row = self.rows_of(line) - 1;
            } else {
                break;
            }
        }
        (line, row)
    }

    /// The position of the line that marks the end of the text, after its last line.
    fn end(&self) -> (usize, usize) {
        (self.lines().len(), 0)
    }

    /// The position of the last row of the text, before the line that marks its end.
    fn last_row(&self) -> (usize, usize) {
        match self.lines().len().checked_sub(1) {
            Some(last) => (last, self.rows_of(last) - 1),
            None => (0, 0),
        }
    }

    /// Counts the rows of the whole text in the width of this render, if it wraps and they were
    /// not counted in it yet: once for each width, not for each frame.
    fn measure(&mut self) {
        let width = self.width;
        if !self.wrap || self.measured.is_some_and(|(_, counted)| counted == width) {
            return;
        }
        let rows = rows_of_lines(self.lines(), width, continued(width));
        self.measured = Some((rows, width));
    }

    /// The rows of the whole text, as [`measure`](Self::measure) counted them.
    fn total_rows(&self) -> usize {
        match self.measured {
            Some((rows, width)) if self.wrap && width == self.width => rows,
            _ => self.lines().len(),
        }
    }

    /// The top that shows the end of the text at the bottom of the screen.
    fn last_top(&self) -> (usize, usize) {
        self.before(self.end(), self.page.saturating_sub(1))
    }

    /// The last position on screen.
    fn bottom(&self) -> (usize, usize) {
        self.after(self.top, self.page.saturating_sub(1))
    }

    /// Scrolls within the held parts; past them, asks for the next part that way.
    fn scroll(&mut self, motion: Motion) {
        self.goal = None;
        self.pull = None;
        let step = motion.step(self.page);
        self.top = match step {
            Step::Back(count) => self.before(self.top, count),
            Step::On(count) => self
                .after(self.top, count)
                .min(self.last_top().max(self.top)),
            Step::Start => {
                self.column = 0;
                (0, 0)
            }
            Step::End => self.last_top(),
        };
        match step {
            Step::Back(_) if self.top == (0, 0) && self.parts.more_before() => {
                self.pull = Some(Pull::Back);
            }
            Step::Start if self.parts.more_before() => self.goal = Some(Goal::Start),
            Step::On(_) | Step::End if self.bottom() == self.end() && self.parts.more_after() => {
                self.pull = Some(Pull::Forward);
            }
            _ => {}
        }
    }

    /// Whether the scroll bar goes in lines, which takes counting the rows of every line held:
    /// when the whole file is held and is no more than a part, else in bytes.
    fn bar_in_lines(&self) -> bool {
        self.parts.whole() && self.parts.held_bytes() <= self.sizes.part
    }

    /// Whether it waits for the file.
    pub fn loading(&self) -> bool {
        matches!(self.state, State::Loading)
    }

    /// Draws the viewer over `area`: a title line, `icon` in front of the path, then the text.
    /// While it waits for the file, the text is empty; the app turns the frames of its spinner
    /// as `icon`.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, styles: &Styles, icon: char) {
        self.bar = None;
        if area.height < 2 || area.width == 0 {
            return;
        }
        self.width = usize::from(area.width);
        self.page = usize::from(area.height - 1);
        // The scroll bar takes the last column when the text, without the line that ends it,
        // does not fit, wrapped narrower even less, or is not all of the file.
        let bar = area.width > 1
            && (!self.parts.whole() || self.after((0, 0), self.page - 1) < self.last_row());
        if bar {
            self.width -= 1;
        }
        // The width may have changed since the position was taken.
        self.top.1 = self.top.1.min(self.rows_of(self.top.0).saturating_sub(1));
        // Other lines on screen, or a wider screen, may need less of the scroll.
        self.column = self.column.min(self.last_column());
        let row = |index: u16| Rect::new(area.x, area.y + index, area.width, 1);
        // Rows of text leave the scroll bar's column alone, which the end line's style would dim.
        let text_width = u16::try_from(self.width).unwrap_or(area.width);
        let text_row = |index: u16| Rect::new(area.x, area.y + index, text_width, 1);
        frame.render_widget(self.header(usize::from(area.width), icon, styles), row(0));
        let text_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        frame.buffer_mut().set_style(text_area, styles.text);
        if self.loading() {
            return;
        }
        if bar {
            if self.bar_in_lines() {
                self.measure();
            }
            let column = Rect::new(area.right() - 1, text_area.y, 1, text_area.height);
            self.bar = scrollbar::render(frame, column, self.thumb(), styles.text, styles.bar);
        }
        let mut position = self.top;
        for screen_row in 0..self.page {
            let y = u16::try_from(screen_row + 1).unwrap_or(u16::MAX);
            let Some(line) = self.lines().get(position.0) else {
                let end = noc_text::rule(&self.end_text(), self.width, styles.line);
                let dim = styles.text.add_modifier(Modifier::DIM);
                let mark = styles.text.patch(styles.mark);
                let line = fitted_line(end, (dim, mark), styles.marks);
                frame.render_widget(line, text_row(y));
                break;
            };
            let text = if self.wrap {
                let text = match self.row_starts(position.0) {
                    Some(starts) => row_of(line, &starts, position.1).to_owned(),
                    None if position.1 == 0 => line.clone(),
                    None => String::new(),
                };
                let mut spans = Vec::new();
                if position.1 > 0 && breaks(self.width) {
                    spans.push(Span::styled(styles.marks.after, styles.mark));
                }
                push_text(&mut spans, text, styles);
                Line::from(spans)
            } else {
                self.cut_line(line, styles)
            };
            frame.render_widget(text, text_row(y));
            let next = self.after(position, 1);
            if next == position {
                break;
            }
            position = next;
        }
    }

    /// The cells of `line` that show from the column on, with the marks of `styles` where text
    /// goes on past either edge: a mark takes the place of the cell of text under it.
    fn cut_line(&self, line: &str, styles: &Styles) -> Line<'static> {
        let (marks, mark) = (styles.marks, styles.mark);
        let extends = wider_than(line, self.column + self.width);
        let precedes = self.column > 0 && wider_than(line, 0) && (self.width > 1 || !extends);
        let taken = usize::from(precedes) + usize::from(extends);
        let room = self.width.saturating_sub(taken);
        let mut spans = Vec::with_capacity(3);
        if precedes {
            spans.push(Span::styled(marks.before, mark));
        }
        let mut text = cut(line, self.column + usize::from(precedes), room);
        if extends {
            // A wide character that does not fit leaves a gap before the mark at the edge.
            let gap = room.saturating_sub(width(&text));
            text.extend(std::iter::repeat_n(' ', gap));
        }
        push_text(&mut spans, text, styles);
        if extends {
            spans.push(Span::styled(marks.after, mark));
        }
        Line::from(spans)
    }

    /// The line that marks the end of the text.
    fn end_text(&self) -> String {
        if self.at_file_end() {
            fl!("viewer-end")
        } else {
            fl!("viewer-end-more")
        }
    }

    /// For the scroll bar, without the line that ends the text, so that the bar reaches its
    /// bottom with the last line as the percentage reaches 100: how many, how many show, and
    /// the first on screen. The thumb goes in lines of the file, from the first to the one on
    /// top when the last shows at the bottom, where it holds still, the end line shown too or
    /// not. Its length is the share of the rows of the whole text that show, so that it keeps
    /// it however many lines wrapped lines leave on screen.
    fn thumb(&self) -> (usize, usize, usize) {
        if !self.bar_in_lines() {
            return self.thumb_in_bytes();
        }
        let last_row = self.last_row();
        let last_top = self.before(last_row, self.page.saturating_sub(1)).0;
        let top = if self.bottom() >= last_row {
            last_top
        } else {
            self.top.0.min(last_top)
        };
        // The page, in lines, that gives the thumb `page` rows of the whole text over the
        // `last_top` lines it moves across: as many lines as rows, where none wraps.
        let others = self.total_rows().saturating_sub(self.page);
        let page = (self.page * last_top + others / 2)
            .checked_div(others)
            .unwrap_or(0)
            .max(1);
        (last_top + page, page, top)
    }

    /// The thumb in bytes of the file, as if the lines of each part were of one length: while
    /// only parts of the file are held, or more than a part.
    fn thumb_in_bytes(&self) -> (usize, usize, usize) {
        let lines = u64::try_from(self.lines().len().max(1)).unwrap_or(u64::MAX);
        let rows = u64::try_from(self.page).unwrap_or(u64::MAX);
        let page = (self.parts.held_bytes() * rows / lines).max(1);
        let total = self.parts.extent().max(page);
        let top = self.parts.byte_at(self.top.0).min(total - page);
        let units = |bytes: u64| usize::try_from(bytes).unwrap_or(usize::MAX);
        (units(total), units(page), units(top))
    }

    /// `icon` and the path on the left, and where the screen is on the right, `width` cells wide
    /// in all: its first line, the lines, and how far the last one on screen is, in percent, or
    /// that the end shows. The lines of the file are a question mark until it is all read; the
    /// percentage is in bytes until all of it is held. The path never moves, whatever `icon` is.
    fn header(&self, width: usize, icon: char, styles: &Styles) -> Line<'static> {
        let held = self.lines().len();
        let mut position = String::new();
        if !self.loading() {
            let first = self.parts.first_line();
            let line = first + (self.top.0 + 1).min(held);
            let bottom = self.bottom();
            let percent = if self.parts.whole() {
                ((bottom.0 + 1) * 100).checked_div(held).unwrap_or(100)
            } else {
                let byte = self.parts.byte_at(bottom.0 + 1);
                let percent = (byte * 100).checked_div(self.parts.extent()).unwrap_or(100);
                usize::try_from(percent.min(100)).unwrap_or(100)
            };
            position = match self.parts.lines() {
                Some(lines) if bottom == self.end() && self.at_file_end() => {
                    fl!("viewer-position-end", line = line, lines = lines)
                }
                Some(lines) => fl!(
                    "viewer-position",
                    line = line,
                    lines = lines,
                    percent = percent
                ),
                None => fl!("viewer-position-part", line = line, percent = percent),
            };
        }
        let position = format!(" {position} ");
        let lead = format!(" {icon} ");
        let room = width.saturating_sub(noc_text::width(&position) + noc_text::width(&lead));
        let title = noc_text::fit_cut(&self.title, room, Align::Left, Cut::Middle);
        let mark = styles.header.patch(styles.header_mark);
        let mut line = fitted_line(title, (styles.header, mark), styles.marks).style(styles.header);
        line.spans.insert(0, Span::styled(lead, styles.header));
        line.push_span(Span::styled(position, styles.header));
        line
    }
}

/// `fitted` in `style`, with its mark of `marks`, if it was cut, in `mark`.
fn fitted_line(fitted: Fitted, (style, mark): (Style, Style), marks: Marks) -> Line<'static> {
    let mut spans = vec![Span::styled(fitted.head.clone(), style)];
    if fitted.cut.is_some() {
        spans.push(Span::styled(fitted.mark(marks), mark));
    }
    spans.push(Span::styled(fitted.tail, style));
    Line::from(spans)
}

/// Pushes `text` to `spans`, each U+FFFD in it as [`Styles::hidden`].
fn push_text(spans: &mut Vec<Span<'static>>, text: String, styles: &Styles) {
    if !text.contains(char::REPLACEMENT_CHARACTER) {
        spans.push(Span::raw(text));
        return;
    }
    for (index, piece) in text.split(char::REPLACEMENT_CHARACTER).enumerate() {
        if index > 0 {
            spans.push(Span::styled(styles.hidden, styles.hidden_style));
        }
        if !piece.is_empty() {
            spans.push(Span::raw(piece.to_owned()));
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    const STYLES: Styles = Styles {
        header: Style::new(),
        header_mark: Style::new(),
        text: Style::new(),
        bar: BarGlyphs {
            up: "▲",
            down: "▼",
            track: "░",
            thumb: "█",
        },
        line: "─",
        mark: Style::new(),
        marks: Marks {
            before: "«",
            after: "»",
        },
        hidden: "·",
        hidden_style: Style::new(),
    };

    const SIZES: Sizes = Sizes {
        part: 1 << 20,
        window: 1 << 22,
    };

    const STAMP: Stamp = Stamp {
        size: None,
        modified: None,
    };

    /// A part of `bytes` at `offset`, new, the last one of the file if `end`.
    fn part(offset: u64, bytes: &[u8], end: bool) -> Part {
        let want = Want {
            offset,
            length: SIZES.part,
            new: true,
        };
        Part::new(want, bytes, (end, false), STAMP)
    }

    impl Viewer {
        /// Shows the whole file of `bytes`, in the same width and wrapping.
        fn show(&mut self, bytes: &[u8]) {
            let (wrap, page, width) = (self.wrap, self.page, self.width);
            *self = Viewer::new(self.title.clone(), self.sizes);
            (self.wrap, self.page, self.width) = (wrap, page, width);
            assert_eq!(self.take(part(0, bytes, true)), Taken::Read);
        }
    }

    fn draw(viewer: &mut Viewer, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| viewer.render(frame, frame.area(), &STYLES, '*'))
            .unwrap();
        terminal.backend().to_string()
    }

    fn numbered(count: usize) -> Vec<u8> {
        use std::fmt::Write as _;
        let mut text = String::new();
        for n in 1..=count {
            writeln!(text, "line {n}").unwrap();
        }
        text.into_bytes()
    }

    #[test]
    fn scrolls_within_the_text_and_stops_at_its_end() {
        let mut viewer = Viewer::new("/srv/log".to_owned(), SIZES);
        assert!(viewer.loading());
        let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
        terminal
            .draw(|frame| viewer.render(frame, frame.area(), &STYLES, '|'))
            .unwrap();
        let text = terminal.backend().to_string();
        let rows: Vec<&str> = text
            .lines()
            .map(|row| row.trim_matches('"').trim_end())
            .collect();
        assert_eq!(rows[..3], [" | /srv/log", "", ""], "{text}");
        viewer.show(&numbered(20));
        assert!(!viewer.loading());
        let mut press = |motion| {
            viewer.handle(Command::Scroll(motion));
            draw(&mut viewer, 30, 6)
        };
        let top = press(Motion::Home);
        assert!(top.contains("/srv/log"), "{top}");
        assert!(top.contains("1/20 25%"), "{top}");
        assert!(top.contains("line 5") && !top.contains("line 6"), "{top}");
        // Ctrl+f keeps two of the five rows on screen, as vim's; Space turns the whole page, as
        // less.
        assert!(press(Motion::PageDown).contains("4/20"));
        assert!(press(Motion::WindowDown).contains("9/20"));
        press(Motion::WindowUp);
        assert!(press(Motion::PageUp).contains("1/20"));
        press(Motion::WindowDown);
        // Half of the five rows, rounded down, as vim's 'scroll'.
        assert!(press(Motion::HalfPageDown).contains("8/20"));
        assert!(press(Motion::HalfPageUp).contains("6/20"));
        let end = press(Motion::End);
        assert!(end.contains("17/20 End"), "{end}");
        assert!(end.contains("line 20") && end.contains("─ EOF ─"), "{end}");
        for motion in [
            Motion::Down,
            Motion::PageDown,
            Motion::WindowDown,
            Motion::HalfPageDown,
        ] {
            assert_eq!(press(motion), end, "{motion:?} stops at the end");
        }
        assert!(press(Motion::Home).contains("1/20"));
    }

    #[test]
    fn long_lines_wrap_again_in_a_new_width_and_for_a_new_file() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show(&[b'a'; 50]);
        let narrow = draw(&mut viewer, 10, 8);
        assert!(
            narrow.contains(&format!("\"{}\"", "a".repeat(10))),
            "{narrow}"
        );
        let wide = draw(&mut viewer, 30, 8);
        assert!(wide.contains(&format!("\"{}\"", "a".repeat(30))), "{wide}");
        viewer.show(&[b'b'; 50]);
        let other = draw(&mut viewer, 30, 8);
        assert!(
            other.contains(&"b".repeat(30)) && !other.contains('a'),
            "{other}"
        );
    }

    #[test]
    fn the_thumb_keeps_its_length_through_wrapped_lines_of_every_length() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        // Short lines first, then lines of three rows each.
        let text = format!(
            "{}{}",
            "x\n".repeat(15),
            format!("{}\n", "x".repeat(50)).repeat(15)
        );
        viewer.show(text.as_bytes());
        let bar = |screen: &str| -> String {
            let rows = screen.lines().skip(1);
            rows.filter_map(|line| line.trim_end_matches('"').chars().last())
                .collect()
        };
        let mut lengths = std::collections::BTreeSet::new();
        let mut screens = Vec::new();
        for _ in 0..60 {
            let screen = bar(&draw(&mut viewer, 20, 24));
            lengths.insert(screen.matches('█').count());
            screens.push(screen);
            viewer.handle(Command::Scroll(Motion::Down));
        }
        assert_eq!(lengths.len(), 1, "{lengths:?}");
        let (first, last) = (&screens[0], &screens[screens.len() - 1]);
        assert!(
            first.starts_with("▲█") && last.ends_with("█▼"),
            "{first} {last}"
        );
    }

    #[test]
    fn a_scroll_bar_shows_where_the_text_does_not_fit() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show(&numbered(20));
        let column = |screen: &str, row: usize| {
            let line = screen.lines().nth(row).unwrap_or_default();
            line.trim_end_matches('"').chars().last()
        };
        let top = draw(&mut viewer, 20, 6);
        assert_eq!(column(&top, 1), Some('▲'), "{top}");
        assert_eq!(column(&top, 2), Some('█'), "{top}");
        assert_eq!(column(&top, 4), Some('░'), "{top}");
        assert_eq!(column(&top, 5), Some('▼'), "{top}");
        viewer.handle(Command::Scroll(Motion::End));
        let end = draw(&mut viewer, 20, 6);
        assert_eq!(column(&end, 2), Some('░'), "{end}");
        assert_eq!(column(&end, 4), Some('█'), "{end}");
        viewer.handle(Command::Scroll(Motion::Up));
        let last = draw(&mut viewer, 20, 6);
        assert!(last.contains("100%") && !last.contains("EOF"), "{last}");
        let bar = |screen: &str| (1..6).map(|row| column(screen, row)).collect::<Vec<_>>();
        assert_eq!(
            bar(&last),
            bar(&end),
            "at the bottom with the last line, and still, the end line or not: {last}"
        );
        viewer.handle(Command::Scroll(Motion::Up));
        let before = draw(&mut viewer, 20, 6);
        assert_eq!(column(&before, 4), Some('░'), "{before}");
        // A long thumb keeps its length too, though the end line takes a row of the text.
        viewer.show(&numbered(10));
        let tall_bar = |viewer: &mut Viewer| {
            let screen = draw(viewer, 20, 9);
            (1..9).map(|row| column(&screen, row)).collect::<Vec<_>>()
        };
        tall_bar(&mut viewer);
        viewer.handle(Command::Scroll(Motion::End));
        let tall_end = tall_bar(&mut viewer);
        viewer.handle(Command::Scroll(Motion::Up));
        assert_eq!(tall_bar(&mut viewer), tall_end);

        viewer.show(&numbered(5));
        let filled = draw(&mut viewer, 20, 6);
        assert!(
            !filled.contains('█'),
            "the end line alone needs no bar: {filled}"
        );
        viewer.show(&numbered(3));
        let short = draw(&mut viewer, 20, 6);
        assert!(!short.contains('█') && !short.contains('░'), "{short}");
        assert!(short.contains("1/3 End"), "{short}");
        let end_row = short.lines().nth(4).unwrap_or_default();
        assert!(end_row.contains("─ EOF ─"), "right after the text: {short}");
    }

    #[test]
    fn the_scroll_bar_is_kept_for_the_mouse_and_its_thumb_puts_a_line_at_the_top() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show(&numbered(20));
        draw(&mut viewer, 20, 12);
        let bar = viewer.bar().unwrap();
        assert_eq!(
            bar.part(ratatui::layout::Position::new(19, 1)),
            Some(scrollbar::Part::Up)
        );
        viewer.handle(Command::Bar(bar.dragged(0, 5)));
        assert!(draw(&mut viewer, 20, 12).contains("7/20"));
        viewer.handle(Command::Top(99));
        let end = draw(&mut viewer, 20, 12);
        assert!(end.contains("11/20 End"), "no further than the end: {end}");
        viewer.show(&numbered(3));
        draw(&mut viewer, 20, 12);
        assert_eq!(viewer.bar(), None, "the text fits");
    }

    #[test]
    fn the_end_line_leaves_the_scroll_bar_undimmed() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show(&numbered(20));
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        let mut render = |viewer: &mut Viewer| {
            terminal
                .draw(|frame| viewer.render(frame, frame.area(), &STYLES, '*'))
                .unwrap()
                .buffer
                .clone()
        };
        render(&mut viewer);
        viewer.handle(Command::Scroll(Motion::End));
        let screen = render(&mut viewer);
        let end_row = 5;
        assert_eq!(screen[(19, end_row)].symbol(), "▼");
        assert!(!screen[(19, end_row)].modifier.contains(Modifier::DIM));
        assert_eq!(screen[(19, end_row - 1)].symbol(), "█");
        assert!(screen[(8, end_row)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn wraps_long_lines_and_scrolls_sideways_without() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        let long: String = ('a'..='z').cycle().take(50).collect();
        viewer.show(format!("{long}\nshort\n").as_bytes());
        let wrapped = draw(&mut viewer, 20, 6);
        insta::assert_snapshot!(wrapped);
        // Three rows of text, twenty cells wide beside the scroll bar: Down goes into the rows
        // of the long line.
        let small = |viewer: &mut Viewer| draw(viewer, 21, 4);
        small(&mut viewer);
        viewer.handle(Command::Scroll(Motion::Down));
        let text = small(&mut viewer);
        assert!(text.contains("short"), "{text}");
        assert!(text.contains("\"»uvwxyzabcdefghijklm "), "{text}");
        let first_row = |text: String| {
            text.contains("\"abcdefghijklmnopqrst") && !text.contains("abcdefghijklmnopqrstu")
        };
        viewer.handle(Command::Scroll(Motion::Up));
        assert!(first_row(small(&mut viewer)));

        viewer.handle(Command::Right);
        assert!(
            first_row(small(&mut viewer)),
            "wrapped lines do not scroll sideways"
        );
        viewer.handle(Command::ToggleWrap);
        viewer.handle(Command::Scroll(Motion::Home));
        viewer.handle(Command::Right);
        viewer.handle(Command::Right);
        let cut = draw(&mut viewer, 20, 6);
        assert!(cut.contains("\"«defghijklmnopqrstu»\""), "{cut}");
        assert!(cut.contains("\"«rt "), "{cut}");
    }

    /// The screen, and whether each of its cells is drawn as a mark.
    fn draw_marks(viewer: &mut Viewer, width: u16, height: u16) -> (String, Vec<String>) {
        let mark = Style::new().add_modifier(Modifier::UNDERLINED);
        let styles = Styles {
            header_mark: mark,
            mark,
            hidden_style: mark,
            ..STYLES
        };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let buffer = terminal
            .draw(|frame| viewer.render(frame, frame.area(), &styles, '*'))
            .unwrap()
            .buffer
            .clone();
        let marks = (0..height)
            .map(|y| {
                (0..width)
                    .filter(|&x| buffer[(x, y)].modifier.contains(Modifier::UNDERLINED))
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect()
            })
            .collect();
        (terminal.backend().to_string(), marks)
    }

    #[test]
    fn cut_lines_mark_text_hidden_past_either_edge() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show("abcdefghijklmnopqrstuvwxyz\nshort\n\nabcdefghij\nabcdefgh文x\n".as_bytes());
        viewer.handle(Command::ToggleWrap);
        let (screen, marks) = draw_marks(&mut viewer, 10, 8);
        let rows: Vec<&str> = screen.lines().collect();
        assert_eq!(rows[1], "\"abcdefghi»\"", "{screen}");
        assert_eq!(rows[2], "\"short     \"", "{screen}");
        assert_eq!(rows[4], "\"abcdefghij\"", "a line that fits: {screen}");
        assert_eq!(
            rows[5], "\"abcdefgh »\"",
            "a wide character that does not fit leaves a gap: {screen}"
        );
        assert_eq!(marks[1], "»");
        assert_eq!(marks[2..=4], ["", "", ""]);
        assert_eq!(marks[5], "»");
        assert_eq!(marks[6], "", "the end line has no marks");

        for _ in 0..3 {
            viewer.handle(Command::Right);
        }
        let (screen, marks) = draw_marks(&mut viewer, 10, 8);
        let rows: Vec<&str> = screen.lines().collect();
        assert_eq!(rows[1], "\"«efghijkl»\"", "{screen}");
        assert_eq!(rows[2], "\"«t        \"", "{screen}");
        assert_eq!(
            rows[3], "\"          \"",
            "an empty line hides nothing: {screen}"
        );
        assert_eq!(rows[4], "\"«efghij   \"", "{screen}");
        assert!(rows[6].contains("EOF") && marks[6].is_empty(), "{screen}");
        assert_eq!(marks[1], "«»");
        assert_eq!(marks[2], "«");
        assert_eq!(marks[3], "");

        viewer.handle(Command::ScrollEnd);
        let (screen, _) = draw_marks(&mut viewer, 10, 8);
        assert!(screen.contains("\"«rstuvwxyz\""), "{screen}");
    }

    #[test]
    fn what_cannot_be_shown_is_a_mark_of_one_cell() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show(b"\x1b[2J\xff ok\x07\n\xfe\xfe\xfe\xfe\xfe\xfe\xfe\xfe\xfe\xfe\xfe\xfe\n");
        let (screen, marks) = draw_marks(&mut viewer, 10, 5);
        let rows: Vec<&str> = screen.lines().collect();
        assert_eq!(rows[1], "\"·[2J· ok· \"", "{screen}");
        assert_eq!(marks[1], "···");
        assert_eq!(rows[3], "\"»··       \"", "a wrapped row: {screen}");
        assert_eq!(marks[3], "»··");

        viewer.handle(Command::ToggleWrap);
        let (screen, _) = draw_marks(&mut viewer, 10, 5);
        let rows: Vec<&str> = screen.lines().collect();
        assert_eq!(rows[2], "\"·········»\"", "a cut line: {screen}");
    }

    #[test]
    fn a_title_and_an_end_line_cut_short_draw_their_marks_as_marks() {
        let mut viewer = Viewer::new("/a/very/long/path/to/the/file.txt".to_owned(), SIZES);
        viewer.show(b"x\n");
        let (screen, marks) = draw_marks(&mut viewer, 30, 4);
        let rows: Vec<&str> = screen.lines().collect();
        assert!(rows[0].contains('»'), "{screen}");
        assert_eq!(marks[0], "»", "{screen}");
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.take(part(0, b"x\n", false));
        let (screen, marks) = draw_marks(&mut viewer, 10, 4);
        let rows: Vec<&str> = screen.lines().collect();
        assert!(rows[2].contains('»'), "{screen}");
        assert_eq!(marks[2], "»", "{screen}");
    }

    #[test]
    fn wrapped_rows_after_the_first_start_with_a_break_mark() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show(format!("{}\nab\n", "x".repeat(12)).as_bytes());
        let (screen, marks) = draw_marks(&mut viewer, 5, 7);
        let rows: Vec<&str> = screen.lines().skip(1).take(4).collect();
        assert_eq!(
            rows,
            ["\"xxxxx\"", "\"»xxxx\"", "\"»xxx \"", "\"ab   \""],
            "{screen}"
        );
        assert_eq!(marks[1..=4], ["", "»", "»", ""]);

        let (screen, marks) = draw_marks(&mut viewer, 1, 6);
        let rows: Vec<&str> = screen.lines().skip(1).take(3).collect();
        assert_eq!(rows, ["\"x\"", "\"x\"", "\"x\""], "no room for the mark");
        assert!(marks.iter().all(String::is_empty));
    }

    #[test]
    fn scrolls_sideways_as_vims_z_commands() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        let mut text = format!("{}\n", "x".repeat(40));
        text.push_str(&"y\n".repeat(10));
        viewer.show(text.as_bytes());
        viewer.handle(Command::ToggleWrap);
        // Three rows of text, nineteen cells wide beside the scroll bar.
        draw(&mut viewer, 20, 4);
        assert_eq!(viewer.width, 19);
        let mut press = |command| {
            viewer.handle(command);
            viewer.column
        };
        assert_eq!(press(Command::ScrollRight(3)), 3);
        assert_eq!(press(Command::ScrollLeft(2)), 1);
        assert_eq!(
            press(Command::ScrollLeft(5)),
            0,
            "not below the first column"
        );
        assert_eq!(
            press(Command::HalfRight(2)),
            18,
            "two halves of 19 cells, rounded down"
        );
        assert_eq!(press(Command::HalfLeft(1)), 9);
        assert_eq!(press(Command::HalfLeft(3)), 0);
        assert_eq!(
            press(Command::ScrollEnd),
            21,
            "the end of the longest line at the edge"
        );
        assert_eq!(press(Command::ScrollStart), 0);
        assert_eq!(
            press(Command::ScrollRight(100)),
            21,
            "no further than the end of the longest line at the right edge"
        );
        assert_eq!(press(Command::Right), 21, "Right stops there too");
        assert_eq!(press(Command::HalfRight(1)), 21);
        assert_eq!(press(Command::Left), 20);

        viewer.handle(Command::Scroll(Motion::Down));
        draw(&mut viewer, 20, 4);
        assert_eq!(
            viewer.column, 0,
            "the long line is off screen and the lines on screen fit"
        );
        viewer.handle(Command::ScrollRight(5));
        assert_eq!(viewer.column, 0, "nor do they scroll");
    }

    #[test]
    fn the_lines_scroll_back_as_far_as_the_lines_on_screen_need() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        let text = format!("{}\n{}\nz\n", "x".repeat(40), "y".repeat(25));
        viewer.show(text.as_bytes());
        viewer.handle(Command::ToggleWrap);
        // Two rows of text, nineteen cells wide beside the scroll bar.
        draw(&mut viewer, 20, 3);
        assert_eq!(viewer.width, 19);
        viewer.handle(Command::ScrollRight(100));
        assert_eq!(viewer.column, 21);
        viewer.handle(Command::Scroll(Motion::Down));
        let screen = draw(&mut viewer, 20, 3);
        assert_eq!(viewer.column, 6, "the end of the line of 25 at the edge");
        let rows: Vec<&str> = screen.lines().collect();
        assert!(rows[1].starts_with("\"«yyyyyyyyyyyyyyyyyy"), "{screen}");
    }

    #[test]
    fn wrapped_lines_hold_still_sideways() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show("x".repeat(40).as_bytes());
        let before = draw(&mut viewer, 20, 6);
        for command in [
            Command::Right,
            Command::ScrollRight(3),
            Command::HalfRight(1),
            Command::ScrollEnd,
            Command::ScrollLeft(1),
            Command::HalfLeft(1),
            Command::ScrollStart,
        ] {
            viewer.handle(command);
            assert_eq!(viewer.column, 0, "{command:?}");
            assert_eq!(draw(&mut viewer, 20, 6), before, "{command:?}");
        }
    }

    /// Lines `range` of five bytes each.
    fn short_lines(range: std::ops::RangeInclusive<usize>) -> Vec<u8> {
        range
            .flat_map(|n| format!("l{n:03}\n").into_bytes())
            .collect()
    }

    /// The part `viewer` wants, read from a file of 24 lines of five bytes each.
    fn reply(viewer: &Viewer) -> Part {
        let want = viewer.want().expect("a part to read");
        let first = usize::try_from(want.offset / 5).unwrap() + 1;
        let last = (first + usize::try_from(want.length / 5).unwrap() - 1).min(24);
        let stamp = Stamp {
            size: Some(120),
            ..STAMP
        };
        Part::new(want, &short_lines(first..=last), (last == 24, false), stamp)
    }

    #[test]
    fn reads_the_next_part_when_scrolled_past_the_held_ones() {
        // Parts of eight lines, three of which are more than the window.
        let sizes = Sizes {
            part: 40,
            window: 80,
        };
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        viewer.take(reply(&viewer));
        let top = draw(&mut viewer, 20, 6);
        assert!(top.contains("1/? "), "{top}");
        assert_eq!(viewer.want(), None, "the screen shows no end");
        viewer.handle(Command::Scroll(Motion::End));
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("more below"), "{end}");
        let next = Want {
            offset: 40,
            length: 40,
            new: true,
        };
        assert_eq!(viewer.want(), Some(next));
        viewer.stop();
        assert_eq!(viewer.want(), None, "stopped until the next move");
        viewer.handle(Command::Scroll(Motion::Down));
        assert_eq!(viewer.want(), Some(next));
        viewer.take(reply(&viewer));
        assert_eq!(viewer.want(), None, "the screen is filled");
        viewer.handle(Command::Scroll(Motion::End));
        viewer.take(reply(&viewer));
        let below = draw(&mut viewer, 20, 6);
        assert!(
            below.contains("13/24") && below.contains("l017"),
            "held still: {below}"
        );
        viewer.handle(Command::Scroll(Motion::End));
        assert_eq!(viewer.want(), None, "the end is read");
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("21/24 End") && end.contains("EOF"), "{end}");
        assert_eq!(viewer.parts.held(), 1..3, "the first part let go");

        viewer.handle(Command::Scroll(Motion::Home));
        let first = Want {
            offset: 0,
            length: 40,
            new: false,
        };
        assert_eq!(viewer.want(), Some(first), "read again");
        viewer.take(reply(&viewer));
        let top = draw(&mut viewer, 20, 6);
        assert!(top.contains("1/24 20%") && top.contains("l001"), "{top}");
        assert_eq!(viewer.parts.held(), 0..2, "the last part let go");

        viewer.handle(Command::Top(19));
        viewer.take(reply(&viewer));
        let line = draw(&mut viewer, 20, 6);
        assert!(line.contains("20/24") && line.contains("l020"), "{line}");
        viewer.handle(Command::Scroll(Motion::Up));
        assert_eq!(viewer.want(), None, "the part before is held");
    }

    #[test]
    fn goes_to_a_line_through_the_parts_before_it() {
        let sizes = Sizes {
            part: 40,
            window: 80,
        };
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        viewer.take(reply(&viewer));
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Top(20));
        while viewer.want().is_some() {
            viewer.take(reply(&viewer));
        }
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("21/24 End") && screen.contains("l024"),
            "{screen}"
        );
        assert_eq!(
            viewer.parts.held(),
            2..3,
            "the parts on the way only counted"
        );
    }

    #[test]
    fn reads_the_file_again_when_it_changed() {
        let sizes = Sizes {
            part: 40,
            window: 80,
        };
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        viewer.take(reply(&viewer));
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        let mut changed = reply(&viewer);
        changed.stamp.size = Some(125);
        assert_eq!(viewer.take(changed), Taken::Changed);
        assert!(viewer.loading());
        assert_eq!(viewer.want().map(|want| want.offset), Some(0));
        viewer.take(reply(&viewer));
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains(" 5/? ") && screen.contains("l005"),
            "where it was: {screen}"
        );
    }
}
