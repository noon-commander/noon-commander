use super::*;

#[test]
fn slash_searches_as_vim_does_and_enter_ends_on_the_match() {
    let mut app = loaded();
    assert!(keys(&mut app, &["a"]).is_empty());
    assert_eq!(app.context(), Context::Panel, "typing does nothing");
    assert!(keys(&mut app, &["/", "r"]).is_empty());
    assert_eq!(app.context(), Context::QuickSearch);
    let shown = screen(&mut app);
    assert!(shown.contains(&prompt("Search", "r")));
    assert_eq!(mode_of(&mut app), "INSERT · Quick search");
    // Enter ends the search on what it found, as in vim; l opens it.
    assert!(keys(&mut app, &["enter"]).is_empty());
    assert_eq!(app.context(), Context::Panel);
    let Effect::List { request, .. } = one(keys(&mut app, &["l"])) else {
        panic!("expected a listing");
    };
    assert_eq!(request.location, local("/srv/right"));

    let mut app = loaded();
    app.handle(action(Action::QuickSearch));
    assert_eq!(
        app.context(),
        Context::QuickSearch,
        "/ starts an empty search"
    );
    app.handle(Resolved::Insert('l'));
    app.handle(action(Action::Backspace));
    assert!(screen(&mut app).contains(&prompt("Search", "")));
    assert!(app.handle(action(Action::Cancel)).is_empty());
    assert_eq!(app.context(), Context::Panel, "Esc ends it");
}

#[test]
fn quick_search_edits_its_text_with_the_keys_of_text_fields() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let names = ["bar", "bin", "boot", "dev"];
    answer(&mut app, effects, &Listing::Dir(names.map(dir).to_vec()));
    app.config.ui.default_search_mode = SearchMode::Plain;
    let under = |app: &App| cursor_name(app, Side::Left);
    app.handle(action(Action::QuickSearch));
    for c in "bo".chars() {
        app.handle(Resolved::Insert(c));
    }
    assert_eq!(under(&app), "boot");
    // Home goes to the start of the text, not to the first row, and the search goes on.
    app.handle(action(Action::TextStart));
    assert_eq!(under(&app), "boot");
    assert_eq!(app.context(), Context::QuickSearch);
    app.handle(action(Action::DeleteChar));
    assert!(screen(&mut app).contains(&prompt("Search", "o")));
    app.handle(action(Action::DeleteToEnd));
    assert!(screen(&mut app).contains(&prompt("Search", "")));
    assert_eq!(app.context(), Context::QuickSearch, "still searching");
    app.handle(Resolved::Insert('d'));
    assert_eq!(under(&app), "dev");
    // Up ends the search and moves the cursor, as in mc.
    keys(&mut app, &["up"]);
    assert_eq!(app.context(), Context::Panel);
    assert_eq!(under(&app), "boot");

    // Abort goes back to where the search started, and keeps nothing for n.
    app.handle(action(Action::QuickSearch));
    for c in "ba".chars() {
        app.handle(Resolved::Insert(c));
    }
    assert_eq!(under(&app), "bar");
    app.handle(action(Action::Abort));
    assert_eq!(app.context(), Context::Panel);
    assert_eq!(under(&app), "boot");
    app.handle(action(Action::NextMatch));
    assert_eq!(under(&app), "dev", "the search before: ba was given up");
}

