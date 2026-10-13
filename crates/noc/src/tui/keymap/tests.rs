use super::*;

/// A key press from its `crokey` name, or a single character such as `é`.
fn key(text: &str) -> KeyEvent {
    let mut chars = text.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && !c.is_ascii()
    {
        return KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    }
    let combination = crokey::parse(text).unwrap().normalized();
    KeyEvent::new(*combination.codes.first(), combination.modifiers)
}

/// Feeds `keys` one by one at the same instant, typed in `mode` where `context` has the keys,
/// and collects what they resolved to.
fn feed(
    keymap: &Keymap,
    state: &mut KeyState,
    place: (Mode, Context),
    keys: &[&str],
) -> Vec<Typed> {
    let now = Instant::now();
    keys.iter()
        .flat_map(|text| keymap.feed(state, place, key(text), now))
        .collect()
}

/// What `keys` resolve to from a fresh state.
fn typed(place: (Mode, Context), keys: &[&str]) -> Vec<Typed> {
    feed(&Keymap::default(), &mut KeyState::default(), place, keys)
}

fn actions(actions: &[Action]) -> Vec<Typed> {
    actions
        .iter()
        .map(|action| Typed::from(Resolved::Action(*action)))
        .collect()
}

fn inserts(text: &str) -> Vec<Typed> {
    text.chars()
        .map(|c| Typed::from(Resolved::Insert(c)))
        .collect()
}

fn with(action: Action, extra: Extra) -> Vec<Typed> {
    vec![Typed {
        resolved: Resolved::Action(action),
        extra,
    }]
}

const PANEL: (Mode, Context) = (Mode::Normal, Context::Panel);
const VISUAL_PANEL: (Mode, Context) = (Mode::Visual, Context::Panel);
const FIELD: (Mode, Context) = (Mode::Normal, Context::DialogInput);
const TYPING: (Mode, Context) = (Mode::Insert, Context::DialogInput);
const SELECTING: (Mode, Context) = (Mode::Visual, Context::DialogInput);

#[test]
fn single_keys_resolve_at_once() {
    assert_eq!(typed(PANEL, &["j"]), actions(&[Action::Down]));
    assert_eq!(typed(PANEL, &["f5"]), actions(&[Action::Copy]));
    assert_eq!(typed(PANEL, &["ctrl-o"]), actions(&[Action::JumpBack]));
}

#[test]
fn sequences_wait_for_their_next_key_and_expire() {
    let keymap = Keymap::default();
    let mut state = KeyState::default();
    let start = Instant::now();
    assert_eq!(keymap.feed(&mut state, PANEL, key("g"), start), []);
    assert_eq!(state.deadline(), Some(start + SEQUENCE_TIMEOUT));
    assert_eq!(state.showcmd(), "g");
    assert_eq!(keymap.expire(&mut state, start), [], "not yet");
    assert_eq!(keymap.expire(&mut state, start + SEQUENCE_TIMEOUT), []);
    assert_eq!(state.deadline(), None, "g alone does nothing");
    assert_eq!(
        feed(&keymap, &mut state, PANEL, &["g", "g"]),
        actions(&[Action::Home])
    );
    // A key that cannot go on gives up the sequence and itself, as in vim.
    assert_eq!(feed(&keymap, &mut state, PANEL, &["g", "j"]), []);
    assert!(state.take_dropped());
    assert_eq!(state.showcmd(), "");
}

#[test]
fn esc_acts_at_once_and_gives_up_what_is_typed() {
    assert_eq!(typed(PANEL, &["esc"]), actions(&[Action::Cancel]));
    assert_eq!(typed(PANEL, &["g", "esc"]), [], "g Esc gives g up");
    assert_eq!(typed(PANEL, &["3", "esc", "j"]), actions(&[Action::Down]));
    assert_eq!(
        typed(FIELD, &["f", "esc", "x"]),
        actions(&[Action::DeleteChar])
    );
    // Esc never waits: Esc / cancels, then searches.
    assert_eq!(
        typed(PANEL, &["esc", "/"]),
        actions(&[Action::Cancel, Action::QuickSearch])
    );
}

#[test]
fn a_count_goes_with_the_command_after_it() {
    let count = |count| Extra {
        count: Some(count),
        ..Extra::default()
    };
    assert_eq!(typed(PANEL, &["3", "j"]), with(Action::Down, count(3)));
    assert_eq!(
        typed(PANEL, &["1", "0", "j"]),
        with(Action::Down, count(10))
    );
    assert_eq!(
        typed(PANEL, &["2", "g", "t"]),
        with(Action::NextTab, count(2))
    );
    assert_eq!(
        typed(FIELD, &["0"]),
        actions(&[Action::TextStart]),
        "0 alone"
    );
    assert_eq!(
        typed(FIELD, &["2", "0", "x"]),
        with(Action::DeleteChar, count(20))
    );
    let mut state = KeyState::default();
    let keymap = Keymap::default();
    feed(&keymap, &mut state, PANEL, &["4", "2"]);
    assert_eq!(state.showcmd(), "42");
    assert_eq!(
        typed(TYPING, &["3", "j"]),
        inserts("3j"),
        "digits are text in Insert mode"
    );
}

