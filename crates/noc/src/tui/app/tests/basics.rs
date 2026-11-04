use super::*;

#[test]
fn the_location_menu_and_the_zoxide_window_remember_how_many_rows_they_had() {
    let mut app = at_slash();
    assert!(app.sizes.menu.is_none());
    menu_on(&mut app, Side::Left);
    let lines = app.menu.as_ref().and_then(LocationMenu::lines_listed);
    assert!(lines.is_some());
    assert_eq!(app.sizes.menu, lines);
    app.handle(action(Action::Cancel));
    assert!(app.menu.is_none());
    let effects = app.handle(action(Action::LocationMenu));
    let Effect::ListPlaces { generation } = one(effects) else {
        panic!("expected the places");
    };
    app.places(generation, Err("no ssh config".to_owned()));
    assert_eq!(app.sizes.menu, lines, "a failed listing keeps them");

    app.handle(action(Action::Cancel));
    app.handle(action(Action::Jump));
    assert!(app.sizes.jump.is_none());
    app.jumps(
        app.jump_queries,
        Ok(vec![Scored {
            score: 1.0,
            path: PathBuf::from("/srv"),
        }]),
    );
    assert_eq!(app.sizes.jump, Some(1));
}

#[test]
fn windows_grow_as_they_open_and_shrink_as_they_close() {
    let mut app = loaded();
    app.config.animations.windows = true;
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let panels = screen_at(&mut app, 12, at(0));
    app.handle(action(Action::Help));
    let growing = screen_at(&mut app, 12, at(1000));
    let due = app.frame_due().unwrap();
    assert!(due > at(1000) && due < at(2000), "{due:?}");
    // Keys go to the window while it grows.
    assert_eq!(app.context(), Context::Help);
    let whole = screen_at(&mut app, 12, at(2000));
    assert_ne!(growing, whole);
    assert_eq!(app.frame_due(), None);
    assert_eq!(screen_at(&mut app, 12, at(2500)), whole, "stays as it is");

    app.handle(action(Action::Cancel));
    let shrinking = screen_at(&mut app, 12, at(3000));
    assert_ne!(shrinking, panels);
    assert!(app.frame_due().is_some());
    assert_eq!(screen_at(&mut app, 12, at(4000)), panels);
    assert_eq!(app.frame_due(), None);

    // A menu of F9 drops from its bar, which shows at once.
    app.handle(action(Action::PullDown));
    screen_at(&mut app, 12, at(5000));
    app.handle(action(Action::Down));
    let dropping = screen_at(&mut app, 12, at(6000));
    assert!(dropping.contains("Command"), "{dropping}");
    assert!(!dropping.contains("Sort by name"), "{dropping}");
    let dropped = screen_at(&mut app, 12, at(7000));
    assert!(dropped.contains("Sort by name"), "{dropped}");
    app.handle(action(Action::Cancel));
    app.handle(action(Action::Cancel));
    screen_at(&mut app, 12, at(8000));

    // Turned off, windows come and go at once.
    app.config.animations.windows = false;
    app.handle(action(Action::Help));
    assert_eq!(screen_at(&mut app, 12, at(9000)), whole);
    assert_eq!(app.frame_due(), None);
}

#[test]
fn dots_alone_draw_half_as_often_as_a_shine() {
    let mut app = loaded();
    app.config.animations.spinners = true;
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let shows = at(0) + spinner::DELAY;

    // A panel reads its directory again: dots turn in its title, alone.
    let reload = app.handle(action(Action::Reload));
    assert!(matches!(&reload[..], [Effect::List { .. }]), "{reload:?}");
    screen_at(&mut app, 20, at(0));
    assert_eq!(app.spin_due(at(0)), Some(shows), "nothing shows before");
    screen_at(&mut app, 20, shows);
    assert_eq!(app.spin_due(shows), Some(shows + spinner::DOTS_FRAME));

    // The location menu waits for its places: a shine crosses the skeleton of its rows.
    let Effect::ListPlaces { .. } = one(on_other(&mut app, Action::LocationMenu)) else {
        panic!("expected the places");
    };
    let opened = shows + spinner::DOTS_FRAME;
    screen_at(&mut app, 20, opened);
    let shines = opened + spinner::DELAY;
    let text = screen_at(&mut app, 20, shines);
    assert_eq!(
        app.spin_due(shines),
        Some(shines + spinner::SHINE_STEP),
        "{text}"
    );
}