#[test]
fn n_and_shift_n_repeat_the_last_quick_search_and_ctrl_g_brings_it_back() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let names = ["bar", "bin", "boot", "dev"];
    answer(&mut app, effects, &Listing::Dir(names.map(dir).to_vec()));
    app.config.ui.default_search_mode = SearchMode::Plain;
    let under = |app: &App| cursor_name(app, Side::Left);
    app.handle(action(Action::NextMatch));
    assert_eq!(under(&app), "", "no search yet: the cursor stays on ..");

    app.handle(action(Action::QuickSearch));
    app.handle(Resolved::Insert('b'));
    assert_eq!(under(&app), "bar");
    app.handle(action(Action::PrevMatch));
    assert_eq!(under(&app), "boot", "back, round to the bottom");
    assert_eq!(app.context(), Context::QuickSearch, "still searching");
    app.handle(action(Action::Cancel));
    for (key, name) in [
        (Action::NextMatch, "bar"),
        (Action::NextMatch, "bin"),
        (Action::PrevMatch, "bar"),
        (Action::PrevMatch, "boot"),
    ] {
        app.handle(action(key));
        assert_eq!(under(&app), name, "{key:?}");
    }
    assert_eq!(app.context(), Context::Panel);

    // With nothing typed, Ctrl+g searches for what the last search found.
    keys(&mut app, &["/", "ctrl-g"]);
    assert_eq!(under(&app), "bar");
    assert!(screen(&mut app).contains(&prompt("Search", "b")));
    // A key that ends a search keeps it too.
    keys(&mut app, &["i", "down"]);
    app.handle(action(Action::Home));
    app.handle(action(Action::NextMatch));
    assert_eq!(under(&app), "bin");
    app.handle(action(Action::NextMatch));
    assert_eq!(under(&app), "bin", "the only match");
}

#[test]
fn ctrl_s_ctrl_s_switches_how_one_quick_search_matches_and_n_matches_as_it_did() {
    let mut app = loaded();
    app.config.ui.default_search_mode = SearchMode::Plain;
    let start = app
        .panel(app.active)
        .name_under_cursor()
        .map(<[u8]>::to_vec);
    app.handle(action(Action::QuickSearch));
    keys(&mut app, &["ctrl-s", "ctrl-s"]);
    type_text(&mut app, "^r");
    assert_eq!(
        app.panel(app.active).name_under_cursor(),
        Some(&b"right"[..]),
        "fzf"
    );
    keys(&mut app, &["ctrl-s", "ctrl-s"]);
    let glyphs = app.theme.glyphs.search_modes;
    assert!(searching(&mut app, "^r", glyphs.regex));
    assert_eq!(
        app.panel(app.active).name_under_cursor(),
        Some(&b"right"[..]),
        "a regular expression"
    );
    keys(&mut app, &["ctrl-s", "ctrl-s"]);
    assert_eq!(
        app.panel(app.active)
            .name_under_cursor()
            .map(<[u8]>::to_vec),
        start,
        "as it is: no name holds it"
    );
    assert!(searching(&mut app, "^r", glyphs.plain), "the text stays");
    keys(
        &mut app,
        &["ctrl-s", "s", "ctrl-s", "s", "ctrl-s", "m", "enter"],
    );
    app.config.ui.default_search_mode = SearchMode::Fzf;
    app.handle(action(Action::QuickSearch));
    assert!(
        searching(&mut app, "", glyphs.regex),
        "the next search starts as the last one was, whatever the default"
    );
    let cases = app.theme.glyphs.case_modes;
    assert!(screen(&mut app).contains(&format!(" {} {} ║", cases.exact, glyphs.regex)));
    keys(&mut app, &["esc"]);
    app.handle(action(Action::Home));
    app.handle(action(Action::NextMatch));
    assert_eq!(
        app.panel(app.active).name_under_cursor(),
        Some(&b"right"[..]),
        "a regular expression, as the last search"
    );
    let last = app.last_search.as_ref().map(|(_, _, input)| input.case());
    assert_eq!(last, Some(search::Case::Match), "and its case");
}

#[test]
fn quick_search_matches_as_fzf_does_or_as_the_search_mode_says() {
    let mut app = loaded();
    app.handle(action(Action::QuickSearch));
    app.handle(Resolved::Insert('r'));
    app.handle(Resolved::Insert('t'));
    assert!(screen(&mut app).contains(&prompt("Search", "rt")));
    let Effect::List { request, .. } = one(app.handle(action(Action::Enter))) else {
        panic!("expected a listing");
    };
    assert_eq!(request.location, local("/srv/right"));

    let mut app = loaded();
    app.config.ui.default_search_mode = SearchMode::Plain;
    app.handle(action(Action::QuickSearch));
    app.handle(Resolved::Insert('r'));
    app.handle(Resolved::Insert('t'));
    assert_eq!(
        app.panel(app.active).search_text(),
        Some("r"),
        "no name holds rt: t is not typed"
    );
    assert_eq!(
        app.panel(app.active).name_under_cursor(),
        Some(&b"right"[..]),
        "the cursor stays"
    );
}
