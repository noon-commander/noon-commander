use std::assert_matches;
use std::num::NonZeroUsize;

use super::*;
use crate::tui::help::Files;

/// The files of the About page, with `log` for the file of the log.
fn files(log: Result<PathBuf, String>) -> Files {
    Files {
        home: PathBuf::from("/home/me"),
        config: PathBuf::from("/home/me/.config/noc/config.toml"),
        hosts: PathBuf::from("/home/me/.config/noc/hosts.toml"),
        workspaces: PathBuf::from("/home/me/.local/share/noc/workspaces.toml"),
        history: PathBuf::from("/home/me/.local/state/noc/history.toml"),
        recent: PathBuf::from("/home/me/.local/state/noc/recent.toml"),
        log,
    }
}

const LOG: &str = "/home/me/.local/state/noc/logs/noc-20261105T120000Z-42.log";

#[test]
fn the_log_shows_from_its_end_in_front_of_what_is_open_which_comes_back() {
    let mut app = with_files(&["a1"]);
    app.about.files = Some(files(Ok(PathBuf::from(LOG))));
    app.handle(action(Action::Help));
    assert_eq!(app.context(), Context::Help);
    let help = screen_of(&mut app, 30);
    assert!(help.contains("Search and renaming"), "{help}");
    let Effect::Read(ReadRequest {
        id, location, host, ..
    }) = one(app.handle(action(Action::ViewLog)))
    else {
        panic!("expected a read");
    };
    assert_eq!((location, host.is_none()), (local(LOG), true));
    assert_eq!(app.context(), Context::Viewer);
    let text = (1..=40)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.read(id, Ok(whole(text)));
    let screen = screen_of(&mut app, 30);
    assert!(
        screen.contains("line 40") && !screen.contains("line 1 "),
        "{screen}"
    );
    assert!(
        !screen.contains("Search and renaming"),
        "the help is behind it: {screen}"
    );
    // F1 would open the help under it.
    app.handle(action(Action::Help));
    assert_eq!(app.context(), Context::Viewer);
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Help);
}

#[test]
fn the_log_shows_in_front_of_the_viewer_and_a_text_field_in_insert_mode() {
    let mut app = with_files(&["a1"]);
    app.about.files = Some(files(Ok(PathBuf::from(LOG))));
    app.handle(action(Action::Command));
    assert_eq!(app.place().0, Mode::Insert);
    let Effect::Read(ReadRequest { location, .. }) = one(app.handle(action(Action::ViewLog)))
    else {
        panic!("expected a read");
    };
    assert_eq!(location, local(LOG));
    assert_eq!(app.place(), (Mode::Normal, Context::Viewer));
    app.handle(action(Action::Quit));
    assert_eq!(app.place().1, Context::CommandLine);
    assert!(!app.quits());
}

#[test]
fn the_log_shows_in_front_of_a_dialog_which_comes_back_as_it_was() {
    let mut app = with_files(&["a1"]);
    app.about.files = Some(files(Ok(PathBuf::from(LOG))));
    app.handle(action(Action::Mkdir));
    typed(&mut app, "new");
    assert_eq!(app.place(), (Mode::Insert, Context::PathInput));
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::ViewLog))) else {
        panic!("expected a read");
    };
    assert_eq!(app.place(), (Mode::Normal, Context::Viewer));
    app.read(id, Ok(whole("the last line\n")));
    let screen = screen_of(&mut app, 30);
    assert!(screen.contains("the last line"), "{screen}");
    assert!(!screen.contains("Create a new directory"), "{screen}");
    // A dialog that comes after it stands in front of it.
    app.show_error_over("boom");
    assert_eq!(app.context(), Context::Dialog);
    assert!(screen_of(&mut app, 30).contains("boom"));
    app.handle(action(Action::Confirm));
    assert_eq!(app.place(), (Mode::Normal, Context::Viewer));
    app.handle(action(Action::Cancel));
    assert_eq!(app.place(), (Mode::Insert, Context::PathInput));
    let screen = screen_of(&mut app, 30);
    assert!(
        screen.contains("Create a new directory") && screen.contains("new"),
        "{screen}"
    );
}

