use std::path::Path;
use std::process::Stdio;

use noc_config::{Config, HostConfig, Hosts, Paths, SftpHost};
use noc_ops::{Event, Step};
use noc_tools::fd;
use noc_tools::fd::Fd;
use noc_tools::ripgrep::{Hit, Query, Ripgrep};
use noc_vfs::{RemotePath, SftpFs};
use tokio::process::Command;

use super::batching::Waiting;
use super::*;

/// A session with a local `sftp-server` that starts in `dir`; `None` without one.
async fn local_sftp(dir: &Path) -> Option<(tokio::process::Child, SftpFs)> {
    let program = std::env::var_os("SFTP_SERVER")
        .filter(|program| !program.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server"]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.exists())
        })?;
    let mut child = Command::new(program)
        .arg("-e")
        .arg("-d")
        .arg(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let fs = SftpFs::from_pipes(stdin, stdout).await.unwrap();
    Some((child, fs))
}

/// A handle to a host whose task shares `fs`, until the handle and its copies go.
fn sharing(fs: SftpFs) -> HostHandle {
    let (handle, mut requests) = HostHandle::channel();
    let fs = Arc::new(fs);
    tokio::spawn(async move {
        while let Some(request) = requests.recv().await {
            if let HostRequest::Share(reply) = request {
                let _ = reply.send(Arc::clone(&fs));
            }
        }
    });
    handle
}

#[tokio::test]
async fn a_job_takes_the_session_of_the_next_connection_once_one_is_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(root.join("a"), b"a").unwrap();
    let (Some((mut first_server, first)), Some((_second_server, second))) =
        (local_sftp(&root).await, local_sftp(&root).await)
    else {
        return;
    };
    let path = RemotePath::from(root.join("a").to_str().unwrap());
    let within = |work| tokio::time::timeout(Duration::from_secs(10), work);

    let relink = Relink::new(sharing(first));
    let fs = Rejoin::new(relink.link());
    assert!(within(fs.metadata(&path)).await.unwrap().is_ok());
    // The connection is lost: what the job tries fails, until the host is back.
    first_server.kill().await.unwrap();
    assert!(within(fs.metadata(&path)).await.unwrap().is_err());
    assert!(within(fs.metadata(&path)).await.unwrap().is_err());
    relink.connected(sharing(second));
    assert!(within(fs.metadata(&path)).await.unwrap().is_ok());

    // A host whose task is gone has no session to give.
    let (gone, requests) = HostHandle::channel();
    drop(requests);
    let fs = Rejoin::new(Relink::new(gone).link());
    assert!(within(fs.metadata(&path)).await.unwrap().is_err());
}

fn task(dir: &Path, start_dir: Option<&str>) -> HostTask {
    let mut hosts = Hosts::default();
    let host = SftpHost {
        start_dir: start_dir.map(str::to_owned),
        ..SftpHost::default()
    };
    hosts.hosts.insert("web".to_owned(), HostConfig::Sftp(host));
    let paths = Paths::resolve(dir, 501, &|_| None);
    let context = Context::new(paths, Config::default(), dir.join("config.toml"), hosts);
    HostTask {
        context: Arc::new(context),
        host: "web".to_owned(),
        connection: 1,
        stop: CancellationToken::new(),
        done: mpsc::unbounded_channel().0,
    }
}

/// Where opening `web` lands, resuming `resume`; else where it failed, if the failure says.
async fn opened(
    task: &HostTask,
    fs: &SftpFs,
    resume: Option<&Path>,
) -> Result<String, Option<String>> {
    let request = ListRequest {
        generation: 1,
        location: Location::Remote {
            host: "web".to_owned(),
            path: RemotePath::from(""),
        },
        resume: resume.map(|path| RemotePath::from(path.to_str().unwrap())),
    };
    let panel = PanelId {
        side: crate::tui::app::Side::Left,
        tab: 1,
    };
    let Done::Listed { result, .. } = task.list(fs, panel, request).await else {
        panic!("expected a listing");
    };
    let path = |location: Location| match location {
        Location::Remote { path, .. } => String::from_utf8_lossy(path.as_bytes()).into_owned(),
        other @ Location::Local(_) => panic!("expected a remote location, got {other:?}"),
    };
    result
        .map(|listed| path(listed.location))
        .map_err(|error| error.location.map(path))
}

