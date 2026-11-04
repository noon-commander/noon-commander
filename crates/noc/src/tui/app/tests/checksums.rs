use super::*;

/// An app with both panels on `/srv`, which holds `sub`, `a`, and `b`, the cursor of the
/// left one on `b`.
fn on_files() -> App {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let listing = Listing::Dir(vec![dir("sub"), file("a", 3), file("b", 4)]);
    answer(&mut app, effects, &listing);
    app.handle(action(Action::End));
    app
}

/// The only effect, which must start a checksum job: its id, its groups of targets with
/// whether they are remote, and the algorithm.
fn checksum_job(effects: Vec<Effect>) -> (u64, Vec<(Vec<Location>, bool)>, Algorithm) {
    match one(effects) {
        Effect::Checksum {
            id,
            targets,
            algorithm,
            ..
        } => {
            let targets = targets
                .into_iter()
                .map(|(group, host)| (group, host.is_some()))
                .collect();
            (id, targets, algorithm)
        }
        other => panic!("expected a checksum job, got {other:?}"),
    }
}

fn sum(path: &str, name: &str, digest: Option<u8>) -> Sum<Location> {
    Sum {
        path: local(path),
        name: name.as_bytes().to_vec(),
        size: 4,
        digest: digest.map(|byte| vec![byte; 32]),
    }
}

/// Ends the job `id` with `sums`.
fn hashed(app: &mut App, id: u64, sums: Vec<Sum<Location>>) -> Vec<Effect> {
    app.job_event(id, JobEvent::Sums(sums));
    app.job_event(id, JobEvent::Finished { complete: true })
}

#[test]
fn f_hash_shows_the_checksum_of_a_file_and_copies_it() {
    let mut app = on_files();
    app.handle(action(Action::Checksum));
    let text = screen_of(&mut app, 20);
    assert!(text.contains("Checksum of \"b\""), "{text}");
    assert!(text.contains("Algorithm: [SHA-256"), "{text}");
    assert!(!text.contains("BLAKE3"), "the list is closed: {text}");
    assert!(text.contains("Expected checksum"), "{text}");
    let placeholder = "Optional: a hex checksum or a line of a checksum file";
    assert!(text.contains(placeholder), "{text}");
    assert!(!text.contains("Compare with"), "the same file: {text}");
    // Alt and its letter open the list, which shows every algorithm, and Esc closes it.
    app.handle(Resolved::Hotkey('a'));
    assert_eq!(app.place(), (Mode::Normal, Context::ComboBox));
    assert!(screen_of(&mut app, 20).contains("BLAKE3"));
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Dialog, "the dialog stays");
    let (id, targets, algorithm) = checksum_job(app.handle(action(Action::Confirm)));
    assert_eq!(targets, [(vec![local("/srv/b")], false)]);
    assert_eq!(algorithm, Algorithm::Sha256);
    assert!(screen(&mut app).contains("Counting"), "the job's window");

    let effects = hashed(&mut app, id, vec![sum("/srv/b", "b", Some(0xab))]);
    assert!(effects.is_empty(), "nothing changed: {effects:?}");
    let text = screen_of(&mut app, 20);
    assert!(text.contains(&"ab".repeat(32)), "{text}");
    assert!(!text.contains("Matches"), "nothing expected: {text}");
    assert_eq!(app.context(), Context::Dialog);
    assert_eq!(app.take_clipboard(), None);
    app.handle(action(Action::Confirm));
    assert_eq!(app.take_clipboard(), Some("ab".repeat(32)));
    assert_eq!(app.take_clipboard(), None, "copied once");
    let text = screen_of(&mut app, 20);
    assert!(text.contains("{ To clipboard }"), "{text}");
    assert!(!text.contains("clipboard."), "no status: {text}");
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);
}