#[test]
fn without_a_log_it_says_why() {
    let mut app = with_files(&["a1"]);
    app.about.files = Some(files(Err("Permission denied".to_owned())));
    assert!(app.handle(action(Action::ViewLog)).is_empty());
    assert_eq!(app.context(), Context::Panel);
    assert_eq!(
        app.message,
        Some(Message::Warning(
            "This run has no log: Permission denied".to_owned()
        ))
    );
}

#[test]
fn a_log_that_cannot_be_read_is_viewed_again_by_retry() {
    let mut app = with_files(&["a1"]);
    app.about.files = Some(files(Ok(PathBuf::from(LOG))));
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::ViewLog))) else {
        panic!("expected a read");
    };
    app.read(id, Err("no such file".to_owned().into()));
    assert_eq!(app.context(), Context::Dialog);
    let Effect::Read(ReadRequest { location, .. }) = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected a read");
    };
    assert_eq!(location, local(LOG));
    assert!(app.viewer_over(), "in front again");
}

#[test]
fn f3_views_files_and_opens_directories() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("sub"), file("notes", 12)]),
    );
    app.handle(action(Action::End));
    let Effect::Read(ReadRequest {
        id,
        location,
        host,
        cancel,
        ..
    }) = one(app.handle(action(Action::View)))
    else {
        panic!("expected a read");
    };
    assert_eq!((location, host.is_none()), (local("/srv/notes"), true));
    assert_eq!(app.context(), Context::Viewer);
    // While it is read, the dots stand in place of its icon, and its window shows how far.
    let text = screen(&mut app);
    let rows: Vec<&str> = text.lines().map(|row| row.trim_matches('"')).collect();
    assert!(rows[0].starts_with(" … /srv/notes "), "{text}");
    assert!(text.contains("Reading") && !text.contains("---"), "{text}");
    app.read(id + 1, Ok(whole(*b"not this")));
    app.read(id, Ok(whole(*b"hello\nworld\n")));
    let text = screen(&mut app);
    assert!(text.starts_with("\"   /srv/notes "), "{text}");
    assert!(text.contains("hello") && !text.contains("---"), "{text}");
    assert!(text.contains("2Wrap") && text.contains("10Quit"), "{text}");

    // F1 shows the help over it, and Esc goes back to it.
    app.handle(action(Action::Help));
    assert_eq!(app.context(), Context::Help);
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Viewer);
    app.handle(action(Action::Quit));
    assert_eq!(app.context(), Context::Panel);
    assert!(cancel.is_cancelled(), "a read in flight stops");
    assert!(!app.quits(), "Quit closes the viewer, not the app");

    // On a directory, F3 opens it.
    app.handle(action(Action::Home));
    app.handle(action(Action::Down));
    let Effect::List { request, .. } = one(app.handle(action(Action::View))) else {
        panic!("expected a listing");
    };
    assert_eq!(request.location, local("/srv/sub"));
}

#[test]
fn a_file_viewed_from_a_line_shows_it_at_the_top() {
    let mut app = with_files(&["a1"]);
    let effects = app.view_file(local("/srv/a1"), &local("/srv"), Some(3), Neighbours::Dir);
    let Some(Effect::Read(ReadRequest { id, .. })) = effects.first() else {
        panic!("expected a read, got {effects:?}");
    };
    let text = (1..=40)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.read(*id, Ok(whole(text)));
    let screen = screen_of(&mut app, 12);
    assert!(
        screen.contains("line 3") && !screen.contains("line 2 "),
        "{screen}"
    );
}

#[test]
fn a_file_that_cannot_be_read_is_an_error() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![file("secret", 1)]));
    app.handle(action(Action::Down));
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
        panic!("expected a read");
    };
    app.read(id, Err("permission denied".to_owned().into()));
    assert_eq!(app.context(), Context::Dialog);
    let text = screen_of(&mut app, 12);
    assert!(
        shows_problem(
            &text,
            "Cannot view",
            Some("/srv/secret"),
            "permission denied"
        ),
        "{text}"
    );
    // Retry views it again.
    let Effect::Read(ReadRequest { location, .. }) = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected a read");
    };
    assert_eq!(location, local("/srv/secret"));
    assert_eq!(app.context(), Context::Viewer);
}

