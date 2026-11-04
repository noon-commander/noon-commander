use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;

use super::*;
use crate::tui::glyphs::Glyphs;
use crate::tui::spinner;

fn draw_at(view: &JobView, now: Instant) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 11)).unwrap();
    terminal
        .draw(|frame| {
            let spin = Spin::new(now, now, false, Glyphs::PLAIN.spinners);
            view.render(frame, frame.area(), &Theme::terminal(), (now, &spin));
        })
        .unwrap();
    terminal.backend().to_string()
}

/// The title line of the window drawn at `now` with turning spinners, the gauge line, and
/// which cells of the gauge are dim.
fn busy_lines(view: &JobView, now: Instant) -> (String, String, Vec<bool>) {
    let mut terminal = Terminal::new(TestBackend::new(80, 11)).unwrap();
    terminal
        .draw(|frame| {
            let spin = Spin::new(now, now, true, Glyphs::PLAIN.spinners);
            view.render(frame, frame.area(), &Theme::terminal(), (now, &spin));
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let line = |y: u16| -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .trim()
            .to_owned()
    };
    // The gauge, inside the frame and its margin.
    let dim = (4..76)
        .map(|x| buffer[(x, 4)].modifier.contains(Modifier::DIM))
        .collect();
    (line(1), line(4), dim)
}

#[test]
fn until_how_far_it_is_is_known_the_gauge_is_a_skeleton_that_a_shine_crosses() {
    let start = Instant::now();
    let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
    view.scanning(42, start);
    let (title, gauge, dim) = busy_lines(&view, start);
    assert!(title.contains("═ Delete ═"), "{title}");
    assert!(gauge.contains("▒▒▒▒▒▒") && !gauge.contains('%'), "{gauge}");
    assert!(dim.iter().all(|dim| *dim), "{dim:?}");
    // The shine starts with the delay of every spinner, and no dots turn in the title.
    let (title, _, dim) = busy_lines(&view, start + spinner::DELAY + secs(1) / 2);
    assert!(title.contains("═ Delete ═"), "{title}");
    assert_eq!(dim.iter().filter(|dim| !**dim).count(), 9, "{dim:?}");
    // It turns at the end of the bar of 72 cells and runs back: eight cells after the last.
    let back = start + spinner::DELAY + Duration::from_millis(40) * 79;
    let (_, _, dim) = busy_lines(&view, back);
    let lit: Vec<usize> = (0..dim.len()).filter(|x| !dim[*x]).collect();
    assert_eq!(lit, (59..=67).collect::<Vec<_>>());
    // A stop leaves it still.
    view.abort();
    let (_, _, dim) = busy_lines(&view, start + spinner::DELAY + secs(1) / 2);
    assert!(dim.iter().all(|dim| *dim), "{dim:?}");
    // Once it is known, the gauge fills, with its percentage.
    view.working("/srv/a".to_owned(), Counts::default(), start);
    let (_, gauge, _) = busy_lines(&view, start + secs(1));
    assert!(gauge.contains('%'), "{gauge}");
}

fn draw(view: &JobView) -> String {
    draw_at(view, Instant::now())
}

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// A copy of 100 MiB, with `done` and `copied` MiB of it.
fn copying(done: u64, copied: u64) -> Counts {
    const MIB: u64 = 1024 * 1024;
    Counts {
        done: 1,
        total: 2,
        bytes_done: done * MIB,
        bytes_total: 100 * MIB,
        bytes_copied: copied * MIB,
    }
}

#[test]
fn shows_counting_then_progress() {
    let start = Instant::now();
    let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
    view.scanning(42, start);
    let text = draw_at(&view, start);
    assert!(text.contains("Counting"), "{text}");
    assert!(text.contains("42 found"), "{text}");
    assert!(!text.contains("Time"), "{text}");

    let counts = Counts {
        done: 3,
        total: 12,
        ..Counts::default()
    };
    view.working("/srv/www/index.html".to_owned(), counts, start);
    insta::assert_snapshot!(draw_at(&view, start + secs(75)));

    // With bytes, the gauge follows them.
    let counts = Counts {
        done: 1,
        total: 2,
        bytes_done: 1536,
        bytes_total: 2048,
        bytes_copied: 1536,
    };
    view.working("big".to_owned(), counts, start);
    let text = draw(&view);
    assert!(text.contains("1 of 2, 1.5K of 2.0K"), "{text}");
    assert!(text.contains(" 75%"), "{text}");

    view.abort();
    assert!(draw(&view).contains("Aborting"));

    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    view.wait();
    assert!(draw(&view).contains("Waiting for other jobs"));
    assert!(view.ratio() < f64::EPSILON);
    assert!(!view.ticking(), "nothing to time yet");
}

#[test]
fn a_copy_shows_its_time_speed_and_time_left() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    view.working("big".to_owned(), copying(0, 0), start);
    let text = draw_at(&view, start + Duration::from_millis(400));
    assert!(text.contains("Time 0:00   ETA -:--   -"), "{text}");

    for second in 1..=10 {
        let counts = copying(2 * second, 2 * second);
        view.working("big".to_owned(), counts, start + secs(second));
    }
    insta::assert_snapshot!(draw_at(&view, start + secs(10)));
    let summary = view.summary(start + secs(10));
    assert_eq!(summary.left, "ETA 0:40");
}

#[test]
fn time_waiting_for_an_answer_does_not_count() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    for second in 0..=5 {
        let counts = copying(second, second);
        view.working("a".to_owned(), counts, start + secs(second));
    }
    // A question after 5 seconds stays open for an hour.
    view.ask(start + secs(5));
    assert!(!view.ticking());
    // Reports that were on their way change nothing.
    view.working("a".to_owned(), copying(5, 5), start + secs(6));
    assert!(!view.ticking());
    let open = start + secs(3605);
    let timing = view.timing(open).unwrap();
    assert_eq!(timing.elapsed, secs(5));
    view.answered(open);
    assert!(view.ticking());

    for second in 6..=10 {
        let counts = copying(second, second);
        view.working("a".to_owned(), counts, open + secs(second - 5));
    }
    let timing = view.timing(open + secs(5)).unwrap();
    assert_eq!(timing.elapsed, secs(10));
    let speed = timing.speed.unwrap();
    assert!((speed - 1024.0 * 1024.0).abs() < 1.0, "1 MiB/s: {speed}");
    assert_eq!(timing.left.map(whole_seconds), Some(90));
    let text = draw_at(&view, open + secs(5));
    assert!(text.contains("Time 0:10   ETA 1:30   1.0M/s"), "{text}");
}

#[test]
fn skipped_and_retried_bytes_do_not_bend_the_speed() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    // 1 MiB a second for 10 seconds, then 15 MiB skipped at once, then a retry that takes
    // 12 MiB back to the start of a file: the speed follows the bytes that moved.
    for second in 0..=10 {
        view.working(
            "a".to_owned(),
            copying(second, second),
            start + secs(second),
        );
    }
    view.working("b".to_owned(), copying(25, 10), start + secs(10));
    let timing = view.timing(start + secs(10)).unwrap();
    assert!((timing.speed.unwrap() - 1024.0 * 1024.0).abs() < 1.0);
    assert_eq!(timing.left.map(whole_seconds), Some(75));
    view.working("c".to_owned(), copying(14, 11), start + secs(11));
    let timing = view.timing(start + secs(11)).unwrap();
    assert!((timing.speed.unwrap() - 1024.0 * 1024.0).abs() < 1.0);
    let forecast = view.forecast.left(secs(11), &copying(14, 11));
    assert_eq!(forecast.map(whole_seconds), Some(86));
    // The time left shown moves there from the 74 seconds counted down, halfway at once.
    assert_eq!(timing.left.map(whole_seconds), Some(80));
}

#[test]
fn the_speed_and_the_time_left_change_with_the_second_shown() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    for second in 0..=10 {
        view.working(
            "a".to_owned(),
            copying(second, second),
            start + secs(second),
        );
    }
    let shown = view.timing(start + secs(10)).unwrap();
    // A burst of 3 MiB within the second that shows changes neither.
    let burst = start + Duration::from_millis(10_300);
    view.working("a".to_owned(), copying(13, 13), burst);
    let timing = view.timing(start + Duration::from_millis(10_400)).unwrap();
    assert_eq!((timing.speed, timing.left), (shown.speed, shown.left));
    assert_eq!(timing.elapsed, Duration::from_millis(10_400));
    // The time shows 0:11 from 10.5 seconds on, and they follow.
    let timing = view.timing(start + Duration::from_millis(10_500)).unwrap();
    let speed = timing.speed.unwrap();
    assert!(speed > 1.4 * 1024.0 * 1024.0, "{speed}");
    assert_ne!(timing.left, shown.left);
}

