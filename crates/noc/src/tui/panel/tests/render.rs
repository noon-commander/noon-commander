use super::*;

#[test]
fn draws_the_listing() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::Down);
    insta::assert_snapshot!(draw(&mut panel, 50, 12, true));
}

#[test]
fn a_skeleton_stands_for_the_rows_of_another_location_once_the_wait_shows() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::Down);
    panel.handle(Action::Mark);
    panel.go(Destination::to(remote("web", "")));
    let theme = Theme::terminal();
    let hosts = |_: &str| HostState::default();
    let mut terminal = Terminal::new(TestBackend::new(50, 12)).unwrap();
    let start = Instant::now();
    let mut screen = |at: Instant| {
        let view = View {
            hosts: &hosts,
            decor: Decor::new(false),
            theme: &theme,
            spin: Spin::new(at, start, false, theme.glyphs.spinners),
            tz: &TimeZone::UTC,
            formats: &Formats::iso(),
        };
        terminal
            .draw(|frame| panel.render(frame, frame.area(), true, &view))
            .unwrap();
        terminal.backend().to_string()
    };
    let first = screen(start);
    assert!(
        first.contains("alpha-link"),
        "a quick listing shows no wait: {first}"
    );
    insta::assert_snapshot!(screen(start + spinner::DELAY));
}

#[test]
fn the_frame_shows_the_space_of_the_file_system_while_it_fits() {
    let (mut panel, request) = Panel::new(local("/srv"), PathBuf::from(HOME), true);
    let space = Space {
        total: 500 << 30,
        available: 123 << 30,
    };
    let reply = Listed {
        location: local("/srv"),
        listing: Listing::Dir(Vec::new()),
        space: Some(space),
    };
    panel.listed(request.generation, Ok(reply));
    insta::assert_snapshot!(draw(&mut panel, 50, 6, true));
    // The total of the directory, ` 0B in 0 files, 0 dirs `, comes first.
    assert!(bottom(&mut panel, 47).contains("123G / 500G (24%)"));
    let narrow = bottom(&mut panel, 46);
    assert!(!narrow.contains("123G"), "too narrow: {narrow}");
    assert!(narrow.contains("0B in 0 files, 0 dirs"), "{narrow}");

    // Without the total, as on a host that is not connected, the space has the frame.
    panel.disconnected = true;
    assert!(bottom(&mut panel, 23).contains("123G / 500G (24%)"));
    assert!(!bottom(&mut panel, 22).contains("123G"), "too narrow");
    panel.disconnected = false;

    // A failed reload keeps it; the next listing replaces it.
    let request = panel.reload_onto(Vec::new());
    panel.listed(request.generation, Err(ListError::other("gone".to_owned())));
    assert_eq!(panel.space, Some(space));
    let request = panel.reload_onto(Vec::new());
    answer(&mut panel, &request, Listing::Dir(Vec::new()));
    assert_eq!(panel.space, None);
    assert!(!bottom(&mut panel, 50).contains('G'));

    // A file system that reports nothing shows nothing.
    panel.space = Some(Space {
        total: 0,
        available: 0,
    });
    assert!(!bottom(&mut panel, 50).contains('%'));
}

