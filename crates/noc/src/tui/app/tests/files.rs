use super::*;

#[test]
fn ctrl_x_g_plus_and_g_minus_mark_in_the_other_panel() {
    let mut app = with_files(&["a.md", "b.txt", "c.md"]);
    app.handle(action(Action::Select));
    let text = screen_of(&mut app, 16);
    assert!(in_the_middle(&text, " Select "), "{text}");
    app.handle(action(Action::Cancel));

    assert!(keys(&mut app, &["ctrl-x", "g", "+"]).is_empty());
    assert_eq!(app.active, Side::Right, "the other panel has the keys");
    let text = screen_of(&mut app, 16);
    assert!(in_the_middle(&text, " Select "), "there too: {text}");
    retype(&mut app, "*.md");
    keys(&mut app, &["enter"]);
    assert_eq!(chosen_names(&app, Side::Right), ["a.md", "c.md"]);
    assert_eq!(chosen_names(&app, Side::Left), [] as [String; 0]);
    assert_eq!(app.active, Side::Left, "the keys come back once it closes");

    on_other(&mut app, Action::Unselect);
    let text = screen_of(&mut app, 16);
    assert!(in_the_middle(&text, " Unselect "), "{text}");
    retype(&mut app, "a*");
    keys(&mut app, &["enter"]);
    assert_eq!(chosen_names(&app, Side::Right), ["c.md"]);

    // g * inverts the marks of the other panel, which has no dialog.
    assert!(keys(&mut app, &["ctrl-x", "g", "*"]).is_empty());
    assert_eq!(chosen_names(&app, Side::Right), ["a.md", "b.txt"]);
    assert_eq!(chosen_names(&app, Side::Left), [] as [String; 0]);
    assert_eq!(app.active, Side::Left);
}

#[test]
fn g_plus_and_g_minus_mark_and_unmark_by_pattern() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let entries = vec![
        file("a.md", 10),
        file("B.MD", 20),
        file("c.txt", 30),
        dir("docs.md"),
    ];
    answer(&mut app, effects, &Listing::Dir(entries));

    app.handle(action(Action::Select));
    assert_eq!(app.context(), Context::DialogInput);
    let text = screen(&mut app);
    assert!(text.contains("Select"));
    let glob = app.theme.glyphs.search_modes.glob;
    let smart = app.theme.glyphs.case_modes.smart;
    assert!(
        text.contains("Pattern:") && text.contains(&format!(" {smart} {glob}  ║")),
        "smart case, and shell patterns, end the field, as in every field that searches: {text}"
    );
    assert_eq!(app.key_place().search, Some(search::Kind::MARK));
    retype(&mut app, "*.md");
    app.handle(action(Action::CaseMatch));
    app.handle(action(Action::Confirm));
    assert_eq!(app.context(), Context::Panel);
    let text = screen(&mut app);
    assert!(text.contains(" 10 B in 2 files "), "case counts: {text}");

    // The dialog opens with the last pattern, and its case.
    app.handle(action(Action::Select));
    let dialog = &app.dialogs.front().unwrap().dialog;
    assert_eq!(dialog.search(0).map(Input::case), Some(search::Case::Match));
    app.handle(action(Action::CaseSmart));
    app.handle(action(Action::Confirm));
    assert!(
        screen(&mut app).contains(" 30 B in 3 files "),
        "smart case: no capital"
    );

    // `g -` with Files only leaves the directory marked.
    app.handle(action(Action::Unselect));
    assert!(screen(&mut app).contains("Unselect"));
    retype(&mut app, "*");
    for step in [Action::NextField, Action::Toggle, Action::Confirm] {
        app.handle(action(step));
    }
    assert!(screen(&mut app).contains(" 0 B in 1 file "));

    // Esc and an empty pattern change nothing.
    app.handle(action(Action::Select));
    retype(&mut app, "c*");
    app.handle(action(Action::Cancel));
    app.handle(action(Action::Select));
    app.handle(action(Action::DeleteToStart));
    app.handle(action(Action::Confirm));
    assert!(screen(&mut app).contains(" 0 B in 1 file "));
    assert_eq!(app.pattern_options.pattern, "*");
}

