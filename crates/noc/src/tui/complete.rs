//! Tab completion, as vim's command line completes with `wildmenu` and `wildmode=full`: the
//! last part of a path is completed from what its directory holds. One match goes in with `/`
//! after a directory and `:` after a host; of several, the first goes in whole and a list of
//! them shows by the field, where Tab and the arrows put the others in, round through what was
//! typed. The fields of Quick cd, F5, F6, and F7 complete paths; the command line completes
//! the word before the cursor, escaped for sh, as vim's `:!` does.

use noc_scroll::scrollbar::{self, Bar};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear};

use super::cells::{self, Align};
use super::dialog::Colors;
use super::keymap::Action;
use super::mouse::{Drawn, Pointer, Press};
use super::theme::Theme;

/// Rows the list of choices shows at most, and how far the page keys go in it.
const MAX_ROWS: usize = 10;
/// What sh reads as more than a character of a name, which a backslash goes before.
const SHELL_SPECIAL: &str = " \t\\'\"$`&;|<>()*?[]#!{}";

/// What a field takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Directories, as `cd` and F7 take them.
    Dirs,
    /// Directories and files, as the target of F5 and F6 and the command line.
    Any,
}

/// How a name goes in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quote {
    /// As it is, in a path field.
    Plain,
    /// With a backslash before what sh would read otherwise, on the command line.
    Shell,
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
    /// The name as the list shows it: `/` after a directory, `:` after a host.
    fn text(&self) -> String {
        self.typed(Quote::Plain)
    }

    /// The name as it goes in the text, quoted as `quote` says, with its mark after it.
    pub(crate) fn typed(&self, quote: Quote) -> String {
        let name = match quote {
            Quote::Plain => self.name.clone(),
            Quote::Shell => escape(&self.name),
        };
        match self.kind {
            Kind::File => name,
            Kind::Dir => format!("{name}/"),
            Kind::Host => format!("{name}:"),
        }
    }
}

/// `name` escaped for sh as vim's `:!` completion does: a backslash before blanks and the
/// characters sh reads otherwise, and before a `~` at its start.
pub(crate) fn escape(name: &str) -> String {
    let mut escaped = String::with_capacity(name.len());
    for (index, c) in name.chars().enumerate() {
        if SHELL_SPECIAL.contains(c) || (index == 0 && c == '~') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// `text` with its backslash escapes taken out, as sh reads a word: `\x` is `x`; a backslash
/// at the end escapes nothing yet, and goes.
fn unescape(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            plain.extend(chars.next());
        } else {
            plain.push(c);
        }
    }
    plain
}

/// The text before the cursor in a path field, cut where completion starts: the directory to
/// read as typed, `dir`, which stays, and `prefix`, the part of a name to complete.
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

/// The word before the cursor on the command line, cut where completion starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Word {
    /// What stays, as typed: the words before, and the directory of this one.
    pub(crate) keep: String,
    /// The directory the word names, without escapes, up to its last `/`; empty for none.
    pub(crate) dir: String,
    /// The part of a name to complete, without escapes.
    pub(crate) prefix: String,
    /// It is the first word of the command, which names the command.
    pub(crate) first: bool,
    /// Where the word starts in the text.
    pub(crate) start: usize,
}

/// Cuts `before`, a shell command before the cursor, as sh reads its words: the word starts
/// after the last blank or line break that no backslash escapes, and its name after its last
/// `/`.
pub(crate) fn word(before: &str) -> Word {
    let parts = |c: char| matches!(c, ' ' | '\t' | '\n');
    let mut start = 0;
    let mut escaped = false;
    for (offset, c) in before.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if parts(c) {
            start = offset + c.len_utf8();
        }
    }
    let typed = &before[start..];
    let cut = typed.rfind('/').map_or(0, |slash| slash + 1);
    Word {
        keep: before[..start + cut].to_owned(),
        dir: unescape(&typed[..cut]),
        prefix: unescape(&typed[cut..]),
        first: before[..start].chars().all(parts),
        start,
    }
}

