use super::*;

fn alice_on_mac() -> Machine {
    Machine {
        host: Some("mac".to_owned()),
        user: Some("alice".to_owned()),
        root: false,
    }
}

fn root_on_mac() -> Machine {
    Machine {
        user: Some("root".to_owned()),
        root: true,
        ..alice_on_mac()
    }
}

#[test]
fn the_line_of_the_mode_names_the_kind_of_window_by_its_keys() {
    let cases: &[(&[&str], &str)] = &[
        (&[], "NORMAL"),
        (&["j", "r", "esc"], "NORMAL · Rename"),
        (&["g", "l"], "INSERT · List"),
        (&["t", "t"], "INSERT · List"),
        (&["t", "t", "esc"], "NORMAL · List"),
        (&["g", "z"], "INSERT · List"),
        (&["t", "w"], "INSERT · List"),
        (&["q", ":"], "INSERT · List"),
        (&["f1"], "NORMAL · Help"),
        (&["f9"], "NORMAL · Menu"),
        (&["f7"], "INSERT · Dialog"),
        (&["j", "f8"], "NORMAL · Dialog"),
        (&["f", "/"], "INSERT · Dialog"),
        (&["f", "j"], "NORMAL · Dialog"),
        (&["j", "g", "f"], "NORMAL · Viewer"),
        (&["ctrl-x", "g", "d"], "(^X) INSERT · Dialog"),
        (&["ctrl-x", "v"], "(^X) VISUAL"),
        (&["ctrl-x", "j"], "NORMAL"),
    ];
    for (typed, shown) in cases {
        let mut app = with_files(&["a", "b"]);
        keys(&mut app, typed);
        assert_eq!(mode_of(&mut app), *shown, "after {typed:?}");
    }
    let mut app = loaded();
    open_configuration(&mut app);
    assert_eq!(mode_of(&mut app), "INSERT · Dialog");
}

#[test]
fn ctrl_g_says_where_the_panel_is_until_the_next_key_as_vims() {
    let mut app = with_files(&["a", "b"]);
    keys(&mut app, &["ctrl-g"]);
    assert_eq!(mode_of(&mut app), "\"/srv\" 1/3 33%", "`..` counts");
    keys(&mut app, &["j"]);
    assert_eq!(mode_of(&mut app), "NORMAL", "gone with the next key");
    keys(&mut app, &["shift-g", "ctrl-g"]);
    assert_eq!(mode_of(&mut app), "\"/srv\" 3/3 100%");
    keys(&mut app, &["ctrl-x", "ctrl-g"]);
    assert_eq!(mode_of(&mut app), "\"/srv\" 1/3 33%", "of the other panel");

    let (mut app, effects) = App::new(Path::new("/home/me/src"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![dir("noc")]));
    keys(&mut app, &["ctrl-g"]);
    assert_eq!(mode_of(&mut app), "\"~/src\" 1/2 50%", "as the title");
    keys(&mut app, &["1", "ctrl-g"]);
    assert_eq!(
        mode_of(&mut app),
        "\"/home/me/src\" 1/2 50%",
        "after a count, the whole path"
    );

    let mut app = at_slash();
    keys(&mut app, &["ctrl-g"]);
    assert_eq!(mode_of(&mut app), "\"/\" 0/0");
}

/// The cell where `text` starts on the screen.
fn cell_of(app: &mut App, text: &str) -> ratatui::buffer::Cell {
    let mut terminal = Terminal::new(TestBackend::new(160, 9)).unwrap();
    terminal
        .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let at = crate::tui::mouse::find(buffer, text);
    buffer[(at.x, at.y)].clone()
}

#[test]
fn what_a_command_tells_is_plain_and_warnings_in_their_color_as_vims() {
    let mut app = with_files(&["a"]);
    let warning = app.theme.message.fg.unwrap();
    let plain = |cell: ratatui::buffer::Cell| cell.fg != warning && cell.modifier.is_empty();
    keys(&mut app, &["ctrl-g"]);
    assert!(plain(cell_of(&mut app, "\"/srv\"")), "as vim's Ctrl+g");
    keys(&mut app, &["n"]);
    assert_eq!(mode_of(&mut app), "No previous search");
    assert_eq!(cell_of(&mut app, "No previous search").fg, warning);
}

#[test]
fn the_command_line_takes_the_line_of_the_mode_while_it_has_the_keys() {
    let mut app = with_files(&["a"]);
    keys(&mut app, &["!", "l", "s"]);
    assert!(mode_line(&mut app, 160).ends_with("$ ls"));
    keys(&mut app, &["ctrl-f", "q", ":"]);
    assert_eq!(app.place().1, Context::CommandHistory);
    assert_eq!(mode_of(&mut app), "INSERT · List");
    keys(&mut app, &["esc", "q"]);
    assert_eq!(app.place().1, Context::CommandLine);
    assert!(mode_line(&mut app, 160).ends_with("$ ls"));
}

#[test]
fn quick_search_takes_the_line_of_the_mode_while_it_has_the_keys() {
    let mut app = with_files(&["a", "b"]);
    let way = glyph(&app);
    keys(&mut app, &["/", "b"]);
    assert!(searching(&mut app, "b", way));
    // In the other panel too, which is drawn active.
    keys(&mut app, &["esc", "ctrl-x", "/", "a"]);
    assert_eq!(app.active, Side::Right);
    assert!(searching(&mut app, "a", way));
    keys(&mut app, &["enter"]);
    assert_eq!(mode_of(&mut app), "NORMAL");
}

