use super::*;

/// Types `text` in the filter of the list in front, after `/`.
fn filter(app: &mut App, text: &str) {
    app.handle(action(Action::QuickSearch));
    type_text(app, text);
}

#[test]
fn g_l_and_ctrl_x_g_l_change_the_location_of_their_panel() {
    let mut app = loaded();
    open_menu(&mut app, Side::Right);
    assert_eq!(app.context(), Context::LocationMenu);
    let text = screen_of(&mut app, 14);
    assert!(
        in_the_middle(&text, " Location ") && text.contains("/Volumes/USB"),
        "centered over both panels, for the other one: {text}"
    );
    assert!(
        text.contains("8Disconn") && text.contains("10Cancel"),
        "{text}"
    );
    // Panel keys do nothing while it is open.
    assert!(app.handle(action(Action::SwitchPanel)).is_empty());
    let Effect::List {
        panel: PanelId { side, .. },
        request,
        ..
    } = one(choose(&mut app, 2))
    else {
        panic!("expected a listing");
    };
    assert_eq!(
        (side, request.location),
        (Side::Right, local("/Volumes/USB"))
    );
    assert_eq!(
        app.active,
        Side::Left,
        "the keys come back to the active panel"
    );
    assert_eq!(app.context(), Context::Panel);

    // Ctrl+c closes the menu; a host connects.
    app.handle(action(Action::SwitchPanel));
    open_menu(&mut app, Side::Left);
    keys(&mut app, &["ctrl-c"]);
    assert!(app.menu.is_none());
    assert_eq!(app.active, Side::Right);
    open_menu(&mut app, Side::Left);
    filter(&mut app, "w");
    let effect = one(keys(&mut app, &["enter"]));
    assert!(
        matches!(&effect, Effect::Connect { host, .. } if host == "web"),
        "a host connects: {effect:?}"
    );
    assert_eq!(app.active, Side::Right, "from the right panel too");
}

#[test]
fn the_menus_of_a_panel_name_no_side_and_reach_it_after_ctrl_w_x() {
    let mut app = loaded();
    app.handle(action(Action::SwapPanels));
    open_menu(&mut app, Side::Right);
    let text = screen_of(&mut app, 14);
    assert!(in_the_middle(&text, " Location "), "{text}");
    assert!(
        !text.contains(" Left ") && !text.contains(" Right "),
        "{text}"
    );
    let Effect::List {
        panel: PanelId { side, .. },
        ..
    } = one(choose(&mut app, 2))
    else {
        panic!("expected a listing");
    };
    assert_eq!(side, Side::Right, "the right panel, drawn on the left");

    assert!(on_other(&mut app, Action::TabList).is_empty());
    let text = screen_of(&mut app, 14);
    assert!(in_the_middle(&text, " Tabs "), "the tabs too: {text}");
    assert!(!text.contains("Left tabs") && !text.contains("Right tabs"));
    assert_eq!(app.tab_menu.as_ref().map(TabMenu::side), Some(Side::Right));
}