#[tokio::test]
async fn a_host_opens_where_it_left_off_else_at_its_start_dir_else_at_home() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::create_dir_all(root.join("www/app")).unwrap();
    let Some((_child, fs)) = local_sftp(&root).await else {
        return;
    };
    let text = |path: &Path| path.to_str().unwrap().to_owned();
    let with_start = task(&root, Some("www"));
    let app = root.join("www/app");
    assert_eq!(opened(&with_start, &fs, Some(&app)).await, Ok(text(&app)));
    let gone = root.join("gone");
    assert_eq!(
        opened(&with_start, &fs, Some(&gone)).await,
        Ok(text(&root.join("www"))),
        "a directory that is gone falls back to start_dir"
    );
    let plain = task(&root, None);
    assert_eq!(opened(&plain, &fs, Some(&gone)).await, Ok(text(&root)));
    // A start_dir that is not there fails at its path, for Up and Go to.
    let absolute = text(&root.join("gone/deeper"));
    for (start_dir, path) in [
        ("missing", root.join("missing")),
        ("missing/deeper", root.join("missing/deeper")),
        (absolute.as_str(), root.join("gone/deeper")),
    ] {
        let missing = task(&root, Some(start_dir));
        assert_eq!(
            opened(&missing, &fs, None).await,
            Err(Some(text(&path))),
            "{start_dir}"
        );
    }
}

#[tokio::test]
async fn names_to_complete_tell_directories_and_links_to_them() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("dir")).unwrap();
    std::fs::write(tmp.path().join("file"), "").unwrap();
    std::os::unix::fs::symlink("dir", tmp.path().join("link")).unwrap();
    let dir = Some(Location::Local(tmp.path().to_path_buf()));
    let mut names = names_in(dir, None).await.unwrap();
    names.sort();
    assert_eq!(
        names,
        [
            (b"dir".to_vec(), true),
            (b"file".to_vec(), false),
            (b"link".to_vec(), true)
        ]
    );
    let missing = Some(Location::Local(tmp.path().join("gone")));
    assert!(names_in(missing, None).await.is_err());
    let remote = Location::Remote {
        host: "web".to_owned(),
        path: RemotePath::from("/srv"),
    };
    assert_eq!(names_in(Some(remote), None).await, Ok(Vec::new()));
}

#[test]
fn commands_are_the_executable_files_of_path_each_once() {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, more) = (tmp.path().join("bin"), tmp.path().join("more"));
    for dir in [&bin, &more] {
        std::fs::create_dir(dir).unwrap();
    }
    let file = |path: PathBuf, mode: u32| {
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    file(bin.join("mytool"), 0o755);
    file(bin.join("mynotes"), 0o644);
    file(bin.join("other"), 0o755);
    file(more.join("mytool"), 0o700);
    file(more.join("MyCase"), 0o100);
    std::fs::create_dir(bin.join("mydir")).unwrap();
    std::os::unix::fs::symlink("mytool", bin.join("mylink")).unwrap();
    std::os::unix::fs::symlink("mynotes", bin.join("mynotelink")).unwrap();
    std::os::unix::fs::symlink("gone", bin.join("mygone")).unwrap();
    let names = |dirs: &[PathBuf], prefix: &str| -> Vec<String> {
        commands_in(dirs, prefix, false)
            .into_iter()
            .map(|(name, dir)| {
                assert!(!dir);
                String::from_utf8(name).unwrap()
            })
            .collect()
    };
    let path = [
        bin.clone(),
        PathBuf::from("relative"),
        tmp.path().join("missing"),
        more,
    ];
    assert_eq!(names(&path, "my"), ["MyCase", "mylink", "mytool"]);
    assert_eq!(names(&path, "MYT"), ["mytool"], "regardless of case");
    assert_eq!(
        names(std::slice::from_ref(&bin), ""),
        ["mylink", "mytool", "other"]
    );
    // A path to a program offers the directories too, as vim's `:!` does.
    let with_dirs: Vec<(String, bool)> = commands_in(&[bin], "my", true)
        .into_iter()
        .map(|(name, dir)| (String::from_utf8(name).unwrap(), dir))
        .collect();
    let expected = [("mydir", true), ("mylink", false), ("mytool", false)];
    assert_eq!(
        with_dirs,
        expected.map(|(name, dir)| (name.to_owned(), dir))
    );
}