/// What vim's Left does in the list of a path completed, `word`, in its 'wildoptions' pum:
/// the parent of the directory it lists, to complete in, as `wildmenu_process_key_filenames`
/// works it out: `../` for the current directory, one more `../` after one.
pub(crate) fn up_dir(word: &str) -> String {
    let b = word.as_bytes();
    let at = |k: usize, s: &[u8]| b.get(k..).is_some_and(|rest| rest.starts_with(s));
    // The last `/` before the last character, and the one before it, if any.
    let mut seps = b[..b.len().saturating_sub(1)]
        .iter()
        .enumerate()
        .rev()
        .filter(|&(k, &c)| k > 0 && c == b'/')
        .map(|(k, _)| k);
    let (first, second) = (seps.next(), seps.next());
    let (start, j) = match (first, second) {
        (None, _) => return "../".to_owned(),
        (Some(_), Some(second)) => (second + 1, second),
        (Some(_), None) => (0, 0),
    };
    if at(j, b"/../") {
        format!("{}../", &word[..j + 4])
    } else if j == start && at(j, b"../") {
        format!("{}../", &word[..j + 3])
    } else {
        word[..start].to_owned()
    }
}

/// What vim's Right does in the list of a path completed, `word`, in its 'wildoptions' pum:
/// the directory to complete in, if `word` names one, or one `../` less after `../`; `None`
/// leaves the key to the field.
pub(crate) fn down_dir(word: &str) -> Option<String> {
    if word.ends_with('/') && !word.ends_with("../") {
        return Some(word.to_owned());
    }
    if !word.starts_with("../") {
        return None;
    }
    let b = word.as_bytes();
    let j = (1..b.len()).rev().find(|&k| b[k] == b'/')?;
    let parent = j >= 2 && b[j - 1] == b'.' && b[j - 2] == b'.';
    (parent && (j == 2 || b[j - 3] == b'/')).then(|| word[..j - 2].to_owned())
}

/// What a Tab finds for a prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Nothing matches.
    Nothing,
    /// One match, which goes in, with no list.
    One(Candidate),
    /// Several, in the order of the list: the first goes in, and the list shows them all.
    List(Vec<Candidate>),
}

/// The candidates that complete `prefix`, in the order of the list. Names that start with a
/// dot count only if the prefix does, as in bash. Case counts, unless nothing matches with it.
pub(crate) fn complete(prefix: &str, candidates: Vec<Candidate>) -> Outcome {
    let shown: Vec<Candidate> = candidates
        .into_iter()
        .filter(|candidate| !candidate.name.starts_with('.') || prefix.starts_with('.'))
        .collect();
    // Case never counts, as vim's 'fileignorecase' has it on macOS.
    let lower = prefix.to_lowercase();
    let mut matches: Vec<Candidate> = shown
        .into_iter()
        .filter(|candidate| candidate.name.to_lowercase().starts_with(&lower))
        .collect();
    matches.sort_by(|a, b| {
        let key = |candidate: &Candidate| candidate.name.to_lowercase();
        key(a)
            .cmp(&key(b))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| (a.kind as u8).cmp(&(b.kind as u8)))
    });
    matches.dedup();
    match matches.len() {
        0 => Outcome::Nothing,
        1 => matches.pop().map_or(Outcome::Nothing, Outcome::One),
        _ => Outcome::List(matches),
    }
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

/// The list of matches by a field, after a Tab that found several, as vim's `wildmenu`: the
/// match on the row chosen is in the field; with no row chosen, what was typed is.
#[derive(Debug)]
pub(crate) struct Choices {
    items: Vec<Candidate>,
    quote: Quote,
    /// The row whose match is in the field; `None` while what was typed is.
    chosen: Option<usize>,
    /// The first row shown, and how many show, at the last render.
    offset: usize,
    page: usize,
    /// The rows scroll to the one chosen, until the thumb of the scroll bar scrolls them.
    follow: bool,
    /// Where the last render drew it, for the mouse.
    drawn: Drawn,
    /// The scroll bar of the last render, on the right of the frame.
    bar: Option<Bar>,
}

