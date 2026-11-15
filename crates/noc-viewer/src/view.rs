//! The viewer on screen: a file's text, scrolled, with long lines wrapped or cut.

use std::cell::RefCell;
use std::rc::Rc;

use noc_scroll::scrollbar::{self, Bar, BarGlyphs};
use noc_scroll::{Motion, Step};
use noc_text::{Align, Cut, Fitted, Marks};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::i18n::fl;
use crate::kept::{Change, Kept};
use crate::parts::{Known, Parts};
use crate::read::{Part, Stamp, Want};
use crate::text::{Columns, cut, row_of, row_starts, rows_of_lines, width};

/// Lines of more bytes have their cells counted once, as [`Columns`], rather than for each
/// frame.
const LONG_LINE: usize = 1 << 12;

/// As many bytes are still worked through in one go, whatever the size of a part: a line that
/// goes on past its part, up to its line feed; a part on the way to a line, whose lines are
/// only counted; and the rows of a whole file wrapped, which its scroll bar goes by. More takes
/// long enough to see.
const AT_ONCE: u64 = 16 << 20;

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
    /// file is held and is not too large to count its rows, else bytes; to a byte that is far
    /// from what was read, by a jump.
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
    /// A part read at once: what the first screen waits for.
    pub part: u64,
    /// The parts held; at least two.
    pub window: u64,
    /// What it reads without being asked, where reading is cheap: a file of no more bytes
    /// whole, part by part ahead of the screen, so that all its lines are counted; and on in
    /// order, rather than by a jump, to the end of a file or to a byte no further from what
    /// was read. Nothing for a file on a host, which sends only what the screen needs.
    pub ahead: u64,
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

