use super::*;

#[test]
fn visual_mode_selects_rows_that_the_commands_of_files_act_on() {
    let mut app = with_files(&["a", "b", "c", "d"]);
    keys(&mut app, &["j", "v", "j", "j"]);
    assert_eq!(app.place(), (Mode::Visual, Context::Panel));
    assert_eq!(mode_of(&mut app), "VISUAL");
    keys(&mut app, &["o"]);
    assert_eq!(cursor_name(&app, Side::Left), "a", "the other end");
    keys(&mut app, &["space"]);
    assert_eq!(app.mode(), Mode::Normal, "Space marks the rows and ends it");
    assert_eq!(mode_of(&mut app), "NORMAL");
    assert_eq!(chosen_names(&app, Side::Left), ["a", "b", "c"]);
    // A command acts on the rows selected, and only on them.
    keys(&mut app, &["shift-g", "shift-v", "k", "d"]);
    assert_eq!(chosen_names(&app, Side::Left), ["c", "d"]);
    assert_eq!(app.context(), Context::Dialog, "it asks first");
    keys(&mut app, &["esc"]);
    // g v selects them again; Esc ends Visual mode.
    keys(&mut app, &["g", "v"]);
    assert_eq!(app.mode(), Mode::Visual);
    keys(&mut app, &["esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Panel));
}

#[test]
fn k_shows_the_whole_name_under_the_cursor_below_the_panels_until_asked_again() {
    let long = format!("{}.txt", "a-very-long-name-".repeat(6));
    let mut app = with_files(&[&long, "b"]);
    keys(&mut app, &["j"]);
    let shown = |app: &mut App| screen_of(app, 14).contains(&long);
    assert!(!shown(&mut app), "the panel cuts it");
    keys(&mut app, &["shift-k"]);
    let screen = screen_of(&mut app, 14);
    assert!(screen.contains(&long), "{screen}");
    keys(&mut app, &["j"]);
    let screen = screen_of(&mut app, 14);
    assert!(!screen.contains(&long), "it follows the cursor: {screen}");
    keys(&mut app, &["k"]);
    assert!(shown(&mut app), "and stays open");
    keys(&mut app, &["esc"]);
    assert!(!shown(&mut app), "Esc closes it");
    keys(&mut app, &["shift-k"]);
    assert!(shown(&mut app));
    keys(&mut app, &["shift-k"]);
    assert!(!shown(&mut app), "so does K again");
}

#[test]
fn k_is_for_the_panels_alone_as_the_lists_always_show_their_row_whole() {
    let mut app = with_files(&["a"]);
    keys(&mut app, &["t", "t", "esc", "shift-k"]);
    assert!(app.tab_menu.is_some(), "K closes no list");
    assert_eq!(app.whole, Whole::Cut, "nor shows the name below the panels");
    keys(&mut app, &["esc", "shift-k", "t", "t"]);
    assert_eq!(app.whole, Whole::Shown, "the panels keep it under a list");
}

#[test]
fn a_count_scrolls_the_names_that_many_columns_as_vims_zl() {
    let mut app = with_files(&[&"x".repeat(100)]);
    keys(&mut app, &["j", "3", "z", "l"]);
    assert_eq!(app.panel(Side::Left).shift(), 3);
    keys(&mut app, &["2", "z", "h"]);
    assert_eq!(app.panel(Side::Left).shift(), 1);
    keys(&mut app, &["z", "e"]);
    assert!(app.panel(Side::Left).shift() > 1);
    keys(&mut app, &["z", "s"]);
    assert_eq!(app.panel(Side::Left).shift(), 0);
    assert_eq!(
        viewer_command(Action::ScrollHalfRight),
        Some(ViewerCommand::HalfRight(1))
    );
    assert_eq!(
        viewer_command(Action::ScrollEnd),
        Some(ViewerCommand::ScrollEnd)
    );
}

#[test]
fn a_count_repeats_a_motion_and_numbers_a_row_or_a_tab() {
    let mut app = with_files(&["a", "b", "c", "d", "e"]);
    keys(&mut app, &["3", "j"]);
    assert_eq!(cursor_name(&app, Side::Left), "c");
    keys(&mut app, &["2", "k"]);
    assert_eq!(cursor_name(&app, Side::Left), "a");
    keys(&mut app, &["5", "shift-g"]);
    assert_eq!(cursor_name(&app, Side::Left), "d", "row 5, after ..");
    keys(&mut app, &["2", "g", "g"]);
    assert_eq!(cursor_name(&app, Side::Left), "a");
    keys(&mut app, &["2", "space"]);
    assert_eq!(chosen_names(&app, Side::Left), ["a", "b"]);
    // Marks stop at the end of the listing: the last row turns once.
    keys(&mut app, &["shift-g", "k", "4", "space"]);
    assert_eq!(chosen_names(&app, Side::Left), ["a", "b", "d", "e"]);
    keys(&mut app, &["9", "shift-up"]);
    assert_eq!(chosen_names(&app, Side::Left), ["c"]);
    keys(&mut app, &["t", "n", "t", "n"]);
    assert_eq!(app.tabs(Side::Left).index(), 2);
    keys(&mut app, &["1", "g", "t"]);
    assert_eq!(app.tabs(Side::Left).index(), 0, "the first tab");
    keys(&mut app, &["2", "g", "shift-t"]);
    assert_eq!(app.tabs(Side::Left).index(), 1, "two back, round");
}

#[test]
fn ctrl_o_and_tab_go_back_and_forth_through_the_jumps() {
    let mut app = loaded();
    let effects = keys(&mut app, &["j", "l"]);
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![file("x", 1), file("y", 1)]),
    );
    assert_eq!(app.panel(Side::Left).location(), &local("/srv/left"));
    keys(&mut app, &["shift-g"]);
    assert_eq!(cursor_name(&app, Side::Left), "y");
    keys(&mut app, &["ctrl-o"]);
    assert_eq!(cursor_name(&app, Side::Left), "", "back before G, on ..");
    let effects = keys(&mut app, &["ctrl-o"]);
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("left"), dir("right")]),
    );
    assert_eq!(app.panel(Side::Left).location(), &local("/srv"));
    assert_eq!(cursor_name(&app, Side::Left), "left", "where it was");
    let effects = keys(&mut app, &["tab"]);
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![file("x", 1), file("y", 1)]),
    );
    assert_eq!(app.panel(Side::Left).location(), &local("/srv/left"));
    keys(&mut app, &["tab"]);
    assert_eq!(cursor_name(&app, Side::Left), "y", "forward to after G");
    assert!(keys(&mut app, &["tab"]).is_empty(), "nothing further");
    // The panels switch with Ctrl+w, as vim's windows do.
    keys(&mut app, &["ctrl-w", "w"]);
    assert_eq!(app.active, Side::Right);
}

