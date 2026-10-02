//! Prompts on the controlling terminal for the command-line subcommands.
//!
//! macOS cannot poll `/dev/tty` with kqueue, so reads are non-blocking with a short sleep
//! in between. That keeps them cancellable: dropping a read restores the terminal echo.

use std::fs::{File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::time::Duration;

use rustix::termios::{LocalModes, OptionalActions, Termios, tcgetattr, tcsetattr};
use zeroize::Zeroizing;

const POLL: Duration = Duration::from_millis(30);

/// The controlling terminal, opened for prompts.
pub(crate) struct Tty {
    reader: File,
    writer: File,
}

impl Tty {
    /// Opens `/dev/tty`; fails when the process has no controlling terminal.
    pub(crate) fn open() -> io::Result<Self> {
        let reader = OpenOptions::new().read(true).open("/dev/tty")?;
        rustix::io::ioctl_fionbio(&reader, true)?;
        let writer = OpenOptions::new().write(true).open("/dev/tty")?;
        Ok(Self { reader, writer })
    }

    /// Writes `text` to the terminal.
    pub(crate) fn print(&self, text: &str) -> io::Result<()> {
        let mut writer = &self.writer;
        writer.write_all(text.as_bytes())?;
        writer.flush()
    }

    /// Shows `prompt` and reads one line, without echo when `secret`.
    pub(crate) async fn read_line(
        &self,
        prompt: &str,
        secret: bool,
    ) -> io::Result<Zeroizing<String>> {
        let echo_off = if secret {
            Some(EchoOff::new(&self.reader)?)
        } else {
            None
        };
        self.print(prompt)?;
        // Preallocated, so the secret is not left behind by a reallocation.
        let mut line = Zeroizing::new(Vec::with_capacity(1024));
        let result = self.read_until_newline(&mut line).await;
        if echo_off.is_some() {
            // The Enter key was not echoed either.
            let _ = self.print("\n");
        }
        drop(echo_off);
        result?;
        let text = String::from_utf8_lossy(&line);
        Ok(Zeroizing::new(
            text.trim_end_matches(['\r', '\n']).to_owned(),
        ))
    }

    async fn read_until_newline(&self, line: &mut Vec<u8>) -> io::Result<()> {
        let mut buffer = Zeroizing::new([0u8; 256]);
        let mut reader = &self.reader;
        loop {
            match reader.read(&mut buffer[..]) {
                // Ctrl-D at the start of a line.
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(read) => {
                    line.extend_from_slice(&buffer[..read]);
                    if line.contains(&b'\n') {
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(POLL).await;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// Turns terminal echo off until dropped.
struct EchoOff<'a> {
    file: &'a File,
    saved: Termios,
}

impl<'a> EchoOff<'a> {
    fn new(file: &'a File) -> io::Result<Self> {
        let saved = tcgetattr(file)?;
        let mut quiet = saved.clone();
        quiet.local_modes.remove(LocalModes::ECHO);
        tcsetattr(file, OptionalActions::Now, &quiet)?;
        Ok(Self { file, saved })
    }
}

impl Drop for EchoOff<'_> {
    fn drop(&mut self) {
        let _ = tcsetattr(self.file, OptionalActions::Now, &self.saved);
    }
}
