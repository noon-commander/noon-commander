//! Session tests against `tests/support/fake-ssh`, which serves SFTP with the local
//! `sftp-server`. No network, no real ssh.

#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

use noc_ssh::askpass::AskpassServer;
use noc_ssh::resolve::resolve;
use noc_ssh::version::{OpenSshVersion, check_version};
use noc_ssh::{Session, SftpChannel, SshError, SshSettings, Target, cleanup_stale};
use openssh_sftp_client::{Sftp, SftpOptions};
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use tokio_util::sync::CancellationToken;

const FAKE_SSH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/fake-ssh");

fn sftp_server() -> Option<PathBuf> {
    let found = std::env::var_os("SFTP_SERVER")
        .map(PathBuf::from)
        .or_else(|| {
            [
                "/usr/libexec/sftp-server",
                "/usr/lib/openssh/sftp-server",
                "/usr/libexec/openssh/sftp-server",
                "/usr/lib/ssh/sftp-server",
            ]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
        });
    if found.is_none() {
        eprintln!("note: sftp-server not found, skipping; set SFTP_SERVER to run this test");
    }
    found
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// A wrapper around the fake ssh with its environment baked in, plus scratch directories.
struct Fake {
    dir: tempfile::TempDir,
    root: tempfile::TempDir,
    /// Short, so control socket paths stay far below the `sun_path` limit.
    runtime: tempfile::TempDir,
    settings: SshSettings,
}

impl Fake {
    fn new(vars: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let runtime = tempfile::Builder::new()
            .prefix("st")
            .tempdir_in("/tmp")
            .unwrap();
        let server = sftp_server().unwrap_or_else(|| PathBuf::from("/nonexistent/sftp-server"));
        let mut script = String::from("#!/bin/sh\n");
        let defaults = [
            ("FAKE_SSH_LOG", dir.path().join("log")),
            ("FAKE_SSH_PIDS", dir.path().join("pids")),
            ("FAKE_SSH_ROOT", root.path().to_path_buf()),
            ("FAKE_SSH_SFTP_SERVER", server),
        ];
        let defaults = defaults
            .iter()
            .map(|(name, path)| (*name, path.to_str().unwrap()));
        for (name, value) in defaults.chain(vars.iter().copied()) {
            writeln!(script, "{name}={}; export {name}", quote(value)).unwrap();
        }
        writeln!(script, "exec {} \"$@\"", quote(FAKE_SSH)).unwrap();
        let program = dir.path().join("ssh");
        std::fs::write(&program, script).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let settings = SshSettings {
            program,
            ..SshSettings::default()
        };
        Self {
            dir,
            root,
            runtime,
            settings,
        }
    }

    /// Every command line the fake ssh was started with, in order.
    fn invocations(&self) -> Vec<Vec<String>> {
        let log = std::fs::read_to_string(self.dir.path().join("log")).unwrap_or_default();
        let mut invocations = Vec::new();
        let mut current = Vec::new();
        for line in log.lines() {
            if line == "--end--" {
                invocations.push(std::mem::take(&mut current));
            } else {
                current.push(line.to_owned());
            }
        }
        invocations
    }

    fn pids(&self) -> Vec<Pid> {
        std::fs::read_to_string(self.dir.path().join("pids"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.parse().ok().and_then(Pid::from_raw))
            .collect()
    }

    fn runtime_entries(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.runtime.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }

    async fn connect(&self, settings: &SshSettings) -> Result<Session, SshError> {
        let cancel = CancellationToken::new();
        Session::connect(
            settings,
            &Target::new("web"),
            self.runtime.path(),
            None,
            &cancel,
        )
        .await
    }
}

fn contains_pair(args: &[String], first: &str, second: &str) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == first && pair[1] == second)
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let index = args.iter().position(|arg| arg == flag)?;
    args.get(index + 1).map(String::as_str)
}

async fn read_file(channel: SftpChannel, path: &str) -> (Vec<u8>, ExitStatus) {
    let SftpChannel {
        stdin,
        stdout,
        process,
    } = channel;
    let sftp = Sftp::new(stdin, stdout, SftpOptions::default())
        .await
        .unwrap();
    let mut fs = sftp.fs();
    let data = fs.read(path).await.unwrap().to_vec();
    drop(fs);
    sftp.close().await.unwrap();
    let status = process.finish().await.unwrap();
    (data, status)
}

fn is_gone(pid: Pid) -> bool {
    matches!(test_kill_process(pid), Err(rustix::io::Errno::SRCH))
}

