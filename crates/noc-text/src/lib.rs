//! Text for the terminal: names made safe to show, widths in cells, and text fitted, wrapped,
//! or split by cells.
//!
//! Every crate that draws text shares these, so that what is safe to show and how wide a
//! character is are decided in one place.

use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

/// Where text goes in a cell wider than the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A name made safe for the terminal: invalid UTF-8 is replaced, and control characters, bidi
/// controls, and marks that join the next character, which could move the cursor, reorder the
/// screen, or shift the rest of a row, are shown as `?`.
pub fn sanitize(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().map(safe).collect()
}

/// `c` as [`sanitize`] shows it: itself, or `?` if the terminal could act on it.
pub fn safe(c: char) -> char {
    if is_unsafe(c) { '?' } else { c }
}

fn is_unsafe(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
        || is_prepended(c)
}

/// Whether `c` joins the character after it into one grapheme (Unicode's `Prepend`): ratatui
/// draws the two in the cells of both, while a terminal that does not join them gives the first
/// a width of its own, often none, and moves the rest of the row.
fn is_prepended(c: char) -> bool {
    matches!(
        c,
        '\u{0600}'..='\u{0605}'
            | '\u{06dd}'
            | '\u{070f}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08e2}'
            | '\u{0d4e}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{111c2}'..='\u{111c3}'
            | '\u{1193f}'
            | '\u{11941}'
            | '\u{11a3a}'
            | '\u{11a84}'..='\u{11a89}'
            | '\u{11d46}'
            | '\u{11f02}'
    )
}

/// Width of `text` in terminal cells.
pub fn width(text: &str) -> usize {
    text.width()
}

/// The marks of text that goes on past where it shows, one cell each. The app picks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marks {
    /// Text hidden before: a path that lost its start, a line scrolled left.
    pub before: &'static str,
    /// Text cut short or going on after: a name that lost its middle, a line past the right
    /// edge, a wrapped line going on in the next row.
    pub after: &'static str,
}

/// Where text that does not fit loses cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// Its middle, so that a name keeps its start and its extension, as mc cuts names.
    Middle,
    /// Its start, so that a path keeps its last directories.
    Start,
    /// Its end.
    End,
}

/// Text fitted into a width: `head`, a mark where text was cut, then `tail`. Together they
/// take exactly the width.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fitted {
    pub head: String,
    /// Where text was cut, if it was: the mark goes between `head` and `tail`.
    pub cut: Option<Cut>,
    pub tail: String,
}

impl Fitted {
    /// The mark of the text cut, of `marks`: [`Marks::before`] where the text lost its start,
    /// else [`Marks::after`]; empty where nothing was cut.
    pub fn mark(&self, marks: Marks) -> &'static str {
        match self.cut {
            None => "",
            Some(Cut::Start) => marks.before,
            Some(Cut::Middle | Cut::End) => marks.after,
        }
    }

    /// The text, with the mark of `marks`.
    pub fn text(&self, marks: Marks) -> String {
        format!("{}{}{}", self.head, self.mark(marks), self.tail)
    }
}

/// Fits `text` into exactly `width` cells: shorter text is padded; longer text loses its
/// middle, to the mark of `marks`.
pub fn fit(text: &str, width: usize, align: Align, marks: Marks) -> String {
    fit_cut(text, width, align, Cut::Middle).text(marks)
}