#[test]
fn a_register_and_a_character_go_with_the_command() {
    assert_eq!(
        typed(FIELD, &["\"", "a", "2", "y", "w"]),
        [
            Typed {
                resolved: Resolved::Action(Action::YankOperator),
                extra: Extra {
                    count: Some(2),
                    register: Some('a'),
                    ..Extra::default()
                }
            },
            Typed::from(Resolved::Action(Action::WordForward))
        ]
    );
    assert_eq!(
        typed(FIELD, &["3", "f", "x"]),
        with(
            Action::FindChar,
            Extra {
                count: Some(3),
                char: Some('x'),
                ..Extra::default()
            }
        )
    );
    // The character is any: f j finds a j, and does not move down.
    assert_eq!(
        typed(FIELD, &["t", "j"]),
        with(
            Action::TillChar,
            Extra {
                char: Some('j'),
                ..Extra::default()
            }
        )
    );
    let keymap = Keymap::default();
    let mut state = KeyState::default();
    feed(&keymap, &mut state, FIELD, &["2", "\"", "b"]);
    assert_eq!(state.showcmd(), "2\"b");
    feed(&keymap, &mut state, FIELD, &["f"]);
    assert_eq!(state.showcmd(), "2\"bf");
    // In Insert mode, Ctrl+r takes the register to put, and Ctrl+v types the next key.
    assert_eq!(
        typed(TYPING, &["ctrl-r", "a"]),
        with(
            Action::PasteRegister,
            Extra {
                char: Some('a'),
                ..Extra::default()
            }
        )
    );
    assert_eq!(typed(TYPING, &["ctrl-v", "tab"]), inserts("\t"));
}

#[test]
fn insert_mode_takes_characters_as_text() {
    // Even those that the fields' Normal mode, the dialog, or the panel bind.
    assert_eq!(typed(TYPING, &["j", "q", "x", " "]), inserts("jqx "));
    assert_eq!(
        typed((Mode::Insert, Context::QuickSearch), &["g", "<", "*"]),
        inserts("g<*")
    );
    assert_eq!(
        typed((Mode::Insert, Context::CommandLine), &["é"]),
        inserts("é")
    );
    assert_eq!(typed(TYPING, &["esc"]), actions(&[Action::NormalMode]));
    assert_eq!(typed(TYPING, &["ctrl-c"]), actions(&[Action::Cancel]));
    assert_eq!(typed(TYPING, &["ctrl-o"]), actions(&[Action::NormalOnce]));
}

#[test]
fn normal_mode_takes_characters_as_commands() {
    assert_eq!(typed(PANEL, &["a"]), [], "nothing binds a in a panel");
    assert_eq!(typed(PANEL, &["?"]), []);
    assert_eq!(typed(FIELD, &["i"]), actions(&[Action::InsertMode]));
    assert_eq!(
        typed(FIELD, &["j"]),
        actions(&[Action::Down]),
        "the dialog's"
    );
    assert_eq!(typed(FIELD, &["q"]), actions(&[Action::Cancel]));
    assert_eq!(typed(FIELD, &["enter"]), actions(&[Action::Confirm]));
    assert_eq!(
        typed(FIELD, &["d", "d"]),
        actions(&[Action::DeleteOperator; 2])
    );
    assert_eq!(typed(SELECTING, &["d"]), actions(&[Action::DeleteOperator]));
    assert_eq!(typed(SELECTING, &["esc"]), actions(&[Action::NormalMode]));
}

#[test]
fn the_lists_take_their_filter_in_insert_mode() {
    let list = (Mode::Normal, Context::LocationMenu);
    let filter = (Mode::Insert, Context::LocationMenu);
    assert_eq!(
        typed(list, &["j", "/"]),
        actions(&[Action::Down, Action::QuickSearch])
    );
    assert_eq!(
        typed(list, &["h", "w", "1"]),
        [],
        "no motions of text, no hotkeys"
    );
    assert_eq!(typed(list, &["d", "d"]), actions(&[Action::Disconnect]));
    assert_eq!(typed(filter, &["j", "1"]), inserts("j1"));
    assert_eq!(
        typed(filter, &["backspace", "home", "esc", "ctrl-c"]),
        actions(&[
            Action::Backspace,
            Action::TextStart,
            Action::NormalMode,
            Action::Cancel
        ])
    );
    assert_eq!(typed(filter, &["f8"]), actions(&[Action::Disconnect]));
}

