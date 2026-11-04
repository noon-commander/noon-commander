use super::*;

#[test]
fn titles_put_a_tilde_for_the_home_directory() {
    let home = Path::new(HOME);
    let text = |path: &str| home_text(Path::new(path), home);
    assert_eq!(text(HOME), "~");
    assert_eq!(text("/home/me/src"), "~/src");
    assert_eq!(text("/home/meow"), "/home/meow", "not inside home");
    assert_eq!(text("/srv"), "/srv");
    assert_eq!(home_text(Path::new("/srv"), Path::new("/")), "/srv");
}

#[test]
fn resolves_what_dialogs_name() {
    let mut panel = loaded("/srv", listing());
    assert_eq!(panel.name_under_cursor(), None, "`..`");
    panel.handle(Action::End);
    assert_eq!(panel.name_under_cursor(), Some(&b"zeta.txt"[..]));
    let resolve = |text: &str| panel.resolve(text);
    assert_eq!(resolve("new"), local("/srv/new"));
    assert_eq!(resolve("a/b//"), local("/srv/a/b"));
    assert_eq!(resolve("/tmp/x"), local("/tmp/x"));
    assert_eq!(resolve("/"), local("/"));
    assert_eq!(resolve("~"), local(HOME));
    assert_eq!(resolve("~/x"), local("/home/me/x"));
    assert_eq!(resolve("~x"), local("/srv/~x"), "not the home directory");
    assert_eq!(
        resolve("\\~/x"),
        local("/srv/~/x"),
        "a name that starts with ~"
    );

    let remote_panel = loaded_at(remote("db", "/srv"), Listing::Dir(Vec::new()));
    let resolve = |text: &str| remote_panel.resolve(text);
    assert_eq!(resolve("new"), remote("db", "/srv/new"));
    assert_eq!(resolve("/abs"), remote("db", "/abs"));
    assert_eq!(resolve("~/x"), remote("db", "x"), "from the remote home");
    assert_eq!(resolve("~"), remote("db", ""));
}

#[test]
fn reload_keeps_the_cursor_on_its_entry() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::End);
    let request = panel.handle(Action::Reload).unwrap();
    assert_eq!(request.location, local("/srv"));
    let mut changed = listing();
    changed.push(entry("new.txt", FileKind::File, 1));
    answer(&mut panel, &request, Listing::Dir(changed));
    assert_eq!(under_cursor(&panel), "zeta.txt");

    let request = panel.handle(Action::Reload).unwrap();
    let other = vec![entry("other", FileKind::File, 1)];
    answer(&mut panel, &request, Listing::Dir(other));
    assert_eq!(
        panel.cursor, 1,
        "the entry is gone: its row, or the last one"
    );
}

#[test]
fn drops_stale_replies() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::Down);
    let first = panel.handle(Action::Enter).unwrap();
    panel.handle(Action::Home);
    let second = panel.handle(Action::Enter).unwrap();
    let stale = vec![entry("stale", FileKind::File, 1)];
    answer(&mut panel, &first, Listing::Dir(stale));
    assert_eq!(
        panel.location,
        local("/srv"),
        "an older request was answered"
    );
    answer(&mut panel, &second, Listing::Dir(listing()));
    assert_eq!(panel.location, local("/"));
    answer(&mut panel, &second, Listing::Dir(Vec::new()));
    assert_eq!(
        panel.rows(),
        listing().len(),
        "a reply counts once, and `/` has no `..`"
    );
}