/// Fits `text` into exactly `width` cells: shorter text is padded as `align` says; longer text
/// loses cells where `cut` says, and a mark takes one of them. A wide character that does not
/// fit leaves a blank next to the mark.
pub fn fit_cut(text: &str, width: usize, align: Align, cut: Cut) -> Fitted {
    let text_width = text.width();
    if text_width <= width {
        let gap = width - text_width;
        let (left, right) = match align {
            Align::Left => (0, gap),
            Align::Center => (gap / 2, gap - gap / 2),
            Align::Right => (gap, 0),
        };
        return Fitted {
            head: format!("{}{text}{}", " ".repeat(left), " ".repeat(right)),
            ..Fitted::default()
        };
    }
    if width == 0 {
        return Fitted::default();
    }
    let kept = width - 1;
    let (head_cells, tail_cells) = match cut {
        Cut::Middle => (kept - kept / 2, kept / 2),
        Cut::Start => (0, kept),
        Cut::End => (kept, 0),
    };
    let (head, head_width) = take_width(text.chars(), head_cells);
    let (tail, tail_width) = take_width(text.chars().rev(), tail_cells);
    let tail: String = tail.chars().rev().collect();
    let gap = " ".repeat(kept - head_width - tail_width);
    let (head, tail) = match cut {
        Cut::End => (head + &gap, tail),
        Cut::Middle | Cut::Start => (head, gap + &tail),
    };
    Fitted {
        head,
        cut: Some(cut),
        tail,
    }
}

/// `text` broken into rows of at most `width` cells, keeping every character: after the last
/// `after` that fits in a row where there is one, as a path breaks after a `/`, otherwise
/// anywhere. A character wider than a row gets one of its own.
pub fn chop(text: &str, width: usize, after: Option<char>) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut rest = text;
    while rest.width() > width {
        let (head, _) = take_width(rest.chars(), width);
        let mut end = head
            .len()
            .max(rest.chars().next().map_or(0, char::len_utf8));
        if let Some(after) = after
            && let Some(at) = head.rfind(after)
            && at > 0
            && at + after.len_utf8() < head.len()
        {
            end = at + after.len_utf8();
        }
        rows.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    if !rest.is_empty() || rows.is_empty() {
        rows.push(rest.to_owned());
    }
    rows
}

/// A line of exactly `width` cells drawn with `line`, a character of one cell, with `text` in
/// its middle between spaces, its middle cut as [`fit`] cuts it where the line is too short
/// for it.
pub fn rule(text: &str, width: usize, line: &str) -> Fitted {
    let head = if text.is_empty() {
        line.repeat(width)
    } else {
        let label = format!(" {text} ");
        let label_width = label.width();
        if label_width + 2 > width {
            return fit_cut(text, width, Align::Center, Cut::Middle);
        }
        let gap = width - label_width;
        let (left, right) = (gap / 2, gap - gap / 2);
        format!("{}{label}{}", line.repeat(left), line.repeat(right))
    };
    Fitted {
        head,
        ..Fitted::default()
    }
}

/// Breaks `text` into lines of at most `width` cells: at its newlines, then between words,
/// and inside words longer than a line. Each line is made terminal-safe.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        let mut used = 0;
        for word in sanitize(paragraph.as_bytes()).split(' ') {
            let mut word = word;
            loop {
                let word_width = word.width();
                let gap = usize::from(used > 0);
                if used + gap + word_width <= width {
                    if gap == 1 {
                        line.push(' ');
                    }
                    line.push_str(word);
                    used += gap + word_width;
                    break;
                }
                if used > 0 {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                    continue;
                }
                // A word longer than a line goes on in the next one; a character wider than a
                // line gets one of its own.
                let (head, _) = take_width(word.chars(), width);
                let head_len = head
                    .len()
                    .max(word.chars().next().map_or(0, char::len_utf8));
                lines.push(word[..head_len].to_owned());
                word = &word[head_len..];
                if word.is_empty() {
                    break;
                }
            }
        }
        lines.push(line);
    }
    lines
}

/// The longest prefix of `chars` that fits into `limit` cells, and its width.
fn take_width(chars: impl Iterator<Item = char>, limit: usize) -> (String, usize) {
    let mut taken = String::new();
    let mut used = 0;
    for c in chars {
        let char_width = c.width().unwrap_or(0);
        if used + char_width > limit {
            break;
        }
        used += char_width;
        taken.push(c);
    }
    (taken, used)
}

