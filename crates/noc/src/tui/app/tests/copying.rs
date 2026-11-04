use super::*;

/// An app on `/srv` with the right panel in `/srv/right` and the cursor of the left one
/// on `left`.
fn two_directories() -> App {
    let mut app = loaded();
    app.active = Side::Right;
    app.handle(action(Action::End));
    let effects = app.handle(action(Action::Enter));
    answer(&mut app, effects, &Listing::Dir(vec![file("old", 1)]));
    app.active = Side::Left;
    app.handle(action(Action::Down));
    app
}

#[test]
fn f5_copies_to_the_other_panel_and_asks_about_taken_names() {
    let mut app = two_directories();
    app.handle(action(Action::Copy));
    let text = screen(&mut app);
    assert!(text.contains("Copy \"left\" to:"), "{text}");
    assert!(text.contains("/srv/right"), "{text}");
    let checked = format!("[{}] Preserve attributes", app.theme.glyphs.check);
    assert!(text.contains(&checked), "{text}");
    let (id, sources, target, remote, options) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(sources, [local("/srv/left")]);
    assert_eq!((target, remote), (local("/srv/right"), (false, false)));
    assert_eq!(
        options,
        CopyOptions {
            preserve: true,
            atomic: true,
            remove_sources: false,
            overwrite: false,
        }
    );
    assert!(screen(&mut app).contains("Counting"), "the job's window");

    // A taken name: the path of the file there, both files with their sizes and times,
    // and Skip as the default; Left goes to Overwrite.
    let mut answer = ask_taken(&mut app, id);
    let text = screen_of(&mut app, 16);
    assert!(text.contains("/srv/right/left/a"), "{text}");
    assert!(text.contains("New "), "{text}");
    assert!(text.contains("12345 bytes  2023-11-14 19:26"), "{text}");
    assert!(text.contains("Existing "), "{text}");
    assert!(text.contains("The rest: "), "{text}");
    assert!(text.contains("Ask each time"), "{text}");
    assert!(text.contains("{ Skip }"), "{text}");
    app.handle(action(Action::Confirm));
    assert_eq!(answer.try_recv(), Ok(Conflict::once(Choice::Skip)));
    let mut answer = ask_taken(&mut app, id);
    app.handle(action(Action::Left));
    app.handle(action(Action::Confirm));
    assert_eq!(answer.try_recv(), Ok(Conflict::once(Choice::Overwrite)));

    // The end reads the target and the source's directory again.
    let effects = app.job_event(id, JobEvent::Finished { complete: true });
    assert_eq!(effects.len(), 2);
    assert_eq!(app.context(), Context::Panel);
}

/// A copy of `/srv/left/a` that the job `id` asks about, as `/srv/right/left/a` is there;
/// `a_2` is free beside it. The answer comes on what it returns.
fn ask_taken(app: &mut App, id: u64) -> oneshot::Receiver<Conflict> {
    let mut metadata = dir("left").metadata;
    metadata.kind = FileKind::File;
    metadata.size = Some(12_345);
    let (reply, answer) = oneshot::channel();
    let taken = TakenName {
        source: local("/srv/left/a"),
        target: local("/srv/right/left/a"),
        source_metadata: metadata.clone(),
        target_metadata: metadata,
        free: b"a_2".to_vec(),
        reply,
    };
    let event = JobEvent::Exists {
        at: Instant::now(),
        taken: Box::new(taken),
    };
    app.job_event(id, event);
    answer
}

/// A copy job from `two_directories`, and its id.
fn copying() -> (App, u64) {
    let mut app = two_directories();
    app.handle(action(Action::Copy));
    let (id, ..) = copy_job(app.handle(action(Action::Confirm)));
    (app, id)
}

