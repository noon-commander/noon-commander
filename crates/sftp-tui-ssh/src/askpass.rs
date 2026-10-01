//! Authentication prompts through the askpass bridge (ADR 0003).
//!
//! sftp-tui starts ssh with `SSH_ASKPASS` pointing to its own binary. When ssh needs input,
//! it runs that binary, which detects [`Invocation::from_env`], forwards the prompt over a
//! Unix socket to the [`AskpassServer`] of the running instance, and prints the answer.

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use zeroize::Zeroizing;

use crate::runtime::{self, ASKPASS_PREFIX, MAX_SOCKET_PATH_LEN};

const ENV_SOCKET: &str = "SFTP_TUI_ASKPASS_SOCKET";
const ENV_TOKEN: &str = "SFTP_TUI_ASKPASS_TOKEN";
const ENV_CONTEXT: &str = "SFTP_TUI_ASKPASS_CONTEXT";

/// Upper bound for one protocol message.
const MAX_MESSAGE: u64 = 64 * 1024;

/// What kind of answer a prompt expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Masked input: passwords, passphrases, PINs, one-time codes.
    Secret,
    /// A question about an unknown or changed host key, answered with `yes` or `no`.
    HostKey,
    /// A yes/no confirmation (`SSH_ASKPASS_PROMPT=confirm`): answer `yes`, or cancel to deny.
    Confirm,
}

impl PromptKind {
    fn classify(hint: Option<&str>, message: &str) -> Self {
        if hint == Some("confirm") {
            Self::Confirm
        } else if message.contains("(yes/no") {
            Self::HostKey
        } else {
            Self::Secret
        }
    }
}

/// A question from ssh that needs an answer.
#[derive(Debug)]
pub struct Prompt {
    /// Unique for the lifetime of the server; matches a later [`AskpassEvent::Closed`].
    pub id: u64,
    /// What the ssh process is connecting to, usually the host alias.
    pub context: String,
    /// The prompt text from ssh, e.g. `deploy@10.0.0.5's password: `.
    pub message: String,
    pub kind: PromptKind,
    reply: oneshot::Sender<Option<SecretString>>,
}

impl Prompt {
    /// Sends the answer to ssh.
    pub fn answer(self, text: SecretString) {
        let _ = self.reply.send(Some(text));
    }

    /// Declines the prompt. Dropping the prompt has the same effect.
    pub fn cancel(self) {
        let _ = self.reply.send(None);
    }
}

/// Something the UI has to show.
#[derive(Debug)]
pub enum AskpassEvent {
    /// A question; answer or cancel it.
    Prompt(Prompt),
    /// Information that needs no answer, such as a request to touch a security key. Show it
    /// until the matching [`AskpassEvent::Closed`].
    Notice {
        id: u64,
        context: String,
        message: String,
    },
    /// The prompt or notice `id` is gone: ssh finished or stopped waiting.
    Closed { id: u64 },
}

/// Environment that makes an ssh process send its prompts to an [`AskpassServer`].
#[derive(Clone)]
pub struct AskpassEnv {
    program: PathBuf,
    socket: PathBuf,
    token: String,
    context: String,
}

