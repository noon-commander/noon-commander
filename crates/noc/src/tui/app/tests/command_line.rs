use super::*;

#[test]
fn bang_runs_a_shell_command_in_the_panels_directory() {
    let mut app = loaded();
    assert!(app.handle(action(Action::Shell)).is_empty());
    assert_eq!(app.context(), Context::CommandLine);
    type_text(&mut app, "make");
    app.handle(action(Action::NewLine));
    type_text(&mut app, "ls");
    let text = screen_of(&mut app, 10);
    assert!(
        text.contains("/srv $ make") && text.contains("> ls"),
        "{text}"
    );
    let effects = without_zoxide(app.handle(action(Action::Confirm)));
    assert!(
        matches!(&effects[..], [Effect::History(HistoryChange::Add { .. })]),
        "{effects:?}"
    );
    assert_eq!(app.context(), Context::Panel);
    let Some(ran) = app.take_run() else {
        panic!("expected a local command");
    };
    let Run {
        place: Place::Local(dir),
        command,
        prompt,
        ..
    } = &ran
    else {
        panic!("expected a local command");
    };
    assert_eq!(
        (dir.as_path(), command.as_str(), prompt.as_str()),
        (Path::new("/srv"), "make\nls", "/srv $ ")
    );
    assert!(app.take_run().is_none());
    let effects = app.ran(Ok(()));
    assert_eq!(effects.len(), 2, "both panels read /srv again");
    let missing = Reason {
        text: "No such file or directory".to_owned(),
        code: Some(Code::Os(2)),
    };
    let problem = Problem::new("Cannot run /bin/nosh".to_owned(), missing);
    app.ran(Err((problem, ran)));
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot run /bin/nosh",
            None,
            "No such file or directory"
        ),
        "{text}"
    );
    assert!(text.contains("Code: 2"), "{text}");
    // Retry runs the command again.
    app.handle(action(Action::Confirm));
    let again = app.take_run().map(|run| run.command);
    assert_eq!(again.as_deref(), Some("make\nls"));
}

#[test]
fn colon_needs_a_bang_and_a_host_its_connection() {
    let mut app = loaded();
    app.handle(action(Action::Command));
    assert!(screen(&mut app).contains(':'));
    type_text(&mut app, "q");
    app.handle(action(Action::Confirm));
    assert!(screen_of(&mut app, 12).contains("Unknown command: q"));
    app.handle(action(Action::Confirm));
    assert_eq!(app.context(), Context::CommandLine, "the line stays");
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
        panic!("expected a connection");
    };
    connect_web(&mut app, Side::Left, connection, "/var/www");
    app.disconnect("web");
    app.handle(action(Action::Shell));
    assert!(
        screen_of(&mut app, 12).contains("Not connected to web"),
        "it asks to connect first"
    );
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);
}

#[test]
fn commands_go_to_the_history_of_their_host_but_not_after_a_space() {
    let mut app = loaded();
    let effects = run(&mut app, "make");
    let [Effect::History(HistoryChange::Add { entry, size: 500 })] = &effects[..] else {
        panic!("expected the command to be kept, got {effects:?}");
    };
    assert_eq!(
        (
            entry.command.as_str(),
            entry.host.as_deref(),
            entry.dir.as_str()
        ),
        ("make", None, "/srv")
    );
    run(&mut app, "ls");
    assert!(run(&mut app, " secret").is_empty(), "a space keeps it out");
    // A command of another host does not come up here.
    app.history_changed(
        HistoryChange::Load,
        Ok(vec![
            entry.clone(),
            HistoryEntry {
                command: "uptime".to_owned(),
                host: Some("web".to_owned()),
                ..entry.clone()
            },
            HistoryEntry {
                command: "ls".to_owned(),
                ..entry.clone()
            },
        ]),
    );
    app.handle(action(Action::Shell));
    type_text(&mut app, "dr");
    app.handle(action(Action::Up));
    assert_eq!(app.command_line.as_ref().map(CommandLine::text), Some("ls"));
    app.handle(action(Action::Up));
    assert_eq!(
        app.command_line.as_ref().map(CommandLine::text),
        Some("make")
    );
    app.handle(action(Action::Up));
    assert_eq!(
        app.command_line.as_ref().map(CommandLine::text),
        Some("make")
    );
    app.handle(action(Action::NewerCommand));
    app.handle(action(Action::NewerCommand));
    assert_eq!(app.command_line.as_ref().map(CommandLine::text), Some("dr"));
    app.handle(action(Action::Cancel));

    app.config.shell.history_size = 0;
    assert!(run(&mut app, "make").is_empty(), "no history at all");
    let invalid = Err("history.toml: invalid".to_owned().into());
    app.history_changed(HistoryChange::Load, invalid);
    assert!(screen_of(&mut app, 12).contains("Cannot keep the command history"));
}