#[test]
fn a_taken_name_offers_skip_and_the_rest_only_where_entries_come_after_it() {
    let (mut app, id) = copying();
    let at = |app: &mut App, done, total| {
        let counts = Counts {
            done,
            total,
            ..Counts::default()
        };
        app.job_event(id, report(local("/srv/left/a"), counts));
    };
    // At the last entry, nothing comes after it.
    at(&mut app, 1, 2);
    let mut answer = ask_taken(&mut app, id);
    let text = screen_of(&mut app, 16);
    assert!(!text.contains("The rest"), "{text}");
    assert!(
        text.contains("[ Overwrite ] { Skip } [ Rename… ] [ Abort ]"),
        "{text}"
    );
    app.handle(action(Action::Confirm));
    assert_eq!(answer.try_recv(), Ok(Conflict::once(Choice::Skip)));

    // For the only entry, Skip would do what Abort does.
    at(&mut app, 0, 1);
    let mut answer = ask_taken(&mut app, id);
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("[ Overwrite ] [ Rename… ] { Abort }"),
        "{text}"
    );
    app.handle(action(Action::Confirm));
    assert_eq!(answer.try_recv(), Ok(Conflict::once(Choice::Abort)));
}

#[test]
fn a_failure_offers_only_the_buttons_and_choices_that_do_something() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (id, ..) = delete_job(app.handle(action(Action::Confirm)));
    // The screen with the question about a failure at entry `done` of `total`, and the
    // answer that Enter gives it, after Alt+t opens the list of the rest, if `list`.
    let mut ask = |(done, total): (u64, u64), retry, code, list| {
        let counts = Counts {
            done,
            total,
            ..Counts::default()
        };
        app.job_event(id, report(local("/srv/left/a"), counts));
        let (reply, mut decision) = oneshot::channel();
        let reason = Reason {
            text: "Busy".to_owned(),
            code,
        };
        let path = local("/srv/left/a");
        let (step, at) = (Step::Remove, Instant::now());
        let failed = JobEvent::Failed {
            at,
            path,
            step,
            reason,
            retry,
            reply,
        };
        app.job_event(id, failed);
        if list {
            app.handle(Resolved::Hotkey('t'));
        }
        let text = screen_of(&mut app, 20);
        if list {
            app.handle(action(Action::Cancel));
        }
        app.handle(action(Action::Confirm));
        (text, decision.try_recv())
    };
    let busy = Some(Code::Os(16));
    let (text, decision) = ask((1, 3), true, busy, false);
    assert!(text.contains("{ Retry } [ Skip ] [ Abort ]"), "{text}");
    assert!(
        text.contains("The rest: ") && text.contains("Code: 16"),
        "{text}"
    );
    assert_eq!(decision, Ok(Resolution::once(Decision::Retry)));
    let (text, _) = ask((1, 3), true, busy, true);
    assert!(text.contains("Skip every failure"), "{text}");
    assert!(text.contains("Skip failures with the same code"), "{text}");
    // Without a code, a dash, and no failures to skip by it.
    let (text, _) = ask((1, 3), true, None, true);
    assert!(text.contains("Code: —"), "{text}");
    assert!(text.contains("Skip every failure"), "{text}");
    assert!(!text.contains("the same code"), "{text}");
    // At the last entry, no failures come after it.
    let (text, _) = ask((2, 3), true, busy, false);
    assert!(!text.contains("The rest"), "{text}");
    assert!(text.contains("{ Retry } [ Skip ] [ Abort ]"), "{text}");
    // For the only entry, Skip would do what Abort does.
    let (text, _) = ask((0, 1), true, busy, false);
    assert!(text.contains("{ Retry } [ Abort ]"), "{text}");
    // Where trying again would fail again, Skip is the default.
    let (text, decision) = ask((1, 3), false, None, false);
    assert!(text.contains("{ Skip } [ Abort ]"), "{text}");
    assert_eq!(decision, Ok(Resolution::once(Decision::Skip)));
}

#[test]
fn the_rest_of_the_taken_names_get_what_the_combo_box_says() {
    let (mut app, id) = copying();
    let rests = [
        Rest::Ask,
        Rest::Same,
        Rest::OverwriteOlder,
        Rest::OverwriteSmaller,
        Rest::OverwriteLarger,
        Rest::OverwriteSizeDiffers,
    ];
    for (index, rest) in rests.into_iter().enumerate() {
        let mut answer = ask_taken(&mut app, id);
        // Alt+t opens the list of the combo box, where j goes down and Enter chooses.
        app.handle(Resolved::Hotkey('t'));
        for _ in 0..index {
            app.handle(action(Action::Down));
        }
        app.handle(action(Action::Confirm));
        app.handle(Resolved::Hotkey('o'));
        let overwrite = Conflict {
            this: Choice::Overwrite,
            rest,
        };
        assert_eq!(answer.try_recv(), Ok(overwrite), "{rest:?}");
    }
    // Esc, as Abort, stops the job.
    let mut answer = ask_taken(&mut app, id);
    app.handle(action(Action::Cancel));
    assert_eq!(answer.try_recv(), Ok(Conflict::once(Choice::Abort)));
}

