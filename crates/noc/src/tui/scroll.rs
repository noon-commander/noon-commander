//! Rows on screen as vim's window shows lines: a port of vim 9.1's scrolling (move.c) for lines
//! one row high, without folds, with 'scrolloff' at [`SCROLLOFF`].
//!
//! A command moves the cursor or scrolls ([`Window::half_page`], [`Window::page`],
//! [`Window::scroll`]). Then [`Window::follow`], called before every render, does what vim does
//! before it redraws.

/// Rows kept on screen above and below the cursor, as vim's defaults.vim sets 'scrolloff'.
pub(crate) const SCROLLOFF: usize = 5;

/// Bounds rows, heights and counts so that sums of a few of them never overflow.
const LIMIT: i64 = 1 << 48;

/// Which rows of a list show, as vim's window shows the lines of a buffer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Window {
    top: usize,
    /// The cursor the rows were last made right for: vim's `VALID_TOPLINE` and `w_valid_cursor`.
    settled: Option<usize>,
}

impl Window {
    /// The first row on screen.
    pub(crate) fn top(self) -> usize {
        self.top
    }

    /// Shows the list from its first row again, as a new listing does.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Clamps `cursor` to `rows`, then scrolls so that it shows with [`SCROLLOFF`] rows around
    /// it, or centres it after a far jump, as vim's `update_topline`. Leaves rows settled for
    /// this cursor alone.
    pub(crate) fn follow(&mut self, cursor: &mut usize, rows: usize, height: usize) {
        self.with(cursor, rows, height, Win::update_topline);
    }

    /// Ctrl+d (`down`) and Ctrl+u: scrolls half a window and moves the cursor as far, as vim's
    /// `pagescroll` with 'scroll' at half the height.
    pub(crate) fn half_page(&mut self, down: bool, cursor: &mut usize, rows: usize, height: usize) {
        self.with(cursor, rows, height, |w| w.pagescroll(down, true));
    }

    /// Ctrl+f (`down`) and Ctrl+b: scrolls a window less up to two rows, as vim's `pagescroll`.
    /// Like vim, Ctrl+f at the end leaves only the last rows at the top.
    pub(crate) fn page(&mut self, down: bool, cursor: &mut usize, rows: usize, height: usize) {
        self.with(cursor, rows, height, |w| w.pagescroll(down, false));
    }

    /// The mouse wheel: scrolls `count` rows and keeps the cursor inside, as vim's
    /// `scroll_redraw` (Ctrl+e and Ctrl+y), but never so far down that rows past the end show;
    /// where it cannot scroll, it does nothing.
    pub(crate) fn scroll(
        &mut self,
        down: bool,
        count: usize,
        cursor: &mut usize,
        rows: usize,
        height: usize,
    ) {
        self.with(cursor, rows, height, |w| {
            let mut count = wide(count);
            if count == 0 {
                return;
            }
            if down {
                let last_top = (w.n - w.h + 1).max(1);
                count = count.min(last_top - w.top).max(0);
            }
            if count > 0 {
                w.scroll_redraw(down, count);
            }
        });
    }

    fn with(&mut self, cursor: &mut usize, rows: usize, height: usize, f: impl FnOnce(&mut Win)) {
        if let Some(mut win) = Win::load(*self, *cursor, rows, height) {
            f(&mut win);
            win.store(self, cursor);
        } else {
            *self = Self::default();
            *cursor = 0;
        }
    }
}

fn wide(x: usize) -> i64 {
    i64::try_from(x).map_or(LIMIT, |x| x.min(LIMIT))
}

fn narrow(x: i64) -> usize {
    usize::try_from(x).unwrap_or(0)
}

fn so() -> i64 {
    wide(SCROLLOFF)
}

/// vim's window over the list, with vim's line numbers from 1.
struct Win {
    /// Lines in the buffer.
    n: i64,
    /// Rows in the window.
    h: i64,
    top: i64,
    cur: i64,
    /// vim's `VALID_TOPLINE`.
    valid: bool,
    /// vim's `w_valid_cursor`.
    valid_cur: i64,
}

impl Win {
    fn load(window: Window, cursor: usize, rows: usize, height: usize) -> Option<Self> {
        if rows == 0 {
            return None;
        }
        let n = wide(rows);
        let cur = wide(cursor).min(n - 1) + 1;
        Some(Self {
            n,
            h: wide(height).max(1),
            top: wide(window.top).min(n - 1) + 1,
            cur,
            valid: window.settled.is_some(),
            valid_cur: window.settled.map_or(cur, |row| wide(row) + 1),
        })
    }