/// The only effect, which must make a directory: where, and through which host.
fn create_dir(effects: Vec<Effect>) -> (PanelId, Location, Option<HostHandle>) {
    match one(effects) {
        Effect::CreateDir {
            panel,
            location,
            host,
        } => (panel, location, host),
        other => panic!("expected a new directory, got {other:?}"),
    }
}

#[test]
fn f7_makes_a_directory_and_puts_the_cursor_on_it() {
    let mut app = loaded();
    app.active = Side::Right;
    app.handle(action(Action::End));
    app.active = Side::Left;

    app.handle(action(Action::Mkdir));
    assert_eq!(app.context(), Context::PathInput);
    let text = screen(&mut app);
    assert!(text.contains("Create a new directory"), "{text}");
    assert!(text.contains("Enter directory name:"), "{text}");
    retype(&mut app, "new");
    let (panel, location, host) = create_dir(app.handle(action(Action::Confirm)));
    assert_eq!(
        (panel.side, &location, host.is_none()),
        (Side::Left, &local("/srv/new"), true)
    );
    // The panel that asked waits, its dots turning, until the directory is made and read.
    let waits = |app: &App| Side::BOTH.map(|side| app.panel(side).wait().is_some());
    assert_eq!(waits(&app), [true, false]);

    // Both panels show /srv and read it again; the one that asked lands on the directory.
    let effects = app.created(panel, &location, Ok(()));
    assert_eq!(effects.len(), 2);
    assert_eq!(waits(&app), [true, true]);
    let listing = Listing::Dir(vec![dir("left"), dir("new"), dir("right")]);
    answer(&mut app, effects, &listing);
    assert_eq!(waits(&app), [false, false]);
    assert_eq!(app.panel(Side::Left).name_under_cursor(), Some(&b"new"[..]));
    assert_eq!(
        app.panel(Side::Right).name_under_cursor(),
        Some(&b"right"[..]),
        "stays"
    );

    // The field opens empty, not with the name under the cursor.
    app.handle(action(Action::Mkdir));
    assert_eq!(app.dialogs.front().map(|open| open.dialog.text()), Some(""));
    type_text(&mut app, "x");
    let (_, location, _) = create_dir(app.handle(action(Action::Confirm)));
    assert_eq!(location, local("/srv/x"));
    app.handle(action(Action::Mkdir));
    type_text(&mut app, "y");
    let (_, location, _) = create_dir(app.handle(action(Action::Confirm)));
    assert_eq!(location, local("/srv/y"));
    // A directory that is not made ends the wait as well, in an error.
    app.created(panel, &local("/srv/x"), Ok(()));
    assert!(
        app.created(
            panel,
            &location,
            Err(Unmade::Failed("busy".to_owned().into()))
        )
        .is_empty()
    );
    for side in Side::BOTH {
        app.panel_mut(side).cancel();
    }
    assert_eq!(waits(&app), [false, false]);

    // Esc and an empty name make nothing.
    app.handle(action(Action::Mkdir));
    assert!(app.handle(action(Action::Cancel)).is_empty());
    app.handle(action(Action::Mkdir));
    app.handle(action(Action::DeleteToStart));
    assert!(app.handle(action(Action::Confirm)).is_empty());
}

#[test]
fn alt_and_a_letter_act_on_a_dialog_while_its_field_takes_the_text() {
    let mut app = loaded();
    app.handle(action(Action::Mkdir));
    retype(&mut app, "new");
    // A letter nothing has does nothing: no Esc, no x deleting a character.
    keys(&mut app, &["alt-x"]);
    assert_eq!(app.place(), (Mode::Insert, Context::PathInput));
    assert_eq!(
        app.dialogs.front().map(|open| open.dialog.text()),
        Some("new")
    );
    let (_, location, _) = create_dir(keys(&mut app, &["alt-o"]));
    assert_eq!(location, local("/srv/new"), "OK, pressed from the field");

    // The letter of the prompt brings the focus back to the field, in Insert mode.
    app.handle(action(Action::Mkdir));
    keys(&mut app, &["down"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Dialog));
    keys(&mut app, &["alt-e"]);
    assert_eq!(app.place(), (Mode::Insert, Context::PathInput));
    let text = screen(&mut app);
    assert!(text.contains("Enter directory name:"), "no & shows: {text}");
    keys(&mut app, &["esc", "esc"]);
    assert!(app.dialogs.is_empty(), "Cancel has no letter, Esc closes");
}

