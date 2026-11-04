use super::*;

#[test]
fn a_secret_prompt_answers_what_was_typed() {
    let mut dialog = secret();
    assert_eq!(dialog.context(), Context::DialogInput);
    typed(&mut dialog, "pässwd");
    assert_eq!(dialog.text(), "", "a secret is not text");
    assert_eq!(confirm(&mut dialog).as_deref(), Some("pässwd"));
}

#[test]
fn the_secret_field_edits_around_its_cursor() {
    let mut dialog = secret();
    typed(&mut dialog, "abcdef");
    for edit in [
        Action::Left,
        Action::Left,
        Action::Backspace,
        Action::TextStart,
    ] {
        dialog.handle(action(edit));
    }
    typed(&mut dialog, "X");
    dialog.handle(action(Action::DeleteChar));
    dialog.handle(action(Action::TextEnd));
    dialog.handle(action(Action::Backspace));
    assert_eq!(confirm(&mut dialog).as_deref(), Some("Xbce"));

    let mut dialog = secret();
    typed(&mut dialog, "abcdef");
    dialog.handle(action(Action::Left));
    dialog.handle(action(Action::DeleteToEnd));
    dialog.handle(action(Action::Left));
    dialog.handle(action(Action::DeleteToStart));
    assert_eq!(confirm(&mut dialog).as_deref(), Some("e"));
}

#[test]
fn the_secret_field_never_grows_its_memory() {
    let mut dialog = secret();
    typed(&mut dialog, &"x".repeat(SECRET_CAPACITY + 10));
    assert_eq!(confirm(&mut dialog).unwrap().len(), SECRET_CAPACITY);
}

#[test]
fn buttons_take_the_focus_in_turn() {
    let mut dialog = secret();
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.context(), Context::Dialog);
    typed(&mut dialog, "ignored");
    dialog.handle(action(Action::NextField));
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::Cancel)
    );
    assert!(
        dialog
            .answer(DialogEvent::Pressed(Button::Cancel))
            .is_none()
    );
    dialog.handle(action(Action::NextField));
    assert_eq!(dialog.context(), Context::DialogInput, "round to the field");
    assert_eq!(confirm(&mut dialog).as_deref(), Some(""));
    assert_eq!(
        dialog.handle(action(Action::Cancel)),
        DialogEvent::Cancelled
    );
    assert!(dialog.answer(DialogEvent::Cancelled).is_none());
}

#[test]
fn questions_default_to_no() {
    let mut dialog = Dialog::prompt("web", "Continue? (yes/no)", PromptKind::HostKey);
    assert_eq!(dialog.context(), Context::Dialog);
    assert_eq!(confirm(&mut dialog), None);
    dialog.handle(action(Action::Left));
    assert_eq!(confirm(&mut dialog).as_deref(), Some("yes"));

    for kind in [
        PromptKind::HostKey,
        PromptKind::Confirm,
        PromptKind::Question,
    ] {
        let dialog = Dialog::prompt("web", "Continue? (yes/no)", kind);
        let no = dialog.answer(DialogEvent::Pressed(Button::No));
        let said = no.as_ref().map(secrecy::ExposeSecret::expose_secret);
        let expected = (kind == PromptKind::Question).then_some("no");
        assert_eq!(said, expected, "No on {kind:?}");
        assert!(dialog.answer(DialogEvent::Cancelled).is_none(), "{kind:?}");
        let tone = if kind == PromptKind::HostKey {
            Tone::Warning
        } else {
            Tone::Plain
        };
        assert_eq!(dialog.tone(), tone, "{kind:?}");
    }

    let mut notice = Dialog::notice("web", "Confirm user presence for key");
    assert_eq!(
        notice.handle(action(Action::Cancel)),
        DialogEvent::Cancelled
    );
    assert_eq!(confirm(&mut notice), None);
}

#[test]
fn draws_a_secret_prompt_with_the_cursor_in_the_field() {
    let mut dialog = secret();
    typed(&mut dialog, "hunter2");
    let mut terminal = draw(&dialog, 50, 9);
    insta::assert_snapshot!(terminal.backend());
    // After the seven stars: the dialog starts at column 2, its field at column 4.
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        Position::new(11, 3)
    );
}

#[test]
fn mc_classic_draws_gray_dialogs_with_a_shadow() {
    use ratatui::style::{Color, Modifier};

    let dialog = secret();
    let terminal = draw_themed(&dialog, 50, 9, &Theme::mc_classic());
    let buffer = terminal.backend().buffer();
    let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
    // The frame spans columns 2 … 47 and rows 1 … 6, and the margin around it the rest of
    // columns 1 … 48 and rows 0 … 7.
    assert_eq!(colors(2, 1), (Color::Black, Color::Gray), "frame");
    assert_eq!(buffer[(2, 1)].symbol(), "╔");
    let title = title_column(buffer, "web");
    assert_eq!(colors(title, 1), (Color::Blue, Color::Gray), "title");
    assert!(buffer[(title, 1)].modifier.contains(Modifier::BOLD));
    // ` web ` between the corners in columns 2 and 47.
    let (left, right) = (title - 1 - 3, 46 - (title + 3));
    assert!(left.abs_diff(right) <= 1, "centered: {left} and {right}");
    assert_eq!(colors(4, 3), (Color::Black, Color::Cyan), "field");
    for (x, y) in [(1, 0), (1, 3), (48, 3), (10, 0), (10, 7)] {
        assert_eq!(colors(x, y).1, Color::Gray, "margin at {x}, {y}");
        assert_eq!(buffer[(x, y)].symbol(), " ", "margin at {x}, {y}");
    }
    assert_eq!(colors(0, 3).1, Color::Reset, "outside");
    assert_eq!(
        colors(49, 2),
        (Color::DarkGray, Color::Black),
        "shadow to the right"
    );
    assert_eq!(
        colors(10, 8),
        (Color::DarkGray, Color::Black),
        "shadow below"
    );
    assert_eq!(
        colors(2, 8).1,
        Color::Reset,
        "the shadow starts two columns in"
    );
}

