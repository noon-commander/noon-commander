use super::*;

#[test]
fn f8_asks_deletes_and_reads_the_directory_again() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    assert_eq!(app.context(), Context::Dialog);
    let text = screen(&mut app);
    assert!(
        text.contains("Delete directory \"left\" and everything in it?"),
        "{text}"
    );
    assert!(
        text.contains("{ Yes }"),
        "Yes is the default, as in mc: {text}"
    );
    let (id, targets, remote, _) = delete_job(app.handle(action(Action::Confirm)));
    assert_eq!((targets, remote), (vec![local("/srv/left")], false));

    assert_eq!(
        app.context(),
        Context::Dialog,
        "the job's window takes the keys"
    );
    let counting = JobEvent::Scanning {
        at: Instant::now(),
        items: 3,
    };
    app.job_event(id, counting);
    assert!(screen(&mut app).contains("3 found"));
    let current = local("/srv/left/a");
    let progress = report(
        current,
        Counts {
            done: 1,
            total: 3,
            ..Counts::default()
        },
    );
    app.job_event(id, progress);
    let text = screen(&mut app);
    assert!(
        text.contains("/srv/left/a") && text.contains("1 of 3"),
        "{text}"
    );

    // A failure waits for an answer: what failed, the path, and why, each on a line of its
    // own, with the code of the system; what to do about the failures after it; and Retry
    // first, the default, then Skip and Abort.
    let (reply, mut decision) = oneshot::channel();
    let failed = JobEvent::Failed {
        at: Instant::now(),
        path: local("/srv/left/b"),
        step: Step::Remove,
        reason: Reason {
            text: "Permission denied".to_owned(),
            code: Some(Code::Os(13)),
        },
        retry: true,
        reply,
    };
    app.job_event(id, failed);
    let text = screen_of(&mut app, 20);
    assert!(
        shows_problem(
            &text,
            "Cannot delete",
            Some("/srv/left/b"),
            "Permission denied"
        ),
        "{text}"
    );
    assert!(text.contains("Code: 13"), "{text}");
    assert!(text.contains("The rest: "), "{text}");
    assert!(text.contains("{ Retry } [ Skip ] [ Abort ]"), "{text}");
    // Alt+t opens the list of the rest, where j goes down and Enter chooses.
    app.handle(Resolved::Hotkey('t'));
    assert!(
        screen_of(&mut app, 20).contains("Skip failures with the same code"),
        "a code to skip by"
    );
    app.handle(action(Action::Down));
    app.handle(action(Action::Down));
    app.handle(action(Action::Confirm));
    app.handle(Resolved::Hotkey('s'));
    let skip_same = Resolution {
        this: Decision::Skip,
        rest: Later::SkipSame,
    };
    assert_eq!(decision.try_recv(), Ok(skip_same));

    // Reports of other jobs change nothing; the end reads /srv again in both panels.
    assert!(
        app.job_event(id + 1, JobEvent::Finished { complete: true })
            .is_empty()
    );
    let effects = app.job_event(id, JobEvent::Finished { complete: true });
    assert_eq!(effects.len(), 2);
    assert_eq!(app.context(), Context::Panel);
}

#[test]
fn f8_names_what_it_deletes_and_no_keeps_it() {
    let (mut app, effects) = App::new(Path::new("/srv"), Path::new("/home/me"), &config());
    let entries = vec![file("a.txt", 1), file("b.txt", 2), dir("c")];
    answer(&mut app, effects, &Listing::Dir(entries));
    app.handle(action(Action::Delete));
    assert!(app.dialogs.is_empty(), "nothing to delete on `..`");

    app.handle(action(Action::End));
    app.handle(action(Action::Delete));
    assert!(screen(&mut app).contains("Delete file \"b.txt\"?"));
    app.handle(action(Action::Right));
    assert!(app.handle(action(Action::Confirm)).is_empty(), "No");

    app.handle(action(Action::InvertMarks));
    app.handle(action(Action::Delete));
    assert!(screen(&mut app).contains("Delete 2 files?"));
    let (_, targets, _, _) = delete_job(app.handle(action(Action::Confirm)));
    assert_eq!(targets, [local("/srv/a.txt"), local("/srv/b.txt")]);
}

