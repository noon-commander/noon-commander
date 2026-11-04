use super::*;

#[test]
fn a_file_edited_at_a_line_hands_the_line_to_the_editor() {
    let mut app = with_files(&["a1"]);
    app.edit_file(local("/srv"), b"a1", Some(7));
    assert!(matches!(
        app.take_handover(),
        Some(Handover::Edit { file, line: Some(7) }) if file == Path::new("/srv/a1")
    ));
}

#[test]
fn f4_edits_local_files_where_they_are() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
    );
    app.handle(action(Action::Down));
    assert!(
        app.handle(action(Action::Edit)).is_empty(),
        "not a directory"
    );
    assert_eq!(app.take_edit(), None);
    app.handle(action(Action::End));
    assert!(app.handle(action(Action::Edit)).is_empty());
    assert_eq!(app.take_edit(), Some(PathBuf::from("/srv/notes")));
    assert_eq!(app.take_edit(), None, "once");
    assert_eq!(app.edited(Ok(true)).len(), 2, "both panels read /srv again");

    app.handle(action(Action::Edit));
    app.take_edit();
    app.edited(Err("cannot run nano: not found".to_owned().into()));
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot edit",
            Some("/srv/notes"),
            "cannot run nano: not found"
        ),
        "{text}"
    );
    // Retry runs the editor again; Cancel ends the edit, and the panels read /srv again.
    assert!(text.contains("{ Retry } [ Cancel ]"), "{text}");
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.take_edit(), Some(PathBuf::from("/srv/notes")));
    app.edited(Err("cannot run nano: not found".to_owned().into()));
    assert_eq!(app.handle(action(Action::Cancel)).len(), 2);
    assert_eq!(app.take_edit(), None);
}

#[test]
fn g_x_opens_the_marked_entries_or_the_one_under_the_cursor_externally() {
    let mut app = with_files(&["a b", "c", "d"]);
    assert!(keys(&mut app, &["g", "x"]).is_empty(), "nothing on ..");
    let effects = keys(&mut app, &["j", "g", "x"]);
    let [Effect::Open(paths)] = &effects[..] else {
        panic!("expected an open, got {effects:?}");
    };
    assert_eq!(paths, &[PathBuf::from("/srv/a b")]);
    // In Visual mode, the rows selected, which become the marks.
    let effects = keys(&mut app, &["j", "shift-v", "j", "g", "x"]);
    let [Effect::Open(paths)] = &effects[..] else {
        panic!("expected an open, got {effects:?}");
    };
    assert_eq!(paths, &[PathBuf::from("/srv/c"), PathBuf::from("/srv/d")]);
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(chosen_names(&app, Side::Left), ["c", "d"]);

    app.open_failed(
        Path::new("/srv/c"),
        "xdg-open exited with exit status: 3".to_owned().into(),
    );
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot open",
            Some("/srv/c"),
            "xdg-open exited with exit status: 3"
        ),
        "{text}"
    );
}

#[test]
fn g_x_counts_in_zoxide() {
    let mut app = recording();
    let effects = keys(&mut app, &["shift-g", "g", "x"]);
    assert_eq!(zoxide_adds(&effects), [PathBuf::from("/srv")]);
}

#[test]
fn g_x_on_a_host_says_that_only_local_files_open_externally() {
    let (mut app, _) = on_a_host();
    assert!(keys(&mut app, &["g", "x"]).is_empty());
    let text = screen(&mut app);
    assert!(text.contains("Only local files open externally"), "{text}");
}

#[test]
fn f4_edits_a_copy_of_a_remote_file_and_sends_it_back() {
    let (mut app, _) = on_a_host();
    let (id, sources, target, ends, options) = copy_job(app.handle(action(Action::Edit)));
    assert_eq!(sources, [remote("web", "/home/deploy/notes")]);
    let Location::Local(copy) = target else {
        panic!("expected a local copy, got {target:?}");
    };
    assert!(copy.starts_with("/run/noc"), "{copy:?}");
    assert!(
        copy.to_string_lossy().ends_with("-notes"),
        "its own name last"
    );
    assert_eq!(ends, (true, false));
    assert!(options.overwrite && options.preserve);
    assert_eq!(app.take_edit(), None, "not before the copy is there");
    let text = screen_of(&mut app, 12);
    assert!(
        !text.contains("Background"),
        "the editor would open later: {text}"
    );

    app.job_event(id, JobEvent::Finished { complete: true });
    assert_eq!(app.take_edit(), Some(copy.clone()));
    let (id, sources, target, ends, options) = copy_job(app.edited(Ok(true)));
    assert_eq!(
        (sources, target),
        (
            vec![Location::Local(copy.clone())],
            remote("web", "/home/deploy/notes")
        )
    );
    assert_eq!(ends, (false, true));
    assert!(options.overwrite, "it goes back over the original");
    let effects = app.job_event(id, JobEvent::Finished { complete: true });
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Discard(path) if *path == copy)),
        "{effects:?}"
    );
}

