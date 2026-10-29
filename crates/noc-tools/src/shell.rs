//! The shell of the command line: `shell.run`, by default `$SHELL -i -c`, with `/bin/sh` for a
//! program that comes out empty (ADR 0019, ADR 0054).

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
use noc_vars::Template;
use rustix::process::{
    Pid, Signal as ProcessSignal, WaitId, WaitIdOptions, getpgrp, kill_process_group, waitid,
};
use tokio::process::{Child, Command};
use tokio::signal::unix::{SignalKind, signal};

use crate::ToolError;

/// The program that runs commands typed on the command line, and the arguments before them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    program: PathBuf,
    args: Vec<OsString>,
}

impl Shell {
    /// `words` with the variables of this process's environment in them.
    pub fn from_env(words: &[Template]) -> Self {
        Self::expand(words, |name| std::env::var_os(name))
    }

    /// `words` with what `lookup` gives for each variable in them; the program, the first
    /// word, is `/bin/sh` if there is none or it comes out empty.
    pub fn expand(words: &[Template], mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut words = words.iter().map(|word| word.expand(&mut lookup));
        let program = words
            .next()
            .filter(|program| !program.is_empty())
            .map_or_else(|| PathBuf::from("/bin/sh"), PathBuf::from);
        Self {
            program,
            args: words.collect(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn args(&self) -> impl Iterator<Item = &OsStr> {
        self.args.iter().map(OsString::as_os_str)
    }

    /// Starts `command` as the last argument of the shell in `dir`, with this process's
    /// terminal, which the caller has handed over. The command goes to the shell as it was
    /// typed, in one argument; it may span lines.
    ///
    /// Where this process's group has the terminal, the shell runs as a job, as a shell runs
    /// one (ADR 0054): in a process group of its own, which gets the terminal, so that Ctrl+c
    /// and Ctrl+z reach it alone. [`Running::wait`] takes the terminal back when it ends or is
    /// stopped. Without the terminal, it runs in this process's group.
    pub fn start(&self, dir: &Path, command: &str) -> Result<Running, ToolError> {
        let spawn_error = |source| ToolError::Spawn {
            program: self.program.clone(),
            source,
        };
        // Before the child, so that no change of its state goes unheard.
        let child_signals = in_foreground()
            .then(|| signal(SignalKind::child()).ok())
            .flatten();
        let mut command_line = Command::new(&self.program);
        command_line
            .args(&self.args)
            .arg(command)
            .current_dir(dir)
            .kill_on_drop(true);
        if child_signals.is_some() {
            command_line.process_group(0);
        }
        let child = command_line.spawn().map_err(spawn_error)?;
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
                // It may have found itself in the background before this, as an interactive
                // shell checks first thing, and stopped itself: it goes on now.
                let _ = job.to_foreground();
                Some(job)
            }
            _ => None,
        };
        Ok(Running {
            child,
            program: self.program.clone(),
            job,
        })
    }
}

/// A shell that [`Shell::start`] started.
#[derive(Debug)]
pub struct Running {
    child: Child,
    program: PathBuf,
    /// Where it runs as a job: in a group of its own with the terminal.
    job: Option<Job>,
}

/// How a shell that [`Running::wait`] waited for is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    Ended(ExitStatus),
    /// Stopped, as by Ctrl+z: [`Running::resume`] or [`Running::end`] says what next.
    Stopped,
}

impl Running {
    /// Waits for the shell to end or to be stopped, and then takes the terminal back for this
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

    /// Gives the terminal back to a stopped shell, and continues it.
    pub fn resume(&mut self) -> io::Result<()> {
        match &self.job {
            Some(job) => job.to_foreground(),
            None => Ok(()),
        }
    }

    /// Ends a stopped shell as a shell ends the stopped jobs it leaves: SIGHUP, then SIGCONT, so
    /// that it hears it, with the terminal, so that it can put the terminal back as it was. A
    /// shell that goes on after that gets SIGKILL the next time.
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

    pub fn program(&self) -> &Path {
        &self.program
    }
}