#[test]
fn esc_aborts_a_job_and_its_questions_go_with_it() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (id, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
    app.handle(action(Action::Cancel));
    assert!(cancel.is_cancelled());
    assert!(screen(&mut app).contains("Aborting"));

    let (reply, mut decision) = oneshot::channel();
    let path = local("/srv/left/x");
    let error = "busy".to_owned();
    app.job_event(id, failed(path, error, reply));
    app.job_event(id, JobEvent::Finished { complete: true });
    assert_eq!(
        app.context(),
        Context::Panel,
        "the question went with the job"
    );
    assert!(decision.try_recv().is_err());

    // Esc in a failure answers Abort.
    app.handle(action(Action::Delete));
    let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    let (reply, mut decision) = oneshot::channel();
    let path = local("/srv/left/y");
    let error = "busy".to_owned();
    app.job_event(id, failed(path, error, reply));
    app.handle(action(Action::Cancel));
    assert_eq!(decision.try_recv(), Ok(Resolution::once(Decision::Abort)));
}

#[test]
fn enter_sends_a_job_to_the_background_where_others_join_it() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (first, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
    app.handle(action(Action::Confirm));
    assert!(!cancel.is_cancelled());
    assert_eq!(app.context(), Context::Panel, "the panels take the keys");
    let progress = report(
        local("/srv/left/a"),
        Counts {
            done: 1,
            total: 4,
            ..Counts::default()
        },
    );
    app.job_event(first, progress);
    let text = screen(&mut app);
    assert!(text.contains(" 1 job 25% ╗"), "{text}");

    // Another job starts in front, and goes behind too.
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (second, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    assert_eq!(app.context(), Context::Dialog);
    app.handle(action(Action::Confirm));
    assert!(screen(&mut app).contains(" 2 jobs 13% "));

    // A question from behind shows over the panels.
    let (reply, mut decision) = oneshot::channel();
    let path = local("/srv/right/x");
    let error = "busy".to_owned();
    app.job_event(second, failed(path, error, reply));
    assert_eq!(app.context(), Context::Dialog);
    app.handle(action(Action::Confirm));
    assert_eq!(decision.try_recv(), Ok(Resolution::once(Decision::Retry)));

    // F10 asks while jobs run; No, the default, goes on.
    app.handle(action(Action::Quit));
    let text = screen_of(&mut app, 12);
    assert!(
        text.contains("2 jobs are still running. Quit and stop them?"),
        "{text}"
    );
    app.handle(action(Action::Confirm));
    assert!(!app.quits());

    // A job that ends reads its directory again.
    let effects = app.job_event(first, JobEvent::Finished { complete: true });
    assert_eq!(effects.len(), 2, "both panels show /srv");
    assert!(screen(&mut app).contains(" 1 job 0% "));
    app.job_event(second, JobEvent::Finished { complete: true });
    assert!(!screen(&mut app).contains(" job"));
    app.handle(action(Action::Quit));
    let text = screen_of(&mut app, 12);
    assert!(text.contains("Quit Noon Commander?"), "{text}");
    app.handle(action(Action::Confirm));
    assert!(app.quits(), "Yes, the default, quits");
}

#[test]
fn a_job_in_the_background_draws_when_its_percentage_changes() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    let changes = |app: &mut App, done| {
        let progress = report(
            local("/srv/left/a"),
            Counts {
                done,
                total: 1000,
                ..Counts::default()
            },
        );
        let (effects, changes) = app.job_report(id, progress);
        assert!(effects.is_empty());
        changes
    };
    assert!(changes(&mut app, 1), "its window shows every report");
    assert!(changes(&mut app, 2));

    app.handle(action(Action::Confirm));
    assert!(screen(&mut app).contains(" 1 job 0% "));
    assert!(!changes(&mut app, 3), "still 0%");
    assert!(changes(&mut app, 10));
    assert!(screen(&mut app).contains(" 1 job 1% "));
    assert!(!changes(&mut app, 11));

    app.handle(action(Action::Jobs));
    assert!(
        !changes(&mut app, 12),
        "its row shows 1%, as the corner does"
    );
    app.handle(action(Action::Cancel));
    assert!(app.jobs_list.is_none());
    assert!(!changes(&mut app, 13));

    let (_, changes) = app.job_report(id, JobEvent::Finished { complete: true });
    assert!(changes, "its end shows");
}

