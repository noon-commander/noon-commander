//! Quick search through the names of the listing.

use crate::tui::dialog::Field;
use crate::tui::keymap::Action;
use crate::tui::search::{Input, Kind, Matcher, Switch};

use super::{Panel, Row, Search, SearchEnd, typed_at_cursor};

impl Panel {
    /// Whether quick search runs.
    pub(crate) fn searching(&self) -> bool {
        self.search.is_some()
    }

    /// The kind of quick search while it runs, whose keys after Ctrl+s go in front of its own
    /// (ADR 0045).
    pub(crate) fn search_kind(&self) -> Option<Kind> {
        self.search_input().map(Input::kind)
    }

    /// How quick search matches while it runs, and whether case counts.
    pub(crate) fn search_input(&self) -> Option<Input> {
        self.search.as_ref().map(|search| search.input)
    }

    /// What quick search has typed, while it runs.
    pub(crate) fn search_text(&self) -> Option<&str> {
        self.search.as_ref().map(|search| search.field.text())
    }

    /// Starts quick search down the rows, as vim's `/`: to match `count` of what is typed,
    /// from the row after the cursor, round to the top, as `input` matches.
    pub(crate) fn search_down(&mut self, count: usize, input: Input) {
        self.search = Some(Search::new(self.cursor, count, false, self.window, input));
    }

    /// Starts quick search up the rows, as vim's `?`: to match `count` of what is typed, from
    /// the row before the cursor, round to the bottom, as `input` matches.
    pub(crate) fn search_up(&mut self, count: usize, input: Input) {
        self.search = Some(Search::new(self.cursor, count, true, self.window, input));
    }

    /// Switches how quick search matches, or whether case counts, as `switch` says, as the keys
    /// after Ctrl+s do, and searches again.
    pub(crate) fn switch_search(&mut self, switch: Switch) {
        let Some(search) = &mut self.search else {
            return;
        };
        if search.input.switch(switch) {
            self.search_again_typed();
        }
    }

    /// Jumps to the next match down, or the next up, of the search that runs, as vim's Ctrl+g
    /// and Ctrl+t: the count before `/` matches on, from the match the cursor is on. Where the
    /// search goes from moves along, so that Enter stays on this match and an edit searches
    /// on from the match before it; as fzf, the next best or the one before, round.
    pub(crate) fn search_step(&mut self, forward: bool) {
        let Some(search) = &self.search else {
            return;
        };
        let text = search.field.text().to_owned();
        if text.trim().is_empty() {
            return;
        }
        let (count, back) = (search.count, search.back);
        let current = search.found.unwrap_or_else(|| search.start_row());
        let mut matcher = search.input.matcher(&text);
        let ranks = matcher.ranks();
        let target = if ranks {
            self.step_ranked(&mut matcher, current, forward, count)
        } else {
            self.find_from(&mut matcher, 2 * current + 1, !forward, count)
                .map(|(row, _)| row)
        };
        let Some(row) = target else {
            return;
        };
        let rows = self.rows();
        let page = self.page.max(1);
        let Some(search) = &mut self.search else {
            return;
        };
        search.found = Some(row);
        // vim keeps the rows on screen at the match for Esc to put back.
        self.window.unsettle();
        self.cursor = row;
        self.window.follow(&mut self.cursor, rows, page);
        search.view = self.window;
        // Fuzzy search ranks the rows: where it goes from stays.
        if ranks {
            return;
        }
        // As vim's may_adjust_incsearch_highlighting moves search_start.
        let at = 2 * row + 1;
        let before = if row == 0 { 1 } else { 2 * row };
        search.from = 2 * current + 1;
        if !forward && !back {
            search.from = before;
        } else if forward && back {
            search.from = at + 1;
        }
        if forward && at < search.from {
            search.from = if back { at + 1 } else { before };
        }
    }

    /// Searches for `text`, what an earlier search found as `input` matched, as if it were typed
    /// that way: Ctrl+g with nothing typed yet, as mc's Ctrl+s Ctrl+s.
    pub(crate) fn search_resume(&mut self, text: &str, input: Input) {
        let Some(search) = &mut self.search else {
            return;
        };
        search.field = Field::plain(text);
        search.input = input;
        self.search_again_typed();
    }