#[test]
fn an_error_on_a_host_that_is_not_connected_offers_to_connect() {
    let (mut app, connection) = on_a_host();
    // F8 on the host, whose connection is lost while it asks: Connect connects, and the
    // user deletes again.
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    app.closed("web", connection, Some("Broken pipe"));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    app.handle(action(Action::Cancel));
    let text = screen_of(&mut app, 16);
    assert!(text.contains("Cannot delete"), "{text}");
    assert!(text.contains("The connection is closed"), "{text}");
    assert!(text.contains("{ Connect } [ Cancel ]"), "{text}");
    let Effect::Connect { connection, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected a connection");
    };
    assert!(app.dialogs.is_empty(), "nothing more");
    // Once connected, it deletes.
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    let deletes = effects
        .iter()
        .any(|effect| matches!(effect, Effect::Delete { .. }));
    assert!(deletes, "{effects:?}");
    assert!(app.pending.is_empty());

    // What waits for a host goes once the host does.
    app.pending.push(Again::Delete {
        dir: remote("web", "/home/deploy"),
        names: vec![b"notes".to_vec()],
    });
    app.disconnect("web");
    assert!(app.pending.is_empty());
}

#[test]
fn a_rename_changes_and_a_write_retries_and_cancel_does_nothing() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
    let panel = app.shown(Side::Left);
    let (from, to) = (local("/srv/a"), local("/srv/b"));
    let busy = || Err(Refused::Failed("Busy".to_owned().into()));
    // A rename that failed: Change… opens its field again, with the name typed.
    app.entry_renamed(panel, &from, &to, busy());
    assert!(screen_of(&mut app, 16).contains("{ Change… } [ Cancel ]"));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.context(), Context::Rename);
    let typed = app
        .panel_mut(Side::Left)
        .rename_field()
        .map(|field| field.text().to_owned());
    assert_eq!(typed.as_deref(), Some("b"));
    let again = one(app.handle(action(Action::Confirm)));
    assert_matches!(
        &again,
        Effect::Rename { from: f, to: t, replace: false, .. } if *f == from && *t == to,
    );
    app.entry_renamed(panel, &from, &to, busy());
    assert!(app.handle(action(Action::Cancel)).is_empty());
    assert!(app.dialogs.is_empty());

    // A save that failed writes the same bytes again.
    let location = local("/srv/SUMS");
    let write = one(app.write_file(location.clone(), b"x".to_vec(), false));
    assert_matches!(write, Effect::WriteFile { .. });
    app.written(&location, busy());
    let Effect::WriteFile {
        location: again,
        bytes,
        ..
    } = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected the file to be written again");
    };
    assert_eq!((again, bytes), (location, b"x".to_vec()));
}

/// The size of `/srv/big`: more than the viewer reads ahead of the screen.
const BIG: u64 = 100 << 20;

/// The viewer of `/srv/big` with the read it starts.
fn view_big() -> (App, Effect) {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    answer(&mut app, effects, &Listing::Dir(vec![file("big", BIG)]));
    app.handle(action(Action::Down));
    let read = one(app.handle(action(Action::View)));
    (app, read)
}

/// The part of `/srv/big` that `want` asks for, holding `bytes`.
fn part_of_big(want: Want, bytes: &[u8]) -> ViewerPart {
    let stamp = Stamp {
        size: Some(BIG),
        modified: None,
    };
    ViewerPart::new(want, bytes, (false, false), stamp)
}

#[test]
fn a_read_that_takes_long_shows_a_window_that_stops_it() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest {
        id,
        want,
        stop,
        cancel,
        ..
    }) = read
    else {
        panic!("expected a read");
    };
    assert_eq!(want.offset, 0);
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("Reading") && text.contains("{ Cancel }"),
        "{text}"
    );
    assert_eq!(app.context(), Context::Dialog);
    assert_eq!(app.place().0, Mode::Normal);
    app.read_progress(
        id,
        ReadProgress {
            read: 250,
            total: 1000,
            size: Some(1000),
        },
    );
    let text = screen_of(&mut app, 16);
    assert!(text.contains("25%"), "{text}");

    // Esc stops the read and keeps what it has read.
    app.handle(action(Action::Cancel));
    assert!(stop.is_cancelled() && !cancel.is_cancelled());
    assert_eq!(app.context(), Context::Viewer);
    let mut part = part_of_big(want, b"one\ntwo\n");
    part.stopped = true;
    assert!(
        app.read(id, Ok(part)).is_empty(),
        "a stopped read starts no other"
    );
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("two") && text.contains("more below"),
        "{text}"
    );
    assert!(!text.contains("Reading"), "{text}");
}

