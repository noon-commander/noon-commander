use super::*;

/// A dialog with a field, a check box, `len` rows without a filter, and OK and Cancel.
fn with_rows(len: usize) -> Dialog {
    let fields = [("Text:".to_owned(), String::new())];
    let checks = [("Case".to_owned(), false)];
    let mut picker = Picker::without_filter();
    picker.set_rows(vec![Vec::new(); len]);
    Dialog::fields("Find", &fields, &checks, vec![Button::Ok, Button::Cancel])
        .with_width(40)
        .with_picker(picker)
}

fn draw_rows(dialog: &mut Dialog, message: Option<&str>) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(40, 16)).unwrap();
    terminal
        .draw(|frame| {
            let len = dialog.picker().unwrap().len();
            let lines: Vec<_> = (0..len).map(MenuLine::Row).collect();
            let body = Body {
                lines: &lines,
                message: message.map(str::to_owned),
                footer: vec![Line::raw("30 rows")],
                fill: true,
                ..Body::default()
            };
            let theme = Theme::terminal();
            dialog.render_picker(frame, frame.area(), &theme, body, &mut |row| {
                Line::raw(format!("row {}", row.index))
            });
        })
        .unwrap();
    terminal
}

#[test]
fn rows_without_a_filter_are_a_stop_whose_motions_move_their_cursor() {
    let mut dialog = with_rows(30);
    let screen = draw_rows(&mut dialog, None).backend().to_string();
    assert!(
        screen.contains("row 0") && screen.contains("30 rows"),
        "{screen}"
    );
    assert!(!screen.contains("Filter"), "no filter: {screen}");
    let cursor = |dialog: &Dialog| dialog.picker().unwrap().cursor();
    dialog.handle(action(Action::NextField));
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.context(), Context::Dialog);
    assert_eq!(dialog.mode(), Mode::Normal);
    dialog.handle(action(Action::Down));
    assert_eq!(cursor(&dialog), 1, "Down moves the cursor, not the focus");
    let three = Extra {
        count: Some(3),
        ..Extra::default()
    };
    let down = Typed {
        resolved: action(Action::Down),
        extra: three,
    };
    dialog.handle_typed(down, &mut Registers::default());
    assert_eq!(cursor(&dialog), 4);
    // Nothing to type in, nor a filter to bring the focus to.
    dialog.handle(Resolved::Insert('x'));
    dialog.handle(action(Action::QuickSearch));
    assert_eq!((dialog.focus, dialog.mode()), (Focus::Picker, Mode::Normal));
    dialog.handle(action(Action::End));
    dialog.handle(action(Action::Down));
    assert_eq!(cursor(&dialog), 29, "the motions stop at the last row");
    let screen = draw_rows(&mut dialog, None).backend().to_string();
    assert!(
        screen.contains("row 29") && !screen.contains("row 0 "),
        "{screen}"
    );
    dialog.handle(action(Action::Home));
    assert_eq!(cursor(&dialog), 0);
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::Ok),
        "Enter presses the default button"
    );
    // Tab goes on to the buttons, and `/` there does not come back.
    dialog.handle(action(Action::NextField));
    dialog.handle(action(Action::QuickSearch));
    assert_eq!(dialog.focus, Focus::Button(0));
    assert_eq!(
        dialog.handle(action(Action::Toggle)),
        DialogEvent::Pressed(Button::Ok)
    );
}

#[test]
fn the_mouse_and_the_wheel_move_over_the_rows() {
    let mut dialog = with_rows(30);
    let terminal = draw_rows(&mut dialog, None);
    let at = crate::tui::mouse::find(terminal.backend().buffer(), "row 2");
    let click = Pointer {
        press: Press::Click,
        at,
    };
    assert_eq!(dialog.pointer(click), DialogEvent::Pending);
    assert_eq!(
        (dialog.picker().unwrap().cursor(), dialog.focus),
        (2, Focus::Picker)
    );
    let double = Pointer {
        press: Press::DoubleClick,
        at,
    };
    assert_eq!(dialog.pointer(double), DialogEvent::Pressed(Button::Ok));
    dialog.picker_mut().unwrap().wheel(Action::Down, 10);
    assert!(
        dialog.picker().unwrap().cursor() > 2,
        "kept inside what shows"
    );
    draw_rows(&mut dialog, None);
    assert!(dialog.picker().unwrap().bar().is_some());
    // A message shows in place of the rows.
    let screen = draw_rows(&mut dialog, Some("Nothing found."))
        .backend()
        .to_string();
    assert!(
        screen.contains("Nothing found.") && !screen.contains("row "),
        "{screen}"
    );
}