#[test]
fn a_batch_of_reports_counts_as_its_reports_one_by_one() {
    const MIB: u64 = 1024 * 1024;
    let start = Instant::now();
    // Twenty files of 3 MiB, each opened in a tenth of a second, then six chunks of half a
    // mebibyte, one each twentieth.
    let mut reports = Vec::new();
    let mut at = start;
    for file in 0..20 {
        for chunk in 0..=6 {
            let bytes = file * 3 * MIB + chunk * MIB / 2;
            let counts = Counts {
                done: file,
                total: 20,
                bytes_done: bytes,
                bytes_total: 60 * MIB,
                bytes_copied: bytes,
            };
            reports.push((at, counts));
            at += Duration::from_millis(if chunk == 0 { 100 } else { 50 });
        }
    }
    let mut one_by_one = JobView::new("Copy".to_owned(), "Copying".to_owned());
    let mut batched = JobView::new("Copy".to_owned(), "Copying".to_owned());
    for batch in reports.chunks(5) {
        for &(at, counts) in batch {
            one_by_one.working("a".to_owned(), counts, at);
        }
        let (&(at, counts), before) = batch.split_last().unwrap();
        for &(at, counts) in before {
            batched.passed(counts, at);
        }
        batched.working("a".to_owned(), counts, at);
        let now = at + Duration::from_millis(5);
        assert_eq!(batched.look(now), one_by_one.look(now));
    }
    let end = at + Duration::from_millis(5);
    assert!(
        batched.timing(end).unwrap().left.is_some(),
        "it shows a time left"
    );
}

#[test]
fn reports_taken_late_count_when_they_were_made_and_show_at_once() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    for second in 0..=10 {
        let counts = copying(second, second);
        view.working("a".to_owned(), counts, start + secs(second));
    }
    // The window is drawn 30 seconds later, as the terminal comes back from another
    // program, before the reports the job made meanwhile: it looks still.
    let back = start + secs(40);
    let timing = view.timing(back).unwrap();
    assert_eq!((timing.speed, timing.left), (Some(0.0), None));
    for second in 11..=40 {
        let counts = copying(second, second);
        view.working("a".to_owned(), counts, start + secs(second));
    }
    // They count at the times the job made them, and show within the same second: the
    // pace of a mebibyte a second, no stall, 60 MiB left.
    let timing = view.timing(back).unwrap();
    let speed = timing.speed.unwrap();
    assert!((speed - 1024.0 * 1024.0).abs() < 1.0, "1 MiB/s: {speed}");
    assert_eq!(timing.left.map(whole_seconds), Some(60));
    // A report that comes in time leaves the second as it is.
    view.working(
        "a".to_owned(),
        copying(41, 41),
        back + Duration::from_millis(100),
    );
    let again = view.timing(back + Duration::from_millis(200)).unwrap();
    assert_eq!((again.speed, again.left), (timing.speed, timing.left));
}

#[test]
fn a_report_taken_late_leaves_the_count_down_where_it_was() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    for second in 0..=20 {
        let now = start + secs(second);
        view.working("a".to_owned(), copying(second, second), now);
        view.timing(now);
    }
    let shown = view
        .timing(start + secs(21))
        .unwrap()
        .left
        .map(whole_seconds);
    assert_eq!(shown, Some(79));
    // A report made 0.7 seconds after the last, with nothing more done, is taken after the
    // second was worked out: the forecast grows by a twentieth, and the time left worked out
    // again keeps counting down, rather than show the forecast as it is.
    let late = start + Duration::from_millis(20_700);
    view.working("a".to_owned(), copying(20, 20), late);
    let forecast = view.forecast.left(secs(21), &copying(20, 20));
    assert!(forecast.is_some_and(|left| left > secs(83)), "{forecast:?}");
    let timing = view.timing(start + secs(21)).unwrap();
    assert_eq!(timing.left.map(whole_seconds), shown);
}

#[test]
fn the_look_changes_only_with_what_the_window_shows() {
    const MIB: u64 = 1024 * 1024;
    /// Reports `done` entries and `bytes` of 100 MiB at `now`, and looks.
    fn report(view: &mut JobView, (done, bytes): (u64, u64), now: Instant) -> Look {
        let counts = Counts {
            done,
            total: 2,
            bytes_done: bytes,
            bytes_total: 100 * MIB,
            bytes_copied: bytes,
        };
        view.working("a".to_owned(), counts, now);
        view.look(now)
    }
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    let shown = report(&mut view, (0, 20 * MIB), at(0));
    let look = report(&mut view, (0, 20 * MIB + 1_000), at(100));
    assert_eq!(look, shown, "still 20.0M, 20%, 0:00");
    let look = report(&mut view, (0, 20 * MIB + 200 * 1024), at(200));
    assert_ne!(look, shown, "20.2M");
    let shown = look;
    assert_ne!(
        report(&mut view, (1, 20 * MIB + 200 * 1024), at(300)),
        shown,
        "1 of 2"
    );
    let shown = view.look(at(300));
    assert_eq!(view.look(at(499)), shown);
    assert_ne!(view.look(at(500)), shown, "0:01");
}

#[test]
fn the_time_draws_again_when_the_second_it_shows_changes() {
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    assert_eq!(view.next_tick(start), None, "nothing to time yet");
    view.working("a".to_owned(), copying(0, 0), start);
    assert_eq!(view.next_tick(at(200)), Some(at(500)));
    assert_eq!(view.next_tick(at(500)), Some(at(1_500)));
    assert_eq!(view.next_tick(at(1_499)), Some(at(1_500)));
    view.ask(at(2_000));
    assert_eq!(view.next_tick(at(9_000)), None, "it stands");
}

#[test]
fn the_time_left_counts_down_by_the_clock_near_the_forecast() {
    let before = |then, shown, smooth| {
        let left = Left {
            shown: secs(shown),
            smooth: secs(smooth),
        };
        Some((then, left))
    };
    let left = |before, second, forecast_millis| {
        let left = count_down(before, second, Duration::from_millis(forecast_millis));
        (whole_seconds(left.shown), whole_seconds(left.smooth))
    };
    // The first is the forecast, in whole seconds.
    assert_eq!(left(None, 10, 99_600), (100, 100));
    // On time, a second less each second.
    assert_eq!(left(before(10, 100, 100), 11, 99_000), (99, 99));
    assert_eq!(left(before(10, 100, 100), 13, 97_000), (97, 97));
    // The forecast a few seconds off for a second: it moves the smoothed one by a tenth of
    // the way, and the time left shown, within 1% of that, does not follow even where the
    // smoothed one shows another second.
    assert_eq!(left(before(10, 100, 100), 11, 103_000), (99, 99));
    assert_eq!(left(before(10, 100, 100), 11, 95_000), (99, 99));
    assert_eq!(left(before(10, 100, 100), 11, 109_500), (99, 100));
    // 1% of the smoothed one, not of the forecast: 109.9 takes it to 100.10, whose band the
    // count of 99 is below, so it shows the edge, 99.10.
    let edge = count_down(before(10, 100, 100), 11, Duration::from_millis(109_900));
    let (shown, smooth) = (edge.shown.as_secs_f64(), edge.smooth.as_secs_f64());
    assert!((shown - smooth * 0.99).abs() < 1e-9, "{edge:?}");
    assert!((smooth - 100.1).abs() < 0.01, "{edge:?}");
    // Three seconds at once, as for a window not drawn: three steps of the smoothing take
    // it to 99.54, whose band the count of 97 is below, so it shows the edge, 98.55.
    assert_eq!(left(before(10, 100, 100), 13, 106_000), (99, 100));
    // The forecast six seconds longer, and on time from then on: the time left shown comes
    // to it, in step with the smoothed forecast, and is on time a minute later.
    let mut last = before(11, 99, 99);
    for second in 12..=71 {
        let next = count_down(last, second, secs(105 - (second - 11)));
        last = Some((second, next));
    }
    assert_eq!(left(last, 72, 44_000), (44, 44));
    // Near the end the smoothing takes a second at least, which leaves it the forecast.
    let near = count_down(before(10, 12, 12), 11, Duration::from_millis(10_500));
    assert_eq!(near.smooth, Duration::from_millis(10_500));
    // Beyond a tenth, halfway there each second.
    assert_eq!(left(before(10, 100, 100), 11, 110_100), (105, 105));
    assert_eq!(left(before(10, 100, 100), 11, 151_000), (125, 125));
    assert_eq!(left(before(10, 100, 100), 11, 52_000), (76, 76));
    assert_eq!(left(before(10, 100, 100), 12, 178_000), (158, 158));
    // Up to twice as long as the forecast, or as short, halfway; beyond, the forecast.
    assert_eq!(left(before(10, 100, 100), 11, 49_600), (74, 74));
    assert_eq!(left(before(10, 100, 100), 11, 49_400), (49, 49));
    assert_eq!(left(before(10, 100, 100), 11, 198_000), (149, 149));
    assert_eq!(left(before(10, 100, 100), 11, 198_100), (198, 198));
    assert_eq!(left(before(10, 100, 100), 11, 39_000), (39, 39));
    assert_eq!(left(before(10, 36_000, 36_000), 11, 60_000), (60, 60));
    // Counted down to nothing, the forecast; to a little more, the forecast too.
    assert_eq!(left(before(10, 2, 2), 12, 30_000), (30, 30));
    assert_eq!(left(before(10, 0, 0), 11, 30_000), (30, 30));
    let little = Left {
        shown: Duration::from_millis(1_300),
        smooth: Duration::from_millis(1_300),
    };
    assert_eq!(left(Some((10, little)), 11, 30_000), (30, 30));
    // Nothing overflows.
    let most = Left {
        shown: Duration::MAX,
        smooth: Duration::MAX,
    };
    let far = count_down(Some((0, most)), 1, Duration::MAX);
    assert!(far.shown > secs(u64::MAX / 2));
}

