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

/// A name made safe for the terminal: invalid UTF-8 is replaced, and control characters and
/// bidi controls, which could move the cursor or reorder the screen, are shown as `?`.
pub fn sanitize(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if is_unsafe(c) { '?' } else { c })
        .collect()
}

fn is_unsafe(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

/// Width of `text` in terminal cells.
pub fn width(text: &str) -> usize {
    text.width()
}

/// Fits `text` into exactly `width` cells: shorter text is padded; longer text loses its
/// middle, marked with `~` as mc does.
pub fn fit(text: &str, width: usize, align: Align) -> String {
    let text_width = text.width();
    if text_width <= width {
        let gap = width - text_width;
        let (left, right) = match align {
            Align::Left => (0, gap),
            Align::Center => (gap / 2, gap - gap / 2),
            Align::Right => (gap, 0),
        };
        return format!("{}{text}{}", " ".repeat(left), " ".repeat(right));
    }
    if width == 0 {
        return String::new();
    }
    let kept = width - 1;
    let (head, head_width) = take_width(text.chars(), kept - kept / 2);
    let (tail, tail_width) = take_width(text.chars().rev(), kept / 2);
    let tail: String = tail.chars().rev().collect();
    // A wide character that does not fit leaves a gap.
    let gap = width - 1 - head_width - tail_width;
    format!("{head}~{}{tail}", " ".repeat(gap))
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

    #[test]
    fn sanitize_hides_what_could_drive_the_terminal() {
        assert_eq!(sanitize(b"notes.txt"), "notes.txt");
        assert_eq!(sanitize(b"evil\x1b]0;title\x07.txt"), "evil?]0;title?.txt");
        assert_eq!(sanitize(b"two\nlines\r\t"), "two?lines??");
        assert_eq!(sanitize("del\u{7f}c1\u{9b}".as_bytes()), "del?c1?");
        assert_eq!(sanitize("txt.\u{202e}exe".as_bytes()), "txt.?exe");
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
        assert_eq!(fit("ab", 5, Align::Left), "ab   ");
        assert_eq!(fit("ab", 5, Align::Right), "   ab");
        assert_eq!(fit("ab", 5, Align::Center), " ab  ");
        assert_eq!(fit("abcde", 5, Align::Left), "abcde");
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
        assert_eq!(fit("verylongname.txt", 9, Align::Left), "very~.txt");
        assert_eq!(fit("abcdef", 4, Align::Right), "ab~f");
        assert_eq!(fit("abc", 1, Align::Left), "~");
        assert_eq!(fit("abc", 0, Align::Left), "");
        let wide = fit("文件文件文件", 6, Align::Left);
        assert_eq!(width(&wide), 6, "{wide:?}");
        assert_eq!(
            wide, "文~ 件",
            "a wide character never splits; its cell stays blank"
        );
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
