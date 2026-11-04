use super::*;

#[test]
fn replies_reach_their_own_panel() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    let Effect::List {
        panel: PanelId { side, .. },
        request,
        ..
    } = one(app.handle(action(Action::Enter)))
    else {
        panic!("expected a listing");
    };
    let reply = |listing| {
        let location = request.location.clone();
        Ok(Listed {
            location,
            listing,
            space: None,
        })
    };
    app.listed(
        app.shown(side.other()),
        request.generation,
        reply(Listing::Dir(Vec::new())),
    );
    app.listed(
        app.shown(side),
        request.generation,
        reply(Listing::Dir(vec![dir("deeper")])),
    );
    let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
    terminal
        .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn a_host_connects_once_and_then_lists_for_every_panel_that_waits() {
    let mut app = at_slash();
    let Effect::Connect {
        host, connection, ..
    } = one(enter_host(&mut app, Side::Left, 1))
    else {
        panic!("expected a connection");
    };
    assert_eq!(host, "web");
    assert!(
        enter_host(&mut app, Side::Right, 1).is_empty(),
        "already on its way"
    );
    let text = screen(&mut app);
    assert_eq!(text.matches("╔ … web ═").count(), 2, "{text}");

    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    let sides: Vec<_> = effects
        .iter()
        .map(|effect| match effect {
            Effect::List {
                panel: PanelId { side, .. },
                request,
                host: Some(_),
            } => {
                assert_eq!(request.location, remote("web", ""));
                *side
            }
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(sides, Side::BOTH);
    assert!(!screen(&mut app).contains("Connecting"));

    // A connected host lists at once.
    let effects = enter_host(&mut app, Side::Left, 1);
    assert!(matches!(&effects[..], [Effect::List { host: Some(_), .. }]));
}

fn with_host(app: &mut App, name: &str, host: SftpHost) {
    let mut hosts = Hosts::default();
    hosts.hosts.insert(name.to_owned(), HostConfig::Sftp(host));
    app.set_hosts(Arc::new(hosts));
}

#[test]
fn opening_a_host_sends_the_other_panel_to_its_other_dir_once() {
    let mut app = at_slash();
    let site = SftpHost {
        other_dir: Some("~/site".to_owned()),
        ..SftpHost::default()
    };
    with_host(&mut app, "web", site);
    let effects = enter_host(&mut app, Side::Left, 1);
    let [
        Effect::Connect { connection, .. },
        Effect::List {
            panel: PanelId {
                side: Side::Right, ..
            },
            request,
            host: None,
        },
    ] = &effects[..]
    else {
        panic!("expected a connection and the other directory, got {effects:?}");
    };
    assert_eq!(request.location, local("/home/me/site"));
    let request = connect_web(&mut app, Side::Left, *connection, "/home/web");
    assert_eq!(request.location, remote("web", ""));
    assert_eq!(
        app.panel(Side::Right).pending_request().map(|r| r.location),
        Some(local("/home/me/site")),
        "connecting did not send it again"
    );

    // Other hosts leave the other panel alone.
    let effects = enter_host(&mut app, Side::Right, 2);
    assert!(
        matches!(&effects[..], [Effect::Connect { .. }]),
        "{effects:?}"
    );
}

#[test]
fn bang_runs_a_command_on_the_host_of_the_panel() {
    let mut app = at_slash();
    let words = |words: &[&str]| {
        noc_config::Run::try_from(
            words
                .iter()
                .map(|&word| word.to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    let site = SftpHost {
        label: Some("Site".to_owned()),
        run: Some(words(&["$SHELL", "-c"])),
        ..SftpHost::default()
    };
    with_host(&mut app, "web", site);
    app.config.shell.run = words(&["/bin/zsh", "-ic"]);
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
        panic!("expected a connection");
    };
    connect_web(&mut app, Side::Left, connection, "/var/www");
    app.handle(action(Action::Shell));
    type_text(&mut app, "ls");
    let text = screen_of(&mut app, 10);
    assert!(text.contains("Site:/var/www $ ls"), "{text}");
    app.handle(action(Action::Confirm));
    let Some(Run {
        place: Place::Remote { dir, .. },
        shell,
        command,
        ..
    }) = app.take_run()
    else {
        panic!("expected a remote command");
    };
    assert_eq!((dir.as_bytes(), command.as_str()), (&b"/var/www"[..], "ls"));
    assert_eq!(shell, words(&["$SHELL", "-c"]), "the host's own");
    let effects = app.ran(Ok(()));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::List { request, host: Some(_), .. } if request.location == remote("web", "/var/www")
        )),
        "{effects:?}"
    );

    // The local panel runs with shell.run.
    app.handle(action(Action::SwitchPanel));
    app.handle(action(Action::Shell));
    type_text(&mut app, "ls");
    app.handle(action(Action::Confirm));
    let Some(Run {
        place: Place::Local(_),
        shell,
        ..
    }) = app.take_run()
    else {
        panic!("expected a local command");
    };
    assert_eq!(shell, words(&["/bin/zsh", "-ic"]));
}

#[test]
fn remember_dir_resumes_the_last_directory_of_the_session() {
    for remember_dir in [true, false] {
        let mut app = at_slash();
        let host = SftpHost {
            remember_dir,
            ..SftpHost::default()
        };
        with_host(&mut app, "web", host);
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
            panic!("expected a connection");
        };
        let first = connect_web(&mut app, Side::Left, connection, "/var/www");
        assert_eq!(first.resume, None);
        let effects = app.handle(action(Action::Enter));
        let [Effect::List { request, .. }] = &effects[..] else {
            panic!("expected a listing, got {effects:?}");
        };
        let listed = Listed {
            location: remote("web", "/var/www/app"),
            listing: Listing::Dir(Vec::new()),
            space: None,
        };
        app.listed(app.shown(Side::Left), request.generation, Ok(listed));

        app.disconnect("web");
        let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 1)) else {
            panic!("expected a connection");
        };
        let again = connect_web(&mut app, Side::Right, connection, "/var/www/app");
        let expected = remember_dir.then(|| RemotePath::from("/var/www/app"));
        assert_eq!(again.resume, expected, "remember_dir = {remember_dir}");
    }
}

