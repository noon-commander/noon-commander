//! Opener tests against `tests/support/fake-open`, which logs its command lines. They never
//! open anything on the desktop.

#![expect(
    clippy::unwrap_used,
    reason = "the helpers, which `allow-unwrap-in-tests` leaves out"
)]

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use noc_tools::ToolError;
use noc_tools::opener::Opener;

const FAKE_OPEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/fake-open");

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Writes `script` to `path` as a program, through a child: this process never has the file
/// open for writing, which a test in another thread could pass on to its child until that
/// child's exec, and Linux runs no file open for writing (ETXTBSY).
fn write_program(path: &Path, script: &str) {
    let mut writer = Command::new("/bin/sh")
        .args(["-c", r#"cat > "$1" && chmod 755 "$1""#, "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = writer.stdin.take().unwrap();
    stdin.write_all(script.as_bytes()).unwrap();
    drop(stdin);
    assert!(writer.wait().unwrap().success());
}

/// A wrapper around the fake opener with its environment baked in.
struct Fake {
    dir: tempfile::TempDir,
    opener: Opener,
}

impl Fake {
    fn new(vars: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let mut script = String::from("#!/bin/sh\n");
        let defaults = [("FAKE_OPEN_LOG", log.to_str().unwrap())];
        for (name, value) in defaults.iter().chain(vars) {
            writeln!(script, "{name}={}; export {name}", quote(value)).unwrap();
        }
        writeln!(script, "exec {} \"$@\"", quote(FAKE_OPEN)).unwrap();
        let program = dir.path().join("open");
        write_program(&program, &script);
        Self {
            opener: Opener::new(program),
            dir,
        }
    }

    /// Every command line the fake was started with, in order.
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
}

#[tokio::test]
async fn opens_the_absolute_path_alone() {
    let fake = Fake::new(&[]);
    let dir = tempfile::tempdir().unwrap();
    let odd = dir.path().join("-rf report.pdf");
    std::fs::write(&odd, b"").unwrap();
    fake.opener.open(&odd).await.unwrap();
    fake.opener.open(dir.path()).await.unwrap();
    assert_eq!(
        fake.invocations(),
        [
            vec![odd.to_str().unwrap().to_owned()],
            vec![dir.path().to_str().unwrap().to_owned()],
        ]
    );
}

#[tokio::test]
async fn reports_the_exit_code_of_a_failure() {
    let fake = Fake::new(&[("FAKE_OPEN_FAIL", "3")]);
    let dir = tempfile::tempdir().unwrap();
    let error = fake.opener.open(dir.path()).await.unwrap_err();
    match error {
        ToolError::Failed { status, .. } => assert_eq!(status.code(), Some(3)),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn a_missing_opener_is_not_found() {
    let opener = Opener::new("/nonexistent/xdg-open");
    let dir = tempfile::tempdir().unwrap();
    assert!(opener.open(dir.path()).await.unwrap_err().is_not_found());
}

#[test]
fn the_desktop_has_its_own() {
    let expected = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    assert_eq!(Opener::default().program().to_str(), Some(expected));
}