#[test]
fn an_unchanged_copy_goes_and_one_that_cannot_go_back_stays() {
    let (mut app, connection) = on_a_host();
    let (id, _, target, _, _) = copy_job(app.handle(action(Action::Edit)));
    app.job_event(id, JobEvent::Finished { complete: true });
    let copy = app.take_edit().unwrap();
    assert_eq!(Location::Local(copy.clone()), target);
    let effects = app.edited(Ok(false));
    assert!(matches!(&effects[..], [Effect::Discard(path)] if *path == copy));

    // The upload fails: the copy stays, and the user hears where it is.
    let (id, _, _, _, _) = copy_job(app.handle(action(Action::Edit)));
    app.job_event(id, JobEvent::Finished { complete: true });
    let copy = app.take_edit().unwrap();
    let (id, _, _, _, _) = copy_job(app.edited(Ok(true)));
    let effects = app.job_event(id, JobEvent::Finished { complete: false });
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Discard(_)))
    );
    let text = screen_of(&mut app, 16);
    assert!(
        shows_problem(
            &text,
            "The changes did not go back to",
            Some("web:/home/deploy/notes"),
            "The copy back did not finish"
        ),
        "{text}"
    );
    assert!(text.contains(&*copy.to_string_lossy()), "{text}");
    assert!(text.contains("{ Retry } [ Cancel ]"), "{text}");
    // Retry sends it again, and it goes once it went back.
    let (id, sources, target, _, _) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(
        (sources, target),
        (
            vec![Location::Local(copy.clone())],
            remote("web", "/home/deploy/notes")
        )
    );
    let effects = app.job_event(id, JobEvent::Finished { complete: true });
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Discard(path) if *path == copy)),
        "{effects:?}"
    );

    // So does one whose host is gone by then.
    let (id, _, _, _, _) = copy_job(app.handle(action(Action::Edit)));
    app.job_event(id, JobEvent::Finished { complete: true });
    app.take_edit().unwrap();
    app.closed("web", connection, Some("Broken pipe"));
    app.handle(action(Action::Cancel));
    assert!(app.edited(Ok(true)).is_empty());
    let text = screen_of(&mut app, 16);
    assert!(text.contains("did not go back"), "{text}");
    assert!(text.contains("The connection is closed"), "{text}");
    // Connect connects, and the copy goes back once the host is connected.
    assert!(text.contains("{ Connect } [ Cancel ]"), "{text}");
    let Effect::Connect { connection, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected a connection");
    };
    assert!(app.dialogs.is_empty(), "nothing asks");
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    let copied = effects.iter().any(|effect| {
        matches!(effect, Effect::Copy { target, .. }
            if *target == remote("web", "/home/deploy/notes"))
    });
    assert!(copied, "{effects:?}");
}

#[test]
fn a_host_given_up_ends_the_jobs_of_f4() {
    // While the copy comes: the partial copy goes, and there is nothing to edit.
    let (mut app, connection) = on_a_host();
    let (_, _, target, _, _) = copy_job(app.handle(action(Action::Edit)));
    assert!(
        app.closed("web", connection, Some("Broken pipe"))
            .is_empty()
    );
    let effects = app.handle(action(Action::Cancel));
    let Location::Local(copy) = target else {
        panic!("expected a local copy");
    };
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Discard(path) if *path == copy)),
        "{effects:?}"
    );
    assert!(app.jobs.is_empty());
    assert_eq!(app.take_edit(), None);

    // While the edited copy goes back: it stays, and the user hears where.
    let (mut app, connection) = on_a_host();
    let (id, _, _, _, _) = copy_job(app.handle(action(Action::Edit)));
    app.job_event(id, JobEvent::Finished { complete: true });
    let copy = app.take_edit().unwrap();
    copy_job(app.edited(Ok(true)));
    app.closed("web", connection, Some("Broken pipe"));
    let effects = app.handle(action(Action::Cancel));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Discard(_)))
    );
    let text = screen_of(&mut app, 16);
    assert!(text.contains("did not go back"), "{text}");
    assert!(text.contains(&*copy.to_string_lossy()), "{text}");
}