/// Searches `dir` for `pattern` with the rg in `PATH`, taking `max_results`: what it
/// found, and how it ended; `None` without rg.
async fn rg(
    dir: &std::path::Path,
    pattern: &str,
    max_results: usize,
) -> Option<(Vec<Hit>, Result<FindSummary, String>)> {
    let ripgrep = Ripgrep::new(PathBuf::from("rg"));
    let query = Query {
        pattern: pattern.to_owned(),
        globs: Vec::new(),
        globs_case: noc_pattern::Case::Smart,
        regex: true,
        case: noc_pattern::Case::Smart,
        hidden: true,
        max_per_file: 1000,
    };
    let (done, mut reports) = mpsc::unbounded_channel();
    let mut batches = Batches {
        generation: 1,
        found: Vec::new(),
        sent: None,
        done,
    };
    let searched = Searched {
        query: &query,
        max_results,
    };
    let location = Location::Local(dir.to_path_buf());
    let result = searched
        .search(&ripgrep, location, None, &mut batches)
        .await;
    if result
        .as_ref()
        .is_err_and(|reason| reason.contains("not installed"))
    {
        return None;
    }
    batches.flush();
    Some((hits_of(&mut reports), result))
}

/// What ripgrep found, from the reports of a search.
fn hits_of(reports: &mut mpsc::UnboundedReceiver<Done>) -> Vec<Hit> {
    let mut hits = Vec::new();
    while let Ok(report) = reports.try_recv() {
        if let Done::Found { found, .. } = report {
            hits.extend(found.into_iter().map(|finding| match finding {
                Finding::Hit(hit) => hit,
                entry @ Finding::Entry { .. } => panic!("expected a hit, got {entry:?}"),
            }));
        }
    }
    hits
}

#[tokio::test]
async fn ripgrep_finds_lines_stops_at_the_limit_and_says_what_is_wrong() {
    let tmp = tempfile::tempdir().unwrap();
    let lines: String = (0..100)
        .map(|n| format!("needle {n}\n"))
        .collect::<Vec<_>>()
        .concat();
    std::fs::write(tmp.path().join("many"), lines).unwrap();
    std::fs::write(tmp.path().join("one"), "a needle\n").unwrap();
    let Some((hits, result)) = rg(tmp.path(), "needle", 1000).await else {
        return;
    };
    assert_eq!(result.unwrap(), FindSummary::default());
    let lines = hits
        .iter()
        .filter(|hit| matches!(hit, Hit::Line { .. }))
        .count();
    assert_eq!(lines, 101);
    assert!(hits.contains(&Hit::File(b"one".to_vec())));
    let (hits, result) = rg(tmp.path(), "needle", 10).await.unwrap();
    assert!(result.unwrap().limit);
    let lines = hits
        .iter()
        .filter(|hit| matches!(hit, Hit::Line { .. }))
        .count();
    assert_eq!(lines, 10);
    let (hits, result) = rg(tmp.path(), "needle (", 10).await.unwrap();
    assert_eq!(hits, []);
    let summary = result.unwrap();
    assert!(
        summary
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("unclosed")),
        "{summary:?}"
    );
}

