use super::*;

/// An editor on `text` in Normal mode, with the cursor at `cursor`.
fn at(text: &str, cursor: usize) -> Editor {
    let mut editor = Editor::new(text, Mode::Normal);
    editor.set_cursor(cursor);
    editor
}

fn press(editor: &mut Editor, registers: &mut Registers, action: Action) -> bool {
    editor.handle(action, Extra::default(), registers)
}

/// Runs `actions`, each with `extra`, which a later one replaces.
fn run(editor: &mut Editor, registers: &mut Registers, actions: &[(Action, Extra)]) {
    for &(action, extra) in actions {
        assert!(editor.handle(action, extra, registers), "{action:?}");
    }
}

fn plain(action: Action) -> (Action, Extra) {
    (action, Extra::default())
}

fn counted(action: Action, count: usize) -> (Action, Extra) {
    (
        action,
        Extra {
            count: Some(count),
            ..Extra::default()
        },
    )
}

fn char(action: Action, c: char) -> (Action, Extra) {
    (
        action,
        Extra {
            char: Some(c),
            ..Extra::default()
        },
    )
}

fn line_register(text: &str) -> Register {
    Register {
        text: text.to_owned(),
        lines: true,
    }
}

fn text_register(text: &str) -> Register {
    Register {
        text: text.to_owned(),
        lines: false,
    }
}

#[test]
fn motions_move_by_characters_words_and_finds() {
    let mut registers = Registers::default();
    let mut editor = at("foo.bar baz  qux", 0);
    let mut go = |action: (Action, Extra)| {
        run(&mut editor, &mut registers, &[action]);
        editor.cursor()
    };
    assert_eq!(
        go(plain(Action::WordForward)),
        3,
        "the dot is a word of its own"
    );
    assert_eq!(go(plain(Action::WordForward)), 4);
    assert_eq!(go(plain(Action::BigWordForward)), 8);
    assert_eq!(go(plain(Action::WordEnd)), 10);
    assert_eq!(go(counted(Action::WordBackward, 2)), 4);
    assert_eq!(go(plain(Action::BigWordBackward)), 0);
    assert_eq!(go(plain(Action::BigWordEnd)), 6);
    assert_eq!(go(plain(Action::TextEnd)), 15, "on the last character");
    assert_eq!(go(plain(Action::Right)), 15);
    assert_eq!(go(plain(Action::TextStart)), 0);
    assert_eq!(go(char(Action::FindChar, 'a')), 5);
    assert_eq!(go(plain(Action::RepeatFind)), 9);
    assert_eq!(go(plain(Action::RepeatFindBack)), 5);
    assert_eq!(go(char(Action::TillChar, 'z')), 9);
    assert_eq!(go(char(Action::FindCharBack, 'o')), 2);
    assert_eq!(go(char(Action::TillCharBack, 'f')), 1);
    assert_eq!(
        go(char(Action::FindChar, '!')),
        1,
        "nothing found: it stays"
    );
    assert_eq!(go(counted(Action::Right, 3)), 4);
    assert_eq!(go(plain(Action::WordForward)), 8);
    assert_eq!(go(plain(Action::FirstNonBlank)), 0);
    let mut indented = at("  ls", 3);
    run(
        &mut indented,
        &mut registers,
        &[plain(Action::FirstNonBlank)],
    );
    assert_eq!(indented.cursor(), 2);
}

#[test]
fn operators_act_on_what_the_motion_moves_over() {
    let mut registers = Registers::default();
    let mut editor = at("one two three", 0);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::DeleteOperator), plain(Action::WordForward)],
    );
    assert_eq!(editor.text(), "two three");
    assert_eq!(registers.text(None), Some("one "));
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::ChangeOperator), plain(Action::WordForward)],
    );
    assert_eq!(editor.text(), " three", "cw changes to the end of the word");
    assert_eq!(editor.mode(), Mode::Insert);
    editor.insert('2');
    run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
    assert_eq!(editor.text(), "2 three");
    assert_eq!(editor.cursor(), 0);
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::YankOperator),
            char(Action::FindChar, 'h'),
            plain(Action::TextEnd),
            plain(Action::PutAfter),
        ],
    );
    assert_eq!(editor.text(), "2 three2 th");
    // 2d3l deletes six characters.
    let mut editor = at("abcdefgh", 0);
    run(
        &mut editor,
        &mut registers,
        &[
            counted(Action::DeleteOperator, 2),
            counted(Action::Right, 3),
        ],
    );
    assert_eq!(editor.text(), "gh");
    // Another key gives the operator up.
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::DeleteOperator)],
    );
    assert_eq!(editor.pending().as_deref(), Some("d"));
    assert!(press(&mut editor, &mut registers, Action::Cancel));
    assert_eq!(editor.pending(), None);
    assert!(
        !press(&mut editor, &mut registers, Action::Cancel),
        "Esc is the field's"
    );
    assert_eq!(editor.text(), "gh");
}

#[test]
fn doubled_operators_act_on_lines() {
    let mut registers = Registers::default();
    let mut field = at("rename me", 3);
    run(
        &mut field,
        &mut registers,
        &[plain(Action::YankOperator), plain(Action::YankOperator)],
    );
    assert_eq!(registers.text(None), Some("rename me"));
    run(
        &mut field,
        &mut registers,
        &[plain(Action::ChangeOperator), plain(Action::ChangeOperator)],
    );
    assert_eq!(field.text(), "");
    assert_eq!(field.mode(), Mode::Insert);
    run(&mut field, &mut registers, &[plain(Action::NormalMode)]);
    run(&mut field, &mut registers, &[plain(Action::PutAfter)]);
    assert_eq!(
        field.text(),
        "rename me",
        "a line goes in as text in a field"
    );
    let mut lines = at("one\ntwo\nthree", 5).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[plain(Action::DeleteOperator), plain(Action::DeleteOperator)],
    );
    assert_eq!(lines.text(), "one\nthree");
    assert_eq!(lines.cursor(), 4);
    run(&mut lines, &mut registers, &[plain(Action::PutBefore)]);
    assert_eq!(lines.text(), "one\ntwo\nthree");
    assert_eq!(lines.cursor(), 4);
    lines.set_cursor(0);
    run(
        &mut lines,
        &mut registers,
        &[
            counted(Action::DeleteOperator, 2),
            plain(Action::DeleteOperator),
        ],
    );
    assert_eq!(lines.text(), "three");
    let mut last = at("one\ntwo", 5).with_lines();
    run(
        &mut last,
        &mut registers,
        &[plain(Action::DeleteOperator), plain(Action::DeleteOperator)],
    );
    assert_eq!(last.text(), "one", "the break before the last line goes");
}

#[test]
fn changes_of_normal_mode() {
    let mut registers = Registers::default();
    let mut editor = at("hello world", 0);
    run(
        &mut editor,
        &mut registers,
        &[counted(Action::DeleteChar, 2)],
    );
    assert_eq!(editor.text(), "llo world");
    run(
        &mut editor,
        &mut registers,
        &[counted(Action::ToggleCase, 3)],
    );
    assert_eq!(editor.text(), "LLO world");
    assert_eq!(editor.cursor(), 3);
    run(&mut editor, &mut registers, &[plain(Action::Backspace)]);
    assert_eq!(editor.text(), "LL world");
    run(
        &mut editor,
        &mut registers,
        &[char(Action::ReplaceChar, '_')],
    );
    assert_eq!(editor.text(), "LL_world");
    run(&mut editor, &mut registers, &[plain(Action::DeleteToEnd)]);
    assert_eq!(editor.text(), "LL");
    assert_eq!(editor.cursor(), 1);
    run(&mut editor, &mut registers, &[plain(Action::Substitute)]);
    assert_eq!((editor.text(), editor.mode()), ("L", Mode::Insert));
    run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::SubstituteLine)],
    );
    assert_eq!(editor.text(), "");
    let mut editor = at("a b", 0);
    run(&mut editor, &mut registers, &[plain(Action::AppendAtEnd)]);
    assert_eq!((editor.cursor(), editor.mode()), (3, Mode::Insert));
    run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
    assert_eq!(editor.cursor(), 2, "Esc moves back onto the last character");
    run(&mut editor, &mut registers, &[plain(Action::ChangeToEnd)]);
    assert_eq!(editor.text(), "a ");
    run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
    run(&mut editor, &mut registers, &[plain(Action::InsertAtStart)]);
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn undo_takes_back_a_change_and_all_one_insert_typed() {
    let mut registers = Registers::default();
    let mut editor = Editor::new("abc", Mode::Insert);
    editor.insert('d');
    editor.insert('e');
    run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
    run(&mut editor, &mut registers, &[plain(Action::DeleteChar)]);
    assert_eq!(editor.text(), "abcd");
    run(&mut editor, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(editor.text(), "abcde");
    run(&mut editor, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(editor.text(), "abc", "what Insert mode typed, at once");
    run(&mut editor, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(editor.text(), "abc", "nothing more");
    run(&mut editor, &mut registers, &[counted(Action::Redo, 2)]);
    assert_eq!(editor.text(), "abcd");
    // A change operator and what is typed after it are one change too.
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::TextStart),
            plain(Action::ChangeOperator),
            plain(Action::WordEnd),
        ],
    );
    editor.insert('x');
    run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
    assert_eq!(editor.text(), "x");
    run(&mut editor, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(editor.text(), "abcd");
}

#[test]
fn visual_mode_selects_and_operators_act_on_the_selection() {
    let mut registers = Registers::default();
    let mut editor = at("copy this text", 5);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::VisualMode), plain(Action::WordEnd)],
    );
    assert_eq!(editor.selection(), Some(5..9));
    run(&mut editor, &mut registers, &[plain(Action::OtherEnd)]);
    assert_eq!(editor.cursor(), 5);
    assert_eq!(editor.selection(), Some(5..9));
    run(&mut editor, &mut registers, &[plain(Action::YankOperator)]);
    assert_eq!(registers.text(None), Some("this"));
    assert_eq!(editor.mode(), Mode::Normal);
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::VisualMode),
            plain(Action::TextEnd),
            plain(Action::ChangeOperator),
        ],
    );
    assert_eq!((editor.text(), editor.mode()), ("copy ", Mode::Insert));
    let mut cased = at("abc", 0);
    run(
        &mut cased,
        &mut registers,
        &[
            plain(Action::VisualMode),
            plain(Action::Right),
            plain(Action::ToggleCase),
        ],
    );
    assert_eq!(cased.text(), "ABc");
    assert_eq!(cased.mode(), Mode::Normal);
}

