//! A program that runs with the terminal handed over, as a shell runs a job (ADR 0054): in a
//! process group of its own, which gets the terminal, so that Ctrl+c and Ctrl+z reach it alone,
//! with the terminal taken back when it ends or is stopped.

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
use rustix::process::{
    Pid, Signal as ProcessSignal, WaitId, WaitIdOptions, getpgrp, kill_process_group, waitid,
};
use tokio::process::{Child, Command};
use tokio::signal::unix::{SignalKind, signal};

use crate::ToolError;

/// Starts `command`, which runs `program`, with this process's terminal, which the caller has
/// handed over. Where this process's group has the terminal, it runs as a job: in a process
/// group of its own, which gets the terminal. Without the terminal, it runs in this process's
/// group. Either way it is killed when the [`Running`] is dropped before it ends.
pub(crate) fn start(mut command: Command, program: &Path) -> Result<Running, ToolError> {
    let spawn_error = |source| ToolError::Spawn {
        program: program.to_path_buf(),
        source,
    };
    // Before the child, so that no change of its state goes unheard.
    let child_signals = in_foreground()
        .then(|| signal(SignalKind::child()).ok())
        .flatten();
    command.kill_on_drop(true);
    if child_signals.is_some() {
        command.process_group(0);
    }
    let child = command.spawn().map_err(spawn_error)?;
    let pid = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw);
    let job = match (pid, child_signals) {
        (Some(pid), Some(signals)) => {
            let job = Job {
                pid,
                signals,
                ended: false,
            };
            // It may have found itself in the background before this, as an interactive shell
            // checks first thing, and stopped itself: it goes on now.
            let _ = job.to_foreground();
            Some(job)
        }
        _ => None,
    };
    Ok(Running {
        child,
        program: program.to_path_buf(),
        job,
    })
}

/// A program that [`Shell::start`](crate::shell::Shell::start) or
/// [`Editor::start`](crate::editor::Editor::start) started.
#[derive(Debug)]
pub struct Running {
    child: Child,
    program: PathBuf,
    /// Where it runs as a job: in a group of its own with the terminal.
    job: Option<Job>,
}

/// How a program that [`Running::wait`] waited for is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    Ended(ExitStatus),
    /// Stopped, as by Ctrl+z: [`Running::resume`] or [`Running::end`] says what next.
    Stopped,
}

impl Running {
    /// Waits for the program to end or to be stopped, and then takes the terminal back for this
    /// process's group. zsh runs the last command of `-c` in its own place, so it is that
    /// command that ends or is stopped, and nothing else would take the terminal back. A
    /// command that a shell runs as a job of its own is the shell's to stop.
    pub async fn wait(&mut self) -> Result<Waited, ToolError> {
        let Self {
            child,
            program,
            job,
        } = self;
        let wait_error = |source| ToolError::Spawn {
            program: program.clone(),
            source,
        };
        let Some(job) = job else {
            return child.wait().await.map(Waited::Ended).map_err(wait_error);
        };
        loop {
            tokio::select! {
                // First, so that a child that ended is reaped before its pid is asked about.
                biased;
                status = child.wait() => {
                    // Nothing better is left to do if this fails: the next use of the terminal
                    // stops this process, and the shell it was started from says so.
                    let _ = set_foreground(getpgrp());
                    return status.map(Waited::Ended).map_err(wait_error);
                }
                heard = job.signals.recv() => {
                    if heard.is_none() {
                        return child.wait().await.map(Waited::Ended).map_err(wait_error);
                    }
                }
            }
            match job.stopped_by() {
                // It is in the background, which it should not be: it goes on in front.
                Some(signal)
                    if signal == Signal::SIGTTIN as i32 || signal == Signal::SIGTTOU as i32 =>
                {
                    let _ = job.to_foreground();
                }
                Some(_) => {
                    let _ = set_foreground(getpgrp());
                    return Ok(Waited::Stopped);
                }
                None => {}
            }
        }
    }

    /// Gives the terminal back to a stopped program, and continues it.
    pub fn resume(&mut self) -> io::Result<()> {
        match &self.job {
            Some(job) => job.to_foreground(),
            None => Ok(()),
        }
    }

    /// Ends a stopped program as a shell ends the stopped jobs it leaves: SIGHUP, then
    /// SIGCONT, so that it hears it, with the terminal, so that it can put the terminal back as
    /// it was. One that goes on after that, as a server that takes SIGHUP for reading its
    /// settings again, gets SIGKILL the next time; [`has_ended`](Self::has_ended) says so.
    pub fn end(&mut self) -> io::Result<()> {
        let Some(job) = &mut self.job else {
            return Ok(());
        };
        let group = job.pid;
        if job.ended {
            return Ok(kill_process_group(group, ProcessSignal::KILL)?);
        }
        job.ended = true;
        let _ = set_foreground(group);
        kill_process_group(group, ProcessSignal::HUP)?;
        Ok(kill_process_group(group, ProcessSignal::CONT)?)
    }

