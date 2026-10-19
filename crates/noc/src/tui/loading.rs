//! The rows of a list that come from a task, as the places of the location menu and the
//! directories of the zoxide window do: which answer the list waits for, how it waits, and how
//! many lines the last one had, so that the skeleton of the next list is as tall (ADR 0037).

use super::cells;
use super::dialog::SKELETON_ROWS;
use super::spinner::Wait;

/// The wait of a list for its rows.
#[derive(Debug)]
pub(crate) struct Loading {
    /// Of the answer the list waits for; an older one is dropped.
    generation: u64,
    /// `true` once the first rows came.
    loaded: bool,
    /// For the first rows.
    wait: Wait,
    /// For rows asked for again, while those that came last show.
    again: Option<Wait>,
    /// Lines of the skeleton until the first rows come, as many as the last list had, if known.
    skeleton: Option<u16>,
    /// Lines of the last rows that came while the filter was empty.
    listed: Option<u16>,
    /// Why the last answer brought no rows.
    error: Option<String>,
}

impl Loading {
    /// Waiting for the answer `generation`.
    pub(crate) fn new(generation: u64) -> Self {
        Self {
            generation,
            loaded: false,
            wait: Wait::default(),
            again: None,
            skeleton: None,
            listed: None,
            error: None,
        }
    }

    /// The same wait, whose skeleton has as many `lines` as the last list had, if any, so that
    /// the box does not grow when the rows come.
    pub(crate) fn lines_like(mut self, lines: Option<u16>) -> Self {
        self.skeleton = lines.or(self.skeleton);
        self
    }

    /// Waits for the answer `generation` instead. What the list shows stays until it comes.
    pub(crate) fn ask(&mut self, generation: u64) {
        self.generation = generation;
        if self.loaded {
            self.again = Some(Wait::default());
        } else {
            self.wait = Wait::default();
        }
    }

    /// Asks again, and forgets why the last answer brought no rows, so that the list waits
    /// rather than say it.
    pub(crate) fn reload(&mut self, generation: u64) {
        self.error = None;
        self.ask(generation);
    }

    /// Whether the answer `generation` is the one the list waits for, which ends the wait.
    pub(crate) fn answers(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.again = None;
        true
    }

    /// The rows came, as many `lines` as the list shows while the filter is empty, if it is.
    pub(crate) fn came(&mut self, lines: Option<usize>) {
        self.loaded = true;
        self.error = None;
        if let Some(lines) = lines {
            self.listed = Some(u16::try_from(lines).unwrap_or(u16::MAX));
        }
    }

    /// No rows came, for `reason`.
    pub(crate) fn failed(&mut self, reason: &str) {
        self.error = Some(cells::sanitize(reason.as_bytes()));
    }

    /// `true` once the first rows came.
    pub(crate) fn loaded(&self) -> bool {
        self.loaded
    }

    /// Why the last answer brought no rows.
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Lines of the last rows that came while the filter was empty, for the skeleton of the
    /// next list.
    pub(crate) fn listed(&self) -> Option<u16> {
        self.listed
    }

    /// The wait for rows, the first or those asked for again, whose dots turn in front of the
    /// title.
    pub(crate) fn wait(&self) -> Option<&Wait> {
        if self.loaded {
            self.again.as_ref()
        } else {
            self.error.is_none().then_some(&self.wait)
        }
    }

    /// Lines of the skeleton that stands in place of the rows until the first come, unless the
    /// list says why they did not.
    pub(crate) fn skeleton(&self) -> Option<u16> {
        (!self.loaded && self.error.is_none()).then_some(self.skeleton.unwrap_or(SKELETON_ROWS))
    }

    /// Whether the skeleton stands for rows that the last list had, rather than for one row
    /// of a list that opens the first time.
    pub(crate) fn rows_known(&self) -> bool {
        self.skeleton.is_some_and(|lines| lines > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_its_answer_and_drops_older_ones() {
        let mut loading = Loading::new(1).lines_like(Some(5));
        assert_eq!(loading.skeleton(), Some(5));
        assert!(loading.rows_known());
        assert!(loading.wait().is_some());
        assert!(!loading.answers(0), "an older answer");
        assert!(loading.answers(1));
        loading.came(Some(7));
        assert_eq!((loading.skeleton(), loading.listed()), (None, Some(7)));
        assert!(loading.wait().is_none(), "the rows show");
        loading.ask(2);
        assert!(loading.wait().is_some(), "asked again, with the rows shown");
        assert!(loading.answers(2));
        loading.came(None);
        assert_eq!(loading.listed(), Some(7), "the filter had text");
        assert!(loading.wait().is_none());
    }

    #[test]
    fn says_why_no_rows_came_until_asked_to_wait_again() {
        let mut loading = Loading::new(1).lines_like(None);
        assert_eq!(loading.skeleton(), Some(SKELETON_ROWS));
        assert!(!loading.rows_known(), "the first list");
        assert!(
            !Loading::new(1).lines_like(Some(0)).rows_known(),
            "one with no rows"
        );
        assert!(loading.answers(1));
        loading.failed("no \u{1b}zoxide");
        assert_eq!(loading.error(), Some("no ?zoxide"));
        assert_eq!(loading.skeleton(), None, "the reason shows instead");
        assert!(loading.wait().is_none());
        loading.ask(2);
        assert!(loading.error().is_some(), "until the answer comes");
        loading.reload(3);
        assert_eq!(loading.error(), None);
        assert!(loading.wait().is_some() && loading.skeleton().is_some());
        assert!(loading.answers(3));
        loading.came(Some(2));
        loading.failed("gone");
        assert!(loading.loaded(), "the rows that came stay");
    }
}