#[test]
fn the_list_of_jobs_draws_for_a_job_behind_only_when_its_row_changes() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    app.handle(action(Action::Confirm));
    app.handle(action(Action::Jobs));
    let changes = |app: &mut App, current: &str, done, bytes_total| {
        let progress = report(
            local(current),
            Counts {
                done,
                total: 1000,
                bytes_total,
                ..Counts::default()
            },
        );
        app.job_report(id, progress).1
    };
    assert!(changes(&mut app, "/srv/left/a", 1, 0), "counted, it works");
    assert!(
        !changes(&mut app, "/srv/left/a", 2, 0),
        "its row shows no count"
    );
    assert!(changes(&mut app, "/srv/left/b", 3, 0), "the entry at hand");
    assert_eq!(app.spin_due(Instant::now()), None, "its row shows no time");
    changes(&mut app, "/srv/left/b", 3, 1024);
    assert!(
        app.spin_due(Instant::now()).is_some(),
        "the time left shows"
    );
}

#[test]
fn a_job_in_front_draws_only_when_its_window_changes() {
    const MIB: u64 = 1024 * 1024;
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    let mut changes = |done, bytes_done| {
        let progress = report(
            local("/srv/left/a"),
            Counts {
                done,
                total: 2,
                bytes_done,
                bytes_total: 100 * MIB,
                bytes_copied: bytes_done,
            },
        );
        app.job_report(id, progress).1
    };
    assert!(changes(0, 20 * MIB), "how far it is shows");
    assert!(!changes(0, 20 * MIB + 1_000), "still 20.0M of 100M, 20%");
    assert!(changes(0, 20 * MIB + 200 * 1024), "20.2M");
    assert!(changes(1, 20 * MIB + 200 * 1024), "1 of 2");
}