#[test]
fn the_status_line_shows_the_target_of_a_symlink_cut_at_its_start() {
    let link = |name: &str, target: Option<&str>, kind: Option<FileKind>| DirEntry {
        target_kind: kind,
        link_target: target.map(|target| target.as_bytes().to_vec()),
        ..entry(name, FileKind::Symlink, 1)
    };
    let config = "/etc/app/config.toml";
    let mut panel = loaded(
        "/srv",
        vec![
            link("conf", Some(config), Some(FileKind::File)),
            link("gone", Some("../missing"), None),
            link("opt", Some("/opt"), Some(FileKind::Dir)),
            link("unread", None, Some(FileKind::File)),
        ],
    );
    let hosts = |_: &str| HostState::default();
    let plain = Theme::mc_classic();
    let nerd = Theme::mc_classic().with_glyphs(&glyphs::Glyphs::NERD);
    // The cells of the status line of a panel 6 high, inside the frame, and their text.
    let status = |panel: &mut Panel, width: u16, icons: bool| {
        let theme = if icons { &nerd } else { &plain };
        let size = (width, 6);
        let terminal = render_themed(panel, size, true, &hosts, Decor::new(icons), theme);
        let buffer = terminal.backend().buffer();
        let cells: Vec<_> = (1..width - 1).map(|x| buffer[(x, 4)].clone()).collect();
        let text: String = cells.iter().map(|cell| cell.symbol().to_owned()).collect();
        (text.trim_end().to_owned(), cells)
    };
    let fg = |style: Style| style.fg.unwrap();
    // The color of the status line's text, which the name keeps.
    let text_fg = fg(plain.panel);

    // Directories first: `opt`, then `conf`.
    panel.handle(Action::Down);
    let (text, cells) = status(&mut panel, 40, false);
    assert_eq!(text, "opt -> /opt/", "a directory's marker after it");
    assert_eq!(cells[0].fg, text_fg, "the name plain");
    assert_eq!(cells[7].fg, fg(plain.directory), "the target by its kind");

    panel.handle(Action::Down);
    let (text, cells) = status(&mut panel, 40, false);
    assert_eq!(text, format!("conf -> {config}"), "no marker after a file");
    assert_eq!(cells[0].fg, text_fg);
    assert_eq!(cells[5].fg, text_fg);
    assert!(
        cells[5].modifier.contains(Modifier::DIM),
        "the arrow toned down"
    );
    assert!(!cells[0].modifier.contains(Modifier::DIM));
    assert_eq!(cells[8].fg, fg(plain.file));
    assert!(!cells[8].modifier.contains(Modifier::DIM));
    let (text, _) = status(&mut panel, 22, false);
    assert_eq!(text, "conf -> «config.toml", "the end of the path stays");
    let (text, cells) = status(&mut panel, 13, false);
    assert_eq!(text, "conf", "no room for the target");
    assert_eq!(cells[0].fg, text_fg);

    let (text, cells) = status(&mut panel, 40, true);
    let arrow = glyphs::Glyphs::NERD.link_arrow;
    let (icon, _) = Decor::new(true).target(Some(FileKind::File), config.as_bytes());
    assert_eq!(text, format!("conf {arrow} {icon}{config}"), "with icons");
    assert_eq!(
        cells[7].fg,
        fg(plain.file),
        "the icon in the target's color"
    );
    assert!(cells[7].modifier.contains(Modifier::DIM), "toned down");
    let (text, _) = status(&mut panel, 22, true);
    assert_eq!(
        text,
        format!(
            "conf {arrow} {icon}{}{}",
            glyphs::Glyphs::NERD.marks.before,
            &config[config.len() - 10..]
        ),
        "the icon stays, and the target loses a cell more"
    );

    panel.handle(Action::Down);
    let (text, cells) = status(&mut panel, 40, false);
    assert_eq!(text, "gone -> ../missing!");
    assert_eq!(cells[0].fg, text_fg);
    assert_eq!(cells[8].fg, fg(plain.stale_link), "a broken link's target");
    let (text, _) = status(&mut panel, 20, false);
    assert_eq!(text, "gone -> «/missing!", "the marker stays");

    panel.handle(Action::Down);
    let (text, _) = status(&mut panel, 40, false);
    assert_eq!(text, "unread", "a target that could not be read");
}

#[test]
fn the_frame_shows_the_size_of_the_shown_files_and_how_many_files_and_dirs() {
    let mut panel = loaded("/srv", listing());
    // 12,345 + 1 + 10,000,000 bytes; a link to a directory is a directory.
    assert_eq!(panel.shown_total(), (3, 3, 10_012_346));
    insta::assert_snapshot!(draw(&mut panel, 40, 6, true));

    panel.set_show_hidden(false);
    assert_eq!(
        panel.shown_total(),
        (2, 3, 10_012_345),
        "hidden files are left out"
    );
    assert!(bottom(&mut panel, 40).contains(" 9.5M in 2 files, 3 dirs "));

    let mut panel = loaded(
        "/srv",
        vec![
            entry("one", FileKind::File, 7),
            entry("d", FileKind::Dir, 0),
        ],
    );
    assert!(bottom(&mut panel, 40).contains(" 7B in 1 file, 1 dir "));
    // ` 7B in 1 file, 1 dir ` and a corner and a line on either side.
    assert!(bottom(&mut panel, 25).contains("1 dir"));
    assert!(!bottom(&mut panel, 24).contains("dir"), "too narrow");
}

