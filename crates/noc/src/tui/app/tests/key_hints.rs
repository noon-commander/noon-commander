use super::*;

#[test]
fn key_hints_show_every_key_of_the_panel_over_it_until_a_dialog_hides_them() {
    let mut app = loaded();
    let mut keys = KeyState::default();
    app.set_hints(&keys);
    assert!(!screen_of(&mut app, 16).contains("Esc closes"));
    // As after Ctrl+x, once they are due.
    app.keymap().show_hints(&mut keys, app.key_place());
    app.set_hints(&keys);
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("Esc closes") && text.contains("Ctrl+w") && text.contains("+9 keys"),
        "{text}"
    );
    // A dialog in front hides them.
    app.handle(action(Action::Mkdir));
    app.set_hints(&keys);
    assert!(!screen_of(&mut app, 16).contains("Esc closes"));
}

/// Feeds `keys` to the app as the event loop does, with the key hints, which `state` keeps.
fn keys_with_hints(app: &mut App, state: &mut KeyState, keys: &[&str]) -> Vec<Effect> {
    let mut effects = Vec::new();
    for key in keys {
        let combination = crokey::parse(key).unwrap().normalized();
        let event = KeyEvent::new(*combination.codes.first(), combination.modifiers);
        effects.extend(app.key(state, event, Instant::now()));
        app.set_hints(state);
    }
    effects
}

#[test]
fn ctrl_k_lists_the_keys_to_filter_and_run_where_they_went() {
    let mut app = loaded();
    let mut state = KeyState::default();
    keys_with_hints(&mut app, &mut state, &["2", "ctrl-k"]);
    assert_eq!(app.place(), (Mode::Insert, Context::KeyHints));
    assert!(app.hints.is_none(), "no hints under the list");
    let text = screen_of(&mut app, 30);
    assert!(
        text.contains("g g, Home") && text.contains(&prompt("Filter", "")),
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
    assert_eq!(app.context(), Context::KeyHints);
    let text = screen_of(&mut app, 30);
    assert!(text.contains("Ctrl+w h") && !text.contains("g g"), "{text}");
    // Ctrl+k in the list leaves it as it is.
    keys_with_hints(&mut app, &mut state, &["ctrl-k"]);
    assert_eq!(app.place(), (Mode::Insert, Context::KeyHints));
    assert_eq!(screen_of(&mut app, 30), text);
    // Esc goes back to the rows, Esc again closes it, and no hints show.
    keys_with_hints(&mut app, &mut state, &["esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::KeyHints));
    keys_with_hints(&mut app, &mut state, &["esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Panel));
    assert!(app.hints.is_none());

    // So does Ctrl+k while the hints of a sequence show.
    keys_with_hints(&mut app, &mut state, &["g"]);
    app.keymap().show_hints(&mut state, app.key_place());
    app.set_hints(&state);
    assert!(app.hints.is_some());
    keys_with_hints(&mut app, &mut state, &["ctrl-k"]);
    assert_eq!(app.context(), Context::KeyHints);
    assert!(app.hints.is_none(), "the list takes the place of the hints");
    assert!(screen_of(&mut app, 30).contains("g g"));
}

#[test]
fn the_list_of_the_keys_after_ctrl_x_runs_its_command_in_the_other_panel() {
    let mut app = loaded();
    let mut state = KeyState::default();
    keys_with_hints(&mut app, &mut state, &["ctrl-x", "ctrl-k"]);
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
fn ctrl_k_lists_the_keys_of_a_dialog_over_it() {
    let mut app = loaded();
    let mut state = KeyState::default();
    app.handle(action(Action::Mkdir));
    let dialog = app.place();
    assert_eq!(dialog, (Mode::Insert, Context::PathInput));
    keys_with_hints(&mut app, &mut state, &["ctrl-k"]);
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