#[tokio::test]
async fn on_a_host_ripgrep_runs_there_through_its_task() {
    let (handle, mut requests) = HostHandle::channel();
    let host = tokio::spawn(async move {
        let Some(HostRequest::Spawn {
            dir,
            program,
            args,
            reply,
        }) = requests.recv().await
        else {
            panic!("expected a program to start");
        };
        assert_eq!(dir.as_bytes(), b"/srv");
        assert_eq!(program, "rg");
        assert!(args.iter().any(|arg| arg == "--json"), "{args:?}");
        let json = r#"{"type":"begin","data":{"path":{"text":"./a"}}}"#;
        let child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "printf '%s\n' \"$0\"", json])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string());
        let _ = reply.send(child);
    });
    let query = Query {
        pattern: "x".to_owned(),
        globs: Vec::new(),
        globs_case: noc_pattern::Case::Smart,
        regex: true,
        case: noc_pattern::Case::Smart,
        hidden: false,
        max_per_file: 10,
    };
    let (done, mut reports) = mpsc::unbounded_channel();
    let mut batches = Batches {
        generation: 1,
        found: Vec::new(),
        sent: None,
        done,
    };
    let searched = Searched {
        query: &query,
        max_results: 10,
    };
    let location = Location::Remote {
        host: "web".to_owned(),
        path: RemotePath::from("/srv"),
    };
    let ripgrep = Ripgrep::new(PathBuf::from("/nonexistent/rg"));
    let result = searched
        .search(&ripgrep, location, Some(handle), &mut batches)
        .await;
    host.await.unwrap();
    assert_eq!(result.unwrap(), FindSummary::default());
    batches.flush();
    assert_eq!(hits_of(&mut reports), [Hit::File(b"a".to_vec())]);
}

/// Looks for `query` in `location` with the fd of `fd`, through `host` if remote, taking
/// `max_results`: what it found, and how it ended.
async fn fd_in(
    (fd, location, host): (&Fd, Location, Option<HostHandle>),
    query: &fd::Query,
    max_results: usize,
) -> (Vec<Finding>, Result<FindSummary, String>) {
    let (done, mut reports) = mpsc::unbounded_channel();
    let mut batches = Batches {
        generation: 1,
        found: Vec::new(),
        sent: None,
        done,
    };
    let named = Named { query, max_results };
    let result = named.search(fd, location, host, &mut batches).await;
    batches.flush();
    let mut found = Vec::new();
    while let Ok(report) = reports.try_recv() {
        if let Done::Found { found: more, .. } = report {
            found.extend(more);
        }
    }
    (found, result)
}

fn names(pattern: &str) -> fd::Query {
    fd::Query {
        pattern: pattern.to_owned(),
        globs: Vec::new(),
        regex: false,
        case: noc_pattern::Case::Smart,
        hidden: false,
    }
}

fn entry(path: &str, dir: bool, matches: &[std::ops::Range<usize>]) -> Finding {
    Finding::Entry {
        path: path.as_bytes().to_vec(),
        dir,
        matches: matches.to_vec(),
    }
}

/// The paths of what fd found, in their order.
fn sorted(mut found: Vec<Finding>) -> Vec<Finding> {
    found.sort_by_key(|finding| match finding {
        Finding::Entry { path, .. } => path.clone(),
        Finding::Hit(_) => Vec::new(),
    });
    found
}

