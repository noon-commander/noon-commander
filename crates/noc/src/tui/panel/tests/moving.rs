use super::*;

#[test]
fn moves_the_cursor_within_the_rows() {
    let mut panel = loaded("/srv", listing());
    panel.page = 3;
    assert_eq!(panel.handle(Action::Up), None);
    assert_eq!(panel.cursor, 0);
    panel.handle(Action::Down);
    assert_eq!(under_cursor(&panel), "alpha-link");
    panel.handle(Action::End);
    assert_eq!(under_cursor(&panel), "zeta.txt");
    panel.handle(Action::Home);
    assert_eq!(panel.cursor, 0);
}

#[test]
fn page_keys_scroll_the_rows_as_vims_and_keep_five_around_the_cursor() {
    let files = (0..29).map(|n| entry(&format!("f{n:02}"), FileKind::File, 1));
    let mut panel = loaded("/srv", files.collect());
    panel.page = 10;
    // Each key, then what a render does, as vim scrolls a window of ten lines.
    let mut press = |action| {
        panel.handle(action);
        panel.scroll(10);
        (panel.cursor, panel.window.top())
    };
    assert_eq!(press(Action::PageDown), (12, 8), "Ctrl+f");
    assert_eq!(press(Action::PageUp), (4, 0), "Ctrl+b");
    assert_eq!(press(Action::HalfPageDown), (9, 5), "Ctrl+d");
    assert_eq!(press(Action::HalfPageDown), (14, 10));
    assert_eq!(press(Action::HalfPageUp), (9, 5), "Ctrl+u");
    assert_eq!(press(Action::End), (29, 20));
    assert_eq!(press(Action::HalfPageDown), (29, 20), "the last row shows");
    assert_eq!(
        press(Action::PageDown),
        (29, 20),
        "unlike vim's, the last row stays at the bottom"
    );
    assert_eq!(press(Action::HalfPageUp), (19, 15));
    assert_eq!(press(Action::Home), (0, 0));
    for _ in 0..5 {
        press(Action::Down);
    }
    assert_eq!(
        press(Action::Down),
        (6, 1),
        "as vim, four rows below in ten"
    );
}

#[test]
fn an_empty_listing_keeps_the_cursor_on_its_first_row() {
    let mut empty = loaded("/srv", Vec::new());
    for action in [
        Action::Down,
        Action::End,
        Action::PageDown,
        Action::HalfPageDown,
    ] {
        assert_eq!(empty.handle(action), None);
        assert_eq!(empty.cursor, 0);
    }
}

#[test]
fn enters_directories_and_links_to_them_but_not_files() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::Down);
    let request = panel.handle(Action::Enter).unwrap();
    assert_eq!(request.location, local("/srv/alpha-link"));
    assert_eq!(request.generation, 2);
    let inside = vec![entry("inside", FileKind::File, 1)];
    answer(&mut panel, &request, Listing::Dir(inside));
    assert_eq!(panel.location, local("/srv/alpha-link"));
    assert_eq!(names(&panel), ["..", "inside"]);

    panel.handle(Action::End);
    assert_eq!(panel.handle(Action::Enter), None, "a file does not open");
}

#[test]
fn going_up_puts_the_cursor_on_the_directory_left() {
    let mut panel = loaded("/srv/bin", vec![entry("tool", FileKind::File, 1)]);
    let request = panel.handle(Action::Parent).unwrap();
    assert_eq!(request.location, local("/srv"));
    answer(&mut panel, &request, Listing::Dir(listing()));
    assert_eq!(under_cursor(&panel), "bin");

    // Enter on `..` does the same.
    panel.handle(Action::Home);
    let request = panel.handle(Action::Enter).unwrap();
    assert_eq!(request.location, local("/"));
    let srv = vec![entry("srv", FileKind::Dir, 1)];
    answer(&mut panel, &request, Listing::Dir(srv));
    assert_eq!(under_cursor(&panel), "srv");
}

