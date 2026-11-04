use super::*;

#[test]
fn f3_views_files_and_opens_directories() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
    );
    app.handle(action(Action::End));
    let Effect::Read(ReadRequest {
        id,
        location,
        host,
        cancel,
        ..
    }) = one(app.handle(action(Action::View)))
    else {
        panic!("expected a read");
    };
    assert_eq!((location, host.is_none()), (local("/srv/notes"), true));
    assert_eq!(app.context(), Context::Viewer);
    // While it is read, the dots stand in place of its icon, and its window shows how far.
    let text = screen(&mut app);
    let rows: Vec<&str> = text.lines().map(|row| row.trim_matches('"')).collect();
    assert!(rows[0].starts_with(" … /srv/notes "), "{text}");
    assert!(text.contains("Reading") && !text.contains("---"), "{text}");
    app.read(id + 1, Ok(whole(*b"not this")));
    app.read(id, Ok(whole(*b"hello\nworld\n")));
    let text = screen(&mut app);
    assert!(text.starts_with("\"   /srv/notes "), "{text}");
    assert!(text.contains("hello") && !text.contains("---"), "{text}");
    assert!(text.contains("2Wrap") && text.contains("10Quit"), "{text}");

    // F1 shows the help over it, and Esc goes back to it.
    app.handle(action(Action::Help));
    assert_eq!(app.context(), Context::Help);
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Viewer);
    app.handle(action(Action::Quit));
    assert_eq!(app.context(), Context::Panel);
    assert!(cancel.is_cancelled(), "a read in flight stops");
    assert!(!app.quits(), "Quit closes the viewer, not the app");

    // On a directory, F3 opens it.
    app.handle(action(Action::Home));
    app.handle(action(Action::Down));
    let Effect::List { request, .. } = one(app.handle(action(Action::View))) else {
        panic!("expected a listing");
    };
    assert_eq!(request.location, local("/srv/sub"));
}

#[test]
fn a_file_viewed_from_a_line_shows_it_at_the_top() {
    let mut app = with_files(&["a1"]);
    let effects = app.view_file(local("/srv/a1"), &local("/srv"), Some(3));
    let Some(Effect::Read(ReadRequest { id, .. })) = effects.first() else {
        panic!("expected a read, got {effects:?}");
    };
    let text = (1..=40)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.read(*id, Ok(whole(text)));
    let screen = screen_of(&mut app, 12);
    assert!(
        screen.contains("line 3") && !screen.contains("line 2 "),
        "{screen}"
    );
}

#[test]
fn a_file_that_cannot_be_read_is_an_error() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![file("secret", 1)]));
    app.handle(action(Action::Down));
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
        panic!("expected a read");
    };
    app.read(id, Err("permission denied".to_owned().into()));
    assert_eq!(app.context(), Context::Dialog);
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot view",
            Some("/srv/secret"),
            "permission denied"
        ),
        "{text}"
    );
    // Retry views it again.
    let Effect::Read(ReadRequest { location, .. }) = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected a read");
    };
    assert_eq!(location, local("/srv/secret"));
    assert_eq!(app.context(), Context::Viewer);
}

#[test]
fn an_error_on_a_host_that_is_not_connected_offers_to_connect() {
    let (mut app, connection) = on_a_host();
    // F8 on the host, whose connection is lost while it asks: Connect connects, and the
    // user deletes again.
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    app.closed("web", connection, Some("Broken pipe"));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    app.handle(action(Action::Cancel));
    let text = screen_of(&mut app, 16);
    assert!(text.contains("Cannot delete"), "{text}");
    assert!(text.contains("The connection is closed"), "{text}");
    assert!(text.contains("{ Connect } [ Cancel ]"), "{text}");
    let Effect::Connect { connection, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected a connection");
    };
    assert!(app.dialogs.is_empty(), "nothing more");
    // Once connected, it deletes.
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    let deletes = effects
        .iter()
        .any(|effect| matches!(effect, Effect::Delete { .. }));
    assert!(deletes, "{effects:?}");
    assert!(app.pending.is_empty());

    // What waits for a host goes once the host does.
    app.pending.push(Again::Delete {
        dir: remote("web", "/home/deploy"),
        names: vec![b"notes".to_vec()],
    });
    app.disconnect("web");
    assert!(app.pending.is_empty());
}

#[test]
fn a_rename_changes_and_a_write_retries_and_cancel_does_nothing() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
    let panel = app.shown(Side::Left);
    let (from, to) = (local("/srv/a"), local("/srv/b"));
    let busy = || Err(Refused::Failed("Busy".to_owned().into()));
    // A rename that failed: Change… opens its field again, with the name typed.
    app.entry_renamed(panel, &from, &to, busy());
    assert!(screen_of(&mut app, 16).contains("{ Change… } [ Cancel ]"));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.context(), Context::Rename);
    let typed = app
        .panel_mut(Side::Left)
        .rename_field()
        .map(|field| field.text().to_owned());
    assert_eq!(typed.as_deref(), Some("b"));
    let again = one(app.handle(action(Action::Confirm)));
    assert!(
        matches!(&again, Effect::Rename { from: f, to: t, replace: false, .. }
            if *f == from && *t == to),
        "{again:?}"
    );
    app.entry_renamed(panel, &from, &to, busy());
    assert!(app.handle(action(Action::Cancel)).is_empty());
    assert!(app.dialogs.is_empty());

    // A save that failed writes the same bytes again.
    let location = local("/srv/SUMS");
    let write = one(app.write_file(location.clone(), b"x".to_vec(), false));
    assert!(matches!(write, Effect::WriteFile { .. }));
    app.written(&location, busy());
    let Effect::WriteFile {
        location: again,
        bytes,
        ..
    } = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected the file to be written again");
    };
    assert_eq!((again, bytes), (location, b"x".to_vec()));
}

