use super::*;

fn with_picker() -> Dialog {
    let mut picker = Picker::new(Input::new(Kind::FILTER, noc_config::SearchMode::Plain));
    let rows = ["alpha", "bravo", "charlie"];
    picker.set_rows(rows.iter().map(|row| vec![(*row).to_owned()]).collect());
    Dialog::question("Pick", "", vec![Button::Ok, Button::Cancel], 0, Tone::Plain)
        .with_width(40)
        .with_picker(picker)
}

fn draw_picker(dialog: &mut Dialog) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(40, 14)).unwrap();
    terminal
        .draw(|frame| {
            let rows = ["alpha", "bravo", "charlie"];
            let shown = dialog.picker().unwrap().shown(rows);
            let lines: Vec<_> = (0..shown.len()).map(MenuLine::Row).collect();
            let body = Body {
                lines: &lines,
                ..Body::default()
            };
            let theme = Theme::terminal();
            dialog.render_picker(frame, frame.area(), &theme, body, &mut |row| {
                Line::raw(shown[row.index])
            });
        })
        .unwrap();
    terminal
}

#[test]
fn the_rows_of_a_picker_are_a_stop_whose_motions_move_its_cursor() {
    let mut dialog = with_picker();
    assert_eq!(dialog.focus, Focus::Picker, "its rows start with the focus");
    assert_eq!(dialog.mode(), Mode::Insert, "typing in its filter");
    assert_eq!(dialog.search_focused(), Some(Kind::FILTER));
    typed(&mut dialog, "r");
    assert_eq!(dialog.picker().unwrap().len(), 2, "bravo and charlie");
    dialog.handle(action(Action::Left));
    assert_eq!(dialog.focus, Focus::Picker, "Left edits the filter");
    // The motions stop at the last row and the first.
    dialog.handle(action(Action::Down));
    dialog.handle(action(Action::Down));
    assert_eq!(
        (dialog.focus, dialog.picker().unwrap().cursor()),
        (Focus::Picker, 1)
    );
    dialog.handle(action(Action::Up));
    dialog.handle(action(Action::Up));
    assert_eq!(
        (dialog.focus, dialog.picker().unwrap().cursor()),
        (Focus::Picker, 0)
    );
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::Ok),
        "Enter on a row presses the default button"
    );
    // Tab leaves them for the buttons, and the filter takes no more.
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.focus, Focus::Button(0));
    assert_eq!(dialog.mode(), Mode::Normal);
    assert_eq!(dialog.search_focused(), None);
    dialog.handle(action(Action::Down));
    assert_eq!(
        dialog.handle(action(Action::Toggle)),
        DialogEvent::Pressed(Button::Cancel)
    );
    // On the buttons the motions go through the stops, back to the rows.
    dialog.handle(action(Action::Down));
    assert_eq!(dialog.focus, Focus::Picker);
    dialog.handle(action(Action::Right));
    assert_eq!(
        dialog.focus,
        Focus::Button(0),
        "Right on the rows moves the focus"
    );
    // `/` on a button types in the filter again.
    dialog.handle(action(Action::QuickSearch));
    assert_eq!((dialog.focus, dialog.mode()), (Focus::Picker, Mode::Insert));
    assert_eq!(
        dialog.handle(Resolved::Hotkey('o')),
        DialogEvent::Pressed(Button::Ok)
    );
    dialog.handle(action(Action::NormalMode));
    assert_eq!(
        dialog.handle(action(Action::Cancel)),
        DialogEvent::Cancelled,
        "Esc on the rows closes the dialog"
    );
}

#[test]
fn clicks_on_the_rows_of_a_picker_move_its_cursor_and_press_the_buttons() {
    let mut dialog = with_picker();
    let terminal = draw_picker(&mut dialog);
    let screen = terminal.backend().to_string();
    assert!(
        screen.contains("Filter:") && screen.contains("{ OK }"),
        "{screen}"
    );
    let buffer = terminal.backend().buffer();
    let find = |text| crate::tui::mouse::find(buffer, text);
    let click = |at| Pointer {
        press: Press::Click,
        at,
    };
    assert_eq!(dialog.pointer(click(find("charlie"))), DialogEvent::Pending);
    assert_eq!(dialog.picker().unwrap().cursor(), 2);
    let double = Pointer {
        press: Press::DoubleClick,
        at: find("alpha"),
    };
    assert_eq!(
        dialog.pointer(double),
        DialogEvent::Pressed(Button::Ok),
        "as Enter there"
    );
    assert_eq!(dialog.picker().unwrap().cursor(), 0);
    assert_eq!(
        dialog.pointer(click(find("Cancel"))),
        DialogEvent::Pressed(Button::Cancel)
    );
    assert_eq!(dialog.focus, Focus::Button(1));
    dialog.pointer(click(find("bravo")));
    assert_eq!(
        (dialog.focus, dialog.mode()),
        (Focus::Picker, Mode::Normal),
        "a click on a row gives the rows the focus"
    );
    assert_eq!(dialog.picker().unwrap().cursor(), 1);
}