#[test]
fn visual_mode_selects_rows_and_acts_on_them() {
    assert_eq!(typed(PANEL, &["shift-v"]), actions(&[Action::VisualMode]));
    assert_eq!(
        typed(VISUAL_PANEL, &["j", "o", "y", "space", "v"]),
        actions(&[
            Action::Down,
            Action::OtherEnd,
            Action::Copy,
            Action::Mark,
            Action::NormalMode
        ])
    );
    assert_eq!(typed(PANEL, &["g", "v"]), actions(&[Action::Reselect]));
}

#[test]
fn tab_and_ctrl_i_go_forward_where_ctrl_o_goes_back() {
    assert_eq!(
        typed(PANEL, &["ctrl-o", "tab", "ctrl-i"]),
        actions(&[Action::JumpBack, Action::JumpForward, Action::JumpForward])
    );
    assert_eq!(
        typed(PANEL, &["ctrl-w", "w"]),
        actions(&[Action::SwitchPanel])
    );
    assert_eq!(
        typed((Mode::Normal, Context::Dialog), &["tab"]),
        actions(&[Action::NextField])
    );
    assert_eq!(
        typed((Mode::Insert, Context::PathInput), &["tab"]),
        actions(&[Action::Complete])
    );
}

#[test]
fn alt_and_a_key_stand_for_esc_and_the_key_in_insert_mode_and_the_panels() {
    let keymap = Keymap::default();
    let state = KeyState::default();
    let alt_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT);
    let plain = Some(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(keymap.alt_as_esc(&state, TYPING, alt_j), plain);
    assert_eq!(keymap.alt_as_esc(&state, PANEL, alt_j), plain);
    assert_eq!(
        keymap.alt_as_esc(&state, (Mode::Insert, Context::CommandLine), alt_j),
        plain
    );
    assert_eq!(
        keymap.alt_as_esc(&state, (Mode::Normal, Context::Dialog), alt_j),
        None,
        "Esc would close the dialog"
    );
    let alt_f1 = KeyEvent::new(KeyCode::F(1), KeyModifiers::ALT);
    assert_eq!(keymap.alt_as_esc(&state, PANEL, alt_f1), None, "bound");
    // Any key: an Esc and a quick Enter, Backspace, or arrow arrive with Alt too.
    for code in [
        KeyCode::Enter,
        KeyCode::Backspace,
        KeyCode::Left,
        KeyCode::Tab,
    ] {
        let alt = KeyEvent::new(code, KeyModifiers::ALT);
        let plain = Some(KeyEvent::new(code, KeyModifiers::NONE));
        assert_eq!(keymap.alt_as_esc(&state, TYPING, alt), plain, "{code:?}");
    }
    let alt_esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT);
    assert_eq!(keymap.alt_as_esc(&state, TYPING, alt_esc), None);
    let ctrl_alt_w = KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::ALT | KeyModifiers::CONTROL,
    );
    assert_eq!(
        keymap.alt_as_esc(&state, TYPING, ctrl_alt_w),
        Some(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
    );
    assert_eq!(
        keymap.alt_as_esc(&state, VISUAL_PANEL, alt_j),
        plain,
        "Esc leaves Visual mode"
    );
    // With a command typed so far, the Esc gives it up, unless the key goes on with it.
    let mut pending = KeyState::default();
    feed(&keymap, &mut pending, PANEL, &["3"]);
    assert_eq!(keymap.alt_as_esc(&pending, PANEL, alt_j), plain);
    let keymap = Keymap::of(&[(
        Mode::Normal,
        Context::Panel,
        &[(Action::Home, &["g alt-j"])],
    )]);
    let mut pending = KeyState::default();
    feed(&keymap, &mut pending, PANEL, &["g"]);
    assert_eq!(keymap.alt_as_esc(&pending, PANEL, alt_j), None);
}

#[test]
fn the_pull_down_menu_runs_letters_with_alt_and_moves_with_hjkl() {
    let menu = (Mode::Normal, Context::PullDown);
    assert_eq!(
        typed(menu, &["h", "j", "k", "l"]),
        actions(&[Action::Left, Action::Down, Action::Up, Action::Right])
    );
    assert_eq!(typed(menu, &["alt-c", "alt-2"]), inserts("c2"));
    assert_eq!(typed(menu, &["q"]), actions(&[Action::Cancel]));
    // A count goes with the letter, not with the key after it.
    assert_eq!(
        typed(menu, &["3", "alt-z", "j"]),
        [
            Typed::from(Resolved::Insert('z')),
            Typed::from(Resolved::Action(Action::Down))
        ]
    );
}

