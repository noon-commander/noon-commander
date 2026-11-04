use super::*;

/// Types `text` into Go to directory in the active panel and presses Enter.
fn go_to_dir(app: &mut App, text: &str) -> Vec<Effect> {
    assert!(app.handle(action(Action::GoToDir)).is_empty());
    assert_eq!(app.context(), Context::PathInput);
    type_text(app, text);
    app.handle(action(Action::Confirm))
}

#[test]
fn g_d_goes_where_cd_would_and_back_with_a_dash() {
    let mut app = loaded();
    let text = {
        app.handle(action(Action::GoToDir));
        let text = screen_of(&mut app, 16);
        app.handle(action(Action::Cancel));
        text
    };
    assert!(
        text.contains("Go to directory") && text.contains("Path:"),
        "{text}"
    );

    let effects = go_to_dir(&mut app, "left/../right/./x");
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(request.location, local("/srv/right/x"));
    answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
    assert_eq!(app.panel(Side::Left).location(), &local("/srv/right/x"));

    // `..` puts the cursor on where the panel was, as Ctrl+PgUp does.
    let effects = go_to_dir(&mut app, "..");
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("w"), dir("x"), dir("y")]),
    );
    assert_eq!(app.panel(Side::Left).name_under_cursor(), Some(&b"x"[..]));

    // `-` goes back, and back again.
    let effects = go_to_dir(&mut app, "-");
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(request.location, local("/srv/right/x"));
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    let effects = go_to_dir(&mut app, "-");
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    assert_eq!(app.panel(Side::Left).location(), &local("/srv/right"));

    // `~`, an empty line, and where the panel is already.
    let [Effect::List { request, .. }] = &go_to_dir(&mut app, "~/src")[..] else {
        panic!("expected a listing");
    };
    assert_eq!(request.location, local("/home/me/src"));
    app.panel_mut(Side::Left).cancel();
    assert!(go_to_dir(&mut app, "").is_empty());
    assert!(go_to_dir(&mut app, ".").is_empty());
    assert_eq!(app.context(), Context::Panel);
}

#[test]
fn g_d_with_a_colon_leaves_a_host_for_the_last_local_directory() {
    // The left panel went to web from /.
    let (mut app, _) = on_a_host();
    let effects = go_to_dir(&mut app, ":");
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(request.location, local("/"));
    answer(&mut app, effects, &Listing::Dir(vec![dir("srv")]));
    assert_eq!(app.panel(Side::Left).location(), &local("/"));
    assert!(go_to_dir(&mut app, ":").is_empty(), "already there");

    let (mut app, _) = on_a_host();
    let [Effect::List { request, .. }] = &go_to_dir(&mut app, ":srv/www")[..] else {
        panic!("expected a listing");
    };
    assert_eq!(
        request.location,
        local("/srv/www"),
        "from the last local directory"
    );
    let (mut app, _) = on_a_host();
    let [Effect::List { request, .. }] = &go_to_dir(&mut app, ":~/src")[..] else {
        panic!("expected a listing");
    };
    assert_eq!(request.location, local("/home/me/src"));
    let (mut app, _) = on_a_host();
    let [Effect::List { request, .. }] = &go_to_dir(&mut app, "/etc")[..] else {
        panic!("expected a listing");
    };
    assert_eq!(
        request.location,
        remote("web", "/etc"),
        "without it, on the host"
    );
}

#[test]
fn g_d_completes_local_paths_after_a_colon_on_a_host() {
    let (mut app, _) = on_a_host();
    app.handle(action(Action::GoToDir));
    type_text(&mut app, ":sr");
    let (generation, dir, through, hosts) = names_request(&app.handle(action(Action::Complete)));
    assert_eq!((dir, through, hosts), (local("/"), false, false));
    app.names(generation, Ok(names_of(&[("srv", true)])), &[]);
    assert_eq!(field(&app), ":srv/|");
    let (_, dir, ..) = names_request(&app.handle(action(Action::Complete)));
    assert_eq!(dir, local("/srv"));
}

