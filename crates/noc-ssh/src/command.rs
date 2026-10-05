//! Assembly of ssh command lines.
//!
//! The order is fixed (AGENTS.md): program → forced options → `-F` and `ssh.args` → role
//! options → `--` → destination. ssh keeps the first value it sees for an option, so `-o`
//! values in user arguments cannot override forced options.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

use crate::askpass::AskpassEnv;
use crate::error::SshError;
use crate::policy;

/// Settings shared by every ssh invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshSettings {
    /// The ssh binary: a name looked up in `PATH`, or a path.
    pub program: PathBuf,
    /// Passed as `-F`; replaces `~/.ssh/config` and `/etc/ssh/ssh_config`.
    pub config_file: Option<PathBuf>,
    /// Extra arguments for every invocation (`ssh.args`).
    pub args: Vec<String>,
    /// One `ControlMaster` connection per host instead of one connection per channel.
    pub multiplex: bool,
    /// The working directory of every ssh process; `None` keeps the one of this process. A
    /// long-lived master or channel holds its directory, whose volume then cannot be unmounted.
    pub work_dir: Option<PathBuf>,
}

impl Default for SshSettings {
    fn default() -> Self {
        Self {
            program: PathBuf::from("ssh"),
            config_file: None,
            args: Vec::new(),
            multiplex: true,
            work_dir: None,
        }
    }
}

/// A host to talk to: an ssh destination, normally an `ssh_config` alias. Per-host options
/// belong in `ssh_config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub destination: String,
}

impl Target {
    pub fn new(destination: impl Into<String>) -> Self {
        Self {
            destination: destination.into(),
        }
    }

    /// Checks the destination and the user-supplied arguments of `settings`.
    pub fn validate(&self, settings: &SshSettings) -> Result<(), SshError> {
        if self.destination.is_empty() || self.destination.starts_with('-') {
            return Err(SshError::InvalidDestination(self.destination.clone()));
        }
        crate::args::validate(&settings.args)?;
        Ok(())
    }
}

/// What a session-related ssh process is started for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Role<'a> {
    /// `ssh -G`: print the effective configuration.
    Resolve,
    /// The `ControlMaster` connection.
    Master { control_path: &'a Path },
    /// An SFTP channel multiplexed over the master.
    MuxSftp { control_path: &'a Path },
    /// An SFTP channel with its own connection.
    DirectSftp,
    /// A command of the command line, with the terminal, over the master if there is one.
    Command {
        control_path: Option<&'a Path>,
        command: &'a OsStr,
    },
}

pub(crate) fn arguments(settings: &SshSettings, target: &Target, role: Role<'_>) -> Vec<OsString> {
    let mut argv = Vec::new();
    for (key, value) in forced_options(role) {
        argv.push(OsString::from("-o"));
        argv.push(OsString::from(format!("{key}={value}")));
    }
    if let Some(file) = &settings.config_file {
        argv.push("-F".into());
        argv.push(file.into());
    }
    argv.extend(settings.args.iter().map(OsString::from));
    match role {
        Role::Resolve => argv.push("-G".into()),
        Role::Master { control_path } => {
            argv.extend(["-M", "-N", "-S"].map(OsString::from));
            argv.push(control_path.into());
            argv.extend(["-o", "ControlPersist=no"].map(OsString::from));
        }
        Role::MuxSftp { control_path } => {
            argv.push("-S".into());
            argv.push(control_path.into());
            argv.extend(["-T", "-s"].map(OsString::from));
        }
        Role::DirectSftp => argv.extend(["-T", "-s"].map(OsString::from)),
        Role::Command { control_path, .. } => {
            if let Some(control_path) = control_path {
                argv.push("-S".into());
                argv.push(control_path.into());
            }
        }
    }
    argv.push("--".into());
    argv.push((&target.destination).into());
    match role {
        Role::MuxSftp { .. } | Role::DirectSftp => argv.push("sftp".into()),
        Role::Command { command, .. } => argv.push(command.into()),
        Role::Resolve | Role::Master { .. } => {}
    }
    argv
}

fn forced_options(role: Role<'_>) -> Vec<(&'static str, &'static str)> {
    let groups: &[&[(&'static str, &'static str)]] = match role {
        Role::Resolve => &[],
        Role::Master { .. } => &[
            policy::MASTER_OPTIONS,
            policy::PROCESS_OPTIONS,
            policy::session_options(),
        ],
        Role::MuxSftp { .. } => &[
            policy::SFTP_CHANNEL_OPTIONS,
            policy::PROCESS_OPTIONS,
            policy::MUX_CHANNEL_OPTIONS,
        ],
        Role::DirectSftp => &[
            policy::SFTP_CHANNEL_OPTIONS,
            policy::PROCESS_OPTIONS,
            policy::session_options(),
        ],
        Role::Command { .. } => &[
            policy::COMMAND_OPTIONS,
            policy::PROCESS_OPTIONS,
            policy::session_options(),
        ],
    };
    let mut options: Vec<(&'static str, &'static str)> = Vec::new();
    for &(key, value) in groups.iter().copied().flatten() {
        // ssh keeps the first value of an option, so a repeated key would be dead weight.
        if !options
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(key))
        {
            options.push((key, value));
        }
    }
    options
}