#[test]
fn registers_are_named_and_shared() {
    let mut registers = Registers::default();
    let named = |name| Extra {
        register: Some(name),
        ..Extra::default()
    };
    let mut one = at("alpha beta", 0);
    run(
        &mut one,
        &mut registers,
        &[
            (Action::YankOperator, named('a')),
            plain(Action::WordEnd),
            plain(Action::WordForward),
            (Action::YankOperator, named('A')),
            plain(Action::WordEnd),
        ],
    );
    assert_eq!(registers.text(Some('a')), Some("alphabeta"));
    assert_eq!(registers.text(Some('b')), None);
    let mut other = Editor::new("", Mode::Insert);
    run(
        &mut other,
        &mut registers,
        &[char(Action::PasteRegister, 'a')],
    );
    assert_eq!(other.text(), "alphabeta");
    run(&mut other, &mut registers, &[plain(Action::NormalMode)]);
    run(
        &mut other,
        &mut registers,
        &[(Action::PutBefore, named('a'))],
    );
    assert_eq!(other.text(), "alphabetalphabetaa");
    // A yank appended leaves the whole register in the unnamed one, and a delete only what
    // it deleted, as vim's register 1 holds it.
    assert_eq!(registers.text(None), Some("alphabeta"));
    let mut deletes = at("one two", 0);
    run(
        &mut deletes,
        &mut registers,
        &[
            (Action::DeleteChar, named('a')),
            (Action::DeleteChar, named('A')),
        ],
    );
    assert_eq!(registers.text(Some('a')), Some("on"));
    assert_eq!(registers.text(None), Some("n"));
}

#[test]
fn insert_mode_edits_around_the_cursor() {
    let mut registers = Registers::default();
    let mut editor = Editor::new("echo one two", Mode::Insert);
    run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(editor.text(), "echo one ");
    run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(editor.text(), "echo ");
    run(&mut editor, &mut registers, &[plain(Action::WordBackward)]);
    assert_eq!(editor.cursor(), 0);
    run(&mut editor, &mut registers, &[plain(Action::WordForward)]);
    assert_eq!(editor.cursor(), 5);
    run(&mut editor, &mut registers, &[plain(Action::DeleteToStart)]);
    assert_eq!(editor.text(), "");
    // Ctrl+o: one command of Normal mode, then Insert mode again.
    let mut editor = Editor::new("one two", Mode::Insert);
    run(&mut editor, &mut registers, &[plain(Action::NormalOnce)]);
    assert_eq!(editor.mode(), Mode::Normal);
    run(&mut editor, &mut registers, &[plain(Action::WordBackward)]);
    assert_eq!((editor.cursor(), editor.mode()), (4, Mode::Insert));
    // A secret has no Normal mode.
    let mut secret = Editor::secret(16);
    assert!(!press(&mut secret, &mut registers, Action::NormalMode));
    secret.set_mode(Mode::Normal);
    assert_eq!(secret.mode(), Mode::Insert);
}

#[test]
fn a_secret_never_grows_its_memory() {
    let mut secret = Editor::secret(4);
    for c in "secret".chars() {
        secret.insert(c);
    }
    assert_eq!(secret.text(), "secr");
    assert_eq!(format!("{secret:?}"), "Editor { mode: Insert, .. }");
    let capacity = secret.text.capacity();
    secret.replace("a much longer text than it can hold", 0);
    secret.insert_str("more than it can hold");
    assert_eq!((secret.text(), secret.text.capacity()), ("secr", capacity));
    // The characters it reads its text as never grow either, so no copy is left behind.
    let chars = secret.chars();
    assert!(chars.capacity() >= chars.len() && chars.capacity() <= secret.text.len());
}

#[test]
fn cw_ends_the_word_under_the_cursor_where_it_is_on_its_last_character() {
    let mut registers = Registers::default();
    let change_word = |text: &str, cursor: usize, count: usize| {
        let mut editor = at(text, cursor);
        let mut registers = Registers::default();
        run(
            &mut editor,
            &mut registers,
            &[
                plain(Action::ChangeOperator),
                counted(Action::WordForward, count),
            ],
        );
        editor.text().to_owned()
    };
    assert_eq!(change_word("a b", 0, 1), " b", "a word of one letter");
    assert_eq!(change_word("ab cd", 1, 1), "a cd", "on its last letter");
    assert_eq!(change_word("a.b", 1, 1), "ab", "a dot is a word");
    assert_eq!(change_word("one two", 0, 1), " two");
    assert_eq!(change_word("a b c", 0, 2), " c", "c2w");
    assert_eq!(change_word("one  two", 3, 1), "onetwo", "on blanks, as dw");
    let mut big = at("a.b c", 0);
    run(
        &mut big,
        &mut registers,
        &[plain(Action::ChangeOperator), plain(Action::BigWordForward)],
    );
    assert_eq!(big.text(), " c");
}

#[test]
fn ctrl_o_runs_one_command_whatever_mode_it_leaves() {
    let mut registers = Registers::default();
    // A command that goes to Insert mode itself leaves nothing behind.
    let mut editor = Editor::new("one two", Mode::Insert);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalOnce), plain(Action::AppendAtEnd)],
    );
    assert_eq!(editor.mode(), Mode::Insert);
    run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
    assert_eq!(editor.mode(), Mode::Normal, "Esc goes to Normal mode again");
    // Esc right after Ctrl+o goes back to Insert mode, as in vim, rather than to the owner.
    let mut editor = Editor::new("one two", Mode::Insert);
    run(&mut editor, &mut registers, &[plain(Action::NormalOnce)]);
    assert!(press(&mut editor, &mut registers, Action::Cancel));
    assert_eq!(editor.mode(), Mode::Insert);
    assert!(
        !press(&mut editor, &mut registers, Action::Cancel),
        "then the owner's"
    );
    // An operator after Ctrl+o, then Esc, gives the operator up first.
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalOnce), plain(Action::DeleteOperator)],
    );
    assert!(press(&mut editor, &mut registers, Action::Cancel));
    assert_eq!((editor.mode(), editor.text()), (Mode::Insert, "one two"));
    // Ctrl+o $ goes after the last character.
    let mut editor = at("one two", 0);
    run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalOnce), plain(Action::TextEnd)],
    );
    assert_eq!((editor.mode(), editor.cursor()), (Mode::Insert, 7));
    // What the keymap gives up, such as Ctrl+o and a key that does nothing, ends it too.
    run(&mut editor, &mut registers, &[plain(Action::NormalOnce)]);
    editor.give_up();
    assert_eq!(editor.mode(), Mode::Insert);
    let mut pending = at("abc def", 0);
    run(
        &mut pending,
        &mut registers,
        &[plain(Action::DeleteOperator)],
    );
    pending.give_up();
    run(&mut pending, &mut registers, &[plain(Action::WordForward)]);
    assert_eq!(pending.text(), "abc def", "d and Esc in the keymap, then w");
}

