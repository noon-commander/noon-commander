use super::*;

fn hashing(disabled: bool) -> Dialog {
    let choices = ["&one".to_owned(), "&two".to_owned()];
    let dialog = Dialog::fields("Hash", &[], &[], vec![Button::Ok, Button::Cancel])
        .with_width(40)
        .with_message("Of x")
        .with_combo_box("&Count:", &choices, 0)
        .with_separator()
        .with_field("&Expected:", "")
        .with_placeholder(0, "Optional: hex")
        .with_check("&Verbose", false);
    if disabled {
        dialog.with_disabled(0)
    } else {
        dialog
    }
}

/// The row of the first line of `terminal` that holds `text`.
fn row_of(terminal: &Terminal<TestBackend>, text: &str) -> u16 {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .find(|&y| {
            let row: String = (0..area.width).map(|x| buffer[(x, y)].symbol()).collect();
            row.contains(text)
        })
        .unwrap()
}

#[test]
fn draws_items_with_their_details_on_the_right_between_lines() {
    let dialog = with_items();
    let terminal = draw(&dialog, 60, 12);
    insta::assert_snapshot!(terminal.backend());
    let (new, existing) = (row_of(&terminal, "New"), row_of(&terminal, "Existing"));
    assert_eq!(existing, new + 1);
    let narrow = draw(&dialog, 40, 12);
    assert!(row_of(&narrow, "New") > 0, "a detail too long is cut");
}

#[test]
fn items_open_from_keys_their_letters_and_clicks_and_the_dialog_stays() {
    let mut dialog = with_items();
    assert_eq!(dialog.focus, Focus::Button(1), "the default has the focus");
    assert_eq!(dialog.handle(Resolved::Hotkey('x')), DialogEvent::Opened(1));
    assert_eq!(dialog.focus, Focus::Item(1));
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Opened(1)
    );
    dialog.handle(action(Action::Up));
    assert_eq!(
        dialog.handle(action(Action::Toggle)),
        DialogEvent::Opened(0)
    );
    dialog.handle(action(Action::NextField));
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.focus, Focus::ComboBox(0), "past the items");
    assert_eq!(
        press_on(&mut dialog, Press::Click, "Existing"),
        DialogEvent::Opened(1)
    );
    let twice = press_on(&mut dialog, Press::DoubleClick, "Existing");
    assert_eq!(twice, DialogEvent::Pending, "the first click opened it");
    assert_eq!(dialog.letters(), ['n', 'x', 't', 'o', 's', 'a']);
}

#[test]
fn draws_the_parts_in_order_with_a_separator_and_a_disabled_field_faint() {
    let dialog = hashing(true);
    let theme = Theme::terminal();
    let terminal = draw_themed(&dialog, 40, 12, &theme);
    insta::assert_snapshot!(terminal.backend());
    let (combo, label) = (row_of(&terminal, "Count:"), row_of(&terminal, "Expected:"));
    assert_eq!(label, combo + 2, "a blank row between them");
    let buffer = terminal.backend().buffer();
    let x = (0..40)
        .find(|&x| buffer[(x, label)].symbol() == "E")
        .unwrap();
    let dim = |style: Style| style.add_modifier.contains(Modifier::DIM);
    assert!(dim(theme.disabled));
    assert!(dim(buffer[(x, label)].style()), "its letter too");
    assert_eq!(
        buffer[(x, label)].style(),
        buffer[(x + 1, label)].style(),
        "its letter as the rest, not set apart nor underlined"
    );
    let field = buffer[(x, label + 1)].style();
    assert!(dim(field), "the field");
    assert_eq!(
        field.add_modifier,
        theme.dialog_input.add_modifier | Modifier::DIM,
        "still in the colors of fields"
    );
    assert_eq!(
        row_of(&terminal, "Optional: hex"),
        label + 1,
        "its placeholder, faint with it"
    );
    assert_eq!(buffer[(x, label + 1)].symbol(), "O");

    let enabled = draw_themed(&hashing(false), 40, 12, &theme);
    assert_eq!(row_of(&enabled, "Optional: hex"), label + 1);
    let buffer = enabled.backend().buffer();
    assert!(
        !dim(buffer[(x, label)].style()),
        "else in its letter's color"
    );
    assert_ne!(
        buffer[(x, label)].style(),
        buffer[(x + 1, label)].style(),
        "and set apart"
    );
}

