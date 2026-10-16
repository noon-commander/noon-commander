//! Text with a letter that Alt and it act on, as mc and Far mark one in menus and dialogs: an
//! `&` in the Fluent message marks it, `&&` stands for `&`. The pull-down menu runs a command
//! by it; a dialog presses a button, switches a check box, picks a choice, or puts the focus in
//! a field.

use std::ops::Range;

use ratatui::style::Style;
use ratatui::text::Span;

use super::cells::{self, Align, Cut, Marks};

/// Text with the letter that an `&` marks, as the Fluent messages write it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Label {
    pub(crate) text: String,
    /// The index among the characters of `text`, and the letter in lower case.
    pub(crate) hotkey: Option<(usize, char)>,
    /// The index among the characters of `text` of the mark of a name cut short in it.
    pub(crate) cut: Option<usize>,
}

impl Label {
    /// `&&` stands for `&`; the first other `&` marks the letter after it.
    pub(crate) fn parse(source: &str) -> Self {
        let mut text = String::new();
        let mut hotkey = None;
        let mut chars = source.chars().peekable();
        let mut index = 0;
        while let Some(c) = chars.next() {
            let c = match (c, chars.peek()) {
                ('&', Some('&')) => {
                    chars.next();
                    '&'
                }
                ('&', Some(&next)) if hotkey.is_none() => {
                    chars.next();
                    hotkey = Some((index, key(next)));
                    next
                }
                _ => c,
            };
            text.push(c);
            index += 1;
        }
        Self {
            text,
            hotkey,
            cut: None,
        }
    }

    /// `text` as it is, without a letter, such as what ssh asks.
    pub(crate) fn plain(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            ..Self::default()
        }
    }

    /// The letter, in lower case.
    pub(crate) fn letter(&self) -> Option<char> {
        self.hotkey.map(|(_, letter)| letter)
    }

    /// Whether Alt and `c` act on it.
    pub(crate) fn has(&self, c: char) -> bool {
        self.letter() == Some(key(c))
    }

    /// The text in `style`, its letter in `hotkey` over it, and the mark of a name cut short
    /// in `mark` over it. A letter that `hotkey` would not set apart, its color that of the
    /// text, as on a title, is underlined.
    pub(crate) fn spans(
        &self,
        style: Style,
        hotkey: Option<Style>,
        mark: Style,
    ) -> Vec<Span<'static>> {
        let apart = |hotkey: Style| {
            if style.patch(hotkey) == style {
                hotkey.underlined()
            } else {
                hotkey
            }
        };
        let letter = self
            .hotkey
            .zip(hotkey)
            .map(|((at, _), hotkey)| (at, apart(hotkey)));
        let over: Vec<(usize, Style)> = letter
            .into_iter()
            .chain(self.cut.map(|at| (at, mark)))
            .collect();
        spans_over(&self.text, style, over)
    }

    /// The text in exactly `width` cells, padded, or cut in its middle to the mark of `marks`,
    /// in `styles`: the text's, and that of the mark over it; its letter in `hotkey` over the
    /// text's, unless the cut took it.
    pub(crate) fn fit(
        &self,
        width: usize,
        (style, mark): (Style, Style),
        hotkey: Style,
        marks: Marks,
    ) -> Vec<Span<'static>> {
        let fitted = cells::fit_cut(&self.text, width, Align::Left, Cut::Middle);
        let head = fitted.head.chars().count();
        let shown = Self {
            text: fitted.text(marks),
            hotkey: self.hotkey.filter(|&(at, _)| at < head),
            cut: fitted.cut.map(|_| head),
        };
        shown.spans(style, Some(hotkey), mark)
    }

    /// The lines of the text broken to `width` cells, as [`cells::wrap`] breaks them, and where
    /// its letter went: the line, and the index among its characters; `None` where it cannot
    /// be told, as in a word broken across lines.
    pub(crate) fn wrap(&self, width: usize) -> (Vec<String>, Option<(usize, usize)>) {
        let lines = cells::wrap(&self.text, width);
        let Some((at, _)) = self.hotkey else {
            return (lines, None);
        };
        let chars: Vec<char> = self.text.chars().collect();
        // Up to the end of the word of the letter, which breaks as the whole text does.
        let end = chars[at..]
            .iter()
            .position(|&c| c == ' ' || c == '\n')
            .map_or(chars.len(), |offset| at + offset);
        let head: String = chars[..end].iter().collect();
        let head = cells::wrap(&head, width);
        let line = head.len().saturating_sub(1);
        let column = head
            .last()
            .map(|last| last.chars().count())
            .and_then(|count| count.checked_sub(end - at));
        let found = column.filter(|&column| {
            lines.get(line).and_then(|text| text.chars().nth(column)) == chars.get(at).copied()
        });
        (lines, found.map(|column| (line, column)))
    }
}