#[test]
fn the_prompt_says_who_is_where_as_a_shell_does() {
    let (mut app, _) = App::new(
        Path::new("/home/me/src/noc"),
        Path::new("/home/me"),
        &config(),
    );
    app.set_machine(alice_on_mac());
    app.handle(action(Action::Shell));
    let prompt = |app: &App, width| cells::text_of(&app.command_prompt(width));
    assert_eq!(prompt(&app, 120), "alice@mac:~/src/noc $ ");
    assert_eq!(
        prompt(&app, 54),
        "alice@mac:~/sr»noc $ ",
        "the directory cut first"
    );
    assert_eq!(
        prompt(&app, 48),
        "mac:~/src/noc $ ",
        "then the user left out"
    );
    assert_eq!(prompt(&app, 30), "mac $ ", "then the directory");
    assert_eq!(prompt(&app, 6), "m» $ ");
    let who = app.command_prompt(120).remove(0);
    assert_eq!(who.style, Style::new(), "only root stands out");

    app.set_machine(root_on_mac());
    assert_eq!(prompt(&app, 120), "root@mac:~/src/noc # ");
    let who = app.command_prompt(120).remove(0);
    assert_eq!(
        (who.content.as_ref(), who.style),
        ("root@mac", app.theme.root)
    );
    assert_eq!(
        app.terminal_title().as_deref(),
        Some("[root] ~/src/noc — noc")
    );
}

#[test]
fn the_line_of_the_mode_says_who_is_where_while_it_fits() {
    let mut app = loaded();
    assert_eq!(
        mode_line(&mut app, 60),
        "NORMAL",
        "a machine without a name"
    );
    app.set_machine(alice_on_mac());
    let line = mode_line(&mut app, 60);
    assert!(line.starts_with("NORMAL "), "{line}");
    assert!(
        line.ends_with(" alice@mac"),
        "left of what is typed: {line}"
    );
    assert_eq!(line.chars().count(), 60 - SHOWCMD_WIDTH - 1);
    assert_eq!(mode_line(&mut app, 26), "NORMAL", "no room");
    app.set_machine(root_on_mac());
    assert!(mode_line(&mut app, 60).ends_with(" # root@mac"));
}

/// Whether a cell of the screen has the background of `style`.
fn shows_background(app: &mut App, style: Style) -> bool {
    let mut terminal = Terminal::new(TestBackend::new(100, 9)).unwrap();
    terminal
        .draw(|frame| app.render(frame, Instant::now(), &TimeZone::UTC))
        .unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content
        .iter()
        .any(|cell| cell.bg == style.bg.unwrap())
}

#[test]
fn the_cursor_is_on_red_in_the_panels_where_the_user_is_root() {
    let mut app = loaded();
    let (cursor, root) = (app.theme.cursor, app.theme.root_cursor);
    assert_ne!(cursor.bg, root.bg);
    assert!(shows_background(&mut app, cursor));
    assert!(!shows_background(&mut app, root));
    app.set_machine(root_on_mac());
    assert!(shows_background(&mut app, root));

    let (mut app, _) = on_a_host();
    app.resolved(
        "web".to_owned(),
        "deploy@10.0.0.5".to_owned(),
        "deploy",
        None,
    );
    assert!(!shows_background(&mut app, root), "not root there");
    app.resolved("web".to_owned(), "root@10.0.0.5".to_owned(), "root", None);
    assert!(shows_background(&mut app, root), "root on the host");
    assert_eq!(app.theme.root_cursor, root, "the theme stays");
}

#[test]
fn on_a_host_the_user_comes_from_ssh_and_a_local_root_still_shows() {
    let (mut app, _) = on_a_host();
    let prompt = |app: &App| cells::text_of(&app.shell_prompt(120));
    assert_eq!(prompt(&app), "web:/home/deploy $ ", "before ssh -G");
    let line = mode_line(&mut app, 100);
    assert!(line.ends_with(" web"), "{line:?}");

    app.resolved(
        "web".to_owned(),
        "deploy@10.0.0.5".to_owned(),
        "deploy",
        None,
    );
    assert_eq!(prompt(&app), "deploy@web:/home/deploy $ ");
    app.set_machine(root_on_mac());
    assert_eq!(prompt(&app), "deploy@web:/home/deploy $ ");
    assert!(
        mode_line(&mut app, 100).ends_with(" # deploy@web"),
        "root here"
    );
    assert_eq!(
        app.terminal_title().as_deref(),
        Some("[root] web:/home/deploy — noc")
    );

    app.set_machine(alice_on_mac());
    app.resolved("web".to_owned(), "root@10.0.0.5".to_owned(), "root", None);
    assert_eq!(prompt(&app), "root@web:/home/deploy # ");
    assert!(mode_line(&mut app, 100).ends_with(" root@web"));
    assert!(!mode_line(&mut app, 100).contains('#'));
    assert_eq!(
        app.terminal_title().as_deref(),
        Some("[root] web:/home/deploy — noc")
    );
}
