//! Tab completion of paths in the fields of Quick cd, F5, F6, and F7, as bash and mc do it:
//! the last part of the path is completed from what its directory holds. One match is put in
//! with `/` after a directory and `:` after a host; several are put in as far as they agree,
//! and a Tab that gets no further lists them, to choose one.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear};

use super::cells::{self, Align};
use super::dialog::Colors;
use super::keymap::Action;
use super::theme::Theme;

/// Rows the list of choices shows at most.
const MAX_ROWS: usize = 10;

/// What a field takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Directories, as `cd` and F7 take them.
    Dirs,
    /// Directories and files, as the target of F5 and F6.
    Any,
}

/// A name that completes the text, and what goes after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) name: String,
    pub(crate) kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Dir,
    Host,
}

impl Candidate {
    /// The name as it goes in the field: `/` after a directory, `:` after a host.
    fn text(&self) -> String {
        match self.kind {
            Kind::File => self.name.clone(),
            Kind::Dir => format!("{}/", self.name),
            Kind::Host => format!("{}:", self.name),
        }
    }
}

/// The text before the cursor, cut where completion starts: the directory to read as typed,
/// `dir`, which stays, and `prefix`, the part of a name to complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Split {
    pub(crate) dir: String,
    pub(crate) prefix: String,
    /// The prefix may be a host's alias too: nothing marks it as a path yet.
    pub(crate) hosts: bool,
}

/// Cuts `before`, the text before the cursor: after its last `/`; else after `host:` where
/// `is_host` takes the text before the first `:`; else at its start, where hosts may be meant.
pub(crate) fn split(before: &str, is_host: impl Fn(&str) -> bool) -> Split {
    if let Some(slash) = before.rfind('/') {
        return Split {
            dir: before[..=slash].to_owned(),
            prefix: before[slash + 1..].to_owned(),
            hosts: false,
        };
    }
    if let Some((host, rest)) = before.split_once(':')
        && !host.is_empty()
        && is_host(host)
    {
        return Split {
            dir: format!("{host}:"),
            prefix: rest.to_owned(),
            hosts: false,
        };
    }
    Split {
        dir: String::new(),
        prefix: before.to_owned(),
        hosts: true,
    }
}

/// What a Tab does with the candidates for `prefix`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Nothing matches.
    Nothing,
    /// Put this in place of the prefix.
    Insert(String),
    /// Several match and agree no further than the prefix: list them.
    List(Vec<Candidate>),
}

/// Completes `prefix` from `candidates`. Names that start with a dot count only if the prefix
/// does, as in bash. Case counts, unless nothing matches with it.
pub(crate) fn complete(prefix: &str, candidates: Vec<Candidate>) -> Outcome {
    let shown: Vec<Candidate> = candidates
        .into_iter()
        .filter(|candidate| !candidate.name.starts_with('.') || prefix.starts_with('.'))
        .collect();
    let exact: Vec<Candidate> = shown
        .iter()
        .filter(|candidate| candidate.name.starts_with(prefix))
        .cloned()
        .collect();
    let (mut matches, fold) = if exact.is_empty() {
        let lower = prefix.to_lowercase();
        let folded = shown
            .into_iter()
            .filter(|candidate| candidate.name.to_lowercase().starts_with(&lower))
            .collect();
        (folded, true)
    } else {
        (exact, false)
    };
    matches.sort_by(|a, b| {
        let key = |candidate: &Candidate| candidate.name.to_lowercase();
        key(a)
            .cmp(&key(b))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| (a.kind as u8).cmp(&(b.kind as u8)))
    });
    matches.dedup();
    match matches.as_slice() {
        [] => Outcome::Nothing,
        [one] => Outcome::Insert(one.text()),
        [first, rest @ ..] => {
            let common = rest.iter().fold(first.name.clone(), |common, candidate| {
                common_prefix(&common, &candidate.name, fold)
            });
            // Case-folded matches may change the case of what was typed, but only to get further.
            if common.chars().count() > prefix.chars().count() {
                Outcome::Insert(common)
            } else {
                Outcome::List(matches)
            }
        }
    }
}

/// The start that `a` and `b` share, as `a` spells it; with `fold`, regardless of case.
fn common_prefix(a: &str, b: &str, fold: bool) -> String {
    let same = |x: char, y: char| {
        if fold {
            x.to_lowercase().eq(y.to_lowercase())
        } else {
            x == y
        }
    };
    a.chars()
        .zip(b.chars())
        .take_while(|(x, y)| same(*x, *y))
        .map(|(x, _)| x)
        .collect()
}