#[test]
fn counts_go_as_far_as_text_goes_and_lines_stay_lines() {
    let mut registers = Registers::default();
    // A count of words stops at a line's end only in the last word.
    let mut lines = at("a\nb\nc d", 0).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[
            plain(Action::DeleteOperator),
            counted(Action::WordForward, 3),
        ],
    );
    assert_eq!(lines.text(), "d");
    // Lines put three times stay three lines.
    let mut lines = at("one\ntwo", 0).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[
            plain(Action::YankOperator),
            plain(Action::YankOperator),
            counted(Action::PutAfter, 3),
        ],
    );
    assert_eq!(lines.text(), "one\none\none\none\ntwo");
    let mut field = at("x", 0);
    run(&mut field, &mut registers, &[counted(Action::PutAfter, 2)]);
    assert_eq!(
        field.text(),
        "xone one",
        "lines in a field, apart by spaces"
    );
    // However large, a count stops where the motion can go no further.
    let mut editor = at("one two three", 0);
    let huge = Extra {
        count: Some(MAX_COUNT),
        ..Extra::default()
    };
    run(
        &mut editor,
        &mut registers,
        &[(Action::DeleteOperator, huge), (Action::WordForward, huge)],
    );
    assert_eq!(editor.text(), "");
}

#[test]
fn an_empty_line_is_a_word_for_w_and_b() {
    let mut registers = Registers::default();
    let mut lines = at("ab\n\ncd\n\n\nef", 0).with_lines();
    let mut go = |action: Action| {
        run(&mut lines, &mut registers, &[plain(action)]);
        lines.cursor()
    };
    assert_eq!(go(Action::WordForward), 3, "the empty line");
    assert_eq!(go(Action::WordForward), 4, "cd");
    assert_eq!(go(Action::WordForward), 7, "the first of two empty lines");
    assert_eq!(go(Action::WordForward), 8, "the second");
    assert_eq!(go(Action::WordForward), 9, "ef");
    assert_eq!(go(Action::WordBackward), 8);
    assert_eq!(go(Action::WordBackward), 7);
    assert_eq!(go(Action::WordBackward), 4);
    assert_eq!(go(Action::WordBackward), 3);
    assert_eq!(go(Action::WordBackward), 0);
    assert_eq!(go(Action::BigWordForward), 3, "for WORDs too");
    // A line with blanks is no empty line.
    let mut blanks = at("ab\n  \ncd", 0).with_lines();
    run(&mut blanks, &mut registers, &[plain(Action::WordForward)]);
    assert_eq!(blanks.cursor(), 6);
}

#[test]
fn c_and_d_take_a_count_of_lines_as_dollar_does() {
    let mut registers = Registers::default();
    let mut lines = at("one\ntwo\nthree", 1).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[counted(Action::DeleteToEnd, 2)],
    );
    assert_eq!(lines.text(), "o\nthree");
    assert_eq!(registers.text(None), Some("ne\ntwo"));
    let mut lines = at("one\ntwo", 1).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[counted(Action::ChangeToEnd, 5)],
    );
    assert_eq!(
        (lines.text(), lines.mode()),
        ("o", Mode::Insert),
        "as far as there are lines"
    );
    // From the last line, more lines than one go nowhere, as in vim: in a field, always.
    let mut field = at("one two", 4);
    run(
        &mut field,
        &mut registers,
        &[counted(Action::DeleteToEnd, 3)],
    );
    assert_eq!(field.text(), "one two");
    let mut last = at("one\ntwo", 5).with_lines();
    run(
        &mut last,
        &mut registers,
        &[
            counted(Action::DeleteOperator, 2),
            plain(Action::DeleteOperator),
        ],
    );
    assert_eq!(last.text(), "one\ntwo", "2dd");
    run(&mut last, &mut registers, &[counted(Action::TextEnd, 2)]);
    assert_eq!(last.cursor(), 5, "2$ stays");
    last.set_cursor(0);
    run(
        &mut last,
        &mut registers,
        &[
            counted(Action::DeleteOperator, 9),
            plain(Action::DeleteOperator),
        ],
    );
    assert_eq!(
        last.text(),
        "",
        "9dd from the first line deletes to the end"
    );
}

#[test]
fn toggling_case_keeps_the_characters_and_changes_nothing_else() {
    let mut registers = Registers::default();
    let mut editor = at("ßab", 0);
    run(
        &mut editor,
        &mut registers,
        &[counted(Action::ToggleCase, 2)],
    );
    assert_eq!((editor.text(), editor.cursor()), ("ßAb", 2));
    let mut digits = at("123", 0);
    run(
        &mut digits,
        &mut registers,
        &[counted(Action::ToggleCase, 3)],
    );
    run(&mut digits, &mut registers, &[plain(Action::DeleteChar)]);
    run(&mut digits, &mut registers, &[plain(Action::Undo)]);
    run(&mut digits, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(
        digits.text(),
        "123",
        "the toggle that changed nothing is no change"
    );
}

#[test]
fn a_change_that_deletes_nothing_is_one_with_what_it_types() {
    let mut registers = Registers::default();
    let mut editor = at("ab", 0);
    run(&mut editor, &mut registers, &[plain(Action::DeleteChar)]);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::ChangeOperator), plain(Action::Left)],
    );
    assert_eq!(editor.mode(), Mode::Insert);
    editor.insert('x');
    run(&mut editor, &mut registers, &[plain(Action::NormalMode)]);
    assert_eq!(editor.text(), "xb");
    run(&mut editor, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(editor.text(), "b", "c h and what it typed, one change");
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::ChangeOperator),
            plain(Action::Left),
            plain(Action::NormalMode),
            plain(Action::Undo),
        ],
    );
    assert_eq!(editor.text(), "b", "c h alone is a change to undo too");
    run(&mut editor, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(editor.text(), "ab");
}

#[test]
fn deletes_of_nothing_are_changes_to_undo_but_where_the_text_has_no_line() {
    let empty_then_undo = |keys: &[(Action, Extra)], cursor: usize| {
        let mut editor = at("abc", cursor);
        let mut registers = Registers::default();
        run(&mut editor, &mut registers, keys);
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        editor.text().to_owned()
    };
    // Emptied by D, the line is still there: x on it takes a step of its own to undo.
    for (keys, cursor, text) in [
        (
            &[plain(Action::DeleteToEnd), plain(Action::DeleteChar)][..],
            0,
            "",
        ),
        (
            &[plain(Action::DeleteToEnd), plain(Action::Backspace)],
            0,
            "",
        ),
        (
            &[
                plain(Action::DeleteToEnd),
                plain(Action::VisualMode),
                plain(Action::DeleteOperator),
            ],
            0,
            "",
        ),
        (
            &[
                plain(Action::DeleteChar),
                plain(Action::DeleteOperator),
                plain(Action::TextStart),
            ],
            0,
            "bc",
        ),
        (
            &[
                plain(Action::DeleteChar),
                plain(Action::DeleteOperator),
                plain(Action::Left),
            ],
            0,
            "bc",
        ),
        // D on an empty line has nothing to delete, and is no change.
        (
            &[plain(Action::DeleteToEnd), plain(Action::DeleteToEnd)],
            0,
            "abc",
        ),
        // d d deleted the last line: nothing is left to delete, and undo brings it back.
        (
            &[
                plain(Action::DeleteOperator),
                plain(Action::DeleteOperator),
                plain(Action::DeleteChar),
            ],
            0,
            "abc",
        ),
        // C on an empty line, and v ~ even where the text has no line, are changes.
        (
            &[
                plain(Action::DeleteToEnd),
                plain(Action::ChangeToEnd),
                plain(Action::NormalMode),
            ],
            0,
            "",
        ),
        (
            &[
                plain(Action::DeleteOperator),
                plain(Action::DeleteOperator),
                plain(Action::VisualMode),
                plain(Action::ToggleCase),
            ],
            0,
            "",
        ),
    ] {
        assert_eq!(empty_then_undo(keys, cursor), text, "{keys:?}");
    }
}

#[test]
fn a_put_of_nothing_and_delete_at_the_end_are_changes_to_undo_too() {
    let mut registers = Registers::default();
    // A put from an empty register, as vim's, which saves for undo first.
    let mut editor = at("abc", 1);
    let empty = Extra {
        register: Some('b'),
        ..Extra::default()
    };
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::DeleteChar),
            (Action::PutBefore, empty),
            plain(Action::Undo),
        ],
    );
    assert_eq!(editor.text(), "ac");
    // Delete at the end of a field in Insert mode is a change too.
    let mut editor = at("abc", 1);
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::DeleteToEnd),
            plain(Action::Append),
            plain(Action::DeleteChar),
            plain(Action::NormalMode),
            plain(Action::Undo),
        ],
    );
    assert_eq!(editor.text(), "a");
}