#[test]
fn a_time_left_shown_too_long_comes_down_to_the_forecast() {
    // 10:00 shown, where 9:15 are left and the forecast says so from then on.
    let mut left = Left {
        shown: secs(600),
        smooth: secs(600),
    };
    let mut steps = Vec::new();
    for second in 11..=100 {
        let forecast = secs(555 - (second - 10));
        let next = count_down(Some((second - 1, left)), second, forecast);
        steps.push(whole_seconds(left.shown) - whole_seconds(next.shown));
        left = next;
        // Within a minute it comes within 3% of the forecast.
        if second >= 70 {
            let off = left.shown.abs_diff(forecast).as_secs_f64();
            assert!(off <= forecast.as_secs_f64() * 0.03, "{second}: {left:?}");
        }
    }
    // Down all the way: by the clock at first, while the forecast smoothed comes down,
    // then two seconds at a time or so, fewer as it comes near.
    assert!(steps[..3].iter().all(|step| *step == 1), "{steps:?}");
    assert!(steps.iter().all(|step| (1..=3).contains(step)), "{steps:?}");
    let quick = steps[60..].iter().filter(|step| **step != 1).count();
    assert!(quick <= 10, "{steps:?}");
}

#[test]
fn a_time_left_holds_still_where_the_forecast_does() {
    // The forecast stays put, as when the pace falls as fast as the work goes: the time left
    // shown never goes up, and comes to rest at the lower edge of 1% around the forecast
    // smoothed, which keeps behind it by the time it takes to follow, less a second. Short
    // of the tenth from the forecast that would move it halfway up, and again and again.
    for (forecast, rest) in [(100, 91), (250, 226), (600, 565)] {
        let mut left = Left {
            shown: secs(forecast),
            smooth: secs(forecast),
        };
        let mut seen = Vec::new();
        for second in 1..=300 {
            left = count_down(Some((second - 1, left)), second, secs(forecast));
            seen.push(whole_seconds(left.shown));
        }
        assert!(seen.windows(2).all(|pair| pair[1] <= pair[0]), "{seen:?}");
        assert!(seen[240..].iter().all(|left| *left == rest), "{seen:?}");
    }
}

#[test]
fn a_slow_start_is_soon_behind_the_speed() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    // 1 MiB a second for 10 seconds, then 5 MiB a second, reported every second.
    let copied = |second: u64| {
        if second <= 10 {
            second
        } else {
            10 + (second - 10) * 5
        }
    };
    for second in 0..=20 {
        let counts = copying(copied(second), copied(second));
        view.working("a".to_owned(), counts, start + secs(second));
    }
    let timing = view.timing(start + secs(20)).unwrap();
    let speed = timing.speed.unwrap();
    assert!(
        (speed - 5.0 * 1024.0 * 1024.0).abs() < 1.0,
        "5 MiB/s: {speed}"
    );
    // The time left looks further back: between the 8 seconds of the last pace and the 13
    // of the average since the start.
    let left = timing.left.unwrap().as_secs();
    assert!((9..13).contains(&left), "{left}");
}

#[test]
fn while_no_data_moves_the_speed_falls_and_the_time_left_is_unknown() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    for second in 0..=10 {
        view.working(
            "a".to_owned(),
            copying(second, second),
            start + secs(second),
        );
    }
    let before = view.timing(start + secs(10)).unwrap().left.unwrap();
    assert_eq!(before.as_secs(), 90);
    // Then nothing comes: the seconds that moved data leave the speed's stretch one by one,
    // while the time left keeps to the pace behind it and counts down.
    let timing = view.timing(start + secs(12)).unwrap();
    let speed = timing.speed.unwrap();
    assert!(
        (speed - 0.6 * 1024.0 * 1024.0).abs() < 1.0,
        "0.6 MiB/s: {speed}"
    );
    assert_eq!(timing.left.map(whole_seconds), Some(88));
    // Five seconds without a report: the speed is nothing, and the time left unknown.
    let timing = view.timing(start + secs(15)).unwrap();
    assert!(timing.speed.unwrap() < f64::EPSILON);
    assert_eq!(timing.left, None);
    let text = draw_at(&view, start + secs(15));
    assert!(text.contains("Time 0:15   ETA -:--   0B/s"), "{text}");
    assert_eq!(view.summary(start + secs(15)).left, "");
}

const MIB: u64 = 1024 * 1024;

/// The chunks of an SFTP channel, after each of which a job reports.
const CHUNK: u64 = 32 * 1024;

/// A copy at work, reported as a job reports: after each chunk, a quarter of a second of
/// data at most, and after each entry.
struct Run {
    view: JobView,
    start: Instant,
    /// Seconds worked.
    at: f64,
    counts: Counts,
}

impl Run {
    fn new(total: u64, bytes_total: u64) -> Self {
        let mut run = Self {
            view: JobView::new("Copy".to_owned(), "Copying".to_owned()),
            start: Instant::now(),
            at: 0.0,
            counts: Counts {
                total,
                bytes_total,
                ..Counts::default()
            },
        };
        run.report();
        run
    }

    fn now(&self) -> Instant {
        self.start + Duration::from_secs_f64(self.at)
    }

    fn report(&mut self) {
        let now = self.now();
        self.view.working("f".to_owned(), self.counts, now);
    }

    /// Moves `bytes` at `speed` bytes a second, within an entry.
    fn stream(&mut self, bytes: u64, speed: u64) {
        let mut left = bytes;
        while left > 0 {
            let chunk = left.min(speed / 4);
            self.at += float(chunk) / float(speed);
            self.counts.bytes_done += chunk;
            self.counts.bytes_copied += chunk;
            left -= chunk;
            self.report();
        }
    }

    /// Copies a file of `size` bytes at `speed` bytes a second, after `opening` seconds.
    fn copy(&mut self, size: u64, speed: u64, opening: f64) {
        self.at += opening;
        self.stream(size, speed);
        self.counts.done += 1;
        self.report();
    }

    /// A file of `size` bytes as a job copies it: a report as it starts, with those before
    /// it done; `open` seconds; its data at `speed` bytes a second, with a report after each
    /// chunk; then `close` seconds, setting attributes and the like, with no report, the
    /// next start telling it done.
    fn file(&mut self, size: u64, speed: u64, (open, close): (f64, f64)) {
        self.report();
        self.at += open;
        let mut left = size;
        while left > 0 {
            let chunk = left.min(CHUNK);
            self.at += float(chunk) / float(speed);
            self.counts.bytes_done += chunk;
            self.counts.bytes_copied += chunk;
            left -= chunk;
            self.report();
        }
        self.at += close;
        self.counts.done += 1;
    }

    /// The same file through a writer that counts its chunks as it queues them: they go into
    /// a queue of 2 MiB at once, and wait for room as the target takes those before; the job
    /// waits for the last of them before it closes. Its reports run ahead of the data.
    fn queued(&mut self, size: u64, speed: u64, (open, close): (f64, f64)) {
        const QUEUE: u64 = 2 * MIB;
        self.report();
        self.at += open;
        let start = self.at;
        let mut queued = 0;
        while queued < size {
            let chunk = CHUNK.min(size - queued);
            queued += chunk;
            let sent = queued.saturating_sub(QUEUE);
            self.at = self.at.max(start + float(sent) / float(speed));
            self.counts.bytes_done += chunk;
            self.counts.bytes_copied += chunk;
            self.report();
        }
        self.at = start + float(size) / float(speed) + close;
        self.counts.done += 1;
    }

    /// Skips a file of `size` bytes that is there already, finding it out in `seconds`, and
    /// reports, as a job does before each entry.
    fn skip(&mut self, size: u64, seconds: f64) {
        self.at += seconds;
        self.counts.done += 1;
        self.counts.bytes_done += size;
        self.report();
    }

    /// The time left in seconds, as the forecast has it, before it is rounded.
    fn left(&self) -> f64 {
        self.left_at(self.at).unwrap()
    }

    /// The time left in seconds when the job has worked `at` seconds, if it is known.
    fn left_at(&self, at: f64) -> Option<f64> {
        let at = self
            .view
            .clock
            .elapsed(self.start + Duration::from_secs_f64(at));
        let left = self.view.forecast.left(at, &self.counts)?;
        Some(left.as_secs_f64())
    }
}

/// Numbers that look random, the same each run.
struct Noise(u64);