/// Command for a master, an SFTP channel, or `ssh -G`.
pub(crate) fn session_command(
    settings: &SshSettings,
    target: &Target,
    role: Role<'_>,
    askpass: Option<&AskpassEnv>,
) -> Command {
    let mut command = base_command(settings);
    command.args(arguments(settings, target, role));
    match role {
        Role::Resolve => {
            command.stdout(Stdio::piped());
        }
        Role::Master { .. } | Role::Command { .. } => {}
        Role::MuxSftp { .. } | Role::DirectSftp => {
            command.stdin(Stdio::piped()).stdout(Stdio::piped());
        }
    }
    if let Some(askpass) = askpass {
        askpass.apply(&mut command);
    }
    command
}

/// `ssh` running a command of the command line: unlike every other ssh child it keeps this
/// process's terminal, which the caller hands over, since the command needs it; ssh asks there
/// too, if it has to (ADR 0019).
pub(crate) fn command_command(
    settings: &SshSettings,
    target: &Target,
    control_path: Option<&Path>,
    remote: &OsStr,
) -> Command {
    let mut command = Command::new(&settings.program);
    let role = Role::Command {
        control_path,
        command: remote,
    };
    command
        .args(arguments(settings, target, role))
        .kill_on_drop(true);
    if let Some(dir) = &settings.work_dir {
        command.current_dir(dir);
    }
    command
}

/// What the host's shell runs for `command` in `dir`: a `cd` to the directory quoted for a
/// POSIX shell, unless it is empty, the home directory where ssh starts; then the command as
/// typed, on lines of its own, so that a failed `cd` runs none of it.
pub(crate) fn remote_command(dir: &[u8], command: &str) -> OsString {
    let mut text = Vec::new();
    if !dir.is_empty() {
        text.extend_from_slice(b"cd -- '");
        for &byte in dir {
            if byte == b'\'' {
                text.extend_from_slice(b"'\\''");
            } else {
                text.push(byte);
            }
        }
        text.extend_from_slice(b"' || exit\n");
    }
    text.extend_from_slice(command.as_bytes());
    OsStr::from_bytes(&text).to_owned()
}

/// `ssh -V`.
pub(crate) fn version_command(settings: &SshSettings) -> Command {
    let mut command = base_command(settings);
    command.arg("-V").stdout(Stdio::piped());
    command
}

/// `ssh -O <operation>` for a control socket. `-F /dev/null` keeps ssh from evaluating any
/// config, and so from running `Match exec`, for a command that only talks to the socket.
pub(crate) fn control_command(
    settings: &SshSettings,
    control_path: &Path,
    operation: &str,
) -> Command {
    let mut command = base_command(settings);
    command
        .args(["-F", "/dev/null", "-S"])
        .arg(control_path)
        .args(["-O", operation, "--", "noc"]);
    command
}

