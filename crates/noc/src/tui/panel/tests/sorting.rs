use super::*;

#[test]
fn lists_directories_first_then_names() {
    let panel = loaded("/srv", listing());
    assert_eq!(
        names(&panel),
        [
            "..",
            "alpha-link",
            "Beta",
            "bin",
            ".hidden",
            "Alpha.md",
            "zeta.txt"
        ]
    );
    assert_eq!(panel.cursor, 0);
}

#[test]
fn sorts_by_name_extension_time_and_size_and_reverses() {
    let mut panel = loaded("/srv", varied());
    let order = |panel: &Panel| names(panel)[1..].join(" ");
    assert_eq!(order(&panel), "Adir dir1 .env a.txt b.md c.txt Z");
    let header = |panel: &mut Panel| draw(panel, 60, 6, true).to_string();
    assert!(header(&mut panel).contains("↑Name"));

    panel.handle(Action::End);
    assert_eq!(panel.handle(Action::SortByExtension), None);
    assert_eq!(order(&panel), "Adir dir1 .env Z b.md a.txt c.txt");
    assert_eq!(under_cursor(&panel), "Z", "the cursor stays on its entry");
    assert!(header(&mut panel).contains("↑Name, by extension"));
    panel.handle(Action::SortByExtension);
    assert_eq!(order(&panel), "dir1 Adir c.txt a.txt b.md Z .env");

    panel.handle(Action::SortByTime);
    assert_eq!(
        order(&panel),
        "Adir dir1 .env Z a.txt c.txt b.md",
        "newest first"
    );
    assert!(header(&mut panel).contains("↓Modify time"));
    panel.handle(Action::SortBySize);
    assert_eq!(
        order(&panel),
        "dir1 Adir a.txt c.txt b.md Z .env",
        "largest first"
    );
    assert!(header(&mut panel).contains("↓Size"));
    panel.handle(Action::SortBySize);
    assert_eq!(order(&panel), "Adir dir1 .env Z b.md c.txt a.txt");
    panel.handle(Action::SortByName);
    assert_eq!(order(&panel), "Adir dir1 .env a.txt b.md c.txt Z");

    // The order holds for the next directory.
    panel.handle(Action::SortByTime);
    let request = panel.handle(Action::Reload).unwrap();
    answer(&mut panel, &request, Listing::Dir(varied()));
    assert_eq!(order(&panel), "Adir dir1 .env Z a.txt c.txt b.md");
}

#[test]
fn hidden_files_come_and_go_and_the_cursor_stays() {
    let (mut panel, request) = Panel::new(local("/srv"), PathBuf::from(HOME), false);
    answer(&mut panel, &request, Listing::Dir(varied()));
    assert_eq!(names(&panel)[1..].join(" "), "Adir dir1 a.txt b.md c.txt Z");
    panel.handle(Action::End);
    panel.set_show_hidden(true);
    assert_eq!(names(&panel).len(), 8);
    assert_eq!(under_cursor(&panel), "Z", "still on its entry");

    panel.handle(Action::Home);
    for _ in 0..3 {
        panel.handle(Action::Down);
    }
    assert_eq!(under_cursor(&panel), ".env");
    panel.set_show_hidden(false);
    assert_eq!(
        under_cursor(&panel),
        "a.txt",
        "the entry is gone; the row stays"
    );
}