    /// Moves the cursor `times` matches of `text` as `input` matches on, as vim's `3n`, and puts where
    /// it was in the jump list, once, even if the match is on the same row, as vim does; a
    /// search that finds nothing leaves the jumps as they are. Whether it went round past the
    /// end, or the start, if it found one.
    pub(crate) fn search_again(
        &mut self,
        (text, input): (&str, Input),
        forward: bool,
        times: usize,
    ) -> Option<bool> {
        let here = self.here();
        let mut matcher = input.matcher(text);
        let (row, round) = if matcher.ranks() {
            let ranked = self.ranked(&mut matcher);
            let count = ranked.len();
            let at = ranked.iter().position(|&(row, _)| row == self.cursor);
            let steps = times % count.max(1);
            let (index, round) = match at {
                Some(at) if forward => ((at + steps) % count, at + times >= count),
                Some(at) => ((at + count - steps) % count, times > at),
                None => (0, false),
            };
            (ranked.get(index).map(|&(row, _)| row)?, round)
        } else {
            self.find_from(&mut matcher, 2 * self.cursor + 1, !forward, times)?
        };
        self.push_jump(here);
        self.cursor = row;
        Some(round)
    }

    /// The next match from `current` as fzf ranks them, `count` on or back, round; the best
    /// from a row that does not match.
    fn step_ranked(
        &self,
        matcher: &mut Matcher,
        current: usize,
        forward: bool,
        count: usize,
    ) -> Option<usize> {
        let ranked = self.ranked(matcher);
        let total = ranked.len();
        let steps = count % total.max(1);
        let index = match ranked.iter().position(|&(row, _)| row == current) {
            Some(at) if forward => (at + steps) % total,
            Some(at) => (at + total - steps) % total,
            None => 0,
        };
        ranked.get(index).map(|&(row, _)| row)
    }

    /// Types `c` at the cursor of quick search and searches again, as vim's incremental search
    /// does: from where the search goes from to the first name that matches the text, or, as
    /// fzf, to the best match, the first from there of several as good. As mc's, a character
    /// after which no name matches is not typed; one that leaves a regular expression unfinished
    /// is, so that the rest of it can be typed.
    pub(crate) fn search_type(&mut self, c: char) {
        let Some(search) = &self.search else {
            return;
        };
        let text = typed_at_cursor(&search.field, c);
        let mut matcher = search.input.matcher(&text);
        let found = self.find_typed(search, &mut matcher, &text);
        if found.is_none() && !text.trim().is_empty() && matcher.error().is_none() {
            return;
        }
        let Some(search) = &mut self.search else {
            return;
        };
        search.field.insert(c);
        self.show_found(found);
    }

    /// Edits the text of quick search with a key of the text fields; an edit searches again,
    /// as typing does. Moves of the text cursor leave the panel's where it is.
    pub(crate) fn search_edit(&mut self, action: Action) {
        let Some(search) = &mut self.search else {
            return;
        };
        let before = search.field.text().to_owned();
        search.field.edit(action);
        if search.field.text() != before {
            self.search_again_typed();
        }
    }

    /// Searches for the text typed, from where the search goes from, and puts the rows on
    /// screen back as they were before it, for the cursor to move from there, as vim does.
    fn search_again_typed(&mut self) {
        let Some(search) = &self.search else {
            return;
        };
        let text = search.field.text().to_owned();
        let mut matcher = search.input.matcher(&text);
        let found = self.find_typed(search, &mut matcher, &text);
        self.show_found(found);
    }

    /// The row that `search` goes to for `text`, which `matcher` matches: from where it goes
    /// from to the first name that matches, or, as fzf, to the best match; none for a blank text.
    fn find_typed(&self, search: &Search, matcher: &mut Matcher, text: &str) -> Option<usize> {
        if text.trim().is_empty() {
            None
        } else if matcher.ranks() {
            self.best_ranked(matcher, search)
        } else {
            self.find_from(matcher, search.from, search.back, search.count)
                .map(|(row, _)| row)
        }
    }

    /// Puts the cursor on `found`, what quick search found, or, if nothing, where it goes from,
    /// with the rows on screen as they were before it.
    fn show_found(&mut self, found: Option<usize>) {
        let Some(search) = &mut self.search else {
            return;
        };
        search.found = found;
        self.window.restore(search.view);
        self.window.unsettle();
        self.cursor = found.unwrap_or_else(|| search.start_row());
    }

    /// The best match as fzf ranks them, the first from where `search` goes from of several as
    /// good; with a count before `/` or `?`, the match of that number, round.
    fn best_ranked(&self, matcher: &mut Matcher, search: &Search) -> Option<usize> {
        let rows = self.rows().max(1);
        let start = search.start_row() % rows;
        let back = search.back;
        let after = |row: usize| {
            if back {
                (start + rows - 1 - row) % rows
            } else {
                (row + rows - start - 1) % rows
            }
        };
        let mut ranked = self.ranked(matcher);
        ranked.sort_by_key(|&(row, score)| (std::cmp::Reverse(score), after(row)));
        let count = ranked.len();
        (count > 0).then(|| ranked[(search.count - 1) % count].0)
    }

