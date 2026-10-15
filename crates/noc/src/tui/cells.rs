//! Text for table cells: sizes and times here, and from `noc-text` terminal-safe names,
//! widths, truncation, and wrapping.

use std::time::SystemTime;

use jiff::Timestamp;
use jiff::tz::TimeZone;
pub(crate) use noc_text::{Align, Cut, Fitted, Marks, chop, fit_cut, sanitize, split, width, wrap};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Width in cells of [`mtime`].
pub(crate) const MTIME_WIDTH: usize = 12;

/// Half a year in seconds: newer times show the time of day, older ones the year.
const SIX_MONTHS: i64 = 15_778_476;

/// `bytes` with a comma between each group of three digits, as mc shows the size of marked
/// files.
pub(crate) fn grouped(bytes: u64) -> String {
    let digits = bytes.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// A file size in at most `width` cells: the bytes when they fit, otherwise K, M, G, …
/// (powers of 1024, rounded), as mc shows it.
pub(crate) fn size(bytes: u64, width: usize) -> String {
    let plain = bytes.to_string();
    if plain.len() <= width {
        return plain;
    }
    let mut value = bytes;
    let mut text = plain;
    for unit in ['K', 'M', 'G', 'T', 'P', 'E'] {
        value = value / 1024 + u64::from(value % 1024 >= 512);
        text = format!("{value}{unit}");
        if text.len() <= width {
            break;
        }
    }
    text
}

/// A modification time as `ls -l` and mc show it: `Sep 30 12:34` for the last six months,
/// `Sep 30  2025` otherwise, spaces when unknown. [`MTIME_WIDTH`] cells.
pub(crate) fn mtime(time: Option<SystemTime>, now: SystemTime, tz: &TimeZone) -> String {
    let Some(stamp) = time.and_then(|time| Timestamp::try_from(time).ok()) else {
        return " ".repeat(MTIME_WIDTH);
    };
    let age = Timestamp::try_from(now).map_or(0, |now| now.as_second() - stamp.as_second());
    // Up to an hour in the future still counts as recent, for clocks that differ a little.
    let format = if (-3600..SIX_MONTHS).contains(&age) {
        "%b %e %H:%M"
    } else {
        "%b %e  %Y"
    };
    stamp.to_zoned(tz.clone()).strftime(format).to_string()
}

/// Fits `text` into exactly `width` cells: shorter text is padded; longer text loses its
/// middle, to the mark of `marks`. Spans in `style`, the mark in `mark` over it.
pub(crate) fn fit(
    text: &str,
    width: usize,
    align: Align,
    styles: (Style, Style),
    marks: Marks,
) -> Vec<Span<'static>> {
    spans(fit_cut(text, width, align, Cut::Middle), styles, marks)
}

/// `fitted` in `style`, with its mark of `marks`, where it was cut, in `mark` over `style`:
/// the mark stands out wherever text is cut short.
pub(crate) fn spans(
    fitted: Fitted,
    (style, mark): (Style, Style),
    marks: Marks,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(fitted.head.clone(), style)];
    if fitted.cut.is_some() {
        spans.push(Span::styled(fitted.mark(marks), style.patch(mark)));
    }
    spans.push(Span::styled(fitted.tail, style));
    spans
}

/// `text` fitted into `width` cells as [`fit_cut`] fits it, in `style`, with the mark of
/// `marks` where it was cut in `mark` over `style`.
pub(crate) fn fitted(
    text: &str,
    width: usize,
    (align, cut): (Align, Cut),
    styles: (Style, Style),
    marks: Marks,
) -> Line<'static> {
    Line::from(spans(fit_cut(text, width, align, cut), styles, marks))
}

/// `spans` in exactly `width` cells: padded in `pad` where they are shorter; where longer, cut
/// at their end, the mark of `marks` in `mark` over the style of the span it cuts.
pub(crate) fn fit_spans(
    spans: Vec<Span<'static>>,
    width: usize,
    (pad, mark): (Style, Style),
    marks: Marks,
) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(Span::width).sum();
    if total <= width {
        let mut spans = spans;
        if total < width {
            spans.push(Span::styled(" ".repeat(width - total), pad));
        }
        return spans;
    }
    let room = width.saturating_sub(1);
    let mut used = 0;
    let mut fitted = Vec::with_capacity(spans.len() + 1);
    for span in spans {
        let cells = span.width();
        if used + cells <= room {
            used += cells;
            fitted.push(span);
            continue;
        }
        let style = span.style;
        let (head, _) = split(&span.content, room - used);
        fitted.push(Span::styled(head, style));
        if width > 0 {
            fitted.push(Span::styled(marks.after, style.patch(mark)));
        }
        break;
    }
    fitted
}