#[test]
fn lists_both_panels_at_start() {
    let (_, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let sides: Vec<Side> = effects
        .iter()
        .map(|effect| match effect {
            Effect::List {
                panel, host: None, ..
            } => panel.side,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(sides, [Side::Left, Side::Right]);
}

#[test]
fn keys_go_to_the_active_panel_and_ctrl_w_switches_it() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    let Effect::List {
        panel: PanelId { side, .. },
        request,
        ..
    } = one(app.handle(action(Action::Enter)))
    else {
        panic!("expected a listing");
    };
    assert_eq!((side, request.location), (Side::Left, local("/srv/left")));

    assert!(app.handle(action(Action::SwitchPanel)).is_empty());
    app.handle(action(Action::End));
    let Effect::List {
        panel: PanelId { side, .. },
        request,
        ..
    } = one(app.handle(action(Action::Enter)))
    else {
        panic!("expected a listing");
    };
    assert_eq!((side, request.location), (Side::Right, local("/srv/right")));

    app.handle(action(Action::SwitchPanel));
    assert_eq!(app.active, Side::Left);
}

#[test]
fn the_work_dir_follows_the_local_directory_of_the_active_panel() {
    let mut app = loaded();
    assert_eq!(app.take_work_dir(), Some(PathBuf::from("/srv")));
    assert_eq!(app.take_work_dir(), None, "only when it changes");

    app.handle(action(Action::Down));
    let effects = app.handle(action(Action::Enter));
    assert_eq!(app.take_work_dir(), None, "not before the listing arrives");
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    assert_eq!(app.take_work_dir(), Some(PathBuf::from("/srv/left")));

    app.handle(action(Action::SwitchPanel));
    assert_eq!(app.take_work_dir(), Some(PathBuf::from("/srv")));
    app.handle(action(Action::SwitchPanel));
    assert_eq!(app.take_work_dir(), Some(PathBuf::from("/srv/left")));

    let mut app = at_slash();
    app.take_work_dir();
    let effects = enter_host(&mut app, Side::Left, 1);
    let Effect::Connect { connection, .. } = one(effects) else {
        panic!("expected a connection");
    };
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    assert!(matches!(
        app.panel(Side::Left).location(),
        Location::Remote { .. }
    ));
    assert_eq!(
        app.take_work_dir(),
        None,
        "a remote panel keeps the last one"
    );
}

/// The titles of the panels drawn on the left and on the right.
fn titles(app: &mut App) -> (String, String) {
    let text = screen(app);
    let top = text.lines().next().unwrap();
    let (left, right) = top.split_once("╗╔").unwrap();
    (left.to_owned(), right.to_owned())
}

#[test]
fn ctrl_u_swaps_where_the_panels_are_and_replies_follow_them() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    let Effect::List {
        panel: PanelId { side, .. },
        request,
        ..
    } = one(app.handle(action(Action::Enter)))
    else {
        panic!("expected a listing");
    };
    assert!(app.handle(action(Action::SwapPanels)).is_empty());
    assert_eq!(app.active, Side::Left, "the active panel stays active");

    let location = request.location.clone();
    let listing = Listing::Dir(vec![dir("inner")]);
    app.listed(
        app.shown(side),
        request.generation,
        Ok(Listed {
            location,
            listing,
            space: None,
        }),
    );
    let (left, right) = titles(&mut app);
    assert!(
        right.contains("/srv/left"),
        "the panel that asked, now on the right"
    );
    assert!(!left.contains("/srv/left"), "{left}");

    app.handle(action(Action::SwapPanels));
    let (left, _) = titles(&mut app);
    assert!(left.contains("/srv/left"), "{left}");
}

#[test]
fn alt_o_and_alt_i_send_the_other_panel_here() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    let Effect::List { panel, request, .. } = one(app.handle(action(Action::OtherPanelOpen)))
    else {
        panic!("expected a listing");
    };
    assert_eq!(
        (panel.side, &request.location),
        (Side::Right, &local("/srv/left"))
    );
    assert_eq!(app.active, Side::Left);
    answer(
        &mut app,
        vec![Effect::List {
            panel,
            request,
            host: None,
        }],
        &Listing::Dir(Vec::new()),
    );

    let effects = app.handle(action(Action::OtherPanelSync));
    let [
        Effect::List {
            panel: PanelId { side, .. },
            request,
            ..
        },
    ] = &effects[..]
    else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!((*side, &request.location), (Side::Right, &local("/srv")));
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("left"), dir("right")]),
    );
    assert_eq!(
        app.panel(Side::Right).here(),
        app.panel(Side::Left).here(),
        "on the row Ctrl+w f moved on to"
    );
}