/// The rows of `panel` drawn 14 cells wide, a name column of 12, four of them, inside the
/// frame.
fn name_rows(panel: &mut Panel) -> Vec<String> {
    let screen = draw(panel, 14, 9, true).to_string();
    screen
        .lines()
        .skip(2)
        .take(4)
        .map(|line| {
            let inside: String = line.trim_matches('"').chars().skip(1).collect();
            inside.chars().take(12).collect()
        })
        .collect()
}

#[test]
fn z_scrolls_the_names_as_vim_scrolls_lines_that_do_not_wrap() {
    let mut panel = loaded(
        "/srv",
        vec![
            entry("dir", FileKind::Dir, 0),
            entry("abcdefghijklmnopqrst", FileKind::File, 1),
            entry("short", FileKind::File, 1),
        ],
    );
    panel.handle(Action::Down);
    panel.handle(Action::Down);
    assert_eq!(under_cursor(&panel), "abcdefghijklmnopqrst");
    assert_eq!(
        name_rows(&mut panel),
        [
            "/..         ",
            "/dir        ",
            " abcde»pqrst",
            " short      "
        ],
        "a name that does not fit loses its middle"
    );
    let press = |panel: &mut Panel, actions: &[Action]| {
        for action in actions {
            panel.handle(*action);
        }
        name_rows(panel)
    };
    // `zl`: a column of the names alone, the markers staying in front, `«` over the first
    // cell after them, and a name cut short at its end.
    assert_eq!(
        press(&mut panel, &[Action::ScrollRight]),
        [
            "/«          ",
            "/«r         ",
            " «cdefghijk»",
            " «ort       "
        ]
    );
    // `zL`: half the column of 12.
    assert_eq!(
        press(&mut panel, &[Action::ScrollHalfRight])[2],
        " «ijklmnopq»"
    );
    assert_eq!(panel.shift, 7);
    // `zh` and `zH`, never past the start.
    assert_eq!(press(&mut panel, &[Action::ScrollLeft])[2], " «hijklmnop»");
    press(
        &mut panel,
        &[Action::ScrollHalfLeft, Action::ScrollHalfLeft],
    );
    assert_eq!(panel.shift, 0);
    // `ze`: the end of the name under the cursor at the right edge.
    assert_eq!(press(&mut panel, &[Action::ScrollEnd])[2], " «klmnopqrst");
    // `zl` stops where `ze` does for the longest name on screen, its end at the right edge,
    // so that the column never scrolls empty.
    assert_eq!(
        press(&mut panel, &[Action::ScrollRight; 40])[2],
        " «klmnopqrst"
    );
    assert_eq!(
        panel.shift, 9,
        "'abcdefghijklmnopqrst' is 20 cells, the 11 after the marker"
    );
    // `zs`: from the start again.
    press(&mut panel, &[Action::ScrollStart]);
    assert_eq!(panel.shift, 0);
    // On a shorter name, the longest on screen still sets how far `zl` goes.
    press(&mut panel, &[Action::Down]);
    assert_eq!(under_cursor(&panel), "short");
    press(&mut panel, &[Action::ScrollRight; 40]);
    assert_eq!(panel.shift, 9);
    // Moving the cursor keeps the names where they are, and `zl` goes no further.
    press(&mut panel, &[Action::Up, Action::ScrollRight]);
    assert_eq!(panel.shift, 9);
    press(&mut panel, &[Action::Up, Action::Up, Action::ScrollRight]);
    assert_eq!(panel.shift, 9);
    // Another directory shows its names from their start.
    panel.cursor = 1;
    let request = panel.handle(Action::Enter).unwrap();
    answer(&mut panel, &request, Listing::Dir(vec![]));
    assert_eq!(panel.shift, 0);
}

