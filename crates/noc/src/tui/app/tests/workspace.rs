use super::*;

/// Saves the tabs of both panels as `name` with t W, and returns what is saved.
fn save_as(app: &mut App, name: &str) -> Workspace {
    assert!(app.handle(action(Action::SaveWorkspace)).is_empty());
    app.handle(action(Action::DeleteToStart));
    typed(app, name);
    let Effect::Workspaces(WorkspaceChange::Save(workspace)) =
        one(app.handle(action(Action::Confirm)))
    else {
        panic!("expected a workspace to save");
    };
    workspace
}

/// `workspaces.toml` holds `saved` now.
fn holds(app: &mut App, saved: &[Workspace]) {
    let workspaces = Workspaces {
        workspaces: saved.to_vec(),
    };
    app.workspaces_changed(WorkspaceChange::Load, Ok(workspaces));
}

/// A workspace: on the left `~/src`, sorted by time, with the cursor on `notes`, then `web:/var/www`,
/// which shows; on the right `/`, which has the keys.
fn noon() -> Workspace {
    use noc_config::{Place, SavedTab, SortBy};

    let tab = |place: Place| SavedTab {
        place,
        sort: SortBy::Name,
        descending: false,
        cursor: None,
        current: false,
    };
    Workspace {
        name: "noon".to_owned(),
        active: PanelSide::Right,
        left: vec![
            SavedTab {
                sort: SortBy::Time,
                descending: true,
                cursor: Some("notes".to_owned()),
                ..tab(Place::Local("~/src".to_owned()))
            },
            SavedTab {
                current: true,
                ..tab(Place::Remote {
                    host: "web".to_owned(),
                    path: "/var/www".to_owned(),
                })
            },
        ],
        right: vec![tab(Place::Local("/".to_owned()))],
    }
}

#[test]
fn alt_shift_w_saves_the_tabs_of_both_panels_under_a_name() {
    use noc_config::{Place, SortBy};

    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::SortBySize));
    app.handle(action(Action::NewTab));
    app.handle(action(Action::PrevTab));
    app.handle(action(Action::SwitchPanel));
    let workspace = save_as(&mut app, " noon ");
    assert_eq!(workspace.name, "noon", "without the spaces around it");
    assert_eq!(workspace.active, PanelSide::Right);
    let left = &workspace.left;
    assert_eq!(left.len(), 2);
    assert_eq!(left[0].place, Place::Local("/srv".to_owned()));
    assert_eq!(left[0].cursor.as_deref(), Some("left"));
    assert_eq!((left[0].sort, left[0].descending), (SortBy::Size, true));
    assert!(left[0].current && !left[1].current);
    assert!(!workspace.right[0].current, "one tab needs no mark");
    assert_eq!(app.workspace.as_deref(), Some("noon"));

    // Its name comes back, and saving it again asks nothing.
    holds(&mut app, &[workspace]);
    app.handle(action(Action::SaveWorkspace));
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("Save workspace") && text.contains("noon"),
        "{text}"
    );
    assert!(matches!(
        one(app.handle(action(Action::Confirm))),
        Effect::Workspaces(WorkspaceChange::Save(_))
    ));
    // An empty name saves nothing.
    app.handle(action(Action::SaveWorkspace));
    app.handle(action(Action::DeleteToStart));
    assert!(app.handle(action(Action::Confirm)).is_empty());
}

#[test]
fn the_name_of_another_workspace_asks_before_replacing_it() {
    let mut app = loaded();
    holds(&mut app, &[noon()]);
    app.handle(action(Action::SaveWorkspace));
    typed(&mut app, "noon");
    assert!(app.handle(action(Action::Confirm)).is_empty(), "asks first");
    let text = screen_of(&mut app, 16);
    assert!(text.contains("Replace it"), "{text}");
    assert!(text.contains("[ Yes ] { No }"), "No is the default: {text}");
    let Effect::Workspaces(WorkspaceChange::Save(workspace)) =
        one(app.handle(Resolved::Hotkey('y')))
    else {
        panic!("expected a workspace to save");
    };
    assert_eq!(
        workspace.right[0].place,
        noc_config::Place::Local("/srv".to_owned())
    );
}