#[test]
fn what_there_is_nothing_to_take_from_goes_to_the_registers_as_vims() {
    let named = |name| Extra {
        register: Some(name),
        ..Extra::default()
    };
    let mut registers = Registers::default();
    let mut editor = at("abc", 0);
    run(
        &mut editor,
        &mut registers,
        &[
            (Action::YankOperator, named('a')),
            plain(Action::YankOperator),
            plain(Action::YankOperator),
            plain(Action::TextStart),
        ],
    );
    assert_eq!(
        registers.text(None),
        Some(""),
        "y 0 at the start yanks nothing"
    );
    assert_eq!(registers.text(Some('a')), Some("abc"));
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::DeleteToEnd),
            (Action::YankOperator, named('A')),
            plain(Action::TextEnd),
        ],
    );
    assert_eq!(
        registers.text(Some('a')),
        Some("abc\n"),
        "\"A y $ appends an empty line to lines"
    );
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::YankOperator), plain(Action::YankOperator)],
    );
    run(&mut editor, &mut registers, &[plain(Action::ChangeToEnd)]);
    assert_eq!(
        registers.get(None).cloned(),
        Some(text_register("")),
        "C on an empty line takes it"
    );
    // d d on the empty line takes it as a line, then the text has none: d d, c c, S, and C
    // take nothing more.
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::NormalMode),
            plain(Action::DeleteOperator),
            plain(Action::DeleteOperator),
        ],
    );
    assert_eq!(registers.get(None).cloned(), Some(line_register("")));
    assert!(editor.no_line);
    let kept = line_register("abc");
    for keys in [
        &[plain(Action::DeleteOperator), plain(Action::DeleteOperator)][..],
        &[
            (Action::ChangeOperator, named('a')),
            plain(Action::ChangeOperator),
            plain(Action::NormalMode),
        ],
        &[plain(Action::SubstituteLine), plain(Action::NormalMode)],
        &[plain(Action::ChangeToEnd), plain(Action::NormalMode)],
    ] {
        let mut editor = at("abc", 0);
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::DeleteOperator), plain(Action::DeleteOperator)],
        );
        registers.keep(Some('a'), kept.clone(), false);
        run(&mut editor, &mut registers, keys);
        assert_eq!(registers.get(None).cloned(), Some(kept.clone()), "{keys:?}");
        assert_eq!(
            registers.get(Some('a')).cloned(),
            Some(kept.clone()),
            "{keys:?}"
        );
    }
}

#[test]
fn visual_mode_goes_after_the_end_of_a_line_and_takes_its_line_break() {
    let visual = |text: &str, cursor: usize, keys: &[(Action, Extra)]| {
        let mut editor = at(text, cursor).with_lines();
        let mut registers = Registers::default();
        run(&mut editor, &mut registers, &[plain(Action::VisualMode)]);
        run(&mut editor, &mut registers, keys);
        (editor, registers)
    };
    let (editor, _) = visual("abc", 2, &[plain(Action::Right)]);
    assert_eq!(editor.cursor(), 3, "l from the last character");
    let (editor, _) = visual(
        "abc",
        2,
        &[
            plain(Action::Right),
            plain(Action::Left),
            plain(Action::DeleteOperator),
        ],
    );
    assert_eq!(editor.text(), "ab", "h comes back to the last character");
    let (_, registers) = visual(
        "abc",
        0,
        &[
            plain(Action::TextEnd),
            plain(Action::Left),
            plain(Action::YankOperator),
        ],
    );
    assert_eq!(registers.text(None), Some("abc"), "$ goes after the end");
    let (editor, _) = visual(
        "a.b c-d  ef",
        9,
        &[
            plain(Action::WordForward),
            plain(Action::Left),
            plain(Action::DeleteOperator),
        ],
    );
    assert_eq!(editor.text(), "a.b c-d  ", "w past the last word");
    let (editor, _) = visual("ab cd", 4, &[plain(Action::WordEnd)]);
    assert_eq!(editor.cursor(), 5, "e at the end");
    let (editor, _) = visual("ab\ncd", 1, &[plain(Action::CharForward)]);
    assert_eq!(editor.cursor(), 2, "Space at the last character");
    let (editor, _) = visual("ab\ncd", 1, &[counted(Action::CharForward, 2)]);
    assert_eq!(editor.cursor(), 3, "then the next line");
    let (editor, _) = visual("ab\ncd", 3, &[plain(Action::CharBackward)]);
    assert_eq!(
        editor.cursor(),
        2,
        "Backspace to the end of the line before"
    );
    // An operator takes the line break where a line follows, and nothing on the last line.
    let (editor, registers) = visual(
        "ab cd\nef",
        1,
        &[plain(Action::TextEnd), plain(Action::YankOperator)],
    );
    assert_eq!(registers.text(None), Some("b cd\n"));
    assert_eq!((editor.mode(), editor.cursor()), (Mode::Normal, 1));
    let (editor, _) = visual(
        "ab cd\nef",
        1,
        &[counted(Action::Right, 9), plain(Action::DeleteOperator)],
    );
    assert_eq!(editor.text(), "aef");
    let (editor, _) = visual(
        "abc",
        1,
        &[plain(Action::TextEnd), plain(Action::NormalMode)],
    );
    assert_eq!(
        editor.cursor(),
        2,
        "Esc puts the cursor back on a character"
    );
    // A count before v goes after the end as l does.
    let mut registers = Registers::default();
    let mut editor = at("ab", 1);
    run(
        &mut editor,
        &mut registers,
        &[
            counted(Action::VisualMode, 5),
            plain(Action::YankOperator),
            counted(Action::VisualMode, 1),
        ],
    );
    assert_eq!(editor.cursor(), 2, "the column after the end counts too");
}

#[test]
fn after_ctrl_o_at_the_end_ctrl_w_and_ctrl_u_stop_once_before_the_last_character() {
    let mut registers = Registers::default();
    let yanked = |typed: &str, keys: &[Action]| {
        let mut editor = at("foo bar", 0);
        let mut registers = Registers::default();
        press(&mut editor, &mut registers, Action::AppendAtEnd);
        for c in typed.chars() {
            editor.insert(c);
        }
        let line = [
            Action::NormalOnce,
            Action::YankOperator,
            Action::YankOperator,
        ];
        for &action in line.iter().chain(keys) {
            press(&mut editor, &mut registers, action);
        }
        editor.text().to_owned()
    };
    assert_eq!(yanked("", &[Action::DeleteWord]), "foo ba");
    assert_eq!(yanked("", &[Action::DeleteToStart]), "foo ba");
    assert_eq!(yanked("xy", &[Action::DeleteWord]), "foo barx");
    assert_eq!(
        yanked("", &[Action::DeleteWord, Action::DeleteWord]),
        "foo "
    );
    assert_eq!(yanked("", &[Action::Backspace, Action::Backspace]), "foo b");
    // On an empty line too, as vim's.
    let mut editor = Editor::new("", Mode::Insert);
    registers.keep(None, text_register("x"), false);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalOnce), plain(Action::PutBefore)],
    );
    assert_eq!((editor.text(), editor.cursor()), ("x", 1));
}

#[test]
fn minus_plus_and_underscore_go_to_lines_and_take_them_whole() {
    let text = "ab\n  cd\nef";
    let go = |cursor: usize, keys: &[(Action, Extra)]| {
        let mut editor = at(text, cursor).with_lines();
        let mut registers = Registers::default();
        run(&mut editor, &mut registers, keys);
        let taken = registers.get(None).cloned().unwrap_or_default();
        (editor.text().to_owned(), editor.cursor(), taken)
    };
    let moved = |cursor, keys: &[(Action, Extra)]| go(cursor, keys).1;
    assert_eq!(moved(4, &[plain(Action::PrevLine)]), 0);
    assert_eq!(moved(4, &[plain(Action::NextLine)]), 8);
    assert_eq!(moved(3, &[plain(Action::ThisLine)]), 5);
    assert_eq!(moved(4, &[counted(Action::ThisLine, 2)]), 8);
    assert_eq!(
        moved(0, &[counted(Action::NextLine, 5)]),
        8,
        "as far as lines go"
    );
    assert_eq!(moved(8, &[counted(Action::PrevLine, 3)]), 0);
    assert_eq!(moved(1, &[plain(Action::PrevLine)]), 1, "no line above");
    assert_eq!(moved(8, &[plain(Action::NextLine)]), 8, "no line below");
    assert_eq!(moved(8, &[counted(Action::ThisLine, 2)]), 8);
    let delete = |motion| (Action::DeleteOperator, Extra::default(), motion);
    for ((operator, extra, motion), cursor, want) in [
        (delete(plain(Action::ThisLine)), 4, ("ab\nef", 3, "  cd")),
        (
            delete(counted(Action::ThisLine, 2)),
            4,
            ("ab", 0, "  cd\nef"),
        ),
        (delete(plain(Action::PrevLine)), 4, ("ef", 0, "ab\n  cd")),
        (delete(plain(Action::NextLine)), 4, ("ab", 0, "  cd\nef")),
        (delete(plain(Action::NextLine)), 8, (text, 8, "")),
    ] {
        let got = go(cursor, &[(operator, extra), motion]);
        assert_eq!(
            (got.0.as_str(), got.1, got.2.text.as_str()),
            want,
            "{motion:?}"
        );
        assert!(got.2.lines || got.2.text.is_empty(), "{motion:?}");
    }
    let (_, cursor, taken) = go(4, &[plain(Action::YankOperator), plain(Action::PrevLine)]);
    assert_eq!(
        (cursor, taken),
        (0, line_register("ab\n  cd")),
        "y - goes up"
    );
    let (_, cursor, _) = go(4, &[plain(Action::YankOperator), plain(Action::NextLine)]);
    assert_eq!(cursor, 4, "y + stays");
    let mut editor = at(text, 4).with_lines();
    let mut registers = Registers::default();
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::ChangeOperator), plain(Action::PrevLine)],
    );
    editor.insert('x');
    assert_eq!(editor.text(), "x\nef", "c - as 2 c c from the line above");
    // In a field of one line, - and + fail, and _ stays on the line.
    let mut field = at("  ab", 3);
    run(&mut field, &mut registers, &[plain(Action::PrevLine)]);
    assert_eq!(field.cursor(), 3);
    run(&mut field, &mut registers, &[plain(Action::ThisLine)]);
    assert_eq!(field.cursor(), 2);
    run(
        &mut field,
        &mut registers,
        &[plain(Action::DeleteOperator), plain(Action::ThisLine)],
    );
    assert_eq!(field.text(), "", "d _ as d d");
}