#[test]
fn the_expected_checksum_picks_its_algorithm_and_gives_the_verdict() {
    let mut app = on_files();
    app.handle(action(Action::Checksum));
    // Pasted as sha256sum prints it, in capitals, while SHA-256 is chosen.
    type_text(&mut app, &format!("{}  b", "CD".repeat(16)));
    let (id, _, algorithm) = checksum_job(app.handle(action(Action::Confirm)));
    assert_eq!(algorithm, Algorithm::Md5);
    let mut md5 = sum("/srv/b", "b", None);
    md5.digest = Some(vec![0xcd; 16]);
    hashed(&mut app, id, vec![md5]);
    let text = screen_of(&mut app, 20);
    assert!(text.contains(" MD5 "), "{text}");
    assert!(text.contains("Matches the expected checksum."), "{text}");
    app.handle(action(Action::Cancel));

    app.handle(action(Action::Checksum));
    let text = screen_of(&mut app, 20);
    assert!(text.contains("Algorithm: [MD5"), "the last choice: {text}");
    type_text(&mut app, &"0".repeat(32));
    let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
    let mut md5 = sum("/srv/b", "b", None);
    md5.digest = Some(vec![0xcd; 16]);
    hashed(&mut app, id, vec![md5]);
    let text = screen_of(&mut app, 20);
    assert!(text.contains("Does not match"), "{text}");
    app.handle(action(Action::Cancel));

    app.handle(action(Action::Checksum));
    type_text(&mut app, "not-hex");
    assert!(app.handle(action(Action::Confirm)).is_empty());
    let text = screen_of(&mut app, 20);
    assert!(text.contains("is not a checksum"), "{text}");
    assert!(
        text.contains(" Checksum "),
        "the dialog shows under it: {text}"
    );
    // OK on the error goes back to the dialog, the text as typed, to put it right.
    assert!(app.handle(action(Action::Confirm)).is_empty());
    let text = screen_of(&mut app, 20);
    assert!(text.contains("Checksum of \"b\""), "{text}");
    assert!(text.contains("not-hex"), "{text}");
    assert_eq!(app.place(), (Mode::Insert, Context::DialogInput));
    app.handle(action(Action::DeleteToStart));
    type_text(&mut app, &"0".repeat(32));
    let (_, _, algorithm) = checksum_job(app.handle(action(Action::Confirm)));
    assert_eq!(algorithm, Algorithm::Md5);
}

#[test]
fn the_algorithm_is_taken_in_the_list_of_its_combo_box() {
    let mut app = on_files();
    app.handle(action(Action::Checksum));
    // From the field up to the combo box, whose list Space opens.
    keys(&mut app, &["up", "space"]);
    assert_eq!(app.place(), (Mode::Normal, Context::ComboBox));
    assert!(
        keys(&mut app, &["3", "j", "enter"]).is_empty(),
        "the list closes"
    );
    assert_eq!(app.context(), Context::Dialog);
    let (id, _, algorithm) = checksum_job(keys(&mut app, &["enter"]));
    assert_eq!(algorithm, Algorithm::Md5, "3j, then Enter twice");
    app.job_event(id, JobEvent::Finished { complete: false });
    app.handle(action(Action::Checksum));
    app.handle(Resolved::Hotkey('a'));
    app.handle(Resolved::Hotkey('b'));
    let (_, _, algorithm) = checksum_job(app.handle(action(Action::Confirm)));
    assert_eq!(algorithm, Algorithm::Blake3, "Alt and the letters");
}

