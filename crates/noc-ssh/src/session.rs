//! Connections to a host: the `ControlMaster` process and SFTP channels (ADR 0002).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process, kill_process_group};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::askpass::AskpassEnv;
use crate::command::{Role, SshSettings, Target, control_command, session_command};
use crate::error::SshError;
use crate::runtime::{self, CONTROL_PREFIX, MAX_CONTROL_PATH_LEN};

/// How often to check whether the master has created its control socket.
const READY_POLL: Duration = Duration::from_millis(50);
/// How long a process gets to exit after SIGTERM or `-O exit` before it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(3);
/// How long to wait for the rest of stderr after a process exited.
const STDERR_GRACE: Duration = Duration::from_millis(500);

const STDERR_LINES: usize = 20;
const STDERR_LINE_MAX: usize = 1024;

/// The last lines a process wrote to stderr.
#[derive(Debug, Clone, Default)]
pub struct StderrTail(Arc<Mutex<VecDeque<String>>>);

impl StderrTail {
    /// The collected lines, oldest first.
    pub fn text(&self) -> String {
        let lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        lines
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn push(&self, line: &[u8]) {
        let line = String::from_utf8_lossy(line);
        let line = line.trim_end();
        if line.is_empty() {
            return;
        }
        let mut line = line.to_owned();
        truncate(&mut line, STDERR_LINE_MAX);
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == STDERR_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    fn drain(&self, pipe: ChildStderr, process: &'static str) -> JoinHandle<()> {
        let tail = self.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(pipe);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = String::from_utf8_lossy(&line);
                        tracing::debug!(process, "ssh: {}", text.trim_end());
                        tail.push(&line);
                    }
                }
            }
        })
    }
}

fn truncate(line: &mut String, max: usize) {
    if line.len() > max {
        let mut end = max;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
    }
}

/// The last lines of a complete stderr output.
pub(crate) fn stderr_tail(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    lines[lines.len().saturating_sub(STDERR_LINES)..].join("\n")
}

/// A connection to one host.
///
/// With multiplexing, a `ControlMaster` process authenticates once and carries every channel;
/// without it, each channel is a separate ssh connection. Dropping a session terminates its
/// master; [`Session::close`] shuts it down gracefully.
#[derive(Debug)]
pub struct Session {
    settings: SshSettings,
    target: Target,
    askpass: Option<AskpassEnv>,
    master: Option<Master>,
}

#[derive(Debug)]
struct Master {
    control_path: PathBuf,
    stderr: StderrTail,
    shutdown: CancellationToken,
    exited: CancellationToken,
    status: Arc<Mutex<Option<ExitStatus>>>,
    task: Option<JoinHandle<()>>,
}

impl Session {
    /// Connects to `target`. With multiplexing this starts the master connection and returns
    /// once it has authenticated, which may involve prompts through `askpass`.
    pub async fn connect(
        settings: &SshSettings,
        target: &Target,
        runtime_dir: &Path,
        askpass: Option<AskpassEnv>,
        cancel: &CancellationToken,
    ) -> Result<Self, SshError> {
        target.validate(settings)?;
        let master = if settings.multiplex {
            Some(start_master(settings, target, runtime_dir, askpass.as_ref(), cancel).await?)
        } else {
            None
        };
        Ok(Self {
            settings: settings.clone(),
            target: target.clone(),
            askpass,
            master,
        })
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn is_multiplexed(&self) -> bool {
        self.master.is_some()
    }

    /// Whether the master connection has exited. Always `false` without multiplexing.
    pub fn is_closed(&self) -> bool {
        self.master
            .as_ref()
            .is_some_and(|master| master.exited.is_cancelled())
    }

    /// Completes when the master connection exits; never without multiplexing.
    pub async fn closed(&self) {
        match &self.master {
            Some(master) => master.exited.cancelled().await,
            None => std::future::pending().await,
        }
    }

    /// Exit status of the master connection, once it has exited.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        let master = self.master.as_ref()?;
        *master.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The last stderr lines of the master connection.
    pub fn stderr(&self) -> String {
        self.master
            .as_ref()
            .map(|master| master.stderr.text())
            .unwrap_or_default()
    }

    /// Starts an `ssh -s … sftp` process. Must be called within a tokio runtime.
    pub fn open_sftp(&self) -> Result<SftpChannel, SshError> {
        let role = match &self.master {
            Some(master) if master.exited.is_cancelled() => {
                return Err(SshError::Disconnected {
                    stderr: master.stderr.text(),
                });
            }
            Some(master) => Role::MuxSftp {
                control_path: &master.control_path,
            },
            None => Role::DirectSftp,
        };
        let mut child = session_command(&self.settings, &self.target, role, self.askpass.as_ref())
            .spawn()
            .map_err(|source| SshError::Spawn {
                program: self.settings.program.clone(),
                source,
            })?;
        let stdin = child.stdin.take().ok_or_else(missing_pipe)?;
        let stdout = child.stdout.take().ok_or_else(missing_pipe)?;
        let stderr = StderrTail::default();
        if let Some(pipe) = child.stderr.take() {
            stderr.drain(pipe, "sftp");
        }
        Ok(SftpChannel {
            stdin,
            stdout,
            process: ChannelProcess { child, stderr },
        })
    }

    /// Shuts the master connection down gracefully (`ssh -O exit`), killing it if needed.
    pub async fn close(mut self) {
        let Some(mut master) = self.master.take() else {
            return;
        };
        if !master.exited.is_cancelled() {
            let mut command = control_command(&self.settings, &master.control_path, "exit");
            let _ = tokio::time::timeout(Duration::from_secs(5), command.output()).await;
            if tokio::time::timeout(EXIT_GRACE, master.exited.cancelled())
                .await
                .is_err()
            {
                master.shutdown.cancel();
            }
        }
        master.shutdown.cancel();
        if let Some(task) = master.task.take() {
            let _ = task.await;
        }
        let _ = tokio::fs::remove_file(&master.control_path).await;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(master) = &self.master {
            master.shutdown.cancel();
        }
    }
}

fn missing_pipe() -> SshError {
    SshError::Io(std::io::Error::other("ssh pipe is not available"))
}

async fn start_master(
    settings: &SshSettings,
    target: &Target,
    runtime_dir: &Path,
    askpass: Option<&AskpassEnv>,
    cancel: &CancellationToken,
) -> Result<Master, SshError> {
    let control_path = runtime_dir.join(runtime::socket_name(CONTROL_PREFIX)?);
    let len = control_path.as_os_str().len();
    if len > MAX_CONTROL_PATH_LEN {
        return Err(SshError::ControlPathTooLong {
            path: control_path,
            len,
            max: MAX_CONTROL_PATH_LEN,
        });
    }
    let role = Role::Master {
        control_path: &control_path,
    };
    let mut child = session_command(settings, target, role, askpass)
        .spawn()
        .map_err(|source| SshError::Spawn {
            program: settings.program.clone(),
            source,
        })?;
    let stderr = StderrTail::default();
    let drain = child.stderr.take().map(|pipe| stderr.drain(pipe, "master"));
    let mut poll = tokio::time::interval(READY_POLL);
    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                terminate(&mut child).await;
                return Err(SshError::Cancelled);
            }
            status = child.wait() => {
                let status = status?;
                if let Some(drain) = drain {
                    let _ = tokio::time::timeout(STDERR_GRACE, drain).await;
                }
                return Err(SshError::Exited { status, stderr: stderr.text() });
            }
            _ = poll.tick() => {
                // ssh creates the control socket once it has authenticated.
                if tokio::fs::symlink_metadata(&control_path).await.is_ok() {
                    break;
                }
            }
        }
    }
    tracing::debug!(destination = %target.destination, "ssh master is ready");
    let shutdown = CancellationToken::new();
    let exited = CancellationToken::new();
    let status = Arc::new(Mutex::new(None));
    let task = tokio::spawn(monitor(
        child,
        shutdown.clone(),
        exited.clone(),
        Arc::clone(&status),
    ));
    Ok(Master {
        control_path,
        stderr,
        shutdown,
        exited,
        status,
        task: Some(task),
    })
}