#[test]
fn f9_opens_the_pull_down_menu_and_its_commands_run() {
    let mut app = loaded();
    app.active = Side::Right;
    app.handle(action(Action::PullDown));
    assert_eq!(app.context(), Context::PullDown);
    let text = screen_of(&mut app, 20);
    assert!(text.contains("Left     File"), "{text}");
    assert!(!text.contains("Change location…"), "the bar alone: {text}");
    app.handle(action(Action::Down));
    let text = screen_of(&mut app, 20);
    let location = line_with(&text, "Change location…");
    assert!(
        location.contains("g l") && !location.contains("Ctrl+x"),
        "{text}"
    );
    let sort = line_with(&text, "Sort by size");
    assert!(
        sort.contains("o s") && !sort.contains("Ctrl+x"),
        "the keys of the active panel: {text}"
    );
    assert!(
        text.contains("9Cancel") && text.contains("10Cancel"),
        "{text}"
    );

    // Right goes round to Left, which acts on the panel on its side: Ctrl+x and its keys.
    app.handle(action(Action::Right));
    let text = screen_of(&mut app, 20);
    assert!(
        line_with(&text, "Sort by size").contains("Ctrl+x o s"),
        "{text}"
    );
    assert!(
        line_with(&text, "Change location…").contains("Ctrl+x g l"),
        "{text}"
    );
    assert!(
        line_with(&text, "Tab list…").contains("Ctrl+x t t"),
        "{text}"
    );
    assert!(app.handle(Resolved::Hotkey('z')).is_empty());
    assert!(app.pulldown.is_none(), "a command closes the menu");
    assert_eq!(app.panel(Side::Left).sort_action(), Action::SortBySize);
    assert_eq!(app.panel(Side::Right).sort_action(), Action::SortByName);
    assert_eq!(app.active, Side::Right);

    // F9 opens the bar alone again; Shift+F9, the menu on the command that ran last, even
    // after Esc left another one.
    app.handle(action(Action::PullDown));
    let text = screen_of(&mut app, 20);
    assert!(!text.contains("Sort by size"), "the bar alone: {text}");
    app.handle(Resolved::Hotkey('f'));
    app.handle(action(Action::Cancel));
    app.handle(action(Action::Cancel));
    assert!(app.pulldown.is_none());
    app.handle(action(Action::PullDownLast));
    let text = screen_of(&mut app, 20);
    let chosen = format!("{} Sort by size", app.theme.glyphs.choice);
    assert!(text.contains(&chosen), "{text}");
    app.handle(action(Action::Confirm));
    assert_eq!(
        app.panel(Side::Left).sort_action(),
        Action::SortBySize,
        "reversed"
    );

    // Options: the hidden files, in both panels.
    app.handle(action(Action::PullDown));
    assert!(app.handle(Resolved::Hotkey('o')).is_empty());
    let checked = format!("{} Show hidden files", app.theme.glyphs.check);
    assert!(screen_of(&mut app, 20).contains(&checked));
    app.handle(Resolved::Hotkey('h'));
    assert!(!app.config.ui.show_hidden);

    // Commands that cannot run do nothing; Esc goes back to the bar, then closes it.
    app.handle(action(Action::PullDown));
    app.handle(Resolved::Hotkey('f'));
    assert!(
        app.handle(Resolved::Hotkey('k')).is_empty(),
        "nothing chosen"
    );
    assert!(app.pulldown.is_some());
    app.handle(action(Action::Cancel));
    assert!(app.pulldown.is_some());
    app.handle(action(Action::Cancel));
    assert!(app.pulldown.is_none());

    // A command that opens something: the location menu of the panel on its side.
    app.handle(action(Action::PullDown));
    app.handle(Resolved::Hotkey('l'));
    let effects = app.handle(Resolved::Hotkey('l'));
    assert!(matches!(&effects[..], [Effect::ListPlaces { .. }]));
    assert_eq!(app.menu.as_ref().map(LocationMenu::side), Some(Side::Left));
}

#[test]
fn o_o_opens_the_configuration_dialog_and_the_menu_shows_it() {
    let mut app = loaded();
    app.handle(action(Action::PullDown));
    app.handle(Resolved::Hotkey('o'));
    let text = screen_of(&mut app, 20);
    assert!(line_with(&text, "Configuration…").contains("o o"), "{text}");
    app.handle(action(Action::Cancel));
    app.handle(action(Action::Cancel));
    assert!(keys(&mut app, &["o", "o"]).is_empty());
    assert!(app.configuration.is_some());
    assert_eq!(app.context(), Context::DialogInput, "on the language");
}

#[test]
fn the_configuration_dialog_applies_and_saves_each_change() {
    let mut app = loaded();
    open_configuration(&mut app);
    assert_eq!(app.context(), Context::DialogInput, "on the language");
    let text = screen_of(&mut app, 24);
    assert!(
        text.contains("Configuration") && !text.contains("OK"),
        "{text}"
    );
    for _ in 0..2 {
        assert!(
            app.handle(action(Action::Down)).is_empty(),
            "nothing changed"
        );
    }
    let effects = app.handle(action(Action::Right));
    assert!(app.configuration.is_some(), "it stays open");
    assert_eq!(app.theme, Theme::terminal());
    let [Effect::SaveConfig { old, new, config }] = &effects[..] else {
        panic!("expected the theme to be saved: {effects:?}");
    };
    assert_eq!(old.ui, ui());
    assert_eq!(new.ui, app.config.ui);
    assert_eq!(**config, app.config, "for new connections and listings");
    assert_eq!(
        Config {
            ui: ui(),
            ..(**new).clone()
        },
        Config {
            ui: ui(),
            ..self::config()
        },
        "only the interface"
    );
    app.handle(action(Action::End));
    app.handle(action(Action::Up));
    assert!(
        app.handle(action(Action::Toggle)).is_empty(),
        "the list opens"
    );
    assert_eq!(app.place(), (Mode::Normal, Context::ComboBox));
    assert!(
        mode_of(&mut app).contains("Combo box"),
        "the line of the mode"
    );
    app.handle(action(Action::Down));
    let effects = app.handle(action(Action::Confirm));
    assert_eq!(app.config.ui.menu_bar, MenuBar::Always);
    let [Effect::SaveConfig { old, .. }] = &effects[..] else {
        panic!("expected the menu bar to be saved: {effects:?}");
    };
    assert_eq!(old.ui.theme, "terminal", "after the theme");
    let (old, new) = (Box::new(Config::default()), Box::new(app.config.clone()));
    app.config_saved(old, new, Err("cannot write config.toml".to_owned().into()));
    assert!(app.dialogs.front().is_some(), "a failure says so");
    app.handle(action(Action::Cancel));
    assert!(app.configuration.is_some());
    assert!(app.handle(action(Action::Cancel)).is_empty());
    assert!(app.configuration.is_none(), "Esc closes it");

    // A language that is no tag stays in its field and is not used.
    open_configuration(&mut app);
    app.handle(Resolved::Insert('?'));
    assert!(app.handle(action(Action::Down)).is_empty());
    assert!(app.handle(action(Action::Cancel)).is_empty());
    assert_eq!(app.config.ui.language, "auto");
}