#[test]
fn alt_c_opens_hosts_and_a_path_that_is_not_there_says_so() {
    let mut app = loaded();
    let effects = go_to_dir(&mut app, "web:/var/log");
    let [Effect::Connect { host, .. }] = &effects[..] else {
        panic!("expected a connection, got {effects:?}");
    };
    assert_eq!(host, "web");
    assert_eq!(
        app.panel(Side::Left).pending_request().map(|r| r.location),
        Some(remote("web", "/var/log"))
    );

    let mut app = loaded();
    let effects = go_to_dir(&mut app, "/nowhere");
    fail(&mut app, effects, ListErrorKind::NotFound);
    assert_eq!(app.panel(Side::Left).location(), &local("/srv"), "it stays");
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot open the directory",
            Some("/nowhere"),
            "no such"
        ),
        "{text}"
    );
}

#[test]
fn a_directory_that_cannot_be_read_asks_what_to_do() {
    let mut app = loaded();
    // Cancel, last, stays; Up would lead where the panel is.
    let effects = go_to_dir(&mut app, "left");
    assert!(fail(&mut app, effects, ListErrorKind::Denied).is_empty());
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot open the directory",
            Some("left"),
            "permission denied"
        ),
        "{text}"
    );
    assert!(text.contains("{ Retry } [ Go to ] [ Cancel ]"), "{text}");
    assert!(!text.contains(" Up "), "{text}");
    app.handle(action(Action::Right));
    app.handle(action(Action::Right));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert!(app.dialogs.is_empty());
    assert_eq!(app.panel(Side::Left).location(), &local("/srv"));

    // Retry, the default, reads it again.
    let effects = go_to_dir(&mut app, "left");
    assert!(fail(&mut app, effects, ListErrorKind::Other).is_empty());
    let Effect::List { request, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected the directory again");
    };
    assert_eq!(request.location, local("/srv/left"));

    // A panel has one such dialog at a time: the last failure replaces the one before.
    let panel = app.shown(Side::Left);
    let error = |reason: &str| ListError::other(reason.to_owned());
    app.listed(panel, request.generation, Err(error("first")));
    let Some(request) = app.panel_mut(Side::Left).handle(Action::Reload) else {
        panic!("expected the directory again");
    };
    app.listed(panel, request.generation, Err(error("second")));
    assert_eq!(app.dialogs.len(), 1);
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(&text, "Cannot open the directory", Some("/srv"), "second"),
        "{text}"
    );

    // Go to opens Go to directory with the path, to mend it.
    app.handle(action(Action::Cancel));
    let effects = go_to_dir(&mut app, "/nowhere/deep");
    fail(&mut app, effects, ListErrorKind::NotFound);
    assert!(app.handle(Resolved::Hotkey('g')).is_empty());
    assert_eq!(app.context(), Context::PathInput);
    let field = app.dialogs.front().map(|open| open.dialog.text());
    assert_eq!(field, Some("/nowhere/deep"));
    assert_eq!(cd_text(&remote("web", "")), "web:");
    assert_eq!(cd_text(&remote("web", "/var")), "web:/var");
}

#[test]
fn up_climbs_to_the_nearest_directory_that_is_there() {
    let mut app = loaded();
    let effects = go_to_dir(&mut app, "/srv/x/y/z");
    answer(&mut app, effects, &Listing::Dir(Vec::new()));

    // The directory of the panel is gone: Up is the default.
    let effects = app.handle(action(Action::Reload));
    assert!(fail(&mut app, effects, ListErrorKind::NotFound).is_empty());
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot open the directory",
            Some("/srv/x/y/z"),
            "no such file or directory"
        ),
        "{text}"
    );
    assert!(
        text.contains("[ Retry ] { Up } [ Go to ] [ Cancel ]"),
        "{text}"
    );
    let effects = app.handle(action(Action::Confirm));
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected the parent, got {effects:?}");
    };
    assert_eq!(request.location, local("/srv/x/y"));
    // Past every directory that is gone too, without asking, up to one that it cannot read.
    let effects = fail(&mut app, effects, ListErrorKind::NotFound);
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected the next parent, got {effects:?}");
    };
    assert_eq!(request.location, local("/srv/x"));
    assert!(fail(&mut app, effects, ListErrorKind::Denied).is_empty());
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot open the directory",
            Some("/srv/x"),
            "permission denied"
        ),
        "{text}"
    );

    // Up again, by its letter, to /srv, on the directory it came from.
    let effects = app.handle(Resolved::Hotkey('u'));
    answer(&mut app, effects, &Listing::Dir(vec![dir("w"), dir("x")]));
    assert_eq!(app.panel(Side::Left).location(), &local("/srv"));
    assert_eq!(cursor_name(&app, Side::Left), "x");
    assert!(app.dialogs.is_empty());
}