/// What the viewer found of the file as it is now, while it follows it
/// ([`Viewer::check`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checked {
    /// As it was when it was read last.
    Same,
    /// It only grew: the viewer reads on from its end.
    Grew,
    /// It changed otherwise: the viewer forgot it, and reads it again, to its end.
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
    /// The end of the file at the bottom of the screen.
    End,
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
/// way; told to go to a line, for the parts on the way to it, which count the lines; told to
/// go to the end of the file or to a byte far from what it read, it jumps: it asks for the
/// part there alone, whose lines have no numbers until the parts read in order reach them.
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
    /// Whether the file is binary, as its start tells, once that is read.
    binary: Option<bool>,
    /// It follows the end of the file as the file grows, as `tail -f`.
    following: bool,
    goal: Option<Goal>,
    pull: Option<Pull>,
    /// What it wanted when it stopped, which [`resume`](Self::resume) wants again.
    stopped: Option<(Option<Goal>, Option<Pull>)>,
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
    /// wrapped in, the last first: moving a row on in a line of megabytes must not wrap it all
    /// again. Two widths, as a render wraps in the whole width to tell whether the text needs
    /// the scroll bar, then in the width beside it.
    wrapped: RefCell<[(usize, Kept<[usize]>); 2]>,
    /// The cells of the long lines cut lately, by line.
    columns: RefCell<Kept<Columns>>,
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
            binary: None,
            following: false,
            goal: None,
            pull: None,
            stopped: None,
            top: (0, 0),
            column: 0,
            wrap: true,
            page: 1,
            width: 80,
            bar: None,
            measured: None,
            wrapped: RefCell::default(),
            columns: RefCell::default(),
        }
    }

    /// The scroll bar of the last render, for the mouse; `None` when the text fit.
    pub fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// The part of the file to read next, if any: the first one while it loads, then the one
    /// a scroll past the held parts or a place to go to needs.
    pub fn want(&self) -> Option<Want> {
        let length = self.sizes.part.max(1);
        let most = length.max(AT_ONCE);
        if self.loading() {
            return Some(Want {
                offset: 0,
                length,
                most,
                new: true,
                after_feed: false,
                ahead: false,
            });
        }
        let Some(goal) = self.goal else {
            return match self.pull {
                Some(Pull::Forward) => self.parts.next(length, most),
                Some(Pull::Back) => self.parts.previous(length),
                None => self.ahead(length, most),
            };
        };
        let part = match goal {
            Goal::Start => self.parts.at(0),
            Goal::Line(line) => self.parts.part_of_line(line),
            Goal::Byte(byte) => self.parts.part_of_byte(byte),
            Goal::End => self.last_part().filter(|_| self.parts.ended()),
        };
        match part {
            Some(index) => self.unheld(index),
            None => self.toward(goal, length, most),
        }
    }

    /// The part to read ahead of the screen, which waits for nothing: the next one of a file
    /// small enough to read and hold whole, until it is held. Not of a file that tells no size,
    /// which may never end.
    fn ahead(&self, length: u64, most: u64) -> Option<Want> {
        let size = self.parts.size().filter(|&size| size > 0)?;
        if self.stopped.is_some() || size > self.sizes.ahead.min(self.window()) {
            return None;
        }
        let next = self.parts.next(length, most)?;
        Some(Want {
            ahead: true,
            ..next
        })
    }

    /// What to ask for to read known part `index` again, unless it is held.
    fn unheld(&self, index: usize) -> Option<Want> {
        if self.parts.held().contains(&index) {
            return None;
        }
        self.parts.again(index)
    }

    /// The part to read on the way to `goal`, which no known part has. A line takes the parts
    /// in order, which count the lines up to it, large ones, as their lines are not made for
    /// the screen. The end of the file and a byte take them in order too while they are near;
    /// else the part there, apart from the others, so that going there takes one part however
    /// large the file.
    fn toward(&self, goal: Goal, length: u64, most: u64) -> Option<Want> {
        let jump = |offset| Want {
            offset,
            length,
            most,
            new: true,
            after_feed: true,
            ahead: false,
        };
        // Where the file ends before the goal, its last part has it.
        let last = || self.unheld(self.last_part()?);
        // A file that reports no size has no place to jump to.
        let size = self.parts.size().unwrap_or(0);
        match goal {
            Goal::Start => self.parts.in_order(length, most),
            Goal::Line(_) => self.parts.in_order(most, most).or_else(last),
            Goal::Byte(byte) => {
                // Not read: between the parts in order and those apart, or after the last.
                let between = self.parts.apart_start().is_some_and(|start| byte < start);
                let read = if between {
                    self.parts.frontier()
                } else {
                    self.parts.last_end()
                };
                let near = read.saturating_add(self.near());
                if byte > near && size > near {
                    // No nearer the end than a part, which then has the byte.
                    Some(jump(byte.min(size - length)))
                } else if between {
                    self.parts.in_order(length, most)
                } else {
                    self.parts.on(length, most).or_else(last)
                }
            }
            Goal::End if size > self.parts.last_end().saturating_add(self.near()) => {
                Some(jump(size - length))
            }
            Goal::End => self.parts.on(length, most),
        }
    }

    /// How far past what was read the end of the file, or a byte to go to, still counts as
    /// near: reading on to it, which keeps the numbers of the lines, takes no longer than one
    /// sees. At least a part, so that a jump lands after what was read.
    fn near(&self) -> u64 {
        self.sizes.ahead.max(self.sizes.part.max(1))
    }

    /// The bytes it holds at most: the window, and no less than two parts.
    fn window(&self) -> u64 {
        self.sizes.window.max(self.sizes.part.saturating_mul(2))
    }

    /// The index of the last known part.
    fn last_part(&self) -> Option<usize> {
        self.parts.known().len().checked_sub(1)
    }

    /// Takes a part read for [`want`](Self::want). A part no longer wanted is kept if it fits.
    /// A file that only grew since the parts before, as a log that is being written, did not
    /// change: it goes on past what was its end.
    pub fn take(&mut self, part: Part) -> Taken {
        let again = !part.want.new && !part.want.after_feed;
        let known = self.parts.at(part.want.offset).filter(|_| again);
        let same =
            part.stopped || known.is_none_or(|index| self.parts.known()[index].len == part.len);
        // As the file was when it was asked for: a larger one has its end elsewhere.
        let wanted = self.want() == Some(part.want);
        let other = self.stamp.filter(|&stamp| stamp != part.stamp);
        let grew = other.is_some_and(|stamp| stamp.size < part.stamp.size && stamp.size.is_some());
        if !same || (other.is_some() && !grew) {
            self.reset();
            return Taken::Changed;
        }
        if grew {
            self.parts.grew();
        }
        self.stamp = Some(part.stamp);
        self.parts.set_size(part.stamp.size);
        if part.offset == 0 {
            self.binary = Some(part.binary);
        }
        if !again {
            self.take_new(part, wanted);
        } else if let Some(index) = known
            && !part.stopped
            && !self.parts.held().contains(&index)
            && self.serves(index)
        {
            self.place(index, part.lines);
        }
        self.settle();
        Taken::Read
    }

    /// Takes a part that was not known; `wanted` if it is the one the viewer asks for.
    fn take_new(&mut self, part: Part, wanted: bool) {
        let want = part.want;
        let jump = want.new && want.after_feed;
        // The part of a jump takes the place of the parts apart, which may be the ones on
        // screen: only while the viewer still goes there.
        if jump && !wanted {
            return;
        }
        self.add(part, jump);
        // A jump that came to nothing, or to a part without the byte it went to, which lies in
        // the line the part starts after, would be asked for again, for ever.
        if jump && self.want() == Some(want) {
            self.goal = None;
        }
    }

    /// Notes a part that was not known among the others, and holds its lines if they are
    /// wanted on screen: always those of a `jump`.
    fn add(&mut self, part: Part, jump: bool) {
        let first = self.parts.known().is_empty();
        if part.is_empty() && !(part.end && first) {
            let next = part.want.new && !part.want.after_feed;
            if next && part.end && part.offset == self.parts.last_end() {
                self.parts.ends();
            }
            if self.loading() {
                // Stopped before a line came.
                self.state = State::Text { lines: Vec::new() };
            }
            return;
        }
        let known = Known {
            offset: part.offset,
            len: part.len,
            lines: part.lines.len(),
            open: part.open,
        };
        let Some(index) = self.parts.add(known, part.want, part.end) else {
            return;
        };
        // The parts on the way to a line are only counted.
        if self.loading() || jump || self.serves(index) {
            self.place(index, part.lines);
        }
    }

    /// Stops wanting parts until the next move, or until it resumes.
    pub fn stop(&mut self) {
        self.stopped = Some((self.goal.take(), self.pull.take()));
    }

    /// Wants again what it wanted when it stopped, unless it moved since.
    pub fn resume(&mut self) {
        if let Some((goal, pull)) = self.stopped.take() {
            self.goal = goal;
            self.pull = pull;
        }
    }

    /// Reads the file again from its start, up to the line on top, as when it changed; to its
    /// end while it follows the file.
    pub fn reload(&mut self) {
        self.reset();
    }

    /// Whether the file is binary, as its start tells ([`noc_text::is_binary`]); `None` until
    /// its start is read.
    pub fn binary(&self) -> Option<bool> {
        self.binary
    }

    /// The size the file reported when a part of it was read last, if it reports one.
    pub fn size(&self) -> Option<u64> {
        self.stamp.and_then(|stamp| stamp.size)
    }

    /// The line of the file at the top of the screen, from 0; `None` while it loads, and where
    /// the lines on screen have no numbers, after a jump.
    pub fn line(&self) -> Option<usize> {
        if self.loading() {
            return None;
        }
        self.parts.first_line().map(|first| first + self.top.0)
    }

    /// Whether it follows the end of the file as the file grows.
    pub fn following(&self) -> bool {
        self.following
    }

    /// Whether it wraps long lines, rather than cutting them.
    pub fn wraps(&self) -> bool {
        self.wrap
    }

    /// Follows the end of the file as it grows, as `tail -f` and less's `F` do, from there; or
    /// stops. A move away from the end stops it too. The app tells it how the file is now with
    /// [`check`](Self::check).
    pub fn follow(&mut self, on: bool) {
        self.following = on;
        if on {
            self.stopped = None;
            self.scroll(Motion::End);
        }
    }

    /// Takes the file as it is now, `stamp`, while it follows it: one that only grew is read on
    /// from what was its end, or read again where its last line had no line feed, as what was
    /// added goes on with that line; one that changed otherwise is read again. Either way to
    /// its end.
    pub fn check(&mut self, stamp: Stamp) -> Checked {
        let Some(known) = self.stamp.filter(|_| self.following && !self.loading()) else {
            return Checked::Same;
        };
        if known == stamp {
            return Checked::Same;
        }
        let grew = matches!((known.size, stamp.size), (Some(old), Some(new)) if new > old);
        if !grew {
            self.reset();
            return Checked::Changed;
        }
        if self.parts.ends_open() {
            self.reset();
        } else {
            self.parts.grew();
            self.go_to(Goal::End);
        }
        Checked::Grew
    }

    /// Forgets the parts of the file, which changed, to read it again up to the line on top,
    /// or to its byte where the lines have no numbers; to its end while it follows the file.
    fn reset(&mut self) {
        if self.following {
            self.goal = Some(Goal::End);
        } else if self.goal.is_none() && !self.loading() {
            self.goal = Some(match self.parts.first_line() {
                Some(first) => Goal::Line(first + self.top.0),
                None => Goal::Byte(self.parts.byte_at(self.top.0)),
            });
        }
        self.parts = Parts::default();
        self.stamp = None;
        self.binary = None;
        self.state = State::Loading;
        self.pull = None;
        self.top = (0, 0);
        self.lines_changed(Change::All);
    }

    /// Forgets the rows of the whole text, and moves what is kept of each line with it.
    fn lines_changed(&mut self, change: Change) {
        self.measured = None;
        for (_, rows) in self.wrapped.get_mut() {
            rows.change(change);
        }
        self.columns.get_mut().change(change);
    }

    /// Whether the file ends with the held parts.
    fn at_file_end(&self) -> bool {
        self.parts.ended() && self.parts.held().end == self.parts.known().len()
    }

    /// Whether the lines of known part `index`, just read, are to be held: it has the goal,
    /// or is the last when the file ends before it; or it goes on from the held parts, unless
    /// it is on the way to a line or a byte, where the parts are only counted.
    fn serves(&self, index: usize) -> bool {
        let known = self.parts.known();
        let last = self.parts.ended() && index + 1 == known.len();
        let joins =
            self.parts.held().is_empty() || self.parts.follows(index) || self.parts.precedes(index);
        match self.goal {
            None => joins,
            Some(Goal::Start) => known[index].offset == 0,
            Some(Goal::Line(line)) => match self.parts.part_of_line(line) {
                Some(at) => at == index,
                None => last && self.parts.all_known(),
            },
            Some(Goal::Byte(byte)) => match self.parts.part_of_byte(byte) {
                Some(at) => at == index,
                None => last && byte >= self.parts.last_end(),
            },
            Some(Goal::End) => last || joins,
        }
    }

    /// Holds the lines of known part `index`: after the held parts, before them, or in their
    /// place.
    fn place(&mut self, index: usize, new: Vec<String>) {
        let held = self.parts.held();
        let (follows, precedes) = (self.parts.follows(index), self.parts.precedes(index));
        let change = match &mut self.state {
            State::Text { lines } if follows => {
                lines.extend(new);
                self.parts.set_held(held.start..index + 1);
                Change::After
            }
            State::Text { lines } if precedes => {
                let count = new.len();
                self.top.0 += count;
                lines.splice(0..0, new);
                self.parts.set_held(index..held.end);
                Change::Before(count)
            }
            _ => {
                self.state = State::Text { lines: new };
                self.top = (0, 0);
                self.parts.set_held(index..index + 1);
                Change::All
            }
        };
        self.lines_changed(change);
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
        let at_end = self.at_file_end();
        let line = match goal {
            Goal::Start => self.parts.at(0).filter(|at| held.contains(at)).map(|_| 0),
            Goal::Line(line) => match self.parts.part_of_line(line) {
                Some(index) if held.contains(&index) => self
                    .parts
                    .first_line()
                    .map(|first| line.saturating_sub(first)),
                None if at_end && self.parts.all_known() => Some(usize::MAX),
                _ => None,
            },
            Goal::Byte(byte) => match self.parts.part_of_byte(byte) {
                Some(index) if held.contains(&index) => Some(self.parts.held_line_at(index, byte)),
                None if at_end && byte >= self.parts.last_end() => Some(usize::MAX),
                _ => None,
            },
            Goal::End => at_end.then_some(usize::MAX),
        };
        let Some(line) = line else {
            return;
        };
        self.top = (line, 0).min(self.last_top());
        self.goal = None;
        match goal {
            Goal::Start => self.column = 0,
            // The last part alone may not fill the screen: the one before it comes too, and
            // the next frame puts the end at the bottom again.
            Goal::End if self.short() && self.parts.more_before() => {
                self.pull = Some(Pull::Back);
            }
            Goal::Line(_) | Goal::Byte(_) | Goal::End => {}
        }
    }

    /// Whether the held text and the line that ends it leave rows of the screen empty.
    fn short(&self) -> bool {
        self.page >= 2 && self.after((0, 0), self.page - 2) == self.end()
    }

    /// Lets go of held parts while they take more than the window: those off screen, on the
    /// side away from it, and never the one a scroll past the held parts just read, which it
    /// would ask for again, for ever.
    fn trim(&mut self) {
        let window = self.window();
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
            let change = if self.top.0 >= front && self.pull != Some(Pull::Back) {
                lines.drain(..front);
                self.top.0 -= front;
                self.parts.set_held(held.start + 1..held.end);
                Change::FirstWent(front)
            } else if bottom + back < lines.len() && self.pull != Some(Pull::Forward) {
                lines.truncate(lines.len() - back);
                self.parts.set_held(held.start..held.end - 1);
                Change::RestWent(lines.len())
            } else {
                return;
            };
            self.lines_changed(change);
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

    /// Scrolls, or switches wrapping. A move away from the end stops following the file.
    pub fn handle(&mut self, command: Command) {
        self.stopped = None;
        let leaves = match command {
            Command::Scroll(motion) => {
                matches!(motion.step(self.page), Step::Back(_) | Step::Start)
            }
            Command::Top(_) | Command::Bar(_) => true,
            _ => false,
        };
        if leaves {
            self.following = false;
        }
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
        let on_screen = (self.top.0..self.lines().len()).take(self.page);
        on_screen
            .map(|index| self.width_of(index))
            .max()
            .unwrap_or(0)
    }

    /// The cells of line `index`.
    fn width_of(&self, index: usize) -> usize {
        match self.columns(index) {
            Some(columns) => columns.width(),
            None => self.lines().get(index).map_or(0, |line| width(line)),
        }
    }

    /// The cells of line `index`, counted once while it is among the last ones cut; `None` for
    /// a line short enough to count each time.
    fn columns(&self, index: usize) -> Option<Rc<Columns>> {
        let line = self.lines().get(index)?;
        if line.len() <= LONG_LINE {
            return None;
        }
        let mut columns = self.columns.borrow_mut();
        Some(columns.get(index, || Rc::new(Columns::of(line))))
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
        if wrapped[0].0 != self.width {
            wrapped.swap(0, 1);
            if wrapped[0].0 != self.width {
                wrapped[0] = (self.width, Kept::default());
            }
        }
        Some(wrapped[0].1.get(index, || {
            row_starts(line, self.width, continued(self.width)).into()
        }))
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

    /// Scrolls within the held parts; past them, asks for the next part that way. The start
    /// and the end are those of the file, whatever is held.
    fn scroll(&mut self, motion: Motion) {
        self.goal = None;
        self.pull = None;
        match motion.step(self.page) {
            Step::Back(count) => {
                self.top = self.before(self.top, count);
                if self.top == (0, 0) && self.parts.more_before() {
                    self.pull = Some(Pull::Back);
                }
            }
            Step::On(count) => {
                self.top = self
                    .after(self.top, count)
                    .min(self.last_top().max(self.top));
                if self.bottom() == self.end() && self.parts.more_after() {
                    self.pull = Some(Pull::Forward);
                }
            }
            Step::Start => {
                self.column = 0;
                self.top = (0, 0);
                if self.parts.more_before() {
                    self.goal = Some(Goal::Start);
                }
            }
            Step::End => self.go_to(Goal::End),
        }
    }

    /// Whether the scroll bar goes in lines, which takes counting the rows of every line held:
    /// when the whole file is held and is not too large for that, else in bytes.
    fn bar_in_lines(&self) -> bool {
        self.parts.whole() && self.parts.held_bytes() <= AT_ONCE
    }

    /// Whether it waits for the file.
    pub fn loading(&self) -> bool {
        matches!(self.state, State::Loading)
    }

    /// Draws the viewer over `area`: a title line, `icon` in front of the path, then the text.
    /// While it waits for the file, the text is empty; the app turns the frames of its spinner
    /// as `icon`. `size` writes a number of bytes short, with its unit, in the formats of the
    /// region: where the screen is in a file after a jump, whose lines have no numbers.
    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        styles: &Styles,
        icon: char,
        size: &dyn Fn(u64) -> String,
    ) {
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
        // A taller or wider screen shows the text above rather than empty rows after its end.
        if self.at_file_end() {
            self.top = self.top.min(self.last_top());
        }
        // Other lines on screen, or a wider screen, may need less of the scroll. Wrapped lines
        // never scroll sideways, and need not be counted.
        if !self.wrap {
            self.column = self.column.min(self.last_column());
        }
        let row = |index: u16| Rect::new(area.x, area.y + index, area.width, 1);
        // Rows of text leave the scroll bar's column alone, which the end line's style would dim.
        let text_width = u16::try_from(self.width).unwrap_or(area.width);
        let text_row = |index: u16| Rect::new(area.x, area.y + index, text_width, 1);
        let header = self.header(usize::from(area.width), icon, styles, size);
        frame.render_widget(header, row(0));
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
                self.cut_line(position.0, line, styles)
            };
            frame.render_widget(text, text_row(y));
            let next = self.after(position, 1);
            if next == position {
                break;
            }
            position = next;
        }
    }

    /// The cells of `line`, line `index`, that show from the column on, with the marks of
    /// `styles` where text goes on past either edge: a mark takes the place of the cell of text
    /// under it.
    fn cut_line(&self, index: usize, line: &str, styles: &Styles) -> Line<'static> {
        let (marks, mark) = (styles.marks, styles.mark);
        let columns = self.columns(index);
        let cells = columns
            .as_ref()
            .map_or_else(|| width(line), |columns| columns.width());
        let extends = cells > self.column + self.width;
        let precedes = self.column > 0 && cells > 0 && (self.width > 1 || !extends);
        let taken = usize::from(precedes) + usize::from(extends);
        let room = self.width.saturating_sub(taken);
        let mut spans = Vec::with_capacity(3);
        if precedes {
            spans.push(Span::styled(marks.before, mark));
        }
        let skip = self.column + usize::from(precedes);
        let (byte, before) = columns.map_or((0, 0), |columns| columns.before(skip));
        let mut text = cut(&line[byte..], skip - before, room);
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
    /// only parts of the file are held, or too much to count its rows.
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
    /// percentage is in bytes until all of it is held. After a jump, where the lines before were
    /// not counted, the byte its first line is at, about, and the size of the file stand for
    /// them. The path never moves, whatever `icon` is.
    fn header(
        &self,
        width: usize,
        icon: char,
        styles: &Styles,
        size: &dyn Fn(u64) -> String,
    ) -> Line<'static> {
        let held = self.lines().len();
        let mut position = String::new();
        if !self.loading() {
            let bottom = self.bottom();
            let percent = if self.parts.whole() {
                ((bottom.0 + 1) * 100).checked_div(held).unwrap_or(100)
            } else {
                let byte = self.parts.byte_at(bottom.0 + 1);
                let percent = (byte * 100).checked_div(self.parts.extent()).unwrap_or(100);
                usize::try_from(percent.min(100)).unwrap_or(100)
            };
            let at_end = bottom == self.end() && self.at_file_end();
            let line = |first: usize| first + (self.top.0 + 1).min(held);
            position = match (self.parts.first_line(), self.parts.lines()) {
                (Some(first), Some(lines)) if at_end && self.following => {
                    fl!("viewer-position-follow", line = line(first), lines = lines)
                }
                (Some(first), Some(lines)) if at_end => {
                    fl!("viewer-position-end", line = line(first), lines = lines)
                }
                (Some(first), Some(lines)) => fl!(
                    "viewer-position",
                    line = line(first),
                    lines = lines,
                    percent = percent
                ),
                (Some(first), None) => {
                    fl!(
                        "viewer-position-part",
                        line = line(first),
                        percent = percent
                    )
                }
                (None, _) => {
                    let at = size(self.parts.byte_at(self.top.0));
                    let size = size(self.parts.extent());
                    if at_end && self.following {
                        fl!("viewer-position-bytes-follow", at = at, size = size)
                    } else if at_end {
                        fl!("viewer-position-bytes-end", at = at, size = size)
                    } else {
                        fl!(
                            "viewer-position-bytes",
                            at = at,
                            size = size,
                            percent = percent
                        )
                    }
                }
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
        ahead: 0,
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
            most: AT_ONCE,
            new: true,
            after_feed: false,
            ahead: false,
        };
        Part::new(want, bytes, (end, false), STAMP)
    }

    /// A size as the tests write it: its bytes.
    fn bytes(size: u64) -> String {
        format!("{size}B")
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
            .draw(|frame| viewer.render(frame, frame.area(), &STYLES, '*', &bytes))
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
            .draw(|frame| viewer.render(frame, frame.area(), &STYLES, '|', &bytes))
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
    fn a_taller_or_wider_screen_shows_the_text_above_rather_than_empty_rows() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show(&numbered(20));
        draw(&mut viewer, 30, 6);
        viewer.handle(Command::Scroll(Motion::End));
        assert!(draw(&mut viewer, 30, 6).contains("17/20 End"));
        let taller = draw(&mut viewer, 30, 11);
        assert!(taller.contains("12/20 End"), "{taller}");
        assert!(
            taller.contains("line 12") && taller.contains("─ EOF ─"),
            "{taller}"
        );
        let all = draw(&mut viewer, 30, 30);
        assert!(all.contains("1/20") && all.contains("line 1 "), "{all}");

        viewer.show(&[b'a'; 100]);
        draw(&mut viewer, 10, 6);
        viewer.handle(Command::Scroll(Motion::End));
        draw(&mut viewer, 10, 6);
        let wide = draw(&mut viewer, 60, 6);
        assert!(wide.contains(&"a".repeat(59)), "{wide}");
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
                .draw(|frame| viewer.render(frame, frame.area(), &STYLES, '*', &bytes))
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
            .draw(|frame| viewer.render(frame, frame.area(), &styles, '*', &bytes))
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
    fn a_line_of_many_marks_is_cut_where_it_shows() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        let line: String = ('a'..='z').cycle().take(10_000).collect();
        assert!(line.len() > LONG_LINE);
        viewer.show(format!("{line}\nshort\n").as_bytes());
        viewer.handle(Command::ToggleWrap);
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::ScrollRight(5000));
        let screen = draw(&mut viewer, 20, 6);
        let row = format!("\"«{}»\"", &line[5001..5019]);
        assert!(screen.contains(&row), "{screen}");
        viewer.handle(Command::ScrollEnd);
        assert_eq!(viewer.column, 9980);
        let screen = draw(&mut viewer, 20, 6);
        let row = format!("\"«{}\"", &line[9981..]);
        assert!(screen.contains(&row), "{screen}");
        assert_eq!(viewer.columns.borrow().iter().count(), 1);
    }

    #[test]
    fn a_render_keeps_the_rows_of_both_widths_it_wraps_in() {
        let mut viewer = Viewer::new("f".to_owned(), SIZES);
        viewer.show(&[b'a'; 500]);
        let widths = |viewer: &mut Viewer| {
            draw(viewer, 20, 6);
            let wrapped = viewer.wrapped.borrow();
            let kept = wrapped
                .iter()
                .map(|(width, rows)| (*width, rows.iter().count()));
            kept.collect::<Vec<_>>()
        };
        // Whether the text fits in the whole width, then beside the scroll bar.
        assert_eq!(widths(&mut viewer), [(19, 1), (20, 1)]);
        assert_eq!(widths(&mut viewer), [(19, 1), (20, 1)]);
    }

    #[test]
    fn the_rows_kept_move_with_their_lines_as_parts_come_and_go() {
        use std::fmt::Write as _;
        let mut text = String::new();
        // Lines of one to four rows, no two in a row of the same length.
        for n in 1..=60 {
            let letter = ('a'..='z').cycle().nth(n).unwrap();
            let rest: String = std::iter::repeat_n(letter, n * 7 % 11 * 3 + 9).collect();
            writeln!(text, "{n:02}{rest}").unwrap();
        }
        let sizes = Sizes {
            part: 128,
            window: 256,
            ahead: 0,
        };
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        let step = |viewer: &mut Viewer, command| {
            viewer.handle(command);
            feed(viewer, text.as_bytes());
            draw(viewer, 11, 6);
            let mut kept = 0;
            for (width, wrapped) in &*viewer.wrapped.borrow() {
                for (index, starts) in wrapped.iter() {
                    let line = &viewer.lines()[index];
                    let fresh = row_starts(line, *width, continued(*width));
                    assert_eq!(*starts, *fresh, "line {index} after {command:?}");
                    kept += 1;
                }
            }
            kept
        };
        step(&mut viewer, Command::Scroll(Motion::Home));
        for _ in 0..100 {
            if viewer.parts.held().start > 0 {
                break;
            }
            step(&mut viewer, Command::Scroll(Motion::WindowDown));
        }
        assert_eq!(viewer.parts.held().start, 1, "the first part let go");
        for _ in 0..100 {
            if viewer.parts.held().start == 0 {
                break;
            }
            assert!(step(&mut viewer, Command::Scroll(Motion::WindowUp)) > 0);
        }
        assert_eq!(viewer.parts.held().start, 0, "read again before the others");
        // A jump to the end, and the parts before it as it scrolls back.
        step(&mut viewer, Command::Scroll(Motion::End));
        assert_eq!(viewer.parts.first_line(), None, "apart from the others");
        for _ in 0..10 {
            assert!(step(&mut viewer, Command::Scroll(Motion::WindowUp)) > 0);
        }
        for _ in 0..10 {
            step(&mut viewer, Command::Scroll(Motion::WindowDown));
        }
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
        crate::read::part_of(&short_lines(1..=24), want)
    }

    /// Gives `viewer` the parts of `text` it wants, until it wants none.
    fn feed(viewer: &mut Viewer, text: &[u8]) {
        for _ in 0..100 {
            let Some(want) = viewer.want() else {
                return;
            };
            viewer.take(crate::read::part_of(text, want));
        }
        panic!("it wants {:?} for ever", viewer.want());
    }

    /// Parts of eight lines of five bytes, three of which are more than the window.
    const SMALL: Sizes = Sizes {
        part: 40,
        window: 80,
        ahead: 0,
    };

    const NEXT: Want = Want {
        offset: 40,
        length: 40,
        most: AT_ONCE,
        new: true,
        after_feed: false,
        ahead: false,
    };

    #[test]
    fn reads_the_next_part_when_scrolled_past_the_held_ones() {
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        viewer.take(reply(&viewer));
        let top = draw(&mut viewer, 20, 6);
        assert!(top.contains("1/? "), "{top}");
        assert_eq!(viewer.want(), None, "the screen shows no end");
        viewer.handle(Command::Scroll(Motion::WindowDown));
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("more below"), "{end}");
        assert_eq!(viewer.want(), Some(NEXT));
        viewer.stop();
        assert_eq!(viewer.want(), None, "stopped until the next move");
        viewer.resume();
        assert_eq!(viewer.want(), Some(NEXT), "or until it resumes");
        viewer.stop();
        viewer.handle(Command::Scroll(Motion::Down));
        assert_eq!(viewer.want(), Some(NEXT));
        viewer.resume();
        assert_eq!(viewer.want(), Some(NEXT), "the move wants it now");
        viewer.take(reply(&viewer));
        assert_eq!(viewer.want(), None, "the screen is filled");
        let below = draw(&mut viewer, 20, 6);
        assert!(
            below.contains("5/? ") && below.contains("l009"),
            "held still: {below}"
        );
        for _ in 0..3 {
            viewer.handle(Command::Scroll(Motion::WindowDown));
            draw(&mut viewer, 20, 6);
        }
        assert_eq!(viewer.want(), Some(Want { offset: 80, ..NEXT }));
        viewer.take(reply(&viewer));
        assert_eq!(viewer.parts.held(), 1..3, "the first part let go");
        viewer.handle(Command::Scroll(Motion::End));
        assert_eq!(viewer.want(), None, "the end is read");
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("21/24 End") && end.contains("EOF"), "{end}");

        viewer.handle(Command::Scroll(Motion::Home));
        let first = Want {
            offset: 0,
            most: 40,
            new: false,
            ..NEXT
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
    fn a_part_read_back_into_a_full_window_is_not_let_go_at_once() {
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        viewer.take(reply(&viewer));
        draw(&mut viewer, 20, 6);
        for _ in 0..6 {
            viewer.handle(Command::Scroll(Motion::WindowDown));
            feed(&mut viewer, &short_lines(1..=24));
            draw(&mut viewer, 20, 6);
        }
        assert_eq!(viewer.parts.held(), 1..3, "the window is full");
        for _ in 0..4 {
            viewer.handle(Command::Scroll(Motion::WindowUp));
        }
        let first = Want {
            offset: 0,
            most: 40,
            new: false,
            ..NEXT
        };
        assert_eq!(viewer.want(), Some(first), "scrolled past the first held");
        viewer.take(reply(&viewer));
        assert_eq!(viewer.want(), None, "held, not asked for again");
        assert_eq!(viewer.parts.held(), 0..2, "the part at the other end went");
        viewer.handle(Command::Scroll(Motion::Up));
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("8/24") && screen.contains("l008"),
            "{screen}"
        );
    }

    #[test]
    fn goes_to_a_line_through_the_parts_before_it() {
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        viewer.take(reply(&viewer));
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Top(20));
        let stretch = Want {
            length: AT_ONCE,
            ..NEXT
        };
        assert_eq!(
            viewer.want(),
            Some(stretch),
            "as much as it counts in one go"
        );
        while viewer.want().is_some() {
            viewer.take(reply(&viewer));
        }
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("21/24 End") && screen.contains("l024"),
            "{screen}"
        );
        assert_eq!(viewer.parts.held(), 1..2, "the part with the line alone");
    }

    #[test]
    fn the_end_of_a_file_is_jumped_to_and_what_is_before_it_comes_as_it_scrolls_up() {
        let text = short_lines(1..=48);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        viewer.handle(Command::Scroll(Motion::End));
        let first = viewer.want().expect("the first part");
        assert_eq!(first.offset, 0, "which tells its size");
        viewer.take(crate::read::part_of(&text, first));
        draw(&mut viewer, 20, 6);
        let last = Want {
            offset: 200,
            after_feed: true,
            ..NEXT
        };
        assert_eq!(viewer.want(), Some(last), "the last part alone");
        viewer.take(crate::read::part_of(&text, last));
        assert_eq!(viewer.want(), None);
        let end = draw(&mut viewer, 20, 6);
        // The lines before it were not counted: where it is goes in bytes.
        assert!(end.contains("220B/240B End"), "{end}");
        assert!(end.contains("l045") && end.contains("l048"), "{end}");
        assert!(end.contains("EOF") && !end.contains("l044"), "{end}");

        for _ in 0..2 {
            viewer.handle(Command::Scroll(Motion::WindowUp));
        }
        let before = Want {
            offset: 160,
            most: 40,
            new: false,
            after_feed: true,
            ..NEXT
        };
        assert_eq!(viewer.want(), Some(before), "the part before the last");
        feed(&mut viewer, &text);
        let above = draw(&mut viewer, 20, 6);
        assert!(
            above.contains("200B/240B") && above.contains("l041"),
            "{above}"
        );
        viewer.handle(Command::Scroll(Motion::Up));
        let above = draw(&mut viewer, 20, 6);
        assert!(
            above.contains("195B/240B") && above.contains("l040"),
            "{above}"
        );

        // What lies between, once no more than a part, joins them to the start: the lines
        // have numbers again.
        for _ in 0..40 {
            viewer.handle(Command::Scroll(Motion::WindowUp));
            feed(&mut viewer, &text);
            draw(&mut viewer, 20, 6);
        }
        let top = draw(&mut viewer, 20, 6);
        assert!(top.contains("1/48 ") && top.contains("l001"), "{top}");
        viewer.handle(Command::Scroll(Motion::End));
        feed(&mut viewer, &text);
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("45/48 End") && end.contains("l048"), "{end}");
    }

    #[test]
    fn a_last_part_shorter_than_the_screen_shows_with_the_part_before_it() {
        let text = short_lines(1..=24);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        viewer.take(reply(&viewer));
        draw(&mut viewer, 20, 13);
        viewer.handle(Command::Scroll(Motion::End));
        feed(&mut viewer, &text);
        let end = draw(&mut viewer, 20, 13);
        assert!(end.contains("l014") && !end.contains("l013"), "{end}");
        assert!(end.contains("14/24 End") && end.contains("EOF"), "{end}");
    }

    #[test]
    fn the_end_of_a_file_that_is_near_or_has_no_size_is_read_in_order() {
        let text = short_lines(1..=16);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        feed(&mut viewer, &text);
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        assert_eq!(viewer.want(), Some(NEXT), "no further than a jump reads");
        feed(&mut viewer, &text);
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("13/16 End") && end.contains("l016"), "{end}");
        assert_eq!(viewer.parts.held(), 0..2, "and held on the way");

        // A file of the system, which makes its text as it is read.
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        viewer.take(part(0, &short_lines(1..=8), false));
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        let next = Want {
            length: SMALL.part,
            ..NEXT
        };
        assert_eq!(viewer.want(), Some(next));
    }

    #[test]
    fn a_file_small_enough_is_read_whole_ahead_of_the_screen() {
        let sizes = Sizes {
            ahead: 200,
            ..SMALL
        };
        let text = short_lines(1..=16);
        let first = |text: &[u8]| {
            let mut viewer = Viewer::new("f".to_owned(), sizes);
            let want = viewer.want().expect("the first part");
            viewer.take(crate::read::part_of(text, want));
            viewer
        };
        let mut viewer = first(&text);
        let top = draw(&mut viewer, 20, 6);
        assert!(top.contains("1/? "), "{top}");
        let ahead = Want {
            ahead: true,
            ..NEXT
        };
        assert_eq!(viewer.want(), Some(ahead), "though the screen shows no end");
        viewer.stop();
        assert_eq!(viewer.want(), None, "stopped until the next move");
        viewer.handle(Command::Scroll(Motion::Down));
        feed(&mut viewer, &text);
        let whole = draw(&mut viewer, 20, 6);
        assert!(whole.contains("2/16 37%"), "all its lines counted: {whole}");
        assert_eq!(viewer.parts.held(), 0..2);

        // Not one that the window does not hold, nor one that tells no size.
        let mut viewer = first(&short_lines(1..=17));
        draw(&mut viewer, 20, 6);
        assert_eq!(viewer.want(), None);
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        viewer.take(part(0, &short_lines(1..=8), false));
        assert_eq!(viewer.want(), None);
    }

    #[test]
    fn the_end_no_further_than_it_reads_ahead_is_read_in_order() {
        let sizes = Sizes {
            ahead: 200,
            ..SMALL
        };
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        let text = short_lines(1..=48);
        viewer.take(crate::read::part_of(&text, viewer.want().expect("a part")));
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        assert_eq!(viewer.want(), Some(NEXT));
        feed(&mut viewer, &text);
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("45/48 End") && end.contains("l048"), "{end}");

        // A byte more, and it jumps.
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        let text = [text, b"x".to_vec()].concat();
        viewer.take(crate::read::part_of(&text, viewer.want().expect("a part")));
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        assert_eq!(viewer.want().map(|want| want.after_feed), Some(true));
    }

    /// Reads the parts `viewer` wants from the file at `path`, as the app does.
    async fn read(viewer: &mut Viewer, path: &std::path::PathBuf) {
        use std::ops::ControlFlow;
        for _ in 0..10_000 {
            let Some(want) = viewer.want() else {
                return;
            };
            let go_on = |_| ControlFlow::Continue(());
            let part = crate::read::read_part(&noc_vfs::LocalFs, path, want, go_on);
            viewer.take(part.await.unwrap());
        }
        panic!("it wants {:?} for ever", viewer.want());
    }

    #[tokio::test]
    async fn a_file_read_part_by_part_has_every_line_once_whichever_way_it_is_read() {
        use std::fmt::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        // A window for all of it, so that the lines held can be told from those of the file.
        let sizes = Sizes {
            part: 4096,
            window: 1 << 30,
            ahead: 0,
        };
        // Lines of many lengths, down the file; some are longer than a part.
        let mut text = String::new();
        for n in 0..4000 {
            let length = if n % 700 == 350 { 9000 } else { n * 31 % 211 };
            writeln!(text, "{n:04} {}", "x".repeat(length)).unwrap();
        }
        std::fs::write(&path, &text).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        for _ in 0..10_000 {
            read(&mut viewer, &path).await;
            draw(&mut viewer, 40, 12);
            if viewer.at_file_end() {
                break;
            }
            viewer.handle(Command::Scroll(Motion::WindowDown));
        }
        assert_eq!(viewer.lines(), lines, "read down");
        assert_eq!(viewer.parts.lines(), Some(lines.len()));

        // Up from a jump to the end; no line is longer than a part, which a jump would cut.
        let text = text.replace(&"x".repeat(9000), "long");
        std::fs::write(&path, &text).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        read(&mut viewer, &path).await;
        draw(&mut viewer, 40, 12);
        viewer.handle(Command::Scroll(Motion::End));
        read(&mut viewer, &path).await;
        let end = draw(&mut viewer, 40, 12);
        assert!(end.contains("3999") && end.contains("EOF"), "{end}");
        assert_eq!(viewer.parts.lines(), None, "not counted");
        for _ in 0..10_000 {
            if !viewer.parts.more_before() {
                break;
            }
            viewer.handle(Command::Scroll(Motion::WindowUp));
            read(&mut viewer, &path).await;
            draw(&mut viewer, 40, 12);
        }
        assert_eq!(viewer.lines(), lines, "read up from the end");
        assert_eq!(viewer.parts.lines(), Some(lines.len()), "the parts met");
    }

    #[test]
    fn a_byte_far_from_what_was_read_is_jumped_to() {
        let text = short_lines(1..=48);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        feed(&mut viewer, &text);
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Bar(150));
        let there = Want {
            offset: 150,
            after_feed: true,
            ..NEXT
        };
        assert_eq!(viewer.want(), Some(there));
        feed(&mut viewer, &text);
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("150B/240B") && screen.contains("l031"),
            "{screen}"
        );
        // In the middle of a line: from the next one on.
        viewer.handle(Command::Bar(102));
        feed(&mut viewer, &text);
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("105B/240B") && screen.contains("l022"),
            "{screen}"
        );
        // No nearer the end than a part, which then shows from the line with the byte.
        viewer.handle(Command::Bar(230));
        assert_eq!(viewer.want().map(|want| want.offset), Some(200));
        feed(&mut viewer, &text);
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("l045") && screen.contains("EOF"),
            "{screen}"
        );
        // A byte near what was read is read up to in order, no further than the parts apart,
        // and its line has a number.
        viewer.handle(Command::Bar(60));
        let on = Want { most: 160, ..NEXT };
        assert_eq!(viewer.want(), Some(on));
        feed(&mut viewer, &text);
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("13/? ") && screen.contains("l013"),
            "{screen}"
        );
    }

    #[test]
    fn a_jump_no_longer_wanted_leaves_the_text_as_it_is() {
        let text = short_lines(1..=48);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        feed(&mut viewer, &text);
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        let last = viewer.want().expect("the last part");
        viewer.handle(Command::Scroll(Motion::Down));
        viewer.take(crate::read::part_of(&text, last));
        assert_eq!(viewer.parts.known().len(), 1);
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("2/? ") && screen.contains("l002"),
            "{screen}"
        );

        // Nor does one take the place of the end on screen.
        viewer.handle(Command::Scroll(Motion::End));
        feed(&mut viewer, &text);
        viewer.handle(Command::Bar(120));
        let there = viewer.want().expect("the part there");
        viewer.handle(Command::Scroll(Motion::Up));
        viewer.take(crate::read::part_of(&text, there));
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("l044") && screen.contains("l048"),
            "{screen}"
        );
    }

    #[test]
    fn a_file_that_grew_goes_on_past_what_was_its_end() {
        let text = short_lines(1..=24);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        for _ in 0..8 {
            feed(&mut viewer, &text);
            draw(&mut viewer, 20, 6);
            viewer.handle(Command::Scroll(Motion::WindowDown));
        }
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("21/24 End") && end.contains("EOF"), "{end}");
        assert_eq!(viewer.parts.held(), 1..3);
        // The part read again on the way back finds more of the file: what was read stays.
        let text = short_lines(1..=28);
        for _ in 0..4 {
            viewer.handle(Command::Scroll(Motion::WindowUp));
        }
        let first = viewer.want().expect("the first part again");
        assert_eq!(viewer.take(crate::read::part_of(&text, first)), Taken::Read);
        assert_eq!(viewer.parts.known().len(), 3);
        let above = draw(&mut viewer, 20, 6);
        assert!(above.contains("9/? ") && above.contains("l009"), "{above}");
        viewer.handle(Command::Scroll(Motion::End));
        feed(&mut viewer, &text);
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("25/28 End") && end.contains("l028"), "{end}");
    }

    #[test]
    fn the_end_of_a_file_that_grows_meanwhile_is_reached() {
        let text = short_lines(1..=48);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        feed(&mut viewer, &text);
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        let last = viewer.want().expect("the last part");
        // The part of the jump comes from a file that has a line more by now.
        let text = short_lines(1..=49);
        assert_eq!(viewer.take(crate::read::part_of(&text, last)), Taken::Read);
        assert_eq!(viewer.parts.known().len(), 2, "taken, though not the end");
        feed(&mut viewer, &text);
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("l049") && end.contains("EOF"), "{end}");
        assert!(end.contains("225B/245B End"), "{end}");
    }

    #[test]
    fn reads_the_file_again_when_it_changed() {
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        viewer.take(reply(&viewer));
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::WindowDown));
        let mut changed = reply(&viewer);
        changed.stamp.size = Some(115);
        assert_eq!(viewer.take(changed), Taken::Changed);
        assert!(viewer.loading());
        assert_eq!(viewer.want().map(|want| want.offset), Some(0));
        viewer.take(reply(&viewer));
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains(" 5/? ") && screen.contains("l005"),
            "where it was: {screen}"
        );

        // Where the lines have no numbers, it goes back to the byte.
        let text = short_lines(1..=48);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        feed(&mut viewer, &text);
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        feed(&mut viewer, &text);
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::WindowUp));
        let mut changed = crate::read::part_of(&text, viewer.want().expect("the part before"));
        // As large as it was, written at another time.
        changed.stamp.modified = Some(std::time::SystemTime::UNIX_EPOCH);
        assert_eq!(viewer.take(changed), Taken::Changed);
        feed(&mut viewer, &text);
        let screen = draw(&mut viewer, 20, 6);
        assert!(
            screen.contains("200B/240B") && screen.contains("l041"),
            "{screen}"
        );
    }

    #[test]
    fn reloads_the_file_up_to_the_line_on_top_and_tells_what_it_is() {
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        assert_eq!(
            (viewer.binary(), viewer.size(), viewer.line()),
            (None, None, None)
        );
        feed(&mut viewer, &short_lines(1..=24));
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::WindowDown));
        feed(&mut viewer, &short_lines(1..=24));
        let before = draw(&mut viewer, 20, 6);
        let line = viewer.line().expect("a line on top");
        assert!(before.contains(&format!(" {}/? ", line + 1)), "{before}");
        assert_eq!((viewer.binary(), viewer.size()), (Some(false), Some(120)));
        viewer.reload();
        assert!(viewer.loading());
        assert_eq!((viewer.binary(), viewer.line()), (None, None));
        let text: Vec<u8> = short_lines(1..=24)
            .iter()
            .map(|&byte| if byte == b'l' { b'm' } else { byte })
            .collect();
        feed(&mut viewer, &text);
        let after = draw(&mut viewer, 20, 6);
        assert_eq!(viewer.line(), Some(line), "where it was: {after}");
        assert!(after.contains(&format!("m{:03}", line + 1)), "{after}");

        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        feed(&mut viewer, b"\x7fELF\x02\x01\x01\0\n");
        assert_eq!(viewer.binary(), Some(true));
        // After a jump, the lines on screen have no numbers.
        let text = short_lines(1..=48);
        let mut viewer = Viewer::new("f".to_owned(), SMALL);
        feed(&mut viewer, &text);
        draw(&mut viewer, 20, 6);
        viewer.handle(Command::Scroll(Motion::End));
        feed(&mut viewer, &text);
        assert_eq!(viewer.line(), None);
        assert_eq!(viewer.binary(), Some(false), "from the start it read first");
    }

    #[test]
    fn follows_the_end_of_a_file_as_it_grows_until_it_scrolls_away() {
        let stamp = |text: &[u8]| Stamp {
            size: Some(text.len() as u64),
            modified: None,
        };
        let text = short_lines(1..=24);
        // Read whole, so that the lines have numbers.
        let sizes = Sizes {
            ahead: 1 << 10,
            window: 1 << 10,
            ..SMALL
        };
        let mut viewer = Viewer::new("f".to_owned(), sizes);
        feed(&mut viewer, &text);
        draw(&mut viewer, 20, 6);
        assert_eq!(viewer.check(stamp(b"")), Checked::Same, "not following");
        viewer.follow(true);
        feed(&mut viewer, &text);
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("21/24 Follow"), "{end}");
        assert_eq!(viewer.check(stamp(&text)), Checked::Same);
        assert_eq!(viewer.want(), None);
        let grown = short_lines(1..=26);
        assert_eq!(viewer.check(stamp(&grown)), Checked::Grew);
        assert_eq!(
            viewer.want().map(|want| want.offset),
            Some(120),
            "on from the end"
        );
        feed(&mut viewer, &grown);
        let end = draw(&mut viewer, 20, 6);
        assert!(
            end.contains("23/26 Follow") && end.contains("l026"),
            "{end}"
        );

        // What goes on with a last line that had no line feed has it read again.
        let mut open = grown.clone();
        open.extend_from_slice(b"l0");
        assert_eq!(viewer.check(stamp(&open)), Checked::Grew);
        feed(&mut viewer, &open);
        draw(&mut viewer, 20, 6);
        let mut closed = open.clone();
        closed.extend_from_slice(b"27\n");
        assert_eq!(viewer.check(stamp(&closed)), Checked::Grew);
        assert!(viewer.loading(), "read again");
        feed(&mut viewer, &closed);
        let end = draw(&mut viewer, 20, 6);
        let rows: Vec<&str> = end
            .lines()
            .filter_map(|row| row.trim_matches('"').split_whitespace().next())
            .collect();
        assert!(rows.contains(&"l026") && rows.contains(&"l027"), "{end}");
        assert!(!rows.contains(&"l0"), "one line: {end}");
        assert!(end.contains("Follow"), "{end}");

        // A file cut short, as a log emptied, is read again, to its end.
        let short = short_lines(1..=3);
        assert_eq!(viewer.check(stamp(&short)), Checked::Changed);
        feed(&mut viewer, &short);
        let end = draw(&mut viewer, 20, 6);
        assert!(end.contains("1/3 Follow") && end.contains("l003"), "{end}");
        viewer.handle(Command::Scroll(Motion::Down));
        assert!(viewer.following(), "the end stays on screen");
        viewer.handle(Command::Scroll(Motion::Up));
        assert!(!viewer.following());
        assert_eq!(
            viewer.check(stamp(&text)),
            Checked::Same,
            "no longer follows"
        );
        assert!(draw(&mut viewer, 20, 6).contains("1/3 End"));
    }
}