#[test]
fn a_register_is_named_wherever_keys_are_commands() {
    // The name is never a command, as in vim: "x moves no files.
    assert_eq!(typed(PANEL, &["\"", "x"]), []);
    assert_eq!(typed(VISUAL_PANEL, &["\"", "d"]), []);
    assert_eq!(
        typed(PANEL, &["\"", "x", "j"]),
        with(
            Action::Down,
            Extra {
                register: Some('x'),
                ..Extra::default()
            }
        )
    );
    assert_eq!(typed((Mode::Normal, Context::Viewer), &["\"", "q"]), []);
    assert_eq!(typed(TYPING, &["\""]), inserts("\""), "text in Insert mode");
    assert_eq!(
        typed((Mode::Insert, Context::CommandLine), &["\""]),
        inserts("\"")
    );
}

#[test]
fn ctrl_k_after_a_sequence_shows_what_can_follow_it() {
    let keymap = Keymap::default();
    let mut state = KeyState::default();
    assert_eq!(feed(&keymap, &mut state, PANEL, &["g", "ctrl-k"]), []);
    let hints = keymap.hints_of(&state, PANEL).expect("the hints of g");
    assert_eq!(hints.typed.as_deref(), Some("g"));
    assert!(!state.take_dropped());
    assert_eq!(
        feed(&keymap, &mut state, PANEL, &["g"]),
        actions(&[Action::Home])
    );
}

#[test]
fn the_command_line_is_vims() {
    let line = (Mode::Insert, Context::CommandLine);
    assert_eq!(typed(line, &["esc"]), actions(&[Action::Cancel]));
    assert_eq!(typed(line, &["ctrl-f"]), actions(&[Action::NormalMode]));
    assert_eq!(
        typed(line, &["ctrl-b", "ctrl-e", "ctrl-j", "ctrl-u", "ctrl-w"]),
        actions(&[
            Action::TextStart,
            Action::TextEnd,
            Action::NewLine,
            Action::DeleteToStart,
            Action::DeleteWord
        ])
    );
    assert_eq!(
        typed(line, &["ctrl-x", "ctrl-e"]),
        actions(&[Action::TextEnd])
    );
    let normal = (Mode::Normal, Context::CommandLine);
    assert_eq!(
        typed(normal, &["k", "w", "q", ":"]),
        actions(&[Action::Up, Action::WordForward, Action::CommandHistory])
    );
    assert_eq!(typed(normal, &["esc"]), actions(&[Action::Cancel]));
}

#[test]
fn quick_search_is_typed_as_vims_search() {
    let search = (Mode::Insert, Context::QuickSearch);
    assert_eq!(
        typed(search, &["enter", "esc", "ctrl-g", "ctrl-t", "backspace"]),
        actions(&[
            Action::Cancel,
            Action::Abort,
            Action::QuickSearch,
            Action::PrevMatch,
            Action::Backspace
        ])
    );
    assert_eq!(
        typed(search, &["down", "pageup", "j"]),
        [actions(&[Action::Down, Action::PageUp]), inserts("j")].concat(),
        "the arrows and the page keys end it, characters are text"
    );
    assert_eq!(
        typed(search, &["f5", "shift-f9", "alt-f1"]),
        actions(&[Action::Copy, Action::PullDownLast, Action::LocationMenuLeft]),
        "and its F-keys"
    );
    assert_eq!(
        typed(search, &["ctrl-o"]),
        [],
        "no Normal mode to go to once"
    );
    let filter = (Mode::Insert, Context::Workspaces);
    assert_eq!(typed(filter, &["ctrl-o"]), []);
    assert_eq!(
        typed(TYPING, &["ctrl-o"]),
        actions(&[Action::NormalOnce]),
        "a field has one"
    );
}

#[test]
fn the_f_keys_work_in_visual_mode_too() {
    let keymap = Keymap::default();
    assert_eq!(
        keymap.fkeys(Mode::Visual, Context::Panel),
        keymap.fkeys(Mode::Normal, Context::Panel)
    );
    assert_eq!(
        typed(VISUAL_PANEL, &["shift-f6", "shift-f9", "alt-f2"]),
        actions(&[
            Action::Rename,
            Action::PullDownLast,
            Action::LocationMenuRight
        ])
    );
}

#[test]
fn a_count_before_a_register_and_one_after_it_multiply() {
    let keymap = Keymap::default();
    let mut state = KeyState::default();
    feed(&keymap, &mut state, FIELD, &["2", "\"", "a", "3"]);
    assert_eq!(state.showcmd(), "2\"a3", "as typed");
    assert_eq!(
        feed(&keymap, &mut state, FIELD, &["x"]),
        with(
            Action::DeleteChar,
            Extra {
                count: Some(6),
                register: Some('a'),
                outer: Some(2),
                char: None
            }
        )
    );
}