#[test]
fn the_black_hole_register_keeps_nothing() {
    let mut registers = Registers::default();
    let mut editor = at("keep gone", 0);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::YankOperator), plain(Action::WordEnd)],
    );
    let black_hole = Extra {
        register: Some('_'),
        ..Extra::default()
    };
    run(
        &mut editor,
        &mut registers,
        &[(Action::DeleteToEnd, black_hole)],
    );
    assert_eq!(registers.text(None), Some("keep"));
    assert_eq!(registers.text(Some('_')), None);
}

#[test]
fn a_paste_is_one_change_in_any_mode_as_in_vim() {
    let mut registers = Registers::default();
    // In Normal mode as typed in Insert mode, then Esc: after the cursor, but at the start
    // of a line before it; the cursor goes to its last character.
    let mut editor = at("ab", 1);
    editor.paste("xyz", &mut registers);
    assert_eq!((editor.text(), editor.mode()), ("abxyz", Mode::Normal));
    assert_eq!(editor.cursor(), 4);
    run(&mut editor, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(
        (editor.text(), editor.cursor()),
        ("ab", 1),
        "where the insert started"
    );
    let mut editor = at("ab", 0);
    editor.paste("xyz", &mut registers);
    assert_eq!((editor.text(), editor.cursor()), ("xyzab", 2));
    let mut empty = Editor::new("", Mode::Normal);
    empty.paste("xy", &mut registers);
    assert_eq!(empty.cursor(), 1, "on the last character, not after it");
    let mut lines = at("ab\ncd", 0).with_lines();
    lines.paste("x\n", &mut registers);
    assert_eq!(
        (lines.text(), lines.cursor()),
        ("x\nab\ncd", 2),
        "Esc stays at a line start"
    );
    // After Ctrl+o, Normal mode stays; an operator that waits is given up, and the paste.
    let mut once = after_ctrl_o("ab");
    once.paste("x", &mut registers);
    assert_eq!((once.text(), once.mode()), ("abx", Mode::Normal));
    let mut waits = at("ab", 0);
    run(&mut waits, &mut registers, &[plain(Action::DeleteOperator)]);
    waits.paste("x", &mut registers);
    assert_eq!(waits.text(), "ab");
    assert!(waits.pending().is_none());
    // In Visual mode in place of the selection, which the unnamed register keeps.
    let mut selected = at("abc def", 0);
    run(
        &mut selected,
        &mut registers,
        &[plain(Action::VisualMode), plain(Action::Right)],
    );
    selected.paste("xyz", &mut registers);
    assert_eq!(
        (selected.text(), selected.mode()),
        ("xyzc def", Mode::Normal)
    );
    assert_eq!(registers.text(None), Some("ab"));
    run(&mut selected, &mut registers, &[plain(Action::Undo)]);
    assert_eq!(selected.text(), "abc def", "one change");
    // In Insert mode at the cursor.
    let mut typing = Editor::new("ab", Mode::Insert);
    typing.paste("x", &mut registers);
    assert_eq!((typing.text(), typing.cursor()), ("abx", 3));
}
/// An editor in Insert mode on `text`, the cursor at its end, after Ctrl+o.
fn after_ctrl_o(text: &str) -> Editor {
    let mut editor = Editor::new(text, Mode::Insert);
    assert!(editor.handle(
        Action::NormalOnce,
        Extra::default(),
        &mut Registers::default()
    ));
    editor
}

#[test]
fn after_ctrl_o_dollar_ctrl_w_and_ctrl_u_go_on_from_the_end_as_vims() {
    let mut registers = Registers::default();
    let after = |start: Action, end: Action, keys: &[Action]| {
        let mut editor = at("foo bar baz", 0);
        let mut registers = Registers::default();
        for &action in [start, Action::NormalOnce, end].iter().chain(keys) {
            press(&mut editor, &mut registers, action);
        }
        editor.text().to_owned()
    };
    // vim's `$` after Ctrl+o puts the cursor after the end, where the insert starts.
    let (append, insert) = (Action::AppendAtEnd, Action::InsertMode);
    assert_eq!(
        after(append, Action::TextEnd, &[Action::DeleteWord]),
        "foo bar "
    );
    assert_eq!(
        after(insert, Action::TextEnd, &[Action::DeleteWord]),
        "foo bar "
    );
    assert_eq!(after(append, Action::TextEnd, &[Action::DeleteToStart]), "");
    let twice = [Action::DeleteWord, Action::DeleteWord];
    assert_eq!(after(insert, Action::TextEnd, &twice), "foo ");
    // A `$` that fails leaves it before the last character, as the end of the line does.
    let mut editor = after_ctrl_o("foo bar baz");
    run(&mut editor, &mut registers, &[counted(Action::TextEnd, 2)]);
    run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(editor.text(), "foo bar ba");
}

#[test]
fn after_ctrl_o_backspace_stops_after_the_end_of_the_line_above_as_vims() {
    let typed = |text: &str, cursor: usize, keys: &[(Action, Extra)]| {
        let mut editor = at(text, 0).with_lines();
        editor.set_cursor(cursor);
        let mut registers = Registers::default();
        run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
        run(&mut editor, &mut registers, &[plain(Action::NormalOnce)]);
        run(&mut editor, &mut registers, keys);
        editor.insert('Q');
        let register = registers.get(None).map(|r| r.text.clone());
        (editor.text().to_owned(), register)
    };
    let back = |n| counted(Action::CharBackward, n);
    assert_eq!(typed("ab cd\nef", 6, &[back(1)]).0, "ab cdQ\nef");
    // The end of the line counts as a place to stop, as in Visual mode.
    assert_eq!(typed("ab cd\nef", 6, &[back(3)]).0, "ab Qcd\nef");
    assert_eq!(typed("ab\ncd\nef", 6, &[back(3)]).0, "ab\nQcd\nef");
    assert_eq!(typed("ab\n\nef", 4, &[back(2)]).0, "abQ\n\nef");
    // `y` onto the line above takes nothing, and the insert goes on after the end.
    let yank = plain(Action::YankOperator);
    let yanked = typed("ab cd\nef", 6, &[yank, back(1)]);
    assert_eq!(yanked, ("ab cdQ\nef".to_owned(), Some(String::new())));
}

#[test]
fn after_ctrl_o_0_and_home_go_on_before_the_last_character_as_vim() {
    let mut registers = Registers::default();
    for (text, key) in [
        ("x", Action::TextStart),
        ("x", Action::FirstNonBlank),
        (" x", Action::FirstNonBlank),
    ] {
        let mut editor = after_ctrl_o(text);
        run(&mut editor, &mut registers, &[plain(key)]);
        editor.insert('Q');
        assert_eq!(
            editor.text(),
            format!("{}Qx", &text[..text.len() - 1]),
            "{key:?}"
        );
    }
    // Another key that leaves the cursor on the last character goes on after it.
    let mut editor = after_ctrl_o("x");
    run(&mut editor, &mut registers, &[plain(Action::WordBackward)]);
    editor.insert('Q');
    assert_eq!(editor.text(), "xQ");
}

#[test]
fn home_and_end_start_a_new_change_where_the_cursor_is_already_as_vim() {
    let mut registers = Registers::default();
    let mut editor = at("foo", 0);
    run(&mut editor, &mut registers, &[plain(Action::AppendAtEnd)]);
    editor.insert('X');
    run(&mut editor, &mut registers, &[plain(Action::TextEnd)]);
    editor.insert('Y');
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalMode), plain(Action::Undo)],
    );
    assert_eq!(editor.text(), "fooX");
    // Ctrl+w still stops where the insert started, as after a move to the right.
    let mut editor = at("foo", 0);
    run(&mut editor, &mut registers, &[plain(Action::AppendAtEnd)]);
    editor.insert('X');
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::TextEnd), plain(Action::DeleteWord)],
    );
    assert_eq!(editor.text(), "foo");
}