#[test]
fn a_dialog_under_another_shows_without_the_cursor() {
    let dialog = host_form();
    let mut terminal = Terminal::new(TestBackend::new(64, 16)).unwrap();
    let theme = Theme::terminal();
    let mut cursor = |under: bool| {
        terminal
            .draw(|frame| {
                if under {
                    dialog.render_under(frame, frame.area(), &theme);
                } else {
                    dialog.render(frame, frame.area(), &theme);
                }
            })
            .unwrap();
        terminal.backend().cursor_visible()
    };
    assert!(cursor(false), "in the field");
    assert!(!cursor(true), "the dialog over it has the keys");
}

#[test]
fn a_disabled_field_takes_no_focus_from_keys_letters_or_the_mouse() {
    let mut dialog = hashing(true);
    assert_eq!(dialog.focus, Focus::ComboBox(0), "the first stop");
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.focus, Focus::Check(0), "past the field");
    dialog.handle(Resolved::Hotkey('e'));
    assert_eq!(dialog.focus, Focus::Check(0), "its letter does nothing");
    assert!(!dialog.letters().contains(&'e'));
    assert!(hashing(false).letters().contains(&'e'));
    let terminal = draw(&dialog, 40, 12);
    let buffer = terminal.backend().buffer();
    let at = (0..12)
        .find_map(|y| {
            let x = (0..40).find(|&x| buffer[(x, y)].symbol() == "E")?;
            Some(Position::new(x, y + 1))
        })
        .unwrap();
    dialog.pointer(Pointer {
        press: Press::Click,
        at,
    });
    assert_eq!(dialog.focus, Focus::Check(0), "a click on it does nothing");
    assert_eq!(
        hashing(false).focus,
        Focus::Field(0),
        "else the field first"
    );
}

#[test]
fn a_separator_goes_anywhere_and_takes_neither_the_focus_nor_clicks() {
    let mut dialog = Dialog::fields("Pick", &[], &[], vec![Button::Ok, Button::Cancel])
        .with_width(40)
        .with_check("&Fast", false)
        .with_separator()
        .with_check("&Slow", false);
    let terminal = draw(&dialog, 40, 12);
    let (fast, slow) = (row_of(&terminal, "Fast"), row_of(&terminal, "Slow"));
    assert_eq!(slow, fast + 2, "a blank row between the check boxes");
    assert_eq!(dialog.stops().len(), 4, "two check boxes and two buttons");
    dialog.focus = Focus::Check(0);
    dialog.handle(action(Action::Down));
    assert_eq!(dialog.focus, Focus::Check(1), "straight past it");
    let blank = Position::new(6, fast + 1);
    assert_eq!(
        dialog.pointer(Pointer {
            press: Press::Click,
            at: blank,
        }),
        DialogEvent::Pending
    );
    assert_eq!(dialog.focus, Focus::Check(1), "a click on it does nothing");
    assert!(!dialog.checked(0) && !dialog.checked(1));
}

#[test]
fn draws_a_form_with_its_check_boxes() {
    let mut dialog = form();
    dialog.handle(action(Action::DeleteToStart));
    typed(&mut dialog, "*.md");
    dialog.handle(action(Action::NextField));
    let terminal = draw(&dialog, 60, 10);
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn draws_the_transfer_dialog_of_a_move_as_that_of_a_copy() {
    use crate::tui::transfer::{self, Kind, Sources};

    let sources = Sources::One("notes.txt".to_owned());
    let dialog = transfer::dialog(Kind::Move, &sources, ("/srv/backup", 11), true);
    let terminal = draw(&dialog, 60, 10);
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn the_focus_moves_through_the_fields_in_order() {
    let mut dialog = host_form();
    assert_eq!(dialog.context(), Context::DialogInput);
    typed(&mut dialog, "Staging");
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.context(), Context::DialogInput);
    typed(&mut dialog, "/srv");
    assert_eq!(dialog.text_of(0), "ProdStaging");
    assert_eq!(dialog.text(), "ProdStaging");
    assert_eq!(dialog.text_of(1), "/var/www/srv");
    assert_eq!(dialog.text_of(2), "");
    assert_eq!(dialog.text_of(3), "", "no such field");
    dialog.handle(action(Action::Up));
    typed(&mut dialog, "!");
    assert_eq!(
        dialog.text_of(0),
        "ProdStaging!",
        "Up goes back to the first field"
    );
    dialog.handle(action(Action::Down));
    dialog.handle(action(Action::Down));
    typed(&mut dialog, "/tmp");
    assert_eq!(dialog.text_of(2), "/tmp");
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.context(), Context::Dialog, "the check box");
    dialog.handle(action(Action::Toggle));
    assert!(!dialog.checked(0));
    dialog.handle(action(Action::NextField));
    dialog.handle(action(Action::NextField));
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::UseCurrent)
    );
    dialog.handle(action(Action::NextField));
    dialog.handle(action(Action::NextField));
    assert_eq!(
        dialog.context(),
        Context::DialogInput,
        "round to the first field"
    );
    dialog.handle(action(Action::Down));
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::Ok),
        "Enter in a field presses OK"
    );
}