#[tokio::test]
#[allow(clippy::single_range_in_vec_init)]
async fn fd_finds_names_keeps_what_the_globs_let_through_and_stops_at_the_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src/find")).unwrap();
    std::fs::write(root.join("src/find.rs"), "").unwrap();
    std::fs::write(root.join("src/main.rs"), "").unwrap();
    std::fs::write(root.join("Find.md"), "").unwrap();
    let fd = Fd::new(PathBuf::from("fd"));
    let local = || Location::Local(root.to_path_buf());
    let (found, result) = fd_in((&fd, local(), None), &names("find"), 100).await;
    if result
        .as_ref()
        .is_err_and(|reason| reason.contains("not installed"))
    {
        return;
    }
    assert_eq!(result.unwrap(), FindSummary::default());
    assert_eq!(
        sorted(found),
        [
            entry("Find.md", false, &[0..4]),
            entry("src/find", true, &[4..8]),
            entry("src/find.rs", false, &[4..8]),
        ]
    );
    let files = fd::Query {
        globs: vec!["*.rs".to_owned()],
        ..names("")
    };
    let (found, _) = fd_in((&fd, local(), None), &files, 100).await;
    assert_eq!(
        sorted(found),
        [
            entry("src/find.rs", false, &[]),
            entry("src/main.rs", false, &[])
        ]
    );
    let (found, result) = fd_in((&fd, local(), None), &names(""), 2).await;
    assert_eq!(found.len(), 2);
    assert!(result.unwrap().limit);
    let bad = fd::Query {
        regex: true,
        ..names("(")
    };
    let (found, result) = fd_in((&fd, local(), None), &bad, 10).await;
    assert_eq!(found, []);
    let result = result.unwrap();
    assert_eq!(result.last_error.as_deref(), Some("error: unclosed group"));
    let missing = Fd::new(PathBuf::from("/nonexistent/fd"));
    let (_, result) = fd_in((&missing, local(), None), &names("x"), 10).await;
    assert!(result.unwrap_err().contains("fd is not installed"));
}

#[tokio::test]
async fn on_a_host_without_fd_fdfind_runs_in_its_place() {
    let (handle, mut requests) = HostHandle::channel();
    let host = tokio::spawn(async move {
        let mut programs = Vec::new();
        while let Some(HostRequest::Spawn { program, reply, .. }) = requests.recv().await {
            programs.push(program);
            // The host has fdfind only: its shell says 127 for fd.
            let script = if program == "fd" {
                "echo 'sh: fd: not found' >&2; exit 127"
            } else {
                "printf 'src/\\0src/a.rs\\0'"
            };
            let child = tokio::process::Command::new("/bin/sh")
                .args(["-c", script])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|error| error.to_string());
            let _ = reply.send(child);
        }
        programs
    });
    let location = Location::Remote {
        host: "web".to_owned(),
        path: RemotePath::from("/srv"),
    };
    let fd = Fd::new(PathBuf::from("/nonexistent/fd"));
    let (found, result) = fd_in((&fd, location, Some(handle)), &names(""), 10).await;
    assert_eq!(result.unwrap(), FindSummary::default());
    assert_eq!(
        found,
        [entry("src", true, &[]), entry("src/a.rs", false, &[])]
    );
    assert_eq!(host.await.unwrap(), ["fd", "fdfind"]);
}

#[tokio::test]
async fn a_checksum_job_reports_its_sums_then_its_end() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("abc");
    std::fs::write(&file, "abc").unwrap();
    let (done, mut reports) = mpsc::unbounded_channel();
    let targets = vec![(vec![Location::Local(file.clone())], None)];
    let cancel = CancellationToken::new();
    let end = run_checksum(7, targets, Algorithm::Sha256, cancel, &done).await;
    assert!(matches!(
        end,
        Done::Job {
            id: 7,
            event: JobEvent::Finished { complete: true }
        }
    ));
    let mut sums = None;
    while let Ok(report) = reports.try_recv() {
        if let Done::Job {
            event: JobEvent::Sums(found),
            ..
        } = report
        {
            sums = Some(found);
        }
    }
    let sums = sums.unwrap();
    assert_eq!(sums.len(), 1);
    assert_eq!(sums[0].path, Location::Local(file));
    assert_eq!(sums[0].name, b"abc");
    assert_eq!(
        sums[0].digest.as_ref().unwrap()[..4],
        [0xba, 0x78, 0x16, 0xbf]
    );

    let cancel = CancellationToken::new();
    cancel.cancel();
    let targets = vec![(vec![Location::Local(tmp.path().join("abc"))], None)];
    run_checksum(8, targets, Algorithm::Sha256, cancel, &done).await;
    while let Ok(report) = reports.try_recv() {
        assert!(
            !matches!(
                report,
                Done::Job {
                    event: JobEvent::Sums(_),
                    ..
                }
            ),
            "an aborted job has no sums"
        );
    }
}

