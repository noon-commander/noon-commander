use super::*;

#[test]
fn ctrl_x_g_d_goes_in_the_other_panel_from_where_it_is() {
    let mut app = loaded();
    app.handle(action(Action::GoToDir));
    let text = screen_of(&mut app, 16);
    assert!(in_the_middle(&text, " Go to directory "), "{text}");
    app.handle(action(Action::Cancel));

    assert!(on_other(&mut app, Action::GoToDir).is_empty());
    let text = screen_of(&mut app, 16);
    assert!(
        in_the_middle(&text, " Go to directory "),
        "there too: {text}"
    );
    type_text(&mut app, "right");
    let effects = keys(&mut app, &["enter"]);
    let [Effect::List { panel, request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(
        (panel.side, &request.location),
        (Side::Right, &local("/srv/right"))
    );
    answer(&mut app, effects, &Listing::Dir(vec![dir("deeper")]));
    assert_eq!(
        app.active,
        Side::Left,
        "the keys come back to the active panel"
    );

    // `-` and relative paths read from the other panel's directory.
    on_other(&mut app, Action::GoToDir);
    type_text(&mut app, "deeper");
    let effects = keys(&mut app, &["enter"]);
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(request.location, local("/srv/right/deeper"));
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    on_other(&mut app, Action::GoToDir);
    type_text(&mut app, "-");
    let effects = keys(&mut app, &["enter"]);
    let [Effect::List { panel, request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(
        (panel.side, &request.location),
        (Side::Right, &local("/srv/right"))
    );
    assert_eq!(app.panel(Side::Left).location(), &local("/srv"));
}

#[test]
fn ctrl_x_g_z_jumps_in_the_other_panel() {
    let mut app = recording();
    app.config.ui.default_search_mode = SearchMode::Plain;
    on_other(&mut app, Action::GoToDir);
    type_text(&mut app, "sub");
    let effects = keys(&mut app, &["enter"]);
    answer(&mut app, effects, &Listing::Dir(Vec::new()));

    let effects = keys(&mut app, &["ctrl-x", "g", "z"]);
    let [
        Effect::ZoxideQuery {
            generation,
            exclude,
            ..
        },
    ] = &effects[..]
    else {
        panic!("expected a query, got {effects:?}");
    };
    assert_eq!(
        exclude.as_deref(),
        Some(Path::new("/srv/sub")),
        "the other panel's directory"
    );
    app.jumps(
        *generation,
        Ok(vec![Scored {
            score: 1.0,
            path: PathBuf::from("/opt/scripts"),
        }]),
    );
    let text = screen_of(&mut app, 12);
    assert!(in_the_middle(&text, " zoxide "), "{text}");
    let effects = keys(&mut app, &["enter"]);
    assert_eq!(zoxide_adds(&effects), [PathBuf::from("/opt/scripts")]);
    let effects = without_zoxide(effects);
    let [Effect::List { panel, request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(
        (panel.side, &request.location),
        (Side::Right, &local("/opt/scripts"))
    );
    assert_eq!(
        app.active,
        Side::Left,
        "the keys come back to the active panel"
    );
}

#[test]
fn ctrl_x_runs_any_command_in_the_other_panel_and_gives_the_keys_back() {
    let mut app = loaded();
    // Those of the cursor too, as if the other panel were active.
    assert!(keys(&mut app, &["ctrl-x", "j"]).is_empty());
    assert_eq!(cursor_name(&app, Side::Right), "left");
    assert_eq!(cursor_name(&app, Side::Left), "");
    assert_eq!(app.active, Side::Left, "at once, as nothing is left open");
    assert_eq!(app.returns, None);

    assert!(keys(&mut app, &["ctrl-x", "t", "n"]).is_empty());
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2, 3], 3));
    assert_eq!(tab_numbers(&app, Side::Left), (vec![1], 1));
    keys(&mut app, &["ctrl-x", "1", "g", "t"]);
    assert_eq!(
        tab_numbers(&app, Side::Right),
        (vec![2, 3], 2),
        "by its number, the count after Ctrl+x"
    );
    keys(&mut app, &["ctrl-x", "g", "shift-t"]);
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2, 3], 3));
    keys(&mut app, &["ctrl-x", "t", "c"]);
    assert_eq!(tab_numbers(&app, Side::Right), (vec![2], 2));

    keys(&mut app, &["ctrl-x", "o", "s"]);
    assert_eq!(app.panel(Side::Right).sort_action(), Action::SortBySize);
    assert_eq!(app.panel(Side::Left).sort_action(), Action::SortByName);
    let effects = keys(&mut app, &["ctrl-x", "shift-r"]);
    let [Effect::List { panel, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(panel.side, Side::Right);
    assert_eq!(app.active, Side::Left);
}

#[test]
fn ctrl_x_gives_the_keys_back_from_a_panel_that_kept_visual_mode() {
    let mut app = loaded();
    keys(&mut app, &["ctrl-w", "w", "v", "ctrl-w", "w"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Panel));
    // Its command ends in the Visual mode it kept, which j extends.
    keys(&mut app, &["ctrl-x", "j"]);
    assert_eq!(app.active, Side::Left);
    assert_eq!(app.panel(Side::Right).visual_size(), Some(1));
    assert_eq!(cursor_name(&app, Side::Right), "left");
    keys(&mut app, &["ctrl-x", "v"]);
    assert_eq!(app.active, Side::Left, "once it ends it too");
    assert!(!app.panel(Side::Right).visual());
}

#[test]
fn the_other_panel_has_the_keys_until_the_command_of_ctrl_x_ends() {
    let mut app = loaded();
    keys(&mut app, &["ctrl-x", "v", "j"]);
    assert_eq!(app.active, Side::Right);
    assert_eq!(app.place(), (Mode::Visual, Context::Panel));
    keys(&mut app, &["esc"]);
    assert_eq!(app.active, Side::Left, "once Visual mode ends");

    keys(&mut app, &["ctrl-x", "/", "r", "i"]);
    assert_eq!(app.active, Side::Right);
    keys(&mut app, &["enter"]);
    assert_eq!(app.active, Side::Left, "once quick search ends");
    assert_eq!(cursor_name(&app, Side::Right), "right");

    assert!(keys(&mut app, &["ctrl-x", "g", "d"]).is_empty());
    assert_eq!(app.active, Side::Right);
    type_text(&mut app, "right");
    let effects = keys(&mut app, &["enter"]);
    assert_eq!(app.active, Side::Left, "once its dialog closes");
    answer(&mut app, effects, &Listing::Dir(vec![file("notes.txt", 1)]));
    // Ctrl+w v there shows its directory here.
    let effects = keys(&mut app, &["ctrl-x", "ctrl-w", "v"]);
    let [Effect::List { panel, request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(
        (panel.side, &request.location),
        (Side::Left, &local("/srv/right"))
    );
    let files = vec![file("a", 1), file("b", 1)];
    answer(&mut app, effects, &Listing::Dir(files));

    // A command run in a shell: the keys come back once the terminal does.
    keys(&mut app, &["ctrl-x", "!", "l", "s", "enter"]);
    assert_eq!(app.active, Side::Right, "the terminal is the command's");
    let Some(Run {
        place: Place::Local(dir),
        ..
    }) = app.take_run()
    else {
        panic!("expected a local command");
    };
    assert_eq!(
        dir,
        Path::new("/srv/right"),
        "in the other panel's directory"
    );
    app.ran(Ok(()));
    app.come_back();
    assert_eq!(app.active, Side::Left);

    // Whatever the command does with the keys, they come back where they were.
    keys(&mut app, &["ctrl-x", "ctrl-w", "w"]);
    assert_eq!(app.active, Side::Left);

    // Esc gives Ctrl+x up, and Ctrl+x again: the keys stay here.
    let right = cursor_name(&app, Side::Right);
    keys(&mut app, &["ctrl-x", "esc", "j"]);
    assert_eq!(cursor_name(&app, Side::Left), "a");
    keys(&mut app, &["ctrl-x", "ctrl-x", "j"]);
    assert_eq!(cursor_name(&app, Side::Left), "b");
    assert_eq!(cursor_name(&app, Side::Right), right);
    assert_eq!(app.active, Side::Left);
}