#[test]
fn set_text_replaces_a_field_and_focuses_it() {
    let mut dialog = host_form();
    dialog.set_text(2, "/home/deploy");
    assert_eq!(dialog.text_of(2), "/home/deploy");
    assert_eq!(dialog.context(), Context::DialogInput);
    typed(&mut dialog, "/app");
    assert_eq!(dialog.text_of(2), "/home/deploy/app", "typing keeps it");
    assert_eq!(dialog.text_of(0), "Prod");
    dialog.set_text(7, "ignored");

    let mut secret = secret();
    secret.set_text(0, "nope");
    assert_eq!(confirm(&mut secret).as_deref(), Some(""), "a secret stays");
}

#[test]
fn draws_a_form_with_labelled_fields() {
    let mut dialog = host_form();
    dialog.handle(action(Action::Down));
    let mut terminal = draw(&dialog, 64, 16);
    insta::assert_snapshot!(terminal.backend());
    // At the end of `/var/www`, under its label.
    let cursor = terminal.get_cursor_position().unwrap();
    let buffer = terminal.backend().buffer();
    let row: String = (0..64).map(|x| buffer[(x, cursor.y)].symbol()).collect();
    assert!(row.contains("/var/www"), "{row}");
    let above: String = (0..64)
        .map(|x| buffer[(x, cursor.y - 1)].symbol())
        .collect();
    assert!(above.contains("Remote directory:"), "{above}");
}

#[test]
fn an_empty_field_shows_its_placeholder_faint_until_typed_in() {
    let theme = Theme::terminal();
    let mut dialog = host_form().with_placeholder(2, "~/site\u{1b}[2J");
    dialog.handle(action(Action::Down));
    dialog.handle(action(Action::Down));
    let row = |mut terminal: Terminal<TestBackend>| {
        let cursor = terminal.get_cursor_position().unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..64).map(|x| buffer[(x, cursor.y)].symbol()).collect();
        let first = (0..64)
            .find(|&x| buffer[(x, cursor.y)].symbol() == "~")
            .map(|x| (x, buffer[(x, cursor.y)].style()));
        (cursor.x, text, first)
    };
    let (cursor, text, first) = row(draw_themed(&dialog, 64, 16, &theme));
    assert!(text.contains("~/site"), "{text}");
    assert!(!text.contains('\u{1b}'), "terminal-safe: {text}");
    let (x, style) = first.unwrap();
    assert_eq!(cursor, x, "the cursor stands at its start");
    let faint = theme.dialog_input.patch(theme.disabled);
    assert_eq!(style.add_modifier, faint.add_modifier);
    assert_ne!(faint.add_modifier, theme.dialog_input.add_modifier);
    assert_eq!(dialog.text_of(2), "", "it is not the text");

    typed(&mut dialog, "/srv");
    let (_, text, _) = row(draw_themed(&dialog, 64, 16, &theme));
    assert!(text.contains("/srv") && !text.contains("~/site"), "{text}");
    dialog.handle(action(Action::DeleteToStart));
    let (_, text, _) = row(draw_themed(&dialog, 64, 16, &theme));
    assert!(text.contains("~/site"), "back once empty: {text}");
    assert_eq!(dialog.text_of(2), "");
}
