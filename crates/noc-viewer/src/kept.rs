//! What the viewer worked out about held lines, kept by line while they are held.

use std::collections::HashMap;
use std::rc::Rc;

/// Lines kept at most: more than a few screens take.
const LINES: usize = 1024;

/// How the held lines changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    /// More came after the last.
    After,
    /// This many came before the first.
    Before(usize),
    /// This many of the first went.
    FirstWent(usize),
    /// The lines from this one on went.
    RestWent(usize),
    /// Others came in their place.
    All,
}

/// What was worked out about held lines, by line: lines that come or go before them move them
/// on or back rather than forget them, so that a line of megabytes is not worked out again
/// for every part read.
#[derive(Debug)]
pub(crate) struct Kept<T: ?Sized> {
    lines: HashMap<usize, Rc<T>>,
}

impl<T: ?Sized> Default for Kept<T> {
    fn default() -> Self {
        Self {
            lines: HashMap::new(),
        }
    }
}

impl<T: ?Sized> Kept<T> {
    /// What line `line` has, made by `make` unless it is kept; with [`LINES`] lines kept, the
    /// others are forgotten first.
    pub(crate) fn get(&mut self, line: usize, make: impl FnOnce() -> Rc<T>) -> Rc<T> {
        if !self.lines.contains_key(&line) && self.lines.len() >= LINES {
            self.lines.clear();
        }
        Rc::clone(self.lines.entry(line).or_insert_with(make))
    }

    pub(crate) fn change(&mut self, change: Change) {
        match change {
            Change::After => {}
            Change::Before(count) => self.shift(|line| line.checked_add(count)),
            Change::FirstWent(count) => self.shift(|line| line.checked_sub(count)),
            Change::RestWent(rest) => self.lines.retain(|&line, _| line < rest),
            Change::All => self.lines.clear(),
        }
    }

    #[cfg(test)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = (usize, &T)> {
        self.lines.iter().map(|(&line, kept)| (line, &**kept))
    }

    fn shift(&mut self, to: impl Fn(usize) -> Option<usize>) {
        self.lines = self
            .lines
            .drain()
            .filter_map(|(line, kept)| Some((to(line)?, kept)))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_keep_what_they_have_as_others_come_and_go() {
        let mut kept = Kept::<usize>::default();
        let mut made = 0;
        let mut get = |kept: &mut Kept<usize>, line| {
            *kept.get(line, || {
                made += 1;
                Rc::new(line)
            })
        };
        for line in 0..4 {
            get(&mut kept, line);
        }
        kept.change(Change::After);
        assert_eq!(get(&mut kept, 3), 3);
        kept.change(Change::Before(2));
        assert_eq!(get(&mut kept, 5), 3, "line 3 is line 5 now");
        kept.change(Change::FirstWent(3));
        assert_eq!(get(&mut kept, 0), 1, "the two that came went, and line 0");
        kept.change(Change::RestWent(2));
        assert_eq!(get(&mut kept, 2), 2, "made again");
        kept.change(Change::All);
        assert_eq!(get(&mut kept, 0), 0);
        assert_eq!(made, 6);

        let mut kept = Kept::<usize>::default();
        for line in 0..=LINES {
            kept.get(line, || Rc::new(line));
        }
        assert_eq!(kept.lines.len(), 1, "forgotten all at once");
    }
}