#[test]
fn the_window_of_a_read_ends_full_and_then_goes() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { id, want, .. }) = read else {
        panic!("expected a read");
    };
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    screen_at(&mut app, 16, at(0));
    let progress = ReadProgress {
        read: 980,
        total: 1000,
        size: Some(1000),
    };
    app.read_progress(id, progress);
    let text = screen_at(&mut app, 16, at(200));
    assert!(text.contains("98%"), "{text}");

    // The part comes before a report of its end: the window shows it full, over the text,
    // and leaves the keys to the viewer.
    app.read(id, Ok(part_of_big(want, b"one\ntwo\n")));
    let text = screen_at(&mut app, 16, at(300));
    assert!(text.contains("100%") && text.contains("two"), "{text}");
    assert_eq!(app.context(), Context::Viewer);
    let goes = app.spin_due(at(300)).unwrap();
    assert_eq!(goes, at(300) + spinner::DELAY);
    let text = screen_at(&mut app, 16, goes);
    assert!(!text.contains("Reading"), "{text}");
    assert_eq!(app.spin_due(goes), None);
}

#[test]
fn enter_in_the_window_of_a_read_stops_it_and_esc_then_closes_the_viewer() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { stop, cancel, .. }) = read else {
        panic!("expected a read");
    };
    screen_of(&mut app, 16);
    // Tab has no other stop to go to.
    app.handle(action(Action::NextField));
    app.handle(action(Action::Confirm));
    assert!(stop.is_cancelled() && !cancel.is_cancelled());
    assert_eq!(app.context(), Context::Viewer);
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);
    assert!(cancel.is_cancelled());
}

#[test]
fn the_viewer_reads_the_next_part_at_the_end_and_keeps_its_text_if_it_fails() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { id, want, .. }) = read else {
        panic!("expected a read");
    };
    assert!(
        app.read(id, Ok(part_of_big(want, b"one\ntwo\n")))
            .is_empty()
    );
    screen_of(&mut app, 16);
    let Effect::Read(ReadRequest { id, want, .. }) = one(app.handle(action(Action::PageDown)))
    else {
        panic!("expected the next part");
    };
    assert_eq!((want.offset, want.new), (8, true));
    assert!(
        app.read(id, Err("broken pipe".to_owned().into()))
            .is_empty()
    );
    let text = screen_of(&mut app, 16);
    assert!(
        shows_problem(
            &text,
            "Cannot read more of",
            Some("/srv/big"),
            "broken pipe"
        ),
        "{text}"
    );
    // Retry reads the same part again.
    let Effect::Read(ReadRequest {
        id, want: again, ..
    }) = one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected the part again");
    };
    assert_eq!(again, want);
    app.read(id, Err("broken pipe".to_owned().into()));
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Viewer);
    let text = screen_of(&mut app, 16);
    assert!(text.contains("two"), "{text}");
}

#[test]
fn g_in_a_large_file_reads_its_last_part_alone_and_says_where_it_is_in_bytes() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { id, want, .. }) = read else {
        panic!("expected a read");
    };
    let stamp = Stamp {
        size: Some(1 << 30),
        modified: None,
    };
    let first = ViewerPart::new(want, b"one\ntwo\n", (false, false), stamp);
    assert!(app.read(id, Ok(first)).is_empty());
    screen_of(&mut app, 16);
    let Effect::Read(ReadRequest { id, want, .. }) = one(app.handle(action(Action::End))) else {
        panic!("expected the last part");
    };
    assert_eq!(
        (want.offset, want.length),
        ((1 << 30) - (1 << 20), 1 << 20),
        "not the parts on the way"
    );
    assert!(want.new && want.after_feed, "from the start of a line");
    let lines = (1..=40)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let last = ViewerPart::new(want, lines.as_bytes(), (true, false), stamp);
    assert!(app.read(id, Ok(last)).is_empty(), "it fills the screen");
    let text = screen_of(&mut app, 16);
    assert!(text.contains("line 40") && text.contains("EOF"), "{text}");
    assert!(text.contains("1023M/1.0G End"), "{text}");
}

