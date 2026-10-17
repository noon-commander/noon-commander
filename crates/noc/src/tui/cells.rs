//! Text for table cells: sizes and times here, in the formats of the region, and from
//! `noc-text` terminal-safe names, widths, truncation, and wrapping.

use std::sync::LazyLock;
use std::time::SystemTime;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use noc_locale::Formats;
pub(crate) use noc_text::{Align, Cut, Fitted, Marks, chop, fit_cut, sanitize, split, width, wrap};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::i18n::fl;

/// The units after sizes, from bytes up in powers of 1024: B, K, M, G, T, P, E.
static UNITS: LazyLock<[String; 7]> = LazyLock::new(|| {
    [
        fl!("size-bytes"),
        fl!("size-kilo"),
        fl!("size-mega"),
        fl!("size-giga"),
        fl!("size-tera"),
        fl!("size-peta"),
        fl!("size-exa"),
    ]
});

/// A file size in at most `width` cells, grouped by thousands as the region writes numbers:
/// the bytes when they fit, otherwise K, M, G, … (powers of 1024, rounded), as Far shows it.
pub(crate) fn size(bytes: u64, width: usize, formats: &Formats) -> String {
    let mut text = formats.integer(bytes);
    let mut value = bytes;
    for unit in &UNITS[1..] {
        if self::width(&text) <= width {
            break;
        }
        value = value / 1024 + u64::from(value % 1024 >= 512);
        text = format!("{}{unit}", formats.integer(value));
    }
    text
}

/// A size with its unit, in powers of 1024 as sizes in panels, with a decimal below 100 so
/// that a slow transfer still shows change: `512B`, `1.5M`, `480K`.
pub(crate) fn short_size(bytes: f64, formats: &Formats) -> String {
    let mut value = bytes.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    let fraction = usize::from(unit > 0 && value < 99.95);
    format!("{}{}", formats.decimal(value, fraction, false), UNITS[unit])
}

/// A modification time in `tz`, its date and its time of day, as every time shows in the
/// formats of the region; nothing when unknown. At most [`Formats::time_width`] cells.
pub(crate) fn mtime(time: Option<SystemTime>, tz: &TimeZone, formats: &Formats) -> String {
    time.and_then(|time| Timestamp::try_from(time).ok())
        .map(|stamp| formats.date_time(stamp.to_zoned(tz.clone()).datetime()))
        .unwrap_or_default()
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

/// `spans` in exactly `width` cells: padded in `pad` where they are shorter; where longer, cut
/// at their start, which the mark of text hidden before of `marks` stands for, in `mark` over
/// the style of the span it cuts: a path whose name, at its end, matters most.
pub(crate) fn fit_spans_end(
    spans: Vec<Span<'static>>,
    width: usize,
    (pad, mark): (Style, Style),
    marks: Marks,
) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(Span::width).sum();
    if total <= width {
        return fit_spans(spans, width, (pad, mark), marks);
    }
    let room = width.saturating_sub(1);
    let mut used = 0;
    let mut kept = Vec::with_capacity(spans.len() + 1);
    for span in spans.into_iter().rev() {
        let cells = span.width();
        if used + cells <= room {
            used += cells;
            kept.push(span);
            continue;
        }
        let style = span.style;
        let (_, tail) = split(&span.content, cells - (room - used));
        kept.push(Span::styled(tail, style));
        if width > 0 {
            kept.push(Span::styled(marks.before, style.patch(mark)));
        }
        break;
    }
    kept.reverse();
    kept
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
    fn spans_too_long_lose_their_start_and_keep_their_styles() {
        let bold = Style::new().bold();
        let spans = || {
            vec![
                Span::raw("src/tui/"),
                Span::styled("find", bold),
                Span::raw(".rs"),
            ]
        };
        let marks = Glyphs::PLAIN.marks;
        let styles = (Style::new(), Style::new());
        let fitted = fit_spans_end(spans(), 9, styles, marks);
        assert_eq!(text_of(&fitted), format!("{}/find.rs", marks.before));
        assert_eq!(fitted[2].style, bold, "the match keeps its style");
        let short = fit_spans_end(spans(), 17, styles, marks);
        assert_eq!(text_of(&short), "src/tui/find.rs  ");
        let wide = vec![Span::raw("文件/a")];
        assert_eq!(width(&text_of(&fit_spans_end(wide, 4, styles, marks))), 4);
    }

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
    fn sizes_fit_their_cells() {
        let iso = Formats::iso();
        assert_eq!(size(0, 10, &iso), "0");
        assert_eq!(size(9_999_999_999, 10, &iso), "9999999999");
        assert_eq!(size(10_000_000_000, 10, &iso), "9765625K");
        assert_eq!(size(1_536, 3, &iso), "2K");
        assert_eq!(size(5 * 1024 * 1024 * 1024, 4, &iso), "5G");
        assert!(
            size(u64::MAX, 10, &iso).len() <= 10,
            "{}",
            size(u64::MAX, 10, &iso)
        );
        // Grouped as the formats group numbers: Unicode's root formats, which no region has.
        let root = Formats::detect("und");
        assert_eq!(size(99_999_999, 10, &root), "99,999,999");
        assert_eq!(size(123_456_789, 10, &root), "120,563K");
    }

    #[test]
    fn short_sizes_have_a_unit_and_a_decimal_below_a_hundred() {
        let iso = Formats::iso();
        assert_eq!(short_size(512.0, &iso), "512B");
        assert_eq!(short_size(1536.0, &iso), "1.5K");
        assert_eq!(short_size(480.0 * 1024.0, &iso), "480K");
        assert_eq!(short_size(12.34 * 1024.0 * 1024.0, &iso), "12.3M");
        assert_eq!(short_size(99.97 * 1024.0, &iso), "100K");
        assert_eq!(short_size(3.0 * 1024.0 * 1024.0 * 1024.0, &iso), "3.0G");
    }

    #[test]
    fn times_show_their_date_and_time() {
        let iso = Formats::iso();
        let at = |seconds| Some(UNIX_EPOCH + Duration::from_secs(seconds));
        // 2023-11-14 22:13:20 UTC
        let utc = TimeZone::UTC;
        assert_eq!(mtime(at(1_700_000_000), &utc, &iso), "2023-11-14 22:13");
        assert_eq!(mtime(at(1_600_000_000), &utc, &iso), "2020-09-13 12:26");
        assert_eq!(mtime(None, &utc, &iso), "");
        let tokyo = TimeZone::get("Asia/Tokyo").unwrap();
        assert_eq!(mtime(at(1_700_000_000), &tokyo, &iso), "2023-11-15 07:13");
    }
}