    /// The row of match `count` of `matcher` from `from`, in half rows: down to the first name
    /// after it that matches, round to the top, or, `back`, up to the first before it, round to
    /// the bottom; and whether the search went round.
    fn find_from(
        &self,
        matcher: &mut Matcher,
        from: usize,
        back: bool,
        count: usize,
    ) -> Option<(usize, bool)> {
        let rows = self.rows();
        let mut from = from;
        let mut found = None;
        let mut round = false;
        // No more steps than rows: round, they come back to where they were.
        for _ in 0..count.clamp(1, rows.max(1)) {
            let mut hit = |row: &usize| {
                self.search_name(*row)
                    .is_some_and(|name| matcher.score(&name).is_some())
            };
            let next = if back {
                (0..rows)
                    .rev()
                    .filter(|row| 2 * row + 1 < from)
                    .chain((0..rows).rev())
                    .find(&mut hit)
            } else {
                (0..rows)
                    .filter(|row| 2 * row + 1 > from)
                    .chain(0..rows)
                    .find(&mut hit)
            }?;
            let at = 2 * next + 1;
            round |= if back { at >= from } else { at <= from };
            from = at;
            found = Some(next);
        }
        found.map(|row| (row, round))
    }

    /// Ends quick search and puts the cursor back on the row it started from, and the rows on
    /// screen as they were, as vim's Esc does.
    pub(crate) fn abort_search(&mut self) {
        if let Some(search) = self.search.take() {
            self.cursor = search.origin.min(self.rows().saturating_sub(1));
            self.window.restore(search.view);
        }
    }

    /// Ends quick search as Enter does in vim: it searches once more from where it goes from,
    /// for the match of the count, and goes there, or stays there if nothing matches. Where it
    /// started goes to the jump list, even if the match is on that row, as vim's; as fzf, the
    /// cursor stays on the match. What it searched for and found, unless nothing was typed.
    pub(crate) fn end_search(&mut self) -> Option<SearchEnd> {
        let search = self.search.take()?;
        let text = search.field.text().to_owned();
        if text.trim().is_empty() {
            return None;
        }
        let start = search.start_row();
        let mut matcher = search.input.matcher(&text);
        let ranks = matcher.ranks();
        let found = if ranks {
            search.found.map(|row| (row, false))
        } else {
            // As vim's Enter: the rows as they were, made right for where it searches from,
            // then for the match.
            self.window.restore(search.view);
            let (rows, page) = (self.rows(), self.page.max(1));
            let mut at = start;
            self.window.follow(&mut at, rows, page);
            self.find_from(&mut matcher, search.from, search.back, search.count)
        };
        let jumped = match found {
            Some((row, _)) if ranks => row != search.origin,
            Some(_) => true,
            // As vim's, which compares the positions with their columns: after Ctrl+g or Ctrl+t,
            // where it searches from may be within the row it started on.
            None => search.from != 2 * search.origin + 1,
        };
        if jumped {
            let origin = self.spot(search.origin);
            self.push_jump(origin);
        }
        self.cursor = found.map_or(start, |(row, _)| row);
        Some(SearchEnd {
            text,
            back: search.back,
            input: search.input,
            found: found.map(|(_, round)| round),
        })
    }

    /// The rows whose names match as fzf matches them, with their scores, best first; rows as
    /// good keep their order. `..` never matches.
    fn ranked(&self, matcher: &mut Matcher) -> Vec<(usize, u32)> {
        let mut ranked: Vec<(usize, u32)> = (0..self.rows())
            .filter_map(|index| Some((index, matcher.score(&self.search_name(index)?)?)))
            .collect();
        ranked.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
        ranked
    }

    /// What quick search looks for while it runs with something typed, to show where names
    /// match it.
    pub(super) fn finder(&self) -> Option<Matcher> {
        let search = self.search.as_ref()?;
        let text = search.field.text();
        if text.trim().is_empty() {
            return None;
        }
        Some(search.input.matcher(text))
    }

    /// The name that quick search finds a row by: the name it shows.
    pub(super) fn search_name(&self, index: usize) -> Option<String> {
        match self.row(index)? {
            Row::Entry(entry) => Some(entry.display_name().into_owned()),
            Row::Parent => None,
        }
    }
}