#[test]
fn the_names_scroll_back_as_far_as_the_names_on_screen_need() {
    let mut entries = vec![entry("abcdefghijklmnopqrst", FileKind::File, 1)];
    entries.extend((0..9).map(|n| entry(&format!("medium-name{n}"), FileKind::File, 1)));
    let mut panel = loaded("/srv", entries);
    name_rows(&mut panel);
    for _ in 0..40 {
        panel.handle(Action::ScrollRight);
    }
    assert_eq!(
        panel.shift, 9,
        "'abcdefghijklmnopqrst' is 20 cells, the 11 after the marker"
    );
    panel.handle(Action::End);
    let rows = name_rows(&mut panel);
    assert!(panel.window.top() > 1, "the long name is off screen");
    assert_eq!(panel.shift, 1, "'medium-name8' is 12 cells");
    assert_eq!(rows[3], " «dium-name8");
    panel.handle(Action::ScrollRight);
    assert_eq!(panel.shift, 1);
    // Back by the long name, it scrolls as far again.
    panel.handle(Action::Home);
    name_rows(&mut panel);
    for _ in 0..40 {
        panel.handle(Action::ScrollRight);
    }
    assert_eq!(panel.shift, 9);
    // Where all the names on screen fit, they do not scroll.
    let mut panel = loaded("/srv", vec![entry("short", FileKind::File, 1)]);
    name_rows(&mut panel);
    panel.handle(Action::ScrollRight);
    assert_eq!(panel.shift, 0);
}

#[test]
fn scrolled_names_keep_their_icons_in_front() {
    use ratatui::style::Modifier;

    let long = entry("abcdefghijklmnopqrst", FileKind::File, 1);
    let mut panel = loaded("/srv", vec![long.clone()]);
    let hosts = |_: &str| HostState::default();
    let theme = Theme::mc_classic();
    let decor = Decor::new(true);
    let draw = |panel: &mut Panel| render_themed(panel, (14, 8), true, &hosts, decor, &theme);
    draw(&mut panel);
    panel.handle(Action::ScrollRight);
    let row = |terminal: &Terminal<TestBackend>, y: u16| -> String {
        let buffer = terminal.backend().buffer();
        (1..13).map(|x| buffer[(x, y)].symbol()).collect()
    };
    let marks = theme.glyphs.marks;
    let terminal = draw(&mut panel);
    // Rows: frame, header, `..`, the long name; a column of 12.
    assert_eq!(
        row(&terminal, 2),
        format!("{}{}{}", decor.parent(), marks.before, " ".repeat(9))
    );
    let (icon, before, after) = (decor.entry(&long), marks.before, marks.after);
    assert_eq!(row(&terminal, 3), format!("{icon}{before}cdefghij{after}"));
    let buffer = terminal.backend().buffer();
    assert!(
        buffer[(1, 3)].modifier.contains(Modifier::DIM),
        "the icon is toned down as ever"
    );
    assert!(!buffer[(4, 3)].modifier.contains(Modifier::DIM));
    // A rename in the row draws its field after the icon, which stays.
    panel.handle(Action::Down);
    assert!(panel.start_rename());
    let terminal = draw(&mut panel);
    assert_eq!(panel.shift, 1);
    assert!(row(&terminal, 3).starts_with(&decor.entry(&long)));
}

#[test]
fn the_marks_of_names_cut_short_have_an_accent_on_every_row() {
    use ratatui::style::{Color, Modifier};

    let mut panel = loaded(
        "/srv",
        vec![
            entry("abcdefghijklmnopqrst", FileKind::File, 1),
            entry("bcdefghijklmnopqrstu", FileKind::File, 1),
        ],
    );
    panel.handle(Action::Down);
    let hosts = |_: &str| HostState::default();
    let theme = Theme::mc_classic();
    let terminal = render_themed(&mut panel, (14, 8), true, &hosts, Decor::new(false), &theme);
    let buffer = terminal.backend().buffer();
    // The mark: after the frame, the marker, and five letters.
    let cell = |y: u16| &buffer[(7, y)];
    assert_eq!(cell(3).symbol(), "»");
    assert_eq!(
        (cell(3).fg, cell(3).bg),
        (Color::LightYellow, Color::Black),
        "under the cursor"
    );
    assert_eq!((cell(4).fg, cell(4).bg), (Color::LightYellow, Color::Blue));
    assert!(cell(4).modifier.contains(Modifier::BOLD));
    assert_eq!(
        buffer[(8, 4)].fg,
        Color::Gray,
        "the name goes on in its color"
    );
}