#[test]
fn a_directory_that_cannot_be_made_is_an_error_dialog() {
    let mut app = loaded();
    let panel = app.shown(Side::Left);
    // A name that is taken: Change… opens F7 again with it, to change it.
    let effects = app.created(
        panel,
        &local("/srv/left"),
        Err(Unmade::Taken(Reason {
            text: "already exists".to_owned(),
            code: Some(Code::Os(17)),
        })),
    );
    assert!(effects.is_empty());
    assert_eq!(app.context(), Context::Dialog);
    let text = screen_of(&mut app, 16);
    assert!(text.contains("Error"), "{text}");
    assert!(
        shows_problem(
            &text,
            "Cannot create the directory",
            Some("/srv/left"),
            "already exists"
        ),
        "{text}"
    );
    assert!(text.contains("{ Change… } [ Cancel ]"), "{text}");
    app.handle(action(Action::Confirm));
    assert_eq!(app.context(), Context::PathInput);
    let field = app.dialogs.front().map(|open| open.dialog.text());
    assert_eq!(field, Some("left"), "the name typed");
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);

    // Anything else: Retry makes it again.
    let denied = Reason {
        text: "Permission denied".to_owned(),
        code: Some(Code::Os(13)),
    };
    app.created(panel, &local("/srv/new"), Err(Unmade::Failed(denied)));
    assert!(screen_of(&mut app, 16).contains("{ Retry } [ Cancel ]"));
    let (_, location, _) = create_dir(app.handle(action(Action::Confirm)));
    assert_eq!(location, local("/srv/new"));

    // A directory elsewhere reads no panel again.
    assert!(
        app.created(app.shown(Side::Left), &local("/tmp/x"), Ok(()))
            .is_empty()
    );
}

#[test]
fn f7_works_on_hosts() {
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
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
        app.shown(Side::Left),
        generation,
        Ok(Listed {
            location,
            listing,
            space: None,
        }),
    );
    assert!(screen(&mut app).contains("7Mkdir"));
    app.handle(action(Action::Mkdir));
    type_text(&mut app, "www");
    let (_, location, host) = create_dir(app.handle(action(Action::Confirm)));
    assert_eq!(location, remote("web", "/home/deploy/www"));
    assert!(host.is_some(), "through the host's task");
}

#[test]
fn pattern_dialogs_wait_for_no_prompt() {
    let mut app = loaded();
    app.handle(action(Action::Select));
    let (password, answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
    app.ask(password);
    app.prompt_closed(1);
    assert_eq!(app.context(), Context::DialogInput);
    assert!(
        screen(&mut app).contains("Select"),
        "the app's own dialog stays"
    );
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);
    assert!(answer.try_recv().is_err(), "the prompt went with ssh");
}

#[test]
fn rename_in_the_question_of_shift_f6_takes_a_name_in_the_directory() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let listing = Listing::Dir(vec![file("report.pdf", 1), file("taken.pdf", 2)]);
    answer(&mut app, effects, &listing);
    let panel = app.shown(Side::Left);
    let (from, to) = (local("/srv/report.pdf"), local("/srv/taken.pdf"));
    app.entry_renamed(panel, &from, &to, taken(Some(1), 2, "taken_2.pdf"));
    // It offers a free name; one with a slash would move it.
    app.handle(Resolved::Hotkey('r'));
    assert!(screen_of(&mut app, 24).contains("taken_2.pdf"));
    retype(&mut app, "x/y");
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert!(screen_of(&mut app, 24).contains("cannot be a name"));
    app.handle(action(Action::Confirm));
    retype(&mut app, "other");
    let other = one(app.handle(action(Action::Confirm)));
    assert!(
        matches!(&other, Effect::Rename { replace: false, to: target, .. }
            if *target == local("/srv/other.pdf")),
        "{other:?}"
    );
    // Cancel there goes back to the question.
    app.entry_renamed(panel, &from, &local("/srv/other.pdf"), Ok(()));
    app.entry_renamed(panel, &from, &to, taken(Some(1), 2, "taken_2.pdf"));
    app.handle(Resolved::Hotkey('r'));
    app.handle(action(Action::Cancel));
    assert!(screen_of(&mut app, 24).contains("File exists"));
}