#[tokio::test]
async fn checks_the_openssh_version() {
    let fake = Fake::new(&[]);
    assert_eq!(
        check_version(&fake.settings).await.unwrap(),
        OpenSshVersion { major: 9, minor: 9 }
    );
    let old = Fake::new(&[("FAKE_SSH_BANNER", "OpenSSH_8.2p1 Ubuntu-4ubuntu0.5")]);
    assert!(matches!(
        check_version(&old.settings).await,
        Err(SshError::TooOld { .. })
    ));
    let other = Fake::new(&[("FAKE_SSH_BANNER", "Dropbear v2022.83")]);
    assert!(matches!(
        check_version(&other.settings).await,
        Err(SshError::NotOpenSsh { .. })
    ));
    let missing = SshSettings {
        program: PathBuf::from("/nonexistent/ssh"),
        ..SshSettings::default()
    };
    assert!(matches!(
        check_version(&missing).await,
        Err(SshError::Spawn { .. })
    ));
}

#[tokio::test]
async fn resolves_effective_settings() {
    let fake = Fake::new(&[
        ("FAKE_SSH_USER", "deploy"),
        ("FAKE_SSH_HOSTNAME", "10.0.0.5"),
        ("FAKE_SSH_PORT", "2222"),
    ]);
    let target = Target::new("web").with_args(vec!["-v".to_owned()]);
    let host = resolve(&fake.settings, &target).await.unwrap();
    assert_eq!(host.address(), "deploy@10.0.0.5:2222");
    assert_eq!(fake.invocations(), [["-v", "-G", "--", "web"]]);
}

#[tokio::test]
async fn multiplexes_sftp_channels_over_one_master() {
    if sftp_server().is_none() {
        return;
    }
    let fake = Fake::new(&[]);
    std::fs::write(fake.root.path().join("hello.txt"), b"hello").unwrap();
    let session = fake.connect(&fake.settings).await.unwrap();
    assert!(session.is_multiplexed());
    assert!(!session.is_closed());

    let first = session.open_sftp().unwrap();
    let second = session.open_sftp().unwrap();
    let (a, b) = tokio::join!(
        read_file(first, "hello.txt"),
        read_file(second, "hello.txt")
    );
    for (data, status) in [a, b] {
        assert_eq!(data, b"hello");
        assert!(status.success());
    }

    let invocations = fake.invocations();
    assert_eq!(invocations.len(), 3);
    let master = &invocations[0];
    assert!(master.iter().any(|arg| arg == "-M"));
    assert!(contains_pair(master, "-o", "ControlPersist=no"));
    assert!(master.ends_with(&["--".to_owned(), "web".to_owned()]));
    let control_path = value_after(master, "-S").unwrap();
    assert!(control_path.starts_with(fake.runtime.path().to_str().unwrap()));
    for channel in &invocations[1..] {
        assert_eq!(value_after(channel, "-S"), Some(control_path));
        assert!(contains_pair(channel, "-o", "BatchMode=yes"));
        assert!(contains_pair(channel, "-o", "ControlMaster=no"));
        assert!(channel.ends_with(&["--", "web", "sftp"].map(String::from)));
    }

    session.close().await;
    assert!(
        fake.runtime_entries().is_empty(),
        "the control socket is left"
    );
    assert_eq!(fake.invocations().last().unwrap()[..2], ["-F", "/dev/null"]);
    assert!(fake.pids().into_iter().all(is_gone));
}

#[tokio::test]
async fn opens_direct_channels_without_multiplexing() {
    if sftp_server().is_none() {
        return;
    }
    let fake = Fake::new(&[]);
    std::fs::write(fake.root.path().join("hello.txt"), b"direct").unwrap();
    let settings = SshSettings {
        multiplex: false,
        ..fake.settings.clone()
    };
    let session = fake.connect(&settings).await.unwrap();
    assert!(!session.is_multiplexed());
    assert!(
        fake.invocations().is_empty(),
        "no master without multiplexing"
    );

    let (data, status) = read_file(session.open_sftp().unwrap(), "hello.txt").await;
    assert_eq!(data, b"direct");
    assert!(status.success());
    let channel = &fake.invocations()[0];
    assert!(!channel.iter().any(|arg| arg == "-S" || arg == "-M"));
    if !noc_ssh::policy::FORWARDING_ENABLED {
        assert!(contains_pair(channel, "-o", "Tunnel=no"));
    }
    session.close().await;
}

#[tokio::test]
async fn reports_authentication_failures() {
    let fake = Fake::new(&[(
        "FAKE_SSH_FAIL",
        "deploy@web: Permission denied (publickey).",
    )]);
    match fake.connect(&fake.settings).await {
        Err(SshError::Exited { status, stderr }) => {
            assert_eq!(status.code(), Some(255));
            assert!(stderr.contains("Permission denied (publickey)"), "{stderr}");
        }
        other => panic!("expected an exit error, got {other:?}"),
    }
    assert_eq!(fake.runtime_entries(), [] as [PathBuf; 0]);
}

