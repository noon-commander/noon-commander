use super::*;

fn rename_listing() -> Vec<DirEntry> {
    vec![
        entry("report.final.pdf", FileKind::File, 10),
        entry("docs", FileKind::Dir, 4096),
        entry(".profile", FileKind::File, 1),
        entry("notes", FileKind::File, 1),
    ]
}

fn cursor_to(panel: &mut Panel, name: &str) {
    panel.cursor = names(panel).iter().position(|shown| shown == name).unwrap();
}

/// The text of the field of the entry being renamed before its cursor, and all of it.
fn rename_field(panel: &mut Panel) -> (String, String) {
    let field = panel.rename_field().unwrap();
    let (shown, column) = field.visible(80);
    let before = shown.chars().take(column).collect();
    (before, field.text().to_owned())
}

#[test]
fn renaming_puts_the_cursor_before_the_last_extension_of_a_file() {
    assert_eq!(stem_chars("report.final.pdf", false), 12);
    assert_eq!(stem_chars("archive.tar.gz", false), 11);
    assert_eq!(stem_chars("notes", false), 5);
    assert_eq!(stem_chars(".profile", false), 8, "a dotfile is all name");
    assert_eq!(stem_chars(".config.toml", false), 7);
    assert_eq!(stem_chars("odd.", false), 4, "no extension after the dot");
    assert_eq!(stem_chars("photos.d", true), 8, "a directory is all name");
    assert_eq!(stem_chars("файл.txt", false), 4, "characters, not bytes");

    let mut panel = loaded("/srv", rename_listing());
    assert!(!panel.start_rename(), "not on `..`");
    cursor_to(&mut panel, "report.final.pdf");
    assert!(panel.start_rename());
    assert!(panel.renaming());
    assert_eq!(
        rename_field(&mut panel),
        ("report.final".to_owned(), "report.final.pdf".to_owned())
    );
    let field = panel.rename_field().unwrap();
    assert_eq!(field.mode(), Mode::Insert);
    field.edit(Action::DeleteToStart);
    for c in "summary".chars() {
        field.insert(c);
    }
    assert_eq!(
        panel.end_rename(),
        Some((b"report.final.pdf".to_vec(), "summary.pdf".to_owned()))
    );
    assert!(!panel.renaming());

    let odd = DirEntry {
        name: b"a\xffb".to_vec(),
        ..entry("x", FileKind::File, 1)
    };
    let mut odd = loaded("/srv", vec![odd]);
    odd.cursor = 1;
    assert!(
        !odd.start_rename(),
        "the field could not give the name back"
    );
}

#[test]
fn renaming_follows_its_entry_and_ends_when_it_goes() {
    let mut panel = loaded("/srv", rename_listing());
    cursor_to(&mut panel, "notes");
    assert!(panel.start_rename());
    panel.rename_field().unwrap().insert('x');

    let request = panel.handle(Action::Reload).unwrap();
    let mut more = rename_listing();
    more.push(entry("a-new-one", FileKind::File, 1));
    answer(&mut panel, &request, Listing::Dir(more));
    assert_eq!(under_cursor(&panel), "notes", "the cursor stays on it");
    assert_eq!(rename_field(&mut panel).1, "notesx", "and so does the text");

    let request = panel.handle(Action::Reload).unwrap();
    answer(&mut panel, &request, Listing::Dir(listing()));
    assert!(!panel.renaming(), "the entry is gone");

    let mut panel = loaded("/srv", rename_listing());
    cursor_to(&mut panel, ".profile");
    assert!(panel.start_rename());
    panel.set_show_hidden(false);
    assert!(!panel.renaming(), "the entry is hidden");

    let mut panel = loaded("/srv", rename_listing());
    cursor_to(&mut panel, "docs");
    assert!(panel.start_rename());
    assert_eq!(rename_field(&mut panel).0, "docs");
    panel.go(Destination::to(local("/")));
    assert!(!panel.renaming(), "the panel goes elsewhere");
}

#[test]
fn draws_the_name_being_renamed_over_its_row() {
    let mut panel = loaded("/srv", rename_listing());
    cursor_to(&mut panel, "report.final.pdf");
    assert!(panel.start_rename());
    let terminal = render_with(
        &mut panel,
        (40, 9),
        true,
        &|_| HostState::default(),
        Decor::new(false),
    );
    insta::assert_snapshot!(terminal.backend());
    let buffer = terminal.backend().buffer();
    let y = 1 + 1 + u16::try_from(panel.cursor - panel.window.top()).unwrap();
    // After the marker, the name, in Insert mode with nothing selected, and the rest of the
    // row, which the size and time columns no longer cut.
    let reversed = |x: u16| buffer[(x, y)].modifier.contains(Modifier::REVERSED);
    assert!(reversed(2) && reversed(14), "nothing is selected");
    assert!(reversed(38), "the field runs to the end of the row");
    assert_eq!(buffer[(30, y)].symbol(), " ", "no line between columns");
    assert_eq!(terminal.backend().cursor_position(), (14, y).into());
}