#[test]
fn a_failed_listing_keeps_the_directory_and_reports_why() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::Down);
    let request = panel.handle(Action::Enter).unwrap();
    let denied = ListError {
        kind: ListErrorKind::Denied,
        reason: "permission denied".to_owned(),
        code: None,
        location: None,
    };
    let failure = panel.listed(request.generation, Err(denied)).unwrap();
    assert_eq!(panel.location, local("/srv"));
    assert_eq!(under_cursor(&panel), "alpha-link");
    assert_eq!(failure.shown, "alpha-link");
    assert_eq!(failure.error.kind, ListErrorKind::Denied);
    assert_eq!(failure.destination.location(), &request.location);
    assert_eq!(
        failure.parent,
        Some(Destination::onto(local("/srv"), "alpha-link")),
        "up is the panel's directory, with the cursor on the name"
    );
    assert!(!failure.stranded && !failure.climbing);
    // The parent is not inside the directory shown, so it gets its full path.
    let request = panel.handle(Action::Parent).unwrap();
    let missing = ListError {
        kind: ListErrorKind::NotFound,
        reason: "no such file or directory".to_owned(),
        code: None,
        location: None,
    };
    let failure = panel.listed(request.generation, Err(missing)).unwrap();
    assert_eq!(failure.shown, "/");
    assert_eq!(failure.parent, None, "nothing is above /");

    let request = panel.handle(Action::Reload).unwrap();
    let bad = ListError::other("bad\x1b[2Jthing".to_owned());
    let failure = panel.listed(request.generation, Err(bad)).unwrap();
    assert_eq!(failure.shown, "/srv");
    assert_eq!(
        failure.error.reason, "bad?[2Jthing",
        "reasons are terminal-safe"
    );
    assert!(failure.stranded, "the panel stands on the directory");

    let request = panel.climb(Destination::to(local("/")));
    let failure = panel
        .listed(request.generation, Err(ListError::other(String::new())))
        .unwrap();
    assert!(failure.climbing);
    let request = panel.go(Destination::to(local("/")));
    let failure = panel
        .listed(request.generation, Err(ListError::other(String::new())))
        .unwrap();
    assert!(!failure.climbing, "a new request does not climb");
}

#[test]
fn a_cancelled_request_is_forgotten() {
    let mut panel = loaded("/srv", listing());
    let request = panel.go(Destination::to(remote("web", "")));
    assert_eq!(panel.pending_request(), Some(request.clone()));
    let screen = draw(&mut panel, 40, 6, true).to_string();
    assert!(screen.contains("╔ … web "), "{screen}");

    panel.cancel();
    assert_eq!(panel.pending_request(), None);
    answer(&mut panel, &request, Listing::Dir(Vec::new()));
    assert_eq!(panel.location, local("/srv"), "the late reply is dropped");
}

/// The title and the status line of `panel` while its hosts are in `status`, and the styles
/// of the icon or the dots and of the path in the title, at its third and fifth cells.
fn waiting_lines(panel: &mut Panel, status: HostStatus) -> ([String; 2], [Style; 2]) {
    let hosts = |_: &str| HostState {
        status,
        address: None,
    };
    let terminal = render_with(panel, (40, 6), true, &hosts, Decor::new(false));
    let buffer = terminal.backend().buffer();
    let line = |y: u16| {
        let line: String = (0..40).map(|x| buffer[(x, y)].symbol()).collect();
        line.trim_end().to_owned()
    };
    let styles = [buffer[(2, 0)].style(), buffer[(4, 0)].style()];
    ([line(0), line(4)], styles)
}

#[test]
fn a_panel_waiting_shows_dots_in_place_of_its_icon_and_what_it_lists() {
    let theme = Theme::terminal();
    let mut panel = loaded("/srv", listing());
    let ([title, _], _) = waiting_lines(&mut panel, HostStatus::Idle);
    assert!(title.starts_with("╔ / /srv "), "{title}");
    panel.go(Destination::to(remote("web", "")));
    let ([title, status], [dots, path]) = waiting_lines(&mut panel, HostStatus::Connecting);
    assert_eq!(title, "╔ … web ══════╤═══════╤════════════════╗");
    assert_eq!(status, "║..                                    ║");
    let title_style = theme.panel_title_active;
    // The colors a cell shows, which cells keep with every color reset that no style sets.
    let shown = |style: Style| {
        let style = Style::reset().patch(style);
        (style.fg, style.add_modifier)
    };
    assert_eq!(shown(dots), shown(title_style.patch(theme.host_connecting)));
    assert_eq!(shown(path), shown(title_style));
    // A host that does not answer shows its marker in red, still; its dialog asks what to do.
    let ([title, status], [dots, _]) = waiting_lines(&mut panel, HostStatus::Stalled);
    assert!(title.starts_with("╔ ✗ web "), "{title}");
    assert_eq!(status, "║..                                    ║");
    assert_eq!(shown(dots), shown(title_style.patch(theme.host_failed)));
    // Listing a host that is connected, the dots are busy as any other.
    let ([title, _], [dots, _]) = waiting_lines(&mut panel, HostStatus::Connected);
    assert!(title.starts_with("╔ … web "), "{title}");
    assert_eq!(shown(dots), shown(title_style.patch(theme.busy)));

    let mut panel = loaded("/srv", listing());
    panel.go(Destination::to(local("/srv/photos")));
    let ([title, _], _) = waiting_lines(&mut panel, HostStatus::Idle);
    assert!(title.starts_with("╔ … /srv/photos "), "{title}");
    // What it lists keeps its last directories where it does not fit.
    let terminal = render_with(
        &mut panel,
        (14, 6),
        true,
        &|_| HostState::default(),
        Decor::new(false),
    );
    let title: String = (0..14)
        .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
        .collect();
    assert_eq!(title, "╔ … «/photos ╗");
    // Given up, the title shows where it is again, its icon back.
    panel.cancel();
    let ([title, _], _) = waiting_lines(&mut panel, HostStatus::Idle);
    assert!(title.starts_with("╔ / /srv "), "{title}");
}

