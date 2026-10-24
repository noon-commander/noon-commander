//! Windows that grow from their middle as they open and shrink back as they close: their
//! frame alone, empty, in the window's colors, a few sizes over a moment, before the window
//! shows whole or after it went (ADR 0034).
//!
//! Windows draw themselves as always. [`Stage::show`] finds where one drew its frame by what
//! it changed on the screen, and while the frame grows puts back what the window covered.
//! Keys work on the window at once; only the screen waits.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Clear, Widget as _};

use super::app::Side;
use super::theme::Theme;

/// How many sizes a frame takes between none and whole.
const STEPS: u32 = 6;
/// How long each size shows.
const STEP: Duration = Duration::from_millis(20);

/// A window that animates, told apart from the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Window {
    KeyHints,
    /// A dialog, by its id.
    Dialog(u64),
    /// The location menu over the panel on its side.
    Menu(Side),
    TabMenu(Side),
    Jump,
    Workspaces,
    History,
    Find,
    /// The results of a checksum job, by the job's id.
    Results(u64),
    /// The window of a job in front, by its id.
    Job(u64),
    JobsList,
    Help,
    Configuration,
    PullDown,
}

/// Where a frame grows from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grows {
    /// Its middle, as dialogs do.
    FromMiddle,
    /// Its top, as a menu that drops from its bar.
    Down,
}

/// Where a window drew its frame, and in which colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drawn {
    /// The frame with the blank cells around it.
    outer: Rect,
    /// The blank cells around the frame: on either side, and above and below it.
    margin: Margin,
    style: Style,
    /// Every cell the window changed; its shadow stays within them.
    bounds: Rect,
    grows: Grows,
}

/// A frame growing or shrinking.
#[derive(Debug)]
struct Zoom {
    window: Window,
    drawn: Drawn,
    start: Instant,
    opens: bool,
    /// How long each size shows.
    each: Duration,
}

impl Zoom {
    /// The size reached at `now`, from 0; `None` once the window shows whole or has gone.
    fn step(&self, now: Instant) -> Option<u32> {
        let elapsed = now.saturating_duration_since(self.start).as_nanos() / self.each.as_nanos();
        u32::try_from(elapsed).ok().filter(|&step| step < STEPS)
    }

    /// How much of the whole frame shows at `step`, in parts of `STEPS + 1`.
    fn parts(&self, step: u32) -> u32 {
        if self.opens { step + 1 } else { STEPS - step }
    }

    /// When the size after the one at `now` is due.
    fn due(&self, now: Instant) -> Option<Instant> {
        self.step(now)
            .map(|step| self.start + self.each * (step + 1))
    }
}

/// What the frame being drawn needs to know.
#[derive(Debug, Clone, Copy)]
struct Now {
    at: Instant,
    border: BorderType,
    shadow: Option<Style>,
}

/// The windows on screen, frame after frame, and those growing or shrinking.
#[derive(Debug)]
pub(crate) struct Stage {
    /// The windows of the last frame, where they drew.
    shown: Vec<(Window, Drawn)>,
    /// The windows of this frame so far.
    drawing: Vec<(Window, Drawn)>,
    zooms: Vec<Zoom>,
    /// The screen of the last frame, and whether the viewer was on it instead of the panels.
    screen: Option<(Rect, bool)>,
    /// The next frame shows what is on it at once, as after the terminal came back.
    settle: bool,
    /// A window opened on this frame.
    opened: bool,
    /// The frame being drawn; none while windows do not animate.
    now: Option<Now>,
    /// How long each size of a frame shows: [`STEP`], or longer in debug builds.
    each: Duration,
    /// The lines of the frames of this frame's windows.
    border: BorderType,
    /// Debug builds only: each window writes its size on its frame.
    sizes: bool,
}

impl Default for Stage {
    fn default() -> Self {
        Self {
            shown: Vec::new(),
            drawing: Vec::new(),
            zooms: Vec::new(),
            screen: None,
            settle: false,
            opened: false,
            now: None,
            each: STEP,
            border: BorderType::default(),
            sizes: false,
        }
    }
}

