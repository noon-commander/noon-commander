use super::*;

fn lettered() -> Dialog {
    let checks = [
        ("&Files only".to_owned(), false),
        ("&Case sensitive".to_owned(), false),
    ];
    Dialog::form("Select", "&Pattern:", "*", &checks)
        .with_width(50)
        .with_combo_box("&Number:", &["O&ne".to_owned(), "T&wo".to_owned()], 0)
}

#[test]
fn alt_and_a_letter_switch_open_focus_and_press_from_anywhere() {
    let mut dialog = lettered();
    assert_eq!(
        dialog.focus,
        Focus::Field(0),
        "the field first, as forms open"
    );
    let hotkey = |dialog: &mut Dialog, c| dialog.handle(Resolved::Hotkey(c));
    assert_eq!(hotkey(&mut dialog, 'f'), DialogEvent::Pending);
    assert!(dialog.checked(0));
    assert_eq!(dialog.focus, Focus::Check(0));
    hotkey(&mut dialog, 'N');
    assert_eq!(
        (dialog.focus, dialog.context()),
        (Focus::ComboBox(0), Context::ComboBox),
        "either case opens the list"
    );
    assert_eq!(
        hotkey(&mut dialog, 'o'),
        DialogEvent::Pending,
        "the list's letters alone, not OK's"
    );
    assert_eq!(dialog.context(), Context::ComboBox);
    hotkey(&mut dialog, 'W');
    assert_eq!((dialog.chosen(0), dialog.context()), (1, Context::Dialog));
    hotkey(&mut dialog, 'p');
    assert_eq!(
        dialog.focus,
        Focus::Field(0),
        "the prompt's letter is the field's"
    );
    assert_eq!(dialog.mode(), Mode::Insert);
    typed(&mut dialog, "x");
    assert_eq!(dialog.text(), "*x");
    hotkey(&mut dialog, 'c');
    assert!(dialog.checked(1), "from the field too");
    hotkey(&mut dialog, 'c');
    assert!(!dialog.checked(1), "again, back");
    hotkey(&mut dialog, 'z');
    assert_eq!(
        dialog.focus,
        Focus::Check(1),
        "a letter nothing has does nothing"
    );
    assert_eq!(dialog.text(), "*x");
    assert_eq!(hotkey(&mut dialog, 'o'), DialogEvent::Pressed(Button::Ok));
    assert_eq!(Button::Cancel.label().letter(), None, "Esc and F10 cancel");
}

#[test]
fn a_letter_in_normal_mode_gives_up_the_command_that_waits() {
    let mut dialog = lettered();
    dialog.handle(action(Action::NormalMode));
    dialog.handle(action(Action::DeleteOperator));
    assert!(dialog.pending().is_some());
    dialog.handle(Resolved::Hotkey('f'));
    assert_eq!(dialog.pending(), None);
    dialog.handle(Resolved::Hotkey('p'));
    assert_eq!(
        dialog.mode(),
        Mode::Insert,
        "a letter brings Insert mode, as Tab"
    );
}