/// Opens the dialog of F4 on `web` from the location menu of the left panel.
fn edit_web(app: &mut App) {
    if app.menu.is_none() {
        menu_on(app, Side::Left);
    }
    app.handle(action(Action::Home));
    app.handle(action(Action::Down));
    assert!(app.supports(Action::EditHost));
    assert!(app.handle(action(Action::EditHost)).is_empty());
    assert_eq!(app.context(), Context::DialogInput);
}

#[test]
fn f4_on_a_host_saves_its_settings() {
    let mut app = at_slash();
    menu_on(&mut app, Side::Left);
    assert!(!screen_of(&mut app, 14).contains("4Edit"), "not on Home");
    app.handle(action(Action::Down));
    assert!(screen_of(&mut app, 14).contains("4Edit"));
    edit_web(&mut app);
    assert!(screen_of(&mut app, 20).contains("Host web"));
    typed(&mut app, "Prod");
    let Effect::SaveHost { name, host } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected the settings to be saved");
    };
    let prod = HostConfig::Sftp(SftpHost {
        label: Some("Prod".to_owned()),
        ..SftpHost::default()
    });
    assert_eq!((name.as_str(), host.as_ref()), ("web", Some(&prod)));

    let mut hosts = Hosts::default();
    hosts.hosts.insert("web".to_owned(), prod.clone());
    let effects = app.host_saved("web".to_owned(), None, Ok(Arc::new(hosts)));
    assert!(
        matches!(&effects[..], [Effect::ListPlaces { .. }]),
        "the menu reads the hosts again"
    );

    // The dialog opens with the settings; unchanged, nothing is saved, and emptied, they go.
    edit_web(&mut app);
    assert!(app.handle(action(Action::Confirm)).is_empty());
    edit_web(&mut app);
    app.handle(action(Action::DeleteToStart));
    let Effect::SaveHost { host, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected the settings to be removed");
    };
    assert_eq!(host, None);

    let denied = "cannot write /cfg/hosts.toml: denied".to_owned().into();
    app.host_saved("web".to_owned(), Some(prod.clone()), Err(denied));
    let text = screen_of(&mut app, 20);
    assert!(text.contains("Cannot save the host settings"), "{text}");
    // Retry writes them again.
    let Effect::SaveHost { name, host } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected the settings to be saved again");
    };
    assert_eq!((name.as_str(), host.as_ref()), ("web", Some(&prod)));
}

#[test]
fn f4_on_a_host_rejects_another_kind_of_other_dir() {
    let mut app = at_slash();
    edit_web(&mut app);
    app.handle(action(Action::Down));
    app.handle(action(Action::Down));
    typed(&mut app, "web:/srv");
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.dialogs.len(), 2, "the reason shows over the dialog");
    app.handle(action(Action::Confirm));
    app.handle(action(Action::DeleteToStart));
    typed(&mut app, "~/site");
    let Effect::SaveHost { host, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected the settings to be saved");
    };
    assert_eq!(
        host.as_ref().and_then(HostConfig::other_dir),
        Some("~/site")
    );
}