#[test]
fn a_host_that_cannot_open_its_start_dir_names_it_for_up_and_go_to() {
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
        panic!("expected a connection");
    };
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    let fail_at_start_dir = |app: &mut App, effects: Vec<Effect>| {
        let Effect::List { panel, request, .. } = one(effects) else {
            panic!("expected a listing");
        };
        assert_eq!(request.location, remote("web", ""));
        let error = ListError {
            kind: ListErrorKind::NotFound,
            reason: "no such file or directory".to_owned(),
            code: Some(Code::Sftp(2)),
            location: Some(remote("web", "/var/www/gone")),
        };
        assert!(app.listed(panel, request.generation, Err(error)).is_empty());
    };
    fail_at_start_dir(&mut app, effects);
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot open the directory",
            Some("web:/var/www/gone"),
            "no such file or directory"
        ),
        "{text}"
    );
    assert!(text.contains(" Up "), "{text}");
    let Effect::List { request, .. } = one(app.handle(Resolved::Hotkey('u'))) else {
        panic!("expected the parent");
    };
    assert_eq!(request.location, remote("web", "/var/www"));

    app.panel_mut(Side::Left).cancel();
    let effects = enter_host(&mut app, Side::Left, 1);
    fail_at_start_dir(&mut app, effects);
    assert!(app.handle(Resolved::Hotkey('g')).is_empty());
    let field = app.dialogs.front().map(|open| open.dialog.text());
    assert_eq!(field, Some("web:/var/www/gone"));
}

#[test]
fn a_key_in_a_panel_not_connected_asks_to_connect() {
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
        panic!("expected a connection");
    };
    connect_web(&mut app, Side::Left, connection, "/var/www");
    app.disconnect("web");
    // Keys that do not need the host work as ever.
    app.handle(action(Action::SwitchPanel));
    app.handle(action(Action::SwitchPanel));
    assert_eq!(app.active, Side::Left);
    assert!(app.dialogs.is_empty());
    // So do those that look through the entries the panel kept.
    assert!(app.handle(action(Action::Down)).is_empty());
    assert!(app.handle(action(Action::Mark)).is_empty());
    assert!(app.handle(action(Action::SortBySize)).is_empty());
    assert!(app.handle(action(Action::Home)).is_empty());
    assert!(app.dialogs.is_empty());
    let panel = app.panel(Side::Left);
    let chosen: Vec<&[u8]> = panel.chosen().iter().map(|e| e.name.as_slice()).collect();
    assert_eq!(chosen, [b"app".as_slice()], "marked");

    // Any other asks; Connect, the default, reads the directory again.
    assert!(app.handle(action(Action::Enter)).is_empty());
    let text = screen_of(&mut app, 12);
    assert!(text.contains("Not connected to web"), "{text}");
    let Effect::Connect { .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected a connection");
    };
    assert_eq!(
        app.panel(Side::Left).pending_request().map(|r| r.location),
        Some(remote("web", "/var/www"))
    );
    app.handle(action(Action::Cancel));
    assert_eq!(app.panel(Side::Left).not_connected(), Some("web"));

    // Local goes to the last local directory of the panel; Cancel stays.
    app.handle(action(Action::Enter));
    assert!(app.handle(action(Action::Cancel)).is_empty());
    assert_eq!(app.panel(Side::Left).not_connected(), Some("web"));
    app.handle(action(Action::Enter));
    let Effect::List { request, .. } = one(app.handle(Resolved::Hotkey('l'))) else {
        panic!("expected a local directory");
    };
    assert_eq!(request.location, local("/"));

    // R connects at once.
    app.panel_mut(Side::Left).cancel();
    let Effect::Connect { .. } = one(app.handle(action(Action::Reload))) else {
        panic!("expected a connection");
    };
    assert!(app.dialogs.is_empty());
}
