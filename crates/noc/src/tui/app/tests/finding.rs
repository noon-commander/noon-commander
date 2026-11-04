use super::*;

/// What ripgrep found, as the search reports it.
fn findings(hits: Vec<Hit>) -> Vec<Finding> {
    hits.into_iter().map(Finding::Hit).collect()
}

/// The search the effects start, which must be one.
fn find_request(effects: &[Effect]) -> &FindRequest {
    let [Effect::Find(request)] = effects else {
        panic!("expected a search, got {effects:?}");
    };
    request
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn f_slash_lists_the_files_then_searches_as_the_text_is_typed() {
    let mut app = with_files(&["a1", "b2"]);
    let effects = keys(&mut app, &["f", "/"]);
    let request = find_request(&effects);
    assert_eq!(request.location, local("/srv"));
    assert!(request.host.is_none());
    assert_eq!(request.delay, Duration::ZERO, "the files show at once");
    assert!(request.query.lines().lists_files());
    assert_eq!(request.query.lines().max_per_file, 100);
    assert!(request.query.lines().hidden, "as the panels show them");
    assert_eq!(request.max_results, 10_000);
    let (first, cancel) = (request.generation, request.cancel.clone());
    assert_eq!(
        app.place(),
        (Mode::Insert, Context::FindInput),
        "typing in its text"
    );
    assert!(screen_of(&mut app, 24).contains("═ … Find in"));
    let effects = keys(&mut app, &["r"]);
    let request = find_request(&effects);
    assert_eq!(request.query.lines().pattern, "r");
    assert_eq!(request.delay, FIND_DELAY);
    assert!(cancel.is_cancelled(), "the search before stops");
    let generation = request.generation;
    app.found(first, vec![Finding::Hit(Hit::File(b"stale".to_vec()))]);
    app.found(
        generation,
        findings(vec![
            Hit::File(b"src/run/main.rs".to_vec()),
            Hit::Line {
                number: 3,
                text: b"fn run() {}".to_vec(),
                matches: vec![3..4],
            },
        ]),
    );
    app.find_ended(generation, Ok(FindSummary::default()));
    let text = screen_of(&mut app, 24);
    assert!(
        text.contains("Find in /srv")
            && text.contains("Text:")
            && text.contains(&format!(" {}  ║", app.theme.glyphs.search_modes.regex))
            && text.contains("1 line"),
        "{text}"
    );
    assert!(
        text.contains("src/run/main.rs") && !text.contains("stale"),
        "{text}"
    );
    // Tab goes to the files, whose globs search again.
    let effects = keys(&mut app, &["tab", "*", "."]);
    assert_eq!(effects.len(), 2);
    assert_eq!(find_request(&effects[1..]).query.lines().globs, ["*."]);
    // Esc leaves Insert mode in the field, and again closes the dialog, which stops the
    // search.
    let cancel = find_request(&effects[1..]).cancel.clone();
    keys(&mut app, &["esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::FindInput));
    keys(&mut app, &["esc"]);
    assert_eq!(app.context(), Context::Panel);
    assert!(cancel.is_cancelled());
}

#[test]
fn f_slash_on_a_host_searches_through_its_connection() {
    let mut app = at_slash();
    let effects = enter_host(&mut app, Side::Left, 1);
    let Effect::Connect { connection, .. } = one(effects) else {
        panic!("expected a connection");
    };
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    let effects = keys(&mut app, &["f", "/"]);
    let request = find_request(&effects);
    assert!(matches!(request.location, Location::Remote { .. }));
    assert!(request.host.is_some(), "its connection");
}

/// The app with the search of a tree open on `/srv`, which found `src/run/main.rs` with a
/// line 3 that matches, under the cursor, which has the focus.
#[allow(clippy::single_range_in_vec_init)]
fn found_main(app: &mut App) {
    let effects = app.handle(action(Action::FindFile));
    let generation = find_request(&effects).generation;
    app.found(
        generation,
        findings(vec![
            Hit::File(b"src/run/main.rs".to_vec()),
            Hit::Line {
                number: 3,
                text: b"fn run() {}".to_vec(),
                matches: vec![3..6],
            },
        ]),
    );
    app.find_ended(generation, Ok(FindSummary::default()));
    // Past the two fields.
    keys(app, &["tab", "tab", "down"]);
    assert_eq!(app.context(), Context::Find);
}

#[test]
fn enter_on_a_file_found_goes_there_and_the_jump_list_keeps_where_the_panel_was() {
    let mut app = with_files(&["a1", "b2"]);
    found_main(&mut app);
    let effects = keys(&mut app, &["enter"]);
    assert_eq!(app.context(), Context::Panel);
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(request.location, local("/srv/src/run"));
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![file("lib.rs", 1), file("main.rs", 1)]),
    );
    assert_eq!(cursor_name(&app, Side::Left), "main.rs");
    keys(&mut app, &["ctrl-o"]);
    assert_eq!(app.panel(Side::Left).heading().0, local("/srv"));
}

#[test]
fn f3_views_a_file_found_at_its_line_and_f4_edits_it_there() {
    let mut app = with_files(&["a1", "b2"]);
    found_main(&mut app);
    let effects = keys(&mut app, &["f3"]);
    let Some(Effect::Read(ReadRequest { id, location, .. })) = effects.first() else {
        panic!("expected a read, got {effects:?}");
    };
    assert_eq!(*location, local("/srv/src/run/main.rs"));
    assert_eq!(app.context(), Context::Viewer);
    let text = (1..=40)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.read(*id, Ok(whole(text)));
    let screen = screen_of(&mut app, 12);
    assert!(
        screen.contains("line 3") && !screen.contains("line 2\n"),
        "{screen}"
    );
    // The viewer goes, and the dialog is there again.
    keys(&mut app, &["q"]);
    assert_eq!(app.context(), Context::Find);
    keys(&mut app, &["f4"]);
    assert!(matches!(
        app.take_handover(),
        Some(Handover::Edit { file, line: Some(3) })
            if file == Path::new("/srv/src/run/main.rs")
    ));
}

#[test]
fn files_and_dirs_find_names_with_fd_and_enter_opens_a_directory() {
    let mut app = with_files(&["a1", "b2"]);
    keys(&mut app, &["f", "/"]);
    keys(&mut app, &["backtab", "space"]);
    assert_eq!(app.place(), (Mode::Normal, Context::ComboBox));
    assert!(
        mode_of(&mut app).contains("Combo box"),
        "the line of the mode"
    );
    let effects = keys(&mut app, &["j", "enter"]);
    let request = find_request(&effects);
    let FindQuery::Names(query) = &request.query else {
        panic!("expected a search by name, got {:?}", request.query);
    };
    assert!(query.pattern.is_empty() && query.hidden);
    let generation = request.generation;
    let found = Finding::Entry {
        path: b"src/run".to_vec(),
        dir: true,
        matches: Vec::new(),
    };
    app.found(generation, vec![found]);
    app.find_ended(generation, Ok(FindSummary::default()));
    // Past the combo box and the fields, to the rows.
    keys(&mut app, &["tab", "tab", "tab", "tab"]);
    let effects = keys(&mut app, &["enter"]);
    assert_eq!(app.context(), Context::Panel);
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(request.location, local("/srv/src/run"));
}

#[test]
fn f_f_opens_the_search_by_name_typing_in_its_text() {
    let mut app = with_files(&["a1", "b2"]);
    let effects = keys(&mut app, &["f", "f"]);
    let request = find_request(&effects);
    let FindQuery::Names(query) = &request.query else {
        panic!("expected a search by name, got {:?}", request.query);
    };
    assert_eq!(query.pattern, "");
    assert_eq!(request.location, local("/srv"));
    assert_eq!(app.place(), (Mode::Insert, Context::FindInput));
    let effects = keys(&mut app, &["m"]);
    let FindQuery::Names(query) = &find_request(&effects).query else {
        panic!("expected a search by name");
    };
    assert_eq!(query.pattern, "m");
}

#[test]
fn the_wheel_scrolls_the_rows_of_the_search_wherever_the_focus_is() {
    let mut app = with_files(&["a1"]);
    let effects = app.handle(action(Action::FindFile));
    let generation = find_request(&effects).generation;
    let hits = (0..60)
        .map(|n| Finding::Hit(Hit::File(format!("f{n:02}").into_bytes())))
        .collect();
    app.found(generation, hits);
    app.find_ended(generation, Ok(FindSummary::default()));
    assert!(screen_of(&mut app, 24).contains("f00"));
    press(&mut app, Press::WheelDown, 40, 12);
    let text = screen_of(&mut app, 24);
    assert!(!text.contains("f00") && text.contains("f10"), "{text}");
    assert_eq!(app.context(), Context::FindInput, "the focus stays");
}