#[test]
fn a_taken_name_views_either_file_over_the_job_and_comes_back() {
    let (mut app, id) = copying();
    let mut answer = ask_taken(&mut app, id);
    // Alt+x views the file there, over the job's window, and the job waits.
    let Effect::Read(ReadRequest {
        id: read, location, ..
    }) = one(app.handle(Resolved::Hotkey('x')))
    else {
        panic!("expected a read");
    };
    assert_eq!(location, local("/srv/right/left/a"));
    app.read(read, Ok(whole(*b"there\n")));
    assert_eq!(app.place(), (Mode::Normal, Context::Viewer));
    assert_eq!(app.front(), Front::Viewer);
    assert!(
        !screen(&mut app).contains("Counting"),
        "the job's window is under it"
    );
    assert!(answer.try_recv().is_err(), "it waits");
    app.handle(action(Action::Help));
    assert_eq!(
        app.context(),
        Context::Viewer,
        "the help would open under it"
    );

    // Esc brings the question back, the focus where it was: on the line of the file there.
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Dialog);
    assert!(screen(&mut app).contains("Existing"));
    // Enter there views it again; on the line of the new file, Space views that one.
    let Effect::Read(ReadRequest { location, .. }) = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected a read");
    };
    assert_eq!(location, local("/srv/right/left/a"));
    app.handle(action(Action::Quit));
    app.handle(action(Action::Up));
    let Effect::Read(ReadRequest { location, .. }) = one(app.handle(action(Action::Toggle))) else {
        panic!("expected a read");
    };
    assert_eq!(location, local("/srv/left/a"));
    app.handle(action(Action::Cancel));
    app.handle(Resolved::Hotkey('s'));
    assert_eq!(answer.try_recv(), Ok(Conflict::once(Choice::Skip)));
}

#[test]
fn a_file_that_cannot_be_viewed_says_so_over_the_question() {
    let (mut app, id) = copying();
    let _answer = ask_taken(&mut app, id);
    let Effect::Read(ReadRequest { id: read, .. }) = one(app.handle(Resolved::Hotkey('n'))) else {
        panic!("expected a read");
    };
    app.read(read, Err("gone".to_owned().into()));
    let text = screen_of(&mut app, 16);
    assert!(
        shows_problem(&text, "Cannot view", Some("/srv/left/a"), "gone"),
        "{text}"
    );
    assert_eq!(app.dialogs.len(), 2, "over the question");
    app.handle(action(Action::Confirm));
    assert!(
        screen_of(&mut app, 16).contains("The rest: "),
        "the question again"
    );
}

#[test]
fn a_job_that_ends_takes_its_question_from_the_viewer_and_rename() {
    let (mut app, id) = copying();
    let _answer = ask_taken(&mut app, id);
    app.handle(Resolved::Hotkey('n'));
    app.job_event(id, JobEvent::Finished { complete: false });
    app.handle(action(Action::Quit));
    assert_eq!(app.context(), Context::Panel, "no question comes back");

    let (mut app, id) = copying();
    let _answer = ask_taken(&mut app, id);
    app.handle(Resolved::Hotkey('r'));
    app.job_event(id, JobEvent::Finished { complete: false });
    assert_eq!(app.context(), Context::Panel);
}