#[test]
fn f_hash_compares_with_the_file_in_the_other_panel() {
    let mut app = on_files();
    app.active = Side::Right;
    app.handle(action(Action::Down));
    let effects = app.handle(action(Action::Enter));
    answer(&mut app, effects, &Listing::Dir(vec![file("b", 4)]));
    app.handle(action(Action::End));
    app.active = Side::Left;
    app.handle(action(Action::Checksum));
    let text = screen_of(&mut app, 20);
    let checked = format!("[{}] Compare with /srv/sub/b", app.theme.glyphs.check);
    assert!(text.contains(&checked), "{text}");
    let (id, targets, _) = checksum_job(app.handle(action(Action::Confirm)));
    assert_eq!(
        targets,
        [
            (vec![local("/srv/b")], false),
            (vec![local("/srv/sub/b")], false)
        ]
    );
    let sums = vec![sum("/srv/b", "b", Some(1)), sum("/srv/sub/b", "b", Some(1))];
    hashed(&mut app, id, sums);
    let text = screen_of(&mut app, 24);
    assert!(text.contains("✓ The files are the same."), "{text}");
    assert!(text.contains("/srv/sub/b"), "{text}");
    assert!(!text.contains("Save"), "two places: {text}");
    // A dialog, with both whole: no filter, no View.
    assert_eq!(app.place(), (Mode::Normal, Context::Dialog));
    assert!(
        !text.contains("Filter:") && !text.contains("View"),
        "{text}"
    );
    assert_eq!(text.matches(&"01".repeat(32)).count(), 2, "{text}");
    app.handle(action(Action::Confirm));
    assert_eq!(app.take_clipboard(), Some("01".repeat(32)));
    assert!(!screen_of(&mut app, 24).contains("clipboard."), "no status");
    app.handle(action(Action::Cancel));

    app.handle(action(Action::Checksum));
    let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
    let sums = vec![sum("/srv/b", "b", Some(1)), sum("/srv/sub/b", "b", Some(2))];
    hashed(&mut app, id, sums);
    assert!(screen_of(&mut app, 24).contains("The files differ."));
}

#[test]
fn the_dialog_of_saving_takes_a_path_from_its_directory_and_completes_it_there() {
    let mut app = on_files();
    let save = |name: &str| Save {
        title: "Save notes".to_owned(),
        dir: local("/data"),
        name: name.to_owned(),
        bytes: b"x\n".to_vec(),
    };
    let written = |effects: Vec<Effect>| match one(effects) {
        Effect::WriteFile {
            location,
            bytes,
            replace,
            ..
        } => (location, bytes, replace),
        other => panic!("expected a write, got {other:?}"),
    };
    app.ask_save(save("notes.txt"));
    let text = screen_of(&mut app, 24);
    assert!(
        text.contains("Save notes") && text.contains("File name:"),
        "{text}"
    );
    assert!(text.contains("notes.txt"), "{text}");
    assert_eq!(
        written(app.handle(action(Action::Confirm))),
        (local("/data/notes.txt"), b"x\n".to_vec(), false)
    );
    // Tab lists the directory of the file, not that of the panel, on no other host.
    app.ask_save(save(""));
    type_text(&mut app, "sub/");
    let (_, dir, host, hosts) = names_request(&app.handle(action(Action::Complete)));
    assert_eq!((dir, host, hosts), (local("/data/sub"), false, false));
    type_text(&mut app, "n");
    let (location, ..) = written(app.handle(action(Action::Confirm)));
    assert_eq!(location, local("/data/sub/n"));
    for (typed, location) in [("~/n", "/home/me/n"), ("/tmp/n", "/tmp/n")] {
        app.ask_save(save(typed));
        let (written, ..) = written(app.handle(action(Action::Confirm)));
        assert_eq!(written, local(location), "{typed}");
    }
    // Nothing typed saves nothing.
    app.ask_save(save(""));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert!(app.dialogs.is_empty());
}