/// `text` cut in two at `at` cells; a wide character across the cut becomes a space on either
/// side, so that both halves keep their widths.
pub fn split(text: &str, at: usize) -> (String, String) {
    let (mut head, head_width) = take_width(text.chars(), at);
    let mut rest = text[head.len()..].chars();
    let mut tail = String::new();
    if head_width < at {
        head.push_str(&" ".repeat(at - head_width));
        if let Some(c) = rest.next() {
            let cut = c.width().unwrap_or(0);
            tail.push_str(&" ".repeat((head_width + cut).saturating_sub(at)));
        }
    }
    tail.extend(rest);
    (head, tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKS: Marks = Marks {
        before: "<",
        after: ">",
    };

    #[test]
    fn sanitize_hides_what_could_drive_the_terminal() {
        assert_eq!(sanitize(b"notes.txt"), "notes.txt");
        assert_eq!(sanitize(b"evil\x1b]0;title\x07.txt"), "evil?]0;title?.txt");
        assert_eq!(sanitize(b"two\nlines\r\t"), "two?lines??");
        assert_eq!(sanitize("del\u{7f}c1\u{9b}".as_bytes()), "del?c1?");
        assert_eq!(sanitize("txt.\u{202e}exe".as_bytes()), "txt.?exe");
        assert_eq!(
            sanitize("\u{0602}R\u{06dd}1".as_bytes()),
            "?R?1",
            "a mark that joins the next character would move the rest of the row"
        );
        assert_eq!(sanitize(b"latin1 \xe9t\xe9"), "latin1 \u{fffd}t\u{fffd}");
        assert_eq!(sanitize("Привет 文件".as_bytes()), "Привет 文件");
    }

    #[test]
    fn widths_count_cells() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("文件"), 4);
        assert_eq!(width("é"), 1);
    }

    #[test]
    fn fit_pads_short_text() {
        assert_eq!(fit("ab", 5, Align::Left, MARKS), "ab   ");
        assert_eq!(fit("ab", 5, Align::Right, MARKS), "   ab");
        assert_eq!(fit("ab", 5, Align::Center, MARKS), " ab  ");
        assert_eq!(fit("abcde", 5, Align::Left, MARKS), "abcde");
    }

    #[test]
    fn rule_draws_a_line_with_text_in_its_middle() {
        let rule = |text, width, line| rule(text, width, line).text(MARKS);
        assert_eq!(rule("EOF", 11, "-"), "--- EOF ---");
        assert_eq!(rule("EOF", 12, "-"), "--- EOF ----");
        assert_eq!(rule("EOF", 7, "-"), "- EOF -");
        assert_eq!(rule("EOF", 6, "-"), " EOF  ", "no room for the line");
        assert_eq!(rule("EOF", 2, "-"), "E>");
        assert_eq!(rule("", 3, "-"), "---");
        assert_eq!(width(&rule("界", 10, "-")), 10);
        assert_eq!(super::rule("EOF", 2, "-").cut, Some(Cut::Middle));
    }

    #[test]
    fn split_keeps_the_widths_of_both_halves() {
        assert_eq!(split("abcdef", 2), ("ab".to_owned(), "cdef".to_owned()));
        assert_eq!(split("ab", 3), ("ab ".to_owned(), String::new()));
        assert_eq!(
            split("a文b", 2),
            ("a ".to_owned(), " b".to_owned()),
            "a wide character across the cut leaves a blank on either side"
        );
        assert_eq!(split("文b", 2), ("文".to_owned(), "b".to_owned()));
    }

    #[test]
    fn fit_cuts_the_middle_of_long_text() {
        assert_eq!(fit("verylongname.txt", 9, Align::Left, MARKS), "very>.txt");
        assert_eq!(fit("abcdef", 4, Align::Right, MARKS), "ab>f");
        assert_eq!(fit("abc", 1, Align::Left, MARKS), ">");
        assert_eq!(fit("abc", 0, Align::Left, MARKS), "");
        let wide = fit("文件文件文件", 6, Align::Left, MARKS);
        assert_eq!(width(&wide), 6, "{wide:?}");
        assert_eq!(
            wide, "文> 件",
            "a wide character never splits; its cell stays blank"
        );
    }

    #[test]
    fn fit_cut_cuts_where_it_is_asked_and_says_where() {
        let parts = |text, width, cut| {
            let fitted = fit_cut(text, width, Align::Left, cut);
            (fitted.head, fitted.cut.is_some(), fitted.tail)
        };
        let owned = |head: &str, cut, tail: &str| (head.to_owned(), cut, tail.to_owned());
        assert_eq!(
            parts("/srv/www/site", 8, Cut::Start),
            owned("", true, "ww/site")
        );
        assert_eq!(parts("abcdef", 4, Cut::End), owned("abc", true, ""));
        assert_eq!(parts("abcdef", 4, Cut::Middle), owned("ab", true, "f"));
        assert_eq!(parts("abc", 4, Cut::Start), owned("abc ", false, ""));
        assert_eq!(parts("abc", 0, Cut::End), owned("", false, ""));
        assert_eq!(parts("a文件", 3, Cut::Start), owned("", true, "件"));
        assert_eq!(parts("文件", 2, Cut::Start), owned("", true, " "));
        assert_eq!(parts("文件b", 3, Cut::End), owned("文", true, ""));
        assert_eq!(parts("文件", 2, Cut::End), owned(" ", true, ""));
        for cut in [Cut::Start, Cut::Middle, Cut::End] {
            let fitted = fit_cut("文件文件文件x", 7, Align::Left, cut).text(MARKS);
            assert_eq!(width(&fitted), 7, "{fitted:?}");
        }
    }

    #[test]
    fn a_path_cut_at_its_start_takes_the_mark_before() {
        let path = fit_cut("/srv/www/site", 8, Align::Left, Cut::Start);
        assert_eq!(path.text(MARKS), "<ww/site");
        let name = fit_cut("abcdef", 4, Align::Left, Cut::End);
        let marks = Marks {
            before: "<",
            after: ">",
        };
        assert_eq!(name.text(marks), "abc>");
        assert_eq!(fit_cut("ab", 4, Align::Left, Cut::End).text(marks), "ab  ");
    }

    #[test]
    fn chop_keeps_every_character_and_breaks_after_a_separator() {
        assert_eq!(chop("abcdefg", 3, None), ["abc", "def", "g"]);
        assert_eq!(chop("a b", 10, None), ["a b"]);
        assert_eq!(chop("", 3, None), [""]);
        assert_eq!(
            chop("/srv/www/clients/acme", 12, Some('/')),
            ["/srv/www/", "clients/acme"]
        );
        assert_eq!(
            chop("/averyverylongname", 6, Some('/')),
            ["/avery", "verylo", "ngname"],
            "with no separator inside a row, anywhere"
        );
        assert_eq!(chop("文件文", 3, None), ["文", "件", "文"]);
        assert_eq!(chop("文", 1, None), ["文"]);
    }

    #[test]
    fn wrap_breaks_lines_at_newlines_spaces_and_long_words() {
        assert_eq!(
            wrap("The authenticity of host 'web' can't be established.", 20),
            ["The authenticity of", "host 'web' can't be", "established."]
        );
        assert_eq!(wrap("one\ntwo  three", 20), ["one", "two  three"]);
        assert_eq!(
            wrap("SHA256:abcdefghijklmnop", 8),
            ["SHA256:a", "bcdefghi", "jklmnop"]
        );
        assert_eq!(wrap("bad\x1b[2J", 20), ["bad?[2J"]);
        assert_eq!(wrap("文件文件", 3), ["文", "件", "文", "件"]);
        assert_eq!(wrap("", 10), Vec::<String>::new());
    }
}