/// What a key or a press did to the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChoicesEvent {
    /// Put this in place of the part completed: a match, or for `None` what was typed; and
    /// with `close`, close the list.
    Show { text: Option<String>, close: bool },
    /// The key is not for the list: it closes, the text stays as it is, and the key goes to
    /// the field.
    Passed,
}

impl Choices {
    /// The list of `items`, whose first is in the field, quoted as `quote` says.
    pub(crate) fn new(items: Vec<Candidate>, quote: Quote) -> Self {
        Self {
            items,
            quote,
            chosen: Some(0),
            offset: 0,
            page: 1,
            follow: true,
            drawn: Drawn::default(),
            bar: None,
        }
    }

    /// The text of the match in the field; `None` while what was typed is.
    pub(crate) fn current(&self) -> Option<String> {
        let item = self.items.get(self.chosen?)?;
        Some(item.typed(self.quote))
    }

    /// Down and Up put the next match in and the one before, and the page keys a page of them
    /// on, round through what was typed; Ctrl+y keeps the match, and Ctrl+e puts back what was
    /// typed.
    pub(crate) fn handle(&mut self, action: Action) -> ChoicesEvent {
        self.follow = true;
        self.chosen = match action {
            Action::Down => self.moved(true, 1),
            Action::Up => self.moved(false, 1),
            Action::PageDown => self.moved(true, MAX_ROWS),
            Action::PageUp => self.moved(false, MAX_ROWS),
            Action::AcceptCompletion => return self.show(true),
            Action::CancelCompletion => {
                return ChoicesEvent::Show {
                    text: None,
                    close: true,
                };
            }
            _ => return ChoicesEvent::Passed,
        };
        self.show(false)
    }

    fn show(&self, close: bool) -> ChoicesEvent {
        ChoicesEvent::Show {
            text: self.current(),
            close,
        }
    }

    /// The row `by` rows on, or back, as vim goes: from the last row, or the first going back,
    /// to what was typed; from there, to the first row, or the last going back.
    fn moved(&self, forward: bool, by: usize) -> Option<usize> {
        let last = self.items.len().checked_sub(1)?;
        match (self.chosen, forward) {
            (None, true) => Some(0),
            (None, false) => Some(last),
            (Some(row), true) if row >= last => None,
            (Some(row), true) => Some((row + by).min(last)),
            (Some(0), false) => None,
            (Some(row), false) => Some(row.saturating_sub(by)),
        }
    }

    /// The scroll bar of the last render, for the mouse.
    pub(crate) fn bar(&self) -> Option<Bar> {
        self.bar
    }

    /// Shows the rows from `top`, where the thumb of the scroll bar was dragged, and leaves the
    /// match in the field as it is.
    pub(crate) fn scroll_to(&mut self, top: usize) {
        self.offset = top.min(self.items.len().saturating_sub(self.page.max(1)));
        self.follow = false;
    }

    /// Whether the list was drawn over `at`.
    pub(crate) fn contains(&self, at: Position) -> bool {
        self.drawn.frame.contains(at)
    }

    /// Takes a press of the mouse on the list: a click puts the match of its row in, as a move
    /// does, and a double click also closes the list, as Ctrl+y.
    pub(crate) fn pointer(&mut self, pointer: Pointer) -> Option<ChoicesEvent> {
        let close = match pointer.press {
            Press::Click => false,
            Press::DoubleClick => true,
            Press::RightClick | Press::WheelUp | Press::WheelDown | Press::Drag => return None,
        };
        self.chosen = Some(self.drawn.row_at(pointer.at)?);
        self.follow = true;
        Some(self.show(close))
    }