#[test]
fn narrow_panels_drop_columns_and_unsafe_names_are_shown_safely() {
    let mut panel = loaded(
        "/a/very/long/path/that/does/not/fit",
        vec![entry("bad\x1b[2Jname", FileKind::File, 3)],
    );
    panel.handle(Action::End);
    insta::assert_snapshot!(draw(&mut panel, 24, 7, true));
}

#[test]
fn marked_rows_keep_their_colors_beside_a_bar_in_the_frame() {
    use ratatui::style::{Color, Modifier};

    let mut panel = loaded("/srv", listing());
    panel.handle(Action::End);
    panel.handle(Action::MarkUp);
    panel.handle(Action::Mark);
    let hosts = |_: &str| HostState::default();
    let theme = Theme::mc_classic();
    let terminal = render_themed(
        &mut panel,
        (40, 12),
        true,
        &hosts,
        Decor::new(false),
        &theme,
    );
    let buffer = terminal.backend().buffer();
    let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
    // Rows: frame, header, `..`, alpha-link, Beta, bin, .hidden, Alpha.md, zeta.txt.
    let bar = glyphs::MARKED_ROW.to_string();
    for y in [7, 8] {
        assert_eq!(buffer[(0, y)].symbol(), bar, "row {y}");
        assert_eq!(colors(0, y), (Color::LightYellow, Color::Blue), "row {y}");
    }
    assert_eq!(buffer[(0, 6)].symbol(), "║", "not the others");
    assert_eq!(
        colors(2, 7),
        (Color::Gray, Color::Blue),
        "marked, as a file"
    );
    assert_eq!(colors(30, 7), (Color::Gray, Color::Blue), "all of it");
    assert_eq!(
        colors(2, 8),
        (Color::Black, Color::Cyan),
        "under the cursor"
    );
    let underlined = |x: u16, y: u16| buffer[(x, y)].modifier.contains(Modifier::UNDERLINED);
    assert!(!underlined(2, 7) && !underlined(2, 8), "never underlined");
    let separator: String = (0..40).map(|x| buffer[(x, 9)].symbol()).collect();
    assert_eq!(separator, "╟─────── 10012345 B in 2 files ────────╢");
    assert_eq!(colors(8, 9), (Color::LightYellow, Color::Blue));
    assert_eq!(colors(7, 9), (Color::Gray, Color::Blue));

    // With a color between the panel's and the mark's, under the row and the bar.
    let depth = crate::tui::theme::ColorDepth::TrueColor;
    let theme = Theme::by_name("catppuccin-mocha", depth).unwrap();
    let terminal = render_themed(
        &mut panel,
        (40, 12),
        true,
        &hosts,
        Decor::new(false),
        &theme,
    );
    let buffer = terminal.backend().buffer();
    let tint = theme.marked.bg.unwrap();
    let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
    assert_eq!(colors(0, 7), (theme.mark.fg.unwrap(), tint), "the bar");
    assert_eq!(colors(2, 7), (theme.file.fg.unwrap(), tint), "the name");
    assert_eq!(colors(30, 7).1, tint, "all of it");
    assert_eq!(colors(2, 6).1, theme.panel.bg.unwrap(), "not the others");
    assert_eq!(
        colors(2, 8),
        (theme.cursor.fg.unwrap(), theme.cursor.bg.unwrap())
    );
}

#[test]
fn the_scroll_bar_runs_down_the_right_of_the_frame_along_the_rows() {
    let hosts = |_: &str| HostState::default();
    let mut short = loaded("/srv", vec![entry("one", FileKind::File, 1)]);
    let fits = render_with(&mut short, (40, 12), true, &hosts, Decor::new(false));
    assert!(!fits.backend().to_string().contains('░'));
    assert_eq!(short.bar(), None, "everything fits");
    let files = (0..40).map(|n| entry(&format!("f{n:02}"), FileKind::File, 1));
    let mut long = loaded("/srv", files.collect());
    render_with(&mut long, (40, 12), true, &hosts, Decor::new(false));
    let area = long.bar().unwrap().area();
    assert_eq!(
        (area.x, area.y, area.height),
        (39, 2, 7),
        "between the header and the line"
    );
    long.scroll_bar(Scroll::By(Motion::Down));
    assert_eq!(
        (long.window.top(), long.cursor),
        (1, 4),
        "a row, as the wheel"
    );
    long.scroll_bar(Scroll::By(Motion::PageDown));
    assert!(long.window.top() > 1, "a page, as Ctrl+f");
    long.scroll_bar(Scroll::To(30));
    assert_eq!(long.window.top(), 30);
    assert!(
        (30..37).contains(&long.cursor),
        "on the screen: {}",
        long.cursor
    );
}