#[test]
fn rename_offers_a_free_name_and_takes_a_path() {
    let (mut app, id) = copying();
    let mut answer = ask_taken(&mut app, id);
    app.handle(Resolved::Hotkey('r'));
    let text = screen(&mut app);
    assert!(text.contains("New name:") && text.contains("a_2"), "{text}");
    assert_eq!(app.place(), (Mode::Insert, Context::DialogInput));
    // Cancel goes back to the question, which still waits.
    app.handle(action(Action::Cancel));
    assert!(screen_of(&mut app, 16).contains("The rest: "));
    assert!(answer.try_recv().is_err());

    // Without a name, OK does nothing.
    app.handle(Resolved::Hotkey('r'));
    app.dialogs[0].dialog.set_text(0, "");
    app.handle(action(Action::Confirm));
    assert!(screen(&mut app).contains("New name:"));
    assert!(answer.try_recv().is_err());

    // A path goes from the directory of the file there; that directory is read again
    // once the job ends.
    app.dialogs[0].dialog.set_text(0, "../kept/b");
    app.handle(action(Action::Confirm));
    let renamed = Conflict::once(Choice::Rename(b"/srv/right/left/../kept/b".to_vec()));
    assert_eq!(answer.try_recv(), Ok(renamed));
    let job = app.jobs.iter().find(|job| job.id == id).unwrap();
    assert!(
        job.changes.contains(&local("/srv/right/left/../kept")),
        "{:?}",
        job.changes
    );
}

#[test]
fn f5_takes_typed_targets_and_remembers_preserve() {
    let mut app = two_directories();
    let (handle, _requests) = HostHandle::channel();
    let stop = CancellationToken::new();
    let web = Host::Connected {
        connection: 1,
        stop,
        handle,
    };
    app.hosts.insert("web".to_owned(), web);
    let copy_to = |app: &mut App, text: &str| {
        app.handle(action(Action::Copy));
        retype(app, text);
        app.handle(action(Action::Confirm))
    };
    let (_, _, target, _, _) = copy_job(copy_to(&mut app, "sub"));
    assert_eq!(target, local("/srv/sub"), "from the source's directory");
    app.jobs.clear();
    let (_, _, target, ends, _) = copy_job(copy_to(&mut app, "web:/var/www"));
    assert_eq!((target, ends), (remote("web", "/var/www"), (false, true)));
    app.jobs.clear();
    let (_, _, target, _, _) = copy_job(copy_to(&mut app, "x:y"));
    assert_eq!(target, local("/srv/x:y"), "no host called x");
    app.jobs.clear();

    // Preserve attributes, switched off, stays off.
    app.handle(action(Action::Copy));
    app.handle(Resolved::Hotkey('p'));
    app.handle(action(Action::Confirm));
    app.jobs.clear();
    app.handle(action(Action::Copy));
    assert!(screen(&mut app).contains("[ ] Preserve attributes"));
    let (_, _, _, _, options) = copy_job(app.handle(action(Action::Confirm)));
    assert!(!options.preserve);
}

#[test]
fn copies_write_directly_without_atomic_upload() {
    let transfer = TransferConfig {
        atomic_upload: false,
        ..TransferConfig::default()
    };
    let (mut app, effects) = App::new(
        Path::new("/srv"),
        Path::new("/home/me"),
        &Config {
            transfer,
            ..config()
        },
    );
    answer(&mut app, effects, &Listing::Dir(vec![dir("left")]));
    app.handle(action(Action::Down));
    app.handle(action(Action::Copy));
    retype(&mut app, "/tmp");
    let (_, _, target, _, options) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(target, local("/tmp"));
    assert!(!options.atomic);
}

#[test]
fn f6_moves_to_the_other_panel_or_renames_in_place() {
    let mut app = two_directories();
    app.handle(action(Action::Move));
    let text = screen(&mut app);
    assert!(text.contains("Move \"left\" to:"), "{text}");
    assert!(text.contains("/srv/right"), "{text}");
    let checked = format!("[{}] Preserve attributes", app.theme.glyphs.check);
    assert!(text.contains(&checked), "{text}");
    let (id, sources, target, _, options) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(
        (sources, target),
        (vec![local("/srv/left")], local("/srv/right"))
    );
    assert!(options.remove_sources && options.preserve);
    assert!(screen(&mut app).contains("Move"));
    let effects = app.job_event(id, JobEvent::Finished { complete: true });
    assert_eq!(effects.len(), 2, "both panels: the source changed too");

    // A new name renames in place.
    app.handle(action(Action::Move));
    retype(&mut app, "renamed");
    let (_, _, target, _, options) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(target, local("/srv/renamed"));
    assert!(options.remove_sources);
    app.jobs.clear();

    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Move));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot move to",
            Some("/srv"),
            "The source and the target are the same"
        ),
        "{text}"
    );
}

