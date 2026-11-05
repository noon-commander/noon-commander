use super::*;

#[test]
fn the_help_opens_on_about_and_then_where_it_was_left() {
    let mut app = loaded();
    app.handle(action(Action::Help));
    let text = screen_of(&mut app, 30);
    assert!(
        text.contains("Noon Commander") && text.contains("Search and renaming"),
        "{text}"
    );
    // The section and how far down it was last until the help opens again, from anywhere.
    app.handle(action(Action::NextField));
    for _ in 0..4 {
        app.handle(action(Action::Down));
    }
    app.handle(action(Action::Confirm));
    let top = screen_of(&mut app, 30);
    assert!(top.contains("Text fields in Insert mode"), "{top}");
    app.handle(action(Action::Down));
    let left = screen_of(&mut app, 30);
    assert!(!left.contains("Text fields in Insert mode"), "{left}");
    app.handle(action(Action::Cancel));
    app.handle(action(Action::Help));
    assert_eq!(screen_of(&mut app, 30), left);
    // So does Keys folded; a click on it opens it again.
    app.handle(action(Action::NextField));
    app.handle(action(Action::Left));
    app.handle(action(Action::Left));
    app.handle(action(Action::Cancel));
    app.handle(action(Action::Help));
    let text = screen_of(&mut app, 30);
    assert!(
        !text.contains("Search and renaming") && text.contains("Keys ▸"),
        "{text}"
    );
    let (x, y) = find(&mut app, " Keys ");
    press(&mut app, Press::Click, x, y);
    assert!(screen_of(&mut app, 30).contains("Search and renaming"));
    app.handle(action(Action::Cancel));
    app.handle(action(Action::Help));
    assert!(screen_of(&mut app, 30).contains("Search and renaming"));
}

#[test]
fn about_shows_the_tools_once_they_answer_and_asks_again_for_another_program() {
    let mut app = loaded();
    app.handle(action(Action::Help));
    screen_of(&mut app, 40);
    app.handle(action(Action::End));
    let text = screen_of(&mut app, 40);
    assert!(text.contains("External tools"), "{text}");
    app.detected(Tool::Zoxide, Detected::Found("zoxide 0.9.8".to_owned()));
    app.detected(Tool::Ripgrep, Detected::Found("ripgrep 15.2.0".to_owned()));
    app.detected(Tool::Fd, Detected::Found("fd 10.5.0".to_owned()));
    let text = screen_of(&mut app, 40);
    assert!(
        ["zoxide 0.9.8", "ripgrep 15.2.0", "fd 10.5.0"]
            .iter()
            .all(|version| text.contains(version)),
        "the open help shows them: {text}"
    );
    assert_eq!(app.context(), Context::Help);

    let mut new = config();
    new.zoxide.program = PathBuf::from("/opt/zoxide");
    let effects = app.apply_config(config(), new);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Detect(Tool::Zoxide))),
        "{effects:?}"
    );
    let mut new = config();
    new.search.program = PathBuf::from("/opt/rg");
    let effects = app.apply_config(config(), new);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Detect(Tool::Ripgrep))),
        "{effects:?}"
    );
    let mut new = config();
    new.search.fd = PathBuf::from("fdfind");
    let effects = app.apply_config(config(), new);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Detect(Tool::Fd))),
        "{effects:?}"
    );
    let mut new = config();
    new.ui.show_hidden = !new.ui.show_hidden;
    let effects = app.apply_config(config(), new);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Detect(_)))
    );
}

#[test]
fn f1_opens_the_help_under_any_prompt() {
    let mut app = loaded();
    app.handle(action(Action::Help));
    assert_eq!(app.context(), Context::Help);
    assert!(screen(&mut app).contains("Help"));
    assert!(screen(&mut app).contains("10Cancel"));

    let (password, answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
    app.ask(password);
    assert_eq!(
        app.context(),
        Context::DialogInput,
        "the prompt takes the keys"
    );
    assert!(screen(&mut app).contains("password"));
    app.handle(action(Action::Cancel));
    assert_eq!(answer.try_recv(), Ok(None));
    assert_eq!(app.context(), Context::Help, "back to the help");
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);
    assert!(!app.quits());
}

#[test]
fn says_at_the_start_that_it_cannot_write_the_log() {
    let mut app = loaded();
    let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    app.cannot_log(Path::new("/home/me/.local/state/noc/logs"), &error);
    let text = screen_of(&mut app, 24);
    assert!(
        text.contains("Cannot write the log")
            && text.contains("/home/me/.local/state/noc/logs")
            && text.contains("permission denied"),
        "{text}"
    );
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel, "and runs on");
}