/// The text of `spans`, without their styles: for widths, and where nothing can be styled.
pub(crate) fn text_of(spans: &[Span<'_>]) -> String {
    spans.iter().map(|span| span.content.as_ref()).collect()
}

/// `text` in exactly `rows` lines of `width` cells, broken after `after` where it can be, as
/// [`chop`] breaks it: the last one cut at its end with the mark in `mark`, if the text goes
/// on, and blank ones under a short text.
pub(crate) fn whole(
    text: &str,
    (width, rows): (usize, usize),
    after: Option<char>,
    (style, mark): (Style, Style),
    marks: Marks,
) -> Vec<Line<'static>> {
    let mut chopped = chop(text, width, after);
    if chopped.len() > rows {
        let rest = chopped.split_off(rows.saturating_sub(1)).concat();
        chopped.push(rest);
    }
    let mut lines: Vec<Line<'static>> = chopped
        .iter()
        .map(|row| fitted(row, width, (Align::Left, Cut::End), (style, mark), marks))
        .collect();
    lines.resize(rows, Line::styled(" ".repeat(width), style));
    lines
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::tui::glyphs::Glyphs;

    #[test]
    fn whole_text_takes_as_many_lines_as_asked_and_marks_the_rest() {
        let styles = (Style::new(), Style::new());
        let text = |lines: Vec<Line<'static>>| -> Vec<String> {
            lines.iter().map(ToString::to_string).collect()
        };
        assert_eq!(
            text(whole(
                "/srv/www/site",
                (9, 2),
                Some('/'),
                styles,
                Glyphs::PLAIN.marks
            )),
            ["/srv/www/", "site     "]
        );
        assert_eq!(
            text(whole("ab", (4, 2), None, styles, Glyphs::PLAIN.marks)),
            ["ab  ", "    "]
        );
        assert_eq!(
            text(whole(
                "abcdefghij",
                (4, 2),
                None,
                styles,
                Glyphs::PLAIN.marks
            )),
            ["abcd", "efg»"],
            "the last line ends with the mark where the text goes on"
        );
    }

    #[test]
    fn the_mark_of_text_cut_short_takes_its_own_style() {
        let style = Style::new().bold();
        let mark = Style::new().underlined();
        let marks = Glyphs::PLAIN.marks;
        let styled = |spans: Vec<Span<'static>>| -> Vec<(String, Style)> {
            spans
                .into_iter()
                .filter(|span| !span.content.is_empty())
                .map(|span| (span.content.into_owned(), span.style))
                .collect()
        };
        let owned = |text: &str, style| (text.to_owned(), style);
        assert_eq!(
            styled(fit("abcdef", 4, Align::Left, (style, mark), marks)),
            [
                owned("ab", style),
                owned("»", style.patch(mark)),
                owned("f", style)
            ]
        );
        assert_eq!(
            styled(fit("ab", 4, Align::Left, (style, mark), marks)),
            [owned("ab  ", style)]
        );
        let spans = vec![Span::styled("abc", style), Span::raw("def")];
        assert_eq!(
            styled(fit_spans(spans.clone(), 5, (style, mark), marks)),
            [
                owned("abc", style),
                owned("d", Style::new()),
                owned("»", Style::new().patch(mark))
            ],
            "cut at the end, the mark over the span it cuts"
        );
        assert_eq!(
            styled(fit_spans(spans.clone(), 8, (style, mark), marks)),
            [
                owned("abc", style),
                owned("def", Style::new()),
                owned("  ", style)
            ]
        );
        assert_eq!(text_of(&fit_spans(spans, 0, (style, mark), marks)), "");
    }

    #[test]
    fn groups_digits_in_threes() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(10_012_345), "10,012,345");
        assert_eq!(grouped(u64::MAX), "18,446,744,073,709,551,615");
    }

    #[test]
    fn sizes_fit_their_cells() {
        assert_eq!(size(0, 7), "0");
        assert_eq!(size(9_999_999, 7), "9999999");
        assert_eq!(size(10_000_000, 7), "9766K");
        assert_eq!(size(1_536, 3), "2K");
        assert_eq!(size(5 * 1024 * 1024 * 1024, 4), "5G");
        assert!(size(u64::MAX, 7).len() <= 7, "{}", size(u64::MAX, 7));
    }

    #[test]
    fn times_show_the_time_of_day_or_the_year() {
        let at = |seconds| UNIX_EPOCH + Duration::from_secs(seconds);
        // 2023-11-14 22:13:20 UTC
        let now = at(1_700_000_000);
        assert_eq!(mtime(Some(now), now, &TimeZone::UTC), "Nov 14 22:13");
        assert_eq!(
            mtime(Some(at(1_700_000_000 - 86_400 * 40)), now, &TimeZone::UTC),
            "Oct  5 22:13"
        );
        assert_eq!(
            mtime(Some(at(1_600_000_000)), now, &TimeZone::UTC),
            "Sep 13  2020"
        );
        assert_eq!(
            mtime(Some(at(1_700_000_000 + 86_400)), now, &TimeZone::UTC),
            "Nov 15  2023",
            "far in the future"
        );
        assert_eq!(mtime(None, now, &TimeZone::UTC), " ".repeat(MTIME_WIDTH));
        for time in [now, at(1_600_000_000)] {
            assert_eq!(width(&mtime(Some(time), now, &TimeZone::UTC)), MTIME_WIDTH);
        }
        let tokyo = TimeZone::get("Asia/Tokyo").unwrap();
        assert_eq!(mtime(Some(now), now, &tokyo), "Nov 15 07:13");
    }
}