#[test]
fn f4_on_a_host_shows_shell_run_until_it_gets_a_run_of_its_own() {
    let mut app = at_slash();
    edit_web(&mut app);
    let text = screen_of(&mut app, 24);
    assert!(text.contains("$SHELL -i -c"), "the placeholder: {text}");
    for _ in 0..3 {
        app.handle(action(Action::Down));
    }
    typed(&mut app, "${SHELL -c");
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.dialogs.len(), 2, "the reason shows over the dialog");
    let text = screen_of(&mut app, 24);
    assert!(text.contains("has no closing }"), "{text}");
    app.handle(action(Action::Confirm));
    app.handle(action(Action::DeleteToStart));
    typed(&mut app, "$SHELL -c");
    let Effect::SaveHost { host, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected the settings to be saved");
    };
    let run = host.as_ref().and_then(HostConfig::run).unwrap();
    assert_eq!(Vec::<String>::from(run.clone()), ["$SHELL", "-c"]);
}

#[test]
fn use_current_fills_in_the_directory_open_on_the_host() {
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Right, 1)) else {
        panic!("expected a connection");
    };
    connect_web(&mut app, Side::Right, connection, "/var/www");
    edit_web(&mut app);
    // Past the four fields and the check box, then OK, to Use Current.
    for _ in 0..6 {
        app.handle(action(Action::NextField));
    }
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.dialogs.len(), 1, "the dialog stays");
    let Effect::SaveHost { host, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected the settings to be saved");
    };
    let start_dir = host.as_ref().and_then(HostConfig::start_dir);
    assert_eq!(start_dir, Some("/var/www"));
}

#[test]
fn a_failed_connection_asks_to_retry_while_the_panel_waits() {
    let mut app = at_slash();
    // The dots turn, so that whether they are due says what waits, however long the test
    // takes: without turning, they are due only before they first show.
    app.config.animations.spinners = true;
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
        panic!("expected a connection");
    };
    let refused = "deploy@db: Permission denied (publickey).";
    assert!(app.closed("db", connection, Some(refused)).is_empty());
    assert!(
        app.closed("db", connection, Some(refused)).is_empty(),
        "reported once"
    );
    assert_eq!(app.host_state("db").status, HostStatus::Stalled);
    assert_eq!(app.dialogs.len(), 1);
    let text = screen_of(&mut app, 20);
    assert_eq!(
        app.spin_due(Instant::now()),
        None,
        "the panel waits for the answer, not for work: nothing turns"
    );
    assert!(
        shows_problem(
            &text,
            "Cannot connect to db",
            None,
            "deploy@db: Permission denied (publickey)."
        ),
        "{text}"
    );
    assert!(text.contains("{ Retry } [ Cancel ]"), "{text}");
    assert!(!text.contains("Retry or Cancel"), "the dialog asks: {text}");

    // Retry, the default, connects again for the panel that still waits.
    let Effect::Connect {
        connection: again, ..
    } = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected a new connection");
    };
    assert_ne!(again, connection);
    assert_eq!(app.host_state("db").status, HostStatus::Connecting);
    assert!(
        app.spin_due(Instant::now()).is_some(),
        "the dots turn again"
    );
    let waiting = app.panel(Side::Left).pending_request();
    assert_eq!(
        waiting.map(|request| request.location),
        Some(remote("db", ""))
    );

    // Cancel: the panel stays where it was; the dialog said why, so no other one does.
    assert!(app.closed("db", again, Some(refused)).is_empty());
    assert!(app.handle(action(Action::Cancel)).is_empty());
    assert_eq!(app.host_state("db").status, HostStatus::Failed);
    assert_eq!(app.panel(Side::Left).location(), &local("/"));
    assert_eq!(app.panel(Side::Left).pending_request(), None);
    assert!(app.dialogs.is_empty());
    assert_eq!(app.panel(Side::Left).not_connected(), None);
    let text = screen(&mut app);
    assert!(!text.contains("Cannot open"), "{text}");
    let Effect::Connect { .. } = one(enter_host(&mut app, Side::Left, 2)) else {
        panic!("expected a new connection");
    };
}