#[test]
fn a_message_taller_than_the_screen_scrolls_and_keeps_the_buttons() {
    let lines: Vec<String> = (1..=30).map(|n| format!("line {n}")).collect();
    let mut dialog = Dialog::question("Long", &lines.join("\n"), vec![Button::Ok], 0, Tone::Plain);
    let shown = |dialog: &Dialog| draw(dialog, 40, 12).backend().to_string();
    let top = shown(&dialog);
    assert!(top.contains("line 1 ") && top.contains("{ OK }"), "{top}");
    assert!(top.contains('▲') && top.contains('█'), "{top}");
    assert!(!top.contains("line 30"), "{top}");
    dialog.scroll(Scroll::By(Motion::Down));
    let down = shown(&dialog);
    assert!(
        !down.contains("line 1 ") && down.contains("line 2 "),
        "{down}"
    );
    dialog.scroll(Scroll::To(usize::MAX));
    let end = shown(&dialog);
    assert!(end.contains("line 30") && end.contains("{ OK }"), "{end}");
    let short = Dialog::question("Short", "one line", vec![Button::Ok], 0, Tone::Plain);
    let text = shown(&short);
    assert!(!text.contains('░') && short.bar().is_none(), "{text}");
}

#[test]
fn the_default_button_is_bold_in_braces_with_the_focus_or_without() {
    let mut dialog = Dialog::question(
        "Sure?",
        "Delete it?",
        vec![Button::Yes, Button::No],
        1,
        Tone::Plain,
    );
    let bold = |dialog: &Dialog, label: &str| {
        let terminal = draw(dialog, 40, 8);
        let buffer = terminal.backend().buffer();
        let row = (0..8)
            .find(|&y| {
                let line: String = (0..40).map(|x| buffer[(x, y)].symbol()).collect();
                line.contains(label)
            })
            .unwrap();
        let line: String = (0..40).map(|x| buffer[(x, row)].symbol()).collect();
        let byte = line.find(label).unwrap();
        let start = u16::try_from(line[..byte].chars().count()).unwrap();
        let width = u16::try_from(label.len()).unwrap();
        (start..start + width)
            .map(|x| buffer[(x, row)].modifier.contains(Modifier::BOLD))
            .collect::<Vec<_>>()
    };
    let all = |cells: Vec<bool>, on: bool| cells.iter().all(|&cell| cell == on);
    assert!(all(bold(&dialog, "{ No }"), true), "the default, focused");
    assert!(all(bold(&dialog, "[ Yes ]"), false), "the other");
    dialog.handle(action(Action::Left));
    assert!(all(bold(&dialog, "{ No }"), true), "the default, unfocused");
    assert!(all(bold(&dialog, "[ Yes ]"), false), "the other, focused");
}

#[test]
fn draws_a_host_key_question_wrapped_and_terminal_safe() {
    let message = "The authenticity of host 'web (10.0.0.5)' can't be established.\n\
            ED25519 key fingerprint is: SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU\n\
            This key is not known by any other names.\x1b[2J\n\
            Are you sure you want to continue connecting (yes/no/[fingerprint])? ";
    let dialog = Dialog::prompt("web", message, PromptKind::HostKey);
    insta::assert_snapshot!(draw(&dialog, 60, 14).backend());
}

#[test]
fn lines_of_text_are_cut_short_or_broken_even_and_the_verdict_stands_apart() {
    let hex = "0123456789".repeat(2) + &"abcdef0123".repeat(2);
    let dialog = Dialog::question("Sums", "", vec![Button::Ok], 0, Tone::Plain)
        .with_width(30)
        .with_text(Text::Line("a name far too long for the dialog".to_owned()))
        .with_text(Text::Even(hex.clone()))
        .with_line()
        .with_text(Text::Verdict {
            text: "Same.".to_owned(),
            good: true,
        });
    let screen = draw(&dialog, 40, 14).backend().to_string();
    let lines: Vec<&str> = screen.lines().collect();
    let row = |text: &str| lines.iter().position(|line| line.contains(text)).unwrap();
    assert!(screen.contains("a name far to»r the dialog"), "{screen}");
    // 40 digits in 26 cells: two lines of 20, not one of 26 and one of 14.
    assert_eq!(row(&hex[..20]) + 1, row(&hex[20..]), "{screen}");
    assert!(!screen.contains(&hex[..21]), "{screen}");
    assert_eq!(
        row(&hex[20..]) + 2,
        row("✓ Same."),
        "a line between, {screen}"
    );
    assert_eq!(row("✓ Same.") + 2, row("{ OK }"), "{screen}");
}