#[test]
fn a_small_file_is_read_on_behind_the_screen_without_a_window() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { id, want, .. }) = read else {
        panic!("expected a read");
    };
    let stamp = Stamp {
        size: Some(1 << 20),
        modified: None,
    };
    let first = ViewerPart::new(want, b"one\ntwo\n", (false, false), stamp);
    let Effect::Read(ReadRequest { id, want, .. }) = one(app.read(id, Ok(first))) else {
        panic!("expected the next part");
    };
    assert!(want.ahead && want.offset == 8, "{want:?}");
    let text = screen_of(&mut app, 16);
    assert!(text.contains("two") && !text.contains("Reading"), "{text}");
    assert_eq!(app.context(), Context::Viewer, "the keys are the viewer's");
    let rest = ViewerPart::new(want, b"three\n", (true, false), stamp);
    assert!(app.read(id, Ok(rest)).is_empty());
    let text = screen_of(&mut app, 16);
    assert!(text.contains("1/3 End") && text.contains("three"), "{text}");
}

#[test]
fn a_file_on_a_host_is_read_only_as_far_as_the_screen_needs() {
    let (mut app, _connection) = on_a_host();
    app.handle(action(Action::Down));
    let Effect::Read(ReadRequest {
        id, want, location, ..
    }) = one(app.handle(action(Action::View)))
    else {
        panic!("expected a read");
    };
    assert_eq!(location, remote("web", "/home/deploy/notes"));
    let stamp = Stamp {
        size: Some(1 << 20),
        modified: None,
    };
    let first = ViewerPart::new(want, b"one\ntwo\n", (false, false), stamp);
    assert!(app.read(id, Ok(first)).is_empty(), "nothing ahead of it");
}

#[test]
fn a_part_of_a_file_on_a_host_has_a_size_of_its_own() {
    let first = |read: Effect| {
        let Effect::Read(ReadRequest { want, .. }) = read else {
            panic!("expected a read");
        };
        (want.offset, want.length)
    };
    let (mut app, _connection) = on_a_host();
    app.handle(action(Action::Down));
    let read = one(app.handle(action(Action::View)));
    assert_eq!(first(read), (0, 256 << 10));
    // The setting takes effect at the next F3.
    app.handle(action(Action::Cancel));
    app.config.viewer.host_part_kib = NonZeroUsize::new(64).unwrap();
    let read = one(app.handle(action(Action::View)));
    assert_eq!(first(read), (0, 64 << 10));

    let (_app, read) = view_big();
    assert_eq!(first(read), (0, 1 << 20), "a local file's");
}

/// Forty lines, `line 1` to `line 40`.
fn forty_lines() -> String {
    (1..=40)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// An app on `/srv`, which holds the files `names`, viewing the first of them, whose text is
/// `text`.
fn viewing(names: &[&str], text: impl Into<Vec<u8>>) -> App {
    let mut app = with_files(names);
    app.handle(action(Action::Down));
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
        panic!("expected a read");
    };
    app.read(id, Ok(whole(text)));
    screen_of(&mut app, 12);
    app
}

/// The file the only effect of `effects` reads for the viewer, and from where.
fn read_of(effects: Vec<Effect>) -> (Location, u64) {
    let Effect::Read(ReadRequest { location, want, .. }) = one(effects) else {
        panic!("expected a read");
    };
    (location, want.offset)
}

