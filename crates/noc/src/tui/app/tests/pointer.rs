use super::*;

#[test]
fn a_click_moves_the_cursor_and_a_double_click_opens() {
    let mut app = loaded();
    screen_of(&mut app, 12);
    assert!(press(&mut app, Press::Click, 90, 3).is_empty());
    assert_eq!(app.active, Side::Right, "the clicked panel gets the keys");
    assert_eq!(cursor_name(&app, Side::Right), "left");
    let Effect::List { panel, request, .. } = one(press(&mut app, Press::DoubleClick, 90, 3))
    else {
        panic!("expected a listing");
    };
    assert_eq!(
        (panel.side, request.location),
        (Side::Right, local("/srv/left"))
    );
    // Until the directory comes, its rows take no clicks.
    press(&mut app, Press::Click, 90, 4);
    assert_eq!(app.panel(Side::Right).name_under_cursor(), None);
    app.panel_mut(Side::Right).cancel();
    assert_eq!(cursor_name(&app, Side::Right), "left");

    // Below the last row the panel only gets the keys.
    app.active = Side::Left;
    press(&mut app, Press::Click, 90, 8);
    assert_eq!(app.active, Side::Right);
    assert_eq!(cursor_name(&app, Side::Right), "left");
    assert!(press(&mut app, Press::DoubleClick, 90, 8).is_empty());
}

#[test]
fn a_right_click_marks_the_row_and_leaves_the_cursor_on_it() {
    let mut app = loaded();
    screen_of(&mut app, 12);
    press(&mut app, Press::RightClick, 10, 4);
    press(&mut app, Press::RightClick, 10, 3);
    assert_eq!(cursor_name(&app, Side::Left), "left");
    assert_eq!(chosen_names(&app, Side::Left), ["left", "right"]);
    press(&mut app, Press::RightClick, 10, 3);
    assert_eq!(cursor_name(&app, Side::Left), "left");
    assert_eq!(chosen_names(&app, Side::Left), ["right"]);
    press(&mut app, Press::RightClick, 10, 2);
    assert_eq!(
        chosen_names(&app, Side::Left),
        ["right"],
        "`..` takes no mark"
    );
}

#[test]
fn the_wheel_scrolls_the_panel_under_it() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let files = (0..20).map(|n| file(&format!("f{n:02}"), 1)).collect();
    answer(&mut app, effects, &Listing::Dir(files));
    screen_of(&mut app, 13);
    press(&mut app, Press::WheelDown, 90, 5);
    assert_eq!(app.active, Side::Left, "the wheel does not take the keys");
    let names = |app: &mut App| {
        screen_of(app, 13);
        [Side::Left, Side::Right].map(|side| cursor_name(app, side))
    };
    // As vim's Ctrl+e and Ctrl+y: the cursor moves only to keep what of 'scrolloff' fits
    // around it in six rows.
    assert_eq!(names(&mut app), ["", "f04"]);
    press(&mut app, Press::WheelUp, 90, 5);
    assert_eq!(names(&mut app)[1], "f01");
    for _ in 0..10 {
        press(&mut app, Press::WheelDown, 90, 5);
        names(&mut app);
    }
    assert_eq!(
        names(&mut app)[1],
        "f19",
        "never past the last row; then the cursor moves to it"
    );

    app.config.ui.wheel = Wheel::Page;
    press(&mut app, Press::WheelUp, 90, 5);
    assert_eq!(names(&mut app)[1], "f10", "a page up");
}

#[test]
fn the_wheel_moves_the_cursor_where_the_rows_all_fit() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let files = (0..5).map(|n| file(&format!("f{n:02}"), 1)).collect();
    answer(&mut app, effects, &Listing::Dir(files));
    let name = |app: &mut App| {
        screen_of(app, 13);
        cursor_name(app, Side::Right)
    };
    name(&mut app);
    press(&mut app, Press::WheelDown, 90, 5);
    assert_eq!(name(&mut app), "f02", "three rows down from ..");
    press(&mut app, Press::WheelDown, 90, 5);
    assert_eq!(name(&mut app), "f04", "no further than the last row");
    press(&mut app, Press::WheelUp, 90, 5);
    assert_eq!(name(&mut app), "f01");
    assert_eq!(app.active, Side::Left, "the wheel does not take the keys");
}