#[test]
fn a_taller_panel_shows_the_rows_above_rather_than_empty_ones_below() {
    let hosts = |_: &str| HostState::default();
    let files = (0..40).map(|n| entry(&format!("f{n:02}"), FileKind::File, 1));
    let mut panel = loaded("/srv", files.collect());
    let last = panel.rows() - 1;
    panel.cursor = last;
    render_with(&mut panel, (40, 12), true, &hosts, Decor::new(false));
    let short = panel.page;
    assert_eq!(panel.window.top(), last + 1 - short);
    let taller = render_with(&mut panel, (40, 20), true, &hosts, Decor::new(false));
    assert!(panel.page > short);
    assert_eq!(
        (panel.window.top(), panel.cursor),
        (last + 1 - panel.page, last)
    );
    let text = taller.backend().to_string();
    assert!(text.contains("f25") && text.contains("f39"), "{text}");
}

#[test]
fn single_borders_draw_single_lines() {
    let mut panel = loaded("/srv", listing());
    let hosts = |_: &str| HostState::default();
    let theme = Theme::terminal().with_borders(noc_config::Borders::Single);
    let terminal = render_themed(
        &mut panel,
        (40, 12),
        true,
        &hosts,
        Decor::new(false),
        &theme,
    );
    let text = terminal.backend().to_string();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("\"┌"), "{text}");
    assert!(lines[9].starts_with("\"├────"), "{text}");
    assert!(lines[11].starts_with("\"└"), "{text}");
    assert!(!text.contains(['═', '║', '╟']), "{text}");
    assert_eq!(lines[0].matches('┬').count(), 2, "{text}");
    assert_eq!(lines[9].matches('┴').count(), 2, "{text}");
}

#[test]
fn column_lines_run_down_the_whole_listing_and_join_the_frame() {
    let mut panel = loaded("/srv", vec![entry("only.txt", FileKind::File, 1)]);
    let text = draw(&mut panel, 44, 10, true).to_string();
    let lines: Vec<&str> = text.lines().collect();
    let column = |line: &str| line.chars().nth(19);
    assert_eq!(column(lines[0]), Some('╤'), "{text}");
    for line in &lines[1..7] {
        assert_eq!(column(line), Some('│'), "below the rows too: {text}");
    }
    assert_eq!(column(lines[7]), Some('┴'), "{text}");
    assert_eq!(
        column(lines[8]),
        Some(' '),
        "not across the status line: {text}"
    );
}

#[test]
fn column_lines_take_the_background_of_the_cursor_and_marks() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::End);
    panel.handle(Action::MarkUp);
    let hosts = |_: &str| HostState::default();
    let theme = Theme::mc_classic();
    // Row 7 is under the cursor, row 8 marked; a line runs between name and size.
    for (active, cursor) in [(true, Color::Cyan), (false, Color::DarkGray)] {
        let terminal = render_themed(
            &mut panel,
            (44, 12),
            active,
            &hosts,
            Decor::new(false),
            &theme,
        );
        let buffer = terminal.backend().buffer();
        for (y, bg) in [(6, Color::Blue), (7, cursor), (8, Color::Blue)] {
            let cell = &buffer[(18, y)];
            assert_eq!(cell.symbol(), "│");
            assert_eq!((cell.fg, cell.bg), (Color::Gray, bg), "row {y}, {active}");
            assert_eq!(buffer[(17, y)].bg, bg, "row {y}, {active}: as the row");
            assert!(
                !cell.modifier.contains(Modifier::UNDERLINED),
                "row {y}, {active}: never underlined"
            );
        }
    }
}