#[test]
fn cancel_stops_a_connection_attempt_for_every_panel() {
    let mut app = at_slash();
    let Effect::Connect {
        connection, stop, ..
    } = one(enter_host(&mut app, Side::Left, 1))
    else {
        panic!("expected a connection");
    };
    enter_host(&mut app, Side::Right, 1);
    app.handle(action(Action::Cancel));
    assert!(stop.is_cancelled());
    assert!(!screen(&mut app).contains("Connecting"));
    for side in Side::BOTH {
        assert_eq!(app.panel(side).pending_request(), None);
    }

    // Reports of the stopped attempt change nothing.
    let (handle, _requests) = HostHandle::channel();
    assert!(app.connected("web", connection, handle).is_empty());
    assert!(app.closed("web", connection, None).is_empty());
    assert!(app.hosts.is_empty());
    // Nothing to stop does nothing.
    assert!(app.handle(action(Action::Cancel)).is_empty());
}

#[test]
fn a_lost_connection_tones_down_its_panels_and_asks_to_retry() {
    let mut app = at_slash();
    let Effect::Connect {
        connection, stop, ..
    } = one(enter_host(&mut app, Side::Left, 1))
    else {
        panic!("expected a connection");
    };
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
    assert!(screen(&mut app).contains("www"));

    assert!(
        app.closed("web", connection, Some("Broken pipe"))
            .is_empty()
    );
    assert!(!stop.is_cancelled(), "it ended on its own");
    let text = screen_of(&mut app, 20);
    assert!(text.contains("www"), "the entries stay: {text}");
    assert!(
        shows_problem(&text, "Lost the connection to web", None, "Broken pipe"),
        "{text}"
    );
    assert_eq!(app.dialogs.len(), 1);
    assert_eq!(app.panel(Side::Left).location(), &remote("web", ""));
    assert_eq!(
        app.spin_due(Instant::now()),
        None,
        "the dialog may wait for hours; the screen sleeps"
    );

    // Retry reads the directory again once the host is back.
    let Effect::Connect { connection, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected a new connection");
    };
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected the directory, got {effects:?}");
    };
    assert_eq!(request.location, remote("web", ""));
    answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
    assert!(screen(&mut app).contains("www"));

    // Cancel leaves the panel on the host, with its entries toned down, until it reads it
    // again.
    app.closed("web", connection, Some("Broken pipe"));
    app.handle(action(Action::Cancel));
    assert!(app.dialogs.is_empty(), "the dialog said why already");
    assert_eq!(app.panel(Side::Left).not_connected(), Some("web"));
    let text = screen(&mut app);
    assert!(text.contains("www"), "{text}");
    assert!(!text.contains("Cannot open"), "{text}");
    let Effect::Connect { .. } = one(app.handle(action(Action::Reload))) else {
        panic!("expected a new connection");
    };
}

#[test]
fn f8_in_the_location_menu_disconnects_the_host_under_the_cursor() {
    let mut app = at_slash();
    let Effect::Connect {
        connection, stop, ..
    } = one(enter_host(&mut app, Side::Left, 1))
    else {
        panic!("expected a connection");
    };
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    answer(&mut app, effects, &Listing::Dir(vec![dir("www")]));
    assert_eq!(app.host_state("web").status, HostStatus::Connected);
    assert!(!screen(&mut app).contains("Disconn"), "F8 deletes here");

    // From the other panel's menu.
    menu_on(&mut app, Side::Right);
    assert!(screen_of(&mut app, 14).contains("8Disconn"));
    app.handle(action(Action::Down));
    assert!(app.handle(action(Action::Disconnect)).is_empty());
    assert!(stop.is_cancelled());
    assert_eq!(app.host_state("web").status, HostStatus::Idle);
    assert!(
        app.closed("web", connection, None).is_empty(),
        "already gone"
    );
    app.handle(action(Action::Cancel));
    let text = screen(&mut app);
    assert!(text.contains("www"), "the entries stay: {text}");
    assert_eq!(app.panel(Side::Left).not_connected(), Some("web"));
    assert!(app.dialogs.is_empty(), "asked for, so no dialog");
    assert_eq!(app.panel(Side::Left).location(), &remote("web", ""));
    // Nothing to close, or not a host.
    menu_on(&mut app, Side::Right);
    app.handle(action(Action::Down));
    assert!(app.handle(action(Action::Disconnect)).is_empty());
    app.handle(action(Action::Home));
    assert!(app.handle(action(Action::Disconnect)).is_empty());
}