#[test]
fn e_and_f4_in_the_viewer_edit_its_file_at_the_line_on_top_which_it_reads_again() {
    let mut app = with_files(&["a1"]);
    app.handle(action(Action::Down));
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
        panic!("expected a read");
    };
    // Not before its start is read.
    assert!(!app.supports(Action::Edit));
    assert!(keys(&mut app, &["e"]).is_empty());
    assert_eq!(app.take_edit(), None);
    app.read(id, Ok(whole(forty_lines())));
    assert!(
        screen_of(&mut app, 12).contains("4Edit"),
        "on the F-key bar"
    );
    keys(&mut app, &["j", "j"]);
    assert!(keys(&mut app, &["e"]).is_empty(), "text, and small");
    assert_matches!(
        app.take_handover(),
        Some(Handover::Edit { file, line: Some(3) }) if file == Path::new("/srv/a1"),
    );
    assert_eq!(app.context(), Context::Viewer, "it stays");
    // A file the editor changed is read again, from its start up to the line on top.
    let effects = app.edited(Ok(true));
    let reads: Vec<u64> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Read(ReadRequest { location, want, .. }) if *location == local("/srv/a1") => {
                Some(want.offset)
            }
            _ => None,
        })
        .collect();
    assert_eq!(reads, [0], "{effects:?}");
    let text: String = forty_lines().replace("line", "row");
    let id = app.viewing.as_ref().unwrap().id;
    app.read(id, Ok(whole(text)));
    let screen = screen_of(&mut app, 12);
    assert!(
        screen.contains("row 3") && !screen.contains("row 2 "),
        "{screen}"
    );
    // One it did not change is not.
    keys(&mut app, &["f4"]);
    assert!(app.take_edit().is_some());
    let effects = app.edited(Ok(false));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Read(_))),
        "{effects:?}"
    );
}

#[test]
fn a_file_that_looks_binary_or_is_large_is_edited_after_a_question() {
    let mut app = viewing(&["a1"], *b"\x7fELF\x02\x01\x01\0\0\0");
    assert!(keys(&mut app, &["e"]).is_empty());
    let text = screen_of(&mut app, 14);
    assert!(text.contains("looks like a binary file"), "{text}");
    assert!(
        text.contains("[ Edit ] { Cancel }"),
        "Cancel the default: {text}"
    );
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert_eq!(app.take_edit(), None);
    assert_eq!(app.context(), Context::Viewer);
    keys(&mut app, &["e"]);
    app.handle(action(Action::Left));
    app.handle(action(Action::Confirm));
    assert_eq!(app.take_edit(), Some(PathBuf::from("/srv/a1")));

    // F4 in the panel asks for a file larger than edit.large_mib, as for a binary one.
    let mut app = with_files(&["big"]);
    app.handle(action(Action::Down));
    let large = Sniffed {
        binary: false,
        size: Some(100 << 20),
    };
    let effects = app.handle(action(Action::Edit));
    assert!(answer_sniff(&mut app, effects, large).is_empty());
    let text = screen_of(&mut app, 14);
    assert!(text.contains("\"big\" is 100M"), "{text}");
    app.handle(action(Action::Cancel));
    assert_eq!(app.take_edit(), None);
    let both = Sniffed {
        binary: true,
        ..large
    };
    let effects = app.handle(action(Action::Edit));
    answer_sniff(&mut app, effects, both);
    let text = screen_of(&mut app, 14);
    assert!(text.contains("binary file, and is 100M"), "{text}");
    app.handle(action(Action::Cancel));
    // 0 asks for no size.
    app.config.edit.large_mib = 0;
    let effects = app.handle(action(Action::Edit));
    answer_sniff(&mut app, effects, large);
    assert_eq!(app.take_edit(), Some(PathBuf::from("/srv/big")));
    // A file it cannot look at goes to the editor all the same, which says why.
    let Effect::Sniff { id, .. } = one(app.handle(action(Action::Edit))) else {
        panic!("expected a look at the file");
    };
    app.sniffed(id, Err("permission denied".to_owned().into()));
    assert_eq!(app.take_edit(), Some(PathBuf::from("/srv/big")));

    // On a host, the file is looked at there, before it is copied, which the question says.
    let (mut app, _) = on_a_host();
    let Effect::Sniff { location, host, .. } = one(app.handle(action(Action::Edit))) else {
        panic!("expected a look at the file");
    };
    assert_eq!(
        (location, host.is_some()),
        (remote("web", "/home/deploy/notes"), true)
    );
    let effects = app.handle(action(Action::Edit));
    answer_sniff(&mut app, effects, both);
    let text = screen_of(&mut app, 14);
    assert!(text.contains("It is copied from web first."), "{text}");
    app.handle(action(Action::Left));
    let (_, sources, ..) = copy_job(app.handle(action(Action::Confirm)));
    assert_eq!(sources, [remote("web", "/home/deploy/notes")]);
}