#[test]
fn a_panel_on_a_host_shows_its_status_in_front_of_the_path() {
    let theme = Theme::terminal();
    let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
    let ([title, _], [icon, _]) = waiting_lines(&mut panel, HostStatus::Connected);
    assert!(title.starts_with("╔ ● db:/srv "), "{title}");
    assert_eq!(
        Style::reset().patch(icon).add_modifier,
        Style::reset()
            .patch(theme.panel_title_active.patch(theme.host_connected))
            .add_modifier
    );
    let ([title, _], _) = waiting_lines(&mut panel, HostStatus::Failed);
    assert!(title.starts_with("╔ ✗ db:/srv "), "{title}");
}

#[test]
fn a_wait_shows_nothing_for_its_first_moment() {
    let mut panel = loaded("/srv", listing());
    panel.handle(Action::Reload);
    let theme = Theme::terminal();
    let hosts = |_: &str| HostState::default();
    let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
    let now = Instant::now();
    let view = View {
        hosts: &hosts,
        decor: Decor::new(false),
        theme: &theme,
        spin: Spin::new(now, now, true, theme.glyphs.spinners),
        tz: &TimeZone::UTC,
        formats: &Formats::iso(),
    };
    terminal
        .draw(|frame| panel.render(frame, frame.area(), true, &view))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let title: String = (0..40).map(|x| buffer[(x, 0)].symbol()).collect();
    assert!(title.starts_with("╔ / /srv "), "{title}");
    assert_eq!(
        panel.wait().and_then(Wait::shows),
        Some(now + spinner::DELAY)
    );
}

#[test]
fn work_waits_in_one_wait_with_the_listing_after_it_until_its_host_goes() {
    let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
    let start = Instant::now();
    panel.begin_work(remote("db", "/srv/new"));
    panel.begin_work(local("/tmp/new"));
    let shows = panel.wait().map(|wait| wait.since(start));
    assert_eq!(shows, Some(start));

    // The listing that the result asks for keeps the wait, so the dots do not start over.
    let request = panel.handle(Action::Reload).unwrap();
    panel.end_work(&local("/tmp/new"));
    let later = start + spinner::DELAY;
    assert_eq!(panel.wait().map(|wait| wait.since(later)), Some(start));
    answer(&mut panel, &request, Listing::Dir(listing()));
    assert!(panel.wait().is_some(), "the work on db");

    // Work on another host goes on; the result of work on a host that goes never comes.
    panel.drop_host("web");
    assert!(panel.wait().is_some());
    panel.drop_host("db");
    assert!(panel.wait().is_none());
    panel.begin_work(remote("db", "/srv/new"));
    assert_eq!(panel.wait().map(|wait| wait.since(later)), Some(later));
    panel.lose_host("db");
    panel.cancel();
    assert!(panel.wait().is_none());
}

