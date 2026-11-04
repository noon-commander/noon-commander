use super::*;

#[test]
fn a_new_tab_shows_the_same_directory_at_once_and_closes_again() {
    let mut app = loaded();
    app.handle(action(Action::End));
    assert!(
        app.handle(action(Action::NewTab)).is_empty(),
        "no listing needed"
    );
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3], 3));
    assert_eq!(
        app.panel(Side::Left).name_under_cursor(),
        Some(&b"right"[..])
    );
    let text = screen(&mut app);
    assert!(text.contains(" 1 srv │ 2 srv "), "{text}");
    assert!(
        text.contains("╠ / /srv ═"),
        "the title keeps the whole path, under tees that join the line: {text}"
    );

    // Each tab keeps its own cursor.
    app.handle(action(Action::Home));
    app.handle(action(Action::PrevTab));
    assert_eq!(
        app.panel(Side::Left).name_under_cursor(),
        Some(&b"right"[..])
    );
    app.handle(action(Action::NextTab));
    assert_eq!(app.panel(Side::Left).name_under_cursor(), None, "on ..");
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2], 2), "its own tabs");

    assert!(app.handle(action(Action::CloseTab)).is_empty());
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1], 1));
    app.handle(action(Action::CloseTab));
    assert_eq!(
        tab_numbers(&app, Side::Left),
        (vec![1], 1),
        "the last one stays"
    );
    assert!(!screen(&mut app).contains(" 1 srv "), "one tab has no bar");
}

#[test]
fn listings_reach_hidden_tabs_and_closed_tabs_drop_them() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    let first = one(app.handle(action(Action::Enter)));
    // The new tab waits for the same directory, with its own request.
    let second = one(app.handle(action(Action::NewTab)));
    let (&Effect::List { panel: one_id, .. }, &Effect::List { panel: two_id, .. }) =
        (&first, &second)
    else {
        panic!("expected listings");
    };
    assert_ne!(one_id, two_id);
    assert_eq!(app.shown(Side::Left), two_id);
    answer(&mut app, vec![first], &Listing::Dir(vec![dir("inner")]));
    assert_eq!(
        app.panel_of(one_id).map(Panel::location),
        Some(&local("/srv/left")),
        "the hidden tab took its listing"
    );
    assert_eq!(
        app.panel(Side::Left).location(),
        &local("/srv"),
        "still waits"
    );

    app.handle(action(Action::CloseTab));
    answer(&mut app, vec![second], &Listing::Dir(Vec::new()));
    assert_eq!(app.shown(Side::Left), one_id);
    assert!(screen(&mut app).contains("inner"));
}

#[test]
fn hidden_tabs_read_their_directory_again_once_they_show() {
    let mut app = loaded();
    app.handle(action(Action::NewTab));
    let effects = app.reload(&local("/srv"));
    let shown: Vec<PanelId> = effects
        .iter()
        .map(|effect| match effect {
            Effect::List { panel, .. } => *panel,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(shown, [app.shown(Side::Left), app.shown(Side::Right)]);
    answer(&mut app, effects, &Listing::Dir(vec![dir("left")]));

    let effects = app.handle(action(Action::PrevTab));
    let [Effect::List { panel, request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(
        (panel.tab, &request.location),
        (1, &local("/srv")),
        "the hidden one, now"
    );
    answer(&mut app, effects, &Listing::Dir(vec![dir("left")]));
    assert!(
        app.handle(action(Action::NextTab)).is_empty(),
        "read already"
    );
}

#[test]
fn disconnecting_tones_down_hidden_tabs_on_the_host_too() {
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
        panic!("expected a connection");
    };
    connect_web(&mut app, Side::Left, connection, "/var/www");
    app.handle(action(Action::NewTab));
    app.handle(action(Action::PrevTab));
    app.disconnect("web");
    assert_eq!(app.left.len(), 2);
    for tab in app.left.iter_mut() {
        assert_eq!(tab.panel.location(), &remote("web", "/var/www"), "it stays");
        assert_eq!(tab.panel.not_connected(), Some("web"));
        tab.panel.handle(Action::End);
        assert!(tab.panel.entry_under_cursor().is_some(), "with its entries");
    }
}

#[test]
fn alt_and_a_digit_show_the_tab_of_that_number_and_alt_0_the_last() {
    let mut app = with_files(&["a", "b"]);
    keys(&mut app, &["t", "n", "t", "n"]);
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3, 4], 4));
    keys(&mut app, &["alt-1"]);
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3, 4], 1));
    keys(&mut app, &["alt-2"]);
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3, 4], 3));
    keys(&mut app, &["alt-0"]);
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3, 4], 4));
    // A number past the last tab does nothing, as `g t` after such a count.
    keys(&mut app, &["alt-1", "alt-4"]);
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3, 4], 1));
    // After Ctrl+x, the other panel's tab, and the keys come back.
    keys(&mut app, &["ctrl-x", "t", "n"]);
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2, 5], 5));
    keys(&mut app, &["ctrl-x", "alt-1"]);
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2, 5], 2));
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3, 4], 1));
    assert_eq!(app.active, Side::Left);
}