impl Noise {
    /// 53 bits.
    fn bits(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    /// From 0 to 1.
    fn next(&mut self) -> f64 {
        float(self.bits()) / float(1 << 53)
    }

    /// From 0 up to `limit`.
    fn below(&mut self, limit: u64) -> u64 {
        self.bits() % limit
    }

    /// `value`, give or take a share of it as large as `spread`.
    fn around(&mut self, value: f64, spread: f64) -> f64 {
        value * (1.0 + spread * (2.0 * self.next() - 1.0))
    }
}

#[test]
fn what_bytes_and_entries_cost() {
    let point = |millis: u64, entries: u64, bytes: u64| Point {
        at: Duration::from_millis(millis),
        entries,
        done: bytes,
        copied: bytes,
        starts: false,
    };
    // 10 MiB streaming in a second within an entry, then three entries without data, a
    // tenth of a second each.
    let mut sums = Sums::default();
    sums.add(point(0, 0, 0), point(1_000, 0, 10 * MIB));
    for n in 0..3 {
        let (from, to) = (1_000 + 100 * n, 1_100 + 100 * n);
        sums.fit(point(from, n, 10 * MIB), point(to, n + 1, 10 * MIB));
    }
    let (byte, entry) = sums.costs();
    assert!((byte * float(10 * MIB) - 1.0).abs() < 1e-9, "{byte}");
    assert!((entry - 0.1).abs() < 1e-9, "{entry}");

    // With no data streaming, files of one size tell bytes and entries nothing apart.
    let mut sums = Sums::default();
    for n in 0..10 {
        sums.fit(
            point(100 * n, n, 1024 * n),
            point(100 * (n + 1), n + 1, 1024 * (n + 1)),
        );
    }
    let (byte, entry) = sums.costs();
    assert!((byte - 1.0).abs() < f64::EPSILON && entry.abs() < f64::EPSILON);

    // Files of up to 30 KiB, each opened in a tenth of a second give or take a third: the
    // entries clearly cost their tenth, the bytes nothing that shows.
    let mut noise = Noise(7);
    let mut sums = Sums::default();
    let (mut at, mut bytes) = (0, 0);
    for n in 0..500 {
        let size = noise.below(30 * 1024);
        let took = Duration::from_secs_f64(noise.around(0.1, 0.3));
        let to = Point {
            at: Duration::from_millis(at) + took,
            ..point(0, n + 1, bytes + size)
        };
        sums.fit(point(at, n, bytes), to);
        at = u64::try_from(to.at.as_millis()).unwrap();
        bytes += size;
    }
    let (byte, entry) = sums.costs();
    assert!(byte >= 0.0 && byte * float(30 * 1024) < 0.01, "{byte}");
    assert!((entry - 0.1).abs() < 0.01, "{entry}");

    // Whole entries that tell both costs clearly come before data streaming that says
    // otherwise, as through a writer that counts what it queues, 2 MiB at once: streaming
    // says 40 MiB a second, entries of 1 and 3 MiB by turns, a tenth of a second each beside
    // their bytes, 10.
    let mut sums = Sums::default();
    sums.add(point(0, 0, 0), point(1_000, 0, 40 * MIB));
    let (mut at, mut bytes) = (1_000, 40 * MIB);
    for n in 0..6 {
        let size = if n % 2 == 0 { MIB } else { 3 * MIB };
        let took = 100 + size * 1_000 / (10 * MIB);
        sums.fit(point(at, n, bytes), point(at + took, n + 1, bytes + size));
        (at, bytes) = (at + took, bytes + size);
    }
    let (byte, entry) = sums.costs();
    assert!((byte * float(MIB) - 0.1).abs() < 1e-9, "{byte}");
    assert!((entry - 0.1).abs() < 1e-9, "{entry}");

    // Four entries of 1 to 4 MiB at 0.1 s a MiB and a quarter of a second each, give or
    // take some: two costs leave the noise two rows of the four, and so neither stands
    // clear, so streaming tells the cost of a byte.
    let mut sums = Sums::default();
    sums.add(point(0, 0, 0), point(1_000, 0, 10 * MIB));
    let (mut at, mut bytes) = (1_000, 10 * MIB);
    for (n, took) in (0..).zip([400, 330, 650, 620]) {
        let size = (n + 1) * MIB;
        sums.fit(point(at, n, bytes), point(at + took, n + 1, bytes + size));
        (at, bytes) = (at + took, bytes + size);
    }
    let (byte, entry) = sums.costs();
    assert!(
        (byte * float(MIB) - 0.1).abs() < 1e-9,
        "streaming's: {byte}"
    );
    assert!((entry - 0.25).abs() < 1e-9, "{entry}");

    // Small files that took a little less the bigger they were: a byte costs nothing,
    // not less.
    let mut sums = Sums::default();
    let (mut at, mut bytes) = (0, 0);
    for (n, took) in (0..).zip([99, 98, 97, 96, 95, 94]) {
        let size = (n + 1) * 10 * 1024;
        sums.fit(point(at, n, bytes), point(at + took, n + 1, bytes + size));
        (at, bytes) = (at + took, bytes + size);
    }
    let (byte, entry) = sums.costs();
    assert!(byte.abs() < f64::EPSILON, "{byte}");
    assert!((entry - 0.1).abs() < 0.01, "{entry}");

    // Streaming at a second a MiB, and files of 1 MiB that took half a second each: an
    // entry costs nothing, not less.
    let mut sums = Sums::default();
    sums.add(point(0, 0, 0), point(1_000, 0, MIB));
    for n in 0..4 {
        let (from, to) = (1_000 + 500 * n, 1_500 + 500 * n);
        sums.fit(
            point(from, n, (n + 1) * MIB),
            point(to, n + 1, (n + 2) * MIB),
        );
    }
    let (byte, entry) = sums.costs();
    assert!((byte * float(MIB) - 1.0).abs() < 1e-9, "{byte}");
    assert!(entry.abs() < f64::EPSILON, "{entry}");
}

#[test]
fn entries_without_data_tell_nothing_of_the_data_left() {
    // A hundred directories made in ten seconds, with 100 MiB of files still to come:
    // entries alone would make the data look as if it took no time.
    let mut run = Run::new(200, 100 * MIB);
    for _ in 0..100 {
        run.at += 0.1;
        run.counts.done += 1;
        run.report();
    }
    assert_eq!(run.left_at(run.at), None);
    // With no data at all, entries alone tell: a tenth of a second each.
    let mut run = Run::new(200, 0);
    for _ in 0..100 {
        run.at += 0.1;
        run.counts.done += 1;
        run.report();
    }
    let left = run.left_at(run.at).unwrap();
    assert!((left - 10.0).abs() < 0.5, "{left}");
}

#[test]
fn a_pause_over_a_minute_ago_no_longer_stretches_a_stall() {
    // A pause of 8 seconds, then 50 seconds of work, then nothing.
    let mut run = Run::new(1, 4_000 * MIB);
    run.stream(100 * MIB, 10 * MIB);
    run.at += 8.0;
    run.stream(500 * MIB, 10 * MIB);
    let seen = run.at;
    // Within a minute of it, a pause of up to 16 seconds is work …
    assert!(run.left_at(seen + 9.0).is_some());
    // … and past it, one of over five is a stall again.
    assert_eq!(run.left_at(seen + 15.0), None);
}

#[test]
fn files_up_and_down_keep_to_the_time_left() {
    // 120 files at 2 MiB a second, each opened in 90 ms and closed in 120, then 2,000 files
    // of 2 KiB. Files of one size tell bytes and entries apart only by the data streaming
    // within them, which must not hold the opening of the next; through a writer that counts
    // what it queues, the time of sending a file's last 2 MiB falls after its last report.
    for (queued, one_size) in [(false, true), (false, false), (true, false)] {
        let mut noise = Noise(3);
        let sizes: Vec<u64> = (0..120)
            .map(|_| {
                if one_size {
                    3 * MIB
                } else {
                    MIB / 2 + noise.below(3 * MIB)
                }
            })
            .collect();
        let bytes = sizes.iter().sum::<u64>() + 2_000 * 2_048;
        let mut run = Run::new(2_120, bytes);
        let copy = |run: &mut Run, size| {
            if queued {
                run.queued(size, 2 * MIB, (0.09, 0.12));
            } else {
                run.file(size, 2 * MIB, (0.09, 0.12));
            }
        };
        let mut seen = Vec::new();
        for (n, size) in sizes.into_iter().enumerate() {
            copy(&mut run, size);
            if n % 10 == 9 && run.at > 30.0 {
                seen.push((run.at, run.left()));
            }
        }
        seen.push((run.at, run.left()));
        for _ in 0..2_000 {
            copy(&mut run, 2_048);
        }
        for (at, left) in seen {
            let truth = run.at - at;
            let off = (left - truth).abs();
            assert!(
                off < truth * 0.05,
                "queued {queued}, one size {one_size}: {left} for {truth} at {at}"
            );
        }
    }
}

#[test]
fn forecasts_agree_once_they_keep_within_a_quarter_of_each_other() {
    let taken = |forecasts: &[(u64, u64)]| -> Vec<Option<(Duration, Duration)>> {
        forecasts
            .iter()
            .map(|&(at, left)| Some((secs(at), secs(left))))
            .collect()
    };
    // A forecast that is right says the same end each second.
    assert!(agree(&taken(&[(0, 100), (1, 99), (2, 98)]), secs(2)));
    // Each less the time since: 100, 112, and 125, at most a quarter longer, or 126.
    assert!(agree(&taken(&[(0, 102), (1, 113), (2, 125)]), secs(2)));
    assert!(!agree(&taken(&[(0, 102), (1, 113), (2, 126)]), secs(2)));
    assert!(agree(&taken(&[(0, 127), (1, 113), (2, 100)]), secs(2)));
    assert!(!agree(&taken(&[(0, 128), (1, 113), (2, 100)]), secs(2)));
    // Two seconds left two seconds ago are none now.
    assert!(!agree(&taken(&[(0, 2), (1, 2), (2, 2)]), secs(2)));
    // Fewer than all.
    let mut few = taken(&[(0, 100), (1, 99), (2, 98)]);
    few[0] = None;
    assert!(!agree(&few, secs(2)));
}

#[test]
fn the_time_left_shows_once_its_forecasts_agree() {
    let mut run = Run::new(1, 1000 * MIB);
    // A fast first second, then an eighth of its pace, which the forecast takes on as the
    // first second fades: it grows by more than a quarter a second, then less.
    run.stream(16 * MIB, 16 * MIB);
    let mut first = None;
    for second in 2..=15 {
        run.stream(2 * MIB, 2 * MIB);
        let timing = run.view.timing(run.now()).unwrap();
        let forecast = run.left();
        match (first, timing.left) {
            (None, None) => {}
            (None, Some(left)) => {
                // The forecast itself, with none shown before.
                assert!((left.as_secs_f64() - forecast).abs() < 1e-6, "{second}");
                first = Some(second);
            }
            (Some(_), left) => assert!(left.is_some(), "it stays, at {second}"),
        }
    }
    let first = first.expect("shown");
    assert!((5..=10).contains(&first), "shown at {first}");
}

#[test]
fn the_time_left_shows_after_half_a_minute_whether_or_not_its_forecasts_agree() {
    let mut run = Run::new(1, 1000 * MIB);
    for second in 1..=31 {
        run.stream(MIB, MIB);
        // Forecasts that never agree.
        run.view.forecast.settled = false;
        run.view.forecast.settling = Default::default();
        let left = run.view.timing(run.now()).unwrap().left;
        assert_eq!(left.is_some(), second >= 30, "{second}");
    }
}

#[test]
fn a_cost_unheard_of_for_hours_is_forgotten() {
    let point = |seconds: f64, entries: u64, bytes: u64| Point {
        at: Duration::from_secs_f64(seconds),
        entries,
        done: bytes,
        copied: bytes,
        starts: false,
    };
    // A hundred files of 64 KiB, a tenth of a second each, then nine hours of one file at
    // 10 MiB a second: the cost of an entry fades out of the sums.
    let mut sums = Sums::default();
    let (mut at, mut entries, mut bytes) = (0.0, 0, 0);
    for _ in 0..100 {
        let from = point(at, entries, bytes);
        (at, entries, bytes) = (at + 0.1, entries + 1, bytes + 64 * 1024);
        sums.fit(from, point(at, entries, bytes));
        sums.add(from, point(at, entries, bytes));
    }
    for _ in 0..9 * 3_600 * 4 {
        let from = point(at, entries, bytes);
        (at, bytes) = (at + 0.25, bytes + 10 * MIB / 4);
        sums.add(from, point(at, entries, bytes));
    }
    let (byte, entry) = sums.costs();
    assert!((byte * float(10 * MIB) - 1.0).abs() < 1e-9, "{byte}");
    assert!(
        entry.abs() < f64::EPSILON,
        "forgotten, not stuck at one: {entry}"
    );
    // Nine hours of files of up to 30 KiB then, each a tenth of a second give or take a
    // third: the cost of streaming fades out as well, and the cost of an entry shows.
    let mut noise = Noise(5);
    for _ in 0..9 * 36_000 {
        let from = point(at, entries, bytes);
        at += noise.around(0.1, 0.3);
        (entries, bytes) = (entries + 1, bytes + noise.below(30 * 1_024));
        sums.fit(from, point(at, entries, bytes));
        sums.add(from, point(at, entries, bytes));
    }
    assert!(sums.stream_time.abs() < f64::EPSILON);
    let (byte, entry) = sums.costs();
    assert!((entry - 0.1).abs() < 0.01, "{entry}");
    assert!(
        byte * float(30 * 1_024) < 0.01,
        "not a second a byte: {byte}"
    );
}

#[test]
fn after_a_stall_the_time_left_keeps_the_pace_of_the_work() {
    let mut run = Run::new(1, 4_000 * MIB);
    run.stream(600 * MIB, 10 * MIB);
    let pace = run.left();
    // Nothing comes for a minute and a half: four seconds on, the time left is known …
    let stalled = run.at;
    assert!(run.left_at(stalled + 4.0).is_some());
    // … and from five on it is not.
    for seconds in [5.0, 30.0, 90.0] {
        assert_eq!(run.left_at(stalled + seconds), None);
    }
    run.at += 90.0;
    // The work goes on at the same pace: the stall does not count, the pace is that of the
    // work at once.
    run.stream(10 * MIB, 10 * MIB);
    let even = float(run.counts.bytes_total - run.counts.bytes_done) / float(10 * MIB);
    let left = run.left();
    assert!(
        (left - even).abs() < even * 0.02,
        "{left} for {even}, {pace} before"
    );
}

#[test]
fn counting_after_some_work_starts_the_time_left_over() {
    // A move renames 400 entries in 40 seconds, then counts for four seconds what it could
    // not rename: a directory of 1,000 files of 64 KiB, which it copies at 10 MiB a second,
    // each opened in a twentieth of a second.
    let speed = 10 * MIB;
    let mut run = Run::new(401, 0);
    for _ in 0..400 {
        run.at += 0.1;
        run.counts.done += 1;
        run.report();
    }
    run.view.scanning(1_001, run.now());
    run.at += 4.0;
    run.counts.total = 400 + 1_001;
    run.counts.bytes_total = 1_000 * 64 * 1024;
    run.file(0, speed, (0.01, 0.0));
    // Its forecasts have to agree first, however long the renames took.
    let timing = run.view.timing(run.now()).unwrap();
    assert_eq!((timing.speed, timing.left), (None, None));
    for _ in 0..50 {
        run.file(64 * 1024, speed, (0.05, 0.005));
    }
    run.report();
    // Neither the renames nor the counting are the pace of the copy.
    let truth = 950.0 * (0.05 + 64.0 * 1024.0 / float(speed) + 0.005);
    let left = run.left();
    assert!((left - truth).abs() < truth * 0.05, "{left} for {truth}");
}

#[test]
fn the_entries_a_move_renamed_are_no_small_files_behind_its_copy() {
    // A move renames 1,000 entries, then copies 1,000 files of 1 MiB that it could not
    // rename, at 10 MiB a second, each opened in a twentieth of a second. The renames count
    // among the entries done, with no data: they made the files done look ten times smaller
    // than those left until over a hundred were done, so the time left stayed unknown.
    let speed = 10 * MIB;
    let mut run = Run::new(1_000, 0);
    for _ in 0..1_000 {
        run.at += 0.1;
        run.counts.done += 1;
        run.report();
    }
    run.view.scanning(1_001, run.now());
    run.counts.total = 1_000 + 1_001;
    run.counts.bytes_total = 1_000 * MIB;
    run.file(0, speed, (0.01, 0.0));
    let start = run.at;
    let mut shown = None;
    for n in 1..=1_000 {
        run.file(MIB, speed, (0.05, 0.005));
        if let Some(left) = run.view.timing(run.now()).unwrap().left {
            shown = Some((n, left));
            break;
        }
    }
    let (n, left) = shown.expect("shown");
    assert!(run.at - start < 5.0, "shown after {} s", run.at - start);
    let truth = float(1_000 - n) * (0.05 + 0.1 + 0.005);
    let left = left.as_secs_f64();
    assert!((left - truth).abs() < truth * 0.05, "{left} for {truth}");
}

#[test]
fn after_skip_all_the_time_left_waits_for_its_forecasts_to_agree() {
    // Skip all over 5,000 files of 64 KiB, each found out in 30 ms, as over a link: two and
    // a half minutes that tell no pace. Then 1,000 files of 64 KiB, each opened in a
    // twentieth of a second. The half minute after which the time left shows whatever its
    // forecasts say counted the skipping, so the first forecast showed at once.
    let size = 64 * 1024;
    let mut run = Run::new(6_000, 6_000 * size);
    for _ in 0..5_000 {
        run.skip(size, 0.03);
    }
    let start = run.at;
    let mut first = None;
    for n in 1..=1_000 {
        run.copy(size, 10 * MIB, 0.05);
        let known = run.left_at(run.at).is_some();
        let shown = run.view.timing(run.now()).unwrap().left.is_some();
        if known && first.is_none() {
            first = Some(run.at - start);
        }
        if let Some(first) = first.filter(|_| shown) {
            let worked = run.at - start;
            assert!(worked >= first + 2.0, "shown {worked} s in, known {first}");
            assert!(worked < first + 5.0, "shown {worked} s in, known {first}");
            return;
        }
        assert!(n < 1_000, "never shown");
    }
}

#[test]
fn opening_a_file_again_after_a_question_is_no_data_streaming() {
    // Files of 20 MiB at 10 MiB a second. The second fails halfway, and Retry copies it
    // again from its start, which takes two seconds to open: no report comes as it does.
    let (size, speed) = (20 * MIB, 10 * MIB);
    let mut run = Run::new(10, 10 * size);
    run.file(size, speed, (0.05, 0.01));
    run.report();
    run.at += 0.05;
    run.stream(size / 2, speed);
    run.view.ask(run.now());
    run.view.answered(run.now());
    run.counts.bytes_done -= size / 2;
    run.at += 2.0;
    run.stream(size, speed);
    let (byte, _) = run.view.forecast.sums.costs();
    assert!((byte * float(speed) - 1.0).abs() < 0.02, "{byte}");
}

#[test]
fn the_time_around_a_failure_is_no_pace_of_the_work() {
    // 100 files of 20 MiB at 10 MiB a second. After one, or forty, the next fails halfway: a
    // read waits a second, then fails; Retry opens it again in two seconds, and hashes it
    // from its start. Checksums reported again before they opened it, copies do not.
    let (size, speed, files) = (20 * MIB, 10 * MIB, 100);
    let (open, close) = (0.05, 0.01);
    let each = open + float(size) / float(speed) + close;
    let chunks = |run: &mut Run, mut left: u64| {
        while left > 0 {
            let chunk = left.min(CHUNK);
            run.at += float(chunk) / float(speed);
            run.counts.bytes_done += chunk;
            run.counts.bytes_copied += chunk;
            left -= chunk;
            run.report();
        }
    };
    for (before, again) in [(1, false), (1, true), (40, false), (40, true)] {
        let mut run = Run::new(files, files * size);
        for _ in 0..before {
            run.file(size, speed, (open, close));
        }
        run.report();
        run.at += open;
        chunks(&mut run, size / 2);
        run.at += 1.0;
        run.view.failed(run.now());
        run.view.answered(run.now());
        run.counts.bytes_done -= size / 2;
        if again {
            run.report();
        }
        run.at += 2.0;
        chunks(&mut run, size);
        // It was 68% too long after one file, 11% after forty.
        let truth = close + float(files - before - 1) * each;
        let left = run.left();
        assert!(
            (left - truth).abs() < truth * 0.02,
            "after {before}, again {again}: {left} for {truth}"
        );
    }
}

#[test]
fn a_question_about_a_taken_name_leaves_the_cost_of_its_entry_in() {
    // 2,000 files of 64 KiB at 10 MiB a second, each opened in a twentieth of a second,
    // each after a question about its name, which tells nothing of the pace but the time
    // it stands: their entries cost their opening.
    let (size, speed, files) = (64 * 1024, 10 * MIB, 2_000);
    let mut run = Run::new(files, files * size);
    for _ in 0..500 {
        run.report();
        run.view.ask(run.now());
        run.view.answered(run.now());
        run.at += 0.05;
        run.stream(size, speed);
        run.at += 0.005;
        run.counts.done += 1;
    }
    run.report();
    let truth = float(files - 500) * (0.05 + float(size) / float(speed) + 0.005);
    let left = run.left();
    assert!((left - truth).abs() < truth * 0.05, "{left} for {truth}");
}

#[test]
fn pauses_that_come_again_are_part_of_the_work() {
    // A link that sends a chunk of 32 KiB every 8 seconds: only the first pause looks like a
    // stall, and the time left keeps to the pace of the chunks.
    let mut run = Run::new(1, 2 * MIB);
    let mut unknown = Vec::new();
    for n in 1..=40 {
        let seen = run.at;
        for second in 1..8 {
            if run.left_at(seen + f64::from(second)).is_none() {
                unknown.push((n, second));
            }
        }
        run.at += 8.0;
        run.counts.bytes_done += CHUNK;
        run.counts.bytes_copied += CHUNK;
        run.report();
    }
    // Before the first chunk no pace is known, and its pause looked like a stall.
    assert!(unknown.iter().all(|(n, _)| *n <= 2), "{unknown:?}");
    let even = float(run.counts.bytes_total - run.counts.bytes_done) / float(CHUNK) * 8.0;
    let left = run.left();
    assert!((left - even).abs() < even * 0.05, "{left} for {even}");

    // A disk that takes 64 MiB in a third of a second, then 12 seconds to write them: after
    // the first time, the time it takes counts, and the time left is known throughout.
    let mut run = Run::new(1, 4_096 * MIB);
    let mut unknown = 0;
    for n in 0..30 {
        run.stream(64 * MIB, 192 * MIB);
        let seen = run.at;
        unknown += (1..12)
            .filter(|second| run.left_at(seen + f64::from(*second)).is_none())
            .count();
        run.at += 12.0;
        if n == 10 {
            let cycles = float(run.counts.bytes_total - run.counts.bytes_done) / float(64 * MIB);
            let even = cycles * (12.0 + 1.0 / 3.0);
            let left = run.left();
            assert!((left - even).abs() < even * 0.1, "{left} for {even}");
        }
    }
    assert_eq!(unknown, 7, "the first 12 seconds, from 5 on");
}

#[test]
fn skipping_entries_one_by_one_is_no_stall() {
    // Files of 64 KiB and 4 MiB by turns, then Skip all over 5,000 files of 1 KiB, each
    // found out in 20 ms as over a link, then 1,000 files of 1 KiB.
    let mut run = Run::new(6_200, 100 * (64 * 1024 + 4 * MIB) + 6_000 * 1024);
    for _ in 0..100 {
        run.copy(64 * 1024, 10 * MIB, 0.05);
        run.copy(4 * MIB, 10 * MIB, 0.05);
    }
    let before = run.left();
    for _ in 0..5_000 {
        run.skip(1024, 0.02);
        // The skipped ones leave the work, and their time does not slow the pace.
        let left = run.left_at(run.at);
        assert!(
            left.is_none_or(|left| left < before),
            "{left:?} after {before}"
        );
    }
    // Most of the work of late was skipped, and what is left to skip would count as work.
    assert_eq!(run.left_at(run.at), None);
    for _ in 0..200 {
        run.copy(1024, 10 * MIB, 0.05);
    }
    let left = run.left();
    let then = run.at;
    for _ in 0..800 {
        run.copy(1024, 10 * MIB, 0.05);
    }
    let took = run.at - then;
    assert!((left - took).abs() < took * 0.2, "{left} for {took}");
}

#[test]
fn a_big_file_after_tiny_ones_never_looks_done() {
    // 2,000 files of up to 30 KiB, each opened in a tenth of a second give or take a
    // third, then a file of 4 GiB at 100 MiB a second: the bytes of the small files cost
    // nothing beside the noise of opening them.
    for seed in 0..10 {
        let mut noise = Noise(seed);
        let sizes: Vec<u64> = (0..2_000).map(|_| noise.below(30 * 1024)).collect();
        let big = 4_096 * MIB;
        let mut run = Run::new(2_001, sizes.iter().sum::<u64>() + big);
        for size in sizes {
            let opening = noise.around(0.1, 0.3);
            run.copy(size, 100 * MIB, opening);
        }
        run.at += 0.1;
        let start = run.at;
        let took = float(big) / float(100 * MIB);
        for part in 1..=8 {
            run.stream(big / 8, 100 * MIB);
            let truth = took - (run.at - start);
            let left = run.left();
            // Never as if it took no time; and right from the second second on.
            assert!(left > truth * 0.8, "seed {seed}: {left} for {truth}");
            if part > 1 {
                assert!(left < truth * 1.2 + 1.0, "seed {seed}: {left} for {truth}");
            }
        }
    }
}

#[test]
fn the_time_left_rides_out_a_pace_that_comes_and_goes() {
    // 2 and 8 MiB a second, by turns, through one file of 2,000 MiB.
    let mut run = Run::new(1, 2_000 * MIB);
    let pace = |second: u64| {
        if second.is_multiple_of(2) {
            2 * MIB
        } else {
            8 * MIB
        }
    };
    for second in 0..60 {
        run.stream(pace(second), pace(second));
    }
    let mut speeds = Vec::new();
    let mut shown = Vec::new();
    for second in 60..70 {
        run.stream(pace(second), pace(second));
        let timing = run.view.timing(run.now()).unwrap();
        speeds.push(timing.speed.unwrap() / float(MIB));
        shown.push(whole_seconds(timing.left.unwrap()));
        // The time left keeps within 3% of what 5 MiB a second leaves …
        let left = run.left();
        let bytes = run.counts.bytes_total - run.counts.bytes_done;
        let even = float(bytes) / float(5 * MIB);
        assert!((left - even).abs() < even * 0.03, "{left} for {even}");
    }
    // … while the speed of the last five seconds swings by a fifth either way.
    let (slow, fast) = speeds
        .iter()
        .fold((f64::MAX, 0.0_f64), |(slow, fast), speed| {
            (slow.min(*speed), fast.max(*speed))
        });
    assert!(slow < 4.5 && fast > 5.5, "{speeds:?}");
    // The time left shown goes down by the clock.
    for pair in shown.windows(2) {
        assert_eq!(pair[0] - pair[1], 1, "{shown:?}");
    }
}

#[test]
fn the_time_left_follows_a_new_pace_within_a_minute() {
    let mut run = Run::new(1, 4_000 * MIB);
    run.stream(600 * MIB, 10 * MIB);
    // Half as fast from now on.
    run.stream(150 * MIB, 5 * MIB);
    let left = run.left();
    let (old, new) = (3_250.0 / 10.0, 3_250.0 / 5.0);
    assert!(left > old * 1.2 && left < new, "{left}");
    run.stream(150 * MIB, 5 * MIB);
    let left = run.left();
    let new = 3_100.0 / 5.0;
    assert!((left - new).abs() < new * 0.15, "{left}");
}

#[test]
fn a_big_file_after_small_ones_of_one_size_is_unknown_until_its_data_streams() {
    // 1,000 files of 4 KiB, each opened in 20 ms and closed in 10, then one of 600 MiB, at
    // 10 MiB a second: what a byte costs cannot come from the small ones, all alike.
    let (small, big, speed) = (4 * 1024, 600 * MIB, 10 * MIB);
    let mut run = Run::new(1_001, 1_000 * small + big);
    for file in 0..1_000 {
        run.file(small, speed, (0.02, 0.01));
        if file > 1 {
            assert_eq!(run.left_at(run.at), None, "file {file}");
        }
    }
    run.report();
    run.at += 0.02;
    let start = run.at;
    for second in 1..=60 {
        run.stream(speed, speed);
        if second > 1 {
            let truth = float(big) / float(speed) - (run.at - start) + 0.01;
            let left = run.left();
            assert!(
                (left - truth).abs() < truth * 0.02 + 0.1,
                "{left} for {truth}"
            );
        }
    }

    // Small files of sizes twice apart tell it: the time left shows before the big file.
    let mut run = Run::new(1_001, 500 * (2 * small + small / 2) + big);
    for _ in 0..500 {
        run.file(2 * small, speed, (0.02, 0.01));
        run.file(small / 2, speed, (0.02, 0.01));
    }
    run.report();
    assert!(run.left_at(run.at).is_some());

    // Files of one size with more of the same left show their time left.
    let mut run = Run::new(2_000, 2_000 * small);
    for _ in 0..1_000 {
        run.file(small, speed, (0.02, 0.01));
    }
    run.report();
    let left = run.left();
    assert!((left - 30.0).abs() < 30.0 * 0.05, "{left} for 30");
}

#[test]
fn a_file_far_larger_than_those_behind_goes_at_the_pace_of_its_data() {
    // 2,000 files of up to 64 KiB, each opened in 20 ms give or take a tenth, then one of
    // 2 GiB whose bytes go at another pace than theirs: slower, as a disk once a cache is
    // full, or a link whose window holds a small file whole; or faster. The small files tell
    // both costs, but the cost of a byte of the big file is that of its data streaming.
    for (small, big) in [(50 * MIB, 10 * MIB), (10 * MIB, 50 * MIB)] {
        let mut noise = Noise(1);
        let sizes: Vec<u64> = (0..2_000).map(|_| 1 + noise.below(64 * 1024)).collect();
        let size = 2_048 * MIB;
        let mut run = Run::new(2_001, sizes.iter().sum::<u64>() + size);
        for bytes in sizes {
            run.file(bytes, small, (noise.around(0.02, 0.1), 0.002));
        }
        let fitted = run.view.forecast.sums.fitted();
        assert!(matches!(fitted, Some((_, (true, true)))), "{fitted:?}");
        run.report();
        run.at += 0.02;
        let start = run.at;
        for second in 1..=30 {
            run.stream(big, big);
            if [2, 5, 10, 30].contains(&second) {
                let truth = float(size) / float(big) - (run.at - start) + 0.002;
                let left = run.left();
                assert!(
                    (left - truth).abs() < truth * 0.03,
                    "{small} then {big}, {second} s in: {left} for {truth}"
                );
            }
        }
    }
}

#[test]
fn a_file_far_larger_than_small_ones_that_took_the_time_of_their_bytes_goes_at_its_pace() {
    // On a disk, 20,000 files of up to 256 KiB at 1 GiB a second, each opened in a tenth of
    // a millisecond, take the time of their bytes rather than of their opening; then one of
    // 8 GiB at a fifth of that pace, as once the disk's cache is full. At its cost of a byte,
    // their entries cost less than nothing: at none, the small files behind weighed two and
    // a half times what they took, and the pace made the time left of the big file half as
    // long two seconds into it, and 70% after ten.
    let (small, big) = (1_024 * MIB, 200 * MIB);
    let mut noise = Noise(1);
    let sizes: Vec<u64> = (0..20_000).map(|_| 1 + noise.below(256 * 1024)).collect();
    let size = 40 * big;
    let mut run = Run::new(20_001, sizes.iter().sum::<u64>() + size);
    for bytes in sizes {
        run.file(bytes, small, (noise.around(0.000_1, 0.1), 0.000_02));
    }
    let fitted = run.view.forecast.sums.fitted();
    assert!(matches!(fitted, Some((_, (true, true)))), "{fitted:?}");
    run.report();
    run.at += 0.000_1;
    let start = run.at;
    for second in 1..=30 {
        run.stream(big, big);
        if [2, 5, 10, 30].contains(&second) {
            let truth = float(size) / float(big) - (run.at - start) + 0.000_02;
            let left = run.left();
            assert!(
                (left - truth).abs() < truth * 0.03,
                "{second} s in: {left} for {truth}"
            );
        }
    }
}

#[test]
fn opening_the_first_file_tells_nothing_of_what_a_byte_costs() {
    // As above, over a link where each file takes longer to open than a stretch lasts, the
    // first too, which the job's first report starts.
    let (small, big, speed) = (4 * 1024, 600 * MIB, 10 * MIB);
    let mut run = Run::new(1_001, 1_000 * small + big);
    for file in 0..1_000 {
        run.file(small, speed, (0.3, 0.01));
        if file > 1 {
            assert_eq!(run.left_at(run.at), None, "file {file}");
        }
    }
    run.report();
    run.at += 0.3;
    let start = run.at;
    for _ in 0..2 {
        run.stream(speed, speed);
    }
    let truth = float(big) / float(speed) - (run.at - start) + 0.01;
    let left = run.left();
    assert!((left - truth).abs() < truth * 0.02, "{left} for {truth}");

    // Small files of many sizes, each taking about 0.4 seconds to open, then smaller ones:
    // what is left takes the time of its entries, not of its bytes.
    let mut noise = Noise(3);
    let sizes: Vec<u64> = (0..200).map(|_| 1 + noise.below(30 * 1024)).collect();
    let mut run = Run::new(400, sizes.iter().sum::<u64>() + 200 * 1024);
    for size in sizes {
        run.file(size, speed, (noise.around(0.4, 0.3), 0.01));
    }
    run.report();
    let (left, then) = (run.left(), run.at);
    for _ in 0..200 {
        run.file(1024, speed, (noise.around(0.4, 0.3), 0.01));
    }
    let took = run.at - then;
    assert!((left - took).abs() < took * 0.2, "{left} for {took}");
}

#[test]
fn many_small_files_left_take_longer_than_their_bytes_say() {
    // A twentieth of a second to open each entry, then 10 MiB a second: files of 64 KiB
    // and of 4 MiB by turns, then 1,000 files of 1 KiB, which take some 50 seconds.
    let mut run = Run::new(1_200, 100 * (64 * 1024 + 4 * MIB) + 1_000 * 1024);
    for _ in 0..100 {
        run.copy(64 * 1024, 10 * MIB, 0.05);
        run.copy(4 * MIB, 10 * MIB, 0.05);
    }
    let left = run.left();
    let then = run.at;
    for _ in 0..1_000 {
        run.copy(1024, 10 * MIB, 0.05);
    }
    let took = run.at - then;
    assert!((left - took).abs() < took * 0.2, "{left} for {took}");
}

#[test]
fn entries_skipped_at_once_do_not_make_the_rest_look_quick() {
    let mut run = Run::new(6_200, 100 * (64 * 1024 + 4 * MIB) + 6_000 * 1024);
    for _ in 0..100 {
        run.copy(64 * 1024, 10 * MIB, 0.05);
        run.copy(4 * MIB, 10 * MIB, 0.05);
    }
    // Skip all: 5,000 files of 1 KiB that are there already, in one report.
    run.counts.done += 5_000;
    run.counts.bytes_done += 5_000 * 1024;
    run.report();
    // Most of the work of late was skipped, until it is more than five seconds behind.
    assert_eq!(run.left_at(run.at), None);
    for _ in 0..120 {
        run.copy(1024, 10 * MIB, 0.05);
    }
    let left = run.left();
    let then = run.at;
    for _ in 0..880 {
        run.copy(1024, 10 * MIB, 0.05);
    }
    let took = run.at - then;
    assert!((left - took).abs() < took * 0.2, "{left} for {took}");
}

#[test]
fn skipped_files_among_copied_ones_leave_a_pace() {
    // 20,000 files of 64 KiB on a fast disk, at 1 GiB a second, each opened and closed in
    // half a millisecond; one in five is there already, found out in 50 µs, and skipped.
    // Hundreds of them go in a quarter of a second, so that each stretch of one held a skip
    // and none counted: the time left was unknown throughout.
    let (size, speed, files) = (64 * 1024, 1024 * MIB, 20_000);
    let (open, close, skip) = (0.000_5, 0.000_5, 0.000_05);
    let mut run = Run::new(files, files * size);
    for n in 0..10_000 {
        if n % 5 == 4 {
            run.report();
            run.skip(size, skip);
        } else {
            run.file(size, speed, (open, close));
        }
    }
    run.report();
    let each = open + float(size) / float(speed) + close;
    let truth = float(files - 10_000) * (0.8 * each + 0.2 * skip);
    // What is left to skip counts as work, as nothing tells which: a quarter more.
    let left = run.left();
    assert!(
        (left / (truth * 1.25) - 1.0).abs() < 0.03,
        "{left} for {truth}"
    );
    assert!(run.view.timing(run.now()).unwrap().left.is_some());
}

#[test]
fn the_time_left_is_unknown_while_most_files_are_skipped() {
    // Files of 256 KiB over a fast link, at 100 MiB a second, opened in 4 ms and closed in
    // 2; one that is there already is found out in a millisecond and skipped. Some twenty
    // seconds each of one in two skipped, of four in five, of one in two again, then of
    // none.
    let (size, speed, times) = (256 * 1024, 100 * MIB, (0.004, 0.002));
    let phases = [(4_000, 1, 2), (8_000, 4, 5), (4_000, 1, 2), (2_400, 0, 1)];
    let files = phases.iter().map(|&(count, ..)| count).sum::<u64>();
    let mut run = Run::new(files, files * size);
    let mut known = Vec::new();
    for (count, skipped, of) in phases {
        let start = run.at;
        let mut seen = Vec::new();
        for n in 0..count {
            if n % of < skipped {
                run.report();
                run.skip(size, 0.001);
            } else {
                run.file(size, speed, times);
            }
            seen.push((run.at - start, run.left_at(run.at).is_some()));
        }
        known.push(seen);
    }
    let from = |phase: &[(f64, bool)], from: f64, shown: bool| {
        phase
            .iter()
            .filter(|&&(at, _)| at >= from)
            .all(|&(_, known)| known == shown)
    };
    // Twice as long as it takes, with the files left to skip as work: it shows.
    assert!(from(&known[0], 5.0, true));
    // Five times as long: unknown, a few seconds in.
    assert!(from(&known[1], 5.0, false));
    // Half the work skipped again does not make it come back.
    assert!(from(&known[2], 0.0, false));
    // With none skipped for five seconds and more, it shows, and keeps showing.
    let first = known[3].iter().position(|&(_, known)| known).unwrap();
    assert!(known[3][first].0 < 6.0, "after {} s", known[3][first].0);
    assert!(known[3][first..].iter().all(|&(_, known)| known));
}

#[test]
fn a_question_before_the_work_does_not_start_the_clock() {
    let start = Instant::now();
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    view.ask(start);
    view.answered(start + secs(5));
    assert!(!view.ticking(), "still counting");
    view.working("a".to_owned(), copying(0, 0), start + secs(7));
    assert_eq!(view.timing(start + secs(9)).unwrap().elapsed, secs(2));
}

#[test]
fn the_clock_stands_while_a_job_counts_after_some_work() {
    // A move renames for ten seconds, then counts for a minute what it could not rename,
    // with a question on the way, then copies for five seconds: it has worked fifteen, as
    // the counting before a copy's first report does not count either.
    let start = Instant::now();
    let mut view = JobView::new("Move".to_owned(), "Moving".to_owned());
    for second in 0..=10 {
        let renames = Counts {
            done: second,
            total: 20,
            ..Counts::default()
        };
        view.working("a".to_owned(), renames, start + secs(second));
    }
    view.scanning(0, start + secs(10));
    assert!(!view.ticking(), "it counts");
    view.failed(start + secs(30));
    view.answered(start + secs(40));
    assert!(!view.ticking(), "still counting");
    view.scanning(5, start + secs(50));
    for second in 0..=5 {
        let counts = copying(second, second);
        view.working("b".to_owned(), counts, start + secs(70 + second));
    }
    assert_eq!(view.timing(start + secs(75)).unwrap().elapsed, secs(15));
}

#[test]
fn jobs_without_data_show_only_their_time() {
    let start = Instant::now();
    let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
    let counts = Counts {
        done: 1,
        total: 4,
        ..Counts::default()
    };
    view.working("a".to_owned(), counts, start);
    let text = draw_at(&view, start + secs(3_725));
    assert!(text.contains("Time 1:02:05"), "{text}");
    assert!(!text.contains("ETA"), "{text}");
    assert_eq!(view.summary(start + secs(10)).left, "");
}

#[test]
fn durations_and_speeds_read_short() {
    assert_eq!(duration(Duration::ZERO), "0:00");
    assert_eq!(duration(Duration::from_millis(59_600)), "1:00");
    assert_eq!(duration(secs(754)), "12:34");
    assert_eq!(duration(secs(36_000)), "10:00:00");
    assert_eq!(speed_size(1536.0), "1.5K");
}

#[test]
fn the_gauge_fills_by_ratio() {
    let theme = Theme::terminal();
    let text = |ratio| gauge(ratio, 15, &theme).to_string();
    assert_eq!(text(0.0), "░░░░░░░░░░   0%");
    assert_eq!(text(0.5), "█████░░░░░  50%");
    assert_eq!(text(1.0), "██████████ 100%");
}

#[test]
fn enter_sends_to_the_background_and_esc_aborts() {
    let press = |view: &mut JobView, action| view.handle(Resolved::Action(action));
    let mut view = JobView::new("Copy".to_owned(), "Copying".to_owned());
    assert!(draw(&view).contains("{ Background } [ Abort ]"));
    assert_eq!(
        press(&mut view, Action::Confirm),
        Some(JobButton::Background)
    );
    assert_eq!(press(&mut view, Action::Cancel), Some(JobButton::Abort));
    assert_eq!(press(&mut view, Action::Right), None);
    assert_eq!(press(&mut view, Action::Toggle), Some(JobButton::Abort));
    assert_eq!(press(&mut view, Action::PrevField), None);
    assert_eq!(
        press(&mut view, Action::Confirm),
        Some(JobButton::Background)
    );
    assert_eq!(view.handle(Resolved::Insert('a')), None);
    assert_eq!(view.handle(Resolved::Hotkey('a')), Some(JobButton::Abort));
    assert_eq!(
        view.handle(Resolved::Hotkey('B')),
        Some(JobButton::Background)
    );
    assert_eq!(view.handle(Resolved::Hotkey('x')), None);

    view.keep_in_front();
    assert_eq!(view.handle(Resolved::Hotkey('b')), None, "no Background");
    assert!(!draw(&view).contains("Background"));
    for action in [Action::Cancel, Action::Confirm, Action::Toggle] {
        assert_eq!(press(&mut view, action), Some(JobButton::Abort));
    }
    assert_eq!(press(&mut view, Action::Right), None);
    assert_eq!(press(&mut view, Action::Confirm), Some(JobButton::Abort));
}

#[test]
fn the_ratio_follows_bytes_or_entries() {
    let now = Instant::now();
    let mut view = JobView::new("Delete".to_owned(), "Deleting".to_owned());
    assert!(view.ratio() < f64::EPSILON);
    let counts = Counts {
        done: 1,
        total: 4,
        ..Counts::default()
    };
    view.working(String::new(), counts, now);
    assert!((view.ratio() - 0.25).abs() < f64::EPSILON);
    let counts = Counts {
        done: 1,
        total: 4,
        bytes_done: 3,
        bytes_total: 4,
        bytes_copied: 3,
    };
    view.working(String::new(), counts, now);
    assert!((view.ratio() - 0.75).abs() < f64::EPSILON);
}