#[test]
fn restoring_replaces_every_tab_and_hidden_ones_list_when_they_show() {
    let mut app = loaded();
    app.handle(action(Action::NewTab));
    holds(&mut app, &[noon()]);
    let effects = app.restore_workspace("noon");
    // The tab that shows on the left waits for web; the hidden one lists nothing yet.
    let [
        Effect::Connect { connection, .. },
        Effect::List {
            panel: right,
            request,
            host: None,
        },
    ] = &effects[..]
    else {
        panic!("unexpected {effects:?}");
    };
    assert_eq!(right.side, Side::Right);
    assert_eq!(request.location, local("/"));
    assert_eq!(app.active, Side::Right);
    assert_eq!(tab_numbers(&app, Side::Left), (vec![4, 5], 5), "new tabs");
    assert_eq!(app.panel(Side::Left).location(), &remote("web", "/var/www"));
    assert_eq!(app.workspace.as_deref(), Some("noon"));

    // web connects: only the tab that shows lists.
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", *connection, handle);
    let [Effect::List { panel, .. }] = &effects[..] else {
        panic!("expected one listing, got {effects:?}");
    };
    assert_eq!(panel.tab, 5);

    // The hidden one asks once it shows, sorted and with its cursor as saved.
    app.active = Side::Left;
    let effects = app.handle(action(Action::PrevTab));
    let [Effect::List { panel, request, .. }] = &effects[..] else {
        panic!("expected a listing, got {effects:?}");
    };
    assert_eq!((panel.tab, &request.location), (4, &local("/home/me/src")));
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("a"), dir("notes"), dir("z")]),
    );
    assert_eq!(
        app.panel(Side::Left).name_under_cursor(),
        Some(&b"notes"[..])
    );
    assert_eq!(
        app.panel(Side::Left).sort_action(),
        Action::SortByTime,
        "sorted as saved"
    );
    assert!(
        app.handle(action(Action::NextTab)).is_empty()
            && app.handle(action(Action::PrevTab)).is_empty(),
        "listed once"
    );
}

#[test]
fn a_workspace_gone_meanwhile_says_so() {
    let mut app = loaded();
    assert!(app.restore_workspace("noon").is_empty());
    assert!(screen_of(&mut app, 16).contains("is not saved any more"));
}

#[test]
fn f9_lists_the_workspaces_and_restores_one_by_its_digit() {
    let mut app = loaded();
    let mut other = noon();
    other.name = "photos".to_owned();
    holds(&mut app, &[other, noon()]);
    app.handle(action(Action::PullDown));
    app.handle(Resolved::Hotkey('w'));
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("Save workspace…") && text.contains("1 photos") && text.contains("2 noon"),
        "{text}"
    );
    let effects = app.handle(Resolved::Hotkey('2'));
    assert!(!effects.is_empty());
    assert_eq!(app.workspace.as_deref(), Some("noon"));
    app.handle(action(Action::PullDownLast));
    assert!(
        screen_of(&mut app, 16).contains(&format!("{} 2 noon", app.theme.glyphs.choice)),
        "the one restored last"
    );
}