#[test]
fn f6_may_leave_the_attributes_as_f5_does() {
    let mut app = two_directories();
    app.handle(action(Action::Move));
    app.handle(Resolved::Hotkey('p'));
    let (_, _, _, _, options) = copy_job(app.handle(action(Action::Confirm)));
    assert!(options.remove_sources && !options.preserve);
    app.jobs.clear();

    // One setting for both.
    app.handle(action(Action::Copy));
    assert!(screen(&mut app).contains("[ ] Preserve attributes"));
}

#[test]
fn f5_f6_and_f8_count_the_marked_files_and_directories() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let mut link = dir("link");
    link.metadata.kind = FileKind::Symlink;
    link.target_kind = Some(FileKind::Dir);
    let entries = vec![dir("a"), dir("b"), link, file("c", 1), file("d", 2)];
    answer(&mut app, effects, &Listing::Dir(entries));
    app.handle(action(Action::Home));
    app.handle(action(Action::Down));
    for _ in 0..5 {
        app.handle(action(Action::Mark));
    }
    let asks = [
        (Action::Copy, "Copy 2 files and 3 directories to:"),
        (Action::Move, "Move 2 files and 3 directories to:"),
        (Action::Delete, "Delete 2 files and 3 directories?"),
    ];
    for (ask, question) in asks {
        app.handle(action(ask));
        let text = screen(&mut app);
        assert!(text.contains(question), "{text}");
        app.handle(action(Action::Cancel));
        assert!(app.dialogs.is_empty(), "{question}");
    }

    let mut app = loaded();
    app.handle(action(Action::Home));
    app.handle(action(Action::Down));
    app.handle(action(Action::Mark));
    app.handle(action(Action::Mark));
    app.handle(action(Action::Copy));
    let text = screen(&mut app);
    assert!(text.contains("Copy 2 directories to:"), "{text}");
}

#[test]
fn f5_does_not_copy_onto_or_into_itself() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Copy));
    assert!(
        app.handle(action(Action::Confirm)).is_empty(),
        "the other panel is here too"
    );
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot copy to",
            Some("/srv"),
            "The source and the target are the same"
        ),
        "{text}"
    );
    app.handle(action(Action::Confirm));

    app.handle(action(Action::Copy));
    retype(&mut app, "left/inner");
    assert!(app.handle(action(Action::Confirm)).is_empty());
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot copy to",
            Some("/srv/left/inner"),
            "It is in /srv/left"
        ),
        "{text}"
    );
    assert!(remote_within(
        &RemotePath::from("/a"),
        &RemotePath::from("/a/b/")
    ));
    assert!(!remote_within(
        &RemotePath::from("/a"),
        &RemotePath::from("/ab")
    ));
    assert!(!remote_within(
        &RemotePath::from("a"),
        &RemotePath::from("/a")
    ));
}

/// Puts the cursor of the left panel on `name`.
fn cursor_on(app: &mut App, name: &str) {
    app.handle(action(Action::Home));
    for _ in 0..10 {
        if app.panel(Side::Left).name_under_cursor() == Some(name.as_bytes()) {
            return;
        }
        app.handle(action(Action::Down));
    }
    panic!("no {name}");
}

/// The text of the field of the dialog in front.
fn field_text(app: &App) -> String {
    app.dialogs
        .front()
        .map(|open| open.dialog.text().to_owned())
        .unwrap_or_default()
}

