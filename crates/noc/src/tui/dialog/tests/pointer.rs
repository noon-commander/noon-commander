use super::*;

#[test]
fn the_mouse_switches_check_boxes_edits_fields_and_presses_buttons() {
    let mut dialog = form();
    assert_eq!(
        press_on(&mut dialog, Press::Click, "Files only"),
        DialogEvent::Pending
    );
    assert!(dialog.checked(0));
    assert_eq!(dialog.focus, Focus::Check(0));
    press_on(&mut dialog, Press::Click, "*");
    assert_eq!(dialog.focus, Focus::Field(0));
    dialog.handle(Resolved::Insert('x'));
    assert_eq!(
        dialog.text(),
        "x*",
        "the text stays and the cursor is where clicked"
    );
    assert_eq!(
        press_on(&mut dialog, Press::DoubleClick, "Cancel"),
        DialogEvent::Pending,
        "the first click pressed it"
    );
    assert_eq!(
        press_on(&mut dialog, Press::Click, "Cancel"),
        DialogEvent::Pressed(Button::Cancel)
    );
    assert_eq!(
        press_on(&mut dialog, Press::Click, "OK"),
        DialogEvent::Pressed(Button::Ok)
    );
}