    /// The widest row, in cells.
    pub(crate) fn width(&self) -> u16 {
        let widest = self.items.iter().map(|item| row_text(item).1).max();
        u16::try_from(widest.unwrap_or(0)).unwrap_or(u16::MAX)
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
        self.drawn = Drawn::default();
        self.bar = None;
        if outer.height < 3 || outer.width < 4 {
            return;
        }
        self.drawn.frame = outer;
        frame.render_widget(Clear, outer);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .style(theme.dialog);
        let inner = block.inner(outer);
        frame.render_widget(block, outer);
        let page = usize::from(inner.height);
        self.page = page;
        if let Some(chosen) = self.chosen.filter(|_| self.follow) {
            self.offset = self
                .offset
                .min(chosen)
                .max((chosen + 1).saturating_sub(page));
        }
        self.offset = self.offset.min(self.items.len().saturating_sub(page));
        let bar = Rect::new(outer.right() - 1, inner.y, 1, inner.height);
        let position = (self.items.len(), page, self.offset);
        self.bar = scrollbar::render(frame, bar, position, theme.dialog, theme.scroll_arrows);
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
            let text = cells::fit(&row_text(item).0, width, Align::Left);
            let style = if Some(row) == self.chosen {
                focused
            } else {
                theme.dialog
            };
            let y = inner.y + u16::try_from(index).unwrap_or(0);
            let line = Rect::new(inner.x, y, inner.width, 1);
            frame.render_widget(Line::styled(text, style), line);
            self.drawn.rows.push((row, line));
        }
    }
}