#[test]
fn shift_f5_copies_the_entry_under_the_cursor_beside_it_under_a_new_name() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let listing = Listing::Dir(vec![file("report.pdf", 1), dir("docs"), file("~", 2)]);
    answer(&mut app, effects, &listing);
    // The marks stay out of it.
    cursor_on(&mut app, "docs");
    app.handle(action(Action::Mark));
    cursor_on(&mut app, "report.pdf");
    app.handle(action(Action::Duplicate));
    let text = screen(&mut app);
    assert!(text.contains("Copy \"report.pdf\" to:"), "{text}");
    let checked = format!("[{}] Preserve attributes", app.theme.glyphs.check);
    assert!(text.contains(&checked), "{text}");
    assert_eq!(field_text(&app), "report.pdf");
    type_text(&mut app, " (1)");
    let (_, sources, target, ends, options) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(sources, [local("/srv/report.pdf")]);
    assert_eq!(target, local("/srv/report (1).pdf"), "before the extension");
    assert_eq!(ends, (false, false));
    assert!(options.preserve && !options.remove_sources);
    let chosen: Vec<&[u8]> = app
        .panel(Side::Left)
        .chosen()
        .iter()
        .map(|entry| entry.name.as_slice())
        .collect();
    assert_eq!(chosen, [b"docs"], "still marked");
    app.jobs.clear();

    // The same name: the source itself.
    app.handle(action(Action::Duplicate));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot copy to",
            Some("/srv/report.pdf"),
            "The source and the target are the same"
        ),
        "{text}"
    );
    // Change… opens the dialog again, with what was typed, to change it.
    assert!(text.contains("{ Change… } [ Cancel ]"), "{text}");
    app.handle(action(Action::Confirm));
    assert_eq!(field_text(&app), "report.pdf");
    assert!(screen(&mut app).contains("Copy \"report.pdf\" to:"));
    app.handle(action(Action::Cancel));

    // A directory's name is whole; elsewhere is typed as for F5.
    cursor_on(&mut app, "docs");
    app.handle(action(Action::Duplicate));
    type_text(&mut app, "-2");
    let (_, sources, target, _, _) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(
        (sources, target),
        (vec![local("/srv/docs")], local("/srv/docs-2"))
    );
    app.jobs.clear();
    app.handle(action(Action::Duplicate));
    retype(&mut app, "/tmp");
    let (_, _, target, _, _) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(target, local("/tmp"));
    app.jobs.clear();

    // `~` alone would be the home directory.
    cursor_on(&mut app, "~");
    app.handle(action(Action::Duplicate));
    assert_eq!(field_text(&app), "\\~");
    type_text(&mut app, "2");
    let (_, _, target, _, _) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(target, local("/srv/~2"));
    app.jobs.clear();

    // Visual mode ends, and the entry under the cursor goes alone.
    cursor_on(&mut app, "docs");
    app.handle(action(Action::Down));
    app.handle(action(Action::VisualMode));
    app.handle(action(Action::Up));
    app.handle(action(Action::Duplicate));
    assert!(!app.panel(Side::Left).visual());
    assert_eq!(field_text(&app), "docs");
    app.handle(action(Action::Cancel));

    // Not on `..`.
    app.handle(action(Action::Home));
    app.handle(action(Action::Duplicate));
    assert_eq!(app.context(), Context::Panel);
}

#[test]
fn shift_f5_cannot_edit_a_name_that_is_not_utf8() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let entry = DirEntry {
        name: b"caf\xe9".to_vec(),
        ..file("x", 1)
    };
    answer(&mut app, effects, &Listing::Dir(vec![entry]));
    app.handle(action(Action::Down));
    app.handle(action(Action::Duplicate));
    let text = screen(&mut app);
    assert!(
        text.contains("the name is not valid UTF-8; F5 can copy it"),
        "{text}"
    );
}

#[test]
fn a_copy_to_a_lost_host_waits_and_ends_when_it_is_disconnected() {
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 1)) else {
        panic!("expected a connection");
    };
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    let generation = request.generation;
    let location = remote("web", "/home/deploy");
    let listing = Listing::Dir(Vec::new());
    app.listed(
        app.shown(Side::Right),
        generation,
        Ok(Listed {
            location,
            listing,
            space: None,
        }),
    );

    app.active = Side::Left;
    let effects = app.go(app.shown(Side::Left), Destination::to(local("/srv")));
    answer(&mut app, effects, &Listing::Dir(vec![file("notes", 5)]));
    app.handle(action(Action::Down));
    app.handle(action(Action::Copy));
    assert!(screen(&mut app).contains("web:/home/deploy"));
    let (_, _, target, ends, _) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(
        (target, ends),
        (remote("web", "/home/deploy"), (false, true))
    );

    app.closed("web", connection, Some("Broken pipe"));
    assert_eq!(app.jobs.len(), 1);
    assert_eq!(app.jobs[0].view.summary(Instant::now()).state, "offline");
    app.disconnect("web");
    assert!(app.jobs.is_empty());
}