impl Stage {
    /// Debug builds only: frames take `whole` to grow or shrink, so they can be watched;
    /// never less than they take anyway.
    #[cfg(debug_assertions)]
    pub(crate) fn slow(&mut self, whole: Duration) {
        self.each = (whole / STEPS).max(STEP);
    }

    /// Debug builds only: each window writes its size on its frame if `on`, once it shows
    /// whole.
    #[cfg(debug_assertions)]
    pub(crate) fn show_sizes(&mut self, on: bool) {
        self.sizes = on;
    }

    /// Starts a frame drawn `at` on `screen`, with the viewer on it or not, in `theme`;
    /// windows animate if `on`. A screen of a new size, or the viewer coming or going, shows
    /// its windows at once.
    pub(crate) fn begin(
        &mut self,
        on: bool,
        (screen, viewing): (Rect, bool),
        theme: &Theme,
        at: Instant,
    ) {
        if !on {
            *self = Self {
                each: self.each,
                border: theme.border_type(),
                sizes: self.sizes,
                ..Self::default()
            };
            return;
        }
        if self.screen != Some((screen, viewing)) {
            self.settle();
        }
        self.screen = Some((screen, viewing));
        self.drawing.clear();
        self.opened = false;
        self.border = theme.border_type();
        self.now = Some(Now {
            at,
            border: theme.border_type(),
            shadow: theme.shadow,
        });
    }

    /// Shows the next frame as it is, without starting or going on with any animation.
    pub(crate) fn settle(&mut self) {
        self.zooms.clear();
        self.settle = true;
    }

