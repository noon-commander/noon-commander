use super::*;

#[test]
fn a_form_opens_in_insert_mode_and_esc_goes_to_normal_mode() {
    let mut dialog = form();
    assert_eq!(dialog.context(), Context::DialogInput);
    assert_eq!(dialog.mode(), Mode::Insert);
    assert_eq!(dialog.text(), "*");
    typed(&mut dialog, ".txt");
    assert_eq!(dialog.text(), "*.txt", "typing goes on from the end");
    assert_eq!(
        dialog.handle(action(Action::NormalMode)),
        DialogEvent::Pending
    );
    assert_eq!(dialog.mode(), Mode::Normal);
    dialog.handle(action(Action::TextStart));
    dialog.handle(action(Action::DeleteChar));
    assert_eq!(dialog.text(), ".txt");
    dialog.handle(action(Action::Undo));
    assert_eq!(dialog.text(), "*.txt");
    assert_eq!(
        dialog.handle(action(Action::Cancel)),
        DialogEvent::Cancelled,
        "Esc in Normal mode closes it"
    );
    let mut dialog = form();
    dialog.handle(action(Action::NormalMode));
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::Ok),
        "Enter in Normal mode too"
    );
    // A secret has no Normal mode: Esc closes its dialog.
    assert_eq!(
        secret().handle(action(Action::NormalMode)),
        DialogEvent::Cancelled
    );
}

#[test]
fn esc_after_ctrl_o_goes_back_to_insert_mode_and_leaves_the_dialog_open() {
    let mut dialog = form();
    assert_eq!(
        dialog.handle(action(Action::NormalOnce)),
        DialogEvent::Pending
    );
    assert_eq!(dialog.handle(action(Action::Cancel)), DialogEvent::Pending);
    assert_eq!(dialog.mode(), Mode::Insert);
    assert!(!dialog.secret_focused());
    assert!(secret().secret_focused());
}

#[test]
fn tab_brings_the_focus_to_a_field_in_insert_mode_and_j_and_k_in_normal_mode() {
    let mut dialog = host_form();
    dialog.handle(action(Action::NormalMode));
    dialog.handle(action(Action::Down));
    assert_eq!(
        (dialog.text_of(1), dialog.mode()),
        ("/var/www", Mode::Normal)
    );
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.mode(), Mode::Insert, "Tab");
    dialog.handle(action(Action::Up));
    assert_eq!(dialog.mode(), Mode::Insert, "from a field in Insert mode");
    assert_eq!(dialog.pending(), None);
    dialog.handle(action(Action::NormalMode));
    dialog.handle(action(Action::DeleteOperator));
    assert_eq!(dialog.pending().as_deref(), Some("d"));
    assert_eq!(
        dialog.handle(action(Action::Cancel)),
        DialogEvent::Pending,
        "Esc gives the operator up first"
    );
    assert_eq!(dialog.pending(), None);
}

#[test]
fn a_key_of_the_dialog_after_ctrl_o_leaves_insert_mode_on_and_g_g_and_g_go_to_the_ends() {
    let mut dialog = host_form();
    dialog.handle(action(Action::NormalOnce));
    dialog.handle(action(Action::Down));
    assert_eq!(
        (dialog.focus, dialog.mode()),
        (Focus::Field(1), Mode::Insert)
    );
    dialog.handle(action(Action::NormalMode));
    dialog.handle(action(Action::End));
    assert_eq!(dialog.focus, Focus::Button(dialog.buttons.len() - 1));
    dialog.handle(action(Action::Home));
    assert_eq!(
        (dialog.focus, dialog.mode()),
        (Focus::Field(0), Mode::Normal)
    );
}

#[test]
fn check_boxes_switch_and_enter_presses_the_default_button() {
    let mut dialog = form();
    assert_eq!(dialog.handle(action(Action::Toggle)), DialogEvent::Pending);
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.context(), Context::Dialog);
    dialog.handle(action(Action::Toggle));
    dialog.handle(action(Action::Down));
    dialog.handle(action(Action::Toggle));
    assert!(dialog.checked(0));
    assert!(!dialog.checked(1));
    assert!(!dialog.checked(2), "no such box");
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::Ok)
    );
    dialog.handle(action(Action::NextField));
    dialog.handle(action(Action::NextField));
    assert_eq!(
        dialog.handle(action(Action::Toggle)),
        DialogEvent::Pressed(Button::Cancel),
        "Space presses a button"
    );
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.context(), Context::DialogInput, "round to the field");
    assert_eq!(dialog.text(), "*");
}