#[test]
fn reports_of_progress_wait_for_their_batch() {
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let counts = |bytes| Counts {
        bytes_done: bytes,
        ..Counts::default()
    };
    let mut waiting = Waiting::new();
    assert_eq!(waiting.due(), None);
    // The first goes at once.
    assert!(waiting.take(at(0), "a", counts(1)));
    let batch = vec![(at(0), counts(1))];
    assert_eq!(waiting.send(at(0)), (None, Some(("a", batch))));
    assert_eq!(waiting.due(), None, "none waits");
    // Those that come within BATCH wait for it.
    assert!(!waiting.take(at(5), "a", counts(2)));
    assert!(!waiting.take(at(15), "b", counts(3)));
    assert_eq!(waiting.due(), Some(at(20)));
    // One that comes after it goes with them, at its entry.
    assert!(waiting.take(at(25), "c", counts(4)));
    let batch = vec![(at(5), counts(2)), (at(15), counts(3)), (at(25), counts(4))];
    assert_eq!(waiting.send(at(25)), (None, Some(("c", batch))));
    // After a quiet spell, the next goes at once.
    assert!(waiting.take(at(100), "d", counts(5)));
}

#[test]
fn reports_of_counting_wait_for_their_batch_and_only_the_last_goes() {
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let mut waiting = Waiting::<&str>::new();
    // The first goes at once.
    assert!(waiting.count(at(0), 1));
    assert_eq!(waiting.send(at(0)), (Some((at(0), 1)), None));
    // Those that come within BATCH wait for it, the last in place of the others, with the
    // time of the first.
    assert!(!waiting.count(at(5), 2));
    assert!(!waiting.count(at(15), 3));
    assert!(!waiting.progress());
    assert_eq!(waiting.due(), Some(at(20)));
    assert_eq!(waiting.send(at(20)), (Some((at(5), 3)), None));
    assert_eq!(waiting.due(), None, "none waits");
    // Progress that comes after a count goes after it.
    assert!(!waiting.count(at(25), 4));
    let counts = Counts::default();
    assert!(waiting.take(at(40), "a", counts));
    let batch = vec![(at(40), counts)];
    assert_eq!(
        waiting.send(at(40)),
        (Some((at(25), 4)), Some(("a", batch)))
    );
}

/// A report of a job's progress at `path`, with `bytes` of a thousand done.
fn progress(path: &str, bytes: u64) -> Event<PathBuf> {
    Event::Progress(noc_ops::Progress {
        current: PathBuf::from(path),
        items_done: 0,
        items_total: 1,
        bytes_done: bytes,
        bytes_total: 1000,
        bytes_copied: bytes,
    })
}

