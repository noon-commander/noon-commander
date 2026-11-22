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

/// What fits of `before` and `after`, the parts on either side of a name in `width` cells, each
/// in its `cells` and a blank beside it, while the name keeps at least `name` cells: the parts at
/// the end of the list, `after`'s last, go first. The parts kept, and the cells left for the name.
pub(crate) fn fit<T>(
    (mut before, mut after): (Vec<T>, Vec<T>),
    width: usize,
    name: usize,
    cells: impl Fn(&T) -> usize,
) -> (Vec<T>, Vec<T>, usize) {
    let left = |before: &[T], after: &[T]| {
        let taken: usize = before.iter().chain(after).map(|part| cells(part) + 1).sum();
        width.saturating_sub(taken)
    };
    while left(&before, &after) < name && (after.pop().is_some() || before.pop().is_some()) {}
    let name = left(&before, &after);
    (before, after, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_at_the_end_of_the_list_go_first() {
        let fitted = |width| fit((vec![3], vec![4, 5]), width, 8, |cells| *cells);
        assert_eq!(fitted(23), (vec![3], vec![4, 5], 8));
        assert_eq!(
            fitted(22),
            (vec![3], vec![4], 13),
            "the last after the name"
        );
        assert_eq!(fitted(16), (vec![3], vec![], 12));
        assert_eq!(fitted(11), (vec![], vec![], 11), "then the ones before it");
        assert_eq!(
            fitted(5),
            (vec![], vec![], 5),
            "the name takes what there is"
        );
    }
}
