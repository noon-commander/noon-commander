use super::*;

#[test]
fn work_in_a_directory_counts_in_zoxide_once_a_visit() {
    let mut app = recording();
    // Passing through does not count.
    app.handle(action(Action::Down));
    let effects = app.handle(action(Action::Enter));
    assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
    answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
    let effects = app.handle(action(Action::Parent));
    assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
    );
    // Viewing a file does, once.
    app.handle(action(Action::End));
    let effects = app.handle(action(Action::View));
    assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);
    app.handle(action(Action::Quit));
    let effects = app.handle(action(Action::Edit));
    assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0], "once a visit");
    app.edited(Ok(false));
    // Reading the directory again is the same visit.
    let effects = app.handle(action(Action::Reload));
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
    );
    app.handle(action(Action::End));
    let effects = app.handle(action(Action::View));
    assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
    app.handle(action(Action::Quit));
    // Coming back is a new visit.
    app.handle(action(Action::Home));
    app.handle(action(Action::Down));
    let effects = app.handle(action(Action::Enter));
    answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
    let effects = app.handle(action(Action::Parent));
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
    );
    app.handle(action(Action::End));
    let effects = app.handle(action(Action::Edit));
    assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);
}

#[test]
fn jobs_count_their_source_and_a_target_a_panel_shows() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &with_zoxide());
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("left"), dir("right")]),
    );
    app.active = Side::Right;
    app.handle(action(Action::End));
    let effects = app.handle(action(Action::Enter));
    answer(&mut app, effects, &Listing::Dir(vec![file("old", 1)]));
    app.active = Side::Left;
    app.handle(action(Action::Down));
    app.handle(action(Action::Copy));
    let effects = app.handle(action(Action::Confirm));
    assert_eq!(
        zoxide_adds(&effects),
        [PathBuf::from("/srv"), PathBuf::from("/srv/right")]
    );
    copy_job(without_zoxide(effects));
    // A typed target that no panel shows does not count; the source did already.
    app.handle(action(Action::Copy));
    app.handle(action(Action::DeleteToStart));
    type_text(&mut app, "/tmp");
    let effects = app.handle(action(Action::Confirm));
    assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
    // Delete, F7, and checksums count in the other panel.
    app.active = Side::Right;
    app.handle(action(Action::End));
    app.handle(action(Action::Checksum));
    let effects = app.handle(action(Action::Confirm));
    assert_eq!(
        zoxide_adds(&effects),
        [] as [PathBuf; 0],
        "counted as the target"
    );
}

#[test]
fn mkdir_and_delete_count_and_remote_directories_never_do() {
    let mut app = recording();
    app.handle(action(Action::Mkdir));
    app.handle(action(Action::DeleteToStart));
    type_text(&mut app, "new");
    let effects = app.handle(action(Action::Confirm));
    assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);

    let mut app = recording();
    app.handle(action(Action::End));
    app.handle(action(Action::Delete));
    let effects = app.handle(action(Action::Confirm));
    assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);

    let mut app = recording();
    app.config.zoxide.record = false;
    app.handle(action(Action::End));
    let effects = app.handle(action(Action::View));
    assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0], "record is off");

    let mut app = on_a_host().0;
    app.config.zoxide.record = true;
    let here = app.panel(app.active).location().clone();
    assert!(matches!(here, Location::Remote { .. }), "{here:?}");
    assert_eq!(zoxide_adds(&app.note(&here)), [] as [PathBuf; 0]);
}

#[test]
fn alt_z_asks_zoxide_and_jumps_in_the_active_panel() {
    let mut app = recording();
    // zoxide matches the keywords.
    app.config.ui.default_search_mode = SearchMode::Plain;
    app.active = Side::Right;
    let effects = app.handle(action(Action::Jump));
    let [
        Effect::ZoxideQuery {
            generation: first,
            keywords,
            exclude,
        },
    ] = &effects[..]
    else {
        panic!("expected a query, got {effects:?}");
    };
    assert_eq!(keywords, &[] as &[String]);
    assert_eq!(exclude.as_deref(), Some(Path::new("/srv")));
    assert_eq!(
        app.place(),
        (Mode::Insert, Context::Jump),
        "it opens typing keywords"
    );
    app.handle(Resolved::Insert('s'));
    let effects = app.handle(Resolved::Insert('c'));
    let [
        Effect::ZoxideQuery {
            generation,
            keywords,
            ..
        },
    ] = &effects[..]
    else {
        panic!("expected a query, got {effects:?}");
    };
    assert_eq!(keywords, &["sc".to_owned()]);
    let found = |paths: &[&str]| {
        Ok(paths
            .iter()
            .map(|path| Scored {
                score: 1.0,
                path: PathBuf::from(path),
            })
            .collect())
    };
    app.jumps(*first, found(&["/stale"]));
    app.jumps(*generation, found(&["/home/me/src", "/opt/scripts"]));
    let text = screen_of(&mut app, 12);
    assert!(text.contains(&prompt("Jump to", "sc")), "{text}");
    assert!(
        text.contains("~/src") && text.contains("/opt/scripts") && !text.contains("/stale"),
        "{text}"
    );
    keys(&mut app, &["esc"]);
    assert_eq!(
        app.place(),
        (Mode::Normal, Context::Jump),
        "Esc goes to the rows"
    );
    keys(&mut app, &["j"]);
    let effects = app.handle(action(Action::Confirm));
    assert_eq!(app.context(), Context::Panel);
    assert_eq!(zoxide_adds(&effects), [PathBuf::from("/opt/scripts")]);
    let effects = without_zoxide(effects);
    let [Effect::List { panel, request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(panel.side, Side::Right, "the active panel");
    assert_eq!(request.location, local("/opt/scripts"));
    answer(&mut app, effects, &Listing::Dir(vec![file("run", 1)]));
    // The jump counted the visit.
    app.handle(action(Action::End));
    let effects = app.handle(action(Action::View));
    assert_eq!(zoxide_adds(&effects), [] as [PathBuf; 0]);
    app.handle(action(Action::Quit));
    // Esc on the rows closes the window; so does a failed query, after saying why.
    app.handle(action(Action::Jump));
    app.jumps(app.jump_queries, Err("zoxide is not installed".to_owned()));
    assert!(screen_of(&mut app, 12).contains("zoxide is not installed"));
    keys(&mut app, &["esc", "esc"]);
    assert_eq!(app.context(), Context::Panel);
}

#[test]
fn in_the_fzf_mode_the_zoxide_window_asks_once_and_filters_as_fzf() {
    let mut app = recording();
    let effects = app.handle(action(Action::Jump));
    let [
        Effect::ZoxideQuery {
            generation,
            keywords,
            ..
        },
    ] = &effects[..]
    else {
        panic!("expected a query, got {effects:?}");
    };
    assert_eq!(keywords, &[] as &[String], "for every directory");
    let found = ["/srv/scripts/c", "/home/me/work/src", "/opt/www"]
        .iter()
        .map(|path| Scored {
            score: 1.0,
            path: PathBuf::from(path),
        })
        .collect();
    app.jumps(*generation, Ok(found));
    for c in "src".chars() {
        assert!(app.handle(Resolved::Insert(c)).is_empty(), "no query");
    }
    let text = screen_of(&mut app, 12);
    assert!(
        text.contains("~/work/src") && text.contains("/srv/scripts/c") && !text.contains("www"),
        "{text}"
    );
    let effects = without_zoxide(app.handle(action(Action::Confirm)));
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(
        request.location,
        local("/home/me/work/src"),
        "the best match"
    );
}