#[test]
fn units_of_sizes_take_colors_by_how_big_and_the_cursor_s_on_its_row() {
    let files = [
        ("a", 512, "B", Color::Gray, false),
        ("b", 5 << 10, "K", Color::Gray, false),
        ("c", 5 << 20, "M", Color::Gray, true),
        ("d", 3 << 30, "G", Color::LightCyan, true),
        ("e", 2 << 40, "T", Color::LightRed, true),
    ];
    let entries = files
        .iter()
        .map(|&(name, size, ..)| entry(name, FileKind::File, size))
        .collect();
    let mut panel = loaded("/srv", entries);
    let hosts = |_: &str| HostState::default();
    let theme = Theme::mc_classic();
    let draw = |panel: &mut Panel, active: bool| {
        let size = (40, 11);
        let terminal = render_themed(panel, size, active, &hosts, Decor::new(false), &theme);
        terminal.backend().buffer().clone()
    };
    // The unit ends the size, before the line of the time column.
    let unit = |buffer: &Buffer, y: u16| {
        let x = (0..40).rfind(|&x| buffer[(x, y)].symbol() == "│").unwrap() - 1;
        (buffer[(x - 1, y)].clone(), buffer[(x, y)].clone())
    };
    let buffer = draw(&mut panel, true);
    for (y, (name, _, symbol, color, bold)) in (3..).zip(files) {
        let (digit, unit) = unit(&buffer, y);
        assert_eq!((unit.symbol(), unit.fg), (symbol, color), "{name}");
        assert_eq!(unit.modifier.contains(Modifier::BOLD), bold, "{name}");
        assert_eq!(digit.fg, Color::Gray, "{name}: the digits as the row");
        assert!(!digit.modifier.contains(Modifier::BOLD), "{name}");
    }

    panel.handle(Action::End);
    let (_, on_cursor) = unit(&draw(&mut panel, true), 7);
    assert_eq!((on_cursor.fg, on_cursor.bg), (Color::Black, Color::Cyan));
    assert!(!on_cursor.modifier.contains(Modifier::BOLD));
    let (_, faint_cursor) = unit(&draw(&mut panel, false), 7);
    assert_eq!(
        (faint_cursor.fg, faint_cursor.bg),
        (Color::LightRed, Color::DarkGray),
        "under the inactive cursor, which keeps the colors"
    );
}

#[test]
fn the_status_line_shows_every_byte_of_a_file_at_its_right() {
    let mut panel = loaded(
        "/srv",
        vec![
            entry("docs", FileKind::Dir, 4096),
            entry("disk.img", FileKind::File, 3_221_225_472),
        ],
    );
    let status = |panel: &mut Panel, width: u16| {
        let screen = draw(panel, width, 6, true).to_string();
        let line = screen.lines().nth(4).unwrap_or_default().to_owned();
        line.trim_matches('"').chars().skip(1).collect::<String>()
    };
    panel.handle(Action::End);
    assert!(
        status(&mut panel, 40).ends_with(" 3221225472║"),
        "{}",
        status(&mut panel, 40)
    );
    let narrow = status(&mut panel, 19);
    assert!(
        narrow.starts_with("disk.img") && !narrow.contains('3'),
        "{narrow}"
    );
    panel.handle(Action::Up);
    let directory = status(&mut panel, 40);
    assert!(!directory.contains("4096"), "{directory}");
}

#[test]
fn column_lines_leave_the_title_whole() {
    let mut panel = loaded("/srv/some-dir", listing());
    let text = draw(&mut panel, 40, 12, true).to_string();
    let top = text.lines().next().unwrap_or_default();
    assert!(top.contains(" /srv/some-dir "), "{text}");
    assert_eq!(top.matches('╤').count(), 1, "only past the title: {text}");
}

#[test]
fn mc_classic_colors_panels_as_mc_does() {
    use ratatui::style::Color;

    let mut panel = loaded("/srv", varied());
    panel.handle(Action::Down);
    let hosts = |_: &str| HostState::default();
    let theme = Theme::mc_classic();
    let terminal = render_themed(
        &mut panel,
        (40, 12),
        true,
        &hosts,
        Decor::new(false),
        &theme,
    );
    let buffer = terminal.backend().buffer();
    let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
    // Rows: frame, header, `..`, Adir (cursor), dir1, .env, a.txt, b.md.
    assert_eq!(colors(0, 0), (Color::Gray, Color::Blue), "frame");
    assert_eq!(colors(1, 1), (Color::LightYellow, Color::Blue), "header");
    assert_eq!(colors(1, 3), (Color::Black, Color::Cyan), "the cursor row");
    assert_eq!(colors(30, 3), (Color::Black, Color::Cyan), "all of it");
    assert_eq!(colors(1, 4), (Color::White, Color::Blue), "a directory");
    assert_eq!(colors(1, 6), (Color::Gray, Color::Blue), "a file");
    assert_eq!(colors(30, 4), (Color::Gray, Color::Blue), "other columns");
    assert_eq!(
        colors(2, 0),
        (Color::Black, Color::Cyan),
        "the active title"
    );
}