/// Types `text` in the text field under the cursor in place of what it holds: A, Ctrl+u,
/// the text, and Esc, as typed in Normal mode.
fn edit_field(app: &mut App, text: &str) {
    app.handle(action(Action::AppendAtEnd));
    retype(app, text);
    app.handle(action(Action::NormalMode));
}

/// Opens the category `index` of the Configuration dialog, with the cursor on its first
/// setting.
fn config_category(app: &mut App, index: usize) {
    app.handle(action(Action::PrevField));
    app.handle(action(Action::Home));
    for _ in 0..index {
        app.handle(action(Action::Down));
    }
    app.handle(action(Action::Right));
}

#[test]
fn transfer_and_ssh_settings_apply_at_once() {
    let mut app = loaded();
    open_configuration(&mut app);
    config_category(&mut app, 2);
    app.handle(action(Action::Toggle));
    assert!(!app.copy_choices.atomic);
    app.handle(action(Action::Down));
    edit_field(&mut app, "5");
    config_category(&mut app, 3);
    assert_eq!(app.parallel_jobs, 5, "as the cursor left the field");
    edit_field(&mut app, "~/bin/ssh");
    let effects = app.handle(action(Action::Confirm));
    let [Effect::SaveConfig { new, config, .. }] = &effects[..] else {
        panic!("expected the settings to be saved, and nothing to reload: {effects:?}");
    };
    assert_eq!(
        new.ssh.program,
        Path::new("~/bin/ssh"),
        "as typed, for the file"
    );
    assert_eq!(
        config.ssh.program,
        Path::new("/home/me/bin/ssh"),
        "expanded, for ssh"
    );
}

#[test]
fn the_settings_of_the_log_apply_at_once() {
    let mut app = loaded();
    open_configuration(&mut app);
    config_category(&mut app, 9);
    let effects = app.handle(action(Action::Right));
    let [Effect::SaveConfig { new, .. }, Effect::Log(log)] = &effects[..] else {
        panic!("expected the level to be saved and used: {effects:?}");
    };
    assert_eq!(
        (new.log.level, log.level),
        (LogLevel::Debug, LogLevel::Debug)
    );
    app.handle(action(Action::Down));
    app.handle(action(Action::Down));
    edit_field(&mut app, "3");
    let effects = app.handle(action(Action::Up));
    let [Effect::SaveConfig { .. }, Effect::Log(log)] = &effects[..] else {
        panic!("expected the days to keep to be saved and used: {effects:?}");
    };
    assert_eq!(log.keep_days, 3);
    // Other settings leave the log alone.
    config_category(&mut app, 1);
    let effects = app.handle(action(Action::Toggle));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Log(_))),
        "{effects:?}"
    );
}

#[test]
fn the_menu_bar_stays_above_the_panels_with_ui_menu_bar() {
    let mut app = loaded();
    let top = |app: &mut App| screen_of(app, 10).lines().next().unwrap_or("").to_owned();
    assert!(!top(&mut app).contains("Left"), "on demand");
    app.config.ui.menu_bar = MenuBar::Always;
    let text = screen_of(&mut app, 10);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].contains("Left     File"), "{text}");
    assert!(lines[1].contains('╔'), "the panels start below it: {text}");
    app.handle(action(Action::PullDown));
    app.handle(action(Action::Down));
    assert!(screen_of(&mut app, 20).contains("Change location…"));
}

