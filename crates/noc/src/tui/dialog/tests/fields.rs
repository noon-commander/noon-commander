use super::*;

#[test]
fn a_long_field_keeps_its_text_still_while_the_cursor_shows_as_vim() {
    // Off the edge, the cursor goes to the middle, as with vim's 'sidescroll' 0.
    let mut field = Field::plain("abcdefghij");
    assert_eq!(field.visible(5), ("ij".to_owned(), 2));
    field.edit(Action::Left);
    field.edit(Action::Left);
    assert_eq!(field.visible(5), ("ij".to_owned(), 0), "the text stays");
    field.edit(Action::Left);
    assert_eq!(field.visible(5), ("fghij".to_owned(), 2));
    field.editor.set_cursor(0);
    assert_eq!(field.visible(5), ("abcde".to_owned(), 0));
    let wide = Field::plain("文件文件");
    assert_eq!(wide.visible(5), ("件".to_owned(), 2));
    let mut secret = Field::secret();
    for c in "hunter2".chars() {
        secret.insert(c);
    }
    assert_eq!(secret.visible(4), ("**".to_owned(), 2));
}

#[test]
fn ctrl_w_deletes_the_word_before_the_cursor_as_vim_does() {
    let mut field = Field::plain("cp a.txt  backup");
    assert!(field.edit(Action::DeleteWord));
    assert_eq!(field.text(), "cp a.txt  ");
    field.edit(Action::DeleteWord);
    assert_eq!(
        field.text(),
        "cp a.",
        "the spaces go with the word, and a dot is one"
    );
    field.edit(Action::TextStart);
    field.edit(Action::DeleteWord);
    assert_eq!(field.text(), "cp a.", "nothing before the cursor");
}

#[test]
fn word_keys_go_by_what_spaces_part_as_on_vims_command_line() {
    let mut field = Field::plain("cp  a.txt backup");
    assert!(field.edit(Action::WordBackward));
    assert_eq!(field.editor.cursor(), 10, "the start of backup");
    field.edit(Action::WordBackward);
    assert_eq!(field.editor.cursor(), 4, "a.txt, the dot no break");
    field.edit(Action::TextStart);
    field.edit(Action::WordForward);
    assert_eq!(field.editor.cursor(), 2, "on the space after cp");
    field.edit(Action::TextEnd);
    field.edit(Action::WordForward);
    assert_eq!(field.editor.cursor(), 16, "the end stays");
}

#[test]
fn visual_mode_shows_what_it_selects() {
    let parts = |field: &Field, room| field.visible_parts(room);
    let mut registers = Registers::default();
    let mut field = Field::normal("report.pdf");
    field.editor.set_cursor(0);
    for action in [Action::VisualMode, Action::WordEnd] {
        assert!(field.handle(action, Extra::default(), &mut registers));
    }
    assert_eq!(
        parts(&field, 20),
        ([String::new(), "report".to_owned(), ".pdf".to_owned()], 5)
    );
    field.handle(Action::NormalMode, Extra::default(), &mut registers);
    assert_eq!(parts(&field, 20).0[1], "", "nothing is selected any more");
    // After the end of the text, as vim's with 'selection' inclusive.
    for action in [Action::TextStart, Action::VisualMode, Action::TextEnd] {
        assert!(field.handle(action, Extra::default(), &mut registers));
    }
    assert_eq!(
        parts(&field, 20),
        ([String::new(), "report.pdf".to_owned(), String::new()], 10)
    );
}