/// The row of `item` as the list shows it, safe for the terminal, and its width in cells.
fn row_text(item: &Candidate) -> (String, usize) {
    let text = cells::sanitize(item.text().as_bytes());
    let width = cells::width(&text);
    (text, width)
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

    fn show(text: Option<&str>, close: bool) -> ChoicesEvent {
        ChoicesEvent::Show {
            text: text.map(str::to_owned),
            close,
        }
    }

    #[test]
    fn left_and_right_go_up_and_down_the_directories_as_vims_pop_up_menu() {
        // Checked against vim 9.1 with 'wildoptions' pum.
        assert_eq!(up_dir("alpha/one"), "");
        assert_eq!(up_dir("alpha/sub/f1"), "alpha/");
        assert_eq!(up_dir("alpha/sub/"), "");
        assert_eq!(up_dir("a b"), "../");
        assert_eq!(up_dir(""), "../");
        assert_eq!(up_dir("../bin/"), "../../");
        assert_eq!(up_dir("../bin/mycmd"), "../");
        assert_eq!(up_dir("x/../y"), "x/../../");
        assert_eq!(down_dir("alpha/"), Some("alpha/".to_owned()));
        assert_eq!(down_dir("alpine.txt"), None);
        assert_eq!(down_dir("../"), Some(String::new()));
        assert_eq!(down_dir("../../A.patch"), Some("../".to_owned()));
        assert_eq!(down_dir("../x"), Some(String::new()));
        assert_eq!(down_dir("../bin/mycmd"), None);
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
    fn the_word_before_the_cursor_starts_after_a_blank_no_backslash_escapes() {
        assert_eq!(
            word("ls -l src/ma"),
            Word {
                keep: "ls -l src/".to_owned(),
                dir: "src/".to_owned(),
                prefix: "ma".to_owned(),
                first: false,
                start: 6,
            }
        );
        assert_eq!(
            word("  gi"),
            Word {
                keep: "  ".to_owned(),
                dir: String::new(),
                prefix: "gi".to_owned(),
                first: true,
                start: 2,
            }
        );
        let escaped = word("cat my\\ dir/a\\ \\$b");
        assert_eq!(escaped.keep, "cat my\\ dir/");
        assert_eq!(escaped.dir, "my dir/");
        assert_eq!(escaped.prefix, "a $b");
        assert!(!escaped.first);
        // An escaped backslash escapes no blank after it.
        assert_eq!(word("echo a\\\\ b").prefix, "b");
        // A line break parts words too, and a backslash at the cursor waits for what it
        // escapes.
        assert_eq!(word("make \\\n  ins\\").prefix, "ins");
        assert_eq!(word("make \\\n  ins\\").keep, "make \\\n  ");
        assert_eq!(word("./run").keep, "./");
        assert!(word("./run").first);
        assert_eq!(word("").prefix, "");
        assert!(word("").first);
        assert!(word("ls ").prefix.is_empty() && !word("ls ").first);
    }

    #[test]
    fn names_are_escaped_for_sh_as_vims_bang_completion_does() {
        assert_eq!(escape("plain-name_1.txt"), "plain-name_1.txt");
        assert_eq!(escape("my file"), "my\\ file");
        assert_eq!(
            escape("a\tb\\c'd\"e$f`g&h;i|j<k>l(m)n*o?p[q]r#s!t{u}"),
            "a\\\tb\\\\c\\'d\\\"e\\$f\\`g\\&h\\;i\\|j\\<k\\>l\\(m\\)n\\*o\\?p\\[q\\]r\\#s\\!t\\{u\\}"
        );
        assert_eq!(escape("~notes~"), "\\~notes~", "a tilde only at the start");
        assert_eq!(unescape(&escape("a b$~")), "a b$~");
        assert_eq!(
            dir("my dir").typed(Quote::Shell),
            "my\\ dir/",
            "the slash as it is"
        );
        assert_eq!(dir("my dir").typed(Quote::Plain), "my dir/");
    }

    #[test]
    fn one_match_goes_in_with_its_mark() {
        assert_eq!(
            complete("lo", vec![dir("logs"), file("x")]),
            Outcome::One(dir("logs"))
        );
        assert_eq!(dir("logs").typed(Quote::Plain), "logs/");
        assert_eq!(host("web").typed(Quote::Plain), "web:");
        assert_eq!(file("notes.txt").typed(Quote::Plain), "notes.txt");
        assert_eq!(
            complete("we", vec![host("web"), dir("x")]),
            Outcome::One(host("web"))
        );
        assert_eq!(complete("zz", vec![dir("logs")]), Outcome::Nothing);
    }

    #[test]
    fn several_go_in_a_list_in_its_order_with_no_common_part_first() {
        let candidates = vec![dir("project-b"), dir("project-a"), file("other")];
        assert_eq!(
            complete("pr", candidates),
            Outcome::List(vec![dir("project-a"), dir("project-b")])
        );
        assert_eq!(
            complete("", vec![dir("b"), dir("a"), dir("a")]),
            Outcome::List(vec![dir("a"), dir("b")]),
            "each once"
        );
    }

    #[test]
    fn dot_names_show_for_a_dot_and_case_never_counts() {
        let candidates = vec![dir(".git"), dir("src"), dir("Docs")];
        assert_eq!(
            complete("", candidates.clone()),
            Outcome::List(vec![dir("Docs"), dir("src")])
        );
        assert_eq!(complete(".", candidates.clone()), Outcome::One(dir(".git")));
        assert_eq!(complete("d", candidates), Outcome::One(dir("Docs")));
        assert_eq!(
            complete("D", vec![dir("Desktop"), dir("dev")]),
            Outcome::List(vec![dir("Desktop"), dir("dev")]),
            "as vim's 'fileignorecase'"
        );
        assert_eq!(
            complete("d", vec![dir("Dev"), dir("Desktop")]),
            Outcome::List(vec![dir("Desktop"), dir("Dev")])
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
    fn moves_go_round_through_what_was_typed() {
        let mut choices = Choices::new(vec![dir("a"), dir("b"), host("c")], Quote::Plain);
        assert_eq!(choices.current().as_deref(), Some("a/"), "the first is in");
        assert_eq!(choices.handle(Action::Down), show(Some("b/"), false));
        assert_eq!(choices.handle(Action::Down), show(Some("c:"), false));
        assert_eq!(
            choices.handle(Action::Down),
            show(None, false),
            "after the last, what was typed"
        );
        assert_eq!(choices.handle(Action::Down), show(Some("a/"), false));
        assert_eq!(
            choices.handle(Action::Up),
            show(None, false),
            "before the first, what was typed"
        );
        assert_eq!(choices.handle(Action::Up), show(Some("c:"), false));
        assert_eq!(choices.handle(Action::Up), show(Some("b/"), false));
    }

    #[test]
    fn pages_stop_at_the_ends_then_go_round_through_what_was_typed() {
        let items = (0..25).map(|n| file(&format!("f{n:02}"))).collect();
        let mut choices = Choices::new(items, Quote::Plain);
        assert_eq!(choices.handle(Action::PageDown), show(Some("f10"), false));
        assert_eq!(choices.handle(Action::PageDown), show(Some("f20"), false));
        assert_eq!(choices.handle(Action::PageDown), show(Some("f24"), false));
        assert_eq!(choices.handle(Action::PageDown), show(None, false));
        assert_eq!(choices.handle(Action::PageDown), show(Some("f00"), false));
        assert_eq!(choices.handle(Action::PageUp), show(None, false));
        assert_eq!(choices.handle(Action::PageUp), show(Some("f24"), false));
        assert_eq!(choices.handle(Action::PageUp), show(Some("f14"), false));
    }

    #[test]
    fn ctrl_y_keeps_the_match_ctrl_e_puts_back_what_was_typed_and_other_keys_pass() {
        let mut choices = Choices::new(vec![dir("my dir"), file("x")], Quote::Shell);
        assert_eq!(
            choices.handle(Action::AcceptCompletion),
            show(Some("my\\ dir/"), true)
        );
        choices.handle(Action::Down);
        assert_eq!(choices.handle(Action::CancelCompletion), show(None, true));
        assert_eq!(choices.handle(Action::Backspace), ChoicesEvent::Passed);
        assert_eq!(choices.handle(Action::Confirm), ChoicesEvent::Passed);
        assert_eq!(choices.handle(Action::NormalMode), ChoicesEvent::Passed);
    }

    fn draw(choices: &mut Choices, field: Rect) -> String {
        let mut terminal = Terminal::new(TestBackend::new(20, 8)).unwrap();
        terminal
            .draw(|frame| choices.render(frame, field, frame.area(), &Theme::terminal()))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn the_list_goes_under_the_field_or_above_it() {
        let mut choices = Choices::new(vec![dir("alpha"), dir("beta")], Quote::Plain);
        let below = draw(&mut choices, Rect::new(2, 1, 10, 1));
        let lines: Vec<&str> = below.lines().collect();
        assert!(
            lines[3].contains("alpha/") && lines[4].contains("beta/"),
            "{below}"
        );
        let above = draw(&mut choices, Rect::new(2, 6, 10, 1));
        let lines: Vec<&str> = above.lines().collect();
        assert!(lines[3].contains("alpha/"), "{above}");
        assert_eq!(choices.width(), 6);
    }

    #[test]
    fn a_click_puts_a_match_in_and_a_double_click_keeps_it() {
        let mut choices = Choices::new(vec![dir("alpha"), dir("beta")], Quote::Plain);
        draw(&mut choices, Rect::new(2, 1, 10, 1));
        let press = |press, y| Pointer {
            press,
            at: Position::new(4, y),
        };
        assert_eq!(
            choices.pointer(press(Press::Click, 4)),
            Some(show(Some("beta/"), false))
        );
        assert_eq!(
            choices.pointer(press(Press::DoubleClick, 3)),
            Some(show(Some("alpha/"), true))
        );
        assert_eq!(choices.pointer(press(Press::Click, 2)), None, "the frame");
        assert_eq!(choices.pointer(press(Press::RightClick, 3)), None);
    }

    #[test]
    fn its_thumb_scrolls_the_list_and_leaves_the_match_until_the_next_move() {
        let names = ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9"];
        let mut choices = Choices::new(names.iter().map(|name| dir(name)).collect(), Quote::Plain);
        let field = Rect::new(2, 1, 10, 1);
        let top = draw(&mut choices, field);
        assert!(top.contains('▲') && top.contains('█'), "{top}");
        choices.scroll_to(99);
        let end = draw(&mut choices, field);
        assert!(end.contains("a9/") && !end.contains("a0/"), "{end}");
        assert_eq!(choices.current().as_deref(), Some("a0/"), "the match stays");
        choices.handle(Action::Down);
        assert!(
            draw(&mut choices, field).contains("a1/"),
            "back to the match"
        );
    }
}