    /// Whether [`end`](Self::end) has asked it to end, so that the next one kills it.
    pub fn has_ended(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.ended)
    }

    pub fn program(&self) -> &Path {
        &self.program
    }
}

impl Drop for Running {
    /// One that has not ended goes with all of its group, which `kill_on_drop` alone would
    /// leave, and the terminal comes back. Jobs that a shell moved to groups of their own are
    /// out of reach: stopped ones end once their group is left without a parent in it, as the
    /// kernel ends such groups; running ones go on, as with `nohup`.
    fn drop(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = kill_process_group(job.pid, ProcessSignal::KILL);
        let _ = set_foreground(getpgrp());
    }
}

/// A program that runs in a process group of its own, whose id is its pid.
#[derive(Debug)]
struct Job {
    pid: Pid,
    /// SIGCHLD, which comes when a child of this process ends or is stopped.
    signals: tokio::signal::unix::Signal,
    /// [`Running::end`] has sent it SIGHUP.
    ended: bool,
}

impl Job {
    /// Gives its group the terminal and continues it, if it was stopped.
    fn to_foreground(&self) -> io::Result<()> {
        set_foreground(self.pid)?;
        Ok(kill_process_group(self.pid, ProcessSignal::CONT)?)
    }

    /// The signal that stopped it, if it is stopped and has not said so yet.
    fn stopped_by(&self) -> Option<i32> {
        let options = WaitIdOptions::STOPPED | WaitIdOptions::NOHANG;
        waitid(WaitId::Pid(self.pid), options)
            .ok()
            .flatten()
            .and_then(|status| status.stopping_signal())
    }
}

/// Whether the terminal of stdin has this process's group in the foreground.
pub(crate) fn in_foreground() -> bool {
    rustix::termios::tcgetpgrp(io::stdin()).is_ok_and(|pgrp| pgrp == getpgrp())
}