    fn store(&self, window: &mut Window, cursor: &mut usize) {
        window.top = narrow(self.top - 1);
        window.settled = self.valid.then(|| narrow(self.valid_cur - 1));
        *cursor = narrow(self.cur - 1);
    }

    /// vim's `w_botline`: the first line below the window.
    fn bot(&self) -> i64 {
        (self.top + self.h).min(self.n + 1)
    }

    /// vim's `check_cursor_moved`.
    fn check_moved(&mut self) {
        if self.cur != self.valid_cur {
            self.valid = false;
            self.valid_cur = self.cur;
        }
    }

    fn settle(&mut self) {
        self.check_moved();
        self.valid = true;
    }

    /// vim's `update_topline`.
    fn update_topline(&mut self) {
        self.check_moved();
        // vim's window never loses its cursor; a list or a window that shrank can.
        let shows = self.top <= self.cur && self.cur < self.top + self.h;
        if self.valid && shows {
            return;
        }
        self.valid = true;
        let so = so();
        if self.top > 1 && self.cur < self.top + so {
            let halfheight = (self.h / 2 - 1).max(2);
            if self.top + so - self.cur >= halfheight {
                self.halfway(false);
                return;
            }
            self.cursor_top();
        }
        let bot = self.bot();
        if bot <= self.n && self.cur >= bot - so {
            if self.cur - bot + 1 + so <= self.h + 1 {
                self.cursor_bot();
            } else {
                self.halfway(false);
            }
        }
    }

