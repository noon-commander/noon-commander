use super::*;

fn search_files(names: &[&str]) -> Panel {
    let files: Vec<DirEntry> = names
        .iter()
        .map(|name| entry(name, FileKind::File, 1))
        .collect();
    loaded("/srv", files)
}

fn typed(panel: &mut Panel, text: &str) {
    for c in text.chars() {
        panel.search_type(c);
    }
}

#[test]
fn quick_search_searches_from_where_it_started_as_vims_incremental_search() {
    let mut panel = search_files(&["bar", "bin", "boot", "cab"]);
    panel.search_down(1, quick(Way::Plain));
    assert!(panel.searching());
    typed(&mut panel, "B");
    assert_eq!(
        panel.search_text(),
        Some(""),
        "smart case: a capital counts, and matches nothing"
    );
    panel.switch_search(Switch::Case(Case::Ignore));
    typed(&mut panel, "B");
    assert_eq!(under_cursor(&panel), "bar", "case ignored");
    typed(&mut panel, "i");
    assert_eq!(under_cursor(&panel), "bin");
    typed(&mut panel, "x");
    assert_eq!(
        panel.search_text(),
        Some("Bi"),
        "a miss is not typed, as in mc"
    );
    assert_eq!(under_cursor(&panel), "bin", "and the cursor stays");
    panel.search_edit(Action::Backspace);
    assert_eq!(
        under_cursor(&panel),
        "bar",
        "an edit searches from the start again"
    );
    typed(&mut panel, "o");
    let hosts = |_: &str| HostState::default();
    let mut terminal = render_with(&mut panel, (40, 6), true, &hosts, Decor::new(false));
    let plain = Theme::terminal().glyphs.search_modes.plain;
    let screen = terminal.backend().to_string();
    let status = screen
        .lines()
        .find(|line| line.contains("║Search: Bo"))
        .unwrap();
    assert!(
        status.contains(&format!(" {plain} ║")),
        "the glyph of how it matches at the right end: {status}"
    );
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        Position::new(11, 4),
        "after the text"
    );
    let end = panel.end_search();
    assert_eq!(
        end,
        Some(SearchEnd {
            text: "Bo".to_owned(),
            back: false,
            input: quick(Way::Plain).with_case(Case::Ignore),
            found: Some(false),
        })
    );
    assert!(!panel.searching());
    // Enter on a text that nothing matches, as the last search's may be, stays where the
    // search started.
    panel.search_up(1, quick(Way::Plain));
    panel.search_resume("z", quick(Way::Plain));
    assert_eq!(
        panel.search_text(),
        Some("z"),
        "what the last search found stays"
    );
    assert_eq!(under_cursor(&panel), "boot");
    assert_eq!(panel.end_search().map(|end| end.found), Some(None));
    assert_eq!(under_cursor(&panel), "boot");
    // Opening another directory ends a search.
    panel.search_down(1, quick(Way::Plain));
    typed(&mut panel, "b");
    panel.handle(Action::Reload);
    assert!(!panel.searching());
}

#[test]
fn quick_search_types_no_character_that_nothing_matches_but_finishes_a_regex() {
    let mut panel = search_files(&["bar", "boot"]);
    panel.search_down(1, quick(Way::Regex));
    typed(&mut panel, "(oz");
    assert_eq!(
        panel.search_text(),
        Some("(oz"),
        "not a regular expression yet"
    );
    assert_eq!(panel.cursor, 0);
    typed(&mut panel, ")");
    assert_eq!(panel.search_text(), Some("(oz"), "one that nothing matches");
    panel.search_edit(Action::Backspace);
    typed(&mut panel, "|a)");
    assert_eq!(panel.search_text(), Some("(o|a)"));
    assert_eq!(under_cursor(&panel), "bar");
    panel.abort_search();

    // A blank text searches for nothing, so it is typed.
    panel.search_down(1, quick(Way::Plain));
    typed(&mut panel, " x");
    assert_eq!(panel.search_text(), Some(" "));
    assert_eq!(panel.cursor, 0);

    // Nothing matches in a directory without names.
    let mut panel = search_files(&[]);
    panel.search_down(1, quick(Way::Fzf));
    typed(&mut panel, "a");
    assert_eq!(panel.search_text(), Some(""));
}