/// Names from a directory listing, as candidates: those that a field can hold, of what it
/// takes. Names that are not UTF-8 or hold control characters cannot be typed, so they are left
/// out.
pub(crate) fn from_names(
    entries: impl IntoIterator<Item = (Vec<u8>, bool)>,
    offer: Offer,
) -> Vec<Candidate> {
    entries
        .into_iter()
        .filter(|(_, dir)| *dir || offer == Offer::Any)
        .filter_map(|(name, dir)| {
            let name = String::from_utf8(name).ok()?;
            if name.chars().any(char::is_control) || name == "." || name == ".." {
                return None;
            }
            let kind = if dir { Kind::Dir } else { Kind::File };
            Some(Candidate { name, kind })
        })
        .collect()
}

/// The list of candidates under a field, after a Tab that got no further.
#[derive(Debug)]
pub(crate) struct Choices {
    items: Vec<Candidate>,
    cursor: usize,
    offset: usize,
}

/// What a key did to the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChoicesEvent {
    Pending,
    Closed,
    /// Put this in place of the prefix.
    Chosen(String),
    /// The key is not for the list: it closes, and the key goes to the field.
    Passed,
}

impl Choices {
    pub(crate) fn new(items: Vec<Candidate>) -> Self {
        Self {
            items,
            cursor: 0,
            offset: 0,
        }
    }

    /// Arrows and Tab move, Enter takes the row, Esc closes the list.
    pub(crate) fn handle(&mut self, action: Action) -> ChoicesEvent {
        let last = self.items.len().saturating_sub(1);
        match action {
            Action::Up => self.cursor = self.cursor.checked_sub(1).unwrap_or(last),
            Action::Down | Action::Complete => {
                self.cursor = if self.cursor >= last {
                    0
                } else {
                    self.cursor + 1
                };
            }
            Action::PageUp => self.cursor = self.cursor.saturating_sub(MAX_ROWS),
            Action::PageDown => self.cursor = (self.cursor + MAX_ROWS).min(last),
            Action::Home => self.cursor = 0,
            Action::End => self.cursor = last,
            Action::Confirm => {
                return self
                    .items
                    .get(self.cursor)
                    .map_or(ChoicesEvent::Closed, |item| {
                        ChoicesEvent::Chosen(item.text())
                    });
            }
            Action::Cancel => return ChoicesEvent::Closed,
            _ => return ChoicesEvent::Passed,
        }
        ChoicesEvent::Pending
    }

