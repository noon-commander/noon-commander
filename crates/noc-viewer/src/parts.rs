//! The parts of a file the viewer read, and the ones whose lines it holds.

use std::ops::Range;

use crate::read::Want;

/// A part of the file read at least once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Known {
    pub(crate) offset: u64,
    pub(crate) len: u64,
    /// The lines the viewer makes of it.
    pub(crate) lines: usize,
}

impl Known {
    fn end(&self) -> u64 {
        self.offset + self.len
    }
}

/// Every part of the file read, and the ones held: the parts read in order from its start on,
/// one after another, and after a jump the parts read from there on, one after another too,
/// apart from the first ones. What lies between was never read, so the lines of the parts apart
/// have no numbers until the parts read in order reach them.
#[derive(Debug, Default)]
pub(crate) struct Parts {
    known: Vec<Known>,
    /// Whether the last known part ends the file.
    end: bool,
    /// The index of the first part apart from the ones read in order, if a jump left any.
    apart: Option<usize>,
    /// The parts whose lines the viewer holds, one after another, never from both sides of the
    /// jump.
    held: Range<usize>,
    /// The size of the file, when it reports one.
    size: Option<u64>,
}

impl Parts {
    pub(crate) fn known(&self) -> &[Known] {
        &self.known
    }

    pub(crate) fn held(&self) -> Range<usize> {
        self.held.clone()
    }

    pub(crate) fn set_held(&mut self, held: Range<usize>) {
        self.held = held;
    }

    pub(crate) fn size(&self) -> Option<u64> {
        self.size
    }

    pub(crate) fn set_size(&mut self, size: Option<u64>) {
        self.size = size;
    }

    /// Where the next part read in order from the start begins.
    pub(crate) fn frontier(&self) -> u64 {
        let in_order = self.apart.unwrap_or(self.known.len());
        self.known[..in_order].last().map_or(0, Known::end)
    }

    /// Where the last known part ends.
    pub(crate) fn last_end(&self) -> u64 {
        self.known.last().map_or(0, Known::end)
    }

    /// Where the parts apart from the ones read in order start, if a jump left any.
    pub(crate) fn apart_start(&self) -> Option<u64> {
        self.apart.map(|index| self.known[index].offset)
    }

    /// Whether the last known part ends the file.
    pub(crate) fn ended(&self) -> bool {
        self.end
    }

    /// Whether the file was read in order to its end, with no part left to read.
    pub(crate) fn all_known(&self) -> bool {
        self.end && self.apart.is_none()
    }

    /// Notes that the file ends with the last known part.
    pub(crate) fn ends(&mut self) {
        self.end = true;
    }

    /// Adds the part read for `want` where it belongs, and gives its index: after the last
    /// part read in order, after the last of those apart, before the first of them, or, read
    /// by a jump, in their place. `None` if it belongs nowhere, as a part asked for before the
    /// others changed.
    pub(crate) fn add(&mut self, part: Known, want: Want, end: bool) -> Option<usize> {
        let start = self.apart_start();
        let in_order = self.apart.unwrap_or(self.known.len());
        let reaches = start == Some(part.end());
        match (want.new, want.after_feed) {
            (true, false) if part.offset == self.frontier() => {
                match start {
                    Some(start) if part.end() > start => return None,
                    Some(_) => {}
                    None if self.end => return None,
                    None => self.end = end,
                }
                self.insert(in_order, part);
                // The parts apart are in order too once the others reach them.
                self.apart = (!reaches && start.is_some()).then_some(in_order + 1);
                Some(in_order)
            }
            (true, false) if start.is_some() && !self.end && part.offset == self.last_end() => {
                self.known.push(part);
                self.end = end;
                Some(self.known.len() - 1)
            }
            (true, true) if part.offset > self.frontier() => {
                if self.held.end > in_order {
                    self.held = in_order..in_order;
                }
                self.known.truncate(in_order);
                self.known.push(part);
                self.apart = Some(in_order);
                self.end = end;
                Some(in_order)
            }
            (false, true) if reaches && part.offset >= self.frontier() => {
                self.insert(in_order, part);
                if part.offset == self.frontier() {
                    self.apart = None;
                }
                Some(in_order)
            }
            _ => None,
        }
    }