#[tokio::test]
async fn a_job_s_progress_goes_in_batches_before_what_it_reports_next() {
    let (events, incoming) = mpsc::unbounded_channel();
    for bytes in 1..=1000 {
        events.send(progress(&format!("/f{bytes}"), bytes)).unwrap();
    }
    let (reply, _decision) = oneshot::channel();
    let path = PathBuf::from("/f1000");
    let error = VfsError::Io(std::io::ErrorKind::PermissionDenied.into());
    let failed = Event::Failed {
        path,
        step: Step::Read,
        error,
        reply,
    };
    events.send(failed).unwrap();
    events.send(progress("/g", 1000)).unwrap();
    drop(events);
    let (done, mut forwarded) = mpsc::unbounded_channel();
    forward(incoming, 7, &done, Location::Local).await;

    let mut batches = Vec::new();
    let mut asked = None;
    while let Ok(Done::Job { id: 7, event }) = forwarded.try_recv() {
        match event {
            JobEvent::Progress { current, reports } => batches.push((current, reports)),
            JobEvent::Failed { .. } => asked = Some(batches.len()),
            other => panic!("{other:?}"),
        }
    }
    // The first goes at once; those that came right after it wait, and go before the
    // question; the last goes as the job ends.
    assert_eq!(batches[0].1.len(), 1);
    assert!(batches.len() < 10, "{} batches", batches.len());
    let before: usize = batches[..asked.unwrap()]
        .iter()
        .map(|batch| batch.1.len())
        .sum();
    assert_eq!(before, 1000);
    assert_eq!(
        batches.last().unwrap().0,
        Location::Local(PathBuf::from("/g"))
    );
    let reports: Vec<(Instant, Counts)> =
        batches.iter().flat_map(|batch| batch.1.clone()).collect();
    let bytes: Vec<u64> = reports.iter().map(|report| report.1.bytes_done).collect();
    let expected: Vec<u64> = (1..=1000).chain([1000]).collect();
    assert_eq!(bytes, expected, "every report, in order");
    assert!(reports.windows(2).all(|pair| pair[0].0 <= pair[1].0));
    for (current, reports) in &batches[..batches.len() - 1] {
        let last = reports.last().unwrap().1.bytes_done;
        assert_eq!(
            *current,
            Location::Local(PathBuf::from(format!("/f{last}")))
        );
    }
}

#[tokio::test]
async fn a_job_s_counting_goes_in_batches_in_order_with_its_progress() {
    // A move tries to rename, then counts 1,000 files one by one, then copies them.
    let (events, incoming) = mpsc::unbounded_channel();
    events.send(progress("/a", 0)).unwrap();
    for items in 1..=1000 {
        events.send(Event::Scanning { items }).unwrap();
    }
    events.send(progress("/b", 1)).unwrap();
    drop(events);
    let (done, mut forwarded) = mpsc::unbounded_channel();
    forward(incoming, 7, &done, Location::Local).await;

    let mut seen = Vec::new();
    while let Ok(Done::Job { id: 7, event }) = forwarded.try_recv() {
        seen.push(match event {
            JobEvent::Scanning { items, .. } => Some(items),
            JobEvent::Progress { .. } => None,
            other => panic!("{other:?}"),
        });
    }
    // The rename first, then a few counts, the last of all of them, then the copy.
    assert_eq!(seen.first(), Some(&None), "{seen:?}");
    assert_eq!(seen.last(), Some(&None), "{seen:?}");
    let counts: Vec<u64> = seen[1..seen.len() - 1]
        .iter()
        .map(|count| count.expect("a count"))
        .collect();
    assert!(counts.len() < 10, "{counts:?}");
    assert!(
        counts.windows(2).all(|pair| pair[0] < pair[1]),
        "{counts:?}"
    );
    assert_eq!(counts.last(), Some(&1000));
}

#[tokio::test]
async fn the_last_reports_of_a_job_go_once_it_is_quiet() {
    let (events, incoming) = mpsc::unbounded_channel();
    let (done, mut forwarded) = mpsc::unbounded_channel();
    let forwarding =
        tokio::spawn(async move { forward(incoming, 7, &done, Location::Local).await });
    events.send(progress("/a", 1)).unwrap();
    events.send(progress("/a", 2)).unwrap();
    let mut next = async || {
        let taken = tokio::time::timeout(Duration::from_secs(5), forwarded.recv()).await;
        match taken {
            Ok(Some(Done::Job {
                event: JobEvent::Progress { reports, .. },
                ..
            })) => reports
                .iter()
                .map(|report| report.1.bytes_done)
                .collect::<Vec<_>>(),
            _ => panic!("no batch"),
        }
    };
    assert_eq!(next().await, [1]);
    // The job goes on, and reports nothing more for now.
    assert_eq!(next().await, [2]);
    drop(events);
    forwarding.await.unwrap();
}