#[test]
fn a_remote_file_edited_from_the_viewer_is_read_again_once_it_went_back() {
    let (mut app, _) = on_a_host();
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
        panic!("expected a read");
    };
    app.read(id, Ok(whole(forty_lines())));
    screen_of(&mut app, 12);
    let (id, ..) = copy_job(keys(&mut app, &["e"]));
    app.job_event(id, JobEvent::Finished { complete: true });
    app.take_edit().unwrap();
    let (id, ..) = copy_job(app.edited(Ok(true)));
    let effects = app.job_event(id, JobEvent::Finished { complete: true });
    let read = effects.iter().any(|effect| {
        matches!(effect, Effect::Read(ReadRequest { location, want, .. })
            if *location == remote("web", "/home/deploy/notes") && want.offset == 0)
    });
    assert!(read, "{effects:?}");
}

#[test]
fn g_x_in_the_viewer_opens_its_file_externally_and_r_reads_it_again() {
    let mut app = viewing(&["a1"], forty_lines());
    let effects = keys(&mut app, &["g", "x"]);
    assert_matches!(
        &effects[..],
        [Effect::Open(paths)] if paths == &[PathBuf::from("/srv/a1")],
        "{effects:?}",
    );
    keys(&mut app, &["j", "j"]);
    assert_eq!(read_of(keys(&mut app, &["shift-r"])), (local("/srv/a1"), 0));
    assert_eq!(app.context(), Context::Viewer);

    // On a host, the line of the mode says that only local files open so.
    let (mut app, _) = on_a_host();
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
        panic!("expected a read");
    };
    app.read(id, Ok(whole("text")));
    assert!(keys(&mut app, &["g", "x"]).is_empty());
    assert!(screen(&mut app).contains("Only local files open externally"));
}

#[test]
fn bracket_f_goes_through_the_files_of_the_panel_with_its_cursor() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let entries = vec![file("a1", 1), dir("sub"), file("b2", 1), file("c3", 1)];
    answer(&mut app, effects, &Listing::Dir(entries));
    app.handle(action(Action::Down));
    app.handle(action(Action::Down));
    assert_eq!(cursor_name(&app, Side::Left), "a1");
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::View))) else {
        panic!("expected a read");
    };
    app.read(id, Ok(whole("a")));
    assert!(keys(&mut app, &["[", "f"]).is_empty(), "the first");
    // Its lines stay cut, past the directory.
    keys(&mut app, &["f2"]);
    assert_eq!(read_of(keys(&mut app, &["]", "f"])), (local("/srv/b2"), 0));
    assert_eq!(cursor_name(&app, Side::Left), "b2");
    assert!(!app.viewing.as_ref().unwrap().viewer.wraps());
    assert_eq!(read_of(keys(&mut app, &["]", "f"])), (local("/srv/c3"), 0));
    assert!(keys(&mut app, &["]", "f"]).is_empty(), "the last");
    // With a count, that many, no further than the first.
    let typed = Typed {
        resolved: action(Action::PrevFile),
        extra: Extra {
            count: Some(5),
            ..Extra::default()
        },
    };
    assert_eq!(read_of(app.typed(typed)), (local("/srv/a1"), 0));
    keys(&mut app, &["q"]);
    assert_eq!(cursor_name(&app, Side::Left), "a1", "where the viewer was");
}