    /// Has `draw` draw `window`, which grows from where `grows` says while it opens.
    pub(crate) fn show(
        &mut self,
        frame: &mut Frame<'_>,
        (window, grows): (Window, Grows),
        draw: impl FnOnce(&mut Frame<'_>),
    ) {
        if self.now.is_none() && !self.sizes {
            draw(frame);
            return;
        }
        let before = frame.buffer_mut().clone();
        draw(frame);
        let after = frame.buffer_mut();
        let Some(drawn) = find(&before, after, self.border, grows) else {
            return;
        };
        let Some(now) = self.now else {
            draw_size(after, &drawn);
            return;
        };
        self.drawing.push((window, drawn));
        let seen = self.shown.iter().any(|(shown, _)| *shown == window);
        if !seen && !self.settle {
            self.zooms.retain(|zoom| zoom.window != window);
            self.zooms.push(Zoom {
                window,
                drawn,
                start: now.at,
                opens: true,
                each: self.each,
            });
            self.opened = true;
        }
        let step = self
            .zooms
            .iter()
            .find(|zoom| zoom.window == window && zoom.opens)
            .and_then(|zoom| Some((zoom, zoom.step(now.at)?)));
        if let Some((zoom, step)) = step {
            put_back(&before, after, &drawn, now.shadow.is_some());
            draw_frame(after, &drawn, zoom.parts(step), now);
        } else if self.sizes {
            draw_size(after, &drawn);
        }
    }

    /// Ends the frame: the windows of the last one that it did not show shrink, over
    /// everything, unless another window opened in their place.
    pub(crate) fn end(&mut self, frame: &mut Frame<'_>) {
        let Some(now) = self.now else {
            return;
        };
        let drawing = &self.drawing;
        let drawn = |window: Window| drawing.iter().any(|(shown, _)| *shown == window);
        self.zooms.retain(|zoom| !zoom.opens || drawn(zoom.window));
        if self.opened || self.settle {
            self.zooms.retain(|zoom| zoom.opens);
        } else {
            for &(window, place) in &self.shown {
                if !drawn(window) {
                    self.zooms.retain(|zoom| zoom.window != window);
                    self.zooms.push(Zoom {
                        window,
                        drawn: place,
                        start: now.at,
                        opens: false,
                        each: self.each,
                    });
                }
            }
        }
        self.zooms.retain(|zoom| zoom.step(now.at).is_some());
        for zoom in self.zooms.iter().filter(|zoom| !zoom.opens) {
            if let Some(step) = zoom.step(now.at) {
                draw_frame(frame.buffer_mut(), &zoom.drawn, zoom.parts(step), now);
            }
        }
        self.shown = std::mem::take(&mut self.drawing);
        self.settle = false;
    }

    /// When the next frame of an animation is due, if one goes on.
    pub(crate) fn due(&self) -> Option<Instant> {
        let at = self.now?.at;
        self.zooms.iter().filter_map(|zoom| zoom.due(at)).min()
    }
}

/// Where the window drawn between `before` and `after` has its frame, in the lines of
/// `border`: the first top left corner it drew, and around it the blank cells it drew on
/// both sides.
fn find(before: &Buffer, after: &Buffer, border: BorderType, grows: Grows) -> Option<Drawn> {
    let bounds = changed(before, after)?;
    let set = border.to_border_set();
    let symbol = |x: u16, y: u16| after[(x, y)].symbol();
    let new = |x: u16, y: u16| before[(x, y)] != after[(x, y)];
    let blank =
        |x: u16, y: u16| bounds.contains(Position::new(x, y)) && new(x, y) && symbol(x, y) == " ";
    let (x, y) = bounds
        .rows()
        .flat_map(Rect::columns)
        .map(|cell| (cell.x, cell.y))
        .find(|&(x, y)| new(x, y) && symbol(x, y) == set.top_left)?;
    let right = (x + 1..bounds.right()).find(|&column| symbol(column, y) == set.top_right)?;
    let bottom = (y + 1..bounds.bottom()).find(|&row| symbol(x, row) == set.bottom_left)?;
    let horizontal = u16::from(x > 0 && blank(x - 1, y) && blank(right.saturating_add(1), y));
    let vertical = u16::from(y > 0 && blank(x, y - 1) && blank(x, bottom.saturating_add(1)));
    let outer = Rect::new(
        x - horizontal,
        y - vertical,
        right - x + 1 + 2 * horizontal,
        bottom - y + 1 + 2 * vertical,
    );
    Some(Drawn {
        outer,
        margin: Margin::new(horizontal, vertical),
        style: after[(x, y)].style(),
        bounds,
        grows,
    })
}

/// The smallest rectangle around every cell that differs between `before` and `after`.
fn changed(before: &Buffer, after: &Buffer) -> Option<Rect> {
    let mut cells = before
        .content
        .iter()
        .zip(&after.content)
        .enumerate()
        .filter(|(_, (was, is))| was != is)
        .map(|(index, _)| after.pos_of(index));
    let (x, y) = cells.next()?;
    let (left, top, right, bottom) = cells
        .fold((x, y, x, y), |(left, top, right, bottom), (x, y)| {
            (left.min(x), top.min(y), right.max(x), bottom.max(y))
        });
    Some(Rect::new(left, top, right - left + 1, bottom - top + 1))
}

/// The cells of the shadow of a box at `outer`: two columns to the right and a row below,
/// as mc draws it.
fn shadow_of(outer: Rect) -> [Rect; 2] {
    [
        Rect::new(outer.right(), outer.y.saturating_add(1), 2, outer.height),
        Rect::new(outer.x.saturating_add(2), outer.bottom(), outer.width, 1),
    ]
}

/// Puts back in `after` what was in `before` under the window at `drawn` and its shadow.
fn put_back(before: &Buffer, after: &mut Buffer, drawn: &Drawn, shadow: bool) {
    let [right, below] = shadow_of(drawn.outer);
    let shadows = if shadow {
        [right, below]
    } else {
        [Rect::ZERO; 2]
    };
    for rect in [drawn.outer].into_iter().chain(shadows) {
        for cell in rect.intersection(after.area).positions() {
            after[cell] = before[cell].clone();
        }
    }
}

/// The frame at `drawn` at `parts` of `STEPS + 1` of its size, empty: from its middle, or
/// from its top for a menu.
fn scaled(drawn: &Drawn, parts: u32) -> Rect {
    let outer = drawn.outer;
    let part = |whole: u16| {
        let size = (u32::from(whole) * parts).div_ceil(STEPS + 1);
        u16::try_from(size)
            .unwrap_or(whole)
            .clamp(whole.min(2), whole)
    };
    let (width, height) = (part(outer.width), part(outer.height));
    let x = outer.x + (outer.width - width) / 2;
    let y = match drawn.grows {
        Grows::FromMiddle => outer.y + (outer.height - height) / 2,
        Grows::Down => outer.y,
    };
    Rect::new(x, y, width, height)
}

/// Draws the frame at `drawn` at `parts` of its size, with its shadow, in the lines of `now`.
fn draw_frame(buffer: &mut Buffer, drawn: &Drawn, parts: u32, now: Now) {
    let rect = scaled(drawn, parts);
    Clear.render(rect, buffer);
    buffer.set_style(rect, drawn.style);
    if let Some(shadow) = now.shadow {
        for part in shadow_of(rect) {
            buffer.set_style(part.intersection(drawn.bounds), shadow);
        }
    }
    let margin = drawn.margin;
    let fits = rect.width >= 2 + 2 * margin.horizontal && rect.height >= 2 + 2 * margin.vertical;
    let framed = if fits { rect.inner(margin) } else { rect };
    Block::bordered()
        .border_type(now.border)
        .style(drawn.style)
        .render(framed, buffer);
}

/// Writes the size of the frame at `drawn` as `WIDTHxHEIGHT` at the right of its bottom line,
/// between a line on either side and its corners, if it fits; over the other windows below it,
/// under those drawn after it.
fn draw_size(buffer: &mut Buffer, drawn: &Drawn) {
    let frame = drawn.outer.inner(drawn.margin);
    let formats = noc_locale::current();
    let size = format!(
        "{}x{}",
        formats.integer(u64::from(frame.width)),
        formats.integer(u64::from(frame.height))
    );
    let Ok(width) = u16::try_from(noc_text::width(&size)) else {
        return;
    };
    if frame.height < 2 || frame.width < width.saturating_add(4) {
        return;
    }
    let style = drawn.style.add_modifier(Modifier::REVERSED);
    buffer.set_string(frame.right() - 2 - width, frame.bottom() - 1, size, style);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    use super::super::dialog::{Colors, draw_box};
    use super::*;

    const WIDTH: u16 = 30;
    const HEIGHT: u16 = 11;
    /// The size of the frame that the dialogs of the tests ask for.
    const SIZE: (u16, u16) = (16, 5);

    fn theme() -> Theme {
        Theme::mc_classic()
    }

    /// A screen of dots in the colors of panels, with the dialog `id` on it if there is one,
    /// drawn on `stage` at `at`.
    fn screen(stage: &mut Stage, dialog: Option<u64>, at: Instant) -> Terminal<TestBackend> {
        screen_animated(stage, (dialog, at), true)
    }

    /// As [`screen`], with windows that animate if `on`.
    fn screen_animated(
        stage: &mut Stage,
        (dialog, at): (Option<u64>, Instant),
        on: bool,
    ) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
        let theme = theme();
        terminal
            .draw(|frame| {
                let area = frame.area();
                for y in area.rows() {
                    frame.buffer_mut().set_string(
                        0,
                        y.y,
                        ".".repeat(usize::from(WIDTH)),
                        theme.panel,
                    );
                }
                stage.begin(on, (area, false), &theme, at);
                if let Some(id) = dialog {
                    stage.show(frame, (Window::Dialog(id), Grows::FromMiddle), |frame| {
                        let colors = Colors::of(&theme, false);
                        draw_box(frame, area, SIZE, "Title", colors, &theme);
                    });
                }
                stage.end(frame);
            })
            .unwrap();
        terminal
    }