    /// Puts `part` at `index`; the held parts from there on move on by one.
    fn insert(&mut self, index: usize, part: Known) {
        self.known.insert(index, part);
        if !self.held.is_empty() && self.held.start >= index {
            self.held = self.held.start + 1..self.held.end + 1;
        }
    }

    /// The index of the known part at `offset`.
    pub(crate) fn at(&self, offset: u64) -> Option<usize> {
        self.known.iter().position(|part| part.offset == offset)
    }

    /// What to ask for to read part `index` again.
    pub(crate) fn again(&self, index: usize) -> Option<Want> {
        let part = self.known.get(index)?;
        Some(Want {
            offset: part.offset,
            length: part.len,
            most: part.len,
            new: false,
            after_feed: false,
            ahead: false,
        })
    }

    /// What to ask for to read the next part in order from the start, of `length` bytes, or
    /// `most` where a line goes on: no further than the parts apart, which it then joins.
    pub(crate) fn in_order(&self, length: u64, most: u64) -> Option<Want> {
        let offset = self.frontier();
        let room = match self.apart_start() {
            Some(start) => start - offset,
            None if self.end => return None,
            None => u64::MAX,
        };
        Some(Want {
            offset,
            length: length.min(room),
            most: most.min(room),
            new: true,
            after_feed: false,
            ahead: false,
        })
    }

    /// What to ask for to read a new part after the last known one, of `length` bytes, or
    /// `most` where a line goes on.
    pub(crate) fn on(&self, length: u64, most: u64) -> Option<Want> {
        (!self.end).then(|| Want {
            offset: self.last_end(),
            length,
            most,
            new: true,
            after_feed: false,
            ahead: false,
        })
    }

    /// What to ask for to read the part after the held ones.
    pub(crate) fn next(&self, length: u64, most: u64) -> Option<Want> {
        if self.apart == Some(self.held.end) {
            return self.in_order(length, most);
        }
        self.again(self.held.end).or_else(|| self.on(length, most))
    }

    /// What to ask for to read the part before the held ones: before the parts apart, what
    /// lies between them and the ones read in order if it is no more than `length` bytes, else
    /// the `length` bytes before them.
    pub(crate) fn previous(&self, length: u64) -> Option<Want> {
        if self.held.is_empty() {
            return None;
        }
        if self.apart != Some(self.held.start) {
            return self.again(self.held.start.checked_sub(1)?);
        }
        let start = self.known[self.held.start].offset;
        if start - self.frontier() <= length {
            return self.in_order(length, length);
        }
        Some(Want {
            offset: start - length,
            length,
            most: length,
            new: false,
            after_feed: true,
            ahead: false,
        })
    }

    /// Whether known part `index` comes right after the held ones in the file.
    pub(crate) fn follows(&self, index: usize) -> bool {
        !self.held.is_empty() && index == self.held.end && self.apart != Some(index)
    }

    /// Whether known part `index` comes right before the held ones in the file.
    pub(crate) fn precedes(&self, index: usize) -> bool {
        !self.held.is_empty() && index + 1 == self.held.start && self.apart != Some(index + 1)
    }

    /// Whether the file has more after the held parts.
    pub(crate) fn more_after(&self) -> bool {
        self.held.end < self.known.len() || !self.end
    }

    /// Whether the file has more before the held parts.
    pub(crate) fn more_before(&self) -> bool {
        !self.held.is_empty() && self.known[self.held.start].offset > 0
    }

    /// Whether the held parts are the whole file.
    pub(crate) fn whole(&self) -> bool {
        !self.more_before() && !self.more_after()
    }

    /// The first line of part `index` in the file, unless it is apart from the ones read in
    /// order.
    pub(crate) fn line_of(&self, index: usize) -> Option<usize> {
        if self.apart.is_some_and(|apart| index >= apart) {
            return None;
        }
        let before = &self.known[..index.min(self.known.len())];
        Some(before.iter().map(|part| part.lines).sum())
    }