#[test]
fn checksums_of_many_files_are_saved_as_sha256sum_writes_them() {
    let mut app = on_files();
    app.handle(action(Action::Home));
    app.handle(action(Action::Down));
    app.handle(action(Action::Mark));
    app.handle(action(Action::Mark));
    app.handle(action(Action::Checksum));
    let text = screen_of(&mut app, 20);
    assert!(
        text.contains("Checksums of 1 file and 1 directory"),
        "{text}"
    );
    assert!(
        text.contains("Expected checksum:"),
        "shown, disabled: {text}"
    );
    assert!(text.contains("Only for a single file"), "why: {text}");
    assert!(!text.contains("Optional:"), "{text}");
    assert_eq!(
        app.place(),
        (Mode::Normal, Context::Dialog),
        "on the combo box"
    );
    // The disabled field takes no focus, neither from the keys that move it nor from its
    // letter: Tab goes from the combo box to the buttons.
    app.handle(action(Action::NextField));
    app.handle(Resolved::Hotkey('e'));
    assert_eq!(app.place(), (Mode::Normal, Context::Dialog));
    let (id, targets, _) = checksum_job(app.handle(action(Action::Confirm)));
    assert_eq!(targets, [(vec![local("/srv/sub"), local("/srv/a")], false)]);
    let sums = vec![
        sum("/srv/sub/x", "sub/x", Some(1)),
        sum("/srv/sub/y", "sub/y", None),
        sum("/srv/a", "a", Some(2)),
    ];
    hashed(&mut app, id, sums);
    let text = screen_of(&mut app, 24);
    assert!(text.contains("skipped"), "{text}");
    // From the rows to Copy, then Copy all; View, Save.
    app.handle(action(Action::Right));
    app.handle(action(Action::Right));
    app.handle(action(Action::Confirm));
    let all = app.take_clipboard().unwrap();
    let line = |byte: &str, name: &str| format!("{}  {name}\n", byte.repeat(32));
    assert_eq!(all, line("01", "sub/x") + &line("02", "a"));
    app.handle(action(Action::Right));
    app.handle(action(Action::Right));
    app.handle(action(Action::Confirm));
    let text = screen_of(&mut app, 24);
    assert!(text.contains("SHA256SUMS"), "{text}");
    let written = |effects: Vec<Effect>| match one(effects) {
        Effect::WriteFile {
            location,
            bytes,
            replace,
            host,
        } => {
            assert!(host.is_none());
            (location, String::from_utf8(bytes).unwrap(), replace)
        }
        other => panic!("expected a write, got {other:?}"),
    };
    let (location, text, replace) = written(app.handle(action(Action::Confirm)));
    assert_eq!(
        (&location, &text, replace),
        (&local("/srv/SHA256SUMS"), &all, false)
    );

    // Taken: the question of a copy, for one file, with Cancel the default; New views what
    // would be written, Rename… writes under the free name it offers, and Overwrite over
    // the file there.
    assert!(
        app.written(&location, taken(None, 5, "SHA256SUMS_2"))
            .is_empty()
    );
    let text = screen_of(&mut app, 24);
    assert!(
        text.contains("File exists") && text.contains("/srv/SHA256SUMS"),
        "{text}"
    );
    assert!(text.contains("New") && text.contains("Existing"), "{text}");
    assert!(text.contains("5 bytes"), "{text}");
    assert!(
        text.contains("[ Overwrite ] [ Rename… ] { Cancel }"),
        "{text}"
    );
    assert!(!text.contains("The rest"), "one file: {text}");
    app.handle(Resolved::Hotkey('n'));
    assert!(screen_of(&mut app, 24).contains("01010101"), "the new file");
    app.handle(action(Action::Quit));
    assert!(screen_of(&mut app, 24).contains("File exists"), "back");
    app.handle(Resolved::Hotkey('r'));
    let (location, _, replace) = written(app.handle(action(Action::Confirm)));
    assert_eq!((&location, replace), (&local("/srv/SHA256SUMS_2"), false));
    // That name is taken too: the question again.
    assert!(
        app.written(&location, taken(None, 5, "SHA256SUMS_3"))
            .is_empty()
    );
    app.handle(action(Action::Left));
    app.handle(action(Action::Left));
    let (overwritten, _, replace) = written(app.handle(action(Action::Confirm)));
    assert_eq!((&overwritten, replace), (&location, true));
    let effects = app.written(&location, Ok(()));
    assert_eq!(effects.len(), 2, "both panels show /srv, where it is");
    assert!(!screen_of(&mut app, 24).contains("Saved to"), "no status");
}