    fn text(terminal: &Terminal<TestBackend>) -> String {
        terminal.backend().to_string()
    }

    /// A stage that showed the dialog 1 on its first frame, at `start`.
    fn shown(start: Instant) -> Stage {
        let mut stage = Stage::default();
        screen(&mut stage, Some(1), start);
        stage
    }

    #[test]
    fn the_first_frame_shows_its_windows_whole() {
        let start = Instant::now();
        let mut stage = Stage::default();
        let first = screen(&mut stage, Some(1), start);
        assert!(text(&first).contains("Title"));
        assert_eq!(stage.due(), None);
    }

    #[test]
    fn a_window_grows_from_its_middle_and_then_shows_whole() {
        let start = Instant::now();
        let mut stage = shown(start);
        let whole = text(&screen(&mut stage, Some(1), start));
        screen(&mut stage, None, start);
        let opened = start + Duration::from_secs(1);
        let first = screen(&mut stage, Some(2), opened);
        insta::assert_snapshot!("grows_first", text(&first));
        assert_eq!(stage.due(), Some(opened + STEP));
        let half = opened + STEP * 3;
        let middle = screen(&mut stage, Some(2), half);
        insta::assert_snapshot!("grows_halfway", text(&middle));
        assert_eq!(stage.due(), Some(opened + STEP * 4));
        let done = screen(&mut stage, Some(2), opened + STEP * STEPS);
        assert_eq!(text(&done), whole);
        assert_eq!(stage.due(), None);
    }