    /// The first line held, in the file, if its number is known.
    pub(crate) fn first_line(&self) -> Option<usize> {
        self.line_of(self.held.start)
    }

    /// The lines of the file, once every part is known.
    pub(crate) fn lines(&self) -> Option<usize> {
        self.all_known()
            .then(|| self.known.iter().map(|part| part.lines).sum())
    }

    /// The bytes held.
    pub(crate) fn held_bytes(&self) -> u64 {
        self.known[self.held.clone()]
            .iter()
            .map(|part| part.len)
            .sum()
    }

    /// The index of the part with line `line` of the file, among the ones read in order.
    pub(crate) fn part_of_line(&self, line: usize) -> Option<usize> {
        let in_order = self.apart.unwrap_or(self.known.len());
        let mut first = 0;
        self.known[..in_order].iter().position(|part| {
            first += part.lines;
            line < first
        })
    }

    /// The index of the known part with byte `byte` of the file.
    pub(crate) fn part_of_byte(&self, byte: u64) -> Option<usize> {
        self.known
            .iter()
            .position(|part| (part.offset..part.end()).contains(&byte))
    }

    /// The held line about where byte `byte` of held part `index` is, as if its lines were of
    /// one length.
    pub(crate) fn held_line_at(&self, index: usize, byte: u64) -> usize {
        let part = self.known[index];
        let before = &self.known[self.held.start.min(index)..index];
        let into = byte.saturating_sub(part.offset).min(part.len);
        let lines = u64::try_from(part.lines).unwrap_or(u64::MAX);
        let line = (into * lines).checked_div(part.len).unwrap_or(0);
        before.iter().map(|part| part.lines).sum::<usize>()
            + usize::try_from(line).unwrap_or(usize::MAX)
    }

    /// The byte of the file about where held line `line` starts, as if the lines of its part
    /// were of one length; after the last, the end of the held parts.
    pub(crate) fn byte_at(&self, mut line: usize) -> u64 {
        for part in &self.known[self.held.clone()] {
            if line < part.lines {
                let line = u64::try_from(line).unwrap_or(u64::MAX);
                let lines = u64::try_from(part.lines).unwrap_or(u64::MAX);
                return part.offset + (line * part.len).checked_div(lines).unwrap_or(0);
            }
            line -= part.lines;
        }
        self.known
            .get(self.held.end.wrapping_sub(1))
            .map_or(0, Known::end)
    }