#[test]
fn shift_f6_renames_in_the_row_and_asks_before_replacing_a_file() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let listing = Listing::Dir(vec![file("report.pdf", 1), file("taken.pdf", 2)]);
    answer(&mut app, effects, &listing);
    app.handle(action(Action::Down));
    app.handle(action(Action::Rename));
    assert_eq!(app.context(), Context::Rename);
    let text = screen(&mut app);
    assert!(!text.contains("Rename:"), "the status line as ever: {text}");
    retype(&mut app, "taken");
    let rename = one(app.handle(action(Action::Confirm)));
    let Effect::Rename {
        panel,
        from,
        to,
        replace: false,
        host: None,
    } = rename
    else {
        panic!("expected a rename, got {rename:?}");
    };
    assert_eq!(
        (&from, &to),
        (&local("/srv/report.pdf"), &local("/srv/taken.pdf"))
    );
    assert_eq!(app.context(), Context::Panel, "the field is gone");
    assert!(app.panel(Side::Left).wait().is_some(), "its dots turn");

    // A file has the name: the question of a copy, for one file, with Cancel the default.
    let taken_name = || taken(Some(1), 2, "taken_2.pdf");
    assert!(
        app.entry_renamed(panel, &from, &to, taken_name())
            .is_empty()
    );
    assert!(
        app.panel(Side::Left).wait().is_none(),
        "while the question waits"
    );
    let text = screen_of(&mut app, 24);
    assert!(
        text.contains("File exists") && text.contains("/srv/taken.pdf"),
        "{text}"
    );
    assert!(
        text.contains("1 byte") && text.contains("2 bytes"),
        "{text}"
    );
    assert!(
        text.contains("[ Overwrite ] [ Rename… ] { Cancel }"),
        "{text}"
    );
    assert!(app.handle(action(Action::Confirm)).is_empty(), "Cancel");
    assert!(app.dialogs.is_empty());

    // Overwrite renames over it.
    app.entry_renamed(panel, &from, &to, taken_name());
    app.handle(action(Action::Left));
    app.handle(action(Action::Left));
    let again = one(app.handle(action(Action::Confirm)));
    assert!(
        matches!(&again, Effect::Rename { replace: true, to: target, .. } if *target == to),
        "{again:?}"
    );
    let effects = app.entry_renamed(panel, &from, &to, Ok(()));
    assert_eq!(effects.len(), 2, "both panels show the directory");
    answer(&mut app, effects, &Listing::Dir(vec![file("taken.pdf", 1)]));
    assert_eq!(
        app.panel(Side::Left).name_under_cursor(),
        Some(&b"taken.pdf"[..])
    );

    let failed = Err(Refused::Failed("No space".to_owned().into()));
    let effects = app.entry_renamed(panel, &from, &to, failed);
    assert!(effects.is_empty());
    let text = screen(&mut app);
    assert!(
        text.contains("Cannot rename") && text.contains("/srv/report.pdf"),
        "{text}"
    );
    assert!(text.contains("No space"), "{text}");
    app.handle(action(Action::Confirm));

    // A slash would move it: the error goes, and the field stays.
    app.handle(action(Action::Rename));
    retype(&mut app, "a/b");
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert!(screen(&mut app).contains("\"a/b.pdf\" cannot be a name"));
    app.handle(action(Action::Confirm));
    assert_eq!(app.context(), Context::Rename);
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);

    // The same name, or none, renames nothing.
    app.handle(action(Action::Rename));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    app.handle(action(Action::Rename));
    app.handle(action(Action::TextEnd));
    app.handle(action(Action::DeleteToStart));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.context(), Context::Panel);

    // Not on `..`.
    app.handle(action(Action::Home));
    app.handle(action(Action::Rename));
    assert_eq!(app.context(), Context::Panel);
}