#[test]
fn a_lost_host_keeps_the_entries_of_its_panels_waiting_with_the_cursor_and_marks() {
    let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
    panel.handle(Action::End);
    panel.handle(Action::Mark);
    panel.lose_host("web");
    assert!(!panel.disconnected, "another host");

    panel.lose_host("db");
    assert_eq!(names(&panel).len(), 1 + listing().len(), "the entries stay");
    assert!(panel.disconnected);
    assert_eq!(panel.marked.len(), 1);
    let request = panel.pending_request().unwrap();
    assert_eq!(request.location, remote("db", "/srv"));
    assert_eq!(panel.not_connected(), None, "it waits");
    answer(&mut panel, &request, Listing::Dir(listing()));
    assert_eq!(under_cursor(&panel), "zeta.txt", "back on its entry");
    assert_eq!(panel.marked.len(), 1, "the marks too");
    assert!(!panel.disconnected);

    // Cancel: the panel keeps its entries, toned down, until it reads the directory again.
    panel.lose_host("db");
    panel.cancel();
    assert_eq!(panel.not_connected(), Some("db"));
    panel.handle(Action::Up);
    assert_eq!(
        under_cursor(&panel),
        "Alpha.md",
        "the cursor moves over them"
    );
    let ([_, line], _) = waiting_lines(&mut panel, HostStatus::Failed);
    assert!(line.starts_with("║Alpha.md"), "{line}");
    let request = panel.handle(Action::Reload).unwrap();
    assert_eq!(request.location, remote("db", "/srv"));
}

#[test]
fn a_disconnected_host_keeps_the_entries_of_its_panels() {
    let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
    panel.drop_host("web");
    assert!(!panel.disconnected, "another host");
    panel.drop_host("db");
    assert_eq!(names(&panel).len(), 1 + listing().len());
    assert!(panel.has_entries());
    assert_eq!(panel.pending_request(), None);
    assert_eq!(panel.not_connected(), Some("db"));
    let request = panel.handle(Action::Enter).unwrap();
    assert_eq!(request.location, remote("db", "/"), "`..` connects too");
    assert_eq!(panel.not_connected(), None, "it waits");

    // A panel that was about to open the host stays where it is.
    let mut panel = loaded("/srv", listing());
    panel.go(Destination::to(remote("db", "")));
    panel.drop_host("db");
    assert_eq!(panel.pending_request(), None);
    assert_eq!(panel.location, local("/srv"));
    assert!(!panel.disconnected);
    assert_eq!(panel.not_connected(), None);
}

#[test]
fn a_panel_not_connected_tones_down_what_it_kept() {
    let mut panel = loaded_at(remote("db", "/srv"), Listing::Dir(listing()));
    panel.handle(Action::Down);
    panel.handle(Action::Mark);
    let dim = |backend: &TestBackend, (x, y): (u16, u16)| {
        let cell = &backend.buffer()[(x, y)];
        cell.modifier.contains(Modifier::DIM) && !cell.modifier.contains(Modifier::BOLD)
    };
    // Rows: frame, header, `..`, alpha-link (marked), Beta (cursor), …, separator, status,
    // frame with the total.
    let (parent, marked, cursor) = ((3, 2), (3, 3), (3, 4));
    let (separator, status, total) = ((20, 9), (2, 10), (4, 11));
    let backend = draw(&mut panel, 40, 12, true);
    for at in [parent, marked, cursor, separator, status, total] {
        assert!(!dim(&backend, at), "connected: {at:?}");
    }

    panel.drop_host("db");
    let backend = draw(&mut panel, 40, 12, true);
    for at in [parent, marked, cursor, separator, status, total] {
        assert!(dim(&backend, at), "{at:?}");
    }
    for at in [(0, 2), (3, 0), (3, 1), (39, 11)] {
        assert!(!dim(&backend, at), "the frame, title, and header: {at:?}");
    }
    let bottom: String = (0..40)
        .map(|x| backend.buffer()[(x, 11)].symbol())
        .collect();
    assert!(bottom.contains(" in "), "the total stays: {bottom}");

    // Quick search types what is not of the host.
    panel.search_down(1, quick(Way::Plain));
    let backend = draw(&mut panel, 40, 12, true);
    assert!(!dim(&backend, status));
}

#[test]
fn the_last_local_directory_is_the_last_local_jump() {
    let mut panel = loaded("/srv", listing());
    assert_eq!(panel.last_local(), None, "no jump yet");
    let request = panel.go(Destination::to(local("/tmp")));
    answer(&mut panel, &request, Listing::Dir(listing()));
    let request = panel.go(Destination::to(remote("db", "/srv")));
    answer(&mut panel, &request, Listing::Dir(listing()));
    assert_eq!(
        panel.last_local().as_ref().map(Destination::location),
        Some(&local("/tmp"))
    );
}