#[test]
fn the_f_key_bar_presses_its_keys_where_they_do_something() {
    let mut app = loaded();
    screen_of(&mut app, 12);
    press(&mut app, Press::DoubleClick, 100, 11);
    assert!(app.dialogs.is_empty(), "a double click is no second press");
    press(&mut app, Press::Click, 100, 11);
    assert_eq!(app.context(), Context::PathInput, "F7 asks for a name");
    press(&mut app, Press::Click, 90, 3);
    assert_eq!(
        app.active,
        Side::Left,
        "a dialog keeps the panels from the mouse"
    );
    screen_of(&mut app, 12);
    // F10 cancels in a dialog.
    press(&mut app, Press::Click, 150, 11);
    assert!(app.dialogs.is_empty());

    app.config.ui.mouse = false;
    press(&mut app, Press::Click, 100, 11);
    press(&mut app, Press::Click, 90, 3);
    assert!(app.dialogs.is_empty());
    assert_eq!(app.active, Side::Left, "the mouse is off");
}

#[test]
fn a_click_on_a_tab_shows_it() {
    for tab_bar in [TabBar::Line, TabBar::Frame] {
        let mut app = loaded();
        app.config.ui.tab_bar = tab_bar;
        app.handle(action(Action::NewTab));
        app.handle(action(Action::SwitchPanel));
        assert_eq!(app.tabs(Side::Left).index(), 1);
        screen_of(&mut app, 12);
        // ` 1 srv ` from column 1 of the line, or after the frame's corner.
        press(&mut app, Press::Click, 3, 0);
        assert_eq!(app.tabs(Side::Left).index(), 0, "{tab_bar:?}");
        assert_eq!(app.active, Side::Left);
    }
}

#[test]
fn the_mouse_runs_commands_of_the_pull_down_menu() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let dirs = (0..20).map(|n| dir(&format!("d{n:02}"))).collect();
    answer(&mut app, effects, &Listing::Dir(dirs));
    let (x, y) = find(&mut app, "PullDn");
    press(&mut app, Press::Click, x, y);
    assert_eq!(app.context(), Context::PullDown);
    let (x, y) = find(&mut app, "Command");
    press(&mut app, Press::Click, x, y);
    let (x, y) = find(&mut app, "Swap panels");
    assert!(press(&mut app, Press::Click, x, y).is_empty());
    assert!(app.swapped);
    assert!(app.pulldown.is_none());
    // The second click of a double click lands on a directory, which it does not open.
    assert!(press(&mut app, Press::DoubleClick, x, y).is_empty());
    assert_eq!(cursor_name(&app, Side::Left), "");

    // A menu bar that stays opens a menu at a click on its title.
    app.config.ui.menu_bar = MenuBar::Always;
    let (x, y) = find(&mut app, "Options");
    press(&mut app, Press::Click, x, y);
    let (x, y) = find(&mut app, "Configuration");
    press(&mut app, Press::Click, x, y);
    assert!(app.configuration.is_some());
}