/// The viewer of `/srv/big` with the read it starts.
fn view_big() -> (App, Effect) {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![file("big", 1000)]));
    app.handle(action(Action::Down));
    let read = one(app.handle(action(Action::View)));
    (app, read)
}

/// The part of `/srv/big`, 1,000 bytes long, that `want` asks for, holding `bytes`.
fn part_of_big(want: Want, bytes: &[u8]) -> ViewerPart {
    let stamp = Stamp {
        size: Some(1000),
        modified: None,
    };
    ViewerPart::new(want, bytes, (false, false), stamp)
}

#[test]
fn a_read_that_takes_long_shows_a_window_that_stops_it() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest {
        id,
        want,
        stop,
        cancel,
        ..
    }) = read
    else {
        panic!("expected a read");
    };
    assert_eq!(want.offset, 0);
    let text = screen_of(&mut app, 16);
    assert!(text.contains("Reading") && text.contains("Stop"), "{text}");
    assert_eq!(app.context(), Context::Dialog);
    assert_eq!(app.place().0, Mode::Normal);
    app.read_progress(
        id,
        ReadProgress {
            read: 250,
            total: 1000,
            size: Some(1000),
        },
    );
    let text = screen_of(&mut app, 16);
    assert!(text.contains("25%"), "{text}");

    // Esc stops the read and keeps what it has read.
    app.handle(action(Action::Cancel));
    assert!(stop.is_cancelled() && !cancel.is_cancelled());
    assert_eq!(app.context(), Context::Viewer);
    let mut part = part_of_big(want, b"one\ntwo\n");
    part.stopped = true;
    assert!(
        app.read(id, Ok(part)).is_empty(),
        "a stopped read starts no other"
    );
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("two") && text.contains("more below"),
        "{text}"
    );
    assert!(!text.contains("Reading"), "{text}");
}

#[test]
fn the_window_of_a_read_ends_full_and_then_goes() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { id, want, .. }) = read else {
        panic!("expected a read");
    };
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    screen_at(&mut app, 16, at(0));
    let progress = ReadProgress {
        read: 980,
        total: 1000,
        size: Some(1000),
    };
    app.read_progress(id, progress);
    let text = screen_at(&mut app, 16, at(200));
    assert!(text.contains("98%"), "{text}");

    // The part comes before a report of its end: the window shows it full, over the text,
    // and leaves the keys to the viewer.
    app.read(id, Ok(part_of_big(want, b"one\ntwo\n")));
    let text = screen_at(&mut app, 16, at(300));
    assert!(text.contains("100%") && text.contains("two"), "{text}");
    assert_eq!(app.context(), Context::Viewer);
    let goes = app.spin_due(at(300)).unwrap();
    assert_eq!(goes, at(300) + spinner::DELAY);
    let text = screen_at(&mut app, 16, goes);
    assert!(!text.contains("Reading"), "{text}");
    assert_eq!(app.spin_due(goes), None);
}

#[test]
fn close_in_the_window_of_a_read_closes_the_viewer() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { cancel, .. }) = read else {
        panic!("expected a read");
    };
    screen_of(&mut app, 16);
    app.handle(action(Action::NextField));
    app.handle(action(Action::Confirm));
    assert_eq!(app.context(), Context::Panel);
    assert!(cancel.is_cancelled());
}

#[test]
fn the_viewer_reads_the_next_part_at_the_end_and_keeps_its_text_if_it_fails() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { id, want, .. }) = read else {
        panic!("expected a read");
    };
    assert!(
        app.read(id, Ok(part_of_big(want, b"one\ntwo\n")))
            .is_empty()
    );
    screen_of(&mut app, 16);
    let Effect::Read(ReadRequest { id, want, .. }) = one(app.handle(action(Action::End))) else {
        panic!("expected the next part");
    };
    assert_eq!((want.offset, want.new), (8, true));
    assert!(
        app.read(id, Err("broken pipe".to_owned().into()))
            .is_empty()
    );
    let text = screen_of(&mut app, 16);
    assert!(
        shows_problem(
            &text,
            "Cannot read more of",
            Some("/srv/big"),
            "broken pipe"
        ),
        "{text}"
    );
    // Retry reads the same part again.
    let Effect::Read(ReadRequest {
        id, want: again, ..
    }) = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected the part again");
    };
    assert_eq!(again, want);
    app.read(id, Err("broken pipe".to_owned().into()));
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Viewer);
    let text = screen_of(&mut app, 16);
    assert!(text.contains("two"), "{text}");
}

#[test]
fn a_file_that_changed_between_parts_is_read_again() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { id, want, .. }) = read else {
        panic!("expected a read");
    };
    assert!(
        app.read(id, Ok(part_of_big(want, b"one\ntwo\n")))
            .is_empty()
    );
    screen_of(&mut app, 16);
    let Effect::Read(ReadRequest { id, want, .. }) = one(app.handle(action(Action::End))) else {
        panic!("expected the next part");
    };
    let mut part = part_of_big(want, b"three\n");
    part.stamp.size = Some(2000);
    let Effect::Read(ReadRequest { want, .. }) = one(app.read(id, Ok(part))) else {
        panic!("expected the file again");
    };
    assert_eq!(want.offset, 0);
    let text = screen_of(&mut app, 16);
    assert!(text.contains("/srv/big changed"), "{text}");
}