impl AskpassEnv {
    /// The variables to set on a process that should send its prompts to the server.
    pub fn vars(&self) -> [(&'static str, &OsStr); 5] {
        [
            ("SSH_ASKPASS", self.program.as_os_str()),
            ("SSH_ASKPASS_REQUIRE", OsStr::new("force")),
            (ENV_SOCKET, self.socket.as_os_str()),
            (ENV_TOKEN, OsStr::new(&self.token)),
            (ENV_CONTEXT, OsStr::new(&self.context)),
        ]
    }

    pub(crate) fn apply(&self, command: &mut Command) {
        command.envs(self.vars());
    }
}

impl fmt::Debug for AskpassEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AskpassEnv")
            .field("program", &self.program)
            .field("socket", &self.socket)
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

/// Receives prompts from ssh processes over a Unix socket in the runtime directory.
///
/// Dropping the server stops it and removes the socket.
#[derive(Debug)]
pub struct AskpassServer {
    socket_path: PathBuf,
    program: PathBuf,
    token: String,
    task: JoinHandle<()>,
}

impl AskpassServer {
    /// Binds the bridge socket in `runtime_dir`. `program` is the askpass executable: the
    /// sftp-tui binary. Must be called within a tokio runtime.
    pub fn bind(
        runtime_dir: &Path,
        program: PathBuf,
    ) -> io::Result<(Self, mpsc::Receiver<AskpassEvent>)> {
        let socket_path = runtime_dir.join(runtime::socket_name(ASKPASS_PREFIX)?);
        if socket_path.as_os_str().len() > MAX_SOCKET_PATH_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "askpass socket path {} is longer than {MAX_SOCKET_PATH_LEN} bytes",
                    socket_path.display()
                ),
            ));
        }
        let listener = UnixListener::bind(&socket_path)?;
        let token = runtime::random_hex(16)?;
        let (events, receiver) = mpsc::channel(16);
        let task = tokio::spawn(serve(listener, Arc::from(token.as_str()), events));
        let server = Self {
            socket_path,
            program,
            token,
            task,
        };
        Ok((server, receiver))
    }

    /// Environment for an ssh process; `context` is shown with its prompts.
    pub fn env(&self, context: &str) -> AskpassEnv {
        AskpassEnv {
            program: self.program.clone(),
            socket: self.socket_path.clone(),
            token: self.token.clone(),
            context: context.to_owned(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for AskpassServer {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[derive(Serialize)]
struct RequestRef<'a> {
    token: &'a str,
    context: &'a str,
    prompt: &'a str,
    hint: Option<&'a str>,
}

#[derive(Deserialize)]
struct Request {
    token: String,
    context: String,
    prompt: String,
    hint: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponseRef<'a> {
    Answer { text: &'a str },
    Cancel,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Response {
    Answer { text: String },
    Cancel,
}

async fn serve(listener: UnixListener, token: Arc<str>, events: mpsc::Sender<AskpassEvent>) {
    let ids = Arc::new(AtomicU64::new(1));
    let uid = rustix::process::getuid().as_raw();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                if stream.peer_cred().map(|cred| cred.uid()).ok() != Some(uid) {
                    tracing::warn!("askpass connection from another user");
                    continue;
                }
                let (token, events, ids) = (Arc::clone(&token), events.clone(), Arc::clone(&ids));
                tokio::spawn(async move {
                    if let Err(error) = handle(stream, &token, &events, &ids).await {
                        tracing::debug!(%error, "askpass request failed");
                    }
                });
            }
            Err(error) => {
                tracing::warn!(%error, "askpass accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle(
    stream: UnixStream,
    token: &str,
    events: &mpsc::Sender<AskpassEvent>,
    ids: &AtomicU64,
) -> io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = Vec::new();
    (&mut reader)
        .take(MAX_MESSAGE)
        .read_until(b'\n', &mut line)
        .await?;
    let request: Request = serde_json::from_slice(&line)?;
    if !constant_time_eq(request.token.as_bytes(), token.as_bytes()) {
        tracing::warn!("askpass request with a wrong token");
        return Ok(());
    }
    let id = ids.fetch_add(1, Ordering::Relaxed);
    let Request {
        context,
        prompt: message,
        hint,
        ..
    } = request;
    tracing::debug!(id, %context, hint = hint.as_deref(), %message, "askpass prompt");
    if hint.as_deref() == Some("none") {
        // ssh kills the notifier when it is done, which closes the connection.
        let notice = AskpassEvent::Notice {
            id,
            context,
            message,
        };
        if events.send(notice).await.is_ok() {
            wait_for_eof(&mut reader).await;
            let _ = events.send(AskpassEvent::Closed { id }).await;
        }
        return Ok(());
    }
    let (reply, answer) = oneshot::channel();
    let prompt = Prompt {
        id,
        context,
        kind: PromptKind::classify(hint.as_deref(), &message),
        message,
        reply,
    };
    if events.send(AskpassEvent::Prompt(prompt)).await.is_err() {
        return respond(&mut write_half, None).await;
    }
    tokio::select! {
        answer = answer => respond(&mut write_half, answer.ok().flatten()).await,
        () = wait_for_eof(&mut reader) => {
            let _ = events.send(AskpassEvent::Closed { id }).await;
            Ok(())
        }
    }
}

async fn respond(writer: &mut OwnedWriteHalf, answer: Option<SecretString>) -> io::Result<()> {
    let response = match &answer {
        Some(text) => ResponseRef::Answer {
            text: text.expose_secret(),
        },
        None => ResponseRef::Cancel,
    };
    let capacity = answer.as_ref().map_or(0, |text| text.expose_secret().len()) * 6 + 64;
    // Sized up front so the buffer holding the secret is never reallocated unwiped.
    let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
    serde_json::to_writer(&mut *bytes, &response)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.shutdown().await
}

async fn wait_for_eof(reader: &mut (impl AsyncRead + Unpin)) {
    let mut buffer = [0; 64];
    while let Ok(read) = reader.read(&mut buffer).await {
        if read == 0 {
            break;
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A prompt from ssh: this process was started as `SSH_ASKPASS` by an ssh child of sftp-tui.
pub struct Invocation {
    socket: PathBuf,
    token: String,
    context: String,
    prompt: String,
    hint: Option<String>,
}

impl fmt::Debug for Invocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Invocation")
            .field("socket", &self.socket)
            .field("context", &self.context)
            .field("prompt", &self.prompt)
            .field("hint", &self.hint)
            .finish_non_exhaustive()
    }
}

/// The user's reaction to a prompt.
pub enum Outcome {
    Answer(Zeroizing<String>),
    Cancelled,
}

impl fmt::Debug for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Answer(_) => f.write_str("Answer(..)"),
            Self::Cancelled => f.write_str("Cancelled"),
        }
    }
}

impl Invocation {
    /// `Some` when ssh started this process as its askpass program.
    pub fn from_env() -> Option<Self> {
        let socket = std::env::var_os(ENV_SOCKET)?;
        let token = std::env::var(ENV_TOKEN).ok()?;
        let prompt = std::env::args_os()
            .nth(1)
            .map(|prompt| prompt.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some(Self {
            socket: PathBuf::from(socket),
            token,
            context: std::env::var(ENV_CONTEXT).unwrap_or_default(),
            prompt,
            hint: std::env::var("SSH_ASKPASS_PROMPT").ok(),
        })
    }

    /// Whether ssh only shows a notice and ignores the answer.
    pub fn is_notice(&self) -> bool {
        self.hint.as_deref() == Some("none")
    }

    /// Sends the prompt to sftp-tui and waits for the answer. Blocking.
    pub fn run(&self) -> io::Result<Outcome> {
        let mut stream = std::os::unix::net::UnixStream::connect(&self.socket)?;
        let request = RequestRef {
            token: &self.token,
            context: &self.context,
            prompt: &self.prompt,
            hint: self.hint.as_deref(),
        };
        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        stream.write_all(&bytes)?;
        let mut line = Zeroizing::new(Vec::new());
        io::BufReader::new(stream)
            .take(MAX_MESSAGE)
            .read_until(b'\n', &mut line)?;
        if line.is_empty() {
            return Ok(Outcome::Cancelled);
        }
        match serde_json::from_slice(&line)? {
            Response::Answer { text } => Ok(Outcome::Answer(Zeroizing::new(text))),
            Response::Cancel => Ok(Outcome::Cancelled),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation(env: &AskpassEnv, prompt: &str, hint: Option<&str>) -> Invocation {
        Invocation {
            socket: env.socket.clone(),
            token: env.token.clone(),
            context: env.context.clone(),
            prompt: prompt.to_owned(),
            hint: hint.map(str::to_owned),
        }
    }

    fn server() -> (
        tempfile::TempDir,
        AskpassServer,
        mpsc::Receiver<AskpassEvent>,
    ) {
        let dir = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let (server, events) =
            AskpassServer::bind(dir.path(), PathBuf::from("/bin/false")).unwrap();
        (dir, server, events)
    }

    async fn next_prompt(events: &mut mpsc::Receiver<AskpassEvent>) -> Prompt {
        match events.recv().await {
            Some(AskpassEvent::Prompt(prompt)) => prompt,
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn answers_reach_the_client() {
        let (_dir, server, mut events) = server();
        let client = invocation(&server.env("web"), "web's password: ", None);
        let outcome = tokio::task::spawn_blocking(move || client.run());
        let prompt = next_prompt(&mut events).await;
        assert_eq!(prompt.context, "web");
        assert_eq!(prompt.message, "web's password: ");
        assert_eq!(prompt.kind, PromptKind::Secret);
        prompt.answer(SecretString::from("s3cr\"et\n"));
        match outcome.await.unwrap().unwrap() {
            Outcome::Answer(text) => assert_eq!(text.as_str(), "s3cr\"et\n"),
            Outcome::Cancelled => panic!("cancelled"),
        }
    }

    #[tokio::test]
    async fn cancel_and_drop_decline() {
        let (_dir, server, mut events) = server();
        for cancel in [true, false] {
            let client = invocation(&server.env("web"), "Password: ", None);
            let outcome = tokio::task::spawn_blocking(move || client.run());
            let prompt = next_prompt(&mut events).await;
            if cancel {
                prompt.cancel();
            } else {
                drop(prompt);
            }
            assert!(matches!(
                outcome.await.unwrap().unwrap(),
                Outcome::Cancelled
            ));
        }
    }

    #[tokio::test]
    async fn classifies_prompts() {
        let (_dir, server, mut events) = server();
        let cases = [
            (
                "Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
                None,
                PromptKind::HostKey,
            ),
            ("Allow use of key? ", Some("confirm"), PromptKind::Confirm),
            ("Verification code: ", None, PromptKind::Secret),
        ];
        for (message, hint, kind) in cases {
            let client = invocation(&server.env("web"), message, hint);
            let outcome = tokio::task::spawn_blocking(move || client.run());
            let prompt = next_prompt(&mut events).await;
            assert_eq!(prompt.kind, kind, "{message}");
            prompt.cancel();
            outcome.await.unwrap().unwrap();
        }
    }

    #[tokio::test]
    async fn notices_close_when_the_client_disconnects() {
        let (_dir, server, mut events) = server();
        let env = server.env("web");
        let mut stream = UnixStream::connect(&env.socket).await.unwrap();
        let request = RequestRef {
            token: &env.token,
            context: "web",
            prompt: "Confirm user presence for key ED25519-SK",
            hint: Some("none"),
        };
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        stream.write_all(&bytes).await.unwrap();
        let id = match events.recv().await {
            Some(AskpassEvent::Notice { id, message, .. }) => {
                assert!(message.starts_with("Confirm user presence"));
                id
            }
            other => panic!("expected a notice, got {other:?}"),
        };
        drop(stream);
        assert!(matches!(
            events.recv().await,
            Some(AskpassEvent::Closed { id: closed }) if closed == id
        ));
    }

    #[tokio::test]
    async fn rejects_a_wrong_token() {
        let (_dir, server, mut events) = server();
        let mut client = invocation(&server.env("web"), "Password: ", None);
        client.token = "0".repeat(32);
        let outcome = tokio::task::spawn_blocking(move || client.run());
        assert!(matches!(
            outcome.await.unwrap().unwrap(),
            Outcome::Cancelled
        ));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn debug_output_hides_the_token() {
        let env = AskpassEnv {
            program: PathBuf::from("/bin/sftp-tui"),
            socket: PathBuf::from("/run/ap"),
            token: "deadbeef".to_owned(),
            context: "web".to_owned(),
        };
        assert!(!format!("{env:?}").contains("deadbeef"));
    }
}