/// Puts `group` in the foreground of the terminal of stdin. This process may be in the
/// background then, after a job that ended or was stopped with the terminal, and taking it from
/// there would stop this process with SIGTTOU, as shells know; so the signal is blocked in this
/// thread meanwhile.
fn set_foreground(group: Pid) -> io::Result<()> {
    let tty = io::stdin();
    if rustix::termios::tcgetpgrp(&tty).is_ok_and(|pgrp| pgrp == group) {
        return Ok(());
    }
    let mut ttou = SigSet::empty();
    ttou.add(Signal::SIGTTOU);
    let mut mask = SigSet::empty();
    pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&ttou), Some(&mut mask))?;
    let set = rustix::termios::tcsetpgrp(&tty, group);
    pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&mask), None)?;
    Ok(set?)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::process::ExitStatusExt as _;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use noc_vars::Template;

    use super::*;
    use crate::editor::Editor;
    use crate::shell::Shell;

    fn shell(words: &[&str]) -> Shell {
        let words: Vec<Template> = words.iter().map(|word| Template::literal(word)).collect();
        Shell::expand(&words, |_| None)
    }

    async fn ended(running: &mut Running) -> ExitStatus {
        match running.wait().await.unwrap() {
            Waited::Ended(status) => status,
            Waited::Stopped => panic!("stopped"),
        }
    }

    /// Whether process `pid` is there and not a zombie.
    fn alive(pid: &str) -> bool {
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout);
        !stat.trim().is_empty() && !stat.trim().starts_with('Z')
    }

    /// The text of `path` once it has some, or after five seconds.
    async fn text_of(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if text.ends_with('\n') || Instant::now() > deadline {
                return text;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// What `script` runs for [`jobs_run_with_the_terminal`], in a terminal of its own, and
    /// prints a line for each part that held. Without a terminal it does nothing.
    #[tokio::test]
    #[ignore = "run in a terminal by jobs_run_with_the_terminal"]
    async fn in_a_terminal_of_its_own() {
        if !in_foreground() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let sh = shell(&["/bin/sh", "-c"]);

        // zsh with job control runs `ls` in its own place, and nothing gives the terminal
        // back when it ends.
        if Path::new("/bin/zsh").exists() {
            let zsh = shell(&["/bin/zsh", "-f", "-i", "-c"]);
            let mut running = zsh.start(dir.path(), "/bin/ls > /dev/null").unwrap();
            assert!(ended(&mut running).await.success());
            assert!(in_foreground(), "the terminal is back");
            println!("AFTER ZSH: OURS");
        } else {
            println!("AFTER ZSH: OURS (no zsh)");
        }

        // The command has the terminal while it runs.
        let ours = getpgrp().as_raw_nonzero().get();
        let check = format!("[ \"$(ps -o tpgid= -p $$)\" -ne {ours} ] && echo apart > out");
        assert!(
            ended(&mut sh.start(dir.path(), &check).unwrap())
                .await
                .success()
        );
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "apart\n");
        println!("WHILE IT RUNS: ITS OWN");

        // Stopped, as by Ctrl+z: the terminal is back, and it goes on, or ends.
        let stops = "kill -TSTP $$; echo went on > out";
        let mut running = sh.start(dir.path(), stops).unwrap();
        assert_eq!(running.wait().await.unwrap(), Waited::Stopped);
        assert!(in_foreground(), "the terminal is back while it is stopped");
        running.resume().unwrap();
        assert!(ended(&mut running).await.success());
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "went on\n");
        assert!(in_foreground());
        println!("STOPPED: GOES ON");

        std::fs::remove_file(&out).unwrap();
        let mut running = sh.start(dir.path(), stops).unwrap();
        assert_eq!(running.wait().await.unwrap(), Waited::Stopped);
        assert!(!running.has_ended());
        running.end().unwrap();
        assert_eq!(ended(&mut running).await.signal(), Some(1));
        assert!(!out.exists(), "it did not go on");
        assert!(in_foreground());
        println!("STOPPED: ENDS");

        // One that takes no SIGHUP goes on, and the next end kills it.
        let deaf = "trap '' HUP; kill -TSTP $$; kill -TSTP $$; echo went on > out";
        let mut running = sh.start(dir.path(), deaf).unwrap();
        assert_eq!(running.wait().await.unwrap(), Waited::Stopped);
        running.end().unwrap();
        assert_eq!(running.wait().await.unwrap(), Waited::Stopped);
        assert!(running.has_ended());
        running.end().unwrap();
        assert_eq!(ended(&mut running).await.signal(), Some(9));
        assert!(!out.exists(), "it did not go on");
        println!("DEAF TO SIGHUP: KILLED");

        // The editor of F4 is a job too.
        let editor_path = dir.path().join("editor");
        std::fs::write(
            &editor_path,
            "#!/bin/sh\nkill -TSTP $$\necho edited >> \"$1\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&editor_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let editor_text = editor_path.to_string_lossy().into_owned();
        let editor = Editor::from_vars(|name| (name == "EDITOR").then(|| editor_text.clone()));
        let mut running = editor.start(&out, None).unwrap();
        assert_eq!(running.wait().await.unwrap(), Waited::Stopped);
        assert!(in_foreground());
        running.resume().unwrap();
        assert!(ended(&mut running).await.success());
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "edited\n");
        println!("EDITOR: A JOB");

        // Dropped before it ends, it goes with all of its group, and the terminal comes back.
        let pids = dir.path().join("pids");
        let two = "sleep 30 & echo \"$! $$\" > pids; exec sleep 30";
        let running = sh.start(dir.path(), two).unwrap();
        let both = text_of(&pids).await;
        let both: Vec<&str> = both.split_whitespace().collect();
        assert_eq!(both.len(), 2, "{both:?}");
        assert!(both.iter().all(|pid| alive(pid)));
        drop(running);
        let deadline = Instant::now() + Duration::from_secs(5);
        while both.iter().any(|pid| alive(pid)) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!both.iter().any(|pid| alive(pid)), "both went");
        assert!(in_foreground());
        println!("DROPPED: ALL OF ITS GROUP GOES");
    }

    #[test]
    fn jobs_run_with_the_terminal() {
        let mut command = in_a_terminal(&[
            &std::env::current_exe().unwrap().to_string_lossy(),
            "--exact",
            "job::tests::in_a_terminal_of_its_own",
            "--ignored",
            "--nocapture",
        ]);
        let out = command.stdin(std::process::Stdio::null()).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        for line in [
            "AFTER ZSH: OURS",
            "WHILE IT RUNS: ITS OWN",
            "STOPPED: GOES ON",
            "STOPPED: ENDS",
            "DEAF TO SIGHUP: KILLED",
            "EDITOR: A JOB",
            "DROPPED: ALL OF ITS GROUP GOES",
        ] {
            assert!(text.contains(line), "{line}: {text}");
        }
    }

    /// `words` run in a terminal of their own, which `script` makes.
    #[cfg(target_os = "macos")]
    fn in_a_terminal(words: &[&str]) -> std::process::Command {
        let mut command = std::process::Command::new("/usr/bin/script");
        command.args(["-q", "/dev/null"]).args(words);
        command
    }

    /// `words` run in a terminal of their own, which util-linux's `script` makes from a command
    /// line for a shell.
    #[cfg(not(target_os = "macos"))]
    fn in_a_terminal(words: &[&str]) -> std::process::Command {
        let line: Vec<String> = words
            .iter()
            .map(|word| format!("'{}'", word.replace('\'', r"'\''")))
            .collect();
        let mut command = std::process::Command::new("script");
        command.args(["-qec", &line.join(" "), "/dev/null"]);
        command
    }
}