#[test]
fn the_menu_disconnects_hosts_and_reads_its_listing_again() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    open_menu(&mut app, Side::Left);
    assert!(
        app.handle(action(Action::Disconnect)).is_empty(),
        "a volume"
    );
    app.handle(action(Action::End));
    let effect = one(app.handle(action(Action::Confirm)));
    assert!(matches!(effect, Effect::Connect { .. }));
    open_menu(&mut app, Side::Left);
    assert!(app.hosts.contains_key("db"));
    app.handle(action(Action::End));
    assert!(app.handle(action(Action::Disconnect)).is_empty());
    assert!(!app.hosts.contains_key("db"), "stopped connecting");
    // R asks again; the earlier listing no longer counts.
    let effects = app.handle(action(Action::Reload));
    assert!(matches!(
        &effects[..],
        [Effect::ListPlaces { generation: 3 }]
    ));
    assert!(app.menu.is_some());
}

#[test]
fn hosts_and_volumes_used_last_come_first_and_go_to_recent_toml() {
    let mut app = at_slash();
    assert_eq!(app.take_recent(), None);
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
        panic!("expected a connection");
    };
    assert_eq!(app.take_recent(), None, "not before it connects");
    let (handle, _requests) = HostHandle::channel();
    app.connected("db", connection, handle);
    let recent = app.take_recent().unwrap();
    assert_eq!(recent.hosts, ["db"]);
    assert_eq!(app.take_recent(), None, "once");

    // `recent.toml` arrives late: what was used since comes first.
    let saved = Recent {
        hosts: vec!["web".to_owned(), "db".to_owned()],
        volumes: vec![PathBuf::from("/")],
    };
    app.recent_loaded(Ok(saved));
    assert_eq!(app.recent.hosts, ["db", "web"]);
    assert_eq!(
        app.take_recent().map(|recent| recent.hosts),
        Some(app.recent.hosts.clone())
    );
    app.recent_loaded(Err("unreadable".to_owned()));
    assert_eq!(app.recent.hosts, ["db", "web"]);

    // db comes first in the menu, after the home directory.
    menu_on(&mut app, Side::Right);
    let Effect::List { request, host, .. } = one(choose(&mut app, 1)) else {
        panic!("expected a listing");
    };
    assert_eq!((request.location, host.is_some()), (remote("db", ""), true));

    // A panel that arrives on a volume uses it.
    let volume = |path: &str| noc_vfs::Volume {
        mount_point: PathBuf::from(path),
        label: None,
        fs_type: None,
        kind: noc_vfs::VolumeKind::Local,
        space: None,
    };
    let places = Places {
        volumes: vec![volume("/"), volume("/Volumes/USB")],
        hosts: Vec::new(),
    };
    app.places(0, Ok(places));
    app.active = Side::Left;
    let effects = app.go(
        app.shown(Side::Left),
        Destination::to(local("/Volumes/USB/a")),
    );
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    assert_eq!(
        app.take_recent().map(|recent| recent.volumes),
        Some(vec![PathBuf::from("/Volumes/USB"), PathBuf::from("/")])
    );
    let effects = app.go(
        app.shown(Side::Left),
        Destination::to(local("/Volumes/USB/b")),
    );
    answer(&mut app, effects, &Listing::Dir(Vec::new()));
    assert_eq!(app.take_recent(), None, "already the first");
}

#[test]
fn disconnect_stops_a_connection_attempt() {
    let mut app = at_slash();
    let Effect::Connect { stop, .. } = one(enter_host(&mut app, Side::Left, 2)) else {
        panic!("expected a connection");
    };
    assert_eq!(app.host_state("db").status, HostStatus::Connecting);
    menu_on(&mut app, Side::Left);
    app.handle(action(Action::End));
    assert!(app.handle(action(Action::Disconnect)).is_empty());
    assert!(stop.is_cancelled());
    assert_eq!(app.panel(Side::Left).pending_request(), None);
    assert_eq!(app.host_state("db").status, HostStatus::Idle);
}

#[test]
fn the_menu_shows_failures_and_fresh_addresses() {
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
        panic!("expected a connection");
    };
    app.resolved("web".to_owned(), "deploy@10.0.0.9".to_owned(), "deploy");
    app.closed("web", connection, Some("Connection refused"));
    app.handle(action(Action::Cancel));
    assert_eq!(
        app.host_state("web"),
        HostState {
            status: HostStatus::Failed,
            address: Some("deploy@10.0.0.9".to_owned()),
        }
    );
    menu_on(&mut app, Side::Left);
    let text = screen_of(&mut app, 14);
    assert!(text.contains("✗ web"), "{text}");
    assert!(text.contains("deploy@10.0.0.9"), "{text}");

    // A new attempt clears the failure.
    app.handle(action(Action::Cancel));
    enter_host(&mut app, Side::Left, 1);
    assert_eq!(app.host_state("web").status, HostStatus::Connecting);
}