#[test]
fn the_mouse_chooses_in_menus_and_presses_the_buttons_of_windows() {
    let mut app = loaded();
    open_menu(&mut app, Side::Right);
    let (x, y) = find(&mut app, "USB");
    press(&mut app, Press::Click, x, y);
    assert!(app.menu.is_some(), "a click puts the cursor on the row");
    let Effect::List { panel, request, .. } = one(press(&mut app, Press::DoubleClick, x, y)) else {
        panic!("expected a listing");
    };
    assert_eq!(
        (panel.side, request.location),
        (Side::Right, local("/Volumes/USB"))
    );
    assert!(app.menu.is_none());
    assert_eq!(
        app.active,
        Side::Left,
        "the keys come back to the active panel"
    );
    app.panel_mut(Side::Right).cancel();
    app.handle(action(Action::FocusRight));
    open_menu(&mut app, Side::Right);
    screen_of(&mut app, 12);
    press(&mut app, Press::Click, 10, 3);
    assert!(app.menu.is_none(), "a click outside closes the menu");
    assert_eq!(app.active, Side::Right, "and does nothing else");

    // A job in front goes behind the panels, then the list of jobs aborts it.
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (_, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
    let (x, y) = find(&mut app, "Background");
    press(&mut app, Press::WheelDown, x, y);
    press(&mut app, Press::Click, x, y);
    assert!(app.in_front().is_none());
    app.handle(action(Action::Jobs));
    let (x, y) = find(&mut app, "Abort");
    press(&mut app, Press::Click, x, y);
    assert!(cancel.is_cancelled());

    // The wheel scrolls the help.
    app.handle(action(Action::Cancel));
    app.handle(action(Action::Help));
    let before = screen_of(&mut app, 12);
    press(&mut app, Press::WheelDown, 80, 5);
    assert_ne!(screen_of(&mut app, 12), before);
}

/// The column of the first scroll bar of what is in front, and the rows of its arrows, as
/// the last render drew it.
fn bar_ends(app: &App) -> (u16, u16, u16) {
    let (_, bar) = app.scroll_bars()[0];
    let area = bar.area();
    (area.x, area.y, area.bottom() - 1)
}

#[test]
fn the_mouse_scrolls_by_the_arrows_and_the_track_and_drags_the_thumb() {
    let mut app = loaded();
    app.handle(action(Action::Help));
    let top = screen_of(&mut app, 12);
    let (x, up, down) = bar_ends(&app);
    press(&mut app, Press::Click, x, down);
    assert_ne!(screen_of(&mut app, 12), top, "the arrow down");
    press(&mut app, Press::DoubleClick, x, up);
    assert_eq!(screen_of(&mut app, 12), top, "the arrow up, as the wheel");
    press(&mut app, Press::Click, x, down - 1);
    let paged = screen_of(&mut app, 12);
    app.handle(action(Action::PageDown));
    app.handle(action(Action::PageUp));
    assert_eq!(screen_of(&mut app, 12), paged, "a page, as PageDown");
    app.handle(action(Action::PageUp));
    assert_eq!(screen_of(&mut app, 12), top);

    press(&mut app, Press::Drag, x, down - 1);
    assert_eq!(screen_of(&mut app, 12), top, "no thumb is held");
    press(&mut app, Press::Click, x, up + 1);
    assert!(app.dragging());
    press(&mut app, Press::Drag, x, down - 1);
    let end = screen_of(&mut app, 12);
    assert!(end.contains(" End "), "{end}");
    press(&mut app, Press::Click, 10, 1);
    assert!(!app.dragging(), "another press lets go");
    press(&mut app, Press::Drag, x, up + 1);
    assert_eq!(screen_of(&mut app, 12), end);

    // The viewer's, through the same parts.
    app.handle(action(Action::Cancel));
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![file("log", 1)]));
    app.handle(action(Action::Down));
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
        panic!("expected a read");
    };
    let lines: Vec<String> = (1..=100).map(|n| format!("line {n}")).collect();
    app.read(id, Ok(whole(lines.join("\n"))));
    screen_of(&mut app, 12);
    let (x, up, down) = bar_ends(&app);
    press(&mut app, Press::Click, x, down);
    assert!(screen_of(&mut app, 12).contains(" 4/100 "), "three lines");
    press(&mut app, Press::Click, x, up + 1);
    press(&mut app, Press::Drag, x, down - 1);
    let end = screen_of(&mut app, 12);
    assert!(
        end.contains(" 92/100 100% "),
        "the last line at the bottom: {end}"
    );
}

#[test]
fn the_scroll_bars_of_the_panels_are_on_their_frames() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let files: Vec<DirEntry> = (10..60).map(|n| file(&format!("file{n}"), 1)).collect();
    answer(&mut app, effects, &Listing::Dir(files));
    let halves = |screen: &str| -> (Vec<String>, Vec<String>) {
        let rows = screen.lines();
        let half = |skip: usize| {
            rows.clone()
                .map(|row| row.chars().skip(skip).take(80).collect())
                .collect()
        };
        (half(0), half(81))
    };
    let before = halves(&screen_of(&mut app, 12));
    let bars = app.scroll_bars();
    assert_eq!(bars.len(), 2, "one on each panel");
    let (target, bar) = bars[0];
    assert_eq!(target, Scrolled::Panel(Side::Left));
    let area = bar.area();
    assert_eq!(area.x, 79, "on the right of the left panel's frame");
    press(&mut app, Press::Click, area.x, area.bottom() - 1);
    let scrolled = halves(&screen_of(&mut app, 12));
    assert_ne!(scrolled.0, before.0, "the left panel scrolled");
    assert_eq!(scrolled.1, before.1, "the right one did not");
    assert_eq!(app.active, Side::Left);
    press(&mut app, Press::Click, area.x, area.y + 1);
    assert!(app.dragging());
    press(&mut app, Press::Drag, area.x, area.bottom());
    let end = screen_of(&mut app, 12);
    assert!(end.contains("file59"), "{end}");
    assert!(
        cursor_name(&app, Side::Left).starts_with("file5"),
        "on screen"
    );

    // A panel that fits has none.
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![file("one", 1)]));
    let screen = screen_of(&mut app, 12);
    assert!(!screen.contains('░'), "{screen}");
    assert_eq!(app.scroll_bars(), Vec::new());
}

#[test]
fn the_mouse_answers_dialogs() {
    let mut app = loaded();
    let (x, y) = find(&mut app, "Mkdir");
    press(&mut app, Press::Click, x, y);
    assert_eq!(app.dialogs.len(), 1);
    let (x, y) = find(&mut app, "Cancel");
    let (panel_x, panel_y) = (90, 3);
    press(&mut app, Press::Click, panel_x, panel_y);
    assert_eq!(
        app.active,
        Side::Left,
        "the dialog keeps the panels from the mouse"
    );
    press(&mut app, Press::Click, x, y);
    assert!(app.dialogs.is_empty());
}