#[test]
fn letters_stand_out_where_they_are_drawn() {
    use ratatui::style::Color;

    let mut dialog = lettered();
    dialog.handle(Resolved::Hotkey('f'));
    let terminal = draw_themed(&dialog, 50, 14, &Theme::mc_classic());
    let buffer = terminal.backend().buffer();
    let find = |text: &str| crate::tui::mouse::find(buffer, text);
    let fg = |at: Position, offset: u16| buffer[(at.x + offset, at.y)].fg;
    let colors = Colors::of(&Theme::mc_classic(), Tone::Plain);
    assert_eq!(colors.hotkey_style(false).fg, Some(Color::Blue));
    let prompt = find("Pattern:");
    assert_eq!((fg(prompt, 0), fg(prompt, 1)), (Color::Blue, Color::Black));
    let combo = find("Number:");
    assert_eq!((fg(combo, 0), fg(combo, 1)), (Color::Blue, Color::Black));
    let choice = find("One");
    assert_eq!(fg(choice, 1), Color::Black, "its letter shows in its list");
    let check = find("Files only");
    assert_eq!(fg(check, 0), Color::Blue, "on the focus too");
    assert_eq!(buffer[(check.x, check.y)].bg, Color::Cyan);
    let ok = find("OK");
    assert_eq!((fg(ok, 0), fg(ok, 1)), (Color::Blue, Color::Black));
    let cancel = find("Cancel");
    assert_eq!(fg(cancel, 0), Color::Black);
    let text: String = (0..buffer.area.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
        .map(|at| buffer[at].symbol())
        .collect();
    assert!(!text.contains('&'), "{text}");
}

/// Asserts that what `dialog` acts on by a letter has one of its own: its combo boxes, its
/// labelled fields, its check boxes, and its buttons but Cancel; and each choice of a combo
/// box one of its own in the list.
fn assert_letters(name: &str, dialog: &Dialog) {
    for labelled in &dialog.combo_boxes {
        let letters = labelled.combo.letters();
        let unique: std::collections::HashSet<char> = letters.iter().copied().collect();
        assert_eq!(unique.len(), letters.len(), "{name}: {letters:?}");
    }
    let labels = dialog
        .combo_boxes
        .iter()
        .map(|labelled| &labelled.label)
        .chain(dialog.fields.iter().map(|field| &field.label))
        .chain(dialog.checks.iter().map(|check| &check.label));
    let missing: Vec<&str> = labels
        .filter(|label| !label.text.is_empty() && label.letter().is_none())
        .map(|label| label.text.as_str())
        .collect();
    assert!(missing.is_empty(), "{name}: {missing:?} have no letter");
    for &button in &dialog.buttons {
        let none = button.label().letter().is_none();
        assert_eq!(none, button == Button::Cancel, "{name}: {button:?}");
    }
    let letters = dialog.letters();
    let unique: std::collections::HashSet<char> = letters.iter().copied().collect();
    assert_eq!(unique.len(), letters.len(), "{name}: {letters:?}");
}

#[test]
fn every_element_of_the_transfer_dialog_has_a_letter_of_its_own() {
    use crate::tui::transfer::{self, Kind, Sources};

    let one = Sources::One("R&&D".to_owned());
    let many = Sources::Many { files: 1, dirs: 1 };
    for kind in [Kind::Copy, Kind::Move] {
        for sources in [&one, &many] {
            let dialog = transfer::dialog(kind, sources, ("", 0), true);
            let name = format!("{kind:?} {sources:?}");
            assert!(dialog.message.letter().is_some(), "{name}: the prompt");
            assert_letters(&name, &dialog);
        }
    }
}

#[test]
fn the_buttons_of_every_question_have_a_letter_each() {
    let questions = [
        vec![Button::Yes, Button::No],
        vec![Button::Retry, Button::Skip, Button::Abort],
        vec![Button::Retry, Button::Up, Button::GoTo, Button::Cancel],
        vec![Button::Overwrite, Button::Rename, Button::Cancel],
        vec![Button::Connect, Button::Local, Button::Cancel],
        vec![Button::Connect, Button::Cancel],
        vec![Button::Change, Button::Cancel],
    ];
    for buttons in questions {
        let name = format!("{buttons:?}");
        let dialog = Dialog::question("Title", "?", buttons, 0, Tone::Error);
        assert_letters(&name, &dialog);
    }
}

#[test]
fn every_element_of_the_apps_dialogs_has_a_letter_of_its_own() {
    use crate::i18n::fl;

    let ok_cancel = || vec![Button::Ok, Button::Cancel];
    let form =
        |message: String, checks: &[(String, bool)]| Dialog::form("Title", &message, "", checks);
    let forms = [
        ("mkdir", form(fl!("mkdir-prompt"), &[])),
        ("go to directory", form(fl!("goto-prompt"), &[])),
        ("save a file", form(fl!("save-prompt"), &[])),
        ("save workspace", form(fl!("workspaces-save-prompt"), &[])),
        (
            "rename workspace",
            form(fl!("workspaces-rename-prompt", name = "x"), &[]),
        ),
    ];
    for (name, dialog) in &forms {
        assert!(dialog.message.letter().is_some(), "{name}: the prompt");
        assert_letters(name, dialog);
    }
    let select = [(fl!("pattern-files-only"), false)];
    let host_fields = [
        (fl!("host-label"), String::new()),
        (fl!("host-start-dir"), String::new()),
        (fl!("host-other-dir"), String::new()),
    ];
    let host_buttons = vec![Button::Ok, Button::UseCurrent, Button::Cancel];
    let algorithms = [
        fl!("checksum-sha256"),
        fl!("checksum-sha512"),
        fl!("checksum-sha1"),
        fl!("checksum-md5"),
        fl!("checksum-blake3"),
    ];
    let question = |buttons: Vec<Button>| Dialog::question("Title", "?", buttons, 0, Tone::Plain);
    let dialogs = [
        (
            "select",
            Dialog::form("Select", &fl!("pattern-text"), "*", &select).with_width(50),
        ),
        (
            "host",
            Dialog::fields(
                "Host",
                &host_fields,
                &[(fl!("host-remember-dir"), false)],
                host_buttons,
            ),
        ),
        (
            "checksum",
            Dialog::fields("Checksum", &[], &[], ok_cancel())
                .with_message(&fl!("checksum-one", name = "x"))
                .with_combo_box(&fl!("checksum-algorithm"), &algorithms, 0)
                .with_separator()
                .with_field(&fl!("checksum-expected"), "")
                .with_check(&fl!("checksum-compare", path = "/x"), false),
        ),
        ("password", secret()),
        (
            "host key",
            Dialog::prompt("web", "yes?", PromptKind::HostKey),
        ),
        (
            "question",
            Dialog::prompt("web", "yes?", PromptKind::Question),
        ),
        (
            "file exists",
            question(vec![
                Button::Overwrite,
                Button::Skip,
                Button::Rename,
                Button::Abort,
            ])
            .with_item(&fl!("copy-exists-new"), "1 byte")
            .with_item(&fl!("copy-exists-existing"), "2 bytes")
            .with_combo_box(&fl!("copy-exists-rest"), &[fl!("copy-exists-ask")], 0),
        ),
        ("error", Dialog::error("No.")),
    ];
    for (name, dialog) in &dialogs {
        assert_letters(name, dialog);
    }
}
