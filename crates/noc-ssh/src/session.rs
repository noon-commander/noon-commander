//! Connections to a host: the `ControlMaster` process and SFTP channels (ADR 0002).

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use noc_vars::Template;
use rustix::process::{Pid, Signal, kill_process, kill_process_group};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;

use crate::askpass::AskpassEnv;
use crate::command::{
    Role, SshSettings, Target, command_command, control_command, exec_command, remote_command,
    session_command,
};
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
pub struct StderrTail {
    lines: Arc<Mutex<VecDeque<String>>>,
    /// To the task that reads the pipe, which forgets what is in it on request.
    reader: Option<mpsc::UnboundedSender<oneshot::Sender<()>>>,
}

impl StderrTail {
    /// The collected lines, oldest first.
    pub fn text(&self) -> String {
        let lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        lines
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Drops what the process has written so far: the lines, and what is still in the pipe,
    /// which the task that reads it may not have had its turn to read. Once connected, what ssh
    /// said while it connected, such as the picture of `VisualHostKey`, is not why the
    /// connection ends.
    pub async fn forget(&self) {
        if let Some(reader) = &self.reader {
            let (done, forgotten) = oneshot::channel();
            if reader.send(done).is_ok() && forgotten.await.is_ok() {
                return;
            }
        }
        // The pipe has ended, or there is none: the lines are all there is.
        self.clear();
    }

    fn clear(&self) {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn push(&self, line: &[u8]) {
        let line = String::from_utf8_lossy(line);
        let line = line.trim_end();
        if line.is_empty() {
            return;
        }
        let mut line = line.to_owned();
        truncate(&mut line, STDERR_LINE_MAX);
        let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == STDERR_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// A tail of `pipe`, which a task of its own reads line by line until it ends.
    fn read(pipe: ChildStderr, process: &'static str) -> (Self, JoinHandle<()>) {
        let tail = Self::default();
        let lines = tail.clone();
        let (ask, mut asked) = mpsc::unbounded_channel::<oneshot::Sender<()>>();
        let task = tokio::spawn(
            async move {
                let mut reader = BufReader::new(pipe);
                let mut line = Vec::new();
                loop {
                    tokio::select! {
                        biased;
                        Some(done) = asked.recv() => {
                            let ended = forget_written(&mut reader, &mut line, process);
                            lines.clear();
                            let _ = done.send(());
                            if ended {
                                break;
                            }
                        }
                        read = reader.read_until(b'\n', &mut line) => match read {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                log_line(process, &line);
                                lines.push(&line);
                                line.clear();
                            }
                        },
                    }
                }
            }
            .in_current_span(),
        );
        let tail = Self {
            reader: Some(ask),
            ..tail
        };
        (tail, task)
    }
}

/// Logs, and leaves out of the tail, what the process has written and was not taken yet: the
/// line begun, what `reader` has read ahead, and what is in the pipe now, read without
/// waiting, as tokio made the pipe non-blocking. Whether the pipe has ended.
fn forget_written(
    reader: &mut BufReader<ChildStderr>,
    line: &mut Vec<u8>,
    process: &'static str,
) -> bool {
    let mut written = std::mem::take(line);
    let ahead = reader.buffer();
    written.extend_from_slice(ahead);
    let ahead = ahead.len();
    reader.consume(ahead);
    let mut chunk = [0; 4096];
    let ended = loop {
        match rustix::io::read(reader.get_ref(), &mut chunk[..]) {
            Ok(0) => break true,
            Ok(read) => written.extend_from_slice(&chunk[..read]),
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => break false,
        }
    };
    for line in written.split(|&byte| byte == b'\n') {
        log_line(process, line);
    }
    ended
}

fn log_line(process: &'static str, line: &[u8]) {
    let text = String::from_utf8_lossy(line);
    let text = text.trim_end();
    if !text.is_empty() {
        tracing::debug!(process, "ssh: {text}");
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
    ///
    /// # Errors
    ///
    /// The errors of [`Target::validate`]. With multiplexing also
    /// [`SshError::ControlPathTooLong`] if the control socket in `runtime_dir` would not fit,
    /// [`SshError::Spawn`] if ssh cannot be started, [`SshError::Exited`] if the master exits
    /// before it has authenticated, [`SshError::Cancelled`] if `cancel` fires first, and
    /// [`SshError::Io`] if no random name can be made or waiting for ssh fails.
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
    ///
    /// # Errors
    ///
    /// [`SshError::Disconnected`] if the master connection is gone, [`SshError::Spawn`] if ssh
    /// cannot be started, [`SshError::Io`] if it has no pipe for stdin or stdout.
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
        let stderr = match child.stderr.take() {
            Some(pipe) => StderrTail::read(pipe, "sftp").0,
            None => StderrTail::default(),
        };
        Ok(SftpChannel {
            stdin,
            stdout,
            process: ChannelProcess { child, stderr },
        })
    }

    /// An ssh command that runs `command`, as typed, on the host in `dir` (the home directory
    /// if empty), as the last argument of `run`, whose variables the host's shell expands, over
    /// the master if there is one. The caller starts it with the terminal handed over, since
    /// the command gets a terminal of its own there (ADR 0019, ADR 0054).
    ///
    /// # Errors
    ///
    /// [`SshError::Disconnected`] if the master connection is gone.
    pub fn command_in(
        &self,
        dir: &[u8],
        run: &[Template],
        command: &str,
    ) -> Result<Command, SshError> {
        let control_path = self.control_path()?;
        let remote = remote_command(dir, run, command);
        Ok(command_command(
            &self.settings,
            &self.target,
            control_path,
            &remote,
        ))
    }

    /// Starts `program` with `args` on the host in `dir` (the home directory if empty), without a
    /// terminal and in a session of its own, over the master if there is one: stdin null, stdout
    /// and stderr piped, killed when the child is dropped. The host's shell gets
    /// `cd -- '<dir>' || exit`, then `exec` and the program and its arguments, each quoted for a
    /// POSIX shell, so that nothing in them is read as shell syntax (ADR 0028). Must be called
    /// within a tokio runtime.
    ///
    /// # Errors
    ///
    /// [`SshError::Disconnected`] if the master connection is gone, [`SshError::Spawn`] if ssh
    /// cannot be started.
    pub fn spawn_in(
        &self,
        dir: &[u8],
        program: &str,
        args: &[OsString],
    ) -> Result<Child, SshError> {
        let control_path = self.control_path()?;
        let remote = exec_command(dir, program, args);
        let role = Role::Exec {
            control_path,
            command: &remote,
        };
        session_command(&self.settings, &self.target, role, self.askpass.as_ref())
            .spawn()
            .map_err(|source| SshError::Spawn {
                program: self.settings.program.clone(),
                source,
            })
    }

    /// The control socket to multiplex over, if there is a master; an error if it is gone.
    fn control_path(&self) -> Result<Option<&Path>, SshError> {
        match &self.master {
            Some(master) if master.exited.is_cancelled() => Err(SshError::Disconnected {
                stderr: master.stderr.text(),
            }),
            Some(master) => Ok(Some(master.control_path.as_path())),
            None => Ok(None),
        }
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
    let child = session_command(settings, target, role, askpass)
        .spawn()
        .map_err(|source| SshError::Spawn {
            program: settings.program.clone(),
            source,
        })?;
    // Until the master exits: its stderr and its end are logged in it.
    let span =
        tracing::info_span!("ssh_master", destination = %target.destination, pid = child.id());
    wait_for_master(child, control_path, cancel)
        .instrument(span)
        .await
}

/// Waits until the master `child` has authenticated, as its control socket at `control_path`
/// shows, and watches it from then on.
async fn wait_for_master(
    mut child: Child,
    control_path: PathBuf,
    cancel: &CancellationToken,
) -> Result<Master, SshError> {
    let (stderr, drain) = match child.stderr.take() {
        Some(pipe) => {
            let (stderr, drain) = StderrTail::read(pipe, "master");
            (stderr, Some(drain))
        }
        None => (StderrTail::default(), None),
    };
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
    tracing::debug!("ssh master is ready");
    // ssh wrote it before it made the socket, so all of it is in the pipe by now.
    stderr.forget().await;
    let shutdown = CancellationToken::new();
    let exited = CancellationToken::new();
    let status = Arc::new(Mutex::new(None));
    let task = tokio::spawn(
        monitor(
            child,
            drain,
            shutdown.clone(),
            exited.clone(),
            Arc::clone(&status),
        )
        .in_current_span(),
    );
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
    drain: Option<JoinHandle<()>>,
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
    // ssh's last words, which say why it ended, may still be in the pipe.
    if let Some(drain) = drain {
        let _ = tokio::time::timeout(STDERR_GRACE, drain).await;
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

    #[tokio::test]
    async fn stderr_tail_forgets_what_came_before() {
        let tail = StderrTail::default();
        tail.push(b"+----[SHA256]-----+\n");
        tail.forget().await;
        assert_eq!(tail.text(), "");
        tail.push(b"Connection closed by remote host\n");
        assert_eq!(tail.text(), "Connection closed by remote host");
    }

    #[tokio::test]
    async fn stderr_tail_forgets_what_is_still_in_the_pipe() {
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                "echo '+----[SHA256]-----+' >&2; echo '|   .o.   |' >&2",
            ])
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let (tail, task) = StderrTail::read(child.stderr.take().unwrap(), "test");
        // All written, and none of it read: this waits without yielding, so that the task that
        // reads gets no turn.
        while child.try_wait().unwrap().is_none() {
            std::thread::sleep(Duration::from_millis(10));
        }
        tail.forget().await;
        task.await.unwrap();
        assert_eq!(tail.text(), "");
    }

    #[tokio::test]
    async fn stderr_tail_keeps_what_comes_after_it_forgot() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read line; echo 'closed by remote host' >&2"])
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let (tail, task) = StderrTail::read(child.stderr.take().unwrap(), "test");
        tail.forget().await;
        drop(child.stdin.take());
        task.await.unwrap();
        assert!(child.wait().await.unwrap().success());
        assert_eq!(tail.text(), "closed by remote host");
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