async fn monitor(
    mut child: Child,
    shutdown: CancellationToken,
    exited: CancellationToken,
    slot: Arc<Mutex<Option<ExitStatus>>>,
) {
    let status = tokio::select! {
        status = child.wait() => status.ok(),
        () = shutdown.cancelled() => terminate(&mut child).await,
    };
    if let Some(status) = status {
        tracing::debug!(%status, "ssh master exited");
    }
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = status;
    exited.cancel();
}

/// SIGTERM lets ssh remove its control socket and stop its `ProxyCommand`. If it does not exit
/// in time, SIGKILL goes to its whole process group, which `setsid` made separate from ours.
async fn terminate(child: &mut Child) -> Option<ExitStatus> {
    let pid = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw);
    if let Some(pid) = pid {
        let _ = kill_process(pid, Signal::TERM);
        if let Ok(Ok(status)) = tokio::time::timeout(EXIT_GRACE, child.wait()).await {
            return Some(status);
        }
        let _ = kill_process_group(pid, Signal::KILL);
    }
    let _ = child.kill().await;
    child.try_wait().ok().flatten()
}

/// An SFTP channel: the pipes of an `ssh -s … sftp` process.
#[derive(Debug)]
pub struct SftpChannel {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    /// Keep it alive as long as the SFTP session; dropping it kills the process.
    pub process: ChannelProcess,
}

/// The ssh process behind an SFTP channel.
#[derive(Debug)]
pub struct ChannelProcess {
    child: Child,
    stderr: StderrTail,
}

impl ChannelProcess {
    /// A handle to the last lines the process wrote to stderr.
    pub fn stderr(&self) -> StderrTail {
        self.stderr.clone()
    }

    /// The operating system process id, while the process is running.
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Completes when the process exits on its own, for example when its connection is lost.
    /// Cancel-safe.
    pub async fn wait(&mut self) -> Option<ExitStatus> {
        self.child.wait().await.ok()
    }

    /// Waits for ssh to exit after its SFTP session was closed; kills it if that takes long.
    pub async fn finish(mut self) -> Option<ExitStatus> {
        match tokio::time::timeout(EXIT_GRACE, self.child.wait()).await {
            Ok(Ok(status)) => Some(status),
            _ => terminate(&mut self.child).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_tail_keeps_the_last_lines() {
        let tail = StderrTail::default();
        for index in 0..30 {
            tail.push(format!("line {index}\r\n").as_bytes());
        }
        tail.push(b"\n");
        let text = tail.text();
        assert_eq!(text.lines().count(), STDERR_LINES);
        assert!(text.starts_with("line 10\n"));
        assert!(text.ends_with("line 29"));
    }

    #[test]
    fn stderr_tail_truncates_long_lines() {
        let tail = StderrTail::default();
        tail.push("é".repeat(STDERR_LINE_MAX).as_bytes());
        assert!(tail.text().len() <= STDERR_LINE_MAX);
    }

    #[test]
    fn stderr_tail_of_complete_output() {
        assert_eq!(stderr_tail("a\n\nb\n"), "a\nb");
        let many = (0..30).map(|index| index.to_string()).collect::<Vec<_>>();
        assert!(stderr_tail(&many.join("\n")).starts_with("10\n"));
    }
}
