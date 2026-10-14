//! Records the commit the binary is built from, for `noc --version`. Sources without git, or
//! inside another project's repository, give `unknown`.

use std::path::{Path, PathBuf};
use std::process::Command;

const UNKNOWN: &str = "unknown";

/// What goes into the binary, relative to the workspace root: changes elsewhere, such as in the
/// docs, do not make a build dirty.
const SOURCES: &[&str] = &["crates", "Cargo.toml", "Cargo.lock"];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let commit = Commit::read(&manifest);
    let commit = commit.as_ref();
    env(
        "NOC_BUILD_COMMIT",
        commit.map_or(UNKNOWN, |commit| &commit.hash),
    );
    env(
        "NOC_BUILD_COMMIT_TIME",
        commit.map_or(UNKNOWN, |commit| &commit.time),
    );
    env(
        "NOC_BUILD_DIRTY",
        match commit.map(|commit| commit.dirty) {
            Some(true) => "yes",
            Some(false) => "no",
            None => UNKNOWN,
        },
    );
    env("NOC_BUILD_TARGET", &cargo_var("TARGET"));
    env("NOC_BUILD_PROFILE", &cargo_var("PROFILE"));
}

struct Commit {
    hash: String,
    /// The commit time in UTC, as RFC 3339.
    time: String,
    /// Whether the sources differ from the commit.
    dirty: bool,
}

impl Commit {
    fn read(manifest: &Path) -> Option<Self> {
        // A source archive unpacked inside another repository must not report that one.
        git(
            manifest,
            &["ls-files", "--error-unmatch", "--", "Cargo.toml"],
        )?;
        let paths = git(
            manifest,
            &[
                "rev-parse",
                "--git-dir",
                "--git-common-dir",
                "--show-toplevel",
            ],
        )?;
        let mut paths = paths.lines().map(|path| manifest.join(path));
        let (git_dir, common_dir, root) = (paths.next()?, paths.next()?, paths.next()?);

        let mut watched = vec![git_dir.join("HEAD"), common_dir.join("packed-refs")];
        if let Some(head) = git(manifest, &["symbolic-ref", "--quiet", "HEAD"]) {
            watched.push(common_dir.join(head));
        }
        watched.extend(SOURCES.iter().map(|source| root.join(source)));
        for path in watched.iter().filter(|path| path.exists()) {
            println!("cargo::rerun-if-changed={}", path.display());
        }

        let hash = git(manifest, &["rev-parse", "--short=12", "HEAD"])?;
        let time = git(
            manifest,
            &[
                "log",
                "-1",
                "--format=%cd",
                "--date=format-local:%Y-%m-%dT%H:%M:%SZ",
            ],
        )?;
        let mut status = vec!["status", "--porcelain", "--untracked-files=no", "--"];
        let pathspecs: Vec<String> = SOURCES.iter().map(|source| format!(":/{source}")).collect();
        status.extend(pathspecs.iter().map(String::as_str));
        let dirty = !git(manifest, &status)?.is_empty();
        Some(Self { hash, time, dirty })
    }
}

/// Runs git in `dir` and returns its trimmed output, or `None` if it fails or is missing.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(dir)
        // `format-local` dates are in the zone of `TZ`.
        .env("TZ", "UTC0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn cargo_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| UNKNOWN.to_owned())
}

fn env(name: &str, value: &str) {
    println!("cargo::rustc-env={name}={value}");
}