#[test]
fn the_workspaces_window_saves_restores_renames_and_deletes() {
    let mut app = loaded();
    let mut photos = noon();
    photos.name = "photos".to_owned();
    holds(&mut app, &[noon(), photos]);
    app.workspace = Some("noon".to_owned());
    assert!(app.handle(action(Action::Workspaces)).is_empty());
    assert_eq!(app.context(), Context::Workspaces);

    // a saves the tabs as a new workspace: the name starts empty.
    app.handle(action(Action::SaveWorkspace));
    assert_eq!(app.context(), Context::DialogInput);
    typed(&mut app, "srv");
    let effects = app.handle(action(Action::Confirm));
    let [Effect::Workspaces(WorkspaceChange::Save(saved))] = &effects[..] else {
        panic!("unexpected {effects:?}");
    };
    assert_eq!(saved.name, "srv");
    let mut photos = noon();
    photos.name = "photos".to_owned();
    holds(&mut app, &[noon(), photos, saved.clone()]);
    assert_eq!(app.context(), Context::Workspaces, "still open");
    assert!(
        matches!(
            &app.handle(action(Action::Confirm))[..],
            [Effect::List { .. }, ..]
        ),
        "the cursor went to the new one, which restores at once"
    );
    assert_eq!(app.workspace.as_deref(), Some("srv"));
    app.workspace = Some("noon".to_owned());
    holds(
        &mut app,
        &[noon(), {
            let mut photos = noon();
            photos.name = "photos".to_owned();
            photos
        }],
    );
    app.open_workspaces();
    let text = screen_of(&mut app, 16);
    assert!(
        text.contains("Workspaces") && text.contains("2 │ 1") && text.contains("web:/var/www"),
        "{text}"
    );
    assert!(
        text.contains("6Rename") && text.contains("8Delete"),
        "{text}"
    );

    // F6: a new name in place; the one restored last follows it.
    app.handle(action(Action::Move));
    app.handle(action(Action::DeleteToStart));
    typed(&mut app, "noc");
    let effects = app.handle(action(Action::Confirm));
    let [Effect::Workspaces(WorkspaceChange::Rename { from, to })] = &effects[..] else {
        panic!("unexpected {effects:?}");
    };
    assert_eq!((from.as_str(), to.as_str()), ("noon", "noc"));
    assert_eq!(app.workspace.as_deref(), Some("noc"));
    let mut renamed = noon();
    renamed.name = "noc".to_owned();
    let mut photos = noon();
    photos.name = "photos".to_owned();
    holds(&mut app, &[renamed, photos]);

    // F6 to the name of another one asks first, No by default.
    app.handle(action(Action::Move));
    app.handle(action(Action::DeleteToStart));
    typed(&mut app, "photos");
    assert!(app.handle(action(Action::Confirm)).is_empty());
    assert!(app.handle(action(Action::Confirm)).is_empty(), "No");

    // F8 asks, Yes by default.
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    assert!(screen_of(&mut app, 16).contains("Delete workspace \"photos\"?"));
    let effects = app.handle(action(Action::Confirm));
    assert!(
        matches!(&effects[..], [Effect::Workspaces(WorkspaceChange::Remove(name))] if name == "photos"),
        "{effects:?}"
    );

    // Enter restores and closes the window.
    app.handle(action(Action::Home));
    assert!(!app.handle(action(Action::Confirm)).is_empty());
    assert!(app.workspaces_window.is_none());
}

#[test]
fn a_file_that_cannot_be_read_or_written_says_why() {
    let mut app = loaded();
    let invalid = Err("invalid workspace".to_owned().into());
    app.workspaces_changed(WorkspaceChange::Load, invalid);
    assert!(screen_of(&mut app, 16).contains("Cannot read the workspaces"));
    // Retry reads it again.
    let again = one(app.handle(action(Action::Confirm)));
    assert!(
        matches!(again, Effect::Workspaces(WorkspaceChange::Load)),
        "{again:?}"
    );
    let removed = WorkspaceChange::Remove("noon".to_owned());
    app.workspaces_changed(removed, Err("cannot write".to_owned().into()));
    let text = screen_of(&mut app, 16);
    assert!(text.contains("Cannot save the workspaces"), "{text}");
    assert!(text.contains("{ Retry } [ Cancel ]"), "{text}");
    assert!(app.handle(action(Action::Cancel)).is_empty());

    // The configuration and the history are written again too.
    let (old, new) = (Box::new(Config::default()), Box::new(app.config.clone()));
    app.config_saved(old, new, Err("cannot write".to_owned().into()));
    let again = one(app.handle(action(Action::Confirm)));
    assert!(matches!(again, Effect::SaveConfig { .. }), "{again:?}");
    let change = HistoryChange::Remove {
        host: None,
        command: "ls".to_owned(),
    };
    app.history_changed(change.clone(), Err("cannot write".to_owned().into()));
    let again = one(app.handle(action(Action::Confirm)));
    assert!(
        matches!(&again, Effect::History(asked) if *asked == change),
        "{again:?}"
    );
}
