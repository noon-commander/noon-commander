//! What a panel shows of its entries beside their names: the details of the columns of its rows,
//! and of its status line, each a list in which the name may stand anywhere (ADR 0096).

/// A detail of an entry, shown beside its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Detail {
    /// The size: rounded, with its unit, in a column; every byte of it on the status line.
    Size,
    /// The time of the last modification.
    Time,
    /// The type and the permissions, as `ls -l` writes them.
    Mode,
    /// The numbers of the owner and of its group.
    Owner,
}

/// The details on either side of the name, each in the order they show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Around {
    pub(crate) before: Vec<Detail>,
    pub(crate) after: Vec<Detail>,
}

/// A mode of a panel: the details of its columns, and of its status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) columns: Around,
    pub(crate) status: Around,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            columns: Around {
                before: Vec::new(),
                after: vec![Detail::Size, Detail::Time],
            },
            status: Around {
                before: Vec::new(),
                after: vec![Detail::Size, Detail::Mode, Detail::Owner],
            },
        }
    }
}

/// The parts on either side of a name kept, each with the cells it takes.
pub(crate) type Kept<T> = Vec<(T, usize)>;

/// What fits of `before` and `after`, the parts on either side of a name in `width` cells, each
/// in the cells that `cells` gives it, and the least it can be cut to, with a blank beside it,
/// while the name keeps at least `name` cells: the part at the end of the list, `after`'s last,
/// gives up first what it can, then goes. The parts kept, and the cells left for the name.
pub(crate) fn fit<T>(
    (before, after): (Vec<T>, Vec<T>),
    width: usize,
    name: usize,
    cells: impl Fn(&T) -> (usize, usize),
) -> (Kept<T>, Kept<T>, usize) {
    let sized = |parts: Vec<T>| -> Kept<T> {
        (parts.into_iter())
            .map(|part| {
                let (whole, _) = cells(&part);
                (part, whole)
            })
            .collect()
    };
    let (mut before, mut after) = (sized(before), sized(after));
    let taken = |before: &Kept<T>, after: &Kept<T>| -> usize {
        before.iter().chain(after).map(|(_, cells)| cells + 1).sum()
    };
    loop {
        let short = (taken(&before, &after) + name).saturating_sub(width);
        let Some((part, given)) = after.last_mut().or(before.last_mut()).filter(|_| short > 0)
        else {
            break;
        };
        let (_, least) = cells(part);
        if *given >= least + short {
            *given -= short;
            break;
        }
        if after.pop().is_none() {
            before.pop();
        }
    }
    let name = width.saturating_sub(taken(&before, &after));
    (before, after, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_at_the_end_of_the_list_go_first() {
        let fitted = |width| fit((vec![3], vec![4, 5]), width, 8, |&cells| (cells, cells));
        assert_eq!(fitted(23), (vec![(3, 3)], vec![(4, 4), (5, 5)], 8));
        assert_eq!(
            fitted(22),
            (vec![(3, 3)], vec![(4, 4)], 13),
            "the last after the name"
        );
        assert_eq!(fitted(16), (vec![(3, 3)], vec![], 12));
        assert_eq!(fitted(11), (vec![], vec![], 11), "then the ones before it");
        assert_eq!(
            fitted(5),
            (vec![], vec![], 5),
            "the name takes what there is"
        );
    }

    #[test]
    fn a_part_gives_up_what_it_can_before_it_goes() {
        // The last can be cut from 9 cells to 4.
        let cells = |&cells: &usize| (cells, if cells == 9 { 4 } else { cells });
        let fitted = |width| fit((vec![], vec![3, 9]), width, 8, cells);
        assert_eq!(fitted(22), (vec![], vec![(3, 3), (9, 9)], 8));
        assert_eq!(fitted(20), (vec![], vec![(3, 3), (9, 7)], 8), "cut");
        assert_eq!(
            fitted(17),
            (vec![], vec![(3, 3), (9, 4)], 8),
            "to its least"
        );
        assert_eq!(fitted(16), (vec![], vec![(3, 3)], 12), "then gone");
        let wider = fit((vec![], vec![3, 20]), 20, 8, |&cells| (cells, cells.min(4)));
        assert_eq!(
            wider,
            (vec![], vec![(3, 3), (20, 7)], 8),
            "by all that the name lacks, the parts wider than the whole"
        );
    }
}