#[test]
fn alt_h_takes_a_command_from_the_history_window_and_delete_removes_one() {
    let mut app = loaded();
    let entry = |command: &str, host: Option<&str>| HistoryEntry {
        command: command.to_owned(),
        host: host.map(str::to_owned),
        dir: "/srv".to_owned(),
        time: 0,
    };
    app.history_changed(
        HistoryChange::Load,
        Ok(vec![entry("make", None), entry("uptime", Some("web"))]),
    );
    app.handle(action(Action::CommandHistory));
    assert_eq!(
        app.context(),
        Context::CommandHistory,
        "q : opens the line and the window"
    );
    let text = screen_of(&mut app, 16);
    assert!(text.contains("make") && !text.contains("uptime"), "{text}");
    app.handle(action(Action::NextField));
    app.handle(action(Action::Home));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.context(), Context::CommandLine);
    assert_eq!(
        app.command_line.as_ref().map(CommandLine::text),
        Some("uptime"),
        "it is put on the line, not run"
    );
    assert!(app.take_run().is_none());

    app.handle(action(Action::CommandHistory));
    let effects = app.handle(action(Action::Delete));
    let [
        Effect::History(HistoryChange::Remove {
            host: None,
            command,
        }),
    ] = &effects[..]
    else {
        panic!("expected a removal, got {effects:?}");
    };
    assert_eq!(command, "make");
    assert_eq!(app.history.len(), 1);
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::CommandLine);
}

#[test]
fn ctrl_o_shows_the_output_from_a_panel_and_the_command_line() {
    let mut app = loaded();
    app.handle(action(Action::UserScreen));
    assert!(matches!(app.take_handover(), Some(Handover::UserScreen)));
    app.handle(action(Action::Shell));
    type_text(&mut app, "ls");
    app.handle(action(Action::UserScreen));
    assert!(matches!(app.take_handover(), Some(Handover::UserScreen)));
    assert_eq!(
        app.command_line.as_ref().map(CommandLine::text),
        Some("ls"),
        "the line stays as it was"
    );
}

#[test]
fn ctrl_o_shows_the_output_over_whatever_is_in_front_which_stays() {
    for opens in [
        Action::QuickSearch,
        Action::Mkdir,
        Action::Help,
        Action::PullDown,
        Action::Jobs,
    ] {
        let mut app = loaded();
        app.handle(action(opens));
        let context = app.context();
        assert_ne!(context, Context::Panel, "{opens:?} opens something");
        app.handle(action(Action::UserScreen));
        assert!(
            matches!(app.take_handover(), Some(Handover::UserScreen)),
            "{opens:?}"
        );
        assert_eq!(app.context(), context, "{opens:?} stays in front");
    }
}

#[test]
fn ctrl_l_redraws_under_whatever_is_in_front_which_stays() {
    for opens in [
        Action::QuickSearch,
        Action::Mkdir,
        Action::Help,
        Action::PullDown,
        Action::Jobs,
        Action::Shell,
    ] {
        let mut app = loaded();
        app.handle(action(opens));
        let context = app.context();
        assert_ne!(context, Context::Panel, "{opens:?} opens something");
        app.handle(action(Action::Redraw));
        assert!(app.take_redraw(), "{opens:?}");
        assert_eq!(app.context(), context, "{opens:?} stays in front");
    }
}

#[test]
fn pasted_text_never_runs_and_panels_ignore_it() {
    let mut app = loaded();
    assert!(app.paste("+*!\r").is_empty());
    assert_eq!(app.context(), Context::Panel, "no key of the panel ran");
    app.handle(action(Action::Shell));
    app.paste("make\nls\n");
    assert_eq!(app.context(), Context::CommandLine, "nothing ran");
    assert_eq!(
        app.command_line.as_ref().map(CommandLine::text),
        Some("make\nls\n")
    );
    app.handle(action(Action::Cancel));
    app.handle(action(Action::QuickSearch));
    app.paste("ri\nght");
    assert!(screen(&mut app).contains(&prompt("Search", "right")));
}

#[test]
fn the_prompt_shows_the_directory_with_a_tilde_for_home() {
    let (mut app, _) = App::new(
        Path::new("/home/me/src/noc"),
        Path::new("/home/me"),
        &config(),
    );
    app.handle(action(Action::Shell));
    let prompt = |app: &App, width| cells::text_of(&app.command_prompt(width));
    assert_eq!(prompt(&app, 80), "~/src/noc $ ");
    assert_eq!(prompt(&app, 18), "~/s»oc $ ", "a third of the width");
    let mark = app
        .command_prompt(18)
        .into_iter()
        .find(|span| span.content == "»");
    assert_eq!(
        mark.map(|span| span.style),
        Some(app.theme.truncated),
        "the mark stands out"
    );
    app.command_line = Some(CommandLine::new(command::Kind::Noc));
    assert_eq!(prompt(&app, 80), ":");
}

#[test]
fn the_terminal_title_shows_the_active_panel_unless_turned_off() {
    let (mut app, _) = App::new(
        Path::new("/home/me/src/noc"),
        Path::new("/home/me"),
        &config(),
    );
    assert_eq!(app.terminal_title().as_deref(), Some("~/src/noc — noc"));
    app.config.ui.terminal_title = false;
    assert_eq!(app.terminal_title(), None);

    let (mut app, effects) = App::new(Path::new("/"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    assert_eq!(app.terminal_title().as_deref(), Some("/ — noc"));
}
