use super::*;

#[test]
fn ctrl_c_in_the_list_of_completions_closes_the_dialog_as_everywhere() {
    let mut app = loaded();
    app.handle(action(Action::GoToDir));
    type_text(&mut app, "left/");
    let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
    app.names(generation, Ok(names_of(&[("a", true), ("b", true)])), &[]);
    assert_eq!(app.context(), Context::Completion);
    keys(&mut app, &["ctrl-c"]);
    assert!(app.dialogs.is_empty());
}

#[test]
fn tab_completes_go_to_dir_from_the_directory_typed() {
    let mut app = loaded();
    app.handle(action(Action::GoToDir));
    type_text(&mut app, "left/pr");
    let (generation, dir, through, hosts) = names_request(&app.handle(action(Action::Complete)));
    assert_eq!((dir, through, hosts), (local("/srv/left"), false, false));
    let entries = [
        ("project-b", true),
        ("project-a", true),
        ("press.txt", false),
    ];
    app.names(generation, Ok(names_of(&entries)), &[]);
    assert_eq!(
        field(&app),
        "left/project-a/|",
        "the first in the list's order, whole; no files"
    );
    assert_eq!(app.place(), (Mode::Insert, Context::Completion));
    let text = screen_of(&mut app, 20);
    assert!(
        text.contains("project-a/") && text.contains("project-b/"),
        "{text}"
    );

    // Moves put each match in at once, round through what was typed, as in vim.
    let mut after = |keys_typed: &[&str]| {
        keys(&mut app, keys_typed);
        field(&app)
    };
    assert_eq!(after(&["tab"]), "left/project-b/|");
    assert_eq!(
        after(&["tab"]),
        "left/pr|",
        "after the last, what was typed"
    );
    assert_eq!(after(&["ctrl-n"]), "left/project-a/|");
    assert_eq!(after(&["backtab"]), "left/pr|", "before the first, too");
    assert_eq!(after(&["backtab"]), "left/project-b/|");
    assert_eq!(after(&["ctrl-p"]), "left/project-a/|");
    assert_eq!(after(&["up"]), "left/pr|");
    assert_eq!(after(&["down"]), "left/project-a/|");
    assert_eq!(after(&["pagedown"]), "left/project-b/|");
    assert_eq!(after(&["pagedown"]), "left/pr|");
    assert_eq!(after(&["pageup"]), "left/project-b/|");
    // Ctrl+y keeps the match.
    assert_eq!(after(&["ctrl-y"]), "left/project-b/|");
    assert_eq!(app.place(), (Mode::Insert, Context::PathInput));

    // Ctrl+e puts back what was typed.
    let two = names_of(&[("x", true), ("y", true)]);
    let (generation, dir, ..) = names_request(&keys(&mut app, &["tab"]));
    assert_eq!(dir, local("/srv/left/project-b"));
    app.names(generation, Ok(two.clone()), &[]);
    assert_eq!(field(&app), "left/project-b/x/|");
    keys(&mut app, &["down", "ctrl-e"]);
    assert_eq!(field(&app), "left/project-b/|");
    assert_eq!(app.place(), (Mode::Insert, Context::PathInput));

    // Right completes in the directory chosen, and Left in the one above, as vim's
    // pop-up menu of the command line.
    let (generation, ..) = names_request(&keys(&mut app, &["tab"]));
    app.names(generation, Ok(two.clone()), &[]);
    let (generation, dir, ..) = names_request(&keys(&mut app, &["right"]));
    assert_eq!(field(&app), "left/project-b/x/|");
    assert_eq!(dir, local("/srv/left/project-b/x"));
    app.names(generation, Ok(two.clone()), &[]);
    assert_eq!(field(&app), "left/project-b/x/x/|");
    let (generation, dir, ..) = names_request(&keys(&mut app, &["left"]));
    assert_eq!(
        (field(&app), dir),
        ("left/project-b/|".to_owned(), local("/srv/left/project-b"))
    );
    app.names(generation, Ok(two.clone()), &[]);
    assert_eq!(field(&app), "left/project-b/x/|");
    keys(&mut app, &["down", "down", "ctrl-e"]);
    assert_eq!(field(&app), "left/project-b/|");

    // Other keys close the list and go to the field, with the match in it: Esc to Normal
    // mode, as in vim; typing edits the field.
    let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
    app.names(generation, Ok(two.clone()), &[]);
    keys(&mut app, &["esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::PathInput));
    assert_eq!(app.dialogs[0].dialog.text(), "left/project-b/x/");
    app.dialogs[0].dialog.set_text(0, "left/");
    let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
    app.names(generation, Ok(two.clone()), &[]);
    app.handle(Resolved::Insert('z'));
    assert_eq!(app.context(), Context::PathInput);
    assert_eq!(field(&app), "left/x/z|");

    // A reply to text that changed since is dropped.
    let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
    app.handle(action(Action::Backspace));
    app.names(generation, Ok(names_of(&[("xyz", true)])), &[]);
    assert_eq!(field(&app), "left/x/|");
    // So is one that nothing matches.
    let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
    app.names(generation, Ok(names_of(&[(".git", true)])), &[]);
    assert_eq!(field(&app), "left/x/|");
    assert_eq!(app.context(), Context::PathInput);
}

#[test]
fn enter_in_the_list_of_completions_confirms_with_the_match() {
    let mut app = loaded();
    app.handle(action(Action::GoToDir));
    type_text(&mut app, "ri");
    let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
    app.names(
        generation,
        Ok(names_of(&[("right", true), ("rig", true)])),
        &[],
    );
    let effects = keys(&mut app, &["tab", "enter"]);
    assert!(app.dialogs.is_empty());
    let [Effect::List { request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!(request.location, local("/srv/right"));
}

#[test]
fn a_click_on_the_list_of_completions_puts_a_match_in_and_a_double_click_keeps_it() {
    let mut app = loaded();
    app.handle(action(Action::GoToDir));
    let (generation, ..) = names_request(&app.handle(action(Action::Complete)));
    let entries = names_of(&[("alpha", true), ("beta", true), ("gamma", true)]);
    app.names(generation, Ok(entries), &[]);
    assert_eq!(field(&app), "alpha/|");
    let (x, y) = find(&mut app, "gamma/");
    press(&mut app, Press::Click, x, y);
    assert_eq!(field(&app), "gamma/|");
    assert_eq!(app.context(), Context::Completion, "the list stays");
    let (x, y) = find(&mut app, "beta/");
    press(&mut app, Press::Click, x, y);
    press(&mut app, Press::DoubleClick, x, y);
    assert_eq!(field(&app), "beta/|");
    assert_eq!(
        app.context(),
        Context::PathInput,
        "a double click closes it"
    );
}

#[test]
fn go_to_dir_completes_hosts_and_home() {
    let mut app = loaded();
    app.handle(action(Action::GoToDir));
    type_text(&mut app, "~");
    assert!(app.handle(action(Action::Complete)).is_empty());
    assert_eq!(field(&app), "~/|");
    app.handle(action(Action::DeleteToStart));
    type_text(&mut app, "we");
    let (generation, dir, _, hosts) = names_request(&app.handle(action(Action::Complete)));
    assert_eq!((dir, hosts), (local("/srv"), true));
    app.names(
        generation,
        Ok(names_of(&[("left", true)])),
        &["web".to_owned(), "db".to_owned()],
    );
    assert_eq!(field(&app), "web:|");
    // On a host that is not connected, there is nothing to read.
    type_text(&mut app, "/v");
    let (_, dir, through, hosts) = names_request(&app.handle(action(Action::Complete)));
    assert_eq!((dir, through, hosts), (remote("web", "/"), false, false));
}

#[test]
fn f5_completes_files_and_connected_hosts_and_f7_takes_no_hosts() {
    // The left panel is on web, at /home/deploy, with its cursor on notes.
    let (mut app, _) = on_a_host();
    app.handle(action(Action::Copy));
    app.handle(action(Action::DeleteToStart));
    type_text(&mut app, "w");
    let (generation, _, _, hosts) = names_request(&app.handle(action(Action::Complete)));
    assert!(!hosts, "F5 knows the connected hosts itself");
    app.names(generation, Ok(names_of(&[("x", false)])), &[]);
    assert_eq!(field(&app), "web:|");
    type_text(&mut app, "no");
    let (generation, dir, through, _) = names_request(&app.handle(action(Action::Complete)));
    assert_eq!((dir, through), (remote("web", ""), true));
    app.names(generation, Ok(names_of(&[("notes", false)])), &[]);
    assert_eq!(field(&app), "web:notes|");
    app.handle(action(Action::Cancel));

    app.handle(action(Action::Mkdir));
    app.handle(action(Action::DeleteToStart));
    type_text(&mut app, "web:x/");
    let (_, dir, ..) = names_request(&app.handle(action(Action::Complete)));
    assert_eq!(
        dir,
        remote("web", "/home/deploy/web:x"),
        "F7 names no hosts"
    );
}

/// The text of the command line, with `|` at the cursor.
fn line_text(app: &App) -> String {
    let line = app.command_line.as_ref().expect("the command line");
    let before = line.before_cursor();
    format!("{before}|{}", &line.text()[before.len()..])
}

/// The listing of commands that a Tab asks for: its generation, directories, and prefix.
fn commands_request(effects: &[Effect]) -> (u64, Vec<PathBuf>, String) {
    match effects {
        [
            Effect::ListCommands {
                generation,
                dirs,
                prefix,
                with_dirs: false,
            },
        ] => (*generation, dirs.clone(), prefix.clone()),
        other => panic!("expected a listing of commands, got {other:?}"),
    }
}

#[test]
fn tab_on_the_command_line_completes_a_command_then_files_escaped_for_the_shell() {
    let mut app = loaded();
    let path = vec![PathBuf::from("/opt/bin"), PathBuf::from("/usr/bin")];
    app.set_command_dirs(path.clone());
    app.handle(action(Action::Shell));
    type_text(&mut app, "my");
    let (generation, dirs, prefix) = commands_request(&keys(&mut app, &["tab"]));
    assert_eq!((dirs, prefix.as_str()), (path, "my"));
    let commands = names_of(&[("mytool", false), ("my tool", false)]);
    app.names(generation, Ok(commands), &[]);
    assert_eq!(line_text(&app), "my\\ tool|", "the first, escaped");
    assert_eq!(app.place(), (Mode::Insert, Context::CommandCompletion));
    // The list goes above the line, where the name starts, after the prompt `/srv $ `.
    assert_eq!(find(&mut app, "mytool"), (7, 8));
    assert_eq!(find(&mut app, "my tool"), (7, 7));
    keys(&mut app, &["tab", "ctrl-y"]);
    assert_eq!(line_text(&app), "mytool|");
    assert_eq!(app.context(), Context::CommandLine);

    // Later words are files and directories, from the panel's directory.
    type_text(&mut app, " sr");
    let (generation, dir, through, hosts) = names_request(&keys(&mut app, &["tab"]));
    assert_eq!((dir, through, hosts), (local("/srv"), false, false));
    let entries = [("src dir", true), ("srv.txt", false), ("other", false)];
    app.names(generation, Ok(names_of(&entries)), &[]);
    assert_eq!(line_text(&app), "mytool src\\ dir/|");
    keys(&mut app, &["tab"]);
    assert_eq!(line_text(&app), "mytool srv.txt|");
    keys(&mut app, &["ctrl-e"]);
    assert_eq!(line_text(&app), "mytool sr|");

    // Escapes are taken out to match, and the word's directory is where it looks.
    type_text(&mut app, "c\\ d");
    let (generation, ..) = names_request(&keys(&mut app, &["tab"]));
    app.names(generation, Ok(names_of(&[("src dir", true)])), &[]);
    assert_eq!(line_text(&app), "mytool src\\ dir/|");
    assert_eq!(app.context(), Context::CommandLine, "one match, no list");
    let (generation, dir, ..) = names_request(&keys(&mut app, &["tab"]));
    assert_eq!(dir, local("/srv/src dir"));
    app.names(generation, Ok(names_of(&[("a$b", false)])), &[]);
    assert_eq!(line_text(&app), "mytool src\\ dir/a\\$b|");

    // `~` alone becomes `~/`, the home directory, as absolute paths are absolute.
    type_text(&mut app, " ~");
    assert!(keys(&mut app, &["tab"]).is_empty());
    assert_eq!(line_text(&app), "mytool src\\ dir/a\\$b ~/|");
    let (_, dir, ..) = names_request(&keys(&mut app, &["tab"]));
    assert_eq!(dir, local("/home/me"));
    type_text(&mut app, " /usr/b");
    let (_, dir, ..) = names_request(&keys(&mut app, &["tab"]));
    assert_eq!(dir, local("/usr"));
    app.handle(action(Action::Confirm));
    let run = app.take_run().expect("a command");
    assert_eq!(run.command, "mytool src\\ dir/a\\$b ~/ /usr/b");

    // A first word with a slash names a program, or a directory, as vim's `:!` has it.
    app.handle(action(Action::Shell));
    type_text(&mut app, "./scr");
    let effects = keys(&mut app, &["tab"]);
    let [
        Effect::ListCommands {
            dirs, with_dirs, ..
        },
    ] = &effects[..]
    else {
        panic!("expected a listing of programs, got {effects:?}");
    };
    assert_eq!(
        (&dirs[..], *with_dirs),
        (&[PathBuf::from("/srv")][..], true)
    );
    // Ctrl+c closes the line, from the list too.
    let (generation, ..) = names_request(&keys(&mut app, &["ctrl-u", "c", "a", "t", " ", "tab"]));
    app.names(generation, Ok(names_of(&[("a", false), ("b", false)])), &[]);
    assert_eq!(app.context(), Context::CommandCompletion);
    keys(&mut app, &["ctrl-c"]);
    assert!(app.command_line.is_none());
}

#[test]
fn the_list_on_the_command_line_passes_other_keys_to_the_line() {
    let mut app = loaded();
    let two = names_of(&[("left", true), ("right", true)]);
    // Enter runs the command with the match in it, as vim's.
    app.handle(action(Action::Shell));
    type_text(&mut app, "cat ");
    let (generation, ..) = names_request(&keys(&mut app, &["ctrl-i"]));
    app.names(generation, Ok(two.clone()), &[]);
    assert_eq!(line_text(&app), "cat left/|");
    keys(&mut app, &["tab", "enter"]);
    assert_eq!(app.take_run().expect("a command").command, "cat right/");
    // Esc closes the line, as vim's.
    app.handle(action(Action::Shell));
    type_text(&mut app, "cat ");
    let (generation, ..) = names_request(&keys(&mut app, &["tab"]));
    app.names(generation, Ok(two), &[]);
    keys(&mut app, &["esc"]);
    assert!(app.command_line.is_none());
    // In Normal mode, Tab does nothing.
    app.handle(action(Action::Shell));
    type_text(&mut app, "cat ");
    keys(&mut app, &["ctrl-f"]);
    assert!(keys(&mut app, &["tab"]).is_empty());
    assert!(app.completion.is_none());
}

#[test]
fn colon_completes_after_its_bang_and_a_host_completes_files_there() {
    let mut app = loaded();
    app.handle(action(Action::Command));
    type_text(&mut app, "wq");
    assert!(keys(&mut app, &["tab"]).is_empty(), "no shell command");
    app.command_line.as_mut().unwrap().set_text("");
    type_text(&mut app, "!gi");
    let (generation, _, prefix) = commands_request(&keys(&mut app, &["tab"]));
    assert_eq!(prefix, "gi");
    app.names(generation, Ok(names_of(&[("git", false)])), &[]);
    assert_eq!(line_text(&app), "!git|");

    // On a host, every word completes as files there, through its session.
    let (mut app, _) = on_a_host();
    app.handle(action(Action::Shell));
    type_text(&mut app, "no");
    let (generation, dir, through, hosts) = names_request(&keys(&mut app, &["tab"]));
    assert_eq!(
        (dir, through, hosts),
        (remote("web", "/home/deploy"), true, false)
    );
    app.names(generation, Ok(names_of(&[("notes", false)])), &[]);
    assert_eq!(line_text(&app), "notes|");
    type_text(&mut app, " ~/s");
    let (_, dir, ..) = names_request(&keys(&mut app, &["tab"]));
    assert_eq!(dir, remote("web", ""));
}

#[test]
fn a_click_on_the_list_over_the_command_line_puts_a_match_in() {
    let mut app = loaded();
    app.handle(action(Action::Shell));
    type_text(&mut app, "cat ");
    let (generation, ..) = names_request(&keys(&mut app, &["tab"]));
    let entries = names_of(&[("alpha", false), ("beta", false), ("gamma", false)]);
    app.names(generation, Ok(entries), &[]);
    let (x, y) = find(&mut app, "beta");
    press(&mut app, Press::Click, x, y);
    assert_eq!(line_text(&app), "cat beta|");
    assert_eq!(app.context(), Context::CommandCompletion);
    let (x, y) = find(&mut app, "gamma");
    // Three lines up: alpha, what was typed, and round to gamma.
    press(&mut app, Press::WheelUp, x, y);
    assert_eq!(line_text(&app), "cat gamma|", "the wheel moves as Up");
    press(&mut app, Press::Click, 0, 0);
    assert_eq!(
        app.context(),
        Context::CommandLine,
        "a press elsewhere closes it"
    );
    assert_eq!(line_text(&app), "cat gamma|");
}