#[test]
fn the_checksums_filter_finds_a_file_and_f3_views_them_all_then_comes_back() {
    let mut app = on_files();
    let first = app.search_opens(Slot::Checksums).way();
    app.handle(action(Action::Home));
    app.handle(action(Action::Down));
    app.handle(action(Action::Mark));
    app.handle(action(Action::Mark));
    app.handle(action(Action::Checksum));
    let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
    let sums = vec![
        sum("/srv/sub/x", "sub/x", Some(1)),
        sum("/srv/sub/y", "sub/y", None),
        sum("/srv/a", "a", Some(2)),
    ];
    hashed(&mut app, id, sums);
    assert_eq!(app.place(), (Mode::Normal, Context::Checksums));
    assert_eq!(mode_of(&mut app), "NORMAL · Dialog");
    let text = screen_of(&mut app, 30);
    assert!(text.contains("Filter:"), "{text}");
    assert!(text.contains("[ View ]"), "{text}");
    keys(&mut app, &["/"]);
    assert_eq!(app.place(), (Mode::Insert, Context::Checksums));
    assert_eq!(app.key_place().search, Some(search::Kind::FILTER));
    keys(&mut app, &["s", "u", "b", "/", "y"]);
    let text = screen_of(&mut app, 30);
    assert!(!text.contains("02020202"), "a is hidden: {text}");
    // Its keys, and the one that switches how it matches, which it keeps.
    keys(&mut app, &["ctrl-s", "ctrl-s"]);
    keys(&mut app, &["esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Checksums));
    keys(&mut app, &["enter"]);
    assert_eq!(app.take_clipboard(), None, "sub/y was skipped");
    keys(&mut app, &["/", "ctrl-u", "a", "enter"]);
    assert_eq!(app.take_clipboard(), Some("02".repeat(32)));
    // F3 shows every line in the viewer, in place of the window.
    assert!(keys(&mut app, &["f3"]).is_empty(), "nothing to read");
    assert_eq!(app.place(), (Mode::Normal, Context::Viewer));
    let text = screen_of(&mut app, 30);
    assert!(text.contains("SHA256SUMS"), "{text}");
    assert!(
        text.contains(&format!("{}  sub/x", "01".repeat(32))),
        "{text}"
    );
    assert!(text.contains(&format!("{}  a", "02".repeat(32))), "{text}");
    assert!(!text.contains("Filter:"), "{text}");
    // q closes the viewer, and the window comes back as it was, its filter typing.
    keys(&mut app, &["q"]);
    assert_eq!(app.place(), (Mode::Insert, Context::Checksums));
    let text = screen_of(&mut app, 30);
    assert!(text.contains("Filter: a"), "{text}");
    keys(&mut app, &["esc", "esc"]);
    assert_eq!(app.context(), Context::Panel);
    let opens = app.search_opens(Slot::Checksums).way();
    assert_ne!(opens, first, "as Ctrl+s Ctrl+s left it");
}

#[test]
fn an_aborted_checksum_job_shows_nothing_and_an_empty_one_says_so() {
    let mut app = on_files();
    app.handle(action(Action::Checksum));
    let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
    app.job_event(id, JobEvent::Finished { complete: false });
    assert_eq!(app.context(), Context::Panel);
    app.handle(action(Action::Checksum));
    let (id, _, _) = checksum_job(app.handle(action(Action::Confirm)));
    hashed(&mut app, id, Vec::new());
    assert!(screen_of(&mut app, 20).contains("There are no files to hash."));
    assert_eq!(
        app.dialogs.front().map(|open| open.dialog.tone()),
        Some(Tone::Warning),
        "a warning"
    );
}

#[test]
fn expected_checksums_and_files_of_them() {
    assert_eq!(
        parse_expected(&"A".repeat(64), Algorithm::Blake3),
        Some(("a".repeat(64), Algorithm::Blake3)),
        "the choice, where its length fits"
    );
    assert_eq!(
        parse_expected(&"a".repeat(64), Algorithm::Md5),
        Some(("a".repeat(64), Algorithm::Sha256))
    );
    assert_eq!(
        parse_expected(&"a".repeat(128), Algorithm::Sha256).map(|(_, a)| a),
        Some(Algorithm::Sha512)
    );
    assert_eq!(parse_expected(&"a".repeat(63), Algorithm::Sha256), None);
    assert_eq!(parse_expected("sha256:abc", Algorithm::Sha256), None);
    let lines = [
        (b"plain".to_vec(), Some("01".to_owned())),
        (b"gone".to_vec(), None),
        (b"two\nlines\\x".to_vec(), Some("02".to_owned())),
    ];
    assert_eq!(
        sums_file(&lines),
        b"01  plain\n\\02  two\\nlines\\\\x\n".to_vec()
    );
}
