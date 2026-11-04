use super::*;

#[test]
fn space_opens_a_combo_box_whose_list_has_the_keys() {
    let mut dialog = picker();
    assert_eq!(
        (dialog.focus, dialog.context()),
        (Focus::ComboBox(0), Context::Dialog),
        "without fields, the combo box first"
    );
    dialog.handle(action(Action::Toggle));
    assert_eq!(
        (dialog.context(), dialog.mode()),
        (Context::ComboBox, Mode::Normal)
    );
    let two = Typed {
        resolved: action(Action::Down),
        extra: Extra {
            count: Some(2),
            ..Extra::default()
        },
    };
    dialog.handle_typed(two, &mut Registers::default());
    assert_eq!(dialog.chosen(0), 0, "the cursor moves, not the choice");
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pending,
        "Enter takes the row and closes the list"
    );
    assert_eq!((dialog.chosen(0), dialog.context()), (2, Context::Dialog));
    dialog.handle(action(Action::Toggle));
    dialog.handle(action(Action::Up));
    dialog.handle(action(Action::Cancel));
    assert_eq!(dialog.chosen(0), 2, "Esc keeps the choice");
    dialog.handle(action(Action::Down));
    assert_eq!(dialog.focus, Focus::Check(0), "j and k move the focus");
    dialog.handle(action(Action::Up));
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::Ok),
        "Enter on the box presses the default button"
    );
}

#[test]
fn a_click_opens_a_combo_box_and_one_in_its_list_takes_a_row() {
    let mut dialog = picker();
    dialog.handle(action(Action::Down));
    assert_eq!(
        press_on(&mut dialog, Press::Click, "Count:"),
        DialogEvent::Pending
    );
    assert_eq!(
        (dialog.focus, dialog.context()),
        (Focus::ComboBox(0), Context::ComboBox)
    );
    assert_eq!(
        press_on(&mut dialog, Press::DoubleClick, "Count:"),
        DialogEvent::Pending
    );
    assert!(
        dialog.combo_box_open(),
        "the second click of the one that opened it"
    );
    let terminal = draw(&dialog, 40, 12);
    let two = crate::tui::mouse::find(terminal.backend().buffer(), "two");
    let at = |press| Pointer { press, at: two };
    assert_eq!(dialog.pointer(at(Press::Click)), DialogEvent::Pending);
    assert_eq!((dialog.chosen(0), dialog.combo_box_open()), (1, false));
    // The row was over the buttons.
    assert_eq!(dialog.pointer(at(Press::DoubleClick)), DialogEvent::Pending);
    assert_eq!(
        dialog.focus,
        Focus::ComboBox(0),
        "its second click acts on nothing under it"
    );
    press_on(&mut dialog, Press::Click, "two");
    assert!(dialog.combo_box_open());
    press_on(&mut dialog, Press::Click, "Pick");
    assert!(!dialog.combo_box_open(), "a click outside closes the list");
    assert_eq!(dialog.chosen(0), 1);
}

#[test]
fn a_combo_box_has_the_colors_of_fields_and_its_focus_stands_apart() {
    use ratatui::style::Color;

    let theme = Theme::mc_classic();
    let mut dialog = picker();
    // The colors of the cells of `text`, from `from` cells before it, as many as `count`.
    let colors = |dialog: &Dialog, text: &str, from: u16, count: u16| {
        let terminal = draw_themed(dialog, 40, 12, &theme);
        let buffer = terminal.backend().buffer();
        let at = crate::tui::mouse::find(buffer, text);
        (0..count)
            .map(|offset| {
                let cell = &buffer[(at.x - from + offset, at.y)];
                (cell.fg, cell.bg, cell.modifier.contains(Modifier::REVERSED))
            })
            .collect::<Vec<_>>()
    };
    // `[one   ▼]`: the bracket, a letter, the arrow, and the bracket.
    let combo = |dialog: &Dialog| {
        let cells = colors(dialog, "[one", 0, 9);
        [cells[0], cells[1], cells[7], cells[8]]
    };
    let focused = (Color::Black, Color::Cyan, true);
    assert_eq!(
        combo(&dialog),
        [focused; 4],
        "fields have the focus's colors in mc-classic: reversed, all of it"
    );
    dialog.handle(action(Action::Down));
    let (faint, field) = (Color::DarkGray, Color::Cyan);
    assert_eq!(
        combo(&dialog),
        [
            (faint, field, false),
            (Color::Black, field, false),
            (Color::Blue, field, false),
            (faint, field, false),
        ],
        "as a field, its brackets faint and its arrow blue as titles"
    );
    // `[✓] Verbose`, with the focus, then without it; its letter is blue either way.
    dialog.handle(action(Action::Toggle));
    let check = |dialog: &Dialog| colors(dialog, "Verbose", 4, 4);
    let focus = (Color::Black, Color::Cyan, false);
    assert_eq!(check(&dialog), [focus; 4]);
    dialog.handle(action(Action::Down));
    let gray = Color::Gray;
    assert_eq!(
        check(&dialog),
        [
            (faint, gray, false),
            (Color::Blue, gray, false),
            (faint, gray, false),
            (Color::Black, gray, false),
        ]
    );
}

#[test]
fn draws_a_combo_box_and_its_list_over_the_dialog() {
    let mut dialog = picker();
    dialog.handle(action(Action::Toggle));
    dialog.handle(action(Action::Down));
    let terminal = draw(&dialog, 40, 12);
    insta::assert_snapshot!(terminal.backend());
}