#[test]
fn prompts_wait_their_turn_and_take_the_keys() {
    let mut app = at_slash();
    let (password, password_answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
    let (host_key, host_key_answer) = ask(2, PromptKind::HostKey, "Continue (yes/no)? ");
    app.ask(password);
    app.ask(host_key);
    assert_eq!(app.context(), Context::DialogInput);
    let text = screen(&mut app);
    assert!(text.contains("deploy@web's password:"), "{text}");
    assert!(!text.contains("Continue"), "the second waits: {text}");
    assert!(text.contains("10Cancel"), "{text}");

    for c in "s3cret".chars() {
        app.handle(Resolved::Insert(c));
    }
    assert!(screen(&mut app).contains("║ ****** "));
    app.handle(action(Action::Confirm));
    assert_eq!(password_answer.try_recv(), Ok(Some("s3cret".to_owned())));

    assert_eq!(app.context(), Context::Dialog);
    assert!(screen(&mut app).contains("Continue (yes/no)?"));
    // Panel keys do nothing while a dialog is open; F10 closes the dialog, not the app.
    app.handle(action(Action::Down));
    app.handle(action(Action::Cancel));
    assert!(!app.quits());
    assert_eq!(host_key_answer.try_recv(), Ok(None));
    assert_eq!(app.context(), Context::Panel);
}

#[test]
fn declining_a_password_or_a_question_of_the_server_stops_connecting() {
    for kind in [PromptKind::Secret, PromptKind::Question] {
        let mut app = at_slash();
        let Effect::Connect {
            connection, stop, ..
        } = one(enter_host(&mut app, Side::Left, 1))
        else {
            panic!("expected a connection");
        };
        let (prompt, answers) = ask(1, kind, "deploy@web's password: ");
        app.ask(prompt);
        app.handle(action(Action::Cancel));
        assert!(stop.is_cancelled(), "{kind:?}: ssh would ask again");
        assert_eq!(
            answers.try_recv(),
            Err(mpsc::TryRecvError::Empty),
            "{kind:?}: ssh would send an empty answer to the server"
        );
        assert!(app.closed("web", connection, None).is_empty());
        assert_eq!(
            answers.try_recv(),
            Err(mpsc::TryRecvError::Disconnected),
            "{kind:?}: let go once the attempt ends"
        );
        assert!(app.dialogs.is_empty(), "{kind:?}: asked for, so no dialog");
        assert_eq!(app.panel(Side::Left).pending_request(), None, "{kind:?}");
    }
}

#[test]
fn answers_that_are_not_a_refusal_go_to_ssh_and_connecting_goes_on() {
    let mut app = at_slash();
    let Effect::Connect { stop, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
        panic!("expected a connection");
    };
    let (password, password_answer) = ask(1, PromptKind::Secret, "Enter passphrase: ");
    app.ask(password);
    app.handle(action(Action::Confirm));
    assert_eq!(
        password_answer.try_recv(),
        Ok(Some(String::new())),
        "OK on nothing skips a key, as in ssh"
    );
    let (question, question_answer) = ask(2, PromptKind::Question, "Continue (yes/no)? ");
    app.ask(question);
    app.handle(action(Action::Confirm));
    assert_eq!(question_answer.try_recv(), Ok(Some("no".to_owned())));
    let (host_key, host_key_answer) = ask(3, PromptKind::HostKey, "Continue (yes/no)? ");
    app.ask(host_key);
    app.handle(action(Action::Cancel));
    assert_eq!(
        host_key_answer.try_recv(),
        Ok(None),
        "ssh ends on its own after a host key is declined"
    );
    assert!(!stop.is_cancelled());

    // Declined without an attempt to stop, the prompt is answered.
    let mut app = at_slash();
    let (password, password_answer) = ask(1, PromptKind::Secret, "Password: ");
    app.ask(password);
    app.handle(action(Action::Cancel));
    assert_eq!(password_answer.try_recv(), Ok(None));
}

#[test]
fn dialogs_close_when_ssh_stops_waiting() {
    let mut app = at_slash();
    let (first, first_answer) = ask(1, PromptKind::Secret, "first");
    let (second, second_answer) = ask(2, PromptKind::Confirm, "second");
    app.ask(first);
    app.ask(second);
    app.notice(3, "web", "Confirm user presence for key ED25519-SK");
    app.prompt_closed(2);
    app.prompt_closed(1);
    assert!(first_answer.try_recv().is_err(), "nobody to answer");
    assert!(second_answer.try_recv().is_err());
    assert!(screen(&mut app).contains("Confirm user presence"));
    app.handle(action(Action::Confirm));
    assert_eq!(app.context(), Context::Panel, "OK dismisses a notice");

    app.notice(4, "web", "Touch your key");
    app.prompt_closed(4);
    assert_eq!(app.context(), Context::Panel);
}