    #[test]
    fn a_window_shrinks_to_its_middle_as_it_closes() {
        let start = Instant::now();
        let mut stage = shown(start);
        let closed = start + Duration::from_secs(1);
        let first = screen(&mut stage, None, closed);
        insta::assert_snapshot!("shrinks_first", text(&first));
        let last = screen(&mut stage, None, closed + STEP * (STEPS - 1));
        insta::assert_snapshot!("shrinks_last", text(&last));
        assert_eq!(stage.due(), Some(closed + STEP * STEPS));
        let gone = screen(&mut stage, None, closed + STEP * STEPS);
        assert!(!text(&gone).contains('═'), "{}", text(&gone));
        assert_eq!(stage.due(), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_slow_stage_takes_its_whole_time_to_grow_a_window() {
        let start = Instant::now();
        let mut stage = shown(start);
        stage.slow(Duration::from_secs(3));
        screen(&mut stage, None, start);
        let opened = start + Duration::from_secs(10);
        screen(&mut stage, Some(2), opened);
        assert_eq!(stage.due(), Some(opened + Duration::from_millis(500)));
        screen(&mut stage, Some(2), opened + Duration::from_millis(2999));
        assert_eq!(stage.due(), Some(opened + Duration::from_secs(3)));
        screen(&mut stage, Some(2), opened + Duration::from_secs(3));
        assert_eq!(stage.due(), None);
        stage.slow(Duration::ZERO);
        assert_eq!(stage.each, STEP);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_window_writes_its_size_once_it_shows_whole() {
        let start = Instant::now();
        let mut stage = Stage::default();
        stage.show_sizes(true);
        let first = screen(&mut stage, Some(1), start);
        insta::assert_snapshot!("size_shown", text(&first));
        screen(&mut stage, None, start);
        let opened = start + Duration::from_secs(1);
        let growing = screen(&mut stage, Some(2), opened);
        assert!(!text(&growing).contains("16x5"), "{}", text(&growing));
        let whole = screen(&mut stage, Some(2), opened + STEP * STEPS);
        assert_eq!(text(&whole), text(&first));
        stage.show_sizes(false);
        let off = screen(&mut stage, Some(2), opened + STEP * STEPS);
        assert!(!text(&off).contains("16x5"), "{}", text(&off));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_window_writes_its_size_without_animations() {
        let start = Instant::now();
        let mut stage = Stage::default();
        stage.show_sizes(true);
        let shown = screen(&mut stage, Some(1), start);
        let still = screen_animated(&mut stage, (Some(1), start), false);
        assert_eq!(text(&still), text(&shown));
        assert!(text(&still).contains("16x5"), "{}", text(&still));
    }

    #[test]
    fn a_size_needs_room_on_its_frame() {
        let drawn = |width| Drawn {
            outer: Rect::new(0, 0, width, 3),
            margin: Margin::new(0, 0),
            style: Style::default(),
            bounds: Rect::new(0, 0, width, 3),
            grows: Grows::FromMiddle,
        };
        let line = |buffer: &Buffer| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, 2)].symbol().to_owned())
                .collect::<String>()
        };
        let mut buffer = Buffer::filled(Rect::new(0, 0, 7, 3), ratatui::buffer::Cell::new("-"));
        draw_size(&mut buffer, &drawn(7));
        assert_eq!(line(&buffer), "--7x3--");
        let mut buffer = Buffer::filled(Rect::new(0, 0, 6, 3), ratatui::buffer::Cell::new("-"));
        draw_size(&mut buffer, &drawn(6));
        assert_eq!(line(&buffer), "------");
    }

