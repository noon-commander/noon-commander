use super::*;

#[test]
fn marks_entries_but_not_dot_dot_and_moves_on() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::Mark);
    assert_eq!(under_cursor(&panel), "alpha-link", "moves on from `..`");
    assert_eq!(panel.marked_total(), (0, 0), "but `..` is not marked");
    panel.handle(Action::Mark);
    panel.handle(Action::End);
    panel.handle(Action::Mark);
    assert_eq!(under_cursor(&panel), "zeta.txt", "the last row stays");
    assert_eq!(
        panel.marked_total(),
        (2, 12_345),
        "directories count, but not their size"
    );
    panel.handle(Action::MarkUp);
    assert_eq!(under_cursor(&panel), "Alpha.md");
    assert_eq!(marked(&panel), ["alpha-link"]);

    panel.handle(Action::InvertMarks);
    assert_eq!(
        marked(&panel),
        [".hidden", "Alpha.md", "alpha-link", "zeta.txt"],
        "files only"
    );
    assert_eq!(panel.marked_total(), (4, 10_012_346));
}

#[test]
fn a_count_marks_as_far_as_the_listing_goes() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::End);
    panel.handle(Action::Up);
    let before = under_cursor(&panel);
    panel.mark_rows(true, 4);
    assert_eq!(
        marked(&panel),
        [before.as_str(), "zeta.txt"],
        "the last row once"
    );
    panel.mark_rows(false, 2);
    assert_eq!(marked(&panel), Vec::<String>::new());
    panel.mark_rows(false, 99);
    assert_eq!(panel.cursor, 0, "on `..`, which is never marked");
    assert_eq!(
        panel.marked_total().0,
        4,
        "the rows above, not the two below"
    );
}

#[test]
fn marks_stay_while_the_directory_does() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::InvertMarks);
    panel.handle(Action::SortBySize);
    assert_eq!(marked(&panel), [".hidden", "Alpha.md", "zeta.txt"]);
    panel.set_show_hidden(false);
    assert_eq!(
        marked(&panel),
        ["Alpha.md", "zeta.txt"],
        "hidden ones lose them"
    );
    panel.set_show_hidden(true);
    assert_eq!(marked(&panel), ["Alpha.md", "zeta.txt"]);

    let request = panel.handle(Action::Reload).unwrap();
    let mut changed = listing();
    changed.retain(|entry| entry.name != b"zeta.txt");
    answer(&mut panel, &request, Listing::Dir(changed));
    assert_eq!(marked(&panel), ["Alpha.md"], "names still there keep them");

    panel.handle(Action::Home);
    let request = panel.handle(Action::Enter).unwrap();
    answer(&mut panel, &request, Listing::Dir(listing()));
    assert!(panel.marked.is_empty(), "another directory starts unmarked");
}
