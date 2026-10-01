//! Answers askpass prompts on the terminal, for the command-line subcommands.

use secrecy::SecretString;
use sftp_tui_ssh::askpass::{AskpassEvent, Prompt, PromptKind};
use tokio::sync::mpsc;

use crate::tty::Tty;

/// Shows prompts from ssh on the controlling terminal until the event channel closes.
/// Without a terminal, every prompt is declined.
pub(crate) async fn answer_on_tty(mut events: mpsc::Receiver<AskpassEvent>) {
    // Opened on the first prompt: most connections never ask anything.
    let mut tty: Option<Option<Tty>> = None;
    while let Some(event) = events.recv().await {
        let tty = tty.get_or_insert_with(|| {
            Tty::open()
                .inspect_err(|error| tracing::warn!(%error, "no terminal for ssh prompts"))
                .ok()
        });
        match event {
            AskpassEvent::Prompt(prompt) => match tty {
                Some(tty) => ask(tty, prompt, &mut events).await,
                None => prompt.cancel(),
            },
            AskpassEvent::Notice {
                context, message, ..
            } => notice(tty.as_ref(), &context, &message),
            AskpassEvent::Closed { .. } => {}
        }
    }
}

async fn ask(tty: &Tty, prompt: Prompt, events: &mut mpsc::Receiver<AskpassEvent>) {
    let text = match prompt.kind {
        PromptKind::Confirm => format!(
            "[{}] {} (yes/no) ",
            prompt.context,
            prompt.message.trim_end()
        ),
        PromptKind::Secret | PromptKind::HostKey => {
            format!("[{}] {}", prompt.context, prompt.message)
        }
    };
    let read = tty.read_line(&text, prompt.kind == PromptKind::Secret);
    tokio::pin!(read);
    loop {
        tokio::select! {
            line = &mut read => {
                match (line, prompt.kind) {
                    (Ok(line), PromptKind::Confirm) => {
                        if line.trim().eq_ignore_ascii_case("yes") {
                            prompt.answer(SecretString::from("yes"));
                        } else {
                            prompt.cancel();
                        }
                    }
                    (Ok(line), PromptKind::Secret | PromptKind::HostKey) => {
                        prompt.answer(SecretString::from(line.as_str()));
                    }
                    (Err(_), _) => prompt.cancel(),
                }
                return;
            }
            event = events.recv() => match event {
                // ssh stopped waiting, for example after a timeout.
                Some(AskpassEvent::Closed { id }) if id == prompt.id => {
                    let _ = tty.print("\n");
                    return;
                }
                Some(AskpassEvent::Notice { context, message, .. }) => {
                    notice(Some(tty), &context, &message);
                }
                Some(AskpassEvent::Prompt(other)) => other.cancel(),
                Some(AskpassEvent::Closed { .. }) => {}
                None => return,
            },
        }
    }
}

fn notice(tty: Option<&Tty>, context: &str, message: &str) {
    let text = format!("[{context}] {}\n", message.trim_end());
    match tty {
        Some(tty) => {
            let _ = tty.print(&text);
        }
        None => eprint!("{text}"),
    }
}