#[test]
fn draws_icons_with_ui_icons() {
    let mut panel = loaded("/srv", listing());
    let hosts = |_: &str| HostState::default();
    let terminal = render_with(&mut panel, (40, 10), true, &hosts, Decor::new(true));
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn icons_are_dimmed_names_in_their_colors() {
    use ratatui::style::{Color, Modifier};

    let mut panel = loaded("/srv", varied());
    let hosts = |_: &str| HostState::default();
    let theme = Theme::mc_classic();
    let terminal = render_themed(
        &mut panel,
        (40, 12),
        false,
        &hosts,
        Decor::new(true),
        &theme,
    );
    let buffer = terminal.backend().buffer();
    let look = |x: u16, y: u16| {
        let cell = &buffer[(x, y)];
        (cell.fg, cell.modifier.contains(Modifier::DIM))
    };
    // Rows: frame, header, `..`, Adir, dir1, .env, a.txt, b.md.
    assert_eq!(look(1, 2), (Color::White, true), "the icon of `..`");
    assert_eq!(look(1, 3), (Color::White, true), "the icon of a directory");
    assert_eq!(look(3, 3), (Color::White, false), "its name");
    assert!(
        !buffer[(1, 3)].modifier.contains(Modifier::BOLD),
        "not bold"
    );
    assert!(
        buffer[(3, 3)].modifier.contains(Modifier::BOLD),
        "unlike the name"
    );
    assert_eq!(look(1, 6), (Color::Gray, true), "the icon of a file");
    assert_eq!(look(3, 6), (Color::Gray, false), "its name");
}

#[test]
fn the_inactive_panel_shows_its_cursor_faintly_and_on_screen() {
    use ratatui::style::Modifier;
    let many: Vec<DirEntry> = (0..30)
        .map(|index| entry(&format!("file{index:02}"), FileKind::File, 1))
        .collect();
    let mut panel = loaded("/srv", many);
    panel.handle(Action::End);
    let backend = draw(&mut panel, 40, 10, false);
    assert!(panel.window.top() > 0, "scrolled to the cursor");
    let buffer = backend.buffer();
    let row_of = |text: &str| {
        (0..buffer.area.height).find(|&y| {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            line.contains(text)
        })
    };
    let cursor_row = row_of("file29").unwrap();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let modifier = buffer[(x, y)].modifier;
            let on_cursor = y == cursor_row && x > 0 && x < buffer.area.width - 1;
            assert_eq!(
                modifier.contains(Modifier::REVERSED | Modifier::DIM),
                on_cursor,
                "({x}, {y}): only the cursor's row, dimmed, and no title highlight"
            );
        }
    }
}

#[test]
fn the_inactive_cursor_keeps_the_colors_of_the_names() {
    let mut panel = loaded(
        "/srv",
        vec![
            entry("docs", FileKind::Dir, 0),
            entry("notes", FileKind::File, 1),
        ],
    );
    panel.handle(Action::Down);
    let theme = Theme::mc_classic();
    let terminal = render_themed(
        &mut panel,
        (40, 8),
        false,
        &|_| HostState::default(),
        Decor::new(false),
        &theme,
    );
    let buffer = terminal.backend().buffer();
    let (width, height) = (buffer.area.width, buffer.area.height);
    let (x, y) = (0..height)
        .find_map(|y| {
            let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
            let x = line[..line.find("docs")?].chars().count();
            Some((u16::try_from(x).ok()?, y))
        })
        .unwrap();
    let cell = &buffer[(x, y)];
    assert_eq!(cell.fg, Color::White, "the directory's color");
    assert_eq!(cell.bg, Color::DarkGray, "the inactive cursor's background");
    assert_eq!(buffer[(x, y + 1)].bg, Color::Blue, "the next row is plain");
}