    /// vim's `scroll_cursor_halfway`: puts the cursor in the middle of the window. Rows past the
    /// end count as below the cursor.
    fn halfway(&mut self, prefer_above: bool) {
        let (mut above, mut below, mut used) = (0, 0, 1);
        let (mut topline, mut bo) = (self.cur, self.cur);
        'fill: while topline > 1 {
            for round in 1..=2 {
                let add_below = if prefer_above {
                    round == 2 && below < above
                } else {
                    round == 1 && below <= above
                };
                if add_below {
                    if bo < self.n {
                        bo += 1;
                        used += 1;
                        if used > self.h {
                            break 'fill;
                        }
                    }
                    below += 1;
                }
                let add_above = round == 1
                    && if prefer_above {
                        below >= above
                    } else {
                        below > above
                    };
                if add_above {
                    used += 1;
                    if used > self.h {
                        break 'fill;
                    }
                    above += 1;
                    topline -= 1;
                }
            }
        }
        self.top = topline;
    }

    /// vim's `scroll_cursor_top` with 'scrolljump' at 1.
    fn cursor_top(&mut self) {
        let so = so();
        let mut scrolled = i64::from(self.cur < self.top);
        let (mut extra, mut used) = (0, 1);
        let (mut top, mut bot) = (self.cur - 1, self.cur + 1);
        let mut new_topline = self.cur;
        while top > 0 {
            if top < self.top {
                scrolled += 1;
            }
            if (new_topline >= self.top || scrolled > 1) && extra >= so {
                break;
            }
            used += 1;
            if extra < so && bot < self.n {
                used += 1;
            }
            if used > self.h {
                break;
            }
            extra += 1;
            new_topline = top;
            top -= 1;
            bot += 1;
        }
        if used > self.h {
            self.halfway(false);
        } else {
            self.top = self.top.min(new_topline).min(self.cur);
        }
    }

    /// vim's `scroll_cursor_bot` with 'scrolljump' at 1, for a window without rows past the end.
    fn cursor_bot(&mut self) {
        let so = so();
        let botline = self.bot();
        let below_window = |line: i64| i64::from(line >= botline);
        let mut scrolled = below_window(self.cur);
        let (mut extra, mut used) = (0, 1);
        let (mut lo, mut bo) = (self.cur, self.cur);
        while lo > 1 {
            if (extra >= so || bo + 1 > self.n) && lo <= botline {
                break;
            }
            lo -= 1;
            used += 1;
            if used > self.h {
                break;
            }
            scrolled += below_window(lo);
            if bo < self.n {
                bo += 1;
                used += 1;
                if used > self.h {
                    break;
                }
                if extra < so || scrolled < 1 {
                    extra += 1;
                    scrolled += below_window(bo);
                }
            }
        }
        let line_count = if scrolled <= 0 {
            0
        } else if used > self.h {
            used
        } else {
            scrolled
        };
        if line_count >= self.h && line_count > 1 {
            self.halfway(true);
        } else if line_count > 0 {
            self.scrollup(line_count);
        }
    }

    /// vim's `scrollup`: the window moves `count` lines down the buffer.
    fn scrollup(&mut self, count: i64) {
        self.top = (self.top + count).min(self.n);
        self.cur = self.cur.max(self.top);
    }

    /// vim's `scrolldown`: the window moves `count` lines up the buffer.
    fn scrolldown(&mut self, count: i64) {
        self.top = (self.top - count).max(1);
        self.cur = self.cur.min(self.top + self.h - 1).max(self.top);
    }

    /// vim's `scroll_redraw`.
    fn scroll_redraw(&mut self, up: bool, count: i64) {
        let (prev_top, prev_cur) = (self.top, self.cur);
        if up {
            self.scrollup(count);
        } else {
            self.scrolldown(count);
        }
        self.cursor_correct();
        self.settle();
        // Like vim, move the cursor at least when the window cannot move.
        while self.top == prev_top {
            if up {
                if self.cur > prev_cur || self.cur >= self.n {
                    break;
                }
                self.cur += 1;
            } else {
                if self.cur < prev_cur || prev_top == 1 || self.cur <= 1 {
                    break;
                }
                self.cur -= 1;
            }
            self.settle();
        }
    }

    /// vim's `cursor_correct`: moves the cursor so that 'scrolloff' lines show around it, fewer at
    /// either end of the buffer.
    fn cursor_correct(&mut self) {
        let so = so();
        let (mut above_wanted, mut below_wanted) = (so, so);
        let bot = self.bot();
        if self.top == 1 {
            above_wanted = 0;
            below_wanted = below_wanted.min(self.h / 2);
        }
        if bot == self.n + 1 {
            below_wanted = 0;
            above_wanted = above_wanted.min((self.h - 1) / 2);
        }
        let cln = self.cur;
        if cln >= self.top + above_wanted && cln < bot - below_wanted {
            return;
        }
        let (mut topline, mut botline) = (self.top, bot - 1);
        let (mut above, mut below) = (0, 0);
        while (above < above_wanted || below < below_wanted) && topline < botline {
            if below < below_wanted && (below <= above || above >= above_wanted) {
                below += 1;
                botline -= 1;
            }
            if above < above_wanted && (above < below || below >= below_wanted) {
                above += 1;
                topline += 1;
            }
        }
        if topline >= botline {
            self.cur = botline;
        } else if cln < topline && self.top > 1 {
            self.cur = topline;
        } else if cln > botline && bot <= self.n {
            self.cur = botline;
        }
        // move.c marks only the top line valid here, yet vim ends up settled for this cursor too.
        self.settle();
    }

    /// vim's `get_scroll_overlap`: Ctrl+f and Ctrl+b keep two lines of the old window, fewer in
    /// a low window or near either end.
    fn overlap(&self, forward: bool) -> i64 {
        let edge = if forward {
            self.bot() > self.n
        } else {
            self.top == 1
        };
        if edge || self.h < 4 {
            return self.h;
        }
        let exists = |k: i64| {
            let line = if forward {
                self.bot() - k
            } else {
                self.top - 1 + k
            };
            (1..=self.n).contains(&line)
        };
        if !exists(1) || !exists(2) {
            self.h
        } else if !exists(3) || self.h < 5 {
            self.h - 1
        } else {
            self.h - 2
        }
    }

    /// vim's `pagescroll`.
    fn pagescroll(&mut self, forward: bool, half: bool) {
        if half {
            let prev = self.cur;
            let curscount = (self.h / 2).max(1);
            let mut count = curscount;
            // Like vim, Ctrl+d stops scrolling when the last line shows.
            if forward && self.top + self.h + count > self.n {
                let lines = 1 + (self.n - self.top).min(self.h + count);
                count = count.min(lines - self.h);
            }
            if count > 0 {
                self.scroll_redraw(forward, count);
                self.cur = prev;
            }
            self.cur = if forward {
                (self.cur + curscount).min(self.n)
            } else {
                (self.cur - curscount).max(1)
            };
        } else {
            let prev_top = self.top;
            self.scroll_redraw(forward, self.overlap(forward));
            if self.top != prev_top {
                self.cur = if forward { self.top } else { self.bot() - 1 };
            }
        }
        self.cursor_correct();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays vim keys on a new window and gives `cursor,top` after each, as vim 9.1 recorded
    /// them. `N^E` and `N^Y` stand for the wheel.
    fn replay(rows: usize, height: usize, keys: &str) -> String {
        let (mut window, mut cursor) = (Window::default(), 0);
        let mut out = Vec::new();
        for key in keys.split(' ') {
            if let Some((count, wheel)) =
                key.split_once('^').filter(|(_, k)| ["E", "Y"].contains(k))
            {
                let count = count.parse().unwrap_or(1);
                window.scroll(wheel == "E", count, &mut cursor, rows, height);
            } else {
                match key {
                    "^D" | "^U" => window.half_page(key == "^D", &mut cursor, rows, height),
                    "^F" | "^B" => window.page(key == "^F", &mut cursor, rows, height),
                    "gg" => cursor = 0,
                    "G" => cursor = rows.saturating_sub(1),
                    _ => {
                        let (count, motion) = key.split_at(key.len() - 1);
                        let count: usize = count.parse().unwrap_or(1);
                        match motion {
                            "j" => cursor += count,
                            "k" => cursor = cursor.saturating_sub(count),
                            _ => cursor = count - 1,
                        }
                    }
                }
            }
            window.follow(&mut cursor, rows, height);
            out.push(format!("{cursor},{}", window.top()));
        }
        out.join(" ")
    }

    #[test]
    fn j_past_the_bottom_margin_scrolls_one_row_at_a_time() {
        assert_eq!(replay(82, 15, "12j j j"), "12,3 13,4 14,5");
    }

    #[test]
    fn a_far_jump_centres_the_cursor() {
        assert_eq!(replay(100, 10, "50G 20G"), "49,45 19,15");
    }

    #[test]
    fn g_and_gg_show_the_last_and_the_first_rows() {
        assert_eq!(replay(40, 12, "G gg"), "39,28 0,0");
    }

    #[test]
    fn ctrl_d_in_the_middle_scrolls_half_a_window() {
        assert_eq!(replay(100, 11, "^D ^D"), "10,5 15,10");
    }

    #[test]
    fn ctrl_d_at_the_end_moves_only_the_cursor() {
        assert_eq!(replay(111, 15, "107G ^D ^D"), "106,96 110,96 110,96");
    }

    #[test]
    fn ctrl_u_at_the_top_moves_only_the_cursor() {
        assert_eq!(replay(100, 10, "7j ^U ^U"), "7,2 2,0 0,0");
    }

    #[test]
    fn ctrl_f_at_the_end_leaves_the_last_rows_at_the_top_as_vim() {
        assert_eq!(replay(10, 8, "^F ^F"), "9,6 9,9");
    }

    #[test]
    fn ctrl_b_goes_back_a_window_less_two_rows() {
        assert_eq!(replay(100, 10, "^F ^F ^B"), "12,8 20,16 12,8");
    }

    #[test]
    fn tiny_windows_keep_what_margin_fits() {
        assert_eq!(
            replay(100, 1, "37G 32G ^B 5k ^F"),
            "36,36 31,31 30,30 25,25 26,26"
        );
        assert_eq!(
            replay(24, 3, "^D 12j 12j 12j 3G"),
            "2,1 14,13 23,21 23,21 2,1"
        );
    }

    #[test]
    fn a_list_shorter_than_the_window_scrolls_only_with_ctrl_f() {
        assert_eq!(replay(8, 23, "^D ^D ^F ^F ^B"), "7,0 7,0 7,7 7,7 7,0");
    }

    #[test]
    fn no_rows_or_huge_values_never_panic() {
        assert_eq!(
            replay(0, 10, "j ^D ^F G ^B ^U 3^E 3^Y"),
            ["0,0"; 8].join(" ")
        );
        let (mut window, mut cursor) = (Window::default(), usize::MAX);
        window.follow(&mut cursor, usize::MAX, 0);
        window.page(true, &mut cursor, usize::MAX, usize::MAX);
        window.half_page(true, &mut cursor, 3, usize::MAX);
        window.scroll(true, usize::MAX, &mut cursor, usize::MAX, 40);
        window.follow(&mut cursor, 3, 0);
        assert_eq!((cursor, window.top()), (2, 2));
    }

    #[test]
    fn the_wheel_scrolls_as_ctrl_e_and_ctrl_y() {
        assert_eq!(replay(100, 10, "3^E 3^E 3^Y"), "7,3 10,6 7,3");
    }

    #[test]
    fn the_wheel_never_scrolls_past_the_end() {
        // vim goes on to "16,12 19,15".
        assert_eq!(
            replay(20, 10, "3^E 3^E 3^E 3^E 3^E"),
            "7,3 10,6 13,9 14,10 14,10"
        );
        assert_eq!(replay(5, 10, "3^E 3^E"), "0,0 0,0");
    }

    #[test]
    fn a_smaller_window_or_list_brings_the_settled_cursor_back_on_screen() {
        let (mut window, mut cursor) = (Window::default(), 12);
        window.follow(&mut cursor, 82, 15);
        window.follow(&mut cursor, 82, 5);
        assert_eq!((cursor, window.top()), (12, 10));
        window.follow(&mut cursor, 8, 5);
        assert_eq!((cursor, window.top()), (7, 3));
    }
}