#[test]
fn a_command_given_up_says_so_for_the_app_to_give_up_its_part() {
    let keymap = Keymap::default();
    let mut state = KeyState::default();
    let give_up = |state: &mut KeyState, keys: &[&str]| {
        let typed = feed(&keymap, state, FIELD, keys);
        (typed, state.take_dropped())
    };
    assert!(!give_up(&mut state, &["d"]).1, "an operator resolves");
    assert_eq!(give_up(&mut state, &["f", "esc"]), (vec![], true));
    assert_eq!(give_up(&mut state, &["2", "esc"]), (vec![], true));
    assert_eq!(
        give_up(&mut state, &["f", "left"]),
        (vec![], true),
        "no character"
    );
    assert_eq!(
        give_up(&mut state, &["z"]),
        (vec![], true),
        "a key that does nothing"
    );
    assert!(!give_up(&mut state, &["esc"]).1, "Esc alone is a key");
    assert!(!state.take_dropped(), "once");
}

#[test]
fn marks_by_pattern_are_under_g_and_mcs_own_keys_are_gone() {
    assert_eq!(typed(PANEL, &["g", "+"]), actions(&[Action::Select]));
    assert_eq!(typed(PANEL, &["g", "-"]), actions(&[Action::Unselect]));
    assert_eq!(typed(PANEL, &["g", "*"]), actions(&[Action::InvertMarks]));
    for key in ["+", "-", "*", "shift-up", "shift-down"] {
        assert_eq!(typed(PANEL, &[key]), [], "{key}");
    }
}

#[test]
fn a_key_that_cannot_go_on_with_a_sequence_gives_it_up_in_normal_and_visual_mode() {
    // As vim beeps at `g x` and Ctrl+w and a key that names no window, neither runs.
    for keys in [
        &["g", "x"][..],
        &["ctrl-w", "e"],
        &["ctrl-w", "ctrl-h"],
        &["3", "g", "x"],
    ] {
        assert_eq!(typed(PANEL, keys), [], "{keys:?}");
    }
    assert_eq!(typed(VISUAL_PANEL, &["ctrl-w", "x"]), [], "no Move");
    assert_eq!(typed(FIELD, &["g", "u"]), [], "no undo");
    // The next key is a command of its own, as `j` after vim's `g3`.
    assert_eq!(typed(PANEL, &["g", "x", "j"]), actions(&[Action::Down]));
    assert_eq!(typed(PANEL, &["g", "3", "j"]), actions(&[Action::Down]));
    // A sequence that does something alone does it, and the key goes on as its own, as a
    // mapping of vim's that starts a longer one.
    let keymap = Keymap::of(&[(
        Mode::Normal,
        Context::Panel,
        &[(Action::Home, &["g"]), (Action::End, &["g g"])],
    )]);
    assert_eq!(
        feed(&keymap, &mut KeyState::default(), PANEL, &["g", "j"]),
        actions(&[Action::Home])
    );
    // And in Insert mode, a character that cannot go on with a sequence is text.
    let keymap = Keymap::of(&[(
        Mode::Insert,
        Context::Input,
        &[(Action::DeleteWord, &["ctrl-x ctrl-w"])],
    )]);
    let mut state = KeyState::default();
    assert_eq!(
        feed(&keymap, &mut state, TYPING, &["ctrl-x", "a"]),
        inserts("a")
    );
}

#[test]
fn a_bound_prefix_that_times_out_under_the_hints_closes_them() {
    let keymap = Keymap::of(&[(
        Mode::Normal,
        Context::Panel,
        &[(Action::Home, &["g"]), (Action::End, &["g g"])],
    )]);
    let mut state = KeyState::default();
    let start = Instant::now();
    keymap.show_hints(&mut state, PANEL);
    assert_eq!(keymap.feed(&mut state, PANEL, key("g"), start), []);
    assert_eq!(
        keymap.expire(&mut state, start + SEQUENCE_TIMEOUT),
        actions(&[Action::Home])
    );
    assert_eq!(keymap.hints_of(&state, PANEL), None);
}

#[test]
fn the_user_screen_goes_back_with_g_lt_enter_q_esc_and_ctrl_c() {
    let screen = (Mode::Normal, Context::UserScreen);
    for keys in [&["g", "<"][..], &["enter"], &["q"], &["esc"]] {
        assert_eq!(typed(screen, keys), actions(&[Action::Cancel]), "{keys:?}");
    }
    assert_eq!(typed(screen, &["ctrl-c"]), actions(&[Action::Cancel]));
    assert_eq!(typed(screen, &["ctrl-o"]), []);
    assert_eq!(typed(screen, &["ctrl-l"]), [], "nothing global");
}

#[test]
fn global_keys_work_in_every_mode() {
    let keymap = Keymap::default();
    for &context in Context::ALL {
        if matches!(context, Context::Global | Context::UserScreen) {
            continue;
        }
        for &mode in Mode::ALL {
            let mut state = KeyState::default();
            assert_eq!(
                feed(&keymap, &mut state, (mode, context), &["ctrl-k", "ctrl-l"]),
                actions(&[Action::KeyHints, Action::Redraw]),
                "{mode:?} {context:?}"
            );
        }
    }
    assert_eq!(typed(PANEL, &["g", "<"]), actions(&[Action::UserScreen]));
    assert_eq!(typed(TYPING, &["g", "<"]), inserts("g<"));
}