#[test]
fn a_count_before_v_after_a_selection_to_the_end_selects_to_the_end_as_vims() {
    let mut registers = Registers::default();
    let mut editor = at("abc def\nxy zw", 0).with_lines();
    editor.set_mode(Mode::Normal);
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::TextEnd),
            plain(Action::VisualMode),
            plain(Action::YankOperator),
        ],
    );
    editor.set_cursor(8);
    run(
        &mut editor,
        &mut registers,
        &[counted(Action::VisualMode, 1), plain(Action::YankOperator)],
    );
    assert_eq!(registers.get(None).map(|r| r.text.as_str()), Some("xy zw"));
}

#[test]
fn a_count_before_v_selects_as_much_as_the_last_visual_command_times_it() {
    let mut registers = Registers::default();
    let mut editor = at("abcdefgh", 1);
    run(
        &mut editor,
        &mut registers,
        &[
            counted(Action::VisualMode, 2),
            plain(Action::DeleteOperator),
        ],
    );
    assert_eq!(editor.text(), "adefgh", "without one, that many characters");
    let mut editor = at("abcdefgh abcdefgh", 0);
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::VisualMode),
            counted(Action::Right, 2),
            plain(Action::YankOperator),
            plain(Action::WordForward),
            counted(Action::VisualMode, 2),
        ],
    );
    assert_eq!(editor.selection(), Some(9..15), "three characters, twice");
    // Without a count, one character.
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalMode), plain(Action::VisualMode)],
    );
    assert_eq!(editor.selection(), Some(14..15));
}

#[test]
fn ctrl_o_at_the_end_of_the_line_acts_on_its_last_character_as_vim() {
    type Case<'a> = (&'a [(Action, Extra)], &'a str, usize);
    let mut registers = Registers::default();
    let cases: [Case<'_>; 7] = [
        (&[plain(Action::DeleteChar)], "ab", 2),
        (&[plain(Action::DeleteToEnd)], "ab", 2),
        (&[plain(Action::ToggleCase)], "abC", 3),
        (&[char(Action::ReplaceChar, 'X')], "abX", 3),
        (&[plain(Action::Backspace)], "ac", 2),
        (&[plain(Action::Left)], "abc", 1),
        (&[plain(Action::Right)], "abc", 3),
    ];
    for (keys, text, cursor) in cases {
        let mut editor = after_ctrl_o("abc");
        run(&mut editor, &mut registers, keys);
        assert_eq!(editor.mode(), Mode::Insert, "{keys:?}");
        assert_eq!((editor.text(), editor.cursor()), (text, cursor), "{keys:?}");
    }
    let mut editor = after_ctrl_o("abc");
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::DeleteOperator), plain(Action::WordBackward)],
    );
    assert_eq!((editor.text(), editor.cursor()), ("c", 1), "d b");
    let mut editor = after_ctrl_o("abc");
    run(
        &mut editor,
        &mut registers,
        &[char(Action::Substitute, 'X')],
    );
    editor.insert('X');
    assert_eq!(editor.text(), "abX", "s");
    let mut editor = after_ctrl_o("abc def");
    run(&mut editor, &mut registers, &[plain(Action::WordEnd)]);
    assert_eq!(
        editor.cursor(),
        7,
        "e stays on the last character, then after it"
    );
    let mut editor = after_ctrl_o("abc def");
    run(
        &mut editor,
        &mut registers,
        &[char(Action::FindCharBack, 'f')],
    );
    assert_eq!(editor.cursor(), 7, "F f finds no other f");
    // From inside a line, the cursor stays where the command leaves it.
    let mut editor = Editor::new("abc", Mode::Insert);
    editor.set_cursor(1);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalOnce), plain(Action::Right)],
    );
    assert_eq!(editor.cursor(), 2);
}

#[test]
fn an_empty_line_is_put_and_appended_as_a_line() {
    let mut registers = Registers::default();
    let mut lines = at("ab\n\ncd", 3).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[
            plain(Action::YankOperator),
            plain(Action::YankOperator),
            plain(Action::PutAfter),
        ],
    );
    assert_eq!(lines.text(), "ab\n\n\ncd", "y y p");
    run(&mut lines, &mut registers, &[counted(Action::PutAfter, 2)]);
    assert_eq!(lines.text(), "ab\n\n\n\n\ncd", "2p");
    let append = |name| Extra {
        register: Some(name),
        ..Extra::default()
    };
    let mut lines = at("\nfoo", 0).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[
            (Action::YankOperator, append('a')),
            plain(Action::YankOperator),
        ],
    );
    lines.set_cursor(1);
    run(
        &mut lines,
        &mut registers,
        &[
            (Action::YankOperator, append('A')),
            plain(Action::YankOperator),
        ],
    );
    assert_eq!(registers.text(Some('a')), Some("\nfoo"), "both lines");
    // In a field of one line, an empty line puts nothing.
    let mut field = at("x", 0);
    run(
        &mut field,
        &mut registers,
        &[(Action::PutAfter, append('z'))],
    );
    assert_eq!(field.text(), "x");
}

#[test]
fn w_with_an_operator_takes_an_empty_line_whole() {
    let mut registers = Registers::default();
    let mut lines = at("ab\n\ncd", 3).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[plain(Action::DeleteOperator), plain(Action::WordForward)],
    );
    assert_eq!(lines.text(), "ab\ncd");
    assert_eq!(registers.text(None), Some(""), "an empty line");
    let mut lines = at("ab\n\ncd", 3).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[plain(Action::ChangeOperator), plain(Action::WordForward)],
    );
    assert_eq!((lines.text(), lines.mode()), ("ab\n\ncd", Mode::Insert));
}

#[test]
fn ctrl_w_and_ctrl_u_stop_once_where_insert_mode_started() {
    let mut registers = Registers::default();
    let mut field = Editor::new("report.txt", Mode::Insert);
    field.set_cursor(6);
    for c in "_v2".chars() {
        field.insert(c);
    }
    run(&mut field, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(field.text(), "report.txt", "what was typed");
    run(&mut field, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(field.text(), ".txt", "then on");
    let mut field = Editor::new("foo bar", Mode::Insert);
    field.insert('x');
    run(&mut field, &mut registers, &[plain(Action::DeleteToStart)]);
    assert_eq!(field.text(), "foo bar");
    run(&mut field, &mut registers, &[plain(Action::DeleteToStart)]);
    assert_eq!(field.text(), "", "a field opens ready to be cleared");
    // On the command line, as on vim's, they do not stop; Ctrl+u at the start of a line
    // joins it to the line above, as Ctrl+w does.
    let mut lines = Editor::new("ab\ncd", Mode::Insert).with_lines();
    lines.insert('x');
    run(&mut lines, &mut registers, &[plain(Action::DeleteToStart)]);
    assert_eq!(lines.text(), "ab\n");
    run(&mut lines, &mut registers, &[plain(Action::DeleteToStart)]);
    assert_eq!(lines.text(), "ab");
}

#[test]
fn moves_in_insert_mode_and_ctrl_u_start_a_new_change_and_redo_keeps_the_cursor() {
    let mut registers = Registers::default();
    let mut editor = at("abc", 0);
    run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
    editor.insert('x');
    run(&mut editor, &mut registers, &[plain(Action::Left)]);
    editor.insert('y');
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalMode), plain(Action::Undo)],
    );
    assert_eq!(editor.text(), "xabc", "the Left split the change");
    let mut editor = at("abc", 0);
    run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
    editor.insert('x');
    run(&mut editor, &mut registers, &[plain(Action::DeleteToStart)]);
    editor.insert('y');
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalMode), plain(Action::Undo)],
    );
    assert_eq!(
        editor.text(),
        "xabc",
        "Ctrl+u too, as vim's defaults.vim maps it"
    );
    let mut editor = at("abc", 0);
    run(&mut editor, &mut registers, &[plain(Action::InsertMode)]);
    editor.insert('x');
    run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
    editor.insert('y');
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalMode), plain(Action::Undo)],
    );
    assert_eq!(editor.text(), "abc", "but not Ctrl+w");
    // The focus coming back starts a new change.
    let mut editor = Editor::new("", Mode::Insert);
    editor.insert('a');
    editor.set_mode(Mode::Insert);
    editor.insert('b');
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::NormalMode), plain(Action::Undo)],
    );
    assert_eq!(editor.text(), "a");
    let mut editor = at("one two three", 4);
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::DeleteOperator),
            plain(Action::WordForward),
            plain(Action::TextStart),
            plain(Action::Undo),
            plain(Action::Redo),
        ],
    );
    assert_eq!((editor.text(), editor.cursor()), ("one three", 4));
}