#[test]
fn the_clock_of_a_job_stands_while_its_question_is_open() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (first, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    // The first goes behind the panels, and the second stays in front.
    app.handle(action(Action::Confirm));
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (second, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    let ticking = |app: &App, id| {
        let job = app.jobs.iter().find(|job| job.id == id).unwrap();
        job.view.ticking()
    };
    for id in [first, second] {
        let progress = report(
            local("/srv/left/a"),
            Counts {
                done: 1,
                total: 4,
                ..Counts::default()
            },
        );
        app.job_event(id, progress);
    }
    assert!(ticking(&app, first) && ticking(&app, second));
    let now = Instant::now();
    let due = app.spin_due(now).unwrap();
    assert!(
        due > now && due <= now + Duration::from_millis(500),
        "the time in front shows 0:01 from half a second on"
    );

    // Both ask; the question of the second waits behind that of the first.
    let mut decisions = Vec::new();
    for id in [first, second] {
        let (reply, decision) = oneshot::channel();
        let path = local("/srv/left/b");
        let error = "busy".to_owned();
        app.job_event(id, failed(path, error, reply));
        decisions.push(decision);
    }
    assert!(!ticking(&app, first) && !ticking(&app, second));
    assert!(app.spin_due(now).is_none(), "nothing moves while they wait");

    app.handle(action(Action::Confirm));
    assert_eq!(
        decisions[0].try_recv(),
        Ok(Resolution::once(Decision::Retry))
    );
    assert!(ticking(&app, first), "the first goes on");
    assert!(
        !ticking(&app, second),
        "the second still waits for its answer"
    );
    app.handle(action(Action::Confirm));
    assert!(ticking(&app, second));

    // A taken name stops it as well.
    let (reply, _conflict) = oneshot::channel();
    let mut metadata = dir("a").metadata;
    metadata.kind = FileKind::File;
    let taken = TakenName {
        source: local("/srv/left/a"),
        target: local("/srv/right/a"),
        source_metadata: metadata.clone(),
        target_metadata: metadata,
        free: b"a_2".to_vec(),
        reply,
    };
    let exists = JobEvent::Exists {
        at: Instant::now(),
        taken: Box::new(taken),
    };
    app.job_event(second, exists);
    assert!(!ticking(&app, second));
    app.handle(action(Action::Cancel));
    assert!(ticking(&app, second));
}

#[test]
fn a_job_counts_its_reports_and_questions_at_the_times_it_made_them() {
    const MIB: u64 = 1024 * 1024;
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (id, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    // Twenty seconds of work at a mebibyte a second that began half a minute ago, taken
    // only now, as by an event loop held up, in batches of eight reports: no stall, and
    // 20 MiB left.
    let start = Instant::now().checked_sub(Duration::from_secs(30)).unwrap();
    let at = |millis| start + Duration::from_millis(millis);
    let reports: Vec<(Instant, Counts)> = (0..=80)
        .map(|tick| {
            let bytes = tick * MIB / 4;
            let counts = Counts {
                total: 1,
                bytes_done: bytes,
                bytes_total: 40 * MIB,
                bytes_copied: bytes,
                ..Counts::default()
            };
            (at(250 * tick), counts)
        })
        .collect();
    for batch in reports.chunks(8) {
        let current = local("/srv/left/a");
        let reports = batch.to_vec();
        app.job_event(id, JobEvent::Progress { current, reports });
    }
    let text = screen_at(&mut app, 12, at(20_000));
    assert!(text.contains("Time 0:20   ETA 0:20"), "{text}");

    // A question it asked at 25 seconds, answered at once at 30, stopped its clock at 25.
    let (reply, _decision) = oneshot::channel();
    let failed = JobEvent::Failed {
        at: at(25_000),
        path: local("/srv/left/a"),
        step: Step::Read,
        reason: "busy".to_owned().into(),
        retry: true,
        reply,
    };
    app.job_event(id, failed);
    app.handle(action(Action::Confirm));
    let text = screen_at(&mut app, 12, at(30_000));
    assert!(text.contains("Time 0:25"), "{text}");
}

#[test]
fn jobs_wait_their_turn() {
    let mut app = loaded();
    app.parallel_jobs = 1;
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (first, _, _, _) = delete_job(app.handle(action(Action::Confirm)));
    app.handle(action(Action::Confirm));
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    assert!(app.handle(action(Action::Confirm)).is_empty(), "it waits");
    let text = screen_of(&mut app, 12);
    assert!(text.contains("Waiting for other jobs to finish"), "{text}");
    app.handle(action(Action::Confirm));
    assert!(screen(&mut app).contains(" 2 jobs 0% "));

    // A third waits too; Abort takes it away at once, as it has not started.
    app.handle(action(Action::Delete));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);
    assert_eq!(app.jobs.len(), 2);

    // When the first ends, the second starts.
    let effects = app.job_event(first, JobEvent::Finished { complete: true });
    let started = deletes(&effects);
    assert_eq!(started.len(), 1, "{effects:?}");
    let (second, targets) = &started[0];
    assert_eq!(targets, &[local("/srv/right")]);
    let effects = app.job_event(*second, JobEvent::Finished { complete: true });
    assert_eq!(deletes(&effects), [] as [(u64, Vec<Location>); 0]);
    assert!(app.jobs.is_empty());
}

#[test]
fn ctrl_x_j_lists_the_jobs_to_show_or_abort() {
    let mut app = loaded();
    app.parallel_jobs = 1;
    app.handle(action(Action::Jobs));
    assert!(screen_of(&mut app, 12).contains("No jobs are running."));
    app.handle(action(Action::Cancel));
    assert_eq!(app.context(), Context::Panel);

    // Two jobs behind the panels, the second waiting.
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (first, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
    app.handle(action(Action::Confirm));
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    app.handle(action(Action::Confirm));
    app.handle(action(Action::Confirm));
    let progress = report(
        local("/srv/left/a"),
        Counts {
            done: 1,
            total: 2,
            ..Counts::default()
        },
    );
    app.job_event(first, progress);
    app.handle(action(Action::Jobs));
    assert_eq!(app.context(), Context::Dialog);
    let text = screen_of(&mut app, 12);
    assert!(
        text.contains("Delete") && text.contains("50%") && text.contains("/srv/left/a"),
        "{text}"
    );
    assert!(text.contains("waiting"), "{text}");

    // Abort takes the waiting one away at once, and stops the running one.
    app.handle(action(Action::Down));
    app.handle(action(Action::Right));
    app.handle(action(Action::Confirm));
    assert_eq!(app.jobs.len(), 1);
    app.handle(action(Action::Confirm));
    assert!(cancel.is_cancelled());
    assert!(screen_of(&mut app, 12).contains("aborting"));

    // Show brings it to the front, in place of the list.
    app.handle(action(Action::Left));
    app.handle(action(Action::Confirm));
    assert!(app.jobs_list.is_none());
    assert!(app.in_front().is_some());
    assert!(screen_of(&mut app, 12).contains("Aborting…"));
}

#[test]
fn a_host_given_up_makes_room_and_f4_never_waits() {
    let (mut app, connection) = on_a_host();
    app.parallel_jobs = 1;
    app.handle(action(Action::Delete));
    delete_job(app.handle(action(Action::Confirm)));
    app.handle(action(Action::Confirm));
    let (id, _, _, _, _) = copy_job(app.handle(action(Action::Edit)));
    app.job_event(id, JobEvent::Finished { complete: false });

    // A local job waits behind the one on the host, and starts when the host goes.
    let effects = enter_host(&mut app, Side::Right, 0);
    answer(&mut app, effects, &Listing::Dir(vec![file("a", 1)]));
    app.handle(action(Action::FocusRight));
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    assert!(app.handle(action(Action::Confirm)).is_empty());
    app.handle(action(Action::Confirm));
    // A lost host keeps its job, which waits for it.
    assert!(
        app.closed("web", connection, Some("Broken pipe"))
            .is_empty()
    );
    assert_eq!(app.jobs.len(), 2);
    // Cancel in Connection gives it up, and the job with it.
    let effects = app.handle(action(Action::Cancel));
    assert_eq!(deletes(&effects).len(), 1, "{effects:?}");
    assert_eq!(app.jobs.len(), 1);
}

#[test]
fn quitting_stops_the_jobs_and_yes_quits() {
    let mut app = loaded();
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (_, _, _, cancel) = delete_job(app.handle(action(Action::Confirm)));
    app.handle(action(Action::Confirm));
    app.handle(action(Action::Quit));
    assert!(screen_of(&mut app, 12).contains("1 job is still running. Quit and stop it?"));
    app.handle(action(Action::Left));
    app.handle(action(Action::Confirm));
    assert!(app.quits());
    assert!(!cancel.is_cancelled(), "not before the loop stops them");
    app.stop_jobs();
    assert!(cancel.is_cancelled());
}

#[test]
fn a_job_on_a_lost_host_waits_for_it_and_goes_on() {
    let mut app = at_slash();
    let Effect::Connect { connection, .. } = one(enter_host(&mut app, Side::Left, 1)) else {
        panic!("expected a connection");
    };
    let (handle, _requests) = HostHandle::channel();
    let effects = app.connected("web", connection, handle);
    answer(
        &mut app,
        effects,
        &Listing::Dir(vec![dir("www"), dir("old")]),
    );
    app.handle(action(Action::Down));
    app.handle(action(Action::Delete));
    let (id, targets, through_host, cancel) = delete_job(app.handle(action(Action::Confirm)));
    assert_eq!(targets, [remote("web", "www")]);
    assert!(through_host, "through a link to the host");
    let www = remote("web", "www");

    // A failure that came as the connection went asks, until the host is known to be lost.
    let (reply, mut first) = oneshot::channel();
    app.job_event(id, failed(www.clone(), "Broken pipe".to_owned(), reply));
    assert!(matches!(
        app.dialogs.front().map(|open| &open.purpose),
        Some(Purpose::Failure { .. })
    ));
    let effects = app.closed("web", connection, Some("Broken pipe"));
    assert!(effects.is_empty(), "the panel waits for an answer");
    assert_eq!(app.jobs.len(), 1, "the job waits");
    assert_eq!(app.dialogs.len(), 1, "no question about the job");
    assert!(matches!(
        app.dialogs.front().map(|open| &open.purpose),
        Some(Purpose::Reconnect { .. })
    ));
    let text = screen_of(&mut app, 16);
    assert!(text.contains("Lost the connection to web"), "{text}");
    // One that comes while it is lost waits too, without asking.
    let (reply, mut second) = oneshot::channel();
    app.job_event(id, failed(www.clone(), "Broken pipe".to_owned(), reply));
    assert_eq!(app.dialogs.len(), 1);
    assert!(first.try_recv().is_err(), "dropped for the later one");

    // Retry connects; once connected, the job tries again.
    let Effect::Connect { connection, .. } = one(app.handle(action(Action::Confirm))) else {
        panic!("expected a connection");
    };
    assert!(second.try_recv().is_err(), "not before the host is back");
    assert!(screen_of(&mut app, 16).contains("Waiting for web to connect…"));
    assert_eq!(app.jobs[0].view.summary(Instant::now()).state, "offline");
    let (handle, _requests) = HostHandle::channel();
    app.connected("web", connection, handle);
    assert_eq!(second.try_recv(), Ok(Resolution::once(Decision::Retry)));
    assert!(app.dialogs.is_empty());
    assert!(!cancel.is_cancelled());

    // Given up, a host takes its jobs with it.
    app.closed("web", connection, Some("Broken pipe"));
    app.handle(action(Action::Cancel));
    assert!(app.jobs.is_empty());
    assert!(cancel.is_cancelled());
}
