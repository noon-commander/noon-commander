use super::*;

fn skeleton_colors(shine: Option<Shine>) -> Vec<Option<Color>> {
    let mut theme = Theme::terminal();
    theme.dialog = Style::new()
        .fg(Color::Rgb(255, 255, 255))
        .bg(Color::Rgb(0, 0, 0));
    let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
    terminal
        .draw(|frame| draw_skeleton(frame, frame.area(), &theme, shine))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..30).map(|x| buffer[(x, 0)].style().fg).collect()
}

#[test]
fn a_skeleton_lights_its_bars_where_the_shine_passes() {
    let faint = Some(Color::Rgb(64, 64, 64));
    let still = skeleton_colors(None);
    // Bars of 15 and 7 cells, the gap between them in the color of the dialog.
    assert!(
        still[..15]
            .iter()
            .chain(&still[23..])
            .all(|fg| *fg == faint)
    );
    assert!(
        still[15..23]
            .iter()
            .all(|fg| *fg == Some(Color::Rgb(255, 255, 255)))
    );
    assert_eq!(
        skeleton_colors(Some(Shine::after(0))),
        still,
        "the shine comes in from the left"
    );
    // Twelve cells on, coming in five cells before the bars, its middle is on cell 7 of both:
    // they shine at once and as fast, the shorter bar on the right at its end already.
    let shining = skeleton_colors(Some(Shine::after(12)));
    let next = Some(Color::Rgb(145, 145, 145));
    assert_eq!(shining[7], Some(Color::Rgb(166, 166, 166)));
    assert_eq!(shining[8], next);
    assert_eq!(shining[..3], still[..3]);
    assert_eq!(shining[23 + 6], next);
    assert_eq!(shining[23 + 2], faint);
}

#[test]
fn a_skeleton_fills_its_area_with_rows_of_bars_of_their_own_lengths() {
    let theme = Theme::terminal();
    let mut terminal = Terminal::new(TestBackend::new(24, 5)).unwrap();
    terminal
        .draw(|frame| draw_skeleton(frame, frame.area(), &theme, None))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let lines: Vec<String> = (0..5)
        .map(|y| (0..24).map(|x| buffer[(x, y)].symbol()).collect())
        .collect();
    assert_eq!(
        lines,
        [
            "------------      ------",
            "--------          ------",
            "--------------      ----",
            "----------        ------",
            "------------      ------",
        ]
    );
}
