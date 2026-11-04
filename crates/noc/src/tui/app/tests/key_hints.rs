use super::*;

#[test]
fn key_hints_show_every_key_of_the_panel_over_it_when_asked_for() {
    let mut app = loaded();
    let mut keys = KeyState::default();
    app.set_hints(&keys);
    assert!(!screen_of(&mut app, 16).contains("Esc closes"));
    app.handle(action(Action::KeyHints));
    let place = app.take_hints_asked().unwrap();
    assert_eq!(place, (Mode::Normal, Context::Panel).into());
    assert_eq!(app.take_hints_asked(), None, "asked once");
    app.keymap().show_hints(&mut keys, place);
    app.set_hints(&keys);
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("Esc closes") && text.contains("Ctrl+w") && text.contains("+9 keys"),
        "{text}"
    );
    // A dialog in front hides them…
    app.handle(action(Action::Mkdir));
    app.set_hints(&keys);
    assert!(!screen_of(&mut app, 16).contains("Esc closes"));
    // …and shows its own when asked, staying open.
    let dialog = app.place();
    assert_eq!(dialog, (Mode::Insert, Context::PathInput));
    assert!(app.handle(action(Action::KeyHints)).is_empty());
    assert_eq!(app.take_hints_asked(), Some(dialog.into()));
    assert_eq!(app.place(), dialog);
    app.keymap().show_hints(&mut keys, dialog);
    app.set_hints(&keys);
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("Esc closes") && text.contains("Complete the path"),
        "{text}"
    );
    // So does a menu.
    app.handle(action(Action::Cancel));
    app.handle(action(Action::PullDown));
    assert_eq!(app.context(), Context::PullDown);
    app.handle(action(Action::KeyHints));
    assert_eq!(
        app.take_hints_asked(),
        Some((Mode::Normal, Context::PullDown).into())
    );
    assert_eq!(app.context(), Context::PullDown);
}

/// Feeds `keys` to the app as the event loop does, with the key hints, which `state` keeps.
fn keys_with_hints(app: &mut App, state: &mut KeyState, keys: &[&str]) -> Vec<Effect> {
    let mut effects = Vec::new();
    for key in keys {
        let combination = crokey::parse(key).unwrap().normalized();
        let event = KeyEvent::new(*combination.codes.first(), combination.modifiers);
        effects.extend(app.key(state, event, Instant::now()));
        if let Some(place) = app.take_hints_asked() {
            app.keymap().show_hints(state, place);
        }
        app.set_hints(state);
    }
    effects
}

#[test]
fn ctrl_k_twice_lists_the_keys_to_filter_and_run_where_they_went() {
    let mut app = loaded();
    let mut state = KeyState::default();
    keys_with_hints(&mut app, &mut state, &["2", "ctrl-k"]);
    assert!(app.hints.is_some());
    assert_eq!(app.place(), (Mode::Normal, Context::Panel));
    keys_with_hints(&mut app, &mut state, &["ctrl-k"]);
    assert_eq!(app.place(), (Mode::Insert, Context::KeyHints));
    assert!(app.hints.is_none(), "the list takes the place of the hints");
    let text = screen_of(&mut app, 30);
    assert!(
        text.contains("Ctrl+w h") && text.contains(&prompt("Filter", "")),
        "{text}"
    );
    // What is typed filters, and Enter runs the command in the panel, with the count.
    let typed: Vec<String> = "'row down".chars().map(String::from).collect();
    let typed: Vec<&str> = typed
        .iter()
        .map(|c| if c == " " { "space" } else { c.as_str() })
        .collect();
    keys_with_hints(&mut app, &mut state, &typed);
    keys_with_hints(&mut app, &mut state, &["enter"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Panel));
    assert_eq!(cursor_name(&app, Side::Left), "right", "two rows down");

    // After the first key of a sequence, the list has the sequences after it.
    keys_with_hints(&mut app, &mut state, &["ctrl-w", "ctrl-k"]);
    assert_eq!(
        app.hints.as_ref().and_then(|hints| hints.typed.as_deref()),
        Some("Ctrl+w")
    );
    keys_with_hints(&mut app, &mut state, &["ctrl-k"]);
    assert_eq!(app.context(), Context::KeyHints);
    let text = screen_of(&mut app, 30);
    assert!(text.contains("Ctrl+w h") && !text.contains("g g"), "{text}");
    // Esc goes back to the rows, Esc again closes it, and the hints do not come back.
    keys_with_hints(&mut app, &mut state, &["esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::KeyHints));
    keys_with_hints(&mut app, &mut state, &["esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Panel));
    assert!(app.hints.is_none());
}

#[test]
fn the_list_of_the_keys_after_ctrl_x_runs_its_command_in_the_other_panel() {
    let mut app = loaded();
    let mut state = KeyState::default();
    keys_with_hints(&mut app, &mut state, &["ctrl-x", "ctrl-k", "ctrl-k"]);
    assert_eq!(app.place(), (Mode::Insert, Context::KeyHints));
    assert_eq!(app.active, Side::Right, "the other panel has the keys");
    let typed: Vec<String> = "'row down".chars().map(String::from).collect();
    let typed: Vec<&str> = typed
        .iter()
        .map(|c| if c == " " { "space" } else { c.as_str() })
        .collect();
    keys_with_hints(&mut app, &mut state, &typed);
    keys_with_hints(&mut app, &mut state, &["enter"]);
    assert_eq!(cursor_name(&app, Side::Right), "left");
    assert_eq!(cursor_name(&app, Side::Left), "");
    assert_eq!(app.active, Side::Left);
}

#[test]
fn ctrl_k_twice_lists_the_keys_of_a_dialog_over_it() {
    let mut app = loaded();
    let mut state = KeyState::default();
    app.handle(action(Action::Mkdir));
    let dialog = app.place();
    assert_eq!(dialog, (Mode::Insert, Context::PathInput));
    keys_with_hints(&mut app, &mut state, &["ctrl-k", "ctrl-k"]);
    assert_eq!(app.place(), (Mode::Insert, Context::KeyHints));
    assert!(screen_of(&mut app, 30).contains("Complete the path"));
    keys_with_hints(&mut app, &mut state, &["esc", "esc"]);
    assert_eq!(app.place(), dialog, "the dialog has the keys again");
}

#[test]
fn key_hints_follow_ui_which_key() {
    let mut app = loaded();
    assert_eq!(app.hints_delay(), Some(Duration::from_millis(500)));
    app.config.ui.which_key_delay_ms = 0;
    assert_eq!(app.hints_delay(), Some(Duration::ZERO));
    app.config.ui.which_key = false;
    assert_eq!(app.hints_delay(), None);
}