/// A shell that runs in a process group of its own, whose id is its pid.
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
fn in_foreground() -> bool {
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
    use super::*;

    fn words(texts: &[&str]) -> Vec<Template> {
        texts
            .iter()
            .map(|text| Template::parse(text).unwrap())
            .collect()
    }

    fn shell(texts: &[&str], vars: &[(&str, &str)]) -> Shell {
        Shell::expand(&words(texts), |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        })
    }

    #[test]
    fn expands_the_words_and_falls_back_to_sh() {
        let zsh = shell(&["$SHELL", "-i", "-c"], &[("SHELL", "/bin/zsh")]);
        assert_eq!(zsh.program(), Path::new("/bin/zsh"));
        assert_eq!(zsh.args().collect::<Vec<_>>(), ["-i", "-c"]);
        let spaced = shell(
            &["$SHELL", "--rcfile", "${HOME}/a b", "-c"],
            &[("SHELL", "/bin/bash"), ("HOME", "/home/u")],
        );
        assert_eq!(
            spaced.args().collect::<Vec<_>>(),
            ["--rcfile", "/home/u/a b", "-c"]
        );
        for empty in [("SHELL", ""), ("OTHER", "/bin/zsh")] {
            let sh = shell(&["$SHELL", "-i", "-c"], &[empty]);
            assert_eq!(sh.program(), Path::new("/bin/sh"));
            assert_eq!(sh.args().collect::<Vec<_>>(), ["-i", "-c"]);
        }
        assert_eq!(shell(&[], &[]).program(), Path::new("/bin/sh"));
    }

    /// Starts `command` with `shell` in `dir` and waits for it to end.
    async fn run(shell: &Shell, dir: &Path, command: &str) -> Result<ExitStatus, ToolError> {
        match shell.start(dir, command)?.wait().await? {
            Waited::Ended(status) => Ok(status),
            Waited::Stopped => panic!("{command} was stopped"),
        }
    }

    #[tokio::test]
    async fn runs_the_command_in_the_directory_as_typed() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell(&["/bin/sh", "-c"], &[]);
        let command = "pwd -P > out \\\n  && echo 'two  words' >> out\nexit 3";
        let status = run(&shell, dir.path(), command).await.unwrap();
        assert_eq!(status.code(), Some(3));
        let out = std::fs::read_to_string(dir.path().join("out")).unwrap();
        let pwd = dir.path().canonicalize().unwrap();
        assert_eq!(out, format!("{}\ntwo  words\n", pwd.display()));
        let missing = Shell::expand(&words(&["/nonexistent/shell", "-c"]), |_| None);
        assert!(
            run(&missing, dir.path(), "true")
                .await
                .unwrap_err()
                .is_not_found()
        );
    }

    #[tokio::test]
    async fn the_command_comes_after_the_arguments() {
        let dir = tempfile::tempdir().unwrap();
        // `-c` takes the script, and `sh -c script name args` gives `$0` and `$1`.
        let shell = shell(&["/bin/sh", "-c", "echo \"$0 $1\" > out", "first"], &[]);
        let status = run(&shell, dir.path(), "second").await.unwrap();
        assert!(status.success());
        let out = std::fs::read_to_string(dir.path().join("out")).unwrap();
        assert_eq!(out, "first second\n");
    }

    /// What `script` runs for [`a_shell_runs_as_a_job_with_the_terminal`], in a terminal of its
    /// own, and prints a line for each part that held. Without a terminal it does nothing.
    #[tokio::test]
    #[ignore = "run in a terminal by a_shell_runs_as_a_job_with_the_terminal"]
    async fn in_a_terminal_of_its_own() {
        if !in_foreground() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");

        // zsh with job control runs `ls` in its own place, and nothing gives the terminal
        // back when it ends.
        let zsh = shell(&["/bin/zsh", "-f", "-i", "-c"], &[]);
        let status = run(&zsh, dir.path(), "/bin/ls > /dev/null").await.unwrap();
        assert!(status.success());
        assert!(in_foreground(), "the terminal is back");
        println!("AFTER ZSH: OURS");

        // The command has the terminal while it runs.
        let sh = shell(&["/bin/sh", "-c"], &[]);
        let ours = getpgrp().as_raw_nonzero().get();
        let check = format!("[ \"$(ps -o tpgid= -p $$)\" -ne {ours} ] && echo apart > out");
        run(&sh, dir.path(), &check).await.unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "apart\n");
        println!("WHILE IT RUNS: ITS OWN");

        // Stopped, as by Ctrl+z: the terminal is back, and it goes on, or ends.
        let mut running = sh
            .start(dir.path(), "kill -TSTP $$; echo went on > out")
            .unwrap();
        assert_eq!(running.wait().await.unwrap(), Waited::Stopped);
        assert!(in_foreground(), "the terminal is back while it is stopped");
        running.resume().unwrap();
        let Waited::Ended(status) = running.wait().await.unwrap() else {
            panic!("stopped again");
        };
        assert!(status.success());
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "went on\n");
        assert!(in_foreground());
        println!("STOPPED: GOES ON");

        let mut running = sh
            .start(dir.path(), "kill -TSTP $$; echo went on > out")
            .unwrap();
        std::fs::remove_file(&out).unwrap();
        assert_eq!(running.wait().await.unwrap(), Waited::Stopped);
        running.end().unwrap();
        let Waited::Ended(status) = running.wait().await.unwrap() else {
            panic!("stopped again");
        };
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(1)
        );
        assert!(!out.exists(), "it did not go on");
        assert!(in_foreground());
        println!("STOPPED: ENDS");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_shell_runs_as_a_job_with_the_terminal() {
        let out = std::process::Command::new("/usr/bin/script")
            .args(["-q", "/dev/null"])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "shell::tests::in_a_terminal_of_its_own"])
            .args(["--ignored", "--nocapture"])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        for line in [
            "AFTER ZSH: OURS",
            "WHILE IT RUNS: ITS OWN",
            "STOPPED: GOES ON",
            "STOPPED: ENDS",
        ] {
            assert!(text.contains(line), "{line}: {text}");
        }
    }
}