#[tokio::test]
async fn cancels_a_pending_connection() {
    let fake = Fake::new(&[("FAKE_SSH_HANG", "1")]);
    let cancel = CancellationToken::new();
    let target = Target::new("web");
    let connect = Session::connect(&fake.settings, &target, fake.runtime.path(), None, &cancel);
    let canceller = async {
        // Cancel only once the master is running, however slow process startup is.
        while fake.pids().is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        cancel.cancel();
    };
    let (result, ()) = tokio::join!(connect, canceller);
    assert!(matches!(result, Err(SshError::Cancelled)), "{result:?}");
    let pids = fake.pids();
    assert_eq!(pids.len(), 1);
    assert!(is_gone(pids[0]), "the master is still running");
}

#[tokio::test]
async fn notices_when_the_master_goes_away() {
    if sftp_server().is_none() {
        return;
    }
    let fake = Fake::new(&[]);
    let session = fake.connect(&fake.settings).await.unwrap();
    let control_path = &fake.runtime_entries()[0];
    let pid: i32 = std::fs::read_to_string(control_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    kill_process(Pid::from_raw(pid).unwrap(), Signal::TERM).unwrap();

    tokio::time::timeout(Duration::from_secs(5), session.closed())
        .await
        .expect("the session did not notice that the master exited");
    assert!(session.is_closed());
    assert!(session.exit_status().is_some_and(|status| status.success()));
    assert!(matches!(
        session.open_sftp(),
        Err(SshError::Disconnected { .. })
    ));
    session.close().await;
}

#[tokio::test]
async fn notices_when_a_channel_ends() {
    if sftp_server().is_none() {
        return;
    }
    let fake = Fake::new(&[]);
    let settings = SshSettings {
        multiplex: false,
        ..fake.settings.clone()
    };
    let session = fake.connect(&settings).await.unwrap();
    let SftpChannel {
        stdin,
        stdout,
        mut process,
    } = session.open_sftp().unwrap();
    let sftp = Sftp::new(stdin, stdout, SftpOptions::default())
        .await
        .unwrap();
    let pid = Pid::from_raw(i32::try_from(process.id().unwrap()).unwrap()).unwrap();
    let wait = tokio::time::timeout(Duration::from_millis(200), process.wait()).await;
    assert!(wait.is_err(), "the channel is still open");

    // Without multiplexing the channel is the connection, so this is a lost connection.
    kill_process(pid, Signal::TERM).unwrap();
    tokio::time::timeout(Duration::from_secs(5), process.wait())
        .await
        .expect("the channel's end went unnoticed");
    drop(sftp);
    session.close().await;
}

#[tokio::test]
async fn passes_prompts_to_the_askpass_program() {
    if sftp_server().is_none() {
        return;
    }
    let fake = Fake::new(&[("FAKE_SSH_PASSWORD", "hunter2")]);
    // Without askpass, ssh cannot prompt and authentication fails.
    assert!(matches!(
        fake.connect(&fake.settings).await,
        Err(SshError::Exited { .. })
    ));

    let askpass = fake.dir.path().join("askpass");
    std::fs::write(&askpass, "#!/bin/sh\nprintf '%s\\n' hunter2\n").unwrap();
    std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (server, _events) = AskpassServer::bind(fake.runtime.path(), askpass).unwrap();
    let session = Session::connect(
        &fake.settings,
        &Target::new("web"),
        fake.runtime.path(),
        Some(server.env("web")),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!session.is_closed());
    session.close().await;
}

#[tokio::test]
async fn cleans_up_after_dead_instances_only() {
    let fake = Fake::new(&[]);
    let stale = fake.runtime.path().join("cm-999999999-0badf00d");
    let live = fake
        .runtime
        .path()
        .join(format!("cm-{}-0badf00d", std::process::id()));
    for path in [&stale, &live] {
        std::fs::write(path, b"").unwrap();
    }
    cleanup_stale(fake.runtime.path(), &fake.settings).await;
    assert!(!stale.exists());
    assert!(live.exists());
    let invocations = fake.invocations();
    assert_eq!(invocations.len(), 1);
    assert!(contains_pair(&invocations[0], "-O", "exit"));
    assert_eq!(value_after(&invocations[0], "-S"), stale.to_str());
}

#[test]
fn fake_ssh_is_executable() {
    let mode = std::fs::metadata(Path::new(FAKE_SSH))
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0, "chmod +x {FAKE_SSH}");
}