    /// The bytes of the file as far as known: its size, or more when it grew.
    pub(crate) fn extent(&self) -> u64 {
        self.size.unwrap_or(0).max(self.last_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEXT: Want = Want {
        offset: 0,
        length: 0,
        most: 0,
        new: true,
        after_feed: false,
        ahead: false,
    };
    const JUMP: Want = Want {
        after_feed: true,
        ..NEXT
    };
    const BEFORE: Want = Want { new: false, ..JUMP };

    fn known(offset: u64, len: u64, lines: usize) -> Known {
        Known { offset, len, lines }
    }

    fn parts() -> Parts {
        let mut parts = Parts::default();
        assert_eq!(parts.add(known(0, 100, 10), NEXT, false), Some(0));
        assert_eq!(parts.add(known(100, 50, 5), NEXT, false), Some(1));
        parts.set_held(1..2);
        parts
    }

    #[test]
    fn finds_parts_by_line_and_byte() {
        let parts = parts();
        assert_eq!(parts.first_line(), Some(10));
        assert_eq!(parts.part_of_line(9), Some(0));
        assert_eq!(parts.part_of_line(10), Some(1));
        assert_eq!(parts.part_of_line(15), None);
        assert_eq!(parts.part_of_byte(120), Some(1));
        assert_eq!(parts.part_of_byte(150), None);
        assert_eq!(parts.held_line_at(1, 120), 2);
        assert_eq!(parts.byte_at(2), 120);
        assert_eq!(parts.byte_at(5), 150, "after the last line");
        assert_eq!(parts.lines(), None);
    }

    #[test]
    fn asks_for_the_parts_around_the_held_ones() {
        let mut parts = parts();
        let next = Want {
            offset: 150,
            length: 64,
            most: 96,
            ..NEXT
        };
        assert_eq!(parts.next(64, 96), Some(next));
        let first = Want {
            length: 100,
            most: 100,
            new: false,
            ..NEXT
        };
        assert_eq!(parts.previous(64), Some(first));
        parts.ends();
        assert_eq!(parts.next(64, 96), None);
        assert!(!parts.more_after() && parts.more_before() && !parts.whole());
        assert_eq!(parts.lines(), Some(15));
        assert_eq!(parts.add(known(150, 10, 1), NEXT, true), None, "all known");
    }

    #[test]
    fn a_jump_leaves_parts_apart_whose_lines_have_no_numbers() {
        let mut parts = parts();
        assert_eq!(parts.add(known(900, 100, 10), JUMP, true), Some(2));
        assert_eq!(
            (parts.frontier(), parts.last_end(), parts.apart_start()),
            (150, 1000, Some(900))
        );
        assert!(parts.ended() && !parts.all_known());
        assert_eq!(parts.lines(), None, "what lies between was not counted");
        assert_eq!(parts.held(), 1..2, "the held parts stay");
        assert!(!parts.follows(2), "not across what lies between");
        assert_eq!(parts.part_of_byte(500), None);
        assert_eq!(parts.part_of_line(20), None, "no line apart has a number");
        let on = Want {
            offset: 150,
            length: 64,
            most: 96,
            ..NEXT
        };
        assert_eq!(parts.next(64, 96), Some(on), "on in order");

        parts.set_held(2..3);
        assert!(parts.more_before() && !parts.more_after());
        assert_eq!((parts.first_line(), parts.byte_at(5)), (None, 950));
        let before = Want {
            offset: 836,
            length: 64,
            most: 64,
            ..BEFORE
        };
        assert_eq!(parts.previous(64), Some(before));
        assert_eq!(parts.add(known(840, 60, 6), BEFORE, false), Some(2));
        assert_eq!((parts.held(), parts.apart_start()), (3..4, Some(840)));
        assert!(parts.precedes(2) && !parts.precedes(1));
        assert_eq!(
            parts.add(known(700, 60, 6), BEFORE, false),
            None,
            "it does not reach them"
        );

        // Another jump takes their place, and the place of the held ones among them.
        assert_eq!(parts.add(known(500, 100, 10), JUMP, false), Some(2));
        assert_eq!((parts.known().len(), parts.held()), (3, 2..2));
        assert!(!parts.ended());
        let after = Want {
            offset: 600,
            length: 64,
            most: 96,
            ..NEXT
        };
        assert_eq!(parts.on(64, 96), Some(after));
        assert_eq!(parts.add(known(600, 50, 5), NEXT, false), Some(3));
    }

    #[test]
    fn the_parts_read_in_order_join_the_ones_apart() {
        let mut parts = parts();
        parts.add(known(400, 100, 10), JUMP, true);
        parts.set_held(2..3);
        // No further than the parts apart, a line that goes on neither.
        let most = Want {
            offset: 150,
            length: 100,
            most: 250,
            ..NEXT
        };
        assert_eq!(parts.in_order(100, 1000), Some(most));
        assert_eq!(parts.add(known(150, 300, 30), NEXT, false), None);
        assert_eq!(parts.add(known(150, 100, 10), NEXT, false), Some(2));
        assert_eq!((parts.held(), parts.apart_start()), (3..4, Some(400)));
        // What lies between, once no more than a part, is read whole.
        let between = Want {
            offset: 250,
            length: 150,
            most: 150,
            ..NEXT
        };
        assert_eq!(parts.previous(200), Some(between));
        assert_eq!(parts.add(known(250, 150, 15), NEXT, false), Some(3));
        assert_eq!((parts.held(), parts.apart_start()), (4..5, None));
        assert!(parts.all_known() && parts.precedes(3));
        assert_eq!((parts.first_line(), parts.lines()), (Some(40), Some(50)));
    }
}