#[test]
fn f_keys_are_mcs() {
    let keymap = Keymap::default();
    let mut panel = [None; 10];
    panel[0] = Some(Action::Help);
    panel[2] = Some(Action::View);
    panel[3] = Some(Action::Edit);
    panel[4] = Some(Action::Copy);
    panel[5] = Some(Action::Move);
    panel[6] = Some(Action::Mkdir);
    panel[7] = Some(Action::Delete);
    panel[8] = Some(Action::PullDown);
    panel[9] = Some(Action::Quit);
    assert_eq!(keymap.fkeys(Mode::Normal, Context::Panel), panel);
    assert_eq!(keymap.fkeys(Mode::Insert, Context::QuickSearch), panel);
    let mut dialog = [None; 10];
    dialog[9] = Some(Action::Cancel);
    assert_eq!(keymap.fkeys(Mode::Normal, Context::Dialog), dialog);
    assert_eq!(keymap.fkeys(Mode::Insert, Context::DialogInput), dialog);
    assert_eq!(
        typed(PANEL, &["alt-f1", "alt-f2", "shift-f6", "shift-f9"]),
        actions(&[
            Action::LocationMenuLeft,
            Action::LocationMenuRight,
            Action::Rename,
            Action::PullDownLast
        ])
    );
}

#[test]
fn the_preset_binds_each_key_once_and_every_action() {
    for (mode, context, bound) in preset::PRESET {
        let mut seen: Vec<Sequence> = Vec::new();
        for (_, keys) in *bound {
            for keys in *keys {
                let sequence = parse_sequence(keys).unwrap_or_else(|| panic!("`{keys}`"));
                assert!(
                    !seen.contains(&sequence),
                    "{mode:?} {context:?}: `{keys}` twice"
                );
                seen.push(sequence);
            }
        }
        // A prefix binds nothing itself, or keys would wait for what follows it.
        for sequence in &seen {
            for len in 1..sequence.len() {
                assert!(
                    !seen.contains(&sequence[..len].to_vec()),
                    "{mode:?} {context:?}: {} starts a longer sequence",
                    describe(&sequence[..len])
                );
            }
        }
    }
    let bound: Vec<Action> = preset::PRESET
        .iter()
        .flat_map(|(_, _, bound)| bound.iter().map(|(action, _)| *action))
        .collect();
    for action in Action::ALL {
        assert!(bound.contains(action), "{action:?} is not bound");
    }
}

#[test]
fn the_preset_has_each_mode_and_context_once_and_global_keys_in_normal_mode() {
    let mut seen: Vec<(Mode, Context)> = Vec::new();
    for (mode, context, bound) in preset::PRESET {
        assert!(
            !seen.contains(&(*mode, *context)),
            "{mode:?} {context:?} twice"
        );
        seen.push((*mode, *context));
        if *context == Context::Global {
            assert_eq!(*mode, Mode::Normal, "Global is the same in every mode");
        }
        // In Insert mode a character is text unless the context itself binds it, so a context
        // that others fall back to would bind it in vain.
        if *mode == Mode::Insert && matches!(context, Context::Input | Context::List) {
            for (action, keys) in *bound {
                for keys in *keys {
                    let first = parse_sequence(keys).unwrap()[0];
                    assert!(!is_character(first), "{action:?}: `{keys}` would be text");
                }
            }
        }
    }
}

#[test]
fn describes_keys_as_the_docs_write_them() {
    let described = |keys: &str| describe(&parse_sequence(keys).unwrap());
    for (keys, text) in [
        ("ctrl-r", "Ctrl+r"),
        ("alt-shift-w", "Alt+W"),
        ("shift-g", "G"),
        ("shift-z shift-z", "Z Z"),
        ("ctrl-w h", "Ctrl+w h"),
        ("shift-f6", "Shift+F6"),
        ("shift-down", "Shift+Down"),
        ("backtab", "Shift+Tab"),
        ("pageup", "PgUp"),
        ("pagedown", "PgDn"),
        ("space", "Space"),
        ("alt-.", "Alt+."),
        ("alt-+", "Alt++"),
        ("\"", "\""),
        ("ctrl-f3", "Ctrl+F3"),
    ] {
        assert_eq!(described(keys), text, "{keys}");
    }
    let typed = KeyCombination::from(KeyEvent::new(KeyCode::Char('*'), KeyModifiers::SHIFT));
    assert_eq!(describe(&[typed]), "*", "a symbol as it is typed");
    let short = |keys: &str| short_key(parse_sequence(keys).unwrap()[0]);
    assert_eq!(short("ctrl-w"), "^W");
    assert_eq!(short("shift-g"), "G");
    assert_eq!(short("\""), "\"");
}

