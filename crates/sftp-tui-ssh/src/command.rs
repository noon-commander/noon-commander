//! Assembly of ssh command lines.
//!
//! The order is fixed (AGENTS.md): program → forced options → `-F` and `ssh.args` → host
//! args → role options → `--` → destination. ssh keeps the first value it sees for an
//! option, so `-o` values in user arguments cannot override forced options.

use std::ffi::OsString;
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
}

impl Default for SshSettings {
    fn default() -> Self {
        Self {
            program: PathBuf::from("ssh"),
            config_file: None,
            args: Vec::new(),
            multiplex: true,
        }
    }
}

/// A host to talk to: an ssh destination, normally an `ssh_config` alias, and its extra
/// arguments (`hosts.<alias>.args`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub destination: String,
    pub args: Vec<String>,
}

impl Target {
    pub fn new(destination: impl Into<String>) -> Self {
        Self {
            destination: destination.into(),
            args: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Checks the destination and the user-supplied arguments of `settings` and `self`.
    pub fn validate(&self, settings: &SshSettings) -> Result<(), SshError> {
        if self.destination.is_empty() || self.destination.starts_with('-') {
            return Err(SshError::InvalidDestination(self.destination.clone()));
        }
        crate::args::validate(&settings.args)?;
        crate::args::validate(&self.args)?;
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
    argv.extend(target.args.iter().map(OsString::from));
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
    }
    argv.push("--".into());
    argv.push((&target.destination).into());
    if matches!(role, Role::MuxSftp { .. } | Role::DirectSftp) {
        argv.push("sftp".into());
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
        Role::Master { .. } => {}
        Role::MuxSftp { .. } | Role::DirectSftp => {
            command.stdin(Stdio::piped()).stdout(Stdio::piped());
        }
    }
    if let Some(askpass) = askpass {
        askpass.apply(&mut command);
    }
    command
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
        .args(["-O", operation, "--", "sftp-tui"]);
    command
}

fn base_command(settings: &SshSettings) -> Command {
    let mut command = Command::new(&settings.program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
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
        Target::new("web").with_args(vec!["-p".to_owned(), "2222".to_owned()])
    }

    fn tail(rest: &[&str]) -> Vec<String> {
        ["-F", "/cfg", "-v", "-p", "2222"]
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
}