    /// Draws the list in a frame under `field`, or above it where there is no room below,
    /// within `area`.
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, field: Rect, area: Rect, theme: &Theme) {
        let rows = self.items.len().min(MAX_ROWS);
        let height = u16::try_from(rows).unwrap_or(0) + 2;
        let below = area.bottom().saturating_sub(field.bottom());
        let above = field.y.saturating_sub(area.y);
        let (y, height) = if below >= height || below >= above {
            (field.bottom(), height.min(below))
        } else {
            let height = height.min(above);
            (field.y - height, height)
        };
        let x = field.x.saturating_sub(1);
        let width = field
            .width
            .saturating_add(2)
            .min(area.right().saturating_sub(x));
        let outer = Rect::new(x, y, width, height);
        if outer.height < 3 || outer.width < 4 {
            return;
        }
        frame.render_widget(Clear, outer);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .style(theme.dialog);
        let inner = block.inner(outer);
        frame.render_widget(block, outer);
        let page = usize::from(inner.height);
        self.offset = self
            .offset
            .min(self.cursor)
            .max((self.cursor + 1).saturating_sub(page));
        let focused = Colors::of(theme, false).focused_style();
        let width = usize::from(inner.width);
        for (index, (row, item)) in self
            .items
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(page)
            .enumerate()
        {
            let text = cells::fit(&cells::sanitize(item.text().as_bytes()), width, Align::Left);
            let style = if row == self.cursor {
                focused
            } else {
                theme.dialog
            };
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            frame.render_widget(
                Line::styled(text, style),
                Rect::new(inner.x, y, inner.width, 1),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn dir(name: &str) -> Candidate {
        Candidate {
            name: name.to_owned(),
            kind: Kind::Dir,
        }
    }

    fn file(name: &str) -> Candidate {
        Candidate {
            name: name.to_owned(),
            kind: Kind::File,
        }
    }

    fn host(name: &str) -> Candidate {
        Candidate {
            name: name.to_owned(),
            kind: Kind::Host,
        }
    }

    fn insert(text: &str) -> Outcome {
        Outcome::Insert(text.to_owned())
    }

    #[test]
    fn splits_after_the_last_slash_or_a_host() {
        let hosts = |host: &str| host == "web";
        let cut = |before: &str| split(before, hosts);
        assert_eq!(
            cut("/srv/www/lo"),
            Split {
                dir: "/srv/www/".to_owned(),
                prefix: "lo".to_owned(),
                hosts: false,
            }
        );
        assert_eq!(cut("web:/var/").dir, "web:/var/");
        assert_eq!(
            cut("web:ap"),
            Split {
                dir: "web:".to_owned(),
                prefix: "ap".to_owned(),
                hosts: false,
            }
        );
        assert_eq!(
            cut("we"),
            Split {
                dir: String::new(),
                prefix: "we".to_owned(),
                hosts: true,
            }
        );
        // Not a host this field takes: a name with a colon.
        assert_eq!(cut("db:x").dir, "");
        assert_eq!(cut("db:x").prefix, "db:x");
        assert_eq!(cut("").prefix, "");
    }

    #[test]
    fn one_match_goes_in_with_its_mark() {
        assert_eq!(
            complete("lo", vec![dir("logs"), file("x")]),
            insert("logs/")
        );
        assert_eq!(complete("no", vec![file("notes.txt")]), insert("notes.txt"));
        assert_eq!(complete("we", vec![host("web"), dir("x")]), insert("web:"));
        assert_eq!(complete("zz", vec![dir("logs")]), Outcome::Nothing);
    }

    #[test]
    fn several_go_in_as_far_as_they_agree_then_list() {
        let candidates = vec![dir("project-a"), dir("project-b"), file("other")];
        assert_eq!(complete("pr", candidates.clone()), insert("project-"));
        assert_eq!(
            complete("project-", candidates),
            Outcome::List(vec![dir("project-a"), dir("project-b")])
        );
        assert_eq!(
            complete("", vec![dir("b"), dir("a")]),
            Outcome::List(vec![dir("a"), dir("b")])
        );
    }

    #[test]
    fn dot_names_show_for_a_dot_and_case_counts_until_nothing_matches() {
        let candidates = vec![dir(".git"), dir("src"), dir("Docs")];
        assert_eq!(
            complete("", candidates.clone()),
            Outcome::List(vec![dir("Docs"), dir("src")])
        );
        assert_eq!(complete(".", candidates.clone()), insert(".git/"));
        assert_eq!(complete("d", candidates.clone()), insert("Docs/"));
        assert_eq!(
            complete("D", vec![dir("Desktop"), dir("dev")]),
            insert("Desktop/"),
            "an exact match comes first"
        );
        assert_eq!(
            complete("d", vec![dir("Desktop"), dir("Documents")]),
            Outcome::List(vec![dir("Desktop"), dir("Documents")]),
            "a change of case alone gets no further"
        );
        assert_eq!(
            complete("d", vec![dir("Desktop"), dir("Dev")]),
            insert("De"),
            "in the case of the names"
        );
    }

    #[test]
    fn listings_become_candidates_that_can_be_typed() {
        let entries = vec![
            (b"src".to_vec(), true),
            (b"notes".to_vec(), false),
            (b"\xff".to_vec(), true),
            (b"a\nb".to_vec(), true),
        ];
        assert_eq!(from_names(entries.clone(), Offer::Dirs), vec![dir("src")]);
        assert_eq!(
            from_names(entries, Offer::Any),
            vec![dir("src"), file("notes")]
        );
    }

    #[test]
    fn the_list_moves_round_and_takes_a_row() {
        let mut choices = Choices::new(vec![dir("a"), dir("b"), host("c")]);
        assert_eq!(choices.handle(Action::Up), ChoicesEvent::Pending);
        assert_eq!(
            choices.handle(Action::Confirm),
            ChoicesEvent::Chosen("c:".to_owned())
        );
        choices.handle(Action::Complete);
        assert_eq!(
            choices.handle(Action::Confirm),
            ChoicesEvent::Chosen("a/".to_owned())
        );
        assert_eq!(choices.handle(Action::Backspace), ChoicesEvent::Passed);
        assert_eq!(choices.handle(Action::Cancel), ChoicesEvent::Closed);
    }

    #[test]
    fn the_list_goes_under_the_field_or_above_it() {
        let mut choices = Choices::new(vec![dir("alpha"), dir("beta")]);
        let draw = |choices: &mut Choices, field: Rect| {
            let mut terminal = Terminal::new(TestBackend::new(20, 8)).unwrap();
            terminal
                .draw(|frame| choices.render(frame, field, frame.area(), &Theme::terminal()))
                .unwrap();
            terminal.backend().to_string()
        };
        let below = draw(&mut choices, Rect::new(2, 1, 10, 1));
        let lines: Vec<&str> = below.lines().collect();
        assert!(
            lines[3].contains("alpha/") && lines[4].contains("beta/"),
            "{below}"
        );
        let above = draw(&mut choices, Rect::new(2, 6, 10, 1));
        let lines: Vec<&str> = above.lines().collect();
        assert!(lines[3].contains("alpha/"), "{above}");
    }
}