#[test]
fn shifted_letters_match_their_bindings() {
    assert_eq!(
        KeyCombination::from(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
        parse_sequence("shift-a").unwrap()[0]
    );
    assert_eq!(
        typed(PANEL, &["shift-g"]),
        actions(&[Action::End]),
        "G as the terminal sends it"
    );
}

#[test]
fn key_names_the_first_sequence_that_does_the_action() {
    let keymap = Keymap::default();
    assert_eq!(
        keymap.key(Context::Panel, Action::View).as_deref(),
        Some("i")
    );
    assert_eq!(
        keymap.key(Context::Panel, Action::NextTab).as_deref(),
        Some("g t")
    );
    assert_eq!(
        keymap.key(Context::Panel, Action::Redraw).as_deref(),
        Some("Ctrl+l")
    );
    assert_eq!(
        keymap.key(Context::Panel, Action::Reload).as_deref(),
        Some("R")
    );
}

#[test]
fn a_new_mode_or_context_forgets_a_pending_command() {
    let keymap = Keymap::default();
    let mut state = KeyState::default();
    feed(&keymap, &mut state, PANEL, &["3", "g"]);
    assert_eq!(feed(&keymap, &mut state, VISUAL_PANEL, &["g"]), []);
    assert_eq!(
        feed(&keymap, &mut state, VISUAL_PANEL, &["g"]),
        actions(&[Action::Home]),
        "without the count"
    );
}

fn hint_keys(hints: &[Hint]) -> Vec<&str> {
    hints.iter().map(|hint| hint.keys.as_str()).collect()
}

#[test]
fn hints_list_the_keys_that_can_follow_in_the_order_of_the_preset() {
    let keymap = Keymap::default();
    let ctrl_w = parse_sequence("ctrl-w").unwrap();
    assert_eq!(
        hint_keys(&keymap.hints(PANEL, &ctrl_w)),
        ["w, Ctrl+w, p", "h", "l", "x, r", "f", "v", "1", "2"]
    );
    let t = parse_sequence("t").unwrap();
    let hints = keymap.hints(PANEL, &t);
    assert_eq!(hint_keys(&hints), ["n", "c", "h", "l", "w", "W"]);
    assert_eq!(
        hints[0],
        Hint {
            keys: "n".to_owned(),
            action: Some(Action::NewTab),
            more: 0
        }
    );
    let every = keymap.hints(PANEL, &[]);
    let hint = |keys: &str| every.iter().find(|hint| hint.keys == keys).cloned();
    assert_eq!(hint("Ctrl+w").map(|hint| hint.more), Some(11));
    assert_eq!(hint("Z").map(|hint| hint.more), Some(1));
    assert!(
        every.iter().all(|hint| !hint.keys.contains("Esc")),
        "Esc closes the hints"
    );
    // In Insert mode, characters are text: the hints leave them out.
    let search = keymap.hints((Mode::Insert, Context::QuickSearch), &[]);
    assert!(hint_keys(&search).iter().all(|keys| keys.len() > 1));
    assert!(hint_keys(&search).contains(&"Ctrl+g"));
}

#[test]
fn hints_wait_for_the_next_key_which_runs_as_it_would_have() {
    let keymap = Keymap::default();
    let mut state = KeyState::default();
    let start = Instant::now();
    let delay = Duration::from_millis(500);
    assert_eq!(keymap.hints_due(&state, delay), None);
    assert_eq!(keymap.feed(&mut state, PANEL, key("t"), start), []);
    assert_eq!(keymap.hints_due(&state, delay), Some(start + delay));
    assert_eq!(keymap.hints_of(&state, PANEL), None, "not yet");
    keymap.show_hints(&mut state, PANEL);
    assert_eq!(state.deadline(), None, "no timeout while they show");
    assert_eq!(keymap.hints_due(&state, delay), None, "they show");
    let hints = keymap.hints_of(&state, PANEL).unwrap();
    assert_eq!(hints.typed.as_deref(), Some("t"));
    assert_eq!(keymap.hints_of(&state, VISUAL_PANEL), None);
    assert_eq!(
        keymap.feed(&mut state, PANEL, key("n"), start),
        actions(&[Action::NewTab])
    );
    assert_eq!(keymap.hints_of(&state, PANEL), None, "done");
}

#[test]
fn esc_closes_the_hints_and_backspace_takes_back_a_key() {
    let keymap = Keymap::default();
    let mut state = KeyState::default();
    let now = Instant::now();
    keymap.show_hints(&mut state, PANEL);
    let every_key = keymap.hints_of(&state, PANEL).unwrap();
    assert_eq!(every_key.typed, None);
    assert_eq!(keymap.feed(&mut state, PANEL, key("ctrl-w"), now), []);
    assert_eq!(state.deadline(), None, "the hints show at once");
    assert_eq!(
        keymap.hints_of(&state, PANEL).unwrap().typed,
        Some("Ctrl+w".to_owned())
    );
    assert_eq!(keymap.feed(&mut state, PANEL, key("backspace"), now), []);
    assert_eq!(keymap.hints_of(&state, PANEL), Some(every_key));
    assert_eq!(
        keymap.feed(&mut state, PANEL, key("esc"), now),
        [],
        "no Cancel"
    );
    assert_eq!(keymap.hints_of(&state, PANEL), None);
    // A key that does something closes them too.
    keymap.show_hints(&mut state, PANEL);
    assert_eq!(
        keymap.feed(&mut state, PANEL, key("f10"), now),
        actions(&[Action::Quit])
    );
    assert_eq!(keymap.hints_of(&state, PANEL), None);
    // Backspace with nothing typed closes them, and does nothing in the panel.
    keymap.show_hints(&mut state, PANEL);
    assert_eq!(keymap.feed(&mut state, PANEL, key("backspace"), now), []);
    assert_eq!(keymap.hints_of(&state, PANEL), None);
    // A count or a register typed before Ctrl+k waits for the key after the hints.
    assert_eq!(
        feed(&keymap, &mut state, PANEL, &["3", "ctrl-k"]),
        actions(&[Action::KeyHints]),
        "without the count"
    );
    keymap.show_hints(&mut state, PANEL);
    assert_eq!(state.showcmd(), "3");
    assert_eq!(
        feed(&keymap, &mut state, PANEL, &["j"]),
        with(
            Action::Down,
            Extra {
                count: Some(3),
                ..Extra::default()
            }
        )
    );
    assert_eq!(
        feed(&keymap, &mut state, FIELD, &["\"", "a", "ctrl-k"]),
        actions(&[Action::KeyHints])
    );
    keymap.show_hints(&mut state, FIELD);
    assert_eq!(
        feed(&keymap, &mut state, FIELD, &["y"]),
        with(
            Action::YankOperator,
            Extra {
                register: Some('a'),
                ..Extra::default()
            }
        )
    );
}

#[test]
fn the_help_lists_the_keys_a_context_binds_in_a_mode() {
    let keymap = Keymap::default();
    let typing = keymap.help(Mode::Insert, Context::Input);
    assert!(typing.contains(&(Action::NormalMode, "Esc, Ctrl+[".to_owned())));
    assert!(typing.contains(&(Action::DeleteWord, "Ctrl+w".to_owned())));
    let normal = keymap.help(Mode::Normal, Context::Rename);
    assert_eq!(
        normal,
        [
            (Action::Confirm, "Enter, Ctrl+m".to_owned()),
            (Action::Cancel, "Esc, Ctrl+[, Ctrl+c".to_owned())
        ]
    );
    let fields = keymap.help(Mode::Normal, Context::Input);
    assert!(fields.contains(&(Action::WordForward, "w, Shift+Right".to_owned())));
    // As vim's Ctrl+Right, a WORD.
    assert!(fields.contains(&(Action::BigWordForward, "W, Ctrl+Right".to_owned())));
    // Keys that a context binds but its chain takes before them are left out.
    let filter = keymap.help(Mode::Insert, Context::List);
    assert!(filter.contains(&(Action::Up, "Up, Ctrl+p".to_owned())));
}

#[test]
fn alt_and_a_shifted_symbol_match_as_the_kitty_protocol_reports_alternate_keys() {
    // With alternate keys reported, crossterm gives the symbol typed and drops the Shift.
    let keymap = Keymap::of(&[(
        Mode::Normal,
        Context::Panel,
        &[
            (Action::KeyHints, &["alt-/"]),
            (Action::SaveWorkspace, &["alt-shift-w"]),
        ],
    )]);
    let mut state = KeyState::default();
    let now = Instant::now();
    let alt = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::ALT);
    assert_eq!(
        keymap.feed(&mut state, PANEL, alt, now),
        actions(&[Action::KeyHints])
    );
    let upper = KeyEvent::new(KeyCode::Char('W'), KeyModifiers::ALT);
    assert_eq!(
        keymap.feed(&mut KeyState::default(), PANEL, upper, now),
        actions(&[Action::SaveWorkspace]),
        "Shift comes back"
    );
}

#[test]
fn rejects_invalid_sequences() {
    assert_eq!(parse_sequence(""), None);
    assert_eq!(parse_sequence("ctrl-"), None);
    assert_eq!(parse_sequence("f10 nosuchkey"), None);
    assert_eq!(parse_sequence("esc 0").map(|keys| keys.len()), Some(2));
}