#[test]
fn fields_open_in_insert_mode_and_take_vims_normal_mode_after_esc() {
    let mut app = with_files(&["report.pdf"]);
    keys(&mut app, &["j", "r"]);
    assert_eq!(app.place(), (Mode::Insert, Context::Rename));
    assert_eq!(mode_of(&mut app), "INSERT · Rename");
    // In Insert mode j is text; Esc goes to Normal mode, where it is not.
    keys(&mut app, &["esc", "0", "c", "w", "n", "e", "w", "esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Rename));
    let text = |app: &mut App| {
        let panel = app.panel_mut(Side::Left);
        panel.rename_field().map(|field| field.text().to_owned())
    };
    assert_eq!(text(&mut app).as_deref(), Some("new.pdf"));
    // A count and an operator wait on the line above the F-key bar.
    keys(&mut app, &["2", "d"]);
    let shown = screen(&mut app);
    assert!(
        line_with(&shown, "2d").ends_with(" 2d\""),
        "on the right: {shown}"
    );
    keys(&mut app, &["esc"]);
    assert_eq!(app.context(), Context::Rename, "Esc gave the operator up");
    keys(&mut app, &["u"]);
    assert_eq!(text(&mut app).as_deref(), Some("report.pdf"), "undone");
    keys(&mut app, &["\"", "a", "y", "y", "esc"]);
    assert_eq!(
        app.context(),
        Context::Panel,
        "Esc in Normal mode leaves it"
    );
    // Registers go from field to field: here, the command line.
    keys(&mut app, &[":", "!", "ctrl-r", "a"]);
    assert_eq!(
        app.command_line.as_ref().map(CommandLine::text),
        Some("!report.pdf")
    );
    // Alt and a key in Insert mode are Esc and the key.
    keys(&mut app, &["alt-k"]);
    assert!(app.command_line.is_none(), "Esc closes vim's command line");
    assert_eq!(
        cursor_name(&app, Side::Left),
        "",
        "and k goes up from report.pdf"
    );
}

#[test]
fn a_command_the_keymap_gives_up_gives_up_the_operator_of_the_field() {
    let mut app = with_files(&["abc def"]);
    let text = |app: &mut App| {
        let panel = app.panel_mut(Side::Left);
        panel.rename_field().map(|field| field.text().to_owned())
    };
    keys(&mut app, &["j", "r", "esc", "0"]);
    for given_up in [&["d", "f", "esc"][..], &["d", "2", "esc"], &["d", "z"]] {
        keys(&mut app, given_up);
        keys(&mut app, &["w"]);
        assert_eq!(text(&mut app).as_deref(), Some("abc def"), "{given_up:?}");
    }
    keys(&mut app, &["0", "c", "w", "x", "ctrl-o", "esc"]);
    assert_eq!(
        app.place(),
        (Mode::Insert, Context::Rename),
        "Esc after Ctrl+o goes back to Insert mode"
    );
    assert_eq!(text(&mut app).as_deref(), Some("x def"));
}

#[test]
fn alt_and_a_key_do_nothing_on_a_password() {
    let mut app = loaded();
    let (password, _answer) = ask(1, PromptKind::Secret, "deploy@web's password: ");
    app.ask(password);
    assert!(keys(&mut app, &["alt-:", "alt-x"]).is_empty());
    assert_eq!(app.context(), Context::DialogInput, "still asking");
    assert!(app.command_line.is_none());
}

#[test]
fn visual_mode_keeps_its_rows_and_stays_with_its_panel() {
    let mut app = with_files(&["a", "b", "c", "d", "e"]);
    keys(&mut app, &["3", "j", "v", "j"]);
    assert_eq!(cursor_name(&app, Side::Left), "d");
    // A listing of the same directory with an entry more keeps the same entries selected.
    let request = app.panel_mut(Side::Left).handle(Action::Reload).unwrap();
    let entries = ["a", "aa", "b", "c", "d", "e"].map(|name| file(name, 1));
    app.listed(
        app.shown(Side::Left),
        request.generation,
        Ok(Listed {
            location: local("/srv"),
            listing: Listing::Dir(entries.to_vec()),
            space: None,
        }),
    );
    keys(&mut app, &["space"]);
    assert_eq!(chosen_names(&app, Side::Left), ["c", "d"]);
    // A count does not mark more once Space ends Visual mode.
    keys(&mut app, &["g", "g", "v", "j", "3", "space"]);
    assert_eq!(chosen_names(&app, Side::Left), ["a", "c", "d"]);
    // Another panel takes the keys: this one keeps Visual mode, and has it again with them.
    keys(&mut app, &["v", "j", "ctrl-w", "w"]);
    assert_eq!(
        app.place(),
        (Mode::Normal, Context::Panel),
        "the other's mode"
    );
    assert_eq!(app.panel(Side::Left).visual_size(), Some(2));
    keys(&mut app, &["ctrl-w", "w"]);
    assert_eq!(app.place(), (Mode::Visual, Context::Panel));
    screen_of(&mut app, 12);
    press(&mut app, Press::Click, 100, 4);
    assert_eq!(app.active, Side::Right);
    assert!(app.panel(Side::Left).visual(), "after a click too");
    keys(&mut app, &["ctrl-w", "w", "esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::Panel));
    // An F-key that does not act on the selection ends Visual mode and does what it does.
    keys(&mut app, &["v", "f7"]);
    assert_eq!(app.context(), Context::PathInput, "F7 asks for a name");
    assert!(!app.panel(Side::Left).visual());
}

#[test]
fn a_failed_listing_is_no_jump_and_n_jumps_once() {
    let mut app = with_files(&["b1", "b2", "b3", "b4", "b5"]);
    // A listing that fails leaves the panel where it was, and no jump.
    keys(&mut app, &["shift-g"]);
    let effects = keys(&mut app, &["g", "g", "l"]);
    fail(&mut app, effects, ListErrorKind::Denied);
    keys(&mut app, &["esc", "ctrl-o"]);
    assert_eq!(
        cursor_name(&app, Side::Left),
        "b5",
        "back where G was, at once"
    );
    // 3n moves three matches on, and goes to the jump list once.
    keys(&mut app, &["g", "g", "/", "b", "enter", "3", "n"]);
    assert_eq!(cursor_name(&app, Side::Left), "b4");
    keys(&mut app, &["ctrl-o"]);
    assert_eq!(cursor_name(&app, Side::Left), "b1");
    // A jump that goes nowhere is one too: Ctrl+o comes back to it first, as in vim.
    keys(&mut app, &["shift-g", "shift-g", "ctrl-o"]);
    assert_eq!(cursor_name(&app, Side::Left), "b5");
    keys(&mut app, &["ctrl-o"]);
    assert_eq!(cursor_name(&app, Side::Left), "b1");
    keys(&mut app, &["tab"]);
    assert_eq!(cursor_name(&app, Side::Left), "b5");
}

#[test]
fn ctrl_l_gives_up_an_operator_and_is_the_one_command_after_ctrl_o() {
    let mut app = with_files(&["report.pdf"]);
    let text = |app: &mut App| {
        let panel = app.panel_mut(Side::Left);
        panel.rename_field().map(|field| field.text().to_owned())
    };
    keys(&mut app, &["j", "r", "esc", "0", "d", "ctrl-l", "w"]);
    assert_eq!(
        text(&mut app).as_deref(),
        Some("report.pdf"),
        "w only moved"
    );
    keys(&mut app, &["shift-a", "ctrl-o"]);
    assert_eq!(mode_of(&mut app), "(insert) · Rename");
    keys(&mut app, &["ctrl-l"]);
    assert_eq!(app.place(), (Mode::Insert, Context::Rename));
    assert_eq!(mode_of(&mut app), "INSERT · Rename");
}

#[test]
fn counts_move_past_the_fields_of_dialogs_and_the_configuration() {
    let mut once = with_files(&["a"]);
    keys(&mut once, &["j", "f", "#", "esc", "g", "g", "9", "j"]);
    let mut each = with_files(&["a"]);
    keys(&mut each, &["j", "f", "#", "esc", "g", "g"]);
    for _ in 0..9 {
        keys(&mut each, &["j"]);
    }
    assert_eq!(once.place(), each.place());
    assert_eq!(screen_of(&mut once, 30), screen_of(&mut each, 30));
    let mut once = loaded();
    open_configuration(&mut once);
    keys(&mut once, &["esc", "3", "j"]);
    let mut each = loaded();
    open_configuration(&mut each);
    keys(&mut each, &["esc", "j", "j", "j"]);
    assert_eq!(
        screen_of(&mut once, 30),
        screen_of(&mut each, 30),
        "from a text field"
    );
}

#[test]
fn q_colon_from_a_panel_closes_with_the_window_of_the_history() {
    let mut app = loaded();
    keys(&mut app, &["q", ":"]);
    assert_eq!(app.place(), (Mode::Insert, Context::CommandHistory));
    keys(&mut app, &["esc", "q"]);
    assert!(
        app.command_line.is_none(),
        "back to the panel, as from vim's :q"
    );
    assert_eq!(app.place().1, Context::Panel);
    // From the command line, it goes back there.
    keys(&mut app, &["!", "ctrl-f", "q", ":"]);
    keys(&mut app, &["esc", "q"]);
    assert_eq!(app.place().1, Context::CommandLine);
}

#[test]
fn a_count_past_the_jump_list_goes_nowhere() {
    let mut app = with_files(&["f1", "f2", "f3", "f4", "f5", "f6"]);
    keys(&mut app, &["2", "shift-g", "4", "shift-g", "6", "shift-g"]);
    assert_eq!(cursor_name(&app, Side::Left), "f5");
    keys(&mut app, &["9", "9", "ctrl-o"]);
    assert_eq!(cursor_name(&app, Side::Left), "f5", "as vim's");
    keys(&mut app, &["2", "ctrl-o"]);
    assert_eq!(cursor_name(&app, Side::Left), "f1");
    keys(&mut app, &["9", "9", "tab"]);
    assert_eq!(cursor_name(&app, Side::Left), "f1");
    keys(&mut app, &["tab"]);
    assert_eq!(cursor_name(&app, Side::Left), "f3");
}

#[test]
fn v_and_shift_v_switch_the_kind_of_visual_mode_and_end_it_as_vims() {
    let mut app = with_files(&["a", "b", "c"]);
    keys(&mut app, &["j", "shift-v", "j"]);
    assert_eq!(mode_of(&mut app), "VISUAL LINE");
    keys(&mut app, &["v"]);
    assert_eq!(mode_of(&mut app), "VISUAL", "the other key switches");
    assert_eq!(
        app.panel(Side::Left).visual_size(),
        Some(2),
        "the rows stay"
    );
    keys(&mut app, &["v"]);
    assert_eq!(app.mode(), Mode::Normal, "its own key ends it");
    keys(&mut app, &["g", "v"]);
    assert_eq!(mode_of(&mut app), "VISUAL", "g v takes the kind back");
    // After a command of files, the cursor is on the first row selected, as in vim.
    keys(&mut app, &["f", "#"]);
    assert_eq!(cursor_name(&app, Side::Left), "a");
}

#[test]
fn counts_select_rows_and_remove_them_from_lists_as_vims_lines() {
    let mut app = with_files(&["a", "b", "c", "d"]);
    keys(&mut app, &["j", "3", "shift-v"]);
    let line = mode_line(&mut app, 160);
    assert!(
        line.starts_with("VISUAL LINE") && line.ends_with(" 3"),
        "{line}"
    );
    keys(&mut app, &["esc", "t", "n", "t", "n", "t", "n"]);
    assert_eq!(app.tabs(Side::Left).len(), 4);
    keys(&mut app, &["t", "t", "esc", "g", "g", "3", "d", "d"]);
    assert_eq!(app.tabs(Side::Left).len(), 1);
}

#[test]
fn quick_search_goes_from_the_row_after_the_cursor_and_to_the_match_of_its_count() {
    let mut app = with_files(&["a1", "a2", "a3", "b"]);
    keys(&mut app, &["j", "/", "a", "enter"]);
    assert_eq!(
        cursor_name(&app, Side::Left),
        "a2",
        "as vim's, not the row it is on"
    );
    keys(&mut app, &["g", "g", "2", "/", "a", "ctrl-j"]);
    assert_eq!(
        cursor_name(&app, Side::Left),
        "a2",
        "the second, and Ctrl+j is Enter"
    );
    assert!(!app.panel(Side::Left).searching());
}

#[test]
fn n_says_when_there_was_no_search_and_when_it_goes_round() {
    let mut app = with_files(&["a1", "b2"]);
    keys(&mut app, &["n"]);
    assert!(screen(&mut app).contains("No previous search"));
    keys(&mut app, &["j"]);
    assert!(
        !screen(&mut app).contains("No previous search"),
        "until the next key"
    );
    keys(&mut app, &["/", "a", "enter", "n"]);
    assert!(screen(&mut app).contains("search hit BOTTOM, continuing at TOP"));
    keys(&mut app, &["shift-n"]);
    assert!(screen(&mut app).contains("search hit TOP, continuing at BOTTOM"));
    // So does Enter in quick search, typed as vim's search, on a match before where it
    // started; fuzzy search ranks the rows instead.
    let mut app = with_files(&["a1", "b2", "c3"]);
    app.config.ui.default_search_mode = SearchMode::Plain;
    keys(&mut app, &["shift-g", "/", "b", "enter"]);
    assert_eq!(cursor_name(&app, Side::Left), "b2");
    assert!(screen(&mut app).contains("search hit BOTTOM, continuing at TOP"));
    keys(&mut app, &["g", "g", "/", "b", "enter"]);
    assert!(!screen(&mut app).contains("search hit"), "after it");
}

#[test]
fn question_mark_searches_up_and_n_goes_the_same_way_as_vims() {
    let mut app = with_files(&["a1", "b2", "a3", "b4"]);
    app.config.ui.default_search_mode = SearchMode::Plain;
    keys(&mut app, &["shift-g", "?"]);
    assert!(screen(&mut app).contains(&prompt("Search up", "")));
    keys(&mut app, &["a", "enter"]);
    assert_eq!(cursor_name(&app, Side::Left), "a3");
    keys(&mut app, &["n"]);
    assert_eq!(cursor_name(&app, Side::Left), "a1");
    keys(&mut app, &["n"]);
    assert_eq!(cursor_name(&app, Side::Left), "a3");
    assert!(screen(&mut app).contains("search hit TOP, continuing at BOTTOM"));
    keys(&mut app, &["shift-n"]);
    assert_eq!(cursor_name(&app, Side::Left), "a1");
    assert!(screen(&mut app).contains("search hit BOTTOM, continuing at TOP"));
    // Enter says so too, on a match not before where it started; a count goes on.
    keys(&mut app, &["g", "g", "?", "b", "enter"]);
    assert_eq!(cursor_name(&app, Side::Left), "b4");
    assert!(screen(&mut app).contains("search hit TOP, continuing at BOTTOM"));
    keys(&mut app, &["2", "?", "a", "enter"]);
    assert_eq!(cursor_name(&app, Side::Left), "a1");
}

#[test]
fn backspace_and_delete_with_nothing_typed_keep_quick_search_open() {
    let mut app = with_files(&["a1", "b2"]);
    app.config.ui.default_search_mode = SearchMode::Plain;
    let location = app.panel(Side::Left).location().clone();
    for key in ["backspace", "delete"] {
        keys(&mut app, &["g", "g", "j", "/", "b"]);
        assert_eq!(cursor_name(&app, Side::Left), "b2");
        keys(&mut app, &["backspace", key, key, key]);
        assert!(app.panel(Side::Left).searching(), "{key}");
        assert_eq!(app.panel(Side::Left).search_text(), Some(""));
        assert_eq!(cursor_name(&app, Side::Left), "a1", "back where it started");
        assert_eq!(
            app.panel(Side::Left).location(),
            &location,
            "not the parent"
        );
        keys(&mut app, &["esc"]);
        assert!(!app.panel(Side::Left).searching());
    }
}

#[test]
fn a_count_before_v_selects_the_last_selection_that_many_times_and_ctrl_j_goes_down() {
    let mut app = with_files(&["a", "b", "c", "d", "e", "f", "g"]);
    keys(&mut app, &["ctrl-j", "3", "v"]);
    assert_eq!(app.panel(Side::Left).visual_size(), Some(3));
    keys(
        &mut app,
        &["esc", "g", "g", "j", "v", "j", "space", "2", "v"],
    );
    assert_eq!(
        app.panel(Side::Left).visual_size(),
        Some(4),
        "two rows marked, twice"
    );
    keys(&mut app, &["esc", "v"]);
    assert_eq!(
        app.panel(Side::Left).visual_size(),
        Some(1),
        "without a count"
    );
}

#[test]
fn a_quote_after_an_operator_gives_it_up_and_the_next_key_is_a_command() {
    let mut app = loaded();
    keys(
        &mut app,
        &[":", "a", "b", "c", "ctrl-f", "0", "d", "\"", "a", "x"],
    );
    let line = app.command_line.as_ref().expect("the command line");
    assert_eq!((line.text(), line.mode()), ("axbc", Mode::Insert));
}

#[test]
fn visual_mode_counts_the_entries_and_d_and_x_delete_them() {
    let mut app = with_files(&["a", "b"]);
    keys(&mut app, &["v", "j"]);
    let line = mode_line(&mut app, 160);
    assert!(
        line.starts_with("VISUAL") && line.ends_with(" 1"),
        "not ..: {line}"
    );
    keys(&mut app, &["shift-d"]);
    assert_eq!(app.dialogs.len(), 1, "the question of the delete");
}

#[test]
fn visual_mode_shows_how_much_it_selects() {
    let mut app = with_files(&["abc", "bcd", "cde"]);
    keys(&mut app, &["j", "v", "j"]);
    let line = mode_line(&mut app, 160);
    assert!(
        line.starts_with("VISUAL ") && line.ends_with(" 2"),
        "{line}"
    );
    keys(&mut app, &["esc", "r", "esc", "0", "v", "l"]);
    let line = mode_line(&mut app, 160);
    assert!(
        line.starts_with("VISUAL · Rename") && line.ends_with(" 2"),
        "{line}"
    );
}

#[test]
fn pastes_go_to_the_field_as_one_change_and_to_a_list_filter() {
    let mut app = with_files(&["report.pdf"]);
    keys(&mut app, &["j", "r", "esc"]);
    app.paste("new");
    keys(&mut app, &["u"]);
    let text = app
        .panel_mut(Side::Left)
        .rename_field()
        .map(|field| field.text().to_owned());
    assert_eq!(text.as_deref(), Some("report.pdf"), "undone at once");
    keys(&mut app, &["\"", "a", "y", "y", "esc", "t", "n", "t", "t"]);
    app.paste("srv");
    assert_eq!(app.place(), (Mode::Insert, Context::TabMenu));
    assert!(screen_of(&mut app, 16).contains(&prompt("Filter", "srv")));
    // Ctrl+r types a register there, as in a text field.
    keys(&mut app, &["ctrl-u", "ctrl-r", "a"]);
    assert!(screen_of(&mut app, 16).contains(&prompt("Filter", "report.pdf")));
}

#[test]
fn lists_open_in_insert_mode_with_the_filter_taking_the_keys() {
    let mut app = loaded();
    keys(&mut app, &["t", "n", "t", "t"]);
    assert_eq!(app.place(), (Mode::Insert, Context::TabMenu));
    keys(&mut app, &["x"]);
    assert!(screen_of(&mut app, 16).contains(&prompt("Filter", "x")));
    keys(&mut app, &["backspace", "esc"]);
    assert_eq!(app.place(), (Mode::Normal, Context::TabMenu));
    keys(&mut app, &["/"]);
    assert_eq!(app.place(), (Mode::Insert, Context::TabMenu));
    keys(&mut app, &["esc", "i"]);
    assert_eq!(
        app.place(),
        (Mode::Normal, Context::TabMenu),
        "i is not /, as it opens the command line in the panels"
    );
    keys(&mut app, &["g", "g", "d", "d"]);
    assert_eq!(app.tabs(Side::Left).len(), 1, "d d closes the tab");
    keys(&mut app, &["q"]);
    assert_eq!(app.context(), Context::Panel);
    for opens in [&["g", "l"][..], &["t", "w"], &["q", ":"], &["g", "z"]] {
        let mut app = loaded();
        keys(&mut app, opens);
        assert_eq!(app.mode(), Mode::Insert, "after {opens:?}");
    }
}

/// Where `glyph` ends a field on the line of `screen` that holds `text`.
fn glyph_cell(screen: &str, text: &str, glyph: &str) -> Position {
    let (y, line) = (screen.lines().enumerate())
        .find(|(_, line)| line.contains(text))
        .unwrap_or_else(|| panic!("no {text:?} in {screen}"));
    let start = line.find(text).unwrap_or(0) + text.len();
    let end = line[start..]
        .find('║')
        .map_or(line.len(), |end| start + end);
    let at = start
        + line[start..end]
            .rfind(&format!(" {glyph} "))
            .unwrap_or_else(|| panic!("no {glyph:?} at the end of the field in {line}"));
    // The test backend quotes each line, and a blank comes before the glyph.
    let x = line[..at].chars().count();
    Position::new(u16::try_from(x).unwrap(), u16::try_from(y).unwrap())
}

#[test]
fn ctrl_s_and_a_click_on_its_glyph_switch_how_the_field_with_the_focus_matches() {
    let mut app = loaded();
    let glyphs = app.theme.glyphs.search_modes;
    let cases = app.theme.glyphs.case_modes;
    // Quick search, whose glyphs end the status line.
    app.config.ui.default_search_mode = SearchMode::Plain;
    app.handle(action(Action::QuickSearch));
    app.handle(Resolved::Insert('r'));
    assert_eq!(
        app.key_place().search,
        Some(search::Kind::QUICK_SEARCH),
        "the keys after Ctrl+s go before its own"
    );
    let at = glyph_cell(&screen_of(&mut app, 12), "║Search: r", glyphs.plain);
    assert!(press(&mut app, Press::Click, at.x, at.y).is_empty());
    assert!(searching(&mut app, "r", glyphs.fzf), "the search goes on");
    assert!(
        screen(&mut app).contains(&format!(" {} {} ║", cases.smart, glyphs.fzf)),
        "the case before the way"
    );
    let at = glyph_cell(&screen_of(&mut app, 12), "║Search: r", cases.smart);
    press(&mut app, Press::Click, at.x, at.y);
    assert!(screen(&mut app).contains(&format!(" {} {} ║", cases.ignore, glyphs.fzf)));
    keys(&mut app, &["ctrl-s", "m"]);
    assert!(screen(&mut app).contains(&format!(" {} {} ║", cases.exact, glyphs.fzf)));
    keys(&mut app, &["ctrl-s", "r"]);
    assert!(
        searching(&mut app, "r", glyphs.regex),
        "a way by its letter"
    );
    keys(&mut app, &["esc"]);
    // A list, whose filter has the focus while it takes the keys.
    app.config.ui.default_search_mode = SearchMode::Fzf;
    keys(&mut app, &["t", "n", "t", "t"]);
    keys(&mut app, &["x"]);
    let screen = screen_of(&mut app, 16);
    let at = glyph_cell(&screen, &prompt("Filter", "x"), glyph(&app));
    keys(&mut app, &["esc"]);
    assert_eq!(app.key_place().search, None, "the rows have the focus");
    keys(&mut app, &["ctrl-s", "ctrl-s"]);
    glyph_cell(&screen_of(&mut app, 16), &prompt("Filter", "x"), glyphs.fzf);
    press(&mut app, Press::Click, at.x, at.y);
    assert_eq!(
        app.place(),
        (Mode::Insert, Context::TabMenu),
        "the filter takes the keys"
    );
    glyph_cell(
        &screen_of(&mut app, 16),
        &prompt("Filter", "x"),
        glyphs.regex,
    );
    keys(&mut app, &["ctrl-s", "ctrl-s"]);
    glyph_cell(
        &screen_of(&mut app, 16),
        &prompt("Filter", "x"),
        glyphs.plain,
    );
}

#[test]
fn each_field_that_searches_opens_as_it_was_last_until_noc_quits() {
    let mut app = loaded();
    app.config.ui.default_search_mode = SearchMode::Plain;
    let glyphs = app.theme.glyphs.search_modes;
    let cases = app.theme.glyphs.case_modes;
    let ends = |app: &mut App, case: &str, way: &str| {
        let text = screen(app);
        assert!(
            text.contains(&format!(" {case} {way} ║"))
                || text.contains(&format!(" {case} {way}  ║")),
            "{case} {way}: {text}"
        );
    };
    // g + starts in the default too, which it has.
    app.handle(action(Action::Select));
    ends(&mut app, cases.smart, glyphs.plain);
    keys(&mut app, &["ctrl-s", "g", "ctrl-s", "i", "esc", "esc"]);
    assert_eq!(app.context(), Context::Panel);
    // The tab menu, switched and given up with Esc.
    app.handle(action(Action::TabList));
    ends(&mut app, cases.smart, glyphs.plain);
    keys(&mut app, &["ctrl-s", "r", "ctrl-s", "m", "esc", "esc"]);
    assert_eq!(app.context(), Context::Panel);
    app.config.ui.default_search_mode = SearchMode::Fzf;
    on_other(&mut app, Action::TabList);
    ends(&mut app, cases.exact, glyphs.regex);
    keys(&mut app, &["esc", "esc"]);
    // Each its own: the workspaces open in the default, which changed.
    app.handle(action(Action::Workspaces));
    ends(&mut app, cases.smart, glyphs.fzf);
    keys(&mut app, &["esc", "esc"]);
    app.handle(action(Action::Select));
    ends(&mut app, cases.ignore, glyphs.glob);
}