#[test]
fn fuzzy_quick_search_goes_to_the_best_match_and_on_to_the_next_best() {
    let mut panel = search_files(&["Cargo.toml", "config.rs", "cfg-if", "notes", "x.cfg"]);
    panel.search_down(1, quick(Way::Fzf));
    typed(&mut panel, "cfg");
    assert_eq!(under_cursor(&panel), "cfg-if", "together, at the start");
    panel.search_step(true);
    assert_eq!(under_cursor(&panel), "x.cfg", "together");
    panel.search_step(true);
    assert_eq!(under_cursor(&panel), "config.rs", "apart");
    panel.search_step(true);
    assert_eq!(under_cursor(&panel), "cfg-if", "round to the best");
    typed(&mut panel, "q");
    assert_eq!(panel.search_text(), Some("cfg"), "a miss is not typed");
    assert_eq!(under_cursor(&panel), "cfg-if", "and the cursor stays");
    panel.search_edit(Action::DeleteToStart);
    assert_eq!(panel.search_text(), Some(""));
    typed(&mut panel, "C");
    assert_eq!(
        under_cursor(&panel),
        "Cargo.toml",
        "case counts with a capital"
    );
    panel.end_search();

    let mut panel = search_files(&["bar", "bat", "baz"]);
    panel.handle(Action::End);
    panel.search_down(1, quick(Way::Fzf));
    typed(&mut panel, "b");
    assert_eq!(
        under_cursor(&panel),
        "bar",
        "as good: the first after the cursor, round, as vim's search goes"
    );
    typed(&mut panel, " ");
    assert_eq!(under_cursor(&panel), "bar", "a space stays");
}

#[test]
fn matches_go_both_ways_round_and_an_ended_search_says_what_it_found() {
    let mut panel = search_files(&["bar", "bin", "cat", "boot"]);
    panel.search_down(1, quick(Way::Plain));
    typed(&mut panel, "b");
    assert_eq!(under_cursor(&panel), "bar");
    panel.search_step(false);
    assert_eq!(under_cursor(&panel), "boot", "up, round to the bottom");
    panel.search_step(true);
    assert_eq!(under_cursor(&panel), "bar", "down, round to the top");
    let end = panel.end_search().expect("a search");
    assert_eq!(
        (end.text.as_str(), end.found),
        ("b", Some(false)),
        "after Ctrl+g, Enter stays on the match without going round, as vim's"
    );
    assert_eq!(
        panel.search_again(("x", quick(Way::Plain)), true, 1),
        None,
        "no match"
    );
    assert_eq!(under_cursor(&panel), "bar", "the cursor stays");
    assert_eq!(
        panel.search_again(("b", quick(Way::Plain)), false, 1),
        Some(true)
    );
    assert_eq!(under_cursor(&panel), "boot", "round");

    // Fuzzy: the previous one as fzf ranks them, round to the worst.
    panel.search_down(1, quick(Way::Plain));
    panel.search_resume("bt", quick(Way::Fzf));
    assert_eq!(panel.search_text(), Some("bt"));
    let best = under_cursor(&panel);
    panel.search_step(false);
    panel.search_step(true);
    assert_eq!(under_cursor(&panel), best, "back and forth");
    panel.search_edit(Action::Backspace);
    panel.search_edit(Action::Backspace);
    assert_eq!(panel.end_search(), None, "nothing typed, nothing kept");
}

#[test]
fn a_count_before_the_search_and_ctrl_g_go_that_many_matches_on() {
    let mut panel = search_files(&["a1", "a2", "a3", "a4"]);
    panel.search_down(2, quick(Way::Plain));
    typed(&mut panel, "a");
    assert_eq!(under_cursor(&panel), "a2");
    panel.search_step(true);
    assert_eq!(under_cursor(&panel), "a4");
    panel.search_step(true);
    assert_eq!(under_cursor(&panel), "a2", "round");
    // Enter searches once more from the match before, with the count, as vim's does.
    assert_eq!(panel.end_search().map(|end| end.found), Some(Some(false)));
    assert_eq!(under_cursor(&panel), "a3");
    // A count that goes round the end says so.
    panel.handle(Action::Home);
    panel.search_down(5, quick(Way::Plain));
    typed(&mut panel, "a");
    assert_eq!(under_cursor(&panel), "a1");
    assert_eq!(panel.end_search().map(|end| end.found), Some(Some(true)));
}

