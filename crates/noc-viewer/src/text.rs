//! A file's bytes as lines for the screen, and the cells of a line that show.

use unicode_width::UnicodeWidthChar as _;

/// Columns between tab stops.
const TAB: usize = 8;

/// The lines of `bytes`, as UTF-8 with invalid bytes replaced: `\r\n` ends a line too, tabs
/// go to the next stop, and what a terminal would act on is shown safely.
pub(crate) fn lines_of(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_suffix('\n').unwrap_or(&text);
    text.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let mut expanded = String::with_capacity(line.len());
            let mut column = 0;
            for c in line.chars() {
                if c == '\t' {
                    let spaces = TAB - column % TAB;
                    expanded.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                } else {
                    expanded.push(c);
                    column += c.width().unwrap_or(0);
                }
            }
            noc_text::sanitize(expanded.as_bytes())
        })
        .collect()
}

/// `line` in rows of at most `width` cells, broken anywhere, as mc wraps; at least one row,
/// and at least one character in each.
pub(crate) fn rows(line: &str, width: usize) -> Vec<&str> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (index, c) in line.char_indices() {
        let char_width = c.width().unwrap_or(0);
        if used + char_width > width && index > start {
            rows.push(&line[start..index]);
            (start, used) = (index, 0);
        }
        used += char_width;
    }
    rows.push(&line[start..]);
    rows
}

/// The cells `skip` … `skip + width` of `line`; a wide character cut in two shows as a space.
pub(crate) fn cut(line: &str, skip: usize, width: usize) -> String {
    let mut text = String::new();
    let (mut column, mut used) = (0, 0);
    for c in line.chars() {
        let char_width = c.width().unwrap_or(0);
        let start = column;
        column += char_width;
        if column <= skip {
            continue;
        }
        let shown = if start < skip {
            column - skip
        } else {
            char_width
        };
        if used + shown > width {
            break;
        }
        if shown < char_width {
            text.push_str(&" ".repeat(shown));
        } else {
            text.push(c);
        }
        used += shown;
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_lines_safe_and_expands_tabs() {
        let lines = lines_of(b"a\tb\r\nwide\xe6\x96\x87\tx\n\x1b[2Jbad \xff\n\n");
        assert_eq!(
            lines,
            ["a       b", "wide文  x", "?[2Jbad \u{fffd}", ""],
            "one line per newline, the last ending none"
        );
        assert_eq!(lines_of(b""), [""]);
    }

    #[test]
    fn wraps_and_cuts_by_cells() {
        assert_eq!(rows("abcdefg", 3), ["abc", "def", "g"]);
        assert_eq!(rows("", 3), [""]);
        assert_eq!(rows("文文文", 5), ["文文", "文"]);
        assert_eq!(
            rows("文", 1),
            ["文"],
            "a character wider than a row gets one"
        );
        assert_eq!(cut("abcdef", 2, 3), "cde");
        assert_eq!(
            cut("文文文", 1, 4),
            " 文",
            "a cut half is a space, and what does not fit is left out"
        );
        assert_eq!(cut("ab", 5, 3), "");
    }
}