#[test]
fn a_tab_keeps_its_visual_mode_while_another_shows() {
    let mut app = with_files(&["a", "b", "c", "d"]);
    keys(&mut app, &["t", "n", "alt-1", "j", "v", "j"]);
    assert_eq!(app.place(), (Mode::Visual, Context::Panel));
    keys(&mut app, &["alt-2"]);
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3], 3));
    assert_eq!(
        app.place(),
        (Mode::Normal, Context::Panel),
        "this tab's mode"
    );
    keys(&mut app, &["g", "shift-t"]);
    assert_eq!(app.place(), (Mode::Visual, Context::Panel), "back again");
    assert_eq!(app.panel(Side::Left).visual_size(), Some(2));
    assert_eq!(cursor_name(&app, Side::Left), "b");
    // So it does while the other panel has the keys.
    keys(&mut app, &["alt-0", "ctrl-w", "w", "ctrl-w", "w", "alt-1"]);
    assert_eq!(app.place(), (Mode::Visual, Context::Panel));
    assert_eq!(app.panel(Side::Left).visual_size(), Some(2));
}

#[test]
fn the_list_of_tabs_shows_the_one_chosen() {
    let mut app = loaded();
    app.handle(action(Action::NewTab));
    app.handle(action(Action::Down));
    let effects = app.handle(action(Action::Enter));
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    app.handle(action(Action::TabList));
    let text = screen_of(&mut app, 16);
    assert!(in_the_middle(&text, " Tabs "), "{text}");
    assert!(
        text.contains("║ /srv ") && text.contains("║ /srv/left "),
        "{text}"
    );
    app.handle(action(Action::Up));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3], 1));
}

#[test]
fn the_tab_menu_closes_tabs_and_stays_open() {
    let mut app = loaded();
    app.handle(action(Action::NewTab));
    app.handle(action(Action::NewTab));
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3, 4], 4));
    app.handle(action(Action::TabList));
    let fkey = |app: &App| app.fkey_actions()[7].as_ref().map(|(action, _)| *action);
    assert_eq!(fkey(&app), Some(Action::CloseTab), "F8");
    // A tab that does not show goes; the one that shows stays.
    app.handle(action(Action::Home));
    assert!(app.handle(action(Action::CloseTab)).is_empty());
    assert_eq!(tab_numbers(&app, Side::Left), (vec![3, 4], 4));
    assert_eq!(app.context(), Context::TabMenu, "the menu stays");
    // The one that shows goes, and the one before it shows.
    app.handle(action(Action::End));
    app.handle(action(Action::CloseTab));
    assert_eq!(tab_numbers(&app, Side::Left), (vec![3], 3));
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("║ /srv ") && !text.contains("║ /srv/"),
        "{text}"
    );
    // The last one stays, and F8 does nothing for it.
    assert!(app.handle(action(Action::CloseTab)).is_empty());
    assert_eq!(tab_numbers(&app, Side::Left), (vec![3], 3));
    assert_eq!(fkey(&app), None);
    assert_eq!(app.active, Side::Left);
}

#[test]
fn the_list_of_the_other_panels_tabs_gives_the_keys_back() {
    let mut app = loaded();
    app.handle(action(Action::NewTab));
    on_other(&mut app, Action::TabList);
    assert_eq!(app.context(), Context::TabMenu);
    let text = screen_of(&mut app, 16);
    assert!(
        in_the_middle(&text, " Tabs "),
        "centered over both panels, as for the active one: {text}"
    );
    assert!(keys(&mut app, &["enter"]).is_empty());
    assert!(app.tab_menu.is_none());
    assert_eq!(
        app.active,
        Side::Left,
        "the keys come back to the active panel"
    );
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2], 2));
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1, 3], 3));
}

#[test]
fn tabs_go_in_the_frame_where_the_config_says() {
    let mut config = config();
    config.ui.tab_bar = TabBar::Frame;
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config);
    answer(&mut app, effects, &Listing::Dir(vec![dir("docs")]));
    app.handle(action(Action::Down));
    let effects = app.handle(action(Action::Enter));
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    app.handle(action(Action::NewTab));
    let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
    terminal
        .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn tabs_in_the_frame_keep_the_column_lines_joined_to_it() {
    let mut config = config();
    config.ui.tab_bar = TabBar::Frame;
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config);
    answer(&mut app, effects, &Listing::Dir(vec![dir("docs")]));
    app.handle(action(Action::NewTab));
    let mut terminal = Terminal::new(TestBackend::new(100, 7)).unwrap();
    terminal
        .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
        .unwrap();
    let text = terminal.backend().to_string();
    let top = text.lines().next().unwrap_or_default();
    let left: String = top.chars().skip(1).take(50).collect();
    assert!(left.contains(" 1 srv ═ 2 /srv "), "{text}");
    assert_eq!(left.matches('╤').count(), 2, "{text}");
}

#[test]
fn the_panel_menus_open_and_close_tabs_on_their_side() {
    let mut app = loaded();
    assert!(app.run(Command::On(Side::Right, Action::NewTab)).is_empty());
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2, 3], 3));
    assert_eq!(app.active, Side::Left, "the keys stay");
    let status = app.command_status(Command::On(Side::Left, Action::CloseTab));
    assert!(!status.enabled, "one tab on the left");
    let status = app.command_status(Command::On(Side::Right, Action::CloseTab));
    assert!(status.enabled);
    assert_eq!(
        status.key.as_deref(),
        Some("Ctrl+x t c"),
        "keys act on the active panel, and after Ctrl+x on the other"
    );
    app.run(Command::On(Side::Right, Action::CloseTab));
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2], 2));
}

#[test]
fn a_line_of_many_tabs_parts_the_sides_and_marks_those_left_out() {
    let mut app = loaded();
    for _ in 0..6 {
        app.handle(action(Action::NewTab));
    }
    app.active = Side::Right;
    for _ in 0..6 {
        app.handle(action(Action::NewTab));
    }
    for _ in 0..3 {
        app.handle(action(Action::PrevTab));
    }
    let mut terminal = Terminal::new(TestBackend::new(80, 4)).unwrap();
    terminal
        .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}