#[test]
fn bracket_f_in_the_log_goes_through_the_logs_of_its_directory() {
    let mut app = with_files(&["a1"]);
    app.about.files = Some(files(Ok(PathBuf::from(LOG))));
    let Effect::Read(ReadRequest { id, .. }) = one(app.handle(action(Action::ViewLog))) else {
        panic!("expected a read");
    };
    app.read(id, Ok(whole("the last line\n")));
    let Effect::ListSiblings {
        id,
        dir: listed,
        host,
    } = one(keys(&mut app, &["]", "f"]))
    else {
        panic!("expected a listing");
    };
    assert_eq!(
        (listed, host.is_none()),
        (local("/home/me/.local/state/noc/logs"), true)
    );
    let logs = vec![
        file("noc-20261106T090000Z-7.log", 1),
        file("noc-20261104T080000Z-3.log", 1),
        file("noc-20261105T120000Z-42.log", 1),
        dir("old"),
    ];
    let next = local("/home/me/.local/state/noc/logs/noc-20261106T090000Z-7.log");
    assert_eq!(read_of(app.siblings(id, Ok(logs.clone()))), (next, 0));
    assert!(app.siblings(id, Ok(logs.clone())).is_empty(), "once");
    let Effect::ListSiblings { id, .. } = one(keys(&mut app, &["[", "f"])) else {
        panic!("expected a listing");
    };
    let before = local("/home/me/.local/state/noc/logs/noc-20261105T120000Z-42.log");
    assert_eq!(read_of(app.siblings(id, Ok(logs))), (before, 0));
    let Effect::ListSiblings { id, .. } = one(keys(&mut app, &["[", "f"])) else {
        panic!("expected a listing");
    };
    app.siblings(id, Err("permission denied".to_owned().into()));
    assert!(screen(&mut app).contains("Cannot list"));
}

#[test]
fn f_follows_the_end_of_the_file_as_it_grows_until_it_scrolls_up() {
    let text = format!("{}\n", forty_lines());
    let mut app = viewing(&["a1"], text.clone());
    let stamp = |size: usize| Stamp {
        size: Some(size as u64),
        modified: None,
    };
    let size = text.len();
    let Effect::Poll(PollRequest { id, location, .. }) = one(keys(&mut app, &["shift-f"])) else {
        panic!("expected a look at the file");
    };
    assert_eq!(location, local("/srv/a1"));
    assert!(screen_of(&mut app, 12).contains("Follow"));
    let Effect::Poll(PollRequest { id, .. }) = one(app.polled(id, Ok(stamp(size)))) else {
        panic!("expected another look, the file as it was");
    };
    // What was added is read; the next look comes once it is.
    let grown = format!("{text}line 41\n");
    let Effect::Read(ReadRequest { id: read, want, .. }) =
        one(app.polled(id, Ok(stamp(grown.len()))))
    else {
        panic!("expected a read");
    };
    assert_eq!(want.offset, size as u64);
    let part = ViewerPart::new(
        want,
        &grown.as_bytes()[size..],
        (true, false),
        stamp(grown.len()),
    );
    let Effect::Poll(PollRequest { id, .. }) = one(app.read(read, Ok(part))) else {
        panic!("expected another look");
    };
    let text = screen_of(&mut app, 12);
    assert!(
        text.contains("line 41") && text.contains("Follow"),
        "{text}"
    );
    // A move up stops it; a look on its way changes nothing.
    keys(&mut app, &["k"]);
    assert!(app.polled(id, Ok(stamp(1 << 20))).is_empty());
    assert!(!screen_of(&mut app, 12).contains("Follow"));
    // F again stops it too; a file that cannot be looked at stops it, and says why.
    let Effect::Poll(PollRequest { id, .. }) = one(keys(&mut app, &["shift-f"])) else {
        panic!("expected a look at the file");
    };
    assert!(
        app.polled(id, Err("no such file".to_owned().into()))
            .is_empty()
    );
    assert!(screen(&mut app).contains("Stopped following /srv/a1: no such file"));
    keys(&mut app, &["shift-f"]);
    assert!(keys(&mut app, &["shift-f"]).is_empty(), "stopped");
}

#[test]
fn a_file_that_changed_between_parts_is_read_again() {
    let (mut app, read) = view_big();
    let Effect::Read(ReadRequest { id, want, .. }) = read else {
        panic!("expected a read");
    };
    assert!(
        app.read(id, Ok(part_of_big(want, b"one\ntwo\n")))
            .is_empty()
    );
    screen_of(&mut app, 16);
    let Effect::Read(ReadRequest { id, want, .. }) = one(app.handle(action(Action::PageDown)))
    else {
        panic!("expected the next part");
    };
    let mut part = part_of_big(want, b"three\n");
    part.stamp.size = Some(2000);
    let Effect::Read(ReadRequest { want, .. }) = one(app.read(id, Ok(part))) else {
        panic!("expected the file again");
    };
    assert_eq!(want.offset, 0);
    let text = screen_of(&mut app, 16);
    assert!(text.contains("/srv/big changed"), "{text}");
}