    #[test]
    fn a_window_in_place_of_another_only_opens() {
        let start = Instant::now();
        let mut stage = shown(start);
        let next = start + Duration::from_secs(1);
        screen(&mut stage, Some(2), next);
        assert_eq!(stage.zooms.len(), 1);
        assert!(stage.zooms[0].opens);
    }

    #[test]
    fn a_window_that_stays_does_not_animate() {
        let start = Instant::now();
        let mut stage = shown(start);
        let whole = text(&screen(&mut stage, Some(1), start));
        let later = screen(&mut stage, Some(1), start + Duration::from_secs(1));
        assert_eq!(text(&later), whole);
        assert_eq!(stage.due(), None);
    }

    #[test]
    fn a_settled_frame_shows_what_opened_whole() {
        let start = Instant::now();
        let mut stage = shown(start);
        screen(&mut stage, None, start);
        stage.settle();
        screen(&mut stage, Some(2), start);
        assert_eq!(stage.due(), None);
    }

    #[test]
    fn without_animations_windows_show_and_go_at_once() {
        let start = Instant::now();
        let mut stage = Stage::default();
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
        let theme = theme();
        terminal
            .draw(|frame| {
                stage.begin(false, (frame.area(), false), &theme, start);
                let area = frame.area();
                stage.show(frame, (Window::Help, Grows::FromMiddle), |frame| {
                    draw_box(
                        frame,
                        area,
                        SIZE,
                        "Title",
                        Colors::of(&theme, false),
                        &theme,
                    );
                });
                stage.end(frame);
            })
            .unwrap();
        assert!(text(&terminal).contains("Title"));
        assert_eq!(stage.shown, Vec::new());
        assert_eq!(stage.due(), None);
    }

    #[test]
    fn finds_the_frame_and_the_blank_cells_around_it() {
        let start = Instant::now();
        let stage = shown(start);
        let [(window, drawn)] = stage.shown.as_slice() else {
            panic!("expected one window, got {:?}", stage.shown);
        };
        assert_eq!(*window, Window::Dialog(1));
        let (width, height) = (SIZE.0 + 2, SIZE.1 + 2);
        let outer = Rect::new((WIDTH - width) / 2, (HEIGHT - height) / 2, width, height);
        assert_eq!(drawn.outer, outer);
        assert_eq!(drawn.margin, Margin::new(1, 1));
        assert_eq!(drawn.style.bg, Some(Color::Gray));
    }

    #[test]
    fn a_menu_drops_from_its_top() {
        let drawn = Drawn {
            outer: Rect::new(4, 1, 14, 8),
            margin: Margin::new(0, 0),
            style: Style::new(),
            bounds: Rect::new(0, 0, 30, 10),
            grows: Grows::Down,
        };
        assert_eq!(scaled(&drawn, 1), Rect::new(10, 1, 2, 2));
        assert_eq!(scaled(&drawn, 4), Rect::new(7, 1, 8, 5));
        assert_eq!(scaled(&drawn, STEPS + 1), drawn.outer);
        let middle = Drawn {
            grows: Grows::FromMiddle,
            ..drawn
        };
        assert_eq!(scaled(&middle, 4), Rect::new(7, 2, 8, 5));
    }
}