#[test]
fn the_operator_waiting_shows_as_typed_and_visual_mode_its_size() {
    let mut registers = Registers::default();
    let register = |outer: Option<usize>, count| Extra {
        count: Some(count),
        register: Some('a'),
        outer,
        ..Extra::default()
    };
    let mut editor = at("abc", 0);
    run(
        &mut editor,
        &mut registers,
        &[(Action::DeleteOperator, register(Some(2), 2))],
    );
    assert_eq!(editor.pending().as_deref(), Some("2\"ad"));
    editor.give_up();
    run(
        &mut editor,
        &mut registers,
        &[(Action::DeleteOperator, register(None, 2))],
    );
    assert_eq!(editor.pending().as_deref(), Some("\"a2d"));
    editor.give_up();
    run(
        &mut editor,
        &mut registers,
        &[(Action::DeleteOperator, register(Some(2), 6))],
    );
    assert_eq!(editor.pending().as_deref(), Some("2\"a3d"));
    editor.give_up();
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::VisualMode), plain(Action::Right)],
    );
    assert_eq!(editor.selection_size(), Some(2), "characters");
    let mut lines = at("ab\ncd\nef", 1).with_lines();
    run(
        &mut lines,
        &mut registers,
        &[plain(Action::VisualMode), char(Action::FindChar, 'x')],
    );
    lines.set_cursor(7);
    assert_eq!(lines.selection_size(), Some(3), "lines");
    lines.set_cursor(2);
    assert_eq!(lines.selection_size(), Some(2), "and the end of the line");
}
#[test]
fn undo_and_redo_put_the_cursor_where_the_change_starts() {
    type Case<'a> = (&'a str, usize, &'a [(Action, Extra)], usize);
    let mut registers = Registers::default();
    let cases: [Case<'_>; 6] = [
        (
            "foo bar",
            5,
            &[plain(Action::DeleteOperator), plain(Action::WordBackward)],
            4,
        ),
        ("abcdef", 3, &[plain(Action::Backspace)], 2),
        (
            "abcdef",
            3,
            &[plain(Action::DeleteOperator), plain(Action::TextStart)],
            0,
        ),
        (
            "abcdef",
            3,
            &[
                plain(Action::DeleteOperator),
                char(Action::FindCharBack, 'a'),
            ],
            0,
        ),
        (
            "abcdef",
            0,
            &[
                plain(Action::VisualMode),
                counted(Action::Right, 2),
                plain(Action::DeleteOperator),
            ],
            0,
        ),
        (
            "  abc",
            3,
            &[plain(Action::SubstituteLine), plain(Action::NormalMode)],
            2,
        ),
    ];
    for (text, cursor, keys, start) in cases {
        let mut editor = at(text, cursor);
        run(&mut editor, &mut registers, keys);
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        assert_eq!((editor.text(), editor.cursor()), (text, start), "{keys:?}");
        run(
            &mut editor,
            &mut registers,
            &[plain(Action::Redo), plain(Action::Undo)],
        );
        assert_eq!(editor.cursor(), start, "{keys:?} again");
    }
    // Visual mode from its right end too.
    let mut editor = at("abcdef", 3);
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::VisualMode),
            counted(Action::Left, 2),
            plain(Action::OtherEnd),
            plain(Action::ToggleCase),
            plain(Action::Undo),
        ],
    );
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn undo_and_redo_of_lines_put_the_cursor_where_vim_does() {
    let mut registers = Registers::default();
    let undone = |text: &str, cursor: usize, keys: &[(Action, Extra)], redo: bool| {
        let mut editor = at(text, 0).with_lines();
        editor.set_cursor(cursor);
        let mut registers = Registers::default();
        run(&mut editor, &mut registers, keys);
        run(&mut editor, &mut registers, &[plain(Action::Undo)]);
        if redo {
            run(&mut editor, &mut registers, &[plain(Action::Redo)]);
        }
        (editor.text().to_owned(), editor.cursor())
    };
    let dd = [plain(Action::DeleteOperator), plain(Action::DeleteOperator)];
    let cc = [
        plain(Action::ChangeOperator),
        plain(Action::ChangeOperator),
        plain(Action::NormalMode),
    ];
    // On one line, the first character that is not blank, or the cursor before it.
    assert_eq!(
        undone("  ab cd\nef", 5, &dd, false),
        ("  ab cd\nef".to_owned(), 2)
    );
    assert_eq!(undone("  ab cd\nef", 1, &dd, false).1, 1);
    assert_eq!(undone("  ab cd\nef", 5, &cc, false).1, 2);
    assert_eq!(undone("  ab cd\nef", 0, &cc, false).1, 0);
    // On more lines, the cursor.
    let two_dd = [
        plain(Action::DeleteOperator),
        counted(Action::DeleteOperator, 2),
    ];
    assert_eq!(undone("  ab cd\nef", 5, &two_dd, false).1, 5);
    // Redo keeps the line and the column, and from a line that is gone goes to the first
    // character of the last one that is not blank.
    assert_eq!(undone("  ab cd\nef", 5, &cc, true), ("\nef".to_owned(), 0));
    assert_eq!(
        undone("ab cd\nef gh\nij", 13, &dd, true),
        ("ab cd\nef gh".to_owned(), 6)
    );
    assert_eq!(
        undone("ab cd\nef gh\nij", 8, &two_dd, true),
        ("ab cd".to_owned(), 0)
    );
    assert_eq!(undone("  ab\n  cd", 5, &dd, true), ("  ab".to_owned(), 2));
    // A change within a line: the same column, on the same line.
    let mut editor = at("abc\ndef", 6).with_lines();
    run(
        &mut editor,
        &mut registers,
        &[
            plain(Action::DeleteChar),
            plain(Action::Undo),
            plain(Action::Redo),
        ],
    );
    assert_eq!((editor.text(), editor.cursor()), ("abc\nde", 5));
}

#[test]
fn ctrl_o_goes_back_to_insert_mode_after_the_text_put_or_the_end_deleted_to() {
    type Case<'a> = (usize, &'a [(Action, Extra)], &'a str, usize);
    let mut registers = Registers::default();
    let mut x = Registers::default();
    x.keep(
        None,
        Register {
            text: "X".to_owned(),
            lines: false,
        },
        false,
    );
    let cases: [Case<'_>; 6] = [
        (2, &[plain(Action::DeleteToEnd)], "ab", 2),
        (3, &[plain(Action::DeleteChar)], "abc", 3),
        (
            2,
            &[plain(Action::DeleteOperator), plain(Action::WordForward)],
            "ab",
            2,
        ),
        (3, &[plain(Action::ToggleCase)], "abcD", 4),
        (2, &[plain(Action::PutAfter)], "abcXd", 4),
        (2, &[plain(Action::PutBefore)], "abXcd", 3),
    ];
    for (cursor, keys, text, after) in cases {
        let mut editor = Editor::new("abcd", Mode::Insert);
        editor.set_cursor(cursor);
        let registers = if keys[0].0 == Action::PutAfter || keys[0].0 == Action::PutBefore {
            &mut x
        } else {
            &mut registers
        };
        assert!(editor.handle(Action::NormalOnce, Extra::default(), registers));
        run(&mut editor, registers, keys);
        assert_eq!(editor.mode(), Mode::Insert, "{keys:?}");
        assert_eq!((editor.text(), editor.cursor()), (text, after), "{keys:?}");
    }
    // From the end of the line, P puts before the last character, and the cursor goes back
    // to the end.
    let mut editor = Editor::new("ab", Mode::Insert);
    run(
        &mut editor,
        &mut x,
        &[plain(Action::NormalOnce), plain(Action::PutBefore)],
    );
    assert_eq!((editor.text(), editor.cursor()), ("aXb", 3));
}

#[test]
fn a_count_on_semicolon_after_t_counts_the_character_it_stopped_before() {
    let mut registers = Registers::default();
    let mut editor = at("a.b a.b a.b", 0);
    run(
        &mut editor,
        &mut registers,
        &[char(Action::TillChar, 'a'), counted(Action::RepeatFind, 2)],
    );
    assert_eq!(
        editor.cursor(),
        7,
        "before the second a after it, as vim's 2;"
    );
    let mut editor = at("a.b a.b a.b", 0);
    run(
        &mut editor,
        &mut registers,
        &[char(Action::TillChar, 'a'), plain(Action::RepeatFind)],
    );
    assert_eq!(editor.cursor(), 7, "one ; skips the a it stopped before");
}