#[test]
fn an_aborted_quick_search_puts_back_the_cursor_and_the_rows_on_screen() {
    let names: Vec<String> = (0..60).map(|n| format!("f{n:02}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut panel = search_files(&names);
    panel.page = 10;
    for _ in 0..20 {
        panel.handle(Action::Down);
    }
    panel.scroll(10);
    let (cursor, top) = (panel.cursor, panel.window.top());
    panel.search_down(1, quick(Way::Plain));
    typed(&mut panel, "f5");
    panel.scroll(10);
    assert_eq!(under_cursor(&panel), "f50");
    assert_ne!(panel.window.top(), top, "the rows scrolled to the match");
    panel.abort_search();
    panel.scroll(10);
    assert!(!panel.searching());
    assert_eq!((panel.cursor, panel.window.top()), (cursor, top));
}

#[test]
fn literal_quick_search_finds_the_text_anywhere_in_a_name_as_the_filters_do() {
    let mut panel = search_files(&["bar", "cab", "Tabs"]);
    panel.search_down(1, quick(Way::Plain));
    typed(&mut panel, "ab");
    assert_eq!(under_cursor(&panel), "cab");
    panel.search_step(true);
    assert_eq!(
        under_cursor(&panel),
        "Tabs",
        "without a capital, either case"
    );
    panel.end_search();
    assert_eq!(
        panel.search_again(("ab", quick(Way::Plain)), true, 1),
        Some(true)
    );
    assert_eq!(under_cursor(&panel), "cab", "round");
}

/// What each row of `panel`, active, drawn `width` cells wide in mc's colors, shows in the
/// colors of what quick search found, for the rows that show some.
fn found(panel: &mut Panel, width: u16) -> Vec<String> {
    let theme = Theme::mc_classic();
    let mut terminal = Terminal::new(TestBackend::new(width, 10)).unwrap();
    let hosts = |_: &str| HostState::default();
    let view = View {
        hosts: &hosts,
        decor: Decor::new(false),
        theme: &theme,
        spin: Spin::new(Instant::now(), Instant::now(), false, theme.glyphs.spinners),
        tz: &TimeZone::UTC,
        formats: &Formats::iso(),
    };
    terminal
        .draw(|frame| panel.render(frame, frame.area(), true, &view))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..10)
        .map(|y| {
            (0..width)
                .filter(|&x| buffer[(x, y)].bg == Color::Green)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .filter(|row| !row.is_empty())
        .collect()
}

#[test]
fn quick_search_shows_where_each_name_on_screen_holds_the_text() {
    let mut panel = search_files(&["abab", "cab", "Tabs", "note"]);
    panel.search_down(1, quick(Way::Plain));
    assert!(found(&mut panel, 60).is_empty(), "nothing typed");
    panel.switch_search(Switch::Case(Case::Ignore));
    typed(&mut panel, "AB");
    // On the cursor's row too, which is cab's, and ignoring case.
    assert_eq!(found(&mut panel, 60), ["abab", "ab", "ab"]);
    panel.end_search();
    assert!(found(&mut panel, 60).is_empty(), "only while it runs");
}

#[test]
fn fuzzy_quick_search_shows_the_characters_it_matched() {
    let mut panel = search_files(&["abab", "cab", "config.rs"]);
    panel.search_down(1, quick(Way::Fzf));
    typed(&mut panel, "cb");
    assert_eq!(found(&mut panel, 60), ["cb"]);
    let mut panel = search_files(&["config.rs"]);
    panel.search_down(1, quick(Way::Fzf));
    typed(&mut panel, "cfg");
    assert_eq!(found(&mut panel, 60), ["cfg"]);
}

#[test]
fn a_name_cut_short_shows_what_it_found_where_it_shows() {
    let name = format!("ab{}ab.md", "x".repeat(60));
    let mut panel = search_files(&[&name]);
    panel.search_down(1, quick(Way::Plain));
    typed(&mut panel, "ab");
    // The start and the end of the name show, not what it found in the middle.
    assert_eq!(found(&mut panel, 47), ["abab"]);
    typed(&mut panel, ".");
    assert_eq!(found(&mut panel, 47), ["ab."]);
    // Scrolled past the x, the mark over the a: the b after it shows what it found.
    let mut panel = search_files(&[&format!("x{name}")]);
    panel.shift_names(1);
    panel.search_down(1, quick(Way::Plain));
    typed(&mut panel, "ab");
    assert_eq!(found(&mut panel, 47), ["b"]);
}

#[test]
fn quick_search_edits_its_text_as_a_text_field_does() {
    let mut panel = search_files(&["bar", "bat", "boot", "cab"]);
    panel.search_down(1, quick(Way::Plain));
    typed(&mut panel, "bo");
    assert_eq!(under_cursor(&panel), "boot");
    // Typed in the middle, a character searches again, and one that nothing matches is not
    // typed there either.
    panel.search_edit(Action::TextStart);
    panel.search_edit(Action::Right);
    assert_eq!(under_cursor(&panel), "boot", "a move of the text cursor");
    typed(&mut panel, "a");
    assert_eq!(panel.search_text(), Some("bo"), "no name holds bao");
    typed(&mut panel, "o");
    assert_eq!(panel.search_text(), Some("boo"));
    panel.search_edit(Action::Backspace);
    assert_eq!(panel.search_text(), Some("bo"));
    panel.search_edit(Action::DeleteChar);
    assert_eq!(panel.search_text(), Some("b"));
    assert_eq!(under_cursor(&panel), "bar", "an edit searches again");
    panel.search_edit(Action::TextEnd);
    typed(&mut panel, "at");
    assert_eq!(under_cursor(&panel), "bat");
    panel.search_edit(Action::TextStart);
    panel.search_edit(Action::DeleteToEnd);
    assert_eq!(panel.search_text(), Some(""));
    assert_eq!(panel.cursor, 0, "nothing typed: where it started");
    let hosts = |_: &str| HostState::default();
    typed(&mut panel, "c");
    panel.search_edit(Action::TextStart);
    let mut terminal = render_with(&mut panel, (40, 6), true, &hosts, Decor::new(false));
    assert!(terminal.backend().to_string().contains("║Search: c"));
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        Position::new(9, 4),
        "on the text cursor"
    );
}