fn base_command(settings: &SshSettings) -> Command {
    let mut command = Command::new(&settings.program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = &settings.work_dir {
        command.current_dir(dir);
    }
    detach_from_terminal(&mut command);
    command
}

/// Starts the child in a new session without a controlling terminal, so ssh can neither read
/// from nor draw on the TUI's terminal; prompts go through the askpass bridge (ADR 0003).
#[allow(unsafe_code)]
fn detach_from_terminal(command: &mut Command) {
    // SAFETY: the hook runs in the forked child before `exec` and only calls setsid(2), which
    // is async-signal-safe; converting its error code does not allocate.
    unsafe {
        command.pre_exec(|| {
            rustix::process::setsid()
                .map(drop)
                .map_err(std::io::Error::from)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(argv: &[OsString]) -> Vec<String> {
        argv.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn options(options: &[(&str, &str)]) -> Vec<String> {
        options
            .iter()
            .flat_map(|(key, value)| ["-o".to_owned(), format!("{key}={value}")])
            .collect()
    }

    fn settings() -> SshSettings {
        SshSettings {
            config_file: Some(PathBuf::from("/cfg")),
            args: vec!["-v".to_owned()],
            ..SshSettings::default()
        }
    }

    fn target() -> Target {
        Target::new("web")
    }

    fn tail(rest: &[&str]) -> Vec<String> {
        ["-F", "/cfg", "-v"]
            .iter()
            .chain(rest)
            .map(|arg| (*arg).to_owned())
            .collect()
    }

    #[test]
    fn resolve_has_no_forced_options() {
        let argv = strings(&arguments(&settings(), &target(), Role::Resolve));
        assert_eq!(argv, tail(&["-G", "--", "web"]));
    }

    #[test]
    fn master_starts_with_forced_options() {
        let path = Path::new("/run/1/abcd");
        let argv = strings(&arguments(
            &settings(),
            &target(),
            Role::Master { control_path: path },
        ));
        let mut expected = options(policy::MASTER_OPTIONS);
        expected.extend(options(policy::PROCESS_OPTIONS));
        expected.extend(options(policy::session_options()));
        expected.extend(tail(&[
            "-M",
            "-N",
            "-S",
            "/run/1/abcd",
            "-o",
            "ControlPersist=no",
            "--",
            "web",
        ]));
        assert_eq!(argv, expected);
    }

    #[test]
    fn mux_channel_uses_sftp_options() {
        let path = Path::new("/run/1/abcd");
        let argv = strings(&arguments(
            &settings(),
            &target(),
            Role::MuxSftp { control_path: path },
        ));
        let mut expected = options(policy::SFTP_CHANNEL_OPTIONS);
        expected.extend(options(policy::PROCESS_OPTIONS));
        expected.extend(options(policy::MUX_CHANNEL_OPTIONS));
        expected.extend(tail(&[
            "-S",
            "/run/1/abcd",
            "-T",
            "-s",
            "--",
            "web",
            "sftp",
        ]));
        assert_eq!(argv, expected);
    }

    #[test]
    fn direct_channel_adds_session_options_once() {
        let argv = strings(&arguments(&settings(), &target(), Role::DirectSftp));
        let forced: Vec<_> = argv
            .chunks(2)
            .take_while(|pair| pair[0] == "-o")
            .map(|pair| pair[1].split('=').next().unwrap_or_default().to_owned())
            .collect();
        let mut unique = forced.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            forced.len(),
            unique.len(),
            "duplicate forced options: {forced:?}"
        );
        for (key, _) in policy::session_options() {
            assert!(forced.iter().any(|forced| forced == key), "{key} missing");
        }
        assert!(argv.ends_with(&tail(&["-T", "-s", "--", "web", "sftp"])));
    }

    #[test]
    fn a_command_keeps_its_text_after_the_destination() {
        let path = Path::new("/run/1/abcd");
        let remote = OsStr::new("ls -l");
        let argv = strings(&arguments(
            &settings(),
            &target(),
            Role::Command {
                control_path: Some(path),
                command: remote,
            },
        ));
        let mut expected = options(policy::COMMAND_OPTIONS);
        expected.extend(options(policy::PROCESS_OPTIONS));
        expected.extend(options(policy::session_options()));
        expected.extend(tail(&["-S", "/run/1/abcd", "--", "web", "ls -l"]));
        assert_eq!(argv, expected);
        let direct = strings(&arguments(
            &settings(),
            &target(),
            Role::Command {
                control_path: None,
                command: remote,
            },
        ));
        assert!(direct.ends_with(&tail(&["--", "web", "ls -l"])));
        assert!(!direct.iter().any(|arg| arg == "-S"));
    }

    #[test]
    fn the_remote_command_changes_to_the_quoted_directory_first() {
        assert_eq!(
            remote_command(b"/srv/it's here", "make\nls"),
            OsStr::new("cd -- '/srv/it'\\''s here' || exit\nmake\nls")
        );
        assert_eq!(remote_command(b"", "uptime"), OsStr::new("uptime"));
        assert_eq!(
            remote_command(b"caf\xe9", "ls").as_bytes(),
            b"cd -- 'caf\xe9' || exit\nls"
        );
    }

    #[test]
    fn destinations_starting_with_a_dash_are_rejected() {
        let settings = SshSettings::default();
        assert!(matches!(
            Target::new("-oProxyCommand=x").validate(&settings),
            Err(SshError::InvalidDestination(_))
        ));
        assert!(matches!(
            Target::new("").validate(&settings),
            Err(SshError::InvalidDestination(_))
        ));
        assert!(Target::new("web").validate(&settings).is_ok());
    }

    #[test]
    fn ssh_runs_in_the_work_dir_if_there_is_one() {
        let path = Path::new("/run/1/abcd");
        let inherited = control_command(&SshSettings::default(), path, "exit");
        assert_eq!(inherited.as_std().get_current_dir(), None);

        let settings = SshSettings {
            work_dir: Some(PathBuf::from("/home/me")),
            ..settings()
        };
        let commands = [
            session_command(&settings, &target(), Role::Resolve, None),
            session_command(&settings, &target(), Role::DirectSftp, None),
            control_command(&settings, path, "exit"),
            version_command(&settings),
        ];
        for command in commands {
            assert_eq!(
                command.as_std().get_current_dir(),
                Some(Path::new("/home/me"))
            );
        }
    }
}
