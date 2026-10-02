//! End-to-end tests of the `noc` binary. ssh is the fake from
//! `crates/noc-ssh/tests/support/fake-ssh`, which serves SFTP with the local `sftp-server`.
//! Every test gets its own home and XDG directories.

#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use noc_ssh::askpass::{AskpassEvent, AskpassServer};
use secrecy::SecretString;

const BINARY: &str = env!("CARGO_BIN_EXE_noc");
const FAKE_SSH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../noc-ssh/tests/support/fake-ssh"
);

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

fn write_executable(path: &Path, script: &str) {
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A home directory, XDG directories, a remote root, and a fake ssh.
struct Sandbox {
    dir: tempfile::TempDir,
    /// Short, so socket paths stay far below the `sun_path` limit.
    runtime: tempfile::TempDir,
}

impl Sandbox {
    fn new(fake_ssh_vars: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let runtime = tempfile::Builder::new()
            .prefix("st")
            .tempdir_in("/tmp")
            .unwrap();
        let sandbox = Self { dir, runtime };
        for sub in ["home/.ssh", "config/noc", "root"] {
            std::fs::create_dir_all(sandbox.path(sub)).unwrap();
        }
        let server = sftp_server().unwrap_or_else(|| PathBuf::from("/nonexistent/sftp-server"));
        let mut script = String::from("#!/bin/sh\n");
        let root = sandbox.path("root");
        let defaults = [
            ("FAKE_SSH_ROOT", root.to_str().unwrap()),
            ("FAKE_SSH_SFTP_SERVER", server.to_str().unwrap()),
        ];
        for (name, value) in defaults.iter().chain(fake_ssh_vars) {
            writeln!(script, "{name}={}; export {name}", quote(value)).unwrap();
        }
        writeln!(script, "exec {} \"$@\"", quote(FAKE_SSH)).unwrap();
        write_executable(&sandbox.path("ssh"), &script);
        sandbox.write_config("");
        sandbox
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// Writes `config.toml`: the fake ssh, an `ssh_config` in the sandbox, and `extra`.
    fn write_config(&self, extra: &str) {
        let config = format!(
            "[ssh]\nprogram = {}\nconfig_file = {}\n{extra}",
            quote(self.path("ssh").to_str().unwrap()),
            quote(self.path("ssh_config").to_str().unwrap()),
        );
        std::fs::write(self.path("config/noc/config.toml"), config).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BINARY);
        command
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .env_remove("NOC_ASKPASS_SOCKET")
            .env_remove("NOC_ASKPASS_TOKEN")
            .env_remove("NOC_LOG")
            .stdin(Stdio::null());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    fn runtime_entries(&self) -> Vec<String> {
        let dir = self.runtime.path().join("noc");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn version_shows_the_forwarding_feature() {
    let output = Sandbox::new(&[]).run(&["--version"]);
    assert!(output.status.success());
    let marker = if noc_ssh::policy::FORWARDING_ENABLED {
        "(+forwarding)"
    } else {
        "(-forwarding)"
    };
    assert!(stdout(&output).contains(marker), "{}", stdout(&output));
}

#[test]
fn the_tui_needs_a_terminal() {
    let sandbox = Sandbox::new(&[]);
    let output = sandbox.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("the TUI needs a terminal"),
        "{}",
        stderr(&output)
    );
    assert!(
        output.stdout.is_empty(),
        "no escape codes without a terminal"
    );
}

#[test]
fn rejects_an_invalid_language() {
    let sandbox = Sandbox::new(&[]);
    sandbox.write_config("[ui]\nlanguage = \"not a tag\"\n");
    let output = sandbox.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("invalid `ui.language`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn rejects_an_unknown_theme() {
    let sandbox = Sandbox::new(&[]);
    sandbox.write_config("[ui]\ntheme = \"solarized\"\n");
    let output = sandbox.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("invalid `ui.theme`")
            && stderr(&output).contains("mc-classic, terminal"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn config_init_writes_the_defaults_once() {
    let sandbox = Sandbox::new(&[]);
    let file = sandbox.path("config/noc/config.toml");
    std::fs::remove_file(&file).unwrap();

    let output = sandbox.run(&["config", "init"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        noc_config::DEFAULT_CONFIG
    );

    let output = sandbox.run(&["config", "init"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("already exists"),
        "{}",
        stderr(&output)
    );
    assert!(sandbox.run(&["config", "init", "--force"]).status.success());
}

#[test]
fn config_init_writes_the_hosts_template_once() {
    let sandbox = Sandbox::new(&[]);
    std::fs::remove_file(sandbox.path("config/noc/config.toml")).unwrap();
    let hosts = sandbox.path("config/noc/hosts.toml");
    std::fs::write(&hosts, "# mine\n").unwrap();
    let output = sandbox.run(&["config", "init"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(std::fs::read_to_string(&hosts).unwrap(), "# mine\n");

    std::fs::remove_file(&hosts).unwrap();
    assert!(sandbox.run(&["config", "init", "--force"]).status.success());
    assert_eq!(
        std::fs::read_to_string(&hosts).unwrap(),
        noc_config::DEFAULT_HOSTS
    );
}

#[test]
fn hosts_in_config_toml_point_to_hosts_toml() {
    let sandbox = Sandbox::new(&[]);
    sandbox.write_config("[hosts.nas]\nlabel = \"Storage\"\n");
    let output = sandbox.run(&["hosts"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("hosts.toml"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn config_paths_follow_xdg_variables() {
    let sandbox = Sandbox::new(&[]);
    let output = sandbox.run(&["config", "paths"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains(sandbox.path("config/noc/config.toml").to_str().unwrap()));
    assert!(text.contains(sandbox.path("config/noc/hosts.toml").to_str().unwrap()));
    assert!(text.contains(sandbox.runtime.path().join("noc").to_str().unwrap()));
}

#[test]
fn rejects_an_invalid_config() {
    let sandbox = Sandbox::new(&[]);
    sandbox.write_config("unknown = 1\n");
    let output = sandbox.run(&["hosts"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("invalid config"),
        "{}",
        stderr(&output)
    );

    sandbox.write_config("args = [\"-L\", \"8080:localhost:80\"]\n");
    let output = sandbox.run(&["hosts"]);
    if noc_ssh::policy::FORWARDING_ENABLED {
        assert!(output.status.success(), "{}", stderr(&output));
    } else {
        assert_eq!(output.status.code(), Some(1));
        assert!(
            stderr(&output).contains("forwarding"),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn hosts_lists_ssh_config_hosts() {
    let sandbox = Sandbox::new(&[("FAKE_SSH_PORT", "2222")]);
    std::fs::create_dir_all(sandbox.path("conf.d")).unwrap();
    std::fs::write(
        sandbox.path("ssh_config"),
        format!(
            "Host prod-web prod-web.example.com\n  User deploy\n\nHost github.com\n\n\
             Include {}/conf.d/*\n\nHost *\n  ServerAliveInterval 30\n",
            sandbox.dir.path().display()
        ),
    )
    .unwrap();
    std::fs::write(sandbox.path("conf.d/work"), "Host nas\n").unwrap();
    std::fs::write(
        sandbox.path("config/noc/hosts.toml"),
        "[nas]\ntype = \"sftp\"\nlabel = \"Storage\"\n",
    )
    .unwrap();

    let output = sandbox.run(&["hosts"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let lines: Vec<String> = stdout(&output)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(
        lines,
        ["prod-web (also prod-web.example.com)", "nas Storage"]
    );

    let output = sandbox.run(&["hosts", "--resolve"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("tester@prod-web.example:2222"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn hosts_shows_cached_addresses_until_ssh_config_changes() {
    let sandbox = Sandbox::new(&[("FAKE_SSH_PORT", "2222")]);
    // The cache also watches the names next to ssh_config, so it gets a directory of its own.
    let ssh_config = sandbox.path("home/.ssh/config");
    std::fs::write(&ssh_config, "Host web\n").unwrap();
    std::fs::write(
        sandbox.path("config/noc/config.toml"),
        format!(
            "[ssh]\nprogram = {}\nconfig_file = {}\n",
            quote(sandbox.path("ssh").to_str().unwrap()),
            quote(ssh_config.to_str().unwrap()),
        ),
    )
    .unwrap();
    let resolved = sandbox.run(&["hosts", "--resolve"]);
    assert!(resolved.status.success(), "{}", stderr(&resolved));
    assert_eq!(stdout(&resolved), "web  tester@web.example:2222\n");
    assert!(sandbox.path("cache/noc/resolve.json").is_file());

    // Without the fake ssh, the address can only come from the cache.
    std::fs::remove_file(sandbox.path("ssh")).unwrap();
    let cached = sandbox.run(&["hosts"]);
    assert!(cached.status.success(), "{}", stderr(&cached));
    assert_eq!(stdout(&cached), stdout(&resolved));

    std::fs::write(&ssh_config, "Host web\n  Port 2200\n").unwrap();
    let changed = sandbox.run(&["hosts"]);
    assert!(changed.status.success(), "{}", stderr(&changed));
    assert_eq!(stdout(&changed), "web\n");
}

#[test]
fn ls_lists_the_virtual_root_and_local_directories() {
    let sandbox = Sandbox::new(&[]);
    std::fs::write(sandbox.path("ssh_config"), "Host web\nHost db\n").unwrap();
    let output = sandbox.run(&["ls"]);
    assert!(output.status.success(), "{}", stderr(&output));
    // The mount points of the volumes, which depend on the machine, the system volume first,
    // then the hosts.
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.first(), Some(&"/"), "{text}");
    assert!(lines.ends_with(&["web:", "db:"]), "{text}");
    assert!(
        lines[..lines.len() - 2]
            .iter()
            .all(|line| line.starts_with('/')),
        "{text}"
    );

    std::fs::write(sandbox.path("root/notes.txt"), b"x").unwrap();
    std::fs::create_dir(sandbox.path("root/docs")).unwrap();
    let output = sandbox.run(&["ls", sandbox.path("root").to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let names: Vec<&str> = text
        .lines()
        .map(|line| line.rsplit(' ').next().unwrap())
        .collect();
    assert_eq!(names, ["docs/", "notes.txt"]);
}

#[test]
fn ls_lists_a_remote_directory_over_sftp() {
    if sftp_server().is_none() {
        return;
    }
    let sandbox = Sandbox::new(&[]);
    std::fs::write(sandbox.path("root/hello.txt"), b"hello").unwrap();
    std::fs::create_dir(sandbox.path("root/www")).unwrap();
    std::fs::write(sandbox.path("root/www/index.html"), b"<html>").unwrap();

    let output = sandbox.run(&["ls", "web:"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("www/"), "{text}");
    assert!(text.contains("hello.txt"), "{text}");

    let output = sandbox.run(&["ls", "web:www"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("index.html"),
        "{}",
        stdout(&output)
    );
    assert!(
        sandbox.runtime_entries().is_empty(),
        "leftover sockets: {:?}",
        sandbox.runtime_entries()
    );
}

#[test]
fn ls_reports_connection_failures() {
    let sandbox = Sandbox::new(&[(
        "FAKE_SSH_FAIL",
        "deploy@web: Permission denied (publickey).",
    )]);
    let output = sandbox.run(&["ls", "web:"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("cannot connect to web"), "{text}");
    assert!(text.contains("Permission denied (publickey)"), "{text}");
    assert_eq!(sandbox.runtime_entries(), [] as [String; 0]);
}

#[test]
fn ls_declines_prompts_without_a_terminal() {
    if std::fs::File::open("/dev/tty").is_ok() {
        eprintln!("note: running with a controlling terminal, skipping");
        return;
    }
    let sandbox = Sandbox::new(&[("FAKE_SSH_PASSWORD", "hunter2")]);
    let output = sandbox.run(&["ls", "web:"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("Permission denied (password)"),
        "{}",
        stderr(&output)
    );
}

#[tokio::test]
async fn askpass_mode_relays_the_prompt_and_answer() {
    let runtime = tempfile::Builder::new()
        .prefix("st")
        .tempdir_in("/tmp")
        .unwrap();
    let (server, mut events) = AskpassServer::bind(runtime.path(), PathBuf::from(BINARY)).unwrap();
    let env = server.env("web");
    let child = tokio::process::Command::new(BINARY)
        .arg("deploy@web's password: ")
        .envs(env.vars())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let Some(AskpassEvent::Prompt(prompt)) = events.recv().await else {
        panic!("no prompt");
    };
    assert_eq!(prompt.context, "web");
    assert_eq!(prompt.message, "deploy@web's password: ");
    prompt.answer(SecretString::from("hunter2"));
    let output = child.wait_with_output().await.unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"hunter2\n");

    let child = tokio::process::Command::new(BINARY)
        .arg("Password: ")
        .envs(env.vars())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let Some(AskpassEvent::Prompt(prompt)) = events.recv().await else {
        panic!("no prompt");
    };
    prompt.cancel();
    let output = child.wait_with_output().await.unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
}