#[test]
fn ctrl_w_and_ctrl_u_after_a_move_stop_where_the_insert_started_as_vims() {
    let mut registers = Registers::default();
    let typed = |text: &str, keys: &[(Action, Extra)], typed: &str| {
        let mut editor = Editor::new(text, Mode::Insert);
        let mut registers = Registers::default();
        for c in typed.chars() {
            editor.insert(c);
        }
        for &(action, extra) in keys {
            assert!(editor.handle(action, extra, &mut registers));
        }
        editor
    };
    // A, XY, Left, Ctrl+w: what was typed before the cursor only.
    let editor = typed(
        "foo bar",
        &[plain(Action::Left), plain(Action::DeleteWord)],
        "XY",
    );
    assert_eq!(editor.text(), "foo barY");
    // A move to the right after typing keeps them stopping where Insert mode started.
    let mut editor = typed("foo bar", &[plain(Action::Left)], "XY");
    editor.insert('Z');
    run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(editor.text(), "foo barY");
    // A move with nothing typed starts the insert where it goes.
    let mut editor = typed("foo bar", &[plain(Action::TextStart)], "");
    run(&mut editor, &mut registers, &[plain(Action::TextEnd)]);
    editor.insert('Z');
    run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(editor.text(), "foo bar");
    // Backspace before where they stop does not move it for what is typed next.
    let mut editor = typed("foo", &[plain(Action::Backspace)], "");
    editor.insert('x');
    run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(editor.text(), "");
    // After Ctrl+w, a move to the left starts the insert where it goes: Ctrl+w stops there.
    let mut editor = typed(
        "foo bar baz",
        &[
            plain(Action::DeleteWord),
            plain(Action::Left),
            plain(Action::Left),
        ],
        "",
    );
    editor.insert('Z');
    run(&mut editor, &mut registers, &[plain(Action::DeleteWord)]);
    assert_eq!(editor.text(), "foo bar ");
    // After Ctrl+u, which vim's defaults.vim maps after Ctrl+g u, they keep stopping where
    // they did.
    let mut editor = typed(
        "foo bar baz",
        &[
            plain(Action::DeleteToStart),
            plain(Action::Left),
            plain(Action::Left),
        ],
        "XY",
    );
    editor.insert('Z');
    run(&mut editor, &mut registers, &[plain(Action::DeleteToStart)]);
    assert_eq!(editor.text(), "az");
}

#[test]
fn d_w_from_an_empty_line_takes_it_whole_before_the_indent_as_vims() {
    let mut editor = at("one\n\n four", 0).with_lines();
    editor.set_mode(Mode::Normal);
    editor.set_cursor(4);
    let mut registers = Registers::default();
    let d = plain(Action::DeleteOperator);
    run(
        &mut editor,
        &mut registers,
        &[d, plain(Action::WordForward)],
    );
    assert_eq!(editor.text(), "one\n four");
    let register = registers.get(None).cloned();
    assert_eq!(
        register,
        Some(Register {
            text: String::new(),
            lines: true,
        })
    );
}

#[test]
fn backspace_from_the_start_of_a_line_to_the_start_of_another_keeps_lines_as_vims_yank() {
    let lines = |text: &str, cursor: usize, keys: &[(Action, Extra)]| {
        let mut editor = at(text, 0).with_lines();
        editor.set_cursor(cursor);
        let mut registers = Registers::default();
        run(&mut editor, &mut registers, keys);
        let register = registers.get(None).map(|r| (r.text.clone(), r.lines));
        (editor.text().to_owned(), register)
    };
    let line = |text: &str| Some((text.to_owned(), true));
    let chars = |text: &str| Some((text.to_owned(), false));
    let d = plain(Action::DeleteOperator);
    let back = |n| counted(Action::CharBackward, n);
    assert_eq!(lines("x\ny", 2, &[d, back(2)]), ("y".to_owned(), line("x")));
    assert_eq!(
        lines("x\n\ny", 3, &[d, back(3)]),
        ("y".to_owned(), line("x\n"))
    );
    // From within the line above, or onto its line break only, the characters.
    assert_eq!(
        lines("ab\ncd", 3, &[d, back(2)]),
        ("acd".to_owned(), chars("b\n"))
    );
    assert_eq!(
        lines("x\ny", 2, &[d, back(1)]),
        ("xy".to_owned(), chars("\n"))
    );
}

#[test]
fn on_lines_operators_take_what_vims_exclusive_and_d_rules_give() {
    let mut registers = Registers::default();
    let lines = |text: &str, cursor: usize, keys: &[(Action, Extra)]| {
        let mut editor = at(text, 0).with_lines();
        editor.set_cursor(cursor);
        let mut registers = Registers::default();
        run(&mut editor, &mut registers, keys);
        (editor.text().to_owned(), registers.get(None).cloned())
    };
    let line = |text: &str| {
        Some(Register {
            text: text.to_owned(),
            lines: true,
        })
    };
    let chars = |text: &str| {
        Some(Register {
            text: text.to_owned(),
            lines: false,
        })
    };
    let d = plain(Action::DeleteOperator);
    // An exclusive motion that ends at the start of a line ends at the end of the one
    // before.
    assert_eq!(
        lines("ab cd\nef gh", 6, &[d, plain(Action::WordBackward)]),
        ("ab \nef gh".to_owned(), chars("cd"))
    );
    // From the indent, it takes the lines whole.
    assert_eq!(
        lines("ab\ncd", 3, &[d, plain(Action::WordBackward)]),
        ("cd".to_owned(), line("ab"))
    );
    assert_eq!(
        lines("ab\n\ncd", 0, &[d, counted(Action::WordForward, 2)]),
        ("cd".to_owned(), line("ab\n"))
    );
    // A delete over lines from the indent to the end of a line takes them whole.
    assert_eq!(
        lines("ab cd\nef gh\nij", 0, &[d, counted(Action::TextEnd, 2)]),
        ("ij".to_owned(), line("ab cd\nef gh"))
    );
    assert_eq!(
        lines("ab cd\nef gh\nij", 6, &[d, counted(Action::WordForward, 3)]),
        ("ab cd".to_owned(), line("ef gh\nij"))
    );
    assert_eq!(
        lines("ab\n\ncd", 0, &[counted(Action::DeleteToEnd, 2)]),
        ("cd".to_owned(), line("ab\n"))
    );
    // Not from inside a line, and not for c and y.
    assert_eq!(
        lines("ab cd\nef gh\nij", 1, &[d, counted(Action::TextEnd, 2)]),
        ("a\nij".to_owned(), chars("b cd\nef gh"))
    );
    assert_eq!(
        lines(
            "ab cd\nef gh\nij",
            0,
            &[plain(Action::YankOperator), counted(Action::TextEnd, 2)]
        )
        .1,
        chars("ab cd\nef gh")
    );
    // Space and Backspace go on to the next line and back to the one before.
    let mut editor = at("ab cd\nef", 4).with_lines();
    run(&mut editor, &mut registers, &[plain(Action::CharForward)]);
    assert_eq!(editor.cursor(), 6);
    run(
        &mut editor,
        &mut registers,
        &[counted(Action::CharBackward, 2)],
    );
    assert_eq!(editor.cursor(), 3);
    assert_eq!(
        lines("ab cd\nef gh", 4, &[d, counted(Action::CharForward, 3)]),
        ("ab cf gh".to_owned(), chars("d\ne"))
    );
    assert_eq!(
        lines("ab cd\nef gh", 4, &[d, counted(Action::CharForward, 2)]),
        ("ab c\nef gh".to_owned(), chars("d"))
    );
    assert_eq!(
        lines("ab\n\ncd", 3, &[d, plain(Action::CharForward)]),
        ("ab\ncd".to_owned(), line(""))
    );
    assert_eq!(
        lines("ab cd\nef gh", 6, &[d, plain(Action::CharBackward)]),
        ("ab cdef gh".to_owned(), chars("\n"))
    );
    assert_eq!(
        lines(
            "ab cd\nef gh",
            6,
            &[plain(Action::YankOperator), plain(Action::CharBackward)]
        )
        .1,
        chars("d")
    );
    // In a field of one line, they stop at its ends, as h and l.
    let mut field = at("ab", 1);
    run(&mut field, &mut registers, &[plain(Action::CharForward)]);
    assert_eq!(field.cursor(), 1);
    run(&mut field, &mut registers, &[d, plain(Action::CharForward)]);
    assert_eq!(field.text(), "a");
}

#[test]
fn smaller_rules_of_vims_motions_and_changes() {
    let mut registers = Registers::default();
    // c b at the start of the text goes nowhere, and stays in Normal mode.
    let mut editor = at("ab", 0);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::ChangeOperator), plain(Action::WordBackward)],
    );
    assert_eq!(editor.mode(), Mode::Normal);
    // On a line of blanks, ^ is the last of them.
    let mut editor = at("   ", 0);
    run(
        &mut editor,
        &mut registers,
        &[plain(Action::DeleteOperator), plain(Action::FirstNonBlank)],
    );
    assert_eq!(editor.text(), " ");
    // ~ on a character without a case is a change for undo too.
    let mut editor = at("ab cd", 0);
    run(
        &mut editor,
        &mut registers,
        &[
            counted(Action::ToggleCase, 2),
            plain(Action::ToggleCase),
            plain(Action::Undo),
        ],
    );
    assert_eq!(editor.text(), "AB cd");
    // Delete at the end deletes the character before on the command line and in a filter,
    // as on vim's command line, and nothing in a field.
    let mut filter = Editor::new("abc", Mode::Insert);
    assert!(filter.edit(Action::DeleteChar));
    assert_eq!(filter.text(), "ab");
    let mut line = Editor::new("abc", Mode::Insert).with_lines();
    press(&mut line, &mut registers, Action::DeleteChar);
    assert_eq!(line.text(), "ab");
    let mut field = Editor::new("abc", Mode::Insert);
    press(&mut field, &mut registers, Action::DeleteChar);
    assert_eq!(field.text(), "abc");
}