#[test]
fn a_slash_is_the_top_without_dot_dot() {
    let mut panel = loaded("/", vec![entry("srv", FileKind::Dir, 1)]);
    assert_eq!(names(&panel), ["srv"]);
    assert_eq!(under_cursor(&panel), "srv", "the first row");
    assert_eq!(panel.handle(Action::Parent), None);
    assert_eq!(
        panel.for_other_panel(),
        Some(Destination::to(local("/srv")))
    );
    let mut empty = loaded("/", Vec::new());
    assert_eq!(empty.rows(), 0);
    assert_eq!(empty.handle(Action::Enter), None);
    assert_eq!(empty.handle(Action::Parent), None);
    assert_eq!(empty.handle(Action::End), None);
    let _ = draw(&mut empty, 40, 6, true);
}

#[test]
fn hosts_open_their_home_directory_and_lead_up_to_slash() {
    let mut panel = loaded("/srv", Vec::new());
    let request = panel.go(Destination::to(remote("staging", "")));
    let refused = "deploy@stg: Permission denied (publickey).".to_owned();
    let failure = panel
        .listed(request.generation, Err(ListError::other(refused.clone())))
        .unwrap();
    assert_eq!(panel.location, local("/srv"), "the panel stays");
    assert_eq!(failure.shown, "staging");
    assert_eq!(failure.error.reason, refused);
    assert_eq!(
        failure.parent, None,
        "the home directory has none to go up to"
    );

    // The reply names the start directory, which `..` then leads up from.
    let request = panel.go(Destination::to(remote("staging", "")));
    let home = remote("staging", "/home/ubuntu");
    let reply = Listed {
        location: home.clone(),
        listing: Listing::Dir(Vec::new()),
        space: None,
    };
    panel.listed(request.generation, Ok(reply));
    assert_eq!(panel.location, home);
    assert!(
        draw(&mut panel, 40, 6, true)
            .to_string()
            .contains("staging:/home/ubuntu")
    );
    let request = panel.handle(Action::Parent).unwrap();
    assert_eq!(request.location, remote("staging", "/home"));

    let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(Vec::new()));
    let request = panel.handle(Action::Parent).unwrap();
    assert_eq!(request.location, remote("db", "/"));
    answer(&mut panel, &request, Listing::Dir(listing()));
    assert_eq!(names(&panel)[0], "alpha-link", "no `..` in a remote `/`");
    assert_eq!(panel.handle(Action::Parent), None);
}

#[test]
fn alt_o_picks_what_the_other_panel_opens_and_moves_on() {
    let mut panel = loaded("/srv", listing());
    let up = Destination {
        location: local("/"),
        focus: Focus::Name(b"srv".to_vec()),
    };
    assert_eq!(
        panel.for_other_panel(),
        Some(up.clone()),
        "`..`: the parent"
    );
    assert_eq!(under_cursor(&panel), "alpha-link", "the cursor moves on");
    let into = Destination {
        location: local("/srv/alpha-link"),
        focus: Focus::First,
    };
    assert_eq!(panel.for_other_panel(), Some(into));
    panel.handle(Action::End);
    assert_eq!(
        panel.for_other_panel(),
        Some(up),
        "a file: the parent, as in mc"
    );
    assert_eq!(under_cursor(&panel), "zeta.txt", "the last row stays");
}

#[test]
fn here_is_the_location_and_the_row_under_the_cursor() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::End);
    let here = Destination {
        location: local("/srv"),
        focus: Focus::Near {
            name: b"zeta.txt".to_vec(),
            row: 6,
        },
    };
    assert_eq!(panel.here(), here);

    let mut other = loaded("/tmp", Vec::new());
    let request = other.go(here);
    answer(&mut other, &request, Listing::Dir(listing()));
    assert_eq!(under_cursor(&other), "zeta.txt");
}

#[test]
fn n_that_finds_nothing_leaves_the_jumps_as_they_are() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::End);
    let end = panel.cursor;
    assert!(panel.jump_back(1).is_none(), "in this directory");
    assert_eq!(panel.cursor, 0);
    assert!(
        panel
            .search_again(("nothing", quick(Way::Plain)), true, 1)
            .is_none()
    );
    assert!(panel.jump_forward(1).is_none());
    assert_eq!(panel.cursor, end, "Tab still goes forward");
}