#[test]
fn quitting_stops_every_connection() {
    let mut app = at_slash();
    let stops: Vec<CancellationToken> = [(Side::Left, 1), (Side::Right, 2)]
        .into_iter()
        .map(|(side, rows)| match &enter_host(&mut app, side, rows)[..] {
            [Effect::Connect { stop, .. }] => stop.clone(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    app.disconnect_all();
    assert!(stops.iter().all(CancellationToken::is_cancelled));
    assert!(app.hosts.is_empty());
}

#[test]
fn hidden_files_switch_in_both_panels() {
    let ui = UiConfig {
        show_hidden: false,
        ..ui()
    };
    let (mut app, effects) = App::new(
        Path::new("/srv"),
        Path::new("/home/me"),
        &Config { ui, ..config() },
    );
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir(".git"), dir("src")]),
    );
    assert!(!screen(&mut app).contains(".git"));
    app.handle(action(Action::ToggleHidden));
    assert_eq!(
        screen(&mut app).matches(".git").count(),
        2,
        "in both panels"
    );
    app.handle(action(Action::ToggleHidden));
    assert!(!screen(&mut app).contains(".git"));
}

#[test]
fn focus_left_and_right_go_by_where_the_panels_are_drawn() {
    let mut app = loaded();
    app.handle(action(Action::FocusRight));
    assert_eq!(app.active, Side::Right);
    app.handle(action(Action::FocusLeft));
    assert_eq!(app.active, Side::Left);
    app.handle(action(Action::SwapPanels));
    app.handle(action(Action::FocusLeft));
    assert_eq!(
        app.active,
        Side::Right,
        "the right panel is drawn on the left"
    );
    app.handle(action(Action::FocusRight));
    assert_eq!(app.active, Side::Left);
}

#[test]
fn the_f_key_bar_colors_whole_slots() {
    use ratatui::style::Color;

    let mut app = loaded();
    let mut terminal = Terminal::new(TestBackend::new(80, 6)).unwrap();
    terminal
        .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
        .unwrap();
    let buffer = terminal.backend().buffer();
    // `10Quit` starts at column 72; its slot runs to the edge.
    assert_eq!(
        (buffer[(72, 5)].fg, buffer[(72, 5)].bg),
        (Color::White, Color::Black)
    );
    assert_eq!(
        (buffer[(74, 5)].fg, buffer[(74, 5)].bg),
        (Color::Black, Color::Cyan)
    );
    assert_eq!(buffer[(79, 5)].bg, Color::Cyan);
    assert_eq!(
        buffer[(1, 5)].bg,
        Color::Cyan,
        "an empty label is colored too"
    );
}

#[test]
fn handles_quit_redraw_and_text() {
    let mut app = loaded();
    assert!(app.handle(Resolved::Insert('q')).is_empty());
    assert!(!app.quits());
    assert!(!app.take_redraw());
    app.handle(action(Action::Redraw));
    assert!(app.take_redraw());
    assert!(!app.take_redraw(), "a redraw is requested once");
    app.handle(action(Action::Quit));
    assert!(!app.quits(), "it asks first");
    assert_eq!(app.context(), Context::Dialog);
    app.handle(action(Action::Cancel));
    assert!(!app.quits(), "Esc stays");
    app.handle(action(Action::Quit));
    app.handle(action(Action::Confirm));
    assert!(app.quits());
}

#[test]
fn every_supported_f_key_action_has_a_label() {
    let app = loaded();
    for action in app
        .keymap
        .fkeys(Mode::Normal, Context::Panel)
        .into_iter()
        .flatten()
    {
        if app.supports(action) {
            assert!(fkey_label(action).is_some(), "{action:?}");
        }
    }
}
