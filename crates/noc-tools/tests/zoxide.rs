//! zoxide tests against `tests/support/fake-zoxide`, which logs its command lines. They never
//! touch the user's zoxide database.

#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use noc_tools::ToolError;
use noc_tools::zoxide::{Scored, Zoxide};

const FAKE_ZOXIDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/fake-zoxide");

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// A wrapper around the fake zoxide with its environment baked in.
struct Fake {
    dir: tempfile::TempDir,
    zoxide: Zoxide,
}

impl Fake {
    fn new(vars: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let mut script = String::from("#!/bin/sh\n");
        let defaults = [("FAKE_ZOXIDE_LOG", log.to_str().unwrap())];
        for (name, value) in defaults.iter().chain(vars) {
            writeln!(script, "{name}={}; export {name}", quote(value)).unwrap();
        }
        writeln!(script, "exec {} \"$@\"", quote(FAKE_ZOXIDE)).unwrap();
        let program = dir.path().join("zoxide");
        std::fs::write(&program, script).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            zoxide: Zoxide::new(program),
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

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

#[tokio::test]
async fn add_passes_the_directory_after_a_separator() {
    let fake = Fake::new(&[]);
    let dir = tempfile::tempdir().unwrap();
    let odd = dir.path().join("-rf dir");
    std::fs::create_dir(&odd).unwrap();
    fake.zoxide.add(&odd).await.unwrap();
    assert_eq!(
        fake.invocations(),
        [strings(&["add", "--", odd.to_str().unwrap()])]
    );
}

#[tokio::test]
async fn add_reports_what_zoxide_says() {
    let fake = Fake::new(&[]);
    let error = fake
        .zoxide
        .add(Path::new("/nonexistent/dir"))
        .await
        .unwrap_err();
    match error {
        ToolError::Failed { stderr, .. } => {
            assert_eq!(stderr, "zoxide: not a directory: /nonexistent/dir");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn query_lists_scored_directories_for_the_keywords() {
    let output = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(output.path(), "  56.0 /home/me/src/noc\n   4.0 /tmp/a b\n").unwrap();
    let fake = Fake::new(&[("FAKE_ZOXIDE_OUTPUT", output.path().to_str().unwrap())]);
    let keywords = strings(&["src", "-n"]);
    let found = fake
        .zoxide
        .query(&keywords, Some(Path::new("/home/me")))
        .await
        .unwrap();
    assert_eq!(
        found,
        [
            Scored {
                score: 56.0,
                path: PathBuf::from("/home/me/src/noc"),
            },
            Scored {
                score: 4.0,
                path: PathBuf::from("/tmp/a b"),
            },
        ]
    );
    let all = fake.zoxide.query(&[], None).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(
        fake.invocations(),
        [
            strings(&[
                "query",
                "--list",
                "--score",
                "--exclude",
                "/home/me",
                "--",
                "src",
                "-n"
            ]),
            strings(&["query", "--list", "--score", "--"]),
        ]
    );
}

#[tokio::test]
async fn query_fails_with_zoxide_and_without_it() {
    let fake = Fake::new(&[("FAKE_ZOXIDE_FAIL", "unable to create data directory")]);
    let error = fake.zoxide.query(&[], None).await.unwrap_err();
    assert!(!error.is_not_found());
    assert!(
        error
            .to_string()
            .ends_with("zoxide: unable to create data directory"),
        "{error}"
    );
    let missing = Zoxide::new("/nonexistent/zoxide");
    assert!(missing.query(&[], None).await.unwrap_err().is_not_found());
}

#[tokio::test]
async fn dropping_a_query_stops_it() {
    let fake = Fake::new(&[("FAKE_ZOXIDE_HANG", "1")]);
    let query = fake.zoxide.query(&[], None);
    let result = tokio::time::timeout(std::time::Duration::from_millis(200), query).await;
    assert!(result.is_err(), "still waiting");
}
