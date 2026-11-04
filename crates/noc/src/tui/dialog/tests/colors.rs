use super::*;

#[test]
fn errors_are_red_in_mc_classic() {
    use ratatui::style::{Color, Modifier};

    let mut dialog = Dialog::error("Cannot create directory /x: already exists");
    let terminal = draw_themed(&dialog, 60, 8, &Theme::mc_classic());
    let buffer = terminal.backend().buffer();
    let colors = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
    // The dialog spans columns 2 … 57 and rows 1 … 6.
    assert_eq!(colors(2, 1), (Color::White, Color::Red), "frame");
    let title = title_column(buffer, "Error");
    assert_eq!(colors(title, 1), (Color::LightYellow, Color::Red), "title");
    assert!(buffer[(title, 1)].modifier.contains(Modifier::BOLD));
    assert_eq!(colors(4, 2), (Color::White, Color::Red), "message");
    let (x, y) = (0..60)
        .flat_map(|x| (0..8).map(move |y| (x, y)))
        .find(|&(x, y)| buffer[(x, y)].symbol() == "{")
        .unwrap();
    assert_eq!(
        colors(x, y),
        (Color::Black, Color::Gray),
        "the focused button"
    );
    assert_eq!(
        dialog.handle(action(Action::Confirm)),
        DialogEvent::Pressed(Button::Ok)
    );
}

#[test]
fn warnings_have_a_background_apart_from_dialogs_and_errors() {
    use ratatui::style::Color;

    let theme = Theme::mc_classic();
    // The colors of the title and the message.
    let colors = |tone: Tone| {
        let dialog = Dialog::question("Delete", "Delete x?", vec![Button::Yes], 0, tone);
        let terminal = draw_themed(&dialog, 60, 8, &theme);
        let buffer = terminal.backend().buffer();
        let at = |x: u16, y: u16| (buffer[(x, y)].fg, buffer[(x, y)].bg);
        (at(title_column(buffer, "Delete"), 1), at(4, 2))
    };
    let (title, body) = colors(Tone::Warning);
    assert_eq!(body, (Color::Black, Color::Yellow));
    assert_eq!(title, (Color::Black, Color::Yellow));
    assert_ne!(body, colors(Tone::Plain).1);
    assert_ne!(body, colors(Tone::Error).1);
}

#[test]
fn fields_boxes_and_checks_take_the_colors_of_errors_and_warnings() {
    use ratatui::style::Color;

    let theme = Theme::mc_classic();
    let colors = |buffer: &Buffer, at: Position| (buffer[at].fg, buffer[at].bg);
    let find = |buffer: &Buffer, text: &str| crate::tui::mouse::find(buffer, text);

    // The combo box of a warning, apart from the button with the focus.
    let mut dialog = with_items();
    let terminal = draw_themed(&dialog, 60, 14, &theme);
    let buffer = terminal.backend().buffer();
    let boxed = find(buffer, "Ask each time");
    assert_eq!(
        colors(buffer, boxed),
        (Color::Black, Color::Gray),
        "the box"
    );
    let button = find(buffer, "{ Skip }");
    assert_eq!(buffer[button].bg, Color::Cyan, "the focus");
    let bracket = Position::new(boxed.x - 1, boxed.y);
    assert_eq!(buffer[bracket].symbol(), "[");
    assert_eq!(colors(buffer, bracket), (Color::DarkGray, Color::Gray));
    dialog.focus = Focus::ComboBox(0);
    let terminal = draw_themed(&dialog, 60, 14, &theme);
    let buffer = terminal.backend().buffer();
    let boxed = find(buffer, "Ask each time");
    assert_eq!(
        colors(buffer, boxed),
        (Color::Black, Color::Cyan),
        "with the focus, as the buttons"
    );

    // A field and a check box of an error dialog.
    let dialog = Dialog::error("Cannot write /x")
        .with_field("&Name:", "x")
        .with_check("&Again", true);
    let terminal = draw_themed(&dialog, 60, 12, &theme);
    let buffer = terminal.backend().buffer();
    let label = find(buffer, "Name:");
    let field = Position::new(label.x, label.y + 1);
    assert_eq!(buffer[field].symbol(), "x");
    assert_eq!(
        colors(buffer, field),
        (Color::White, Color::Black),
        "the field"
    );
    let check = find(buffer, "Again");
    let bracket = Position::new(check.x - 4, check.y);
    assert_eq!(buffer[bracket].symbol(), "[");
    assert_eq!(colors(buffer, bracket), (Color::Gray, Color::Red));
    let symbol = Position::new(check.x - 3, check.y);
    assert_eq!(colors(buffer, symbol), (Color::LightYellow, Color::Red));
}

#[test]
fn a_warning_has_its_title_the_colors_of_warnings_and_ok() {
    let dialog = Dialog::warning("Checksum", "There are no files to hash.");
    assert_eq!(dialog.tone(), Tone::Warning);
    assert_eq!(dialog.title, "Checksum");
    assert_eq!(dialog.buttons, vec![Button::Ok]);
    assert_eq!(dialog.focus, Focus::Button(0));
}
