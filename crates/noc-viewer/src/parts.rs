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

/// Every part of the file read, from its start on, one after another, and the ones held.
#[derive(Debug, Default)]
pub(crate) struct Parts {
    known: Vec<Known>,
    /// Whether the last known part ends the file.
    end: bool,
    /// The parts whose lines the viewer holds, one after another.
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

    pub(crate) fn set_size(&mut self, size: Option<u64>) {
        self.size = size;
    }

    /// Where the next new part starts.
    pub(crate) fn frontier(&self) -> u64 {
        self.known.last().map_or(0, |last| last.offset + last.len)
    }

    /// Whether the file has no part left to read.
    pub(crate) fn all_known(&self) -> bool {
        self.end
    }

    /// Adds the new part after the last, and gives its index.
    pub(crate) fn push(&mut self, part: Known, end: bool) -> usize {
        self.known.push(part);
        self.end = end;
        self.known.len() - 1
    }

    /// Notes that the file ends with the last known part.
    pub(crate) fn ends(&mut self) {
        self.end = true;
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
            new: false,
        })
    }

    /// What to ask for to read a new part of `length` bytes.
    pub(crate) fn new_part(&self, length: u64) -> Option<Want> {
        (!self.end).then(|| Want {
            offset: self.frontier(),
            length,
            new: true,
        })
    }

    /// What to ask for to read the part after the held ones.
    pub(crate) fn next(&self, length: u64) -> Option<Want> {
        self.again(self.held.end).or_else(|| self.new_part(length))
    }

    /// What to ask for to read the part before the held ones.
    pub(crate) fn previous(&self) -> Option<Want> {
        self.held
            .start
            .checked_sub(1)
            .and_then(|index| self.again(index))
    }

    /// Whether the file has more after the held parts.
    pub(crate) fn more_after(&self) -> bool {
        self.held.end < self.known.len() || !self.end
    }

    /// Whether the file has more before the held parts.
    pub(crate) fn more_before(&self) -> bool {
        self.held.start > 0
    }

    /// Whether the held parts are the whole file.
    pub(crate) fn whole(&self) -> bool {
        !self.more_before() && !self.more_after()
    }

    /// The first line of part `index` in the file.
    pub(crate) fn line_of(&self, index: usize) -> usize {
        self.known[..index.min(self.known.len())]
            .iter()
            .map(|part| part.lines)
            .sum()
    }

    /// The first line held, in the file.
    pub(crate) fn first_line(&self) -> usize {
        self.line_of(self.held.start)
    }

    /// The lines of the file, once every part is known.
    pub(crate) fn lines(&self) -> Option<usize> {
        self.end.then(|| self.line_of(self.known.len()))
    }

    /// The bytes held.
    pub(crate) fn held_bytes(&self) -> u64 {
        self.known[self.held.clone()]
            .iter()
            .map(|part| part.len)
            .sum()
    }

    /// The index of the known part with line `line` of the file.
    pub(crate) fn part_of_line(&self, line: usize) -> Option<usize> {
        let mut first = 0;
        self.known.iter().position(|part| {
            first += part.lines;
            line < first
        })
    }

    /// The index of the known part with byte `byte` of the file.
    pub(crate) fn part_of_byte(&self, byte: u64) -> Option<usize> {
        self.known
            .iter()
            .position(|part| byte < part.offset + part.len)
    }

    /// The line of the file about where byte `byte` of part `index` is, as if its lines were of
    /// one length.
    pub(crate) fn line_at(&self, index: usize, byte: u64) -> usize {
        let part = self.known[index];
        let into = byte.saturating_sub(part.offset).min(part.len);
        let lines = u64::try_from(part.lines).unwrap_or(u64::MAX);
        let line = (into * lines).checked_div(part.len).unwrap_or(0);
        self.line_of(index) + usize::try_from(line).unwrap_or(usize::MAX)
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
            .map_or(0, |last| last.offset + last.len)
    }

    /// The bytes of the file as far as known: its size, or more when it grew.
    pub(crate) fn extent(&self) -> u64 {
        self.size.unwrap_or(0).max(self.frontier())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> Parts {
        let mut parts = Parts::default();
        parts.push(
            Known {
                offset: 0,
                len: 100,
                lines: 10,
            },
            false,
        );
        parts.push(
            Known {
                offset: 100,
                len: 50,
                lines: 5,
            },
            false,
        );
        parts.set_held(1..2);
        parts
    }

    #[test]
    fn finds_parts_by_line_and_byte() {
        let parts = parts();
        assert_eq!(parts.first_line(), 10);
        assert_eq!(parts.part_of_line(9), Some(0));
        assert_eq!(parts.part_of_line(10), Some(1));
        assert_eq!(parts.part_of_line(15), None);
        assert_eq!(parts.part_of_byte(120), Some(1));
        assert_eq!(parts.line_at(1, 120), 12);
        assert_eq!(parts.byte_at(2), 120);
        assert_eq!(parts.byte_at(5), 150, "after the last line");
        assert_eq!(parts.lines(), None);
    }

    #[test]
    fn asks_for_the_parts_around_the_held_ones() {
        let mut parts = parts();
        assert_eq!(
            parts.next(64),
            Some(Want {
                offset: 150,
                length: 64,
                new: true
            })
        );
        assert_eq!(
            parts.previous(),
            Some(Want {
                offset: 0,
                length: 100,
                new: false
            })
        );
        parts.ends();
        assert_eq!(parts.next(64), None);
        assert!(!parts.more_after() && parts.more_before() && !parts.whole());
        assert_eq!(parts.lines(), Some(15));
    }
}