/// `text` in `style`, each character at an index of `over` in its style over it.
pub(crate) fn spans_over(
    text: &str,
    style: Style,
    mut over: Vec<(usize, Style)>,
) -> Vec<Span<'static>> {
    over.sort_by_key(|(at, _)| *at);
    let chars: Vec<char> = text.chars().collect();
    let part = |range: Range<usize>| chars[range].iter().collect::<String>();
    let mut spans = Vec::new();
    let mut from = 0;
    for (at, over) in over {
        if at < from || at >= chars.len() {
            continue;
        }
        spans.push(Span::styled(part(from..at), style));
        spans.push(Span::styled(part(at..at + 1), style.patch(over)));
        from = at + 1;
    }
    spans.push(Span::styled(part(from..chars.len()), style));
    spans
}

/// Text the user named, such as a file name, for a Fluent message with a letter: its `&`
/// stays a character and marks no letter.
pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&&")
}

/// The letter that `c` typed with Alt stands for: the same in either case.
fn key(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_mark_their_letter() {
        assert_eq!(
            Label::parse("Sort by si&ze"),
            Label {
                text: "Sort by size".to_owned(),
                hotkey: Some((10, 'z')),
                cut: None,
            }
        );
        assert_eq!(Label::parse("&View").hotkey, Some((0, 'v')));
        assert_eq!(
            Label::parse("R&&D &Notes"),
            Label {
                text: "R&D Notes".to_owned(),
                hotkey: Some((4, 'n')),
                cut: None,
            }
        );
        assert_eq!(Label::parse("Plain").hotkey, None);
        assert_eq!(Label::plain("R&D").text, "R&D");
        assert_eq!(Label::plain("R&D").hotkey, None);
    }

    #[test]
    fn either_case_has_the_letter() {
        let label = Label::parse("&Match case");
        assert!(label.has('m'));
        assert!(label.has('M'));
        assert!(!label.has('a'));
    }

    #[test]
    fn escaped_text_marks_no_letter() {
        let label = Label::parse(&format!("&Copy \"{}\" to:", escape("R&D")));
        assert_eq!(label.text, "Copy \"R&D\" to:");
        assert_eq!(label.letter(), Some('c'));
    }

    #[test]
    fn the_letter_keeps_its_place_in_wrapped_lines() {
        let label = Label::parse("Copy \"a long name\" to the &place:");
        let (lines, at) = label.wrap(12);
        assert_eq!(lines, ["Copy \"a long", "name\" to the", "place:"]);
        assert_eq!(at, Some((2, 0)));
        let label = Label::parse("&Enter directory name:");
        assert_eq!(label.wrap(40).1, Some((0, 0)));
        let label = Label::parse("Enter directory na&me:");
        assert_eq!(
            label.wrap(10),
            (
                vec![
                    "Enter".to_owned(),
                    "directory".to_owned(),
                    "name:".to_owned()
                ],
                Some((2, 2))
            )
        );
        assert_eq!(Label::parse("No letter").wrap(5).1, None);
    }

    #[test]
    fn a_letter_in_the_color_of_its_text_is_underlined() {
        use ratatui::style::Color;

        let label = Label::parse("&Keys");
        let gold = Style::new().fg(Color::Yellow);
        let spans = label.spans(gold.bold(), Some(gold), Style::new());
        assert_eq!(spans[1].style, gold.bold().underlined());
        let spans = label.spans(Style::new(), Some(gold), Style::new());
        assert_eq!(spans[1].style, gold, "a color of its own");
    }

    #[test]
    fn spans_set_the_letter_and_the_mark_apart() {
        let mut label = Label::parse("&Open the file");
        label.cut = Some(5);
        let (style, hotkey, mark) = (Style::new(), Style::new().bold(), Style::new().italic());
        let spans = label.spans(style, Some(hotkey), mark);
        let styled: Vec<(&str, Style)> = spans
            .iter()
            .map(|span| (span.content.as_ref(), span.style))
            .collect();
        assert_eq!(
            styled,
            [
                ("", style),
                ("O", hotkey),
                ("pen ", style),
                ("t", mark),
                ("he file", style),
            ]
        );
    }
}
